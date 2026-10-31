use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::Mutex;
use log::info;
use minifb::Key;

pub const ID_FILE_INSERT: usize = 1000;
pub const ID_FILE_OPEN: usize = 1001;
pub const ID_FILE_RESET: usize = 1002;
pub const ID_FILE_EJECT: usize = 1004;
pub const ID_FILE_EXIT: usize = 1003;

pub const ID_PHYSICAL_DRIVE_BOOT_BASE: usize = 1100;
pub const ID_PHYSICAL_DRIVE_SWAP_BASE: usize = 1130;

pub const ID_PROFILE_STANDARD: usize = 1010;
pub const ID_PROFILE_WASD: usize = 1011;
pub const ID_PROFILE_NUMPAD: usize = 1012;
pub const ID_PROFILE_CUSTOM: usize = 1015;
pub const ID_CONTROLS_EDIT_INI: usize = 1016;
pub const ID_CONTROLS_RELOAD_INI: usize = 1017;
pub const ID_CONTROLS_INFO: usize = 1013;
pub const ID_CONTROLS_RELEASE_MOUSE: usize = 1014;
pub const ID_INPUT_DEVICE_GAMEPAD: usize = 1060;
pub const ID_INPUT_DEVICE_MOUSE: usize = 1061;

pub const ID_AUDIO_MUTE: usize = 1020;
pub const ID_AUDIO_VOL_UP: usize = 1021;
pub const ID_AUDIO_VOL_DOWN: usize = 1022;

pub const ID_VIDEO_FULLSCREEN: usize = 1030;
pub const ID_VIDEO_SCREENSHOT: usize = 1031;
pub const ID_VIDEO_PAL: usize = 1032;
pub const ID_VIDEO_NTSC: usize = 1033;
pub const ID_EMU_WAITSTATES: usize = 1050;
pub const ID_EMU_DVC: usize = 1051;

pub const ID_HELP_CONTROLS: usize = 1040;
pub const ID_HELP_ABOUT: usize = 1041;
pub const ID_HELP_FEEDBACK: usize = 1042;
pub const ID_FILE_CLEAR_NVRAM: usize = 1005;

pub const ID_STATE_QUICK_SAVE: usize = 1070;
pub const ID_STATE_QUICK_LOAD: usize = 1071;
pub const ID_STATE_SAVE_BASE: usize = 1072; // 1072..1076 (Slots 1..5)
pub const ID_STATE_LOAD_BASE: usize = 1080; // 1080..1084 (Slots 1..5)

pub const ID_RECENT_IMAGE_BASE: usize = 1200;
pub const ID_RECENT_CLEAR: usize = 1210;
pub const MAX_RECENT_IMAGES: usize = 10;
const RECENT_FILE: &str = "opencdi_recent.txt";

static RECENT_IMAGES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

// Win32 Constants
const MF_STRING: u32 = 0x0000_0000;
const MF_POPUP: u32 = 0x0000_0010;
const MF_SEPARATOR: u32 = 0x0000_0800;
const MF_BYCOMMAND: u32 = 0x0000_0000;
const MF_BYPOSITION: u32 = 0x0000_0400;
const MF_CHECKED: u32 = 0x0000_0008;
const MF_UNCHECKED: u32 = 0x0000_0000;
const MF_GRAYED: u32 = 0x0000_0001;

const WM_COMMAND: u32 = 0x0111;
const WM_SETICON: u32 = 0x0080;
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const IMAGE_ICON: u32 = 1;
const LR_LOADFROMFILE: u32 = 0x0000_0010;
const LR_SHARED: u32 = 0x0000_8000;

const GWLP_WNDPROC: i32 = -4;

const MB_OK: u32 = 0x0000_0000;
const MB_YESNO: u32 = 0x0000_0004;
const MB_YESNOCANCEL: u32 = 0x0000_0003;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const MB_ICONQUESTION: u32 = 0x0000_0020;
const IDYES: i32 = 6;
const IDNO: i32 = 7;

#[repr(C)]
#[allow(non_snake_case)]
pub struct OPENFILENAMEW {
    pub lStructSize: u32,
    pub hwndOwner: *mut std::ffi::c_void,
    pub hInstance: *mut std::ffi::c_void,
    pub lpstrFilter: *const u16,
    pub lpstrCustomFilter: *mut u16,
    pub nMaxCustFilter: u32,
    pub nFilterIndex: u32,
    pub lpstrFile: *mut u16,
    pub nMaxFile: u32,
    pub lpstrFileTitle: *mut u16,
    pub nMaxFileTitle: u32,
    pub lpstrInitialDir: *const u16,
    pub lpstrTitle: *const u16,
    pub Flags: u32,
    pub nFileOffset: u16,
    pub nFileExtension: u16,
    pub lpstrDefExt: *const u16,
    pub lCustData: usize,
    pub lpfnHook: *mut std::ffi::c_void,
    pub lpTemplateName: *const u16,
    pub pvReserved: *mut std::ffi::c_void,
    pub dwReserved: u32,
    pub FlagsEx: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateMenu() -> *mut std::ffi::c_void;
    fn CreatePopupMenu() -> *mut std::ffi::c_void;
    fn AppendMenuW(hmenu: *mut std::ffi::c_void, uflags: u32, uid: usize, lpitem: *const u16) -> i32;
    fn SetMenu(hwnd: *mut std::ffi::c_void, hmenu: *mut std::ffi::c_void) -> i32;
    fn DrawMenuBar(hwnd: *mut std::ffi::c_void) -> i32;
    fn CheckMenuItem(hmenu: *mut std::ffi::c_void, uid: u32, ucheck: u32) -> u32;
    fn MessageBoxW(hwnd: *mut std::ffi::c_void, lptext: *const u16, lpcaption: *const u16, utype: u32) -> i32;
    fn SetWindowLongPtrW(hwnd: *mut std::ffi::c_void, nindex: i32, dwnewlong: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn SendMessageW(hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn LoadImageW(hinst: *mut std::ffi::c_void, name: *const u16, utype: u32, cx: i32, cy: i32, fu_load: u32) -> *mut std::ffi::c_void;
    fn DeleteMenu(hmenu: *mut std::ffi::c_void, uposition: u32, uflags: u32) -> i32;
    fn GetMenuItemCount(hmenu: *mut std::ffi::c_void) -> i32;
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(lpofn: *mut OPENFILENAMEW) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(lpRootPathName: *const u16) -> u32;
    fn GetModuleHandleW(lpmodulename: *const u16) -> *mut std::ffi::c_void;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: *mut std::ffi::c_void,
        lpoperation: *const u16,
        lpfile: *const u16,
        lpparameters: *const u16,
        lpdirectory: *const u16,
        nshowcmd: i32,
    ) -> *mut std::ffi::c_void;
}

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn get_optical_drives() -> Vec<char> {
    let mut drives = Vec::new();
    #[cfg(target_os = "windows")]
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26 {
            if (mask & (1 << i)) != 0 {
                let letter = (b'A' + i as u8) as char;
                let root = format!("{}:\\", letter);
                let wide_root = to_wide(&root);
                if GetDriveTypeW(wide_root.as_ptr()) == 5 { // 5 = DRIVE_CDROM
                    drives.push(letter);
                }
            }
        }
    }
    drives
}

