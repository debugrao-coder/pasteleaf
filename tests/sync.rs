use pasteleaf::{
    model::{Content, Model},
    store::Store,
    sync::{
        self,
        crypto::{self, Key},
        Exchange, Value,
    },
};
use std::{fs, path::PathBuf};
const PASSWORD: &str = "synthetic-test-password-2026";

#[test]
fn image_arrives_late_then_validates_and_pending_files_survive_cleanup() {
    let mut lab = Lab::new();
    let store = Store::new(lab.root.join("a")).unwrap();
    let content = store.write_image(8, 8, &[120; 256]).unwrap();
    let old = lab.a.clone();
    let id = lab.a.capture(content.clone()).unwrap();
    lab.a.assign(id, Some("收藏".into())).unwrap();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    let ex = sync::exchange(&lab.a.sync, &store.root, &lab.key, true).unwrap();
    sync::merge(&mut lab.a, ex).unwrap();
    let image = fs::read_dir(lab.root.join("云端/images"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let held = lab.root.join("late-image");
    fs::rename(&image, &held).unwrap();
    let ex = sync::exchange(&lab.b.sync, &lab.root.join("b"), &lab.key, false).unwrap();
    assert_eq!(ex.waiting, 1);
    sync::merge(&mut lab.b, ex).unwrap();
    assert_eq!(lab.b.items.len(), 1);
    fs::rename(held, image).unwrap();
    lab.cycle();
    assert!(lab.b.items.iter().any(|i| i.content == content));
    let mut candidate = lab.a.clone();
    candidate.items.retain(|i| i.id != id);
    store.cleanup(&candidate).unwrap();
    if let Content::Image { file, .. } = content {
        assert!(store.root.join("images").join(file).exists());
    }
}

#[test]
fn full_library_keeps_incoming_favorite_pending_and_retries_after_space_is_freed() {
    let mut lab = Lab::new();
    while lab.b.items.len() < 500 {
        lab.b
            .add_favorite_text("local capacity fixture", "收藏")
            .unwrap();
    }
    let old = lab.a.clone();
    lab.a
        .add_favorite_text("incoming retained favorite", "收藏")
        .unwrap();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    lab.cycle();
    assert_eq!(lab.b.items.len(), 500);
    assert!(!lab
        .b
        .items
        .iter()
        .any(|i| i.content == Content::Text("incoming retained favorite".into())));
    lab.b.items.pop();
    lab.cycle();
    assert!(lab
        .b
        .items
        .iter()
        .any(|i| i.content == Content::Text("incoming retained favorite".into())));
}

#[test]
fn reconnect_preserves_identity_and_interrupted_creation_can_retry() {
    let mut lab = Lab::new();
    let ids = lab.a.sync.item_ids.clone();
    sync::disconnect(&mut lab.a);
    let (rejoined, _) = sync::connect(
        &lab.a,
        &lab.root.join("a"),
        &lab.root.join("云端"),
        PASSWORD,
        false,
        false,
        false,
    )
    .unwrap();
    assert_eq!(rejoined.items.len(), 1);
    assert_eq!(rejoined.sync.item_ids, ids);
    let folder = lab.root.join("interrupted");
    let initial = Model::default();
    sync::connect(
        &initial,
        &lab.root.join("a"),
        &folder,
        PASSWORD,
        true,
        false,
        false,
    )
    .unwrap();
    let (retried, _) = sync::connect(
        &initial,
        &lab.root.join("a"),
        &folder,
        PASSWORD,
        true,
        false,
        false,
    )
    .unwrap();
    assert!(retried.sync.config.is_some());
}

#[test]
fn renamed_duplicate_operation_is_idempotent_and_tampering_blocks_import() {
    let mut lab = Lab::new();
    let path = fs::read_dir(lab.root.join("云端/operations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let duplicate = path.with_file_name("cloud-computer-conflict.plop");
    fs::copy(&path, &duplicate).unwrap();
    lab.cycle();
    assert_eq!(lab.b.items.len(), 1);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&duplicate).unwrap()).unwrap();
    let bytes = envelope["bytes"].as_array_mut().unwrap();
    bytes[25] = serde_json::json!((bytes[25].as_u64().unwrap() + 1) % 256);
    fs::write(duplicate, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let before = serde_json::to_vec(&lab.b).unwrap();
    assert!(sync::exchange(&lab.b.sync, &lab.root.join("b"), &lab.key, true).is_err());
    assert_eq!(before, serde_json::to_vec(&lab.b).unwrap());
}
struct Lab {
    root: PathBuf,
    a: Model,
    b: Model,
    key: Key,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("pasteleaf-sync-{}", sync::id()));
        Store::new(root.join("a")).unwrap();
        Store::new(root.join("b")).unwrap();
        let mut seed = Model::default();
        let id = seed.add_favorite_text("SYNTHETIC BODY", "收藏").unwrap();
        seed.set_tags(id, "项目，密码").unwrap();
        let (mut a, key) = sync::connect(
            &seed,
            &root.join("a"),
            &root.join("云端"),
            PASSWORD,
            true,
            false,
            true,
        )
        .unwrap();
        let ex = sync::exchange(&a.sync, &root.join("a"), &key, true).unwrap();
        sync::merge(&mut a, ex).unwrap();
        let (b, _) = sync::connect(
            &Model::default(),
            &root.join("b"),
            &root.join("云端"),
            PASSWORD,
            false,
            false,
            false,
        )
        .unwrap();
        Self { root, a, b, key }
    }
    fn cycle(&mut self) {
        for folder in ["a", "b", "a"] {
            let m = if folder == "a" {
                &mut self.a
            } else {
                &mut self.b
            };
            let ex = sync::exchange(&m.sync, &self.root.join(folder), &self.key, true).unwrap();
            sync::merge(m, ex).unwrap();
        }
    }
    fn edit(m: &mut Model, tag: &str) {
        let old = m.clone();
        let id = m.items[0].id;
        m.set_tags(id, tag).unwrap();
        sync::record_changes(&old, m, None).unwrap();
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn encrypted_join_restart_and_original_payload() {
    let mut lab = Lab::new();
    lab.cycle();
    assert_eq!(lab.b.items.len(), 1);
    assert_eq!(
        lab.b.items[0].content,
        Content::Text("SYNTHETIC BODY".into())
    );
    assert_eq!(lab.b.items[0].tags, vec!["项目", "密码"]);
    for entry in fs::read_dir(lab.root.join("云端/operations")).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("SYNTHETIC"));
        assert!(!text.contains(PASSWORD));
        assert!(!text.contains("项目"));
    }
    let store = Store::new(lab.root.join("a")).unwrap();
    store.save(&lab.a).unwrap();
    let restored = store.load().unwrap();
    assert_eq!(restored.sync.item_ids, lab.a.sync.item_ids);
    let saved = crypto::unprotect(&restored.sync.config.unwrap().remembered_key).unwrap();
    assert_eq!(*saved, *lab.key);
}

#[test]
fn forgetting_key_does_not_leave_a_recoverable_key_in_previous_snapshot() {
    let mut lab = Lab::new();
    let store = Store::new(lab.root.join("a")).unwrap();
    store.save(&lab.a).unwrap();
    lab.a.sync.config.as_mut().unwrap().remembered_key.clear();
    store.save(&lab.a).unwrap();
    let backup: Model =
        serde_json::from_slice(&fs::read(store.root.join("library.previous.json")).unwrap())
            .unwrap();
    assert!(backup.sync.config.unwrap().remembered_key.is_empty());
    assert!(store
        .load()
        .unwrap()
        .sync
        .config
        .unwrap()
        .remembered_key
        .is_empty());
    sync::disconnect(&mut lab.a);
    store.save(&lab.a).unwrap();
    assert!(store.load().unwrap().sync.config.is_none());
}
#[test]
fn concurrent_edits_keep_both_then_converge() {
    let mut lab = Lab::new();
    Lab::edit(&mut lab.a, "甲");
    Lab::edit(&mut lab.b, "乙");
    lab.cycle();
    let conflicts = lab.a.sync.conflicts();
    assert_eq!(conflicts.len(), 1);
    let entity = &conflicts[0];
    assert_eq!(lab.b.sync.heads(entity).len(), 2);
    let selected = lab.a.sync.heads(entity)[0].id.clone();
    sync::resolve(&mut lab.a, entity, &selected, true).unwrap();
    lab.cycle();
    assert!(lab.a.sync.conflicts().is_empty());
    assert!(lab.b.sync.conflicts().is_empty());
    assert_eq!(lab.a.items.len(), 2);
    assert_eq!(lab.b.items.len(), 2);
    let tags: std::collections::BTreeSet<_> =
        lab.b.items.iter().map(|i| i.tags[0].as_str()).collect();
    assert_eq!(tags, std::collections::BTreeSet::from(["甲", "乙"]));
}
#[test]
fn deletion_wins_over_old_replay_but_conflicts_with_concurrent_edit() {
    let mut lab = Lab::new();
    let old = lab.a.clone();
    let id = lab.a.items[0].id;
    lab.a.items.clear();
    sync::record_changes(&old, &mut lab.a, Some(id)).unwrap();
    lab.cycle();
    assert!(lab.b.items.is_empty());
    sync::merge(
        &mut lab.b,
        Exchange {
            operations: old
                .sync
                .operations
                .values()
                .filter(|o| o.value != Value::Retired)
                .cloned()
                .collect(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(lab.b.items.is_empty());
    let mut lab = Lab::new();
    let old = lab.a.clone();
    let id = lab.a.items[0].id;
    lab.a.items.clear();
    sync::record_changes(&old, &mut lab.a, Some(id)).unwrap();
    Lab::edit(&mut lab.b, "offline edit");
    lab.cycle();
    assert_eq!(lab.a.sync.conflicts().len(), 1);
    assert_eq!(lab.b.items.len(), 1);
}
#[test]
fn clear_and_eviction_stay_local_and_scope_switch_uploads_history() {
    let mut lab = Lab::new();
    let old = lab.a.clone();
    lab.a.capture(Content::Text("HISTORY ONLY".into())).unwrap();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    lab.cycle();
    assert_eq!(lab.b.items.len(), 1);
    sync::set_scope(&mut lab.a, true).unwrap();
    lab.cycle();
    assert_eq!(lab.b.items.len(), 2);
    let old = lab.a.clone();
    lab.a.clear_history();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    lab.cycle();
    lab.cycle();
    assert_eq!(lab.a.items.len(), 1);
    assert_eq!(lab.b.items.len(), 2);
    sync::set_scope(&mut lab.a, false).unwrap();
    lab.cycle();
    assert!(!lab.b.sync.all_records());
    assert_eq!(lab.b.items.len(), 2);
}
#[test]
fn unpin_and_category_rename_preserve_stable_ids() {
    let mut lab = Lab::new();
    let old = lab.a.clone();
    let uuid = lab.a.sync.category_ids["收藏"].clone();
    lab.a.rename_category("收藏", "工作").unwrap();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    lab.cycle();
    assert_eq!(lab.a.sync.category_ids["工作"], uuid);
    assert!(lab.b.items[0]
        .category
        .as_deref()
        .unwrap()
        .starts_with("工作"));
    let old = lab.a.clone();
    lab.a.assign(lab.a.items[0].id, None).unwrap();
    sync::record_changes(&old, &mut lab.a, None).unwrap();
    lab.cycle();
    assert!(lab.b.items[0].category.is_none());
    assert_eq!(lab.b.items[0].tags, vec!["项目", "密码"]);
}
#[test]
fn wrong_password_tampering_unknown_version_and_local_write_failure() {
    let lab = Lab::new();
    let descriptor = &lab.a.sync.config.as_ref().unwrap().descriptor;
    assert!(descriptor.unlock("wrong-password-123").is_err());
    let mut bad = descriptor.clone();
    bad.protocol = 99;
    assert!(bad.unlock(PASSWORD).is_err());
    let mut bytes = crypto::seal(&lab.key, &descriptor.library, "test", b"synthetic").unwrap();
    bytes[25] ^= 1;
    assert!(crypto::open(&lab.key, &descriptor.library, "test", &bytes).is_err());
    assert!(crypto::open(&lab.key, "wrong-library", "test", &bytes).is_err());
    let store = Store::new(lab.root.join("fail")).unwrap();
    fs::create_dir(store.root.join("library.json.tmp")).unwrap();
    let before = serde_json::to_vec(&lab.a).unwrap();
    assert!(store.save(&lab.a).is_err());
    assert_eq!(before, serde_json::to_vec(&lab.a).unwrap());
}
#[test]
fn outbox_survives_crash_and_republishing_is_idempotent() {
    let mut lab = Lab::new();
    Lab::edit(&mut lab.a, "restart tag");
    let store = Store::new(lab.root.join("a")).unwrap();
    store.save(&lab.a).unwrap();
    let first = sync::exchange(&lab.a.sync, &store.root, &lab.key, true).unwrap();
    assert!(!first.sent.is_empty());
    let mut restored = store.load().unwrap();
    let second = sync::exchange(&restored.sync, &store.root, &lab.key, true).unwrap();
    sync::merge(&mut restored, second).unwrap();
    assert!(restored.sync.outbox.is_empty());
    lab.a = restored;
    lab.cycle();
    assert_eq!(lab.b.items[0].tags, vec!["restart tag"]);
}
#[test]
fn unordered_and_missing_parent_operations_wait_then_converge() {
    let mut lab = Lab::new();
    let before = lab.a.clone();
    Lab::edit(&mut lab.a, "first");
    Lab::edit(&mut lab.a, "second");
    let mut ops: Vec<_> = lab
        .a
        .sync
        .operations
        .values()
        .filter(|o| !before.sync.operations.contains_key(&o.id))
        .cloned()
        .collect();
    let index = ops
        .iter()
        .position(|o| matches!(&o.value,Value::Record(r) if r.tags==vec!["second"]))
        .unwrap();
    let second = ops.remove(index);
    sync::merge(
        &mut lab.b,
        Exchange {
            operations: vec![second.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(lab.b.items[0].tags, vec!["second"]);
    sync::merge(
        &mut lab.b,
        Exchange {
            operations: ops,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(lab.b.items[0].tags, vec!["second"]);
    sync::merge(
        &mut lab.b,
        Exchange {
            operations: vec![second],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(lab.b.items.len(), 1);
}
#[test]
fn old_data_defaults_to_disabled_and_disconnect_keeps_content() {
    let old = r#"{"version":1,"items":[],"categories":["收藏"],"hotkey":"Ctrl+Q","paused":false,"next_id":1}"#;
    let model: Model = serde_json::from_str(old).unwrap();
    assert!(model.sync.config.is_none());
    let mut lab = Lab::new();
    let content = lab.a.items[0].content.clone();
    sync::disconnect(&mut lab.a);
    assert!(lab.a.sync.config.is_none());
    assert_eq!(lab.a.items[0].content, content);
    assert!(lab.root.join("云端/pasteleaf-v1.json").exists());
}
