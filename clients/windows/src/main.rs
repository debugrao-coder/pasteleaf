#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]
mod bridge;
mod design_tokens;
mod image_io;
mod layout;
mod model;
#[cfg(windows)]
mod native;
mod presentation;
fn main() {
    #[cfg(windows)]
    if let Err(message) = native::run() {
        native::show_error(&message);
    }
    #[cfg(not(windows))]
    eprintln!("PasteLeaf Windows requires Windows 10 or later. Portable UI state tests work on other hosts.");
}
