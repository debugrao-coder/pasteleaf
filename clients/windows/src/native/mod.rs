pub mod clipboard;
mod render;
use crate::{
    bridge::{Bridge, Tag},
    design_tokens::*,
    layout::{self, Action as ShelfAction},
    model::{parse_hotkey, ViewModel},
    presentation::{
        conflict_changes, conflict_version_details, content_metadata, CONFLICT_VERSIONS_PER_PAGE,
    },
};
use render::{rect, Renderer};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    mem::size_of,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{
            Com::*,
            DataExchange::*,
            LibraryLoader::*,
            Registry::*,
            SystemServices::{SS_ENDELLIPSIS, SS_NOPREFIX},
            Threading::*,
        },
        UI::{
            Controls::*,
            HiDpi::*,
            Input::{Ime::*, KeyboardAndMouse::*},
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
};

const SEARCH: u32 = 101;
const FIELD1: u32 = 110;
const FIELD2: u32 = 111;
const FIELD3: u32 = 112;
const HISTORY: u32 = 200;
const FAVORITES: u32 = 201;
const SYNC: u32 = 202;
const SETTINGS: u32 = 203;
const HIDE: u32 = 204;
const PAUSE: u32 = 205;
const BACK: u32 = 206;
const SAVE: u32 = 207;
const PREVIOUS: u32 = 208;
const NEXT: u32 = 209;
const PASTE: u32 = 210;
const COPY: u32 = 211;
const PLAIN: u32 = 212;
const QUEUE: u32 = 213;
const PREVIEW: u32 = 214;
const STAR: u32 = 215;
const TAGS: u32 = 216;
const DELETE: u32 = 217;
const ADD_CATEGORY: u32 = 218;
const CLEAR_LOCAL: u32 = 219;
const SYNC_CREATE: u32 = 220;
const SYNC_JOIN: u32 = 221;
const SYNC_UNLOCK: u32 = 222;
const SYNC_NOW: u32 = 223;
const SYNC_PAUSE: u32 = 224;
const SYNC_DISCONNECT: u32 = 225;
const CONFLICTS: u32 = 226;
const ENABLE: u32 = 227;
const EXIT: u32 = 228;
const STARTUP: u32 = 229;
const RENAME_CATEGORY: u32 = 230;
const DELETE_CATEGORY: u32 = 231;
const THEME: u32 = 232;
const QUOTA: u32 = 233;
const IMPORT: u32 = 234;
const BROWSE: u32 = 235;
const MOTION: u32 = 236;
const ASSIGN: u32 = 237;
const UNFAVORITE: u32 = 238;
const KEEP_ALL: u32 = 239;
const ACTIONS: u32 = 240;
const ASSIGN_BASE: u32 = 5000;
const RESOLVE_BASE: u32 = 6000;
const STATUS_TEXT: u32 = 150;
const CATEGORY: u32 = 2000;
const CARD: u32 = 3000;
const CONFLICT: u32 = 4000;
const CONFLICT_TEXT: u32 = 8000;
const TIMER: u32 = 1;
const SEARCH_TIMER: u32 = 2;
const PASTE_TIMER: u32 = 3;
const TRAY: u32 = WM_APP + 8;
const RELAYOUT: u32 = WM_APP + 9;
const HOTKEY: i32 = 1;
const NEXT_HOTKEY: i32 = 2;
thread_local! {static APP:RefCell<Option<App>>=const {RefCell::new(None)};}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn colorref(c: u32) -> COLORREF {
    COLORREF(((c >> 16) & 255) | (c & 0xff00) | ((c & 255) << 16))
}
fn ctrl() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}
fn shift() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|slot| {
        slot.try_borrow_mut()
            .ok()
            .and_then(|mut x| x.as_mut().map(f))
    })
}
#[derive(Clone, PartialEq)]
enum Screen {
    Main,
    Settings,
    Sync,
    Preview(String),
    Tags(String),
    Category(Option<String>),
    Confirm(String),
    Conflicts,
    Quotas,
    Collections,
    Actions,
    Import,
    AssignCategory,
    ConflictDetail(usize),
}
struct Control {
    hwnd: HWND,
    id: u32,
    label: String,
    primary: bool,
    card: Option<usize>,
}
struct DragSession {
    hwnd: HWND,
    origin: POINT,
    ids: Vec<String>,
    active: bool,
}
struct App {
    hwnd: HWND,
    bridge: Bridge,
    model: ViewModel,
    renderer: Renderer,
    controls: Vec<Control>,
    screen: Screen,
    palette: Palette,
    brush: HBRUSH,
    background_brush: HBRUSH,
    font: HFONT,
    scale: f32,
    width: f32,
    height: f32,
    demo: bool,
    target: HWND,
    root: PathBuf,
    hotkey: String,
    hotkey_registered: bool,
    next_registered: bool,
    ime: bool,
    pending_layout: bool,
    status_data: Value,
    card_first: usize,
    category_offset: usize,
    pending_anchor: Option<String>,
    hover: Option<u32>,
    drag: Option<DragSession>,
    scroll: f32,
    thumbnails: HashMap<String, image::RgbaImage>,
    last_refresh: Instant,
    last_capture: u32,
    paste_copy_only: bool,
    conflicts: Vec<Value>,
    conflict_snapshot: Option<Value>,
    conflict_offset: usize,
    version_offset: usize,
    delete_targets: Vec<String>,
    closed: bool,
    pending_paste: bool,
    queued_inflight: Option<crate::model::Item>,
    tray: NOTIFYICONDATAW,
}

