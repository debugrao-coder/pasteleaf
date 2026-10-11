//! Platform-independent presentation state; never stores passwords or clipboard payloads in logs.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Attachment {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub hash: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub html: Option<String>,
    pub rtf: Option<String>,
    pub image_path: Option<String>,
    pub image: Option<Attachment>,
    pub files: Vec<Attachment>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub source: String,
    pub copied_at: i64,
    pub available: bool,
}
impl Item {
    pub fn title(&self) -> String {
        if self.kind == "files" {
            self.files
                .first()
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "文件附件".into())
        } else if self.kind == "image" {
            "图片".into()
        } else {
            self.text
                .lines()
                .find(|s| !s.trim().is_empty())
                .unwrap_or("富文本")
                .chars()
                .take(96)
                .collect()
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self.kind.as_str() {
            "image" => "图片",
            "files" => "文件",
            "rich_text" => "富文本",
            "link" => "链接",
            _ if self.text.starts_with("https://") || self.text.starts_with("http://") => "链接",
            _ => "文字",
        }
    }
    pub fn accessible_name(&self) -> String {
        // Bound names while keeping native screen-reader access to the displayed content.
        format!(
            "{} · {} · {}{}",
            self.type_name(),
            self.title(),
            self.source,
            if self.available {
                ""
            } else {
                " · 等待附件"
            }
        )
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
}
#[derive(Clone, Debug)]
pub enum PastePayload {
    Item(Box<Item>),
    Text(String),
    Files(Vec<String>),
}
#[derive(Default)]
pub struct ViewModel {
    pub items: Vec<Item>,
    pub total: usize,
    pub query: String,
    pub category: Option<String>,
    pub offset: usize,
    pub selected: BTreeSet<String>,
    pub focused: Option<String>,
    pub request_generation: u64,
    pub queue: VecDeque<Item>,
    pub paused: bool,
    pub status: String,
    pub categories: Vec<Category>,
    pub settings: Value,
    pub loading: bool,
    pub error: Option<String>,
}
impl ViewModel {
    pub fn request_list(&mut self) -> (u64, Value) {
        self.request_generation += 1;
        self.loading = true;
        (
            self.request_generation,
            json!({"op":"list","query":self.query,"category":self.category,"offset":self.offset,"limit":64}),
        )
    }
    pub fn apply_list(&mut self, generation: u64, data: Value) -> bool {
        if generation != self.request_generation {
            return false;
        }
        let parsed: Vec<Item> = match serde_json::from_value(data["items"].clone()) {
            Ok(v) => v,
            Err(_) => {
                self.error = Some("历史记录响应格式无效；请重试".into());
                self.loading = false;
                return false;
            }
        };
        self.items = parsed;
        self.total = data["total"].as_u64().unwrap_or(self.items.len() as u64) as usize;
        self.selected
            .retain(|id| self.items.iter().any(|i| &i.id == id));
        if !self
            .items
            .iter()
            .any(|i| Some(&i.id) == self.focused.as_ref())
        {
            self.focused = self.items.first().map(|i| i.id.clone());
        }
        self.loading = false;
        self.error = None;
        true
    }
    pub fn select(&mut self, index: usize, extend: bool) {
        if let Some(item) = self.items.get(index) {
            if !extend {
                self.selected.clear();
            }
            if extend && self.selected.contains(&item.id) {
                self.selected.remove(&item.id);
            } else {
                self.selected.insert(item.id.clone());
            }
            self.focused = Some(item.id.clone());
        }
    }
    pub fn selected_items(&self) -> Vec<Item> {
        let ids = if self.selected.is_empty() {
            self.focused.clone().into_iter().collect()
        } else {
            self.selected.clone()
        };
        self.items
            .iter()
            .filter(|i| ids.contains(&i.id))
            .cloned()
            .collect()
    }
    pub fn prepare_paste(&mut self, plain: bool) -> Result<PastePayload, String> {
        let items = self.selected_items();
        if items.is_empty() {
            return Err("请先选择一条记录".into());
        }
        if items.iter().any(|i| {
            !matches!(
                i.kind.as_str(),
                "text" | "link" | "rich_text" | "image" | "files"
            )
        }) {
            return Err("此内容类型需要新版客户端，未修改系统剪贴板".into());
        }
        if items.iter().any(|i| !i.available) {
            return Err("附件尚未完整到达，请稍后重试".into());
        }
        if plain
            || items
                .iter()
                .all(|i| matches!(i.kind.as_str(), "text" | "link" | "rich_text"))
        {
            if plain && items.iter().any(|i| i.text.is_empty()) {
                return Err("所选内容没有可粘贴的纯文本".into());
            }
            if plain || items.len() > 1 {
                return Ok(PastePayload::Text(
                    items
                        .iter()
                        .map(|i| i.text.clone())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
            }
        }
        if items.iter().all(|i| i.kind == "files") {
            return Ok(PastePayload::Files(
                items
                    .iter()
                    .flat_map(|i| i.files.iter().map(|f| f.path.clone()))
                    .collect(),
            ));
        }
        if items.len() > 1 {
            self.queue = items.into();
            return self.next_paste();
        }
        Ok(PastePayload::Item(Box::new(items[0].clone())))
    }
    pub fn next_paste(&mut self) -> Result<PastePayload, String> {
        self.queue
            .pop_front()
            .map(|item| PastePayload::Item(Box::new(item)))
            .ok_or_else(|| "顺序队列已完成".into())
    }
}

pub fn build_cf_html(fragment: &str) -> String {
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let header="Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
    let start = header.len();
    let frag = start + prefix.len();
    let end_frag = frag + fragment.len();
    let end = end_frag + suffix.len();
    format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{frag:010}\r\nEndFragment:{end_frag:010}\r\n{prefix}{fragment}{suffix}")
}
pub fn extract_cf_html(value: &str) -> Option<String> {
    let offset = |key: &str| -> Option<usize> {
        value
            .lines()
            .find_map(|line| line.strip_prefix(key).and_then(|s| s.trim().parse().ok()))
    };
    let start = offset("StartFragment:")?;
    let end = offset("EndFragment:")?;
    if end < start {
        return None;
    }
    value.get(start..end).map(str::to_owned)
}
pub fn parse_hotkey(input: &str) -> Result<(u32, u32), String> {
    let mut mods = 0u32;
    let mut key = None;
    for part in input.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= 2,
            "alt" => mods |= 1,
            "shift" => mods |= 4,
            "win" | "super" => mods |= 8,
            _ => {
                if key.is_some() {
                    return Err("快捷键只能包含一个主键".into());
                }
                let upper = part.to_ascii_uppercase();
                key = if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() {
                    Some(upper.as_bytes()[0] as u32)
                } else if let Some(n) = upper.strip_prefix('F').and_then(|s| s.parse::<u32>().ok())
                {
                    if (1..=24).contains(&n) {
                        Some(0x70 + n - 1)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if key.is_none() {
                    return Err("使用 Ctrl / Alt / Shift / Win 加字母、数字或 F1–F24".into());
                }
            }
        }
    }
    if mods == 0 || key.is_none() {
        return Err("请设置至少一个修饰键和一个主键".into());
    }
    Ok((mods, key.unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str, kind: &str) -> Item {
        Item {
            id: id.into(),
            kind: kind.into(),
            text: format!("内容{id}"),
            available: true,
            ..Default::default()
        }
    }
    #[test]
    fn rejects_stale_search() {
        let mut m = ViewModel::default();
        let (a, _) = m.request_list();
        let (b, _) = m.request_list();
        assert!(!m.apply_list(a, json!({"items":[],"total":0})));
        assert!(m.apply_list(b, json!({"items":[],"total":0})));
    }
    #[test]
    fn refresh_preserves_selection() {
        let mut m = ViewModel {
            items: vec![item("a", "text")],
            ..Default::default()
        };
        m.select(0, false);
        let (g, _) = m.request_list();
        m.apply_list(
            g,
            json!({"items":[item("b","text"),item("a","text")],"total":2}),
        );
        assert_eq!(m.focused.as_deref(), Some("a"));
        assert!(m.selected.contains("a"));
    }
    #[test]
    fn mixed_queue_only_advances_explicitly() {
        let mut m = ViewModel {
            items: vec![item("a", "text"), item("b", "image")],
            ..Default::default()
        };
        m.select(0, false);
        m.select(1, true);
        assert!(matches!(
            m.prepare_paste(false).unwrap(),
            PastePayload::Item(_)
        ));
        assert_eq!(m.queue.len(), 1);
        assert!(m.next_paste().is_ok());
        assert!(m.next_paste().is_err());
    }
    #[test]
    fn unavailable_attachment_is_never_pasted() {
        let mut m = ViewModel::default();
        let mut i = item("a", "files");
        i.available = false;
        m.items.push(i);
        m.select(0, false);
        assert!(m.prepare_paste(false).is_err());
    }
    #[test]
    fn multiple_texts_join_in_display_order() {
        let mut m = ViewModel {
            items: vec![item("b", "text"), item("a", "rich_text")],
            ..Default::default()
        };
        m.select(0, false);
        m.select(1, true);
        match m.prepare_paste(false).unwrap() {
            PastePayload::Text(s) => assert_eq!(s, "内容b\n内容a"),
            _ => panic!(),
        }
    }
    #[test]
    fn cf_html_offsets_are_utf8_bytes() {
        let f = "<b>中文🙂</b>";
        let s = build_cf_html(f);
        assert_eq!(extract_cf_html(&s).as_deref(), Some(f));
        assert!(extract_cf_html("StartFragment:99999\nEndFragment:100000").is_none());
    }
    #[test]
    fn unknown_kind_is_not_silently_downgraded() {
        let mut m = ViewModel {
            items: vec![item("a", "future_format")],
            ..Default::default()
        };
        m.select(0, false);
        assert!(m.prepare_paste(false).is_err());
    }
    #[test]
    fn pagination_remains_bounded_with_100k_records() {
        let mut m = ViewModel {
            total: 100_000,
            offset: 99_968,
            ..Default::default()
        };
        let (_, v) = m.request_list();
        assert_eq!(v["limit"], 64);
        assert_eq!(v["offset"], 99_968);
    }
    #[test]
    fn hotkey_validation() {
        assert_eq!(parse_hotkey("Ctrl+Q").unwrap(), (2, 81));
        assert_eq!(parse_hotkey("Alt+Shift+F12").unwrap(), (5, 123));
        assert!(parse_hotkey("Q").is_err());
        assert!(parse_hotkey("Ctrl+QQ").is_err());
        assert!(parse_hotkey("Ctrl+A+B").is_err());
    }
}
