// Headless libretro frontend: dlopens the core, runs N frames, prints the same FNV video hash as src/bin/harness.rs.
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::sync::Mutex;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const ENV_GET_SYSTEM_DIRECTORY: c_uint = 9;
const ENV_SET_PIXEL_FORMAT: c_uint = 10;
const ENV_SET_SUPPORT_NO_GAME: c_uint = 18;
const ENV_GET_LOG_INTERFACE: c_uint = 27;
const PIXEL_FORMAT_XRGB8888: c_uint = 1;
const MEMORY_SYSTEM_RAM: c_uint = 2;

#[repr(C)]
struct SystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
}

#[repr(C)]
#[derive(Default)]
struct AvInfo {
    base_width: c_uint,
    base_height: c_uint,
    max_width: c_uint,
    max_height: c_uint,
    aspect_ratio: f32,
    fps: f64,
    sample_rate: f64,
}

#[repr(C)]
struct GameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

struct HostState {
    sysdir: CString,
    video_all: u64,
    video_last: u64,
    frames: u64,
    audio: u64,
    audio_frames: u64,
    pixel_format_ok: bool,
    no_game_ok: bool,
}

static HOST: Mutex<Option<HostState>> = Mutex::new(None);

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn with_host<R>(f: impl FnOnce(&mut HostState) -> R) -> R {
    f(HOST.lock().unwrap().as_mut().unwrap())
}

unsafe extern "C" fn env_cb(cmd: c_uint, data: *mut c_void) -> bool {
    with_host(|h| match cmd {
        ENV_GET_SYSTEM_DIRECTORY => {
            unsafe { *(data as *mut *const c_char) = h.sysdir.as_ptr() };
            true
        }
        ENV_SET_PIXEL_FORMAT => {
            h.pixel_format_ok = unsafe { *(data as *const c_uint) } == PIXEL_FORMAT_XRGB8888;
            h.pixel_format_ok
        }
        ENV_SET_SUPPORT_NO_GAME => {
            h.no_game_ok = unsafe { *(data as *const bool) };
            true
        }
        ENV_GET_LOG_INTERFACE => false,
        _ => false,
    })
}

unsafe extern "C" fn video_cb(data: *const c_void, width: c_uint, height: c_uint, pitch: usize) {
    with_host(|h| {
        assert!(!data.is_null(), "core sent a dupe frame; harness has no equivalent");
        let mut v = FNV_OFFSET;
        let mut all = h.video_all;
        for y in 0..height as usize {
            let row = unsafe { std::slice::from_raw_parts((data as *const u8).add(y * pitch) as *const u32, width as usize) };
            for &p in row {
                let b = p.to_le_bytes();
                all = fnv(all, &b);
                v = fnv(v, &b);
            }
        }
        h.video_all = all;
        h.video_last = v;
        h.frames += 1;
    })
}

unsafe extern "C" fn audio_sample_cb(_l: i16, _r: i16) {}

unsafe extern "C" fn audio_batch_cb(data: *const i16, frames: usize) -> usize {
    with_host(|h| {
        let s = unsafe { std::slice::from_raw_parts(data, frames * 2) };
        for &x in s {
            h.audio = fnv(h.audio, &x.to_le_bytes());
        }
        h.audio_frames += frames as u64;
    });
    frames
}

unsafe extern "C" fn input_poll_cb() {}

unsafe extern "C" fn input_state_cb(_port: c_uint, _device: c_uint, _index: c_uint, _id: c_uint) -> i16 {
    0
}

