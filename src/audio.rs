use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use log::{info, warn, error};
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

const HEADER_OFFSET_8BIT: [usize; 4] = [4, 5, 6, 7];
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

pub struct CdiAudio {
    pub left_state: ChannelState,
    pub right_state: ChannelState,
    pub map_left_state: ChannelState,
    pub map_right_state: ChannelState,
    pub sample_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
    pub sfx_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
    pub dvc_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
    pub source_rate: Arc<AtomicU32>,
    pub dvc_rate: Arc<AtomicU32>,
    pub attenuation: [u8; 4],
    pub volume: f32,
    pub muted: bool,
    pub total_sectors_decoded: u64,
}

impl CdiAudio {
    pub fn new() -> Self {
        Self {
            left_state: ChannelState::default(),
            right_state: ChannelState::default(),
            map_left_state: ChannelState::default(),
            map_right_state: ChannelState::default(),
            sample_queue: Arc::new(Mutex::new(VecDeque::with_capacity(65536))),
            sfx_queue: Arc::new(Mutex::new(VecDeque::with_capacity(65536))),
            dvc_queue: Arc::new(Mutex::new(VecDeque::with_capacity(65536))),
            source_rate: Arc::new(AtomicU32::new(37800)),
            dvc_rate: Arc::new(AtomicU32::new(44100)),
            attenuation: [0xFF, 0, 0xFF, 0],
            volume: 1.0,
            muted: false,
            total_sectors_decoded: 0,
        }
    }

    pub fn reset(&mut self) {
        self.left_state.reset();
        self.right_state.reset();
        self.map_left_state.reset();
        self.map_right_state.reset();
        self.attenuation = [0xFF, 0, 0xFF, 0];
        if let Ok(mut q) = self.sample_queue.lock() {
            q.clear();
        }
        if let Ok(mut sq) = self.sfx_queue.lock() {
            sq.clear();
        }
        if let Ok(mut dq) = self.dvc_queue.lock() {
            dq.clear();
        }
    }

    pub fn set_attenuation(&mut self, atten: [u8; 4]) {
        self.attenuation = atten;
    }

