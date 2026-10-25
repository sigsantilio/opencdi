// Host front-end state shared by every platform: key bindings, input modes and the GUI action queue.
use std::path::PathBuf;
use std::sync::Mutex;
use log::info;
use winit::keyboard::KeyCode as Key;

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
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::KeyA => "A",
        Key::KeyB => "B",
        Key::KeyC => "C",
        Key::KeyD => "D",
        Key::KeyE => "E",
        Key::KeyF => "F",
        Key::KeyG => "G",
        Key::KeyH => "H",
        Key::KeyI => "I",
        Key::KeyJ => "J",
        Key::KeyK => "K",
        Key::KeyL => "L",
        Key::KeyM => "M",
        Key::KeyN => "N",
        Key::KeyO => "O",
        Key::KeyP => "P",
        Key::KeyQ => "Q",
        Key::KeyR => "R",
        Key::KeyS => "S",
        Key::KeyT => "T",
        Key::KeyU => "U",
        Key::KeyV => "V",
        Key::KeyW => "W",
        Key::KeyX => "X",
        Key::KeyY => "Y",
        Key::KeyZ => "Z",
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::ShiftLeft => "LeftShift",
        Key::ShiftRight => "RightShift",
        Key::ControlLeft => "LeftCtrl",
        Key::ControlRight => "RightCtrl",
        Key::AltLeft => "LeftAlt",
        Key::AltRight => "RightAlt",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Numpad0 => "NumPad0",
        Key::Numpad1 => "NumPad1",
        Key::Numpad2 => "NumPad2",
        Key::Numpad3 => "NumPad3",
        Key::Numpad4 => "NumPad4",
        Key::Numpad5 => "NumPad5",
        Key::Numpad6 => "NumPad6",
        Key::Numpad7 => "NumPad7",
        Key::Numpad8 => "NumPad8",
        Key::Numpad9 => "NumPad9",
        _ => "Z",
    }
}

