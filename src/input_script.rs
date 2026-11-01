// Shared pointer input script for the harness: comma or newline separated `frame:x:y:buttons` and `frame:BUTTON:len` items.
use std::fmt;

pub const MAX_X: i32 = 767;
pub const MAX_Y: i32 = 559;
pub const START_X: i32 = 384;
pub const START_Y: i32 = 280;
// Pointer speed of the d-pad, pixels per frame per axis.
pub const PAD_SPEED: i32 = 3;

// Button mask as passed to `Slave::update_mouse`: button 1, button 2, third button (presses both on Mono-I).
pub const BTN_1: u8 = 1;
pub const BTN_2: u8 = 2;
pub const BTN_3: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadButton {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Y,
}

impl PadButton {
    const ALL: [(PadButton, &'static str); 7] = [
        (PadButton::Up, "UP"),
        (PadButton::Down, "DOWN"),
        (PadButton::Left, "LEFT"),
        (PadButton::Right, "RIGHT"),
        (PadButton::A, "A"),
        (PadButton::B, "B"),
        (PadButton::Y, "Y"),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(b, _)| *b == self).map(|(_, n)| *n).unwrap()
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(b, _)| *b)
    }

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

// Pad buttons held in one frame, one bit per `PadButton`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad(pub u8);

impl Pad {
    pub fn held(self, b: PadButton) -> bool {
        self.0 & b.bit() != 0
    }

    pub fn with(self, b: PadButton) -> Self {
        Pad(self.0 | b.bit())
    }

    // B = button 1, Y = button 2, A = third button.
    pub fn buttons(self) -> u8 {
        let mut m = 0;
        if self.held(PadButton::B) { m |= BTN_1; }
        if self.held(PadButton::Y) { m |= BTN_2; }
        if self.held(PadButton::A) { m |= BTN_3; }
        m
    }

    // Moves (x, y) by PAD_SPEED per held direction, clamped to the slave's range.
    pub fn step(self, x: i32, y: i32) -> (i32, i32) {
        let dx = self.held(PadButton::Right) as i32 - self.held(PadButton::Left) as i32;
        let dy = self.held(PadButton::Down) as i32 - self.held(PadButton::Up) as i32;
        ((x + dx * PAD_SPEED).clamp(0, MAX_X), (y + dy * PAD_SPEED).clamp(0, MAX_Y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Pointer { frame: u64, x: i32, y: i32, buttons: u8 },
    Pad { frame: u64, button: PadButton, len: u64 },
}

impl Event {
    pub fn frame(&self) -> u64 {
        match *self {
            Event::Pointer { frame, .. } | Event::Pad { frame, .. } => frame,
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Event::Pointer { frame, x, y, buttons } => write!(f, "{}:{}:{}:{}", frame, x, y, buttons),
            Event::Pad { frame, button, len } => write!(f, "{}:{}:{}", frame, button.name(), len),
        }
    }
}

pub fn format(events: &[Event]) -> String {
    events.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(",")
}

fn num<T: std::str::FromStr>(s: &str, what: &str, item: &str) -> Result<T, String> {
    s.trim().parse().map_err(|_| format!("bad {} in '{}'", what, item))
}

// Items separated by commas or newlines; '#' starts a comment to end of line; events are stably sorted by frame.
pub fn parse(text: &str) -> Result<Vec<Event>, String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        for item in line.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let f: Vec<&str> = item.split(':').collect();
            let ev = match f.len() {
                4 => {
                    let buttons: u8 = num(f[3], "buttons", item)?;
                    if buttons > 7 {
                        return Err(format!("buttons must be 0..7 in '{}'", item));
                    }
                    Event::Pointer { frame: num(f[0], "frame", item)?, x: num(f[1], "x", item)?, y: num(f[2], "y", item)?, buttons }
                }
                3 => {
                    let button = PadButton::from_name(f[1].trim()).ok_or_else(|| format!("unknown pad button in '{}'", item))?;
                    Event::Pad { frame: num(f[0], "frame", item)?, button, len: num(f[2], "length", item)? }
                }
                _ => return Err(format!("expected frame:x:y:buttons or frame:BUTTON:len, got '{}'", item)),
            };
            out.push(ev);
        }
    }
    out.sort_by_key(Event::frame);
    Ok(out)
}

// Applies a script frame by frame; `step` says what to pass to `update_mouse` before that frame's `run_frame`.
#[derive(Clone)]
pub struct Player {
    events: Vec<Event>,
    next: usize,
    holds: Vec<(u64, PadButton, u64)>,
    x: i32,
    y: i32,
    pointer_buttons: u8,
    sent: (i32, i32, u8),
}

impl Player {
    pub fn new(events: Vec<Event>) -> Self {
        Player { events, next: 0, holds: Vec::new(), x: START_X, y: START_Y, pointer_buttons: 0, sent: (START_X, START_Y, 0) }
    }

    // Adopts a position the guest wrote to the device (slave `device_mouse_updated`).
    pub fn warp(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.sent.0 = x;
        self.sent.1 = y;
    }

    pub fn pad_at(&self, frame: u64) -> Pad {
        self.holds.iter().filter(|(s, _, l)| frame >= *s && frame - *s < *l).fold(Pad::default(), |p, (_, b, _)| p.with(*b))
    }

