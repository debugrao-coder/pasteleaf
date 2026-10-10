use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    Text(String),
    Image {
        file: String,
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub content: Content,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub copied_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    #[serde(default)]
    pub sync: crate::sync::State,
    #[serde(default)]
    pub theme: crate::theme::Theme,
    pub version: u32,
    pub items: Vec<Item>,
    pub categories: Vec<String>,
    pub hotkey: String,
    pub paused: bool,
    pub next_id: u64,
    #[serde(default)]
    pub category_colors: std::collections::HashMap<String, u32>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            sync: Default::default(),
            theme: Default::default(),
            version: 1,
            items: vec![],
            categories: vec!["收藏".into()],
            hotkey: "Ctrl+Q".into(),
            paused: false,
            next_id: 1,
            category_colors: Default::default(),
        }
    }
}

impl Model {
    pub fn add_favorite_text(&mut self, text: &str, category: &str) -> Result<u64, String> {
        if !self.categories.iter().any(|name| name == category) {
            return Err("分类不存在".into());
        }
        if text.trim().is_empty() {
            return Err("请输入收藏内容".into());
        }
        let content = Content::Text(text.into());
        content.validate()?;
        let text_bytes = self
            .items
            .iter()
            .try_fold(content.text_len(), |total, item| {
                total.checked_add(item.content.text_len())
            })
            .ok_or("文字总量达到 64 MiB，请清理部分记录")?;
        if text_bytes > 64 * 1024 * 1024 {
            return Err("文字总量达到 64 MiB，请清理部分记录".into());
        }
        let id = self.next_id;
        let next = id.checked_add(1).ok_or("记录编号已耗尽")?;
        if id == 0 || self.items.iter().any(|item| item.id >= id) {
            return Err("记录编号无效".into());
        }
        let evict = if self.items.len() >= 500 {
            Some(
                self.items
                    .iter()
                    .rposition(|item| item.category.is_none())
                    .ok_or("已达到 500 条收藏上限，请清理部分内容")?,
            )
        } else {
            None
        };
        if let Some(index) = evict {
            self.items.remove(index);
        }
        self.next_id = next;
        self.items.insert(
            0,
            Item {
                id,
                content,
                category: Some(category.into()),
                tags: Vec::new(),
                source: "手动添加".into(),
                copied_at: 0,
            },
        );
        Ok(id)
    }

