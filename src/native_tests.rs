use super::*;

#[test]
fn sync_settings_native_controls_themes_scales_and_cancel() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let before = serde_json::to_vec(&app.model).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        app.model.theme = theme;
        app.refresh_theme();
        for scale in [1.0, 1.5, 2.0] {
            app.reset_fonts(scale);
            for width in [488, 1200] {
                app.width = width;
                app.open_dialog(Dialog::Sync);
                app.set_text(SYNC_PATH, "C:\\合成测试\\iCloud Drive\\PasteLeaf Sync");
                render_tag_snapshot(
                    app,
                    &format!("sync-{theme:?}-{}-{width}", (scale * 100.) as i32),
                );
                assert!(app
                    .hits
                    .iter()
                    .any(|(_, hit)| *hit == Hit::Sync(SyncAction::Create)));
                for control in [SYNC_PATH, SYNC_PASSWORD, SYNC_CONFIRM] {
                    let mut bounds: RECT = unsafe { zeroed() };
                    // SAFETY: read rectangles and styles of isolated test-owned EDITs.
                    unsafe {
                        GetWindowRect(app.control(control), &mut bounds);
                        MapWindowPoints(null_mut(), app.hwnd, (&mut bounds as *mut RECT).cast(), 2);
                        if control != SYNC_PATH {
                            assert_ne!(
                                GetWindowLongW(app.control(control), GWL_STYLE) & ES_PASSWORD,
                                0
                            );
                        }
                    }
                    assert!(bounds.left >= 0 && bounds.right <= app.px(width));
                    assert!(bounds.top >= 0 && bounds.bottom < app.px(214));
                }
                for (area, _) in &app.hits {
                    assert!(area.x >= 0 && area.x + area.w <= width);
                }
                app.set_text(SYNC_PASSWORD, "synthetic-draft-only");
                app.keyboard(VK_ESCAPE).unwrap();
                assert!(app.text(SYNC_PASSWORD).is_empty());
                assert!(app.model.sync.config.is_none());
            }
        }
    }
    app.model.theme = serde_json::from_slice::<Model>(&before).unwrap().theme;
    assert_eq!(before, serde_json::to_vec(&app.model).unwrap());
}

#[test]
fn sync_background_result_rebases_on_new_local_edits_and_persists_ack() {
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let remote = std::env::temp_dir().join(format!("pasteleaf-sync-ui-{}", pasteleaf::sync::id()));
    let (model, key) = pasteleaf::sync::connect(
        &app.model,
        &app.store.root,
        &remote,
        "synthetic UI sync password",
        true,
        false,
        true,
    )
    .unwrap();
    app.store.save(&model).unwrap();
    app.model = model;
    app.sync_runtime = SyncRuntime::new(&app.model);
    app.sync_runtime.key = Some(key);
    app.sync_tick();
    assert!(app.sync_runtime.busy());
    let mut changed = app.model.clone();
    changed
        .add_favorite_text("created while sync was running", "收藏")
        .unwrap();
    app.commit(changed).unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while app.sync_runtime.last.is_none() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.sync_tick();
    }
    assert!(
        app.sync_runtime.last.is_some(),
        "{}",
        app.sync_runtime.status
    );
    assert!(app
        .store
        .load()
        .unwrap()
        .items
        .iter()
        .any(|i| i.content == Content::Text("created while sync was running".into())));
    assert!(
        !app.model.sync.outbox.is_empty(),
        "new edits must remain queued after acknowledging the older worker snapshot"
    );
    app.sync_action(SyncAction::Now).unwrap();
    app.sync_tick();
    while app.sync_runtime.busy() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.sync_tick();
    }
    assert!(app.store.load().unwrap().sync.outbox.is_empty());
    std::fs::remove_dir_all(remote).unwrap();
}

