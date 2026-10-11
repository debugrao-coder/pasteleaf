//! Passive presentation of frozen conflict payloads. Never exposes local file paths,
//! evaluates markup, or chooses a version on the user's behalf.
use crate::model::{Category, Item};
use base64::Engine;
use serde_json::{json, Value};

pub const CONFLICT_VERSIONS_PER_PAGE: usize = 2;

pub fn byte_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    }
}

pub fn content_metadata(item: &Item) -> String {
    match item.kind.as_str() {
        "image" => match &item.image {
            Some(image) => {
                let format = match image
                    .name
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "png" => "PNG 图片",
                    "jpg" | "jpeg" => "JPEG 图片",
                    _ => "图片",
                };
                format!("{format} · {}", byte_size(image.size))
            }
            None => "图片 · 附件信息待获取".into(),
        },
        "files" => format!(
            "{} 个文件 · {}",
            item.files.len(),
            byte_size(
                item.files
                    .iter()
                    .fold(0u64, |n, f| n.saturating_add(f.size))
            )
        ),
        "rich_text" => format!("富文本 · {} 字符", item.text.chars().count()),
        _ => format!("{} 字符", item.text.chars().count()),
    }
}

fn differs(versions: &[Value], value: impl Fn(&Value) -> Value) -> bool {
    versions.first().is_some_and(|first| {
        versions
            .iter()
            .skip(1)
            .any(|next| value(first) != value(next))
    })
}

fn attachment_signature(attachment: &Value) -> Value {
    // Deliberately omit device-local paths from comparison and presentation.
    json!({"name":attachment["name"],"size":attachment["size"],"hash":attachment["hash"]})
}

pub fn conflict_changes(conflict: &Value) -> String {
    let versions = conflict["versions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if versions.len() < 2 {
        return "未提供可比较的多个版本，请刷新冲突列表。".into();
    }
    if versions.iter().any(|v| {
        v["deleted"].as_bool() != Some(true)
            && !v["record"].is_object()
            && !v["category"].is_object()
    }) {
        return "版本内容不完整，无法确认差异；请勿仅凭设备标识决定保留。".into();
    }
    let mut fields = Vec::new();
    if differs(versions, |v| json!(v["deleted"].as_bool().unwrap_or(false))) {
        fields.push("删除状态");
    }
    for (field, label) in [
        ("kind", "内容类型"),
        ("tags", "标签"),
        ("category", "收藏分类"),
        ("text", "正文"),
        ("html", "HTML 格式"),
        ("rtf", "RTF 格式"),
        ("source", "来源应用"),
        ("copied_at", "记录时间"),
    ] {
        if differs(versions, |v| v["record"][field].clone()) {
            fields.push(label);
        }
    }
    if differs(versions, |v| {
        Value::Array(
            v["record"]["files"]
                .as_array()
                .map(|a| a.iter().map(attachment_signature).collect())
                .unwrap_or_default(),
        )
    }) {
        fields.push("文件附件");
    }
    if differs(versions, |v| attachment_signature(&v["record"]["image"])) {
        fields.push("图片附件");
    }
    if differs(versions, |v| v["category"]["name"].clone()) {
        fields.push("分类名称");
    }
    if differs(versions, |v| v["category"]["color"].clone()) {
        fields.push("分类颜色");
    }
    if fields.is_empty() {
        "内容字段相同；来源操作不同，请核对版本。".into()
    } else {
        format!("差异：{}", fields.join("、"))
    }
}

fn preview(text: &str, limit: usize) -> String {
    let value: String = text.chars().take(limit).collect();
    if value.len() == text.len() {
        value
    } else {
        format!(
            "{value}\n…（预览已截取；完整内容 BLAKE3：{}）",
            blake3::hash(text.as_bytes())
        )
    }
}

fn category_name(id: Option<&str>, categories: &[Category]) -> String {
    match id {
        None => "未收藏".into(),
        Some(id) => categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| format!("{}（{id}）", c.name))
            .unwrap_or_else(|| format!("未知或已删除分类（{id}）")),
    }
}

fn attachment_details(attachment: &Value) -> String {
    format!(
        "{} · {}\n内容 BLAKE3：{}",
        attachment["name"].as_str().unwrap_or("名称未提供"),
        attachment["size"]
            .as_u64()
            .map(byte_size)
            .unwrap_or_else(|| "大小未提供".into()),
        attachment["hash"].as_str().unwrap_or("未提供")
    )
}

