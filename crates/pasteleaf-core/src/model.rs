use crate::error::{invalid, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
pub const DEFAULT_CATEGORY: &str = "726ce7b5-c6df-4d40-84fe-a05dc3ca430d";
pub const DEFAULT_QUOTA: u64 = 10 * 1024 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 100 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 16 * 1024 * 1024;
pub const OP_LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub name: String,
    pub size: u64,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf: Option<String>,
    #[serde(default)]
    pub files: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Attachment>,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: String,
    pub copied_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
// Most operations contain records. Keep the wire model allocation-free for small
// categories/deletions instead of adding an allocation to every record read.
#[allow(clippy::large_enum_variant)]
pub enum Payload {
    Record(Record),
    Category(Category),
    Deleted,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub protocol: u32,
    pub id: String,
    pub entity: String,
    pub device: String,
    pub parents: BTreeSet<String>,
    pub value: Payload,
}
pub fn uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn default_settings() -> Value {
    json!({"paused":true,"onboarding_complete":false,"excluded_apps":[],
        "local_limit_bytes":DEFAULT_QUOTA,"sync_limit_bytes":DEFAULT_QUOTA,
        "local_quota_bytes":DEFAULT_QUOTA,"sync_quota_bytes":DEFAULT_QUOTA,
        "max_file_bytes":FILE_LIMIT,"appearance":"system",
        "hotkey":if cfg!(target_os="macos") {"option+command+v"} else {"Ctrl+Q"},
        "launch_at_login":false,"reduce_motion":false,"reduce_transparency":false})
}
pub fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 240
        && !s.contains(['/', '\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        && !s.ends_with(['.', ' '])
        && !matches!(
            s.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
        && s != "."
        && s != ".."
        && !s.chars().any(char::is_control)
}
impl Attachment {
    pub fn validate(&self) -> Result<()> {
        if !valid_name(&self.name) || !valid_hash(&self.hash) || self.size > FILE_LIMIT {
            return Err(invalid("Attachment metadata is invalid or exceeds 100 MiB"));
        }
        Ok(())
    }
}
impl Record {
    pub fn attachments(&self) -> impl Iterator<Item = &Attachment> {
        self.files.iter().chain(self.image.iter())
    }
    pub fn validate(&self) -> Result<()> {
        if !uuid(&self.id)
            || self.category.as_ref().is_some_and(|s| !uuid(s))
            || self.source.len() > 4096
            || self.copied_at < 0
            || self.text.len()
                + self.html.as_ref().map_or(0, String::len)
                + self.rtf.as_ref().map_or(0, String::len)
                > TEXT_LIMIT
        {
            return Err(invalid(
                "Record metadata exceeds limits or contains invalid identifiers",
            ));
        }
        if self.tags.len() > 64
            || self
                .tags
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        {
            return Err(invalid("Use at most 64 tags of 128 bytes each"));
        }
        if let Some(rtf) = &self.rtf {
            base64::engine::general_purpose::STANDARD
                .decode(rtf)
                .map_err(|_| invalid("RTF must be base64 of original bytes"))?;
        }
        if self.files.len() > 128 {
            return Err(invalid("At most 128 ordinary files per clipboard item"));
        }
        for a in self.attachments() {
            a.validate()?;
        }
        match self.kind.as_str() {
            "text"
                if self.image.is_none()
                    && self.files.is_empty()
                    && self.html.is_none()
                    && self.rtf.is_none() => {}
            "rich_text"
                if self.image.is_none()
                    && self.files.is_empty()
                    && (self.html.is_some() || self.rtf.is_some()) => {}
            "image"
                if self.image.is_some()
                    && self.files.is_empty()
                    && self.html.is_none()
                    && self.rtf.is_none() => {}
            "files"
                if !self.files.is_empty()
                    && self.image.is_none()
                    && self.html.is_none()
                    && self.rtf.is_none() => {}
            _ => return Err(invalid("Unsupported or inconsistent content type")),
        }
        Ok(())
    }
    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {}",
            self.text,
            self.source,
            self.tags.join(" "),
            self.files
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase()
    }
    pub fn fingerprint(&self) -> String {
        blake3::hash(
            serde_json::to_vec(&json!([
                self.kind,
                self.text,
                self.html,
                self.rtf,
                self.files,
                self.image.as_ref().map(|a| (&a.hash, a.size))
            ]))
            .expect("serializable record")
            .as_slice(),
        )
        .to_hex()
        .to_string()
    }
}
impl Category {
    pub fn validate(&self) -> Result<()> {
        if !uuid(&self.id)
            || self.name.trim().is_empty()
            || self.name.len() > 240
            || self.name.chars().any(char::is_control)
            || self.color.len() != 7
            || !self.color.starts_with('#')
            || !self.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid(
                "Invalid category name, identifier or #RRGGBB color",
            ));
        }
        Ok(())
    }
}
impl Operation {
    pub fn validate(&self) -> Result<()> {
        if self.protocol != 2
            || !uuid(&self.id)
            || !uuid(&self.device)
            || self.parents.len() > 1024
            || self.parents.contains(&self.id)
            || self.parents.iter().any(|p| !uuid(p))
        {
            return Err(invalid(
                "Unsupported operation version or causal identifiers",
            ));
        }
        let (kind, entity) = self
            .entity
            .split_once(':')
            .ok_or_else(|| invalid("Invalid entity"))?;
        if !uuid(entity) {
            return Err(invalid("Invalid entity identifier"));
        }
        match &self.value {
            Payload::Record(r) if kind == "item" && r.id == entity => r.validate(),
            Payload::Category(c) if kind == "category" && c.id == entity => c.validate(),
            Payload::Deleted if kind == "item" || kind == "category" => Ok(()),
            _ => Err(invalid("Operation payload does not match its entity")),
        }
    }
}
