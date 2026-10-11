use crate::{db, error::invalid, files, model::*, text, Engine, Error, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
impl Engine {
    pub(crate) fn import_v1(&mut self, v: &Value) -> Result<Value> {
        let directory = Path::new(text(v, "directory")?);
        if !directory.is_absolute() {
            return Err(invalid("Legacy directory must be an absolute path"));
        }
        let source = directory.join("library.json");
        let bytes = files::limited(&source, 256 * 1024 * 1024)?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let key = format!("import_v1:{hash}");
        if let Some(backup) = db::meta(&self.conn, &key)? {
            return Ok(json!({"imported":0,"already_imported":true,"backup":backup}));
        }
        let legacy: Value = serde_json::from_slice(&bytes)?;
        if legacy["version"].as_u64() != Some(1) {
            return Err(Error::new(
                "legacy_version",
                "Only validated v1 library snapshots can be imported",
            ));
        }
        let items = legacy["items"]
            .as_array()
            .ok_or_else(|| invalid("Legacy item list is missing"))?;
        let names = legacy["categories"]
            .as_array()
            .ok_or_else(|| invalid("Legacy categories are missing"))?;
        let mut categories = BTreeMap::new();
        let mut imported = Vec::new();
        for name in names {
            let name = name
                .as_str()
                .ok_or_else(|| invalid("Invalid legacy category"))?;
            let stable = legacy["sync"]["category_ids"][name]
                .as_str()
                .filter(|s| uuid(s))
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if name == "收藏" {
                        DEFAULT_CATEGORY.into()
                    } else {
                        id()
                    }
                });
            let color = legacy["category_colors"][name].as_u64().unwrap_or(0xEE7658) & 0xffffff;
            let c = Category {
                id: stable,
                name: name.into(),
                color: format!("#{color:06X}"),
            };
            c.validate()?;
            categories.insert(name.to_string(), c);
        }
        // Validate and back up before writing a single new visible record.
        let mut backup_images = BTreeMap::new();
        for item in items {
            let old_id = item["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| invalid("Invalid legacy item identifier"))?;
            let stable = legacy["sync"]["item_ids"][old_id.to_string()]
                .as_str()
                .filter(|s| uuid(s))
                .map(str::to_owned)
                .unwrap_or_else(id);
            let mut r = Record {
                id: stable,
                kind: "text".into(),
                text: String::new(),
                html: None,
                rtf: None,
                files: vec![],
                image: None,
                category: item["category"]
                    .as_str()
                    .map(|name| {
                        categories
                            .get(name)
                            .map(|c| c.id.clone())
                            .ok_or_else(|| invalid("Legacy item has an unknown category"))
                    })
                    .transpose()?,
                tags: item["tags"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|t| {
                                t.as_str()
                                    .map(str::to_owned)
                                    .ok_or_else(|| invalid("Invalid legacy tag"))
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
                source: item["source"].as_str().unwrap_or("").into(),
                copied_at: item["copied_at"]
                    .as_u64()
                    .unwrap_or(0)
                    .checked_mul(1000)
                    .and_then(|v| i64::try_from(v).ok())
                    .ok_or_else(|| invalid("Invalid legacy timestamp"))?,
            };
            if let Some(s) = item["content"]["Text"].as_str() {
                r.text = s.into();
            } else if let Some(image) = item["content"].get("Image") {
                let name = image["file"]
                    .as_str()
                    .ok_or_else(|| invalid("Legacy image file is missing"))?;
                if !valid_name(name) || !name.ends_with(".png") {
                    return Err(invalid("Invalid legacy image filename"));
                }
                let bytes = files::limited(&directory.join("images").join(name), FILE_LIMIT)?;
                files::validate_image(&bytes)?;
                let a = Attachment {
                    name: name.into(),
                    size: bytes.len() as u64,
                    hash: blake3::hash(&bytes).to_hex().to_string(),
                };
                r.kind = "image".into();
                r.image = Some(a);
                backup_images.insert(name.to_string(), bytes);
            } else {
                return Err(invalid("Unsupported legacy clipboard content"));
            }
            r.validate()?;
            if imported.iter().any(|x: &Record| x.id == r.id) {
                return Err(invalid("Legacy snapshot contains duplicate stable IDs"));
            }
            imported.push(r);
        }
        let required = bytes.len() as u64
            + backup_images
                .values()
                .map(|b| b.len() as u64 * 2)
                .sum::<u64>()
            + imported
                .iter()
                .map(|r| {
                    serde_json::to_vec(r)
                        .map(|b| b.len() as u64 * 4 + 8192)
                        .unwrap_or(0)
                })
                .sum::<u64>();
        self.check_quota(required)?;
        let backup = self.root.join("migrations").join(format!("v1-{}", id()));
        files::safe_dir(&backup)?;
        files::publish(&backup.join("library.json"), &bytes)?;
        for (name, bytes) in &backup_images {
            files::publish(&backup.join("images").join(name), bytes)?;
        }
        for r in &imported {
            if let Some(a) = &r.image {
                files::publish(&files::blob_path(&self.root, a), &backup_images[&a.name])?;
            }
        }
        let tx = self.conn.transaction()?;
        for c in categories.values() {
            if db::heads(&tx, &format!("category:{}", c.id))?.is_empty() {
                db::local(
                    &tx,
                    &self.root,
                    &self.device,
                    &format!("category:{}", c.id),
                    Payload::Category(c.clone()),
                    false,
                )?;
            }
        }
        for r in &imported {
            if db::record(&tx, &r.id)?.is_some() {
                return Err(Error::new(
                    "import_collision",
                    "A stable legacy item already exists; no history was overwritten",
                ));
            }
            db::local(
                &tx,
                &self.root,
                &self.device,
                &format!("item:{}", r.id),
                Payload::Record(r.clone()),
                false,
            )?;
        }
        db::put_meta(&tx, &key, &backup.to_string_lossy())?;
        tx.commit()?;
        Ok(
            json!({"imported":imported.len(),"backup":backup.to_string_lossy(),"source_unchanged":true,"v1_sync_untouched":true}),
        )
    }
}
