//! Offline-first replicated registers. Only immutable authenticated files cross devices.
pub mod crypto;
use crate::{
    model::{Content, Item, Model},
    store::Store,
};
use crypto::{Descriptor, Key};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const FILE_LIMIT: u64 = 24 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub config: Option<Config>,
    #[serde(default)]
    pub library_hint: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub item_ids: BTreeMap<u64, String>,
    #[serde(default)]
    pub category_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub operations: BTreeMap<String, Operation>,
    #[serde(default)]
    pub outbox: BTreeSet<String>,
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    #[serde(default)]
    pub materialized: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub directory: PathBuf,
    pub descriptor: Descriptor,
    pub paused: bool,
    pub remembered_key: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub content: Content,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub source: String,
    pub copied_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    Record(Record),
    Category {
        name: String,
        color: u32,
    },
    Scope(bool),
    Deleted,
    /// Local-only causal stub. Never published; immutable cloud history remains intact.
    Retired,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub entity: String,
    pub device: String,
    pub parents: BTreeSet<String>,
    pub value: Value,
}
impl State {
    pub fn heads(&self, entity: &str) -> Vec<&Operation> {
        let retired: BTreeSet<_> = self
            .operations
            .values()
            .filter(|o| o.entity == entity)
            .flat_map(|o| o.parents.iter())
            .collect();
        self.operations
            .values()
            .filter(|o| o.entity == entity && !retired.contains(&o.id))
            .collect()
    }
    pub fn all_records(&self) -> bool {
        let h = self.heads("scope");
        h.len() == 1 && h[0].value == Value::Scope(true) && ready(self, h[0], &mut BTreeSet::new())
    }
    pub fn conflicts(&self) -> Vec<String> {
        self.operations
            .values()
            .map(|o| o.entity.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|e| self.heads(e).len() > 1)
            .collect()
    }
    pub fn image_files(&self) -> BTreeSet<String> {
        self.operations
            .values()
            .filter_map(|o| match &o.value {
                Value::Record(Record {
                    content: Content::Image { file, .. },
                    ..
                }) => Some(file.clone()),
                _ => None,
            })
            .collect()
    }
    pub fn emit(&mut self, entity: String, value: Value) {
        let parents = self.heads(&entity).iter().map(|o| o.id.clone()).collect();
        let op = Operation {
            id: id(),
            entity,
            device: self.device.clone(),
            parents,
            value,
        };
        self.outbox.insert(op.id.clone());
        self.operations.insert(op.id.clone(), op);
    }
    fn compact(&mut self) {
        let ancestors: BTreeSet<_> = self
            .operations
            .values()
            .flat_map(|o| o.parents.iter().cloned())
            .collect();
        for (id, operation) in &mut self.operations {
            if ancestors.contains(id) && !self.outbox.contains(id) {
                operation.value = Value::Retired;
            }
        }
    }
}

/// Diff local user actions only. Remote imports never pass through this function.
pub fn initialize_identifiers(model: &mut Model) -> bool {
    let mut changed = false;
    if model.sync.device.is_empty() {
        model.sync.device = id();
        changed = true;
    }
    for item in &model.items {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            model.sync.item_ids.entry(item.id)
        {
            entry.insert(id());
            changed = true;
        }
    }
    for category in &model.categories {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            model.sync.category_ids.entry(category.clone())
        {
            entry.insert(id());
            changed = true;
        }
    }
    changed
}

