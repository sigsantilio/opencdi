// SPDX-License-Identifier: MIT
//
// Motion-adaptive presentation de-interlacer for interlaced MCD-212 output.
// Algorithm adapted from E-Di (`crates/cdi-frontend/src/presentation.rs`, MIT License,
// https://github.com/whatever-industries/edi_emulator).
//
// Presentation-only: the core MCD-212 framebuffer, savestates, and diagnostic captures
// always keep the exact raw hardware field weave.

use crate::mcd212::{SCREEN_HEIGHT, SCREEN_WIDTH};

// Per-channel thresholds distinguishing genuine inter-field motion from small
// decoder/rounding noise; hysteresis + short hold prevents fades/edges from chattering.
const MOTION_START_THRESHOLD: u8 = 8;
const MOTION_CONTINUE_THRESHOLD: u8 = 3;
const MOTION_HOLD_FIELDS: u8 = 4;

const FRAME_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT;

#[inline]
fn component_delta(a: u32, b: u32, shift: u32) -> u8 {
    (((a >> shift) & 0xFF) as u8).abs_diff(((b >> shift) & 0xFF) as u8)
}

#[inline]
fn pixel_changed(a: u32, b: u32, threshold: u8) -> bool {
    component_delta(a, b, 16) >= threshold
        || component_delta(a, b, 8) >= threshold
        || component_delta(a, b, 0) >= threshold
}

#[inline]
fn average_pixel(a: u32, b: u32) -> u32 {
    let rb = ((a & 0x00FF_00FF) + (b & 0x00FF_00FF) + 0x0001_0001) >> 1;
    let g = ((a & 0x0000_FF00) + (b & 0x0000_FF00) + 0x0000_0100) >> 1;
    0xFF00_0000 | (rb & 0x00FF_00FF) | (g & 0x0000_FF00)
}

#[inline]
fn bob_pixel(current: &[u32], width: usize, height: usize, x: usize, y: usize) -> u32 {
    let upper = if y > 0 { y - 1 } else { (y + 1).min(height - 1) };
    let lower = if y + 1 < height { y + 1 } else { upper };
    average_pixel(current[upper * width + x], current[lower * width + x])
}

#[inline]
fn nearby_field_motion(motion: &[u8], width: usize, height: usize, x: usize, y: usize) -> bool {
    let first_row = y.saturating_sub(1);
    let last_row = (y + 1).min(height - 1);
    let first_col = x.saturating_sub(1);
    let last_col = (x + 1).min(width - 1);
    (first_row..=last_row).any(|row| {
        motion[row * width + first_col..=row * width + last_col]
            .iter()
            .any(|&v| v != 0)
    })
}

/// Stateful presentation-only motion-adaptive de-interlacer.
///
/// Static areas retain both interlaced fields (full vertical detail). Moving areas
/// hold a fixed anchor field phase and reconstruct the complementary rows via vertical
/// averaging (bob), avoiding 1-scanline vertical jitter.
pub struct Deinterlacer {
    previous: Vec<u32>,
    has_previous: bool,
    motion: Vec<u8>,
    output: Vec<u32>,
    last_y_offset: Option<usize>,
}

impl Default for Deinterlacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deinterlacer {
    pub const fn new() -> Self {
        Self {
            previous: Vec::new(),
            has_previous: false,
            motion: Vec::new(),
            output: Vec::new(),
            last_y_offset: None,
        }
    }

    pub fn reset(&mut self) {
        self.has_previous = false;
        self.last_y_offset = None;
        self.motion.fill(0);
    }

