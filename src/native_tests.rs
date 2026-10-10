use super::*;
use std::{
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

struct ManualFavoriteFixture {
    app: App,
    root: PathBuf,
}

impl ManualFavoriteFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "pasteleaf-manual-favorite-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::new(root.clone()).unwrap();
        let mut model = Model {
            paused: true,
            ..Model::default()
        };
        model.add_category("工作").unwrap();
        model.capture(Content::Text("已有记录".into())).unwrap();
        store.save(&model).unwrap();
        // SAFETY: hidden STATIC parent and native EDIT controls are owned by
        // this fixture. No clipboard, global hotkey or startup registry is used.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("Manual favorite regression").as_ptr(),
                WS_POPUP,
                0,
                0,
                1200,
                368,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        assert!(!hwnd.is_null());
        let app = App::new(hwnd, store, model, 1.0).unwrap();
        Self { app, root }
    }

    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.app.model).unwrap()
    }
}

impl Drop for ManualFavoriteFixture {
    fn drop(&mut self) {
        // SAFETY: only this fixture's hidden parent and its GDI resources.
        unsafe { DestroyWindow(self.app.hwnd) };
        self.app.release_resources();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn manual_favorite_saves_multiline_text_while_capture_paused() {
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    app.select_board(1);
    app.command(218).unwrap();
    assert_ne!(
        app.dialog,
        Dialog::None,
        "new-item command must open an editor"
    );
    let text = "你好，手动收藏\r\nSecond line: Rust + 中文\r\n第三行";
    app.set_text(131, text);
    let before_enter = serde_json::to_vec(&app.model).unwrap();
    let disk_before_enter = std::fs::read(app.store.root.join("library.json")).unwrap();
    assert!(
        !app.keyboard(VK_RETURN).unwrap(),
        "plain Enter must reach the multiline EDIT instead of saving"
    );
    assert_eq!(serde_json::to_vec(&app.model).unwrap(), before_enter);
    assert_eq!(
        std::fs::read(app.store.root.join("library.json")).unwrap(),
        disk_before_enter
    );
    assert_ne!(app.dialog, Dialog::None);
    app.accept_dialog().unwrap();
    assert_eq!(app.dialog, Dialog::None);
    assert!(
        app.model.paused,
        "manual creation must preserve capture pause"
    );
    assert_eq!(app.model.items.len(), 2);
    let saved = &app.model.items[0];
    assert_eq!(saved.content, Content::Text(text.into()));
    assert_eq!(saved.category.as_deref(), Some("收藏"));
    assert_eq!(app.selected, Some(saved.id));
    assert_eq!(app.visible, vec![saved.id]);
    let reopened = app.store.load().unwrap();
    assert_eq!(reopened.items[0].content, saved.content);
    assert_eq!(reopened.items[0].category, saved.category);
    assert!(reopened.paused);
}

#[test]
fn manual_favorite_uses_custom_category_and_clears_search() {
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    app.select_board(2);
    app.set_text(SEARCH, "unmatched-search");
    app.refresh();
    app.command(218).unwrap();
    assert_ne!(
        app.dialog,
        Dialog::None,
        "new-item command must open an editor"
    );
    app.set_text(131, "custom category note");
    app.accept_dialog().unwrap();
    let saved = &app.model.items[0];
    assert_eq!(saved.category.as_deref(), Some("工作"));
    assert_eq!(app.board, 2);
    assert_eq!(app.text(SEARCH), "");
    assert_eq!(app.selected, Some(saved.id));
    assert_eq!(app.visible, vec![saved.id]);
    assert_eq!(app.store.load().unwrap().items[0].category, saved.category);
}

#[test]
fn manual_favorite_rejects_blank_and_cancel_preserves_library() {
    let mut fixture = ManualFavoriteFixture::new();
    let before = fixture.snapshot();
    let disk_before = std::fs::read(fixture.root.join("library.json")).unwrap();
    fixture.app.select_board(1);
    fixture.app.command(218).unwrap();
    assert_ne!(
        fixture.app.dialog,
        Dialog::None,
        "new-item command must open an editor"
    );
    let blank = " \r\n\t ";
    fixture.app.set_text(131, blank);
    let error = fixture.app.accept_dialog().unwrap_err();
    fixture.app.notify(error);
    assert_ne!(fixture.app.dialog, Dialog::None);
    assert_eq!(fixture.app.text(131), blank);
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(
        std::fs::read(fixture.root.join("library.json")).unwrap(),
        disk_before
    );
    fixture.app.close_dialog();
    fixture.app.command(218).unwrap();
    fixture.app.set_text(131, "cancel this draft");
    fixture.app.close_dialog();
    assert_eq!(fixture.app.dialog, Dialog::None);
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(
        std::fs::read(fixture.root.join("library.json")).unwrap(),
        disk_before
    );
}

#[test]
fn manual_favorite_disk_failure_preserves_draft_and_previous_library() {
    let mut fixture = ManualFavoriteFixture::new();
    let before = fixture.snapshot();
    let disk_before = std::fs::read(fixture.root.join("library.json")).unwrap();
    fixture.app.select_board(1);
    fixture.app.command(218).unwrap();
    assert_ne!(
        fixture.app.dialog,
        Dialog::None,
        "new-item command must open an editor"
    );
    let draft = "保存失败时保留草稿\r\nretry after disk recovery";
    fixture.app.set_text(131, draft);
    let obstruction = fixture.root.join("library.json.tmp");
    std::fs::create_dir(&obstruction).unwrap();
    let error = fixture.app.accept_dialog().unwrap_err();
    fixture.app.notify(error);
    assert_ne!(fixture.app.dialog, Dialog::None);
    assert_eq!(fixture.app.text(131), draft);
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(
        std::fs::read(fixture.root.join("library.json")).unwrap(),
        disk_before
    );
    std::fs::remove_dir(obstruction).unwrap();
    fixture.app.accept_dialog().unwrap();
    assert_eq!(fixture.app.dialog, Dialog::None);
    assert!(
        fixture.app.status.contains("已添加"),
        "a successful retry must replace the previous save-error status"
    );
    let reopened = fixture.app.store.load().unwrap();
    assert_eq!(reopened.items.len(), 2);
    assert_eq!(reopened.items[0].content, Content::Text(draft.into()));
    assert_eq!(reopened.items[0].category.as_deref(), Some("收藏"));
}

struct ManualFavoriteGraphics(usize);

impl Drop for ManualFavoriteGraphics {
    fn drop(&mut self) {
        render::stop_graphics(self.0);
    }
}

fn themed_menu_actions(app: &App) -> Vec<usize> {
    fn collect(items: &[popup::MenuItem], actions: &mut Vec<usize>) {
        for item in items {
            if let Some(action) = item.action {
                actions.push(action);
            }
            collect(&item.children, actions);
        }
    }
    let mut actions = Vec::new();
    collect(
        &app.popup
            .as_ref()
            .expect("themed popup must open")
            .page()
            .items,
        &mut actions,
    );
    actions
}

#[test]
fn themed_menu_card_and_category_preserve_complete_command_sets() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let id = app.model.items[0].id;
    app.open_context_menu(Some(Hit::Card(id)), POINT { x: 0, y: 0 })
        .unwrap();
    assert_eq!(app.selected, Some(id));
    assert_eq!(
        themed_menu_actions(app),
        vec![201, 202, 203, 1000, 1001, 205, 204]
    );
    app.run_menu_action(1001).unwrap();
    assert_eq!(app.model.items[0].category.as_deref(), Some("工作"));
    assert_eq!(
        app.store.load().unwrap().items[0].category.as_deref(),
        Some("工作")
    );
    app.open_context_menu(Some(Hit::Board(1)), POINT { x: 0, y: 0 })
        .unwrap();
    assert_eq!(app.board, 1);
    assert_eq!(themed_menu_actions(app), vec![218, 216, 217]);
    app.run_menu_action(218).unwrap();
    assert_eq!(app.dialog, Dialog::NewItem("收藏".into()));
    app.close_dialog();
}

#[test]
fn themed_menu_hundred_categories_are_complete_and_selectable() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    for index in 2..100 {
        app.model.add_category(&format!("分类{index:03}")).unwrap();
    }
    app.board_menu().unwrap();
    assert_eq!(themed_menu_actions(app), (2000..=2100).collect::<Vec<_>>());
    assert!(app.popup.as_ref().unwrap().rows() <= 12);
    app.run_menu_action(2100).unwrap();
    assert_eq!(app.board, 100);
    assert_eq!(app.board_offset, 97);
    let id = app.model.items[0].id;
    app.open_context_menu(Some(Hit::Card(id)), POINT { x: 0, y: 0 })
        .unwrap();
    let actions = themed_menu_actions(app);
    assert_eq!(
        actions
            .into_iter()
            .filter(|action| (1000..1100).contains(action))
            .collect::<Vec<_>>(),
        (1000..1100).collect::<Vec<_>>()
    );
    app.close_menu();
}

