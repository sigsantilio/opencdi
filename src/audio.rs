use std::collections::VecDeque;
use std::sync::Mutex;
#[cfg(feature = "host")]
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
#[cfg(feature = "host")]
use log::{info, warn, error};
use crate::mcd212::VideoMode;
#[cfg(feature = "host")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

// CD-i / CD-ROM XA ADPCM Filter Coefficients (K0, K1 scaled by 256)
// From Philips CD-i Green Book & MAME cdicdic.cpp
const XA_FILTER_COEF: [[i32; 2]; 4] = [
    [0x000,  0x000],  // Filter 0: K0 = 0.0,      K1 = 0.0
    [0x0F0,  0x000],  // Filter 1: K0 = 240/256,  K1 = 0.0
    [0x1CC, -0x0D0],  // Filter 2: K0 = 460/256,  K1 = -208/256
    [0x188, -0x0DC],  // Filter 3: K0 = 392/256,  K1 = -220/256
];

const HEADER_OFFSET_4BIT: [usize; 8] = [4, 5, 6, 7, 12, 13, 14, 15];
const DATA_OFFSET_4BIT: [usize; 8]   = [16, 16, 17, 17, 18, 18, 19, 19];

// 8-bit groups repeat the four sound parameters four times; the decoder uses the last copy (bytes 12-15).
const HEADER_OFFSET_8BIT: [usize; 4] = [12, 13, 14, 15];
const DATA_OFFSET_8BIT: [usize; 4]   = [16, 17, 18, 19];

#[derive(Default, Clone, Copy)]
pub struct ChannelState {
    pub s0: i16,
    pub s1: i16,
}

impl ChannelState {
    pub fn reset(&mut self) {
        self.s0 = 0;
        self.s1 = 0;
    }

    #[inline]
    pub fn decode_xa_unit(&mut self, param: u8, sample: i16) -> i16 {
        let filter_idx = ((param >> 4) & 3) as usize;
        let filter = XA_FILTER_COEF[filter_idx];
        let range = (param & 0x0F).min(12) as usize;

        let sample32 = ((sample as i32) >> range)
            + ((filter[0] * (self.s0 as i32) + filter[1] * (self.s1 as i32) + 128) >> 8);

        let out = sample32.clamp(-32768, 32767) as i16;
        self.s1 = self.s0;
        self.s0 = out;
        out
    }
}

pub type Frame = (f32, f32);

// Rate of the emulated audio output; ADPCM (37.8 and 18.9 kHz) is resampled to it, MPEG audio runs at it.
pub const OUT_RATE: u32 = 44_100;
const SECTORS_PER_SECOND: u64 = 75;

pub fn frame_cycles(mode: VideoMode) -> u64 {
    mode.total_scanlines() as u64 * mode.cycles_per_scanline() as u64
}

// Video frame rate on the CD sector clock (75 sectors per second), so one frame of audio is exactly what the emulation produces.
pub fn frame_rate(mode: VideoMode) -> f64 {
    (SECTORS_PER_SECOND * mode.cycles_per_sector() as u64) as f64 / frame_cycles(mode) as f64
}

// Hard clip at full scale, then scale to 16 bits.
pub fn to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0) as i16
}

// Output frames due for the cycles ticked: OUT_RATE / 75 per sector period, remainder carried.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct OutputClock {
    pub rem: u64,
}

impl OutputClock {
    pub fn advance(&mut self, cycles: u64, sector_cycles: u32) -> usize {
        self.rem += (OUT_RATE as u64 / SECTORS_PER_SECOND) * cycles;
        let n = self.rem / sector_cycles as u64;
        self.rem %= sector_cycles as u64;
        n as usize
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

// Four-path gain matrix (left-left, left-right, right-right, right-left) on a stereo run.
fn apply_gains(frames: &mut [Frame], [ll, lr, rr, rl]: [f32; 4]) {
    for (l, r) in frames.iter_mut() {
        (*l, *r) = (*l * ll + *r * rl, *r * rr + *l * lr);
    }
}

// Polyphase Catmull-Rom resampler to OUT_RATE with an exact rational step; plain f32 ops (no FMA), so results are the same on every IEEE-754 target.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Resampler {
    rate: u32,
    num: u32,
    den: u32,
    pos: u32,
    hist: [Frame; 3],
}

// Catmull-Rom weights for the four inputs around the interval the phase t (0 to 1) falls in.
fn catmull_rom(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [(2.0 * t2 - t3 - t) * 0.5, (3.0 * t3 - 5.0 * t2 + 2.0) * 0.5, (4.0 * t2 - 3.0 * t3 + t) * 0.5, (t3 - t2) * 0.5]
}

impl Resampler {
    pub const fn new() -> Self {
        Resampler { rate: 0, num: 0, den: 0, pos: 0, hist: [(0.0, 0.0); 3] }
    }

