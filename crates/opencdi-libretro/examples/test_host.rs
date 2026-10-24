// Headless libretro frontend: dlopens the core, runs N frames, prints the same FNV video hash as src/bin/harness.rs.
use libloading::{Library, Symbol};
use opencdi::input_script::{self, BTN_1, BTN_2, BTN_3, Event, Pad, PadButton};
use opencdi::mcd212::{SCREEN_HEIGHT, SCREEN_WIDTH};
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::sync::Mutex;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const ENV_GET_SYSTEM_DIRECTORY: c_uint = 9;
const ENV_SET_PIXEL_FORMAT: c_uint = 10;
const ENV_SET_SUPPORT_NO_GAME: c_uint = 18;
const ENV_GET_LOG_INTERFACE: c_uint = 27;
const ENV_GET_VARIABLE: c_uint = 15;
const ENV_SET_VARIABLES: c_uint = 16;
const PIXEL_FORMAT_XRGB8888: c_uint = 1;
const MEMORY_SYSTEM_RAM: c_uint = 2;
const DEVICE_JOYPAD: c_uint = 1;
const DEVICE_POINTER: c_uint = 6;
const JOYPAD_IDS: [(c_uint, PadButton); 7] = [
    (0, PadButton::B),
    (1, PadButton::Y),
    (4, PadButton::Up),
    (5, PadButton::Down),
    (6, PadButton::Left),
    (7, PadButton::Right),
    (8, PadButton::A),
];
// Harness autoplay's shell-state long (0xC000 + 0xFFFF9172, wrapped) as an offset into plane A.
const SHELL_STATE: usize = 0x5172;

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
struct Variable {
    key: *const c_char,
    value: *const c_char,
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
    nonblack: usize,
    pointer: (i16, i16, bool),
    pad: Pad,
    options: Vec<(CString, CString)>,
    audio_out: Option<Vec<u8>>,
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
        ENV_SET_VARIABLES => true,
        ENV_GET_VARIABLE => {
            let v = unsafe { &mut *(data as *mut Variable) };
            let key = unsafe { CStr::from_ptr(v.key) };
            match h.options.iter().find(|(k, _)| k.as_c_str() == key) {
                Some((_, val)) => {
                    v.value = val.as_ptr();
                    true
                }
                None => false,
            }
        }
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
        h.nonblack = (0..height as usize)
            .map(|y| unsafe { std::slice::from_raw_parts((data as *const u8).add(y * pitch) as *const u32, width as usize) })
            .map(|row| row.iter().filter(|&&p| p & 0x00FF_FFFF != 0).count())
            .sum();
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
        if let Some(out) = h.audio_out.as_mut() {
            out.extend(s.iter().flat_map(|x| x.to_le_bytes()));
        }
        h.audio_frames += frames as u64;
    });
    frames
}

unsafe extern "C" fn input_poll_cb() {}

unsafe extern "C" fn input_state_cb(port: c_uint, device: c_uint, _index: c_uint, id: c_uint) -> i16 {
    if port != 0 {
        return 0;
    }
    with_host(|h| match (device, id) {
        (DEVICE_POINTER, 0) => h.pointer.0,
        (DEVICE_POINTER, 1) => h.pointer.1,
        (DEVICE_POINTER, 2) => h.pointer.2 as i16,
        (DEVICE_JOYPAD, _) => JOYPAD_IDS.iter().find(|(i, _)| *i == id).is_some_and(|(_, b)| h.pad.held(*b)) as i16,
        _ => 0,
    })
}

fn usage() -> ! {
    eprintln!("usage: test_host <core.so> <system_dir> [frames=600] [game_path|none] [--replay <script|@file> | --memory] [--sample <n>] [--roundtrip <frame>] [--option <key>=<value>]... [--audio-out <file.s16>] [--state-out <frame> <file>] [--state-in <file>]");
    std::process::exit(2);
}

