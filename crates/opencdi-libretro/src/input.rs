// RetroPad, mouse and pointer on port 0 translated to the CD-i pointer (Slave::update_mouse).
use crate::libretro::*;
use opencdi::input_script::{self, Pad, PadButton, PadSpeed, START_X, START_Y, MAX_X, MAX_Y};
use opencdi::mcd212::{SCREEN_HEIGHT, SCREEN_WIDTH};
use opencdi::bus::CdiBus;
use std::ffi::c_uint;

// Left stick inner deadzone (15.6% of full deflection); motion above it scales proportionally.
const ANALOG_DEADZONE: i32 = 0x1400;

pub const DEFAULT_DPAD_SPEED: i32 = input_script::PAD_SPEED;
pub const DEFAULT_ANALOG_SPEED: i32 = 6;
pub const DEFAULT_BOOST_X10: i32 = 20;

/// User-configurable cursor movement speeds and hold-to-boost multiplier for Libretro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorConfig {
    pub dpad_speed_x: i32,
    pub dpad_speed_y: i32,
    pub analog_speed_x: i32,
    pub analog_speed_y: i32,
    /// Boost multiplier scaled by 10 when holding R (10 = 1.0x/off, 15 = 1.5x, 20 = 2.0x, 30 = 3.0x).
    pub boost_multiplier_x10: i32,
}

impl CursorConfig {
    pub const fn default_config() -> Self {
        Self {
            dpad_speed_x: DEFAULT_DPAD_SPEED,
            dpad_speed_y: DEFAULT_DPAD_SPEED,
            analog_speed_x: DEFAULT_ANALOG_SPEED,
            analog_speed_y: DEFAULT_ANALOG_SPEED,
            boost_multiplier_x10: DEFAULT_BOOST_X10,
        }
    }
}

fn analog_norm(raw: i16) -> f32 {
    let v = i32::from(raw);
    let mag = v.abs();
    if mag <= ANALOG_DEADZONE {
        0.0
    } else {
        let frac = (mag - ANALOG_DEADZONE) as f32 / (32767 - ANALOG_DEADZONE) as f32;
        let sign = if v < 0 { -1.0 } else { 1.0 };
        (sign * frac).clamp(-1.0, 1.0)
    }
}

fn axis_step(sub: &mut f32, active: bool, dir_or_norm: f32, base_speed: i32, num: i32, den: i32) -> i32 {
    if !active || dir_or_norm == 0.0 {
        *sub = 0.0;
        return 0;
    }
    let v = (dir_or_norm * (base_speed * num) as f32) / (den as f32);
    *sub += v;
    let step = sub.trunc() as i32;
    *sub -= step as f32;
    step
}

pub struct Input {
    pub device: c_uint,
    x: i32,
    y: i32,
    pointer_buttons: u8,
    raw_pointer: (i16, i16, bool),
    sent: (i32, i32, u8),
    pad_speed: PadSpeed,
    speed_btn_held: bool,
    pub config: CursorConfig,
    sub_x: f32,
    sub_y: f32,
}

impl Input {
    pub const fn new() -> Self {
        Input {
            device: RETRO_DEVICE_JOYPAD,
            x: START_X,
            y: START_Y,
            pointer_buttons: 0,
            raw_pointer: (0, 0, false),
            sent: (START_X, START_Y, 0),
            pad_speed: PadSpeed::Medium,
            speed_btn_held: false,
            config: CursorConfig::default_config(),
            sub_x: 0.0,
            sub_y: 0.0,
        }
    }

    // Back to the power-on pointer; the port device, pad speed switch, and cursor config are kept.
    pub fn reset(&mut self) {
        *self = Input {
            device: self.device,
            pad_speed: self.pad_speed,
            config: self.config,
            ..Input::new()
        };
    }

