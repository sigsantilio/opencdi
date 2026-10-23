// Layouts and constants copied from libretro.h, (C) 2010-2024 The RetroArch team, MIT (see THIRD_PARTY.md).
#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_uint, c_void};

pub const RETRO_API_VERSION: c_uint = 1;

pub const RETRO_DEVICE_NONE: c_uint = 0;
pub const RETRO_DEVICE_JOYPAD: c_uint = 1;
pub const RETRO_DEVICE_MOUSE: c_uint = 2;
pub const RETRO_DEVICE_ANALOG: c_uint = 5;
pub const RETRO_DEVICE_POINTER: c_uint = 6;

pub const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
pub const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_JOYPAD_UP: c_uint = 4;
pub const RETRO_DEVICE_ID_JOYPAD_DOWN: c_uint = 5;
pub const RETRO_DEVICE_ID_JOYPAD_LEFT: c_uint = 6;
pub const RETRO_DEVICE_ID_JOYPAD_RIGHT: c_uint = 7;
pub const RETRO_DEVICE_ID_JOYPAD_A: c_uint = 8;

pub const RETRO_DEVICE_INDEX_ANALOG_LEFT: c_uint = 0;
pub const RETRO_DEVICE_ID_ANALOG_X: c_uint = 0;
pub const RETRO_DEVICE_ID_ANALOG_Y: c_uint = 1;

pub const RETRO_DEVICE_ID_MOUSE_X: c_uint = 0;
pub const RETRO_DEVICE_ID_MOUSE_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_MOUSE_LEFT: c_uint = 2;
pub const RETRO_DEVICE_ID_MOUSE_RIGHT: c_uint = 3;
pub const RETRO_DEVICE_ID_MOUSE_MIDDLE: c_uint = 6;

pub const RETRO_DEVICE_ID_POINTER_X: c_uint = 0;
pub const RETRO_DEVICE_ID_POINTER_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_POINTER_PRESSED: c_uint = 2;

pub const RETRO_REGION_NTSC: c_uint = 0;
pub const RETRO_REGION_PAL: c_uint = 1;

pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
pub const RETRO_MEMORY_RTC: c_uint = 1;
pub const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;
pub const RETRO_MEMORY_VIDEO_RAM: c_uint = 3;

pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;

pub const RETRO_LOG_DEBUG: c_uint = 0;
pub const RETRO_LOG_INFO: c_uint = 1;
pub const RETRO_LOG_WARN: c_uint = 2;
pub const RETRO_LOG_ERROR: c_uint = 3;

pub const RETRO_PIXEL_FORMAT_0RGB1555: c_uint = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;

pub type retro_environment_t = Option<unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool>;
pub type retro_video_refresh_t = Option<unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize)>;
pub type retro_audio_sample_t = Option<unsafe extern "C" fn(left: i16, right: i16)>;
pub type retro_audio_sample_batch_t = Option<unsafe extern "C" fn(data: *const i16, frames: usize) -> usize>;
pub type retro_input_poll_t = Option<unsafe extern "C" fn()>;
pub type retro_input_state_t = Option<unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16>;
pub type retro_log_printf_t = Option<unsafe extern "C" fn(level: c_uint, fmt: *const c_char, ...)>;

#[repr(C)]
pub struct retro_game_info {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

#[repr(C)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
pub struct retro_game_geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
pub struct retro_system_timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
pub struct retro_system_av_info {
    pub geometry: retro_game_geometry,
    pub timing: retro_system_timing,
}

#[repr(C)]
pub struct retro_log_callback {
    pub log: retro_log_printf_t,
}

#[repr(C)]
pub struct retro_input_descriptor {
    pub port: c_uint,
    pub device: c_uint,
    pub index: c_uint,
    pub id: c_uint,
    pub description: *const c_char,
}

#[repr(C)]
pub struct retro_controller_description {
    pub desc: *const c_char,
    pub id: c_uint,
}

#[repr(C)]
pub struct retro_controller_info {
    pub types: *const retro_controller_description,
    pub num_types: c_uint,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    const P: usize = size_of::<usize>();

    #[test]
    fn game_info_layout() {
        assert_eq!(size_of::<retro_game_info>(), 4 * P);
        assert_eq!(align_of::<retro_game_info>(), P);
        assert_eq!(offset_of!(retro_game_info, data), P);
        assert_eq!(offset_of!(retro_game_info, size), 2 * P);
        assert_eq!(offset_of!(retro_game_info, meta), 3 * P);
    }

    #[test]
    fn system_info_layout() {
        assert_eq!(size_of::<retro_system_info>(), 4 * P);
        assert_eq!(align_of::<retro_system_info>(), P);
        assert_eq!(offset_of!(retro_system_info, need_fullpath), 3 * P);
        assert_eq!(offset_of!(retro_system_info, block_extract), 3 * P + 1);
    }

    #[test]
    fn geometry_layout() {
        assert_eq!(size_of::<retro_game_geometry>(), 20);
        assert_eq!(align_of::<retro_game_geometry>(), 4);
        assert_eq!(offset_of!(retro_game_geometry, aspect_ratio), 16);
    }

    #[test]
    fn timing_layout() {
        assert_eq!(size_of::<retro_system_timing>(), 16);
        assert_eq!(align_of::<retro_system_timing>(), align_of::<f64>());
        assert_eq!(offset_of!(retro_system_timing, sample_rate), 8);
    }

    #[test]
    fn av_info_layout() {
        let a = align_of::<f64>();
        let timing_off = 20usize.div_ceil(a) * a;
        assert_eq!(offset_of!(retro_system_av_info, timing), timing_off);
        assert_eq!(size_of::<retro_system_av_info>(), timing_off + 16);
        assert_eq!(align_of::<retro_system_av_info>(), a);
        #[cfg(target_arch = "x86_64")]
        assert_eq!(size_of::<retro_system_av_info>(), 40);
    }

    #[test]
    fn input_descriptor_layout() {
        assert_eq!(offset_of!(retro_input_descriptor, id), 12);
        assert_eq!(offset_of!(retro_input_descriptor, description), 16usize.div_ceil(P) * P);
        assert_eq!(size_of::<retro_input_descriptor>(), 16usize.div_ceil(P) * P + P);
        assert_eq!(align_of::<retro_input_descriptor>(), P.max(4));
    }

    #[test]
    fn controller_info_layout() {
        assert_eq!(offset_of!(retro_controller_description, id), P);
        assert_eq!(size_of::<retro_controller_description>(), 2 * P);
        assert_eq!(offset_of!(retro_controller_info, num_types), P);
        assert_eq!(size_of::<retro_controller_info>(), 2 * P);
    }

    #[test]
    fn log_callback_layout() {
        assert_eq!(size_of::<retro_log_callback>(), P);
        assert_eq!(align_of::<retro_log_callback>(), P);
    }

    #[test]
    fn callback_pointers_are_pointer_sized() {
        assert_eq!(size_of::<retro_environment_t>(), P);
        assert_eq!(size_of::<retro_video_refresh_t>(), P);
        assert_eq!(size_of::<retro_audio_sample_batch_t>(), P);
        assert_eq!(size_of::<retro_input_state_t>(), P);
        assert_eq!(size_of::<retro_log_printf_t>(), P);
    }
}