#[test]
fn sync_connected_settings_and_conflict_versions_are_rendered_and_actionable() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let remote = std::env::temp_dir().join(format!(
        "pasteleaf-sync-conflict-ui-{}",
        pasteleaf::sync::id()
    ));
    let (model, key) = pasteleaf::sync::connect(
        &app.model,
        &app.store.root,
        &remote,
        "synthetic conflict password",
        true,
        false,
        true,
    )
    .unwrap();
    app.model = model;
    app.sync_runtime = SyncRuntime::new(&app.model);
    app.sync_runtime.key = Some(key);
    let id = app
        .model
        .add_favorite_text("synthetic conflict text", "收藏")
        .unwrap();
    let old = app.model.clone();
    pasteleaf::sync::record_changes(&old, &mut app.model, None).unwrap();
    let mut other = app.model.clone();
    other.sync.device = pasteleaf::sync::id();
    let old = other.clone();
    other.set_tags(id, "remote tag").unwrap();
    pasteleaf::sync::record_changes(&old, &mut other, None).unwrap();
    let old = app.model.clone();
    app.model.set_tags(id, "local tag").unwrap();
    pasteleaf::sync::record_changes(&old, &mut app.model, None).unwrap();
    pasteleaf::sync::merge(
        &mut app.model,
        pasteleaf::sync::Exchange {
            operations: other.sync.operations.values().cloned().collect(),
            ..Default::default()
        },
    )
    .unwrap();
    let entity = app.model.sync.conflicts()[0].clone();
    for theme in [Theme::Dark, Theme::Light] {
        app.model.theme = theme;
        app.refresh_theme();
        for scale in [1.0, 1.5, 2.0] {
            app.reset_fonts(scale);
            for width in [488, 1200] {
                app.width = width;
                app.open_dialog(Dialog::Sync);
                render_tag_snapshot(
                    app,
                    &format!("sync-connected-{theme:?}-{}-{width}", (scale * 100.) as i32),
                );
                assert!(app
                    .hits
                    .iter()
                    .any(|(_, h)| *h == Hit::Sync(SyncAction::Disconnect)));
                app.open_dialog(Dialog::SyncConflict(entity.clone(), 0));
                render_tag_snapshot(
                    app,
                    &format!("sync-conflict-{theme:?}-{}-{width}", (scale * 100.) as i32),
                );
                assert!(app.text(PREVIEW).contains("synthetic conflict text"));
                let first = app.text(PREVIEW);
                app.sync_action(SyncAction::NextVersion).unwrap();
                assert_ne!(first, app.text(PREVIEW));
                let mut bounds: RECT = unsafe { zeroed() };
                unsafe {
                    GetWindowRect(app.control(PREVIEW), &mut bounds);
                    MapWindowPoints(null_mut(), app.hwnd, (&mut bounds as *mut RECT).cast(), 2);
                }
                assert!(bounds.left >= 0 && bounds.right <= app.px(width));
            }
        }
    }
    app.sync_action(SyncAction::KeepBoth).unwrap();
    assert!(app.model.sync.conflicts().is_empty());
    assert!(app.model.items.iter().any(|i| i.tags == vec!["local tag"]));
    assert!(app.model.items.iter().any(|i| i.tags == vec!["remote tag"]));
    std::fs::remove_dir_all(remote).unwrap();
}

