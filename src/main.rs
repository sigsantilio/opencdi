// OpenCDi Release Build - 2026-09-29 15:45
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use std::path::{Path, PathBuf};
use log::{info, error, warn};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode as Key, ModifiersState, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use opencdi::audio::{self, Frame, HostAudioOutput};
use opencdi::frontend::{pop_action, push_action, GuiAction, InputDeviceMode, KeyBindings};
#[cfg(target_os = "windows")]
use opencdi::gui;
use opencdi::bios;
use opencdi::bus::BoardKind;
use opencdi::machine::Machine;
use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT, VideoMode};
use opencdi::savestate::{Reader, State, StateError};

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(hwnd: *mut std::ffi::c_void, lptext: *const u16, lpcaption: *const u16, utype: u32) -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn AllocConsole() -> i32;
    fn GetStdHandle(id: u32) -> *mut std::ffi::c_void;
    fn SetStdHandle(id: u32, handle: *mut std::ffi::c_void) -> i32;
    fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut std::ffi::c_void, disposition: u32, flags: u32, template: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

// --console: the parent's console (cmd, PowerShell) or a new one, so a GUI-subsystem release build can show its log
#[cfg(target_os = "windows")]
fn attach_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;
    unsafe {
        let usable = |h: *mut std::ffi::c_void| !h.is_null() && h != INVALID_HANDLE_VALUE && GetFileType(h) != 0;
        let saved = [GetStdHandle(STD_OUTPUT_HANDLE), GetStdHandle(STD_ERROR_HANDLE)];
        // Output already goes to a file, a pipe or (under WINE) the Unix terminal
        if saved.iter().all(|&h| usable(h)) {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 && AllocConsole() == 0 {
            return;
        }
        // Redirected handles stay; the others are pointed at the console
        let conout: Vec<u16> = "CONOUT$".encode_utf16().chain(std::iter::once(0)).collect();
        for (id, h) in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].into_iter().zip(saved) {
            if usable(h) {
                SetStdHandle(id, h);
                continue;
            }
            let con = CreateFileW(conout.as_ptr(), 0x8000_0000 | 0x4000_0000, 0x1 | 0x2, std::ptr::null_mut(), 3, 0, std::ptr::null_mut());
            if con != INVALID_HANDLE_VALUE {
                SetStdHandle(id, con);
            }
        }
    }
}

const USAGE: &str = "\
Usage: opencdi [options] [disc image (cue, chd, iso, bin)]

  --rom <path>             System BIOS (default: search bios/, the current and the program directory)
  --frames <n>             Quit after n emulated frames
  --no-autoplay            Leave the BIOS shell waiting instead of clicking PLAY CD-I for the user
  --mouse                  Start in mouse mode (host mouse drives the CD-i pointer; click in the window to capture it)
  --mouse-speed <f>        Scale captured mouse motion (default 0.5)
  --mouse-sensitivity <v>  Mouse sensitivity multiplier (e.g. 0.5, default: 0.5)
  --recent [n]             List the recent disc images, or boot recent image number n
  --no-host-input          Ignore host keyboard, mouse and gamepads; never grab the cursor
  --bios, --standalone     Boot the BIOS shell without a disc
  --board mono1|mono4      Console board (default mono1; mono4 is the CD-i 490, give --rom its BIOS)
  --dvc <path>             Digital Video Cartridge ROM (default: search dvc/, bios/, the current and the program directory)
  --no-dvc, --nodvc        Boot without the Digital Video Cartridge
  --slave-debug            Log slave (mouse channel) bytes
  --nvram <path>           Console NVRAM save file (default: bios/nvram_mk48t08.bin or nvram_mk48t08.bin; 490: nvram_cdi490a.bin)
  --fullscreen, -f         Start directly in borderless fullscreen
  --no-menu, --nomenu      Start without the top menu bar (useful for frontends)
  --monitor <n>            Monitor index to display on in fullscreen
  --screenshot-dir <path>  Folder for F8/F12 PNG screenshots (default: screenshots/ next to the program)
  --run-in-menu            Windows: keep emulating while a menu or dialog is open, or the window is moved (default: paused)
  --console                Windows: show the log in the parent console or a new console window
  --help, -h               Show this text
";


fn default_nvram490_save_path() -> PathBuf {
    if Path::new("bios").is_dir() {
        return PathBuf::from("bios/nvram_cdi490a.bin");
    }
    PathBuf::from("nvram_cdi490a.bin")
}

// Write whichever board NVRAM changed to its file
fn flush_nvram(bus: &mut opencdi::bus::CdiBus) {
    bus.nvram.flush_if_dirty();
    if let Some(m4) = bus.mono4.as_deref_mut() {
        m4.nvram.flush_if_dirty();
    }
}

fn default_nvram_save_path() -> PathBuf {
    if let Some(p) = find_nvram_file() {
        return p;
    }
    if Path::new("bios").is_dir() {
        return PathBuf::from("bios/nvram_mk48t08.bin");
    }
    PathBuf::from("nvram_mk48t08.bin")
}

// Pointer and buttons go to the IKAT on the Mono-IV board, to the slave otherwise
fn send_pointer(bus: &mut opencdi::bus::CdiBus, x: i32, y: i32, buttons: u8) {
    match bus.mono4.as_deref_mut() {
        Some(m4) => m4.ikat.update_pointer(x, y, buttons),
        None => bus.slave.update_mouse(x, y, buttons),
    }
}

fn find_file(relative_subpath: &str) -> Option<PathBuf> {
    let sub = Path::new(relative_subpath);
    let leaf_name = sub.file_name().and_then(|s| s.to_str()).unwrap_or(relative_subpath);

    let mut candidate_bases: Vec<PathBuf> = vec![
        PathBuf::from("."),
        PathBuf::from(".."),
        PathBuf::from("file utili"),
        PathBuf::from("..").join("file utili"),
    ];

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidate_bases.push(exe_dir.to_path_buf());
            candidate_bases.push(exe_dir.join(".."));
            candidate_bases.push(exe_dir.join("file utili"));
            candidate_bases.push(exe_dir.join("..").join("file utili"));
        }
    }

    for base in &candidate_bases {
        // 1. Try exact relative subpath (e.g., bios/cdi220b.rom)
        let exact = base.join(relative_subpath);
        if exact.is_file() {
            return Some(exact);
        }
        // 2. Try direct leaf filename (e.g., cdi220b.rom alongside executable)
        let leaf = base.join(leaf_name);
        if leaf.is_file() {
            return Some(leaf);
        }
    }
    None
}

fn find_bios_rom() -> Option<PathBuf> {
    const BIOS_CANDIDATES: &[&str] = &[
        "bios/cdi220b.rom",
        "cdi220b.rom",
        "bios/cdi200.rom",
        "cdi200.rom",
        "bios/cdimono1.zip",
        "cdimono1.zip",
        "bios/cdibios.zip",
        "cdibios.zip",
        "bios/cdi.rom",
        "cdi.rom",
        "bios/cdi220.rom",
        "cdi220.rom",
        "bios/cdi490a.rom",
        "cdi490a.rom",
        "bios/cdi490a.zip",
        "cdi490a.zip",
    ];
    // Each found file is checked; the best one wins.
    let mut seen = HashSet::new();
    let found = BIOS_CANDIDATES.iter().filter_map(|n| find_file(n)).filter(|p| seen.insert(p.clone())).filter_map(|p| {
        match Machine::read_bios(&p) {
            Ok((_, img)) => {
                info!("BIOS candidate {:?} [{}]: {}", p, img.report.status, img.report.message);
                Some((p, img))
            }
            Err(e) => {
                warn!("BIOS candidate {:?}: {}", p, e);
                None
            }
        }
    });
    bios::best(found).filter(|(_, img)| img.report.status != bios::Status::Error).map(|(p, _)| p)
}

fn find_all_available_bioses() -> Vec<(PathBuf, String)> {
    let mut seen = HashSet::new();
    let mut bioses = Vec::new();

    let mut dirs_to_scan = vec![
        PathBuf::from("bios"),
        PathBuf::from("."),
        PathBuf::from("file utili/bios"),
        PathBuf::from("../file utili/bios"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs_to_scan.push(parent.join("bios"));
            dirs_to_scan.push(parent.to_path_buf());
            dirs_to_scan.push(parent.join("file utili").join("bios"));
            dirs_to_scan.push(parent.join("..").join("file utili").join("bios"));
        }
    }

    for dir in dirs_to_scan {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                if ext != "rom" && ext != "bin" && ext != "zip" {
                    continue;
                }
                let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                if !seen.insert(canon) {
                    continue;
                }
                match Machine::read_bios(&p) {
                    Ok((entry, img)) if img.report.status != bios::Status::Error => {
                        let file_str = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                        let label = if let Some(k) = img.report.known {
                            if let Some(e) = entry {
                                format!("{} [{} / {}]", k.model, file_str, e)
                            } else {
                                format!("{} ({})", k.model, file_str)
                            }
                        } else {
                            format!("Custom CD-i ROM ({})", file_str)
                        };
                        bioses.push((p, label));
                    }
                    _ => {}
                }
            }
        }
    }
    bioses
}

