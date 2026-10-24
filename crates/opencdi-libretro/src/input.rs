// RetroPad, mouse and pointer on port 0 translated to the CD-i pointer (Slave::update_mouse).
use crate::libretro::*;
use opencdi::input_script::{self, Pad, PadButton, START_X, START_Y, MAX_X, MAX_Y};
use opencdi::mcd212::{SCREEN_HEIGHT, SCREEN_WIDTH};
use opencdi::slave::SlaveHle;
use std::ffi::c_uint;

// Left stick deflection that counts as a d-pad press.
const ANALOG_THRESHOLD: i16 = 0x4000;

pub struct Input {
    pub device: c_uint,
    x: i32,
    y: i32,
    pointer_buttons: u8,
    raw_pointer: (i16, i16, bool),
    sent: (i32, i32, u8),
}

impl Input {
    pub const fn new() -> Self {
        Input { device: RETRO_DEVICE_JOYPAD, x: START_X, y: START_Y, pointer_buttons: 0, raw_pointer: (0, 0, false), sent: (START_X, START_Y, 0) }
    }

    // Back to the power-on pointer; the port device is kept.
    pub fn reset(&mut self) {
        *self = Input { device: self.device, ..Input::new() };
    }

    // Call after input_poll and before run_frame; only calls the slave when (x, y, buttons) changed.
    pub fn apply(&mut self, state: retro_input_state_t, slave: &mut SlaveHle) {
        if slave.device_mouse_updated {
            slave.device_mouse_updated = false;
            self.x = slave.device_mouse_x;
            self.y = slave.device_mouse_y;
            self.sent.0 = self.x;
            self.sent.1 = self.y;
        }
        let Some(f) = state else { return };
        if self.device == RETRO_DEVICE_NONE {
            return;
        }
        // SAFETY: the frontend's input_state callback, port 0.
        let q = |device: c_uint, index: c_uint, id: c_uint| unsafe { f(0, device, index, id) };
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
                self.x = (self.x + q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_X) as i32).clamp(0, MAX_X);
                self.y = (self.y + q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_Y) as i32).clamp(0, MAX_Y);
                let mut b = 0;
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT) != 0 { b |= input_script::BTN_1; }
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT) != 0 { b |= input_script::BTN_2; }
                if q(RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0 { b |= input_script::BTN_3; }
                self.pointer_buttons = b;
            }
            _ => {}
        }
        let mut pad = Pad::default();
        let ax = q(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X);
        let ay = q(RETRO_DEVICE_ANALOG, RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y);
        for (id, b, stick) in [
            (RETRO_DEVICE_ID_JOYPAD_UP, PadButton::Up, ay < -ANALOG_THRESHOLD),
            (RETRO_DEVICE_ID_JOYPAD_DOWN, PadButton::Down, ay > ANALOG_THRESHOLD),
            (RETRO_DEVICE_ID_JOYPAD_LEFT, PadButton::Left, ax < -ANALOG_THRESHOLD),
            (RETRO_DEVICE_ID_JOYPAD_RIGHT, PadButton::Right, ax > ANALOG_THRESHOLD),
            (RETRO_DEVICE_ID_JOYPAD_A, PadButton::A, false),
            (RETRO_DEVICE_ID_JOYPAD_B, PadButton::B, false),
            (RETRO_DEVICE_ID_JOYPAD_Y, PadButton::Y, false),
        ] {
            if stick || q(RETRO_DEVICE_JOYPAD, 0, id) != 0 {
                pad = pad.with(b);
            }
        }
        (self.x, self.y) = pad.step(self.x, self.y);
        let now = (self.x, self.y, self.pointer_buttons | pad.buttons());
        if now != self.sent {
            self.sent = now;
            slave.update_mouse(now.0, now.1, now.2);
        }
    }
}

// Pointer position and last values sent to the slave go into the savestate; the port device is configuration.
opencdi::impl_state!(Input { x, y, pointer_buttons, raw_pointer, sent } skip { device });

const fn desc(device: c_uint, index: c_uint, id: c_uint, text: &'static std::ffi::CStr) -> retro_input_descriptor {
    retro_input_descriptor { port: 0, device, index, id, description: text.as_ptr() }
}

// Static FFI tables hold raw pointers to static data only.
pub struct Table<T>(pub T);
// SAFETY: the wrapped tables are immutable and point only at static data.
unsafe impl<T> Sync for Table<T> {}

pub static DESCRIPTORS: Table<[retro_input_descriptor; 10]> = Table([
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, c"Pointer up"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, c"Pointer down"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, c"Pointer left"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, c"Pointer right"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, c"Button 1"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_Y, c"Button 2"),
    desc(RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, c"Button 3 (both buttons)"),
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
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

    static DEVICE_HELD: AtomicU32 = AtomicU32::new(u32::MAX);
    static ID_HELD: AtomicU32 = AtomicU32::new(u32::MAX);
    static PX: AtomicI32 = AtomicI32::new(0);
    static PY: AtomicI32 = AtomicI32::new(0);

    unsafe extern "C" fn fake(_port: c_uint, device: c_uint, _index: c_uint, id: c_uint) -> i16 {
        match (device, id) {
            (RETRO_DEVICE_POINTER, RETRO_DEVICE_ID_POINTER_X) => PX.load(Ordering::SeqCst) as i16,
            (RETRO_DEVICE_POINTER, RETRO_DEVICE_ID_POINTER_Y) => PY.load(Ordering::SeqCst) as i16,
            (d, i) if d == DEVICE_HELD.load(Ordering::SeqCst) && i == ID_HELD.load(Ordering::SeqCst) => 1,
            _ => 0,
        }
    }

    #[test]
    fn dpad_and_pointer_reach_the_slave() {
        let mut s = SlaveHle::new();
        let mut inp = Input::new();
        inp.apply(Some(fake), &mut s);
        assert_eq!((s.device_mouse_x, s.device_mouse_y), (START_X, START_Y));
        DEVICE_HELD.store(RETRO_DEVICE_JOYPAD, Ordering::SeqCst);
        ID_HELD.store(RETRO_DEVICE_ID_JOYPAD_RIGHT, Ordering::SeqCst);
        inp.apply(Some(fake), &mut s);
        inp.apply(Some(fake), &mut s);
        assert_eq!(s.device_mouse_x, START_X + 2 * input_script::PAD_SPEED);
        DEVICE_HELD.store(u32::MAX, Ordering::SeqCst);
        inp.device = RETRO_DEVICE_POINTER;
        PX.store(input_script::pixel_to_norm(575, SCREEN_WIDTH as i32) as i32, Ordering::SeqCst);
        PY.store(input_script::pixel_to_norm(335, SCREEN_HEIGHT as i32) as i32, Ordering::SeqCst);
        inp.apply(Some(fake), &mut s);
        assert_eq!((s.device_mouse_x, s.device_mouse_y), (575, 335));
    }
}