#[test]
fn themed_menu_readonly_preview_disables_every_mutating_edit_action() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let id = app.model.items[0].id;
    app.open_dialog(Dialog::Preview(id));
    let editor = app.control(PREVIEW);
    // SAFETY: select only text in this fixture's read-only preview EDIT.
    unsafe { SendMessageW(editor, EM_SETSEL, 0, -1) };
    app.editor_menu(editor, POINT { x: 0, y: 0 }).unwrap();
    let items = &app
        .popup
        .as_ref()
        .expect("editor popup must open")
        .page()
        .items;
    for action in [300, 301, 303, 304] {
        assert!(
            !items
                .iter()
                .any(|item| item.action == Some(action) && item.enabled),
            "read-only preview must not enable edit action {action}"
        );
    }
    for action in [302, 305] {
        assert!(items
            .iter()
            .any(|item| item.action == Some(action) && item.enabled));
    }
    app.close_menu();
    assert_eq!(app.text(PREVIEW), "已有记录");
    assert_eq!(app.dialog, Dialog::Preview(id));
}

#[test]
fn themed_menu_long_editor_selection_delete_and_undo_preserve_draft() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let before = fixture.snapshot();
    let disk_before = std::fs::read(fixture.root.join("library.json")).unwrap();
    let app = &mut fixture.app;
    app.select_board(1);
    app.command(218).unwrap();
    let draft = format!("{}DELETE_ME{}", "x".repeat(70_000), "尾段".repeat(100));
    app.set_text(ITEM_TEXT, &draft);
    let editor = app.control(ITEM_TEXT);
    // SAFETY: selection uses full 32-bit indexes in this fixture's EDIT.
    unsafe { SendMessageW(editor, EM_SETSEL, 70_000, 70_009) };
    app.editor_menu(editor, POINT { x: 0, y: 0 }).unwrap();
    assert_eq!(app.menu_editor, Some(editor));
    assert!(app
        .popup
        .as_ref()
        .unwrap()
        .page()
        .items
        .iter()
        .any(|item| item.action == Some(304) && item.enabled));
    app.run_menu_action(304).unwrap();
    assert_eq!(
        app.text(ITEM_TEXT),
        format!("{}{}", "x".repeat(70_000), "尾段".repeat(100))
    );
    app.editor_menu(editor, POINT { x: 0, y: 0 }).unwrap();
    assert!(app
        .popup
        .as_ref()
        .unwrap()
        .page()
        .items
        .iter()
        .any(|item| item.action == Some(300) && item.enabled));
    app.run_menu_action(300).unwrap();
    assert_eq!(app.text(ITEM_TEXT), draft);
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(
        std::fs::read(fixture.root.join("library.json")).unwrap(),
        disk_before
    );
}