// Alternates between two raw values of the same pixel so a repeated event is still a pointer change.
fn raw_pointer(x: i32, y: i32, pressed: bool, alt: &mut i16) -> (i16, i16, bool) {
    *alt ^= 1;
    let raw = |p: i32, size: usize| {
        let n = input_script::pixel_to_norm(p, size as i32) + *alt;
        assert_eq!(input_script::norm_to_pixel(n, size as i32), p.clamp(0, size as i32 - 1), "pointer inverse");
        n
    };
    (raw(x, SCREEN_WIDTH), raw(y, SCREEN_HEIGHT), pressed)
}

// Replays a shared-format script through the core: pointer events via RETRO_DEVICE_POINTER, pad events via the RetroPad.
#[derive(Clone)]
struct Replay {
    events: Vec<Event>,
    next: usize,
    alt: i16,
    pointer: (i16, i16, bool),
    pointer_pad: Pad,
}

impl Replay {
    fn has_pointer(&self) -> bool {
        self.events.iter().any(|e| matches!(e, Event::Pointer { .. }))
    }

    // Raw pointer and pad state the frontend reports for `frame`.
    fn at(&mut self, frame: u64) -> ((i16, i16, bool), Pad) {
        while let Some(&ev) = self.events.get(self.next) {
            if ev.frame() > frame {
                break;
            }
            self.next += 1;
            if let Event::Pointer { x, y, buttons, .. } = ev {
                self.pointer = raw_pointer(x, y, buttons & BTN_1 != 0, &mut self.alt);
                let mut pad = Pad::default();
                if buttons & BTN_2 != 0 { pad = pad.with(PadButton::Y); }
                if buttons & BTN_3 != 0 { pad = pad.with(PadButton::A); }
                self.pointer_pad = pad;
            }
        }
        let held = self.events.iter().filter_map(|e| match *e {
            Event::Pad { frame: f, button, len } if frame >= f && frame - f < len => Some(button),
            _ => None,
        });
        (self.pointer, held.fold(self.pointer_pad, Pad::with))
    }
}

// Harness autoplay decision from plane A through retro_get_memory_data; see doc/handover/status-and-todo.md for the launch substitute.
#[derive(Clone)]
struct Memory {
    ready: u32,
    launched: bool,
    was_ready: bool,
    alt: i16,
    pointer: (i16, i16, bool),
}

