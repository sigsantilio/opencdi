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

// Doubles 18.9 kHz audio to 37.8 kHz: each input passes through and the point halfway to the next is a Catmull-Rom cubic over the four nearest inputs, so output trails input by two samples.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Upsampler {
    pub hist: [(f32, f32); 3],
}

impl Upsampler {
    fn run(&mut self, input: &[(f32, f32)], out: &mut Vec<(f32, f32)>) {
        let mid = |a: f32, b: f32, c: f32, d: f32| (9.0 * (b + c) - (a + d)) / 16.0;
        for &d in input {
            let [a, b, c] = self.hist;
            out.push(b);
            out.push((mid(a.0, b.0, c.0, d.0), mid(a.1, b.1, c.1, d.1)));
            self.hist = [b, c, d];
        }
    }
}

pub type Frame = (f32, f32);

// Rate of the emulated audio output; 18.9 kHz ADPCM arrives already doubled.
pub const OUT_RATE: u32 = 37_800;
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

// Linear interpolation to OUT_RATE with an exact rational step; plain f32 ops (no FMA), so results are the same on every IEEE-754 target.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Resampler {
    rate: u32,
    num: u32,
    den: u32,
    pos: u32,
    prev: Frame,
}

impl Resampler {
    pub const fn new() -> Self {
        Resampler { rate: 0, num: 0, den: 0, pos: 0, prev: (0.0, 0.0) }
    }

    pub fn set_rate(&mut self, rate: u32) {
        if rate == 0 || rate == self.rate {
            return;
        }
        let g = gcd(rate, OUT_RATE);
        (self.rate, self.num, self.den, self.pos) = (rate, rate / g, OUT_RATE / g, 0);
    }

    pub fn process(&mut self, input: &[Frame], out: &mut Vec<Frame>) {
        if self.den == 0 {
            return;
        }
        let den = self.den as f32;
        for &cur in input {
            while self.pos < self.den {
                let t = self.pos as f32 / den;
                out.push((self.prev.0 + (cur.0 - self.prev.0) * t, self.prev.1 + (cur.1 - self.prev.1) * t));
                self.pos += self.num;
            }
            self.pos -= self.den;
            self.prev = cur;
        }
    }
}