    // A new rate restarts the phase and keeps the history, so a sector edge has no step.
    pub fn set_rate(&mut self, rate: u32) {
        if rate == 0 || rate == self.rate {
            return;
        }
        let g = gcd(rate, OUT_RATE);
        (self.rate, self.num, self.den, self.pos) = (rate, rate / g, OUT_RATE / g, 0);
    }

    // Output trails input by two samples (the cubic needs the next two).
    pub fn process(&mut self, input: &[Frame], out: &mut Vec<Frame>) {
        if self.den == 0 {
            return;
        }
        let den = self.den as f32;
        for &d in input {
            let [a, b, c] = self.hist;
            while self.pos < self.den {
                let w = catmull_rom(self.pos as f32 / den);
                out.push((
                    w[0] * a.0 + w[1] * b.0 + w[2] * c.0 + w[3] * d.0,
                    w[0] * a.1 + w[1] * b.1 + w[2] * c.1 + w[3] * d.1,
                ));
                self.pos += self.num;
            }
            self.pos -= self.den;
            self.hist = [b, c, d];
        }
    }
}

// Silence in front of an ADPCM stream that starts from empty: one sector period, as the decoder needs a whole sector first.
pub const ADPCM_LATENCY: usize = OUT_RATE as usize / 75;
// An MPEG stream starts 70 frames (1.6 ms) after its first decoded samples, as the decoder output FIFO fills first.
pub const MPEG_LATENCY: usize = 70;
// An MPEG stream holding more than this (two seconds) is reported; nothing is dropped.
const MPEG_WARN: usize = 2 * OUT_RATE as usize;

// One stream of the output mix at OUT_RATE.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Stream {
    fifo: VecDeque<Frame>,
    primed: bool,
    warned: bool,
    pub restarts: u64,
    pub warnings: u64,
    pub frames_in: u64,
}

impl Stream {
    // Queues `src`, after `latency` frames of silence when the stream starts; holding more than `bound` frames is reported once per start.
    pub fn push(&mut self, src: &[Frame], latency: usize, bound: usize, what: &dyn Fn() -> String) {
        if src.is_empty() {
            return;
        }
        self.frames_in += src.len() as u64;
        if !self.primed {
            self.fifo.resize(self.fifo.len() + latency, (0.0, 0.0));
            self.primed = true;
            self.warned = false;
            self.restarts += 1;
        }
        self.fifo.extend(src.iter().copied());
        if self.fifo.len() > bound && !self.warned {
            self.warned = true;
            self.warnings += 1;
            eprintln!("audio: {} stream holds {} frames, more than {}", what(), self.fifo.len(), bound);
        }
    }

    // Adds up to out.len() frames into out; running dry ends the stream, so the next push prefills again.
    pub fn mix_into(&mut self, out: &mut [Frame]) {
        let n = out.len().min(self.fifo.len());
        for (o, s) in out.iter_mut().zip(self.fifo.drain(..n)) {
            o.0 += s.0;
            o.1 += s.1;
        }
        if n < out.len() {
            self.primed = false;
        }
    }

    pub fn len(&self) -> usize {
        self.fifo.len()
    }

    // Drops the unplayed frames; the next push starts the stream again.
    pub fn clear(&mut self) {
        self.fifo.clear();
        self.primed = false;
    }

    pub fn is_empty(&self) -> bool {
        self.fifo.is_empty()
    }
}

// Coding of the last ADPCM sector a stream received: output frames per sector and the level, for the hold bound.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct SectorKind {
    pub frames: usize,
    pub is_8bit: bool,
    pub is_stereo: bool,
    pub rate: u32,
}

impl SectorKind {
    // Start latency plus one playing and one waiting sector.
    fn bound(&self) -> usize {
        ADPCM_LATENCY + 2 * self.frames
    }

    fn level(&self) -> String {
        let level = if self.is_8bit { "A" } else if self.rate == 18900 { "C" } else { "B" };
        format!("level {} {}", level, if self.is_stereo { "stereo" } else { "mono" })
    }
}

