//! Filesystem fixtures only: no clipboard, account, Keychain or real iCloud access.
use pasteleaf_core::Engine;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

const PASSWORD: &str = "synthetic-discovery-password";

fn response(engine: &mut Engine, command: Value) -> Value {
    serde_json::from_str(&engine.request_json(&command.to_string())).unwrap()
}
fn call(engine: &mut Engine, command: Value) -> Value {
    let reply = response(engine, command);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["data"].clone()
}
fn error(engine: &mut Engine, command: Value, expected: &str) {
    let reply = response(engine, command);
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], expected, "{reply}");
    // Recovery text must never interpolate the request password or local path.
    assert!(!reply.to_string().contains(PASSWORD));
}
fn open(root: &Path) -> Engine {
    let mut engine = Engine::open(root).unwrap();
    call(
        &mut engine,
        json!({"op":"settings","values":{"paused":false}}),
    );
    call(
        &mut engine,
        json!({"op":"capture","kind":"text","text":"retained synthetic history","source":"synthetic.discovery"}),
    );
    engine
}
fn local_state(engine: &mut Engine) -> Value {
    json!({"status":call(engine,json!({"op":"status"})),
        "history":call(engine,json!({"op":"list"}))})
}
fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn visit(root: &Path, current: &Path, result: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let key = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if metadata.is_dir() {
                result.insert(key, None);
                visit(root, &path, result);
            } else if metadata.file_type().is_symlink() {
                result.insert(
                    key,
                    Some(
                        fs::read_link(&path)
                            .unwrap()
                            .to_string_lossy()
                            .as_bytes()
                            .to_vec(),
                    ),
                );
            } else {
                result.insert(key, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
fn fixture_descriptor(root: &Path) -> Vec<u8> {
    let mut creator = Engine::open(root.join("creator")).unwrap();
    call(
        &mut creator,
        json!({"op":"sync_create","directory":root.join("source"),"password":PASSWORD,"remember":false}),
    );
    fs::read(root.join("source/PasteLeaf-v2/pasteleaf-v2.json")).unwrap()
}
fn inspect(engine: &mut Engine, directory: &Path, state: &str) -> Value {
    let result = call(engine, json!({"op":"sync_inspect","directory":directory}));
    assert_eq!(result["state"], state);
    assert!(result.get("descriptor").is_none());
    assert!(result.get("key").is_none());
    result
}
fn join(directory: &Path) -> Value {
    json!({"op":"sync_join","directory":directory,"password":PASSWORD,"remember":false})
}

#[test]
fn missing_join_and_inspection_never_create_directories_or_change_history() {
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = open(&tmp.path().join("local"));
    let before = local_state(&mut engine);
    let selected = tmp.path().join("empty-selected-folder");
    fs::create_dir(&selected).unwrap();
    inspect(&mut engine, &selected, "missing");
    error(&mut engine, join(&selected), "sync_library_missing");
    assert!(tree(&selected).is_empty());
    let missing = tmp.path().join("not-created/by-join");
    error(
        &mut engine,
        json!({"op":"sync_inspect","directory":missing}),
        "sync_path_missing",
    );
    error(&mut engine, join(&missing), "sync_path_missing");
    assert!(!tmp.path().join("not-created").exists());
    assert_eq!(local_state(&mut engine), before);
    drop(engine);
    let mut reopened = Engine::open(tmp.path().join("local")).unwrap();
    assert_eq!(local_state(&mut reopened), before);
}

#[test]
fn legacy_parent_and_empty_v2_child_are_classified_without_migration() {
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = open(&tmp.path().join("local"));
    let selected = tmp.path().join("legacy");
    fs::create_dir_all(selected.join("operations")).unwrap();
    fs::write(
        selected.join("pasteleaf-v1.json"),
        br#"{"protocol":1,"synthetic":true}"#,
    )
    .unwrap();
    fs::write(
        selected.join("operations/old-operation"),
        b"retained v1 bytes",
    )
    .unwrap();
    let before = local_state(&mut engine);
    let disk = tree(&selected);
    inspect(&mut engine, &selected, "legacy_v1");
    error(&mut engine, join(&selected), "sync_legacy_library");
    assert_eq!(tree(&selected), disk);
    let v2 = selected.join("PasteLeaf-v2");
    fs::create_dir(&v2).unwrap();
    let disk = tree(&selected);
    for directory in [&selected, &v2] {
        inspect(&mut engine, directory, "legacy_v1");
        error(&mut engine, join(directory), "sync_legacy_library");
        assert_eq!(tree(&selected), disk);
    }
    assert_eq!(local_state(&mut engine), before);
}

#[test]
fn parent_direct_and_renamed_v2_join_use_same_readonly_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = fixture_descriptor(tmp.path());
    let parent = tmp.path().join("parent");
    let v2 = parent.join("PasteLeaf-v2");
    fs::create_dir_all(&v2).unwrap();
    fs::write(
        parent.join("pasteleaf-v1.json"),
        b"legacy marker remains untouched",
    )
    .unwrap();
    fs::write(v2.join("pasteleaf-v2.json"), &descriptor).unwrap();
    let renamed = tmp.path().join("renamed-v2-library");
    fs::create_dir(&renamed).unwrap();
    fs::write(renamed.join("pasteleaf-v2.json"), &descriptor).unwrap();
    // Direct descriptor selection takes precedence, never guesses a nested library.
    fs::create_dir(renamed.join("PasteLeaf-v2")).unwrap();
    fs::write(
        renamed.join("PasteLeaf-v2/pasteleaf-v2.json"),
        b"invalid nested decoy",
    )
    .unwrap();
    for (index, directory) in [&parent, &v2, &renamed].into_iter().enumerate() {
        let mut engine = open(&tmp.path().join(format!("local-{index}")));
        let before = local_state(&mut engine);
        let disk = tree(directory);
        let inspected = inspect(&mut engine, directory, "v2");
        assert_eq!(local_state(&mut engine), before);
        let command = join(directory);
        call(&mut engine, command);
        assert_eq!(
            call(&mut engine, json!({"op":"status"}))["sync_directory"],
            inspected["directory"]
        );
        assert_eq!(
            tree(directory),
            disk,
            "join must not even create missing operations/attachments/receipts folders"
        );
        assert_eq!(call(&mut engine, json!({"op":"list"})), before["history"]);
        call(&mut engine, json!({"op":"sync_now"}));
        call(&mut engine, json!({"op":"sync_now"}));
        assert!(engine.sync_directory().unwrap().join("operations").is_dir());
        assert!(engine.sync_directory().unwrap().join("receipts").is_dir());
    }
}

#[test]
fn create_cannot_nest_a_library_inside_a_direct_descriptor_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = fixture_descriptor(tmp.path());
    let mut engine = open(&tmp.path().join("local"));
    let selected = tmp.path().join("renamed-existing-library");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("pasteleaf-v2.json"), descriptor).unwrap();
    let before = local_state(&mut engine);
    let disk = tree(&selected);
    error(
        &mut engine,
        json!({"op":"sync_create","directory":selected,"password":PASSWORD}),
        "sync_exists",
    );
    assert_eq!(tree(&selected), disk);
    assert_eq!(local_state(&mut engine), before);
}

