use std::collections::VecDeque;
use std::path::Path;
use log::{info, trace};

#[path = "mpeg/mod.rs"]
pub mod mpeg;

use self::mpeg::Video;

/// DCLK runs at 45 kHz (15.5 MHz / ~344.44)
pub const DCLK_FREQ: u64 = 45_000;
pub const MASTER_CLOCK: u64 = 15_500_000;

// FMV Register Byte Offsets (within 0xE04000..0xE040FF)
pub const FMV_IER: usize = 0x60;
pub const FMV_ISR: usize = 0x62;
pub const FMV_TIMER: usize = 0x64;
pub const FMV_PICTURE_PERIOD: usize = 0xA8;
pub const FMV_DISPLAY_PERIOD: usize = 0xAA;
pub const FMV_SYSCMD: usize = 0xC0;
pub const FMV_VIDCMD: usize = 0xC2;
pub const FMV_STREAM: usize = 0xC4;
pub const FMV_IVEC: usize = 0xDC;
pub const FMV_XFER: usize = 0xDE;
pub const FMV_BORDER_R: usize = 0x66;
pub const FMV_SCRPOS_Y: usize = 0x74;

// Largest MPEG-1 picture the DVC framebuffer holds (constrained parameters).
pub const FB_MAX_W: usize = 768;
pub const FB_MAX_H: usize = 576;

// FMA Register Byte Offsets (within 0xE03000..0xE030FF)
pub const FMA_CMD: usize = 0x00;
pub const FMA_MODE: usize = 0x04;
pub const FMA_STREAM: usize = 0x08;
pub const FMA_IVEC: usize = 0x0C;
pub const FMA_ISR: usize = 0x1A;
pub const FMA_IER: usize = 0x1C;

// FMV Interrupt Status / Enable bits
pub const FMV_IRQ_SEQ: u16   = 1 << 0;  // 0x0001: Sequence header / first picture
pub const FMV_IRQ_GOP: u16   = 1 << 1;  // 0x0002: Group of Pictures header
pub const FMV_IRQ_PIC: u16   = 1 << 2;  // 0x0004: Picture displayed
pub const FMV_IRQ_EOD: u16   = 1 << 3;  // 0x0008: End of data / last picture
pub const FMV_IRQ_RFB: u16   = 1 << 4;  // 0x0010: Request for bits
pub const FMV_IRQ_NDAT: u16  = 1 << 5;  // 0x0020: No data / underflow
pub const FMV_IRQ_OVF: u16   = 1 << 6;  // 0x0040: Overflow
pub const FMV_IRQ_DCL: u16   = 1 << 7;  // 0x0080: Display control latched
pub const FMV_IRQ_TIM: u16   = 1 << 8;  // 0x0100: Timer tick
pub const FMV_IRQ_ESI: u16   = 1 << 9;  // 0x0200: End of sequence in stream
pub const FMV_IRQ_EII: u16   = 1 << 10; // 0x0400: End of ISO program in stream
pub const FMV_IRQ_VSYNC: u16 = 1 << 11; // 0x0800: Vertical sync
pub const FMV_IRQ_PAI: u16   = 1 << 12; // 0x1000: Pause acknowledged
pub const FMV_IRQ_VCUP: u16  = 1 << 13; // 0x2000: Video control update
pub const FMV_IRQ_ERDD: u16  = 1 << 14; // 0x4000: Error DRAM decoder
pub const FMV_IRQ_ERDV: u16  = 1 << 15; // 0x8000: Error DRAM video

// FMA Interrupt Status / Enable bits
pub const FMA_IRQ_EOI: u16   = 1 << 0;  // 0x0001: End of ISO program
pub const FMA_IRQ_UNF: u16   = 1 << 3;  // 0x0008: Underflow
pub const FMA_IRQ_DEC: u16   = 1 << 2;  // 0x0004: Decoded data ready
pub const FMA_IRQ_PLAY: u16  = 1 << 4;  // 0x0010: Playback active
pub const FMA_IRQ_POLL: u16  = 1 << 8;  // 0x0100: Timer tick
pub const FMA_MODE_DRAIN: u16 = 0x1F;  // madriv mode: 7 = play, 0x1F = drain at end of data or before a loop restart

// =========================================================================
// MPEG Audio Layer-II Decoder (oxideav-mp2)
// =========================================================================

pub struct Mp2Decoder {
    state: oxideav_mp2::frame::FrameDecodeState,
    pub pcm: VecDeque<(f32, f32)>,
    pub last_sample_rate: u32,
    pub errors: u64,
    pub resync_bytes: u64,
    pub synchronized: bool,
    pub conceal_discontinuity_frame: bool,
    pub concealed_frames: u64,
}

impl Default for Mp2Decoder {
    fn default() -> Self {
        Self {
            state: oxideav_mp2::frame::FrameDecodeState::default(),
            pcm: VecDeque::new(),
            last_sample_rate: 44_100,
            errors: 0,
            resync_bytes: 0,
            synchronized: false,
            conceal_discontinuity_frame: false,
            concealed_frames: 0,
        }
    }
}

impl Mp2Decoder {
    pub fn reset(&mut self) {
        self.state.reset();
        self.pcm.clear();
        self.errors = 0;
        self.resync_bytes = 0;
        self.synchronized = false;
        self.conceal_discontinuity_frame = false;
        self.concealed_frames = 0;
    }

    pub fn decode_available(&mut self, input: &mut VecDeque<u8>) -> u64 {
        let mut decoded = 0;
        let mut scanned = 0u64;
        loop {
            if input.len() < 4 {
                break;
            }
            let head = [input[0], input[1], input[2], input[3]];
            let header = match oxideav_mp2::header::FrameHeader::parse(&head) {
                Ok(header) => header,
                Err(_) => {
                    let lost_sync = self.synchronized;
                    input.pop_front();
                    if lost_sync {
                        self.errors += 1;
                        self.synchronized = false;
                    } else {
                        self.resync_bytes += 1;
                        scanned += 1;
                    }
                    continue;
                }
            };
            let frame_size = header.frame_size_bytes();
            if input.len() < frame_size {
                break;
            }
            let contiguous = input.make_contiguous();
            match oxideav_mp2::frame::decode_frame_with(&contiguous[..frame_size], &mut self.state) {
                Ok(frame) => {
                    if frame.header.sample_rate > 0 {
                        self.last_sample_rate = frame.header.sample_rate;
                    }
                    if self.conceal_discontinuity_frame {
                        if scanned != 0 {
                            let samples = frame.pcm.first().map_or(0, Vec::len);
                            self.pcm.extend(std::iter::repeat((0.0f32, 0.0f32)).take(samples));
                            self.concealed_frames += 1;
                        }
                        self.conceal_discontinuity_frame = false;
                    }
                    Self::append_native(&mut self.pcm, &frame.pcm);
                    input.drain(..frame_size);
                    decoded += 1;
                    self.synchronized = true;
                }
                Err(_) => {
                    input.pop_front();
                    self.state.reset();
                    self.errors += 1;
                    self.synchronized = false;
                }
            }
        }
        decoded
    }

    // A trailing partial frame cannot be decoded, so it does not keep the decoder fed.
    pub fn has_frame(input: &VecDeque<u8>) -> bool {
        if input.len() < 4 {
            return false;
        }
        let head = [input[0], input[1], input[2], input[3]];
        oxideav_mp2::header::FrameHeader::parse(&head).map_or(false, |h| input.len() >= h.frame_size_bytes())
    }

