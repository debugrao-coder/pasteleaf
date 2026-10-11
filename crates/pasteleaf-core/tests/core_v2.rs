use pasteleaf_core::{model::DEFAULT_CATEGORY, Engine};
use serde_json::{json, Value};
use std::{fs, path::Path};
use tempfile::TempDir;
fn call(e: &mut Engine, v: Value) -> Value {
    let response: Value = serde_json::from_str(&e.request_json(&v.to_string())).unwrap();
    assert_eq!(response["ok"], true, "{}", response);
    response["data"].clone()
}
fn fail(e: &mut Engine, v: Value) -> String {
    let response: Value = serde_json::from_str(&e.request_json(&v.to_string())).unwrap();
    assert_eq!(response["ok"], false, "{}", response);
    response["error"]["code"].as_str().unwrap().into()
}
fn open(root: &Path) -> Engine {
    let mut e = Engine::open(root).unwrap();
    call(
        &mut e,
        json!({"op":"settings","values":{"paused":false,"onboarding_complete":true}}),
    );
    e
}
fn capture(e: &mut Engine, text: &str) -> Value {
    call(
        e,
        json!({"op":"capture","kind":"text","text":text,"source":"synthetic.test"}),
    )
}
fn list(e: &mut Engine) -> Value {
    call(e, json!({"op":"list","limit":500}))
}
fn pair() -> (TempDir, Engine, Engine) {
    let tmp = tempfile::tempdir().unwrap();
    let mut a = open(&tmp.path().join("a"));
    let mut b = open(&tmp.path().join("b"));
    call(
        &mut a,
        json!({"op":"sync_create","directory":tmp.path().join("cloud"),"password":"synthetic-test-password","remember":false}),
    );
    call(&mut a, json!({"op":"sync_now"}));
    call(
        &mut b,
        json!({"op":"sync_join","directory":tmp.path().join("cloud"),"password":"synthetic-test-password","remember":false}),
    );
    (tmp, a, b)
}
fn exchange(a: &mut Engine, b: &mut Engine) {
    call(a, json!({"op":"sync_now"}));
    call(b, json!({"op":"sync_now"}));
    call(a, json!({"op":"sync_now"}));
}
#[test]
fn defaults_require_consent_and_validate_settings() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"text","text":"secret"})
        ),
        "capture_paused"
    );
    assert_eq!(list(&mut e)["total"], 0);
    assert_eq!(
        call(&mut e, json!({"op":"status"}))["sync_state"],
        "disconnected"
    );
    assert_eq!(
        fail(&mut e, json!({"op":"settings","values":{"paused":"false"}})),
        "invalid_request"
    );
    call(
        &mut e,
        json!({"op":"settings","values":{"paused":false,"excluded_apps":["com.example.private"]}}),
    );
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"text","text":"ignored","source":"com.example.private"})
        ),
        "source_excluded"
    );
    assert_eq!(list(&mut e)["total"], 0);
}
#[test]
fn history_is_permanent_deduplicated_indexed_and_paged() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(t.path());
    for i in 0..625 {
        capture(&mut e, &format!("剪贴板记录 内容 项目 {i:04}"));
    }
    assert_eq!(list(&mut e)["total"], 625);
    let duplicate = capture(&mut e, "剪贴板记录 内容 项目 0001");
    assert_eq!(list(&mut e)["total"], 625);
    for query in ["剪", "项目", "贴板记", "内容 项目 0001"] {
        let r = call(
            &mut e,
            json!({"op":"list","query":query,"limit":20,"offset":0}),
        );
        assert!(r["total"].as_i64().unwrap() > 0, "{query}");
    }
    let r = call(&mut e, json!({"op":"list","query":"0001"}));
    assert_eq!(r["total"], 1);
    assert_eq!(r["items"][0]["id"], duplicate["id"]);
    assert_eq!(
        call(&mut e, json!({"op":"list","offset":600,"limit":50}))["items"]
            .as_array()
            .unwrap()
            .len(),
        25
    );
    drop(e);
    let mut e = Engine::open(t.path()).unwrap();
    assert_eq!(list(&mut e)["total"], 625);
}
#[test]
fn favorites_tags_categories_and_local_clear() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(t.path());
    let a = capture(&mut e, "ordinary");
    let b = capture(&mut e, "keep favorite");
    let cat = call(
        &mut e,
        json!({"op":"category_create","name":"设计","color":"#347865"}),
    );
    call(
        &mut e,
        json!({"op":"favorite","id":b["id"],"category":cat["id"]}),
    );
    call(
        &mut e,
        json!({"op":"tags","id":b["id"],"tags":["工作","Work","work"]}),
    );
    let r = call(&mut e, json!({"op":"list","query":"工作"}));
    assert_eq!(r["total"], 1);
    assert_eq!(r["items"][0]["tags"].as_array().unwrap().len(), 2);
    let before = call(&mut e, json!({"op":"status"}))["pending"].clone();
    call(&mut e, json!({"op":"clear_local"}));
    assert_eq!(call(&mut e, json!({"op":"status"}))["pending"], before);
    assert_eq!(list(&mut e)["total"], 1);
    assert_eq!(fail(&mut e, json!({"op":"get","id":a["id"]})), "not_found");
    call(&mut e, json!({"op":"category_delete","id":cat["id"]}));
    assert_eq!(list(&mut e)["items"][0]["category"], Value::Null);
    assert_eq!(list(&mut e)["total"], 1);
}
#[test]
fn rich_text_is_lossless_and_passive() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(t.path());
    use base64::Engine as _;
    let rtf = base64::engine::general_purpose::STANDARD.encode(b"{\\rtf1\\ansi \'e9\xff}");
    let r = call(
        &mut e,
        json!({"op":"capture","kind":"rich_text","text":"é","html":"<b>é</b><img src='https://invalid.test/no-fetch'>","rtf":rtf,"source":"fixture"}),
    );
    let get = call(&mut e, json!({"op":"get","id":r["id"]}));
    assert_eq!(get["rtf"], rtf);
    assert!(get["html"].as_str().unwrap().contains("no-fetch"));
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"rich_text","text":"x","rtf":"not base64"})
        ),
        "invalid_request"
    );
}
#[test]
fn ordinary_files_are_copied_and_checked_on_get() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(&t.path().join("store"));
    let source = t.path().join("资料.txt");
    fs::write(&source, b"attachment fixture").unwrap();
    let r = call(
        &mut e,
        json!({"op":"capture","kind":"files","paths":[source],"text":"/private/source/path","source":"fixture"}),
    );
    assert_eq!(r["text"], "资料.txt");
    let local = std::path::PathBuf::from(r["files"][0]["path"].as_str().unwrap());
    fs::remove_file(&source).unwrap();
    assert_eq!(fs::read(&local).unwrap(), b"attachment fixture");
    call(&mut e, json!({"op":"get","id":r["id"]}));
    fs::write(local, b"corrupt attachment").unwrap();
    assert_eq!(
        fail(&mut e, json!({"op":"get","id":r["id"]})),
        "attachment_corrupt"
    );
}
#[test]
fn file_limits_and_links_are_not_silently_skipped() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(&t.path().join("store"));
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"files","paths":[t.path()]})
        ),
        "invalid_request"
    );
    let path = t.path().join("too-large.bin");
    fs::File::create(&path)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"files","paths":[path]})
        ),
        "file_limit"
    );
    #[cfg(unix)]
    {
        let target = t.path().join("target.txt");
        fs::write(&target, b"target").unwrap();
        let link = t.path().join("link.txt");
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(
            fail(
                &mut e,
                json!({"op":"capture","kind":"files","paths":[link]})
            ),
            "invalid_request"
        );
    }
    assert_eq!(list(&mut e)["total"], 0);
}
#[test]
fn invalid_image_does_not_enter_history() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(&t.path().join("store"));
    let p = t.path().join("not.png");
    fs::write(&p, b"not a png").unwrap();
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"image","image_path":p})
        ),
        "invalid_request"
    );
    assert_eq!(list(&mut e)["total"], 0);
}
#[test]
fn image_metadata_is_consistent_across_views_without_source_paths() {
    let (t, mut a, mut b) = pair();
    let source_directory = t.path().join("source-device-private-folder");
    fs::create_dir(&source_directory).unwrap();
    let source = source_directory.join("元信息.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 120, 80, 255]))
        .save(&source)
        .unwrap();
    let bytes = fs::read(&source).unwrap();
    let expected = json!({
        "name":"元信息.png",
        "size":bytes.len(),
        "hash":blake3::hash(&bytes).to_hex().to_string()
    });
    let captured = call(
        &mut a,
        json!({"op":"capture","kind":"image","image_path":source,"source":"synthetic.test"}),
    );
    exchange(&mut a, &mut b);
    let assert_view = |view: &Value, device: &str| {
        assert_eq!(view["image"], expected);
        assert!(view["files"].as_array().unwrap().is_empty());
        let local_path = Path::new(view["image_path"].as_str().unwrap());
        let root = t.path().join(device).canonicalize().unwrap();
        assert!(local_path.starts_with(root.join("attachments")));
        assert_ne!(local_path, source);
        assert!(!view.to_string().contains("source-device-private-folder"));
        assert_eq!(view["available"], true);
    };
    assert_view(&captured, "a");
    for (engine, device) in [(&mut a, "a"), (&mut b, "b")] {
        assert_view(
            &call(engine, json!({"op":"get","id":captured["id"]})),
            device,
        );
        assert_view(&list(engine)["items"][0], device);
    }
    call(
        &mut a,
        json!({"op":"tags","id":captured["id"],"tags":["A"]}),
    );
    call(
        &mut b,
        json!({"op":"tags","id":captured["id"],"tags":["B"]}),
    );
    exchange(&mut a, &mut b);
    for (engine, device) in [(&mut a, "a"), (&mut b, "b")] {
        let conflicts = call(engine, json!({"op":"conflicts"}));
        let versions = conflicts["items"][0]["versions"].as_array().unwrap();
        assert_eq!(versions.len(), 2);
        for version in versions {
            assert_view(&version["record"], device);
        }
        // The immutable operation is the cloud plaintext before encryption.
        // Neither source paths nor generated device-local paths belong to it.
        let database = rusqlite::Connection::open_with_flags(
            t.path().join(device).join("library-v2.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement = database
            .prepare("SELECT data FROM operations WHERE entity=?1")
            .unwrap();
        let rows = statement
            .query_map(
                [format!("item:{}", captured["id"].as_str().unwrap())],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        for row in rows {
            let raw = row.unwrap();
            assert!(!raw.contains("source-device-private-folder"));
            assert!(!raw.contains("image_path"));
            assert!(!raw.contains("\"path\""));
            let op: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(op["value"]["data"]["image"], expected);
        }
    }
}
#[test]
fn local_quota_failure_preserves_existing_history() {
    let t = tempfile::tempdir().unwrap();
    let mut e = open(t.path());
    capture(&mut e, "retained");
    let old = call(&mut e, json!({"op":"status"}))["pending"].clone();
    call(
        &mut e,
        json!({"op":"settings","values":{"local_limit_bytes":1}}),
    );
    assert_eq!(
        fail(
            &mut e,
            json!({"op":"capture","kind":"text","text":"blocked"})
        ),
        "local_quota"
    );
    assert_eq!(list(&mut e)["total"], 1);
    assert_eq!(call(&mut e, json!({"op":"status"}))["pending"], old);
}
#[test]
fn sync_roundtrip_receipts_and_v1_namespace_isolation() {
    let (t, mut a, mut b) = pair();
    fs::write(t.path().join("cloud/pasteleaf-v1.json"), b"untouched v1").unwrap();
    let r = capture(&mut a, "跨端 fixture 😀");
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["items"][0]["id"], r["id"]);
    assert_eq!(
        call(&mut a, json!({"op":"status"}))["sync_state"],
        "peer_imported"
    );
    assert_eq!(
        fs::read(t.path().join("cloud/pasteleaf-v1.json")).unwrap(),
        b"untouched v1"
    );
    let cloud = fs::read_dir(t.path().join("cloud/PasteLeaf-v2/operations")).unwrap();
    for entry in cloud {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture"));
    }
    call(&mut a, json!({"op":"delete","id":r["id"]}));
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["total"], 0);
}
#[test]
fn offline_queue_clear_local_and_pause_do_not_delete_peer() {
    let (_t, mut a, mut b) = pair();
    let r = capture(&mut a, "keep on peer");
    exchange(&mut a, &mut b);
    call(&mut a, json!({"op":"clear_local"}));
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut a)["total"], 0);
    assert_eq!(list(&mut b)["items"][0]["id"], r["id"]);
    call(&mut a, json!({"op":"sync_pause","paused":true}));
    capture(&mut a, "offline queued");
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["total"], 1);
    call(&mut a, json!({"op":"sync_pause","paused":false}));
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["total"], 2);
}
#[test]
fn concurrent_edits_are_explicit_and_keep_all_preserves_versions() {
    let (_t, mut a, mut b) = pair();
    let r = capture(&mut a, "conflict item");
    exchange(&mut a, &mut b);
    call(&mut a, json!({"op":"tags","id":r["id"],"tags":["A"]}));
    call(&mut b, json!({"op":"tags","id":r["id"],"tags":["B"]}));
    exchange(&mut a, &mut b);
    let cs = call(&mut a, json!({"op":"conflicts"}));
    assert_eq!(cs["items"].as_array().unwrap().len(), 1);
    assert_eq!(cs["items"][0]["versions"].as_array().unwrap().len(), 2);
    assert_eq!(
        fail(
            &mut a,
            json!({"op":"favorite","id":r["id"],"category":DEFAULT_CATEGORY})
        ),
        "conflict"
    );
    call(
        &mut a,
        json!({"op":"resolve","entity":cs["items"][0]["entity"],"keep":"all"}),
    );
    exchange(&mut a, &mut b);
    assert_eq!(call(&mut a, json!({"op":"status"}))["conflicts"], 0);
    assert_eq!(list(&mut b)["total"], 2);
}
#[test]
fn file_roundtrip_waits_for_complete_attachment() {
    let (t, mut a, mut b) = pair();
    let source = t.path().join("attachment.txt");
    fs::write(&source, b"file sync fixture").unwrap();
    let r = call(
        &mut a,
        json!({"op":"capture","kind":"files","paths":[source],"source":"fixture"}),
    );
    call(&mut a, json!({"op":"sync_now"}));
    let blob = fs::read_dir(t.path().join("cloud/PasteLeaf-v2/attachments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let hidden = blob.with_extension("held");
    fs::rename(&blob, &hidden).unwrap();
    call(&mut b, json!({"op":"sync_now"}));
    assert_eq!(list(&mut b)["total"], 0);
    assert_eq!(
        call(&mut b, json!({"op":"status"}))["sync_state"],
        "waiting_attachments"
    );
    fs::rename(hidden, blob).unwrap();
    exchange(&mut a, &mut b);
    let got = call(&mut b, json!({"op":"get","id":r["id"]}));
    assert_eq!(
        fs::read(got["files"][0]["path"].as_str().unwrap()).unwrap(),
        b"file sync fixture"
    );
    assert_ne!(got["files"][0]["path"], r["files"][0]["path"]);
}
#[test]
fn wrong_password_tampering_and_unknown_versions_fail_closed() {
    let (t, mut a, mut b) = pair();
    let mut wrong = open(&t.path().join("wrong"));
    assert_eq!(
        fail(
            &mut wrong,
            json!({"op":"sync_join","directory":t.path().join("cloud"),"password":"incorrect-password"})
        ),
        "authentication"
    );
    assert_eq!(
        call(&mut wrong, json!({"op":"status"}))["sync_state"],
        "disconnected"
    );
    capture(&mut a, "auth fixture");
    call(&mut a, json!({"op":"sync_now"}));
    let file = fs::read_dir(t.path().join("cloud/PasteLeaf-v2/operations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_stem().unwrap() != "5fc5d0d4-4f25-4262-8dc9-f5dd25f6954c")
        .unwrap();
    let mut env: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    env["protocol"] = json!(3);
    fs::write(&file, env.to_string()).unwrap();
    assert_eq!(fail(&mut b, json!({"op":"sync_now"})), "protocol_version");
    assert_eq!(list(&mut b)["total"], 0);
    env["protocol"] = json!(2);
    let mut cipher = env["ciphertext"].as_str().unwrap().as_bytes().to_vec();
    cipher[48] = if cipher[48] == b'A' { b'B' } else { b'A' };
    env["ciphertext"] = json!(String::from_utf8(cipher).unwrap());
    fs::write(file, env.to_string()).unwrap();
    assert_eq!(fail(&mut b, json!({"op":"sync_now"})), "authentication");
    assert_eq!(list(&mut b)["total"], 0);
}
#[test]
fn sync_quota_keeps_outbox_for_retry() {
    let (t, mut a, mut b) = pair();
    let r = capture(&mut a, "quota fixture");
    call(
        &mut a,
        json!({"op":"settings","values":{"sync_limit_bytes":1}}),
    );
    assert_eq!(fail(&mut a, json!({"op":"sync_now"})), "sync_quota");
    assert!(
        call(&mut a, json!({"op":"status"}))["pending"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert_eq!(list(&mut a)["items"][0]["id"], r["id"]);
    call(
        &mut a,
        json!({"op":"settings","values":{"sync_limit_bytes":10737418240u64}}),
    );
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["total"], 1);
    assert!(t.path().join("cloud/PasteLeaf-v2").exists());
}
#[test]
fn same_length_corrupt_existing_attachment_is_not_imported_or_acknowledged() {
    let (t, mut a, mut b) = pair();
    let source = t.path().join("verified.txt");
    let bytes = b"authenticated attachment fixture";
    fs::write(&source, bytes).unwrap();
    let record = call(
        &mut a,
        json!({"op":"capture","kind":"files","paths":[source],"source":"fixture"}),
    );
    call(&mut a, json!({"op":"sync_now"}));
    let attachment = &record["files"][0];
    let local = t
        .path()
        .join("b/attachments")
        .join(attachment["hash"].as_str().unwrap())
        .join(attachment["name"].as_str().unwrap());
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    fs::write(&local, vec![b'x'; bytes.len()]).unwrap();
    assert_eq!(fail(&mut b, json!({"op":"sync_now"})), "attachment_corrupt");
    assert_eq!(list(&mut b)["total"], 0);
    call(&mut a, json!({"op":"sync_now"}));
    assert_ne!(
        call(&mut a, json!({"op":"status"}))["sync_state"],
        "peer_imported"
    );
    // The engine does not overwrite a damaged history blob. A controlled repair
    // that removes it allows authenticated bytes to download on the next pass.
    fs::remove_file(&local).unwrap();
    exchange(&mut a, &mut b);
    assert_eq!(list(&mut b)["total"], 1);
    assert_eq!(fs::read(local).unwrap(), bytes);
    assert_eq!(
        call(&mut a, json!({"op":"status"}))["sync_state"],
        "peer_imported"
    );
}
#[test]
fn v1_import_backups_preserves_ids_and_is_idempotent() {
    let t = tempfile::tempdir().unwrap();
    let legacy = t.path().join("v1");
    fs::create_dir(&legacy).unwrap();
    let stable = "8f7d55b8-9d1c-4606-b697-dab96c3db7a9";
    let old = json!({"version":1,"items":[{"id":7,"content":{"Text":"旧版记录"},"category":"收藏","source":"legacy","copied_at":1700000000,"tags":["测试"]}],"categories":["收藏"],"sync":{"item_ids":{"7":stable},"category_ids":{"收藏":DEFAULT_CATEGORY}}});
    let raw = old.to_string();
    fs::write(legacy.join("library.json"), &raw).unwrap();
    let mut e = open(&t.path().join("v2"));
    let result = call(&mut e, json!({"op":"import_v1","directory":legacy}));
    assert_eq!(result["imported"], 1);
    assert_eq!(
        fs::read(legacy.join("library.json")).unwrap(),
        raw.as_bytes()
    );
    assert_eq!(
        fs::read(Path::new(result["backup"].as_str().unwrap()).join("library.json")).unwrap(),
        raw.as_bytes()
    );
    let r = list(&mut e);
    assert_eq!(r["items"][0]["id"], stable);
    assert_eq!(r["items"][0]["copied_at"], 1700000000000i64);
    assert_eq!(
        call(&mut e, json!({"op":"import_v1","directory":legacy}))["already_imported"],
        true
    );
}
#[test]
fn reopen_without_remembering_requires_unlock_and_retains_queue() {
    let (t, mut a, _b) = pair();
    capture(&mut a, "survives restart");
    drop(a);
    let mut a = Engine::open(t.path().join("a")).unwrap();
    assert_eq!(call(&mut a, json!({"op":"status"}))["sync_state"], "locked");
    assert_eq!(list(&mut a)["total"], 1);
    call(
        &mut a,
        json!({"op":"sync_unlock","password":"synthetic-test-password","remember":false}),
    );
    call(&mut a, json!({"op":"sync_now"}));
    assert_eq!(call(&mut a, json!({"op":"status"}))["pending"], 0);
}
#[test]
fn ffi_roundtrip_version_null_and_ownership() {
    use pasteleaf_core::*;
    use std::ffi::{CStr, CString};
    let t = tempfile::tempdir().unwrap();
    unsafe {
        assert_eq!(pl_api_version(), 2);
        let path = CString::new(t.path().to_str().unwrap()).unwrap();
        let handle = pl_open(path.as_ptr());
        assert!(!handle.is_null());
        let input = CString::new("{\"op\":\"status\"}").unwrap();
        let result = pl_request(handle, input.as_ptr());
        let response: Value =
            serde_json::from_str(CStr::from_ptr(result).to_str().unwrap()).unwrap();
        assert_eq!(response["ok"], true);
        pl_free(result);
        let result = pl_request(std::ptr::null_mut(), input.as_ptr());
        let response: Value =
            serde_json::from_str(CStr::from_ptr(result).to_str().unwrap()).unwrap();
        assert_eq!(response["ok"], false);
        pl_free(result);
        pl_close(handle);
        pl_close(std::ptr::null_mut());
        pl_free(std::ptr::null_mut());
    }
}