#[test]
fn delayed_descriptor_placeholder_and_partial_json_are_retryable_without_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = fixture_descriptor(tmp.path());
    let mut engine = open(&tmp.path().join("local"));
    let parent = tmp.path().join("delayed");
    let v2 = parent.join("PasteLeaf-v2");
    fs::create_dir_all(&v2).unwrap();
    fs::write(
        parent.join("pasteleaf-v1.json"),
        b"legacy does not override pending v2",
    )
    .unwrap();
    fs::write(
        v2.join(".pasteleaf-v2.json.icloud"),
        b"synthetic placeholder",
    )
    .unwrap();
    let before = local_state(&mut engine);
    for directory in [&parent, &v2] {
        let disk = tree(&parent);
        inspect(&mut engine, directory, "pending_download");
        error(&mut engine, join(directory), "pending_hydration");
        assert_eq!(tree(&parent), disk);
    }
    fs::remove_file(v2.join(".pasteleaf-v2.json.icloud")).unwrap();
    for partial in [b"".as_slice(), br#"{"protocol":2,"library":"#.as_slice()] {
        fs::write(v2.join("pasteleaf-v2.json"), partial).unwrap();
        let disk = tree(&parent);
        inspect(&mut engine, &parent, "pending_download");
        error(&mut engine, join(&parent), "pending_hydration");
        assert_eq!(tree(&parent), disk);
    }
    assert_eq!(local_state(&mut engine), before);
    fs::write(v2.join("pasteleaf-v2.json"), descriptor).unwrap();
    inspect(&mut engine, &parent, "v2");
    let disk = tree(&parent);
    let command = join(&parent);
    call(&mut engine, command);
    assert_eq!(tree(&parent), disk);
}

