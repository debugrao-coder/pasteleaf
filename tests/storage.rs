use pasteleaf::{
    model::{Content, Model},
    store::Store,
};
use std::{fs, path::PathBuf};

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pasteleaf-test-{name}-{}", std::process::id()))
}

#[test]
fn save_load_and_corrupt_store_preservation() {
    let dir = root("roundtrip");
    let store = Store::new(dir.clone()).unwrap();
    let mut model = Model::default();
    model
        .capture(Content::Text("你好 persistent".into()))
        .unwrap();
    model.hotkey = "Ctrl+Alt+F8".into();
    model.theme = pasteleaf::theme::Theme::System;
    store.save(&model).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.items[0].content, model.items[0].content);
    assert_eq!(loaded.hotkey, "Ctrl+Alt+F8");
    assert_eq!(loaded.theme, pasteleaf::theme::Theme::System);
    fs::write(dir.join("library.json"), b"broken").unwrap();
    assert!(store.load().is_err());
    assert_eq!(fs::read(dir.join("library.json")).unwrap(), b"broken");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn repeated_reopen_retains_every_saved_record() {
    let dir = root("restart");
    for index in 0..30 {
        let store = Store::new(dir.clone()).unwrap();
        let mut model = store.load().unwrap();
        assert_eq!(model.items.len(), index);
        model
            .capture(Content::Text(format!("restart-record-{index}")))
            .unwrap();
        store.save(&model).unwrap();
    }
    let model = Store::new(dir.clone()).unwrap().load().unwrap();
    assert_eq!(model.items.len(), 30);
    for index in 0..30 {
        assert!(model
            .items
            .iter()
            .any(|item| item.content == Content::Text(format!("restart-record-{index}"))));
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn png_roundtrip_and_unreferenced_cleanup() {
    let dir = root("images");
    let store = Store::new(dir.clone()).unwrap();
    let pixels = vec![210u8; 4 * 8 * 6];
    let content = store.write_image(8, 6, &pixels).unwrap();
    let file = match &content {
        Content::Image { file, .. } => file.clone(),
        _ => unreachable!(),
    };
    let mut model = Model::default();
    model.capture(content).unwrap();
    store.save(&model).unwrap();
    assert_eq!(store.read_image(&file).unwrap().into_raw(), pixels);
    store.cleanup(&model).unwrap();
    assert!(dir.join("images").join(&file).exists());
    model.clear_history();
    store.save(&model).unwrap();
    store.cleanup(&model).unwrap();
    // Previous snapshot must remain usable, including its image.
    assert!(dir.join("images").join(&file).exists());
    store.save(&model).unwrap();
    store.cleanup(&model).unwrap();
    assert!(!dir.join("images").join(file).exists());
    assert!(store.read_image("../../secret.png").is_err());
    assert!(store.write_image(8, 6, &[0]).is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn extreme_dimensions_are_rejected_without_panicking() {
    let dir = root("dimensions");
    let store = Store::new(dir.clone()).unwrap();
    assert!(store.write_image(u32::MAX, u32::MAX, &[]).is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn escaped_json_limit_preserves_readable_previous_store() {
    let dir = root("escaped-limit");
    let store = Store::new(dir.clone()).unwrap();
    let mut model = Model::default();
    model.capture(Content::Text("retained".into())).unwrap();
    store.save(&model).unwrap();
    for i in 0..72 {
        model
            .capture(Content::Text(format!("{i}{}", "\\".repeat(510 * 1024))))
            .unwrap();
    }
    assert!(store.save(&model).is_err());
    assert_eq!(
        store.load().unwrap().items[0].content,
        Content::Text("retained".into())
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn previous_snapshot_retains_history_before_replacement() {
    let dir = root("previous-snapshot");
    let store = Store::new(dir.clone()).unwrap();
    let mut model = Model::default();
    model
        .capture(Content::Text("saved-before-restart".into()))
        .unwrap();
    store.save(&model).unwrap();
    model
        .capture(Content::Text("saved-after-restart".into()))
        .unwrap();
    store.save(&model).unwrap();
    let backup: Model =
        serde_json::from_slice(&fs::read(dir.join("library.previous.json")).unwrap()).unwrap();
    assert_eq!(backup.items.len(), 1);
    assert_eq!(store.load().unwrap().items.len(), 2);
    assert_eq!(
        backup.items[0].content,
        Content::Text("saved-before-restart".into())
    );
    fs::remove_dir_all(dir).unwrap();
}