fn resolve_or_guide_bios(rom_arg: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = rom_arg {
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(saved) = opencdi::frontend::load_saved_bios() {
        if Machine::read_bios(&saved).is_ok() {
            info!("Loaded saved system BIOS from configuration (opencdi.ini): {:?}", saved);
            return Some(saved);
        }
    }
    if let Some(found) = find_bios_rom() {
        return Some(found);
    }

    #[cfg(target_os = "windows")]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        let prompt_text =
            "OpenCDi - Philips CD-i System BIOS Missing\n            ==========================================\n\n            Nessun file BIOS di sistema Philips CD-i è stato trovato nelle cartelle di ricerca.\n            OpenCDi richiede una ROM BIOS autentica (es. cdi220b.rom, cdi490a.rom, cdimono1.zip).\n\n            Cosa desideri fare?\n\n            • [SÌ] SFOGLIA: Seleziona manualmente il file del BIOS (o archivio .zip) dal tuo PC\n            • [NO] GUIDAMI ALLA CARTELLA: Crea e apri la cartella 'bios/' in Esplora Risorse dove puoi copiare i file ROM\n            • [ANNULLA] Esci da OpenCDi";

        let wide_text: Vec<u16> = std::ffi::OsStr::new(prompt_text).encode_wide().chain(std::iter::once(0)).collect();
        let wide_title: Vec<u16> = std::ffi::OsStr::new("OpenCDi - Required BIOS Missing").encode_wide().chain(std::iter::once(0)).collect();

        loop {
            let res = MessageBoxW(
                std::ptr::null_mut(),
                wide_text.as_ptr(),
                wide_title.as_ptr(),
                0x0000_0003 | 0x0000_0020, // MB_YESNOCANCEL | MB_ICONQUESTION
            );

            if res == 6 { // IDYES
                if let Some(picked) = gui::open_bios_dialog(std::ptr::null_mut()) {
                    match Machine::read_bios(&picked) {
                        Ok((_, img)) if img.report.status != bios::Status::Error => {
                            opencdi::frontend::save_bios_preference(&picked);
                            return Some(picked);
                        }
                        Ok((_, img)) => {
                            let err_msg = format!("Il file selezionato è stato rifiutato come BIOS CD-i:\n\n{}", img.report.message);
                            let wide_err: Vec<u16> = std::ffi::OsStr::new(&err_msg).encode_wide().chain(std::iter::once(0)).collect();
                            MessageBoxW(std::ptr::null_mut(), wide_err.as_ptr(), wide_title.as_ptr(), 0x0000_0000 | 0x0000_0010); // MB_OK | MB_ICONERROR
                        }
                        Err(e) => {
                            let err_msg = format!("Impossibile aprire il file selezionato:\n\n{}", e);
                            let wide_err: Vec<u16> = std::ffi::OsStr::new(&err_msg).encode_wide().chain(std::iter::once(0)).collect();
                            MessageBoxW(std::ptr::null_mut(), wide_err.as_ptr(), wide_title.as_ptr(), 0x0000_0000 | 0x0000_0010);
                        }
                    }
                }
            } else if res == 7 { // IDNO
                let bios_dir = Path::new("bios");
                let _ = std::fs::create_dir_all(bios_dir);
                gui::open_folder(bios_dir);

                let guide_text =
                    "La cartella 'bios/' è stata aperta in Esplora Risorse!\n\n                    Copia nella cartella uno o più dei seguenti file:\n                      • cdi220b.rom (512 KB, Philips CD-i 220 European PAL - consigliato)\n                      • cdi490a.rom (512 KB, Philips CD-i 490 Mono-IV)\n                      • vmpega.rom (256 KB, Digital Video Cartridge per FMV/MPEG)\n                      • oppure archivi cdimono1.zip / cdibios.zip\n\n                    Dopo aver copiato i file, clicca [OK] per verificare e avviare OpenCDi.";

                let wide_guide: Vec<u16> = std::ffi::OsStr::new(guide_text).encode_wide().chain(std::iter::once(0)).collect();
                let guide_res = MessageBoxW(
                    std::ptr::null_mut(),
                    wide_guide.as_ptr(),
                    wide_title.as_ptr(),
                    0x0000_0001 | 0x0000_0040, // MB_OKCANCEL | MB_ICONINFORMATION
                );

                if guide_res == 1 { // IDOK
                    if let Some(found) = find_bios_rom() {
                        opencdi::frontend::save_bios_preference(&found);
                        return Some(found);
                    } else {
                        let not_found = "Nessun BIOS valido trovato nella cartella 'bios/'.\nAssicurati di aver incollato il file (es. cdi220b.rom) e riprova.";
                        let wide_nf: Vec<u16> = std::ffi::OsStr::new(not_found).encode_wide().chain(std::iter::once(0)).collect();
                        MessageBoxW(std::ptr::null_mut(), wide_nf.as_ptr(), wide_title.as_ptr(), 0x0000_0000 | 0x0000_0030); // MB_OK | MB_ICONWARNING
                    }
                }
            } else {
                // IDCANCEL
                return None;
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    None
}

// First candidate that passes the cartridge check; refused files are logged with the reason.
fn find_dvc_rom() -> Option<PathBuf> {
    const DVC_CANDIDATES: &[&str] = &[
        "dvc/vmpega.rom",
        "bios/vmpega.rom",
        "vmpega.rom",
        "dvc/impega.rom",
        "bios/impega.rom",
        "impega.rom",
    ];
    for &name in DVC_CANDIDATES {
        let Some(p) = find_file(name) else { continue };
        match Machine::read_dvc(&p) {
            Ok((_, img)) if img.report.status != bios::Status::Error => return Some(p),
            Ok((_, img)) => warn!("DVC ROM {:?} refused: {}", p, img.report.message),
            Err(e) => warn!("DVC ROM {:?}: {}", p, e),
        }
    }
    None
}

// Fits a cartridge ROM and logs the check verdict: refused images say so, unknown ones warn.
fn load_dvc_logged(m: &mut Machine, path: &std::path::Path) -> bool {
    match m.load_dvc(path) {
        Ok(r) if r.status == bios::Status::Warning => {
            warn!("DVC ROM {:?}: {}", path, r.message);
            true
        }
        Ok(r) => {
            info!("DVC ROM {:?} [{}]: {}", path, r.status, r.message);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            warn!("DVC ROM {:?} refused: {}", path, e);
            false
        }
        Err(e) => {
            warn!("DVC ROM {:?}: {}", path, e);
            false
        }
    }
}

fn find_nvram_file() -> Option<PathBuf> {
    const NVRAM_CANDIDATES: &[&str] = &[
        "bios/nvram_mk48t08.bin",
        "nvram_mk48t08.bin",
        "bios/nvram.bin",
        "nvram.bin",
    ];
    for &name in NVRAM_CANDIDATES {
        if let Some(p) = find_file(name) {
            return Some(p);
        }
    }
    None
}

// ASCII letters, digits, '-' and '_' only: spaces become '_', brackets are dropped
fn file_stem(game: &str) -> String {
    let mut name = String::new();
    for c in game.chars().filter(|c| !"()[]{}".contains(*c)) {
        let c = if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' };
        if c != '_' || !name.ends_with('_') {
            name.push(c);
        }
    }
    let name = name.trim_matches('_');
    if name.is_empty() { "opencdi".to_string() } else { name.to_string() }
}

// Savestates go into states/ next to the binary
fn states_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("states")))
        .unwrap_or_else(|| PathBuf::from("states"))
}

// RetroArch naming: slot 0 is <name>.state, slot N is <name>.stateN
fn state_path(dir: &Path, game: &str, slot: u32) -> PathBuf {
    let name: String = game.chars().map(|c| if c.is_alphanumeric() || " ()-_.,'&!".contains(c) { c } else { '_' }).collect();
    let name = if name.is_empty() { "opencdi".to_string() } else { name };
    if slot == 0 { dir.join(format!("{}.state", name)) } else { dir.join(format!("{}.state{}", name, slot)) }
}

// Written to a temporary file first so a crash never leaves a half-written slot
fn write_state_file(path: &Path, m: &Machine, frontend: &[u8]) -> std::io::Result<usize> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut buf = Vec::new();
    m.save_state(frontend, &mut buf);
    let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, &buf)?;
    std::fs::rename(&tmp, path)?;
    Ok(buf.len())
}

// Returns the frontend bytes; the machine is unchanged on any error
fn read_state_file(path: &Path, m: &mut Machine) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    m.load_state(&data).map(|fe| fe.to_vec()).map_err(|e: StateError| e.to_string())
}

// <game>_<frame>.png in dir, created on demand
fn save_screenshot(dir: &Path, game: &str, frame_count: u64, framebuffer: &[u32]) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        error!("Cannot create screenshot directory {:?}: {}", dir, e);
        return;
    }
    let path = dir.join(format!("{}_{}.png", file_stem(game), frame_count));
    let mut rgb = Vec::with_capacity(SCREEN_WIDTH * SCREEN_HEIGHT * 3);
    for &pixel in &framebuffer[..SCREEN_WIDTH * SCREEN_HEIGHT] {
        rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
    }
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&path)?), SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&rgb)?;
        Ok(())
    };
    match write() {
        Ok(()) => info!("Screenshot salvato in {:?}", path),
        Err(e) => error!("Cannot write screenshot {:?}: {}", path, e),
    }
}

// Flags that take a value, so the value is never mistaken for the disc
const VALUE_FLAGS: &[&str] = &[
    "--rom",
    "--frames",
    "--dvc",
    "--nvram",
    "--board",
    "--mouse-speed",
    "--mouse-sensitivity",
    "--screenshot-dir",
    "--monitor",
    "--video-levels",
];