// The board's audio output: decoders and the DVC feed three streams, mixed at OUT_RATE as the board is ticked.
pub struct CdiAudio {
    pub left_state: ChannelState,
    pub right_state: ChannelState,
    pub map_left_state: ChannelState,
    pub map_right_state: ChannelState,
    pub resampler: Resampler,
    pub map_resampler: Resampler,
    pub attenuation: [u8; 4],
    pub total_sectors_decoded: u64,
    // ADPCM disc or CPU-fed, audio map or TACS, DVC MPEG audio.
    pub main: Stream,
    pub sfx: Stream,
    pub mpeg: Stream,
    rate_warned: bool,
    clock: OutputClock,
    out: Vec<Frame>,
    scratch: Vec<Frame>,
}

impl Default for CdiAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl CdiAudio {
    pub fn new() -> Self {
        Self {
            left_state: ChannelState::default(),
            right_state: ChannelState::default(),
            map_left_state: ChannelState::default(),
            map_right_state: ChannelState::default(),
            resampler: Resampler::new(),
            map_resampler: Resampler::new(),
            attenuation: [0xFF, 0, 0xFF, 0],
            total_sectors_decoded: 0,
            main: Stream::default(),
            sfx: Stream::default(),
            mpeg: Stream::default(),
            rate_warned: false,
            clock: OutputClock::default(),
            out: Vec::new(),
            scratch: Vec::new(),
        }
    }

    // Player reset: the decoders restart; what the streams already hold plays on.
    pub fn reset(&mut self) {
        self.left_state.reset();
        self.right_state.reset();
        self.map_left_state.reset();
        self.map_right_state.reset();
        self.resampler = Resampler::new();
        self.map_resampler = Resampler::new();
        self.attenuation = [0xFF, 0, 0xFF, 0];
    }

    // Player attenuation matrix of the Mono-I slave (left-to-left, left-to-right, right-to-right, right-to-left); None while it passes stereo straight through.
    pub fn slave_gains(&self) -> Option<[f32; 4]> {
        if self.attenuation == [0xFF, 0, 0xFF, 0] {
            return None;
        }
        let g = self.attenuation.map(|a| a as f32 / 255.0);
        Some(g)
    }

    // Mixes the output frames that fall in `cycles` of board time; `gains` is the player attenuation as it stands now (ADPCM streams only), `dvc_gains` the cartridge's own (MPEG stream only).
    pub fn advance(&mut self, cycles: u32, sector_cycles: u32, gains: Option<[f32; 4]>, dvc_gains: Option<[f32; 4]>) {
        let n = self.clock.advance(cycles as u64, sector_cycles);
        let start = self.out.len();
        self.out.resize(start + n, (0.0, 0.0));
        let frames = &mut self.out[start..];
        self.main.mix_into(frames);
        self.sfx.mix_into(frames);
        if let Some(g) = gains {
            apply_gains(frames, g);
        }
        match dvc_gains {
            Some(g) if !self.mpeg.is_empty() => {
                self.scratch.clear();
                self.scratch.resize(n, (0.0, 0.0));
                self.mpeg.mix_into(&mut self.scratch);
                apply_gains(&mut self.scratch, g);
                for (o, s) in self.out[start..].iter_mut().zip(self.scratch.iter()) {
                    o.0 += s.0;
                    o.1 += s.1;
                }
            }
            _ => self.mpeg.mix_into(frames),
        }
    }

    // Queues decoded DVC PCM as it is; MPEG audio is 44.1 kHz, another rate is reported once and plays at the wrong pitch. It plays MPEG_LATENCY frames after the stream starts.
    pub fn push_mpeg(&mut self, pcm: &mut VecDeque<Frame>, rate: u32) {
        if pcm.is_empty() {
            return;
        }
        if rate != OUT_RATE && !self.rate_warned {
            self.rate_warned = true;
            eprintln!("audio: mpeg stream at {} Hz is not supported, played as {} Hz", rate, OUT_RATE);
        }
        self.scratch.clear();
        self.scratch.extend(pcm.drain(..));
        self.mpeg.push(&self.scratch, MPEG_LATENCY, MPEG_WARN, &|| format!("mpeg {} Hz", rate));
    }

    // FMA stop or reset: the unplayed DVC audio goes.
    pub fn stop_mpeg(&mut self) {
        self.mpeg.clear();
    }

    // Appends the mixed frames made since the last call; returns how many.
    pub fn drain(&mut self, out: &mut Vec<Frame>) -> usize {
        let n = self.out.len();
        out.append(&mut self.out);
        n
    }

    pub fn set_attenuation(&mut self, atten: [u8; 4]) {
        self.attenuation = atten;
    }