#[test]
fn themed_menu_editor_popup_is_owned_top_level_and_cancel_retains_selection() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    app.select_board(1);
    app.command(218).unwrap();
    let draft = "未保存草稿\r\nselection stays here";
    app.set_text(ITEM_TEXT, draft);
    let editor = app.control(ITEM_TEXT);
    // SAFETY: native control and popup both belong to this fixture's thread.
    unsafe { SendMessageW(editor, EM_SETSEL, 2, 7) };
    app.editor_menu(editor, POINT { x: 0, y: 0 }).unwrap();
    assert!(!app.popup_hwnd.is_null());
    unsafe {
        let style = GetWindowLongPtrW(app.popup_hwnd, GWL_STYLE) as u32;
        assert_ne!(style & WS_POPUP, 0);
        assert_eq!(style & WS_CHILD, 0, "EDIT must not clip the popup window");
        assert_eq!(GetWindow(app.popup_hwnd, GW_OWNER), app.hwnd);
    }
    app.close_menu();
    assert!(app.popup.is_none());
    assert_eq!(app.text(ITEM_TEXT), draft);
    assert_eq!(app.dialog, Dialog::NewItem("收藏".into()));
    let mut start = 0u32;
    let mut end = 0u32;
    unsafe {
        SendMessageW(
            editor,
            EM_GETSEL,
            &mut start as *mut u32 as usize,
            &mut end as *mut u32 as isize,
        );
    }
    assert_eq!((start, end), (2, 7));
}

