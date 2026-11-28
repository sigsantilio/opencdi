// macOS standalone support: per-user data folder, native file dialogs and alerts, and the menu bar.
//
// The Win32 menu and dialogs in `opencdi::gui` have no macOS counterpart, so a double-clicked
// OpenCDi.app would otherwise open a window with no way to load a disc, and would try to write
// NVRAM and settings into `/` (Finder starts apps with that as the working directory).
use std::path::{Path, PathBuf};

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use winit::window::Window;

/// The emulator window, once it exists. Dialogs given one open as sheets on it; without one,
/// rfd falls back to a system alert, which is fine only before the window is created.
pub type Parent<'a> = Option<&'a Window>;

fn file_dialog(parent: Parent) -> FileDialog {
    match parent {
        Some(window) => FileDialog::new().set_parent(window),
        None => FileDialog::new(),
    }
}

fn message_dialog(parent: Parent) -> MessageDialog {
    match parent {
        Some(window) => MessageDialog::new().set_parent(window),
        None => MessageDialog::new(),
    }
}

/// True when the executable runs from inside an `.app` bundle (`*.app/Contents/MacOS/<exe>`).
pub fn running_from_app_bundle() -> bool {
    std::env::current_exe().is_ok_and(|exe| is_app_bundle_executable(&exe))
}

fn is_app_bundle_executable(exe: &Path) -> bool {
    let mut parents = exe.ancestors().skip(1);
    let (Some(macos), Some(contents), Some(bundle)) = (parents.next(), parents.next(), parents.next()) else {
        return false;
    };
    macos.file_name().is_some_and(|n| n == "MacOS")
        && contents.file_name().is_some_and(|n| n == "Contents")
        && bundle.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

/// `~/Library/Application Support/OpenCDi`.
pub fn app_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library").join("Application Support").join("OpenCDi"))
}

/// When launched from an `.app`, makes `~/Library/Application Support/OpenCDi` the working
/// directory, so the relative paths OpenCDi already uses (bios/, states/, screenshots/,
/// opencdi.ini, opencdi_controls.ini, opencdi_recent.txt, NVRAM) land in a writable per-user folder.
/// Returns the folder when it was entered.
pub fn enter_app_data_dir() -> Option<PathBuf> {
    if !running_from_app_bundle() {
        return None;
    }
    let dir = app_data_dir()?;
    if let Err(e) = std::fs::create_dir_all(dir.join("bios")) {
        log::warn!("Cannot create {:?}: {}", dir, e);
        return None;
    }
    match std::env::set_current_dir(&dir) {
        Ok(()) => Some(dir),
        Err(e) => {
            log::warn!("Cannot use {:?} as the data folder: {}", dir, e);
            None
        }
    }
}

/// Opens a folder in Finder, or a URL in the default browser.
pub fn open_in_finder_or_browser(target: &std::ffi::OsStr) {
    if let Err(e) = std::process::Command::new("open").arg(target).spawn() {
        log::warn!("Cannot open {:?}: {}", target, e);
    }
}

pub const DISC_EXTENSIONS: &[&str] = &["chd", "cue", "iso", "bin"];

pub fn pick_disc(parent: Parent, title: &str) -> Option<PathBuf> {
    file_dialog(parent)
        .set_title(title)
        .add_filter("CD-i Disc Images", DISC_EXTENSIONS)
        .add_filter("All Files", &["*"])
        .pick_file()
}

pub fn pick_bios(parent: Parent) -> Option<PathBuf> {
    file_dialog(parent)
        .set_title("Select Philips CD-i System BIOS")
        .add_filter("CD-i BIOS (ROM or ZIP)", &["rom", "bin", "zip"])
        .add_filter("All Files", &["*"])
        .pick_file()
}

pub fn pick_dvc(parent: Parent) -> Option<PathBuf> {
    file_dialog(parent)
        .set_title("Select Digital Video Cartridge ROM")
        .add_filter("DVC ROM", &["rom", "bin", "zip"])
        .add_filter("All Files", &["*"])
        .pick_file()
}