    fn decode_xa_payload(
        payload: &[u8],
        is_stereo: bool,
        is_8bit: bool,
        left_state: &mut ChannelState,
        right_state: &mut ChannelState,
    ) -> Vec<(f32, f32)> {
        let groups = payload.len() / 128;
        let mut samples = Vec::with_capacity(groups * 224);

        for g in 0..groups {
            let group = &payload[g * 128..(g + 1) * 128];

            if is_8bit {
                if is_stereo {
                    let mut group_samples = [[0i16; 56]; 2];
                    for u in 0..4 {
                        let ch = u & 1;
                        let param = group[HEADER_OFFSET_8BIT[u]];
                        let data_start = DATA_OFFSET_8BIT[u];
                        let sample_base = (u >> 1) * 28;

                        for line in 0..28 {
                            let raw_sample = ((group[data_start + line * 4] as u16) << 8) as i16;
                            let out = if ch == 0 {
                                left_state.decode_xa_unit(param, raw_sample)
                            } else {
                                right_state.decode_xa_unit(param, raw_sample)
                            };
                            group_samples[ch][sample_base + line] = out;
                        }
                    }
                    for s in 0..56 {
                        let sl = group_samples[0][s] as f32 / 32768.0;
                        let sr = group_samples[1][s] as f32 / 32768.0;
                        samples.push((sl, sr));
                    }
                } else {
                    let mut mono_samples = [0i16; 112];
                    for u in 0..4 {
                        let param = group[HEADER_OFFSET_8BIT[u]];
                        let data_start = DATA_OFFSET_8BIT[u];
                        let sample_base = u * 28;

                        for line in 0..28 {
                            let raw_sample = ((group[data_start + line * 4] as u16) << 8) as i16;
                            mono_samples[sample_base + line] = left_state.decode_xa_unit(param, raw_sample);
                        }
                    }
                    for s in 0..112 {
                        let sm = mono_samples[s] as f32 / 32768.0;
                        samples.push((sm, sm));
                    }
                }
            } else {
                // 4-bit ADPCM (Level B / Level C)
                if is_stereo {
                    let mut group_samples = [[0i16; 112]; 2];
                    for u in 0..8 {
                        let ch = u & 1;
                        let param = group[HEADER_OFFSET_4BIT[u]];
                        let data_start = DATA_OFFSET_4BIT[u];
                        let shift = if (u & 1) != 0 { 4 } else { 0 };
                        let sample_base = (u >> 1) * 28;

                        for line in 0..28 {
                            let nibble = (group[data_start + line * 4] >> shift) & 0x0F;
                            let raw_sample = ((nibble as u16) << 12) as i16;
                            let out = if ch == 0 {
                                left_state.decode_xa_unit(param, raw_sample)
                            } else {
                                right_state.decode_xa_unit(param, raw_sample)
                            };
                            group_samples[ch][sample_base + line] = out;
                        }
                    }
                    for s in 0..112 {
                        let sl = group_samples[0][s] as f32 / 32768.0;
                        let sr = group_samples[1][s] as f32 / 32768.0;
                        samples.push((sl, sr));
                    }
                } else {
                    let mut mono_samples = [0i16; 224];
                    for u in 0..8 {
                        let param = group[HEADER_OFFSET_4BIT[u]];
                        let data_start = DATA_OFFSET_4BIT[u];
                        let shift = if (u & 1) != 0 { 4 } else { 0 };
                        let sample_base = u * 28;

                        for line in 0..28 {
                            let nibble = (group[data_start + line * 4] >> shift) & 0x0F;
                            let raw_sample = ((nibble as u16) << 12) as i16;
                            mono_samples[sample_base + line] = left_state.decode_xa_unit(param, raw_sample);
                        }
                    }
                    for s in 0..224 {
                        let sm = mono_samples[s] as f32 / 32768.0;
                        samples.push((sm, sm));
                    }
                }
            }
        }
        samples
    }

    // Brings a decoded sector from its ADPCM rate to OUT_RATE.
    fn resample(r: &mut Resampler, samples: &[Frame], sample_rate: u32) -> Vec<Frame> {
        let mut out = Vec::with_capacity(samples.len() * 7 / 3 + 2);
        r.set_rate(sample_rate);
        r.process(samples, &mut out);
        out
    }

    pub fn decode_sector(&mut self, payload: &[u8], is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() < 18 * 128 {
            return;
        }

        self.total_sectors_decoded += 1;

        let samples = Self::decode_xa_payload(
            payload,
            is_stereo,
            is_8bit,
            &mut self.left_state,
            &mut self.right_state,
        );
        let final_samples = Self::resample(&mut self.resampler, &samples, sample_rate);

        let k = SectorKind { frames: final_samples.len(), is_8bit, is_stereo, rate: sample_rate };
        self.main.push(&final_samples, ADPCM_LATENCY, k.bound(), &|| format!("main {}", k.level()));
    }