struct ThemedEditorDispatchFixture {
    root: PathBuf,
    class: Vec<u16>,
    instance: HINSTANCE,
    previous: Option<App>,
}

impl ThemedEditorDispatchFixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pasteleaf-editor-dispatch-{}-{nonce}",
            std::process::id()
        ));
        let store = Store::new(root.clone()).unwrap();
        let model = Model {
            paused: true,
            ..Model::default()
        };
        store.save(&model).unwrap();
        // SAFETY: unique hidden parent uses the production WndProc; EDITs use
        // production editor_proc. No clipboard listener or global hotkey exists.
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide(&format!("PasteLeaf.EditorDispatch.{nonce}"));
            let mut definition: WNDCLASSW = zeroed();
            definition.lpfnWndProc = Some(window_proc);
            definition.hInstance = instance;
            definition.lpszClassName = class.as_ptr();
            assert_ne!(RegisterClassW(&definition), 0);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("Editor dispatch regression").as_ptr(),
                WS_POPUP,
                0,
                0,
                1200,
                368,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            assert!(!hwnd.is_null());
            let app = App::new(hwnd, store, model, 1.0).unwrap();
            let previous = APP.with(|cell| cell.borrow_mut().replace(app));
            Self {
                root,
                class,
                instance,
                previous,
            }
        }
    }
}

