//! Custom frame policy, handled before application state is borrowed.
use std::{
    mem::size_of,
    ptr::{from_ref, null_mut},
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Diagnostic results are also available to callers on older Windows.
pub(super) struct ThemeReport {
    pub(super) dark: i32,
    pub(super) border: i32,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(super) struct FrameReport {
    pub(super) dark: i32,
    pub(super) border: i32,
    pub(super) corner: i32,
    pub(super) transitions: i32,
    pub(super) frame_changed: bool,
}

fn set_attribute(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) -> i32 {
    // SAFETY: DWM validates the HWND; the u32 matches the BOOL/COLORREF/enum
    // buffer used by these attributes and remains live for the synchronous call.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute as u32,
            from_ref(&value).cast(),
            size_of::<u32>() as u32,
        )
    }
}

pub(super) fn set_theme(hwnd: HWND, dark: bool) -> ThemeReport {
    ThemeReport {
        dark: set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark)),
        // Theme and activation changes must never restore the system border.
        border: set_attribute(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE),
    }
}

pub(super) fn configure(hwnd: HWND, dark: bool) -> FrameReport {
    let transitions = set_attribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, 1);
    let theme = set_theme(hwnd, dark);
    let corner = set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND as u32);
    // Even an unchanged window size needs a frame recalculation on creation.
    // https://learn.microsoft.com/en-us/windows/win32/dwm/customframe
    // SAFETY: DWM attributes and the frame change target only the supplied HWND;
    // NOSIZE/NOMOVE/NOZORDER/NOACTIVATE preserve its geometry and activation.
    let frame_changed = unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) != 0
    };
    FrameReport {
        dark: theme.dark,
        border: theme.border,
        corner,
        transitions,
        frame_changed,
    }
}