    // A raw CD-DA sector (16-bit little-endian stereo at 44.1 kHz, the mixer rate) goes straight into the main stream.
    pub fn push_cdda(&mut self, sector: &[u8]) {
        let frames: Vec<Frame> = sector
            .chunks_exact(4)
            .map(|c| (i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0, i16::from_le_bytes([c[2], c[3]]) as f32 / 32768.0))
            .collect();
        let bound = ADPCM_LATENCY + 2 * frames.len();
        self.main.push(&frames, ADPCM_LATENCY, bound, &|| "main cdda".to_string());
    }

    pub fn decode_audio_map_sector(&mut self, payload: &[u8], is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() >= 18 * 128 {
            self.decode_audio_map_groups(&payload[..18 * 128], true, is_stereo, is_8bit, sample_rate);
        }
    }

    // Decodes sound groups of the audio map buffer that is playing; `first` marks the start of a buffer.
    pub fn decode_audio_map_groups(&mut self, payload: &[u8], first: bool, is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() < 128 {
            return;
        }

        if first {
            self.total_sectors_decoded += 1;
        }

        let samples = Self::decode_xa_payload(
            payload,
            is_stereo,
            is_8bit,
            &mut self.map_left_state,
            &mut self.map_right_state,
        );
        let final_samples = Self::resample(&mut self.map_resampler, &samples, sample_rate);

        let k = SectorKind { frames: final_samples.len() * 18 / (payload.len() / 128), is_8bit, is_stereo, rate: sample_rate };
        self.sfx.push(&final_samples, ADPCM_LATENCY, k.bound(), &|| format!("sfx {}", k.level()));
    }
}

#[cfg(feature = "host")]
pub struct HostAudioOutput {
    _stream: cpal::Stream,
}

// Most frames the sound-card ring holds (about 1.7 s); the oldest are dropped beyond it.
pub const RING_CAP: usize = 65536;

// Appends output frames to the sound-card ring, dropping the oldest beyond RING_CAP.
pub fn feed_ring(ring: &Mutex<VecDeque<Frame>>, frames: &[Frame]) {
    if let Ok(mut q) = ring.lock() {
        q.extend(frames.iter().copied());
        if q.len() > RING_CAP {
            let n = q.len() - RING_CAP;
            q.drain(..n);
        }
    }
}

