mod input;
pub mod libretro;

use input::{Input, Table};
use libretro::*;
use opencdi::audio;
use opencdi::bios;
use opencdi::deinterlace::Deinterlacer;
use opencdi::machine::Machine;
use opencdi::mcd212::{draw_crosshair, CrosshairMode, VideoMode, SCREEN_HEIGHT, SCREEN_WIDTH};
use opencdi::savestate::{Reader, State};
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

// Looked up case-insensitively in the system directory, in this order; Mono-IV images boot only with the opencdi_mono4 option on.
pub const BIOS_NAMES: &[&str] = &["cdi220b.rom", "cdi200.rom", "cdimono1.zip", "cdibios.zip", "cdi.rom", "cdi220.rom", "cdi490a.rom", "cdi490a.zip"];
// Cartridge ROM, looked up the same way; cdimono1.zip carries vmpega.rom next to the system ROMs.
pub const DVC_NAMES: &[&str] = &["vmpega.rom", "cdimono1.zip", "cdibios.zip"];
// The IMPEG cartridge ROM, fitted only on the CD-i 490 (Mono-IV) board.
pub const IMPEG_NAMES: &[&str] = &["impega.rom"];
const MESSAGE_FRAMES: c_uint = 360;
const AUDIO_RATE: u32 = opencdi::audio::OUT_RATE;
// Room above the size measured at the first serialize_size call, for buffers that grow later (DVC decoder, FIFOs).
const STATE_HEADROOM: usize = 4 << 20;
const STATE_ALIGN: usize = 64 << 10;

#[derive(Clone, Copy)]
struct Callbacks {
    env: retro_environment_t,
    video: retro_video_refresh_t,
    audio_sample: retro_audio_sample_t,
    audio_batch: retro_audio_sample_batch_t,
    input_poll: retro_input_poll_t,
    input_state: retro_input_state_t,
}

#[derive(Clone)]
struct LoadSpec {
    bios: PathBuf,
    dvc: Option<PathBuf>,
    disc: Option<PathBuf>,
    mode: VideoMode,
}

const OPT_DVC: &CStr = c"opencdi_dvc";
const OPT_MONO4: &CStr = c"opencdi_mono4";
const OPT_REGION: &CStr = c"opencdi_region";
const OPT_NVRAM_MODE: &CStr = c"opencdi_nvram_mode";
const OPT_CROSSHAIR: &CStr = c"opencdi_crosshair";
const OPT_LEVELS: &CStr = c"opencdi_video_levels";
const OPT_DEINTERLACE: &CStr = c"opencdi_deinterlace";
const OPT_DPAD_SPEED_X: &CStr = c"opencdi_dpad_speed_x";
const OPT_DPAD_SPEED_Y: &CStr = c"opencdi_dpad_speed_y";
const OPT_ANALOG_SPEED_X: &CStr = c"opencdi_analog_speed_x";
const OPT_ANALOG_SPEED_Y: &CStr = c"opencdi_analog_speed_y";
const OPT_CURSOR_BOOST: &CStr = c"opencdi_cursor_boost";
const NO_VALUE: retro_core_option_value = retro_core_option_value { value: std::ptr::null(), label: std::ptr::null() };

const DPAD_SPEED_VALUES: [(&CStr, &CStr); 12] = [
    (c"1", c"1 px/frame (Slowest)"),
    (c"2", c"2 px/frame"),
    (c"3", c"3 px/frame (Default)"),
    (c"4", c"4 px/frame"),
    (c"5", c"5 px/frame"),
    (c"6", c"6 px/frame"),
    (c"7", c"7 px/frame"),
    (c"8", c"8 px/frame"),
    (c"9", c"9 px/frame"),
    (c"10", c"10 px/frame"),
    (c"11", c"11 px/frame"),
    (c"12", c"12 px/frame (Fastest)"),
];

const ANALOG_SPEED_VALUES: [(&CStr, &CStr); 16] = [
    (c"1", c"1 px/frame (Slowest)"),
    (c"2", c"2 px/frame"),
    (c"3", c"3 px/frame"),
    (c"4", c"4 px/frame"),
    (c"5", c"5 px/frame"),
    (c"6", c"6 px/frame (Default)"),
    (c"7", c"7 px/frame"),
    (c"8", c"8 px/frame"),
    (c"9", c"9 px/frame"),
    (c"10", c"10 px/frame"),
    (c"11", c"11 px/frame"),
    (c"12", c"12 px/frame"),
    (c"13", c"13 px/frame"),
    (c"14", c"14 px/frame"),
    (c"15", c"15 px/frame"),
    (c"16", c"16 px/frame (Fastest)"),
];

const fn option_values(list: &[(&CStr, &CStr)]) -> [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX] {
    let mut out = [NO_VALUE; RETRO_NUM_CORE_OPTION_VALUES_MAX];
    let mut i = 0;
    while i < list.len() {
        out[i] = retro_core_option_value { value: list[i].0.as_ptr(), label: list[i].1.as_ptr() };
        i += 1;
    }
    out
}