    fn decode_xa_payload(
        payload: &[u8],
        is_stereo: bool,
        is_8bit: bool,
        vol: f32,
        attenuation: [u8; 4],
        left_state: &mut ChannelState,
        right_state: &mut ChannelState,
    ) -> Vec<(f32, f32)> {
        let mut samples = Vec::with_capacity(18 * 224);
        let scale_ll = (attenuation[0] as f32 / 255.0) * vol;
        let scale_lr = (attenuation[1] as f32 / 255.0) * vol;
        let scale_rr = (attenuation[2] as f32 / 255.0) * vol;
        let scale_rl = (attenuation[3] as f32 / 255.0) * vol;

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

    fn upsample_if_needed(samples: Vec<(f32, f32)>, sample_rate: u32) -> Vec<(f32, f32)> {
        if sample_rate == 18900 {
            let mut upsampled = Vec::with_capacity(samples.len() * 2);
            for i in 0..samples.len() {
                let s0 = samples[i];
                let s1 = if i + 1 < samples.len() { samples[i + 1] } else { s0 };
                upsampled.push(s0);
                upsampled.push(((s0.0 + s1.0) * 0.5, (s0.1 + s1.1) * 0.5));
            }
            upsampled
        } else {
            samples
        }
    }

    pub fn decode_sector(&mut self, payload: &[u8], is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() < 18 * 128 {
            return;
        }

        self.source_rate.store(37800, Ordering::Relaxed);
        self.total_sectors_decoded += 1;

        let vol = if self.muted { 0.0 } else { self.volume };
        let samples = Self::decode_xa_payload(
            payload,
            is_stereo,
            is_8bit,
            vol,
            self.attenuation,
            &mut self.left_state,
            &mut self.right_state,
        );
        let final_samples = Self::upsample_if_needed(samples, sample_rate);

        if let Ok(mut q) = self.sample_queue.lock() {
            // Cap queue at ~1.7s of audio (65536 samples) to prevent runaway latency while avoiding clicks
            const MAX_QUEUE_SAMPLES: usize = 65536;
            let current_len = q.len();
            if current_len + final_samples.len() > MAX_QUEUE_SAMPLES {
                let overflow = (current_len + final_samples.len()) - MAX_QUEUE_SAMPLES;
                let to_drain = overflow.min(current_len);
                q.drain(0..to_drain);
            }
            q.extend(final_samples);
        }
    }

    pub fn decode_audio_map_sector(&mut self, payload: &[u8], is_stereo: bool, is_8bit: bool, sample_rate: u32) {
        if payload.len() < 18 * 128 {
            return;
        }

        self.total_sectors_decoded += 1;

        let vol = if self.muted { 0.0 } else { self.volume };
        let samples = Self::decode_xa_payload(
            payload,
            is_stereo,
            is_8bit,
            vol,
            self.attenuation,
            &mut self.map_left_state,
            &mut self.map_right_state,
        );
        let final_samples = Self::upsample_if_needed(samples, sample_rate);

        if let Ok(mut sq) = self.sfx_queue.lock() {
            const MAX_SFX_SAMPLES: usize = 65536;
            let current_len = sq.len();
            if current_len + final_samples.len() > MAX_SFX_SAMPLES {
                let overflow = (current_len + final_samples.len()) - MAX_SFX_SAMPLES;
                let to_drain = overflow.min(current_len);
                sq.drain(0..to_drain);
            }
            sq.extend(final_samples);
        }
    }
}

#[cfg(feature = "host")]
pub struct HostAudioOutput {
    _stream: cpal::Stream,
}

#[cfg(feature = "host")]
impl HostAudioOutput {
    pub fn try_start(
        sample_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
        sfx_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
        dvc_queue: Arc<Mutex<VecDeque<(f32, f32)>>>,
        source_rate: Arc<AtomicU32>,
        dvc_rate: Arc<AtomicU32>,
    ) -> Option<Self> {
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

        let mut resample_phase: f64 = 0.0;
        let mut curr_sample = (0.0f32, 0.0f32);
        let mut next_sample = (0.0f32, 0.0f32);
        let mut prebuffering = true;
        let mut starved_count: usize = 0;

        let mut dvc_phase: f64 = 0.0;
        let mut dvc_curr = (0.0f32, 0.0f32);
        let mut dvc_next = (0.0f32, 0.0f32);
        let mut dvc_prebuffering = true;
        let mut dvc_starved: usize = 0;

        let mut sfx_phase: f64 = 0.0;
        let mut sfx_curr = (0.0f32, 0.0f32);
        let mut sfx_next = (0.0f32, 0.0f32);
        let mut sfx_prebuffering = true;
        let mut sfx_starved: usize = 0;

        let err_fn = |err| error!("Audio output stream error: {:?}", err);

        let stream_res = device.build_output_stream(
            config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let src_rate = source_rate.load(Ordering::Relaxed) as f64;
                let mpeg_rate = dvc_rate.load(Ordering::Relaxed) as f64;

                let mut q_guard = match sample_queue.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        output.fill(0.0);
                        return;
                    }
                };
                let mut sfx_guard = sfx_queue.lock().ok();
                let mut dvc_guard = dvc_queue.lock().ok();

                let q_len = q_guard.len();
                let sq_len = sfx_guard.as_ref().map_or(0, |sq| sq.len());
                let dvc_len = dvc_guard.as_ref().map_or(0, |dq| dq.len());