impl Drop for ThemedEditorDispatchFixture {
    fn drop(&mut self) {
        let current = APP.with(|cell| cell.borrow_mut().take());
        if let Some(mut app) = current {
            // SAFETY: destroy only this test's parent and owned popup.
            unsafe { DestroyWindow(app.hwnd) };
            app.release_resources();
        }
        APP.with(|cell| *cell.borrow_mut() = self.previous.take());
        // SAFETY: only this test's unique class; remove WM_QUIT posted by its
        // production WndProc so the test worker can run another native test.
        unsafe {
            UnregisterClassW(self.class.as_ptr(), self.instance);
            let mut message: MSG = zeroed();
            while PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE) != 0 {}
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct ThemedKeyboardState([u8; 256]);

impl Drop for ThemedKeyboardState {
    fn drop(&mut self) {
        // SAFETY: restore only this test worker's thread keyboard-state table;
        // no keyboard events are injected into Windows or another application.
        unsafe { SetKeyboardState(self.0.as_ptr()) };
    }
}

#[test]
fn themed_menu_edit_subclass_posts_context_for_all_controls_and_dialog_keys() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let fixture = ThemedEditorDispatchFixture::new();
    let disk_before = std::fs::read(fixture.root.join("library.json")).unwrap();
    for id in [SEARCH, CATEGORY_NAME, HOTKEY, PREVIEW, ITEM_TEXT] {
        let (parent, editor) = APP.with(|cell| {
            let mut state = cell.borrow_mut();
            let app = state.as_mut().unwrap();
            app.close_menu();
            app.set_text(id, "context draft 中文");
            let editor = app.control(id);
            // SAFETY: production subclass receives a keyboard-context request
            // while APP is borrowed. Posting must defer access to the WndProc.
            unsafe { SendMessageW(editor, WM_CONTEXTMENU, editor as usize, -1) };
            assert!(
                app.popup.is_none(),
                "subclass must not reenter APP synchronously"
            );
            (app.hwnd, editor)
        });
        // SAFETY: dispatch only the posted private context message belonging to
        // this hidden parent; no real mouse or global keyboard state is used.
        unsafe {
            let mut message: MSG = zeroed();
            assert_ne!(
                PeekMessageW(
                    &mut message,
                    parent,
                    EDIT_MENU_MESSAGE,
                    EDIT_MENU_MESSAGE,
                    PM_REMOVE,
                ),
                0,
                "editor {id} must post its context request"
            );
            assert_eq!(message.wParam, editor as usize);
            assert_eq!(message.lParam, -1);
            DispatchMessageW(&message);
        }
        APP.with(|cell| {
            let mut state = cell.borrow_mut();
            let app = state.as_mut().unwrap();
            assert!(
                app.popup.is_some(),
                "editor {id} must open the themed popup"
            );
            assert_eq!(app.menu_editor, Some(editor));
            app.close_menu();
            assert_eq!(app.text(id), "context draft 中文");
        });
    }
    APP.with(|cell| {
        let mut state = cell.borrow_mut();
        let app = state.as_mut().unwrap();
        app.select_board(1);
        app.command(218).unwrap();
        assert!(app.keyboard(VK_APPS).unwrap());
        assert!(
            app.popup.is_some(),
            "menu key must remain reachable inside a dialog"
        );
        assert_eq!(app.dialog, Dialog::NewItem("收藏".into()));
        app.close_menu();
        let mut original = [0u8; 256];
        // SAFETY: change only this test worker's thread-local key-state table
        // to exercise the Shift+F10 branch without SendInput or user focus.
        unsafe {
            assert_ne!(GetKeyboardState(original.as_mut_ptr()), 0);
            let _keyboard = ThemedKeyboardState(original);
            let mut shifted = original;
            shifted[VK_SHIFT as usize] = 0x80;
            assert_ne!(SetKeyboardState(shifted.as_ptr()), 0);
            assert!(app.keyboard(VK_F10).unwrap());
        }
        assert!(
            app.popup.is_some(),
            "Shift+F10 must remain reachable inside a dialog"
        );
        assert_eq!(app.dialog, Dialog::NewItem("收藏".into()));
        app.close_menu();
    });
    assert_eq!(
        std::fs::read(fixture.root.join("library.json")).unwrap(),
        disk_before
    );
}

#[test]
fn manual_favorite_header_controls_do_not_overlap_at_small_widths() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    for index in 0..8 {
        fixture
            .app
            .model
            .add_category(&format!("更多分类{index}"))
            .unwrap();
    }
    for width in [488, 616, 744, 1200] {
        let app = &mut fixture.app;
        app.width = width;
        // SAFETY: resize only this hidden test-owned window.
        unsafe {
            SetWindowPos(app.hwnd, null_mut(), 0, 0, width, 368, SWP_NOACTIVATE);
        }
        app.select_board(1);
        app.layout_controls();
        app.paint();
        let header: Vec<_> = app
            .hits
            .iter()
            .filter(|(_, hit)| {
                matches!(
                    hit,
                    Hit::Board(_) | Hit::MoreBoards | Hit::Add | Hit::NewItem | Hit::Search
                )
            })
            .collect();
        assert!(header.iter().any(|(_, hit)| *hit == Hit::NewItem));
        for (index, (left, left_hit)) in header.iter().enumerate() {
            assert!(left.x >= 0 && left.x + left.w <= width);
            for (right, right_hit) in header.iter().skip(index + 1) {
                let overlaps = left.x < right.x + right.w
                    && right.x < left.x + left.w
                    && left.y < right.y + right.h
                    && right.y < left.y + left.h;
                assert!(
                    !overlaps,
                    "header controls {left_hit:?} and {right_hit:?} overlap at width {width}"
                );
            }
        }
    }
}