    fn append_native(output: &mut VecDeque<(f32, f32)>, channels: &[Vec<f64>]) {
        let Some(left) = channels.first() else { return; };
        let right = channels.get(1).unwrap_or(left);
        for (&l, &r) in left.iter().zip(right.iter()) {
            output.push_back((
                (l as f32).clamp(-1.0, 1.0),
                (r as f32).clamp(-1.0, 1.0),
            ));
        }
    }
}

// =========================================================================
// MPEG-1 System Stream Demuxer
// =========================================================================

#[derive(Default)]
pub struct MpegSystemDemux {
    pub pending: Vec<u8>,
    pub video: Vec<u8>,
    pub audio: VecDeque<u8>,
    pub last_scr: Option<u64>,
    pub first_video_pts: Option<u64>,
    pub last_video_pts: Option<u64>,
    pub last_video_dts: Option<u64>,
    pub first_audio_pts: Option<u64>,
    pub last_audio_pts: Option<u64>,
    pub selected_audio_stream: Option<u8>,
    pub selected_video_stream: Option<u8>,
    pub program_ends: u64,
}

impl MpegSystemDemux {
    pub fn clear(&mut self) {
        self.pending.clear();
        self.video.clear();
        self.audio.clear();
        self.last_scr = None;
        self.first_video_pts = None;
        self.last_video_pts = None;
        self.last_video_dts = None;
        self.first_audio_pts = None;
        self.last_audio_pts = None;
        self.program_ends = 0;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        self.parse_available();
    }

    fn find_prefix(bytes: &[u8]) -> Option<usize> {
        bytes.windows(3).position(|w| w == [0, 0, 1])
    }

    fn parse_available(&mut self) {
        loop {
            let Some(prefix) = Self::find_prefix(&self.pending) else {
                if self.pending.len() > 2 {
                    let keep = self.pending.split_off(self.pending.len() - 2);
                    self.pending = keep;
                }
                return;
            };
            if prefix != 0 {
                self.pending.drain(..prefix);
            }
            if self.pending.len() < 4 {
                return;
            }

            let stream_id = self.pending[3];
            match stream_id {
                0xBA => { // Pack Header
                    if self.pending.len() < 12 {
                        return;
                    }
                    if self.pending[4] & 0xF0 == 0x20 {
                        self.last_scr = Some(Self::decode_mpeg1_scr(&self.pending[4..9]));
                        self.pending.drain(..12);
                    } else {
                        if self.pending.len() < 14 {
                            return;
                        }
                        let size = 14 + (self.pending[13] & 7) as usize;
                        if self.pending.len() < size {
                            return;
                        }
                        self.pending.drain(..size);
                    }
                }
                0xB9 => { // ISO Program End
                    self.pending.drain(..4);
                    self.program_ends += 1;
                }
                _ => {
                    if self.pending.len() < 6 {
                        return;
                    }
                    let payload_len = usize::from(u16::from_be_bytes([self.pending[4], self.pending[5]]));
                    if payload_len == 0 {
                        let Some(next) = Self::find_prefix(&self.pending[4..]).map(|v| v + 4) else {
                            return;
                        };
                        self.pending.drain(..next);
                        continue;
                    }
                    let packet_len = 6 + payload_len;
                    if self.pending.len() < packet_len {
                        return;
                    }
                    let packet: Vec<u8> = self.pending.drain(..packet_len).collect();
                    self.consume_packet(stream_id, &packet);
                }
            }
        }
    }

    fn consume_packet(&mut self, stream_id: u8, packet: &[u8]) {
        if !(0xC0..=0xEF).contains(&stream_id) {
            return;
        }
        let Some((payload, pts, dts)) = Self::pes_payload(packet) else {
            return;
        };
        if (0xE0..=0xEF).contains(&stream_id)
            && self.selected_video_stream.map_or(true, |selected| stream_id & 0x0F == selected)
        {
            self.video.extend_from_slice(payload);
            if let Some(pts) = pts {
                self.first_video_pts.get_or_insert(pts);
                self.last_video_pts = Some(pts);
            }
            if let Some(dts) = dts.or(pts) {
                self.last_video_dts = Some(dts);
            }
        } else if (0xC0..=0xDF).contains(&stream_id)
            && self.selected_audio_stream.map_or(true, |selected| stream_id & 0x1F == selected)
        {
            self.audio.extend(payload.iter().copied());
            if let Some(pts) = pts {
                self.first_audio_pts.get_or_insert(pts);
                self.last_audio_pts = Some(pts);
            }
        }
    }

    fn decode_mpeg1_scr(bytes: &[u8]) -> u64 {
        (u64::from(bytes[0] & 0x0E) << 29)
            | (u64::from(bytes[1]) << 22)
            | (u64::from(bytes[2] & 0xFE) << 14)
            | (u64::from(bytes[3]) << 7)
            | u64::from(bytes[4] >> 1)
    }

    fn pes_payload(packet: &[u8]) -> Option<(&[u8], Option<u64>, Option<u64>)> {
        let mut at = 6usize;
        while packet.get(at) == Some(&0xFF) {
            at += 1;
        }
        if packet.get(at).is_some_and(|b| b & 0xC0 == 0x40) {
            at += 2;
        }

        let marker = *packet.get(at)?;
        let mut pts = None;
        let mut dts = None;
        if marker & 0xC0 == 0x80 {
            // MPEG-2 PES header
            let flags = *packet.get(at + 1)?;
            let header_len = usize::from(*packet.get(at + 2)?);
            if flags & 0x80 != 0 && header_len >= 5 {
                pts = Self::decode_pts(packet.get(at + 3..at + 8)?);
            }
            if flags & 0x40 != 0 && header_len >= 10 {
                dts = Self::decode_pts(packet.get(at + 8..at + 13)?);
            }
            at += 3 + header_len;
        } else {
            match marker >> 4 {
                0x2 => {
                    pts = Self::decode_pts(packet.get(at..at + 5)?);
                    at += 5;
                }
                0x3 => {
                    pts = Self::decode_pts(packet.get(at..at + 5)?);
                    dts = Self::decode_pts(packet.get(at + 5..at + 10)?);
                    at += 10;
                }
                _ if marker == 0x0F => at += 1,
                _ => {} // Direct payload without PTS or stuffing
            }
        }
        packet.get(at..).map(|payload| (payload, pts, dts))
    }

    fn decode_pts(bytes: &[u8]) -> Option<u64> {
        if bytes.len() < 5 || bytes[0] & 1 == 0 || bytes[2] & 1 == 0 || bytes[4] & 1 == 0 {
            return None;
        }
        Some(
            (u64::from((bytes[0] >> 1) & 7) << 30)
                | (u64::from(bytes[1]) << 22)
                | (u64::from(bytes[2] >> 1) << 15)
                | (u64::from(bytes[3]) << 7)
                | u64::from(bytes[4] >> 1),
        )
    }
}

// =========================================================================
// Digital Video Cartridge (DVC) Core
// =========================================================================