pub fn load_recent_images() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Ok(content) = std::fs::read_to_string(RECENT_FILE) {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let p = PathBuf::from(trimmed);
                if p.exists() || crate::cdrom::parse_drive_letter(&p).is_some() {
                    if !list.contains(&p) && list.len() < MAX_RECENT_IMAGES {
                        list.push(p);
                    }
                }
            }
        }
    }
    list
}

pub fn save_recent_images(list: &[PathBuf]) {
    let mut content = String::new();
    for p in list.iter().take(MAX_RECENT_IMAGES) {
        content.push_str(&p.to_string_lossy());
        content.push('\n');
    }
    let _ = std::fs::write(RECENT_FILE, content);
}

pub fn add_recent_image(path: &std::path::Path) {
    {
        let mut list = RECENT_IMAGES.lock().unwrap();
        let p = path.to_path_buf();
        list.retain(|x| x != &p);
        list.insert(0, p);
        if list.len() > MAX_RECENT_IMAGES {
            list.truncate(MAX_RECENT_IMAGES);
        }
        save_recent_images(&list);
    }
    unsafe {
        if !RECENT_MENU_HANDLE.is_null() {
            rebuild_recent_menu_items(RECENT_MENU_HANDLE);
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

pub fn set_window_icon(hwnd: *mut std::ffi::c_void) {
    if hwnd.is_null() { return; }
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        // Load embedded resource icon (ID 1 compiled by winres)
        let mut hicon_big = LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 32, 32, LR_SHARED);
        let mut hicon_small = LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 16, 16, LR_SHARED);

        // Fallback to loading icon.ico from current directory if needed
        if (hicon_big.is_null() || hicon_small.is_null()) && std::path::Path::new("icon.ico").exists() {
            let wide_ico = to_wide("icon.ico");
            if hicon_big.is_null() {
                hicon_big = LoadImageW(std::ptr::null_mut(), wide_ico.as_ptr(), IMAGE_ICON, 32, 32, LR_LOADFROMFILE | LR_SHARED);
            }
            if hicon_small.is_null() {
                hicon_small = LoadImageW(std::ptr::null_mut(), wide_ico.as_ptr(), IMAGE_ICON, 16, 16, LR_LOADFROMFILE | LR_SHARED);
            }
        }

        if !hicon_big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG, hicon_big as isize);
        }
        if !hicon_small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL, hicon_small as isize);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputProfile {
    Standard, // Frecce + Z / X
    Wasd,     // W, A, S, D + J / K
    Numpad,   // Numpad 8, 4, 6, 2 + 1 / 2
    Custom,   // Personalizzato da opencdi_controls.ini
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDeviceMode {
    Gamepad, // Controller CD-i (Gamepad USB / Joystick / Tastiera centrate)
    Mouse,   // Mouse Philips CD-i (Cursore Fisico Host)
}

pub fn key_to_string(key: Key) -> &'static str {
    match key {
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::LeftShift => "LeftShift",
        Key::RightShift => "RightShift",
        Key::LeftCtrl => "LeftCtrl",
        Key::RightCtrl => "RightCtrl",
        Key::LeftAlt => "LeftAlt",
        Key::RightAlt => "RightAlt",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::NumPad0 => "NumPad0",
        Key::NumPad1 => "NumPad1",
        Key::NumPad2 => "NumPad2",
        Key::NumPad3 => "NumPad3",
        Key::NumPad4 => "NumPad4",
        Key::NumPad5 => "NumPad5",
        Key::NumPad6 => "NumPad6",
        Key::NumPad7 => "NumPad7",
        Key::NumPad8 => "NumPad8",
        Key::NumPad9 => "NumPad9",
        _ => "Z",
    }
}