struct InstanceGuard(HANDLE);
impl InstanceGuard {
    unsafe fn acquire(root: &std::path::Path) -> Result<Self, String> {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        root.canonicalize()
            .unwrap_or_else(|_| root.to_path_buf())
            .hash(&mut hash);
        let name = wide(&format!("Local\\PasteLeafV2-{:016x}", hash.finish()));
        let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr())).map_err(|e| e.to_string())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            if let Ok(hwnd) = FindWindowW(w!("PasteLeafV2Window"), None) {
                let _ = PostMessageW(hwnd, TRAY, WPARAM(0), LPARAM(WM_LBUTTONUP as isize));
            }
            return Err("此数据目录的 PasteLeaf 已在运行，已请求显示现有窗口。".into());
        }
        Ok(Self(handle))
    }
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn show_error(message: &str) {
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(wide(message).as_ptr()),
            w!("PasteLeaf"),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub fn run() -> Result<(), String> {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let demo = args.iter().any(|a| a == "--demo");
        let root = if demo {
            std::env::temp_dir().join(format!(
                "PasteLeaf-v2-demo-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ))
        } else if let Some(i) = args.iter().position(|a| a == "--data-dir") {
            PathBuf::from(args.get(i + 1).ok_or("--data-dir 需要路径")?)
        } else {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("无法确定本机应用数据目录")?)
                .join("PasteLeaf")
                .join("v2")
        };
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("PasteLeafV2Window");
        // Win32 MAKEINTRESOURCE(1) is an integer resource identifier, not a pointer.
        #[allow(clippy::manual_dangling_ptr)]
        let icon = LoadIconW(instance, PCWSTR(1usize as *const u16))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
        let _instance_guard = if demo {
            None
        } else {
            Some(InstanceGuard::acquire(&root)?)
        };
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(instance.0),
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("无法注册窗口类".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class,
            w!("PasteLeaf · 剪贴板历史"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            1100,
            400,
            None,
            None,
            instance,
            None,
        )
        .map_err(|e| e.to_string())?;
        let scale = layout::scale_for_dpi(GetDpiForWindow(hwnd));
        let bridge = Bridge::start(root.clone(), demo, hwnd.0 as usize);
        let renderer = Renderer::new().map_err(|e| format!("Direct2D 初始化失败: {e}"))?;
        let brush = CreateSolidBrush(colorref(LIGHT.elevated));
        let font = CreateFontW(
            -(13. * scale) as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        let mut tray = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: icon,
            ..Default::default()
        };
        let tip = wide("PasteLeaf · Ctrl+Q");
        tray.szTip[..tip.len()].copy_from_slice(&tip);
        if demo {
            if let Some(index) = args.iter().position(|s| s == "--theme") {
                if let Some(theme) = args.get(index + 1) {
                    if matches!(theme.as_str(), "light" | "dark" | "system") {
                        bridge.request(
                            Tag::Action,
                            json!({"op":"settings","values":{"appearance":theme}}),
                        );
                    }
                }
            }
        }
        let mut app = App {
            hwnd,
            bridge,
            model: ViewModel {
                paused: true,
                status: if demo {
                    "设计演示 · 不读取系统剪贴板".into()
                } else {
                    "正在打开历史库…".into()
                },
                ..Default::default()
            },
            renderer,
            controls: vec![],
            screen: Screen::Main,
            palette: LIGHT,
            brush,
            background_brush: CreateSolidBrush(colorref(LIGHT.background)),
            font,
            scale,
            width: 1100.,
            height: PANEL_HEIGHT,
            demo,
            target: HWND::default(),
            root,
            hotkey: "Ctrl+Q".into(),
            hotkey_registered: false,
            next_registered: false,
            ime: false,
            pending_layout: false,
            status_data: Value::Null,
            card_first: 0,
            category_offset: 0,
            pending_anchor: None,
            hover: None,
            drag: None,
            scroll: 0.,
            thumbnails: HashMap::new(),
            last_refresh: Instant::now(),
            last_capture: GetClipboardSequenceNumber(),
            paste_copy_only: false,
            conflicts: vec![],
            conflict_snapshot: None,
            conflict_offset: 0,
            version_offset: 0,
            delete_targets: vec![],
            closed: false,
            pending_paste: false,
            queued_inflight: None,
            tray,
        };
        if !demo {
            app.register_hotkey("Ctrl+Q");
            AddClipboardFormatListener(hwnd).map_err(|e| e.to_string())?;
            let _ = Shell_NotifyIconW(NIM_ADD, &app.tray);
        }
        app.position();
        app.refresh();
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        with_app(|a| {
            a.rebuild();
            if !args.iter().any(|s| s == "--background") || demo {
                a.show();
            }
        });
        SetTimer(hwnd, TIMER as usize, 100, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let used = with_app(|a| a.keyboard(&msg)).unwrap_or(false);
            if !used && !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        APP.with(|slot| {
            slot.borrow_mut().take();
        });
        CoUninitialize();
        Ok(())
    }
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            if !self.demo {
                let _ = RemoveClipboardFormatListener(self.hwnd);
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.tray);
            }
            let _ = UnregisterHotKey(self.hwnd, HOTKEY);
            let _ = UnregisterHotKey(self.hwnd, NEXT_HOTKEY);
            let _ = DeleteObject(self.brush);
            let _ = DeleteObject(self.background_brush);
            let _ = DeleteObject(self.font);
        }
    }
}
impl App {
    fn refresh(&mut self) {
        let (generation, value) = self.model.request_list();
        self.bridge.request(Tag::List(generation), value);
        self.bridge.request(Tag::Status, json!({"op":"status"}));
        self.last_refresh = Instant::now();
    }
    fn action(&mut self, value: Value) {
        self.bridge.request(Tag::Action, value);
    }
    fn status_error(&mut self, error: impl Into<String>) {
        self.model.error = Some(error.into());
        unsafe {
            self.update_status_label();
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }
    unsafe fn register_hotkey(&mut self, value: &str) -> bool {
        let (modifiers, key) = match parse_hotkey(value) {
            Ok(v) => v,
            Err(e) => {
                self.status_error(e);
                return false;
            }
        };
        if self.hotkey_registered && value == self.hotkey {
            return true;
        }
        let temp = 3;
        if RegisterHotKey(
            self.hwnd,
            temp,
            HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
            key,
        )
        .is_err()
        {
            self.status_error("快捷键已被占用，未抢占。请在设置中换一个组合。");
            return false;
        }
        let _ = UnregisterHotKey(self.hwnd, temp);
        if self.hotkey_registered {
            let _ = UnregisterHotKey(self.hwnd, HOTKEY);
        }
        if RegisterHotKey(
            self.hwnd,
            HOTKEY,
            HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
            key,
        )
        .is_err()
        {
            self.hotkey_registered = false;
            self.status_error("快捷键注册失败，请打开托盘设置重试");
            return false;
        }
        self.hotkey = value.into();
        self.hotkey_registered = true;
        true
    }
    unsafe fn position(&mut self) {
        let monitor = MonitorFromWindow(
            if self.target.0.is_null() {
                self.hwnd
            } else {
                self.target
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return;
        }
        let work = layout::PixelRect {
            x: mi.rcWork.left,
            y: mi.rcWork.top,
            width: mi.rcWork.right - mi.rcWork.left,
            height: mi.rcWork.bottom - mi.rcWork.top,
        };
        // First move to the target monitor, then read this per-monitor-aware HWND's actual DPI.
        // No origin is converted to DIPs: negative monitor offsets and taskbar insets stay exact.
        for _ in 0..2 {
            let bounds = layout::placement(work, self.scale, self.screen == Screen::Main);
            self.width = bounds.logical_width;
            self.height = bounds.logical_height;
            let r = bounds.physical;
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                r.x,
                r.y,
                r.width,
                r.height,
                SWP_NOACTIVATE,
            );
            let scale = layout::scale_for_dpi(GetDpiForWindow(self.hwnd));
            if (scale - self.scale).abs() < f32::EPSILON {
                break;
            }
            self.update_scale(scale);
        }
        let scrollable = self.screen != Screen::Main && self.height < 620.;
        let _ = ShowScrollBar(self.hwnd, SB_VERT, scrollable);
        if scrollable {
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: 620,
                nPage: self.height as u32,
                nPos: self.scroll as i32,
                ..Default::default()
            };
            SetScrollInfo(self.hwnd, SB_VERT, &info, true);
        }
        let mut client = RECT::default();
        if GetClientRect(self.hwnd, &mut client).is_ok() {
            self.width = (client.right - client.left) as f32 / self.scale;
            self.height = (client.bottom - client.top) as f32 / self.scale;
        }
        // Opaque surfaces are the compatibility fallback on Windows 10 and reduced-transparency systems.
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            self.hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const _,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        let dark = BOOL::from(self.palette.background == DARK.background);
        let _ = DwmSetWindowAttribute(
            self.hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const _,
            size_of::<BOOL>() as u32,
        );
    }
    unsafe fn update_scale(&mut self, scale: f32) {
        self.scale = scale;
        let font = CreateFontW(
            -(13. * scale) as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        if !font.0.is_null() {
            let old = self.font;
            self.font = font;
            for control in &self.controls {
                SendMessageW(control.hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
            let _ = DeleteObject(old);
        }
    }
    unsafe fn relayout(&mut self) {
        // Display/DPI changes must not erase unfinished settings or a typed sync password.
        // These values remain transient in memory and are never logged or sent to the core.
        let fields: Vec<_> = if self.screen == Screen::Main {
            Vec::new()
        } else {
            [FIELD1, FIELD2, FIELD3]
                .into_iter()
                .filter(|id| self.controls.iter().any(|c| c.id == *id))
                .map(|id| (id, self.text_value(id)))
                .collect()
        };
        self.position();
        self.rebuild();
        for (id, value) in fields {
            if let Some(c) = self.controls.iter().find(|c| c.id == id) {
                SendMessageW(
                    c.hwnd,
                    WM_SETTEXT,
                    WPARAM(0),
                    LPARAM(wide(&value).as_ptr() as isize),
                );
            }
        }
    }
    unsafe fn show(&mut self) {
        let foreground = GetForegroundWindow();
        if foreground != self.hwnd && !IsChild(self.hwnd, foreground).as_bool() {
            self.target = foreground;
        }
        self.refresh();
        self.relayout();
        let mut animations = ANIMATIONINFO {
            cbSize: size_of::<ANIMATIONINFO>() as u32,
            iMinAnimate: 1,
        };
        let _ = SystemParametersInfoW(
            SPI_GETANIMATION,
            size_of::<ANIMATIONINFO>() as u32,
            Some(&mut animations as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if !self.model.settings["reduce_motion"]
            .as_bool()
            .unwrap_or(false)
            && animations.iMinAnimate != 0
            && !IsWindowVisible(self.hwnd).as_bool()
        {
            let _ = AnimateWindow(self.hwnd, (MOTION_SECONDS * 1000.) as u32, AW_BLEND);
        }
        let _ = ShowWindow(self.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(self.hwnd);
        if let Some(c) = self.controls.iter().find(|c| c.id == SEARCH) {
            let _ = SetFocus(c.hwnd);
        }
    }
    unsafe fn hide(&mut self) {
        let _ = ShowWindow(self.hwnd, SW_HIDE);
        if !self.target.0.is_null() {
            let _ = SetForegroundWindow(self.target);
        }
    }
    // Native controls take explicit geometry, accessible text, and visual state.
    #[allow(clippy::too_many_arguments)]
    unsafe fn button(
        &mut self,
        id: u32,
        label: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        primary: bool,
        card: Option<usize>,
    ) {
        let hwnd = self.control(
            id,
            "BUTTON",
            label,
            WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            x,
            y,
            w,
            h,
        );
        let _ = SetWindowSubclass(hwnd, Some(button_subclass), 1, id as usize);
        self.controls.push(Control {
            hwnd,
            id,
            label: label.into(),
            primary,
            card,
        });
    }
    // Native controls take explicit geometry, accessible text, and visual state.
    #[allow(clippy::too_many_arguments)]
    unsafe fn edit(
        &mut self,
        id: u32,
        label: &str,
        value: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        password: bool,
        multiline: bool,
    ) {
        self.label(label, x, y - 23., w, 20.);
        let mut style = WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        if password {
            style |= WINDOW_STYLE(ES_PASSWORD as u32);
        }
        if multiline {
            style |=
                WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32)
                    | WS_VSCROLL;
        }
        let hwnd = self.control(id, "EDIT", value, style, x, y, w, h);
        let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
        let _ = SendMessageW(
            hwnd,
            EM_SETLIMITTEXT,
            WPARAM(if password { 256 } else { 65536 }),
            LPARAM(0),
        );
        self.controls.push(Control {
            hwnd,
            id,
            label: label.into(),
            primary: false,
            card: None,
        });
    }
    // Read-only, wrapping native EDIT keeps long metadata available to keyboard and UIA users.
    #[allow(clippy::too_many_arguments)]
    unsafe fn readonly_details(
        &mut self,
        id: u32,
        label: &str,
        value: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        self.label(label, x, y - 23., w, 20.);
        let hwnd = self.control(
            id,
            "EDIT",
            value,
            WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_READONLY as u32),
            x,
            y,
            w,
            h,
        );
        SendMessageW(hwnd, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
        self.controls.push(Control {
            hwnd,
            id,
            label: label.into(),
            primary: false,
            card: None,
        });
    }
    unsafe fn label(&mut self, value: &str, x: f32, y: f32, w: f32, h: f32) {
        let id = 7000 + self.controls.len() as u32;
        let hwnd = self.control(id, "STATIC", value, WINDOW_STYLE(0), x, y, w, h);
        let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
        self.controls.push(Control {
            hwnd,
            id,
            label: value.into(),
            primary: false,
            card: None,
        });
    }
    // Native controls take explicit geometry, accessible text, and visual state.
    #[allow(clippy::too_many_arguments)]
    unsafe fn control(
        &self,
        id: u32,
        class: &str,
        text: &str,
        style: WINDOW_STYLE,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> HWND {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(wide(class).as_ptr()),
            PCWSTR(wide(text).as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            (x * self.scale) as i32,
            ((y - if id == STATUS_TEXT { 0. } else { self.scroll }) * self.scale) as i32,
            (w * self.scale) as i32,
            (h * self.scale) as i32,
            self.hwnd,
            HMENU(id as usize as *mut _),
            GetModuleHandleW(None).unwrap_or_default(),
            None,
        )
        .unwrap_or_default()
    }
    unsafe fn text_value(&self, id: u32) -> String {
        let h = self
            .controls
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.hwnd)
            .unwrap_or_default();
        let len = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
    unsafe fn clear_controls(&mut self) {
        for c in self.controls.drain(..) {
            let _ = DestroyWindow(c.hwnd);
        }
    }
    unsafe fn switch(&mut self, screen: Screen) {
        self.screen = screen;
        self.scroll = 0.;
        self.position();
        self.rebuild();
    }
    unsafe fn rebuild(&mut self) {
        let focus = GetFocus();
        let focused = self.controls.iter().find(|c| c.hwnd == focus).map(|c| {
            if c.card.is_some() {
                self.model
                    .items
                    .iter()
                    .position(|i| Some(&i.id) == self.model.focused.as_ref())
                    .map(|i| CARD + i as u32)
                    .unwrap_or(c.id)
            } else {
                c.id
            }
        });
        let mut caret_start = 0u32;
        let mut caret_end = 0u32;
        if focused == Some(SEARCH) {
            SendMessageW(
                focus,
                EM_GETSEL,
                WPARAM(&mut caret_start as *mut _ as usize),
                LPARAM(&mut caret_end as *mut _ as isize),
            );
        }
        self.clear_controls();
        if self.screen == Screen::Main {
            let header = layout::header(self.width);
            let r = header.search;
            self.edit(
                SEARCH,
                "",
                &self.model.query.clone(),
                r.x,
                r.y,
                r.width,
                r.height,
                false,
                false,
            );
            if let Some(c) = self.controls.iter().find(|c| c.id == SEARCH) {
                SendMessageW(
                    c.hwnd,
                    EM_SETCUEBANNER,
                    WPARAM(0),
                    LPARAM(wide("搜索历史、标签与内容").as_ptr() as isize),
                );
            }
            if let Some(r) = header.history {
                self.button(
                    HISTORY,
                    "历史",
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    self.model.category.is_none(),
                    None,
                );
                let mut x = header.category_start;
                for (i, c) in self.model.categories.clone().iter().enumerate() {
                    let width = (c.name.chars().count() as f32 * 13. + 32.).clamp(70., 136.);
                    if x + width > header.category_end {
                        break;
                    }
                    self.button(
                        CATEGORY + i as u32,
                        &c.name,
                        x,
                        r.y,
                        width,
                        r.height,
                        self.model.category.as_ref() == Some(&c.id),
                        None,
                    );
                    x += width + CATEGORY_GAP;
                }
            }
            for ((id, title), r) in [
                (FAVORITES, "分类"),
                (SYNC, "同步"),
                (SETTINGS, "设置"),
                (HIDE, "收起"),
            ]
            .into_iter()
            .zip(header.right)
            {
                self.button(id, title, r.x, r.y, r.width, r.height, false, None);
            }
            let shelf = layout::Shelf::new(self.width, self.height);
            let visible = shelf.visible_cards;
            let first = self
                .card_first
                .min(self.model.items.len().saturating_sub(1));
            let cards: Vec<_> = self
                .model
                .items
                .iter()
                .enumerate()
                .skip(first)
                .take(visible)
                .map(|(i, item)| (i, item.accessible_name()))
                .collect();
            for (n, (i, label)) in cards.iter().enumerate() {
                let r = shelf.card(n);
                self.button(
                    CARD + *i as u32,
                    label,
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    false,
                    Some(*i),
                );
            }
            for button in layout::footer(
                self.width,
                self.height,
                !self.model.queue.is_empty(),
                self.model.paused && !self.demo,
            ) {
                let (id, label, primary) = match button.action {
                    ShelfAction::Previous => (PREVIOUS, "←".into(), false),
                    ShelfAction::Next => (NEXT, "→".into(), false),
                    ShelfAction::Paste => (PASTE, "粘贴 ↵".into(), true),
                    ShelfAction::Plain => (PLAIN, "纯文本".into(), false),
                    ShelfAction::Preview => (PREVIEW, "预览".into(), false),
                    ShelfAction::Favorite => (STAR, "收藏".into(), false),
                    ShelfAction::Queue => {
                        (QUEUE, format!("下一项 ({})", self.model.queue.len()), true)
                    }
                    ShelfAction::Enable => (ENABLE, "开始记录".into(), true),
                    ShelfAction::More => (ACTIONS, "更多".into(), false),
                };
                let r = button.rect;
                self.button(id, &label, r.x, r.y, r.width, r.height, primary, None);
            }
        } else {
            self.button(BACK, "← 返回", 24., 22., 86., 34., false, None);
            match self.screen.clone() {
                Screen::Collections | Screen::AssignCategory => {
                    self.button(
                        HISTORY,
                        "全部历史",
                        32.,
                        94.,
                        130.,
                        36.,
                        self.model.category.is_none(),
                        None,
                    );
                    self.button(
                        ADD_CATEGORY,
                        "＋ 新建分类",
                        174.,
                        94.,
                        140.,
                        36.,
                        true,
                        None,
                    );
                    let categories = self.model.categories.clone();
                    for (row, (i, c)) in categories
                        .iter()
                        .enumerate()
                        .skip(self.category_offset)
                        .take(7)
                        .enumerate()
                    {
                        self.button(
                            (if self.screen == Screen::AssignCategory {
                                ASSIGN_BASE
                            } else {
                                CATEGORY
                            }) + i as u32,
                            &c.name,
                            32.,
                            152. + row as f32 * 44.,
                            self.width - 64.,
                            36.,
                            self.model.category.as_ref() == Some(&c.id),
                            None,
                        );
                    }
                    self.button(PREVIOUS, "上一页", 32., 480., 94., 36., false, None);
                    self.button(NEXT, "下一页", 138., 480., 94., 36., false, None);
                }
                Screen::Actions => {
                    let mut actions = vec![
                        (PASTE, "粘贴到原窗口"),
                        (PLAIN, "粘贴纯文本"),
                        (COPY, "复制，手动粘贴"),
                        (PREVIEW, "内容预览"),
                        (STAR, "收藏 / 取消收藏"),
                        (ASSIGN, "选择收藏分类"),
                        (TAGS, "编辑标签"),
                        (UNFAVORITE, "移出收藏"),
                        (DELETE, "删除并同步"),
                    ];
                    if !self.model.queue.is_empty() {
                        actions.push((QUEUE, "粘贴顺序队列下一项"));
                    }
                    if self.model.paused && !self.demo {
                        actions.push((ENABLE, "开始记录"));
                    }
                    for (n, (id, label)) in actions.iter().enumerate() {
                        let column = n % 2;
                        let row = n / 2;
                        self.button(
                            *id,
                            label,
                            32. + column as f32 * (self.width - 64.) / 2.,
                            108. + row as f32 * 70.,
                            (self.width - 80.) / 2.,
                            46.,
                            *id == PASTE,
                            None,
                        );
                    }
                }
                Screen::Import => {
                    self.label(
                        "仅导入旧 PasteLeaf 数据；先备份，不覆盖旧库，也不读取 Paste 数据。",
                        32.,
                        108.,
                        self.width - 64.,
                        55.,
                    );
                    self.edit(
                        FIELD1,
                        "旧 PasteLeaf 数据目录",
                        "",
                        32.,
                        210.,
                        self.width - 210.,
                        40.,
                        false,
                        false,
                    );
                    self.button(
                        BROWSE,
                        "选择目录",
                        self.width - 166.,
                        210.,
                        134.,
                        40.,
                        false,
                        None,
                    );
                    self.button(SAVE, "备份并导入", 32., 280., 150., 38., true, None);
                }
                Screen::Settings => self.settings_controls(),
                Screen::Sync => self.sync_controls(),
                Screen::Preview(id) => {
                    if let Some(item) = self.model.items.iter().find(|i| i.id == id).cloned() {
                        self.label(
                            &format!("{} · {}", item.type_name(), item.source),
                            32.,
                            94.,
                            self.width - 64.,
                            24.,
                        );
                        if item.kind == "image" {
                            self.label(
                                &format!("{} · 粘贴前校验附件", content_metadata(&item)),
                                32.,
                                125.,
                                self.width - 64.,
                                22.,
                            );
                        } else {
                            let text = if item.kind == "files" {
                                item.files
                                    .iter()
                                    .map(|f| {
                                        format!(
                                            "{}  ·  {:.1} MiB",
                                            f.name,
                                            f.size as f64 / (1024. * 1024.)
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            } else {
                                item.text.clone()
                            };
                            self.edit(
                                FIELD1,
                                "内容预览（不运行 HTML / 外部资源）",
                                &text,
                                32.,
                                150.,
                                self.width - 64.,
                                280.,
                                false,
                                true,
                            );
                            if let Some(c) = self.controls.iter().find(|c| c.id == FIELD1) {
                                SendMessageW(c.hwnd, EM_SETREADONLY, WPARAM(1), LPARAM(0));
                            }
                        }
                        self.button(COPY, "复制", 32., 454., 90., 36., false, None);
                        self.button(PASTE, "粘贴到原窗口", 134., 454., 140., 36., true, None);
                        self.button(TAGS, "编辑标签", 286., 454., 100., 36., false, None);
                        self.button(DELETE, "删除并传播", 398., 454., 120., 36., false, None);
                    }
                }
                Screen::Tags(id) => {
                    let tags = self
                        .model
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .map(|i| i.tags.join(", "))
                        .unwrap_or_default();
                    self.edit(
                        FIELD1,
                        "标签（使用逗号分隔）",
                        &tags,
                        32.,
                        140.,
                        self.width - 64.,
                        40.,
                        false,
                        false,
                    );
                    self.button(SAVE, "保存标签", 32., 208., 110., 36., true, None);
                }
                Screen::Category(id) => {
                    let name = id
                        .as_ref()
                        .and_then(|id| self.model.categories.iter().find(|c| &c.id == id))
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    self.edit(
                        FIELD1,
                        "收藏分类名称",
                        &name,
                        32.,
                        140.,
                        self.width - 64.,
                        40.,
                        false,
                        false,
                    );
                    self.button(SAVE, "保存分类", 32., 208., 110., 36., true, None);
                    if id.is_some() {
                        self.button(
                            DELETE_CATEGORY,
                            "删除分类",
                            154.,
                            208.,
                            110.,
                            36.,
                            false,
                            None,
                        );
                    }
                }
                Screen::Confirm(action) => {
                    let copy = if action == "clear_local" {
                        "仅清空本机历史，不向其他设备发送删除。此操作不可撤销。"
                    } else if action == "sync_disconnect" {
                        "断开此设备的同步连接；保留本机历史与云端操作文件。"
                    } else if action.starts_with("category:") {
                        "删除分类并同步到其他设备，其中的记录仍然保留。"
                    } else {
                        "删除所选记录并同步到其他设备。云端历史操作仍可能保留旧正文。"
                    };
                    self.label(copy, 32., 130., self.width - 64., 110.);
                    self.button(SAVE, "确认操作", 32., 266., 110., 36., true, None);
                    self.button(BACK, "取消", 154., 266., 86., 36., false, None);
                }
                Screen::ConflictDetail(_) => {
                    if let Some(conflict) = self.conflict_snapshot.clone() {
                        self.label(
                            &format!("{}\n核对下方标签、分类与附件；详情可滚动。保留全部不会覆盖其他版本。", conflict_changes(&conflict)),
                            32., 96., self.width - 64., 54.,
                        );
                        for (n, version) in conflict["versions"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .skip(self.version_offset)
                            .take(CONFLICT_VERSIONS_PER_PAGE)
                            .enumerate()
                        {
                            let title = format!("版本 {} · 只读详情", self.version_offset + n + 1);
                            let details = conflict_version_details(version, &self.model.categories);
                            self.readonly_details(
                                CONFLICT_TEXT + n as u32,
                                &title,
                                &details,
                                32.,
                                176. + n as f32 * 180.,
                                self.width - 246.,
                                136.,
                            );
                            self.button(
                                RESOLVE_BASE + n as u32,
                                "保留此版本",
                                self.width - 194.,
                                176. + n as f32 * 180.,
                                160.,
                                40.,
                                false,
                                None,
                            );
                        }
                        self.button(KEEP_ALL, "保留全部版本", 32., 518., 164., 38., true, None);
                        if self.version_offset > 0 {
                            self.button(PREVIOUS, "上一页", 212., 518., 94., 38., false, None);
                        }
                        if self.version_offset + CONFLICT_VERSIONS_PER_PAGE
                            < conflict["versions"].as_array().map(Vec::len).unwrap_or(0)
                        {
                            self.button(NEXT, "下一页", 318., 518., 94., 38., false, None);
                        }
                    }
                }
                Screen::Conflicts => {
                    if self.conflicts.is_empty() {
                        self.label("没有需要处理的冲突。", 32., 130., self.width - 64., 30.);
                    } else {
                        self.conflict_offset = self
                            .conflict_offset
                            .min((self.conflicts.len().saturating_sub(1) / 6) * 6);
                        let entries = self.conflicts.clone();
                        for (i, v) in entries
                            .iter()
                            .skip(self.conflict_offset)
                            .take(6)
                            .enumerate()
                        {
                            let entity = v["entity"].as_str().unwrap_or("未知记录");
                            self.label(
                                &format!("冲突记录 {}", entity),
                                32.,
                                120. + i as f32 * 64.,
                                self.width - 230.,
                                42.,
                            );
                            self.button(
                                CONFLICT + i as u32,
                                "比较版本",
                                self.width - 188.,
                                120. + i as f32 * 64.,
                                150.,
                                38.,
                                true,
                                None,
                            );
                        }
                        if self.conflict_offset > 0 {
                            self.button(PREVIOUS, "上一页", 32., 514., 94., 38., false, None);
                        }
                        if self.conflict_offset + 6 < entries.len() {
                            self.button(NEXT, "下一页", 138., 514., 94., 38., false, None);
                        }
                    }
                }
                Screen::Quotas => {
                    self.edit(
                        FIELD1,
                        "本机保护上限（GiB，达到上限暂停，不删除历史）",
                        &format!(
                            "{}",
                            self.model.settings["local_limit_bytes"]
                                .as_u64()
                                .unwrap_or(10 * 1024 * 1024 * 1024)
                                / (1024 * 1024 * 1024)
                        ),
                        32.,
                        140.,
                        self.width - 64.,
                        40.,
                        false,
                        false,
                    );
                    self.edit(
                        FIELD2,
                        "同步目录保护上限（GiB，不等于 iCloud 配额）",
                        &format!(
                            "{}",
                            self.model.settings["sync_limit_bytes"]
                                .as_u64()
                                .unwrap_or(10 * 1024 * 1024 * 1024)
                                / (1024 * 1024 * 1024)
                        ),
                        32.,
                        238.,
                        self.width - 64.,
                        40.,
                        false,
                        false,
                    );
                    self.button(SAVE, "保存容量限制", 32., 316., 150., 36., true, None);
                }
                _ => {}
            }
        }
        let has_selection = !self.model.selected_items().is_empty();
        for c in &self.controls {
            if matches!(
                c.id,
                PASTE | PLAIN | COPY | PREVIEW | STAR | TAGS | DELETE | ASSIGN | UNFAVORITE
            ) {
                let _ = EnableWindow(c.hwnd, has_selection && !self.pending_paste);
            }
            if c.id == PREVIOUS && self.screen == Screen::Main {
                let _ = EnableWindow(c.hwnd, self.card_first > 0 || self.model.offset > 0);
            }
            if c.id == NEXT && self.screen == Screen::Main {
                let _ = EnableWindow(
                    c.hwnd,
                    self.card_first + self.visible_count() < self.model.items.len()
                        || self.model.offset + self.model.items.len() < self.model.total,
                );
            }
        }
        let status = self.status_message();
        let status_rect = if self.screen == Screen::Main {
            layout::Shelf::new(self.width, self.height).status
        } else {
            layout::Rect::new(24., self.height - 24., self.width - 48., 22.)
        };
        let status_hwnd = self.control(
            STATUS_TEXT,
            "STATIC",
            &status,
            WINDOW_STYLE(SS_ENDELLIPSIS.0 | SS_NOPREFIX.0),
            status_rect.x,
            status_rect.y,
            status_rect.width,
            status_rect.height,
        );
        SendMessageW(
            status_hwnd,
            WM_SETFONT,
            WPARAM(self.font.0 as usize),
            LPARAM(1),
        );
        self.controls.push(Control {
            hwnd: status_hwnd,
            id: STATUS_TEXT,
            label: status,
            primary: false,
            card: None,
        });
        if let Some(id) = focused {
            if let Some(c) = self.controls.iter().find(|c| c.id == id) {
                let _ = SetFocus(c.hwnd);
                if id == SEARCH {
                    SendMessageW(
                        c.hwnd,
                        EM_SETSEL,
                        WPARAM(caret_start as usize),
                        LPARAM(caret_end as isize),
                    );
                }
            }
        }
        let _ = InvalidateRect(self.hwnd, None, false);
    }
    unsafe fn settings_controls(&mut self) {
        self.label(
            "本地正文未加密。启用后可能记录密码、Token 和其他私密内容。",
            32.,
            100.,
            self.width - 64.,
            38.,
        );
        self.edit(
            FIELD1,
            "全局快捷键",
            &self.hotkey.clone(),
            32.,
            172.,
            260.,
            36.,
            false,
            false,
        );
        self.edit(
            FIELD2,
            "排除应用（进程文件名，逗号分隔；默认不排除）",
            &self.model.settings["excluded_apps"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
            32.,
            254.,
            self.width - 64.,
            42.,
            false,
            false,
        );
        self.button(SAVE, "保存设置", 32., 320., 116., 36., true, None);
        self.button(
            PAUSE,
            if self.model.paused {
                "恢复记录"
            } else {
                "暂停记录"
            },
            160.,
            320.,
            112.,
            36.,
            false,
            None,
        );
        let appearance = self.model.settings["appearance"]
            .as_str()
            .unwrap_or("system");
        self.button(
            THEME,
            &format!(
                "外观：{}",
                match appearance {
                    "dark" => "深色",
                    "light" => "浅色",
                    _ => "跟随系统",
                }
            ),
            284.,
            320.,
            170.,
            36.,
            false,
            None,
        );
        self.button(
            STARTUP,
            if self.model.settings["launch_at_login"]
                .as_bool()
                .unwrap_or(false)
            {
                "关闭登录启动"
            } else {
                "启用登录启动"
            },
            32.,
            370.,
            150.,
            36.,
            false,
            None,
        );
        self.button(QUOTA, "容量保护", 194., 370., 120., 36., false, None);
        self.button(
            CLEAR_LOCAL,
            "清空本机历史",
            326.,
            370.,
            150.,
            36.,
            false,
            None,
        );
        self.button(SYNC, "iCloud Drive 设置", 32., 420., 170., 36., false, None);
        self.button(EXIT, "退出 PasteLeaf", 214., 420., 150., 36., false, None);
        self.button(IMPORT, "导入旧版历史", 380., 420., 150., 36., false, None);
        self.button(
            MOTION,
            if self.model.settings["reduce_motion"]
                .as_bool()
                .unwrap_or(false)
            {
                "允许淡入动画"
            } else {
                "减少动态效果"
            },
            32.,
            470.,
            160.,
            36.,
            false,
            None,
        );
        self.label(
            &format!("当前采用不透明兼容背景 · 本机数据：{}", self.root.display()),
            32.,
            518.,
            self.width - 64.,
            30.,
        );
        if self.model.category.is_some() {
            self.button(
                RENAME_CATEGORY,
                "管理当前分类",
                204.,
                470.,
                160.,
                36.,
                false,
                None,
            );
        }
    }
    unsafe fn sync_controls(&mut self) {
        self.label(
            "文件同步不是即时消息。已写入目录 ≠ 对端已导入。",
            32.,
            97.,
            self.width - 64.,
            30.,
        );
        let directory = self.status_data["sync_directory"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        self.edit(
            FIELD1,
            "iCloud Drive 专用同步目录（两端选择同一共享文件夹）",
            &directory,
            32.,
            166.,
            self.width - 64.,
            38.,
            false,
            false,
        );
        self.edit(
            FIELD2,
            "同步密码（不会显示或写入日志）",
            "",
            32.,
            248.,
            self.width - 64.,
            38.,
            true,
            false,
        );
        self.button(SYNC_CREATE, "创建同步库", 32., 314., 140., 36., true, None);
        self.button(SYNC_JOIN, "加入已有库", 184., 314., 140., 36., false, None);
        self.button(SYNC_UNLOCK, "解锁", 336., 314., 86., 36., false, None);
        self.button(SYNC_NOW, "立即扫描", 32., 366., 120., 36., false, None);
        self.button(
            SYNC_PAUSE,
            "暂停 / 恢复同步",
            164.,
            366.,
            170.,
            36.,
            false,
            None,
        );
        self.button(CONFLICTS, "处理冲突", 346., 366., 120., 36., false, None);
        self.button(
            SYNC_DISCONNECT,
            "断开同步",
            32.,
            418.,
            120.,
            36.,
            false,
            None,
        );
        self.label(
            &format!(
                "密钥交由 Windows DPAPI 保存。忘记密码不能找回。\n待发布：{}  ·  冲突：{}\n{}",
                self.status_data["pending"].as_u64().unwrap_or(0),
                self.status_data["conflicts"].as_u64().unwrap_or(0),
                self.status_data["detail"].as_str().unwrap_or("")
            ),
            32.,
            478.,
            self.width - 64.,
            52.,
        );
    }
    unsafe fn command(&mut self, id: u32) {
        match id {
            SEARCH=>{},
            HISTORY=>{self.model.category=None;self.model.offset=0;self.card_first=0;self.pending_anchor=None;self.refresh();self.switch(Screen::Main);},
            FAVORITES=>self.switch(Screen::Collections),
            ACTIONS=>self.switch(Screen::Actions),
            ASSIGN=>{self.category_offset=0;self.switch(Screen::AssignCategory);},
            UNFAVORITE=>{for item in self.model.selected_items(){self.action(json!({"op":"favorite","id":item.id,"category":null}));}self.switch(Screen::Main);},
            KEEP_ALL=>{if let Screen::ConflictDetail(_)=self.screen{if let Some(v)=self.conflict_snapshot.as_ref(){self.action(json!({"op":"resolve","entity":v["entity"],"keep":"all"}));self.bridge.request(Tag::Conflicts,json!({"op":"conflicts"}));self.switch(Screen::Conflicts);}}},
            SETTINGS=>self.switch(Screen::Settings),
            SYNC=>self.switch(Screen::Sync),
            IMPORT=>self.switch(Screen::Import),
            BACK=>self.switch(Screen::Main),
            HIDE=>self.hide(),
            BROWSE=>match choose_folder(self.hwnd){
                Ok(Some(path))=>{if let Some(c)=self.controls.iter().find(|c|c.id==FIELD1){let _=SetWindowTextW(c.hwnd,PCWSTR(wide(&path).as_ptr()));}},
                Ok(None)=>{},Err(e)=>self.status_error(e)
            },
            MOTION=>self.action(json!({"op":"settings","values":{"reduce_motion":!self.model.settings["reduce_motion"].as_bool().unwrap_or(false)}})),
            PAUSE|ENABLE=>{
                if self.demo{self.status_error("演示模式始终暂停系统剪贴板读取");}
                else{self.action(json!({"op":"settings","values":{"paused":!self.model.paused,"onboarding_complete":true}}));}
            },
            PREVIOUS if self.screen==Screen::Conflicts=>{self.conflict_offset=self.conflict_offset.saturating_sub(6);self.rebuild();},
            NEXT if self.screen==Screen::Conflicts=>{if self.conflict_offset+6<self.conflicts.len(){self.conflict_offset+=6;self.rebuild();}},
            PREVIOUS if matches!(self.screen,Screen::ConflictDetail(_))=>{self.version_offset=self.version_offset.saturating_sub(CONFLICT_VERSIONS_PER_PAGE);self.rebuild();},
            NEXT if matches!(self.screen,Screen::ConflictDetail(_))=>{if self.version_offset+CONFLICT_VERSIONS_PER_PAGE<self.conflict_snapshot.as_ref().and_then(|c|c["versions"].as_array()).map(Vec::len).unwrap_or(0){self.version_offset+=CONFLICT_VERSIONS_PER_PAGE;self.rebuild();}},
            PREVIOUS if matches!(self.screen,Screen::Collections|Screen::AssignCategory)=>{self.category_offset=self.category_offset.saturating_sub(7);self.rebuild();},
            NEXT if matches!(self.screen,Screen::Collections|Screen::AssignCategory)=>{if self.category_offset+7<self.model.categories.len(){self.category_offset+=7;self.rebuild();}},
            PREVIOUS=>{
                if self.card_first>0{self.card_first=self.card_first.saturating_sub(self.visible_count());self.rebuild();}
                else if self.model.offset>0{self.model.offset=self.model.offset.saturating_sub(64);self.card_first=0;self.pending_anchor=None;self.refresh();}
            },
            NEXT=>{
                if self.card_first+self.visible_count()<self.model.items.len(){self.card_first+=self.visible_count();self.rebuild();}
                else if self.model.offset+self.model.items.len()<self.model.total{self.model.offset+=64;self.card_first=0;self.pending_anchor=None;self.refresh();}
            },
            PASTE|COPY|PLAIN=>{
                if self.demo{self.status_error("设计演示不会修改系统剪贴板");return;}
                if self.pending_paste{return;}
                match self.model.prepare_paste(id==PLAIN){
                    Ok(payload)=>{self.paste_copy_only=id==COPY;self.pending_paste=true;
                        if !self.model.queue.is_empty(){if let crate::model::PastePayload::Item(item)=&payload{self.queued_inflight=Some((**item).clone());}}
                        self.bridge.paste(payload,self.model.selected_items().iter().map(|i|i.id.clone()).collect());},
                    Err(e)=>self.status_error(e)
                }
            },
            QUEUE=>{
                if !self.demo&&!self.pending_paste{match self.model.next_paste(){
                    Ok(payload)=>{self.paste_copy_only=false;self.pending_paste=true;
                        if let crate::model::PastePayload::Item(item)=&payload{self.queued_inflight=Some((**item).clone());}
                        self.bridge.paste(payload,self.model.selected_items().iter().map(|i|i.id.clone()).collect());},Err(e)=>self.status_error(e)
                }}
            },
            PREVIEW=>if let Some(id)=self.model.focused.clone(){self.switch(Screen::Preview(id));},
            STAR=>{
                let selected=self.model.selected_items();
                if let Some(category)=self.model.category.clone().or_else(||self.model.categories.first().map(|c|c.id.clone())){
                    for item in selected{self.action(json!({"op":"favorite","id":item.id,"category":if item.category==Some(category.clone()){None}else{Some(category.clone())}}));}
                }else{self.switch(Screen::Category(None));}
            },
            TAGS=>if let Some(id)=self.model.focused.clone(){self.switch(Screen::Tags(id));},
            DELETE=>{self.delete_targets=self.model.selected_items().iter().map(|i|i.id.clone()).collect();if !self.delete_targets.is_empty(){self.switch(Screen::Confirm("delete".into()));}},
            ADD_CATEGORY=>self.switch(Screen::Category(None)),
            RENAME_CATEGORY=>if let Some(id)=self.model.category.clone(){self.switch(Screen::Category(Some(id)));},
            DELETE_CATEGORY=>if let Screen::Category(Some(id))=&self.screen{self.switch(Screen::Confirm(format!("category:{id}")));},
            CLEAR_LOCAL=>self.switch(Screen::Confirm("clear_local".into())),
            SYNC_DISCONNECT=>self.switch(Screen::Confirm("sync_disconnect".into())),
            SYNC_CREATE|SYNC_JOIN=>{
                if self.demo{self.status_error("设计演示不连接真实同步库");return;}
                let directory=self.text_value(FIELD1);let password=self.text_value(FIELD2);
                if directory.trim().is_empty()||password.is_empty(){self.status_error("请填写同步目录和密码");return;}
                self.action(json!({"op":if id==SYNC_CREATE{"sync_create"}else{"sync_join"},"directory":directory.trim(),"password":password,"remember":true}));self.clear_password();
            },
            SYNC_UNLOCK=>{if !self.demo{let password=self.text_value(FIELD2);self.action(json!({"op":"sync_unlock","password":password,"remember":true}));self.clear_password();}},
            SYNC_NOW=>self.action(json!({"op":"sync_now"})),
            SYNC_PAUSE=>self.action(json!({"op":"sync_pause","paused":!self.status_data["sync_paused"].as_bool().unwrap_or(false)})),
            CONFLICTS=>{self.conflict_offset=0;self.bridge.request(Tag::Conflicts,json!({"op":"conflicts"}));self.switch(Screen::Conflicts);},
            THEME=>{let next=match self.model.settings["appearance"].as_str().unwrap_or("system"){"system"=>"light","light"=>"dark",_=>"system"};self.action(json!({"op":"settings","values":{"appearance":next}}));},
            STARTUP=>{
                if self.demo{self.status_error("演示模式不修改登录启动");return;}
                let enable=!self.model.settings["launch_at_login"].as_bool().unwrap_or(false);
                match launch_at_login(enable){Ok(())=>self.action(json!({"op":"settings","values":{"launch_at_login":enable}})),Err(e)=>self.status_error(e)}
            },
            QUOTA=>self.switch(Screen::Quotas),
            SAVE=>self.save_screen(),
            EXIT=>{self.closed=true;let _=DestroyWindow(self.hwnd);},
            value if (CATEGORY..CARD).contains(&value)=>{
                if let Some(c)=self.model.categories.get((value-CATEGORY) as usize){self.model.category=Some(c.id.clone());self.model.offset=0;self.card_first=0;self.pending_anchor=None;self.refresh();self.switch(Screen::Main);}
            },
            value if (CARD..CONFLICT).contains(&value)=>{self.model.select((value-CARD) as usize,ctrl());self.repaint_cards();},
            value if (CONFLICT..CONFLICT+1000).contains(&value)=>{let index=self.conflict_offset+(value-CONFLICT) as usize;if let Some(conflict)=self.conflicts.get(index).cloned(){self.conflict_snapshot=Some(conflict);self.version_offset=0;self.switch(Screen::ConflictDetail(index));}},
            value if (ASSIGN_BASE..ASSIGN_BASE+1000).contains(&value)=>{if let Some(category)=self.model.categories.get((value-ASSIGN_BASE) as usize).map(|c|c.id.clone()){for item in self.model.selected_items(){self.action(json!({"op":"favorite","id":item.id,"category":category}));}self.switch(Screen::Main);}},
            value if (RESOLVE_BASE..RESOLVE_BASE+1000).contains(&value)=>{if let Screen::ConflictDetail(_)=self.screen{if let Some(conflict)=self.conflict_snapshot.as_ref(){if let Some(version)=conflict["versions"].as_array().and_then(|v|v.get(self.version_offset+(value-RESOLVE_BASE) as usize)){self.action(json!({"op":"resolve","entity":conflict["entity"],"keep":version["op_id"]}));self.bridge.request(Tag::Conflicts,json!({"op":"conflicts"}));self.switch(Screen::Conflicts);}}}},
            _=>{}
        }
    }
    unsafe fn clear_password(&self) {
        if let Some(c) = self.controls.iter().find(|c| c.id == FIELD2) {
            let _ = SetWindowTextW(c.hwnd, w!(""));
        }
    }
    unsafe fn save_screen(&mut self) {
        match self.screen.clone() {
            Screen::Import => {
                if self.demo {
                    self.status_error("设计演示不导入用户历史");
                    return;
                }
                let directory = self.text_value(FIELD1);
                if directory.trim().is_empty() {
                    self.status_error("请选择旧版数据目录");
                    return;
                }
                self.action(json!({"op":"import_v1","directory":directory.trim()}));
            }
            Screen::Settings => {
                let hotkey = self.text_value(FIELD1);
                if !self.demo && !self.register_hotkey(&hotkey) {
                    return;
                }
                let exclusions = self
                    .text_value(FIELD2)
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                self.action(
                    json!({"op":"settings","values":{"hotkey":hotkey,"excluded_apps":exclusions}}),
                );
            }
            Screen::Tags(id) => {
                let tags = self
                    .text_value(FIELD1)
                    .split([',', '，'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                self.action(json!({"op":"tags","id":id,"tags":tags}));
                self.switch(Screen::Main);
            }
            Screen::Category(id) => {
                let name = self.text_value(FIELD1).trim().to_owned();
                if name.is_empty() {
                    self.status_error("分类名称不能为空");
                    return;
                }
                if let Some(id) = id {
                    self.action(json!({"op":"category_rename","id":id,"name":name}));
                } else {
                    self.action(
                        json!({"op":"category_create","name":name,"color":CATEGORY_COLORS[0]}),
                    );
                }
                self.switch(Screen::Main);
            }
            Screen::Confirm(action) => {
                if action == "delete" {
                    for id in self.delete_targets.clone() {
                        self.action(json!({"op":"delete","id":id}));
                    }
                } else if let Some(id) = action.strip_prefix("category:") {
                    self.action(json!({"op":"category_delete","id":id}));
                    self.model.category = None;
                } else {
                    self.action(json!({"op":action}));
                }
                self.switch(Screen::Main);
            }
            Screen::Quotas => {
                let local = self.text_value(FIELD1).trim().parse::<u64>();
                let sync = self.text_value(FIELD2).trim().parse::<u64>();
                match (local, sync) {
                    (Ok(l), Ok(s)) if (1..=65536).contains(&l) && (1..=65536).contains(&s) => {
                        self.action(json!({"op":"settings","values":{"local_limit_bytes":l*1024*1024*1024,"sync_limit_bytes":s*1024*1024*1024}}));
                        self.switch(Screen::Settings);
                    }
                    _ => self.status_error("请输入 1–65536 范围内的整数 GiB"),
                }
            }
            _ => {}
        }
    }
    fn visible_count(&self) -> usize {
        layout::Shelf::new(self.width, self.height).visible_cards
    }
    unsafe fn repaint_cards(&self) {
        for c in self.controls.iter().filter(|c| c.card.is_some()) {
            if let Some(item) = c.card.and_then(|n| self.model.items.get(n)) {
                let title = format!(
                    "{}{}",
                    if self.model.selected.contains(&item.id) {
                        "已选择 · "
                    } else {
                        ""
                    },
                    item.accessible_name()
                );
                let _ = SetWindowTextW(c.hwnd, PCWSTR(wide(&title).as_ptr()));
                windows::Win32::UI::Accessibility::NotifyWinEvent(
                    EVENT_OBJECT_NAMECHANGE,
                    c.hwnd,
                    OBJID_CLIENT.0,
                    0,
                );
            }
            let _ = InvalidateRect(c.hwnd, None, false);
        }
        let _ = InvalidateRect(self.hwnd, None, false);
    }
    unsafe fn poll(&mut self) {
        if self.drag.is_some() {
            return;
        }
        if self.pending_layout && !self.ime {
            self.pending_layout = false;
            self.relayout();
        }
        let replies = self.bridge.rx.try_iter().collect::<Vec<_>>();
        let mut rebuild = false;
        for reply in replies {
            if matches!(reply.tag,Tag::List(g) if g!=self.model.request_generation) {
                continue;
            }
            match reply.result {
                Err(e) => {
                    self.model.loading = false;
                    if matches!(reply.tag, Tag::PasteReady) {
                        if let Some(item) = self.queued_inflight.take() {
                            self.model.queue.push_front(item);
                        }
                        self.pending_paste = false;
                    }
                    self.status_error(e);
                }
                Ok(data) => match reply.tag {
                    Tag::List(g) => {
                        if self.model.apply_list(g, data) {
                            if let Some(anchor) = self.pending_anchor.take() {
                                if let Some(index) =
                                    self.model.items.iter().position(|i| i.id == anchor)
                                {
                                    self.card_first = index;
                                }
                            }
                            self.thumbnails
                                .retain(|id, _| self.model.items.iter().any(|i| &i.id == id));
                            rebuild = true;
                            for i in self.model.items.iter().filter(|i| i.available) {
                                if let Some(path) = &i.image_path {
                                    if !self.thumbnails.contains_key(&i.id) {
                                        self.bridge.request(
                                            Tag::Thumbnail(i.id.clone()),
                                            json!({"_thumbnail":path}),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Tag::Thumbnail(id) => {
                        use base64::Engine;
                        let decoded = base64::engine::general_purpose::STANDARD
                            .decode(data["rgba"].as_str().unwrap_or(""));
                        if let Ok(pixels) = decoded {
                            if let Some(image) = image::RgbaImage::from_raw(
                                data["width"].as_u64().unwrap_or(0) as u32,
                                data["height"].as_u64().unwrap_or(0) as u32,
                                pixels,
                            ) {
                                self.thumbnails.insert(id, image);
                                self.repaint_cards();
                            }
                        }
                    }
                    Tag::Status => {
                        let old_paused = self.model.paused;
                        let old_categories = serde_json::to_string(&self.status_data["categories"])
                            .unwrap_or_default();
                        let old_appearance = self.model.settings["appearance"].clone();
                        self.model.paused = self.demo || data["paused"].as_bool().unwrap_or(true);
                        self.model.settings = data["settings"].clone();
                        self.model.categories =
                            serde_json::from_value(data["categories"].clone()).unwrap_or_default();
                        self.model.status = if self.demo {
                            "设计演示 · 不读取、不写入系统剪贴板".into()
                        } else {
                            sync_label(data["sync_state"].as_str().unwrap_or("unknown")).into()
                        };
                        if self.model.paused && !self.demo {
                            self.model.status = format!("记录已暂停 · {}", self.model.status);
                        }
                        if !self.demo {
                            if let Some(h) =
                                self.model.settings["hotkey"].as_str().map(str::to_owned)
                            {
                                if h != self.hotkey {
                                    self.register_hotkey(&h);
                                }
                            }
                        }
                        self.status_data = data;
                        let dark = match self.model.settings["appearance"]
                            .as_str()
                            .unwrap_or("system")
                        {
                            "dark" => true,
                            "light" => false,
                            _ => system_dark(),
                        };
                        self.palette = if dark { DARK } else { LIGHT };
                        let _ = DeleteObject(self.brush);
                        self.brush = CreateSolidBrush(colorref(self.palette.elevated));
                        let _ = DeleteObject(self.background_brush);
                        self.background_brush = CreateSolidBrush(colorref(self.palette.background));
                        rebuild |= old_paused != self.model.paused
                            || old_categories
                                != serde_json::to_string(&self.status_data["categories"])
                                    .unwrap_or_default()
                            || old_appearance != self.model.settings["appearance"];
                        let _ = InvalidateRect(self.hwnd, None, false);
                    }
                    Tag::Action => {
                        self.pending_anchor =
                            self.model.items.get(self.card_first).map(|i| i.id.clone());
                        self.refresh();
                    }
                    Tag::Conflicts => {
                        self.conflicts = data["items"].as_array().cloned().unwrap_or_default();
                        if self.screen == Screen::Conflicts {
                            rebuild = true;
                        }
                    }
                    Tag::PasteReady => {
                        self.queued_inflight = None;
                        self.pending_paste = false;
                        if self.paste_copy_only {
                            self.model.status = "已复制；请在目标应用手动粘贴".into();
                        } else {
                            self.hide();
                            self.pending_paste = true;
                            SetTimer(self.hwnd, PASTE_TIMER as usize, 100, None);
                        }
                        self.update_queue_hotkey();
                    }
                },
            }
        }
        for c in &mut self.controls {
            let label = match c.id {
                PAUSE => Some(
                    if self.model.paused {
                        "恢复记录"
                    } else {
                        "暂停记录"
                    }
                    .to_string(),
                ),
                STARTUP => Some(
                    if self.model.settings["launch_at_login"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "关闭登录启动"
                    } else {
                        "启用登录启动"
                    }
                    .to_string(),
                ),
                MOTION => Some(
                    if self.model.settings["reduce_motion"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "允许淡入动画"
                    } else {
                        "减少动态效果"
                    }
                    .to_string(),
                ),
                THEME => Some(format!(
                    "外观：{}",
                    match self.model.settings["appearance"]
                        .as_str()
                        .unwrap_or("system")
                    {
                        "dark" => "深色",
                        "light" => "浅色",
                        _ => "跟随系统",
                    }
                )),
                _ => None,
            };
            if let Some(label) = label {
                if c.label != label {
                    c.label = label;
                    let _ = SetWindowTextW(c.hwnd, PCWSTR(wide(&c.label).as_ptr()));
                    let _ = InvalidateRect(c.hwnd, None, false);
                }
            }
        }
        self.update_status_label();
        if rebuild
            && !self.ime
            && matches!(
                self.screen,
                Screen::Main | Screen::Conflicts | Screen::Collections
            )
        {
            self.rebuild();
        } else if rebuild {
            for c in &self.controls {
                let _ = InvalidateRect(c.hwnd, None, false);
            }
        }

        if self.last_refresh.elapsed().as_secs() >= 5 {
            if self.screen == Screen::Main && !self.ime && IsWindowVisible(self.hwnd).as_bool() {
                self.pending_anchor = self.model.items.get(self.card_first).map(|i| i.id.clone());
                self.refresh();
            } else {
                self.bridge.request(Tag::Status, json!({"op":"status"}));
                self.last_refresh = Instant::now();
            }
        }
    }
    unsafe fn update_queue_hotkey(&mut self) {
        if self.model.queue.is_empty() {
            if self.next_registered {
                let _ = UnregisterHotKey(self.hwnd, NEXT_HOTKEY);
                self.next_registered = false;
            }
        } else if !self.next_registered {
            if let Ok((mods, key)) = parse_hotkey(&self.hotkey) {
                self.next_registered = RegisterHotKey(
                    self.hwnd,
                    NEXT_HOTKEY,
                    HOT_KEY_MODIFIERS(mods | 4) | MOD_NOREPEAT,
                    key,
                )
                .is_ok();
                if !self.next_registered {
                    self.status_error("顺序粘贴快捷键被占用；请呼出面板点击“下一项”");
                }
            }
        }
    }
    unsafe fn paste_to_target(&mut self) {
        KillTimer(self.hwnd, PASTE_TIMER as usize).ok();
        if !self.pending_paste {
            return;
        }
        self.pending_paste = false;
        if self.target.0.is_null()
            || !IsWindow(self.target).as_bool()
            || GetForegroundWindow() != self.target
        {
            self.status_error("已复制，未自动粘贴：无法恢复目标窗口，请手动粘贴");
            self.show();
            return;
        }
        // Never synthesize Return. UIPI may block delivery into an elevated process.
        let inputs = [
            key_input(VK_CONTROL, false),
            key_input(VK_V, false),
            key_input(VK_V, true),
            key_input(VK_CONTROL, true),
        ];
        if SendInput(&inputs, size_of::<INPUT>() as i32) != inputs.len() as u32 {
            self.status_error("已复制，但 Windows 拒绝模拟输入；请手动粘贴");
            self.show();
        }
    }
    unsafe fn capture(&mut self) {
        if self.demo || self.model.paused || self.bridge.writing.load(Ordering::SeqCst) {
            return;
        }
        let sequence = GetClipboardSequenceNumber();
        if sequence == self.last_capture
            || sequence == self.bridge.own_sequence.load(Ordering::SeqCst)
        {
            return;
        }
        self.last_capture = sequence;
        let source = foreground_source();
        let excluded = self.model.settings["excluded_apps"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .any(|name| name.eq_ignore_ascii_case(&source))
            })
            .unwrap_or(false);
        if !excluded {
            self.bridge.capture(source, sequence);
        }
    }
    unsafe fn keyboard(&mut self, msg: &MSG) -> bool {
        if msg.message == WM_KEYDOWN && msg.wParam.0 as u16 == VK_ESCAPE.0 && self.drag.is_some() {
            self.cancel_drag();
            return true;
        }
        if msg.message == WM_IME_STARTCOMPOSITION {
            self.ime = true;
            return false;
        }
        if msg.message == WM_IME_ENDCOMPOSITION {
            self.ime = false;
            return false;
        }
        if msg.message != WM_KEYDOWN || self.ime {
            return false;
        }
        let context = ImmGetContext(msg.hwnd);
        if !context.0.is_null() {
            let composing = ImmGetCompositionStringW(context, GCS_COMPSTR, None, 0) > 0;
            let _ = ImmReleaseContext(msg.hwnd, context);
            if composing {
                return false;
            }
        }
        let key = msg.wParam.0 as u16;
        let focus = GetFocus();
        let editing = self
            .controls
            .iter()
            .any(|c| c.hwnd == focus && matches!(c.id, SEARCH | FIELD1 | FIELD2 | FIELD3));
        if key == VK_ESCAPE.0 {
            if self.screen != Screen::Main {
                self.switch(Screen::Main);
            } else if !self.model.query.is_empty() {
                self.model.query.clear();
                self.model.offset = 0;
                self.card_first = 0;
                self.refresh();
            } else {
                self.hide();
            }
            return true;
        }
        if self.screen != Screen::Main {
            if !editing && key == VK_NEXT.0 {
                self.scroll_by(180.);
                return true;
            }
            if !editing && key == VK_PRIOR.0 {
                self.scroll_by(-180.);
                return true;
            }
            return false;
        }
        if key == VK_RETURN.0 {
            self.command(if ctrl() { PLAIN } else { PASTE });
            return true;
        }
        if editing && !matches!(key, 0x26 | 0x28) {
            return false;
        }
        if key == VK_SPACE.0 {
            self.command(PREVIEW);
            return true;
        }
        if key == VK_DELETE.0 {
            self.command(DELETE);
            return true;
        }
        if key == VK_F2.0 {
            self.command(TAGS);
            return true;
        }
        if ctrl() && key == VK_A.0 {
            self.model.selected = self.model.items.iter().map(|i| i.id.clone()).collect();
            self.repaint_cards();
            return true;
        }
        if matches!(key, 0x25..=0x28) {
            let current = self
                .model
                .items
                .iter()
                .position(|i| Some(&i.id) == self.model.focused.as_ref())
                .unwrap_or(0);
            let next = if matches!(key, 0x25 | 0x26) {
                current.saturating_sub(1)
            } else {
                (current + 1).min(self.model.items.len().saturating_sub(1))
            };
            self.model.select(next, shift());
            if next < self.card_first || next >= self.card_first + self.visible_count() {
                self.card_first = next;
                self.rebuild();
            }
            if let Some(c) = self.controls.iter().find(|c| c.card == Some(next)) {
                let _ = SetFocus(c.hwnd);
            }
            self.repaint_cards();
            return true;
        }
        false
    }
    unsafe fn paint(&mut self, dc: HDC, bounds: &RECT) {
        if self.renderer.begin(dc, bounds, self.scale).is_err() {
            return;
        }
        let p = self.palette;
        self.renderer
            .fill(rect(0., 0., self.width, self.height), p.background);
        if self.screen == Screen::Main {
            if self.model.items.is_empty() {
                let title = if self.model.loading {
                    "正在打开历史库…"
                } else if !self.model.query.is_empty() {
                    "没有找到匹配的内容"
                } else if self.model.paused {
                    "你的下一次灵感，从这里开始"
                } else {
                    "复制内容，就会出现在这里"
                };
                self.renderer.text(
                    title,
                    rect(36., 128., self.width - 72., 50.),
                    24.,
                    p.text,
                    true,
                    false,
                );
                self.renderer.text(
                    if self.model.paused {
                        "点击“开始记录”前不会读取系统剪贴板。密码与 Token 也可能被记录。"
                    } else {
                        "文字、图片、富文本与普通文件，按原来的顺序妥善保存。"
                    },
                    rect(36., 189., self.width - 72., 56.),
                    14.,
                    p.muted,
                    false,
                    false,
                );
            }
        } else {
            let title = match self.screen {
                Screen::Settings => "偏好设置",
                Screen::Sync => "iCloud Drive 同步",
                Screen::Preview(_) => "内容预览",
                Screen::Tags(_) => "整理标签",
                Screen::Category(_) => "收藏分类",
                Screen::Confirm(_) => "确认操作",
                Screen::Conflicts => "同步冲突",
                Screen::Quotas => "容量保护",
                Screen::Collections => "收藏分类",
                Screen::Actions => "记录操作",
                Screen::Import => "导入旧版历史",
                Screen::AssignCategory => "选择收藏分类",
                Screen::ConflictDetail(_) => "比较冲突版本",
                _ => "PasteLeaf",
            };
            self.renderer.text(
                title,
                rect(130., 25. - self.scroll, self.width - 166., 44.),
                23.,
                p.text,
                true,
                false,
            );
            self.renderer.text(
                &self.model.status,
                rect(32., self.height - 55., self.width - 64., 40.),
                12.,
                p.muted,
                false,
                false,
            );
        }
        if let Screen::Preview(ref id) = self.screen {
            if let Some(image) = self.thumbnails.get(id) {
                self.renderer
                    .bitmap(image, rect(32., 154. - self.scroll, self.width - 64., 276.));
            }
        }
        if let Some(error) = self
            .model
            .error
            .as_ref()
            .filter(|_| self.screen != Screen::Main)
        {
            self.renderer.round(
                rect(24., self.height - 94., self.width - 48., 32.),
                CONTROL_RADIUS,
                p.elevated,
                Some((p.danger, 1.)),
            );
            self.renderer.text(
                error,
                rect(34., self.height - 89., self.width - 68., 24.),
                11.,
                p.danger,
                false,
                false,
            );
        }
        let _ = self.renderer.end();
    }
    unsafe fn draw_control(&mut self, item: &DRAWITEMSTRUCT) {
        let Some(control) = self.controls.iter().find(|c| c.id == item.CtlID) else {
            return;
        };
        let bounds = item.rcItem;
        if self.renderer.begin(item.hDC, &bounds, self.scale).is_err() {
            return;
        }
        let width = (bounds.right - bounds.left) as f32 / self.scale;
        let height = (bounds.bottom - bounds.top) as f32 / self.scale;
        if let Some(index) = control.card {
            if let Some(record) = self.model.items.get(index) {
                self.renderer.card(
                    record,
                    width,
                    height,
                    self.palette,
                    self.model.selected.contains(&record.id),
                    item.itemState.0 & ODS_FOCUS.0 != 0 || self.hover == Some(control.id),
                    self.thumbnails.get(&record.id),
                );
            }
        } else {
            self.renderer.button(
                &control.label,
                width,
                height,
                self.palette,
                control.primary,
                item.itemState.0 & ODS_FOCUS.0 != 0 || self.hover == Some(control.id),
                item.itemState.0 & ODS_SELECTED.0 != 0,
                item.itemState.0 & ODS_DISABLED.0 != 0,
            );
        }
        let _ = self.renderer.end();
    }
    unsafe fn begin_drag(&mut self, hwnd: HWND, control: u32) {
        if self.screen != Screen::Main {
            return;
        }
        let Some(index) = control
            .checked_sub(CARD)
            .map(|n| n as usize)
            .filter(|&n| n < self.model.items.len())
        else {
            return;
        };
        let item = &self.model.items[index];
        let ids = if self.model.selected.contains(&item.id) {
            self.model
                .selected_items()
                .iter()
                .map(|i| i.id.clone())
                .collect()
        } else {
            vec![item.id.clone()]
        };
        let mut origin = POINT::default();
        if GetCursorPos(&mut origin).is_ok() {
            self.drag = Some(DragSession {
                hwnd,
                origin,
                ids,
                active: false,
            });
        }
    }
    unsafe fn drag_move(&mut self) {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return;
        }
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if !drag.active {
            let dx = GetSystemMetricsForDpi(SM_CXDRAG, GetDpiForWindow(self.hwnd)).max(4);
            let dy = GetSystemMetricsForDpi(SM_CYDRAG, GetDpiForWindow(self.hwnd)).max(4);
            if (point.x - drag.origin.x).abs() < dx && (point.y - drag.origin.y).abs() < dy {
                return;
            }
            drag.active = true;
            let _ = SetCapture(drag.hwnd);
            self.model.selected = drag.ids.iter().cloned().collect();
            self.model.status = "拖到收藏分类；按 Esc 可取消".into();
            self.repaint_cards();
            self.update_status_label();
        }
        let target = self.drop_category(point);
        SetCursor(
            LoadCursorW(None, if target.is_some() { IDC_HAND } else { IDC_NO }).unwrap_or_default(),
        );
    }
    unsafe fn drop_category(&self, point: POINT) -> Option<String> {
        let hwnd = WindowFromPoint(point);
        let control = self.controls.iter().find(|c| c.hwnd == hwnd)?;
        if !(CATEGORY..CARD).contains(&control.id) {
            return None;
        }
        self.model
            .categories
            .get((control.id - CATEGORY) as usize)
            .map(|c| c.id.clone())
    }
    unsafe fn end_drag(&mut self) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if !drag.active {
            return false;
        }
        let mut point = POINT::default();
        let category = if GetCursorPos(&mut point).is_ok() {
            self.drop_category(point)
        } else {
            None
        };
        let _ = ReleaseCapture();
        SendMessageW(drag.hwnd, BM_SETSTATE, WPARAM(0), LPARAM(0));
        SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
        if let Some(category) = category {
            for id in drag.ids {
                self.action(json!({"op":"favorite","id":id,"category":category}));
            }
            self.model.status = "正在移入收藏分类…".into();
        } else {
            self.model.status = "拖动已取消，记录未变更".into();
        }
        self.update_status_label();
        true
    }
    unsafe fn cancel_drag(&mut self) {
        if let Some(drag) = self.drag.take() {
            if drag.active {
                let _ = ReleaseCapture();
                SendMessageW(drag.hwnd, BM_SETSTATE, WPARAM(0), LPARAM(0));
                SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                self.model.status = "拖动已取消，记录未变更".into();
                self.update_status_label();
            }
        }
    }
    unsafe fn context_menu(&mut self) {
        self.switch(Screen::Actions);
    }
    unsafe fn scroll_by(&mut self, delta: f32) {
        if self.screen == Screen::Main {
            return;
        }
        let old = self.scroll;
        self.scroll = (self.scroll + delta).clamp(0., (620. - self.height).max(0.));
        let dy = ((old - self.scroll) * self.scale) as i32;
        for c in &self.controls {
            if c.id == STATUS_TEXT {
                continue;
            }
            let mut r = RECT::default();
            if GetWindowRect(c.hwnd, &mut r).is_ok() {
                let mut p = POINT {
                    x: r.left,
                    y: r.top,
                };
                let _ = ScreenToClient(self.hwnd, &mut p);
                let _ = SetWindowPos(
                    c.hwnd,
                    None,
                    p.x,
                    p.y + dy,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        SetScrollPos(self.hwnd, SB_VERT, self.scroll as i32, true);
        let _ = InvalidateRect(self.hwnd, None, false);
    }
    fn status_message(&self) -> String {
        if let Some(error) = &self.model.error {
            format!("需要处理：{error}")
        } else {
            format!("{} 条记录 · {}", self.model.total, self.model.status)
        }
    }
    unsafe fn update_status_label(&self) {
        if let Some(c) = self.controls.iter().find(|c| c.id == STATUS_TEXT) {
            let _ = SetWindowTextW(c.hwnd, PCWSTR(wide(&self.status_message()).as_ptr()));
            windows::Win32::UI::Accessibility::NotifyWinEvent(
                EVENT_OBJECT_NAMECHANGE,
                c.hwnd,
                OBJID_CLIENT.0,
                0,
            );
        }
    }
}

fn sync_label(state: &str) -> &'static str {
    match state {
        "disconnected" => "仅本机 · 未连接同步",
        "locked" => "同步已锁定",
        "paused" => "同步已暂停",
        "pending" => "待发布",
        "published" => "已写入同步目录 · 等待对端",
        "waiting_attachments" => "等待附件到达",
        "peer_imported" => "对端已导入",
        "conflict" => "需要处理同步冲突",
        "error" => "同步失败 · 打开同步设置",
        _ => "同步状态待确认",
    }
}
unsafe fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}
unsafe fn foreground_source() -> String {
    let hwnd = GetClipboardOwner().unwrap_or_else(|_| GetForegroundWindow());
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(p) => p,
        Err(_) => return "未知应用".into(),
    };
    let mut path = vec![0u16; 32768];
    let mut size = path.len() as u32;
    let ok = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        windows::core::PWSTR(path.as_mut_ptr()),
        &mut size,
    )
    .is_ok();
    let _ = CloseHandle(process);
    if ok {
        let text = String::from_utf16_lossy(&path[..size as usize]);
        PathBuf::from(text)
            .file_name()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未知应用".into())
    } else {
        "未知应用".into()
    }
}
unsafe fn system_dark() -> bool {
    let mut value = 1u32;
    let mut len = 4u32;
    RegGetValueW(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("AppsUseLightTheme"),
        RRF_RT_REG_DWORD,
        None,
        Some(&mut value as *mut _ as *mut _),
        Some(&mut len),
    )
    .is_ok()
        && value == 0
}
unsafe fn choose_folder(owner: HWND) -> Result<Option<String>, String> {
    let dialog: IFileOpenDialog =
        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
    dialog
        .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
        .map_err(|e| e.to_string())?;
    if let Err(e) = dialog.Show(owner) {
        if e.code() == windows::core::HRESULT(0x800704c7u32 as i32) {
            return Ok(None);
        }
        return Err(e.to_string());
    }
    let item = dialog.GetResult().map_err(|e| e.to_string())?;
    let text = item
        .GetDisplayName(SIGDN_FILESYSPATH)
        .map_err(|e| e.to_string())?;
    let path = text.to_string().map_err(|e| e.to_string())?;
    CoTaskMemFree(Some(text.0.cast()));
    Ok(Some(path))
}
unsafe fn launch_at_login(enable: bool) -> Result<(), String> {
    let mut key = HKEY::default();
    RegCreateKeyExW(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        0,
        None,
        REG_OPTION_NON_VOLATILE,
        KEY_SET_VALUE,
        None,
        &mut key,
        None,
    )
    .ok()
    .map_err(|e| e.to_string())?;
    let result = if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let value = wide(&format!("\"{}\" --background", exe.display()));
        let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
        RegSetValueExW(key, w!("PasteLeafV2"), 0, REG_SZ, Some(bytes)).ok()
    } else {
        RegDeleteValueW(key, w!("PasteLeafV2")).ok()
    };
    let _ = RegCloseKey(key);
    result.map_err(|e| e.to_string())
}

unsafe extern "system" fn button_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_LBUTTONDOWN {
        with_app(|a| a.begin_drag(hwnd, data as u32));
    } else if msg == WM_MOUSEMOVE {
        with_app(|a| {
            a.drag_move();
            if a.hover != Some(data as u32) {
                a.hover = Some(data as u32);
                let _ = InvalidateRect(hwnd, None, false);
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tracking);
            }
        });
    } else if msg == WM_LBUTTONUP {
        if with_app(|a| a.end_drag()).unwrap_or(false) {
            return LRESULT(0);
        }
    } else if msg == WM_CAPTURECHANGED || msg == WM_CANCELMODE {
        with_app(|a| a.cancel_drag());
    } else if msg == WM_MOUSELEAVE {
        with_app(|a| {
            if a.hover == Some(data as u32) {
                a.hover = None;
                let _ = InvalidateRect(hwnd, None, false);
            }
        });
    } else if msg == WM_SETCURSOR {
        if with_app(|a| {
            if a.drag.as_ref().is_some_and(|d| d.active) {
                a.drag_move();
                true
            } else {
                false
            }
        })
        .unwrap_or(false)
        {
            return LRESULT(1);
        }
    } else if msg == WM_NCDESTROY {
        with_app(|a| a.cancel_drag());
        let _ = RemoveWindowSubclass(hwnd, Some(button_subclass), 1);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_GETOBJECT => {
            // All interactive controls are real BUTTON/EDIT/STATIC HWNDs. Their standard
            // Win32 UI Automation providers supply names, focus, Invoke and Value patterns.
            // Do not replace them with inaccessible canvas-only hit regions.
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut bounds = RECT::default();
            let _ = GetClientRect(hwnd, &mut bounds);
            with_app(|a| a.paint(dc, &bounds));
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        WM_DRAWITEM => {
            if lp.0 != 0 {
                with_app(|a| a.draw_control(&*(lp.0 as *const DRAWITEMSTRUCT)));
            }
            return LRESULT(1);
        }
        WM_COMMAND => {
            let id = (wp.0 & 65535) as u32;
            let code = ((wp.0 >> 16) & 65535) as u32;
            with_app(|a| {
                if id == SEARCH && code == EN_CHANGE {
                    a.model.query = a.text_value(SEARCH);
                    a.pending_anchor = None;
                    a.model.offset = 0;
                    a.card_first = 0;
                    a.model.request_generation += 1;
                    SetTimer(hwnd, SEARCH_TIMER as usize, 250, None);
                } else if code == BN_CLICKED {
                    a.command(id);
                }
            });
            return LRESULT(0);
        }
        WM_TIMER => {
            with_app(|a| match wp.0 as u32 {
                TIMER => a.poll(),
                SEARCH_TIMER => {
                    KillTimer(hwnd, SEARCH_TIMER as usize).ok();
                    if !a.ime {
                        a.refresh();
                    } else {
                        SetTimer(hwnd, SEARCH_TIMER as usize, 250, None);
                    }
                }
                PASTE_TIMER => a.paste_to_target(),
                _ => {}
            });
            return LRESULT(0);
        }
        WM_HOTKEY => {
            with_app(|a| {
                if wp.0 as i32 == NEXT_HOTKEY {
                    a.command(QUEUE);
                } else if IsWindowVisible(hwnd).as_bool() {
                    a.hide();
                } else {
                    a.show();
                }
            });
            return LRESULT(0);
        }
        WM_CLIPBOARDUPDATE => {
            with_app(|a| a.capture());
            return LRESULT(0);
        }
        WM_CONTEXTMENU => {
            with_app(|a| {
                if let Some(index) = a
                    .controls
                    .iter()
                    .find(|c| c.hwnd == HWND(wp.0 as *mut _))
                    .and_then(|c| c.card)
                {
                    if a.model
                        .items
                        .get(index)
                        .is_some_and(|i| !a.model.selected.contains(&i.id))
                    {
                        a.model.select(index, false);
                    }
                }
                a.context_menu();
            });
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            with_app(|a| {
                let delta = ((wp.0 >> 16) as u16 as i16) as f32;
                if a.screen == Screen::Main {
                    if delta < 0. {
                        a.command(NEXT);
                    } else {
                        a.command(PREVIOUS);
                    }
                } else {
                    a.scroll_by(-delta / 3.);
                }
            });
            return LRESULT(0);
        }
        WM_VSCROLL => {
            with_app(|a| match (wp.0 & 65535) as u32 {
                0 => a.scroll_by(-32.),
                1 => a.scroll_by(32.),
                2 => a.scroll_by(-200.),
                3 => a.scroll_by(200.),
                4 | 5 => {
                    let target = ((wp.0 >> 16) & 65535) as f32;
                    a.scroll_by(target - a.scroll);
                }
                _ => {}
            });
            return LRESULT(0);
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            let _ = PostMessageW(hwnd, RELAYOUT, WPARAM(0), LPARAM(0));
            return LRESULT(0);
        }
        RELAYOUT => {
            with_app(|a| {
                if a.ime || a.drag.is_some() {
                    a.pending_layout = true;
                } else {
                    a.relayout();
                }
            });
            return LRESULT(0);
        }
        WM_SETTINGCHANGE => {
            with_app(|a| {
                a.bridge.request(Tag::Status, json!({"op":"status"}));
            });
            let _ = PostMessageW(hwnd, RELAYOUT, WPARAM(0), LPARAM(0));
            return LRESULT(0);
        }
        WM_POWERBROADCAST => {
            if wp.0 == 0x12 || wp.0 == 0x7 {
                with_app(|a| {
                    a.bridge.request(Tag::Action, json!({"op":"sync_now"}));
                });
            }
            return LRESULT(1);
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            if let Some(brush) = with_app(|a| {
                let dc = HDC(wp.0 as *mut _);
                SetTextColor(
                    dc,
                    colorref(
                        if GetDlgCtrlID(HWND(lp.0 as *mut _)) == STATUS_TEXT as i32
                            && a.model.error.is_some()
                        {
                            a.palette.danger
                        } else {
                            a.palette.text
                        },
                    ),
                );
                SetBkColor(
                    dc,
                    colorref(if msg == WM_CTLCOLORSTATIC {
                        a.palette.background
                    } else {
                        a.palette.elevated
                    }),
                );
                if msg == WM_CTLCOLORSTATIC {
                    a.background_brush
                } else {
                    a.brush
                }
            }) {
                return LRESULT(brush.0 as isize);
            }
        }
        WM_ACTIVATE => {
            if wp.0 & 65535 == WA_INACTIVE as usize {
                with_app(|a| {
                    if a.screen == Screen::Main {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                });
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            with_app(|a| a.hide());
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        TRAY => {
            if lp.0 as u32 == WM_LBUTTONUP {
                with_app(|a| a.show());
            } else if lp.0 as u32 == WM_RBUTTONUP {
                with_app(|a| {
                    a.show();
                    a.switch(Screen::Settings);
                });
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