    // Returns the new (x, y, buttons) when it differs from what was last sent, else None.
    pub fn step(&mut self, frame: u64) -> Option<(i32, i32, u8)> {
        while let Some(ev) = self.events.get(self.next).copied() {
            if ev.frame() > frame {
                break;
            }
            self.next += 1;
            match ev {
                Event::Pointer { x, y, buttons, .. } => {
                    self.x = x;
                    self.y = y;
                    self.pointer_buttons = buttons;
                }
                Event::Pad { frame: f, button, len } => self.holds.push((f, button, len)),
            }
        }
        let pad = self.pad_at(frame);
        (self.x, self.y) = pad.step(self.x, self.y);
        let state = (self.x, self.y, self.pointer_buttons | pad.buttons());
        self.holds.retain(|(s, _, l)| s + l > frame + 1);
        if state == self.sent {
            return None;
        }
        self.sent = state;
        Some(state)
    }
}

// libretro pointer axis (-0x7fff..0x7fff over the whole screen) to a pixel 0..size-1.
pub fn norm_to_pixel(v: i16, size: i32) -> i32 {
    let v = (v as i64).clamp(-0x7fff, 0x7fff);
    (((v + 0x7fff) * size as i64) / 0xffff) as i32
}

// Exact inverse of `norm_to_pixel`: norm_to_pixel(pixel_to_norm(p, s), s) == p for 0 <= p < s.
pub fn pixel_to_norm(p: i32, size: i32) -> i16 {
    let p = p.clamp(0, size - 1) as i64;
    let s = size as i64;
    ((p * 0xffff + s - 1) / s - 0x7fff) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_forms_with_comments_and_newlines() {
        let ev = parse("10:575:335:1, 5:RIGHT:3 # comment\n\n2:384:280:0").unwrap();
        assert_eq!(ev, vec![
            Event::Pointer { frame: 2, x: 384, y: 280, buttons: 0 },
            Event::Pad { frame: 5, button: PadButton::Right, len: 3 },
            Event::Pointer { frame: 10, x: 575, y: 335, buttons: 1 },
        ]);
    }

    #[test]
    fn rejects_bad_items() {
        for bad in ["1:2", "a:1:2:0", "1:2:3:8", "1:JUMP:4", "1:2:3:4:5", "1:1:1:-1"] {
            assert!(parse(bad).is_err(), "{}", bad);
        }
        assert_eq!(parse("  ,\n# only a comment").unwrap(), vec![]);
    }

    #[test]
    fn format_round_trips() {
        let s = "3:575:335:0,33:575:335:1,40:A:20";
        assert_eq!(format(&parse(s).unwrap()), s);
    }

    #[test]
    fn same_frame_keeps_script_order() {
        let ev = parse("7:1:1:1,7:2:2:0").unwrap();
        let mut p = Player::new(ev);
        assert_eq!(p.step(7), Some((2, 2, 0)));
    }

    #[test]
    fn player_emits_only_changes() {
        let mut p = Player::new(parse("2:575:335:0,4:575:335:1,6:575:335:0,8:575:335:0").unwrap());
        let got: Vec<_> = (0..10).map(|f| p.step(f)).collect();
        assert_eq!(got, vec![
            None, None, Some((575, 335, 0)), None, Some((575, 335, 1)), None, Some((575, 335, 0)), None, None, None,
        ]);
    }

    #[test]
    fn start_position_is_not_sent() {
        let mut p = Player::new(parse("3:384:280:0").unwrap());
        assert!((0..5).all(|f| p.step(f).is_none()));
    }

    #[test]
    fn pad_moves_clamps_and_maps_buttons() {
        let mut p = Player::new(parse("0:RIGHT:2,1:DOWN:1,5:A:2,5:B:1,6:Y:1").unwrap());
        assert_eq!(p.step(0), Some((387, 280, 0)));
        assert_eq!(p.step(1), Some((390, 283, 0)));
        assert_eq!(p.step(2), None);
        assert_eq!(p.step(5), Some((390, 283, BTN_3 | BTN_1)));
        assert_eq!(p.step(6), Some((390, 283, BTN_3 | BTN_2)));
        assert_eq!(p.step(7), Some((390, 283, 0)));
        let mut q = Player::new(parse("0:LEFT:1000").unwrap());
        for f in 0..1000 {
            q.step(f);
        }
        assert_eq!(q.x, 0);
    }

    #[test]
    fn warp_is_adopted_without_sending() {
        let mut p = Player::new(parse("0:100:100:0,5:RIGHT:1").unwrap());
        assert_eq!(p.step(0), Some((100, 100, 0)));
        p.warp(200, 50);
        assert_eq!(p.step(1), None);
        assert_eq!(p.step(5), Some((203, 50, 0)));
    }

    #[test]
    fn pointer_inverse_is_exact() {
        for size in [560, 768] {
            for px in 0..size {
                assert_eq!(norm_to_pixel(pixel_to_norm(px, size), size), px, "size {} px {}", size, px);
            }
            assert_eq!(norm_to_pixel(-0x7fff, size), 0);
            assert_eq!(norm_to_pixel(i16::MIN, size), 0);
            assert_eq!(norm_to_pixel(0x7fff, size), size - 1);
        }
        assert!((0..768).all(|px| pixel_to_norm(px, 768) != 0) && (0..560).all(|py| pixel_to_norm(py, 560) != 0));
    }
}