    pub fn capture(&mut self, content: Content) -> Result<u64, String> {
        content.validate()?;
        if let Some(index) = self.items.iter().position(|x| x.content == content) {
            let item = self.items.remove(index);
            let id = item.id;
            self.items.insert(0, item);
            return Ok(id);
        }
        let text_bytes: usize = self.items.iter().map(|x| x.content.text_len()).sum();
        if text_bytes + content.text_len() > 64 * 1024 * 1024 {
            return Err("文字总量达到 64 MiB，请清理部分记录".into());
        }
        if self.items.len() >= 500 {
            let index = self
                .items
                .iter()
                .rposition(|x| x.category.is_none())
                .ok_or("已达到 500 条收藏上限，请清理部分内容")?;
            self.items.remove(index);
        }
        let next = self.next_id.checked_add(1).ok_or("记录编号已耗尽")?;
        let id = self.next_id;
        self.next_id = next;
        self.items.insert(
            0,
            Item {
                id,
                content,
                category: None,
                tags: Vec::new(),
                source: String::new(),
                copied_at: 0,
            },
        );
        let mut history = 0;
        self.items.retain(|x| {
            if x.category.is_some() {
                true
            } else {
                history += 1;
                history <= 200
            }
        });
        Ok(id)
    }
    /// Update descriptive metadata without changing the clipboard payload.
    pub fn set_tags(&mut self, id: u64, input: &str) -> Result<(), String> {
        let mut tags = Vec::new();
        for tag in input
            .split([',', '，'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if tags
                .iter()
                .any(|s: &String| s.to_lowercase() == tag.to_lowercase())
            {
                continue;
            }
            tags.push(tag.to_string());
        }
        validate_tags(&tags)?;
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or("记录不存在")?
            .tags = tags;
        Ok(())
    }
    pub fn assign(&mut self, id: u64, category: Option<String>) -> Result<(), String> {
        if let Some(c) = &category {
            if !self.categories.contains(c) {
                return Err("分类不存在".into());
            }
        }
        let item = self
            .items
            .iter_mut()
            .find(|x| x.id == id)
            .ok_or("记录不存在")?;
        item.category = category;
        Ok(())
    }
    pub fn clear_history(&mut self) {
        self.items.retain(|x| x.category.is_some());
    }
    pub fn add_category(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        validate_category(name)?;
        if self.categories.len() >= 100 {
            return Err("最多支持 100 个分类".into());
        }
        if self
            .categories
            .iter()
            .any(|x| x.to_lowercase() == name.to_lowercase())
        {
            return Err("分类名称已存在".into());
        }
        self.categories.push(name.into());
        Ok(())
    }
    pub fn rename_category(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        validate_category(new)?;
        let index = self
            .categories
            .iter()
            .position(|x| x == old)
            .ok_or("分类不存在")?;
        if self
            .categories
            .iter()
            .enumerate()
            .any(|(i, x)| i != index && x.to_lowercase() == new.to_lowercase())
        {
            return Err("分类名称已存在".into());
        }
        self.categories[index] = new.into();
        if let Some(id) = self.sync.category_ids.remove(old) {
            self.sync.category_ids.insert(new.into(), id);
        }
        if let Some(color) = self.category_colors.remove(old) {
            self.category_colors.insert(new.into(), color);
        }
        for item in &mut self.items {
            if item.category.as_deref() == Some(old) {
                item.category = Some(new.into());
            }
        }
        Ok(())
    }
    pub fn delete_category(&mut self, name: &str) -> Result<(), String> {
        let index = self
            .categories
            .iter()
            .position(|x| x == name)
            .ok_or("分类不存在")?;
        self.categories.remove(index);
        self.category_colors.remove(name);
        for item in &mut self.items {
            if item.category.as_deref() == Some(name) {
                item.category = None;
            }
        }
        Ok(())
    }
    pub fn visible(&self, query: &str, category: Option<&str>) -> Vec<u64> {
        let query = query.to_lowercase();
        self.items
            .iter()
            .filter(|x| {
                let matches_category = category.is_none() || x.category.as_deref() == category;
                let matches_query = match &x.content {
                    Content::Text(text) => text.to_lowercase().contains(&query),
                    Content::Image { .. } => "图片 image png".contains(&query),
                };
                matches_category
                    && (matches_query
                        || x.tags.iter().any(|tag| tag.to_lowercase().contains(&query)))
            })
            .map(|x| x.id)
            .collect()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.items.len() > 500 || self.categories.len() > 100 {
            return Err("数据版本或记录数量不支持".into());
        }
        Hotkey::parse(&self.hotkey)?;
        let mut names = std::collections::HashSet::new();
        for name in &self.categories {
            validate_category(name)?;
            if !names.insert(name.to_lowercase()) {
                return Err("重复分类".into());
            }
        }
        let mut ids = std::collections::HashSet::new();
        let mut text_bytes = 0;
        for item in &self.items {
            item.content.validate()?;
            validate_tags(&item.tags)?;
            text_bytes += item.content.text_len();
            if item.id == 0 || item.id >= self.next_id || !ids.insert(item.id) {
                return Err("无效记录编号".into());
            }
            if let Some(category) = &item.category {
                if !self.categories.contains(category) {
                    return Err("记录引用未知分类".into());
                }
            }
        }
        if text_bytes > 64 * 1024 * 1024 {
            return Err("文字总量超限".into());
        }
        Ok(())
    }
}

fn validate_tags(tags: &[String]) -> Result<(), String> {
    if tags.len() > 8 {
        return Err("每条内容最多添加 8 个标签".into());
    }
    let mut seen = std::collections::HashSet::new();
    for tag in tags {
        if tag.is_empty()
            || tag.trim() != tag
            || tag.chars().count() > 24
            || tag.chars().any(|c| c.is_control() || c == ',' || c == '，')
        {
            return Err("每个标签应为 1–24 个字符，不能包含逗号或控制字符".into());
        }
        if !seen.insert(tag.to_lowercase()) {
            return Err("标签不能重复".into());
        }
    }
    Ok(())
}

fn validate_category(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name != name.trim()
        || name.chars().count() > 40
        || name.chars().any(char::is_control)
        || ["全部", "历史记录", "未收藏"].contains(&name)
    {
        return Err("分类名称需为 1–40 个字符，且不能使用系统分类名称".into());
    }
    Ok(())
}

pub fn valid_image_file(file: &str) -> bool {
    file.len() == 68
        && file.ends_with(".png")
        && file.as_bytes()[..64].iter().all(|x| x.is_ascii_hexdigit())
}

impl Content {
    fn text_len(&self) -> usize {
        match self {
            Self::Text(t) => t.len(),
            _ => 0,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Text(t) if t.is_empty() || t.len() > 512 * 1024 || t.contains('\0') => {
                Err("文字为空、含空字符或超过 512 KiB".into())
            }
            Self::Image {
                file,
                width,
                height,
            } if !valid_image_file(file)
                || *width == 0
                || *height == 0
                || (*width as u64) * (*height as u64) > 4 * 1024 * 1024 =>
            {
                Err("图片数据无效或超过 16 MiB".into())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub key: u32,
}
impl Hotkey {
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<String> = s
            .split('+')
            .map(|x| x.trim().to_ascii_uppercase())
            .collect();
        if parts.len() < 2 {
            return Err("请填写组合键，例如 Ctrl+Alt+P".into());
        }
        let mut modifiers = 0;
        for part in &parts[..parts.len() - 1] {
            let bit = match part.as_str() {
                "CTRL" | "CONTROL" => 2,
                "ALT" => 1,
                "SHIFT" => 4,
                "WIN" => 8,
                _ => return Err("修饰键只支持 Ctrl、Alt、Shift、Win".into()),
            };
            if modifiers & bit != 0 {
                return Err("修饰键不能重复".into());
            }
            modifiers |= bit;
        }
        if modifiers == 4 {
            return Err("请至少使用 Ctrl、Alt 或 Win 中的一个修饰键".into());
        }
        let last = parts.last().ok_or("缺少按键")?;
        let key = if last.len() == 1 && last.as_bytes()[0].is_ascii_alphanumeric() {
            last.as_bytes()[0] as u32
        } else if last == "SPACE" {
            0x20
        } else if let Some(n) = last
            .strip_prefix('F')
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| (1..=11).contains(n))
        {
            0x6f + n
        } else {
            return Err("按键支持字母、数字、Space、F1–F11".into());
        };
        if modifiers == 8 && key == b'L' as u32 {
            return Err("Win+L 是系统锁屏快捷键".into());
        }
        if modifiers == 2 && key == b'V' as u32 {
            return Err("Ctrl+V 用于向目标程序粘贴，不能作为呼出快捷键".into());
        }
        Ok(Self { modifiers, key })
    }
    pub fn label(self) -> String {
        let mut parts = vec![];
        for (bit, name) in [(2, "Ctrl"), (1, "Alt"), (4, "Shift"), (8, "Win")] {
            if self.modifiers & bit != 0 {
                parts.push(name.to_string());
            }
        }
        parts.push(match self.key {
            0x20 => "Space".into(),
            0x70..=0x7a => format!("F{}", self.key - 0x6f),
            x => char::from_u32(x).unwrap_or('?').to_string(),
        });
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_favorite_preserves_multiline_text_and_marks_source() {
        let mut m = Model::default();
        let text = "  first line\n\tsecond line\n";

        let id = m.add_favorite_text(text, "收藏").unwrap();

        assert_eq!(id, 1);
        assert_eq!(m.next_id, 2);
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.items[0].content, Content::Text(text.into()));
        assert_eq!(m.items[0].category.as_deref(), Some("收藏"));
        assert_eq!(m.items[0].source, "手动添加");
        assert_eq!(m.items[0].copied_at, 0);
        m.validate().unwrap();
    }

    #[test]
    fn manual_duplicate_does_not_move_or_reassign_existing_items() {
        let mut m = Model::default();
        m.add_category("工作").unwrap();
        let existing = m.capture(Content::Text("same text".into())).unwrap();
        m.assign(existing, Some("工作".into())).unwrap();
        m.items[0].source = "Original App".into();
        m.items[0].copied_at = 123;
        let history = m.capture(Content::Text("history".into())).unwrap();

        let created = m.add_favorite_text("same text", "收藏").unwrap();

        assert_ne!(created, existing);
        assert_eq!(m.items.len(), 3);
        assert_eq!(m.items[0].id, created);
        assert_eq!(m.items[0].category.as_deref(), Some("收藏"));
        assert_eq!(m.items[1].id, history);
        assert_eq!(m.items[2].id, existing);
        assert_eq!(m.items[2].category.as_deref(), Some("工作"));
        assert_eq!(m.items[2].source, "Original App");
        assert_eq!(m.items[2].copied_at, 123);
        m.validate().unwrap();
    }

    #[test]
    fn manual_duplicate_in_same_category_still_gets_new_id() {
        let mut m = Model::default();
        let existing = m.capture(Content::Text("same text".into())).unwrap();
        m.assign(existing, Some("收藏".into())).unwrap();

        let created = m.add_favorite_text("same text", "收藏").unwrap();

        assert_ne!(created, existing);
        assert_eq!(
            m.visible("same text", Some("收藏")),
            vec![created, existing]
        );
        m.validate().unwrap();
    }

    #[test]
    fn manual_blank_text_is_rejected_without_changing_model() {
        let mut m = Model::default();
        m.capture(Content::Text("keep".into())).unwrap();
        let before = serde_json::to_value(&m).unwrap();

        for text in ["", " \t\r\n", "\u{3000}\u{a0}"] {
            assert!(m.add_favorite_text(text, "收藏").is_err());
            assert_eq!(serde_json::to_value(&m).unwrap(), before);
        }
    }

    #[test]
    fn manual_text_uses_existing_content_size_and_null_validation() {
        let mut m = Model::default();
        let before = serde_json::to_value(&m).unwrap();

        for text in ["text\0text".into(), "x".repeat(512 * 1024 + 1)] {
            assert!(m.add_favorite_text(&text, "收藏").is_err());
            assert_eq!(serde_json::to_value(&m).unwrap(), before);
        }
        assert!(m.add_favorite_text(&"x".repeat(512 * 1024), "收藏").is_ok());
        m.validate().unwrap();
    }

    #[test]
    fn manual_unknown_category_is_rejected_without_changing_model() {
        let mut m = Model::default();
        m.capture(Content::Text("keep".into())).unwrap();
        let before = serde_json::to_value(&m).unwrap();

        assert!(m.add_favorite_text("new text", "不存在").is_err());

        assert_eq!(serde_json::to_value(&m).unwrap(), before);
    }

    #[test]
    fn manual_text_total_cannot_exceed_64_mib() {
        let mut m = Model::default();
        for id in 1..=127 {
            m.items.push(Item {
                id,
                content: Content::Text("x".repeat(512 * 1024)),
                category: Some("收藏".into()),
                tags: Vec::new(),
                source: String::new(),
                copied_at: 0,
            });
        }
        m.next_id = 128;
        m.add_favorite_text(&"x".repeat(512 * 1024), "收藏")
            .unwrap();
        m.validate().unwrap();
        let before = serde_json::to_value(&m).unwrap();

        assert!(m.add_favorite_text("x", "收藏").is_err());

        assert_eq!(serde_json::to_value(&m).unwrap(), before);
    }

    fn full_model_with_history() -> Model {
        Model {
            items: (1..=500)
                .rev()
                .map(|id| Item {
                    id,
                    content: Content::Text(format!("item {id}")),
                    category: if id == 2 || id == 4 {
                        None
                    } else {
                        Some("收藏".into())
                    },
                    source: String::new(),
                    tags: Vec::new(),
                    copied_at: 0,
                })
                .collect(),
            next_id: 501,
            ..Model::default()
        }
    }

    #[test]
    fn manual_full_model_evicts_only_oldest_unfavorited_item() {
        let mut m = full_model_with_history();

        let created = m.add_favorite_text("new favorite", "收藏").unwrap();

        assert_eq!(created, 501);
        assert_eq!(m.items.len(), 500);
        assert_eq!(m.items[0].id, created);
        assert!(!m.items.iter().any(|item| item.id == 2));
        assert!(m.items.iter().any(|item| item.id == 4));
        assert!(m.items.iter().any(|item| item.id == 1));
        m.validate().unwrap();
    }

    #[test]
    fn manual_full_favorites_are_rejected_without_changing_model() {
        let mut m = full_model_with_history();
        for item in &mut m.items {
            item.category = Some("收藏".into());
        }
        let before = serde_json::to_value(&m).unwrap();

        assert!(m.add_favorite_text("new favorite", "收藏").is_err());

        assert_eq!(serde_json::to_value(&m).unwrap(), before);
    }

    #[test]
    fn manual_numbering_failure_never_evicts_or_mutates_existing_items() {
        let mut m = full_model_with_history();
        for next_id in [u64::MAX, 0, 500] {
            m.next_id = next_id;
            let before = serde_json::to_value(&m).unwrap();

            assert!(m.add_favorite_text("new favorite", "收藏").is_err());

            assert_eq!(serde_json::to_value(&m).unwrap(), before);
        }
    }

    #[test]
    fn default_shortcut_is_ctrl_q() {
        assert_eq!(Model::default().hotkey, "Ctrl+Q");
    }

    #[test]
    fn duplicates_move_to_front_without_losing_collection() {
        let mut m = Model::default();
        let a = m.capture(Content::Text("hello".into())).unwrap();
        m.assign(a, Some("收藏".into())).unwrap();
        m.capture(Content::Text("other".into())).unwrap();
        let again = m.capture(Content::Text("hello".into())).unwrap();
        assert_eq!(a, again);
        assert_eq!(m.items.len(), 2);
        assert_eq!(m.items[0].category.as_deref(), Some("收藏"));
    }

    #[test]
    fn history_limit_and_clear_preserve_favorites() {
        let mut m = Model::default();
        let a = m.capture(Content::Text("keep".into())).unwrap();
        m.assign(a, Some("收藏".into())).unwrap();
        for n in 0..250 {
            m.capture(Content::Text(format!("item{n}"))).unwrap();
        }
        assert_eq!(m.items.len(), 201);
        m.clear_history();
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.items[0].id, a);
    }

