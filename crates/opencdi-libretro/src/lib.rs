pub mod libretro;

use libretro::*;
use opencdi::machine::Machine;
use opencdi::mcd212::{VideoMode, SCREEN_HEIGHT, SCREEN_WIDTH};
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

// Looked up in the frontend's system directory, in this order (cdi490a.rom is left out until Mono-IV exists).
pub const BIOS_NAMES: &[&str] = &["cdi220b.rom", "cdi220.rom", "cdi200.rom", "cdi.rom"];
const AUDIO_RATE: u32 = 37800;

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
    disc: Option<PathBuf>,
    mode: VideoMode,
}

struct Core {
    machine: Option<Machine>,
    spec: Option<LoadSpec>,
    temp_disc: Option<PathBuf>,
    mix: Vec<(f32, f32)>,
    sfx: Vec<(f32, f32)>,
    pcm: Vec<i16>,
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
    sfx: Vec::new(),
    pcm: Vec::new(),
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

// Mono-I BIOS images start with the initial SSP 0x00001500; a byte-swapped dump reads 0x00000015.
const BIOS_FIRST_LONG: u32 = 0x0000_1500;

fn first_long(p: &Path) -> Option<u32> {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p).ok()?.read_exact(&mut b).ok()?;
    Some(u32::from_be_bytes(b))
}

fn find_bios(dir: &Path) -> Option<PathBuf> {
    for p in BIOS_NAMES.iter().map(|n| dir.join(n)).filter(|p| p.is_file()) {
        match first_long(&p) {
            Some(BIOS_FIRST_LONG) => return Some(p),
            v => log(RETRO_LOG_WARN, &format!(
                "skipping {}: first long {}, expected {:08x} (byte-swapped or not a CD-i BIOS?)",
                p.display(), v.map_or("unreadable".into(), |x| format!("{:08x}", x)), BIOS_FIRST_LONG
            )),
        }
    }
    None
}

fn is_ntsc_name(path: &Path, machine: &Machine) -> bool {
    let name = path.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
    (name.contains("(USA)") || name.contains("(JAPAN)") || name.contains("NTSC"))
        && machine.bus.cdrom.as_ref().is_none_or(|cd| cd.disc_type != 0x20)
}

fn boot(spec: &LoadSpec) -> Result<Machine, String> {
    let mut m = Machine::new();
    m.load_bios(&spec.bios).map_err(|e| format!("cannot load BIOS {}: {}", spec.bios.display(), e))?;
    if let Some(d) = &spec.disc {
        m.load_disc(d).map_err(|e| format!("cannot open disc {}: {}", d.display(), e))?;
    }
    m.set_video_mode(spec.mode);
    m.reset();
    Ok(m)
}

fn spill_to_temp(data: &[u8]) -> Result<PathBuf, String> {
    let ext = if data.starts_with(b"MComprHD") { "chd" } else { "bin" };
    let p = std::env::temp_dir().join(format!("opencdi-libretro-{}.{}", std::process::id(), ext));
    std::fs::write(&p, data).map_err(|e| format!("cannot write {}: {}", p.display(), e))?;
    Ok(p)
}

fn fps(mode: VideoMode) -> f64 {
    1.0 / mode.frame_duration().as_secs_f64()
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
        *lock(&LOG) = if ok { lcb.log } else { None };
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
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {
    guard("retro_set_controller_port_device", (), true, || {})
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    guard("retro_reset", (), true, || {
        let mut core = lock(&CORE);
        let Some(spec) = core.spec.clone() else { return };
        match boot(&spec) {
            Ok(m) => core.machine = Some(m),
            Err(e) => log(RETRO_LOG_ERROR, &format!("reset failed: {}", e)),
        }
    })
}

fn send_audio(core: &mut Core, batch: retro_audio_sample_batch_t) {
    let Core { machine, mix, sfx, pcm, .. } = core;
    let Some(m) = machine.as_mut() else { return };
    let audio = &m.bus.cdic.audio;
    mix.clear();
    sfx.clear();
    mix.extend(lock(&audio.sample_queue).drain(..));
    sfx.extend(lock(&audio.sfx_queue).drain(..));
    if mix.len() < sfx.len() {
        mix.resize(sfx.len(), (0.0, 0.0));
    }
    for (a, b) in mix.iter_mut().zip(sfx.iter()) {
        a.0 += b.0;
        a.1 += b.1;
    }
    let Some(f) = batch else { return };
    pcm.clear();
    let to_i16 = |x: f32| (x.clamp(-1.0, 1.0) * 32767.0) as i16;
    pcm.extend(mix.iter().flat_map(|&(l, r)| [to_i16(l), to_i16(r)]));
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
        let Some(m) = core.machine.as_mut() else { return };
        m.run_frame();
        if let Some(video) = cb.video {
            let fb = m.framebuffer();
            // SAFETY: `fb` is SCREEN_WIDTH * SCREEN_HEIGHT XRGB8888 pixels, valid for the duration of the call.
            unsafe { video(fb.as_ptr() as *const c_void, SCREEN_WIDTH as c_uint, SCREEN_HEIGHT as c_uint, SCREEN_WIDTH * 4) };
        }
        send_audio(&mut core, cb.audio_batch);
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    guard("retro_serialize_size", 0, true, || 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize(_data: *mut c_void, _len: usize) -> bool {
    guard("retro_serialize", false, true, || false)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_unserialize(_data: *const c_void, _len: usize) -> bool {
    guard("retro_unserialize", false, true, || false)
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
    let bios = find_bios(&dir).ok_or_else(|| format!("no BIOS in {} (looked for {})", dir.display(), BIOS_NAMES.join(", ")))?;
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
    let mut spec = LoadSpec { bios, disc, mode: VideoMode::Pal };
    let mut m = boot(&spec)?;
    if let Some(d) = &spec.disc {
        if is_ntsc_name(d, &m) {
            spec.mode = VideoMode::Ntsc;
            m = boot(&spec)?;
        }
    }
    log(RETRO_LOG_INFO, &format!(
        "BIOS {}, disc {}, {:?}",
        spec.bios.display(),
        spec.disc.as_ref().map_or("none".into(), |d| d.display().to_string()),
        spec.mode
    ));
    let mut core = lock(&CORE);
    core.machine = Some(m);
    core.spec = Some(spec);
    core.temp_disc = temp;
    Ok(())
}

fn unload() {
    let mut core = lock(&CORE);
    core.machine = None;
    core.spec = None;
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

// SYSTEM_RAM is plane A (CPU 0x000000-0x07FFFF, big-endian bytes); plane B is not exposed yet.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    guard("retro_get_memory_data", std::ptr::null_mut(), true, || {
        let mut core = lock(&CORE);
        match (id, core.machine.as_mut()) {
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
            (RETRO_MEMORY_SYSTEM_RAM, Some(m)) => m.bus.mcd212.plane_a.len(),
            _ => 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

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