                // Prebuffering management for CDIC XA stream (~4096 samples / ~108 ms at 37.8 kHz)
                // Ensures at least 1-2 sectors of Level B/C Mono are queued before playback starts
                const PREBUFFER_SAMPLES: usize = 1536;
                if prebuffering && q_len >= PREBUFFER_SAMPLES {
                    prebuffering = false;
                    curr_sample = q_guard.pop_front().unwrap_or((0.0, 0.0));
                    next_sample = q_guard.pop_front().unwrap_or(curr_sample);
                    resample_phase = 0.0;
                    starved_count = 0;
                }

                // Prebuffering management for DVC MP2 stream (~2304 samples = 2 MP2 frames / ~52 ms at 44.1 kHz)
                const DVC_PREBUFFER_SAMPLES: usize = 2304;
                if dvc_prebuffering && dvc_len >= DVC_PREBUFFER_SAMPLES {
                    if let Some(ref mut dq) = dvc_guard {
                        dvc_prebuffering = false;
                        dvc_curr = dq.pop_front().unwrap_or((0.0, 0.0));
                        dvc_next = dq.pop_front().unwrap_or(dvc_curr);
                        dvc_phase = 0.0;
                        dvc_starved = 0;
                    }
                }

                // Prebuffering management for SFX / Audio Map stream (~1536 samples / ~40 ms at 37.8 kHz)
                const SFX_PREBUFFER_SAMPLES: usize = 1536;
                if sfx_prebuffering && sq_len >= SFX_PREBUFFER_SAMPLES {
                    if let Some(ref mut sq) = sfx_guard {
                        sfx_prebuffering = false;
                        sfx_curr = sq.pop_front().unwrap_or((0.0, 0.0));
                        sfx_next = sq.pop_front().unwrap_or(sfx_curr);
                        sfx_phase = 0.0;
                        sfx_starved = 0;
                    }
                }

                if prebuffering && dvc_prebuffering && sfx_prebuffering {
                    output.fill(0.0);
                    return;
                }

                // Smooth Proportional Dynamic Rate Control (DRC) for CDIC stream:
                // Target ~8192 samples (~216ms at 37.8kHz) gives ample room for 4032-sample
                // Level B Mono sector bursts without queue underruns or stuttering.
                const TARGET_QUEUE: f64 = 8192.0;
                let diff = (q_len as f64 - TARGET_QUEUE) / TARGET_QUEUE;
                let drc = (1.0 + diff * 0.008).clamp(0.990, 1.010);
                let step = (src_rate / host_sample_rate) * drc;

                // Smooth Proportional DRC for DVC MP2 stream:
                const TARGET_DVC_QUEUE: f64 = 4000.0;
                let dvc_diff = (dvc_len as f64 - TARGET_DVC_QUEUE) / TARGET_DVC_QUEUE;
                let dvc_drc = (1.0 + dvc_diff * 0.004).clamp(0.996, 1.004);
                let dvc_step = (mpeg_rate / host_sample_rate) * dvc_drc;

                // Smooth Proportional DRC for SFX / Audio Map stream:

                const TARGET_SFX_QUEUE: f64 = 4096.0;

                let sfx_diff = (sq_len as f64 - TARGET_SFX_QUEUE) / TARGET_SFX_QUEUE;

                let sfx_drc = (1.0 + sfx_diff * 0.008).clamp(0.990, 1.010);

                let sfx_step = (37800.0 / host_sample_rate) * sfx_drc;

