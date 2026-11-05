use opencdi::mcd212::VideoMode;
use std::collections::VecDeque;

pub use opencdi::audio::{Frame, OUT_RATE, frame_rate};
use opencdi::audio::{OutputClock, Resampler, frame_cycles};

// Silence put in front of a stream that starts from empty, so sector-sized bursts never run dry (about 54 ms).
pub const PREFILL: usize = 2048;
// Most frames one channel may hold (about 0.43 s); the oldest are dropped beyond it.
pub const FIFO_CAP: usize = 16384;

// One audio stream at OUT_RATE between the emulation and the frontend.
#[derive(Default)]
pub struct Channel {
    fifo: VecDeque<Frame>,
    primed: bool,
    pub restarts: u64,
    pub dropped: u64,
    pub frames_in: u64,
}

impl Channel {
    const fn new() -> Self {
        Channel { fifo: VecDeque::new(), primed: false, restarts: 0, dropped: 0, frames_in: 0 }
    }

    pub fn push(&mut self, src: &[Frame]) {
        if src.is_empty() {
            return;
        }
        self.frames_in += src.len() as u64;
        if !self.primed {
            self.fifo.resize(self.fifo.len() + PREFILL, (0.0, 0.0));
            self.primed = true;
            self.restarts += 1;
        }
        self.fifo.extend(src.iter().copied());
        if self.fifo.len() > FIFO_CAP {
            let n = self.fifo.len() - FIFO_CAP;
            self.fifo.drain(..n);
            self.dropped += n as u64;
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

    #[cfg(test)]
    fn len(&self) -> usize {
        self.fifo.len()
    }
}

opencdi::impl_state!(Channel { fifo, primed } skip { restarts, dropped, frames_in });

// Hands the frontend exactly one field of audio per retro_run: CDIC main and audio-map streams plus DVC MPEG audio resampled.
pub struct Pacer {
    pub main: Channel,
    pub sfx: Channel,
    pub mpeg: Channel,
    resampler: Resampler,
    clock: OutputClock,
    scratch: Vec<Frame>,
    out: Vec<Frame>,
    pub frames_out: u64,
}

impl Pacer {
    pub const fn new() -> Self {
        Pacer {
            main: Channel::new(),
            sfx: Channel::new(),
            mpeg: Channel::new(),
            resampler: Resampler::new(),
            clock: OutputClock { rem: 0 },
            scratch: Vec::new(),
            out: Vec::new(),
            frames_out: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Pacer::new();
    }

    // Frames due this field: OUT_RATE / 75 per sector times the sectors the field lasts, remainder carried.
    fn field_len(&mut self, mode: VideoMode) -> usize {
        self.clock.advance(frame_cycles(mode), mode.cycles_per_sector())
    }

    pub fn push_mpeg(&mut self, pcm: &[Frame], rate: u32) {
        if pcm.is_empty() {
            return;
        }
        self.resampler.set_rate(rate);
        self.scratch.clear();
        self.resampler.process(pcm, &mut self.scratch);
        self.mpeg.push(&self.scratch);
    }

    pub fn field(&mut self, mode: VideoMode) -> &[Frame] {
        let n = self.field_len(mode);
        self.out.clear();
        self.out.resize(n, (0.0, 0.0));
        self.main.mix_into(&mut self.out);
        self.sfx.mix_into(&mut self.out);
        self.mpeg.mix_into(&mut self.out);
        self.frames_out += n as u64;
        &self.out
    }
}

opencdi::impl_state!(Pacer { main, sfx, mpeg, resampler, clock } skip { scratch, out, frames_out });

#[cfg(test)]
mod tests {
    use super::*;
    use opencdi::savestate::{Reader, State};

    #[test]
    fn channel_prefills_pads_and_caps() {
        let mut c = Channel::default();
        c.push(&[(0.5, 0.5); 100]);
        assert_eq!(c.len(), PREFILL + 100);
        let mut out = vec![(0.0, 0.0); PREFILL + 150];
        c.mix_into(&mut out);
        assert_eq!(out[PREFILL - 1], (0.0, 0.0));
        assert_eq!(out[PREFILL], (0.5, 0.5));
        assert_eq!(out[PREFILL + 100], (0.0, 0.0));
        c.push(&[(0.25, 0.25); FIFO_CAP]);
        assert_eq!((c.len(), c.restarts, c.dropped), (FIFO_CAP, 2, PREFILL as u64));
    }

    #[test]
    fn pacer_state_round_trips() {
        let mut a = Pacer::new();
        a.main.push(&[(0.1, -0.1); 3000]);
        a.push_mpeg(&[(0.2, 0.3); 1152], 44_100);
        a.field(VideoMode::Ntsc);
        let mut buf = Vec::new();
        a.save(&mut buf);
        let mut b = Pacer::new();
        b.load(&mut Reader::new(&buf)).unwrap();
        for _ in 0..8 {
            a.push_mpeg(&[(0.4, 0.1); 1152], 44_100);
            b.push_mpeg(&[(0.4, 0.1); 1152], 44_100);
            assert_eq!(a.field(VideoMode::Ntsc), b.field(VideoMode::Ntsc));
        }
    }
}