    pub fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    // Call after input_poll and before run_frame; only calls the slave when (x, y, buttons) changed.
    // Returns `Some(new_speed)` on the frame the user cycles the 3-level CD-i pad speed switch (L button).
    pub fn apply(&mut self, state: retro_input_state_t, bus: &mut CdiBus) -> Option<PadSpeed> {
        let slave = &mut bus.slave;
        if slave.device_mouse_updated {
            slave.device_mouse_updated = false;
            self.x = slave.device_mouse_x;
            self.y = slave.device_mouse_y;
            self.sent.0 = self.x;
            self.sent.1 = self.y;
            self.sub_x = 0.0;
            self.sub_y = 0.0;
        }
        let Some(f) = state else { return None };
        if self.device == RETRO_DEVICE_NONE {
            return None;
        }
        // SAFETY: the frontend's input_state callback, port 0.
        let q = |device: c_uint, index: c_uint, id: c_uint| unsafe { f(0, device, index, id) };
        // Movement lost to the host position's edge clamp.
        let mut over = (0, 0);
        match self.device {
            RETRO_DEVICE_POINTER => {
                let raw = (
                    q(RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_X),
                    q(RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_Y),
                    q(RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_PRESSED) != 0,
                );
                if raw != self.raw_pointer {
                    self.raw_pointer = raw;
                    self.x = input_script::norm_to_pixel(raw.0, SCREEN_WIDTH as i32);
                    self.y = input_script::norm_to_pixel(raw.1, SCREEN_HEIGHT as i32);
                    self.pointer_buttons = if raw.2 { input_script::BTN_1 } else { 0 };
                }
            }
            RETRO_DEVICE_MOUSE => {
                let (nx, ny) = (self.x + q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_X) as i32, self.y + q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_Y) as i32);
                (self.x, self.y) = (nx.clamp(0, MAX_X), ny.clamp(0, MAX_Y));
                over = (nx - self.x, ny - self.y);
                let mut b = 0;
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT) != 0 { b |= input_script::BTN_1; }
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT) != 0 { b |= input_script::BTN_2; }
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0 { b |= input_script::BTN_3; }
                self.pointer_buttons = b;
            }
            _ => {}
        }
        let l_pressed = q(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_L) != 0;
        let speed_changed = if l_pressed && !self.speed_btn_held {
            self.pad_speed = self.pad_speed.next();
            Some(self.pad_speed)
        } else {
            None
        };
        self.speed_btn_held = l_pressed;

        let mut pad = Pad::default();
        for (id, b) in [
            (RETRO_DEVICE_ID_JOYPAD_UP, PadButton::Up),
            (RETRO_DEVICE_ID_JOYPAD_DOWN, PadButton::Down),
            (RETRO_DEVICE_ID_JOYPAD_LEFT, PadButton::Left),
            (RETRO_DEVICE_ID_JOYPAD_RIGHT, PadButton::Right),
            (RETRO_DEVICE_ID_JOYPAD_A, PadButton::A),
            (RETRO_DEVICE_ID_JOYPAD_B, PadButton::B),
            (RETRO_DEVICE_ID_JOYPAD_Y, PadButton::Y),
        ] {
            if q(RETRO_DEVICE_JOYPAD, 0, id) != 0 {
                pad = pad.with(b);
            }
        }

        let (pad_num, pad_den) = match self.pad_speed {
            PadSpeed::Low => (2, 3),
            PadSpeed::Medium => (1, 1),
            PadSpeed::High => (2, 1),
        };
        let r_held = q(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_R) != 0;
        let boost_x10 = if r_held && self.config.boost_multiplier_x10 > 10 {
            self.config.boost_multiplier_x10
        } else {
            10
        };
        let num = pad_num * boost_x10;
        let den = pad_den * 10;

        let dpad_x = pad.held(PadButton::Right) as i32 - pad.held(PadButton::Left) as i32;
        let dpad_y = pad.held(PadButton::Down) as i32 - pad.held(PadButton::Up) as i32;
        let ax = analog_norm(q(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X));
        let ay = analog_norm(q(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y));

        let dx = if dpad_x != 0 {
            axis_step(&mut self.sub_x, true, dpad_x as f32, self.config.dpad_speed_x, num, den)
        } else {
            axis_step(&mut self.sub_x, ax != 0.0, ax, self.config.analog_speed_x, num, den)
        };
        let dy = if dpad_y != 0 {
            axis_step(&mut self.sub_y, true, dpad_y as f32, self.config.dpad_speed_y, num, den)
        } else {
            axis_step(&mut self.sub_y, ay != 0.0, ay, self.config.analog_speed_y, num, den)
        };

        let (nx, ny) = (self.x + dx, self.y + dy);
        (self.x, self.y) = (nx.clamp(0, MAX_X), ny.clamp(0, MAX_Y));
        over = (over.0 + nx - self.x, over.1 + ny - self.y);
        let now = (self.x, self.y, self.pointer_buttons | pad.buttons());
        let changed = now != self.sent;
        self.sent = now;
        match bus.mono4.as_deref_mut() {
            Some(m4) => {
                if changed {
                    m4.ikat.update_pointer(now.0, now.1, now.2);
                }
                m4.ikat.push_pointer(over.0, over.1);
            }
            None if changed => bus.slave.update_mouse(now.0, now.1, now.2),
            None => {}
        }
        speed_changed
    }
}