pub fn string_to_key(s: &str) -> Option<Key> {
    let s_upper = s.trim().to_uppercase();
    match s_upper.as_str() {
        "UP" => Some(Key::Up),
        "DOWN" => Some(Key::Down),
        "LEFT" => Some(Key::Left),
        "RIGHT" => Some(Key::Right),
        "A" => Some(Key::A),
        "B" => Some(Key::B),
        "C" => Some(Key::C),
        "D" => Some(Key::D),
        "E" => Some(Key::E),
        "F" => Some(Key::F),
        "G" => Some(Key::G),
        "H" => Some(Key::H),
        "I" => Some(Key::I),
        "J" => Some(Key::J),
        "K" => Some(Key::K),
        "L" => Some(Key::L),
        "M" => Some(Key::M),
        "N" => Some(Key::N),
        "O" => Some(Key::O),
        "P" => Some(Key::P),
        "Q" => Some(Key::Q),
        "R" => Some(Key::R),
        "S" => Some(Key::S),
        "T" => Some(Key::T),
        "U" => Some(Key::U),
        "V" => Some(Key::V),
        "W" => Some(Key::W),
        "X" => Some(Key::X),
        "Y" => Some(Key::Y),
        "Z" => Some(Key::Z),
        "SPACE" => Some(Key::Space),
        "ENTER" | "RETURN" => Some(Key::Enter),
        "LEFTSHIFT" | "SHIFT" => Some(Key::LeftShift),
        "RIGHTSHIFT" => Some(Key::RightShift),
        "LEFTCTRL" | "CTRL" => Some(Key::LeftCtrl),
        "RIGHTCTRL" => Some(Key::RightCtrl),
        "LEFTALT" | "ALT" => Some(Key::LeftAlt),
        "RIGHTALT" => Some(Key::RightAlt),
        "TAB" => Some(Key::Tab),
        "BACKSPACE" => Some(Key::Backspace),
        "NUMPAD0" | "KP0" | "0" => Some(Key::NumPad0),
        "NUMPAD1" | "KP1" | "1" => Some(Key::NumPad1),
        "NUMPAD2" | "KP2" | "2" => Some(Key::NumPad2),
        "NUMPAD3" | "KP3" | "3" => Some(Key::NumPad3),
        "NUMPAD4" | "KP4" | "4" => Some(Key::NumPad4),
        "NUMPAD5" | "KP5" | "5" => Some(Key::NumPad5),
        "NUMPAD6" | "KP6" | "6" => Some(Key::NumPad6),
        "NUMPAD7" | "KP7" | "7" => Some(Key::NumPad7),
        "NUMPAD8" | "KP8" | "8" => Some(Key::NumPad8),
        "NUMPAD9" | "KP9" | "9" => Some(Key::NumPad9),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct KeyBindings {
    pub profile: InputProfile,
    pub input_device: InputDeviceMode,
    pub up: Key,
    pub down: Key,
    pub left: Key,
    pub right: Key,
    pub btn1: Key,
    pub btn2: Key,
}

impl KeyBindings {
    pub fn new() -> Self {
        let mut bindings = Self {
            profile: InputProfile::Standard,
            input_device: InputDeviceMode::Gamepad,
            up: Key::Up,
            down: Key::Down,
            left: Key::Left,
            right: Key::Right,
            btn1: Key::Z,
            btn2: Key::X,
        };
        bindings.load_from_ini();
        bindings
    }

    pub fn set_input_device(&mut self, mode: InputDeviceMode) {
        self.input_device = mode;
        self.save_to_ini();
    }

    pub fn set_profile(&mut self, profile: InputProfile) {
        self.profile = profile;
        match profile {
            InputProfile::Standard => {
                self.up = Key::Up;
                self.down = Key::Down;
                self.left = Key::Left;
                self.right = Key::Right;
                self.btn1 = Key::Z;
                self.btn2 = Key::X;
            }
            InputProfile::Wasd => {
                self.up = Key::W;
                self.down = Key::S;
                self.left = Key::A;
                self.right = Key::D;
                self.btn1 = Key::J;
                self.btn2 = Key::K;
            }
            InputProfile::Numpad => {
                self.up = Key::NumPad8;
                self.down = Key::NumPad2;
                self.left = Key::NumPad4;
                self.right = Key::NumPad6;
                self.btn1 = Key::NumPad1;
                self.btn2 = Key::NumPad0;
            }
            InputProfile::Custom => {
                // Keep custom key configuration intact
            }
        }
        self.save_to_ini();
    }

    pub fn save_to_ini(&self) {
        let prof_str = match self.profile {
            InputProfile::Standard => "Standard",
            InputProfile::Wasd => "WASD",
            InputProfile::Numpad => "Numpad",
            InputProfile::Custom => "Custom",
        };
        let dev_str = match self.input_device {
            InputDeviceMode::Gamepad => "Gamepad",
            InputDeviceMode::Mouse => "Mouse",
        };
        let content = format!(
            "# OpenCDi Controls Configuration\n\
             # Input Device: Gamepad | Mouse (Exclusive active input device)\n\
             input_device={}\n\
             # Profile: Standard | WASD | Numpad | Custom\n\
             profile={}\n\n\
             # Custom key bindings (active with profile=Custom or as overrides):\n\
             up={}\n\
             down={}\n\
             left={}\n\
             right={}\n\
             btn1={}\n\
             btn2={}\n",
            dev_str,
            prof_str,
            key_to_string(self.up),
            key_to_string(self.down),
            key_to_string(self.left),
            key_to_string(self.right),
            key_to_string(self.btn1),
            key_to_string(self.btn2),
        );
        let _ = std::fs::write("opencdi_controls.ini", content);
    }

    pub fn load_from_ini(&mut self) {
        if let Ok(content) = std::fs::read_to_string("opencdi_controls.ini") {
            let mut detected_custom = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split('=').map(|s| s.trim()).collect();
                if parts.len() == 2 {
                    match parts[0].to_lowercase().as_str() {
                        "input_device" => match parts[1].to_lowercase().as_str() {
                            "mouse" => self.input_device = InputDeviceMode::Mouse,
                            _ => self.input_device = InputDeviceMode::Gamepad,
                        },
                        "profile" => match parts[1].to_lowercase().as_str() {
                            "wasd" => self.set_profile(InputProfile::Wasd),
                            "numpad" => self.set_profile(InputProfile::Numpad),
                            "custom" => {
                                self.profile = InputProfile::Custom;
                                detected_custom = true;
                            }
                            _ => self.set_profile(InputProfile::Standard),
                        },
                        "up" => if let Some(k) = string_to_key(parts[1]) { self.up = k; detected_custom = true; }
                        "down" => if let Some(k) = string_to_key(parts[1]) { self.down = k; detected_custom = true; }
                        "left" => if let Some(k) = string_to_key(parts[1]) { self.left = k; detected_custom = true; }
                        "right" => if let Some(k) = string_to_key(parts[1]) { self.right = k; detected_custom = true; }
                        "btn1" => if let Some(k) = string_to_key(parts[1]) { self.btn1 = k; detected_custom = true; }
                        "btn2" => if let Some(k) = string_to_key(parts[1]) { self.btn2 = k; detected_custom = true; }
                        _ => {}
                    }
                }
            }
            if detected_custom && self.profile == InputProfile::Custom {
                info!("Comandi personalizzati caricati da opencdi_controls.ini: up={:?}, down={:?}, left={:?}, right={:?}, btn1={:?}, btn2={:?}",
                    self.up, self.down, self.left, self.right, self.btn1, self.btn2);
            }
        } else {
            self.save_to_ini();
        }
    }
}

pub enum GuiAction {
    InsertDisc(PathBuf),
    OpenDisc(PathBuf),
    EjectDisc,
    Reset,
    Exit,
    SaveState(usize),
    LoadState(usize),
    SetProfile(InputProfile),
    SetInputDevice(InputDeviceMode),
    ReloadControls,
    ToggleReleaseMouse,
    ToggleMute,
    VolumeUp,
    VolumeDown,
    ToggleFullscreen,
    TakeScreenshot,
    SetVideoMode(crate::mcd212::VideoMode),
    ToggleWaitStates,
    ToggleDvc,
    ShowHelp,
    ShowAbout,
    ClearNvram,
}

static PENDING_ACTIONS: Mutex<Vec<GuiAction>> = Mutex::new(Vec::new());
static mut PREV_WNDPROC: isize = 0;
static mut MENU_HANDLE: *mut std::ffi::c_void = std::ptr::null_mut();
static mut OWNER_HWND: *mut std::ffi::c_void = std::ptr::null_mut();
static mut RECENT_MENU_HANDLE: *mut std::ffi::c_void = std::ptr::null_mut();

pub fn rebuild_recent_menu_items(menu: *mut std::ffi::c_void) {
    if menu.is_null() { return; }
    unsafe {
        let count = GetMenuItemCount(menu);
        for _ in 0..count {
            DeleteMenu(menu, 0, MF_BYPOSITION);
        }
        let list = RECENT_IMAGES.lock().unwrap();
        if list.is_empty() {
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, to_wide("(No recent disc images)").as_ptr());
        } else {
            for (i, p) in list.iter().enumerate().take(MAX_RECENT_IMAGES) {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string());
                let label = format!("&{} {}", (i + 1) % 10, name);
                AppendMenuW(menu, MF_STRING, ID_RECENT_IMAGE_BASE + i, to_wide(&label).as_ptr());
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING, ID_RECENT_CLEAR, to_wide("Clear Recent Images History").as_ptr());
        }
    }
}

