// OpenCDi Release Build - 2026-09-29 15:45
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use std::path::{Path, PathBuf};
use log::{info, error};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode as Key, ModifiersState, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use opencdi::audio::HostAudioOutput;
use opencdi::frontend::{pop_action, push_action, GuiAction, InputDeviceMode, KeyBindings};
#[cfg(target_os = "windows")]
use opencdi::gui;
use opencdi::machine::Machine;
use opencdi::mcd212::{SCREEN_WIDTH, SCREEN_HEIGHT, VideoMode};

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

  --rom <path>        System BIOS (default: search bios/, the current and the program directory)
  --frames <n>        Quit after n emulated frames
  --mouse             Start in mouse mode (host mouse drives the CD-i pointer)
  --recent [n]        List the recent disc images, or boot recent image number n
  --no-host-input     Ignore host keyboard, mouse and gamepads; never grab the cursor
  --bios, --standalone  Boot the BIOS shell without a disc
  --dvc <path>        Digital Video Cartridge ROM (default: search dvc/, bios/, the current and the program directory)
  --no-dvc, --nodvc   Boot without the Digital Video Cartridge
  --slave-debug       Log slave (mouse channel) bytes
  --run-in-menu       Windows: keep emulating while a menu or dialog is open, or the window is moved (default: paused)
  --console           Windows: show the log in the parent console or a new console window
  --help, -h          Show this text
";

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
        "bios/cdi220.rom",
        "cdi220.rom",
        "bios/cdi200.rom",
        "cdi200.rom",
        "bios/cdi.rom",
        "cdi.rom",
        "bios/cdi490a.rom",
        "cdi490a.rom",
    ];
    for &name in BIOS_CANDIDATES {
        if let Some(p) = find_file(name) {
            return Some(p);
        }
    }
    None
}

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
        if let Some(p) = find_file(name) {
            return Some(p);
        }
    }
    None
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