/// Add replication metadata in the same candidate transaction as the local edit.
pub fn record_changes(
    previous: &Model,
    next: &mut Model,
    deleted: Option<u64>,
) -> Result<(), String> {
    initialize_identifiers(next);
    if next.sync.config.is_none() {
        return Ok(());
    }
    if next.sync.device.is_empty() {
        next.sync.device = id();
    }
    for name in &next.categories {
        next.sync
            .category_ids
            .entry(name.clone())
            .or_insert_with(id);
    }
    for item in &next.items {
        next.sync.item_ids.entry(item.id).or_insert_with(id);
    }
    for name in &next.categories {
        let uuid = next.sync.category_ids[name].clone();
        let value = Value::Category {
            name: name.clone(),
            color: *next.category_colors.get(name).unwrap_or(&0xEC6482),
        };
        let entity = format!("category:{uuid}");
        // Ordinary edits cannot implicitly resolve an existing concurrent branch.
        let changed = previous
            .sync
            .category_ids
            .iter()
            .find(|(_, value)| **value == uuid)
            .is_some_and(|(old, _)| {
                old != name || previous.category_colors.get(old) != next.category_colors.get(name)
            });
        if changed && next.sync.heads(&entity).len() > 1 {
            return Err("这个分类存在同步冲突，请先处理冲突".into());
        }
        if (next.sync.heads(&entity).is_empty() || changed)
            && next.sync.heads(&entity).len() <= 1
            && next
                .sync
                .heads(&entity)
                .first()
                .is_none_or(|o| o.value != value)
        {
            next.sync.emit(entity, value);
        }
    }
    for (name, uuid) in &previous.sync.category_ids {
        if !next.sync.category_ids.values().any(|v| v == uuid)
            || (!next.categories.contains(name) && next.sync.category_ids.get(name) == Some(uuid))
        {
            next.sync.emit(format!("category:{uuid}"), Value::Deleted);
            next.sync.category_ids.remove(name);
        }
    }
    for old in &previous.items {
        if !next.items.iter().any(|i| i.id == old.id) {
            if let Some(uuid) = next.sync.item_ids.get(&old.id).cloned() {
                let entity = format!("item:{uuid}");
                if deleted == Some(old.id) {
                    if next.sync.heads(&entity).len() > 1 {
                        return Err("这条记录存在同步冲突，请先处理冲突".into());
                    }
                    next.sync.emit(entity, Value::Deleted);
                } else {
                    next.sync.hidden.insert(entity);
                }
            }
        }
    }
    let all = next.sync.all_records();
    for item in &next.items {
        let entity = format!("item:{}", next.sync.item_ids[&item.id]);
        let known = !next.sync.heads(&entity).is_empty();
        let old = previous.items.iter().find(|i| i.id == item.id);
        let unpin = old.is_some_and(|i| i.category.is_some()) && item.category.is_none();
        if !all && item.category.is_none() && !(known && unpin) {
            continue;
        }
        let record = Record {
            content: item.content.clone(),
            category: item
                .category
                .as_ref()
                .map(|c| next.sync.category_ids[c].clone()),
            tags: item.tags.clone(),
            source: item.source.clone(),
            copied_at: item.copied_at,
        };
        let value = Value::Record(record);
        if next.sync.heads(&entity).len() > 1 {
            if old.is_some_and(|i| {
                i.content != item.content || i.tags != item.tags || i.category != item.category
            }) {
                return Err("这条记录存在同步冲突，请先在同步设置中处理".into());
            }
            continue;
        }
        if next
            .sync
            .heads(&entity)
            .first()
            .is_none_or(|o| o.value != value)
        {
            next.sync.hidden.remove(&entity);
            next.sync.emit(entity.clone(), value);
        }
        if let Some(head) = next.sync.heads(&entity).first() {
            next.sync.materialized.insert(entity, head.id.clone());
        }
    }
    Ok(())
}

pub fn set_scope(model: &mut Model, all: bool) -> Result<(), String> {
    model.sync.emit("scope".into(), Value::Scope(all));
    let before = model.clone();
    record_changes(&before, model, None)
}

