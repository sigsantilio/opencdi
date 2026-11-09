mod input;
pub mod libretro;

use input::{Input, Table};
use libretro::*;
use opencdi::audio;
use opencdi::bios;
use opencdi::machine::Machine;
use opencdi::mcd212::{VideoMode, SCREEN_HEIGHT, SCREEN_WIDTH};
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
const OPT_LEVELS: &CStr = c"opencdi_video_levels";
const NO_VALUE: retro_core_option_value = retro_core_option_value { value: std::ptr::null(), label: std::ptr::null() };

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
static OPTIONS: Table<[retro_core_option_definition; 4]> = Table([
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
        key: OPT_LEVELS.as_ptr(),
        desc: c"Video levels".as_ptr(),
        info: c"studio passes the CD-i video levels through unchanged (black 16, white 235), as the hardware outputs them. full stretches them to 0-255 (black 0), as PC displays expect. Takes effect at once.".as_ptr(),
        values: option_values(&[(c"studio", c"studio (black 16)"), (c"full", c"full (black 0)")]),
        default_value: c"studio".as_ptr(),
    },
    retro_core_option_definition { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null(), values: [NO_VALUE; RETRO_NUM_CORE_OPTION_VALUES_MAX], default_value: std::ptr::null() },
]);
static VARIABLES: Table<[retro_variable; 4]> = Table([
    retro_variable { key: OPT_DVC.as_ptr(), value: c"DVC cartridge (restart); auto|off".as_ptr() },
    retro_variable { key: OPT_MONO4.as_ptr(), value: c"CD-i 490 BIOS, experimental (restart); off|on".as_ptr() },
    retro_variable { key: OPT_LEVELS.as_ptr(), value: c"Video levels; studio|full".as_ptr() },
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

fn is_ntsc_name(path: &Path, machine: &Machine) -> bool {
    let name = path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
    (name.contains("(USA)") || name.contains("(JAPAN)") || name.contains("NTSC"))
        && machine.bus.cdrom.as_ref().is_none_or(|cd| cd.disc_type != 0x20)
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
                core.frames_out = 0;
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
        let Core { machine, input, uart_line, .. } = &mut *core;
        let Some(m) = machine.as_mut() else { return };
        if options_updated() {
            opencdi::mcd212::set_studio_levels(option_value(OPT_LEVELS).as_deref() != Some("full"));
        }
        input.apply(cb.input_state, &mut m.bus);
        m.run_frame();
        if let Some(buf) = m.bus.uart_capture.as_mut() {
            drain_uart(buf, uart_line);
        }
        if let Some(video) = cb.video {
            let fb = m.framebuffer();
            // SAFETY: `fb` is SCREEN_WIDTH * SCREEN_HEIGHT XRGB8888 pixels, valid for the duration of the call.
            unsafe { video(fb.as_ptr() as *const c_void, SCREEN_WIDTH as c_uint, SCREEN_HEIGHT as c_uint, SCREEN_WIDTH * 4) };
        }
        send_audio(&mut core, cb.audio_batch);
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
    let mut spec = LoadSpec { bios, dvc, disc, mode: VideoMode::Pal };
    let mut m = boot(&spec)?;
    if let Some(d) = &spec.disc {
        if is_ntsc_name(d, &m) {
            spec.mode = VideoMode::Ntsc;
            m = boot(&spec)?;
        }
    }
    log(RETRO_LOG_INFO, &format!(
        "BIOS {}, DVC {}, disc {}, {:?}",
        spec.bios.display(),
        spec.dvc.as_ref().map_or("none".into(), |d| d.display().to_string()),
        spec.disc.as_ref().map_or("none".into(), |d| d.display().to_string()),
        spec.mode
    ));
    let mut core = lock(&CORE);
    core.machine = Some(m);
    core.spec = Some(spec);
    core.temp_disc = temp;
    core.input.reset();
    core.frames_out = 0;
    core.state_size = 0;
    Ok(())
}

fn unload() {
    let mut core = lock(&CORE);
    flush_uart_line(&mut core.uart_line);
    if let Some(m) = core.machine.as_ref() {
        let a = &m.bus.cdic.audio;
        log(RETRO_LOG_INFO, &format!(
            "audio: {} frames sent; received main={} sfx={} mpeg={}; prefill restarts main={} sfx={} mpeg={}; hold warnings main={} sfx={} mpeg={}",
            core.frames_out, a.main.frames_in, a.sfx.frames_in, a.mpeg.frames_in, a.main.restarts, a.sfx.restarts, a.mpeg.restarts, a.main.warnings, a.sfx.warnings, a.mpeg.warnings
        ));
    }
    core.machine = None;
    core.spec = None;
    core.state_size = 0;
    core.state_buf = Vec::new();
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
        match (id, core.machine.as_mut()) {
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