/// Handle frame messages before entering any application-state borrow.
///
/// # Safety
/// The parameters must come from the window procedure for `hwnd`.
pub(super) unsafe fn message(hwnd: HWND, msg: u32, wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
    match msg {
        // Both the RECT creation form and the NCCALCSIZE_PARAMS resize form
        // retain the proposed full-window rectangle as the client rectangle.
        WM_NCCALCSIZE => Some(0),
        // Keep normal activation bookkeeping without default frame repainting.
        // https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-ncactivate
        // SAFETY: the HWND/message/WPARAM are the native procedure arguments;
        // lParam=-1 is the documented request to suppress non-client painting.
        WM_NCACTIVATE => Some(unsafe { DefWindowProcW(hwnd, msg, wp, -1) }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        mem::{size_of, zeroed},
        ptr::{null, null_mut},
        sync::atomic::{AtomicU64, Ordering},
    };
    use windows_sys::Win32::{
        Foundation::RECT, Graphics::Dwm::DwmIsCompositionEnabled,
        System::LibraryLoader::GetModuleHandleW,
    };

    thread_local! {
        static NC_CALCULATIONS: Cell<usize> = const { Cell::new(0) };
        static NC_PAINT_REQUESTS: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_NCCALCSIZE {
            NC_CALCULATIONS.with(|count| count.set(count.get() + 1));
        }
        if msg == WM_NCPAINT {
            NC_PAINT_REQUESTS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: native parameters are passed through unchanged to this window.
        unsafe {
            if let Some(result) = message(hwnd, msg, wp, lp) {
                return result;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }

    struct HiddenWindow {
        hwnd: HWND,
        class: Vec<u16>,
        requested: (i32, i32),
    }

    impl HiddenWindow {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let class: Vec<_> = format!(
                "PasteLeaf.FrameTest.{}.{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            // SAFETY: unique test-owned class and hidden window; no global hooks,
            // clipboard, hotkeys, foreground activation or user data are touched.
            unsafe {
                let mut wc: WNDCLASSW = zeroed();
                wc.lpfnWndProc = Some(window_proc);
                wc.hInstance = GetModuleHandleW(null());
                wc.lpszClassName = class.as_ptr();
                assert_ne!(RegisterClassW(&wc), 0);
                // Windows clamps sizable windows to the desktop's maximum
                // tracking dimensions, including on small CI displays.
                let width = 1200.min(GetSystemMetrics(SM_CXMAXTRACK));
                let height = 368.min(GetSystemMetrics(SM_CYMAXTRACK));
                assert!(width > 0 && height > 0);
                let hwnd = CreateWindowExW(
                    WS_EX_APPWINDOW | WS_EX_TOPMOST,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
                    0,
                    0,
                    width,
                    height,
                    null_mut(),
                    null_mut(),
                    wc.hInstance,
                    null(),
                );
                assert!(!hwnd.is_null());
                assert_eq!(IsWindowVisible(hwnd), 0);
                Self {
                    hwnd,
                    class,
                    requested: (width, height),
                }
            }
        }

        fn dimensions(&self) -> ((i32, i32), (i32, i32)) {
            // SAFETY: both output rectangles are live and the hidden HWND is owned.
            unsafe {
                let mut client: RECT = zeroed();
                let mut window: RECT = zeroed();
                assert_ne!(GetClientRect(self.hwnd, &mut client), 0);
                assert_ne!(GetWindowRect(self.hwnd, &mut window), 0);
                (
                    (client.right - client.left, client.bottom - client.top),
                    (window.right - window.left, window.bottom - window.top),
                )
            }
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            // SAFETY: only this fixture's hidden HWND and unique class are removed.
            unsafe {
                DestroyWindow(self.hwnd);
                UnregisterClassW(self.class.as_ptr(), GetModuleHandleW(null()));
            }
        }
    }

    #[repr(C)]
    struct WindowsVersion {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut WindowsVersion) -> i32;
    }

    fn has_windows_11_frame_attributes() -> bool {
        // SAFETY: repr(C) matches OSVERSIONINFOW and its declared buffer size.
        unsafe {
            let mut version: WindowsVersion = zeroed();
            version.size = size_of::<WindowsVersion>() as u32;
            RtlGetVersion(&mut version) >= 0 && version.build >= 22000
        }
    }

    #[test]
    fn created_frame_has_full_client_area_without_a_resize() {
        let before_calculations = NC_CALCULATIONS.with(Cell::get);
        let window = HiddenWindow::new();
        let (client, outer) = window.dimensions();
        assert!(NC_CALCULATIONS.with(Cell::get) > before_calculations);
        assert_eq!(outer, window.requested);
        assert_eq!(
            client, outer,
            "creation must not leave a standard sizing border"
        );
    }

    #[test]
    fn configure_recalculates_the_frame_without_changing_geometry() {
        let window = HiddenWindow::new();
        let before = window.dimensions();
        let before_calculations = NC_CALCULATIONS.with(Cell::get);
        let report = configure(window.hwnd, true);
        assert!(report.frame_changed, "frame recalculation must succeed");
        assert!(NC_CALCULATIONS.with(Cell::get) > before_calculations);
        assert_eq!(window.dimensions(), before);
        // SAFETY: owned hidden HWND remains hidden after configuration.
        assert_eq!(unsafe { IsWindowVisible(window.hwnd) }, 0);
    }

    #[test]
    fn configure_sets_supported_dwm_attributes_and_theme_can_change() {
        let window = HiddenWindow::new();
        let report = configure(window.hwnd, true);
        let mut composition = 0;
        // SAFETY: DWM writes to the live BOOL-sized output buffer.
        if unsafe { DwmIsCompositionEnabled(&mut composition) } >= 0 && composition != 0 {
            assert!(report.transitions >= 0, "DWM transitions must be disabled");
            if has_windows_11_frame_attributes() {
                assert!(report.corner >= 0, "corner attribute rejected: {report:?}");
                assert!(report.border >= 0, "border attribute rejected: {report:?}");
                assert!(
                    report.dark >= 0,
                    "dark frame attribute rejected: {report:?}"
                );
                let before_calculations = NC_CALCULATIONS.with(Cell::get);
                let theme = set_theme(window.hwnd, false);
                assert!(theme.dark >= 0 && theme.border >= 0, "{theme:?}");
                assert_eq!(NC_CALCULATIONS.with(Cell::get), before_calculations);
            }
        }
    }

    #[test]
    fn activation_preserves_state_without_requesting_nonclient_paint() {
        let window = HiddenWindow::new();
        configure(window.hwnd, true);
        NC_PAINT_REQUESTS.with(|count| count.set(0));
        // SAFETY: only the hidden test window receives activation notifications;
        // no actual foreground window or activation state is changed by SendMessage.
        unsafe {
            assert!(message(window.hwnd, WM_NCACTIVATE, 0, 0).is_some());
            assert_ne!(SendMessageW(window.hwnd, WM_NCACTIVATE, 0, 0), 0);
            SendMessageW(window.hwnd, WM_NCACTIVATE, 1, 0);
        }
        assert_eq!(NC_PAINT_REQUESTS.with(Cell::get), 0);
        // SAFETY: these messages belong to the caller's existing handlers.
        unsafe {
            assert_eq!(message(window.hwnd, WM_ERASEBKGND, 0, 0), None);
            assert_eq!(message(window.hwnd, WM_PAINT, 0, 0), None);
        }
    }

    #[test]
    fn invalid_window_reports_failure_without_panicking() {
        let report = configure(null_mut(), true);
        assert!(!report.frame_changed);
        assert!(report.dark < 0 && report.border < 0);
        assert!(report.corner < 0 && report.transitions < 0);
    }
}
