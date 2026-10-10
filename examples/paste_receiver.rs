//! Isolated, disposable native editor for end-to-end paste verification.
#![windows_subsystem = "windows"]
use std::{
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}
fn main() {
    // SAFETY: all pointers are live UTF-16 buffers for synchronous native calls;
    // the editor and message loop are owned by this UI thread.
    unsafe {
        let instance = GetModuleHandleW(null());
        let name = wide("PasteLeaf.TestReceiver");
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = instance;
        wc.lpszClassName = name.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            wide("PasteLeaf 粘贴验证（临时文本）").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            200,
            120,
            700,
            350,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
            10,
            10,
            660,
            280,
            hwnd,
            1usize as _,
            instance,
            null(),
        );
        SetForegroundWindow(hwnd);
        windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(edit);
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