/// Create/join using a candidate model. Caller persists the candidate before installation.
pub fn connect(
    model: &Model,
    local: &Path,
    directory: &Path,
    password: &str,
    create: bool,
    all: bool,
    remember: bool,
) -> Result<(Model, Key), String> {
    if password.chars().count() < 12 {
        return Err("同步密码至少 12 个字符".into());
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let local = local.canonicalize().map_err(|e| e.to_string())?;
    if directory.starts_with(&local) || local.starts_with(&directory) {
        return Err("同步文件夹与本地数据目录不能重叠".into());
    }
    let descriptor_path = directory.join("pasteleaf-v1.json");
    let (descriptor, key) = if create && !descriptor_path.exists() {
        if fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
        {
            return Err("创建同步库需要一个空文件夹；已有库请选择加入".into());
        }
        let (d, k) = Descriptor::create(password)?;
        publish(
            &descriptor_path,
            &serde_json::to_vec(&d).map_err(|e| e.to_string())?,
        )?;
        (d, k)
    } else {
        if create && directory.join("operations").exists() {
            return Err("已有同步库请选择加入".into());
        }
        let d: Descriptor = serde_json::from_slice(&read_limited(&descriptor_path, 8192)?)
            .map_err(|_| "同步库描述损坏")?;
        let k = d.unlock(password)?;
        (d, k)
    };
    let mut next = model.clone();
    if next.sync.library_hint != descriptor.library {
        next.sync = State {
            device: if model.sync.device.is_empty() {
                id()
            } else {
                model.sync.device.clone()
            },
            item_ids: model.sync.item_ids.clone(),
            category_ids: model.sync.category_ids.clone(),
            library_hint: descriptor.library.clone(),
            ..State::default()
        };
    }
    next.sync.config = Some(Config {
        directory,
        descriptor,
        paused: false,
        remembered_key: if remember {
            crypto::protect(&*key)?
        } else {
            vec![]
        },
    });
    if create {
        next.sync.emit("scope".into(), Value::Scope(all));
    } else {
        // Remote scope is authoritative; do not publish local data before reading it.
        if next.items.is_empty() && next.categories == ["收藏"] {
            next.categories.clear();
        }
        let remote = exchange(&next.sync, &local, &key, false)?;
        merge(&mut next, remote)?;
        if next.sync.heads("scope").is_empty()
            || next
                .sync
                .heads("scope")
                .iter()
                .any(|o| !ready(&next.sync, o, &mut BTreeSet::new()))
        {
            return Err("同步库尚未下载完整，请等待 iCloud Drive 下载完成后重试".into());
        }
    }
    record_changes(model, &mut next, None)?;
    next.validate()?;
    Ok((next, key))
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > limit {
        return Err("同步文件超过大小限制".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}
fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let parent = path.parent().ok_or("同步路径无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!("{}.partial", id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            let _ = fs::remove_file(temporary);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Clone, Default)]
pub struct Exchange {
    pub operations: Vec<Operation>,
    pub sent: BTreeSet<String>,
    pub waiting: usize,
    pub notice: Option<String>,
}
/// Worker-only IO. The UI applies these operations to its *current* model, not an old snapshot.
pub fn exchange(state: &State, local: &Path, key: &Key, upload: bool) -> Result<Exchange, String> {
    exchange_with_limit(state, local, key, upload, LIMIT)
}
fn exchange_with_limit(
    state: &State,
    local: &Path,
    key: &Key,
    upload: bool,
    limit: u64,
) -> Result<Exchange, String> {
    let c = state.config.as_ref().ok_or("同步未启用")?;
    let d: Descriptor =
        serde_json::from_slice(&read_limited(&c.directory.join("pasteleaf-v1.json"), 8192)?)
            .map_err(|_| "同步库描述损坏")?;
    if d.library != c.descriptor.library || d.protocol != 1 {
        return Err("同步库已更换或版本不支持".into());
    }
    d.verify(key)?;
    let ops_dir = c.directory.join("operations");
    let blobs = c.directory.join("images");
    let mut result = Exchange::default();
    if ops_dir.exists() {
        for entry in fs::read_dir(&ops_dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("plop") {
                continue;
            }
            let bytes = read_limited(&path, FILE_LIMIT)?;
            // Internal ID in an authenticated envelope allows cloud-client renamed duplicates.
            let envelope: Envelope =
                serde_json::from_slice(&bytes).map_err(|_| "同步操作文件损坏")?;
            let raw = crypto::open(key, &d.library, &envelope.id, &envelope.bytes)?;
            let op: Operation = serde_json::from_slice(&raw).map_err(|_| "同步操作格式无效")?;
            if op.id != envelope.id {
                return Err("同步操作标识不匹配".into());
            }
            validate_operation(&op)?;
            if let Some(existing) = state.operations.get(&envelope.id) {
                if !same_operation(existing, &op) {
                    return Err("重复操作标识内容不一致".into());
                }
                continue;
            }
            if let Value::Record(Record {
                content:
                    Content::Image {
                        file,
                        width,
                        height,
                    },
                ..
            }) = &op.value
            {
                let object = blob_id(key, file);
                let path = blobs.join(format!("{object}.plimg"));
                if !path.exists() {
                    result.waiting += 1;
                    continue;
                }
                let data =
                    crypto::open(key, &d.library, &object, &read_limited(&path, FILE_LIMIT)?)?;
                let reader = image::ImageReader::with_format(
                    std::io::Cursor::new(&data),
                    image::ImageFormat::Png,
                );
                if reader.into_dimensions().map_err(|_| "同步图片损坏")? != (*width, *height)
                {
                    return Err("同步图片尺寸不匹配".into());
                }
                let image = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
                    .map_err(|_| "同步图片损坏")?
                    .to_rgba8();
                if image.width() != *width
                    || image.height() != *height
                    || image.len() > 16 * 1024 * 1024
                {
                    return Err("同步图片尺寸不匹配".into());
                }
                let store = Store::new(local.to_path_buf())?;
                if store.write_image(*width, *height, image.as_raw())?
                    != (Content::Image {
                        file: file.clone(),
                        width: *width,
                        height: *height,
                    })
                {
                    return Err("同步图片内容校验失败".into());
                }
            }
            result.operations.push(op);
        }
    }
    if upload {
        let mut size = directory_size(&c.directory)?;
        for pending in &state.outbox {
            let op = state.operations.get(pending).ok_or("待发送操作丢失")?;
            if let Value::Record(Record {
                content: Content::Image { file, .. },
                ..
            }) = &op.value
            {
                let object = blob_id(key, file);
                let path = blobs.join(format!("{object}.plimg"));
                if !path.exists() {
                    let raw = read_limited(&local.join("images").join(file), FILE_LIMIT)?;
                    let data = crypto::seal(key, &d.library, &object, &raw)?;
                    size = size.saturating_add(data.len() as u64);
                    if size > limit {
                        result.notice =
                            Some("同步目录达到 2 GiB，新增发布已暂停；本地仍可使用".into());
                        break;
                    }
                    publish(&path, &data)?;
                }
            }
            let path = ops_dir.join(format!("{}.plop", op.id));
            if !path.exists() {
                let bytes = crypto::seal(
                    key,
                    &d.library,
                    &op.id,
                    &serde_json::to_vec(op).map_err(|e| e.to_string())?,
                )?;
                let envelope = serde_json::to_vec(&Envelope {
                    id: op.id.clone(),
                    bytes,
                })
                .map_err(|e| e.to_string())?;
                size = size.saturating_add(envelope.len() as u64);
                if size > limit {
                    result.notice = Some("同步目录达到 2 GiB，新增发布已暂停；本地仍可使用".into());
                    break;
                }
                publish(&path, &envelope)?;
            }
            result.sent.insert(op.id.clone());
        }
    }
    Ok(result)
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    id: String,
    bytes: Vec<u8>,
}
fn blob_id(key: &Key, file: &str) -> String {
    blake3::keyed_hash(key, file.as_bytes())
        .to_hex()
        .to_string()
}
fn directory_size(path: &Path) -> Result<u64, String> {
    let mut total: u64 = 0;
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err("同步目录不能包含符号链接".into());
        }
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        total = total.saturating_add(if metadata.is_dir() {
            directory_size(&entry.path())?
        } else {
            metadata.len()
        });
    }
    Ok(total)
}
fn validate_operation(op: &Operation) -> Result<(), String> {
    if uuid::Uuid::parse_str(&op.id).is_err()
        || uuid::Uuid::parse_str(&op.device).is_err()
        || op.parents.len() > 1024
        || op.parents.contains(&op.id)
        || op.parents.iter().any(|p| uuid::Uuid::parse_str(p).is_err())
    {
        return Err("同步操作标识无效".into());
    }
    let item = op
        .entity
        .strip_prefix("item:")
        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok());
    let category = op
        .entity
        .strip_prefix("category:")
        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok());
    match &op.value {
        Value::Scope(_) if op.entity == "scope" => Ok(()),
        Value::Deleted if item || category => Ok(()),
        Value::Category { name, .. } if category => {
            let mut m = Model::default();
            m.categories.clear();
            m.add_category(name)
        }
        Value::Record(r) if item => {
            if r.source.len() > 4096
                || r.category
                    .as_ref()
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_err())
            {
                return Err("同步记录格式无效".into());
            }
            let mut m = Model::default();
            m.items.push(Item {
                id: 1,
                content: r.content.clone(),
                category: None,
                tags: r.tags.clone(),
                source: r.source.clone(),
                copied_at: r.copied_at,
            });
            m.next_id = 2;
            m.validate()
        }
        _ => Err("同步实体类型不匹配".into()),
    }
}