// Disc = first positional argument, skipping flag values; --recent takes a value only when it is a number
fn first_positional(args: &[String]) -> Option<String> {
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            it.next();
        } else if a == "--recent" {
            if it.peek().is_some_and(|n| n.parse::<usize>().is_ok()) {
                it.next();
            }
        } else if !a.starts_with("--") {
            return Some(a.clone());
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has_flag = |f: &str| args.iter().any(|a| a == f);
    let help = has_flag("--help") || has_flag("-h");
    #[cfg(target_os = "windows")]
    if help || has_flag("--console") || has_flag("--recent") {
        attach_console();
    }
    if help {
        print!("{}", USAGE);
        return Ok(());
    }
    // --recent alone lists the recent disc images; --recent <n> boots number n
    let recent_pick: Option<usize> = args.iter().position(|a| a == "--recent").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok());
    if has_flag("--recent") && recent_pick.is_none() {
        let list = opencdi::frontend::recent_images();
        if list.is_empty() {
            println!("No recent disc images");
        }
        for (i, p) in list.iter().enumerate() {
            println!("{:2}  {}", i + 1, p.display());
        }
        return Ok(());
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    #[cfg(target_os = "windows")]
    unsafe {
        timeBeginPeriod(1);
    }

    info!("=== OpenCDi - Philips CD-i Emulator ===");

    let flag_value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let rom_arg = flag_value("--rom").map(PathBuf::from);
    let max_frames: Option<u64> = flag_value("--frames").and_then(|s| s.parse().ok());

    if let Some(v) = flag_value("--video-levels") {
        match v.as_str() {
            "full" => opencdi::mcd212::set_studio_levels(false),
            "studio" => opencdi::mcd212::set_studio_levels(true),
            _ => {
                eprintln!("--video-levels takes full or studio");
                std::process::exit(2);
            }
        }
    }

    let board = flag_value("--board").map(|v| BoardKind::parse(&v).unwrap_or_else(|| {
        eprintln!("--board takes mono1 or mono4");
        std::process::exit(2);
    }));

    let mut m = Machine::new();
    m.bus.slave.debug_log = has_flag("--slave-debug");
    if m.bus.slave.debug_log {
        info!("Debug Slave XBUS abilitato (--slave-debug): log byte CH0 (mouse)");
    }

    // 1. Load BIOS ROM (CLI --rom, saved config, auto-detect, or interactive guide)
    let bios_path = match resolve_or_guide_bios(rom_arg.clone()) {
        Some(path) => path,
        None => {
            info!("OpenCDi exit: no system BIOS selected.");
            std::process::exit(0);
        }
    };
    let bios_report = match m.load_bios(&bios_path) {
        Ok(r) if r.status == bios::Status::Ok => {
            info!("Loaded system BIOS from {:?}: {}", bios_path, r.message);
            Some(r)
        }
        Ok(r) => {
            warn!("Loaded system BIOS from {:?} [{}]: {}", bios_path, r.status, r.message);
            Some(r)
        }
        Err(e) => {
            error!("Cannot use system BIOS {:?}: {}", bios_path, e);
            eprintln!("Cannot use system BIOS {:?}: {}", bios_path, e);
            std::process::exit(1);
        }
    };

    let detected_board = if bios_report.as_ref().map_or(false, |r| r.board == Some(bios::Board::Mono4)) {
        Some(BoardKind::Mono4)
    } else {
        Some(BoardKind::Mono1)
    };
    if let Some(b) = board.or(detected_board) {
        m.bus.set_board(b);
    }
    let available_bioses = find_all_available_bioses();

    // 2. Load NVRAM if present
    let nvram_arg = flag_value("--nvram").map(PathBuf::from);
    let mono4_nvram = m.bus.mono4.is_some();
    let resolved_nvram_path = nvram_arg.clone().unwrap_or_else(if mono4_nvram { default_nvram490_save_path } else { default_nvram_save_path });
    if let Some(m4) = m.bus.mono4.as_deref_mut() {
        match m4.nvram.attach_file(&resolved_nvram_path) {
            Ok(true) => info!("Mono-IV NVRAM loaded from {:?}", resolved_nvram_path),
            Ok(false) => info!("Mono-IV NVRAM starts empty, saved to {:?}", resolved_nvram_path),
            Err(e) => log::warn!("Cannot load Mono-IV NVRAM from {:?}: {}", resolved_nvram_path, e),
        }
    } else {
        m.bus.nvram.save_path = resolved_nvram_path.clone();
        if resolved_nvram_path.is_file() {
            if let Err(e) = m.bus.nvram.load_from_file(&resolved_nvram_path) {
                log::warn!("Impossibile caricare NVRAM da {:?}: {:?}", resolved_nvram_path, e);
            } else {
                info!("NVRAM caricata da {:?}", resolved_nvram_path);
            }
        } else if nvram_arg.is_some() {
            if let Err(e) = m.bus.nvram.save_to_file() {
                log::warn!("Impossibile creare file iniziale NVRAM in {:?}: {:?}", resolved_nvram_path, e);
            } else {
                info!("Inizializzato nuovo file NVRAM in {:?}", resolved_nvram_path);
            }
        } else {
            info!("File NVRAM {:?} non presente: verrà creato automaticamente al salvataggio", resolved_nvram_path);
        }
    }

    // 2b. Load DVC ROM (Digital Video Cartridge) by default if present
    let dvc_disabled = has_flag("--no-dvc") || has_flag("--nodvc");
    let dvc_arg = flag_value("--dvc").map(PathBuf::from);
    let mut resolved_dvc_path = None;
    if !dvc_disabled {
        let dvc_candidate = dvc_arg.clone()
            .or_else(opencdi::frontend::load_saved_dvc)
            .or_else(find_dvc_rom);
        if let Some(dvc_path) = dvc_candidate {
            if load_dvc_logged(&mut m, &dvc_path) {
                info!("Loaded Digital Video Cartridge (DVC) ROM from {:?}", dvc_path);
                resolved_dvc_path = Some(dvc_path);
            }
        } else {
            info!("No DVC ROM (vmpega.rom) found: booting in Base CD-i mode (MPEG video disabled).");
        }
    } else {
        info!("Forced boot without DVC (--no-dvc): Base CD-i mode");
    }

    // 3. Optional Game Disc (from command line argument, or None for standalone BIOS)
    let mut current_game_name = "No disc inserted (Standalone BIOS)".to_string();
    // Disc = first positional argument (skipping flag values); --bios / --standalone force no disc
    let standalone = has_flag("--bios") || has_flag("--standalone");
    let mut disc_arg = first_positional(&args);
    if let Some(n) = recent_pick {
        disc_arg = opencdi::frontend::recent_images().get(n.wrapping_sub(1)).map(|p| p.to_string_lossy().to_string());
        if disc_arg.is_none() {
            log::warn!("No recent disc image number {} (see --recent)", n);
        }
    }
    let initial_disc = disc_arg.filter(|_| !standalone).and_then(|a| {
        find_file(&a).or_else(|| {
            let p = PathBuf::from(&a);
            if p.exists() || opencdi::cdrom::parse_drive_letter(&p).is_some() { Some(p) } else { None }
        })
    });

    if let Some(p) = initial_disc {
        match m.bus.load_cdrom(&p) {
            Ok(_) => {
                opencdi::frontend::add_recent_image(&p);
                current_game_name = opencdi::frontend::disc_display_name(&p);
                info!("Loaded game disc at startup: {}", current_game_name);

                let name_upper = current_game_name.to_uppercase();
                let is_ntsc = m.bus.cdrom.as_ref().and_then(|cd| cd.is_ntsc_hint(&current_game_name))
                    .unwrap_or_else(|| {
                        (name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC"))
                            && m.bus.cdrom.as_ref().map_or(true, |cd| cd.disc_type != 0x20)
                    });
                if is_ntsc {
                    info!("Detected NTSC disc (USA/Japan): auto-configured NTSC 60 Hz video mode");
                    m.bus.set_video_mode(VideoMode::Ntsc);
                } else {
                    info!("Console booted in standard PAL 50 Hz mode");
                    m.bus.set_video_mode(VideoMode::Pal);
                }
            }
            Err(e) => error!("Error loading game disc from {:?}: {:?}", p, e),
        }
    } else {
        info!("Starting console in Standalone BIOS mode (no disc inserted) - PAL 50 Hz");
        m.bus.set_video_mode(VideoMode::Pal);
    }

    // 4. Initialize 68070 CPU Core
    m.reset();
    info!("CPU SCC68070 reset - PC: 0x{:08X}", m.cpu.pc);

    // 5. Initialize Controls Configuration
    let mut bindings = KeyBindings::new();
    let mouse_sens_cli = flag_value("--mouse-speed")
        .or_else(|| flag_value("--mouse-sensitivity"))
        .and_then(|s| {
            let clean = s.trim_end_matches('%');
            clean.parse::<f32>().ok().map(|v| if v > 5.0 { v / 100.0 } else { v })
        });
    if let Some(sens) = mouse_sens_cli {
        bindings.mouse_sensitivity = sens.clamp(0.05, 10.0);
    }
    let input_device_mode = if has_flag("--mouse") {
        InputDeviceMode::Mouse
    } else {
        bindings.input_device
    };

    // 6. Initialize Host Audio Playback
    let audio_ring: Arc<Mutex<VecDeque<Frame>>> = Arc::new(Mutex::new(VecDeque::with_capacity(audio::RING_CAP)));
    let audio_gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let host_audio = HostAudioOutput::try_start(audio_ring.clone(), audio_gain.clone());

    // 7. Initialize USB Gamepad subsystem (Gilrs / XInput / DirectInput)
    let gilrs = gilrs::Gilrs::new().ok();
    if gilrs.is_some() {
        info!("USB Gamepad subsystem active (Xbox / PlayStation / Generic USB)");
    }

    m.bus.emulate_wait_states = true; // Authentic hardware wait-states (15 MHz)
    let start_fullscreen = has_flag("--fullscreen") || has_flag("-f");
    let no_menu = has_flag("--no-menu") || has_flag("--nomenu");
    let monitor_index: Option<usize> = flag_value("--monitor").and_then(|s| {
        let n = s.parse().ok();
        if n.is_none() {
            log::warn!("--monitor {:?} is not a number, using the current monitor", s);
        }
        n
    });
    let screenshot_dir = flag_value("--screenshot-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("screenshots")))
                .unwrap_or_else(|| PathBuf::from("screenshots"))
        });

    let (pad_x, pad_y) = if m.bus.cdrom.is_some() { (575, 335) } else { (384, 280) };
    let prev_vsr = (m.bus.mcd212.get_vsr::<0>(), m.bus.mcd212.get_vsr::<1>());
    let now = Instant::now();
    let frame_duration = m.bus.mcd212.video_mode.frame_duration();
    let mut app = App {
        m,
        bindings,
        input_device_mode,
        dvc_disabled,
        dvc_arg,
        max_frames,
        current_game_name,
        nvram_path: Some(resolved_nvram_path),
        active_bios_path: bios_path.clone(),
        available_bioses,
        bios_report,
        dvc_path: resolved_dvc_path,
        start_fullscreen,
        no_menu,
        monitor_index,
        screenshot_dir,
        no_autoplay: has_flag("--no-autoplay"),
        window: None,
        surface: None,
        image_rect: (0, 0, SCREEN_WIDTH, SCREEN_HEIGHT),
        windowed: None,
        restore_check: None,
        _host_audio: host_audio,
        audio_ring,
        audio_gain,
        audio_buf: Vec::new(),
        volume: 1.0,
        muted: false,
        gilrs,
        keys: HashSet::new(),
        mods: ModifiersState::empty(),
        focused: false,
        cursor: None,
        rel_pointer: None,
        rel_over: (0.0, 0.0),
        mouse_left: false,
        swallow_left: false,
        mouse_left_latched: false,
        mouse_right: false,
        mouse_right_latched: false,
        mouse_middle: false,
        prev_middle: false,
        mouse_captured: start_fullscreen && no_menu,
        host_input: !has_flag("--no-host-input"),
        run_in_menu: has_flag("--run-in-menu"),
        in_modal_pump: false,
        paused: false,
        fast_forward: false,
        last_present: now,
        state_slot: 0,
        occluded: false,
        grab_applied: None,
        frame_count: 0,
        frames_run: 0,
        disc_launched: false,
        shell_ready_count: 0,
        pad_x,
        pad_y,
        pad_x_f: pad_x as f32,
        pad_y_f: pad_y as f32,
        prev_key_btn: 0,
        run_at: now,
        next_frame_time: now + frame_duration,
        fps_timer: now,
        telemetry_frames: 0,
        paused_ticks: 0,
        measured_fps: 50.0,
        prev_instructions: 0,
        prev_cd_sectors: 0,
        prev_vsr,
        buffer_flips_interval: 0,
        measured_render_fps: 0.0,
        measured_mips: 0.0,
        measured_cd_rate: 0.0,
    };

    info!("Emulation loop started. Shortcuts: [F11]/[Alt+Enter] Fullscreen, [F5]/[H] Reset, [P] Pause, [Space] hold Fast-forward, [F8]/[F12] Screenshot (PNG), [F2]/[F4] Save/Load state, [F6]/[F7] State slot, [M] Mute, [+]/[-] Volume, [L] Video levels studio/full, [Ctrl+I] Insert Disc, [Ctrl+O] Open & Reset, [Ctrl+B] Select BIOS, [Middle Click] Capture/Release Mouse, [Esc] Quit");

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;

    #[cfg(target_os = "windows")]
    unsafe {
        timeEndPeriod(1);
    }

    flush_nvram(&mut app.m.bus);
    info!("Emulation stopped. Final PC: 0x{:08X}", app.m.cpu.pc);
    Ok(())
}

// Letterbox rectangle (x, y, w, h) of the CD-i picture in a dst_w x dst_h surface, as minifb's AspectRatioStretch computes it
// Application icon from icon.ico (the 64x64 PNG entry), as on the Windows executable
fn window_icon() -> Option<winit::window::Icon> {
    let ico: &[u8] = include_bytes!("../icon.ico");
    let count = u16::from_le_bytes([*ico.get(4)?, *ico.get(5)?]) as usize;
    let entry = (0..count).map(|i| &ico[6 + i * 16..22 + i * 16]).find(|e| e[0] == 64)?;
    let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
    let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ico.get(offset..offset + size)?));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    winit::window::Icon::from_rgba(buf, info.width, info.height).ok()
}

fn image_rect(dst_w: usize, dst_h: usize) -> (usize, usize, usize, usize) {
    if dst_w == 0 || dst_h == 0 {
        return (0, 0, 0, 0);
    }
    let buffer_aspect = SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32;
    let win_aspect = dst_w as f32 / dst_h as f32;
    if buffer_aspect > win_aspect {
        let h = ((dst_w as f32 / buffer_aspect) as usize).clamp(1, dst_h);
        (0, (dst_h - h) / 2, dst_w, h)
    } else {
        let w = ((dst_h as f32 * buffer_aspect) as usize).clamp(1, dst_w);
        ((dst_w - w) / 2, 0, w, dst_h)
    }
}

// Host cursor position mapped through the letterbox onto the CD-i picture, clamped
fn cursor_to_cdi(pos: PhysicalPosition<f64>, rect: (usize, usize, usize, usize)) -> (i32, i32) {
    let (x0, y0, w, h) = rect;
    if w == 0 || h == 0 {
        return (0, 0);
    }
    let mx = (pos.x - x0 as f64) * SCREEN_WIDTH as f64 / w as f64;
    let my = (pos.y - y0 as f64) * SCREEN_HEIGHT as f64 / h as f64;
    ((mx.round() as i32).clamp(0, SCREEN_WIDTH as i32 - 1), (my.round() as i32).clamp(0, SCREEN_HEIGHT as i32 - 1))
}