pub fn conflict_version_details(version: &Value, categories: &[Category]) -> String {
    let mut lines = Vec::new();
    if version["deleted"].as_bool() == Some(true) {
        lines.push("状态：已删除\n选择此版本将保留删除状态，不保留其他版本的记录内容。".into());
    } else if version["category"].is_object() {
        let category = &version["category"];
        lines.push(format!(
            "状态：保留分类\n分类名称：{}\n分类颜色：{}\n分类标识：{}",
            category["name"].as_str().unwrap_or("未提供"),
            category["color"].as_str().unwrap_or("未提供"),
            category["id"].as_str().unwrap_or("未提供")
        ));
    } else if version["record"].is_object() {
        let record = &version["record"];
        lines.push("状态：保留记录".into());
        lines.push(format!(
            "标签：{}",
            match record["tags"].as_array() {
                Some(tags) if tags.is_empty() => "无标签".into(),
                Some(tags) => serde_json::to_string(tags).unwrap_or_else(|_| "格式无效".into()),
                None => "未提供".into(),
            }
        ));
        lines.push(format!(
            "收藏分类：{}",
            category_name(record["category"].as_str(), categories)
        ));
        if let Ok(item) = serde_json::from_value::<Item>(record.clone()) {
            lines.push(format!(
                "内容：{} · {}",
                item.type_name(),
                content_metadata(&item)
            ));
        }
        lines.push(format!(
            "来源应用：{}",
            record["source"].as_str().unwrap_or("未提供")
        ));
        if let Some(time) = record["copied_at"].as_i64() {
            lines.push(format!("记录时间（Unix ms）：{time}"));
        }
        if record["image"].is_object() {
            lines.push(format!(
                "图片附件：\n{}",
                attachment_details(&record["image"])
            ));
        }
        if let Some(files) = record["files"].as_array().filter(|files| !files.is_empty()) {
            for (index, attachment) in files.iter().enumerate() {
                lines.push(format!(
                    "文件附件 {}：\n{}",
                    index + 1,
                    attachment_details(attachment)
                ));
            }
        }
        if matches!(record["kind"].as_str(), Some("image" | "files")) {
            lines.push(if record["available"].as_bool() == Some(true) {
                "可用状态：附件已到达；粘贴前仍会校验内容。".into()
            } else {
                "可用状态：等待附件，暂不可粘贴。".into()
            });
        }
        if let Some(text) = record["text"].as_str().filter(|text| !text.is_empty()) {
            lines.push(format!("正文：\n{}", preview(text, 2048)));
        }
        match record["html"].as_str() {
            Some(html) => lines.push(format!(
                "HTML（仅源码，不执行）：\nBLAKE3：{}\n{}",
                blake3::hash(html.as_bytes()),
                preview(html, 2048)
            )),
            None if record["kind"] == "rich_text" => lines.push("HTML：无".into()),
            None => {}
        }
        match record["rtf"].as_str() {
            Some(rtf) => lines.push(
                match base64::engine::general_purpose::STANDARD.decode(rtf) {
                    Ok(bytes) => format!(
                        "RTF：{} · BLAKE3：{}",
                        byte_size(bytes.len() as u64),
                        blake3::hash(&bytes)
                    ),
                    Err(_) => "RTF：编码无效，不能视为有效富文本。".into(),
                },
            ),
            None if record["kind"] == "rich_text" => lines.push("RTF：无".into()),
            None => {}
        }
    } else {
        lines.push("版本内容未提供或格式未知；请勿仅凭设备标识决定保留。".into());
    }
    lines.push(format!(
        "设备：{}\n操作：{}",
        version["device"].as_str().unwrap_or("未知"),
        version["op_id"].as_str().unwrap_or("未知")
    ));
    // Native multiline EDIT consumes CRLF, and all content remains passive text.
    lines.join("\n").replace("\r\n", "\n").replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text_version(tag: &str) -> Value {
        json!({"op_id":tag,"device":"synthetic-device","deleted":false,"record":{"kind":"text","text":"相同的正文","tags":[tag],"category":null}})
    }

    #[test]
    fn tags_only_conflict_shows_actual_tags_not_just_identical_body() {
        let first = text_version("工作");
        let second = text_version("旅行");
        assert_eq!(
            conflict_changes(&json!({"versions":[first,second]})),
            "差异：标签"
        );
        let a = conflict_version_details(&first, &[]);
        let b = conflict_version_details(&second, &[]);
        assert!(a.contains("标签：[\"工作\"]"));
        assert!(b.contains("标签：[\"旅行\"]"));
        assert_ne!(a, b);
    }
    #[test]
    fn category_assignment_resolves_name_but_retains_stable_id() {
        let mut value = text_version("工作");
        value["record"]["category"] = json!("folder-1");
        let categories = vec![Category {
            id: "folder-1".into(),
            name: "产品灵感".into(),
            color: String::new(),
        }];
        assert!(conflict_version_details(&value, &categories).contains("产品灵感（folder-1）"));
        assert!(conflict_version_details(&value, &[]).contains("未知或已删除分类（folder-1）"));
    }
    #[test]
    fn category_entities_show_name_and_color() {
        let a = json!({"category":{"id":"folder-1","name":"工作","color":"#347A58"}});
        let b = json!({"category":{"id":"folder-1","name":"休息","color":"#497BC1"}});
        assert_eq!(
            conflict_changes(&json!({"versions":[a,b]})),
            "差异：分类名称、分类颜色"
        );
        let detail = conflict_version_details(&a, &[]);
        assert!(detail.contains("分类名称：工作"));
        assert!(detail.contains("分类颜色：#347A58"));
    }
    #[test]
    fn deletion_is_explicit_and_does_not_show_stale_body() {
        let mut deleted = text_version("旧标签");
        deleted["deleted"] = json!(true);
        let detail = conflict_version_details(&deleted, &[]);
        assert!(detail.contains("状态：已删除"));
        assert!(detail.contains("保留删除状态"));
        assert!(!detail.contains("相同的正文"));
        assert!(
            conflict_changes(&json!({"versions":[text_version("旧标签"),deleted]}))
                .contains("删除状态")
        );
    }
    #[test]
    fn attachment_hash_differences_are_visible_without_source_paths() {
        let mut a = text_version("文档");
        a["record"]["kind"] = json!("files");
        a["record"]["files"] =
            json!([{"name":"需求.txt","size":2048,"hash":"aaaa","path":"C:\\private\\source.txt"}]);
        let mut b = a.clone();
        b["record"]["files"][0]["hash"] = json!("bbbb");
        assert!(conflict_changes(&json!({"versions":[a,b]})).contains("文件附件"));
        let detail = conflict_version_details(&a, &[]);
        assert!(detail.contains("需求.txt · 2.0 KiB"));
        assert!(detail.contains("BLAKE3：aaaa"));
        assert!(!detail.contains("private"));
        b = a.clone();
        b["record"]["files"][0]["path"] = json!("D:\\other.txt");
        assert!(!conflict_changes(&json!({"versions":[a,b]})).contains("文件附件"));
    }
    #[test]
    fn rich_text_same_plain_body_exposes_format_fingerprint() {
        let mut a = text_version("样式");
        a["record"]["kind"] = json!("rich_text");
        a["record"]["html"] = json!("<b>相同的正文</b>");
        let mut b = a.clone();
        b["record"]["html"] = json!("<i>相同的正文</i>");
        assert!(conflict_changes(&json!({"versions":[a,b]})).contains("HTML 格式"));
        let detail = conflict_version_details(&a, &[]);
        assert!(detail.contains("<b>相同的正文</b>"));
        assert!(detail.contains("BLAKE3："));
        assert_ne!(detail, conflict_version_details(&b, &[]));
    }
    #[test]
    fn rtf_format_conflict_uses_original_bytes_and_invalid_data_is_explicit() {
        let mut a = text_version("样式");
        a["record"]["kind"] = json!("rich_text");
        let rtf = b"{\\rtf1\\b synthetic}";
        a["record"]["rtf"] = json!(base64::engine::general_purpose::STANDARD.encode(rtf));
        let mut b = a.clone();
        b["record"]["rtf"] =
            json!(base64::engine::general_purpose::STANDARD.encode(b"{\\rtf1\\i synthetic}"));
        assert!(conflict_changes(&json!({"versions":[a,b]})).contains("RTF 格式"));
        assert!(conflict_version_details(&a, &[]).contains(&blake3::hash(rtf).to_string()));
        b["record"]["rtf"] = json!("invalid base64");
        assert!(conflict_version_details(&b, &[]).contains("RTF：编码无效"));
    }
    #[test]
    fn image_metadata_uses_bytes_not_zero_character_count() {
        let item:Item=serde_json::from_value(json!({"kind":"image","text":"","image":{"name":"photo.JPEG","size":4096,"hash":"synthetic"}})).unwrap();
        assert_eq!(content_metadata(&item), "JPEG 图片 · 4.0 KiB");
        let missing = Item {
            kind: "image".into(),
            ..Default::default()
        };
        assert_eq!(content_metadata(&missing), "图片 · 附件信息待获取");
        assert!(!content_metadata(&item).contains("字符"));
    }
    #[test]
    fn image_attachment_changed_bytes_are_identifiable() {
        let a = json!({"record":{"kind":"image","image":{"name":"photo.png","size":1024,"hash":"aaaa"},"available":false}});
        let mut b = a.clone();
        b["record"]["image"]["hash"] = json!("bbbb");
        assert!(conflict_changes(&json!({"versions":[a,b]})).contains("图片附件"));
        let detail = conflict_version_details(&a, &[]);
        assert!(detail.contains("图片附件："));
        assert!(detail.contains("BLAKE3：aaaa"));
        assert!(detail.contains("等待附件"));
        assert!(!detail.contains("0 字符"));
    }
    #[test]
    fn truncated_unicode_body_keeps_full_content_identity() {
        let a = "中文🙂".repeat(1000);
        let b = format!("{a}不同结尾");
        assert!(preview(&a, 20).contains("完整内容 BLAKE3"));
        assert_ne!(preview(&a, 20), preview(&b, 20));
        assert!(conflict_version_details(&json!({}), &[]).contains("版本内容未提供"));
        assert!(conflict_changes(&json!({"versions":[{},{}]})).contains("版本内容不完整"));
    }
}
