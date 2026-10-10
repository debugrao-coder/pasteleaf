#[derive(Clone, Debug)]
pub(crate) struct MenuItem {
    pub(crate) label: String,
    pub(crate) hint: String,
    pub(crate) action: Option<usize>,
    pub(crate) children: Vec<MenuItem>,
    pub(crate) enabled: bool,
}

impl MenuItem {
    pub(crate) fn action(label: impl Into<String>, hint: impl Into<String>, id: usize) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            action: Some(id),
            children: Vec::new(),
            enabled: true,
        }
    }
    pub(crate) fn submenu(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            action: None,
            children,
            enabled: true,
        }
    }
    pub(crate) fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            action: None,
            children: Vec::new(),
            enabled: false,
        }
    }
    pub(crate) fn separator() -> Self {
        Self::disabled("")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MenuPage {
    pub(crate) title: String,
    pub(crate) items: Vec<MenuItem>,
}

#[derive(Clone, Debug)]
pub(crate) struct PopupMenu {
    pub(crate) pages: Vec<MenuPage>,
    pub(crate) selected: Option<usize>,
    pub(crate) first: usize,
    pub(crate) max_rows: usize,
}

impl PopupMenu {
    pub(crate) fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        let selected = items.iter().position(selectable);
        let mut menu = Self {
            pages: vec![MenuPage {
                title: title.into(),
                items,
            }],
            selected,
            first: 0,
            max_rows: 12,
        };
        menu.ensure_visible();
        menu
    }
    pub(crate) fn page(&self) -> &MenuPage {
        self.pages.last().expect("popup always has a root page")
    }
    pub(crate) fn rows(&self) -> usize {
        self.page().items.len().clamp(1, self.max_rows.clamp(1, 12))
    }
    pub(crate) fn fit_rows(&mut self, rows: usize) {
        self.max_rows = rows.clamp(1, 12);
        self.ensure_visible();
    }
    pub(crate) fn move_selection(&mut self, delta: i32) {
        let indices: Vec<usize> = self
            .page()
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| selectable(item).then_some(index))
            .collect();
        if indices.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|selected| indices.iter().position(|index| *index == selected));
        let start = current
            .map(|index| index as i64)
            .unwrap_or(if delta > 0 { -1 } else { 0 });
        let next = (start + i64::from(delta)).rem_euclid(indices.len() as i64) as usize;
        self.selected = Some(indices[next]);
        self.ensure_visible();
    }
    pub(crate) fn select(&mut self, index: usize) {
        if self.page().items.get(index).is_some_and(selectable) {
            self.selected = Some(index);
            self.ensure_visible();
        }
    }
    pub(crate) fn activate(&mut self) -> Option<usize> {
        let item = self.page().items.get(self.selected?)?;
        if !selectable(item) {
            return None;
        }
        if item.children.is_empty() {
            return item.action;
        }
        let page = MenuPage {
            title: item.label.clone(),
            items: item.children.clone(),
        };
        self.selected = page.items.iter().position(selectable);
        self.first = 0;
        self.pages.push(page);
        self.ensure_visible();
        None
    }
    pub(crate) fn back(&mut self) -> bool {
        if self.pages.len() <= 1 {
            return false;
        }
        self.pages.pop();
        self.selected = self.page().items.iter().position(selectable);
        self.first = 0;
        self.ensure_visible();
        true
    }
    pub(crate) fn scroll(&mut self, delta: i32) {
        let rows = self.rows();
        let limit = self.page().items.len().saturating_sub(rows);
        self.first = (self.first as i64 + i64::from(delta)).clamp(0, limit as i64) as usize;
        let selected = self.selected.unwrap_or(self.first);
        self.selected = self
            .page()
            .items
            .iter()
            .enumerate()
            .skip(self.first)
            .take(rows)
            .filter_map(|(index, item)| selectable(item).then_some(index))
            .min_by_key(|index| index.abs_diff(selected));
    }
    pub(crate) fn ensure_visible(&mut self) {
        let rows = self.rows();
        let limit = self.page().items.len().saturating_sub(rows);
        self.first = self.first.min(limit);
        if let Some(selected) = self.selected {
            if !self.page().items.get(selected).is_some_and(selectable) {
                self.selected = None;
            } else if selected < self.first {
                self.first = selected;
            } else if selected >= self.first + rows {
                self.first = selected + 1 - rows;
            }
        }
    }
    pub(crate) fn row_at(&self, y: i32) -> Option<usize> {
        let top = 8 + if self.page().title.is_empty() { 0 } else { 32 };
        if y < top {
            return None;
        }
        let row = ((y - top) / 36) as usize;
        let index = self.first + row;
        (row < self.rows() && index < self.page().items.len()).then_some(index)
    }
    pub(crate) fn width(&self) -> i32 {
        280
    }
    pub(crate) fn height(&self) -> i32 {
        16 + if self.page().title.is_empty() { 0 } else { 32 } + self.rows() as i32 * 36
    }
}