fn usage() -> ! {
    eprintln!("usage: test_host <core.so> <system_dir> [frames=600] [game_path]");
    std::process::exit(2);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        usage();
    }
    let frames: u64 = a.get(3).map(|s| s.parse().unwrap_or_else(|_| usage())).unwrap_or(600);
    let game = a.get(4).map(|s| CString::new(s.as_str()).unwrap());
    *HOST.lock().unwrap() = Some(HostState {
        sysdir: CString::new(a[2].as_str()).unwrap(),
        video_all: FNV_OFFSET,
        video_last: FNV_OFFSET,
        frames: 0,
        audio: FNV_OFFSET,
        audio_frames: 0,
        pixel_format_ok: false,
        no_game_ok: false,
    });

    unsafe {
        let lib = Library::new(&a[1]).expect("dlopen core");
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let s: Symbol<$ty> = lib.get(concat!($name, "\0").as_bytes()).expect($name);
                *s
            }};
        }
        let api_version = sym!("retro_api_version", unsafe extern "C" fn() -> c_uint);
        let get_system_info = sym!("retro_get_system_info", unsafe extern "C" fn(*mut SystemInfo));
        let get_av_info = sym!("retro_get_system_av_info", unsafe extern "C" fn(*mut AvInfo));
        let set_env = sym!("retro_set_environment", unsafe extern "C" fn(unsafe extern "C" fn(c_uint, *mut c_void) -> bool));
        let set_video = sym!("retro_set_video_refresh", unsafe extern "C" fn(unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize)));
        let set_audio = sym!("retro_set_audio_sample", unsafe extern "C" fn(unsafe extern "C" fn(i16, i16)));
        let set_audio_batch = sym!("retro_set_audio_sample_batch", unsafe extern "C" fn(unsafe extern "C" fn(*const i16, usize) -> usize));
        let set_input_poll = sym!("retro_set_input_poll", unsafe extern "C" fn(unsafe extern "C" fn()));
        let set_input_state = sym!("retro_set_input_state", unsafe extern "C" fn(unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16));
        let init = sym!("retro_init", unsafe extern "C" fn());
        let deinit = sym!("retro_deinit", unsafe extern "C" fn());
        let load_game = sym!("retro_load_game", unsafe extern "C" fn(*const GameInfo) -> bool);
        let unload_game = sym!("retro_unload_game", unsafe extern "C" fn());
        let run = sym!("retro_run", unsafe extern "C" fn());
        let mem_size = sym!("retro_get_memory_size", unsafe extern "C" fn(c_uint) -> usize);
        let region = sym!("retro_get_region", unsafe extern "C" fn() -> c_uint);

        assert_eq!(api_version(), 1, "RETRO_API_VERSION");
        let mut si = SystemInfo {
            library_name: std::ptr::null(),
            library_version: std::ptr::null(),
            valid_extensions: std::ptr::null(),
            need_fullpath: false,
            block_extract: false,
        };
        get_system_info(&mut si);
        let cs = |p: *const c_char| CStr::from_ptr(p).to_string_lossy().into_owned();
        eprintln!(
            "core {} {} exts={} need_fullpath={}",
            cs(si.library_name), cs(si.library_version), cs(si.valid_extensions), si.need_fullpath
        );

        set_env(env_cb);
        set_video(video_cb);
        set_audio(audio_sample_cb);
        set_audio_batch(audio_batch_cb);
        set_input_poll(input_poll_cb);
        set_input_state(input_state_cb);
        init();

        let gi = game.as_ref().map(|p| GameInfo { path: p.as_ptr(), data: std::ptr::null(), size: 0, meta: std::ptr::null() });
        let ok = load_game(gi.as_ref().map_or(std::ptr::null(), |g| g as *const GameInfo));
        assert!(ok, "retro_load_game failed");
        let mut av = AvInfo::default();
        get_av_info(&mut av);
        eprintln!(
            "av {}x{} max {}x{} aspect {:.4} fps {:.3} rate {} region {} system_ram {}",
            av.base_width, av.base_height, av.max_width, av.max_height, av.aspect_ratio, av.fps, av.sample_rate,
            region(), mem_size(MEMORY_SYSTEM_RAM)
        );

        for _ in 0..frames {
            run();
        }

        with_host(|h| {
            assert!(h.pixel_format_ok && h.no_game_ok, "pixel format or no-game not negotiated");
            println!(
                "HOST frames={} video_last={:016x} video_all={:016x} audio={:016x} audio_frames={}",
                h.frames, h.video_last, h.video_all, h.audio, h.audio_frames
            );
        });
        unload_game();
        deinit();
    }
}