pub struct Dvc {
    pub rom: Option<Box<[u8; 256 * 1024]>>,
    pub ram1: Box<[u8; 1024 * 1024]>,   // 1 MB DVC RAM block 1 (0xD00000..0xDFFFFF)
    pub ram2: Box<[u8; 512 * 1024]>,    // 512 KB Picture DRAM (0xE80000..0xEFFFFF)
    pub ram2_enabled: bool,
    pub reg_write_count: u32,
    pub reg_read_count: u32,
    pub rom_read_count: u32,

    // Register files (256 bytes each)
    pub fma_regs: Box<[u8; 256]>,
    pub fmv_regs: Box<[u8; 256]>,

    // Mirrored fields for easy access
    pub fma_cmd: u16,
    pub fma_isr: u16,
    pub fma_ier: u16,
    pub fma_ivec: u16,
    pub fmv_isr: u16,
    pub fmv_ier: u16,
    pub fmv_ivec: u16,
    pub fmv_playback_active: bool,
    pub fmv_decoder_active: bool,

    // Clock tracking
    pub total_cycles: u64,
    pub dclk: u32,
    pub dclk_accum: u64,
    pub timer_accum: u64,
    pub timer_counter: u16,

    // Controls
    pub video_playing: bool,
    pub video_armed: bool,
    pub decoder_enabled: bool,
    pub audio_armed: bool,
    pub audio_enabled: bool,
    pub vcd_pixel_clock_13_5: bool,

    // Timecode tracking
    pub display_timecode: u32,
    pub decoder_timecode: u32,
    pub display_frame_counter: u32,
    pub decoder_frame_counter: u32,
    pub fps: f64,
    pub frame_accum: f64,
    pub vsync_hz: f64,

    // Stream & Demux
    pub dvc_stream_fifo: Vec<u8>,
    pub fma_stream_fifo: Vec<u8>,
    pub transfer_for_fma: bool,
    pub fma_dma_seen: bool,
    pub dma_requested: bool,
    pub framebuffer: Box<[u32; FB_MAX_W * FB_MAX_H]>,
    /// Display window latched by VIDCMD WIN: screen y, screen x, height, width, picture y, picture x (pixels, x in normal resolution).
    pub window: [u16; 6],
    pub border_color: u32,

    pub demux: MpegSystemDemux,
    pub fma_demux: MpegSystemDemux,
    pub mp2: Mp2Decoder,
    pub audio_sample_rate: u32,
    pub audio_out: VecDeque<(f32, f32)>,

    pub video_decoder: Option<Video>,
    pub video_width: usize,
    pub video_height: usize,
    pub frames_decoded_count: usize,
    pub frames_presented_count: usize,
    pub vsync_counter: usize,
    pub underflow_count: u32,
    pub fma_underflow_count: u32,
    /// Audio samples decoded since the last FMA play or stop.
    pub fma_decoded_since_play: u64,
    // Cycle (total_cycles) at which the decoded MPEG audio would have finished playing; 0 when none is pending.
    pub fma_playout_end: u64,
    pub has_valid_frame: bool,
}

impl Dvc {
    pub fn new() -> Self {
        let mut d = Self {
            rom: None,
            ram1: vec![0u8; 1024 * 1024].into_boxed_slice().try_into().unwrap(),
            ram2: vec![0u8; 512 * 1024].into_boxed_slice().try_into().unwrap(),
            ram2_enabled: false,
            reg_write_count: 0,
            reg_read_count: 0,
            rom_read_count: 0,

            fma_regs: vec![0u8; 256].into_boxed_slice().try_into().unwrap(),
            fmv_regs: vec![0u8; 256].into_boxed_slice().try_into().unwrap(),

            fma_cmd: 0,
            fma_isr: 0,
            fma_ier: 0,
            fma_ivec: 0,
            fmv_isr: 0,
            fmv_ier: 0,
            fmv_ivec: 0,
            fmv_playback_active: false,
            fmv_decoder_active: false,

            total_cycles: 0,
            dclk: 0,
            dclk_accum: 0,
            timer_accum: 0,
            timer_counter: 0,

            video_playing: false,
            video_armed: false,
            decoder_enabled: false,
            audio_armed: false,
            audio_enabled: false,
            vcd_pixel_clock_13_5: false,

            display_timecode: 0,
            decoder_timecode: 0,
            display_frame_counter: 0,
            decoder_frame_counter: 0,
            fps: 25.0,
            frame_accum: 0.0,
            vsync_hz: 50.0,

            dvc_stream_fifo: Vec::with_capacity(128 * 1024),
            fma_stream_fifo: Vec::with_capacity(64 * 1024),
            transfer_for_fma: false,
            fma_dma_seen: false,
            dma_requested: false,
            framebuffer: vec![0xFF00_0000; FB_MAX_W * FB_MAX_H].into_boxed_slice().try_into().unwrap(),
            window: [0; 6],
            border_color: 0xFF00_0000,

            demux: MpegSystemDemux::default(),
            fma_demux: MpegSystemDemux::default(),
            mp2: Mp2Decoder::default(),
            audio_sample_rate: 44_100,
            audio_out: VecDeque::with_capacity(16 * 1024),

            video_decoder: None,
            video_width: 352,
            video_height: 240,
            frames_decoded_count: 0,
            frames_presented_count: 0,
            vsync_counter: 0,
            underflow_count: 0,
            fma_underflow_count: 0,
            fma_decoded_since_play: 0,
            fma_playout_end: 0,
            has_valid_frame: false,
        };
        d.reset_regs();
        d
    }

    pub fn reset(&mut self) {
        self.ram1.fill(0);
        self.ram2.fill(0);
        self.framebuffer.fill(0xFF00_0000);
        self.window = [0; 6];
        self.border_color = 0xFF00_0000;
        self.has_valid_frame = false;
        self.ram2_enabled = false;
        self.reg_write_count = 0;
        self.reg_read_count = 0;
        self.rom_read_count = 0;

        self.reset_stream();
        self.reset_regs();
    }

    pub fn reset_stream(&mut self) {
        self.video_playing = false;
        self.video_armed = false;
        self.decoder_enabled = false;
        self.audio_armed = false;
        self.audio_enabled = false;
        self.vcd_pixel_clock_13_5 = false;
        self.fmv_playback_active = false;
        self.fmv_decoder_active = false;
        self.dma_requested = false;
        self.total_cycles = 0;
        self.fma_playout_end = 0;
        self.dclk = 0;
        self.dclk_accum = 0;
        self.timer_accum = 0;
        self.timer_counter = 0;
        self.dvc_stream_fifo.clear();
        self.fma_stream_fifo.clear();
        self.transfer_for_fma = false;
        self.fma_dma_seen = false;
        self.demux.clear();
        self.fma_demux.clear();
        self.mp2.reset();
        self.audio_out.clear();
        self.video_decoder = None;
        self.frames_decoded_count = 0;
        self.frames_presented_count = 0;
        self.vsync_counter = 0;
        self.underflow_count = 0;
        self.fma_underflow_count = 0;
        self.fma_decoded_since_play = 0;
        self.frame_accum = 0.0;
        self.display_timecode = 0;
        self.decoder_timecode = 0;
        self.display_frame_counter = 0;
        self.decoder_frame_counter = 0;
        self.has_valid_frame = false;
        self.framebuffer.fill(0xFF00_0000);
    }