// Pointer position and last values sent to the slave go into the savestate; host device/speed/cursor config stay local.
opencdi::impl_state!(Input { x, y, pointer_buttons, raw_pointer, sent } skip { device, pad_speed, speed_btn_held, config, sub_x, sub_y });

const fn desc(device: c_uint, index: c_uint, id: c_uint, text: &'static std::ffi::CStr) -> retro_input_descriptor {
    retro_input_descriptor { port: 0, device, index, id, description: text.as_ptr() }
}

// Static FFI tables hold raw pointers to static data only.
pub struct Table<T>(pub T);
// SAFETY: the wrapped tables are immutable and point only at static data.
unsafe impl<T> Sync for Table<T> {}

pub static DESCRIPTORS: Table<[retro_input_descriptor; 12]> = Table([
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, c"Pointer up"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, c"Pointer down"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, c"Pointer left"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, c"Pointer right"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, c"Button 1"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_Y, c"Button 2"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, c"Button 1+2 (Combined)"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_L, c"Pad speed (1/2/3)"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_R, c"Cursor speed boost (hold)"),
    desc(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X, c"Pointer X"),
    desc(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y, c"Pointer Y"),
    retro_input_descriptor { port: 0, device: 0, index: 0, id: 0, description: std::ptr::null() },
]);

static CONTROLLER_TYPES: Table<[retro_controller_description; 3]> = Table([
    retro_controller_description { desc: c"RetroPad".as_ptr(), id: RETRO_DEVICE_JOYPAD },
    retro_controller_description { desc: c"Mouse".as_ptr(), id: RETRO_DEVICE_MOUSE },
    retro_controller_description { desc: c"Pointer".as_ptr(), id: RETRO_DEVICE_POINTER },
]);

// One port, terminated by an empty entry.
pub static CONTROLLER_INFO: Table<[retro_controller_info; 2]> = Table([
    retro_controller_info { types: &CONTROLLER_TYPES.0 as *const _ as *const retro_controller_description, num_types: 3 },
    retro_controller_info { types: std::ptr::null(), num_types: 0 },
]);