#[test]
fn unknown_versions_malformed_descriptors_and_password_failure_are_nonmutating() {
    let tmp = tempfile::tempdir().unwrap();
    let original = fixture_descriptor(tmp.path());
    let mut engine = open(&tmp.path().join("local"));
    let v2 = tmp.path().join("PasteLeaf-v2");
    fs::create_dir(&v2).unwrap();
    let descriptor = v2.join("pasteleaf-v2.json");
    let before = local_state(&mut engine);
    let valid: Value = serde_json::from_slice(&original).unwrap();
    let mut version = valid.clone();
    version["protocol"] = json!(3);
    let mut parameters = valid.clone();
    parameters["memory_kib"] = json!(u32::MAX);
    let mut identity = valid.clone();
    identity["library"] = json!("invalid identity");
    for (raw, expected) in [
        (version.to_string().into_bytes(), "protocol_version"),
        (parameters.to_string().into_bytes(), "protocol_version"),
        (identity.to_string().into_bytes(), "sync_invalid_library"),
        (b"not-json".to_vec(), "sync_invalid_library"),
        (vec![b'x'; 8193], "sync_invalid_library"),
    ] {
        fs::write(&descriptor, raw).unwrap();
        let disk = tree(&v2);
        error(
            &mut engine,
            json!({"op":"sync_inspect","directory":v2}),
            expected,
        );
        error(&mut engine, join(&v2), expected);
        assert_eq!(tree(&v2), disk);
        assert_eq!(local_state(&mut engine), before);
    }
    fs::write(descriptor, original).unwrap();
    let disk = tree(&v2);
    inspect(&mut engine, &v2, "v2");
    error(
        &mut engine,
        json!({"op":"sync_join","directory":v2,"password":"wrong-synthetic-password"}),
        "authentication",
    );
    assert_eq!(tree(&v2), disk);
    assert_eq!(local_state(&mut engine), before);
}

#[test]
fn inspection_and_failed_rejoin_never_replace_an_existing_connection() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = fixture_descriptor(tmp.path());
    let mut engine = open(&tmp.path().join("local"));
    let command = join(&tmp.path().join("source"));
    call(&mut engine, command);
    let before = local_state(&mut engine);
    let selected = tmp.path().join("other");
    fs::create_dir(&selected).unwrap();
    let disk = tree(&selected);
    inspect(&mut engine, &selected, "missing");
    error(&mut engine, join(&selected), "already_connected");
    assert_eq!(tree(&selected), disk);
    fs::write(selected.join("pasteleaf-v2.json"), descriptor).unwrap();
    inspect(&mut engine, &selected, "v2");
    error(
        &mut engine,
        json!({"op":"sync_join","directory":selected,"password":"wrong-synthetic-password"}),
        "already_connected",
    );
    assert_eq!(local_state(&mut engine), before);
}

#[test]
fn invalid_paths_and_descriptor_links_are_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = open(&tmp.path().join("local"));
    let before = local_state(&mut engine);
    let file = tmp.path().join("ordinary-file");
    fs::write(&file, b"synthetic file").unwrap();
    for directory in [file.clone(), file.join("child"), "relative-folder".into()] {
        error(
            &mut engine,
            json!({"op":"sync_inspect","directory":directory}),
            "sync_path_invalid",
        );
        error(&mut engine, join(&directory), "sync_path_invalid");
    }
    #[cfg(unix)]
    {
        let link = tmp.path().join("linked-folder");
        std::os::unix::fs::symlink(tmp.path(), &link).unwrap();
        error(&mut engine, join(&link), "sync_path_invalid");
        let v2 = tmp.path().join("PasteLeaf-v2");
        fs::create_dir(&v2).unwrap();
        std::os::unix::fs::symlink(&file, v2.join("pasteleaf-v2.json")).unwrap();
        error(
            &mut engine,
            json!({"op":"sync_inspect","directory":v2}),
            "sync_invalid_library",
        );
        error(&mut engine, join(&v2), "sync_invalid_library");
        assert_eq!(fs::read(file).unwrap(), b"synthetic file");
    }
    assert_eq!(local_state(&mut engine), before);
}

#[cfg(unix)]
#[test]
fn unreadable_descriptor_is_access_denied_not_missing() {
    use std::os::unix::fs::PermissionsExt;
    // Root can bypass mode bits; the portable classifier has separate unit coverage.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut engine = open(&tmp.path().join("local"));
    let v2 = tmp.path().join("PasteLeaf-v2");
    fs::create_dir(&v2).unwrap();
    let descriptor = v2.join("pasteleaf-v2.json");
    fs::write(&descriptor, b"synthetic inaccessible descriptor").unwrap();
    let before = local_state(&mut engine);
    fs::set_permissions(&descriptor, fs::Permissions::from_mode(0o0)).unwrap();
    let inspected = response(&mut engine, json!({"op":"sync_inspect","directory":v2}));
    let command = join(&v2);
    let joined = response(&mut engine, command);
    fs::set_permissions(&descriptor, fs::Permissions::from_mode(0o600)).unwrap();
    for reply in [inspected, joined] {
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"]["code"], "sync_access_denied");
        assert!(!reply.to_string().contains(tmp.path().to_str().unwrap()));
    }
    assert_eq!(local_state(&mut engine), before);
    assert_eq!(
        fs::read(descriptor).unwrap(),
        b"synthetic inaccessible descriptor"
    );
}