pub fn alert(parent: Parent, level: MessageLevel, title: &str, text: &str) {
    message_dialog(parent).set_level(level).set_title(title).set_description(text).set_buttons(MessageButtons::Ok).show();
}

pub fn alert_error(parent: Parent, text: &str) {
    alert(parent, MessageLevel::Error, "OpenCDi", text);
}

pub fn confirm(parent: Parent, title: &str, text: &str, ok_label: &str) -> bool {
    let result = message_dialog(parent)
        .set_level(MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(MessageButtons::OkCancelCustom(ok_label.to_string(), "Cancel".to_string()))
        .show();
    matches!(result, MessageDialogResult::Ok) || matches!(&result, MessageDialogResult::Custom(s) if s == ok_label)
}

pub enum BiosMissingChoice {
    Browse,
    ShowFolder,
    Quit,
}

pub fn ask_bios_missing() -> BiosMissingChoice {
    const BROWSE: &str = "Choose BIOS File…";
    const FOLDER: &str = "Show BIOS Folder";
    let result = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("Philips CD-i System BIOS Missing")
        .set_description(
            "OpenCDi needs an original Philips CD-i system BIOS, such as cdi220b.rom (CD-i 220, \
             recommended), cdi490a.rom (CD-i 490), or a cdimono1.zip / cdibios.zip archive.\n\n\
             Choose the file now, or open the BIOS folder and copy it there.",
        )
        .set_buttons(MessageButtons::YesNoCancelCustom(BROWSE.to_string(), FOLDER.to_string(), "Quit".to_string()))
        .show();
    match result {
        MessageDialogResult::Yes => BiosMissingChoice::Browse,
        MessageDialogResult::No => BiosMissingChoice::ShowFolder,
        MessageDialogResult::Custom(s) if s == BROWSE => BiosMissingChoice::Browse,
        MessageDialogResult::Custom(s) if s == FOLDER => BiosMissingChoice::ShowFolder,
        _ => BiosMissingChoice::Quit,
    }
}

/// Shown after the BIOS folder is opened; true when the user says the files are in place.
pub fn ask_bios_copied(folder: &Path) -> bool {
    confirm(
        None,
        "Copy the BIOS Files",
        &format!(
            "Copy one or more of these files into the folder that just opened:\n\n\
             • cdi220b.rom (CD-i 220, recommended)\n\
             • cdi490a.rom (CD-i 490, Mono-IV)\n\
             • vmpega.rom (Digital Video Cartridge, for FMV titles)\n\
             • or a cdimono1.zip / cdibios.zip archive\n\n\
             Folder: {}\n\nThen click Continue.",
            folder.display()
        ),
        "Continue",
    )
}

pub const CONTROLS_TEXT: &str = "\
Keyboard (Standard profile)
  Arrow keys: move  ·  Z: button 1  ·  X: button 2  ·  C: button 1+2 (combined)
  The WASD, Numpad and Custom profiles are set in opencdi_controls.ini.

Mouse
  Left click: button 1 (captures the mouse)  ·  Right click: button 2
  Middle click: capture / release the mouse  ·  F9: switch gamepad ↔ mouse  ·  F3: lightgun crosshair

USB gamepad (remappable in opencdi_controls.ini)
  D-pad or left stick: move  ·  A / Cross: button 1  ·  B / Circle: button 2
  X / Square: button 1+2 (combined)  ·  L1 / LB (or Tab): cycle 3-level CD-i pad speed

Shortcuts
  ⌘O: open disc and restart  ·  ⌘I: insert disc  ·  ⌘E: eject
  F5 or H: reset  ·  P: pause  ·  Space (hold): fast-forward
  F2 / F4: save / load state  ·  F6 / F7: previous / next state slot
  F8 or F12: screenshot  ·  F11 or ⌥↩: fullscreen
  M: mute  ·  L: studio / full video levels  ·  Esc: quit

You can also drag a disc image onto the window.";

pub fn show_controls(parent: Parent) {
    alert(parent, MessageLevel::Info, "OpenCDi Controls", CONTROLS_TEXT);
}

pub fn show_about(parent: Parent, data_dir: Option<&Path>) {
    let mut text = format!(
        "OpenCDi {} - Philips CD-i emulator\nhttps://github.com/sigsantilio/opencdi\n\n\
         Gianluca \"sigsantilio\", ArnoldDeRuiter and OpenCDi contributors.\n\
         Special thanks to whatever-industries (E-Di) for motion-adaptive de-interlacing (MIT) and CD-i hardware research.\n\
         CD-i drivers ported with reference to MAME (BSD-3-Clause).",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(dir) = data_dir {
        text.push_str(&format!("\n\nSettings, BIOS, saves and screenshots:\n{}", dir.display()));
    }
    alert(parent, MessageLevel::Info, "About OpenCDi", &text);
}

/// A menu bar command, handled by the app on its next loop iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    About,
    Quit,
    OpenDisc,
    InsertDisc,
    EjectDisc,
    Screenshot,
    Reset,
    Pause,
    SaveState,
    LoadState,
    Mute,
    Fullscreen,
    InputGamepad,
    InputMouse,
    CycleCrosshair,
    SelectBios,
    ShowBiosFolder,
    SelectDvc,
    ToggleDvc,
    ClearNvram,
    ShowDataFolder,
    Controls,
    ReportIssue,
}

const COMMANDS: &[(MenuCommand, &str)] = &[
    (MenuCommand::About, "about"),
    (MenuCommand::Quit, "quit"),
    (MenuCommand::OpenDisc, "open_disc"),
    (MenuCommand::InsertDisc, "insert_disc"),
    (MenuCommand::EjectDisc, "eject_disc"),
    (MenuCommand::Screenshot, "screenshot"),
    (MenuCommand::Reset, "reset"),
    (MenuCommand::Pause, "pause"),
    (MenuCommand::SaveState, "save_state"),
    (MenuCommand::LoadState, "load_state"),
    (MenuCommand::Mute, "mute"),
    (MenuCommand::Fullscreen, "fullscreen"),
    (MenuCommand::InputGamepad, "input_gamepad"),
    (MenuCommand::InputMouse, "input_mouse"),
    (MenuCommand::CycleCrosshair, "cycle_crosshair"),
    (MenuCommand::SelectBios, "select_bios"),
    (MenuCommand::ShowBiosFolder, "show_bios_folder"),
    (MenuCommand::SelectDvc, "select_dvc"),
    (MenuCommand::ToggleDvc, "toggle_dvc"),
    (MenuCommand::ClearNvram, "clear_nvram"),
    (MenuCommand::ShowDataFolder, "show_data_folder"),
    (MenuCommand::Controls, "controls"),
    (MenuCommand::ReportIssue, "report_issue"),
];

fn command_id(cmd: MenuCommand) -> &'static str {
    COMMANDS.iter().find(|(c, _)| *c == cmd).map(|(_, id)| *id).expect("every menu command has an id")
}