pub fn string_to_key(s: &str) -> Option<Key> {
    let s_upper = s.trim().to_uppercase();
    match s_upper.as_str() {
        "UP" => Some(Key::ArrowUp),
        "DOWN" => Some(Key::ArrowDown),
        "LEFT" => Some(Key::ArrowLeft),
        "RIGHT" => Some(Key::ArrowRight),
        "A" => Some(Key::KeyA),
        "B" => Some(Key::KeyB),
        "C" => Some(Key::KeyC),
        "D" => Some(Key::KeyD),
        "E" => Some(Key::KeyE),
        "F" => Some(Key::KeyF),
        "G" => Some(Key::KeyG),
        "H" => Some(Key::KeyH),
        "I" => Some(Key::KeyI),
        "J" => Some(Key::KeyJ),
        "K" => Some(Key::KeyK),
        "L" => Some(Key::KeyL),
        "M" => Some(Key::KeyM),
        "N" => Some(Key::KeyN),
        "O" => Some(Key::KeyO),
        "P" => Some(Key::KeyP),
        "Q" => Some(Key::KeyQ),
        "R" => Some(Key::KeyR),
        "S" => Some(Key::KeyS),
        "T" => Some(Key::KeyT),
        "U" => Some(Key::KeyU),
        "V" => Some(Key::KeyV),
        "W" => Some(Key::KeyW),
        "X" => Some(Key::KeyX),
        "Y" => Some(Key::KeyY),
        "Z" => Some(Key::KeyZ),
        "SPACE" => Some(Key::Space),
        "ENTER" | "RETURN" => Some(Key::Enter),
        "LEFTSHIFT" | "SHIFT" => Some(Key::ShiftLeft),
        "RIGHTSHIFT" => Some(Key::ShiftRight),
        "LEFTCTRL" | "CTRL" => Some(Key::ControlLeft),
        "RIGHTCTRL" => Some(Key::ControlRight),
        "LEFTALT" | "ALT" => Some(Key::AltLeft),
        "RIGHTALT" => Some(Key::AltRight),
        "TAB" => Some(Key::Tab),
        "BACKSPACE" => Some(Key::Backspace),
        "NUMPAD0" | "KP0" | "0" => Some(Key::Numpad0),
        "NUMPAD1" | "KP1" | "1" => Some(Key::Numpad1),
        "NUMPAD2" | "KP2" | "2" => Some(Key::Numpad2),
        "NUMPAD3" | "KP3" | "3" => Some(Key::Numpad3),
        "NUMPAD4" | "KP4" | "4" => Some(Key::Numpad4),
        "NUMPAD5" | "KP5" | "5" => Some(Key::Numpad5),
        "NUMPAD6" | "KP6" | "6" => Some(Key::Numpad6),
        "NUMPAD7" | "KP7" | "7" => Some(Key::Numpad7),
        "NUMPAD8" | "KP8" | "8" => Some(Key::Numpad8),
        "NUMPAD9" | "KP9" | "9" => Some(Key::Numpad9),
        "ARROWUP" => Some(Key::ArrowUp),
        "ARROWDOWN" => Some(Key::ArrowDown),
        "ARROWLEFT" => Some(Key::ArrowLeft),
        "ARROWRIGHT" => Some(Key::ArrowRight),
        "SHIFTLEFT" => Some(Key::ShiftLeft),
        "SHIFTRIGHT" => Some(Key::ShiftRight),
        "CONTROLLEFT" => Some(Key::ControlLeft),
        "CONTROLRIGHT" => Some(Key::ControlRight),
        "ALTLEFT" => Some(Key::AltLeft),
        "ALTRIGHT" => Some(Key::AltRight),
        // winit-style letter names such as KeyA
        k if k.len() == 4 && k.starts_with("KEY") => string_to_key(&k[3..]),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct KeyBindings {
    pub profile: InputProfile,
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
            up: Key::ArrowUp,
            down: Key::ArrowDown,
            left: Key::ArrowLeft,
            right: Key::ArrowRight,
            btn1: Key::KeyZ,
            btn2: Key::KeyX,
        };
        bindings.load_from_ini();
        bindings
    }

    // User choice: apply and write the ini
    pub fn set_profile(&mut self, profile: InputProfile) {
        self.apply_profile(profile);
        self.save_to_ini();
    }

    fn apply_profile(&mut self, profile: InputProfile) {
        self.profile = profile;
        match profile {
            InputProfile::Standard => {
                self.up = Key::ArrowUp;
                self.down = Key::ArrowDown;
                self.left = Key::ArrowLeft;
                self.right = Key::ArrowRight;
                self.btn1 = Key::KeyZ;
                self.btn2 = Key::KeyX;
            }
            InputProfile::Wasd => {
                self.up = Key::KeyW;
                self.down = Key::KeyS;
                self.left = Key::KeyA;
                self.right = Key::KeyD;
                self.btn1 = Key::KeyJ;
                self.btn2 = Key::KeyK;
            }
            InputProfile::Numpad => {
                self.up = Key::Numpad8;
                self.down = Key::Numpad2;
                self.left = Key::Numpad4;
                self.right = Key::Numpad6;
                self.btn1 = Key::Numpad1;
                self.btn2 = Key::Numpad0;
            }
            InputProfile::Custom => {
                // Keep custom key configuration intact
            }
        }
    }

    pub fn save_to_ini(&self) {
        let prof_str = match self.profile {
            InputProfile::Standard => "Standard",
            InputProfile::Wasd => "WASD",
            InputProfile::Numpad => "Numpad",
            InputProfile::Custom => "Custom",
        };
        let content = format!(
            "# OpenCDi Controls Configuration\n\
             # Profile: Standard | WASD | Numpad | Custom\n\
             profile={}\n\n\
             # Custom key bindings (active with profile=Custom or as overrides):\n\
             up={}\n\
             down={}\n\
             left={}\n\
             right={}\n\
             btn1={}\n\
             btn2={}\n",
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
                        "profile" => match parts[1].to_lowercase().as_str() {
                            "wasd" => self.apply_profile(InputProfile::Wasd),
                            "numpad" => self.apply_profile(InputProfile::Numpad),
                            "custom" => {
                                self.profile = InputProfile::Custom;
                                detected_custom = true;
                            }
                            _ => self.apply_profile(InputProfile::Standard),
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
}

static PENDING_ACTIONS: Mutex<Vec<GuiAction>> = Mutex::new(Vec::new());

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

pub const MAX_RECENT_IMAGES: usize = 10;
const RECENT_FILE: &str = "opencdi_recent.txt";

static RECENT_IMAGES: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);

// Reads opencdi_recent.txt: one path per line, newest first, missing files skipped
fn load_recent_images() -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = Vec::new();
    if let Ok(content) = std::fs::read_to_string(RECENT_FILE) {
        for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let p = PathBuf::from(line);
            if (p.exists() || crate::cdrom::parse_drive_letter(&p).is_some()) && !list.contains(&p) && list.len() < MAX_RECENT_IMAGES {
                list.push(p);
            }
        }
    }
    list
}

fn save_recent_images(list: &[PathBuf]) {
    let content: String = list.iter().take(MAX_RECENT_IMAGES).map(|p| format!("{}\n", p.to_string_lossy())).collect();
    let _ = std::fs::write(RECENT_FILE, content);
}

/// The recent disc images, newest first (at most `MAX_RECENT_IMAGES`).
pub fn recent_images() -> Vec<PathBuf> {
    RECENT_IMAGES.lock().unwrap().get_or_insert_with(load_recent_images).clone()
}

/// Moves `path` to the front of the recent list and saves it.
pub fn add_recent_image(path: &std::path::Path) {
    let mut guard = RECENT_IMAGES.lock().unwrap();
    let list = guard.get_or_insert_with(load_recent_images);
    // Stored absolute so the list works from any working directory; drive letters stay as given
    let p = if crate::cdrom::parse_drive_letter(path).is_some() { path.to_path_buf() } else { std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()) };
    list.retain(|x| x != &p);
    list.insert(0, p);
    list.truncate(MAX_RECENT_IMAGES);
    save_recent_images(list);
}

pub fn clear_recent_images() {
    *RECENT_IMAGES.lock().unwrap() = Some(Vec::new());
    save_recent_images(&[]);
}

/// Window title for a loaded disc: the image name, or the drive for a physical disc.
pub fn disc_display_name(path: &std::path::Path) -> String {
    match crate::cdrom::parse_drive_letter(path) {
        Some(letter) => format!("Real Optical CD Drive ({}:)", letter),
        None => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "CD-i Disc".to_string()),
    }
}