// Core options v1, terminated by an empty entry; frontends without v1 get VARIABLES below.
static OPTIONS: Table<[retro_core_option_definition; 13]> = Table([
    retro_core_option_definition {
        key: OPT_DVC.as_ptr(),
        desc: c"DVC cartridge (restart)".as_ptr(),
        info: c"auto fits the VMPEG Digital Video Cartridge when vmpega.rom (or cdimono1.zip holding it) is in the system directory. Needed by Digital Video titles; takes effect when content is loaded.".as_ptr(),
        values: option_values(&[(c"auto", c"auto"), (c"off", c"off")]),
        default_value: c"auto".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_MONO4.as_ptr(),
        desc: c"CD-i 490 BIOS, experimental (restart)".as_ptr(),
        info: c"on also accepts the Mono-IV system ROM (cdi490a.rom) when no Mono-I ROM is found; with impega.rom present (and the DVC option on auto) the IMPEG Digital Video cartridge is fitted. The 490 board is experimental. Takes effect when content is loaded.".as_ptr(),
        values: option_values(&[(c"off", c"off"), (c"on", c"on")]),
        default_value: c"off".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_REGION.as_ptr(),
        desc: c"Console region / video standard (restart)".as_ptr(),
        info: c"auto selects NTSC (60 Hz) or PAL (50 Hz) from filename tags, MPEG sequence headers, and PVD metadata; pal or ntsc forces that video standard regardless of filename. Takes effect when content is loaded.".as_ptr(),
        values: option_values(&[(c"auto", c"auto"), (c"pal", c"PAL (50 Hz)"), (c"ntsc", c"NTSC (60 Hz)")]),
        default_value: c"auto".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_NVRAM_MODE.as_ptr(),
        desc: c"Console NVRAM mode (restart)".as_ptr(),
        info: c"per_game lets RetroArch keep separate .srm save RAM per disc (seeded from shared player settings when new); shared uses one console-wide NVRAM file across all games just like real CD-i hardware. Takes effect when content is loaded.".as_ptr(),
        values: option_values(&[(c"per_game", c"per_game (separate .srm)"), (c"shared", c"shared (console-wide NVRAM)")]),
        default_value: c"per_game".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_CROSSHAIR.as_ptr(),
        desc: c"Lightgun crosshair".as_ptr(),
        info: c"Draws a non-destructive reticle at the CD-i pointer position: auto appears only when the game hides its hardware cursor (such as Mad Dog McCree GUN calibration/gameplay). Takes effect at once.".as_ptr(),
        values: option_values(&[(c"off", c"off"), (c"auto", c"auto (when game hides cursor)"), (c"always", c"always")]),
        default_value: c"off".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_LEVELS.as_ptr(),
        desc: c"Video levels".as_ptr(),
        info: c"studio passes the CD-i video levels through unchanged (black 16, white 235), as the hardware outputs them. full stretches them to 0-255 (black 0), as PC displays expect. Takes effect at once.".as_ptr(),
        values: option_values(&[(c"studio", c"studio (black 16)"), (c"full", c"full (black 0)")]),
        default_value: c"studio".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_DEINTERLACE.as_ptr(),
        desc: c"De-interlacing (FMV / Interlaced)".as_ptr(),
        info: c"Motion-adaptive presentation de-interlacer: keeps full weave resolution on static areas and reconstructs moving interlaced regions (such as FMV camera pans) without vertical jitter. Takes effect at once.".as_ptr(),
        values: option_values(&[(c"off", c"off"), (c"on", c"on (motion-adaptive)")]),
        default_value: c"off".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_DPAD_SPEED_X.as_ptr(),
        desc: c"Cursor D-Pad speed (horizontal)".as_ptr(),
        info: c"Horizontal CD-i pointer speed (pixels per frame at Medium pad speed) when controlled with the D-Pad. Combines with the 3-level L button pad speed switch and R boost. Takes effect at once.".as_ptr(),
        values: option_values(&DPAD_SPEED_VALUES),
        default_value: c"3".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_DPAD_SPEED_Y.as_ptr(),
        desc: c"Cursor D-Pad speed (vertical)".as_ptr(),
        info: c"Vertical CD-i pointer speed (pixels per frame at Medium pad speed) when controlled with the D-Pad. Useful because CD-i native output is 768x560. Takes effect at once.".as_ptr(),
        values: option_values(&DPAD_SPEED_VALUES),
        default_value: c"3".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_ANALOG_SPEED_X.as_ptr(),
        desc: c"Cursor analog stick speed (horizontal)".as_ptr(),
        info: c"Maximum horizontal CD-i pointer speed (pixels per frame at full deflection) for the left analog stick. Movement scales proportionally beyond the inner deadzone. Takes effect at once.".as_ptr(),
        values: option_values(&ANALOG_SPEED_VALUES),
        default_value: c"6".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_ANALOG_SPEED_Y.as_ptr(),
        desc: c"Cursor analog stick speed (vertical)".as_ptr(),
        info: c"Maximum vertical CD-i pointer speed (pixels per frame at full deflection) for the left analog stick. Movement scales proportionally beyond the inner deadzone. Takes effect at once.".as_ptr(),
        values: option_values(&ANALOG_SPEED_VALUES),
        default_value: c"6".as_ptr(),
    },
    retro_core_option_definition {
        key: OPT_CURSOR_BOOST.as_ptr(),
        desc: c"Cursor speed boost (hold R)".as_ptr(),
        info: c"Speed multiplier applied while holding the R (or mapped boost) button on D-Pad or left analog cursor movement. Takes effect at once.".as_ptr(),
        values: option_values(&[(c"1.0", c"Off (1.0x)"), (c"1.5", c"1.5x"), (c"2.0", c"2.0x (Default)"), (c"2.5", c"2.5x"), (c"3.0", c"3.0x")]),
        default_value: c"2.0".as_ptr(),
    },
    retro_core_option_definition { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null(), values: [NO_VALUE; RETRO_NUM_CORE_OPTION_VALUES_MAX], default_value: std::ptr::null() },
]);
static VARIABLES: Table<[retro_variable; 13]> = Table([
    retro_variable { key: OPT_DVC.as_ptr(), value: c"DVC cartridge (restart); auto|off".as_ptr() },
    retro_variable { key: OPT_MONO4.as_ptr(), value: c"CD-i 490 BIOS, experimental (restart); off|on".as_ptr() },
    retro_variable { key: OPT_REGION.as_ptr(), value: c"Console region / video standard (restart); auto|pal|ntsc".as_ptr() },
    retro_variable { key: OPT_NVRAM_MODE.as_ptr(), value: c"Console NVRAM mode (restart); per_game|shared".as_ptr() },
    retro_variable { key: OPT_CROSSHAIR.as_ptr(), value: c"Lightgun crosshair; off|auto|always".as_ptr() },
    retro_variable { key: OPT_LEVELS.as_ptr(), value: c"Video levels; studio|full".as_ptr() },
    retro_variable { key: OPT_DEINTERLACE.as_ptr(), value: c"De-interlacing (FMV / Interlaced); off|on".as_ptr() },
    retro_variable { key: OPT_DPAD_SPEED_X.as_ptr(), value: c"Cursor D-Pad speed (horizontal); 3|1|2|4|5|6|7|8|9|10|11|12".as_ptr() },
    retro_variable { key: OPT_DPAD_SPEED_Y.as_ptr(), value: c"Cursor D-Pad speed (vertical); 3|1|2|4|5|6|7|8|9|10|11|12".as_ptr() },
    retro_variable { key: OPT_ANALOG_SPEED_X.as_ptr(), value: c"Cursor analog stick speed (horizontal); 6|1|2|3|4|5|7|8|9|10|11|12|13|14|15|16".as_ptr() },
    retro_variable { key: OPT_ANALOG_SPEED_Y.as_ptr(), value: c"Cursor analog stick speed (vertical); 6|1|2|3|4|5|7|8|9|10|11|12|13|14|15|16".as_ptr() },
    retro_variable { key: OPT_CURSOR_BOOST.as_ptr(), value: c"Cursor speed boost (hold R); 2.0|1.0|1.5|2.5|3.0".as_ptr() },
    retro_variable { key: std::ptr::null(), value: std::ptr::null() },
]);

struct Core {
    machine: Option<Machine>,
    spec: Option<LoadSpec>,
    temp_disc: Option<PathBuf>,
    mix: Vec<(f32, f32)>,
    pcm: Vec<i16>,
    frames_out: u64,
    input: Input,
    uart_line: Vec<u8>,
    state_buf: Vec<u8>,
    state_size: usize,
    deinterlace: bool,
    deinterlacer: Deinterlacer,
    crosshair: CrosshairMode,
    present_fb: Vec<u32>,
    shared_nvram_mode: bool,
    shared_nvram_path: Option<PathBuf>,
    is_no_game: bool,
    rtc_seeded: bool,
}

