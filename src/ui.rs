//! Native clipboard dock. Handles stay on the UI thread; RefCell guards reentry.
use pasteleaf::{
    model::{Content, Hotkey, Model},
    store::Store,
    theme::{Palette, Theme},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};
#[path = "frame.rs"]
mod frame;
#[path = "popup.rs"]
mod popup;
#[path = "render.rs"]
mod render;
use popup::{MenuItem, PopupMenu};
const SEARCH: i32 = 101;
const CATEGORY_NAME: i32 = 103;
const HOTKEY: i32 = 113;
const PREVIEW: i32 = 130;
const ITEM_TEXT: i32 = 131;
const TRAY: u32 = WM_APP + 1;
const EXIT_MESSAGE: u32 = WM_APP + 2;
const EDIT_MENU_MESSAGE: u32 = WM_APP + 3;
const EDIT_UNDO: usize = 300;
const EDIT_CUT: usize = 301;
const EDIT_COPY: usize = 302;
const EDIT_PASTE: usize = 303;
const EDIT_DELETE: usize = 304;
const EDIT_SELECT_ALL: usize = 305;
const TEXT_CLIPBOARD_FORMAT: u32 = 13; // CF_UNICODETEXT
const CMD_BOARD_BASE: usize = 2000;
const CMD_PASTE: usize = 201;
const CMD_COPY: usize = 202;
const CMD_PREVIEW: usize = 203;
const CMD_DELETE: usize = 204;
const CMD_UNPIN: usize = 205;
const CMD_SETTINGS: usize = 210;
const CMD_PAUSE: usize = 211;
const CMD_CLEAR: usize = 212;
const CMD_EXIT: usize = 213;
const CMD_SHOW: usize = 214;
const CMD_NEW: usize = 215;
const CMD_RENAME: usize = 216;
const CMD_DELETE_BOARD: usize = 217;
const CMD_NEW_ITEM: usize = 218;
const CMD_PIN_BASE: usize = 1000;
const COLORS: [u32; 6] = [0xEC6482, 0x7D7AE8, 0x48A4E8, 0xE4AA4F, 0x53B98E, 0xB381CF];
const CARD_W: i32 = 232;
const CARD_GAP: i32 = 18;
const CARD_TOP: i32 = 76;
const CARD_H: i32 = 242;
const PANEL_H: i32 = 368;
thread_local! {static APP:RefCell<Option<App>>=const{RefCell::new(None)};}
thread_local! {static CAPTURE_PENDING:Cell<bool>=const{Cell::new(false)};}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn colorref(c: u32) -> u32 {
    ((c >> 16) & 255) | (c & 0xff00) | ((c & 255) << 16)
}
fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn down(key: u16) -> bool {
    // SAFETY: read-only thread input state.
    unsafe { GetKeyState(key as i32) < 0 }
}
pub fn error_dialog(s: &str) {
    // SAFETY: UTF-16 buffers live for the synchronous call.
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(s).as_ptr(),
            wide("PasteLeaf").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    None,
    NewBoard,
    NewItem(String),
    RenameBoard(String),
    Settings,
    Preview(u64),
}
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Startup,
    Theme(Theme),
    Board(usize),
    Add,
    NewItem,
    Settings,
    Close,
    Search,
    Card(u64),
    Previous,
    Next,
    Accept,
    Cancel,
    Color(usize),
    MoreBoards,
}
#[derive(Clone, Copy)]
struct Area {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl Area {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}
struct App {
    popup: Option<PopupMenu>,
    popup_hwnd: HWND,
    menu_editor: Option<HWND>,
    menu_anchor: POINT,
    startup_enabled: Option<bool>,
    hwnd: HWND,
    store: Store,
    model: Model,
    scale: f32,
    width: i32,
    fonts: [HFONT; 5],
    edit_brush: HBRUSH,
    palette: Palette,
    selected: Option<u64>,
    visible: Vec<u64>,
    board: usize,
    board_offset: usize,
    offset: usize,
    status: String,
    hotkey_id: Option<i32>,
    ignore_sequence: u32,
    attempts: u8,
    thumbnails: HashMap<u64, image::RgbaImage>,
    taskbar_created: u32,
    tray_ok: bool,
    hits: Vec<(Area, Hit)>,
    hover: Option<Hit>,
    dialog: Dialog,
    chosen_color: usize,
    drag_origin: Option<(i32, i32)>,
    dragging: bool,
    target: HWND,
    target_pid: u32,
    pending_paste: Option<HWND>,
    paste_attempts: u8,
}
impl App {
    fn refresh_theme(&mut self) {
        self.palette = Palette::new(self.model.theme.is_dark(system_dark()));
        // SAFETY: brush replacement occurs on the UI thread outside painting;
        // redraw all owned child editors after updating their shared colors.
        unsafe {
            let brush = CreateSolidBrush(colorref(self.palette.input));
            if !brush.is_null() {
                DeleteObject(self.edit_brush);
                self.edit_brush = brush;
            }
            frame::set_theme(self.hwnd, self.palette.dark);
            if !self.popup_hwnd.is_null() {
                frame::set_theme(self.popup_hwnd, self.palette.dark);
                InvalidateRect(self.popup_hwnd, null(), 0);
            }
            RedrawWindow(
                self.hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN,
            );
        }
    }
    fn new(hwnd: HWND, store: Store, model: Model, scale: f32) -> Result<Self, String> {
        // SAFETY: GDI resources stay on this thread until release_resources.
        let fonts = unsafe {
            [14, 17, 20, 11, 26].map(|n| {
                CreateFontW(
                    -((n as f32 * scale).round() as i32),
                    0,
                    0,
                    0,
                    if n == 20 { 600 } else { 400 },
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    wide("Segoe UI").as_ptr(),
                )
            })
        };
        if fonts.iter().any(|x| x.is_null()) {
            return Err("无法创建字体".into());
        }
        // SAFETY: brush released after window destruction.
        let palette = Palette::new(model.theme.is_dark(system_dark()));
        let edit_brush = unsafe { CreateSolidBrush(colorref(palette.input)) };
        let mut app = Self {
            popup: None,
            popup_hwnd: null_mut(),
            menu_editor: None,
            menu_anchor: POINT { x: 0, y: 0 },
            startup_enabled: None,
            hwnd,
            store,
            model,
            scale,
            width: 1200,
            fonts,
            edit_brush,
            palette,
            selected: None,
            visible: vec![],
            board: 0,
            board_offset: 0,
            offset: 0,
            status: String::new(),
            hotkey_id: None,
            ignore_sequence: 0,
            attempts: 0,
            thumbnails: HashMap::new(),
            taskbar_created: 0,
            tray_ok: false,
            hits: vec![],
            hover: None,
            dialog: Dialog::None,
            chosen_color: 0,
            drag_origin: None,
            dragging: false,
            target: null_mut(),
            target_pid: 0,
            pending_paste: None,
            paste_attempts: 0,
        };
        app.setup_controls()?;
        app.refresh();
        app.status = format!("已加载 {} 条记录 · 本地历史已恢复", app.model.items.len());
        Ok(app)
    }
    fn px(&self, n: i32) -> i32 {
        (n as f32 * self.scale).round() as i32
    }
    fn rect(&self, a: Area) -> RECT {
        RECT {
            left: self.px(a.x),
            top: self.px(a.y),
            right: self.px(a.x + a.w),
            bottom: self.px(a.y + a.h),
        }
    }
    fn control(&self, id: i32) -> HWND {
        // SAFETY: parent outlives its children.
        unsafe { GetDlgItem(self.hwnd, id) }
    }
    fn text(&self, id: i32) -> String {
        // SAFETY: allocated length includes the terminator.
        unsafe {
            let h = self.control(id);
            let mut b = vec![0u16; GetWindowTextLengthW(h).max(0) as usize + 1];
            let n = GetWindowTextW(h, b.as_mut_ptr(), b.len() as i32).max(0) as usize;
            String::from_utf16_lossy(&b[..n])
        }
    }
    fn set_text(&self, id: i32, s: &str) {
        // SAFETY: EDIT copies the temporary string synchronously.
        unsafe {
            SetWindowTextW(self.control(id), wide(s).as_ptr());
        }
    }
    fn invalidate(&self) {
        // SAFETY: owned window only.
        unsafe {
            InvalidateRect(self.hwnd, null(), 0);
        }
    }
    fn notify(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.invalidate();
    }
    fn page_size(&self) -> usize {
        ((self.width - 48) / (CARD_W + CARD_GAP)).max(1) as usize
    }
    fn refresh(&mut self) {
        self.board = self.board.min(self.model.categories.len());
        let q = self.text(SEARCH);
        let category = if q.is_empty() {
            self.board
                .checked_sub(1)
                .and_then(|i| self.model.categories.get(i).map(String::as_str))
        } else {
            None
        };
        self.visible = self.model.visible(&q, category);
        if !self.visible.iter().any(|i| Some(*i) == self.selected) {
            self.selected = self.visible.first().copied();
        }
        self.offset = self
            .offset
            .min(self.visible.len().saturating_sub(self.page_size()));
        self.thumbnails
            .retain(|id, _| self.model.items.iter().any(|x| x.id == *id));
        self.invalidate();
    }
    fn ensure_selection_visible(&mut self) {
        if let Some(i) = self
            .visible
            .iter()
            .position(|id| Some(*id) == self.selected)
        {
            if i < self.offset {
                self.offset = i;
            }
            if i >= self.offset + self.page_size() {
                self.offset = i + 1 - self.page_size();
            }
        }
        self.invalidate();
    }
    fn commit(&mut self, next: Model) -> Result<(), String> {
        self.store.save(&next)?;
        self.model = next;
        if let Err(e) = self.store.cleanup(&self.model) {
            self.status = format!("已保存；图片清理待重试：{e}");
        }
        self.refresh();
        Ok(())
    }
    fn capture(&mut self) -> Result<(), String> {
        if self.model.paused {
            return Ok(());
        }
        // SAFETY: sequence query doesn't transfer ownership.
        if unsafe { GetClipboardSequenceNumber() } == self.ignore_sequence {
            return Ok(());
        }
        let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let content = match cb.get_text() {
            Ok(t) if !t.is_empty() => Content::Text(t),
            _ => match cb.get_image() {
                Ok(i) => self.store.write_image(
                    u32::try_from(i.width).map_err(|_| "图片过大")?,
                    u32::try_from(i.height).map_err(|_| "图片过大")?,
                    &i.bytes,
                )?,
                Err(arboard::Error::ContentNotAvailable) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            },
        };
        let mut next = self.model.clone();
        let result = next.capture(content).and_then(|id| {
            if let Some(i) = next.items.iter_mut().find(|x| x.id == id) {
                i.source = clipboard_source();
                i.copied_at = timestamp();
            }
            self.commit(next)
        });
        if result.is_err() {
            let _ = self.store.cleanup(&self.model);
        }
        result
    }
    fn copy_selected(&mut self) -> Result<(), String> {
        let item = self
            .model
            .items
            .iter()
            .find(|x| Some(x.id) == self.selected)
            .ok_or("请先选择一张卡片")?;
        let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        match &item.content {
            Content::Text(t) => cb.set_text(t).map_err(|e| e.to_string())?,
            Content::Image { file, .. } => {
                let i = self.store.read_image(file)?;
                cb.set_image(arboard::ImageData {
                    width: i.width() as usize,
                    height: i.height() as usize,
                    bytes: std::borrow::Cow::Borrowed(i.as_raw()),
                })
                .map_err(|e| e.to_string())?;
            }
        }
        // SAFETY: ignore the event produced by our own clipboard write.
        self.ignore_sequence = unsafe { GetClipboardSequenceNumber() };
        self.notify("已复制");
        Ok(())
    }
    fn apply_hotkey(&mut self) -> Result<(), String> {
        let key = Hotkey::parse(&self.text(HOTKEY))?;
        if self.hotkey_id.is_some() && key.label() == self.model.hotkey {
            return Ok(());
        }
        let id = if self.hotkey_id == Some(1) { 2 } else { 1 };
        // SAFETY: new ID is reserved before the old binding is removed.
        unsafe {
            if RegisterHotKey(self.hwnd, id, key.modifiers | MOD_NOREPEAT, key.key) == 0 {
                return Err("快捷键被占用或由系统保留，原快捷键保持不变".into());
            }
        }
        let mut next = self.model.clone();
        next.hotkey = key.label();
        if let Err(e) = self.store.save(&next) {
            // SAFETY: tentative binding belongs to this window.
            unsafe {
                UnregisterHotKey(self.hwnd, id);
            }
            return Err(e);
        }
        // SAFETY: old ID belongs to this thread and window.
        unsafe {
            if let Some(old) = self.hotkey_id {
                UnregisterHotKey(self.hwnd, old);
            }
        }
        self.hotkey_id = Some(id);
        self.model = next;
        self.notify(format!("快捷键已保存：{}", self.model.hotkey));
        Ok(())
    }
    fn paste_selected(&mut self) -> Result<(), String> {
        self.copy_selected()?;
        // SAFETY: original HWND and PID are both checked before focus transfer.
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(self.target, &mut pid);
            if self.target.is_null()
                || IsWindow(self.target) == 0
                || pid != self.target_pid
                || pid == GetCurrentProcessId()
            {
                return Err("已复制；请先从目标窗口按快捷键呼出，或手动按 Ctrl+V".into());
            }
            ShowWindow(self.hwnd, SW_HIDE);
            if SetForegroundWindow(self.target) == 0 {
                self.show_without_target();
                return Err("已复制；目标窗口无法激活，请手动按 Ctrl+V".into());
            }
            self.pending_paste = Some(self.target);
            self.paste_attempts = 0;
            SetTimer(self.hwnd, 2, 35, None);
        }
        Ok(())
    }
    fn finish_paste(&mut self) {
        let Some(target) = self.pending_paste else {
            return;
        };
        // SAFETY: send only Ctrl+V, only to the verified foreground window after
        // modifiers are released. UIPI failures are surfaced, never bypassed.
        unsafe {
            self.paste_attempts += 1;
            if GetForegroundWindow() != target
                || IsWindow(target) == 0
                || GetClipboardSequenceNumber() != self.ignore_sequence
            {
                KillTimer(self.hwnd, 2);
                self.pending_paste = None;
                self.status = "已复制；焦点已改变，未自动粘贴".into();
                return;
            }
            let held = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
                .iter()
                .any(|k| GetAsyncKeyState(*k as i32) < 0);
            if held && self.paste_attempts < 30 {
                return;
            }
            KillTimer(self.hwnd, 2);
            self.pending_paste = None;
            if held {
                self.status = "已复制；请松开修饰键后手动粘贴".into();
                return;
            }
            let inputs = [
                key_input(VK_CONTROL, false),
                key_input(b'V' as u16, false),
                key_input(b'V' as u16, true),
                key_input(VK_CONTROL, true),
            ];
            if SendInput(4, inputs.as_ptr(), size_of::<INPUT>() as i32) != 4 {
                let release = [key_input(b'V' as u16, true), key_input(VK_CONTROL, true)];
                SendInput(2, release.as_ptr(), size_of::<INPUT>() as i32);
                self.show_without_target();
                self.notify("已复制；目标程序权限较高，请手动按 Ctrl+V");
            }
        }
    }
    fn remember_target(&mut self) {
        // SAFETY: borrowed identifiers are validated again before use.
        unsafe {
            let h = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            if !h.is_null() && pid != GetCurrentProcessId() {
                self.target = h;
                self.target_pid = pid;
            }
        }
    }
    fn show(&mut self) {
        self.refresh_theme();
        self.remember_target();
        self.position();
        self.show_without_target();
    }
    fn show_without_target(&mut self) {
        self.offset = 0;
        self.selected = None;
        self.refresh();
        // SAFETY: activate only our dock.
        unsafe {
            ShowWindow(self.hwnd, SW_SHOW);
            // Paint directly: UpdateWindow would reenter APP while it is borrowed.
            self.paint();
            SetForegroundWindow(self.hwnd);
            SetFocus(self.hwnd);
        }
    }
    fn hide(&mut self) {
        // SAFETY: owned window, recording continues independently.
        unsafe {
            self.close_menu();
            ShowWindow(self.hwnd, SW_HIDE);
        }
        self.close_dialog();
    }
    fn position(&mut self) {
        // SAFETY: initialized monitor records and owned HWND.
        unsafe {
            let monitor = MonitorFromWindow(
                if self.target.is_null() {
                    self.hwnd
                } else {
                    self.target
                },
                MONITOR_DEFAULTTONEAREST,
            );
            let mut info: MONITORINFO = zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return;
            }
            let mut dx = 96;
            let mut dy = 96;
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let scale = dx as f32 / 96.0;
            if (scale - self.scale).abs() > 0.01 {
                self.reset_fonts(scale);
            }
            self.width =
                ((info.rcWork.right - info.rcWork.left) as f32 / self.scale).round() as i32 - 24;
            let height = self.px(PANEL_H);
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                info.rcWork.left + self.px(12),
                info.rcWork.bottom - height - self.px(10),
                self.px(self.width),
                height,
                SWP_NOACTIVATE,
            );
            self.layout_controls();
        }
    }
    fn setup_controls(&self) -> Result<(), String> {
        // SAFETY: standard child EDIT controls with synchronous strings.
        unsafe {
            for (id, style) in [
                (SEARCH, ES_AUTOHSCROLL),
                (CATEGORY_NAME, ES_AUTOHSCROLL),
                (HOTKEY, ES_AUTOHSCROLL),
                (PREVIEW, ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY),
                (ITEM_TEXT, ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN),
            ] {
                let h = CreateWindowExW(
                    0,
                    wide("EDIT").as_ptr(),
                    wide("").as_ptr(),
                    WS_CHILD
                        | WS_TABSTOP
                        | style as u32
                        | if id == PREVIEW || id == ITEM_TEXT {
                            WS_VSCROLL
                        } else {
                            0
                        },
                    0,
                    0,
                    1,
                    1,
                    self.hwnd,
                    id as usize as HMENU,
                    GetModuleHandleW(null()),
                    null(),
                );
                if h.is_null() {
                    return Err("无法创建输入控件".into());
                }
                if SetWindowSubclass(h, Some(editor_proc), 1, self.hwnd as usize) == 0 {
                    return Err("无法初始化输入框菜单".into());
                }
                SendMessageW(h, WM_SETFONT, self.fonts[0] as usize, 1);
                SendMessageW(
                    h,
                    EM_SETLIMITTEXT,
                    if id == PREVIEW || id == ITEM_TEXT {
                        512 * 1024
                    } else if id == SEARCH {
                        200
                    } else {
                        80
                    },
                    0,
                );
            }
            SendMessageW(
                self.control(SEARCH),
                EM_SETCUEBANNER,
                1,
                wide("搜索剪贴板").as_ptr() as isize,
            );
            SendMessageW(
                self.control(CATEGORY_NAME),
                EM_SETCUEBANNER,
                1,
                wide("收藏分类名称").as_ptr() as isize,
            );
            SendMessageW(
                self.control(ITEM_TEXT),
                EM_SETCUEBANNER,
                1,
                wide("输入要收藏的文字、链接或提示词").as_ptr() as isize,
            );
        }
        self.layout_controls();
        Ok(())
    }
    fn layout_controls(&self) {
        let compact = self.width < 800;
        let item_width = (self.width - 48).min(720);
        // SAFETY: owned controls; positions are converted from logical to physical pixels.
        unsafe {
            MoveWindow(
                self.control(SEARCH),
                self.px(self.width - if compact { 178 } else { 278 }),
                self.px(26),
                self.px(if compact { 76 } else { 176 }),
                self.px(23),
                1,
            );
            ShowWindow(
                self.control(SEARCH),
                if self.dialog == Dialog::None {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            let x = (self.width - 520) / 2;
            MoveWindow(
                self.control(CATEGORY_NAME),
                self.px(x + 30),
                self.px(166),
                self.px(450),
                self.px(28),
                1,
            );
            MoveWindow(
                self.control(HOTKEY),
                self.px(x + 30),
                self.px(260),
                self.px(330),
                self.px(28),
                1,
            );
            MoveWindow(
                self.control(PREVIEW),
                self.px(60),
                self.px(111),
                self.px(self.width - 120),
                self.px(190),
                1,
            );
            MoveWindow(
                self.control(ITEM_TEXT),
                self.px((self.width - item_width) / 2 + 30),
                self.px(152),
                self.px(item_width - 60),
                self.px(104),
                1,
            );
        }
    }
    fn reset_fonts(&mut self, scale: f32) {
        self.scale = scale;
        // SAFETY: detach old fonts from child controls before deleting them.
        unsafe {
            let old = self.fonts;
            self.fonts = [14, 17, 20, 11, 26].map(|n| {
                CreateFontW(
                    -self.px(n),
                    0,
                    0,
                    0,
                    if n == 20 { 600 } else { 400 },
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    wide("Segoe UI").as_ptr(),
                )
            });
            for id in [SEARCH, CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT] {
                SendMessageW(self.control(id), WM_SETFONT, self.fonts[0] as usize, 1);
            }
            for font in old {
                DeleteObject(font);
            }
        }
        self.thumbnails.clear();
    }
    fn open_dialog(&mut self, dialog: Dialog) {
        self.close_menu();
        self.dialog = dialog;
        self.status.clear();
        self.chosen_color = 0;
        // SAFETY: only the active editor is shown; modal background is custom drawn.
        unsafe {
            for id in [CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT] {
                ShowWindow(self.control(id), SW_HIDE);
            }
            match &self.dialog {
                Dialog::NewBoard => {
                    self.set_text(CATEGORY_NAME, "");
                    ShowWindow(self.control(CATEGORY_NAME), SW_SHOW);
                    SetFocus(self.control(CATEGORY_NAME));
                }
                Dialog::NewItem(_) => {
                    self.set_text(ITEM_TEXT, "");
                    ShowWindow(self.control(ITEM_TEXT), SW_SHOW);
                    SetFocus(self.control(ITEM_TEXT));
                }
                Dialog::RenameBoard(name) => {
                    self.set_text(CATEGORY_NAME, name);
                    self.chosen_color = self
                        .model
                        .category_colors
                        .get(name)
                        .and_then(|c| COLORS.iter().position(|x| x == c))
                        .unwrap_or(0);
                    ShowWindow(self.control(CATEGORY_NAME), SW_SHOW);
                    SetFocus(self.control(CATEGORY_NAME));
                    SendMessageW(self.control(CATEGORY_NAME), EM_SETSEL, 0, -1);
                }
                Dialog::Settings => {
                    match pasteleaf::startup::enabled() {
                        Ok(enabled) => self.startup_enabled = Some(enabled),
                        Err(error) => {
                            self.startup_enabled = None;
                            self.status = error;
                        }
                    }
                    self.set_text(HOTKEY, &self.model.hotkey);
                    ShowWindow(self.control(HOTKEY), SW_SHOW);
                    SetFocus(self.control(HOTKEY));
                }
                Dialog::Preview(id) => {
                    if let Some(item) = self.model.items.iter().find(|x| x.id == *id) {
                        if let Content::Text(t) = &item.content {
                            self.set_text(PREVIEW, t);
                            ShowWindow(self.control(PREVIEW), SW_SHOW);
                            SetFocus(self.control(PREVIEW));
                        }
                    }
                }
                Dialog::None => {}
            }
        }
        self.layout_controls();
        self.invalidate();
    }
    fn close_dialog(&mut self) {
        self.close_menu();
        self.dialog = Dialog::None;
        // SAFETY: hiding children doesn't destroy saved data.
        unsafe {
            for id in [CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT] {
                ShowWindow(self.control(id), SW_HIDE);
            }
            SetFocus(self.hwnd);
        }
        self.set_text(ITEM_TEXT, "");
        self.layout_controls();
        self.invalidate();
    }
    fn accept_dialog(&mut self) -> Result<(), String> {
        match self.dialog.clone() {
            Dialog::NewItem(category) => {
                let mut next = self.model.clone();
                let id = next.add_favorite_text(&self.text(ITEM_TEXT), &category)?;
                if let Some(item) = next.items.iter_mut().find(|item| item.id == id) {
                    item.copied_at = timestamp();
                }
                let previous_status = std::mem::take(&mut self.status);
                if let Err(error) = self.commit(next) {
                    self.status = previous_status;
                    return Err(error);
                }
                self.board = self
                    .model
                    .categories
                    .iter()
                    .position(|name| name == &category)
                    .ok_or("分类不存在")?
                    + 1;
                self.offset = 0;
                self.selected = Some(id);
                self.set_text(SEARCH, "");
                if self.status.is_empty() {
                    self.status = format!("已添加到「{category}」");
                }
            }
            Dialog::NewBoard => {
                let mut n = self.model.clone();
                let name = self.text(CATEGORY_NAME).trim().to_string();
                n.add_category(&name)?;
                n.category_colors.insert(name, COLORS[self.chosen_color]);
                self.commit(n)?;
                self.board = self.model.categories.len();
                self.board_offset = self.board.saturating_sub(3);
            }
            Dialog::RenameBoard(old) => {
                let mut n = self.model.clone();
                let name = self.text(CATEGORY_NAME).trim().to_string();
                n.rename_category(&old, &name)?;
                n.category_colors.insert(name, COLORS[self.chosen_color]);
                self.commit(n)?;
            }
            Dialog::Settings => self.apply_hotkey()?,
            _ => {}
        }
        self.close_dialog();
        self.refresh();
        Ok(())
    }
    fn select_board(&mut self, i: usize) {
        self.board = i;
        self.offset = 0;
        self.selected = None;
        self.set_text(SEARCH, "");
        self.refresh();
    }
    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(a, _)| a.contains(x, y))
            .map(|(_, h)| h.clone())
    }
    fn click(&mut self, h: Hit) -> Result<(), String> {
        match h {
            Hit::Startup => {
                let enabled = pasteleaf::startup::enabled()?;
                if let Err(error) = pasteleaf::startup::set_enabled(!enabled) {
                    self.startup_enabled = pasteleaf::startup::enabled().ok();
                    self.invalidate();
                    return Err(error);
                }
                self.startup_enabled = Some(!enabled);
                self.status = if enabled {
                    "已关闭开机启动"
                } else {
                    "已开启开机启动，登录后进入托盘"
                }
                .into();
                self.invalidate();
            }
            Hit::Theme(theme) => {
                let mut next = self.model.clone();
                next.theme = theme;
                self.commit(next)?;
                self.refresh_theme();
            }
            Hit::Board(i) => self.select_board(i),
            Hit::Add => self.open_dialog(Dialog::NewBoard),
            Hit::NewItem => self.command(CMD_NEW_ITEM)?,
            Hit::Settings => {
                if self.popup.is_some() {
                    self.close_menu();
                } else {
                    self.open_context_menu(None, self.screen_point(self.width - 300, 61))?;
                }
            }
            Hit::Close => self.hide(),
            Hit::Search => {
                // SAFETY: owned search EDIT.
                unsafe {
                    SetFocus(self.control(SEARCH));
                }
            }
            Hit::Card(id) => {
                self.selected = Some(id);
                self.invalidate();
            }
            Hit::Previous => {
                self.offset = self.offset.saturating_sub(1);
                self.invalidate();
            }
            Hit::Next => {
                self.offset =
                    (self.offset + 1).min(self.visible.len().saturating_sub(self.page_size()));
                self.invalidate();
            }
            Hit::Accept => self.accept_dialog()?,
            Hit::Cancel => self.close_dialog(),
            Hit::Color(i) => {
                self.chosen_color = i;
                self.invalidate();
            }
            Hit::MoreBoards => self.board_menu()?,
        }
        Ok(())
    }
    fn command(&mut self, id: usize) -> Result<(), String> {
        match id {
            CMD_PASTE => self.paste_selected()?,
            CMD_COPY => self.copy_selected()?,
            CMD_PREVIEW => {
                if let Some(i) = self.selected {
                    self.open_dialog(Dialog::Preview(i));
                }
            }
            CMD_DELETE => {
                if let Some(i) = self.selected {
                    if self.confirm("删除这条记录？此操作不能撤销。") {
                        let mut n = self.model.clone();
                        n.items.retain(|x| x.id != i);
                        self.commit(n)?;
                    }
                }
            }
            CMD_UNPIN => {
                if let Some(i) = self.selected {
                    let mut n = self.model.clone();
                    n.assign(i, None)?;
                    self.commit(n)?;
                }
            }
            CMD_NEW => {
                self.show_without_target();
                self.open_dialog(Dialog::NewBoard);
            }
            CMD_NEW_ITEM => {
                let category = self
                    .board
                    .checked_sub(1)
                    .and_then(|index| self.model.categories.get(index))
                    .cloned()
                    .ok_or("请先选择收藏分类，再新建收藏项")?;
                self.open_dialog(Dialog::NewItem(category));
            }
            CMD_RENAME => {
                if let Some(name) = self
                    .board
                    .checked_sub(1)
                    .and_then(|i| self.model.categories.get(i))
                {
                    self.open_dialog(Dialog::RenameBoard(name.clone()));
                }
            }
            CMD_DELETE_BOARD => {
                if let Some(name) = self
                    .board
                    .checked_sub(1)
                    .and_then(|i| self.model.categories.get(i))
                    .cloned()
                {
                    if self.confirm("删除这个分类？内容会保留在剪贴板历史中。")
                    {
                        let mut n = self.model.clone();
                        n.delete_category(&name)?;
                        self.board = 0;
                        self.commit(n)?;
                    }
                }
            }
            CMD_SETTINGS => {
                self.show_without_target();
                self.open_dialog(Dialog::Settings);
            }
            CMD_PAUSE => {
                let mut n = self.model.clone();
                n.paused = !n.paused;
                self.commit(n)?;
            }
            CMD_CLEAR => {
                if self.confirm("清空未收藏的历史记录？收藏内容会保留。") {
                    let mut n = self.model.clone();
                    n.clear_history();
                    self.commit(n)?;
                }
            }
            CMD_EXIT => {
                // SAFETY: defer destruction until this dispatch finishes.
                unsafe {
                    PostMessageW(self.hwnd, EXIT_MESSAGE, 0, 0);
                }
            }
            CMD_SHOW => self.show(),
            n if (CMD_PIN_BASE..CMD_PIN_BASE + 100).contains(&n) => {
                if let (Some(id), Some(c)) = (
                    self.selected,
                    self.model.categories.get(n - CMD_PIN_BASE).cloned(),
                ) {
                    let mut next = self.model.clone();
                    next.assign(id, Some(c))?;
                    self.commit(next)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn confirm(&self, s: &str) -> bool {
        // SAFETY: owned HWND and live strings.
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(s).as_ptr(),
                wide("PasteLeaf").as_ptr(),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            ) == IDYES
        }
    }
    fn screen_point(&self, x: i32, y: i32) -> POINT {
        let mut point = POINT {
            x: self.px(x),
            y: self.px(y),
        };
        // SAFETY: only converts a point in our owned window.
        unsafe {
            ClientToScreen(self.hwnd, &mut point);
        }
        point
    }
    fn menu(&mut self, hit: Option<Hit>) -> Result<(), String> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: initialized output point.
        unsafe {
            GetCursorPos(&mut point);
        }
        if hit.is_none() {
            self.show_without_target();
        }
        self.open_context_menu(hit, point)
    }
    fn open_context_menu(&mut self, hit: Option<Hit>, point: POINT) -> Result<(), String> {
        let (title, items) = match hit {
            Some(Hit::Card(id)) => {
                self.selected = Some(id);
                self.invalidate();
                let categories = self
                    .model
                    .categories
                    .iter()
                    .enumerate()
                    .map(|(i, c)| MenuItem::action(c.clone(), "", CMD_PIN_BASE + i))
                    .collect();
                let mut unpin = MenuItem::action("取消收藏", "", CMD_UNPIN);
                unpin.enabled = self
                    .model
                    .items
                    .iter()
                    .any(|item| item.id == id && item.category.is_some());
                (
                    "内容操作".to_string(),
                    vec![
                        MenuItem::action("粘贴", "Enter", CMD_PASTE),
                        MenuItem::action("复制", "Ctrl+C", CMD_COPY),
                        MenuItem::action("预览", "Space", CMD_PREVIEW),
                        MenuItem::separator(),
                        MenuItem::submenu("收藏到", categories),
                        unpin,
                        MenuItem::separator(),
                        MenuItem::action("删除…", "Delete", CMD_DELETE),
                    ],
                )
            }
            Some(Hit::Board(i)) if i > 0 && i <= self.model.categories.len() => {
                self.select_board(i);
                (
                    self.model.categories[i - 1].clone(),
                    vec![
                        MenuItem::action("新建收藏项…", "Ctrl+N", CMD_NEW_ITEM),
                        MenuItem::separator(),
                        MenuItem::action("编辑分类…", "", CMD_RENAME),
                        MenuItem::action("删除分类…", "", CMD_DELETE_BOARD),
                    ],
                )
            }
            _ => (
                "PasteLeaf".to_string(),
                vec![
                    MenuItem::action("设置", "Ctrl+,", CMD_SETTINGS),
                    MenuItem::action("新建收藏分类", "Ctrl+Shift+N", CMD_NEW),
                    MenuItem::action(
                        if self.model.paused {
                            "继续记录"
                        } else {
                            "暂停记录"
                        },
                        "Ctrl+T",
                        CMD_PAUSE,
                    ),
                    MenuItem::separator(),
                    MenuItem::action("清空历史…", "", CMD_CLEAR),
                    MenuItem::action("退出 PasteLeaf", "", CMD_EXIT),
                ],
            ),
        };
        self.open_popup(PopupMenu::new(title, items), point, None)
    }
    fn board_menu(&mut self) -> Result<(), String> {
        let mut items = vec![MenuItem::action("剪贴板历史", "", CMD_BOARD_BASE)];
        items.extend(
            self.model
                .categories
                .iter()
                .enumerate()
                .map(|(i, c)| MenuItem::action(c.clone(), "", CMD_BOARD_BASE + i + 1)),
        );
        self.open_popup(
            PopupMenu::new("收藏分类", items),
            self.screen_point(180, 62),
            None,
        )
    }
    fn editor_menu(&mut self, editor: HWND, mut point: POINT) -> Result<(), String> {
        // SAFETY: only the five owned EDIT controls are accepted. EM_GETSEL uses
        // full DWORD output pointers so selections beyond 65,535 stay intact.
        unsafe {
            if ![SEARCH, CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT]
                .iter()
                .any(|id| self.control(*id) == editor)
            {
                return Ok(());
            }
            if point.x == -1 && point.y == -1 {
                let mut caret = POINT { x: 12, y: 12 };
                GetCaretPos(&mut caret);
                point = caret;
                ClientToScreen(editor, &mut point);
                point.y += self.px(24);
            }
            let readonly = GetWindowLongPtrW(editor, GWL_STYLE) as u32 & ES_READONLY as u32 != 0;
            let mut start = 0u32;
            let mut end = 0u32;
            SendMessageW(
                editor,
                EM_GETSEL,
                &mut start as *mut _ as usize,
                &mut end as *mut _ as isize,
            );
            let selected = start != end;
            let mut items = vec![
                MenuItem::action("撤销", "Ctrl+Z", EDIT_UNDO),
                MenuItem::separator(),
                MenuItem::action("剪切", "Ctrl+X", EDIT_CUT),
                MenuItem::action("复制", "Ctrl+C", EDIT_COPY),
                MenuItem::action("粘贴", "Ctrl+V", EDIT_PASTE),
                MenuItem::action("删除", "", EDIT_DELETE),
                MenuItem::separator(),
                MenuItem::action("全选", "Ctrl+A", EDIT_SELECT_ALL),
            ];
            items[0].enabled = !readonly && SendMessageW(editor, EM_CANUNDO, 0, 0) != 0;
            items[2].enabled = !readonly && selected;
            items[3].enabled = selected;
            items[4].enabled = !readonly && IsClipboardFormatAvailable(TEXT_CLIPBOARD_FORMAT) != 0;
            items[5].enabled = !readonly && selected;
            items[7].enabled = GetWindowTextLengthW(editor) > 0;
            self.open_popup(PopupMenu::new("文字编辑", items), point, Some(editor))
        }
    }
    fn ensure_popup_window(&mut self) -> Result<(), String> {
        if !self.popup_hwnd.is_null() {
            return Ok(());
        }
        // SAFETY: one class per process, all popup HWNDs are owned by their dock.
        unsafe {
            let name = wide(&format!("PasteLeaf.Popup.{}", std::process::id()));
            let mut class: WNDCLASSW = zeroed();
            class.lpfnWndProc = Some(popup_proc);
            class.hInstance = GetModuleHandleW(null());
            class.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
            class.lpszClassName = name.as_ptr();
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err("无法初始化菜单窗口".into());
            }
            self.popup_hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                name.as_ptr(),
                wide("PasteLeaf 菜单").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                self.hwnd,
                null_mut(),
                class.hInstance,
                null(),
            );
            if self.popup_hwnd.is_null() {
                return Err("无法打开菜单".into());
            }
            frame::configure(self.popup_hwnd, self.palette.dark);
        }
        Ok(())
    }
    fn open_popup(
        &mut self,
        menu: PopupMenu,
        point: POINT,
        editor: Option<HWND>,
    ) -> Result<(), String> {
        self.close_menu();
        self.ensure_popup_window()?;
        self.popup = Some(menu);
        self.menu_editor = editor;
        self.menu_anchor = point;
        self.layout_popup();
        // SAFETY: no-activate owned popup; key handling remains on the dock.
        unsafe {
            SetFocus(self.hwnd);
            ShowWindow(self.popup_hwnd, SW_SHOWNOACTIVATE);
            InvalidateRect(self.popup_hwnd, null(), 0);
            self.paint_popup();
            SetCapture(self.popup_hwnd);
        }
        Ok(())
    }
    fn layout_popup(&mut self) {
        let Some(menu) = self.popup.as_mut() else {
            return;
        };
        // SAFETY: monitor work bounds are initialized before use. Clamp in
        // physical screen coordinates, including monitors left of the primary.
        unsafe {
            let monitor = MonitorFromPoint(self.menu_anchor, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return;
            }
            let max_height = ((info.rcWork.bottom - info.rcWork.top) as f32 / self.scale) as i32;
            menu.fit_rows(((max_height - 64).max(36) / 36) as usize);
            let width = (menu.width() as f32 * self.scale).round() as i32;
            let height = (menu.height() as f32 * self.scale).round() as i32;
            let x = self.menu_anchor.x.clamp(
                info.rcWork.left,
                (info.rcWork.right - width).max(info.rcWork.left),
            );
            let y = self.menu_anchor.y.clamp(
                info.rcWork.top,
                (info.rcWork.bottom - height).max(info.rcWork.top),
            );
            SetWindowPos(
                self.popup_hwnd,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            InvalidateRect(self.popup_hwnd, null(), 0);
        }
    }
    fn close_menu(&mut self) {
        self.popup = None;
        let editor = self.menu_editor.take();
        // SAFETY: capture is released only if it belongs to this popup.
        unsafe {
            if GetCapture() == self.popup_hwnd {
                ReleaseCapture();
            }
            if !self.popup_hwnd.is_null() {
                ShowWindow(self.popup_hwnd, SW_HIDE);
            }
            if let Some(editor) = editor {
                if IsWindowVisible(self.hwnd) != 0 && IsWindowVisible(editor) != 0 {
                    SetFocus(editor);
                }
            }
        }
    }
    fn activate_menu(&mut self) -> Result<(), String> {
        if let Some(command) = self.popup.as_mut().and_then(PopupMenu::activate) {
            self.run_menu_action(command)?;
        } else {
            self.layout_popup();
        }
        Ok(())
    }
    fn run_menu_action(&mut self, command: usize) -> Result<(), String> {
        let editor = self.menu_editor;
        self.close_menu();
        if (EDIT_UNDO..=EDIT_SELECT_ALL).contains(&command) {
            if let Some(editor) = editor {
                // SAFETY: EDIT maintains its own selection and undo stack. Never
                // replace the document text to implement cut/paste/delete.
                unsafe {
                    let readonly =
                        GetWindowLongPtrW(editor, GWL_STYLE) as u32 & ES_READONLY as u32 != 0;
                    if readonly && ![EDIT_COPY, EDIT_SELECT_ALL].contains(&command) {
                        return Ok(());
                    }
                    match command {
                        EDIT_UNDO => {
                            SendMessageW(editor, EM_UNDO, 0, 0);
                        }
                        EDIT_CUT => {
                            SendMessageW(editor, WM_CUT, 0, 0);
                        }
                        EDIT_COPY => {
                            SendMessageW(editor, WM_COPY, 0, 0);
                        }
                        EDIT_PASTE => {
                            SendMessageW(editor, WM_PASTE, 0, 0);
                        }
                        EDIT_DELETE => {
                            SendMessageW(editor, EM_REPLACESEL, 1, wide("").as_ptr() as isize);
                        }
                        EDIT_SELECT_ALL => {
                            SendMessageW(editor, EM_SETSEL, 0, -1);
                        }
                        _ => {}
                    }
                    SetFocus(editor);
                }
                if editor == self.control(SEARCH) {
                    self.offset = 0;
                    self.refresh();
                }
            }
            return Ok(());
        }
        if (CMD_BOARD_BASE..CMD_BOARD_BASE + self.model.categories.len() + 1).contains(&command) {
            self.select_board(command - CMD_BOARD_BASE);
            self.board_offset = self.board.saturating_sub(3);
            return Ok(());
        }
        self.command(command)
    }
    fn keyboard(&mut self, key: u16) -> Result<bool, String> {
        let ctrl = down(VK_CONTROL);
        let shift = down(VK_SHIFT);
        if self.popup.is_some() {
            match key {
                VK_ESCAPE | VK_LEFT => {
                    if !self.popup.as_mut().unwrap().back() {
                        self.close_menu();
                    }
                }
                VK_UP => self.popup.as_mut().unwrap().move_selection(-1),
                VK_DOWN | VK_TAB => {
                    self.popup
                        .as_mut()
                        .unwrap()
                        .move_selection(if shift { -1 } else { 1 })
                }
                VK_RETURN | VK_SPACE => self.activate_menu()?,
                VK_RIGHT => {
                    let menu = self.popup.as_ref().unwrap();
                    if menu
                        .selected
                        .is_some_and(|index| !menu.page().items[index].children.is_empty())
                    {
                        self.activate_menu()?;
                    }
                }
                _ => return Ok(true),
            }
            self.layout_popup();
            return Ok(true);
        }
        if key == VK_APPS || (shift && key == VK_F10) {
            // SAFETY: current UI-thread focus; only owned EDITs are accepted.
            let focus = unsafe { GetFocus() };
            if [SEARCH, CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT]
                .iter()
                .any(|id| self.control(*id) == focus)
            {
                self.editor_menu(focus, POINT { x: -1, y: -1 })?;
            } else {
                let hit = self
                    .selected
                    .map(Hit::Card)
                    .or(Some(Hit::Board(self.board)));
                let point = self
                    .hits
                    .iter()
                    .find(|(_, h)| Some(h) == hit.as_ref())
                    .map(|(area, _)| self.screen_point(area.x + 16, area.y + 30))
                    .unwrap_or_else(|| self.screen_point(24, 60));
                self.open_context_menu(hit, point)?;
            }
            return Ok(true);
        }
        if key == VK_ESCAPE {
            if self.dialog != Dialog::None {
                self.close_dialog();
            } else {
                self.hide();
            }
            return Ok(true);
        }
        if self.dialog != Dialog::None {
            if matches!(self.dialog, Dialog::NewItem(_)) {
                if ctrl && key == VK_RETURN {
                    self.accept_dialog()?;
                    return Ok(true);
                }
                return Ok(false);
            }
            if key == VK_RETURN && !matches!(self.dialog, Dialog::Preview(_)) {
                self.accept_dialog()?;
                return Ok(true);
            }
            return Ok(false);
        }
        if ctrl && key == b'F' as u16 {
            // SAFETY: owned EDIT.
            unsafe {
                SetFocus(self.control(SEARCH));
                SendMessageW(self.control(SEARCH), EM_SETSEL, 0, -1);
            }
            return Ok(true);
        }
        if ctrl && key == b'N' as u16 && shift {
            self.open_dialog(Dialog::NewBoard);
            return Ok(true);
        }
        if ctrl && key == b'N' as u16 {
            self.command(CMD_NEW_ITEM)?;
            return Ok(true);
        }
        if ctrl && key == VK_OEM_COMMA {
            self.open_dialog(Dialog::Settings);
            return Ok(true);
        }
        if ctrl && key == b'T' as u16 {
            self.command(CMD_PAUSE)?;
            return Ok(true);
        }
        if ctrl && (key == VK_LEFT || key == VK_RIGHT) {
            let n = self.model.categories.len() + 1;
            let b = if key == VK_LEFT {
                (self.board + n - 1) % n
            } else {
                (self.board + 1) % n
            };
            self.select_board(b);
            self.board_offset = b.saturating_sub(3);
            return Ok(true);
        }
        if ctrl && (b'1' as u16..=b'9' as u16).contains(&key) {
            if let Some(id) = self.visible.get(self.offset + (key - b'1' as u16) as usize) {
                self.selected = Some(*id);
                self.paste_selected()?;
            }
            return Ok(true);
        }
        // SAFETY: thread-local focus query.
        let searching = unsafe { GetFocus() == self.control(SEARCH) };
        if key == VK_TAB {
            // SAFETY: focus moves only between our controls.
            unsafe {
                SetFocus(if searching {
                    self.hwnd
                } else {
                    self.control(SEARCH)
                });
            }
            return Ok(true);
        }
        if key == VK_RETURN {
            self.paste_selected()?;
            return Ok(true);
        }
        if searching {
            return Ok(false);
        }
        match key {
            VK_LEFT | VK_RIGHT => {
                let i = self
                    .visible
                    .iter()
                    .position(|id| Some(*id) == self.selected)
                    .unwrap_or(0);
                let n = if key == VK_LEFT {
                    i.saturating_sub(1)
                } else {
                    (i + 1).min(self.visible.len().saturating_sub(1))
                };
                self.selected = self.visible.get(n).copied();
                self.ensure_selection_visible();
            }
            VK_SPACE => {
                if let Some(i) = self.selected {
                    self.open_dialog(Dialog::Preview(i));
                }
            }
            VK_DELETE => self.command(CMD_DELETE)?,
            k if ctrl && k == b'C' as u16 => self.copy_selected()?,
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn tray(&mut self, op: u32) -> bool {
        // SAFETY: initialized POD, shared system icon needs no destruction.
        unsafe {
            let mut n: NOTIFYICONDATAW = zeroed();
            n.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            n.hWnd = self.hwnd;
            n.uID = 1;
            n.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            n.uCallbackMessage = TRAY;
            n.hIcon = LoadIconW(
                GetModuleHandleW(null()),
                std::ptr::without_provenance::<u16>(1),
            );
            for (d, s) in n
                .szTip
                .iter_mut()
                .zip(wide(&format!("PasteLeaf · {}", self.model.hotkey)))
            {
                *d = s;
            }
            Shell_NotifyIconW(op, &n) != 0
        }
    }
    fn release_resources(&mut self) {
        self.close_menu();
        // SAFETY: invoked after window destruction, none of these remain selected.
        unsafe {
            if !self.popup_hwnd.is_null() {
                if IsWindow(self.popup_hwnd) != 0 {
                    DestroyWindow(self.popup_hwnd);
                }
                self.popup_hwnd = null_mut();
            }
            for f in self.fonts {
                DeleteObject(f);
            }
            DeleteObject(self.edit_brush);
        }
    }
}
fn key_input(key: u16, up: bool) -> INPUT {
    // SAFETY: union tag and active member both represent keyboard input.
    unsafe {
        let mut i: INPUT = zeroed();
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki = KEYBDINPUT {
            wVk: key,
            wScan: 0,
            dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
            time: 0,
            dwExtraInfo: 0,
        };
        i
    }
}
fn clipboard_source() -> String {
    // SAFETY: read-only process query, closed before return.
    unsafe {
        let mut h = GetClipboardOwner();
        if h.is_null() {
            h = GetForegroundWindow();
        }
        let mut pid = 0;
        GetWindowThreadProcessId(h, &mut pid);
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return "剪贴板".into();
        }
        let mut b = vec![0u16; 1024];
        let mut n = b.len() as u32;
        let ok = QueryFullProcessImageNameW(p, 0, b.as_mut_ptr(), &mut n);
        CloseHandle(p);
        if ok == 0 {
            return "剪贴板".into();
        }
        let path = String::from_utf16_lossy(&b[..n as usize]);
        PathBuf::from(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "剪贴板".into())
    }
}
unsafe extern "system" fn editor_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    owner: usize,
) -> LRESULT {
    // SAFETY: installed on owned EDITs on the same UI thread. DefSubclassProc
    // preserves native editing behavior; only context menus are intercepted.
    unsafe {
        if msg == WM_CONTEXTMENU {
            PostMessageW(owner as HWND, EDIT_MENU_MESSAGE, hwnd as usize, lp);
            return 0;
        }
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(editor_proc), 1);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: popup messages stay on the dock UI thread. It never activates and
    // never runs a nested native menu loop while APP is mutably borrowed.
    unsafe {
        if let Some(result) = frame::message(hwnd, msg, wp, lp) {
            return result;
        }
        if msg == WM_MOUSEACTIVATE {
            return MA_NOACTIVATE as isize;
        }
        if msg == WM_ERASEBKGND {
            return 1;
        }
        let result = APP.with(|cell| {
            let Ok(mut state) = cell.try_borrow_mut() else {
                return None;
            };
            let a = state.as_mut()?;
            if a.popup_hwnd != hwnd {
                return None;
            }
            match msg {
                WM_PAINT => {
                    a.paint_popup();
                    Some(0)
                }
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN => {
                    let x = (lp as u16 as i16 as f32 / a.scale) as i32;
                    let y = ((lp >> 16) as u16 as i16 as f32 / a.scale) as i32;
                    let Some(menu) = a.popup.as_mut() else {
                        return Some(0);
                    };
                    let inside = x >= 0 && x < menu.width() && y >= 0 && y < menu.height();
                    if !inside {
                        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
                            a.close_menu();
                        }
                        return Some(0);
                    }
                    if y < 40 && !menu.page().title.is_empty() && menu.pages.len() > 1 {
                        if msg == WM_LBUTTONUP {
                            menu.back();
                            a.layout_popup();
                        }
                        return Some(0);
                    }
                    if let Some(index) = menu.row_at(y) {
                        menu.select(index);
                        InvalidateRect(hwnd, null(), 0);
                        if msg == WM_LBUTTONUP && menu.page().items[index].enabled {
                            if let Err(error) = a.activate_menu() {
                                a.notify(error);
                            }
                        }
                    }
                    Some(0)
                }
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    if let Some(menu) = a.popup.as_mut() {
                        menu.scroll(if ((wp >> 16) as u16 as i16) < 0 {
                            1
                        } else {
                            -1
                        });
                        a.layout_popup();
                    }
                    Some(0)
                }
                WM_CAPTURECHANGED => {
                    if a.popup.is_some() && lp as HWND != hwnd {
                        a.close_menu();
                    }
                    Some(0)
                }
                _ => None,
            }
        });
        result.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: Windows invokes this on our UI thread; nested state access is refused.
    unsafe {
        if let Some(result) = frame::message(hwnd, msg, wp, lp) {
            return result;
        }
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        if msg == EXIT_MESSAGE {
            DestroyWindow(hwnd);
            return 0;
        }
        // Clipboard notifications can arrive inside a native modal message loop
        // while APP is borrowed. Schedule outside the borrow so they survive it.
        if msg == WM_CLIPBOARDUPDATE {
            CAPTURE_PENDING.with(|pending| pending.set(true));
            SetTimer(hwnd, 1, 60, None);
            return 0;
        }
        let result = APP.with(|cell| {
            let Ok(mut state) = cell.try_borrow_mut() else {
                return None;
            };
            let a = state.as_mut()?;
            if msg == a.taskbar_created && msg != 0 {
                a.tray_ok = a.tray(NIM_ADD);
                return Some(0);
            }
            match msg {
                WM_CLOSE => {
                    a.hide();
                    Some(0)
                }
                WM_HOTKEY => {
                    if IsWindowVisible(hwnd) != 0 {
                        a.hide();
                    } else {
                        a.show();
                    }
                    Some(0)
                }
                WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
                    if a.dialog == Dialog::None
                        && !a.dragging
                        && std::env::var_os("PASTELEAF_KEEP_OPEN").is_none()
                    {
                        a.close_menu();
                        ShowWindow(hwnd, SW_HIDE);
                    }
                    Some(0)
                }
                EDIT_MENU_MESSAGE => {
                    let point = POINT {
                        x: lp as u16 as i16 as i32,
                        y: (lp >> 16) as u16 as i16 as i32,
                    };
                    if let Err(error) = a.editor_menu(wp as HWND, point) {
                        a.notify(error);
                    }
                    Some(0)
                }
                WM_CONTEXTMENU => {
                    let mut point = POINT {
                        x: lp as u16 as i16 as i32,
                        y: (lp >> 16) as u16 as i16 as i32,
                    };
                    let hit = if point.x == -1 && point.y == -1 {
                        let selected = a.selected.map(Hit::Card).or(Some(Hit::Board(a.board)));
                        if let Some((area, _)) = a
                            .hits
                            .iter()
                            .find(|(_, hit)| Some(hit) == selected.as_ref())
                        {
                            point = a.screen_point(area.x + 16, area.y + 30);
                        } else {
                            point = a.screen_point(24, 60);
                        }
                        selected
                    } else {
                        let mut client = point;
                        ScreenToClient(hwnd, &mut client);
                        a.hit(
                            (client.x as f32 / a.scale) as i32,
                            (client.y as f32 / a.scale) as i32,
                        )
                    };
                    if let Err(error) = a.open_context_menu(hit, point) {
                        a.notify(error);
                    }
                    Some(0)
                }
                WM_COMMAND => {
                    if wp & 0xffff == SEARCH as usize && (wp >> 16) & 0xffff == EN_CHANGE as usize {
                        a.offset = 0;
                        a.refresh();
                    }
                    Some(0)
                }
                WM_TIMER if wp == 1 => {
                    KillTimer(hwnd, 1);
                    if CAPTURE_PENDING.with(|pending| pending.replace(false)) {
                        a.attempts = 0;
                    }
                    if let Err(e) = a.capture() {
                        a.attempts += 1;
                        if a.attempts < 4 {
                            SetTimer(hwnd, 1, 100, None);
                        } else {
                            a.notify(format!("未记录：{e}"));
                        }
                    }
                    Some(0)
                }
                WM_TIMER if wp == 2 => {
                    a.finish_paste();
                    Some(0)
                }
                TRAY => {
                    match lp as u32 {
                        WM_LBUTTONUP => a.show(),
                        WM_RBUTTONUP => {
                            a.remember_target();
                            if let Err(e) = a.menu(None) {
                                a.notify(e);
                            }
                        }
                        _ => {}
                    }
                    Some(0)
                }
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                    let x = ((lp as u16 as i16) as f32 / a.scale) as i32;
                    let y = (((lp >> 16) as u16 as i16) as f32 / a.scale) as i32;
                    if a.popup.is_some() {
                        a.close_menu();
                        return Some(0);
                    }
                    if let Some(h) = a.hit(x, y) {
                        if let Hit::Card(id) = h {
                            SetFocus(hwnd);
                            a.selected = Some(id);
                            a.drag_origin = Some((x, y));
                            SetCapture(hwnd);
                            if msg == WM_LBUTTONDBLCLK {
                                a.drag_origin = None;
                                ReleaseCapture();
                                if let Err(e) = a.paste_selected() {
                                    a.notify(e);
                                }
                            } else {
                                a.invalidate();
                            }
                        } else if let Err(e) = a.click(h) {
                            a.notify(e);
                        }
                    }
                    Some(0)
                }
                WM_MOUSEMOVE => {
                    let x = ((lp as u16 as i16) as f32 / a.scale) as i32;
                    let y = (((lp >> 16) as u16 as i16) as f32 / a.scale) as i32;
                    if let Some((ox, oy)) = a.drag_origin {
                        if (x - ox).abs() + (y - oy).abs() > 8 {
                            a.dragging = true;
                        }
                    }
                    let h = a.hit(x, y);
                    if a.hover != h {
                        a.hover = h;
                        a.invalidate();
                    }
                    Some(0)
                }
                WM_LBUTTONUP => {
                    ReleaseCapture();
                    if a.dragging {
                        if let Some(Hit::Board(i)) = &a.hover {
                            if *i > 0 {
                                if let Err(e) = a.command(CMD_PIN_BASE + i - 1) {
                                    a.notify(e);
                                }
                            }
                        }
                    }
                    a.dragging = false;
                    a.drag_origin = None;
                    a.invalidate();
                    Some(0)
                }
                WM_CAPTURECHANGED => {
                    a.drag_origin = None;
                    Some(0)
                }
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    let d = (wp >> 16) as u16 as i16;
                    if let Some(menu) = a.popup.as_mut() {
                        menu.scroll(if d < 0 { 1 } else { -1 });
                        a.layout_popup();
                        return Some(0);
                    }
                    let forward = if msg == WM_MOUSEHWHEEL { d > 0 } else { d < 0 };
                    a.offset = if forward {
                        (a.offset + 1).min(a.visible.len().saturating_sub(a.page_size()))
                    } else {
                        a.offset.saturating_sub(1)
                    };
                    a.invalidate();
                    Some(0)
                }
                WM_CHAR if a.dialog == Dialog::None && a.popup.is_none() => {
                    if wp >= 32 && wp != 127 {
                        SetFocus(a.control(SEARCH));
                        SendMessageW(a.control(SEARCH), WM_CHAR, wp, lp);
                        a.offset = 0;
                        a.refresh();
                    }
                    Some(0)
                }
                WM_DPICHANGED => {
                    a.position();
                    Some(0)
                }
                WM_PAINT => {
                    a.paint();
                    Some(0)
                }
                WM_ERASEBKGND => Some(1),
                WM_SETTINGCHANGE | WM_THEMECHANGED => {
                    a.refresh_theme();
                    Some(0)
                }
                WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
                    SetTextColor(wp as HDC, colorref(a.palette.text));
                    SetBkColor(wp as HDC, colorref(a.palette.input));
                    Some(a.edit_brush as isize)
                }
                _ => None,
            }
        });
        result.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
    }
}
fn system_dark() -> bool {
    use windows_sys::Win32::System::Registry::*;
    let mut light = 1u32;
    let mut size = 4u32;
    // SAFETY: predefined read-only key, live UTF-16 names and a DWORD-sized buffer.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut light as *mut _ as _,
            &mut size,
        ) == ERROR_SUCCESS
            && light == 0
    }
}