/// Atomically persist the returned model before changing visible data.
pub fn merge(model: &mut Model, exchange: Exchange) -> Result<usize, String> {
    for op in exchange.operations {
        validate_operation(&op)?;
        if let Some(old) = model.sync.operations.get(&op.id) {
            if !same_operation(old, &op) {
                return Err("重复同步标识内容不一致".into());
            }
            continue;
        }
        model.sync.operations.insert(op.id.clone(), op);
    }
    for sent in exchange.sent {
        model.sync.outbox.remove(&sent);
    }
    let pending = project(model)?;
    model.sync.compact();
    Ok(pending)
}
fn same_operation(a: &Operation, b: &Operation) -> bool {
    a.id == b.id
        && a.entity == b.entity
        && a.device == b.device
        && a.parents == b.parents
        && (a.value == Value::Retired || a.value == b.value)
}
fn ready(state: &State, op: &Operation, visiting: &mut BTreeSet<String>) -> bool {
    let mut stack = vec![(op, false)];
    let mut done = BTreeSet::new();
    while let Some((current, exit)) = stack.pop() {
        if exit {
            visiting.remove(&current.id);
            done.insert(current.id.clone());
            continue;
        }
        if done.contains(&current.id) {
            continue;
        }
        if !visiting.insert(current.id.clone()) {
            return false;
        }
        stack.push((current, true));
        for parent in &current.parents {
            let Some(parent) = state.operations.get(parent) else {
                return false;
            };
            if parent.entity != op.entity {
                return false;
            }
            stack.push((parent, false));
        }
    }
    true
}
fn project(model: &mut Model) -> Result<usize, String> {
    let entities: BTreeSet<_> = model
        .sync
        .operations
        .values()
        .map(|o| o.entity.clone())
        .collect();
    let mut pending = 0;
    // Category keys sort before items; scope is read from the complete register.
    for entity in entities {
        if entity == "scope" {
            continue;
        }
        let heads = model.sync.heads(&entity);
        if heads.len() != 1 || !ready(&model.sync, heads[0], &mut BTreeSet::new()) {
            pending += 1;
            continue;
        }
        let op = heads[0].clone();
        if model.sync.materialized.get(&entity) == Some(&op.id)
            || model.sync.hidden.contains(&entity)
        {
            continue;
        }
        if let Some(uuid) = entity.strip_prefix("category:") {
            let old = model
                .sync
                .category_ids
                .iter()
                .find(|(_, v)| v.as_str() == uuid)
                .map(|(n, _)| n.clone());
            match &op.value {
                Value::Category { name, color } => {
                    let collision = model
                        .categories
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(name) && Some(n) != old.as_ref());
                    let name = if collision {
                        format!(
                            "{} · {}",
                            name.chars().take(18).collect::<String>(),
                            &uuid[..8]
                        )
                    } else {
                        name.clone()
                    };
                    if let Some(old) = old {
                        if old != name {
                            model.rename_category(&old, &name)?;
                        }
                    } else {
                        if model.categories.len() >= 100 {
                            pending += 1;
                            continue;
                        }
                        model.add_category(&name)?;
                    }
                    model.sync.category_ids.insert(name.clone(), uuid.into());
                    model.category_colors.insert(name, *color);
                }
                Value::Deleted => {
                    if let Some(old) = old {
                        model.delete_category(&old)?;
                        model.sync.category_ids.remove(&old);
                    }
                }
                _ => return Err("分类操作无效".into()),
            }
        } else if let Some(uuid) = entity.strip_prefix("item:") {
            let local_id = model
                .sync
                .item_ids
                .iter()
                .find(|(_, v)| v.as_str() == uuid)
                .map(|(i, _)| *i);
            match &op.value {
                Value::Record(r) => {
                    let category = r.category.as_ref().and_then(|c| {
                        model
                            .sync
                            .category_ids
                            .iter()
                            .find(|(_, v)| *v == c)
                            .map(|(n, _)| n.clone())
                    });
                    if let Some(c) = &r.category {
                        if category.is_none()
                            && !model
                                .sync
                                .heads(&format!("category:{c}"))
                                .iter()
                                .any(|o| o.value == Value::Deleted)
                        {
                            pending += 1;
                            continue;
                        }
                    }
                    if r.category.is_none() && !model.sync.all_records() && local_id.is_none() {
                        continue;
                    }
                    let local_id = if let Some(i) = local_id {
                        i
                    } else {
                        let i = model.next_id;
                        model.next_id = model.next_id.checked_add(1).ok_or("记录编号已耗尽")?;
                        model.sync.item_ids.insert(i, uuid.into());
                        i
                    };
                    let item = Item {
                        id: local_id,
                        content: r.content.clone(),
                        category,
                        tags: r.tags.clone(),
                        source: r.source.clone(),
                        copied_at: r.copied_at,
                    };
                    let mut candidate = model.clone();
                    if let Some(current) = candidate.items.iter_mut().find(|i| i.id == local_id) {
                        *current = item;
                    } else {
                        candidate.items.push(item);
                    }
                    if candidate.validate().is_err()
                        || candidate
                            .items
                            .iter()
                            .filter(|i| i.category.is_none())
                            .count()
                            > 200
                    {
                        pending += 1;
                        continue;
                    }
                    model.items = candidate.items;
                }
                Value::Deleted => {
                    if let Some(i) = local_id {
                        model.items.retain(|x| x.id != i);
                    }
                }
                _ => return Err("记录操作无效".into()),
            }
        }
        model.sync.materialized.insert(entity, op.id);
    }
    model.validate()?;
    Ok(pending)
}