static CALLBACKS: Mutex<Callbacks> = Mutex::new(Callbacks {
    env: None,
    video: None,
    audio_sample: None,
    audio_batch: None,
    input_poll: None,
    input_state: None,
});
static CORE: Mutex<Core> = Mutex::new(Core {
    machine: None,
    spec: None,
    temp_disc: None,
    mix: Vec::new(),
    uart_line: Vec::new(),
    pcm: Vec::new(),
    frames_out: 0,
    input: Input::new(),
    state_buf: Vec::new(),
    state_size: 0,
    deinterlace: false,
    deinterlacer: Deinterlacer::new(),
    crosshair: CrosshairMode::Off,
    present_fb: Vec::new(),
    shared_nvram_mode: false,
    shared_nvram_path: None,
    is_no_game: false,
    rtc_seeded: false,
});
static LOG: Mutex<retro_log_printf_t> = Mutex::new(None);
static POISONED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static PANIC_IN_RUN: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn callbacks() -> Callbacks {
    *lock(&CALLBACKS)
}

fn log(level: c_uint, msg: &str) {
    let cb = *lock(&LOG);
    let text = format!("[OpenCDI] {}\n", msg);
    match cb {
        Some(f) => {
            let c = CString::new(text.replace('\0', " ")).unwrap_or_default();
            // SAFETY: the frontend's printf-style logger; our text is passed as the "%s" argument, never as the format.
            unsafe { f(level, c"%s".as_ptr(), c.as_ptr()) };
        }
        None => eprint!("{}", text),
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> &str {
    p.downcast_ref::<&str>().copied().or_else(|| p.downcast_ref::<String>().map(|s| s.as_str())).unwrap_or("non-string payload")
}

// Runs `f` with unwinding stopped at the FFI boundary; a panic poisons the core until retro_deinit.
fn guard<R>(name: &str, fallback: R, check_poison: bool, f: impl FnOnce() -> R) -> R {
    if check_poison && POISONED.load(Ordering::SeqCst) {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(p) => {
            POISONED.store(true, Ordering::SeqCst);
            log(RETRO_LOG_ERROR, &format!("panic in {}: {}; core disabled until retro_deinit", name, panic_text(&*p)));
            fallback
        }
    }
}

unsafe fn env_call(cmd: c_uint, data: *mut c_void) -> bool {
    match callbacks().env {
        // SAFETY: the frontend's environment callback, `data` matches what `cmd` expects.
        Some(f) => unsafe { f(cmd, data) },
        None => false,
    }
}

fn save_directory() -> Option<PathBuf> {
    let mut p: *const c_char = std::ptr::null();
    // SAFETY: GET_SAVE_DIRECTORY writes a `const char *` through the pointer.
    let ok = unsafe { env_call(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &mut p as *mut _ as *mut c_void) };
    if !ok || p.is_null() {
        return None;
    }
    // SAFETY: non-null NUL-terminated string owned by the frontend.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn system_directory() -> Option<PathBuf> {
    let mut p: *const c_char = std::ptr::null();
    // SAFETY: GET_SYSTEM_DIRECTORY writes a `const char *` through the pointer.
    let ok = unsafe { env_call(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, &mut p as *mut _ as *mut c_void) };
    if !ok || p.is_null() {
        return None;
    }
    // SAFETY: non-null NUL-terminated string owned by the frontend.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

// Shows a short on-screen message (SET_MESSAGE); the full text also goes to the log.
fn show_message(msg: &str) {
    let Ok(c) = CString::new(msg) else { return };
    let mut m = retro_message { msg: c.as_ptr(), frames: MESSAGE_FRAMES };
    // SAFETY: SET_MESSAGE reads a `retro_message` during the call; `c` outlives it.
    unsafe { env_call(RETRO_ENVIRONMENT_SET_MESSAGE, &mut m as *mut retro_message as *mut c_void) };
}

fn candidates(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    for name in names {
        let hit = |p: &&PathBuf| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name));
        out.extend(files.iter().filter(hit).cloned());
    }
    out
}

// Checks candidates in order, logs each verdict, stops at the first good image and returns the best usable one.
fn find_bios(dir: &Path, allow_mono4: bool) -> Result<PathBuf, String> {
    // With the Mono-IV option on, a good 490 image counts as usable; Mono-I images still rank first.
    let usable = |r: &bios::Report| r.usable() || (allow_mono4 && r.status != bios::Status::Error && r.board == Some(bios::Board::Mono4));
    let candidates = candidates(dir, BIOS_NAMES);
    if candidates.is_empty() {
        return Err(format!("no CD-i system ROM in {} (looked for {})", dir.display(), BIOS_NAMES.join(", ")));
    }
    let mut checked = Vec::new();
    for p in candidates {
        let (entry, img) = match Machine::read_bios(&p) {
            Ok(x) => x,
            Err(e) => {
                log(RETRO_LOG_WARN, &format!("BIOS {}: {}", p.display(), e));
                continue;
            }
        };
        let r = &img.report;
        let level = match r.rank() {
            3 => RETRO_LOG_INFO,
            0 => RETRO_LOG_ERROR,
            _ => RETRO_LOG_WARN,
        };
        let inside = entry.map_or_else(String::new, |e| format!(" ({})", e));
        log(level, &format!("BIOS {}{} [{}]: {}", p.display(), inside, r.status, r.message));
        let good = r.rank() == 3;
        checked.push((p, img));
        if good {
            break;
        }
    }
    let file = |p: &Path| p.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    match bios::best(checked) {
        Some((p, img)) if usable(&img.report) => {
            if img.report.status != bios::Status::Ok {
                show_message(&format!("CD-i BIOS {}: {}", file(&p), img.report.message));
            }
            Ok(p)
        }
        Some((p, img)) => Err(format!("no usable CD-i system ROM; best candidate {}: {}", file(&p), img.report.message)),
        None => Err(format!("no readable CD-i system ROM in {}", dir.display())),
    }
}

// Value of a core option, None when the frontend has no answer.
fn option_value(key: &CStr) -> Option<String> {
    let mut v = retro_variable { key: key.as_ptr(), value: std::ptr::null() };
    // SAFETY: GET_VARIABLE fills `value` with a frontend-owned string or leaves it null.
    let ok = unsafe { env_call(RETRO_ENVIRONMENT_GET_VARIABLE, &mut v as *mut retro_variable as *mut c_void) };
    // SAFETY: non-null NUL-terminated string owned by the frontend.
    (ok && !v.value.is_null()).then(|| unsafe { CStr::from_ptr(v.value) }.to_string_lossy().into_owned())
}

// True when the frontend reports changed core options since the last call.
fn options_updated() -> bool {
    let mut changed = false;
    // SAFETY: GET_VARIABLE_UPDATE writes one bool.
    let ok = unsafe { env_call(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, &mut changed as *mut bool as *mut c_void) };
    ok && changed
}

fn parse_cursor_speed(raw: Option<&str>, default: i32, min: i32, max: i32) -> i32 {
    raw.and_then(|s| s.trim().parse::<i32>().ok())
        .map_or(default, |v| v.clamp(min, max))
}

fn parse_boost_x10(raw: Option<&str>) -> i32 {
    match raw.map(str::trim) {
        Some("1.0") => 10,
        Some("1.5") => 15,
        Some("2.0") => 20,
        Some("2.5") => 25,
        Some("3.0") => 30,
        _ => input::DEFAULT_BOOST_X10,
    }
}

fn parse_crosshair(raw: Option<&str>) -> CrosshairMode {
    raw.and_then(CrosshairMode::from_str_opt).unwrap_or(CrosshairMode::Off)
}

fn resolve_shared_nvram_paths(sys_dir: &Path, mono4: bool) -> (PathBuf, PathBuf) {
    let primary_name = if mono4 { "opencdi_shared_nvram490.bin" } else { "opencdi_shared_nvram.bin" };
    let sys_fallback = if mono4 { "nvram_cdi490a.bin" } else { "nvram_mk48t08.bin" };
    let base_dir = save_directory().unwrap_or_else(|| sys_dir.to_path_buf());
    (base_dir.join(primary_name), sys_dir.join(sys_fallback))
}

fn load_shared_nvram_into_machine(m: &mut Machine, primary: &Path, fallback: &Path) {
    let src = if primary.is_file() {
        Some(primary)
    } else if fallback.is_file() {
        Some(fallback)
    } else {
        None
    };
    let Some(path) = src else { return };
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Some(m4) = m.bus.mono4.as_deref_mut() {
                m4.nvram.load_bytes(&bytes);
                m4.nvram.dirty = false;
            } else {
                let n = bytes.len().min(m.bus.nvram.ram.len());
                m.bus.nvram.ram[..n].copy_from_slice(&bytes[..n]);
                m.bus.nvram.dirty = false;
            }
            log(RETRO_LOG_INFO, &format!("seeded console NVRAM from {}", path.display()));
        }
        Err(e) => log(RETRO_LOG_WARN, &format!("cannot read shared NVRAM {}: {}", path.display(), e)),
    }
}

