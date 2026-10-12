//! Protocol v2: immutable authenticated objects; dedicated namespace, no v1 writes.
use crate::{
    crypto::{self, Descriptor, Key},
    db,
    error::{invalid, Error, Result},
    files,
    model::*,
};
use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub directory: PathBuf,
    pub descriptor: Descriptor,
    pub paused: bool,
    pub remembered: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: u32,
    id: String,
    ciphertext: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol: u32,
    device: String,
    op: String,
    imported_at: i64,
}
#[derive(Default)]
pub struct Exchange {
    pub waiting: usize,
    pub sent: usize,
    pub imported: usize,
    pub receipts: usize,
}
fn crypt(e: String) -> Error {
    Error::new("authentication", e)
}
fn object(key: &Key, identity: &str) -> String {
    blake3::keyed_hash(key, identity.as_bytes())
        .to_hex()
        .to_string()
}
fn pack(key: &Key, library: &str, id: &str, raw: &[u8]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Envelope {
        protocol: 2,
        id: id.into(),
        ciphertext: base64::engine::general_purpose::STANDARD
            .encode(crypto::seal(key, library, id, raw).map_err(crypt)?),
    })?)
}
fn unpack(key: &Key, library: &str, bytes: &[u8]) -> Result<(String, Vec<u8>)> {
    let e: Envelope = serde_json::from_slice(bytes)?;
    if e.protocol != 2 || e.id.len() > 128 {
        return Err(Error::new(
            "protocol_version",
            "Unsupported encrypted envelope version",
        ));
    }
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(e.ciphertext)
        .map_err(|_| invalid("Invalid encrypted envelope encoding"))?;
    Ok((
        e.id.clone(),
        crypto::open(key, library, &e.id, &ciphertext).map_err(crypt)?,
    ))
}
pub fn config(c: &Connection) -> Result<Option<Config>> {
    db::meta(c, "sync_config")?
        .map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
pub fn save_config(c: &Connection, config: &Config) -> Result<()> {
    db::put_meta(c, "sync_config", &serde_json::to_string(config)?)
}
const NAMESPACE: &str = "PasteLeaf-v2";
const DESCRIPTOR: &str = "pasteleaf-v2.json";

fn sync_storage_error(error: Error) -> Error {
    match error.code.as_str() {
        "io_not_found" => Error::new(
            "sync_path_missing",
            "The selected sync directory is no longer available. Select an existing directory in iCloud Drive and download it locally before retrying.",
        ),
        "io_permission_denied" => Error::new(
            "sync_access_denied",
            "PasteLeaf cannot read the selected sync directory. Select it again and allow access; check its macOS or Windows permissions before retrying.",
        ),
        "io_not_directory" => Error::new(
            "sync_path_invalid",
            "Select the folder containing the v2 sync library, not a file or a path inside a file.",
        ),
        "pending_hydration" => pending_download(),
        "io" | "io_unavailable" => Error::new(
            "sync_unavailable",
            "The selected sync library could not be read. Check that iCloud Drive is available and the files are downloaded locally, then retry. No library was created.",
        ),
        _ => error,
    }
}
fn pending_download() -> Error {
    Error::new(
        "pending_hydration",
        "The v2 sync library has not finished downloading. Download PasteLeaf-v2 and pasteleaf-v2.json in Finder or File Explorer, wait for iCloud Drive, then retry joining. Do not create a replacement library.",
    )
}
/// Metadata-only probes never turn a missing file into a directory or hide a
/// permissions failure as an absent library (Path::exists would do the latter).
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(sync_storage_error(e.into())),
    }
}
fn existing_directory(path: &Path) -> Result<()> {
    let m = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Windows reports PATH_NOT_FOUND even when a parent component is a file.
            // Inspect existing ancestors read-only so invalid paths are not presented
            // as merely missing cloud folders. Never create a parent while joining.
            for parent in path.ancestors().skip(1) {
                match fs::symlink_metadata(parent) {
                    Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                        return Err(Error::new(
                            "sync_path_invalid",
                            "The selected sync path contains a non-directory component.",
                        ));
                    }
                    Ok(_) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(sync_storage_error(error.into())),
                }
            }
            return Err(sync_storage_error(error.into()));
        }
        Err(error) => return Err(sync_storage_error(error.into())),
    };
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(Error::new(
            "sync_path_invalid",
            "Select an existing ordinary sync folder, not a file or symbolic link.",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        if m.st_flags() & 0x40000000 != 0 {
            return Err(pending_download());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
            return Err(pending_download());
        }
    }
    Ok(())
}
fn missing_library(path: &Path) -> Result<Error> {
    // Older iCloud clients may expose a hidden placeholder instead of the
    // descriptor. Recognize it, but never open it as JSON or hydrate it here.
    if present(&path.join(format!(".{DESCRIPTOR}.icloud")))? {
        return Ok(pending_download());
    }
    let parent = path
        .parent()
        .filter(|_| path.file_name().is_some_and(|n| n == NAMESPACE));
    if let Some(parent) = parent {
        // A folder placeholder takes precedence over a nearby v1 marker.
        if present(&parent.join(format!(".{NAMESPACE}.icloud")))? {
            return Ok(pending_download());
        }
    }
    if present(&path.join("pasteleaf-v1.json"))? {
        return Ok(legacy_library());
    }
    if let Some(parent) = parent {
        if present(&parent.join("pasteleaf-v1.json"))? {
            return Ok(legacy_library());
        }
    }
    Ok(Error::new(
        "sync_library_missing",
        "No pasteleaf-v2.json descriptor was found in this folder or its PasteLeaf-v2 subfolder. Select the existing v2 library folder and wait for iCloud Drive to download it. For a new library, explicitly choose Create on the first device; Join never creates one.",
    ))
}
fn legacy_library() -> Error {
    Error::new(
        "sync_legacy_library",
        "This folder contains a legacy PasteLeaf v1 library, but no v2 descriptor. v2 cannot join or overwrite v1. If another device already created v2, wait for PasteLeaf-v2/pasteleaf-v2.json to download. Otherwise explicitly create a separate v2 library first; import old local history through the separate backup-and-import action.",
    )
}
fn read_descriptor(path: &Path) -> Result<Descriptor> {
    let raw = match files::limited(&path.join(DESCRIPTOR), 8192) {
        Ok(raw) => raw,
        Err(e) if e.code == "io_not_found" => return Err(missing_library(path)?),
        Err(e) if e.code == "invalid_request" => {
            return Err(Error::new(
                "sync_invalid_library",
                "The v2 descriptor must be an ordinary, complete JSON file no larger than 8192 bytes. Download the original library again; do not replace or recreate it.",
            ));
        }
        Err(e) => return Err(sync_storage_error(e)),
    };
    let document: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| {
        if e.is_eof() {
            pending_download()
        } else {
            Error::new("sync_invalid_library", "The v2 descriptor is damaged or not JSON. Select the original library and download it completely before retrying.")
        }
    })?;
    let protocol = document.get("protocol").and_then(serde_json::Value::as_u64)
        .ok_or_else(|| Error::new("sync_invalid_library", "The sync descriptor does not identify its protocol. Select the original complete PasteLeaf library."))?;
    if protocol != 2 {
        return Err(Error::new("protocol_version", "This sync library uses an unsupported protocol. Use compatible clients on both devices; the library was not modified."));
    }
    let descriptor: Descriptor = serde_json::from_value(document).map_err(|_| Error::new(
        "sync_invalid_library", "The v2 descriptor is incomplete or has invalid fields. Download the original library completely before retrying."))?;
    // Match Descriptor::derive's fixed v2 bounds without deriving a password in
    // the read-only preflight. Unknown KDF settings must never reach Argon2.
    if descriptor.memory_kib != 65536 || descriptor.iterations != 3 || descriptor.lanes != 1 {
        return Err(Error::new("protocol_version", "This sync library uses unsupported key parameters. Use compatible clients; no password derivation or library modification was attempted."));
    }
    if !uuid(&descriptor.library) || descriptor.check.len() < 40 {
        return Err(Error::new("sync_invalid_library", "The v2 descriptor contains an invalid library identity or password check. Download the original library completely before retrying."));
    }
    Ok(descriptor)
}
/// Join discovery is deliberately read-only. A directly selected descriptor
/// folder (including a renamed folder) wins over any nested namespace. Otherwise
/// only the immediate PasteLeaf-v2 child is inspected; no recursive guessing.
fn discover(directory: &Path) -> Result<(PathBuf, Descriptor)> {
    if !directory.is_absolute() {
        return Err(Error::new(
            "sync_path_invalid",
            "Select an absolute local iCloud Drive directory.",
        ));
    }
    existing_directory(directory)?;
    let direct = directory.file_name().is_some_and(|n| n == NAMESPACE)
        || present(&directory.join(DESCRIPTOR))?
        || present(&directory.join(format!(".{DESCRIPTOR}.icloud")))?;
    let path = if direct {
        directory.to_path_buf()
    } else {
        directory.join(NAMESPACE)
    };
    match existing_directory(&path) {
        Ok(()) => {}
        Err(e) if e.code == "sync_path_missing" => return Err(missing_library(&path)?),
        Err(e) => return Err(e),
    }
    let path = path
        .canonicalize()
        .map_err(|e| sync_storage_error(e.into()))?;
    let descriptor = read_descriptor(&path)?;
    for sub in ["operations", "attachments", "receipts"] {
        let sub = path.join(sub);
        if present(&sub)? {
            existing_directory(&sub)?;
        }
    }
    Ok((path, descriptor))
}
/// Safe preflight for native folder selection: no password/KDF, writes or sync.
pub fn inspect(directory: &Path) -> Result<serde_json::Value> {
    match discover(directory) {
        Ok((path, _)) => Ok(serde_json::json!({"state":"v2", "directory":path,
            "detail":"A readable v2 descriptor was found. The password and encrypted content have not been verified."})),
        Err(error) => {
            let state = match error.code.as_str() {
                "sync_legacy_library" => "legacy_v1",
                "sync_library_missing" => "missing",
                "pending_hydration" => "pending_download",
                _ => return Err(error),
            };
            Ok(serde_json::json!({"state":state,"detail":error.message}))
        }
    }
}
pub fn connect(
    directory: &Path,
    password: &str,
    create: bool,
    remembered: bool,
) -> Result<(Config, Key)> {
    if !create {
        let (path, descriptor) = discover(directory)?;
        let key = descriptor.unlock(password).map_err(crypt)?;
        return Ok((
            Config {
                directory: path,
                descriptor,
                paused: false,
                remembered,
            },
            key,
        ));
    }
    if !directory.is_absolute() {
        return Err(invalid("Select an absolute local iCloud Drive directory"));
    }
    files::safe_dir(directory)?;
    if present(&directory.join(DESCRIPTOR))?
        || present(&directory.join(format!(".{DESCRIPTOR}.icloud")))?
    {
        return Err(Error::new(
            "sync_exists",
            "The selected folder already contains a v2 library or its download placeholder. Join the existing library instead; creating a nested replacement is not allowed.",
        ));
    }
    let path = if directory.file_name().is_some_and(|n| n == NAMESPACE) {
        directory.to_path_buf()
    } else {
        directory.join(NAMESPACE)
    };
    files::safe_dir(&path)?;
    let path = path.canonicalize()?;
    let descriptor_path = path.join(DESCRIPTOR);
    if descriptor_path.exists() || fs::read_dir(&path)?.next().is_some() {
        return Err(Error::new(
            "sync_exists",
            "The v2 folder is not empty; join it instead or choose a separate folder",
        ));
    }
    let (descriptor, key) = Descriptor::create(password).map_err(crypt)?;
    if !files::publish(&descriptor_path, &serde_json::to_vec(&descriptor)?)? {
        return Err(Error::new(
            "sync_exists",
            "Another device created this library; join it instead",
        ));
    }
    for sub in ["operations", "attachments", "receipts"] {
        files::safe_dir(&path.join(sub))?;
    }
    Ok((
        Config {
            directory: path,
            descriptor,
            paused: false,
            remembered,
        },
        key,
    ))
}
fn quota_publish(path: &Path, raw: &[u8], usage: &mut u64, limit: u64) -> Result<()> {
    if path.exists() {
        files::ordinary(path)?;
        return Ok(());
    }
    if usage.saturating_add(raw.len() as u64) > limit {
        return Err(Error::new(
            "sync_quota",
            "Sync directory limit reached; publishing paused without deleting history",
        ));
    }
    if files::publish(path, raw)? {
        *usage = usage.saturating_add(raw.len() as u64);
    }
    Ok(())
}
fn download_attachments(
    root: &Path,
    cfg: &Config,
    key: &Key,
    record: &Record,
    limit: u64,
) -> Result<bool> {
    let mut complete = true;
    for a in record.attachments() {
        if files::available(root, a) {
            files::verify(root, a)?;
            if record.kind == "image" {
                files::validate_image(&files::limited(&files::blob_path(root, a), FILE_LIMIT)?)?;
            }
            continue;
        }
        let object = object(key, &format!("blob:{}", a.hash));
        let cloud = cfg
            .directory
            .join("attachments")
            .join(format!("{object}.plblob"));
        if !cloud.exists() {
            complete = false;
            continue;
        }
        let encrypted = match files::limited(&cloud, FILE_LIMIT + 40) {
            Ok(bytes) => bytes,
            Err(e) if e.code == "pending_hydration" => {
                complete = false;
                continue;
            }
            Err(e) => return Err(e),
        };
        let raw = crypto::open(key, &cfg.descriptor.library, &object, &encrypted).map_err(crypt)?;
        if raw.len() as u64 != a.size || blake3::hash(&raw).to_hex().as_str() != a.hash {
            return Err(Error::new(
                "attachment_corrupt",
                "Authenticated attachment hash or size did not match",
            ));
        }
        if record.kind == "image" {
            files::validate_image(&raw)?;
        }
        if files::size(root)?.saturating_add(raw.len() as u64) > limit {
            return Err(Error::new(
                "local_quota",
                "Local limit reached; remote attachment remains pending",
            ));
        }
        files::publish(&files::blob_path(root, a), &raw)?;
        files::verify(root, a)?;
    }
    Ok(complete)
}
fn entries(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    files::safe_dir(dir)?;
    let mut paths = Vec::new();
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|s| s.to_str()) == Some(extension) {
            paths.push(p);
        }
    }
    paths.sort();
    Ok(paths)
}
fn signature(path: &Path) -> Result<String> {
    let m = files::ordinary(path)?;
    Ok(format!(
        "{}:{}",
        m.len(),
        m.modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
fn seen(c: &Connection, path: &Path, sig: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT signature FROM cloud_seen WHERE path=?1",
        [path.to_string_lossy()],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .as_deref()
        == Some(sig))
}
fn remember(c: &Connection, path: &Path, sig: &str) -> Result<()> {
    c.execute("INSERT INTO cloud_seen(path,signature) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET signature=excluded.signature",params![path.to_string_lossy(),sig])?;
    Ok(())
}
pub fn exchange(
    c: &mut Connection,
    root: &Path,
    cfg: &Config,
    key: &Key,
    device: &str,
    local_limit: u64,
    sync_limit: u64,
) -> Result<Exchange> {
    let disk: Descriptor = serde_json::from_slice(&files::limited(
        &cfg.directory.join("pasteleaf-v2.json"),
        8192,
    )?)?;
    if serde_json::to_vec(&disk)? != serde_json::to_vec(&cfg.descriptor)? {
        return Err(Error::new(
            "library_changed",
            "Sync library descriptor changed; disconnect and review before reconnecting",
        ));
    }
    disk.verify(key).map_err(crypt)?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_seen(path TEXT PRIMARY KEY,signature TEXT NOT NULL);",
    )?;
    let mut result = Exchange::default();
    // Inbound first; nothing from v1 is read, modified, or reinterpreted here.
    for path in entries(&cfg.directory.join("operations"), "plop")? {
        let sig = match signature(&path) {
            Ok(sig) => sig,
            Err(e) if e.code == "pending_hydration" => {
                result.waiting += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        if seen(c, &path, &sig)? {
            continue;
        }
        let (identity, raw) = unpack(key, &disk.library, &files::limited(&path, OP_LIMIT)?)?;
        let op: Operation = serde_json::from_slice(&raw)?;
        op.validate()?;
        if op.id != identity {
            return Err(Error::new(
                "identity_mismatch",
                "Encrypted operation ID does not match its envelope",
            ));
        }
        let complete = if let Payload::Record(r) = &op.value {
            download_attachments(root, cfg, key, r, local_limit)?
        } else {
            true
        };
        if files::size(root)?.saturating_add(raw.len() as u64) > local_limit
            && db::operation(c, &op.id)?.is_none()
        {
            return Err(Error::new(
                "local_quota",
                "Local limit reached; inbound history remains queued",
            ));
        }
        let tx = c.transaction()?;
        if db::insert(&tx, &op, false)? {
            result.imported += 1;
        }
        db::promote(&tx, root)?;
        if complete {
            remember(&tx, &path, &sig)?;
        } else {
            result.waiting += 1;
        }
        tx.commit()?;
    }
    // Revisit stored operations if only their attachment arrived this cycle.
    let pending = {
        let mut q = c.prepare("SELECT data FROM operations WHERE ready=0")?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for raw in pending {
        let op: Operation = serde_json::from_str(&raw)?;
        if let Payload::Record(r) = op.value {
            download_attachments(root, cfg, key, &r, local_limit)?;
        }
    }
    {
        let tx = c.transaction()?;
        result.waiting += db::promote(&tx, root)?;
        tx.commit()?;
    }
    let mut usage = files::size(&cfg.directory)?;
    let pending = {
        let mut q=c.prepare("SELECT o.data FROM outbox b JOIN operations o ON o.id=b.op WHERE b.published_at IS NULL ORDER BY o.rowid")?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for raw in pending {
        let op: Operation = serde_json::from_str(&raw)?;
        if let Payload::Record(r) = &op.value {
            for a in r.attachments() {
                files::verify(root, a)?;
                let object = object(key, &format!("blob:{}", a.hash));
                let path = cfg
                    .directory
                    .join("attachments")
                    .join(format!("{object}.plblob"));
                if !path.exists() {
                    let encrypted = crypto::seal(
                        key,
                        &disk.library,
                        &object,
                        &files::limited(&files::blob_path(root, a), FILE_LIMIT)?,
                    )
                    .map_err(crypt)?;
                    quota_publish(&path, &encrypted, &mut usage, sync_limit)?;
                } else {
                    let decoded = crypto::open(
                        key,
                        &disk.library,
                        &object,
                        &files::limited(&path, FILE_LIMIT + 40)?,
                    )
                    .map_err(crypt)?;
                    if decoded.len() as u64 != a.size
                        || blake3::hash(&decoded).to_hex().as_str() != a.hash
                    {
                        return Err(Error::new(
                            "attachment_corrupt",
                            "Existing cloud attachment does not match its hash",
                        ));
                    }
                }
            }
        }
        let path = cfg
            .directory
            .join("operations")
            .join(format!("{}.plop", op.id));
        if path.exists() {
            let (_, existing) = unpack(key, &disk.library, &files::limited(&path, OP_LIMIT)?)?;
            let existing: Operation = serde_json::from_slice(&existing)?;
            if existing != op {
                return Err(Error::new(
                    "equivocation",
                    "Published immutable operation differs from local operation",
                ));
            }
        } else {
            quota_publish(
                &path,
                &pack(key, &disk.library, &op.id, raw.as_bytes())?,
                &mut usage,
                sync_limit,
            )?;
        }
        c.execute(
            "UPDATE outbox SET published_at=?2 WHERE op=?1",
            params![op.id, now()],
        )?;
        remember(c, &path, &signature(&path)?)?;
        result.sent += 1;
    }
    let receipts = {
        let mut q=c.prepare("SELECT r.op FROM receipt_outbox r JOIN operations o ON o.id=r.op WHERE r.published_at IS NULL AND json_extract(o.data,'$.device')!=?1")?;
        let rows = q
            .query_map([device], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for op in receipts {
        let receipt = Receipt {
            protocol: 2,
            device: device.into(),
            op: op.clone(),
            imported_at: now(),
        };
        let identity = object(key, &format!("receipt:{device}:{op}"));
        let path = cfg
            .directory
            .join("receipts")
            .join(format!("{identity}.plreceipt"));
        quota_publish(
            &path,
            &pack(
                key,
                &disk.library,
                &identity,
                &serde_json::to_vec(&receipt)?,
            )?,
            &mut usage,
            sync_limit,
        )?;
        c.execute(
            "UPDATE receipt_outbox SET published_at=?2 WHERE op=?1",
            params![op, now()],
        )?;
    }
    for path in entries(&cfg.directory.join("receipts"), "plreceipt")? {
        let sig = match signature(&path) {
            Ok(sig) => sig,
            Err(e) if e.code == "pending_hydration" => {
                result.waiting += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        if seen(c, &path, &sig)? {
            continue;
        }
        let (identity, raw) = unpack(key, &disk.library, &files::limited(&path, 8192)?)?;
        let r: Receipt = serde_json::from_slice(&raw)?;
        if r.protocol != 2
            || !uuid(&r.device)
            || !uuid(&r.op)
            || r.imported_at < 0
            || identity != object(key, &format!("receipt:{}:{}", r.device, r.op))
        {
            return Err(invalid("Invalid authenticated receipt"));
        }
        if r.device != device {
            c.execute(
                "INSERT OR IGNORE INTO receipts(device,op,imported_at) VALUES(?1,?2,?3)",
                params![r.device, r.op, r.imported_at],
            )?;
            result.receipts += 1;
        }
        remember(c, &path, &sig)?;
    }
    db::put_meta(c, "last_exchange_ms", &now().to_string())?;
    Ok(result)
}