pub fn pop_action() -> Option<GuiAction> {
    let mut actions = PENDING_ACTIONS.lock().unwrap();
    if actions.is_empty() {
        None
    } else {
        Some(actions.remove(0))
    }
}

pub fn push_action(action: GuiAction) {
    PENDING_ACTIONS.lock().unwrap().push(action);
}

pub fn update_input_device_menu(mode: InputDeviceMode) {
    unsafe {
        if !MENU_HANDLE.is_null() {
            CheckMenuItem(
                MENU_HANDLE,
                ID_INPUT_DEVICE_GAMEPAD as u32,
                MF_BYCOMMAND | if mode == InputDeviceMode::Gamepad { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                MENU_HANDLE,
                ID_INPUT_DEVICE_MOUSE as u32,
                MF_BYCOMMAND | if mode == InputDeviceMode::Mouse { MF_CHECKED } else { MF_UNCHECKED },
            );
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

pub fn update_profile_menu(profile: InputProfile) {
    unsafe {
        if !MENU_HANDLE.is_null() {
            CheckMenuItem(
                MENU_HANDLE,
                ID_PROFILE_STANDARD as u32,
                MF_BYCOMMAND | if profile == InputProfile::Standard { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                MENU_HANDLE,
                ID_PROFILE_WASD as u32,
                MF_BYCOMMAND | if profile == InputProfile::Wasd { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                MENU_HANDLE,
                ID_PROFILE_NUMPAD as u32,
                MF_BYCOMMAND | if profile == InputProfile::Numpad { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                MENU_HANDLE,
                ID_PROFILE_CUSTOM as u32,
                MF_BYCOMMAND | if profile == InputProfile::Custom { MF_CHECKED } else { MF_UNCHECKED },
            );
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

pub fn update_video_menu(mode: crate::mcd212::VideoMode) {
    unsafe {
        if !MENU_HANDLE.is_null() {
            CheckMenuItem(
                MENU_HANDLE,
                ID_VIDEO_PAL as u32,
                MF_BYCOMMAND | if mode == crate::mcd212::VideoMode::Pal { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                MENU_HANDLE,
                ID_VIDEO_NTSC as u32,
                MF_BYCOMMAND | if mode == crate::mcd212::VideoMode::Ntsc { MF_CHECKED } else { MF_UNCHECKED },
            );
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

pub fn update_waitstates_menu(enabled: bool) {
    unsafe {
        if !MENU_HANDLE.is_null() {
            CheckMenuItem(
                MENU_HANDLE,
                ID_EMU_WAITSTATES as u32,
                MF_BYCOMMAND | if enabled { MF_CHECKED } else { MF_UNCHECKED },
            );
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

pub fn update_dvc_menu(fitted: bool) {
    unsafe {
        if !MENU_HANDLE.is_null() {
            CheckMenuItem(
                MENU_HANDLE,
                ID_EMU_DVC as u32,
                MF_BYCOMMAND | if fitted { MF_CHECKED } else { MF_UNCHECKED },
            );
            if !OWNER_HWND.is_null() {
                DrawMenuBar(OWNER_HWND);
            }
        }
    }
}

unsafe extern "system" fn custom_wndproc(
    hwnd: *mut std::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if msg == WM_COMMAND {
        let menu_id = (wparam & 0xFFFF) as usize;
        if menu_id >= ID_PHYSICAL_DRIVE_BOOT_BASE && menu_id < ID_PHYSICAL_DRIVE_BOOT_BASE + 26 {
            let letter = (b'A' + (menu_id - ID_PHYSICAL_DRIVE_BOOT_BASE) as u8) as char;
            push_action(GuiAction::OpenDisc(PathBuf::from(format!("{}:", letter))));
            return 0;
        }
        if menu_id >= ID_PHYSICAL_DRIVE_SWAP_BASE && menu_id < ID_PHYSICAL_DRIVE_SWAP_BASE + 26 {
            let letter = (b'A' + (menu_id - ID_PHYSICAL_DRIVE_SWAP_BASE) as u8) as char;
            push_action(GuiAction::InsertDisc(PathBuf::from(format!("{}:", letter))));
            return 0;
        }
        if menu_id >= ID_RECENT_IMAGE_BASE && menu_id < ID_RECENT_IMAGE_BASE + MAX_RECENT_IMAGES {
            let idx = menu_id - ID_RECENT_IMAGE_BASE;
            let list = RECENT_IMAGES.lock().unwrap();
            if let Some(path) = list.get(idx) {
                push_action(GuiAction::OpenDisc(path.clone()));
            }
            return 0;
        }
        if menu_id == ID_RECENT_CLEAR {
            {
                let mut list = RECENT_IMAGES.lock().unwrap();
                list.clear();
                save_recent_images(&list);
            }
            unsafe {
                if !RECENT_MENU_HANDLE.is_null() {
                    rebuild_recent_menu_items(RECENT_MENU_HANDLE);
                    if !OWNER_HWND.is_null() {
                        DrawMenuBar(OWNER_HWND);
                    }
                }
            }
            return 0;
        }
        if menu_id >= ID_STATE_SAVE_BASE && menu_id < ID_STATE_SAVE_BASE + 5 {
            let slot = (menu_id - ID_STATE_SAVE_BASE) + 1;
            push_action(GuiAction::SaveState(slot));
            return 0;
        }
        if menu_id >= ID_STATE_LOAD_BASE && menu_id < ID_STATE_LOAD_BASE + 5 {
            let slot = (menu_id - ID_STATE_LOAD_BASE) + 1;
            push_action(GuiAction::LoadState(slot));
            return 0;
        }
        match menu_id {
            ID_STATE_QUICK_SAVE => {
                push_action(GuiAction::SaveState(1));
                return 0;
            }
            ID_STATE_QUICK_LOAD => {
                push_action(GuiAction::LoadState(1));
                return 0;
            }
            ID_FILE_INSERT => {
                if let Some(path) = open_file_dialog(hwnd) {
                    push_action(GuiAction::InsertDisc(path));
                }
                return 0;
            }
            ID_FILE_OPEN => {
                if let Some(path) = open_file_dialog(hwnd) {
                    push_action(GuiAction::OpenDisc(path));
                }
                return 0;
            }
            ID_FILE_EJECT => {
                push_action(GuiAction::EjectDisc);
                return 0;
            }
            ID_FILE_RESET => {
                push_action(GuiAction::Reset);
                return 0;
            }
            ID_FILE_EXIT => {
                push_action(GuiAction::Exit);
                return 0;
            }
            ID_PROFILE_STANDARD => {
                push_action(GuiAction::SetProfile(InputProfile::Standard));
                return 0;
            }
            ID_PROFILE_WASD => {
                push_action(GuiAction::SetProfile(InputProfile::Wasd));
                return 0;
            }
            ID_PROFILE_NUMPAD => {
                push_action(GuiAction::SetProfile(InputProfile::Numpad));
                return 0;
            }
            ID_PROFILE_CUSTOM => {
                push_action(GuiAction::SetProfile(InputProfile::Custom));
                return 0;
            }
            ID_INPUT_DEVICE_GAMEPAD => {
                push_action(GuiAction::SetInputDevice(InputDeviceMode::Gamepad));
                return 0;
            }
            ID_INPUT_DEVICE_MOUSE => {
                push_action(GuiAction::SetInputDevice(InputDeviceMode::Mouse));
                return 0;
            }
            ID_CONTROLS_EDIT_INI => {
                let _ = std::process::Command::new("notepad.exe").arg("opencdi_controls.ini").spawn();
                push_action(GuiAction::SetProfile(InputProfile::Custom));
                return 0;
            }
            ID_CONTROLS_RELOAD_INI => {
                push_action(GuiAction::ReloadControls);
                return 0;
            }
            ID_CONTROLS_INFO => {
                show_controls_dialog(hwnd);
                return 0;
            }
            ID_CONTROLS_RELEASE_MOUSE => {
                push_action(GuiAction::ToggleReleaseMouse);
                return 0;
            }
            ID_AUDIO_MUTE => {
                push_action(GuiAction::ToggleMute);
                return 0;
            }
            ID_AUDIO_VOL_UP => {
                push_action(GuiAction::VolumeUp);
                return 0;
            }
            ID_AUDIO_VOL_DOWN => {
                push_action(GuiAction::VolumeDown);
                return 0;
            }
            ID_VIDEO_FULLSCREEN => {
                push_action(GuiAction::ToggleFullscreen);
                return 0;
            }
            ID_VIDEO_SCREENSHOT => {
                push_action(GuiAction::TakeScreenshot);
                return 0;
            }
            ID_VIDEO_PAL => {
                push_action(GuiAction::SetVideoMode(crate::mcd212::VideoMode::Pal));
                return 0;
            }
            ID_VIDEO_NTSC => {
                push_action(GuiAction::SetVideoMode(crate::mcd212::VideoMode::Ntsc));
                return 0;
            }
            ID_EMU_WAITSTATES => {
                push_action(GuiAction::ToggleWaitStates);
                return 0;
            }
            ID_EMU_DVC => {
                push_action(GuiAction::ToggleDvc);
                return 0;
            }
            ID_HELP_CONTROLS => {
                show_controls_dialog(hwnd);
                return 0;
            }
            ID_FILE_CLEAR_NVRAM => {
                let confirm = unsafe {
                    MessageBoxW(
                        hwnd,
                        to_wide("Are you sure you want to clear console memory (NVRAM)?\n\nAll saved games in NVRAM will be erased and the console will be reset to factory defaults.").as_ptr(),
                        to_wide("OpenCDi - Clear Console Memory").as_ptr(),
                        MB_YESNO | MB_ICONQUESTION,
                    )
                };
                if confirm == IDYES {
                    push_action(GuiAction::ClearNvram);
                }
                return 0;
            }
            ID_HELP_FEEDBACK => {
                open_feedback_url();
                return 0;
            }
            ID_HELP_ABOUT => {
                show_about_dialog(hwnd);
                return 0;
            }
            _ => {}
        }
    }

    unsafe {
        if PREV_WNDPROC != 0 {
            CallWindowProcW(PREV_WNDPROC, hwnd, msg, wparam, lparam)
        } else {
            0
        }
    }
}

pub fn attach_gui_menu(
    hwnd: *mut std::ffi::c_void,
    current_profile: InputProfile,
    current_device_mode: InputDeviceMode,
    current_video_mode: crate::mcd212::VideoMode,
) {
    if hwnd.is_null() {
        return;
    }

    unsafe {
        OWNER_HWND = hwnd;

        let main_menu = CreateMenu();
        if main_menu.is_null() {
            return;
        }
        MENU_HANDLE = main_menu;

        // 1. File Menu
        let file_menu = CreatePopupMenu();
        AppendMenuW(file_menu, MF_STRING, ID_FILE_INSERT, to_wide("Insert Disc Image (Hot-swap)...\tCtrl+I").as_ptr());
        AppendMenuW(file_menu, MF_STRING, ID_FILE_OPEN, to_wide("Open Disc Image & Restart Console...\tCtrl+O").as_ptr());

        // Submenu: Recent Disc Images
        let recent_menu = CreatePopupMenu();
        RECENT_MENU_HANDLE = recent_menu;
        let recents = load_recent_images();
        *RECENT_IMAGES.lock().unwrap() = recents;
        rebuild_recent_menu_items(recent_menu);
        AppendMenuW(file_menu, MF_POPUP, recent_menu as usize, to_wide("Recent Disc Images").as_ptr());

        AppendMenuW(file_menu, MF_STRING, ID_FILE_EJECT, to_wide("Eject Disc").as_ptr());

        let optical_drives = get_optical_drives();
        if !optical_drives.is_empty() {
            AppendMenuW(file_menu, MF_SEPARATOR, 0, std::ptr::null());
            let drives_submenu = CreatePopupMenu();
            for &letter in &optical_drives {
                let boot_id = ID_PHYSICAL_DRIVE_BOOT_BASE + (letter as usize - 'A' as usize);
                let swap_id = ID_PHYSICAL_DRIVE_SWAP_BASE + (letter as usize - 'A' as usize);

                let single_drive_menu = CreatePopupMenu();
                AppendMenuW(
                    single_drive_menu,
                    MF_STRING,
                    boot_id,
                    to_wide(&format!("Boot Real Disc ({}:) & Restart", letter)).as_ptr(),
                );
                AppendMenuW(
                    single_drive_menu,
                    MF_STRING,
                    swap_id,
                    to_wide(&format!("Hot-swap Real Disc ({}:)", letter)).as_ptr(),
                );

                AppendMenuW(
                    drives_submenu,
                    MF_POPUP,
                    single_drive_menu as usize,
                    to_wide(&format!("Optical Drive ({}:)", letter)).as_ptr(),
                );
            }
            AppendMenuW(file_menu, MF_POPUP, drives_submenu as usize, to_wide("Real Optical CD Drives").as_ptr());
        }

        AppendMenuW(file_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(file_menu, MF_STRING, ID_STATE_QUICK_SAVE, to_wide("Quick Save State (Slot 1)\tF6").as_ptr());
        AppendMenuW(file_menu, MF_STRING, ID_STATE_QUICK_LOAD, to_wide("Quick Load State (Slot 1)\tF7").as_ptr());

        let save_state_menu = CreatePopupMenu();
        let load_state_menu = CreatePopupMenu();
        for slot in 1..=5 {
            let save_label = if slot == 1 {
                format!("Slot &{} (Quick Save)\tF6", slot)
            } else {
                format!("Slot &{}", slot)
            };
            let load_label = if slot == 1 {
                format!("Slot &{} (Quick Load)\tF7", slot)
            } else {
                format!("Slot &{}", slot)
            };
            AppendMenuW(save_state_menu, MF_STRING, ID_STATE_SAVE_BASE + (slot - 1), to_wide(&save_label).as_ptr());
            AppendMenuW(load_state_menu, MF_STRING, ID_STATE_LOAD_BASE + (slot - 1), to_wide(&load_label).as_ptr());
        }
        AppendMenuW(file_menu, MF_POPUP, save_state_menu as usize, to_wide("Save State to Slot").as_ptr());
        AppendMenuW(file_menu, MF_POPUP, load_state_menu as usize, to_wide("Load State from Slot").as_ptr());

        AppendMenuW(file_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(file_menu, MF_STRING, ID_FILE_CLEAR_NVRAM, to_wide("Clear Console Memory (Reset NVRAM)...").as_ptr());
        AppendMenuW(file_menu, MF_STRING, ID_FILE_RESET, to_wide("Restart Console (Reset)\tF5").as_ptr());
        AppendMenuW(file_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(file_menu, MF_STRING, ID_FILE_EXIT, to_wide("Exit\tAlt+F4").as_ptr());
        AppendMenuW(main_menu, MF_POPUP, file_menu as usize, to_wide("File").as_ptr());

        // 2. Controls Menu
        let controls_menu = CreatePopupMenu();

        let device_menu = CreatePopupMenu();
        AppendMenuW(device_menu, MF_STRING, ID_INPUT_DEVICE_GAMEPAD, to_wide("CD-i Gamepad (USB Controller / Keyboard)\tF9").as_ptr());
        AppendMenuW(device_menu, MF_STRING, ID_INPUT_DEVICE_MOUSE, to_wide("Philips CD-i Mouse (Host Cursor)\tF9").as_ptr());
        AppendMenuW(controls_menu, MF_POPUP, device_menu as usize, to_wide("Input Device (Exclusive)").as_ptr());
        AppendMenuW(controls_menu, MF_SEPARATOR, 0, std::ptr::null());

        AppendMenuW(controls_menu, MF_STRING, ID_PROFILE_STANDARD, to_wide("Key Profile: Standard (Arrow Keys + Z / X)").as_ptr());
        AppendMenuW(controls_menu, MF_STRING, ID_PROFILE_WASD, to_wide("Key Profile: WASD + J / K (PC Style)").as_ptr());
        AppendMenuW(controls_menu, MF_STRING, ID_PROFILE_NUMPAD, to_wide("Key Profile: Numpad (8 4 6 2 + 1 / 0)").as_ptr());
        AppendMenuW(controls_menu, MF_STRING, ID_PROFILE_CUSTOM, to_wide("Key Profile: Custom (opencdi_controls.ini)").as_ptr());
        AppendMenuW(controls_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(controls_menu, MF_STRING, ID_CONTROLS_EDIT_INI, to_wide("Edit Controls in Notepad...").as_ptr());
        AppendMenuW(controls_menu, MF_STRING, ID_CONTROLS_RELOAD_INI, to_wide("Reload Controls from INI").as_ptr());
        AppendMenuW(controls_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(controls_menu, MF_STRING, ID_CONTROLS_RELEASE_MOUSE, to_wide("Release / Capture Mouse\tMiddle Click").as_ptr());
        AppendMenuW(controls_menu, MF_STRING, ID_CONTROLS_INFO, to_wide("View Controls Configuration...").as_ptr());
        AppendMenuW(main_menu, MF_POPUP, controls_menu as usize, to_wide("Controls").as_ptr());

        // 3. Audio Menu
        let audio_menu = CreatePopupMenu();
        AppendMenuW(audio_menu, MF_STRING, ID_AUDIO_MUTE, to_wide("Mute / Unmute Audio\tM").as_ptr());
        AppendMenuW(audio_menu, MF_STRING, ID_AUDIO_VOL_UP, to_wide("Volume Up (+)\t+").as_ptr());
        AppendMenuW(audio_menu, MF_STRING, ID_AUDIO_VOL_DOWN, to_wide("Volume Down (-)\t-").as_ptr());
        AppendMenuW(main_menu, MF_POPUP, audio_menu as usize, to_wide("Audio").as_ptr());

        // 4. Video Menu
        let video_menu = CreatePopupMenu();
        AppendMenuW(video_menu, MF_STRING, ID_VIDEO_FULLSCREEN, to_wide("Fullscreen\tF11").as_ptr());
        AppendMenuW(video_menu, MF_STRING, ID_VIDEO_SCREENSHOT, to_wide("Save Screenshot\tF12").as_ptr());
        AppendMenuW(video_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(video_menu, MF_STRING, ID_VIDEO_PAL, to_wide("PAL (50 Hz) [CD-i 220 Standard]").as_ptr());
        AppendMenuW(video_menu, MF_STRING, ID_VIDEO_NTSC, to_wide("NTSC (60 Hz)").as_ptr());
        AppendMenuW(video_menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(video_menu, MF_STRING | MF_CHECKED, ID_EMU_WAITSTATES, to_wide("Authentic Hardware Speed (Wait-States)").as_ptr());
        AppendMenuW(video_menu, MF_STRING | MF_CHECKED, ID_EMU_DVC, to_wide("Digital Video Cartridge (DVC) [vmpega.rom]").as_ptr());
        AppendMenuW(main_menu, MF_POPUP, video_menu as usize, to_wide("Video").as_ptr());

        // 5. Help Menu
        let help_menu = CreatePopupMenu();
        AppendMenuW(help_menu, MF_STRING, ID_HELP_CONTROLS, to_wide("Controls Guide...").as_ptr());
        AppendMenuW(help_menu, MF_STRING, ID_HELP_ABOUT, to_wide("About OpenCDi...").as_ptr());
        AppendMenuW(main_menu, MF_POPUP, help_menu as usize, to_wide("Help").as_ptr());

        // 6. Prominent Top-Level Send Feedback Button
        AppendMenuW(main_menu, MF_STRING, ID_HELP_FEEDBACK, to_wide("Send Feedback (F1)").as_ptr());

        SetMenu(hwnd, main_menu);
        DrawMenuBar(hwnd);
        set_window_icon(hwnd);

        update_input_device_menu(current_device_mode);
        update_profile_menu(current_profile);
        update_video_menu(current_video_mode);

        // Subclass window procedure
        let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, custom_wndproc as *const () as usize as isize);
        PREV_WNDPROC = prev;
        info!("Win32 Native Menu attached and window subclassed successfully.");
    }
}

pub fn open_file_dialog(owner: *mut std::ffi::c_void) -> Option<PathBuf> {
    let mut file_buf = [0u16; 1024];

    // Filter string: "CD-i Disc Images (*.chd, *.iso, *.bin, *.cue)\0*.chd;*.iso;*.bin;*.cue\0All Files (*.*)\0*.*\0\0"
    let mut filter: Vec<u16> = Vec::new();
    filter.extend(OsStr::new("CD-i Disc Images (*.chd;*.iso;*.bin;*.cue)").encode_wide());
    filter.push(0);
    filter.extend(OsStr::new("*.chd;*.iso;*.bin;*.cue").encode_wide());
    filter.push(0);
    filter.extend(OsStr::new("All Files (*.*)").encode_wide());
    filter.push(0);
    filter.extend(OsStr::new("*.*").encode_wide());
    filter.push(0);
    filter.push(0);

    let title = to_wide("Select CD-i Disc Image (CHD / ISO / BIN / CUE)");

    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file_buf.as_mut_ptr();
    ofn.nMaxFile = file_buf.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = 0x00000800 | 0x00001000 | 0x00000004 | 0x00000008; // OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR

    unsafe {
        if GetOpenFileNameW(&mut ofn) != 0 {
            let len = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
            let os_string = OsString::from_wide(&file_buf[..len]);
            Some(PathBuf::from(os_string))
        } else {
            None
        }
    }
}

pub fn show_controls_dialog(owner: *mut std::ffi::c_void) {
    let text = to_wide(
        "OpenCDi - Philips CD-i Controls Configuration\n\
        =================================================\n\n\
        Quick control profiles available under the 'Controls' menu:\n\n\
        1. STANDARD PROFILE (Default):\n\
           • Directional Movement / D-Pad: Arrow Keys\n\
           • Button 1 (Action / Confirm): [Z]\n\
           • Button 2 (Jump / Action 2): [X]\n\n\
        2. WASD PROFILE (PC Style):\n\
           • Movement: [W] [A] [S] [D]\n\
           • Button 1: [J]  |  Button 2: [K]\n\n\
        3. NUMPAD PROFILE:\n\
           • Movement: [8] [4] [6] [2]\n\
           • Button 1: [1]  |  Button 2: [0]\n\n\
        4. CUSTOM PROFILE:\n\
           • Configurable via 'Controls -> Edit Controls in Notepad...'\n\
           • Map any key (e.g. Space, Enter, Shift, etc.) in opencdi_controls.ini\n\n\
        MOUSE & TOUCH:\n\
           • Move host mouse to move the on-screen pointer or cursor.\n\
           • Left Click = Button 1  |  Right Click = Button 2\n\
           • Middle Click: Lock / release host mouse cursor to/from the window.\n\n\
        USB GAMEPAD / XBOX / PLAYSTATION:\n\
           • D-Pad or Left Analog Stick = Movement\n\
           • Button A / Cross = Button 1\n\
           • Button B / Circle / X = Button 2\n\
           • Start Button = Action / Pause\n\n\
        KEYBOARD SHORTCUTS:\n\
           • F6: Quick Save State (Slot 1)\n\
           • F7: Quick Load State (Slot 1)\n\
           • F9: Switch Active Input Device (Gamepad <-> Mouse)\n\
           • F12: Save Screenshot\n\
           • Ctrl+O: Open disc image (CHD / ISO / BIN)\n\
           • F5: Restart console (Reset)\n\
           • F11: Fullscreen\n\
           • M: Mute / Unmute audio\n\
           • + / - : Adjust volume"
    );
    let title = to_wide("OpenCDi - Controls & Key Mapping Guide");
    unsafe {
        MessageBoxW(owner, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

pub fn show_about_dialog(owner: *mut std::ffi::c_void) {
    let text = to_wide(
        "OpenCDi - Philips CD-i Emulator\n\
        Version: 0.1.8 (High Performance Edition)\n\
        Built with Rust & MiniFB\n\n\
        Key Emulation Features:\n\
        • Philips SCC68070 Microprocessor & OS-9 CD-RTOS kernel\n\
        • Philips MCD-212 Video Display Controller (Planes A & B, ICA/DCA display lists)\n\
        • CDIC CD-i Controller & ADPCM Audio Decoder (Levels A, B, C stereo/mono)\n\
        • Motorola 68HC05 Slave Microcontroller (HLE keyboard, mouse, RTC, NVRAM)\n\
        • Digital Video Cartridge (DVC / MPEG-1) for Full Motion Video titles\n\
        • Win32 GUI menu with disc hot-swapping and configurable controls\n\
        • Direct USB gamepad (XInput / DirectInput) and precise mouse capture\n\n\
        Special Thanks & Acknowledgements:\n\
        • The MAME Team (BSD-3-Clause CD-i drivers: cdi, mcd212, cdicdic, scc68070)\n\
        • The CDi_MiSTer project (ash2020) for FPGA hardware research\n\
        • The CD-i preservation community"
    );
    let title = to_wide("About OpenCDi");
    unsafe {
        MessageBoxW(owner, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

pub fn open_feedback_url() {
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            to_wide("open").as_ptr(),
            to_wide("https://cdi-tester-feedback.pages.dev/").as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}

pub fn show_nvram_cleared_dialog(owner: *mut std::ffi::c_void) {
    let text = to_wide(
        "Console memory (NVRAM) has been cleared successfully.\n\n\
        All saved games and preferences have been reset to factory defaults.\n\
        The console has been restarted."
    );
    let title = to_wide("OpenCDi - Memory Cleared");
    unsafe {
        MessageBoxW(owner, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