fn persist_shared_nvram(path: Option<&Path>, m: &mut Machine) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    let res = if let Some(m4) = m.bus.mono4.as_deref_mut() {
        let r = std::fs::write(path, &m4.nvram.ram);
        if r.is_ok() {
            m4.nvram.dirty = false;
        }
        r
    } else {
        let r = std::fs::write(path, &m.bus.nvram.ram[..]);
        if r.is_ok() {
            m.bus.nvram.dirty = false;
        }
        r
    };
    if let Err(e) = res {
        log(RETRO_LOG_WARN, &format!("cannot write shared NVRAM {}: {}", path.display(), e));
    }
}

fn read_cursor_config() -> input::CursorConfig {
    input::CursorConfig {
        dpad_speed_x: parse_cursor_speed(option_value(OPT_DPAD_SPEED_X).as_deref(), input::DEFAULT_DPAD_SPEED, 1, 12),
        dpad_speed_y: parse_cursor_speed(option_value(OPT_DPAD_SPEED_Y).as_deref(), input::DEFAULT_DPAD_SPEED, 1, 12),
        analog_speed_x: parse_cursor_speed(option_value(OPT_ANALOG_SPEED_X).as_deref(), input::DEFAULT_ANALOG_SPEED, 1, 16),
        analog_speed_y: parse_cursor_speed(option_value(OPT_ANALOG_SPEED_Y).as_deref(), input::DEFAULT_ANALOG_SPEED, 1, 16),
        boost_multiplier_x10: parse_boost_x10(option_value(OPT_CURSOR_BOOST).as_deref()),
    }
}

// The IMPEG cartridge for the 490 board; only the known dump is fitted.
fn find_impeg(dir: &Path) -> Option<PathBuf> {
    for p in candidates(dir, IMPEG_NAMES) {
        match Machine::read_dvc_for(&p, opencdi::bus::BoardKind::Mono4) {
            Ok((_, img)) if img.report.status == bios::Status::Ok => {
                log(RETRO_LOG_INFO, &format!("IMPEG {}: {}", p.display(), img.report.message));
                return Some(p);
            }
            Ok((_, img)) => log(RETRO_LOG_WARN, &format!("IMPEG {}: {}", p.display(), img.report.message)),
            Err(e) => log(RETRO_LOG_WARN, &format!("IMPEG {}: {}", p.display(), e)),
        }
    }
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !candidates(dir, DVC_NAMES).is_empty() && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log(RETRO_LOG_WARN, "a VMPEG cartridge ROM is present but does not fit the CD-i 490; it needs impega.rom");
    }
    log(RETRO_LOG_INFO, "no IMPEG cartridge ROM (impega.rom); Digital Video titles will ask for the cartridge");
    None
}

// Checks cartridge candidates in order and logs each verdict; only a known VMPEG dump is fitted.
fn find_dvc(dir: &Path) -> Option<PathBuf> {
    let candidates = candidates(dir, DVC_NAMES);
    if candidates.is_empty() {
        log(RETRO_LOG_INFO, &format!("no DVC cartridge ROM in {} (looked for {}); Digital Video titles will ask for the cartridge", dir.display(), DVC_NAMES.join(", ")));
        return None;
    }
    for p in candidates {
        match Machine::read_dvc(&p) {
            Ok((entry, img)) => {
                let r = &img.report;
                let level = if r.status == bios::Status::Ok { RETRO_LOG_INFO } else { RETRO_LOG_WARN };
                let inside = entry.map_or_else(String::new, |e| format!(" ({})", e));
                let skip = if r.status == bios::Status::Warning { "; not fitted, only the known VMPEG dump is used" } else { "" };
                log(level, &format!("DVC {}{} [{}]: {}{}", p.display(), inside, r.status, r.message, skip));
                if r.status == bios::Status::Ok {
                    return Some(p);
                }
            }
            Err(e) => log(RETRO_LOG_WARN, &format!("DVC {}: {}", p.display(), e)),
        }
    }
    show_message("No usable DVC cartridge ROM (vmpega.rom); Digital Video titles will not play");
    None
}

fn resolve_video_mode(region_opt: Option<&str>, path: Option<&Path>, machine: &Machine) -> VideoMode {
    match region_opt.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("pal") => VideoMode::Pal,
        Some("ntsc") => VideoMode::Ntsc,
        _ => {
            let Some(p) = path else { return VideoMode::Pal };
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if machine.bus.cdrom.as_ref().and_then(|cd| cd.is_ntsc_hint(&name)).unwrap_or(false) {
                VideoMode::Ntsc
            } else {
                VideoMode::Pal
            }
        }
    }
}

fn boot(spec: &LoadSpec) -> Result<Machine, String> {
    let mut m = Machine::new();
    m.bus.nvram.persist = false;
    m.load_bios(&spec.bios).map_err(|e| format!("cannot load BIOS {}: {}", spec.bios.display(), e))?;
    if let Some(p) = &spec.dvc {
        let (_, img) = Machine::read_dvc_for(p, m.bus.board).map_err(|e| format!("cannot load DVC ROM {}: {}", p.display(), e))?;
        if img.report.status != bios::Status::Ok {
            return Err(format!("DVC ROM {} changed: {}", p.display(), img.report.message));
        }
        m.load_dvc_image(&img);
    }
    if let Some(d) = &spec.disc {
        m.load_disc(d).map_err(|e| format!("cannot open disc {}: {}", d.display(), e))?;
    }
    m.set_video_mode(spec.mode);
    m.reset();
    m.bus.uart_capture = Some(Vec::new());
    Ok(m)
}