// Silence in front of an ADPCM stream that starts from empty: one sector period, as the decoder needs a whole sector first.
pub const ADPCM_LATENCY: usize = OUT_RATE as usize / 75;
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
    pub upsampler: Upsampler,
    pub map_upsampler: Upsampler,
    pub attenuation: [u8; 4],
    /// Gains left-to-left, left-to-right, right-to-right, right-to-left; None passes the stereo pair unchanged.
    pub matrix: Option<[f32; 4]>,
    pub total_sectors_decoded: u64,
    // ADPCM disc or CPU-fed, audio map or TACS, DVC MPEG audio.
    pub main: Stream,
    pub sfx: Stream,
    pub mpeg: Stream,
    resampler: Resampler,
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
            upsampler: Upsampler::default(),
            map_upsampler: Upsampler::default(),
            attenuation: [0xFF, 0, 0xFF, 0],
            matrix: None,
            total_sectors_decoded: 0,
            main: Stream::default(),
            sfx: Stream::default(),
            mpeg: Stream::default(),
            resampler: Resampler::new(),
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
        self.upsampler = Upsampler::default();
        self.map_upsampler = Upsampler::default();
        self.attenuation = [0xFF, 0, 0xFF, 0];
    }

    // Mixes the output frames that fall in `cycles` of board time.
    pub fn advance(&mut self, cycles: u32, sector_cycles: u32) {
        let n = self.clock.advance(cycles as u64, sector_cycles);
        let start = self.out.len();
        self.out.resize(start + n, (0.0, 0.0));
        let frames = &mut self.out[start..];
        self.main.mix_into(frames);
        self.sfx.mix_into(frames);
        self.mpeg.mix_into(frames);
    }

    // Queues decoded DVC PCM, resampled from `rate` to OUT_RATE; it plays from the next output frame, a start delay belongs to the decoder.
    pub fn push_mpeg(&mut self, pcm: &mut VecDeque<Frame>, rate: u32) {
        if pcm.is_empty() {
            return;
        }
        self.resampler.set_rate(rate);
        self.scratch.clear();
        let (a, b) = pcm.as_slices();
        self.resampler.process(a, &mut self.scratch);
        self.resampler.process(b, &mut self.scratch);
        pcm.clear();
        self.mpeg.push(&self.scratch, 0, MPEG_WARN, &|| format!("mpeg {} Hz", rate));
    }

    // FMA stop or reset: the unplayed DVC audio and the resampler history go.
    pub fn stop_mpeg(&mut self) {
        self.mpeg.clear();
        self.resampler = Resampler::new();
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
        attenuation: [u8; 4],
        left_state: &mut ChannelState,
        right_state: &mut ChannelState,
    ) -> Vec<(f32, f32)> {
        let mut samples = Vec::with_capacity(18 * 224);
        let scale_ll = attenuation[0] as f32 / 255.0;
        let scale_lr = attenuation[1] as f32 / 255.0;
        let scale_rr = attenuation[2] as f32 / 255.0;
        let scale_rl = attenuation[3] as f32 / 255.0;

        for g in 0..18 {
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
                        let out_l = sl * scale_ll + sr * scale_rl;
                        let out_r = sl * scale_lr + sr * scale_rr;
                        samples.push((out_l, out_r));
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
                        let out_l = sm * (scale_ll + scale_rl);
                        let out_r = sm * (scale_rr + scale_lr);
                        samples.push((out_l, out_r));
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
                        let out_l = sl * scale_ll + sr * scale_rl;
                        let out_r = sl * scale_lr + sr * scale_rr;
                        samples.push((out_l, out_r));
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
                        let out_l = sm * (scale_ll + scale_rl);
                        let out_r = sm * (scale_rr + scale_lr);
                        samples.push((out_l, out_r));
                    }
                }
            }
        }
        samples
    }

    fn apply_matrix(&self, mut samples: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
        if let Some([ll, lr, rr, rl]) = self.matrix {
            for (l, r) in samples.iter_mut() {
                (*l, *r) = (*l * ll + *r * rl, *r * rr + *l * lr);
            }
        }
        samples
    }

    // 18.9 kHz goes through the upsampler; a 37.8 kHz sector passes as is and clears the upsampler's history.
    fn upsample_if_needed(up: &mut Upsampler, samples: Vec<(f32, f32)>, sample_rate: u32) -> Vec<(f32, f32)> {
        if sample_rate != 18900 {
            *up = Upsampler::default();
            return samples;
        }
        let mut out = Vec::with_capacity(samples.len() * 2);
        up.run(&samples, &mut out);
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
            self.attenuation,
            &mut self.left_state,
            &mut self.right_state,
        );
        let final_samples = Self::upsample_if_needed(&mut self.upsampler, samples, sample_rate);
        let final_samples = self.apply_matrix(final_samples);

        let k = SectorKind { frames: final_samples.len(), is_8bit, is_stereo, rate: sample_rate };
        self.main.push(&final_samples, ADPCM_LATENCY, k.bound(), &|| format!("main {}", k.level()));
    }

    pub fn decode_audio_map_sector(&mut self, payload: &[u8], is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() < 18 * 128 {
            return;
        }

        self.total_sectors_decoded += 1;

        let samples = Self::decode_xa_payload(
            payload,
            is_stereo,
            is_8bit,
            self.attenuation,
            &mut self.map_left_state,
            &mut self.map_right_state,
        );
        let final_samples = Self::upsample_if_needed(&mut self.map_upsampler, samples, sample_rate);
        let final_samples = self.apply_matrix(final_samples);

        let k = SectorKind { frames: final_samples.len(), is_8bit, is_stereo, rate: sample_rate };
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

opencdi::impl_state!(ChannelState { s0, s1 } skip {});
opencdi::impl_state!(OutputClock { rem } skip {});
opencdi::impl_state!(Resampler { rate, num, den, pos, prev } skip {});

opencdi::impl_state!(Stream { fifo, primed } skip { warned, restarts, warnings, frames_in });

impl opencdi::savestate::State for CdiAudio {
    fn save(&self, w: &mut Vec<u8>) {
        let Self { left_state, right_state, map_left_state, map_right_state, upsampler, map_upsampler, attenuation, matrix: _, total_sectors_decoded, main, sfx, mpeg, resampler, clock, out, scratch: _ } = self;
        for s in [left_state, right_state, map_left_state, map_right_state] {
            s.save(w);
        }
        attenuation.save(w);
        total_sectors_decoded.save(w);
        upsampler.hist.save(w);
        map_upsampler.hist.save(w);
        for s in [main, sfx, mpeg] {
            s.save(w);
        }
        resampler.save(w);
        clock.save(w);
        out.save(w);
    }

    fn load(&mut self, r: &mut opencdi::savestate::Reader) -> Result<(), opencdi::savestate::StateError> {
        let Self { left_state, right_state, map_left_state, map_right_state, upsampler, map_upsampler, attenuation, matrix: _, total_sectors_decoded, main, sfx, mpeg, resampler, clock, out, scratch: _ } = self;
        for s in [left_state, right_state, map_left_state, map_right_state] {
            s.load(r)?;
        }
        attenuation.load(r)?;
        total_sectors_decoded.load(r)?;
        upsampler.hist.load(r)?;
        map_upsampler.hist.load(r)?;
        for s in [main, sfx, mpeg] {
            s.load(r)?;
        }
        resampler.load(r)?;
        clock.load(r)?;
        out.load(r)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_matrix_mixes_and_mutes_the_stereo_pair() {
        let mut a = CdiAudio::new();
        let input = vec![(0.5f32, 0.25f32)];
        assert_eq!(a.apply_matrix(input.clone()), input);
        a.matrix = Some([1.0, 1.0, 1.0, 1.0]);
        assert_eq!(a.apply_matrix(input.clone()), vec![(0.75, 0.75)]);
        a.matrix = Some([0.0, 0.0, 1.0, 0.0]);
        assert_eq!(a.apply_matrix(input), vec![(0.0, 0.25)]);
    }

    #[test]
    fn eight_bit_groups_use_the_last_parameter_copy() {
        let mut payload = vec![0u8; 18 * 128];
        for g in payload.chunks_exact_mut(128) {
            g[0..12].fill(0x08); // range 8 in the first three copies
            g[12..16].fill(0x00); // range 0 in the last copy
            g[16..].fill(0x01);
        }
        let mut a = CdiAudio::new();
        a.decode_sector(&payload, false, true, 37800);
        let mut out = vec![(0.0, 0.0); ADPCM_LATENCY + 1];
        a.main.mix_into(&mut out);
        let first = out[ADPCM_LATENCY].0;
        assert_eq!(first, 256.0 / 32768.0);
    }

    #[test]
    fn upsampler_passes_inputs_and_interpolates_between_them() {
        let mut up = Upsampler::default();
        let mut out = Vec::new();
        let x: Vec<(f32, f32)> = [1.0f32, 2.0, 3.0, 5.0].iter().map(|&v| (v, -v)).collect();
        up.run(&x[..2], &mut out);
        up.run(&x[2..], &mut out);
        // Two samples of delay: inputs 1 and 2 come out at 4 and 6, the cubic midpoint of 1..2 at 5.
        assert_eq!(out.len(), 8);
        assert_eq!((out[4], out[6]), ((1.0, -1.0), (2.0, -2.0)));
        assert_eq!(out[5], (1.5, -1.5));
        assert_eq!(up.hist, [x[1], x[2], x[3]]);
    }

    #[test]
    fn frame_rate_and_lengths_follow_the_sector_clock() {
        let mut c = OutputClock::default();
        let pal: usize = (0..1000).map(|_| c.advance(frame_cycles(VideoMode::Pal), VideoMode::Pal.cycles_per_sector())).sum();
        assert_eq!(pal, (504u64 * 299_520 * 1000 / 200_000) as usize);
        assert!((frame_rate(VideoMode::Pal) - 50.0801).abs() < 1e-4);
        let mut c = OutputClock::default();
        let lens: Vec<usize> = (0..4).map(|_| c.advance(frame_cycles(VideoMode::Ntsc), VideoMode::Ntsc.cycles_per_sector())).collect();
        assert_eq!(lens, [629, 629, 630, 629]);
        assert!((frame_rate(VideoMode::Ntsc) - 60.0543).abs() < 1e-4);
    }

    // Counts rising zero crossings of the left channel.
    fn crossings(s: &[Frame]) -> usize {
        s.windows(2).filter(|w| w[0].0 < 0.0 && w[1].0 >= 0.0).count()
    }

    #[test]
    fn resampler_keeps_pitch_for_mp2_rates() {
        for rate in [32_000u32, 44_100, 48_000, 37_800] {
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
            for chunk in input.chunks(1152) {
                r.process(chunk, &mut out);
            }
            let want = OUT_RATE as usize * 2;
            assert!(out.len().abs_diff(want) <= 1, "{} Hz: {} frames, want {}", rate, out.len(), want);
            assert!(crossings(&out[..OUT_RATE as usize]).abs_diff(1000) <= 1, "{} Hz: {} crossings", rate, crossings(&out[..OUT_RATE as usize]));
        }
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
    fn mpeg_plays_without_start_latency() {
        let mut a = CdiAudio::new();
        a.push_mpeg(&mut VecDeque::from(vec![(0.25, 0.25); 1152]), OUT_RATE);
        a.advance(960, VideoMode::Pal.cycles_per_sector());
        let mut out = Vec::new();
        a.drain(&mut out);
        // The linear resampler delays by one input frame; no silence is added in front.
        assert_eq!((out[0], out[1]), ((0.0, 0.0), (0.25, 0.25)));
    }

    #[test]
    fn output_state_round_trips() {
        use opencdi::savestate::{Reader, State};
        let sector = VideoMode::Ntsc.cycles_per_sector();
        let run = |a: &mut CdiAudio, v: f32| {
            a.main.push(&[(v, -v); 700], ADPCM_LATENCY, usize::MAX, &String::new);
            a.push_mpeg(&mut VecDeque::from(vec![(0.2, 0.3); 1152]), 44_100);
            for _ in 0..262 {
                a.advance(960, sector);
            }
        };
        let mut a = CdiAudio::new();
        run(&mut a, 0.1);
        let mut buf = Vec::new();
        a.save(&mut buf);
        let mut b = CdiAudio::new();
        b.load(&mut Reader::new(&buf)).unwrap();
        for _ in 0..8 {
            run(&mut a, 0.4);
            run(&mut b, 0.4);
            let (mut x, mut y) = (Vec::new(), Vec::new());
            assert_eq!(a.drain(&mut x), b.drain(&mut y));
            assert_eq!(x, y);
        }
    }
}