fn command_for_id(id: &str) -> Option<MenuCommand> {
    COMMANDS.iter().find(|(_, i)| *i == id).map(|(c, _)| *c)
}

fn item(cmd: MenuCommand, text: &str, accel: Option<Accelerator>) -> MenuItem {
    MenuItem::with_id(command_id(cmd), text, true, accel)
}

fn cmd(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, code))
}

/// The installed menu bar. It must stay alive for the menus to keep working.
pub struct MenuBar {
    _menu: Menu,
}

impl MenuBar {
    /// Builds the menu bar and installs it as the application menu. Call once the event loop runs.
    pub fn install() -> Result<Self, muda::Error> {
        let sep = PredefinedMenuItem::separator;
        let app = Submenu::with_items(
            "OpenCDi",
            true,
            &[
                &item(MenuCommand::About, "About OpenCDi", None),
                &sep(),
                &item(MenuCommand::ShowDataFolder, "Show Settings Folder", None),
                &sep(),
                &PredefinedMenuItem::hide(Some("Hide OpenCDi")),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &sep(),
                // Not the predefined Quit: that terminates the process without letting OpenCDi
                // write NVRAM, so quitting goes through the same exit path as Esc.
                &item(MenuCommand::Quit, "Quit OpenCDi", cmd(Code::KeyQ)),
            ],
        )?;
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &item(MenuCommand::OpenDisc, "Open Disc Image…", cmd(Code::KeyO)),
                &item(MenuCommand::InsertDisc, "Insert Disc Without Reset…", cmd(Code::KeyI)),
                &item(MenuCommand::EjectDisc, "Eject Disc", cmd(Code::KeyE)),
                &sep(),
                &item(MenuCommand::Screenshot, "Take Screenshot", None),
            ],
        )?;
        let emulation = Submenu::with_items(
            "Emulation",
            true,
            &[
                &item(MenuCommand::Reset, "Reset Console", cmd(Code::KeyR)),
                &item(MenuCommand::Pause, "Pause / Resume", None),
                &sep(),
                &item(MenuCommand::SaveState, "Save State", None),
                &item(MenuCommand::LoadState, "Load State", None),
                &sep(),
                &item(MenuCommand::Mute, "Mute / Unmute", None),
                &item(MenuCommand::InputGamepad, "Use Gamepad Input", None),
                &item(MenuCommand::InputMouse, "Use Mouse Input", None),
                &item(MenuCommand::CycleCrosshair, "Cycle Lightgun Crosshair", None),
            ],
        )?;
        let system = Submenu::with_items(
            "System",
            true,
            &[
                &item(MenuCommand::SelectBios, "Select BIOS…", cmd(Code::KeyB)),
                &item(MenuCommand::ShowBiosFolder, "Show BIOS Folder", None),
                &sep(),
                &item(MenuCommand::SelectDvc, "Select DVC Cartridge ROM…", None),
                &item(MenuCommand::ToggleDvc, "Enable / Disable DVC Cartridge", None),
                &sep(),
                &item(MenuCommand::ClearNvram, "Clear Console Memory (NVRAM)…", None),
            ],
        )?;
        let view = Submenu::with_items(
            "View",
            true,
            &[&item(
                MenuCommand::Fullscreen,
                "Toggle Full Screen",
                Some(Accelerator::new(Modifiers::META | Modifiers::CONTROL, Code::KeyF)),
            )],
        )?;
        let window = Submenu::with_items("Window", true, &[&PredefinedMenuItem::minimize(None)])?;
        let help = Submenu::with_items(
            "Help",
            true,
            &[
                &item(MenuCommand::Controls, "OpenCDi Controls", None),
                &item(MenuCommand::ReportIssue, "Report an Issue…", None),
            ],
        )?;
        let menu = Menu::with_items(&[&app, &file, &emulation, &system, &view, &window, &help])?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
        Ok(Self { _menu: menu })
    }

    /// Menu commands chosen since the last call.
    pub fn poll(&self) -> Vec<MenuCommand> {
        std::iter::from_fn(|| MenuEvent::receiver().try_recv().ok()).filter_map(|e| command_for_id(&e.id.0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_app_bundle_executables() {
        assert!(is_app_bundle_executable(Path::new("/Applications/OpenCDi.app/Contents/MacOS/opencdi")));
        assert!(!is_app_bundle_executable(Path::new("/usr/local/bin/opencdi")));
        assert!(!is_app_bundle_executable(Path::new("/tmp/build/release/opencdi")));
        assert!(!is_app_bundle_executable(Path::new("/Applications/OpenCDi/Contents/MacOS/opencdi")));
    }

    #[test]
    fn menu_ids_round_trip() {
        for (cmd, id) in COMMANDS {
            assert_eq!(command_for_id(id), Some(*cmd));
            assert_eq!(command_id(*cmd), *id);
        }
        assert_eq!(command_for_id("nope"), None);
    }
}