// Nearest-neighbour stretch with black bars, same 10-bit fixed-point stepping as minifb's scaler
fn blit_aspect(dst: &mut [u32], dst_w: usize, dst_h: usize, src: &[u32], rect: (usize, usize, usize, usize)) {
    dst.fill(0);
    let (x0, y0, w, h) = rect;
    if w == 0 || h == 0 {
        return;
    }
    let step_x = ((SCREEN_WIDTH as f32 / w as f32) * 1024.0) as usize;
    let step_y = ((SCREEN_HEIGHT as f32 / h as f32) * 1024.0) as usize;
    let mut fixed_y = 0usize;
    for row in dst.chunks_exact_mut(dst_w).skip(y0).take(h.min(dst_h - y0)) {
        let src_row = &src[(fixed_y >> 10).min(SCREEN_HEIGHT - 1) * SCREEN_WIDTH..][..SCREEN_WIDTH];
        let mut fixed_x = 0usize;
        for px in &mut row[x0..x0 + w] {
            *px = src_row[(fixed_x >> 10).min(SCREEN_WIDTH - 1)];
            fixed_x += step_x;
        }
        fixed_y += step_y;
    }
}

struct App {
    m: Machine,
    bindings: KeyBindings,
    input_device_mode: InputDeviceMode,
    dvc_disabled: bool,
    dvc_arg: Option<PathBuf>,
    max_frames: Option<u64>,
    current_game_name: String,
    nvram_path: Option<PathBuf>,
    active_bios_path: PathBuf,
    available_bioses: Vec<(PathBuf, String)>,
    bios_report: Option<bios::Report>,
    dvc_path: Option<PathBuf>,
    start_fullscreen: bool,
    no_menu: bool,
    monitor_index: Option<usize>,
    screenshot_dir: PathBuf,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    image_rect: (usize, usize, usize, usize),
    windowed: Option<(Option<PhysicalPosition<i32>>, PhysicalSize<u32>)>,
    restore_check: Option<Instant>,
    _host_audio: Option<HostAudioOutput>,
    audio_ring: Arc<Mutex<VecDeque<Frame>>>,
    audio_gain: Arc<AtomicU32>,
    audio_buf: Vec<Frame>,
    volume: f32,
    muted: bool,
    gilrs: Option<gilrs::Gilrs>,
    keys: HashSet<Key>,
    mods: ModifiersState,
    focused: bool,
    cursor: Option<PhysicalPosition<f64>>,
    rel_pointer: Option<(f64, f64)>,
    rel_over: (f64, f64), // raw mouse motion past the screen edge since the last frame, in CD-i pixels
    mouse_left: bool,
    swallow_left: bool,
    mouse_left_latched: bool,
    mouse_right: bool,
    mouse_right_latched: bool,
    mouse_middle: bool,
    prev_middle: bool,
    mouse_captured: bool,
    no_autoplay: bool,
    host_input: bool,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    run_in_menu: bool,
    in_modal_pump: bool,
    occluded: bool,
    paused: bool,
    fast_forward: bool,
    last_present: Instant,
    state_slot: u32,
    grab_applied: Option<bool>,
    frame_count: u64,
    frames_run: u64,
    disc_launched: bool,
    shell_ready_count: u32,
    pad_x: i32,
    pad_y: i32,
    pad_x_f: f32,
    pad_y_f: f32,
    prev_key_btn: u8,
    run_at: Instant,
    next_frame_time: Instant,
    fps_timer: Instant,
    telemetry_frames: u32,
    paused_ticks: u32,
    measured_fps: f64,
    prev_instructions: u64,
    prev_cd_sectors: u32,
    prev_vsr: (u32, u32),
    buffer_flips_interval: u32,
    measured_render_fps: f64,
    measured_mips: f64,
    measured_cd_rate: f64,
}

impl App {
    fn key(&self, k: Key) -> bool {
        self.keys.contains(&k)
    }

