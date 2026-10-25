#[cfg(windows)]
include!("main_win32.rs");

#[cfg(not(windows))]
fn main() {
    eprintln!("The opencdi window is Win32-only here; use the harness binary on other platforms.");
}