fn save_screenshot(frame_count: u64, framebuffer: &[u32]) {
    let _ = std::fs::create_dir_all("screenshots_hotel_mario");
    let filename = format!("screenshots_hotel_mario/screenshot_frame_{}.bmp", frame_count);
    let width = SCREEN_WIDTH as i32;
    let height = SCREEN_HEIGHT as i32;
    let file_size = 54 + (width * height * 4) as u32;
    let mut bmp = Vec::with_capacity(file_size as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&(-height).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&((width * height * 4) as u32).to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    for &pixel in framebuffer {
        bmp.push((pixel & 0xFF) as u8);
        bmp.push(((pixel >> 8) & 0xFF) as u8);
        bmp.push(((pixel >> 16) & 0xFF) as u8);
        bmp.push(0xFF);
    }
    if let Ok(_) = std::fs::write(&filename, &bmp) {
        info!("Screenshot salvato in {}", filename);
    }
}

// F8: binary PPM (no image crate needed) into screenshots/ next to the binary
fn save_screenshot_ppm(frame_count: u64, framebuffer: &[u32]) {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("screenshots")))
        .unwrap_or_else(|| PathBuf::from("screenshots"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        error!("Cannot create screenshot directory {:?}: {}", dir, e);
        return;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("opencdi_{}_frame{}.ppm", stamp, frame_count));
    let mut ppm = format!("P6\n{} {}\n255\n", SCREEN_WIDTH, SCREEN_HEIGHT).into_bytes();
    ppm.reserve(SCREEN_WIDTH * SCREEN_HEIGHT * 3);
    for &pixel in &framebuffer[..SCREEN_WIDTH * SCREEN_HEIGHT] {
        ppm.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
    }
    match std::fs::write(&path, &ppm) {
        Ok(_) => info!("Screenshot saved to {:?}", path),
        Err(e) => error!("Cannot write screenshot {:?}: {}", path, e),
    }
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

    let mut m = Machine::new();
    m.bus.slave.debug_log = has_flag("--slave-debug");
    if m.bus.slave.debug_log {
        info!("Debug Slave XBUS abilitato (--slave-debug): log byte CH0 (mouse)");
    }

    // 1. Load BIOS ROM
    let bios_path = match rom_arg.or_else(find_bios_rom) {
        Some(path) => path,
        None => {
            let error_msg =
                "OpenCDi - Required System BIOS Missing\n\
                ======================================\n\n\
                The emulator cannot start because no Philips CD-i system BIOS ROM was found.\n\n\
                Recommended File:\n\
                  • cdi220b.rom (512 KB, Philips CD-i 220 European PAL BIOS)\n\
                    SHA-1: 53360A1F21DDAC952E95306CED64186A3FC0B93E\n\
                    MD5:   AC0D468BE366779C9DF633BE98DA250A\n\n\
                Alternative Supported BIOSes:\n\
                  • cdi220.rom, cdi200.rom, cdi490a.rom\n\n\
                Where to place the BIOS ROM:\n\
                  1. In a 'bios/' subfolder next to opencdi.exe\n\
                  2. OR directly in the same folder as opencdi.exe\n\
                  3. OR pass it on the command line: --rom <path>\n\n\
                Digital Video Cartridge (DVC) for MPEG / FMV games:\n\
                  • Place 'vmpega.rom' (256 KB) in 'dvc/', 'bios/', or next to opencdi.exe.";

            error!("{}", error_msg);

            #[cfg(target_os = "windows")]
            unsafe {
                use std::os::windows::ffi::OsStrExt;
                let wide_text: Vec<u16> = std::ffi::OsStr::new(error_msg)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let wide_title: Vec<u16> = std::ffi::OsStr::new("OpenCDi - Required BIOS Missing")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                MessageBoxW(
                    std::ptr::null_mut(),
                    wide_text.as_ptr(),
                    wide_title.as_ptr(),
                    0x0000_0000 | 0x0000_0010, // MB_OK | MB_ICONERROR
                );
            }
            std::process::exit(1);
        }
    };
    match m.load_bios(&bios_path) {
        Ok(()) => info!("Loaded system BIOS from {:?}", bios_path),
        Err(e) => {
            error!("Cannot use system BIOS {:?}: {}", bios_path, e);
            eprintln!("Cannot use system BIOS {:?}: {}", bios_path, e);
            std::process::exit(1);
        }
    }

    // 2. Load NVRAM if present
    if let Some(nvram_path) = find_nvram_file() {
        let _ = m.bus.nvram.load_from_file(&nvram_path);
        info!("Loaded NVRAM from {:?}", nvram_path);
    }

    // 2b. Load DVC ROM (Digital Video Cartridge) by default if present
    let dvc_disabled = has_flag("--no-dvc") || has_flag("--nodvc");
    let dvc_arg = flag_value("--dvc").map(PathBuf::from);
    if !dvc_disabled {
        if let Some(dvc_path) = dvc_arg.clone().or_else(find_dvc_rom) {
            if let Ok(_) = m.load_dvc_rom(&dvc_path) {
                info!("Loaded Digital Video Cartridge (DVC) ROM from {:?}", dvc_path);
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
    let mut disc_arg = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--rom" || a == "--frames" || a == "--dvc" || (a == "--recent" && recent_pick.is_some()) {
            it.next();
        } else if !a.starts_with("--") {
            disc_arg = Some(a.clone());
            break;
        }
    }
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
                let is_ntsc = (name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC"))
                    && m.bus.cdrom.as_ref().map_or(true, |cd| cd.disc_type != 0x20);
                if is_ntsc {
                    info!("Detected NTSC disc (USA/Japan): auto-configured NTSC 60 Hz video mode");
                    m.bus.set_video_mode(VideoMode::Ntsc);
                } else {
                    info!("Console booted in standard PAL 50 Hz mode (CD-i 220 BIOS)");
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
    let bindings = KeyBindings::new();
    let input_device_mode = if has_flag("--mouse") { InputDeviceMode::Mouse } else { InputDeviceMode::Gamepad };

    // 6. Initialize Host Audio Playback
    let host_audio = HostAudioOutput::try_start(
        m.bus.cdic.audio.sample_queue.clone(),
        m.bus.cdic.audio.sfx_queue.clone(),
        m.bus.cdic.audio.source_rate.clone(),
    );

    // 7. Initialize USB Gamepad subsystem (Gilrs / XInput / DirectInput)
    let gilrs = gilrs::Gilrs::new().ok();
    if gilrs.is_some() {
        info!("USB Gamepad subsystem active (Xbox / PlayStation / Generic USB)");
    }

    m.bus.emulate_wait_states = true; // Authentic hardware wait-states (15.5 MHz)
    let (pad_x, pad_y) = if m.bus.cdrom.is_some() { (575, 335) } else { (384, 280) };
    let prev_vsr = (m.bus.mcd212.get_vsr::<0>(), m.bus.mcd212.get_vsr::<1>());
    let now = Instant::now();
    let mut app = App {
        m,
        bindings,
        input_device_mode,
        dvc_disabled,
        dvc_arg,
        max_frames,
        current_game_name,
        window: None,
        surface: None,
        image_rect: (0, 0, SCREEN_WIDTH, SCREEN_HEIGHT),
        windowed: None,
        restore_check: None,
        _host_audio: host_audio,
        gilrs,
        keys: HashSet::new(),
        mods: ModifiersState::empty(),
        focused: false,
        cursor: None,
        rel_pointer: None,
        mouse_left: false,
        mouse_right: false,
        mouse_middle: false,
        prev_middle: false,
        mouse_captured: true,
        host_input: !has_flag("--no-host-input"),
        run_in_menu: has_flag("--run-in-menu"),
        in_modal_pump: false,
        paused: false,
        fast_forward: false,
        last_present: now,
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
        next_frame_time: now,
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

    info!("Emulation loop started. Shortcuts: [F11]/[Alt+Enter] Fullscreen, [F5]/[H] Reset, [P] Pause, [Space] hold Fast-forward, [F8] Screenshot (PPM), [F12] Screenshot (BMP), [M] Mute, [+]/[-] Volume, [Ctrl+I] Insert Disc, [Ctrl+O] Open & Reset, [Middle Click] Capture/Release Mouse, [Esc] Quit");

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;

    #[cfg(target_os = "windows")]
    unsafe {
        timeEndPeriod(1);
    }

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
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    image_rect: (usize, usize, usize, usize),
    windowed: Option<(Option<PhysicalPosition<i32>>, PhysicalSize<u32>)>,
    restore_check: Option<Instant>,
    _host_audio: Option<HostAudioOutput>,
    gilrs: Option<gilrs::Gilrs>,
    keys: HashSet<Key>,
    mods: ModifiersState,
    focused: bool,
    cursor: Option<PhysicalPosition<f64>>,
    rel_pointer: Option<(f64, f64)>,
    mouse_left: bool,
    mouse_right: bool,
    mouse_middle: bool,
    prev_middle: bool,
    mouse_captured: bool,
    host_input: bool,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    run_in_menu: bool,
    in_modal_pump: bool,
    occluded: bool,
    paused: bool,
    fast_forward: bool,
    last_present: Instant,
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

    fn toggle_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            let full = window.fullscreen().is_none();
            if full {
                // A maximised window is restored by the platform itself
                self.windowed = Some((window.outer_position().ok(), window.inner_size())).filter(|_| !window.is_maximized());
            }
            window.set_fullscreen(if full { Some(Fullscreen::Borderless(None)) } else { None });
            self.restore_check = None;
            if !full && self.windowed.is_some() {
                self.restore_windowed();
                // Window managers may apply their own geometry after the switch (seen under WINE): check once more
                self.restore_check = Some(Instant::now() + Duration::from_millis(300));
            }
            info!("Schermo intero: {}", if full { "SCHERMO INTERO" } else { "FINESTRA" });
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

    fn on_key_pressed(&mut self, el: &ActiveEventLoop, code: Key) {
        let ctrl = self.mods.control_key();
        let alt = self.mods.alt_key();
        match code {
            Key::Escape => el.exit(),
            Key::KeyI if ctrl => self.open_dialog_action(false),
            Key::KeyO if ctrl => self.open_dialog_action(true),
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
            Key::F8 => save_screenshot_ppm(self.frame_count, &self.m.bus.mcd212.framebuffer[..]),
            Key::F12 => save_screenshot(self.frame_count, &self.m.bus.mcd212.framebuffer[..]),
            Key::KeyM if !self.bound(code) => {
                self.m.bus.cdic.audio.muted = !self.m.bus.cdic.audio.muted;
                info!("Audio: {}", if self.m.bus.cdic.audio.muted { "MUTED" } else { "UNMUTED" });
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
                                    if let Ok(_) = self.m.load_dvc_rom(&dvc_path) {
                                        info!("Loaded Digital Video Cartridge (DVC) extension from {:?}", dvc_path);
                                        #[cfg(target_os = "windows")]
                                        gui::update_dvc_menu(true);
                                    }
                                }
                            }

                            let name_upper = self.current_game_name.to_uppercase();
                            let is_ntsc = (name_upper.contains("(USA)") || name_upper.contains("(JAPAN)") || name_upper.contains("NTSC"))
                                && self.m.bus.cdrom.as_ref().map_or(true, |cd| cd.disc_type != 0x20);
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
                            self.mouse_captured = true;
                            self.m.bus.emulate_wait_states = true;
                            self.next_frame_time = Instant::now();
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
                    self.m.bus.slave.set_disc_inserted(false);
                    self.m.bus.cdrom = None;
                    self.disc_launched = false;
                    self.shell_ready_count = 0;
                    self.current_game_name = "No disc inserted (Standalone BIOS)".to_string();
                    self.pad_x = 384;
                    self.pad_y = 280;
                    self.pad_x_f = 384.0;
                    self.pad_y_f = 280.0;
                }
                GuiAction::Reset => {
                    info!("Console reset requested");
                    self.m.bus.reset_system();
                    self.m.cpu.reset(&mut self.m.bus);
                    self.reset_state();
                    self.mouse_captured = true;
                    self.m.bus.emulate_wait_states = true;
                    self.next_frame_time = Instant::now();
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
                    #[cfg(target_os = "windows")]
                    gui::update_input_device_menu(mode);
                    info!("Input device mode set to: {:?}", mode);
                }
                GuiAction::ReloadControls => {
                    self.bindings.load_from_ini();
                    #[cfg(target_os = "windows")]
                    gui::update_profile_menu(self.bindings.profile);
                    info!("Controls reloaded from INI file. Profile: {:?}", self.bindings.profile);
                }
                GuiAction::ToggleReleaseMouse => {
                    self.mouse_captured = !self.mouse_captured;
                    info!("Mouse capture: {}", if self.mouse_captured { "CAPTURED" } else { "RELEASED" });
                }
                GuiAction::ToggleMute => {
                    self.m.bus.cdic.audio.muted = !self.m.bus.cdic.audio.muted;
                    info!("Audio: {}", if self.m.bus.cdic.audio.muted { "MUTED" } else { "UNMUTED" });
                }
                GuiAction::VolumeUp => {
                    self.m.bus.cdic.audio.volume = (self.m.bus.cdic.audio.volume + 0.05).min(2.0);
                }
                GuiAction::VolumeDown => {
                    self.m.bus.cdic.audio.volume = (self.m.bus.cdic.audio.volume - 0.05).max(0.0);
                }
                GuiAction::ToggleFullscreen => {
                    self.toggle_fullscreen();
                }
                GuiAction::TakeScreenshot => {
                    save_screenshot(self.frame_count, &self.m.bus.mcd212.framebuffer[..]);
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
                            if let Ok(_) = self.m.load_dvc_rom(&dvc_path) {
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
            }
        }
        true
    }

    fn autoplay(&mut self) {
        // Se un disco è presente, avvia automaticamente la riproduzione una sola volta quando la shell del BIOS è pronta
        let m = &mut self.m;
        if m.bus.cdrom.is_some() && !self.disc_launched {
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

            if shell_state >= 2 {
                self.shell_ready_count += 1;
                if self.shell_ready_count == 1 || self.shell_ready_count == 150 {
                    m.bus.slave.update_mouse(575, 335, 0);
                } else if self.shell_ready_count == 30 || self.shell_ready_count == 180 {
                    info!("Shell BIOS pronta (stato {}): avvio automatico disco CD-i (click PLAY CD-I)...", shell_state);
                    m.bus.slave.update_mouse(575, 335, 1);
                } else if self.shell_ready_count == 50 || self.shell_ready_count == 200 {
                    m.bus.slave.update_mouse(575, 335, 0);
                }
            }
            if ((m.cpu.pc >= 0x00010000 && m.cpu.pc < 0x00300000) || (m.cpu.pc >= 0x00D00000 && m.cpu.pc < 0x00E00000)) && self.frame_count > 250 {
                info!("Applicazione disco in esecuzione in RAM (PC=0x{:06X}), controllo passato all'utente.", m.cpu.pc);
                self.disc_launched = true;
                m.bus.emulate_wait_states = true;
                self.pad_x = 384;
                self.pad_y = 280;
                self.pad_x_f = 384.0;
                self.pad_y_f = 280.0;
                m.bus.slave.update_mouse(384, 280, 0);
            }
        }
    }

    // Keyboard, gamepad and host mouse into the slave controller, once per emulated frame
    fn update_input(&mut self) {
        let is_active = self.focused && self.host_input;

        // Middle mouse button (rotellina/tasto centrale): toggle mouse capture / release focus
        if is_active && self.mouse_middle && !self.prev_middle {
            self.mouse_captured = !self.mouse_captured;
            info!("Controllo mouse: {}", if self.mouse_captured { "CATTURATO (focus console, puntatore host nascosto)" } else { "LIBERO (puntatore desktop visibile, rotellina o clic per ricatturare)" });
        }
        self.prev_middle = self.mouse_middle;

        // Left click inside window re-captures mouse if released
        if is_active && !self.mouse_captured && self.mouse_left && self.cursor.is_some() {
            self.mouse_captured = true;
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

        if is_active && self.mouse_captured {
            if let Some((x, y)) = self.rel_pointer {
                (mouse_x, mouse_y) = (x.round() as i32, y.round() as i32);
            } else if let Some(pos) = self.cursor {
                (mouse_x, mouse_y) = cursor_to_cdi(pos, self.image_rect);
            }
            if self.rel_pointer.is_some() || self.cursor.is_some() {
                if self.mouse_left { mouse_btn |= 1; }
                if self.mouse_right { mouse_btn |= 2; }
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

        if !self.disc_launched && m.bus.cdrom.is_some() {
            // During automatic boot sequence, do not let host mouse drift override PLAY CD-I auto-click
        } else {
            match self.input_device_mode {
                InputDeviceMode::Gamepad => {
                    const PAD_SPEED: f32 = 2.5;
                    let mut moved = false;
                    if dir_x != 0 {
                        self.pad_x_f = (self.pad_x_f + (dir_x.clamp(-1, 1) as f32) * PAD_SPEED).clamp(0.0, 767.0);
                        self.pad_x = self.pad_x_f.round() as i32;
                        moved = true;
                    }
                    if dir_y != 0 {
                        self.pad_y_f = (self.pad_y_f + (dir_y.clamp(-1, 1) as f32) * PAD_SPEED).clamp(0.0, 559.0);
                        self.pad_y = self.pad_y_f.round() as i32;
                        moved = true;
                    }
                    if moved || key_btn != self.prev_key_btn {
                        m.bus.slave.update_mouse(self.pad_x, self.pad_y, key_btn);
                    }
                    self.prev_key_btn = key_btn;
                }
                InputDeviceMode::Mouse => {
                    self.pad_x = mouse_x;
                    self.pad_y = mouse_y;
                    self.pad_x_f = mouse_x as f32;
                    self.pad_y_f = mouse_y as f32;
                    m.bus.slave.update_mouse(self.pad_x, self.pad_y, mouse_btn | key_btn);
                }
            }
        }

        // Audio hotkeys (+ / - = Volume, held)
        let audio = &mut self.m.bus.cdic.audio;
        if self.keys.contains(&Key::Equal) || self.keys.contains(&Key::NumpadAdd) {
            audio.volume = (audio.volume + 0.02).min(2.0);
        }
        if self.keys.contains(&Key::Minus) || self.keys.contains(&Key::NumpadSubtract) {
            audio.volume = (audio.volume - 0.02).max(0.0);
        }
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
                "[TELEMETRY CPU ANOMALY] Unexpected CPU throughput: {:.2} MIPS (expected ~3.2-3.5 MIPS for SCC68070 at 15.5MHz)",
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
                "OpenCDi – {} | {} | CPU: {:.2} MIPS | CD: {} | Render: {} | WaitStates: {}",
                self.current_game_name, speed, self.measured_mips, cd_str, render_str, wait_status
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
            // Keep the title live (disc, wait states) at about two updates a second
            self.paused_ticks += 1;
            if self.paused_ticks % 25 == 0 {
                self.refresh_title();
            }
            return true;
        }
        self.autoplay();
        self.m.run_frame();

        let curr_vsr = (self.m.bus.mcd212.get_vsr::<0>(), self.m.bus.mcd212.get_vsr::<1>());
        if curr_vsr != self.prev_vsr {
            self.prev_vsr = curr_vsr;
            self.buffer_flips_interval += 1;
        }

        self.update_input();
        // Emulation runs on the timer; only the picture is skipped while the window is hidden
        if self.fast_forward {
            // Drop fast-forward audio so the queues never hold sped-up backlog
            let audio = &self.m.bus.cdic.audio;
            for q in [&audio.sample_queue, &audio.sfx_queue] {
                if let Ok(mut q) = q.lock() {
                    q.clear();
                }
            }
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

    // Precision host frame pacing: frames start on a fixed schedule, a late frame drops the backlog
    fn schedule_next(&mut self) {
        let target_frame_duration = self.m.bus.mcd212.video_mode.frame_duration();
        let now = Instant::now();
        if now < self.next_frame_time {
            self.run_at = self.next_frame_time;
            self.next_frame_time += target_frame_duration;
        } else {
            // Drop backlog immediately so emulation never bursts at host CPU speed
            self.run_at = now;
            self.next_frame_time = now + target_frame_duration;
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
            gui::attach_gui_menu(self.hwnd(), self.bindings.profile, self.input_device_mode, self.m.bus.mcd212.video_mode);
            gui::update_dvc_menu(self.m.bus.dvc.is_fitted());
            // SetMenu takes the menu bar out of the client area; winit adds it back when sizing
            if let Some(window) = &self.window {
                let _ = window.request_inner_size(LogicalSize::new(SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64));
            }
        }
        let now = Instant::now();
        self.run_at = now;
        self.next_frame_time = now;
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
                    MouseButton::Left => self.mouse_left = down,
                    MouseButton::Right => self.mouse_right = down,
                    MouseButton::Middle => self.mouse_middle = down,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Relative motion in host pixels, scaled like the letterboxed picture
        if let (DeviceEvent::MouseMotion { delta: (dx, dy) }, Some((x, y))) = (event, self.rel_pointer) {
            let (_, _, w, h) = self.image_rect;
            if w > 0 && h > 0 {
                let x = (x + dx * SCREEN_WIDTH as f64 / w as f64).clamp(0.0, (SCREEN_WIDTH - 1) as f64);
                let y = (y + dy * SCREEN_HEIGHT as f64 / h as f64).clamp(0.0, (SCREEN_HEIGHT - 1) as f64);
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
}