const UART_LINE_MAX: usize = 256;

// Sends complete BIOS UART lines to the log; control bytes other than tab are shown as [XX] like the stdout path.
fn drain_uart(bytes: &mut Vec<u8>, line: &mut Vec<u8>) {
    for b in bytes.drain(..) {
        match b {
            b'\n' => flush_uart_line(line),
            b'\r' => {}
            _ => {
                if b == b'\t' || (0x20..0x7f).contains(&b) {
                    line.push(b);
                } else {
                    line.extend_from_slice(format!("[{:02X}]", b).as_bytes());
                }
                if line.len() >= UART_LINE_MAX {
                    flush_uart_line(line);
                }
            }
        }
    }
}

fn flush_uart_line(line: &mut Vec<u8>) {
    if !line.is_empty() {
        log(RETRO_LOG_INFO, &format!("UART {}", String::from_utf8_lossy(line)));
        line.clear();
    }
}

fn spill_to_temp(data: &[u8]) -> Result<PathBuf, String> {
    let ext = if data.starts_with(b"MComprHD") { "chd" } else { "bin" };
    let p = std::env::temp_dir().join(format!("opencdi-libretro-{}.{}", std::process::id(), ext));
    std::fs::write(&p, data).map_err(|e| format!("cannot write {}: {}", p.display(), e))?;
    Ok(p)
}

fn fps(mode: VideoMode) -> f64 {
    opencdi::audio::frame_rate(mode)
}

fn region_of(mode: VideoMode) -> c_uint {
    match mode {
        VideoMode::Ntsc => RETRO_REGION_NTSC,
        VideoMode::Pal => RETRO_REGION_PAL,
    }
}