fn render_tag_snapshot(app: &mut App, name: &str) {
    let width = app.px(app.width);
    let height = app.px(PANEL_H);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    // SAFETY: isolated offscreen bitmap; no desktop pixels or clipboard data are read.
    unsafe {
        let screen = GetDC(app.hwnd);
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        assert!(!dc.is_null() && !bitmap.is_null());
        let previous = SelectObject(dc, bitmap);
        app.draw_content(dc);
        SelectObject(dc, previous);
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        assert_eq!(
            GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS
            ),
            height
        );
        DeleteObject(bitmap);
        DeleteDC(dc);
        ReleaseDC(app.hwnd, screen);
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    assert!(pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[0] > 230 && p[1] > 230 && p[2] > 230));
    if let Some(output) = std::env::var_os("PASTELEAF_RENDER_OUTPUT") {
        let path = PathBuf::from(output);
        std::fs::create_dir_all(&path).unwrap();
        image::save_buffer(
            path.join(format!("{name}.png")),
            &pixels,
            width as u32,
            height as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
fn tags_render_themes_scales_and_overflow_without_exposing_payload_in_tooltips() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    app.model.items.clear();
    app.model
        .add_favorite_text("https://example.com", "收藏")
        .unwrap();
    app.model
        .add_favorite_text("const count = 3;", "收藏")
        .unwrap();
    let picture = app.store.write_image(4, 4, &[140; 64]).unwrap();
    let id = app.model.capture(picture).unwrap();
    app.model.assign(id, Some("收藏".into())).unwrap();
    app.model
        .set_tags(
            id,
            "这是一个很长的中文标签用于验证边界,设计,图片,工作,参考,归档,素材,示例",
        )
        .unwrap();
    let text_id = app
        .model
        .add_favorite_text("Synthetic preview\r\nSample content only", "收藏")
        .unwrap();
    app.model.set_tags(text_id, "workbuddy2api,密码").unwrap();
    app.select_board(1);
    app.status.clear();
    for theme in [Theme::Dark, Theme::Light] {
        app.model.theme = theme;
        app.refresh_theme();
        for scale in [1.0, 1.5, 2.0] {
            app.reset_fonts(scale);
            for width in [488, 1200] {
                app.width = width;
                app.tip_point = None;
                let name = format!("tags-{theme:?}-{}-{width}", (scale * 100.) as i32);
                render_tag_snapshot(app, &name);
                assert!(app.tips.iter().any(|(_, s)| s == "文本"));
                assert!(app.tips.iter().any(|(_, s)| s == "workbuddy2api\n密码"));
                assert!(app.tips.iter().all(|(a, text)| a.x >= 0
                    && a.x + a.w <= width
                    && !text.contains("Synthetic preview")));
                if width == 1200 {
                    for kind in ["图片", "代码", "链接"] {
                        assert!(app.tips.iter().any(|(_, s)| s == kind));
                    }
                    app.tip_point = Some((316, CARD_TOP + 53));
                    render_tag_snapshot(app, &format!("{name}-tooltip"));
                }
            }
        }
    }
}

#[test]
fn tags_search_stays_in_selected_collection_and_editor_scales() {
    let _graphics = ManualFavoriteGraphics(render::start_graphics().unwrap());
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let id = app.model.add_favorite_text("synthetic", "工作").unwrap();
    app.model.set_tags(id, "查找").unwrap();
    app.select_board(1);
    app.set_text(SEARCH, "查找");
    app.refresh();
    assert!(app.visible.is_empty());
    app.select_board(0);
    app.set_text(SEARCH, "查找");
    app.refresh();
    assert_eq!(app.visible, vec![id]);
    for scale in [1.0, 1.5, 2.0] {
        app.reset_fonts(scale);
        for width in [488, 744, 1200] {
            app.width = width;
            for dialog in [Dialog::NewItem("收藏".into()), Dialog::EditTags(id)] {
                app.open_dialog(dialog.clone());
                let mut bounds: RECT = unsafe { zeroed() };
                // SAFETY: bounds of this fixture's own EDIT only.
                unsafe {
                    GetWindowRect(app.control(ITEM_TAGS), &mut bounds);
                    MapWindowPoints(null_mut(), app.hwnd, (&mut bounds as *mut RECT).cast(), 2);
                }
                assert!(bounds.left >= 0 && bounds.right <= app.px(width));
                assert!(bounds.top >= 0 && bounds.bottom < app.px(282));
                app.keyboard(VK_ESCAPE).unwrap();
                assert_eq!(app.dialog, Dialog::None);
                assert_eq!(
                    unsafe { GetWindowLongW(app.control(ITEM_TAGS), GWL_STYLE) } as u32
                        & WS_VISIBLE,
                    0
                );
            }
        }
    }
}

#[test]
fn tags_new_item_normalizes_and_rejects_limits_without_losing_draft() {
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    app.select_board(1);
    app.command(218).unwrap();
    assert!(!app.control(132).is_null(), "tag input must exist");
    app.set_text(131, "synthetic body");
    app.set_text(132, &"长".repeat(25));
    assert!(app.accept_dialog().is_err());
    assert_eq!(app.text(131), "synthetic body");
    app.set_text(132, "a,b,c,d,e,f,g,h,i");
    assert!(app.accept_dialog().is_err());
    app.set_text(
        132,
        " workbuddy2api ， 密码,workbuddy2api, WORKBUDDY2API, , ",
    );
    app.accept_dialog().unwrap();
    let value = serde_json::to_value(app.store.load().unwrap()).unwrap();
    assert_eq!(
        value["items"][0]["tags"],
        serde_json::json!(["workbuddy2api", "密码"])
    );
    assert_eq!(value["items"][0]["content"]["Text"], "synthetic body");
}

#[test]
fn tags_edit_cancel_and_failed_save_preserve_previous_data() {
    let mut fixture = ManualFavoriteFixture::new();
    let app = &mut fixture.app;
    let id = app
        .model
        .add_favorite_text("synthetic body", "收藏")
        .unwrap();
    app.store.save(&app.model).unwrap();
    app.selected = Some(id);
    app.command(219).unwrap();
    assert_ne!(
        app.dialog,
        Dialog::None,
        "edit-tags command must open editor"
    );
    app.set_text(132, "cancelled");
    let before = serde_json::to_vec(&app.model).unwrap();
    app.close_dialog();
    assert_eq!(serde_json::to_vec(&app.model).unwrap(), before);
    app.command(219).unwrap();
    assert_eq!(app.text(132), "");
    app.set_text(132, "workbuddy2api，密码");
    let disk = std::fs::read(app.store.root.join("library.json")).unwrap();
    let obstruction = app.store.root.join("library.json.tmp");
    std::fs::create_dir(&obstruction).unwrap();
    assert!(app.accept_dialog().is_err());
    assert_eq!(app.text(132), "workbuddy2api，密码");
    assert_eq!(serde_json::to_vec(&app.model).unwrap(), before);
    assert_eq!(
        std::fs::read(app.store.root.join("library.json")).unwrap(),
        disk
    );
    std::fs::remove_dir(obstruction).unwrap();
    app.accept_dialog().unwrap();
    assert_eq!(app.dialog, Dialog::None);
    assert_eq!(
        app.store.load().unwrap().visible("密码", Some("收藏")),
        vec![id]
    );
    app.command(219).unwrap();
    assert_eq!(app.text(132), "workbuddy2api, 密码");
    app.set_text(132, "");
    app.accept_dialog().unwrap();
    assert!(app.store.load().unwrap().visible("密码", None).is_empty());
}
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
        vec![201, 202, 203, 219, 1000, 1001, 205, 204]
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