pub fn run() -> Result<(), String> {
    let autostart = std::env::args_os().any(|arg| arg == "--autostart");
    let root = std::env::var_os("PASTELEAF_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|s| PathBuf::from(s).join("PasteLeaf")))
        .ok_or("找不到本地数据目录")?;
    let store = Store::new(root)?;
    let model = store.load()?;
    let key = blake3::hash(store.root.to_string_lossy().to_lowercase().as_bytes()).to_hex();
    // SAFETY: all native resources belong to this thread until the loop terminates.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mutex = CreateMutexW(null(), 0, wide(&format!("Local\\PasteLeaf.{key}")).as_ptr());
        if mutex.is_null() {
            return Err("无法创建单实例锁".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            if autostart {
                return Ok(());
            }
            return Err("PasteLeaf 已在运行，请用快捷键或托盘打开。".into());
        }
        let graphics = render::start_graphics()?;
        let instance = GetModuleHandleW(null());
        let name = wide(&format!("PasteLeaf.{}", std::process::id()));
        let mut wc: WNDCLASSW = zeroed();
        wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = instance;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(instance, std::ptr::without_provenance::<u16>(1));
        wc.lpszClassName = name.as_ptr();
        if RegisterClassW(&wc) == 0 {
            CloseHandle(mutex);
            return Err("无法注册窗口".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW | WS_EX_TOPMOST,
            name.as_ptr(),
            wide("PasteLeaf").as_ptr(),
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            0,
            0,
            1200,
            368,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            CloseHandle(mutex);
            return Err("无法创建窗口".into());
        }
        let mut app = App::new(hwnd, store, model, GetDpiForWindow(hwnd) as f32 / 96.0)?;
        frame::configure(hwnd, app.palette.dark);
        app.remember_target();
        app.position();
        app.ignore_sequence = GetClipboardSequenceNumber();
        app.taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        app.tray_ok = app.tray(NIM_ADD);
        if AddClipboardFormatListener(hwnd) == 0 {
            DestroyWindow(hwnd);
            CloseHandle(mutex);
            return Err("无法监听剪贴板".into());
        }
        if let Ok(k) = Hotkey::parse(&app.model.hotkey) {
            if RegisterHotKey(hwnd, 1, k.modifiers | MOD_NOREPEAT, k.key) != 0 {
                app.hotkey_id = Some(1);
            } else {
                app.status = "快捷键被占用，请在设置中更换；可从托盘打开".into();
            }
        }
        let show = !autostart || !app.tray_ok || app.hotkey_id.is_none();
        APP.with(|s| *s.borrow_mut() = Some(app));
        if show {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            UpdateWindow(hwnd);
        }
        let mut msg: MSG = zeroed();
        loop {
            if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
                break;
            }
            let consumed = if msg.message == WM_KEYDOWN {
                APP.with(|cell| {
                    let mut s = cell.borrow_mut();
                    if let Some(a) = s.as_mut() {
                        match a.keyboard(msg.wParam as u16) {
                            Ok(v) => v,
                            Err(e) => {
                                a.notify(e);
                                true
                            }
                        }
                    } else {
                        false
                    }
                })
            } else {
                false
            };
            if !consumed {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        APP.with(|s| {
            if let Some(mut a) = s.borrow_mut().take() {
                a.tray(NIM_DELETE);
                RemoveClipboardFormatListener(hwnd);
                if let Some(id) = a.hotkey_id {
                    UnregisterHotKey(hwnd, id);
                }
                a.release_resources();
            }
        });
        render::stop_graphics(graphics);
        CloseHandle(mutex);
    }
    Ok(())
}
#[cfg(test)]
#[path = "native_tests.rs"]
mod native_tests;