    /// Processes a `SCREEN_WIDTH x SCREEN_HEIGHT` XRGB8888 framebuffer for presentation.
    ///
    /// Returns `current` directly with zero copy when `enabled` is false or the display
    /// is progressive (`!interlaced`).
    pub fn process<'a>(
        &'a mut self,
        current: &'a [u32],
        interlaced: bool,
        rendered_odd_field: bool,
        y_offset: usize,
        enabled: bool,
    ) -> &'a [u32] {
        if !enabled || !interlaced || current.len() < FRAME_LEN {
            self.has_previous = false;
            self.last_y_offset = None;
            return current;
        }

        if self.output.len() < FRAME_LEN {
            self.output.resize(FRAME_LEN, 0xFF00_0000);
        }
        if self.motion.len() < FRAME_LEN {
            self.motion.resize(FRAME_LEN, 0);
        }
        if self.previous.len() < FRAME_LEN {
            self.previous.resize(FRAME_LEN, 0xFF00_0000);
        }

        if self.last_y_offset != Some(y_offset) {
            self.has_previous = false;
            self.last_y_offset = Some(y_offset);
        }

        self.process_dims(
            current,
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
            rendered_odd_field,
            y_offset,
        );
        &self.output[..FRAME_LEN]
    }

    fn process_dims(
        &mut self,
        current: &[u32],
        width: usize,
        height: usize,
        rendered_odd_field: bool,
        y_offset: usize,
    ) {
        let len = width * height;
        self.output[..len].copy_from_slice(&current[..len]);

        // Compare only the freshly rendered field rows with the preceding frame's
        // same-parity rows (the weave framebuffer still holds the previous field on
        // the complementary rows, so previous[same_parity] is 2 fields earlier).
        let current_row_parity = (y_offset + usize::from(rendered_odd_field)) & 1;
        if self.has_previous {
            for v in &mut self.motion[..len] {
                *v = v.saturating_sub(1);
            }
            for y in (current_row_parity..height).step_by(2) {
                let row = y * width;
                for x in 0..width {
                    let idx = row + x;
                    let threshold = if self.motion[idx] == 0 {
                        MOTION_START_THRESHOLD
                    } else {
                        MOTION_CONTINUE_THRESHOLD
                    };
                    if pixel_changed(current[idx], self.previous[idx], threshold) {
                        self.motion[idx] = MOTION_HOLD_FIELDS;
                    }
                }
            }
        } else {
            self.motion[..len].fill(MOTION_HOLD_FIELDS);
        }

        // Anchor reconstruction to a fixed phase derived from the first active row so
        // moving video does not jitter vertically by 1 scanline on alternating fields.
        let source_row_parity = y_offset & 1;
        for y in ((source_row_parity ^ 1)..height).step_by(2) {
            let row = y * width;
            for x in 0..width {
                if !self.has_previous || nearby_field_motion(&self.motion, width, height, x, y) {
                    self.output[row + x] = bob_pixel(current, width, height, x, y);
                }
            }
        }

        self.previous[..len].copy_from_slice(&current[..len]);
        self.has_previous = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_deinterlacer_for_test(w: usize, h: usize) -> Deinterlacer {
        let len = w * h;
        Deinterlacer {
            previous: vec![0xFF00_0000; len],
            has_previous: false,
            motion: vec![0; len],
            output: vec![0xFF00_0000; len],
            last_y_offset: None,
        }
    }

    #[test]
    fn disabled_or_progressive_passes_through_unchanged() {
        let mut d = Deinterlacer::new();
        let fb = vec![0xFF12_3456u32; FRAME_LEN];
        let out_off = d.process(&fb, true, false, 0, false);
        assert!(std::ptr::eq(out_off.as_ptr(), fb.as_ptr()));
        let out_prog = d.process(&fb, false, false, 0, true);
        assert!(std::ptr::eq(out_prog.as_ptr(), fb.as_ptr()));
    }

    #[test]
    fn static_interlaced_image_preserves_full_vertical_detail() {
        let (w, h) = (8, 8);
        let mut d = make_deinterlacer_for_test(w, h);
        // Alternating horizontal stripes (even rows = bright, odd rows = dark)
        let mut frame = vec![0xFF00_0000u32; w * h];
        for y in 0..h {
            let c = if y % 2 == 0 { 0xFFCC_CC00 } else { 0xFF10_1080 };
            for x in 0..w {
                frame[y * w + x] = c;
            }
        }
        // Run enough static fields for initial MOTION_HOLD_FIELDS to decay to 0
        for step in 0..=MOTION_HOLD_FIELDS + 1 {
            d.process_dims(&frame, w, h, step % 2 == 1, 0);
        }
        // Static area must preserve exact original weave rows without bobbing
        assert_eq!(&d.output[..w * h], &frame[..]);
    }

    #[test]
    fn moving_interlaced_region_reconstructs_only_moving_rows() {
        let (w, h) = (12, 8);
        let mut d = make_deinterlacer_for_test(w, h);

        let mut base = vec![0xFF00_0000u32; w * h];
        for y in 0..h {
            let c = if y % 2 == 0 { 0xFF20_2020 } else { 0xFF80_8080 };
            for x in 0..w {
                base[y * w + x] = c;
            }
        }
        // Settle static motion hold
        for step in 0..=MOTION_HOLD_FIELDS + 1 {
            d.process_dims(&base, w, h, step % 2 == 1, 0);
        }

        // Introduce motion on right half (x >= 8) in the even field (rendered_odd_field = false)
        let mut moving = base.clone();
        for y in (0..h).step_by(2) {
            for x in 8..w {
                moving[y * w + x] = 0xFFE0_E0E0;
            }
        }
        d.process_dims(&moving, w, h, false, 0);

        // Left static side (x = 2, odd row y = 3) keeps its exact weave pixel (0xFF808080)
        assert_eq!(d.output[3 * w + 2], 0xFF80_8080);
        // Right moving side (x = 10, odd row y = 3) is bobbed from adjacent even rows (0xFFE0E0E0)
        assert_eq!(d.output[3 * w + 10], 0xFFE0_E0E0);
    }
}