/// Resolve all known concurrent heads. Other offline changes can still conflict later.
pub fn resolve(
    model: &mut Model,
    entity: &str,
    selected: &str,
    keep_both: bool,
) -> Result<(), String> {
    let heads = model.sync.heads(entity);
    let chosen = heads
        .iter()
        .find(|o| o.id == selected)
        .ok_or("冲突版本已变化，请刷新")?
        .value
        .clone();
    if keep_both {
        let others: Vec<_> = heads
            .iter()
            .filter(|o| o.id != selected)
            .filter_map(|o| {
                if let Value::Record(r) = &o.value {
                    Some(r.clone())
                } else {
                    None
                }
            })
            .collect();
        for r in others {
            model.sync.emit(format!("item:{}", id()), Value::Record(r));
        }
    }
    model.sync.emit(entity.into(), chosen);
    project(model)?;
    Ok(())
}
pub fn disconnect(model: &mut Model) {
    // Retain stable identities for rejoining the same library, but forget its key/path.
    model.sync.config = None;
}
pub fn suggested_directory() -> Option<PathBuf> {
    let profile = PathBuf::from(std::env::var_os("USERPROFILE")?);
    icloud_directory(&profile)
}

fn icloud_directory(profile: &Path) -> Option<PathBuf> {
    // A custom iCloud Drive location can be selected with the native folder picker.
    // Do not create a lookalike root when iCloud Drive has not been configured.
    ["iCloudDrive", "iCloud Drive"]
        .into_iter()
        .map(|name| profile.join(name))
        .find(|path| path.is_dir())
        .map(|path| path.join("PasteLeaf Sync"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icloud_discovery_requires_existing_root_and_prefers_current_name() {
        let profile = std::env::temp_dir().join(format!("pasteleaf-discovery-{}", id()));
        fs::create_dir_all(profile.join("OneDrive")).unwrap();
        assert_eq!(icloud_directory(&profile), None);
        fs::write(profile.join("iCloudDrive"), b"not a directory").unwrap();
        assert_eq!(icloud_directory(&profile), None);
        fs::create_dir_all(profile.join("iCloud Drive")).unwrap();
        assert_eq!(
            icloud_directory(&profile),
            Some(profile.join("iCloud Drive/PasteLeaf Sync"))
        );
        fs::remove_file(profile.join("iCloudDrive")).unwrap();
        fs::create_dir_all(profile.join("iCloudDrive")).unwrap();
        assert_eq!(
            icloud_directory(&profile),
            Some(profile.join("iCloudDrive/PasteLeaf Sync"))
        );
        assert!(!profile.join("iCloudDrive/PasteLeaf Sync").exists());
        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn quota_keeps_outbox_and_read_only_exchange_works() {
        let root = std::env::temp_dir().join(format!("pasteleaf-quota-{}", id()));
        let store = Store::new(root.join("local")).unwrap();
        let (model, key) = connect(
            &Model::default(),
            &store.root,
            &root.join("sync"),
            "synthetic quota password",
            true,
            false,
            false,
        )
        .unwrap();
        let result = exchange_with_limit(&model.sync, &store.root, &key, true, 1).unwrap();
        assert!(result.notice.is_some());
        assert!(result.sent.is_empty());
        assert!(!model.sync.outbox.is_empty());
        assert!(exchange_with_limit(&model.sync, &store.root, &key, false, 1).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