impl Memory {
    fn at(&mut self, frame: u64, ram: &[u8]) -> (i16, i16, bool) {
        if self.launched {
            return self.pointer;
        }
        let state = u32::from_be_bytes(ram[SHELL_STATE..SHELL_STATE + 4].try_into().unwrap());
        if state >= 2 {
            self.ready += 1;
            match self.ready {
                1 | 50 | 150 | 200 => self.pointer = raw_pointer(575, 335, false, &mut self.alt),
                30 | 180 => self.pointer = raw_pointer(575, 335, true, &mut self.alt),
                _ => {}
            }
        }
        // No cpu.pc through libretro: the launch is taken as the shell state falling from ready (>= 2) back below 2.
        let was_ready = std::mem::replace(&mut self.was_ready, state >= 2);
        if was_ready && state < 2 && frame > 250 {
            self.launched = true;
            self.pointer = raw_pointer(384, 280, false, &mut self.alt);
        }
        self.pointer
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        usage();
    }
    let frames: u64 = a.get(3).map(|s| s.parse().unwrap_or_else(|_| usage())).unwrap_or(600);
    let game = a.get(4).filter(|s| !s.starts_with("--") && s.as_str() != "none").map(|s| CString::new(s.as_str()).unwrap());
    let mut replay: Option<Replay> = None;
    let mut memory: Option<Memory> = None;
    let mut sample = 0u64;
    let mut roundtrip: Option<u64> = None;
    let mut options = Vec::new();
    let mut audio_out: Option<String> = None;
    let mut state_out: Option<(u64, String)> = None;
    let mut state_in: Option<String> = None;
    let mut i = if a.len() > 4 && !a[4].starts_with("--") { 5 } else { 4 };
    while i < a.len() {
        match a[i].as_str() {
            "--replay" => {
                i += 1;
                let s = a.get(i).unwrap_or_else(|| usage());
                let text = match s.strip_prefix('@') {
                    Some(f) => std::fs::read_to_string(f).expect("read input script"),
                    None => s.clone(),
                };
                let events = input_script::parse(&text).unwrap_or_else(|e| panic!("input script: {}", e));
                replay = Some(Replay { events, next: 0, alt: 0, pointer: (0, 0, false), pointer_pad: Pad::default() });
            }
            "--memory" => memory = Some(Memory { ready: 0, launched: false, was_ready: false, alt: 0, pointer: (0, 0, false) }),
            "--sample" => { i += 1; sample = a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()); }
            "--roundtrip" => { i += 1; roundtrip = Some(a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage())); }
            "--option" => {
                i += 1;
                let (k, v) = a.get(i).and_then(|s| s.split_once('=')).unwrap_or_else(|| usage());
                options.push((CString::new(k).unwrap(), CString::new(v).unwrap()));
            }
            "--audio-out" => { i += 1; audio_out = Some(a.get(i).cloned().unwrap_or_else(|| usage())); }
            "--state-out" => {
                let at = a.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
                state_out = Some((at, a.get(i + 2).cloned().unwrap_or_else(|| usage())));
                i += 2;
            }
            "--state-in" => { i += 1; state_in = Some(a.get(i).cloned().unwrap_or_else(|| usage())); }
            _ => usage(),
        }
        i += 1;
    }
    if (replay.is_some() && memory.is_some()) || (memory.is_some() && game.is_none()) || roundtrip.is_some_and(|n| n >= frames) {
        usage();
    }
    *HOST.lock().unwrap() = Some(HostState {
        sysdir: CString::new(a[2].as_str()).unwrap(),
        video_all: FNV_OFFSET,
        video_last: FNV_OFFSET,
        frames: 0,
        audio: FNV_OFFSET,
        audio_frames: 0,
        pixel_format_ok: false,
        no_game_ok: false,
        nonblack: 0,
        pointer: (0, 0, false),
        pad: Pad::default(),
        options,
        audio_out: audio_out.as_ref().map(|_| Vec::new()),
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
        let mem_data = sym!("retro_get_memory_data", unsafe extern "C" fn(c_uint) -> *mut c_void);
        let set_port = sym!("retro_set_controller_port_device", unsafe extern "C" fn(c_uint, c_uint));
        let serialize_size = sym!("retro_serialize_size", unsafe extern "C" fn() -> usize);
        let serialize = sym!("retro_serialize", unsafe extern "C" fn(*mut c_void, usize) -> bool);
        let unserialize = sym!("retro_unserialize", unsafe extern "C" fn(*const c_void, usize) -> bool);

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

        if let Some(f) = &state_in {
            // A state from another setup must be refused without poisoning the core.
            let bytes = std::fs::read(f).expect("read state");
            let loaded = unserialize(bytes.as_ptr() as *const c_void, bytes.len());
            println!("STATEIN {}", if loaded { "loaded" } else { "refused" });
        }
        if memory.is_some() || replay.as_ref().is_some_and(Replay::has_pointer) {
            set_port(0, DEVICE_POINTER);
        }
        let step = |frame: u64, replay: &mut Option<Replay>, memory: &mut Option<Memory>| {
            if let Some(r) = replay.as_mut() {
                let (pointer, pad) = r.at(frame);
                with_host(|h| (h.pointer, h.pad) = (pointer, pad));
            }
            if let Some(mm) = memory.as_mut() {
                let ram = std::slice::from_raw_parts(mem_data(MEMORY_SYSTEM_RAM) as *const u8, mem_size(MEMORY_SYSTEM_RAM));
                let pointer = mm.at(frame, ram);
                with_host(|h| h.pointer = pointer);
            }
            run();
            if let Some((at, f)) = &state_out {
                if frame + 1 == *at {
                    let size = serialize_size();
                    let mut buf = vec![0u8; size];
                    assert!(serialize(buf.as_mut_ptr() as *mut c_void, size), "retro_serialize failed");
                    std::fs::write(f, &buf).expect("write state");
                }
            }
            if sample > 0 && (frame + 1) % sample == 0 {
                eprintln!("SAMPLE frame={} nonblack={}", frame + 1, with_host(|h| h.nonblack));
            }
        };
        let split = roundtrip.unwrap_or(frames);
        for frame in 0..split {
            step(frame, &mut replay, &mut memory);
        }
        if let Some(at) = roundtrip {
            // Save at `at`, run to the end (A), load, run the same frames with the same input again (B); A and B must match.
            let size = serialize_size();
            assert!(size > 0, "retro_serialize_size returned 0");
            let mut saved = vec![0u8; size];
            assert!(serialize(saved.as_mut_ptr() as *mut c_void, size), "retro_serialize failed");
            let used = saved.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
            let (replay0, memory0) = (replay.clone(), memory.clone());
            let restart = || with_host(|h| {
                (h.video_all, h.video_last, h.audio, h.audio_frames, h.frames) = (FNV_OFFSET, FNV_OFFSET, FNV_OFFSET, 0, 0);
            });
            let result = || with_host(|h| (h.video_all, h.video_last, h.audio, h.audio_frames, h.frames, h.nonblack));
            restart();
            for frame in at..frames {
                step(frame, &mut replay, &mut memory);
            }
            let a = result();
            let t = std::time::Instant::now();
            assert!(unserialize(saved.as_ptr() as *const c_void, size), "retro_unserialize failed");
            let load_ms = t.elapsed().as_secs_f64() * 1e3;
            let mut again = vec![0u8; size];
            let t = std::time::Instant::now();
            assert!(serialize(again.as_mut_ptr() as *mut c_void, size), "retro_serialize after load failed");
            let save_ms = t.elapsed().as_secs_f64() * 1e3;
            let reser = if again == saved { "same" } else { "DIFF" };
            (replay, memory) = (replay0, memory0);
            restart();
            for frame in at..frames {
                step(frame, &mut replay, &mut memory);
            }
            let b = result();
            let stable = serialize_size() == size;
            // Rejections must return false without poisoning the core.
            let mut bad = saved.clone();
            bad[0] ^= 0xFF;
            let mut reject = !unserialize(bad.as_ptr() as *const c_void, size);
            reject &= !unserialize(saved.as_ptr() as *const c_void, used / 2);
            bad = saved.clone();
            bad[used / 2] ^= 0x01;
            reject &= !unserialize(bad.as_ptr() as *const c_void, size);
            reject &= serialize(again.as_mut_ptr() as *mut c_void, size);
            eprintln!(
                "ROUNDTRIP-DETAIL size={} used={} save_ms={:.2} load_ms={:.2} a=video_all:{:016x},video_last:{:016x},audio:{:016x},audio_frames:{},nonblack:{} b=video_all:{:016x},video_last:{:016x},audio:{:016x},audio_frames:{},nonblack:{}",
                size, used, save_ms, load_ms, a.0, a.1, a.2, a.3, a.5, b.0, b.1, b.2, b.3, b.5
            );
            println!(
                "ROUNDTRIP at={} frames={} reser={} video={} audio={} size_stable={} reject={}",
                at, a.4,
                reser,
                if (a.0, a.1) == (b.0, b.1) { "match" } else { "DIFFER" },
                if (a.2, a.3) == (b.2, b.3) { "match" } else { "DIFFER" },
                if stable { "yes" } else { "NO" },
                if reject { "ok" } else { "FAILED" }
            );
            unload_game();
            deinit();
            return;
        }

        if let Some(f) = &audio_out {
            std::fs::write(f, with_host(|h| h.audio_out.take()).unwrap_or_default()).expect("write audio");
        }
        with_host(|h| {
            assert!(h.pixel_format_ok && h.no_game_ok, "pixel format or no-game not negotiated");
            println!(
                "HOST frames={} video_last={:016x} video_all={:016x} audio={:016x} audio_frames={} nonblack={}",
                h.frames, h.video_last, h.video_all, h.audio, h.audio_frames, h.nonblack
            );
        });
        unload_game();
        deinit();
    }
}
