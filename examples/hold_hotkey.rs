//! Isolated integration fixture: reserve one shortcut for the conflict test.
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
fn main() -> Result<(), String> {
    // SAFETY: registration is owned by this thread and released before it exits.
    unsafe {
        if RegisterHotKey(
            std::ptr::null_mut(),
            91,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            0x78,
        ) == 0
        {
            return Err("Cannot reserve test shortcut".into());
        }
        println!("Reserved Ctrl+Alt+F9 for 120 seconds.");
        std::thread::sleep(std::time::Duration::from_secs(120));
        UnregisterHotKey(std::ptr::null_mut(), 91);
    }
    Ok(())
}
