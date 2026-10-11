use crate::{
    error::{invalid, Error, Result},
    files,
    model::*,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::BTreeSet, path::Path};

pub fn open(root: &Path) -> Result<Connection> {
    files::safe_dir(root)?;
    files::safe_dir(&root.join("attachments"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let dbpath = root.join("library-v2.sqlite3");
    if dbpath.exists() {
        files::ordinary(&dbpath)?;
    }
    let conn = Connection::open(dbpath)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v != 0 && v != 2 {
        return Err(Error::new(
            "schema_version",
            "Unsupported local database version; original data untouched",
        ));
    }
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
    )?;
    conn.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS records(id TEXT UNIQUE NOT NULL,data TEXT NOT NULL,search TEXT NOT NULL,fingerprint TEXT NOT NULL,category TEXT,copied_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS record_time ON records(copied_at DESC,id);
        CREATE INDEX IF NOT EXISTS record_category ON records(category,copied_at DESC);
        CREATE INDEX IF NOT EXISTS record_fingerprint ON records(fingerprint);
        CREATE TABLE IF NOT EXISTS categories(id TEXT PRIMARY KEY,data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS hidden(id TEXT PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,entity TEXT NOT NULL,data TEXT NOT NULL,ready INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS operation_entity ON operations(entity);
        CREATE INDEX IF NOT EXISTS operation_ready ON operations(ready);
        CREATE TABLE IF NOT EXISTS parents(op TEXT NOT NULL,parent TEXT NOT NULL,PRIMARY KEY(op,parent));
        CREATE INDEX IF NOT EXISTS parent_child ON parents(parent);
        CREATE TABLE IF NOT EXISTS heads(entity TEXT NOT NULL,op TEXT NOT NULL,PRIMARY KEY(entity,op));
        CREATE TABLE IF NOT EXISTS outbox(op TEXT PRIMARY KEY,published_at INTEGER);
        CREATE TABLE IF NOT EXISTS receipts(device TEXT NOT NULL,op TEXT NOT NULL,imported_at INTEGER NOT NULL,PRIMARY KEY(device,op));
        CREATE TABLE IF NOT EXISTS receipt_outbox(op TEXT PRIMARY KEY,published_at INTEGER);
        CREATE VIRTUAL TABLE IF NOT EXISTS records_fts USING fts5(search,content=records,content_rowid=rowid,tokenize='trigram');
        CREATE TRIGGER IF NOT EXISTS records_ai AFTER INSERT ON records BEGIN INSERT INTO records_fts(rowid,search) VALUES(new.rowid,new.search); END;
        CREATE TRIGGER IF NOT EXISTS records_ad AFTER DELETE ON records BEGIN INSERT INTO records_fts(records_fts,rowid,search) VALUES('delete',old.rowid,old.search); END;
        CREATE TRIGGER IF NOT EXISTS records_au AFTER UPDATE OF search ON records BEGIN INSERT INTO records_fts(records_fts,rowid,search) VALUES('delete',old.rowid,old.search); INSERT INTO records_fts(rowid,search) VALUES(new.rowid,new.search); END;
        CREATE TABLE IF NOT EXISTS short_search(token TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(token,id)) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS short_record ON short_search(id);
        PRAGMA user_version=2; COMMIT;")?;
    if meta(&conn, "device")?.is_none() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if meta(&conn, "device")?.is_some() {
                return Ok(());
            }
            put_meta(&conn, "device", &id())?;
            put_meta(
                &conn,
                "settings",
                &serde_json::to_string(&default_settings())?,
            )?;
            let initial = Operation {
                protocol: 2,
                id: "5fc5d0d4-4f25-4262-8dc9-f5dd25f6954c".into(),
                entity: format!("category:{DEFAULT_CATEGORY}"),
                device: "00000000-0000-4000-8000-000000000001".into(),
                parents: BTreeSet::new(),
                value: Payload::Category(Category {
                    id: DEFAULT_CATEGORY.into(),
                    name: "收藏".into(),
                    color: "#EE7658".into(),
                }),
            };
            insert(&conn, &initial, true)?;
            promote(&conn, root)?;
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    Ok(conn)
}
pub fn meta(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(
        c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .optional()?,
    )
}
pub fn put_meta(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
    Ok(())
}
pub fn record(c: &Connection, id: &str) -> Result<Option<Record>> {
    c.query_row("SELECT data FROM records WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|s| serde_json::from_str(&s).map_err(Into::into))
    .transpose()
}
pub fn operation(c: &Connection, id: &str) -> Result<Option<Operation>> {
    c.query_row("SELECT data FROM operations WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|s| serde_json::from_str(&s).map_err(Into::into))
    .transpose()
}
pub fn heads(c: &Connection, entity: &str) -> Result<Vec<Operation>> {
    let mut q = c.prepare(
        "SELECT o.data FROM heads h JOIN operations o ON o.id=h.op WHERE h.entity=?1 ORDER BY h.op",
    )?;
    let rows = q.query_map([entity], |r| r.get::<_, String>(0))?;
    rows.map(|r| serde_json::from_str(&r?).map_err(Into::into))
        .collect()
}
pub fn local(
    c: &Connection,
    root: &Path,
    device: &str,
    entity: &str,
    value: Payload,
    resolve: bool,
) -> Result<String> {
    let h = heads(c, entity)?;
    if h.len() > 1 && !resolve {
        return Err(Error::new(
            "conflict",
            "Resolve this entity's concurrent versions before editing it",
        ));
    }
    let op = Operation {
        protocol: 2,
        id: id(),
        entity: entity.into(),
        device: device.into(),
        parents: h.iter().map(|x| x.id.clone()).collect(),
        value,
    };
    insert(c, &op, true)?;
    promote(c, root)?;
    Ok(op.id)
}
pub fn insert(c: &Connection, op: &Operation, local: bool) -> Result<bool> {
    op.validate()?;
    if let Some(old) = operation(c, &op.id)? {
        if old != *op {
            return Err(Error::new(
                "equivocation",
                "The same immutable operation ID has different authenticated content",
            ));
        }
        return Ok(false);
    }
    c.execute(
        "INSERT INTO operations(id,entity,data) VALUES(?1,?2,?3)",
        params![op.id, op.entity, serde_json::to_string(op)?],
    )?;
    for p in &op.parents {
        c.execute(
            "INSERT INTO parents(op,parent) VALUES(?1,?2)",
            params![op.id, p],
        )?;
    }
    let cycle:bool=c.query_row("WITH RECURSIVE ancestors(id) AS (SELECT parent FROM parents WHERE op=?1 UNION SELECT p.parent FROM parents p JOIN ancestors a ON p.op=a.id) SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?1)",[&op.id],|r|r.get(0))?;
    if cycle {
        return Err(Error::new(
            "causal_cycle",
            "Cyclic synchronization history rejected",
        ));
    }
    // Child may have arrived earlier. Cross-entity parent links are never valid.
    let bad:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM parents p JOIN operations child ON child.id=p.op JOIN operations parent ON parent.id=p.parent WHERE child.entity!=parent.entity AND (child.id=?1 OR parent.id=?1))",[&op.id],|r|r.get(0))?;
    if bad {
        return Err(Error::new(
            "causal_entity",
            "A synchronization parent belongs to another entity",
        ));
    }
    if local {
        c.execute("INSERT INTO outbox(op) VALUES(?1)", [&op.id])?;
    }
    Ok(true)
}
pub fn promote(c: &Connection, root: &Path) -> Result<usize> {
    loop {
        let mut q = c.prepare("SELECT data FROM operations WHERE ready=0 ORDER BY entity,id")?;
        let candidates = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut progress = false;
        for raw in candidates {
            let op: Operation = serde_json::from_str(&raw)?;
            let missing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM parents p LEFT JOIN operations o ON o.id=p.parent WHERE p.op=?1 AND (o.id IS NULL OR o.ready=0))",[&op.id],|r|r.get(0))?;
            if missing {
                continue;
            }
            if let Payload::Record(r) = &op.value {
                if r.attachments().any(|a| !files::available(root, a)) {
                    continue;
                }
                // A same-length corrupt local blob must not make an operation
                // visible or generate an import receipt. This check also covers
                // previously staged operations whose parent arrived later.
                for a in r.attachments() {
                    files::verify(root, a)?;
                    if r.kind == "image" {
                        files::validate_image(&files::limited(
                            &files::blob_path(root, a),
                            FILE_LIMIT,
                        )?)?;
                    }
                }
            }
            c.execute("UPDATE operations SET ready=1 WHERE id=?1", [&op.id])?;
            for p in &op.parents {
                c.execute(
                    "DELETE FROM heads WHERE entity=?1 AND op=?2",
                    params![op.entity, p],
                )?;
            }
            c.execute(
                "INSERT INTO heads(entity,op) VALUES(?1,?2)",
                params![op.entity, op.id],
            )?;
            project(c, &op.entity)?;
            c.execute(
                "INSERT OR IGNORE INTO receipt_outbox(op) VALUES(?1)",
                [&op.id],
            )?;
            progress = true;
        }
        if !progress {
            break;
        }
    }
    Ok(
        c.query_row("SELECT count(*) FROM operations WHERE ready=0", [], |r| {
            r.get::<_, i64>(0)
        })? as usize,
    )
}
pub fn project(c: &Connection, entity: &str) -> Result<()> {
    let h = heads(c, entity)?;
    if h.len() != 1 {
        return Ok(());
    }
    let op = &h[0];
    match &op.value {
        Payload::Record(r) => upsert_record(c, r)?,
        Payload::Category(cat) => {
            c.execute("INSERT INTO categories(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![cat.id,serde_json::to_string(cat)?])?;
        }
        Payload::Deleted => {
            if let Some(id) = entity.strip_prefix("item:") {
                c.execute("DELETE FROM short_search WHERE id=?1", [id])?;
                c.execute("DELETE FROM records WHERE id=?1", [id])?;
            } else if let Some(id) = entity.strip_prefix("category:") {
                c.execute("DELETE FROM categories WHERE id=?1", [id])?;
                c.execute("UPDATE records SET category=NULL,data=json_set(data,'$.category',NULL) WHERE category=?1",[id])?;
            }
        }
    }
    Ok(())
}
pub fn upsert_record(c: &Connection, r: &Record) -> Result<()> {
    let mut r = r.clone();
    if let Some(category) = &r.category {
        let h = heads(c, &format!("category:{category}"))?;
        if h.len() == 1 && matches!(h[0].value, Payload::Deleted) {
            r.category = None;
        }
    }
    let search = r.search_text();
    let old: Option<String> = c
        .query_row("SELECT search FROM records WHERE id=?1", [&r.id], |row| {
            row.get(0)
        })
        .optional()?;
    c.execute("INSERT INTO records(id,data,search,fingerprint,category,copied_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET data=excluded.data,search=excluded.search,fingerprint=excluded.fingerprint,category=excluded.category,copied_at=excluded.copied_at",params![r.id,serde_json::to_string(&r)?,search,r.fingerprint(),r.category,r.copied_at])?;
    if old.as_deref() != Some(&search) {
        c.execute("DELETE FROM short_search WHERE id=?1", [&r.id])?;
        let chars: Vec<char> = search.chars().collect();
        let mut tokens = BTreeSet::new();
        for i in 0..chars.len() {
            tokens.insert(chars[i].to_string());
            if i + 1 < chars.len() {
                tokens.insert(chars[i..i + 2].iter().collect::<String>());
            }
        }
        let mut stmt =
            c.prepare_cached("INSERT OR IGNORE INTO short_search(token,id) VALUES(?1,?2)")?;
        for token in tokens {
            stmt.execute(params![token, r.id])?;
        }
    }
    Ok(())
}
pub fn categories(c: &Connection) -> Result<Vec<Category>> {
    let mut q = c.prepare("SELECT data FROM categories ORDER BY json_extract(data,'$.name'),id")?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| serde_json::from_str(&r?).map_err(Into::into))
        .collect::<Result<Vec<_>>>();
    rows
}
pub fn ensure_category(c: &Connection, id: &str) -> Result<()> {
    if !uuid(id)
        || !c.query_row(
            "SELECT EXISTS(SELECT 1 FROM categories WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Err(invalid("Category does not exist"));
    }
    Ok(())
}
pub fn list(
    c: &Connection,
    query: &str,
    category: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Record>, i64)> {
    if offset < 0 || !(1..=500).contains(&limit) || query.len() > 4096 {
        return Err(invalid(
            "Use nonnegative offset, 1..500 limit, and a query of at most 4096 bytes",
        ));
    }
    let q = query.to_lowercase();
    let search = if q.is_empty() {
        ""
    } else if q.chars().count() < 3 {
        " JOIN short_search s ON s.id=r.id AND s.token=?1 "
    } else {
        " JOIN records_fts f ON f.rowid=r.rowid AND records_fts MATCH ?1 "
    };
    let arg = if q.chars().count() >= 3 {
        format!("\"{}\"", q.replace('"', "\"\""))
    } else {
        q
    };
    let filter=" WHERE NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=r.id) AND (?2 IS NULL OR r.category=?2 OR (?2='favorites' AND r.category IS NOT NULL)) ";
    let total: i64 = c.query_row(
        &format!("SELECT count(*) FROM records r {search} {filter} AND (?1 IS NOT NULL)"),
        params![arg, category],
        |r| r.get(0),
    )?;
    let mut stmt=c.prepare(&format!("SELECT r.data FROM records r {search} {filter} AND (?1 IS NOT NULL) ORDER BY r.copied_at DESC,r.id LIMIT ?3 OFFSET ?4"))?;
    let rows = stmt.query_map(params![arg, category, limit, offset], |r| {
        r.get::<_, String>(0)
    })?;
    let items = rows
        .map(|r| serde_json::from_str(&r?).map_err(Into::into))
        .collect::<Result<_>>()?;
    Ok((items, total))
}