#[cfg(feature = "host")]
impl HostAudioOutput {
    // Plays the ring (OUT_RATE frames) on the default device; `gain` holds the volume as f32 bits (0 is mute).
    pub fn try_start(ring: Arc<Mutex<VecDeque<Frame>>>, gain: Arc<AtomicU32>) -> Option<Self> {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                warn!("No audio output device found; running in silent mode");
                return None;
            }
        };

        info!("Initializing audio on default output device");

        let default_config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                warn!("Could not get default output config: {:?}; running silent", e);
                return None;
            }
        };

        let sample_format = default_config.sample_format();
        let config: cpal::StreamConfig = default_config.into();
        let host_sample_rate = config.sample_rate as f64;
        let channels = config.channels as usize;

        info!(
            "Host audio initialized: {} Hz, {} channels, format {:?}",
            config.sample_rate, channels, sample_format
        );

        let mut phase: f64 = 0.0;
        let mut curr = (0.0f32, 0.0f32);
        let mut next = (0.0f32, 0.0f32);
        let mut prebuffering = true;
        let mut starved: usize = 0;

        let err_fn = |err| error!("Audio output stream error: {:?}", err);

        let stream_res = device.build_output_stream(
            config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let vol = f32::from_bits(gain.load(Ordering::Relaxed));
                let mut q = match ring.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        output.fill(0.0);
                        return;
                    }
                };
                let q_len = q.len();

                // Start playing once about 40 ms are queued.
                const PREBUFFER: usize = 1536;
                if prebuffering && q_len >= PREBUFFER {
                    prebuffering = false;
                    curr = q.pop_front().unwrap_or((0.0, 0.0));
                    next = q.pop_front().unwrap_or(curr);
                    phase = 0.0;
                    starved = 0;
                }
                if prebuffering {
                    output.fill(0.0);
                    return;
                }

                // Proportional rate control towards about 8192 queued frames absorbs the drift between emulated and wall-clock time.
                const TARGET: f64 = 8192.0;
                let diff = (q_len as f64 - TARGET) / TARGET;
                let drc = (1.0 + diff * 0.008).clamp(0.990, 1.010);
                let step = (OUT_RATE as f64 / host_sample_rate) * drc;

                #[inline]
                fn soft_saturate(x: f32) -> f32 {
                    if x > 1.25 {
                        1.0
                    } else if x < -1.25 {
                        -1.0
                    } else {
                        x - 0.16 * x * x * x
                    }
                }

                let mut out_idx = 0;
                while out_idx < output.len() {
                    let (mut l, mut r) = (0.0f32, 0.0f32);
                    if !prebuffering {
                        while phase >= 1.0 {
                            phase -= 1.0;
                            curr = next;
                            if let Some(s) = q.pop_front() {
                                next = s;
                                starved = 0;
                            } else {
                                starved += 1;
                                next = (curr.0 * 0.95, curr.1 * 0.95);
                                if starved >= 512 {
                                    prebuffering = true;
                                    break;
                                }
                            }
                        }
                        if !prebuffering {
                            let t = phase as f32;
                            l = (curr.0 * (1.0 - t) + next.0 * t) * vol;
                            r = (curr.1 * (1.0 - t) + next.1 * t) * vol;
                            phase += step;
                        }
                    }
                    l = soft_saturate(l);
                    r = soft_saturate(r);

                    if channels == 1 {
                        output[out_idx] = (l + r) * 0.5;
                        out_idx += 1;
                    } else if channels >= 2 {
                        output[out_idx] = l;
                        output[out_idx + 1] = r;
                        for ch in 2..channels {
                            if out_idx + ch < output.len() {
                                output[out_idx + ch] = 0.0;
                            }
                        }
                        out_idx += channels;
                    }
                }
            },
            err_fn,
            None,
        );

        match stream_res {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    warn!("Failed to play audio stream: {:?}; running silent", e);
                    return None;
                }
                info!("Real-time host audio stream started successfully");
                Some(Self { _stream: stream })
            }
            Err(e) => {
                warn!("Failed to build output stream: {:?}; running silent", e);
                None
            }
        }
    }
}

// Savestate blob of the mixer: decoder history, resamplers, stream queues and the output clock.
struct Blob<'a>(&'a [u8]);

impl Blob<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_at_checked(N)?;
        self.0 = rest;
        head.try_into().ok()
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take::<8>().map(u64::from_le_bytes)
    }
    fn i16(&mut self) -> Option<i16> {
        self.take::<2>().map(i16::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
    fn frame(&mut self) -> Option<Frame> {
        Some((self.f32()?, self.f32()?))
    }
    fn frames(&mut self) -> Option<Vec<Frame>> {
        let n = self.u32()? as usize;
        if n > self.0.len() / 8 {
            return None;
        }
        (0..n).map(|_| self.frame()).collect()
    }
}

fn put_frame(w: &mut Vec<u8>, f: Frame) {
    w.extend_from_slice(&f.0.to_bits().to_le_bytes());
    w.extend_from_slice(&f.1.to_bits().to_le_bytes());
}

fn put_frames<'a>(w: &mut Vec<u8>, n: usize, frames: impl Iterator<Item = &'a Frame>) {
    w.extend_from_slice(&(n as u32).to_le_bytes());
    for &f in frames {
        put_frame(w, f);
    }
}

impl Resampler {
    fn save(&self, w: &mut Vec<u8>) {
        for v in [self.rate, self.num, self.den, self.pos] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for &f in &self.hist {
            put_frame(w, f);
        }
    }

    fn load(&mut self, r: &mut Blob) -> Option<()> {
        (self.rate, self.num, self.den, self.pos) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        for f in &mut self.hist {
            *f = r.frame()?;
        }
        Some(())
    }
}

impl Stream {
    fn save(&self, w: &mut Vec<u8>) {
        put_frames(w, self.fifo.len(), self.fifo.iter());
        w.push(self.primed as u8);
    }

    fn load(&mut self, r: &mut Blob) -> Option<()> {
        self.fifo = r.frames()?.into();
        self.primed = r.take::<1>()?[0] != 0;
        Some(())
    }
}