                let mut out_idx = 0;
                while out_idx < output.len() {
                    let (mut l, mut r) = (0.0f32, 0.0f32);

                    if !prebuffering {
                        while resample_phase >= 1.0 {
                            resample_phase -= 1.0;
                            curr_sample = next_sample;
                            if let Some(s) = q_guard.pop_front() {
                                next_sample = s;
                                starved_count = 0;
                            } else {
                                starved_count += 1;
                                next_sample = (curr_sample.0 * 0.95, curr_sample.1 * 0.95);
                                if starved_count >= 512 {
                                    prebuffering = true;
                                    break;
                                }
                            }
                        }
                        if !prebuffering {
                            let t = resample_phase as f32;
                            l += curr_sample.0 * (1.0 - t) + next_sample.0 * t;
                            r += curr_sample.1 * (1.0 - t) + next_sample.1 * t;
                            resample_phase += step;
                        }
                    }

                    if !dvc_prebuffering {
                        while dvc_phase >= 1.0 {
                            dvc_phase -= 1.0;
                            dvc_curr = dvc_next;
                            let popped = dvc_guard.as_mut().and_then(|dq| dq.pop_front());
                            if let Some(s) = popped {
                                dvc_next = s;
                                dvc_starved = 0;
                            } else {
                                dvc_starved += 1;
                                dvc_next = (dvc_curr.0 * 0.95, dvc_curr.1 * 0.95);
                                if dvc_starved >= 512 {
                                    dvc_prebuffering = true;
                                    break;
                                }
                            }
                        }
                        if !dvc_prebuffering {
                            let dt = dvc_phase as f32;
                            l += dvc_curr.0 * (1.0 - dt) + dvc_next.0 * dt;
                            r += dvc_curr.1 * (1.0 - dt) + dvc_next.1 * dt;
                            dvc_phase += dvc_step;
                        }
                    }

                    if !sfx_prebuffering {
                        while sfx_phase >= 1.0 {
                            sfx_phase -= 1.0;
                            sfx_curr = sfx_next;
                            let popped = sfx_guard.as_mut().and_then(|sq| sq.pop_front());
                            if let Some(s) = popped {
                                sfx_next = s;
                                sfx_starved = 0;
                            } else {
                                sfx_starved += 1;
                                sfx_next = (sfx_curr.0 * 0.95, sfx_curr.1 * 0.95);
                                if sfx_starved >= 512 {
                                    sfx_prebuffering = true;
                                    break;
                                }
                            }
                        }
                        if !sfx_prebuffering {
                            let st = sfx_phase as f32;
                            let sl = sfx_curr.0 * (1.0 - st) + sfx_next.0 * st;
                            let sr = sfx_curr.1 * (1.0 - st) + sfx_next.1 * st;
                            l += sl;
                            r += sr;
                            sfx_phase += sfx_step;
                        }
                    }

                    // Smooth soft-saturation: prevents harsh clipping distortion when mixing
                    // disc audio stream and audio map sound effects
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

// Host queues are not state (the frontend drains them every frame); volume and mute are host settings.
impl opencdi::savestate::State for CdiAudio {
    fn save(&self, w: &mut Vec<u8>) {
        let Self { left_state, right_state, map_left_state, map_right_state, sample_queue: _, sfx_queue: _, dvc_queue: _, source_rate, dvc_rate, attenuation, volume: _, muted: _, total_sectors_decoded } = self;
        for s in [left_state, right_state, map_left_state, map_right_state] {
            s.save(w);
        }
        source_rate.load(Ordering::Relaxed).save(w);
        dvc_rate.load(Ordering::Relaxed).save(w);
        attenuation.save(w);
        total_sectors_decoded.save(w);
    }

    fn load(&mut self, r: &mut opencdi::savestate::Reader) -> Result<(), opencdi::savestate::StateError> {
        let Self { left_state, right_state, map_left_state, map_right_state, sample_queue, sfx_queue, dvc_queue, source_rate, dvc_rate, attenuation, volume: _, muted: _, total_sectors_decoded } = self;
        for s in [left_state, right_state, map_left_state, map_right_state] {
            s.load(r)?;
        }
        let mut rate = 0u32;
        rate.load(r)?;
        source_rate.store(rate, Ordering::Relaxed);
        rate.load(r)?;
        dvc_rate.store(rate, Ordering::Relaxed);
        attenuation.load(r)?;
        total_sectors_decoded.load(r)?;
        for q in [sample_queue, sfx_queue, dvc_queue] {
            if let Ok(mut q) = q.lock() {
                q.clear();
            }
        }
        Ok(())
    }
}