fn selectable(item: &MenuItem) -> bool {
    item.enabled && (!item.label.is_empty() || item.action.is_some() || !item.children.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn categories() -> Vec<MenuItem> {
        (0..100)
            .map(|id| MenuItem::action(format!("Category {id}"), "", id))
            .collect()
    }

    #[test]
    fn constructors_retain_labels_hints_actions_and_enabled_state() {
        let action = MenuItem::action("Copy", "Ctrl+C", 7);
        assert_eq!(action.label, "Copy");
        assert_eq!(action.hint, "Ctrl+C");
        assert_eq!(action.action, Some(7));
        assert!(action.enabled);
        assert!(action.children.is_empty());

        let submenu = MenuItem::submenu("Move", vec![action]);
        assert_eq!(submenu.label, "Move");
        assert_eq!(submenu.children.len(), 1);
        assert_eq!(submenu.action, None);
        assert!(submenu.enabled);

        let disabled = MenuItem::disabled("Unavailable");
        assert_eq!(disabled.label, "Unavailable");
        assert!(!disabled.enabled);
        assert_eq!(disabled.action, None);
        let separator = MenuItem::separator();
        assert!(separator.label.is_empty());
        assert!(separator.children.is_empty());
        assert_eq!(separator.action, None);
        assert!(!separator.enabled);
    }

    #[test]
    fn one_hundred_categories_fit_twelve_rows_and_remain_keyboard_reachable() {
        let mut menu = PopupMenu::new("Categories", categories());
        assert_eq!(menu.rows(), 12);
        assert_eq!(menu.selected, Some(0));
        menu.select(99);
        assert_eq!(menu.selected, Some(99));
        assert_eq!(menu.first, 88);
        assert_eq!(menu.row_at(40), Some(88));
        assert_eq!(menu.row_at(40 + 11 * 36), Some(99));
        menu.move_selection(1);
        assert_eq!(menu.selected, Some(0));
        assert_eq!(menu.first, 0);
        menu.move_selection(-1);
        assert_eq!(menu.selected, Some(99));
        assert_eq!(menu.first, 88);
    }

    #[test]
    fn viewport_can_fit_smaller_monitors_without_hiding_selection() {
        let mut menu = PopupMenu::new("Categories", categories());
        menu.select(99);
        menu.fit_rows(2);
        assert_eq!(menu.max_rows, 2);
        assert_eq!(menu.rows(), 2);
        assert_eq!(menu.first, 98);
        assert_eq!(menu.selected, Some(99));
        assert_eq!(menu.height(), 16 + 32 + 2 * 36);
        menu.fit_rows(0);
        assert_eq!(menu.rows(), 1);
        assert_eq!(menu.first, 99);
        menu.fit_rows(100);
        assert_eq!(menu.max_rows, 12);
        assert_eq!(menu.rows(), 12);
        assert_eq!(menu.first, 88);
    }

    #[test]
    fn keyboard_navigation_wraps_and_skips_disabled_items_and_separators() {
        let mut menu = PopupMenu::new(
            "",
            vec![
                MenuItem::disabled("Disabled"),
                MenuItem::action("First", "", 1),
                MenuItem::separator(),
                MenuItem::action("Last", "", 2),
            ],
        );
        assert_eq!(menu.selected, Some(1));
        menu.move_selection(1);
        assert_eq!(menu.selected, Some(3));
        menu.move_selection(1);
        assert_eq!(menu.selected, Some(1));
        menu.move_selection(-1);
        assert_eq!(menu.selected, Some(3));
        menu.move_selection(i32::MIN);
        assert_eq!(menu.selected, Some(3));
        menu.select(0);
        menu.select(2);
        menu.select(100);
        assert_eq!(menu.selected, Some(3));
        assert_eq!(menu.activate(), Some(2));
    }

    #[test]
    fn disabled_only_and_empty_menus_never_activate() {
        let mut disabled = PopupMenu::new(
            "",
            vec![MenuItem::disabled("Disabled"), MenuItem::separator()],
        );
        assert_eq!(disabled.selected, None);
        disabled.move_selection(1);
        disabled.move_selection(-1);
        assert_eq!(disabled.selected, None);
        assert_eq!(disabled.activate(), None);

        let mut empty = PopupMenu::new("", vec![]);
        assert_eq!(empty.rows(), 1);
        assert_eq!(empty.height(), 52);
        assert_eq!(empty.row_at(8), None);
        empty.move_selection(1);
        empty.scroll(i32::MAX);
        assert_eq!(empty.selected, None);
        assert_eq!(empty.first, 0);
        assert_eq!(empty.activate(), None);
    }

    #[test]
    fn submenu_activation_pushes_page_and_back_returns_to_parent() {
        let mut menu = PopupMenu::new(
            "Root",
            vec![MenuItem::submenu(
                "Move to",
                vec![
                    MenuItem::disabled("Unavailable"),
                    MenuItem::action("Work", "", 9),
                ],
            )],
        );
        assert!(!menu.back());
        assert_eq!(menu.activate(), None);
        assert_eq!(menu.pages.len(), 2);
        assert_eq!(menu.page().title, "Move to");
        assert_eq!(menu.selected, Some(1));
        assert_eq!(menu.first, 0);
        assert_eq!(menu.activate(), Some(9));
        assert!(menu.back());
        assert_eq!(menu.pages.len(), 1);
        assert_eq!(menu.page().title, "Root");
        assert_eq!(menu.selected, Some(0));
        assert_eq!(menu.first, 0);
        assert!(!menu.back());
    }

    #[test]
    fn scrolling_clamps_at_edges_and_keeps_selection_in_visible_rows() {
        let mut menu = PopupMenu::new("", categories());
        menu.scroll(5);
        assert_eq!(menu.first, 5);
        assert_eq!(menu.selected, Some(5));
        menu.scroll(i32::MAX);
        assert_eq!(menu.first, 88);
        assert_eq!(menu.selected, Some(88));
        menu.select(99);
        menu.scroll(-1);
        assert_eq!(menu.first, 87);
        assert_eq!(menu.selected, Some(98));
        menu.scroll(i32::MIN);
        assert_eq!(menu.first, 0);
        assert_eq!(menu.selected, Some(11));
    }

    #[test]
    fn scrolling_over_disabled_rows_clears_selection_until_action_is_visible() {
        let mut items = vec![MenuItem::action("First", "", 1)];
        items.extend((0..20).map(|_| MenuItem::disabled("Disabled")));
        let mut menu = PopupMenu::new("", items);
        menu.scroll(9);
        assert_eq!(menu.first, 9);
        assert_eq!(menu.selected, None);
        menu.scroll(-9);
        assert_eq!(menu.first, 0);
        assert_eq!(menu.selected, Some(0));
    }

    #[test]
    fn geometry_reserves_title_padding_and_gives_separators_full_rows() {
        let menu = PopupMenu::new(
            "Root",
            vec![MenuItem::action("Copy", "", 1), MenuItem::separator()],
        );
        assert_eq!(menu.width(), 280);
        assert_eq!(menu.height(), 16 + 32 + 2 * 36);
        assert_eq!(menu.row_at(-1), None);
        assert_eq!(menu.row_at(0), None);
        assert_eq!(menu.row_at(39), None);
        assert_eq!(menu.row_at(40), Some(0));
        assert_eq!(menu.row_at(75), Some(0));
        assert_eq!(menu.row_at(76), Some(1));
        assert_eq!(menu.row_at(111), Some(1));
        assert_eq!(menu.row_at(112), None);
        assert_eq!(menu.row_at(menu.height() - 1), None);

        let no_title = PopupMenu::new("", vec![MenuItem::action("Copy", "", 1)]);
        assert_eq!(no_title.height(), 52);
        assert_eq!(no_title.row_at(7), None);
        assert_eq!(no_title.row_at(8), Some(0));
        assert_eq!(no_title.row_at(43), Some(0));
        assert_eq!(no_title.row_at(44), None);
    }
}