pub fn supported(device: c_uint) -> bool {
    device == RETRO_DEVICE_NONE || CONTROLLER_TYPES.0.iter().any(|t| t.id == device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static DEVICE_HELD: Cell<u32> = const { Cell::new(u32::MAX) };
        static ID_HELD: Cell<u32> = const { Cell::new(u32::MAX) };
        static PX: Cell<i32> = const { Cell::new(0) };
        static PY: Cell<i32> = const { Cell::new(0) };
        static AX: Cell<i16> = const { Cell::new(0) };
        static AY: Cell<i16> = const { Cell::new(0) };
        static R_HELD: Cell<bool> = const { Cell::new(false) };
    }

    fn clear_fake_inputs() {
        DEVICE_HELD.set(u32::MAX);
        ID_HELD.set(u32::MAX);
        PX.set(0);
        PY.set(0);
        AX.set(0);
        AY.set(0);
        R_HELD.set(false);
    }

    unsafe extern "C" fn fake(_port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
        match (device, index, id) {
            (RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_X) => PX.get() as i16,
            (RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_Y) => PY.get() as i16,
            (RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X) => AX.get(),
            (RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y) => AY.get(),
            (RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_R) if R_HELD.get() => 1,
            (d, 0, i) if d == DEVICE_HELD.get() && i == ID_HELD.get() => 1,
            _ => 0,
        }
    }

    #[test]
    fn dpad_and_pointer_reach_the_slave() {
        clear_fake_inputs();
        let mut bus = CdiBus::new();
        let mut inp = Input::new();
        inp.apply(Some(fake), &mut bus);
        assert_eq!((bus.slave.device_mouse_x, bus.slave.device_mouse_y), (START_X, START_Y));
        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_RIGHT);
        inp.apply(Some(fake), &mut bus);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 2 * input_script::PAD_SPEED);
        DEVICE_HELD.set(u32::MAX);
        inp.device = RETRO_DEVICE_POINTER;
        PX.set(input_script::pixel_to_norm(575, SCREEN_WIDTH as i32) as i32);
        PY.set(input_script::pixel_to_norm(335, SCREEN_HEIGHT as i32) as i32);
        inp.apply(Some(fake), &mut bus);
        assert_eq!((bus.slave.device_mouse_x, bus.slave.device_mouse_y), (575, 335));
        clear_fake_inputs();
    }

    #[test]
    fn l_button_cycles_three_cdi_pad_speeds() {
        clear_fake_inputs();
        let mut bus = CdiBus::new();
        let mut inp = Input::new();
        assert_eq!(inp.pad_speed, PadSpeed::Medium);

        // Press L -> High (6 px/frame), holding L on next frame does not re-trigger
        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_L);
        assert_eq!(inp.apply(Some(fake), &mut bus), Some(PadSpeed::High));
        assert_eq!(inp.apply(Some(fake), &mut bus), None);

        // Release L, then move right 1 frame at High speed (6 px)
        DEVICE_HELD.set(u32::MAX);
        assert_eq!(inp.apply(Some(fake), &mut bus), None);
        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_RIGHT);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + PadSpeed::High.pixels_per_frame());

        // Cycle to Low (2 px/frame) and back to Medium (3 px/frame)
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_L);
        assert_eq!(inp.apply(Some(fake), &mut bus), Some(PadSpeed::Low));
        DEVICE_HELD.set(u32::MAX);
        inp.apply(Some(fake), &mut bus);
        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_L);
        assert_eq!(inp.apply(Some(fake), &mut bus), Some(PadSpeed::Medium));
        clear_fake_inputs();
    }

    #[test]
    fn custom_dpad_xy_speeds_apply_independently() {
        clear_fake_inputs();
        let mut bus = CdiBus::new();
        let mut inp = Input::new();
        inp.config.dpad_speed_x = 7;
        inp.config.dpad_speed_y = 2;

        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_RIGHT);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 7);
        assert_eq!(bus.slave.device_mouse_y, START_Y);

        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_DOWN);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 7);
        assert_eq!(bus.slave.device_mouse_y, START_Y + 2);
        clear_fake_inputs();
    }

    #[test]
    fn analog_stick_moves_proportionally_with_deadzone_and_separate_speed() {
        clear_fake_inputs();
        let mut bus = CdiBus::new();
        let mut inp = Input::new();
        inp.config.analog_speed_x = 10;
        inp.config.analog_speed_y = 4;

        // Inside deadzone: no movement
        AX.set(0x1000);
        AY.set(-0x1000);
        inp.apply(Some(fake), &mut bus);
        assert_eq!((bus.slave.device_mouse_x, bus.slave.device_mouse_y), (START_X, START_Y));

        // Half deflection above deadzone on X (~50.002%), full positive deflection on Y
        let half_x = (ANALOG_DEADZONE + (32767 - ANALOG_DEADZONE + 1) / 2) as i16;
        AX.set(half_x);
        AY.set(i16::MAX);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 5);
        assert_eq!(bus.slave.device_mouse_y, START_Y + 4);
        clear_fake_inputs();
    }

    #[test]
    fn hold_r_boosts_cursor_speed_with_fractional_accumulation() {
        clear_fake_inputs();
        let mut bus = CdiBus::new();
        let mut inp = Input::new();
        inp.config.dpad_speed_x = 3;
        inp.config.boost_multiplier_x10 = 15; // 1.5x -> 4.5 px/frame

        DEVICE_HELD.set(RETRO_DEVICE_JOYPAD);
        ID_HELD.set(RETRO_DEVICE_ID_JOYPAD_RIGHT);
        R_HELD.set(true);

        // Frame 1: +4 px (0.5 carried), Frame 2: +5 px (total +9 px exact)
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 4);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 9);

        // Releasing R clears fractional boost on the next move
        R_HELD.set(false);
        inp.apply(Some(fake), &mut bus);
        assert_eq!(bus.slave.device_mouse_x, START_X + 12);
        clear_fake_inputs();
    }
}
