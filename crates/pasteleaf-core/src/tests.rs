use super::*;
use std::{collections::BTreeSet, time::Instant};
fn record(id: &str, text: &str) -> Record {
    Record {
        id: id.into(),
        kind: "text".into(),
        text: text.into(),
        html: None,
        rtf: None,
        files: vec![],
        image: None,
        category: None,
        tags: vec!["中文".into()],
        source: "synthetic.fixture".into(),
        copied_at: 1700000000000,
    }
}
fn operation(entity: &str, parents: BTreeSet<String>) -> Operation {
    Operation {
        protocol: 2,
        id: id(),
        entity: format!("item:{entity}"),
        device: id(),
        parents,
        value: Payload::Record(record(entity, "fixture")),
    }
}
#[test]
fn causal_parents_arriving_out_of_order_are_not_materialized_early() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    let entity = id();
    let first = operation(&entity, BTreeSet::new());
    let mut second = operation(&entity, BTreeSet::from([first.id.clone()]));
    if let Payload::Record(r) = &mut second.value {
        r.text = "newest".into();
    }
    let tx = e.conn.transaction().unwrap();
    db::insert(&tx, &second, false).unwrap();
    assert_eq!(db::promote(&tx, &e.root).unwrap(), 1);
    assert!(db::record(&tx, &entity).unwrap().is_none());
    tx.commit().unwrap();
    let tx = e.conn.transaction().unwrap();
    db::insert(&tx, &first, false).unwrap();
    assert_eq!(db::promote(&tx, &e.root).unwrap(), 0);
    assert_eq!(db::record(&tx, &entity).unwrap().unwrap().text, "newest");
    tx.commit().unwrap();
}
#[test]
fn operation_equivocation_cycles_and_cross_entity_parents_are_rejected() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    let entity = id();
    let first = operation(&entity, BTreeSet::new());
    {
        let tx = e.conn.transaction().unwrap();
        db::insert(&tx, &first, false).unwrap();
        db::promote(&tx, &e.root).unwrap();
        tx.commit().unwrap();
    }
    let mut altered = first.clone();
    if let Payload::Record(r) = &mut altered.value {
        r.text = "altered".into();
    }
    {
        let tx = e.conn.transaction().unwrap();
        assert_eq!(
            db::insert(&tx, &altered, false).unwrap_err().code,
            "equivocation"
        );
    }
    let other = operation(&id(), BTreeSet::from([first.id.clone()]));
    {
        let tx = e.conn.transaction().unwrap();
        assert_eq!(
            db::insert(&tx, &other, false).unwrap_err().code,
            "causal_entity"
        );
    }
    let mut a = operation(&entity, BTreeSet::new());
    let b = operation(&entity, BTreeSet::from([a.id.clone()]));
    a.parents.insert(b.id.clone());
    {
        let tx = e.conn.transaction().unwrap();
        db::insert(&tx, &a, false).unwrap();
        tx.commit().unwrap();
    }
    {
        let tx = e.conn.transaction().unwrap();
        assert_eq!(db::insert(&tx, &b, false).unwrap_err().code, "causal_cycle");
    }
    assert!(db::operation(&e.conn, &b.id).unwrap().is_none());
}
#[test]
fn outbox_failure_rolls_back_record_and_operation_together() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    e.request(json!({"op":"settings","values":{"paused":false}}))
        .unwrap();
    let count: i64 = e
        .conn
        .query_row("SELECT count(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    e.conn.execute_batch("CREATE TRIGGER test_full BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT,'simulated write failure'); END;").unwrap();
    assert_eq!(
        e.request(json!({"op":"capture","kind":"text","text":"must rollback"}))
            .unwrap_err()
            .code,
        "database"
    );
    assert_eq!(
        e.conn
            .query_row("SELECT count(*) FROM operations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(e.request(json!({"op":"list"})).unwrap()["total"], 0);
}
#[test]
fn sqlite_full_rolls_back_without_deleting_history() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    e.request(json!({"op":"settings","values":{"paused":false}}))
        .unwrap();
    e.request(json!({"op":"capture","kind":"text","text":"preserve"}))
        .unwrap();
    let page_count: i64 = e
        .conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    e.conn
        .pragma_update(None, "max_page_count", page_count)
        .unwrap();
    let result = e.request(json!({"op":"capture","kind":"text","text":"q".repeat(1024*1024)}));
    assert!(result.is_err());
    assert_eq!(e.request(json!({"op":"list"})).unwrap()["total"], 1);
}
#[test]
fn authentication_binds_library_and_object_and_bounds_kdf() {
    let (mut d, key) = crypto::Descriptor::create("synthetic-password-only").unwrap();
    let ciphertext = crypto::seal(&key, &d.library, "one", b"fixture").unwrap();
    assert_eq!(
        crypto::open(&key, &d.library, "one", &ciphertext).unwrap(),
        b"fixture"
    );
    assert!(crypto::open(&key, &d.library, "two", &ciphertext).is_err());
    assert!(crypto::open(&key, &id(), "one", &ciphertext).is_err());
    d.memory_kib = u32::MAX;
    assert!(d.unlock("synthetic-password-only").is_err());
    d.memory_kib = 65536;
    d.protocol = 3;
    assert!(d.unlock("synthetic-password-only").is_err());
}
#[test]
fn password_and_source_paths_never_enter_cloud_operation_payloads() {
    let source = std::env::temp_dir().join("example-source-name.txt");
    let a = model::Attachment {
        name: "example-source-name.txt".into(),
        size: 0,
        hash: blake3::hash(b"").to_hex().to_string(),
    };
    let mut r = record(&id(), "example-source-name.txt");
    r.kind = "files".into();
    r.files = vec![a];
    let value = serde_json::to_value(r).unwrap();
    assert!(value.get("path").is_none());
    assert!(value["files"][0].get("path").is_none());
    assert!(!value.to_string().contains(source.to_str().unwrap()));
}
#[test]
#[ignore = "Explicit release-mode 100,000-row benchmark; run --release -- --ignored --nocapture"]
fn benchmark_100k_indexed_history() {
    let t = tempfile::tempdir().unwrap();
    let mut e = Engine::open(t.path()).unwrap();
    let start = Instant::now();
    {
        let tx = e.conn.transaction().unwrap();
        for i in 0..100_000 {
            let mut r = record(
                &format!("00000000-0000-4000-8000-{i:012}"),
                &format!("设计评审 项目交付 Clipboard fixture {i:06}"),
            );
            r.copied_at += i;
            r.tags = vec![format!("任务{}", i % 100), "研发".into()];
            r.source = if i % 2 == 0 {
                "com.example.editor"
            } else {
                "com.example.browser"
            }
            .into();
            db::upsert_record(&tx, &r).unwrap();
        }
        tx.commit().unwrap();
    }
    e.conn
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); ANALYZE;")
        .unwrap();
    eprintln!("seed_100k_ms={}", start.elapsed().as_millis());
    let mut all = vec![];
    for query in ["", "设计", "设计评审", "001337", "任务23", "研"] {
        let mut times = vec![];
        for _ in 0..20 {
            let start = Instant::now();
            let result = e
                .request(json!({"op":"list","query":query,"limit":100}))
                .unwrap();
            assert!(result["total"].as_i64().unwrap() > 0);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let p95 = times[18];
        eprintln!("query={query:?} p95_ms={p95:.2}");
        all.push(p95);
    }
    assert!(
        all.iter().all(|t| *t <= 200.0),
        "Core warm query p95 target exceeded; native panel timing is a separate gate: {all:?}"
    );
}
#[test]
fn windows_hydrated_cloud_tags_are_not_treated_as_symlinks() {
    assert!(files::validate_windows_attributes(0x20, 0).is_ok());
    assert!(files::validate_windows_attributes(0x400, 0x9000001A).is_ok());
    for n in 0..=15 {
        assert!(files::validate_windows_attributes(0x400, 0x9000001A | (n << 12)).is_ok());
    }
    assert_eq!(
        files::validate_windows_attributes(0x400 | 0x400000, 0x9000001A)
            .unwrap_err()
            .code,
        "pending_hydration"
    );
    assert_eq!(
        files::validate_windows_attributes(0x400 | 0x1000, 0x9000001A)
            .unwrap_err()
            .code,
        "pending_hydration"
    );
    assert!(files::validate_windows_attributes(0x400, 0xA000000C).is_err());
    assert!(files::validate_windows_attributes(0x400, 0xA0000003).is_err());
    assert!(files::validate_windows_attributes(0x400, 0x8000001B).is_err());
}
#[test]
fn publication_never_overwrites_existing_or_concurrent_immutable_files() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("immutable");
    assert!(files::publish(&path, b"first").unwrap());
    assert!(!files::publish(&path, b"second").unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), b"first");
    let path = t.path().join("race");
    let handles: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                files::publish(&path, format!("writer-{n}").as_bytes()).unwrap()
            })
        })
        .collect();
    let wins = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .filter(|won| *won)
        .count();
    assert_eq!(wins, 1);
}