fn current_mode() -> VideoMode {
    lock(&CORE).machine.as_ref().map_or(VideoMode::Pal, |m| m.bus.mcd212.video_mode)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_api_version() -> c_uint {
    guard("retro_api_version", RETRO_API_VERSION, false, || RETRO_API_VERSION)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_environment(cb: retro_environment_t) {
    guard("retro_set_environment", (), false, || {
        lock(&CALLBACKS).env = cb;
        let mut no_game = true;
        // SAFETY: SET_SUPPORT_NO_GAME reads a `bool`.
        unsafe { env_call(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &mut no_game as *mut bool as *mut c_void) };
        let mut lcb = retro_log_callback { log: None };
        // SAFETY: GET_LOG_INTERFACE fills a `retro_log_callback`.
        let ok = unsafe { env_call(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &mut lcb as *mut _ as *mut c_void) };
        // A later call without a log interface must not drop the one already received.
        if ok && lcb.log.is_some() {
            *lock(&LOG) = lcb.log;
        }
        // SAFETY: SET_CONTROLLER_INFO reads a terminated `retro_controller_info` array; it is static.
        unsafe { env_call(RETRO_ENVIRONMENT_SET_CONTROLLER_INFO, input::CONTROLLER_INFO.0.as_ptr() as *mut c_void) };
        let mut version: c_uint = 0;
        // SAFETY: GET_CORE_OPTIONS_VERSION writes an `unsigned`.
        let v1 = unsafe { env_call(RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, &mut version as *mut c_uint as *mut c_void) } && version >= 1;
        // SAFETY: both calls read a terminated static array during the call.
        if !v1 || !unsafe { env_call(RETRO_ENVIRONMENT_SET_CORE_OPTIONS, OPTIONS.0.as_ptr() as *mut c_void) } {
            unsafe { env_call(RETRO_ENVIRONMENT_SET_VARIABLES, VARIABLES.0.as_ptr() as *mut c_void) };
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_video_refresh(cb: retro_video_refresh_t) {
    guard("retro_set_video_refresh", (), false, || lock(&CALLBACKS).video = cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample(cb: retro_audio_sample_t) {
    guard("retro_set_audio_sample", (), false, || lock(&CALLBACKS).audio_sample = cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample_batch(cb: retro_audio_sample_batch_t) {
    guard("retro_set_audio_sample_batch", (), false, || lock(&CALLBACKS).audio_batch = cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_poll(cb: retro_input_poll_t) {
    guard("retro_set_input_poll", (), false, || lock(&CALLBACKS).input_poll = cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_state(cb: retro_input_state_t) {
    guard("retro_set_input_state", (), false, || lock(&CALLBACKS).input_state = cb)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_init() {
    guard("retro_init", (), false, || {
        log(RETRO_LOG_INFO, concat!("core ", env!("CARGO_PKG_VERSION"), " initialised"));
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_deinit() {
    guard("retro_deinit", (), false, || {
        unload();
        POISONED.store(false, Ordering::SeqCst);
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_info(info: *mut retro_system_info) {
    guard("retro_get_system_info", (), false, || {
        if info.is_null() {
            return;
        }
        let v = retro_system_info {
            library_name: c"OpenCDI".as_ptr(),
            library_version: concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char,
            valid_extensions: c"cue|chd|bin|iso".as_ptr(),
            need_fullpath: true,
            block_extract: false,
        };
        // SAFETY: frontend passes a writable `retro_system_info`.
        unsafe { info.write(v) };
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut retro_system_av_info) {
    guard("retro_get_system_av_info", (), true, || {
        if info.is_null() {
            return;
        }
        let mode = current_mode();
        let v = retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: SCREEN_WIDTH as c_uint,
                base_height: SCREEN_HEIGHT as c_uint,
                max_width: SCREEN_WIDTH as c_uint,
                max_height: SCREEN_HEIGHT as c_uint,
                aspect_ratio: 4.0 / 3.0,
            },
            timing: retro_system_timing { fps: fps(mode), sample_rate: AUDIO_RATE as f64 },
        };
        // SAFETY: frontend passes a writable `retro_system_av_info`.
        unsafe { info.write(v) };
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_controller_port_device(port: c_uint, device: c_uint) {
    guard("retro_set_controller_port_device", (), true, || {
        if port == 0 && input::supported(device) {
            lock(&CORE).input.device = device;
        } else {
            log(RETRO_LOG_WARN, &format!("ignoring device {} on port {} (port 0: RetroPad, mouse or pointer)", device, port));
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    guard("retro_reset", (), true, || {
        let mut core = lock(&CORE);
        let Some(spec) = core.spec.clone() else { return };
        match boot(&spec) {
            Ok(mut m) => {
                if let Some(old) = core.machine.as_mut() {
                    m.bus.nvram.ram = old.bus.nvram.ram; // reset keeps the battery-backed RAM the frontend saves
                    if let (Some(n), Some(o)) = (m.bus.mono4.as_deref_mut(), old.bus.mono4.as_deref_mut()) {
                        std::mem::swap(&mut n.nvram.ram, &mut o.nvram.ram); // same buffer, so the frontend's save RAM pointer stays valid
                    }
                }
                core.machine = Some(m);
                core.input.reset();
                core.deinterlacer.reset();
                core.frames_out = 0;
                core.rtc_seeded = false;
            }
            Err(e) => log(RETRO_LOG_ERROR, &format!("reset failed: {}", e)),
        }
    })
}

// Sends the frames the emulation made this frame, hard-clipped to 16 bits.
fn send_audio(core: &mut Core, batch: retro_audio_sample_batch_t) {
    let Core { machine, mix, pcm, frames_out, .. } = core;
    let Some(m) = machine.as_mut() else { return };
    mix.clear();
    *frames_out += m.drain_audio(mix) as u64;
    let Some(f) = batch else { return };
    pcm.clear();
    pcm.extend(mix.iter().flat_map(|&(l, r)| [audio::to_i16(l), audio::to_i16(r)]));
    let mut done = 0;
    let frames = pcm.len() / 2;
    while done < frames {
        // SAFETY: `pcm` holds `frames` interleaved stereo frames and outlives the call.
        let n = unsafe { f(pcm.as_ptr().add(done * 2), frames - done) };
        if n == 0 {
            break;
        }
        done += n;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_run() {
    guard("retro_run", (), true, || {
        #[cfg(test)]
        if PANIC_IN_RUN.swap(false, Ordering::SeqCst) {
            panic!("test hook in retro_run");
        }
        let cb = callbacks();
        if let Some(poll) = cb.input_poll {
            // SAFETY: frontend callback, no arguments.
            unsafe { poll() };
        }
        let mut core = lock(&CORE);
        let Core {
            machine,
            input,
            uart_line,
            deinterlace,
            deinterlacer,
            crosshair,
            present_fb,
            shared_nvram_mode,
            shared_nvram_path,
            rtc_seeded,
            ..
        } = &mut *core;
        let Some(m) = machine.as_mut() else { return };
        if options_updated() {
            opencdi::mcd212::set_studio_levels(option_value(OPT_LEVELS).as_deref() != Some("full"));
            let new_deint = option_value(OPT_DEINTERLACE).as_deref() == Some("on");
            if new_deint != *deinterlace {
                *deinterlace = new_deint;
                deinterlacer.reset();
            }
            *crosshair = parse_crosshair(option_value(OPT_CROSSHAIR).as_deref());
            input.config = read_cursor_config();
        }
        // Seed host local time once on frame 0 (after frontend SAVE_RAM overlay, before frame 0 executes).
        // Savestate capture/restore runs at frame >= 1 so determinism and --roundtrip stay intact.
        if !*rtc_seeded {
            m.bus.seed_rtc_from_host();
            *rtc_seeded = true;
        }
        let speed_changed = input.apply(cb.input_state, &mut m.bus);
        m.run_frame();
        if *shared_nvram_mode && m.bus.current_frame % 300 == 0 {
            let dirty = m.bus.mono4.as_deref().map_or(m.bus.nvram.dirty, |m4| m4.nvram.dirty);
            if dirty {
                persist_shared_nvram(shared_nvram_path.as_deref(), m);
            }
        }
        if let Some(buf) = m.bus.uart_capture.as_mut() {
            drain_uart(buf, uart_line);
        }
        if let Some(video) = cb.video {
            let (interlaced, odd_field, y_offset) = m.bus.mcd212.display_interlace_info();
            let hw_cursor_visible = m.bus.mcd212.hardware_cursor_visible();
            let fb = deinterlacer.process(m.framebuffer(), interlaced, odd_field, y_offset, *deinterlace);
            let out_ptr = if crosshair.should_draw(hw_cursor_visible) {
                present_fb.clear();
                present_fb.extend_from_slice(&fb[..SCREEN_WIDTH * SCREEN_HEIGHT]);
                let (cx, cy) = input.position();
                draw_crosshair(present_fb, SCREEN_WIDTH, SCREEN_HEIGHT, cx, cy);
                present_fb.as_ptr()
            } else {
                fb.as_ptr()
            };
            // SAFETY: `out_ptr` points to SCREEN_WIDTH * SCREEN_HEIGHT XRGB8888 pixels valid for this call.
            unsafe { video(out_ptr as *const c_void, SCREEN_WIDTH as c_uint, SCREEN_HEIGHT as c_uint, SCREEN_WIDTH * 4) };
        }
        send_audio(&mut core, cb.audio_batch);
        drop(core);
        if let Some(speed) = speed_changed {
            show_message(&format!("CD-i Pad Speed: {}", speed.label()));
        }
    })
}

// Fills `core.state_buf` with the machine state plus the input translator's state.
fn save_state(core: &mut Core) -> bool {
    let Core { machine, input, state_buf, .. } = core;
    let Some(m) = machine.as_ref() else { return false };
    let mut fe = Vec::new();
    input.save(&mut fe);
    m.save_state(&fe, state_buf);
    true
}

// Fixed per loaded game (rewind and runahead need a stable size): first measured size plus headroom.
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    guard("retro_serialize_size", 0, true, || {
        let mut core = lock(&CORE);
        if core.state_size == 0 && save_state(&mut core) {
            core.state_size = (core.state_buf.len() + STATE_HEADROOM).div_ceil(STATE_ALIGN) * STATE_ALIGN;
            log(RETRO_LOG_INFO, &format!("savestate size {} bytes ({} used now)", core.state_size, core.state_buf.len()));
        }
        core.state_size
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize(data: *mut c_void, len: usize) -> bool {
    guard("retro_serialize", false, true, || {
        if data.is_null() {
            return false;
        }
        let mut core = lock(&CORE);
        if !save_state(&mut core) {
            return false;
        }
        let n = core.state_buf.len();
        if n > len {
            log(RETRO_LOG_ERROR, &format!("savestate needs {} bytes, frontend buffer has {}", n, len));
            return false;
        }
        // SAFETY: the frontend passes a writable buffer of `len` bytes.
        let out = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, len) };
        out[..n].copy_from_slice(&core.state_buf);
        out[n..].fill(0);
        true
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_unserialize(data: *const c_void, len: usize) -> bool {
    guard("retro_unserialize", false, true, || {
        if data.is_null() {
            return false;
        }
        // SAFETY: the frontend passes a readable buffer of `len` bytes.
        let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, len) };
        let mut core = lock(&CORE);
        let Core { machine, input, .. } = &mut *core;
        let Some(m) = machine.as_mut() else { return false };
        let fe = match m.load_state(bytes) {
            Ok(fe) => fe,
            Err(e) => {
                log(RETRO_LOG_WARN, &format!("savestate rejected: {}", e));
                return false;
            }
        };
        let mut r = Reader::new(fe);
        if let Err(e) = input.load(&mut r) {
            log(RETRO_LOG_WARN, &format!("savestate input section: {}", e));
            input.reset();
            return false;
        }
        true
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_reset() {
    guard("retro_cheat_reset", (), true, || {})
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_set(_index: c_uint, _enabled: bool, _code: *const c_char) {
    guard("retro_cheat_set", (), true, || {})
}

fn load(game: *const retro_game_info) -> Result<(), String> {
    unload();
    let dir = system_directory().ok_or("frontend gave no system directory")?;
    opencdi::mcd212::set_studio_levels(option_value(OPT_LEVELS).as_deref() != Some("full"));
    let allow_mono4 = option_value(OPT_MONO4).as_deref() == Some("on");
    let bios = find_bios(&dir, allow_mono4).inspect_err(|e| show_message(e))?;
    let mut temp = None;
    let disc = if game.is_null() {
        None
    } else {
        // SAFETY: non-null `retro_game_info` from the frontend, valid for this call.
        let g = unsafe { &*game };
        if !g.path.is_null() && unsafe { *g.path } != 0 {
            // SAFETY: non-null NUL-terminated path string.
            Some(PathBuf::from(unsafe { CStr::from_ptr(g.path) }.to_string_lossy().into_owned()))
        } else if !g.data.is_null() && g.size > 0 {
            // SAFETY: frontend buffer of `size` bytes.
            let bytes = unsafe { std::slice::from_raw_parts(g.data as *const u8, g.size) };
            let p = spill_to_temp(bytes)?;
            temp = Some(p.clone());
            Some(p)
        } else {
            None
        }
    };
    let mut fmt = RETRO_PIXEL_FORMAT_XRGB8888;
    // SAFETY: SET_PIXEL_FORMAT reads an `enum retro_pixel_format` (int-sized).
    if !unsafe { env_call(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &mut fmt as *mut c_uint as *mut c_void) } {
        return Err("frontend refused XRGB8888".into());
    }
    // SAFETY: SET_INPUT_DESCRIPTORS reads a NULL-terminated static descriptor array.
    unsafe { env_call(RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS, input::DESCRIPTORS.0.as_ptr() as *mut c_void) };
    let mono4 = Machine::read_bios(&bios).is_ok_and(|(_, img)| img.report.board == Some(bios::Board::Mono4));
    let dvc = if option_value(OPT_DVC).as_deref() == Some("off") {
        log(RETRO_LOG_INFO, "DVC cartridge off (core option)");
        None
    } else if mono4 {
        find_impeg(&dir)
    } else {
        find_dvc(&dir)
    };
    let region_opt = option_value(OPT_REGION);
    let initial_mode = resolve_video_mode(region_opt.as_deref(), None, &Machine::new());
    let mut spec = LoadSpec { bios, dvc, disc, mode: initial_mode };
    let mut m = boot(&spec)?;
    let target_mode = resolve_video_mode(region_opt.as_deref(), spec.disc.as_deref(), &m);
    if target_mode != spec.mode {
        spec.mode = target_mode;
        m = boot(&spec)?;
    }
    let shared_nvram_mode = option_value(OPT_NVRAM_MODE).as_deref() == Some("shared");
    let (shared_primary, shared_fallback) = resolve_shared_nvram_paths(&dir, mono4);
    load_shared_nvram_into_machine(&mut m, &shared_primary, &shared_fallback);
    let is_no_game = spec.disc.is_none();
    log(RETRO_LOG_INFO, &format!(
        "BIOS {}, DVC {}, disc {}, {:?}, nvram_mode={}",
        spec.bios.display(),
        spec.dvc.as_ref().map_or("none".into(), |d| d.display().to_string()),
        spec.disc.as_ref().map_or("none".into(), |d| d.display().to_string()),
        spec.mode,
        if shared_nvram_mode { "shared" } else { "per_game" }
    ));
    let deinterlace = option_value(OPT_DEINTERLACE).as_deref() == Some("on");
    let crosshair = parse_crosshair(option_value(OPT_CROSSHAIR).as_deref());
    let cursor_config = read_cursor_config();
    let mut core = lock(&CORE);
    core.machine = Some(m);
    core.spec = Some(spec);
    core.temp_disc = temp;
    core.input.reset();
    core.input.config = cursor_config;
    core.deinterlace = deinterlace;
    core.deinterlacer.reset();
    core.crosshair = crosshair;
    core.shared_nvram_mode = shared_nvram_mode;
    core.shared_nvram_path = Some(shared_primary);
    core.is_no_game = is_no_game;
    core.rtc_seeded = false;
    core.frames_out = 0;
    core.state_size = 0;
    Ok(())
}

fn unload() {
    let mut core = lock(&CORE);
    flush_uart_line(&mut core.uart_line);
    let persist_target = if core.shared_nvram_mode || core.is_no_game {
        core.shared_nvram_path.clone()
    } else {
        None
    };
    let frames_out = core.frames_out;
    if let Some(m) = core.machine.as_mut() {
        if let Some(ref p) = persist_target {
            persist_shared_nvram(Some(p.as_path()), m);
        }
        let a = &m.bus.cdic.audio;
        log(RETRO_LOG_INFO, &format!(
            "audio: {} frames sent; received main={} sfx={} mpeg={}; prefill restarts main={} sfx={} mpeg={}; hold warnings main={} sfx={} mpeg={}",
            frames_out, a.main.frames_in, a.sfx.frames_in, a.mpeg.frames_in, a.main.restarts, a.sfx.restarts, a.mpeg.restarts, a.main.warnings, a.sfx.warnings, a.mpeg.warnings
        ));
    }
    core.machine = None;
    core.spec = None;
    core.shared_nvram_path = None;
    core.shared_nvram_mode = false;
    core.is_no_game = false;
    core.rtc_seeded = false;
    core.state_size = 0;
    core.state_buf = Vec::new();
    core.present_fb.clear();
    core.deinterlacer.reset();
    if let Some(p) = core.temp_disc.take() {
        let _ = std::fs::remove_file(p);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_load_game(game: *const retro_game_info) -> bool {
    guard("retro_load_game", false, true, || match load(game) {
        Ok(()) => true,
        Err(e) => {
            log(RETRO_LOG_ERROR, &format!("load failed: {}", e));
            unload();
            false
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game_special(_type: c_uint, _info: *const retro_game_info, _num: usize) -> bool {
    guard("retro_load_game_special", false, true, || false)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_unload_game() {
    guard("retro_unload_game", (), true, unload)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    guard("retro_get_region", RETRO_REGION_PAL, true, || region_of(current_mode()))
}

// SAVE_RAM is the NVRAM (8 KB, or 32 KB on the 490), kept by the frontend as a .srm per game; SYSTEM_RAM is plane A (CPU 0x000000-0x07FFFF, big-endian bytes).
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    guard("retro_get_memory_data", std::ptr::null_mut(), true, || {
        let mut core = lock(&CORE);
        let shared = core.shared_nvram_mode;
        match (id, core.machine.as_mut()) {
            (RETRO_MEMORY_SAVE_RAM, _) if shared => std::ptr::null_mut(),
            (RETRO_MEMORY_SAVE_RAM, Some(m)) => match m.bus.mono4.as_deref_mut() {
                Some(m4) => m4.nvram.ram.as_mut_ptr() as *mut c_void,
                None => m.bus.nvram.ram.as_mut_ptr() as *mut c_void,
            },
            (RETRO_MEMORY_SYSTEM_RAM, Some(m)) => m.bus.mcd212.plane_a.as_mut_ptr() as *mut c_void,
            _ => std::ptr::null_mut(),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(id: c_uint) -> usize {
    guard("retro_get_memory_size", 0, true, || {
        let core = lock(&CORE);
        match (id, core.machine.as_ref()) {
            (RETRO_MEMORY_SAVE_RAM, _) if core.shared_nvram_mode => 0,
            (RETRO_MEMORY_SAVE_RAM, Some(m)) => m.bus.mono4.as_deref().map_or(m.bus.nvram.ram.len(), |m4| m4.nvram.ram.len()),
            (RETRO_MEMORY_SYSTEM_RAM, Some(m)) => m.bus.mcd212.plane_a.len(),
            _ => 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    // Synthetic Mono-I style image (reset vector plus filler), never real ROM bytes.
    fn synthetic_rom() -> Vec<u8> {
        let mut d: Vec<u8> = (0..bios::SYSTEM_ROM_SIZE as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8 | 1).collect();
        d[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        d
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("opencdi-bios-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn find_bios_skips_bad_images_and_matches_names_case_insensitively() {
        let d = temp_dir("pick");
        assert!(find_bios(&d, false).unwrap_err().contains("no CD-i system ROM"));
        std::fs::write(d.join("cdi220b.rom"), vec![0xFFu8; 1000]).unwrap();
        assert!(find_bios(&d, false).unwrap_err().contains("no usable"));
        let mut swapped = synthetic_rom();
        bios::swap_words(&mut swapped);
        std::fs::write(d.join("CDI.ROM"), &swapped).unwrap();
        assert_eq!(find_bios(&d, false).unwrap(), d.join("CDI.ROM"));
        let mut m = Machine::new();
        let r = m.load_bios(d.join("CDI.ROM")).unwrap();
        assert!(r.word_swapped);
        assert_eq!(m.bus.rom[..], synthetic_rom()[..]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn find_dvc_fits_only_a_known_vmpega_dump() {
        let d = temp_dir("dvc");
        assert_eq!(find_dvc(&d), None);
        std::fs::write(d.join("VMPEGA.ROM"), &synthetic_rom()[..bios::DVC_ROM_SIZE]).unwrap();
        assert_eq!(find_dvc(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn guard_catches_panic_and_returns_fallback() {
        let _s = lock(&SERIAL);
        let v = guard("test", 7u32, false, || -> u32 { panic!("boom") });
        assert_eq!(v, 7);
        POISONED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn panic_in_retro_run_is_caught_and_poisons() {
        let _s = lock(&SERIAL);
        PANIC_IN_RUN.store(true, Ordering::SeqCst);
        retro_run();
        assert!(POISONED.load(Ordering::SeqCst), "panic should poison the core");
        // SAFETY: null game is the no-game path; poisoned core returns false before touching it.
        assert!(!unsafe { retro_load_game(std::ptr::null()) });
        assert_eq!(retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM), 0);
        retro_deinit();
        assert!(!POISONED.load(Ordering::SeqCst), "retro_deinit clears the poison");
    }

    #[test]
    fn static_ffi_tables_are_null_terminated() {
        assert!(OPTIONS.0.last().unwrap().key.is_null(), "OPTIONS must end with null terminator");
        assert!(VARIABLES.0.last().unwrap().key.is_null(), "VARIABLES must end with null terminator");
        assert!(input::DESCRIPTORS.0.last().unwrap().description.is_null(), "DESCRIPTORS must end with null terminator");
        assert!(input::CONTROLLER_INFO.0.last().unwrap().types.is_null(), "CONTROLLER_INFO must end with null terminator");
    }

    #[test]
    fn cursor_speed_and_boost_options_parse_and_clamp() {
        assert_eq!(parse_cursor_speed(None, 3, 1, 12), 3);
        assert_eq!(parse_cursor_speed(Some("0"), 3, 1, 12), 1);
        assert_eq!(parse_cursor_speed(Some("99"), 6, 1, 16), 16);
        assert_eq!(parse_cursor_speed(Some(" 8 "), 6, 1, 16), 8);
        assert_eq!(parse_boost_x10(Some("1.0")), 10);
        assert_eq!(parse_boost_x10(Some("1.5")), 15);
        assert_eq!(parse_boost_x10(Some("3.0")), 30);
        assert_eq!(parse_boost_x10(None), 20);
        assert_eq!(parse_crosshair(Some("auto")), CrosshairMode::Auto);
        assert_eq!(parse_crosshair(Some("always")), CrosshairMode::Always);
        assert_eq!(parse_crosshair(None), CrosshairMode::Off);
    }

    #[test]
    fn region_option_overrides_filename_auto_detection() {
        let m = Machine::new();
        let usa = Path::new("Hotel Mario (USA).chd");
        let unk = Path::new("hotel_mario.chd");
        assert_eq!(resolve_video_mode(Some("pal"), Some(usa), &m), VideoMode::Pal);
        assert_eq!(resolve_video_mode(Some("ntsc"), Some(unk), &m), VideoMode::Ntsc);
        assert_eq!(resolve_video_mode(Some("auto"), Some(unk), &m), VideoMode::Pal);
    }

    #[test]
    fn shared_nvram_seeds_new_games_and_hides_sram_in_shared_mode() {
        let sys_dir = temp_dir("nvram_sys");
        let save_dir = temp_dir("nvram_save");
        let _ = save_dir;
        let (primary, fallback) = resolve_shared_nvram_paths(&sys_dir, false);
        let mut seed = vec![0u8; 8192];
        seed[42] = 0x7E;
        std::fs::write(&fallback, &seed).unwrap();

        let mut m = Machine::new();
        load_shared_nvram_into_machine(&mut m, &primary, &fallback);
        assert_eq!(m.bus.nvram.ram[42], 0x7E);

        m.bus.nvram.ram[43] = 0xA5;
        persist_shared_nvram(Some(&primary), &mut m);
        assert!(primary.is_file());
        let saved = std::fs::read(&primary).unwrap();
        assert_eq!(saved[43], 0xA5);

        let _s = lock(&SERIAL);
        {
            let mut c = lock(&CORE);
            c.machine = Some(m);
            c.shared_nvram_mode = true;
        }
        assert!(retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).is_null());
        assert_eq!(retro_get_memory_size(RETRO_MEMORY_SAVE_RAM), 0);
        {
            let mut c = lock(&CORE);
            c.machine = None;
            c.shared_nvram_mode = false;
        }
        let _ = std::fs::remove_dir_all(&sys_dir);
    }

    #[cfg(unix)]
    extern "C" fn unguarded() {
        panic!("unguarded panic in extern \"C\"");
    }

    #[cfg(unix)]
    #[test]
    fn unguarded_extern_c_panic_aborts() {
        use std::os::unix::process::ExitStatusExt;
        if std::env::var_os("OPENCDI_ABORT_CHILD").is_some() {
            unguarded();
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::unguarded_extern_c_panic_aborts", "--nocapture", "--test-threads=1"])
            .env("OPENCDI_ABORT_CHILD", "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.signal(), Some(6), "expected SIGABRT, got {:?}", out.status);
        assert!(err.contains("panic in a function that cannot unwind"), "stderr: {}", err);
    }
}