    pub fn reset_regs(&mut self) {
        self.fma_regs.fill(0);
        self.fmv_regs.fill(0);

        // FMA defaults
        self.set_fma_word(0x02, 0x0200); // Ready / Idle status
        self.set_fma_word(0x04, 0x0007);
        self.set_fma_word(0x06, 0x0900);
        self.set_fma_word(0x0E, 0x0042);
        self.set_fma_word(0x24, 0x0004); // HF2 DSP flag ready

        // FMV defaults
        self.set_fmv_word(FMV_TIMER, 55);
        self.set_fmv_word(0x5E, 0x2000); // FIFO Ready
        self.set_fmv_word(0x9E, 0xFE96); // Dec delay
        self.set_fmv_word(FMV_PICTURE_PERIOD, 1800); // 25 fps PAL
        self.set_fmv_word(FMV_DISPLAY_PERIOD, 1800); // 25 fps PAL (0x0708)
        self.set_fmv_word(0xAC, 1800);               // Frame rate ratio (word 0x56)

        // Offsets & sizes
        self.set_fmv_word(0x6C, 26);  // Y offset
        self.set_fmv_word(0x6E, 74);  // X offset
        self.set_fmv_word(0x02, 352); // Width
        self.set_fmv_word(0x04, 240); // Height
        self.set_fmv_word(0x52, 352); // Width
        self.set_fmv_word(0x54, 240); // Height
        self.set_fmv_word(0x06, 0x23); // PRPA (4:3 PAL)
        self.set_fmv_word(0x56, 0x0708);

        self.sync_mirrors();
    }

    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let data = std::fs::read(path)?;
        let mut rom_buf: Box<[u8; 256 * 1024]> = vec![0xFFu8; 256 * 1024].into_boxed_slice().try_into().unwrap();
        let copy_len = data.len().min(256 * 1024);
        rom_buf[..copy_len].copy_from_slice(&data[..copy_len]);
        if copy_len == 128 * 1024 {
            rom_buf[128 * 1024..256 * 1024].copy_from_slice(&data[..128 * 1024]);
        }
        self.rom = Some(rom_buf);
        info!("DVC ROM caricata ({} byte)", copy_len);
        Ok(())
    }

    pub fn is_fitted(&self) -> bool {
        self.rom.is_some()
    }

    #[inline]
    fn sync_mirrors(&mut self) {
        self.fma_cmd = self.get_fma_word(FMA_CMD);
        self.fma_isr = self.get_fma_word(FMA_ISR);
        self.fma_ier = self.get_fma_word(FMA_IER);
        self.fma_ivec = self.get_fma_word(FMA_IVEC);
        self.fmv_isr = self.get_fmv_word(FMV_ISR);
        self.fmv_ier = self.get_fmv_word(FMV_IER);
        self.fmv_ivec = self.get_fmv_word(FMV_IVEC);
        self.fmv_playback_active = self.video_playing;
        self.fmv_decoder_active = self.decoder_enabled;
    }

    // Helper register getters / setters
    #[inline]
    pub fn get_fma_word(&self, offset: usize) -> u16 {
        u16::from_be_bytes([self.fma_regs[offset], self.fma_regs[offset + 1]])
    }

    #[inline]
    pub fn set_fma_word(&mut self, offset: usize, val: u16) {
        let bytes = val.to_be_bytes();
        self.fma_regs[offset] = bytes[0];
        self.fma_regs[offset + 1] = bytes[1];
        self.sync_mirrors();
    }

    #[inline]
    pub fn or_fma_word(&mut self, offset: usize, val: u16) {
        let cur = self.get_fma_word(offset);
        self.set_fma_word(offset, cur | val);
    }

    #[inline]
    pub fn get_fmv_word(&self, offset: usize) -> u16 {
        u16::from_be_bytes([self.fmv_regs[offset], self.fmv_regs[offset + 1]])
    }

    #[inline]
    pub fn set_fmv_word(&mut self, offset: usize, val: u16) {
        let bytes = val.to_be_bytes();
        self.fmv_regs[offset] = bytes[0];
        self.fmv_regs[offset + 1] = bytes[1];
        self.sync_mirrors();
    }

    #[inline]
    pub fn or_fmv_word(&mut self, offset: usize, val: u16) {
        let cur = self.get_fmv_word(offset);
        self.set_fmv_word(offset, cur | val);
    }

    pub fn irq_pending(&self) -> bool {
        let fma_isr = self.get_fma_word(FMA_ISR);
        let fma_ier = self.get_fma_word(FMA_IER);
        let fma_pending = (fma_isr & fma_ier) != 0;

        let fmv_isr = self.get_fmv_word(FMV_ISR);
        let fmv_ier = self.get_fmv_word(FMV_IER);
        let fmv_pending = (fmv_isr & fmv_ier) != 0;

        fma_pending || fmv_pending
    }

    pub fn intack_vector(&mut self) -> u8 {
        let fma_isr = self.get_fma_word(FMA_ISR);
        let fma_ier = self.get_fma_word(FMA_IER);
        let fma_ivec = self.get_fma_word(FMA_IVEC);
        if (fma_isr & fma_ier) != 0 {
            let vec = (fma_ivec & 0x00FF) as u8;
            if vec != 0 { vec } else { 0x1C }
        } else {
            let vec = ((self.get_fmv_word(FMV_IVEC) >> 3) & 0xFF) as u8;
            if vec != 0 { vec } else { 0x1C }
        }
    }

    pub fn tick(&mut self, cycles: u32) {
        self.total_cycles += cycles as u64;

        // In drain mode (0x1F, end of data or loop) the driver waits for underflow, which comes when the decoded audio has been played.
        if self.fma_playout_end != 0 && self.total_cycles >= self.fma_playout_end {
            self.fma_playout_end = 0;
            let draining = self.get_fma_word(FMA_MODE) == FMA_MODE_DRAIN;
            if draining && (self.audio_enabled || self.audio_armed) && self.fma_underflow_count == 0 && self.fma_drained() {
                self.raise_fma_underflow();
                self.fma_underflow_count = 10;
            }
        }

        // DCLK runs at 45 kHz
        self.dclk_accum += (cycles as u64) * DCLK_FREQ;
        while self.dclk_accum >= MASTER_CLOCK {
            self.dclk_accum -= MASTER_CLOCK;
            self.dclk = self.dclk.wrapping_add(1);
        }

        // FMV Timer runs at 5,625 Hz (period set by TCNT)
        self.timer_accum += (cycles as u64) * 5_625;
        while self.timer_accum >= MASTER_CLOCK {
            self.timer_accum -= MASTER_CLOCK;
            let compare = self.get_fmv_word(FMV_TIMER);
            if compare > 0 && self.timer_counter >= compare {
                self.timer_counter = 0;
                if (self.get_fma_word(FMA_IER) & FMA_IRQ_POLL) != 0 {
                    self.or_fma_word(FMA_ISR, FMA_IRQ_POLL);
                }
                if (self.get_fmv_word(FMV_IER) & FMV_IRQ_TIM) != 0 {
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_TIM);
                }
            } else if compare > 0 {
                self.timer_counter = self.timer_counter.wrapping_add(1);
            }
        }
    }

    pub fn refresh_fmv_dynamic(&mut self) {
        let fifo_ready = self.dvc_stream_fifo.len() < 512 * 1024;
        self.set_fmv_word(0x5E, if fifo_ready { 0x2000 } else { 0 }); // SYS_STS bit 13
        self.set_fmv_word(0x98, ((self.dclk >> 6) & 0xFFFF) as u16);  // GEN_SYSCR DCLK/64
        let queue_len = if self.video_decoder.is_some() { 1 } else { 0 };
        self.set_fmv_word(0xA4, queue_len as u16);                     // Pictures in queue
        let stream = self.get_fmv_word(FMV_STREAM) & 0xF;
        self.set_fmv_word(FMV_STREAM, stream);
    }

    pub fn refresh_fma_dynamic(&mut self, offset: usize) {
        if offset == 0x24 {
            self.set_fma_word(0x24, 0x0004); // HF2 flag ready
        }
        if offset == 0x10 {
            self.set_fma_word(0x12, self.dclk as u16);
        }
        self.set_fma_word(0x10, (self.dclk >> 16) as u16);
        let stream = self.get_fma_word(FMA_STREAM) & 0xF;
        self.set_fma_word(0x0A, stream);
        if self.audio_enabled {
            self.fma_regs[0x18] |= 1;
        }
        let audio_src = if !self.fma_demux.audio.is_empty() {
            &self.fma_demux.audio
        } else {
            &self.demux.audio
        };
        if audio_src.len() >= 4 {
            let header = [
                audio_src[0],
                audio_src[1],
                audio_src[2],
                audio_src[3],
            ];
            self.fma_regs[0x14..0x18].copy_from_slice(&header);
        }
    }

    pub fn acknowledge_fmv_isr(&mut self) {
        self.set_fmv_word(FMV_ISR, 0);
    }

    pub fn acknowledge_fma_isr(&mut self) {
        self.set_fma_word(FMA_ISR, 0);
    }

    fn count_reg_write(&mut self) {
        if !self.ram2_enabled {
            self.reg_write_count = self.reg_write_count.saturating_add(1);
            if self.reg_write_count >= 64 {
                self.ram2_enabled = true;
                info!("DVC Picture DRAM abilitata (64 scritture registri completate)");
            }
        }
    }

    pub fn read_byte(&mut self, addr: u32) -> u8 {
        self.reg_read_count += 1;
        let a = addr & 0x00FF_FFFF;
        match a {
            0xD00000..=0xDFFFFF => self.read_ram1((a - 0xD00000) as usize),
            0xE01000..=0xE01FFF => 0xFF,
            0xE03000..=0xE030FF => {
                let offset = (a - 0xE03000) as usize;
                self.refresh_fma_dynamic(offset);
                let val = self.fma_regs[offset];
                if offset == FMA_ISR + 1 {
                    self.acknowledge_fma_isr();
                }
                val
            }
            0xE04000..=0xE040FF => {
                let offset = (a - 0xE04000) as usize;
                self.refresh_fmv_dynamic();
                let val = self.fmv_regs[offset];
                if offset == FMV_ISR + 1 {
                    self.acknowledge_fmv_isr();
                }
                val
            }
            0xE40000..=0xE7FFFF => self.read_rom((a - 0xE40000) as usize),
            0xE80000..=0xEFFFFF => {
                if self.ram2_enabled {
                    self.read_ram2((a - 0xE80000) as usize)
                } else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write_byte(&mut self, addr: u32, val: u8) {
        let a = addr & 0x00FF_FFFF;
        match a {
            0xD00000..=0xDFFFFF => self.write_ram1((a - 0xD00000) as usize, val),
            0xE01000..=0xE01FFF => {
                self.vcd_pixel_clock_13_5 = (val & 1) != 0;
                self.count_reg_write();
            }
            0xE03000..=0xE030FF => {
                let offset = (a - 0xE03000) as usize;
                self.fma_regs[offset] = val;
                if (offset & 1) != 0 {
                    self.handle_fma_write(offset & !1);
                    self.count_reg_write();
                }
            }
            0xE04000..=0xE040FF => {
                let offset = (a - 0xE04000) as usize;
                self.fmv_regs[offset] = val;
                if (offset & 1) != 0 {
                    self.handle_fmv_write(offset & !1);
                    self.count_reg_write();
                }
            }
            0xE80000..=0xEFFFFF => {
                if self.ram2_enabled {
                    self.write_ram2((a - 0xE80000) as usize, val);
                }
            }
            _ => {}
        }
    }

    pub fn read_word(&mut self, addr: u32) -> u16 {
        let hi = self.read_byte(addr) as u16;
        let lo = self.read_byte(addr + 1) as u16;
        (hi << 8) | lo
    }

    pub fn write_word(&mut self, addr: u32, val: u16) {
        self.write_byte(addr, (val >> 8) as u8);
        self.write_byte(addr + 1, val as u8);
    }

    pub fn read_reg(&mut self, word_offset: u32) -> u16 {
        let addr = 0x00E00000 + ((word_offset & 0x7FFF) << 1);
        self.read_word(addr)
    }

    pub fn write_reg(&mut self, word_offset: u32, data: u16) {
        let addr = 0x00E00000 + ((word_offset & 0x7FFF) << 1);
        self.write_word(addr, data);
    }

    pub fn handle_fma_write(&mut self, offset: usize) {
        let value = self.get_fma_word(offset);
        match offset {
            FMA_CMD => {
                if value & 0x0001 != 0 { // STOP
                    self.audio_armed = false;
                    self.audio_enabled = false;
                    self.fma_stream_fifo.clear();
                    self.fma_demux.clear();
                    self.mp2.reset();
                    self.fma_decoded_since_play = 0;
                    self.fma_playout_end = 0;
                    self.set_fma_word(0x02, 0x0200);
                    self.set_fma_word(FMA_ISR, 0);
                }
                if value & 0x0002 != 0 { // PLAY ARM
                    if !self.audio_enabled {
                        self.fma_decoded_since_play = 0;
                    }
                    self.audio_armed = true;
                    self.audio_enabled = true;
                    self.or_fma_word(0x02, 0x0010);
                    self.or_fma_word(FMA_ISR, 0x0010);
                }
                if value & 0x8000 != 0 { // DMA request Audio
                    self.dma_requested = true;
                    self.transfer_for_fma = true;
                    let status = self.get_fma_word(0x02);
                    self.set_fma_word(0x02, status & !0x0008);
                }
            }
            FMA_STREAM => {
                let selected = (value & 0x1F) as u8;
                self.fma_demux.selected_audio_stream = Some(selected);
                self.demux.selected_audio_stream = Some(selected);
            }
            FMA_ISR => {
                self.acknowledge_fma_isr();
            }
            _ => {}
        }
    }

    pub fn handle_fmv_write(&mut self, offset: usize) {
        let value = self.get_fmv_word(offset);
        match offset {
            FMV_SYSCMD => {
                if value & 0x0008 != 0 { // PLAY
                    self.video_armed = true;
                    self.video_playing = true;
                    self.decoder_enabled = true;
                    self.frames_presented_count = 0;
                    self.underflow_count = 0;
                    self.display_frame_counter = 0;
                    self.decoder_frame_counter = 0;
                    self.display_timecode = 0;
                    self.decoder_timecode = 0;
                    self.set_fmv_word(0x58, 0);
                    self.set_fmv_word(0x5A, 0);
                    self.set_fmv_word(0x08, 0);
                    self.set_fmv_word(0x0A, 0);
                    if self.video_decoder.as_ref().map_or(true, |d| d.is_buffer_empty()) {
                        self.video_decoder = None;
                    }
                }
                if value & 0x0020 != 0 { // CONTINUE
                    self.video_armed = false;
                    self.video_playing = true;
                }
                if value & 0x0010 != 0 { // PAUSE
                    self.video_playing = false;
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_PAI);
                }
                if value & 0x0080 != 0 { // STOP
                    // NOTE: Real DVC hardware keeps the last decoded picture in DRAM.
                    self.video_armed = false;
                    self.video_playing = false;
                    self.decoder_enabled = false;
                    self.underflow_count = 0;
                    self.video_decoder = None;
                    self.frames_presented_count = 0;
                    self.frames_decoded_count = 0;
                    self.frame_accum = 0.0;
                    self.display_frame_counter = 0;
                    self.decoder_frame_counter = 0;
                    self.display_timecode = 0;
                    self.decoder_timecode = 0;
                    self.set_fmv_word(0x58, 0);
                    self.set_fmv_word(0x5A, 0);
                    self.set_fmv_word(0x08, 0);
                    self.set_fmv_word(0x0A, 0);
                    self.dvc_stream_fifo.clear();
                    self.demux.clear();
                    self.set_fmv_word(0x5C, 0x0000);
                }
                if value & 0x0100 != 0 { // RESET
                    self.video_playing = false;
                    self.video_armed = false;
                    self.reset_stream();
                    self.set_fmv_word(FMV_ISR, 0);
                }
                if value & 0x1000 != 0 { // DEC_ON
                    self.decoder_enabled = true;
                }
                if value & 0x2000 != 0 { // DEC_OFF
                    // NOTE: Real DVC hardware keeps the last decoded picture in DRAM.
                    self.decoder_enabled = false;
                    self.video_playing = false;
                    self.underflow_count = 0;
                    self.video_decoder = None;
                    self.frames_presented_count = 0;
                    self.frames_decoded_count = 0;
                    self.frame_accum = 0.0;
                }
                if value & 0x8000 != 0 { // DMA request Video
                    self.dma_requested = true;
                    self.transfer_for_fma = false;
                }
            }
            FMV_VIDCMD => {
                if value & 0x0008 != 0 { // WIN: apply position, window, offset and border
                    self.latch_window();
                }
                if value & 0x0004 != 0 || value & 0x0008 != 0 {
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_VCUP | FMV_IRQ_DCL);
                }
            }
            FMV_STREAM => {
                self.demux.selected_video_stream = Some((value & 0x0F) as u8);
            }
            FMV_ISR => {
                self.acknowledge_fmv_isr();
            }
            FMV_TIMER => {
                self.timer_counter = 0;
            }
            FMV_XFER => {
                self.push_stream_byte((value >> 8) as u8);
                self.push_stream_byte(value as u8);
            }
            _ => {}
        }
    }

    pub fn push_stream_byte(&mut self, byte: u8) {
        if self.dvc_stream_fifo.len() < 1024 * 1024 {
            self.dvc_stream_fifo.push(byte);
        }
    }

    pub fn push_fma_stream_byte(&mut self, byte: u8) {
        self.fma_dma_seen = true;
        if self.fma_stream_fifo.len() < 1024 * 1024 {
            self.fma_stream_fifo.push(byte);
        }
    }

    pub fn process_stream(&mut self) {
        if !self.dvc_stream_fifo.is_empty() {
            let bytes = std::mem::take(&mut self.dvc_stream_fifo);
            self.demux.feed(&bytes);
        }
        if !self.fma_stream_fifo.is_empty() {
            let bytes = std::mem::take(&mut self.fma_stream_fifo);
            self.fma_demux.feed(&bytes);
        }

        // Process audio stream from fma_demux
        if !self.fma_demux.audio.is_empty() {
            let decoded = self.mp2.decode_available(&mut self.fma_demux.audio);
            if decoded != 0 {
                self.fma_underflow_count = 0;
                self.audio_sample_rate = self.mp2.last_sample_rate;
                self.or_fma_word(0x02, 0x0004);
                self.or_fma_word(FMA_ISR, 0x0004);
                self.fma_decoded_since_play += self.mp2.pcm.len() as u64;
                self.extend_playout(self.mp2.pcm.len());
                while let Some(sample) = self.mp2.pcm.pop_front() {
                    if self.audio_out.len() < 32 * 1024 {
                        self.audio_out.push_back(sample);
                    }
                }
            }
        }

        // Process audio stream if demux has any audio (fallback)
        if !self.demux.audio.is_empty() {
            let decoded = self.mp2.decode_available(&mut self.demux.audio);
            if decoded != 0 {
                self.fma_underflow_count = 0;
                self.audio_sample_rate = self.mp2.last_sample_rate;
                self.or_fma_word(0x02, 0x0004);
                self.or_fma_word(FMA_ISR, 0x0004);
                self.fma_decoded_since_play += self.mp2.pcm.len() as u64;
                self.extend_playout(self.mp2.pcm.len());
                while let Some(sample) = self.mp2.pcm.pop_front() {
                    if self.audio_out.len() < 32 * 1024 {
                        self.audio_out.push_back(sample);
                    }
                }
            }
        }

        // Process video stream (drain from both FMV demux and FMA demux)
        let mut video_data = std::mem::take(&mut self.demux.video);
        if !self.fma_demux.video.is_empty() {
            if video_data.is_empty() {
                video_data = std::mem::take(&mut self.fma_demux.video);
            } else {
                video_data.extend(self.fma_demux.video.drain(..));
            }
        }
        if !video_data.is_empty() {
            if self.video_decoder.is_none() {
                trace!(">>> [DVC] INITIALIZING VIDEO DECODER WITH FIRST PACKET (len={})", video_data.len());
                self.video_decoder = Some(Video::new(video_data));
            } else {
                self.video_decoder.as_mut().unwrap().append(&video_data);
            }
            if let Some(ref mut dec) = self.video_decoder {
                if dec.has_header() {
                    let w = dec.width() as u16;
                    let h = dec.height() as u16;
                    if w > 0 && h > 0 {
                        self.set_fmv_word(0x52, w);
                        self.set_fmv_word(0x54, h);
                    }
                }
            }
        }

        // Check for program end in video stream
        if self.demux.program_ends > 0 {
            trace!(">>> [DVC FMV] PROGRAM END (0xB9) IN VIDEO STREAM (fma_dma_seen={})", self.fma_dma_seen);
            self.or_fmv_word(FMV_ISR, FMV_IRQ_EII | FMV_IRQ_ESI);
            if !self.fma_dma_seen {
                self.or_fma_word(FMA_ISR, FMA_IRQ_EOI);
            }
            self.demux.program_ends = 0;
        }

        // Check for program end in audio stream
        if self.fma_demux.program_ends > 0 {
            trace!(">>> [DVC FMA] PROGRAM END (0xB9) IN AUDIO STREAM");
            self.or_fma_word(FMA_ISR, FMA_IRQ_EOI);
            self.fma_demux.program_ends = 0;
        }

        // Update DTS if newly available
        if let Some(dts) = self.demux.last_video_dts {
            self.set_fmv_word(0xA0, ((dts >> 7) & 0x7FFF) as u16); // word 0x50
            self.or_fmv_word(0x8C, 0x4000);                         // word 0x46 bit 14
        }
    }

    pub fn advance_display_timecode(&mut self) {
        self.display_frame_counter += 1;
        let fps = if self.fps > 0.0 { self.fps.round() as u32 } else { 25 };
        let pictures = self.display_frame_counter % fps;
        let total_sec = self.display_frame_counter / fps;
        let seconds = total_sec % 60;
        let total_min = total_sec / 60;
        let minutes = total_min % 60;
        let hours = (total_min / 60) % 24;
        let sec_pic = (((seconds & 0x3F) << 6) | (pictures & 0x3F)) as u16;
        let hr_min = (((hours & 0x1F) << 6) | (minutes & 0x3F)) as u16;
        self.display_timecode = ((hr_min as u32) << 16) | (sec_pic as u32);

        // CL450 register 0x58 = Seconds/Pictures, 0x5A = Hours/Minutes
        // vmpega.rom driver routine 0xE5429E unpacks (MOVE.L (A3), D0):
        //   D0 high word (0x58) -> Seconds (bits 6..11) and Pictures (bits 0..5)
        //   D0 low word (0x5A)  -> Hours (bits 6..10) and Minutes (bits 0..5)
        // returning (Hours << 24) | (Minutes << 16) | (Seconds << 8) | Pictures.
        self.set_fmv_word(0x58, sec_pic); // word 0x2C
        self.set_fmv_word(0x5A, hr_min);  // word 0x2D
    }

    pub fn advance_decoder_timecode(&mut self) {
        self.decoder_frame_counter += 1;
        let fps = if self.fps > 0.0 { self.fps.round() as u32 } else { 25 };
        let pictures = self.decoder_frame_counter % fps;
        let total_sec = self.decoder_frame_counter / fps;
        let seconds = total_sec % 60;
        let total_min = total_sec / 60;
        let minutes = total_min % 60;
        let hours = (total_min / 60) % 24;
        let sec_pic = (((seconds & 0x3F) << 6) | (pictures & 0x3F)) as u16;
        let hr_min = (((hours & 0x1F) << 6) | (minutes & 0x3F)) as u16;
        self.decoder_timecode = ((hr_min as u32) << 16) | (sec_pic as u32);

        // CL450 register 0x08 = Seconds/Pictures, 0x0A = Hours/Minutes
        self.set_fmv_word(0x08, sec_pic); // word 0x04
        self.set_fmv_word(0x0A, hr_min);  // word 0x05
    }

    // Input holds no whole MP2 frame and all decoded audio has been played.
    fn fma_drained(&self) -> bool {
        // Concealment: report underflow only once playback has run (half a second decoded), not during pre-roll.
        let fma_started = self.fma_decoded_since_play >= u64::from(self.audio_sample_rate / 2);
        fma_started
            && self.fma_playout_end == 0
            && self.fma_stream_fifo.is_empty()
            && !Mp2Decoder::has_frame(&self.fma_demux.audio)
            && !Mp2Decoder::has_frame(&self.demux.audio)
    }

    fn raise_fma_underflow(&mut self) {
        trace!(">>> [DVC FMA] AUDIO BUFFER UNDERFLOW (raising FMA_IRQ_UNF)");
        self.or_fma_word(FMA_ISR, FMA_IRQ_UNF);
        self.or_fma_word(0x02, 0x0008);
    }

    // Decoded samples are queued for output at once, but play for their length at the stream's rate.
    fn extend_playout(&mut self, samples: usize) {
        let rate = u64::from(self.audio_sample_rate.max(1));
        self.fma_playout_end = self.fma_playout_end.max(self.total_cycles) + samples as u64 * MASTER_CLOCK / rate;
    }

    pub fn on_vsync(&mut self) {
        self.process_stream();

        // Check for FMA audio buffer drain / underflow when audio is active
        if self.audio_enabled || self.audio_armed {
            if self.fma_drained() {
                self.fma_underflow_count = self.fma_underflow_count.saturating_add(1);
                if self.fma_underflow_count == 10 {
                    self.raise_fma_underflow();
                } else if self.fma_underflow_count > 10 && self.fma_underflow_count % 15 == 0 {
                    self.or_fma_word(FMA_ISR, FMA_IRQ_UNF);
                }
            } else {
                self.fma_underflow_count = 0;
            }
        }

        // Always raise VSYNC in ISR on every vertical blank interval (matches hardware VMPEG ASIC)
        self.or_fmv_word(FMV_ISR, FMV_IRQ_VSYNC);

        self.vsync_counter += 1;

        if let Some(ref d) = self.video_decoder {
            if d.framerate() > 0.0 {
                self.fps = d.framerate();
            }
        }

        // Fractional frame presentation
        let fps = if self.fps > 0.0 { self.fps } else { 25.0 };
        let vsync_hz = if self.vsync_hz > 0.0 { self.vsync_hz } else { 50.0 };
        if self.video_playing || self.decoder_enabled || self.video_decoder.is_some() {
            self.frame_accum += fps / vsync_hz;
        } else {
            self.frame_accum = 0.0;
        }

        if self.frame_accum >= 1.0 {
            self.frame_accum -= 1.0;
            if self.frame_accum >= 1.0 {
                self.frame_accum = self.frame_accum.min(1.0);
            }

            let mut decoded_w = 0;
            let mut decoded_h = 0;
            let mut got_frame = false;
            let mut gop_seen = false;
            let mut seq_seen = false;
            let mut gop_tc = None;
            if let Some(ref mut decoder) = self.video_decoder {
                if let Some(frame_idx) = decoder.decode() {
                    self.frames_decoded_count += 1;
                    self.frames_presented_count += 1;
                    gop_seen = decoder.take_gop_seen();
                    seq_seen = decoder.take_sequence_seen();
                    gop_tc = decoder.take_gop_timecode();
                    if self.frames_decoded_count % 30 == 1 {
                        trace!(">>> [DVC] DECODED MPEG FRAME #{} (idx={})", self.frames_decoded_count, frame_idx);
                    }
                    decoded_w = decoder.width();
                    decoded_h = decoder.height();
                    let target_w = (decoded_w as usize).min(FB_MAX_W).max(1);
                    let target_h = (decoded_h as usize).min(FB_MAX_H).max(1);
                    decoder.frame(frame_idx).write_rgb_scaled(&mut *self.framebuffer, target_w, target_h);
                    self.video_width = target_w;
                    self.video_height = target_h;
                    self.has_valid_frame = true;
                    got_frame = true;
                }
            }

            if got_frame {
                self.underflow_count = 0;
                // Update frame geometry & timing registers
                self.set_fmv_word(0x52, decoded_w as u16);
                self.set_fmv_word(0x54, decoded_h as u16);
                self.set_fmv_word(0x06, 0x23);  // PRPA
                self.set_fmv_word(0x56, 1800);  // Frame rate ratio

                // Synchronize with embedded MPEG-1 GOP timecode if present, otherwise advance by 1 frame
                if let Some(tc) = gop_tc {
                    let fps = if self.fps > 0.0 { self.fps.round() as u32 } else { 25 };
                    let pictures = tc & 0x3F;
                    let seconds = (tc >> 6) & 0x3F;
                    let minutes = (tc >> 16) & 0x3F;
                    let hours = (tc >> 22) & 0x1F;
                    let frame_count = ((hours * 3600 + minutes * 60 + seconds) * fps) + pictures;
                    self.display_frame_counter = frame_count;
                    self.decoder_frame_counter = frame_count;
                    self.display_timecode = tc;
                    self.decoder_timecode = tc;
                    let sec_pic = (tc & 0xFFFF) as u16;
                    let hr_min = (tc >> 16) as u16;
                    self.set_fmv_word(0x58, sec_pic);
                    self.set_fmv_word(0x5A, hr_min);
                    self.set_fmv_word(0x08, sec_pic);
                    self.set_fmv_word(0x0A, hr_min);
                } else {
                    self.advance_display_timecode();
                    self.advance_decoder_timecode();
                }

                // Display status: active picture
                self.set_fmv_word(0x5C, 0x0001);

                // Set FMV ISR bits
                self.or_fmv_word(FMV_ISR, FMV_IRQ_PIC);
                if gop_seen || self.frames_presented_count == 1 {
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_GOP);
                }
                if seq_seen || self.frames_presented_count == 1 {
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_SEQ);
                }
            } else if self.frames_decoded_count > 0 && (self.video_playing || self.video_armed) {
                let sequence_ended = self
                    .video_decoder
                    .as_mut()
                    .map_or(false, |d| d.take_sequence_end());
                let buffer_drained = self.dvc_stream_fifo.is_empty()
                    && self.demux.video.is_empty()
                    && self.fma_stream_fifo.is_empty()
                    && self.fma_demux.video.is_empty()
                    && self.video_decoder.as_ref().map_or(true, |d| d.is_buffer_empty());

                self.underflow_count = self.underflow_count.saturating_add(1);

                if sequence_ended {
                    trace!(">>> [DVC] END OF SEQUENCE (seq_end) after {} frames", self.frames_decoded_count);
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_EOD | FMV_IRQ_ESI);
                    if !self.fma_dma_seen {
                        self.or_fma_word(FMA_ISR, FMA_IRQ_EOI);
                    }
                    self.set_fmv_word(0x5C, 0x0000); // Return display status to idle
                    self.video_playing = false;
                    self.video_armed = false;
                } else if buffer_drained {
                    // Real CL450 MPEG-1 decoder asserts FMV_IRQ_NDAT (Need Data) on underflow,
                    // repeating the last frame in DRAM without stopping or destroying the decoder.
                    self.or_fmv_word(FMV_ISR, FMV_IRQ_NDAT);
                }
            } else if (self.get_fmv_word(FMV_IER) & FMV_IRQ_NDAT) != 0
                && self.dvc_stream_fifo.is_empty()
                && self.demux.video.is_empty()
                && self.fma_stream_fifo.is_empty()
                && self.fma_demux.video.is_empty()
            {
                self.or_fmv_word(FMV_ISR, FMV_IRQ_NDAT);
            }
        }
    }

    pub fn read_ram1(&self, offset: usize) -> u8 {
        self.ram1[offset & 0xFFFFF]
    }

    pub fn write_ram1(&mut self, offset: usize, val: u8) {
        self.ram1[offset & 0xFFFFF] = val;
    }

    pub fn read_ram2(&self, offset: usize) -> u8 {
        if self.ram2_enabled {
            self.ram2[offset & 0x7FFFF]
        } else {
            0x00
        }
    }

    pub fn write_ram2(&mut self, offset: usize, val: u8) {
        if self.ram2_enabled {
            self.ram2[offset & 0x7FFFF] = val;
        }
    }

    pub fn read_rom(&mut self, offset: usize) -> u8 {
        self.rom_read_count += 1;
        if let Some(ref rom) = self.rom {
            rom[offset % rom.len()]
        } else {
            0xFF
        }
    }

    fn latch_window(&mut self) {
        for i in 0..6 {
            self.window[i] = self.get_fmv_word(FMV_SCRPOS_Y + i * 2);
        }
        // Border components are studio-range levels (16 = black); expand like the MCD212 output does.
        let level = |off: usize| ((((self.get_fmv_word(off) & 0xFF) as i32 - 16) * 298 + 128) >> 8).clamp(0, 255) as u32;
        self.border_color = 0xFF00_0000 | (level(FMV_BORDER_R) << 16) | (level(FMV_BORDER_R + 2) << 8) | level(FMV_BORDER_R + 4);
    }

    /// One field line of DVC output at 768 high-resolution pixels; `present` marks pixels inside the display window.
    pub fn get_scanline(&self, active_line: usize, out: &mut [u32; 768], present: &mut [bool; 768]) {
        present.fill(false);
        if !self.has_valid_frame {
            out.fill(0xFF00_0000);
            return;
        }
        out.fill(self.border_color);
        let [pos_y, pos_x, win_h, win_w, off_y, off_x] = self.window.map(usize::from);
        let (pic_w, pic_h) = (self.video_width, self.video_height);
        // The window is clipped to the decoded picture (mv_window adapts its size the same way).
        let off_x = off_x.min(pic_w);
        let off_y = off_y.min(pic_h);
        let win_w = win_w.min(pic_w - off_x);
        let win_h = win_h.min(pic_h - off_y);
        if active_line < pos_y || active_line >= pos_y + win_h {
            return;
        }
        let row = (off_y + active_line - pos_y) * pic_w + off_x;
        // One picture pixel per normal-resolution pixel, so two high-resolution pixels each; no scaling.
        for col in 0..win_w {
            let x = (pos_x + col) * 2;
            if x + 1 >= 768 {
                break;
            }
            let pixel = self.framebuffer[row + col];
            out[x] = pixel;
            out[x + 1] = pixel;
            present[x] = true;
            present[x + 1] = true;
        }
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn window_places_and_clips_the_picture() {
        let mut d = Dvc::new();
        d.video_width = 8;
        d.video_height = 4;
        for (i, px) in d.framebuffer[..32].iter_mut().enumerate() {
            *px = 0xFF00_0000 | i as u32;
        }
        d.has_valid_frame = true;
        // Screen y 10, x 5; window 3 lines by 6 pixels; picture offset y 1, x 4 (width clips to 4).
        for (i, v) in [10u16, 5, 3, 6, 1, 4].iter().enumerate() {
            d.set_fmv_word(FMV_SCRPOS_Y + i * 2, *v);
        }
        d.set_fmv_word(FMV_VIDCMD, 0x0008);
        d.handle_fmv_write(FMV_VIDCMD);
        let mut out = [0u32; 768];
        let mut present = [false; 768];
        d.get_scanline(9, &mut out, &mut present);
        assert!(!present.iter().any(|&p| p));
        d.get_scanline(10, &mut out, &mut present);
        assert_eq!(&out[10..18], &[0xFF00_000C, 0xFF00_000C, 0xFF00_000D, 0xFF00_000D, 0xFF00_000E, 0xFF00_000E, 0xFF00_000F, 0xFF00_000F]);
        assert!(present[10..18].iter().all(|&p| p) && !present[9] && !present[18]);
        d.get_scanline(12, &mut out, &mut present);
        assert_eq!(out[10], 0xFF00_001C);
        d.get_scanline(13, &mut out, &mut present);
        assert!(!present.iter().any(|&p| p));
    }
}