    #[test]
    fn category_rename_and_delete_keep_contents() {
        let mut m = Model::default();
        m.add_category("工作").unwrap();
        let id = m.capture(Content::Text("ABC 搜索内容".into())).unwrap();
        m.assign(id, Some("工作".into())).unwrap();
        m.rename_category("工作", "代码").unwrap();
        assert_eq!(m.visible("abc", Some("代码")).len(), 1);
        assert!(m.add_category("代码").is_err());
        assert!(m.assign(id, Some("不存在".into())).is_err());
        m.delete_category("代码").unwrap();
        assert_eq!(m.items.len(), 1);
        assert!(m.items[0].category.is_none());
    }

    #[test]
    fn invalid_content_is_rejected() {
        let mut m = Model::default();
        assert!(m.capture(Content::Text("".into())).is_err());
        assert!(m
            .capture(Content::Text("x".repeat(512 * 1024 + 1)))
            .is_err());
        assert!(m.add_category("  ").is_err());
        assert!(m.add_category("全部").is_err());
        assert!(m
            .capture(Content::Image {
                file: "../escape.png".into(),
                width: 1,
                height: 1
            })
            .is_err());
    }

    #[test]
    fn hotkeys_accept_normalize_and_reject_unsafe_combinations() {
        let h = Hotkey::parse("ctrl + shift + v").unwrap();
        assert_eq!(h.modifiers, 6);
        assert_eq!(h.key, b'V' as u32);
        assert_eq!(h.label(), "Ctrl+Shift+V");
        assert_eq!(Hotkey::parse("Win+Alt+F9").unwrap().key, 0x78);
        for invalid in [
            "V",
            "Ctrl",
            "Ctrl+Ctrl+V",
            "Ctrl+V",
            "Ctrl+V+X",
            "Ctrl+F25",
            "Ctrl+F12",
            "Ctrl+Shift",
            "Win+L",
        ] {
            assert!(Hotkey::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn full_favorites_never_evict_silently() {
        let mut m = Model::default();
        for n in 0..500 {
            let id = m.capture(Content::Text(format!("favorite {n}"))).unwrap();
            m.assign(id, Some("收藏".into())).unwrap();
        }
        assert!(m.capture(Content::Text("overflow".into())).is_err());
        assert_eq!(m.items.len(), 500);
        assert!(m.capture(Content::Text("favorite 0".into())).is_ok());
    }
}