#[test]
fn manual_favorite_editor_and_save_stay_within_small_windows() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    for width in [488, 616, 744, 1200] {
        let app = &mut fixture.app;
        app.width = width;
        // SAFETY: only this hidden test window and owned native EDIT rectangle.
        unsafe {
            SetWindowPos(app.hwnd, null_mut(), 0, 0, width, 368, SWP_NOACTIVATE);
        }
        app.select_board(1);
        app.layout_controls();
        app.command(218).unwrap();
        app.paint();
        let mut bounds: RECT = unsafe { zeroed() };
        // SAFETY: GetWindowRect fills two points mapped into our parent's client.
        unsafe {
            assert_ne!(GetWindowRect(app.control(131), &mut bounds), 0);
            MapWindowPoints(null_mut(), app.hwnd, (&mut bounds as *mut RECT).cast(), 2);
        }
        assert!(
            bounds.left >= 0 && bounds.right <= width,
            "editor horizontal bounds {}..{} exceed width {width}",
            bounds.left,
            bounds.right
        );
        assert!(bounds.top >= 0 && bounds.bottom <= 368);
        let (save, _) = app
            .hits
            .iter()
            .find(|(_, hit)| *hit == Hit::Accept)
            .expect("new-item dialog must expose save hit area");
        assert!(
            save.x >= 0 && save.x + save.w <= width,
            "save button extends outside width {width}"
        );
        app.close_dialog();
    }
}

#[test]
fn clipboard_notification_during_modal_reentry_schedules_capture() {
    // SAFETY: test-owned hidden window; no clipboard contents are read or changed.
    unsafe {
        let instance = GetModuleHandleW(null());
        let name = wide(&format!("PasteLeaf.ModalTest.{}", GetCurrentThreadId()));
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = instance;
        wc.lpszClassName = name.as_ptr();
        assert_ne!(RegisterClassW(&wc), 0);
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            wide("modal test").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            100,
            100,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        assert!(!hwnd.is_null());
        APP.with(|cell| {
            let _busy = cell.borrow_mut();
            SendMessageW(hwnd, WM_CLIPBOARDUPDATE, 0, 0);
        });
        let scheduled = KillTimer(hwnd, 1);
        DestroyWindow(hwnd);
        UnregisterClassW(name.as_ptr(), instance);
        let mut message: MSG = zeroed();
        PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE);
        assert_ne!(scheduled, 0, "modal notifications must not be dropped");
    }
}