    #[cfg(target_os = "windows")]
    fn hwnd(&self) -> *mut std::ffi::c_void {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        self.window
            .as_ref()
            .and_then(|w| w.window_handle().ok())
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
                _ => None,
            })
            .unwrap_or(std::ptr::null_mut())
    }

    fn take_screenshot(&self) {
        save_screenshot(&self.screenshot_dir, &self.current_game_name, self.frame_count, &self.m.bus.mcd212.framebuffer[..]);
    }

    fn set_fullscreen(&mut self, full: bool) {
        if let Some(window) = &self.window {
            if full {
                // A maximised window is restored by the platform itself
                if self.windowed.is_none() {
                    self.windowed = Some((window.outer_position().ok(), window.inner_size())).filter(|_| !window.is_maximized());
                }
                let target_monitor = self.monitor_index.and_then(|idx| {
                    let monitors: Vec<_> = window.available_monitors().collect();
                    if idx > 0 && idx <= monitors.len() {
                        Some(monitors[idx - 1].clone())
                    } else if idx < monitors.len() {
                        Some(monitors[idx].clone())
                    } else {
                        None
                    }
                });
                window.set_fullscreen(Some(Fullscreen::Borderless(target_monitor)));
                #[cfg(target_os = "windows")]
                gui::set_menu_visible(self.hwnd(), false);
            } else {
                window.set_fullscreen(None);
                #[cfg(target_os = "windows")]
                if !self.no_menu {
                    gui::set_menu_visible(self.hwnd(), true);
                }
                self.restore_check = None;
                if self.windowed.is_some() {
                    self.restore_windowed();
                    // Window managers may apply their own geometry after the switch (seen under WINE): check once more
                    self.restore_check = Some(Instant::now() + Duration::from_millis(300));
                }
            }
            info!("Schermo intero: {}", if full { "SCHERMO INTERO" } else { "FINESTRA" });
        }
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            let full = window.fullscreen().is_none();
            self.set_fullscreen(full);
        }
    }

    // Puts the window back to the size and position it had before fullscreen
    fn restore_windowed(&self) {
        let (Some(window), Some((pos, size))) = (&self.window, self.windowed) else { return };
        if let Some(pos) = pos.filter(|&p| window.outer_position().ok() != Some(p)) {
            window.set_outer_position(pos);
        }
        if window.inner_size() != size {
            let _ = window.request_inner_size(size);
        }
    }

    fn switch_bios(&mut self, new_path: &Path) -> bool {
        info!("Switching system BIOS to: {:?}", new_path);
        let (entry, img) = match Machine::read_bios(new_path) {
            Ok(res) => res,
            Err(e) => {
                error!("Cannot read BIOS file {:?}: {}", new_path, e);
                #[cfg(target_os = "windows")]
                gui::show_disc_error(self.hwnd(), &format!("Cannot read BIOS file:\n\n{:?}\n\nError: {}", new_path, e));
                return false;
            }
        };

        if img.report.status == bios::Status::Error {
            error!("BIOS file {:?} is invalid: {}", new_path, img.report.message);
            #[cfg(target_os = "windows")]
            gui::show_disc_error(self.hwnd(), &format!("Selected BIOS file was rejected:\n\n{}", img.report.message));
            return false;
        }

        let is_mono4 = img.report.board == Some(bios::Board::Mono4);
        let target_board = if is_mono4 { BoardKind::Mono4 } else { BoardKind::Mono1 };

        self.m.bus.set_board(target_board);
        self.m.load_bios_image(&img);
        let mut rep = img.report.clone();
        if let Some(e) = entry {
            rep.message = format!("{}: {}", e, rep.message);
        }
        info!("Loaded system BIOS from {:?} [{}]: {}", new_path, rep.status, rep.message);

        // Configure NVRAM for the target board
        let new_nvram = if is_mono4 { default_nvram490_save_path() } else { default_nvram_save_path() };
        if let Some(m4) = self.m.bus.mono4.as_deref_mut() {
            let _ = m4.nvram.attach_file(&new_nvram);
        } else {
            self.m.bus.nvram.save_path = new_nvram.clone();
            if new_nvram.is_file() {
                let _ = self.m.bus.nvram.load_from_file(&new_nvram);
            }
        }
        self.nvram_path = Some(new_nvram);

        // Load DVC ROM if enabled
        if !self.dvc_disabled {
            if let Some(dvc_p) = self.dvc_arg.clone().or_else(opencdi::frontend::load_saved_dvc).or_else(find_dvc_rom) {
                if load_dvc_logged(&mut self.m, &dvc_p) {
                    self.dvc_path = Some(dvc_p);
                }
            }
        }

        // Full console & CPU reset
        self.m.bus.reset_system();
        self.m.cpu.reset(&mut self.m.bus);
        self.reset_state();
        self.mouse_captured = false;
        self.apply_grab();
        self.m.bus.emulate_wait_states = true;
        self.next_frame_time = Instant::now() + self.m.bus.mcd212.video_mode.frame_duration();
        self.run_at = Instant::now();
        self.reset_telemetry();

        self.active_bios_path = new_path.to_path_buf();
        self.bios_report = Some(rep);
        opencdi::frontend::save_bios_preference(new_path);

        #[cfg(target_os = "windows")]
        {
            gui::update_bios_menu(self.hwnd(), &self.active_bios_path, &self.available_bioses);
            gui::update_dvc_menu(self.m.bus.dvc.is_fitted());
            self.refresh_title();
        }
        true
    }

    fn reset_state(&mut self) {
        self.frame_count = 0;
        self.disc_launched = false;
        self.shell_ready_count = 0;
        self.pad_x = if self.m.bus.cdrom.is_some() { 575 } else { 384 };
        self.pad_y = if self.m.bus.cdrom.is_some() { 335 } else { 280 };
        self.pad_x_f = self.pad_x as f32;
        self.pad_y_f = self.pad_y as f32;
    }

    // Hide and confine the host cursor while captured and focused (Win32 ShowCursor/ClipCursor before)
    fn apply_grab(&mut self) {
        let Some(window) = &self.window else { return };
        let want = self.host_input && self.focused && self.mouse_captured;
        if self.grab_applied == Some(want) {
            return;
        }
        self.grab_applied = Some(want);
        // While grabbed the host cursor is hidden and pinned (Windows: window centre), so mouse mode follows raw motion
        self.rel_pointer = want.then(|| match self.cursor {
            Some(pos) => {
                let (x, y) = cursor_to_cdi(pos, self.image_rect);
                (x as f64, y as f64)
            }
            None => (self.pad_x as f64, self.pad_y as f64),
        });
        window.set_cursor_visible(!want);
        let mode = if want { CursorGrabMode::Confined } else { CursorGrabMode::None };
        if let Err(e) = window.set_cursor_grab(mode) {
            info!("Cursor grab {:?} not available: {}", mode, e);
        }
    }

    fn add_recent(&self, path: &Path) {
        #[cfg(target_os = "windows")]
        gui::add_recent_image(path);
        #[cfg(not(target_os = "windows"))]
        opencdi::frontend::add_recent_image(path);
    }

    fn open_dialog_action(&self, open_and_reset: bool) {
        #[cfg(target_os = "windows")]
        gui::post_menu_command(self.hwnd(), if open_and_reset { gui::ID_FILE_OPEN } else { gui::ID_FILE_INSERT });
        #[cfg(not(target_os = "windows"))]
        {
            let _ = open_and_reset;
            info!("No file dialog on this platform: pass the disc path on the command line, or --recent <n>");
        }
    }

    // A key bound to a CD-i control in opencdi_controls.ini is not a shortcut
    fn bound(&self, k: Key) -> bool {
        let b = &self.bindings;
        [b.up, b.down, b.left, b.right, b.btn1, b.btn2].contains(&k)
    }

    // Host-side input and autoplay state saved alongside the machine so a load does not re-run autoplay clicks
    fn frontend_state(&self) -> Vec<u8> {
        let mut w = Vec::new();
        (self.frame_count, self.disc_launched, self.shell_ready_count).save(&mut w);
        (self.pad_x, self.pad_y, self.prev_key_btn).save(&mut w);
        (self.pad_x_f, self.pad_y_f).save(&mut w);
        w
    }

    fn set_frontend_state(&mut self, data: &[u8]) -> Result<(), StateError> {
        let mut r = Reader::new(data);
        let mut a = (self.frame_count, self.disc_launched, self.shell_ready_count);
        let mut b = (self.pad_x, self.pad_y, self.prev_key_btn);
        let mut c = (self.pad_x_f, self.pad_y_f);
        a.load(&mut r)?;
        b.load(&mut r)?;
        c.load(&mut r)?;
        (self.frame_count, self.disc_launched, self.shell_ready_count) = a;
        (self.pad_x, self.pad_y, self.prev_key_btn) = b;
        (self.pad_x_f, self.pad_y_f) = c;
        Ok(())
    }

    fn state_file(&self) -> PathBuf {
        state_path(&states_dir(), &self.current_game_name, self.state_slot)
    }

    fn save_state(&mut self) {
        let path = self.state_file();
        match write_state_file(&path, &self.m, &self.frontend_state()) {
            Ok(n) => info!("State slot {} saved to {:?} ({} bytes)", self.state_slot, path, n),
            Err(e) => error!("State slot {} not saved to {:?}: {}", self.state_slot, path, e),
        }
    }

    fn load_state(&mut self) {
        let path = self.state_file();
        match read_state_file(&path, &mut self.m) {
            Ok(fe) => {
                if let Err(e) = self.set_frontend_state(&fe) {
                    error!("State slot {}: frontend section unreadable ({}); autoplay state kept", self.state_slot, e);
                }
                self.reset_telemetry();
                info!("State slot {} loaded from {:?}", self.state_slot, path);
            }
            Err(e) => error!("State slot {} not loaded from {:?}: {}", self.state_slot, path, e),
        }
    }

    fn on_key_pressed(&mut self, el: &ActiveEventLoop, code: Key) {
        let ctrl = self.mods.control_key();
        let alt = self.mods.alt_key();
        match code {
            Key::Escape => el.exit(),
            Key::F1 => push_action(GuiAction::OpenFeedback),
            Key::F9 => {
                let next_mode = match self.input_device_mode {
                    InputDeviceMode::Gamepad => InputDeviceMode::Mouse,
                    InputDeviceMode::Mouse => InputDeviceMode::Gamepad,
                };
                push_action(GuiAction::SetInputDevice(next_mode));
            }
            Key::KeyI if ctrl => self.open_dialog_action(false),
            Key::KeyO if ctrl => self.open_dialog_action(true),
            Key::KeyB if ctrl => push_action(GuiAction::OpenBiosDialog),
            Key::F5 => push_action(GuiAction::Reset),
            Key::KeyH if !ctrl && !self.bound(code) => push_action(GuiAction::Reset),
            Key::KeyP if !ctrl && !self.bound(code) => {
                self.paused = !self.paused;
                info!("Emulation: {}", if self.paused { "PAUSED" } else { "RUNNING" });
                self.reset_telemetry();
                self.refresh_title();
            }
            Key::F11 => self.toggle_fullscreen(),
            Key::Enter | Key::NumpadEnter if alt => self.toggle_fullscreen(),
            Key::F8 | Key::F12 => self.take_screenshot(),
            Key::F2 => self.save_state(),
            Key::F4 => self.load_state(),
            Key::F6 | Key::F7 => {
                self.state_slot = if code == Key::F6 { self.state_slot.saturating_sub(1) } else { (self.state_slot + 1).min(999) };
                info!("State slot: {} ({:?})", self.state_slot, self.state_file());
                self.refresh_title();
            }
            Key::KeyL if !ctrl && !self.bound(code) => {
                let studio = !opencdi::mcd212::studio_levels();
                opencdi::mcd212::set_studio_levels(studio);
                info!("Video levels: {}", if studio { "studio (black 16)" } else { "full (black 0)" });
            }
            Key::KeyM if !self.bound(code) => {
                self.muted = !self.muted;
                self.publish_gain();
                info!("Audio: {}", if self.muted { "MUTED" } else { "UNMUTED" });
            }
            _ => {}
        }
    }

    // Returns false when the loop must stop
    fn handle_actions(&mut self) -> bool {
        while let Some(action) = pop_action() {
            match action {
                GuiAction::InsertDisc(path) => {
                    info!("Hot-swapping disc into CD-i drive: {:?}", path);
                    match self.m.bus.change_disc(&path) {
                        Ok(_) => {
                            self.current_game_name = opencdi::frontend::disc_display_name(&path);
                            self.add_recent(&path);
                            info!("New disc inserted successfully: {}", self.current_game_name);

                            let name_upper = self.current_game_name.to_uppercase();
                            let is_ntsc = self.m.bus.cdrom.as_ref().and_then(|cd| cd.is_ntsc_hint(&self.current_game_name))
                                .unwrap_or_else(|| {
                                    (name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC"))
                                        && self.m.bus.cdrom.as_ref().map_or(true, |cd| cd.disc_type != 0x20)
                                });
                            let mode = if is_ntsc {
                                info!("Detected NTSC disc: auto-configured NTSC 60 Hz video mode");
                                VideoMode::Ntsc
                            } else {
                                VideoMode::Pal
                            };
                            self.m.bus.set_video_mode(mode);
                            #[cfg(target_os = "windows")]
                            gui::update_video_menu(mode);
                        }
                        Err(e) => {
                            error!("Failed to hot-swap disc {:?}: {:?}", path, e);
                            #[cfg(target_os = "windows")]
                            gui::show_disc_error(self.hwnd(), &format!("Errore nell'inserimento del disco:\n\n{}", e));
                        }
                    }
                }
                GuiAction::OpenDisc(path) => {
                    info!("Loading and booting new disc (Reset): {:?}", path);
                    match self.m.bus.load_cdrom(&path) {
                        Ok(_) => {
                            self.current_game_name = opencdi::frontend::disc_display_name(&path);
                            self.add_recent(&path);
                            info!("Disc loaded successfully: {}", self.current_game_name);

                            if !self.dvc_disabled && !self.m.bus.dvc.is_fitted() {
                                if let Some(dvc_path) = self.dvc_arg.clone().or_else(find_dvc_rom) {
                                    if load_dvc_logged(&mut self.m, &dvc_path) {
                                        info!("Loaded Digital Video Cartridge (DVC) extension from {:?}", dvc_path);
                                        #[cfg(target_os = "windows")]
                                        gui::update_dvc_menu(true);
                                    }
                                }
                            }

                            let name_upper = self.current_game_name.to_uppercase();
                            let is_ntsc = self.m.bus.cdrom.as_ref().and_then(|cd| cd.is_ntsc_hint(&self.current_game_name))
                                .unwrap_or_else(|| {
                                    (name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC"))
                                        && self.m.bus.cdrom.as_ref().map_or(true, |cd| cd.disc_type != 0x20)
                                });
                            let mode = if is_ntsc {
                                info!("Detected NTSC disc (USA/Japan): auto-configured NTSC 60 Hz video mode");
                                VideoMode::Ntsc
                            } else {
                                VideoMode::Pal
                            };
                            self.m.bus.set_video_mode(mode);
                            #[cfg(target_os = "windows")]
                            gui::update_video_menu(mode);

                            self.m.bus.reset_system();
                            self.m.cpu.reset(&mut self.m.bus);
                            self.reset_state();
                            self.mouse_captured = false;
                            self.apply_grab();
                            self.m.bus.emulate_wait_states = true;
                            self.next_frame_time = Instant::now() + self.m.bus.mcd212.video_mode.frame_duration();
                            self.run_at = Instant::now();
                            self.reset_telemetry();
                        }
                        Err(e) => {
                            error!("Failed to load disc {:?}: {:?}", path, e);
                            #[cfg(target_os = "windows")]
                            gui::show_disc_error(self.hwnd(), &format!("Errore nel caricamento del disco:\n\n{}", e));
                        }
                    }
                }
                GuiAction::EjectDisc => {
                    info!("Ejecting disc from CD-i drive");
                    self.m.bus.slave.set_disc_inserted(false, false);
                    self.m.bus.cdrom = None;
                    self.disc_launched = false;
                    self.shell_ready_count = 0;
                    self.current_game_name = "No disc inserted (Standalone BIOS)".to_string();
                    self.pad_x = 384;
                    self.pad_y = 280;
                    self.pad_x_f = 384.0;
                    self.pad_y_f = 280.0;
                    self.mouse_captured = false;
                    self.apply_grab();
                }
                GuiAction::Reset => {
                    info!("Console reset requested");
                    self.m.bus.reset_system();
                    self.m.cpu.reset(&mut self.m.bus);
                    self.reset_state();
                    self.mouse_captured = false;
                    self.apply_grab();
                    self.m.bus.emulate_wait_states = true;
                    self.next_frame_time = Instant::now() + self.m.bus.mcd212.video_mode.frame_duration();
                    self.run_at = Instant::now();
                    self.reset_telemetry();
                }
                GuiAction::Exit => {
                    info!("Exit requested from File menu");
                    return false;
                }
                GuiAction::SetProfile(profile) => {
                    self.bindings.set_profile(profile);
                    #[cfg(target_os = "windows")]
                    gui::update_profile_menu(self.bindings.profile);
                    info!("Control profile updated to: {:?}", self.bindings.profile);
                }
                GuiAction::SetInputDevice(mode) => {
                    self.input_device_mode = mode;
                    self.bindings.input_device = mode;
                    self.bindings.save_to_ini();
                    #[cfg(target_os = "windows")]
                    gui::update_input_device_menu(mode);
                    info!("Input device mode set to: {:?}", mode);
                }
                GuiAction::SetMouseSensitivity(sens) => {
                    self.bindings.mouse_sensitivity = sens;
                    self.bindings.save_to_ini();
                    #[cfg(target_os = "windows")]
                    gui::update_mouse_sensitivity_menu(sens);
                    info!("Sensibilità mouse impostata su: {:.0}% ({:.2})", sens * 100.0, sens);
                }
                GuiAction::ReloadControls => {
                    self.bindings.load_from_ini();
                    #[cfg(target_os = "windows")]
                    gui::update_profile_menu(self.bindings.profile);
                    #[cfg(target_os = "windows")]
                    gui::update_input_device_menu(self.bindings.input_device);
                    #[cfg(target_os = "windows")]
                    gui::update_mouse_sensitivity_menu(self.bindings.mouse_sensitivity);
                    self.input_device_mode = self.bindings.input_device;
                    info!("Controls reloaded from INI file. Profile: {:?}, Device: {:?}, Sensitivity: {:.2}", self.bindings.profile, self.bindings.input_device, self.bindings.mouse_sensitivity);
                }
                GuiAction::ToggleReleaseMouse => {
                    self.mouse_captured = !self.mouse_captured;
                    self.apply_grab();
                    info!("Mouse capture: {}", if self.mouse_captured { "CAPTURED" } else { "RELEASED" });
                }
                GuiAction::ToggleMute => {
                    self.muted = !self.muted;
                    self.publish_gain();
                    info!("Audio: {}", if self.muted { "MUTED" } else { "UNMUTED" });
                }
                GuiAction::VolumeUp => {
                    self.volume = (self.volume + 0.05).min(2.0);
                    self.publish_gain();
                }
                GuiAction::VolumeDown => {
                    self.volume = (self.volume - 0.05).max(0.0);
                    self.publish_gain();
                }
                GuiAction::ToggleFullscreen => {
                    self.toggle_fullscreen();
                }
                GuiAction::TakeScreenshot => {
                    self.take_screenshot();
                }
                GuiAction::SetVideoMode(mode) => {
                    self.m.bus.set_video_mode(mode);
                    #[cfg(target_os = "windows")]
                    gui::update_video_menu(mode);
                    info!("Video mode set to: {:?}", mode);
                    self.reset_telemetry();
                }
                GuiAction::ToggleWaitStates => {
                    self.m.bus.emulate_wait_states = !self.m.bus.emulate_wait_states;
                    #[cfg(target_os = "windows")]
                    gui::update_waitstates_menu(self.m.bus.emulate_wait_states);
                    info!("Authentic Hardware Speed (Wait-States): {}", if self.m.bus.emulate_wait_states { "ENABLED (Authentic CD-i 220)" } else { "DISABLED (Turbo Zero-Wait)" });
                }
                GuiAction::ToggleDvc => {
                    if self.m.bus.dvc.is_fitted() {
                        info!("Removing Digital Video Cartridge (Base CD-i mode)");
                        self.m.bus.dvc = opencdi::dvc::Dvc::new();
                        #[cfg(target_os = "windows")]
                        gui::update_dvc_menu(false);
                    } else {
                        if let Some(dvc_path) = self.dvc_arg.clone().or_else(find_dvc_rom) {
                            if load_dvc_logged(&mut self.m, &dvc_path) {
                                info!("Inserted Digital Video Cartridge (DVC) from {:?}", dvc_path);
                                #[cfg(target_os = "windows")]
                                gui::update_dvc_menu(true);
                            }
                        } else {
                            error!("Cannot enable DVC: vmpega.rom not found in dvc/, bios/, or root directory");
                        }
                    }
                    self.m.bus.reset_system();
                    self.m.cpu.reset(&mut self.m.bus);
                    self.reset_state();
                }
                GuiAction::ShowHelp => {
                    #[cfg(target_os = "windows")]
                    gui::post_menu_command(self.hwnd(), gui::ID_HELP_CONTROLS);
                }
                GuiAction::ShowAbout => {
                    #[cfg(target_os = "windows")]
                    gui::post_menu_command(self.hwnd(), gui::ID_HELP_ABOUT);
                }
                GuiAction::QuickSaveState | GuiAction::SaveState(_) | GuiAction::QuickLoadState | GuiAction::LoadState(_) => {
                    self.state_slot = match action {
                        GuiAction::SaveState(slot) | GuiAction::LoadState(slot) => slot as u32,
                        _ => 1,
                    };
                    if matches!(action, GuiAction::QuickSaveState | GuiAction::SaveState(_)) {
                        self.save_state();
                    } else {
                        self.load_state();
                    }
                    self.refresh_title();
                }
                GuiAction::ClearNvram => {
                    if let Some(m4) = self.m.bus.mono4.as_deref_mut() {
                        m4.nvram.clear();
                    } else {
                        let _ = self.m.bus.nvram.clear();
                        if let Some(ref path) = self.nvram_path {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                    self.m.bus.reset_system();
                    self.m.cpu.reset(&mut self.m.bus);
                    self.reset_state();
                    info!("Console memory (NVRAM) cancellata e console riavviata.");
                }
                GuiAction::OpenFeedback => {
                    #[cfg(target_os = "windows")]
                    gui::open_feedback_url();
                    #[cfg(not(target_os = "windows"))]
                    let _ = std::process::Command::new("xdg-open").arg("https://github.com/sigsantilio/opencdi/issues").spawn();
                }
                GuiAction::OpenBiosDialog => {
                    #[cfg(target_os = "windows")]
                    gui::post_menu_command(self.hwnd(), gui::ID_BIOS_SELECT);
                }
                GuiAction::SelectBios(path) => {
                    self.switch_bios(&path);
                }
                GuiAction::OpenBiosFolder => {
                    let bios_dir = Path::new("bios");
                    let _ = std::fs::create_dir_all(bios_dir);
                    #[cfg(target_os = "windows")]
                    gui::open_folder(bios_dir);
                }
                GuiAction::ShowBiosInfo => {
                    #[cfg(target_os = "windows")]
                    gui::show_bios_info(
                        self.hwnd(),
                        &self.active_bios_path,
                        self.bios_report.as_ref(),
                        self.m.bus.board,
                        self.m.bus.dvc.is_fitted(),
                        self.dvc_path.as_deref(),
                    );
                }
                GuiAction::SelectDvc(path) => {
                    info!("Loading user-selected DVC ROM: {:?}", path);
                    if load_dvc_logged(&mut self.m, &path) {
                        self.dvc_path = Some(path.clone());
                        opencdi::frontend::save_dvc_preference(&path);
                        #[cfg(target_os = "windows")]
                        gui::update_dvc_menu(true);
                        info!("DVC ROM loaded successfully: {:?}", path);
                    } else {
                        #[cfg(target_os = "windows")]
                        gui::show_disc_error(self.hwnd(), &format!("The selected DVC ROM is invalid:\n\n{:?}", path));
                    }
                }
                GuiAction::OpenDvcFolder => {
                    let dvc_dir = if Path::new("dvc").is_dir() { Path::new("dvc") } else { Path::new("bios") };
                    let _ = std::fs::create_dir_all(dvc_dir);
                    #[cfg(target_os = "windows")]
                    gui::open_folder(dvc_dir);
                }
            }
        }
        true
    }

    fn autoplay(&mut self) {
        // Se un disco è presente, avvia automaticamente la riproduzione una sola volta quando la shell del BIOS è pronta
        let mut just_launched = false;
        if self.m.bus.cdrom.is_some() && !self.disc_launched {
            let m = &mut self.m;
            let (_effective_shell_base, shell_state) = if m.bus.dvc.is_fitted() {
                let s_dvc = m.bus.peek_long(0x00D08090u32.wrapping_add(0xFFFF9172)).unwrap_or(0);
                if s_dvc >= 2 {
                    (0x00D08090u32, s_dvc)
                } else {
                    (0x0000C000u32, m.bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0))
                }
            } else {
                (0x0000C000u32, m.bus.peek_long(0x0000C000u32.wrapping_add(0xFFFF9172)).unwrap_or(0))
            };

            // The shell click targets the Mono-I shell; on the 490 the user clicks Play
            if shell_state >= 2 && m.bus.mono4.is_none() && !self.no_autoplay {
                self.shell_ready_count += 1;
                let phase = self.shell_ready_count % 60;
                if phase == 1 {
                    info!("Shell BIOS pronta (stato {}): avvio automatico disco CD-i (click PLAY CD-I)...", shell_state);
                    send_pointer(&mut m.bus, 575, 335, 1);
                } else if phase == 26 {
                    send_pointer(&mut m.bus, 575, 335, 0);
                }
            }
            if ((m.cpu.pc >= 0x00010000 && m.cpu.pc < 0x00300000) || (m.cpu.pc >= 0x00D00000 && m.cpu.pc < 0x00E00000)) && self.frame_count > 250 {
                info!("Applicazione disco in esecuzione in RAM (PC=0x{:06X}), controllo passato all'utente.", m.cpu.pc);
                self.disc_launched = true;
                self.mouse_captured = true;
                just_launched = true;
                m.bus.emulate_wait_states = true;
                self.pad_x = 384;
                self.pad_y = 280;
                self.pad_x_f = 384.0;
                self.pad_y_f = 280.0;
                send_pointer(&mut m.bus, 384, 280, 0);
            }
        }
        if just_launched {
            self.apply_grab();
        }
    }

    // Keyboard, gamepad and host mouse into the slave controller, once per emulated frame
    fn update_input(&mut self) {
        let is_active = self.focused && self.host_input;

        let left_clicked = self.mouse_left || self.mouse_left_latched;
        self.mouse_left_latched = false;
        let right_clicked = self.mouse_right || self.mouse_right_latched;
        self.mouse_right_latched = false;

        // Middle mouse button (rotellina/tasto centrale): toggle mouse capture / release focus
        if is_active && self.mouse_middle && !self.prev_middle {
            self.mouse_captured = !self.mouse_captured;
            info!("Controllo mouse: {}", if self.mouse_captured { "CATTURATO (focus console, puntatore host nascosto)" } else { "LIBERO (puntatore desktop visibile, rotellina o clic per ricatturare)" });
        }
        self.prev_middle = self.mouse_middle;

        // Left click inside window re-captures mouse if released
        if is_active && !self.mouse_captured && left_clicked && self.cursor.is_some() {
            self.mouse_captured = true;
            self.swallow_left = true;
            info!("Controllo mouse: CATTURATO (focus console, puntatore host nascosto)");
        }
        self.apply_grab();

        // Directional state from Keyboard (Arrow keys, WASD, or current profile)
        let b = &self.bindings;
        let mut dir_x: i32 = 0;
        let mut dir_y: i32 = 0;
        let mut key_btn = 0;

        if is_active {
            if self.key(b.left) || self.key(Key::ArrowLeft) || self.key(Key::KeyA) {
                dir_x -= 1;
            }
            if self.key(b.right) || self.key(Key::ArrowRight) || self.key(Key::KeyD) {
                dir_x += 1;
            }
            if self.key(b.up) || self.key(Key::ArrowUp) || self.key(Key::KeyW) {
                dir_y -= 1;
            }
            if self.key(b.down) || self.key(Key::ArrowDown) || self.key(Key::KeyS) {
                dir_y += 1;
            }

            // Button 1 (Action / Sword / Start) and Button 2 (Jump / Secondary)
            // Space is fast-forward unless bound in the ini; Enter with Alt is fullscreen
            if self.key(b.btn1)
                || (self.key(Key::Enter) && !self.mods.alt_key())
                || self.key(Key::KeyZ)
                || self.key(Key::KeyJ)
            {
                key_btn |= 1;
            }
            if self.key(b.btn2)
                || self.key(Key::ShiftLeft)
                || self.key(Key::ShiftRight)
                || self.key(Key::KeyX)
                || self.key(Key::KeyK)
            {
                key_btn |= 2;
            }
        }

        // Gamepad handling via Gilrs (Xbox / PlayStation / USB)
        if let Some(ref mut g) = self.gilrs {
            while let Some(_) = g.next_event() {} // Drain events to refresh state
            for (_id, gamepad) in g.gamepads().filter(|_| self.host_input) {
                if gamepad.is_connected() {
                    let mut pad_dir_x = 0;
                    let mut pad_dir_y = 0;

                    // 1. D-Pad buttons (highest priority)
                    if gamepad.is_pressed(gilrs::Button::DPadUp) { pad_dir_y = -1; }
                    else if gamepad.is_pressed(gilrs::Button::DPadDown) { pad_dir_y = 1; }

                    if gamepad.is_pressed(gilrs::Button::DPadLeft) { pad_dir_x = -1; }
                    else if gamepad.is_pressed(gilrs::Button::DPadRight) { pad_dir_x = 1; }

                    // 2. D-Pad axes (for gamepads where D-Pad is reported as Hat/Axis, only if button not pressed)
                    if pad_dir_x == 0 {
                        let dpx = gamepad.value(gilrs::Axis::DPadX);
                        if dpx < -0.4 { pad_dir_x = -1; }
                        else if dpx > 0.4 { pad_dir_x = 1; }
                    }
                    if pad_dir_y == 0 {
                        let dpy = gamepad.value(gilrs::Axis::DPadY);
                        if dpy > 0.4 { pad_dir_y = -1; }
                        else if dpy < -0.4 { pad_dir_y = 1; }
                    }

                    // 3. Left Analog Stick (with deadzone 0.35, only if D-pad is neutral on that axis)
                    if pad_dir_x == 0 {
                        let lx = gamepad.value(gilrs::Axis::LeftStickX);
                        if lx < -0.35 { pad_dir_x = -1; }
                        else if lx > 0.35 { pad_dir_x = 1; }
                    }
                    if pad_dir_y == 0 {
                        let ly = gamepad.value(gilrs::Axis::LeftStickY);
                        if ly > 0.35 { pad_dir_y = -1; }
                        else if ly < -0.35 { pad_dir_y = 1; }
                    }

                    dir_x += pad_dir_x;
                    dir_y += pad_dir_y;

                    // Button 1: South (A / Cross)
                    if gamepad.is_pressed(gilrs::Button::South) {
                        key_btn |= 1;
                    }
                    // Button 2: East (B / Circle) or West (X / Square) -> Jump
                    if gamepad.is_pressed(gilrs::Button::East) || gamepad.is_pressed(gilrs::Button::West) {
                        key_btn |= 2;
                    }
                    // Start Button -> Confirm / Action
                    if gamepad.is_pressed(gilrs::Button::Start) {
                        key_btn |= 1;
                    }
                }
            }
        }

        // Mouse pointer: host cursor mapped through the letterbox onto the 768x560 picture
        let mut mouse_btn = 0;
        let mut mouse_x = self.pad_x;
        let mut mouse_y = self.pad_y;
        // Whole pixels go out this frame, the fraction carries over so slow drags at the edge still count
        let rel_over = (self.rel_over.0.round(), self.rel_over.1.round());
        self.rel_over = (self.rel_over.0 - rel_over.0, self.rel_over.1 - rel_over.1);

        if is_active && self.mouse_captured {
            if let Some((x, y)) = self.rel_pointer {
                (mouse_x, mouse_y) = (x.round() as i32, y.round() as i32);
            } else if let Some(pos) = self.cursor {
                (mouse_x, mouse_y) = cursor_to_cdi(pos, self.image_rect);
            }
            if self.rel_pointer.is_some() || self.cursor.is_some() {
                self.swallow_left &= self.mouse_left;
                if left_clicked && !self.swallow_left { mouse_btn |= 1; }
                if right_clicked { mouse_btn |= 2; }
            }
        }

        let m = &mut self.m;
        if m.bus.slave.device_mouse_updated {
            self.pad_x = m.bus.slave.device_mouse_x;
            self.pad_y = m.bus.slave.device_mouse_y;
            self.pad_x_f = self.pad_x as f32;
            self.pad_y_f = self.pad_y as f32;
            m.bus.slave.device_mouse_updated = false;
            if self.rel_pointer.is_some() {
                self.rel_pointer = Some((self.pad_x as f64, self.pad_y as f64));
            }
        }

        if !self.disc_launched && m.bus.cdrom.is_some() && m.bus.mono4.is_none() && !self.no_autoplay && self.shell_ready_count < 240 {
            // During automatic boot sequence, let autoplay pulse click PLAY CD-I until launched or timeout
        } else {
            match self.input_device_mode {
                InputDeviceMode::Gamepad => {
                    const PAD_SPEED: f32 = 2.5;
                    let mut moved = false;
                    let mut over = (0.0f32, 0.0f32);
                    if dir_x != 0 {
                        let x = self.pad_x_f + (dir_x.clamp(-1, 1) as f32) * PAD_SPEED;
                        self.pad_x_f = x.clamp(0.0, 767.0);
                        over.0 = x - self.pad_x_f;
                        self.pad_x = self.pad_x_f.round() as i32;
                        moved = true;
                    }
                    if dir_y != 0 {
                        let y = self.pad_y_f + (dir_y.clamp(-1, 1) as f32) * PAD_SPEED;
                        self.pad_y_f = y.clamp(0.0, 559.0);
                        over.1 = y - self.pad_y_f;
                        self.pad_y = self.pad_y_f.round() as i32;
                        moved = true;
                    }
                    if is_active && self.mouse_captured {
                        if mouse_x != self.pad_x || mouse_y != self.pad_y {
                            self.pad_x = mouse_x;
                            self.pad_y = mouse_y;
                            self.pad_x_f = mouse_x as f32;
                            self.pad_y_f = mouse_y as f32;
                            moved = true;
                        }
                    }
                    let combined_btn = key_btn | mouse_btn;
                    if moved || combined_btn != self.prev_key_btn {
                        send_pointer(&mut m.bus, self.pad_x, self.pad_y, combined_btn);
                    }
                    // The 490 reports relative moves, so a direction held against the screen edge keeps moving
                    if let Some(m4) = m.bus.mono4.as_deref_mut() {
                        m4.ikat.push_pointer(over.0.round() as i32, over.1.round() as i32);
                    }
                    self.prev_key_btn = combined_btn;
                }
                InputDeviceMode::Mouse => {
                    self.pad_x = mouse_x;
                    self.pad_y = mouse_y;
                    self.pad_x_f = mouse_x as f32;
                    self.pad_y_f = mouse_y as f32;
                    send_pointer(&mut m.bus, self.pad_x, self.pad_y, mouse_btn | key_btn);
                    if let Some(m4) = m.bus.mono4.as_deref_mut() {
                        m4.ikat.push_pointer(self.rel_over.0 as i32, self.rel_over.1 as i32);
                    }
                }
            }
        }

        // Audio hotkeys (+ / - = Volume, held)
        if self.keys.contains(&Key::Equal) || self.keys.contains(&Key::NumpadAdd) {
            self.volume = (self.volume + 0.02).min(2.0);
            self.publish_gain();
        }
        if self.keys.contains(&Key::Minus) || self.keys.contains(&Key::NumpadSubtract) {
            self.volume = (self.volume - 0.02).max(0.0);
            self.publish_gain();
        }
    }

    // Hands the volume (0 while muted) to the sound-card callback.
    fn publish_gain(&self) {
        let g = if self.muted { 0.0f32 } else { self.volume };
        self.audio_gain.store(g.to_bits(), Ordering::Relaxed);
    }

    // Start a fresh speed window (pause, video mode switch, reset, fast-forward toggle)
    fn reset_telemetry(&mut self) {
        self.fps_timer = Instant::now();
        self.telemetry_frames = 0;
        self.prev_instructions = self.m.instructions;
        self.prev_cd_sectors = self.m.bus.cdic.curr_lba;
        self.buffer_flips_interval = 0;
    }

    fn update_telemetry(&mut self) {
        self.telemetry_frames += 1;
        let m = &self.m;
        let fps_interval = if m.bus.mcd212.video_mode == VideoMode::Pal { 25 } else { 30 };
        if self.telemetry_frames < fps_interval {
            return;
        }
        let elapsed_secs = self.fps_timer.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 {
            self.measured_fps = (self.telemetry_frames as f64) / elapsed_secs;
            let inst_delta = m.instructions.saturating_sub(self.prev_instructions);
            self.measured_mips = (inst_delta as f64 / elapsed_secs) / 1_000_000.0;
            self.prev_instructions = m.instructions;

            let curr_sectors = m.bus.cdic.curr_lba;
            let sec_delta = curr_sectors.saturating_sub(self.prev_cd_sectors);
            self.measured_cd_rate = sec_delta as f64 / elapsed_secs;
            self.prev_cd_sectors = curr_sectors;

            self.measured_render_fps = (self.buffer_flips_interval as f64) / elapsed_secs;
            self.buffer_flips_interval = 0;
        }
        self.fps_timer = Instant::now();
        self.telemetry_frames = 0;

        let target_fps = m.bus.mcd212.video_mode.fps();
        let speed_pct = (self.measured_fps / target_fps) * 100.0;

        // Real-Time Anomaly Detector
        if self.frame_count > 100 && !self.fast_forward && (speed_pct > 108.0 || (speed_pct < 92.0 && speed_pct > 20.0)) {
            log::warn!(
                "[TELEMETRY SPEED ANOMALY] Speed: {:.1}% ({:.1} FPS on target {:.0} Hz) | MIPS: {:.2} | CD: {:.1} sec/s",
                speed_pct, self.measured_fps, target_fps, self.measured_mips, self.measured_cd_rate
            );
        }
        if self.measured_mips > 4.5 && m.bus.emulate_wait_states {
            log::warn!(
                "[TELEMETRY CPU ANOMALY] Unexpected CPU throughput: {:.2} MIPS (expected ~3.1-3.4 MIPS for SCC68070 at 15 MHz)",
                self.measured_mips
            );
        }

        self.refresh_title();
    }

    // Title bar from the last measurements; shows PAUSED instead of the speed while paused
    fn refresh_title(&self) {
        let m = &self.m;
        let video_mode = match m.bus.mcd212.video_mode {
            VideoMode::Pal => "50Hz (PAL)",
            VideoMode::Ntsc => "60Hz (NTSC)",
        };
        let wait_status = if m.bus.emulate_wait_states { "ON" } else { "TURBO" };
        let cd_speed_x = self.measured_cd_rate / 75.0;
        let cd_str = if self.measured_cd_rate > 0.5 {
            format!("{:.1} sec/s ({:.1}x)", self.measured_cd_rate, cd_speed_x)
        } else {
            "IDLE".to_string()
        };
        let render_str = if self.measured_render_fps > 0.5 {
            format!("{:.0} FPS", self.measured_render_fps)
        } else {
            format!("{:.0} FPS", self.measured_fps)
        };
        let speed = if self.paused {
            format!("PAUSED ({})", video_mode)
        } else {
            let speed_pct = (self.measured_fps / m.bus.mcd212.video_mode.fps()) * 100.0;
            format!("{:.0}% ({:.1} FPS {})", speed_pct, self.measured_fps, video_mode)
        };
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "OpenCDi – {} | {} | CPU: {:.2} MIPS | CD: {} | Render: {} | WaitStates: {} | Slot: {}",
                self.current_game_name, speed, self.measured_mips, cd_str, render_str, wait_status, self.state_slot
            ));
        }
    }

    // One emulated frame plus host input; returns false when the loop must stop
    fn tick(&mut self) -> bool {
        if !self.handle_actions() {
            return false;
        }
        if self.restore_check.is_some_and(|t| Instant::now() >= t) {
            self.restore_windowed();
            self.restore_check = None;
            self.windowed = None;
        }
        if self.paused {
            // Keep the title live (disc, slot, wait states) at about two updates a second
            self.paused_ticks += 1;
            if self.paused_ticks % 25 == 0 {
                self.refresh_title();
            }
            return true;
        }
        self.autoplay();
        self.update_input();
        self.m.run_frame();
        if self.frame_count % 300 == 0 {
            flush_nvram(&mut self.m.bus);
        }

        let curr_vsr = (self.m.bus.mcd212.get_vsr::<0>(), self.m.bus.mcd212.get_vsr::<1>());
        if curr_vsr != self.prev_vsr {
            self.prev_vsr = curr_vsr;
            self.buffer_flips_interval += 1;
        }
        // Emulation runs on the timer; only the picture is skipped while the window is hidden
        self.audio_buf.clear();
        self.m.drain_audio(&mut self.audio_buf);
        if self.fast_forward {
            // Drop fast-forward audio so the ring never holds sped-up backlog
            if let Ok(mut q) = self.audio_ring.lock() {
                q.clear();
            }
        } else {
            audio::feed_ring(&self.audio_ring, &self.audio_buf);
        }
        // While fast-forwarding, present at most about 60 times a second
        let due = !self.in_modal_pump && (!self.fast_forward || self.last_present.elapsed() >= Duration::from_millis(16));
        if let Some(window) = self.window.as_ref().filter(|_| !self.occluded && due) {
            window.request_redraw();
        }

        self.frame_count += 1;
        self.update_telemetry();

        self.frames_run += 1;
        if self.max_frames.is_some_and(|n| self.frames_run >= n) {
            info!("Stopping after {} frames (--frames)", self.frames_run);
            return false;
        }
        true
    }

    // Precision host frame pacing: frames start on a fixed schedule.
    // If a frame completes late, advance to the next future target so the emulator NEVER bursts at host CPU speed.
    fn schedule_next(&mut self) {
        let target_frame_duration = self.m.bus.mcd212.video_mode.frame_duration();
        let now = Instant::now();
        if now < self.next_frame_time {
            self.run_at = self.next_frame_time;
            self.next_frame_time += target_frame_duration;
        } else {
            // Late: if lag was large (>2 frames backlog e.g. window dragging/menu), resync cleanly
            if now.duration_since(self.next_frame_time) > target_frame_duration * 2 {
                self.next_frame_time = now + target_frame_duration;
            } else {
                while self.next_frame_time <= now {
                    self.next_frame_time += target_frame_duration;
                }
            }
            self.run_at = self.next_frame_time;
        }
    }

    // --run-in-menu: steps due frames from a Win32 modal loop, where about_to_wait does not run; true if a frame ran
    #[cfg(target_os = "windows")]
    fn modal_pump(&mut self, el: &ActiveEventLoop) -> bool {
        let mut ran = false;
        for _ in 0..4 {
            if el.exiting() || Instant::now() < self.run_at {
                break;
            }
            self.in_modal_pump = true;
            let go = self.tick();
            self.in_modal_pump = false;
            ran = true;
            if !go {
                el.exit();
                gui::end_menu();
                break;
            }
            self.schedule_next();
        }
        ran
    }

    fn present(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else { return };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (std::num::NonZeroU32::new(size.width), std::num::NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        self.last_present = Instant::now();
        let (dw, dh) = (size.width as usize, size.height as usize);
        self.image_rect = image_rect(dw, dh);
        if let Ok(mut buffer) = surface.buffer_mut() {
            blit_aspect(&mut buffer, dw, dh, &self.m.bus.mcd212.framebuffer[..], self.image_rect);
            // Wayland: ties the next redraw to a frame callback, so a hidden window is never presented to
            window.pre_present_notify();
            let _ = buffer.present();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("OpenCDi - Philips CD-i")
            .with_inner_size(LogicalSize::new(SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64))
            .with_resizable(true)
            .with_window_icon(window_icon());
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                error!("Cannot create the window: {}", e);
                el.exit();
                return;
            }
        };
        let surface = softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        match surface {
            Ok(s) => self.surface = Some(s),
            Err(e) => {
                error!("Cannot create the drawing surface: {}", e);
                el.exit();
                return;
            }
        }
        let size = window.inner_size();
        self.image_rect = image_rect(size.width as usize, size.height as usize);
        self.focused = window.has_focus();
        self.window = Some(window);

        // Attach Native Win32 Menu GUI
        #[cfg(target_os = "windows")]
        {
            gui::set_run_in_modal(self.run_in_menu);
            let show_menu = !self.no_menu && !self.start_fullscreen;
            gui::attach_gui_menu(
                self.hwnd(),
                self.bindings.profile,
                self.input_device_mode,
                self.m.bus.mcd212.video_mode,
                self.bindings.mouse_sensitivity,
                show_menu,
                &self.active_bios_path,
                &self.available_bioses,
                self.m.bus.dvc.is_fitted(),
            );
            gui::update_dvc_menu(self.m.bus.dvc.is_fitted());
            // SetMenu takes the menu bar out of the client area; winit adds it back when sizing
            if show_menu {
                if let Some(window) = &self.window {
                    let _ = window.request_inner_size(LogicalSize::new(SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64));
                }
            }
        }
        if self.start_fullscreen {
            self.set_fullscreen(true);
        }
        let now = Instant::now();
        self.run_at = now;
        self.next_frame_time = now + self.m.bus.mcd212.video_mode.frame_duration();
        self.fps_timer = now;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                #[cfg(target_os = "windows")]
                if self.run_in_menu && gui::modal_active() {
                    let timer = gui::take_timer_paint();
                    if !self.modal_pump(el) && timer {
                        return;
                    }
                }
                self.present();
            }
            WindowEvent::Resized(size) => {
                self.image_rect = image_rect(size.width as usize, size.height as usize);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Focused(f) => {
                self.focused = f;
                if !f {
                    self.keys.clear();
                    self.mouse_left = false;
                    self.mouse_right = false;
                    self.mouse_middle = false;
                }
                self.apply_grab();
            }
            WindowEvent::Occluded(o) => self.occluded = o,
            // --no-host-input: keyboard and mouse never reach the emulator or the shortcuts
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } if !self.host_input => {}
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        self.keys.insert(code);
                        if !event.repeat {
                            self.on_key_pressed(el, code);
                        }
                    } else {
                        self.keys.remove(&code);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = Some(position),
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        self.mouse_left = down;
                        if down {
                            self.mouse_left_latched = true;
                        }
                    }
                    MouseButton::Right => {
                        self.mouse_right = down;
                        if down {
                            self.mouse_right_latched = true;
                        }
                    }
                    MouseButton::Middle => self.mouse_middle = down,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Relative motion in host pixels, scaled like the letterboxed picture and mouse sensitivity
        if let (DeviceEvent::MouseMotion { delta: (dx, dy) }, Some((x, y))) = (event, self.rel_pointer) {
            let (_, _, w, h) = self.image_rect;
            if w > 0 && h > 0 {
                let sens = self.bindings.mouse_sensitivity as f64;
                let (nx, ny) = (x + dx * sens * SCREEN_WIDTH as f64 / w as f64, y + dy * sens * SCREEN_HEIGHT as f64 / h as f64);
                let x = nx.clamp(0.0, (SCREEN_WIDTH - 1) as f64);
                let y = ny.clamp(0.0, (SCREEN_HEIGHT - 1) as f64);
                self.rel_over = (self.rel_over.0 + nx - x, self.rel_over.1 + ny - y);
                self.rel_pointer = Some((x, y));
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() || el.exiting() {
            return;
        }
        let ff = self.host_input && self.focused && self.key(Key::Space) && !self.bound(Key::Space);
        if ff != self.fast_forward {
            self.fast_forward = ff;
            info!("Fast-forward: {}", if ff { "ON" } else { "OFF" });
            self.reset_telemetry();
            if !ff {
                let now = Instant::now();
                self.run_at = now;
                self.next_frame_time = now + self.m.bus.mcd212.video_mode.frame_duration();
            }
        }
        let now = Instant::now();
        if !ff && now < self.run_at {
            let remaining = self.run_at - now;
            if remaining > Duration::from_millis(2) {
                el.set_control_flow(ControlFlow::WaitUntil(self.run_at - Duration::from_millis(1)));
                return;
            }
            while Instant::now() < self.run_at {
                std::hint::spin_loop();
            }
        }
        if !self.tick() {
            el.exit();
            return;
        }
        if ff {
            // Uncapped: next frame as soon as pending events are handled
            let now = Instant::now();
            self.run_at = now;
            self.next_frame_time = now + self.m.bus.mcd212.video_mode.frame_duration();
            el.set_control_flow(ControlFlow::Poll);
        } else {
            self.schedule_next();
            el.set_control_flow(ControlFlow::WaitUntil(self.run_at));
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        // Restore host cursor and release mouse confinement on exit
        self.mouse_captured = false;
        self.apply_grab();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_flag_values() {
        assert_eq!(BoardKind::parse("mono1"), Some(BoardKind::Mono1));
        assert_eq!(BoardKind::parse("mono4"), Some(BoardKind::Mono4));
        assert_eq!(BoardKind::parse("490"), None);
    }

    #[test]
    fn letterbox_matches_aspect_stretch() {
        assert_eq!(image_rect(768, 560), (0, 0, 768, 560));
        assert_eq!(image_rect(1920, 1080), (219, 0, 1481, 1080));
        assert_eq!(image_rect(768, 1000), (0, 220, 768, 560));
        let src: Vec<u32> = (0..(SCREEN_WIDTH * SCREEN_HEIGHT) as u32).collect();
        let mut dst = vec![1u32; SCREEN_WIDTH * SCREEN_HEIGHT];
        blit_aspect(&mut dst, SCREEN_WIDTH, SCREEN_HEIGHT, &src, image_rect(SCREEN_WIDTH, SCREEN_HEIGHT));
        assert_eq!(dst, src);
        let mut big = vec![1u32; 1536 * 1120];
        blit_aspect(&mut big, 1536, 1120, &src, image_rect(1536, 1120));
        assert_eq!(big[1536 * 3 + 5], src[SCREEN_WIDTH + 2]);
        assert_eq!(cursor_to_cdi(PhysicalPosition::new(100.0, 200.0), image_rect(768, 560)), (100, 200));
        assert_eq!(cursor_to_cdi(PhysicalPosition::new(219.0 + 1481.0, 1080.0), image_rect(1920, 1080)), (767, 559));
        assert_eq!(cursor_to_cdi(PhysicalPosition::new(0.0, 0.0), image_rect(1920, 1080)), (0, 0));
    }

    #[test]
    fn disc_is_the_first_positional_after_flag_values() {
        let v = |a: &[&str]| first_positional(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(v(&["--nvram", "/tmp/n.bin", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["--rom", "b.rom", "--frames", "10", "--dvc", "d.rom", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["disc.cue", "--nvram", "/tmp/n.bin"]), Some("disc.cue".into()));
        assert_eq!(v(&["--recent", "2", "--mouse"]), None);
        assert_eq!(v(&["--recent", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["--mouse", "--bios"]), None);
        assert_eq!(v(&["--mouse-speed", "0.5", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["--mouse", "--mouse-speed", "0.5", "disc.cue", "--mouse-speed", "2"]), Some("disc.cue".into()));
        assert_eq!(v(&["--screenshot-dir", "/tmp/shots", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["disc.cue", "--screenshot-dir", "/tmp/shots"]), Some("disc.cue".into()));
        assert_eq!(v(&["--monitor", "1", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["--fullscreen", "--monitor", "0", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["disc.cue", "--fullscreen", "--monitor", "1"]), Some("disc.cue".into()));
        assert_eq!(v(&["--no-autoplay", "disc.cue"]), Some("disc.cue".into()));
        assert_eq!(v(&["disc.cue", "--no-autoplay"]), Some("disc.cue".into()));
    }

    #[test]
    fn screenshot_names_are_plain_ascii() {
        assert_eq!(file_stem("Hotel Mario (Europe)"), "Hotel_Mario_Europe");
        assert_eq!(file_stem("Zelda - The Wand [USA] & co.!"), "Zelda_-_The_Wand_USA_co");
        assert_eq!(file_stem("Caf\u{e9}/x"), "Caf_x");
        assert_eq!(file_stem("()"), "opencdi");
    }

    #[test]
    fn screenshot_is_a_png_in_the_given_folder() {
        let dir = std::env::temp_dir().join(format!("opencdi-shot-test-{}", std::process::id())).join("nested");
        let fb = vec![0x00FF8040u32; SCREEN_WIDTH * SCREEN_HEIGHT];
        save_screenshot(&dir, "Hotel Mario (Europe)", 7, &fb);
        let data = std::fs::read(dir.join("Hotel_Mario_Europe_7.png")).unwrap();
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        let mut rd = png::Decoder::new(std::io::Cursor::new(&data)).read_info().unwrap();
        let mut buf = vec![0; rd.output_buffer_size().unwrap()];
        let info = rd.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
        assert_eq!(&buf[..3], &[0xFF, 0x80, 0x40]);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn state_file_round_trip_and_names() {
        let dir = std::env::temp_dir().join(format!("opencdi-state-test-{}", std::process::id()));
        assert_eq!(state_path(&dir, "Hotel Mario (Europe)", 0), dir.join("Hotel Mario (Europe).state"));
        assert_eq!(state_path(&dir, "a/b", 3), dir.join("a_b.state3"));
        let mut m = Machine::new();
        m.bus.sys_ram[7] = 0x5A;
        let p = state_path(&dir, "test", 1);
        write_state_file(&p, &m, b"front").unwrap();
        let mut n = Machine::new();
        assert_eq!(read_state_file(&p, &mut n).unwrap(), b"front");
        assert_eq!(n.bus.sys_ram[7], 0x5A);
        n.bus.rom[0] = 9;
        assert!(read_state_file(&p, &mut n).is_err());
        assert!(read_state_file(&state_path(&dir, "missing", 0), &mut n).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
    }
}
