//! Shared PasteLeaf v2 core. No clipboard polling or platform UI lives here.
mod crypto;
mod db;
mod error;
mod ffi;
mod files;
mod import;
pub mod model;
mod runtime;
mod sync;
mod vault;
use crate::{error::invalid, model::*};
pub use error::{Error, Result};
pub use ffi::{pl_api_version, pl_close, pl_free, pl_open, pl_request};
pub use runtime::Runtime;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct Engine {
    conn: Connection,
    root: PathBuf,
    device: String,
    key: Option<crypto::Key>,
    settings: Value,
    config: Option<sync::Config>,
    last_error: Option<Error>,
    waiting: usize,
}
impl Engine {
    /// Opens isolated local storage. Does not read the system clipboard or start timers.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        if !root.as_ref().is_absolute() {
            return Err(invalid("Data root must be an absolute path"));
        }
        let conn = db::open(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        let device =
            db::meta(&conn, "device")?.ok_or_else(|| invalid("Missing device identifier"))?;
        let settings: Value = serde_json::from_str(
            &db::meta(&conn, "settings")?.ok_or_else(|| invalid("Missing settings"))?,
        )?;
        let config = sync::config(&conn)?;
        let mut key = None;
        if let Some(c) = &config {
            if c.remembered {
                if let Ok(k) = vault::load(&root, &format!("{}-{device}", c.descriptor.library)) {
                    if c.descriptor.verify(&k).is_ok() {
                        key = Some(k);
                    }
                }
            }
        }
        Ok(Self {
            conn,
            root,
            device,
            key,
            settings,
            config,
            last_error: None,
            waiting: 0,
        })
    }
    /// Always returns a JSON envelope, never logs request/body/password content.
    pub fn request_json(&mut self, input: &str) -> String {
        let response = if input.len() > model::OP_LIMIT as usize {
            Err(invalid("Request exceeds the supported limit"))
        } else {
            serde_json::from_str(input)
                .map_err(Error::from)
                .and_then(|v| self.request(v))
        };
        envelope(response)
    }
    pub fn request(&mut self, v: Value) -> Result<Value> {
        let op = text(&v, "op")?;
        match op {
            "list" => query_list(&self.conn, &self.root, &v),
            "get" => {
                let r = self.current(text(&v, "id")?)?;
                for a in r.attachments() {
                    files::verify(&self.root, a)?;
                }
                Ok(self.view(&r))
            }
            "capture" => self.capture(&v),
            "delete" => {
                let id = text(&v, "id")?;
                self.current(id)?;
                self.check_quota(4096)?;
                let tx = self.conn.transaction()?;
                db::local(
                    &tx,
                    &self.root,
                    &self.device,
                    &format!("item:{id}"),
                    Payload::Deleted,
                    false,
                )?;
                tx.commit()?;
                Ok(json!({"deleted":id,"propagates":true,"cloud_history_retained":true}))
            }
            "clear_local" => {
                let all = v.get("all").and_then(Value::as_bool).unwrap_or(false);
                let n=self.conn.execute(if all{"INSERT OR IGNORE INTO hidden(id) SELECT id FROM records"}else{"INSERT OR IGNORE INTO hidden(id) SELECT id FROM records WHERE category IS NULL"},[])?;
                Ok(json!({"hidden":n,"propagates":false,"retained_attachments":true}))
            }
            "favorite" | "tags" => {
                let mut r = self.current(text(&v, "id")?)?;
                if op == "favorite" {
                    r.category = match v.get("category") {
                        Some(Value::String(s)) => {
                            db::ensure_category(&self.conn, s)?;
                            Some(s.clone())
                        }
                        Some(Value::Null) => None,
                        None => {
                            if r.category.is_some() {
                                None
                            } else {
                                Some(DEFAULT_CATEGORY.into())
                            }
                        }
                        _ => return Err(invalid("Category must be a stable ID or null")),
                    };
                } else {
                    r.tags = tags(
                        v.get("tags")
                            .ok_or_else(|| invalid("Tags array is required"))?,
                    )?;
                }
                if let Some(category) = &r.category {
                    db::ensure_category(&self.conn, category)?;
                }
                r.validate()?;
                self.check_quota(serde_json::to_vec(&r)?.len() as u64 * 3 + 4096)?;
                let tx = self.conn.transaction()?;
                db::local(
                    &tx,
                    &self.root,
                    &self.device,
                    &format!("item:{}", r.id),
                    Payload::Record(r.clone()),
                    false,
                )?;
                tx.commit()?;
                Ok(self.view(&r))
            }
            "category_create" | "category_rename" => {
                let cat = if op == "category_create" {
                    Category {
                        id: id(),
                        name: text(&v, "name")?.trim().into(),
                        color: v
                            .get("color")
                            .and_then(Value::as_str)
                            .unwrap_or("#EE7658")
                            .into(),
                    }
                } else {
                    let id = text(&v, "id")?;
                    let mut cat = db::categories(&self.conn)?
                        .into_iter()
                        .find(|c| c.id == id)
                        .ok_or_else(|| invalid("Category not found"))?;
                    cat.name = text(&v, "name")?.trim().into();
                    if let Some(color) = v.get("color").and_then(Value::as_str) {
                        cat.color = color.into();
                    }
                    cat
                };
                cat.validate()?;
                if db::categories(&self.conn)?
                    .iter()
                    .any(|c| c.id != cat.id && c.name.to_lowercase() == cat.name.to_lowercase())
                {
                    return Err(invalid("Category name already exists"));
                }
                self.check_quota(8192)?;
                let tx = self.conn.transaction()?;
                db::local(
                    &tx,
                    &self.root,
                    &self.device,
                    &format!("category:{}", cat.id),
                    Payload::Category(cat.clone()),
                    false,
                )?;
                tx.commit()?;
                Ok(serde_json::to_value(cat)?)
            }
            "category_delete" => {
                let id = text(&v, "id")?;
                db::ensure_category(&self.conn, id)?;
                self.check_quota(8192)?;
                let tx = self.conn.transaction()?;
                db::local(
                    &tx,
                    &self.root,
                    &self.device,
                    &format!("category:{id}"),
                    Payload::Deleted,
                    false,
                )?;
                tx.commit()?;
                Ok(json!({"deleted":id,"items_retained":true}))
            }
            "settings" => self.set_settings(&v),
            "status" => self.status(),
            "sync_inspect" => sync::inspect(Path::new(text(&v, "directory")?)),
            "sync_create" | "sync_join" => self.connect(&v, op == "sync_create"),
            "sync_unlock" => {
                let mut c = self
                    .config
                    .clone()
                    .ok_or_else(|| invalid("Sync is not configured"))?;
                let key = c
                    .descriptor
                    .unlock(text(&v, "password")?)
                    .map_err(|m| Error::new("authentication", m))?;
                let was_remembered = c.remembered;
                c.remembered = v.get("remember").and_then(Value::as_bool).unwrap_or(false);
                let account = format!("{}-{}", c.descriptor.library, self.device);
                if c.remembered {
                    vault::save(&self.root, &account, &key)?;
                } else if was_remembered {
                    vault::forget(&self.root, &account)?;
                }
                sync::save_config(&self.conn, &c)?;
                self.key = Some(key);
                self.config = Some(c);
                self.last_error = None;
                self.status()
            }
            "sync_pause" => {
                let mut c = self
                    .config
                    .clone()
                    .ok_or_else(|| invalid("Sync is not configured"))?;
                c.paused = v
                    .get("paused")
                    .and_then(Value::as_bool)
                    .unwrap_or(!c.paused);
                sync::save_config(&self.conn, &c)?;
                self.config = Some(c);
                self.status()
            }
            "sync_disconnect" => {
                if let Some(c) = &self.config {
                    if c.remembered {
                        vault::forget(
                            &self.root,
                            &format!("{}-{}", c.descriptor.library, self.device),
                        )?;
                    }
                }
                self.conn
                    .execute("DELETE FROM meta WHERE key='sync_config'", [])?;
                self.key = None;
                self.config = None;
                self.last_error = None;
                self.status()
            }
            "sync_now" => {
                self.exchange()?;
                self.status()
            }
            "conflicts" => self.conflicts(),
            "resolve" => self.resolve(&v),
            "import_v1" => self.import_v1(&v),
            _ => Err(invalid("Unknown command for PasteLeaf core API 2")),
        }
    }
    fn current(&self, id: &str) -> Result<Record> {
        if !model::uuid(id) {
            return Err(invalid("Item ID must be a UUID"));
        }
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM hidden WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::new(
                "not_found",
                "This item is hidden on this device",
            ));
        }
        db::record(&self.conn, id)?
            .ok_or_else(|| Error::new("not_found", "Clipboard item was not found"))
    }
    fn view(&self, r: &Record) -> Value {
        record_view(&self.root, r)
    }
    fn quota(&self, key: &str) -> u64 {
        self.settings
            .get(key)
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_QUOTA)
    }
    fn check_quota(&self, extra: u64) -> Result<()> {
        if files::size(&self.root)?.saturating_add(extra) > self.quota("local_limit_bytes") {
            return Err(Error::new(
                "local_quota",
                "Local capacity limit reached; no history was deleted",
            ));
        }
        Ok(())
    }
    fn capture(&mut self, v: &Value) -> Result<Value> {
        if self.settings["paused"].as_bool().unwrap_or(true) {
            return Err(Error::new(
                "capture_paused",
                "Clipboard capture is paused; enable it after reviewing privacy settings",
            ));
        }
        let source = v.get("source").and_then(Value::as_str).unwrap_or("");
        if self.settings["excluded_apps"]
            .as_array()
            .is_some_and(|a| a.iter().any(|s| s.as_str() == Some(source)))
        {
            return Err(Error::new(
                "source_excluded",
                "This source application is excluded",
            ));
        }
        let kind = text(v, "kind")?;
        let mut r = Record {
            id: id(),
            kind: kind.into(),
            text: v.get("text").and_then(Value::as_str).unwrap_or("").into(),
            html: v.get("html").and_then(Value::as_str).map(Into::into),
            rtf: v.get("rtf").and_then(Value::as_str).map(Into::into),
            files: vec![],
            image: None,
            category: None,
            tags: vec![],
            source: source.into(),
            copied_at: now(),
        };
        let max_file = self.settings["max_file_bytes"]
            .as_u64()
            .unwrap_or(FILE_LIMIT);
        match kind {
            "image" => {
                files::validate_image(&files::limited(
                    Path::new(text(v, "image_path")?),
                    FILE_LIMIT,
                )?)?;
                let a = files::copy_attachment(
                    &self.root,
                    Path::new(text(v, "image_path")?),
                    self.quota("local_limit_bytes"),
                    max_file,
                )?;
                files::validate_image(&files::limited(
                    &files::blob_path(&self.root, &a),
                    FILE_LIMIT,
                )?)?;
                r.image = Some(a);
            }
            "files" => {
                let paths = v
                    .get("paths")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("Ordinary file paths are required"))?;
                if paths.is_empty() || paths.len() > 128 {
                    return Err(invalid("Capture between 1 and 128 ordinary files"));
                }
                for path in paths {
                    r.files.push(files::copy_attachment(
                        &self.root,
                        Path::new(
                            path.as_str()
                                .ok_or_else(|| invalid("File paths must be strings"))?,
                        ),
                        self.quota("local_limit_bytes"),
                        max_file,
                    )?);
                }
                r.text = r
                    .files
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            "text" => {
                if r.text.is_empty() {
                    return Err(invalid("Empty text is not a clipboard item"));
                }
            }
            "rich_text" => {}
            _ => {
                return Err(invalid(
                    "Capture supports text, image, rich_text and ordinary files",
                ))
            }
        }
        r.validate()?;
        self.check_quota(serde_json::to_vec(&r)?.len() as u64 * 4 + 16_384)?;
        let old:Option<String>=self.conn.query_row("SELECT id FROM records WHERE fingerprint=?1 AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=records.id) ORDER BY copied_at DESC LIMIT 1",[r.fingerprint()],|row|row.get(0)).optional()?;
        if let Some(old) = old {
            if let Some(previous) = db::record(&self.conn, &old)? {
                r.id = old;
                r.category = previous.category;
                r.tags = previous.tags;
            }
        }
        let tx = self.conn.transaction()?;
        db::local(
            &tx,
            &self.root,
            &self.device,
            &format!("item:{}", r.id),
            Payload::Record(r.clone()),
            false,
        )?;
        tx.commit()?;
        Ok(self.view(&r))
    }
    fn set_settings(&mut self, v: &Value) -> Result<Value> {
        let Some(patch) = v.get("values") else {
            return Ok(self.settings.clone());
        };
        let patch = patch
            .as_object()
            .ok_or_else(|| invalid("Settings values must be an object"))?;
        let mut next = self.settings.clone();
        for (k, v) in patch {
            if k.len() > 128 || v.to_string().len() > 65536 {
                return Err(invalid("Setting value exceeds supported size"));
            }
            match k.as_str() {
                "paused"
                | "onboarding_complete"
                | "launch_at_login"
                | "reduce_motion"
                | "reduce_transparency" => {
                    if !v.is_boolean() {
                        return Err(invalid("Boolean setting expected"));
                    }
                }
                "local_limit_bytes" | "sync_limit_bytes" | "local_quota_bytes"
                | "sync_quota_bytes" => {
                    if v.as_u64().is_none_or(|x| x == 0 || x > i64::MAX as u64) {
                        return Err(invalid("Storage limit must be positive bytes"));
                    }
                }
                "max_file_bytes" => {
                    if v.as_u64().is_none_or(|x| x == 0 || x > FILE_LIMIT) {
                        return Err(invalid("File limit must be from 1 byte to 100 MiB"));
                    }
                }
                "appearance" => {
                    if !matches!(v.as_str(), Some("system" | "light" | "dark")) {
                        return Err(invalid("Unsupported appearance"));
                    }
                }
                "excluded_apps" => {
                    if !v.as_array().is_some_and(|a| {
                        a.len() <= 1024
                            && a.iter()
                                .all(|x| x.as_str().is_some_and(|s| s.len() <= 4096))
                    }) {
                        return Err(invalid(
                            "Excluded apps must be an array of source identifiers",
                        ));
                    }
                }
                "hotkey" if !v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128) => {
                    return Err(invalid("Invalid hotkey"));
                }
                _ => {}
            }
            next[k] = v.clone();
            match k.as_str() {
                "local_limit_bytes" => next["local_quota_bytes"] = v.clone(),
                "local_quota_bytes" => next["local_limit_bytes"] = v.clone(),
                "sync_limit_bytes" => next["sync_quota_bytes"] = v.clone(),
                "sync_quota_bytes" => next["sync_limit_bytes"] = v.clone(),
                _ => {}
            }
        }
        db::put_meta(&self.conn, "settings", &serde_json::to_string(&next)?)?;
        self.settings = next;
        Ok(self.settings.clone())
    }
    fn connect(&mut self, v: &Value, create: bool) -> Result<Value> {
        if self.config.is_some() {
            return Err(Error::new(
                "already_connected",
                "Disconnect the current sync library before selecting another",
            ));
        }
        let remember = v.get("remember").and_then(Value::as_bool).unwrap_or(false);
        let (cfg, key) = sync::connect(
            Path::new(text(v, "directory")?),
            text(v, "password")?,
            create,
            remember,
        )?;
        if remember {
            vault::save(
                &self.root,
                &format!("{}-{}", cfg.descriptor.library, self.device),
                &key,
            )?;
        }
        let tx = self.conn.transaction()?;
        sync::save_config(&tx, &cfg)?;
        tx.execute(
            "INSERT OR IGNORE INTO outbox(op) SELECT id FROM operations WHERE ready=1",
            [],
        )?;
        tx.execute("UPDATE outbox SET published_at=NULL", [])?;
        tx.execute("DELETE FROM receipts", [])?;
        tx.execute("UPDATE receipt_outbox SET published_at=NULL", [])?;
        tx.execute("DELETE FROM meta WHERE key='last_exchange_ms'", [])?;
        tx.commit()?;
        self.config = Some(cfg);
        self.key = Some(key);
        self.last_error = None;
        // Connection success is distinct from transport success. Runtime will exchange.
        self.status()
    }
    pub fn exchange(&mut self) -> Result<()> {
        let Some(c) = &self.config else { return Ok(()) };
        if c.paused {
            return Ok(());
        }
        let Some(key) = &self.key else {
            return Err(Error::new(
                "key_locked",
                "Sync is locked; enter the library password",
            ));
        };
        let local_limit = self.quota("local_limit_bytes");
        let sync_limit = self.quota("sync_limit_bytes");
        match sync::exchange(
            &mut self.conn,
            &self.root,
            c,
            key,
            &self.device,
            local_limit,
            sync_limit,
        ) {
            Ok(x) => {
                self.waiting = x.waiting;
                self.last_error = None;
                Ok(())
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                Err(e)
            }
        }
    }
    pub fn sync_directory(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|c| c.directory.clone())
    }
    fn status(&self) -> Result<Value> {
        let pending: i64 = self.conn.query_row(
            "SELECT count(*) FROM outbox WHERE published_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let conflicts: i64 = self.conn.query_row(
            "SELECT count(*) FROM (SELECT entity FROM heads GROUP BY entity HAVING count(*)>1)",
            [],
            |r| r.get(0),
        )?;
        let receipts: i64 = self.conn.query_row(
            "SELECT count(*) FROM receipts r JOIN outbox o ON o.op=r.op",
            [],
            |r| r.get(0),
        )?;
        let unack:i64=self.conn.query_row("SELECT count(*) FROM outbox o WHERE o.published_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM receipts r WHERE r.op=o.op)",[],|r|r.get(0))?;
        let waiting: i64 =
            self.conn
                .query_row("SELECT count(*) FROM operations WHERE ready=0", [], |r| {
                    r.get(0)
                })?;
        let waiting = waiting.max(self.waiting as i64);
        let (state, detail) = if self.config.is_none() {
            (
                "disconnected",
                "Local history only; iCloud Drive is not connected",
            )
        } else if self.config.as_ref().is_some_and(|c| c.paused) {
            ("paused", "Sync is paused; local history is retained")
        } else if self.key.is_none() {
            ("locked", "Unlock the encrypted sync library")
        } else if self
            .last_error
            .as_ref()
            .is_some_and(|e| e.code == "pending_hydration")
        {
            (
                "waiting_attachments",
                "Waiting for iCloud to download a sync file",
            )
        } else if self.last_error.is_some() {
            ("error", "Sync needs attention; local history is retained")
        } else if conflicts > 0 {
            (
                "conflict",
                "Concurrent versions require a choice; none were silently overwritten",
            )
        } else if waiting > 0 {
            (
                "waiting_attachments",
                "Waiting for attachments or causal history to arrive through iCloud Drive",
            )
        } else if pending > 0 {
            ("pending", "Local changes are queued for publication")
        } else if receipts > 0 && unack == 0 {
            (
                "peer_imported",
                "Another device acknowledged importing all published operations",
            )
        } else {
            (
                "published",
                "Written to the local sync folder; cloud delivery is not yet confirmed",
            )
        };
        Ok(
            json!({"sync_state":state,"detail":self.last_error.as_ref().map(|e|e.message.as_str()).unwrap_or(detail),"error":self.last_error,
            "pending":pending,"conflicts":conflicts,"waiting":waiting,"last_exchange_ms":db::meta(&self.conn,"last_exchange_ms")?.and_then(|s|s.parse::<i64>().ok()),
            "peer_receipts":receipts,"paused":self.settings["paused"],"sync_paused":self.config.as_ref().is_some_and(|c|c.paused),"sync_directory":self.sync_directory().map(|p|p.to_string_lossy().to_string()),
            "settings":self.settings,"categories":db::categories(&self.conn)?,"api_version":2,
            "privacy":{"local_history_encrypted":false,"sync_encrypted":true,"passwords_may_be_recorded":true,"cloud_history_retained_after_delete":true}}),
        )
    }
    fn conflicts(&self) -> Result<Value> {
        let mut q = self.conn.prepare(
            "SELECT entity FROM heads GROUP BY entity HAVING count(*)>1 ORDER BY entity",
        )?;
        let entities = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut items = vec![];
        for entity in entities {
            let mut versions = vec![];
            for op in db::heads(&self.conn, &entity)? {
                let mut v = json!({"op_id":op.id,"device":op.device,"deleted":matches!(op.value,Payload::Deleted)});
                match op.value {
                    Payload::Record(r) => v["record"] = self.view(&r),
                    Payload::Category(c) => v["category"] = serde_json::to_value(c)?,
                    _ => {}
                }
                versions.push(v);
            }
            items.push(json!({"entity":entity,"versions":versions}));
        }
        Ok(json!({"items":items}))
    }
    fn resolve(&mut self, v: &Value) -> Result<Value> {
        let entity = text(v, "entity")?;
        let keep = text(v, "keep")?;
        let heads = db::heads(&self.conn, entity)?;
        if heads.len() < 2 {
            return Err(invalid(
                "Entity no longer has a conflict; refresh before resolving",
            ));
        }
        let chosen = if keep == "all" {
            heads
                .iter()
                .find(|o| !matches!(o.value, Payload::Deleted))
                .unwrap_or(&heads[0])
        } else {
            heads
                .iter()
                .find(|o| o.id == keep)
                .ok_or_else(|| invalid("Selected conflict version does not exist"))?
        };
        self.check_quota(
            heads
                .iter()
                .map(|h| serde_json::to_vec(h).map(|v| v.len() as u64).unwrap_or(0))
                .sum::<u64>()
                * 3
                + 8192,
        )?;
        let tx = self.conn.transaction()?;
        if keep == "all" {
            for h in &heads {
                if h.id == chosen.id {
                    continue;
                }
                match &h.value {
                    Payload::Record(r) => {
                        let mut r = r.clone();
                        r.id = id();
                        db::local(
                            &tx,
                            &self.root,
                            &self.device,
                            &format!("item:{}", r.id),
                            Payload::Record(r),
                            false,
                        )?;
                    }
                    Payload::Category(c) => {
                        let mut c = c.clone();
                        c.id = id();
                        c.name = format!(
                            "{} · {}",
                            c.name.chars().take(50).collect::<String>(),
                            &c.id[..6]
                        );
                        db::local(
                            &tx,
                            &self.root,
                            &self.device,
                            &format!("category:{}", c.id),
                            Payload::Category(c),
                            false,
                        )?;
                    }
                    Payload::Deleted => {}
                }
            }
        }
        db::local(
            &tx,
            &self.root,
            &self.device,
            entity,
            chosen.value.clone(),
            true,
        )?;
        tx.commit()?;
        self.conflicts()
    }
}
fn record_view(root: &Path, r: &Record) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        // Keep portable image metadata (name/size/hash); only image_path below
        // is device-local. Attachment never stores the source computer's path.
        m.insert("files".into(),Value::Array(r.files.iter().map(|a|json!({"name":a.name,"size":a.size,"hash":a.hash,"path":files::blob_path(root,a).to_string_lossy()})).collect()));
        if let Some(a) = &r.image {
            m.insert(
                "image_path".into(),
                json!(files::blob_path(root, a).to_string_lossy()),
            );
        }
        m.insert(
            "available".into(),
            json!(r.attachments().all(|a| files::available(root, a))),
        );
    }
    v
}
/// Read-only snapshot connection for panel queries while the serialized sync writer
/// is blocked on filesystem/crypto work. No schema writes or vault access occurs.
pub(crate) fn readonly_list(root: &Path, request: &Value) -> Result<Value> {
    let conn = Connection::open_with_flags(
        root.join("library-v2.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(100))?;
    conn.execute_batch("BEGIN DEFERRED")?;
    query_list(&conn, root, request)
}
fn query_list(conn: &Connection, root: &Path, v: &Value) -> Result<Value> {
    let query = v.get("query").and_then(Value::as_str).unwrap_or("");
    let category = v
        .get("category")
        .and_then(Value::as_str)
        .filter(|s| *s != "history" && *s != "all" && !s.is_empty());
    let (items, total) = db::list(
        conn,
        query,
        category,
        v.get("offset").and_then(Value::as_i64).unwrap_or(0),
        v.get("limit").and_then(Value::as_i64).unwrap_or(100),
    )?;
    Ok(json!({"items":items.iter().map(|r|record_view(root,r)).collect::<Vec<_>>(),"total":total}))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("Missing string field: {key}")))
}
fn tags(v: &Value) -> Result<Vec<String>> {
    let a = v
        .as_array()
        .ok_or_else(|| invalid("Tags must be an array"))?;
    let mut seen = BTreeSet::new();
    let mut result = vec![];
    for t in a {
        let s = t
            .as_str()
            .ok_or_else(|| invalid("Tags must be strings"))?
            .trim();
        if !s.is_empty() && seen.insert(s.to_lowercase()) {
            result.push(s.to_string());
        }
    }
    Ok(result)
}
pub fn envelope(result: Result<Value>) -> String {
    match result {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => json!({"ok":false,"error":error}),
    }
    .to_string()
}

#[cfg(test)]
mod tests;