#[test]
fn first_typed_character_refreshes_native_search_and_selection() {
    let root = std::env::temp_dir().join(format!(
        "pasteleaf-search-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut model = Model::default();
    let matching = model.capture(Content::Text("zebra".into())).unwrap();
    let stale = model.capture(Content::Text("apple".into())).unwrap();
    // SAFETY: hidden test-owned parent uses the real WndProc and native EDIT.
    // Synchronous WM_CHAR -> EN_CHANGE exercises RefCell reentry without any
    // foreground activation, keyboard injection or clipboard access.
    unsafe {
        let instance = GetModuleHandleW(null());
        let name = wide(&format!("PasteLeaf.SearchTest.{}", GetCurrentThreadId()));
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = instance;
        wc.lpszClassName = name.as_ptr();
        assert_ne!(RegisterClassW(&wc), 0);
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            wide("Search regression").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            1200,
            368,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        assert!(!hwnd.is_null());
        let app = App::new(hwnd, Store::new(root.clone()).unwrap(), model, 1.0).unwrap();
        assert_eq!(app.selected, Some(stale));
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        SendMessageW(hwnd, WM_CHAR, b'z' as usize, 1);
        let mut app = APP.with(|cell| cell.borrow_mut().take().unwrap());
        let observed = (app.text(SEARCH), app.visible.clone(), app.selected);
        DestroyWindow(hwnd);
        app.release_resources();
        UnregisterClassW(name.as_ptr(), instance);
        // DestroyWindow posts WM_QUIT on this worker; consume it before reuse.
        let mut message: MSG = zeroed();
        PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE);
        assert_eq!(observed, ("z".into(), vec![matching], Some(matching)));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_hotkey_conflict_and_disk_failure_preserve_previous_binding() {
    let root = std::env::temp_dir().join(format!(
        "pasteleaf-native-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // SAFETY: this test creates a hidden window on its own thread and destroys
    // it before returning. No user window or system clipboard is touched.
    unsafe {
        let hwnd = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("PasteLeaf native test").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            1060,
            530,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        assert!(!hwnd.is_null());
        let store = Store::new(root.clone()).unwrap();
        let mut app = App::new(hwnd, store, Model::default(), 1.0).unwrap();
        app.set_text(HOTKEY, "Ctrl+Alt+Shift+F10");
        app.apply_hotkey().unwrap();
        assert_eq!(app.store.load().unwrap().hotkey, "Ctrl+Alt+Shift+F10");
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let reservation = std::thread::spawn(move || {
            let result = RegisterHotKey(null_mut(), 91, 7 | MOD_NOREPEAT, 0x7a);
            ready_tx.send(result).unwrap();
            done_rx.recv().unwrap();
            if result != 0 {
                UnregisterHotKey(null_mut(), 91);
            }
        });
        assert_ne!(ready_rx.recv().unwrap(), 0);
        app.set_text(HOTKEY, "Ctrl+Alt+Shift+F11");
        assert!(app.apply_hotkey().unwrap_err().contains("占用"));
        assert_eq!(app.model.hotkey, "Ctrl+Alt+Shift+F10");
        assert_eq!(app.store.load().unwrap().hotkey, "Ctrl+Alt+Shift+F10");
        done_tx.send(()).unwrap();
        reservation.join().unwrap();

        std::fs::create_dir(root.join("library.json.tmp")).unwrap();
        app.set_text(HOTKEY, "Ctrl+Alt+Shift+F8");
        assert!(app.apply_hotkey().is_err());
        assert_eq!(app.store.load().unwrap().hotkey, "Ctrl+Alt+Shift+F10");
        let result = std::thread::spawn(|| {
            let old = RegisterHotKey(null_mut(), 92, 7 | MOD_NOREPEAT, 0x79);
            let tentative = RegisterHotKey(null_mut(), 93, 7 | MOD_NOREPEAT, 0x77);
            if old != 0 {
                UnregisterHotKey(null_mut(), 92);
            }
            if tentative != 0 {
                UnregisterHotKey(null_mut(), 93);
            }
            (old, tentative)
        })
        .join()
        .unwrap();
        assert_eq!(result.0, 0, "old shortcut must remain registered");
        assert_ne!(result.1, 0, "failed tentative shortcut must be released");
        UnregisterHotKey(hwnd, app.hotkey_id.unwrap());
        DestroyWindow(hwnd);
        app.release_resources();
    }
    std::fs::remove_dir_all(root).unwrap();
}