impl CdiAudio {
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = Vec::new();
        for s in [&self.left_state, &self.right_state, &self.map_left_state, &self.map_right_state] {
            w.extend_from_slice(&s.s0.to_le_bytes());
            w.extend_from_slice(&s.s1.to_le_bytes());
        }
        w.extend_from_slice(&self.attenuation);
        w.extend_from_slice(&self.total_sectors_decoded.to_le_bytes());
        self.resampler.save(&mut w);
        self.map_resampler.save(&mut w);
        for s in [&self.main, &self.sfx, &self.mpeg] {
            s.save(&mut w);
        }
        w.extend_from_slice(&self.clock.rem.to_le_bytes());
        put_frames(&mut w, self.out.len(), self.out.iter());
        w
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Option<()> {
        let mut r = Blob(bytes);
        for s in [&mut self.left_state, &mut self.right_state, &mut self.map_left_state, &mut self.map_right_state] {
            (s.s0, s.s1) = (r.i16()?, r.i16()?);
        }
        self.attenuation = r.take::<4>()?;
        self.total_sectors_decoded = r.u64()?;
        self.resampler.load(&mut r)?;
        self.map_resampler.load(&mut r)?;
        for s in [&mut self.main, &mut self.sfx, &mut self.mpeg] {
            s.load(&mut r)?;
        }
        self.clock.rem = r.u64()?;
        self.out = r.frames()?;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_attenuation_acts_on_adpcm_at_the_mix_and_not_on_mpeg() {
        let mut a = CdiAudio::new();
        a.main.push(&[(0.5, 0.25); 4000], 0, usize::MAX, &String::new);
        a.mpeg.push(&[(0.1, 0.1); 4000], 0, usize::MAX, &String::new);
        let sector = VideoMode::Pal.cycles_per_sector();
        let mut out = Vec::new();
        a.advance(960, sector, None, None);
        a.advance(960, sector, Some([1.0, 1.0, 1.0, 1.0]), None);
        a.advance(960, sector, Some([0.0, 0.0, 1.0, 0.0]), None);
        a.drain(&mut out);
        let n = out.len() / 3;
        assert_eq!(out[0], (0.6, 0.35));
        assert_eq!(out[n + 1], (0.85, 0.85));
        assert_eq!(out[2 * n + 1], (0.1, 0.35));
    }

    #[test]
    fn cartridge_attenuation_acts_on_mpeg_only() {
        let mut a = CdiAudio::new();
        a.main.push(&[(0.5, 0.25); 100], 0, usize::MAX, &String::new);
        a.mpeg.push(&[(0.4, 0.2); 100], 0, usize::MAX, &String::new);
        a.advance(960, VideoMode::Pal.cycles_per_sector(), None, Some([0.5, 0.0, 0.5, 0.0]));
        let mut out = Vec::new();
        a.drain(&mut out);
        assert_eq!(out[0], (0.7, 0.35));
    }

    #[test]
    fn slave_gains_follow_the_attenuation_registers() {
        let mut a = CdiAudio::new();
        assert_eq!(a.slave_gains(), None);
        a.set_attenuation([255, 255, 0, 0]);
        assert_eq!(a.slave_gains(), Some([1.0, 1.0, 0.0, 0.0]));
    }

    #[test]
    fn eight_bit_groups_use_the_last_parameter_copy() {
        let mut payload = vec![0u8; 18 * 128];
        for g in payload.chunks_exact_mut(128) {
            g[0..12].fill(0x08); // range 8 in the first three copies
            g[12..16].fill(0x00); // range 0 in the last copy
            g[16..].fill(0x01);
        }
        let (mut l, mut r) = (ChannelState::default(), ChannelState::default());
        let out = CdiAudio::decode_xa_payload(&payload, false, true, &mut l, &mut r);
        assert_eq!(out[0].0, 256.0 / 32768.0);
    }

    #[test]
    fn frame_rate_and_lengths_follow_the_sector_clock() {
        let mut c = OutputClock::default();
        let pal: usize = (0..1000).map(|_| c.advance(frame_cycles(VideoMode::Pal), VideoMode::Pal.cycles_per_sector())).sum();
        assert_eq!(pal, (588u64 * 299_520 * 1000 / 200_000) as usize);
        assert!((frame_rate(VideoMode::Pal) - 50.0801).abs() < 1e-4);
        let mut c = OutputClock::default();
        let lens: Vec<usize> = (0..4).map(|_| c.advance(frame_cycles(VideoMode::Ntsc), VideoMode::Ntsc.cycles_per_sector())).collect();
        assert_eq!(lens, [734, 734, 735, 734]);
        assert!((frame_rate(VideoMode::Ntsc) - 60.0543).abs() < 1e-4);
    }

    // Counts rising zero crossings of the left channel.
    fn crossings(s: &[Frame]) -> usize {
        s.windows(2).filter(|w| w[0].0 < 0.0 && w[1].0 >= 0.0).count()
    }

    #[test]
    fn resampler_keeps_pitch_for_adpcm_rates() {
        for rate in [37_800u32, 18_900] {
            let tone = 1000.0f64;
            let input: Vec<Frame> = (0..rate as usize * 2)
                .map(|i| {
                    let v = (2.0 * std::f64::consts::PI * tone * (i as f64 + 0.25) / rate as f64).sin() as f32;
                    (v, v)
                })
                .collect();
            let mut r = Resampler::default();
            r.set_rate(rate);
            let mut out = Vec::new();
            for chunk in input.chunks(2016) {
                r.process(chunk, &mut out);
            }
            let want = OUT_RATE as usize * 2;
            assert!(out.len().abs_diff(want) <= 1, "{} Hz: {} frames, want {}", rate, out.len(), want);
            assert!(crossings(&out[..OUT_RATE as usize]).abs_diff(1000) <= 1, "{} Hz: {} crossings", rate, crossings(&out[..OUT_RATE as usize]));
        }
    }

    #[test]
    fn resampler_follows_a_ramp_and_converts_sector_lengths() {
        let mut r = Resampler::default();
        r.set_rate(37_800);
        let ramp: Vec<Frame> = (0..2016).map(|i| (i as f32, -(i as f32))).collect();
        let mut out = Vec::new();
        r.process(&ramp, &mut out);
        assert_eq!(out.len(), 2016 * 7 / 6);
        // Catmull-Rom is exact on a line: input i sits at output 7i/6 after the two samples of delay.
        let (a, b) = (out[100], out[101]);
        assert!((b.0 - a.0 - 6.0 / 7.0).abs() < 1e-3 && (a.0 + a.1).abs() < 1e-3);
        let mut c = Resampler::default();
        c.set_rate(18_900);
        let mut out = Vec::new();
        c.process(&vec![(0.0, 0.0); 4032], &mut out);
        assert_eq!(out.len(), 9408);
    }

    #[test]
    fn to_i16_clips_hard() {
        assert_eq!((to_i16(2.0), to_i16(-2.0), to_i16(0.5)), (32767, -32767, 16383));
    }

    #[test]
    fn stream_prefills_pads_and_warns() {
        const PREFILL: usize = 2048;
        let mut c = Stream::default();
        let what = || "test".to_string();
        c.push(&[(0.5, 0.5); 100], PREFILL, usize::MAX, &what);
        assert_eq!(c.len(), PREFILL + 100);
        let mut out = vec![(0.0, 0.0); PREFILL + 150];
        c.mix_into(&mut out);
        assert_eq!(out[PREFILL - 1], (0.0, 0.0));
        assert_eq!(out[PREFILL], (0.5, 0.5));
        assert_eq!(out[PREFILL + 100], (0.0, 0.0));
        c.push(&[(0.25, 0.25); 3000], PREFILL, PREFILL + 2000, &what);
        c.push(&[(0.25, 0.25); 10], PREFILL, PREFILL + 2000, &what);
        assert_eq!((c.len(), c.restarts, c.warnings), (PREFILL + 3010, 2, 1));
    }

    #[test]
    fn mpeg_starts_after_the_output_fifo_fills() {
        let mut a = CdiAudio::new();
        a.push_mpeg(&mut VecDeque::from(vec![(0.25, 0.25); 1152]), 44_100);
        assert_eq!(a.mpeg.len(), MPEG_LATENCY + 1152);
        a.advance(960, VideoMode::Pal.cycles_per_sector(), None, None);
        let mut out = Vec::new();
        a.drain(&mut out);
        assert_eq!((out[0], out[1]), ((0.0, 0.0), (0.0, 0.0)));
    }

    #[test]
    fn output_state_round_trips() {
        let sector = VideoMode::Ntsc.cycles_per_sector();
        let run = |a: &mut CdiAudio, v: f32| {
            a.main.push(&[(v, -v); 700], ADPCM_LATENCY, usize::MAX, &String::new);
            a.push_mpeg(&mut VecDeque::from(vec![(0.2, 0.3); 1152]), 44_100);
            for _ in 0..262 {
                a.advance(960, sector, None, None);
            }
        };
        let mut a = CdiAudio::new();
        run(&mut a, 0.1);
        let buf = a.save_state();
        let mut b = CdiAudio::new();
        b.load_state(&buf).unwrap();
        for _ in 0..8 {
            run(&mut a, 0.4);
            run(&mut b, 0.4);
            let (mut x, mut y) = (Vec::new(), Vec::new());
            assert_eq!(a.drain(&mut x), b.drain(&mut y));
            assert_eq!(x, y);
        }
    }
}
