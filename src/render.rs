//! Antialiased GDI+ shapes with native ClearType text; no web runtime.
use super::*;
use std::ffi::c_void;

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *mut c_void,
    suppress_thread: i32,
    suppress_codecs: i32,
}
#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateFromHDC(dc: HDC, graphics: *mut *mut c_void) -> i32;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
    fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> i32;
    fn GdipCreatePath(fill: i32, path: *mut *mut c_void) -> i32;
    fn GdipAddPathArc(
        path: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        start: f32,
        sweep: f32,
    ) -> i32;
    fn GdipClosePathFigure(path: *mut c_void) -> i32;
    fn GdipDeletePath(path: *mut c_void) -> i32;
    fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
    fn GdipDeleteBrush(brush: *mut c_void) -> i32;
    fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
    fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> i32;
    fn GdipDeletePen(pen: *mut c_void) -> i32;
    fn GdipDrawLine(
        graphics: *mut c_void,
        pen: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> i32;
    fn GdipFlush(graphics: *mut c_void, intention: i32) -> i32;
}
pub(super) fn start_graphics() -> Result<usize, String> {
    let input = StartupInput {
        version: 1,
        callback: null_mut(),
        suppress_thread: 0,
        suppress_codecs: 0,
    };
    let mut token = 0;
    // SAFETY: repr(C) matches GdiplusStartupInput, output is optional.
    if unsafe { GdiplusStartup(&mut token, &input, null_mut()) } != 0 {
        Err("无法启动原生图形引擎".into())
    } else {
        Ok(token)
    }
}
pub(super) fn stop_graphics(token: usize) {
    // SAFETY: called after every GDI+ graphics object was released.
    unsafe {
        GdiplusShutdown(token);
    }
}
struct Painter {
    g: *mut c_void,
    scale: f32,
}
impl Painter {
    fn line(&self, x1: i32, y1: i32, x2: i32, y2: i32, color: u32) {
        if self.g.is_null() {
            return;
        }
        // SAFETY: pen is local to this draw, and the graphics handle is owned by Painter.
        unsafe {
            let mut pen = null_mut();
            if GdipCreatePen1(0xff000000 | color, 1.4 * self.scale, 2, &mut pen) == 0 {
                GdipDrawLine(
                    self.g,
                    pen,
                    x1 as f32 * self.scale,
                    y1 as f32 * self.scale,
                    x2 as f32 * self.scale,
                    y2 as f32 * self.scale,
                );
                GdipDeletePen(pen);
            }
        }
    }
    fn type_icon(&self, kind: &str, x: i32, y: i32) {
        let stroke = 0xFFFFFF;
        let line = |a, b, c, d| self.line(x + a, y + b, x + c, y + d, stroke);
        match kind {
            "代码" => {
                line(5, 3, 1, 8);
                line(1, 8, 5, 13);
                line(11, 3, 15, 8);
                line(15, 8, 11, 13);
                line(9, 2, 7, 14);
            }
            "链接" => {
                self.round(
                    Area {
                        x,
                        y: y + 4,
                        w: 10,
                        h: 8,
                    },
                    4,
                    None,
                    Some((stroke, 1.4)),
                );
                self.round(
                    Area {
                        x: x + 6,
                        y: y + 4,
                        w: 10,
                        h: 8,
                    },
                    4,
                    None,
                    Some((stroke, 1.4)),
                );
                line(5, 8, 11, 8);
            }
            "图片" => {
                self.round(
                    Area {
                        x,
                        y: y + 1,
                        w: 16,
                        h: 14,
                    },
                    2,
                    None,
                    Some((stroke, 1.4)),
                );
                line(2, 12, 6, 8);
                line(6, 8, 9, 11);
                line(9, 11, 12, 7);
                line(12, 7, 14, 10);
                self.round(
                    Area {
                        x: x + 3,
                        y: y + 4,
                        w: 2,
                        h: 2,
                    },
                    1,
                    Some(stroke),
                    None,
                );
            }
            _ => {
                self.round(
                    Area {
                        x: x + 2,
                        y,
                        w: 12,
                        h: 16,
                    },
                    2,
                    None,
                    Some((stroke, 1.4)),
                );
                for row in [4, 8, 12] {
                    line(5, row, if row == 12 { 9 } else { 11 }, row);
                }
            }
        }
    }
    fn new(dc: HDC, scale: f32) -> Self {
        let mut g = null_mut();
        // SAFETY: live paint DC; graphics is dropped before releasing DC.
        unsafe {
            GdipCreateFromHDC(dc, &mut g);
            if !g.is_null() {
                GdipSetSmoothingMode(g, 4);
            }
        }
        Self { g, scale }
    }
    fn round(&self, a: Area, r: i32, fill: Option<u32>, border: Option<(u32, f32)>) {
        if self.g.is_null() {
            return;
        }
        let x = a.x as f32 * self.scale;
        let y = a.y as f32 * self.scale;
        let w = a.w as f32 * self.scale;
        let h = a.h as f32 * self.scale;
        let d = (r.max(1) * 2) as f32 * self.scale;
        // SAFETY: path/brush/pen handles have bounded local lifetimes. All geometry
        // is finite and expressed in physical pixels, avoiding bitmap DPI scaling.
        unsafe {
            let mut path = null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            GdipAddPathArc(path, x, y, d, d, 180., 90.);
            GdipAddPathArc(path, x + w - d, y, d, d, 270., 90.);
            GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0., 90.);
            GdipAddPathArc(path, x, y + h - d, d, d, 90., 90.);
            GdipClosePathFigure(path);
            if let Some(c) = fill {
                let mut brush = null_mut();
                if GdipCreateSolidFill(0xff000000 | c, &mut brush) == 0 {
                    GdipFillPath(self.g, brush, path);
                    GdipDeleteBrush(brush);
                }
            }
            if let Some((c, width)) = border {
                let mut pen = null_mut();
                if GdipCreatePen1(0xff000000 | c, width * self.scale, 2, &mut pen) == 0 {
                    GdipDrawPath(self.g, pen, path);
                    GdipDeletePen(pen);
                }
            }
            GdipDeletePath(path);
            GdipFlush(self.g, 1);
        }
    }
}
impl Drop for Painter {
    fn drop(&mut self) {
        // SAFETY: uniquely owned graphics, DC still live.
        unsafe {
            if !self.g.is_null() {
                GdipDeleteGraphics(self.g);
            }
        }
    }
}
fn lighter(c: u32) -> u32 {
    let channel = |shift: u32| {
        let n = (c >> shift) & 255;
        (n + (255 - n) / 5) << shift
    };
    channel(16) | channel(8) | channel(0)
}

impl App {
    fn text_width(&self, dc: HDC, text: &str, font: usize) -> i32 {
        let utf16 = wide(text);
        // SAFETY: live DC/font and initialized output; restore previous selection.
        unsafe {
            let previous = SelectObject(dc, self.fonts[font]);
            let mut size: SIZE = zeroed();
            GetTextExtentPoint32W(dc, utf16.as_ptr(), (utf16.len() - 1) as i32, &mut size);
            SelectObject(dc, previous);
            (size.cx as f32 / self.scale).ceil() as i32
        }
    }
    fn draw_card_tags(
        &mut self,
        p: &Painter,
        dc: HDC,
        position: (i32, i32),
        kind: &str,
        tags: &[String],
        accent: u32,
    ) {
        let (x, y) = position;
        if x + 36 >= self.width {
            return;
        }
        p.type_icon(kind, x + 16, y + 53);
        self.tips.push((
            Area {
                x: x + 12,
                y: y + 48,
                w: 24,
                h: 26,
            },
            kind.into(),
        ));
        let start = x + 42;
        let end = (x + CARD_W - 14).min(self.width - 12);
        let mut cursor = start;
        for (i, tag) in tags.iter().enumerate() {
            let remainder = tags.len() - i - 1;
            let reserve = if remainder > 0 { 32 } else { 0 };
            let wanted = self.text_width(dc, tag, 3) + 16;
            let available = end - cursor - reserve;
            if available < 28 || (i > 0 && wanted > available) {
                break;
            }
            let width = wanted.min(available);
            let badge = Area {
                x: cursor,
                y: y + 49,
                w: width,
                h: 23,
            };
            p.round(badge, 6, Some(lighter(accent)), None);
            self.label(
                dc,
                tag,
                Area {
                    x: cursor + 8,
                    y: y + 52,
                    w: width - 16,
                    h: 17,
                },
                0xFFFFFF,
                3,
                DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            cursor += width + 6;
            // Reserve the final badge until every remaining tag is known to fit.
            let remaining_width: i32 = tags[i + 1..]
                .iter()
                .map(|t| self.text_width(dc, t, 3) + 22)
                .sum();
            if remainder > 0 && remaining_width - 6 > end - cursor {
                let overflow = format!("+{remainder}");
                let a = Area {
                    x: cursor,
                    y: y + 49,
                    w: 26,
                    h: 23,
                };
                p.round(a, 6, Some(lighter(accent)), None);
                self.label(
                    dc,
                    &overflow,
                    Area {
                        y: y + 52,
                        h: 17,
                        ..a
                    },
                    0xFFFFFF,
                    3,
                    DT_SINGLELINE | DT_CENTER,
                );
                break;
            }
        }
        if !tags.is_empty() && end > start {
            self.tips.push((
                Area {
                    x: start,
                    y: y + 48,
                    w: end - start,
                    h: 26,
                },
                tags.join("\n"),
            ));
        }
    }
    fn draw_tooltip(&self, p: &Painter, dc: HDC) {
        if self.dialog != Dialog::None || self.popup.is_some() || self.dragging {
            return;
        }
        let Some((mx, my)) = self.tip_point else {
            return;
        };
        let Some((anchor, text)) = self.tips.iter().find(|(area, _)| area.contains(mx, my)) else {
            return;
        };
        let width = self
            .text_width(
                dc,
                text.lines()
                    .max_by_key(|s| s.chars().count())
                    .unwrap_or(text),
                3,
            )
            .clamp(32, (self.width - 48).min(320))
            + 24;
        let mut bounds = self.rect(Area {
            x: 0,
            y: 0,
            w: width - 24,
            h: 0,
        });
        // SAFETY: calculation only on a live DC; the selected font is restored.
        unsafe {
            let previous = SelectObject(dc, self.fonts[3]);
            DrawTextW(
                dc,
                wide(text).as_ptr(),
                -1,
                &mut bounds,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(dc, previous);
        }
        let height = ((bounds.bottom - bounds.top) as f32 / self.scale).ceil() as i32 + 18;
        let area = Area {
            x: anchor.x.clamp(12, self.width - width - 12),
            y: (anchor.y + anchor.h + 6).min(PANEL_H - height - 12),
            w: width,
            h: height,
        };
        p.round(
            area,
            7,
            Some(self.palette.surface),
            Some((self.palette.border, 1.)),
        );
        self.label(
            dc,
            text,
            Area {
                x: area.x + 12,
                y: area.y + 9,
                w: area.w - 24,
                h: area.h - 18,
            },
            self.palette.text,
            3,
            DT_WORDBREAK | DT_NOPREFIX,
        );
    }
    fn label(&self, dc: HDC, text: &str, a: Area, c: u32, font: usize, flags: u32) {
        // SAFETY: live memory DC and valid font handle; selection is restored.
        unsafe {
            let old = SelectObject(dc, self.fonts[font]);
            SetTextColor(dc, colorref(c));
            SetBkMode(dc, TRANSPARENT as i32);
            let mut r = self.rect(a);
            let mut s = wide(text);
            DrawTextW(dc, s.as_mut_ptr(), -1, &mut r, flags | DT_NOPREFIX);
            SelectObject(dc, old);
        }
    }
    fn badge(&mut self, p: &Painter, dc: HDC, a: Area, text: &str, hit: Hit, active: bool) {
        let hover = self.hover.as_ref() == Some(&hit);
        if active || hover {
            p.round(
                a,
                9,
                Some(if active {
                    self.palette.surface
                } else {
                    self.palette.hover
                }),
                if active {
                    Some((self.palette.border, 0.6))
                } else {
                    None
                },
            );
        }
        self.label(
            dc,
            text,
            Area {
                x: a.x + 10,
                y: a.y + 6,
                w: a.w - 20,
                h: a.h - 10,
            },
            if active {
                self.palette.text
            } else {
                self.palette.muted
            },
            0,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER,
        );
        self.hits.push((a, hit));
    }
    fn draw_header(&mut self, p: &Painter, dc: HDC) {
        let compact = self.width < 800;
        let small = self.width < 680;
        let actions_left = self.width
            - if self.board > 0 {
                if compact {
                    340
                } else {
                    440
                }
            } else if compact {
                210
            } else {
                310
            };
        self.badge(
            p,
            dc,
            Area {
                x: 22,
                y: 18,
                w: if small { 42 } else { 142 },
                h: 38,
            },
            if small { "◷" } else { "◷  剪贴板历史" },
            Hit::Board(0),
            self.board == 0,
        );
        let mut x = if small { 74 } else { 176 };
        // Leave room for an overflow menu, the category add button and spacing.
        let end = actions_left - 76;
        let mut omitted = false;
        let cats = self.model.categories.clone();
        for (i, name) in cats.iter().enumerate().skip(self.board_offset) {
            let width = (name.chars().count() as i32 * 14 + 36).clamp(76, 142);
            if x + width > end {
                omitted = true;
                break;
            }
            let active = self.board == i + 1;
            let a = Area {
                x,
                y: 18,
                w: width,
                h: 38,
            };
            let hit = Hit::Board(i + 1);
            let hover = self.hover.as_ref() == Some(&hit);
            if active || hover {
                p.round(
                    a,
                    9,
                    Some(if self.dragging && hover {
                        self.palette.selected
                    } else if active {
                        self.palette.surface
                    } else {
                        self.palette.hover
                    }),
                    None,
                );
            }
            let color = *self
                .model
                .category_colors
                .get(name)
                .unwrap_or(&COLORS[i % COLORS.len()]);
            p.round(
                Area {
                    x: x + 11,
                    y: 33,
                    w: 8,
                    h: 8,
                },
                4,
                Some(color),
                None,
            );
            self.label(
                dc,
                name,
                Area {
                    x: x + 26,
                    y: 25,
                    w: width - 32,
                    h: 24,
                },
                if active {
                    self.palette.text
                } else {
                    self.palette.muted
                },
                0,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            self.hits.push((a, hit));
            x += width + 6;
        }
        if omitted || self.board_offset > 0 {
            self.badge(
                p,
                dc,
                Area {
                    x,
                    y: 18,
                    w: 32,
                    h: 38,
                },
                "···",
                Hit::MoreBoards,
                false,
            );
            x += 36;
        }
        self.badge(
            p,
            dc,
            Area {
                x,
                y: 18,
                w: 34,
                h: 38,
            },
            "+",
            Hit::Add,
            false,
        );
        if self.board > 0 {
            self.badge(
                p,
                dc,
                Area {
                    x: actions_left,
                    y: 18,
                    w: 118,
                    h: 38,
                },
                "+ 新建收藏项",
                Hit::NewItem,
                true,
            );
        }
        let search = Area {
            x: self.width - if compact { 210 } else { 310 },
            y: 18,
            w: if compact { 124 } else { 224 },
            h: 38,
        };
        p.round(
            search,
            9,
            Some(self.palette.input),
            Some((self.palette.border, 0.7)),
        );
        self.label(
            dc,
            "⌕",
            Area {
                x: search.x + 8,
                y: 23,
                w: 24,
                h: 28,
            },
            self.palette.muted,
            2,
            DT_SINGLELINE | DT_CENTER,
        );
        self.hits.push((search, Hit::Search));
        self.badge(
            p,
            dc,
            Area {
                x: self.width - 77,
                y: 18,
                w: 32,
                h: 38,
            },
            "···",
            Hit::Settings,
            false,
        );
        self.badge(
            p,
            dc,
            Area {
                x: self.width - 42,
                y: 18,
                w: 28,
                h: 38,
            },
            "×",
            Hit::Close,
            false,
        );
    }
    fn draw_cards(&mut self, p: &Painter, dc: HDC) {
        if self.visible.is_empty() {
            p.round(
                Area {
                    x: self.width / 2 - 26,
                    y: 133,
                    w: 52,
                    h: 52,
                },
                14,
                Some(self.palette.surface),
                None,
            );
            self.label(
                dc,
                "▤",
                Area {
                    x: self.width / 2 - 24,
                    y: 140,
                    w: 48,
                    h: 40,
                },
                self.palette.muted,
                4,
                DT_CENTER | DT_SINGLELINE,
            );
            self.label(
                dc,
                if self.text(SEARCH).is_empty() {
                    if self.board > 0 {
                        "还没有收藏项"
                    } else {
                        "复制一点灵感，留在这里"
                    }
                } else {
                    "没有找到匹配内容"
                },
                Area {
                    x: 20,
                    y: 202,
                    w: self.width - 40,
                    h: 35,
                },
                self.palette.text,
                2,
                DT_SINGLELINE | DT_CENTER,
            );
            self.label(
                dc,
                if self.board > 0 {
                    "点击「新建收藏项」输入文字，或将历史卡片拖到分类"
                } else if self.model.paused {
                    "记录已暂停 · 点击右上角菜单继续"
                } else {
                    "文字、链接和图片会自动出现在剪贴板历史中"
                },
                Area {
                    x: 20,
                    y: 245,
                    w: self.width - 40,
                    h: 24,
                },
                self.palette.muted,
                0,
                DT_SINGLELINE | DT_CENTER,
            );
            return;
        }
        let ids: Vec<u64> = self
            .visible
            .iter()
            .skip(self.offset)
            .take(self.page_size() + 1)
            .copied()
            .collect();
        for (n, id) in ids.iter().enumerate() {
            let Some(item) = self.model.items.iter().find(|x| x.id == *id).cloned() else {
                continue;
            };
            let x = 24 + n as i32 * (CARD_W + CARD_GAP);
            let y = CARD_TOP;
            let a = Area {
                x,
                y,
                w: CARD_W,
                h: CARD_H,
            };
            let selected = self.selected == Some(*id);
            let hovered = self.hover == Some(Hit::Card(*id));
            if selected {
                p.round(
                    Area {
                        x: x - 4,
                        y: y - 4,
                        w: CARD_W + 8,
                        h: CARD_H + 8,
                    },
                    15,
                    None,
                    Some((self.palette.accent, 2.)),
                );
            }
            p.round(
                Area {
                    x,
                    y: y + 3,
                    w: CARD_W,
                    h: CARD_H,
                },
                12,
                Some(self.palette.shadow),
                None,
            );
            let (code, kind, accent) = match &item.content {
                Content::Image { .. } => (
                    false,
                    "图片",
                    if self.palette.dark {
                        0x866236
                    } else {
                        0xDCA450
                    },
                ),
                Content::Text(t) if t.starts_with("http://") || t.starts_with("https://") => (
                    false,
                    "链接",
                    if self.palette.dark {
                        0x356B9B
                    } else {
                        0x55A0E4
                    },
                ),
                Content::Text(t)
                    if t.contains("fn ") || t.contains("const ") || t.contains("SELECT ") =>
                {
                    (
                        true,
                        "代码",
                        if self.palette.dark {
                            0x69518F
                        } else {
                            0x9677D1
                        },
                    )
                }
                _ => (
                    false,
                    "文本",
                    if self.palette.dark {
                        0x9A4D68
                    } else {
                        0xE56B82
                    },
                ),
            };
            p.round(a, 12, Some(accent), None);
            p.round(
                Area {
                    x,
                    y: y + 78,
                    w: CARD_W,
                    h: CARD_H - 78,
                },
                12,
                Some(if hovered {
                    self.palette.hover
                } else {
                    self.palette.surface
                }),
                None,
            );
            p.round(
                Area {
                    x: x + 14,
                    y: y + 13,
                    w: 27,
                    h: 27,
                },
                7,
                Some(lighter(accent)),
                None,
            );
            let source = if item.source.is_empty() {
                "剪贴板"
            } else {
                &item.source
            };
            let letter = source
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "P".into());
            self.label(
                dc,
                &letter,
                Area {
                    x: x + 14,
                    y: y + 15,
                    w: 27,
                    h: 24,
                },
                0xFFFFFF,
                0,
                DT_SINGLELINE | DT_CENTER,
            );
            self.label(
                dc,
                source,
                Area {
                    x: x + 49,
                    y: y + 17,
                    w: 130,
                    h: 23,
                },
                0xFFFFFF,
                0,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            self.draw_card_tags(p, dc, (x, y), kind, &item.tags, accent);
            self.label(
                dc,
                &format!("Ctrl+{}", n + 1),
                Area {
                    x: x + 174,
                    y: y + 18,
                    w: 46,
                    h: 20,
                },
                0xF7F7FA,
                3,
                DT_SINGLELINE | DT_RIGHT,
            );
            match &item.content {
                Content::Text(t) => {
                    let preview: String = t.chars().take(320).collect();
                    self.label(
                        dc,
                        &preview,
                        Area {
                            x: x + 16,
                            y: y + 94,
                            w: CARD_W - 32,
                            h: 116,
                        },
                        if code {
                            self.palette.code
                        } else {
                            self.palette.text
                        },
                        1,
                        DT_WORDBREAK | DT_END_ELLIPSIS | DT_EDITCONTROL,
                    );
                    let detail = if kind == "链接" {
                        t.lines()
                            .next()
                            .unwrap_or("")
                            .trim_start_matches("https://")
                            .trim_start_matches("http://")
                            .split('/')
                            .next()
                            .unwrap_or("")
                            .to_string()
                    } else {
                        format!("{} 个字符", t.chars().count())
                    };
                    self.label(
                        dc,
                        &detail,
                        Area {
                            x: x + 16,
                            y: y + 218,
                            w: CARD_W - 32,
                            h: 19,
                        },
                        self.palette.muted,
                        3,
                        DT_SINGLELINE | DT_END_ELLIPSIS,
                    );
                }
                Content::Image {
                    file,
                    width,
                    height,
                } => {
                    if !self.thumbnails.contains_key(id) {
                        if let Ok(img) = self.store.read_image(file) {
                            let mut thumb = image::imageops::thumbnail(
                                &img,
                                self.px(CARD_W - 24) as u32,
                                self.px(126) as u32,
                            );
                            for pixel in thumb.pixels_mut() {
                                pixel.0.swap(0, 2);
                            }
                            self.thumbnails.insert(*id, thumb);
                        }
                    }
                    if let Some(img) = self.thumbnails.get(id) {
                        let bx = self.px(x + 12) + (self.px(CARD_W - 24) - img.width() as i32) / 2;
                        let by = self.px(y + 87) + (self.px(126) - img.height() as i32) / 2;
                        draw_bitmap(dc, img, bx, by, img.width() as i32, img.height() as i32);
                    } else {
                        self.label(
                            dc,
                            "图片不可读取",
                            Area {
                                x: x + 15,
                                y: y + 135,
                                w: CARD_W - 30,
                                h: 30,
                            },
                            self.palette.muted,
                            0,
                            DT_CENTER,
                        );
                    }
                    self.label(
                        dc,
                        &format!("{width} × {height}"),
                        Area {
                            x: x + 16,
                            y: y + 218,
                            w: CARD_W - 32,
                            h: 19,
                        },
                        self.palette.muted,
                        3,
                        DT_SINGLELINE,
                    );
                }
            }
            self.hits.push((a, Hit::Card(*id)));
            let age = if item.copied_at == 0 {
                "本地保存".to_string()
            } else {
                let secs = timestamp().saturating_sub(item.copied_at);
                if secs < 60 {
                    "刚刚".into()
                } else if secs < 3600 {
                    format!("{} 分钟前", secs / 60)
                } else if secs < 86400 {
                    format!("{} 小时前", secs / 3600)
                } else {
                    format!("{} 天前", secs / 86400)
                }
            };
            let caption = match &item.category {
                Some(c) => format!("● {c}  ·  {age}"),
                None => age,
            };
            self.label(
                dc,
                &caption,
                Area {
                    x,
                    y: y + CARD_H + 9,
                    w: CARD_W,
                    h: 18,
                },
                self.palette.muted,
                3,
                DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
    }
    fn draw_dialog(&mut self, p: &Painter, dc: HDC) {
        self.hits.clear();
        if matches!(self.dialog, Dialog::Sync | Dialog::SyncConflict(..)) {
            self.draw_sync(p, dc);
            return;
        }
        let x = (self.width - 520) / 2;
        if let Dialog::NewItem(category) = self.dialog.clone() {
            self.draw_new_item(p, dc, &category);
            return;
        }
        if matches!(self.dialog, Dialog::EditTags(_)) {
            self.draw_tag_editor(p, dc);
            return;
        }
        if self.dialog == Dialog::Settings {
            self.draw_settings(p, dc, x);
            return;
        }
        if let Dialog::Preview(id) = self.dialog {
            p.round(
                Area {
                    x: 28,
                    y: 65,
                    w: self.width - 56,
                    h: 270,
                },
                14,
                Some(self.palette.input),
                Some((self.palette.border, 0.6)),
            );
            self.label(
                dc,
                "内容预览",
                Area {
                    x: 51,
                    y: 79,
                    w: 250,
                    h: 27,
                },
                self.palette.text,
                2,
                DT_SINGLELINE,
            );
            self.badge(
                p,
                dc,
                Area {
                    x: self.width - 80,
                    y: 73,
                    w: 34,
                    h: 32,
                },
                "×",
                Hit::Cancel,
                false,
            );
            if let Some(item) = self.model.items.iter().find(|i| i.id == id) {
                if let Content::Image { file, .. } = &item.content {
                    if let Ok(image) = self.store.read_image(file) {
                        let mut img = image::imageops::thumbnail(
                            &image,
                            self.px(self.width - 140) as u32,
                            self.px(208) as u32,
                        );
                        for pixel in img.pixels_mut() {
                            pixel.0.swap(0, 2);
                        }
                        draw_bitmap(
                            dc,
                            &img,
                            (self.px(self.width) - img.width() as i32) / 2,
                            self.px(111),
                            img.width() as i32,
                            img.height() as i32,
                        );
                    }
                }
            }
            return;
        }
        p.round(
            Area {
                x: x + 1,
                y: 73,
                w: 520,
                h: 252,
            },
            17,
            Some(self.palette.shadow),
            None,
        );
        p.round(
            Area {
                x,
                y: 70,
                w: 520,
                h: 252,
            },
            16,
            Some(self.palette.surface),
            Some((self.palette.border, 0.6)),
        );
        self.label(
            dc,
            if matches!(self.dialog, Dialog::RenameBoard(_)) {
                "编辑收藏分类"
            } else {
                "新建收藏分类"
            },
            Area {
                x: x + 26,
                y: 94,
                w: 450,
                h: 32,
            },
            self.palette.text,
            2,
            DT_SINGLELINE,
        );
        self.label(
            dc,
            "给常用内容一个名字，再选一种颜色。",
            Area {
                x: x + 28,
                y: 130,
                w: 460,
                h: 25,
            },
            self.palette.muted,
            0,
            DT_SINGLELINE,
        );
        p.round(
            Area {
                x: x + 24,
                y: 158,
                w: 470,
                h: 42,
            },
            8,
            Some(self.palette.input),
            Some((self.palette.border, 1.)),
        );
        {
            for (i, c) in COLORS.iter().enumerate() {
                let a = Area {
                    x: x + 28 + i as i32 * 40,
                    y: 217,
                    w: 24,
                    h: 24,
                };
                p.round(a, 12, Some(*c), None);
                if i == self.chosen_color {
                    p.round(
                        Area {
                            x: a.x - 3,
                            y: a.y - 3,
                            w: 30,
                            h: 30,
                        },
                        15,
                        None,
                        Some((*c, 1.5)),
                    );
                }
                self.hits.push((
                    Area {
                        x: a.x - 4,
                        y: a.y - 4,
                        w: 32,
                        h: 32,
                    },
                    Hit::Color(i),
                ));
            }
        }
        self.badge(
            p,
            dc,
            Area {
                x: x + 287,
                y: 266,
                w: 88,
                h: 34,
            },
            "取消",
            Hit::Cancel,
            false,
        );
        let save = Area {
            x: x + 390,
            y: 266,
            w: 103,
            h: 34,
        };
        p.round(save, 8, Some(0x4A86EF), None);
        self.label(
            dc,
            "完成",
            Area {
                x: save.x,
                y: save.y + 6,
                w: save.w,
                h: 24,
            },
            0xFFFFFF,
            0,
            DT_SINGLELINE | DT_CENTER,
        );
        self.hits.push((save, Hit::Accept));
    }
    fn draw_new_item(&mut self, p: &Painter, dc: HDC, category: &str) {
        let width = (self.width - 48).min(720);
        let x = (self.width - width) / 2;
        p.round(
            Area {
                x: x + 1,
                y: 61,
                w: width,
                h: 275,
            },
            16,
            Some(self.palette.shadow),
            None,
        );
        p.round(
            Area {
                x,
                y: 58,
                w: width,
                h: 275,
            },
            16,
            Some(self.palette.surface),
            Some((self.palette.border, 0.6)),
        );
        self.label(
            dc,
            "新建收藏项",
            Area {
                x: x + 24,
                y: 76,
                w: width - 60,
                h: 30,
            },
            self.palette.text,
            2,
            DT_SINGLELINE,
        );
        self.label(
            dc,
            &format!("保存到「{category}」 · Enter 换行，Ctrl+Enter 保存"),
            Area {
                x: x + 26,
                y: 112,
                w: width - 52,
                h: 24,
            },
            self.palette.muted,
            0,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        p.round(
            Area {
                x: x + 24,
                y: 144,
                w: width - 48,
                h: 74,
            },
            8,
            Some(self.palette.input),
            Some((self.palette.border, 1.)),
        );
        self.label(
            dc,
            "标签（可选，用逗号分隔）",
            Area {
                x: x + 26,
                y: 220,
                w: width - 52,
                h: 18,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        p.round(
            Area {
                x: x + 24,
                y: 240,
                w: width - 48,
                h: 34,
            },
            7,
            Some(self.palette.input),
            Some((self.palette.border, 1.)),
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + width - 257,
                y: 282,
                w: 88,
                h: 36,
            },
            "取消",
            Hit::Cancel,
            false,
        );
        let save = Area {
            x: x + width - 152,
            y: 282,
            w: 128,
            h: 36,
        };
        p.round(
            save,
            8,
            Some(self.palette.selected),
            Some((self.palette.accent, 0.8)),
        );
        self.label(
            dc,
            "保存收藏项",
            Area {
                y: save.y + 6,
                h: 24,
                ..save
            },
            self.palette.text,
            0,
            DT_SINGLELINE | DT_CENTER,
        );
        self.hits.push((save, Hit::Accept));
    }
    fn draw_tag_editor(&mut self, p: &Painter, dc: HDC) {
        let width = (self.width - 48).min(720);
        let x = (self.width - width) / 2;
        p.round(
            Area {
                x,
                y: 78,
                w: width,
                h: 226,
            },
            16,
            Some(self.palette.surface),
            Some((self.palette.border, 0.6)),
        );
        self.label(
            dc,
            "编辑标签",
            Area {
                x: x + 24,
                y: 96,
                w: width - 48,
                h: 30,
            },
            self.palette.text,
            2,
            DT_SINGLELINE,
        );
        self.label(
            dc,
            "用逗号分隔，最多 8 个，每个不超过 24 个字",
            Area {
                x: x + 26,
                y: 136,
                w: width - 52,
                h: 22,
            },
            self.palette.muted,
            0,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        p.round(
            Area {
                x: x + 24,
                y: 166,
                w: width - 48,
                h: 40,
            },
            8,
            Some(self.palette.input),
            Some((self.palette.border, 1.)),
        );
        self.label(
            dc,
            "留空可移除全部标签 · Enter 保存",
            Area {
                x: x + 26,
                y: 216,
                w: width - 52,
                h: 20,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + width - 234,
                y: 252,
                w: 88,
                h: 34,
            },
            "取消",
            Hit::Cancel,
            false,
        );
        let save = Area {
            x: x + width - 130,
            y: 252,
            w: 106,
            h: 34,
        };
        p.round(
            save,
            8,
            Some(self.palette.selected),
            Some((self.palette.accent, 0.8)),
        );
        self.label(
            dc,
            "保存标签",
            Area {
                y: save.y + 6,
                h: 24,
                ..save
            },
            self.palette.text,
            0,
            DT_SINGLELINE | DT_CENTER,
        );
        self.hits.push((save, Hit::Accept));
    }
    fn draw_settings(&mut self, p: &Painter, dc: HDC, x: i32) {
        p.round(
            Area {
                x: x + 1,
                y: 61,
                w: 520,
                h: 275,
            },
            16,
            Some(self.palette.shadow),
            None,
        );
        p.round(
            Area {
                x,
                y: 58,
                w: 520,
                h: 275,
            },
            16,
            Some(self.palette.surface),
            Some((self.palette.border, 0.6)),
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + 268,
                y: 72,
                w: 158,
                h: 28,
            },
            "iCloud Drive 同步",
            Hit::Sync(SyncAction::Open),
            false,
        );
        self.label(
            dc,
            "设置",
            Area {
                x: x + 24,
                y: 72,
                w: 200,
                h: 30,
            },
            self.palette.text,
            2,
            DT_SINGLELINE,
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + 443,
                y: 72,
                w: 54,
                h: 28,
            },
            "关闭",
            Hit::Cancel,
            false,
        );

        self.label(
            dc,
            "常规",
            Area {
                x: x + 24,
                y: 111,
                w: 70,
                h: 24,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        self.label(
            dc,
            "开机启动",
            Area {
                x: x + 104,
                y: 109,
                w: 210,
                h: 24,
            },
            self.palette.text,
            0,
            DT_SINGLELINE,
        );
        self.label(
            dc,
            "登录 Windows 后在托盘运行",
            Area {
                x: x + 104,
                y: 136,
                w: 290,
                h: 21,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        let enabled = self.startup_enabled == Some(true);
        let toggle = Area {
            x: x + 447,
            y: 114,
            w: 48,
            h: 26,
        };
        p.round(
            toggle,
            13,
            Some(if enabled {
                self.palette.accent
            } else {
                self.palette.border
            }),
            None,
        );
        p.round(
            Area {
                x: toggle.x + if enabled { 25 } else { 3 },
                y: toggle.y + 3,
                w: 20,
                h: 20,
            },
            10,
            Some(if enabled {
                self.palette.surface
            } else {
                self.palette.text
            }),
            None,
        );
        self.label(
            dc,
            match self.startup_enabled {
                Some(true) => "已开启",
                Some(false) => "已关闭",
                None => "重试",
            },
            Area {
                x: x + 380,
                y: 115,
                w: 60,
                h: 23,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE | DT_RIGHT,
        );
        self.hits.push((
            Area {
                x: x + 376,
                y: 105,
                w: 121,
                h: 47,
            },
            Hit::Startup,
        ));
        for y in [168, 225] {
            p.round(
                Area {
                    x: x + 24,
                    y,
                    w: 472,
                    h: 1,
                },
                0,
                Some(self.palette.border),
                None,
            );
        }
        self.label(
            dc,
            "外观",
            Area {
                x: x + 24,
                y: 185,
                w: 70,
                h: 24,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        for (i, (theme, title)) in [
            (Theme::Light, "浅色"),
            (Theme::Dark, "深色"),
            (Theme::System, "跟随系统"),
        ]
        .into_iter()
        .enumerate()
        {
            self.badge(
                p,
                dc,
                Area {
                    x: x + 104 + i as i32 * 132,
                    y: 181,
                    w: 124,
                    h: 32,
                },
                title,
                Hit::Theme(theme),
                self.model.theme == theme,
            );
        }
        self.label(
            dc,
            "快捷键",
            Area {
                x: x + 24,
                y: 234,
                w: 180,
                h: 22,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        p.round(
            Area {
                x: x + 24,
                y: 256,
                w: 348,
                h: 36,
            },
            8,
            Some(self.palette.input),
            Some((self.palette.border, 1.0)),
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + 384,
                y: 256,
                w: 112,
                h: 36,
            },
            "保存快捷键",
            Hit::Accept,
            true,
        );
        self.label(
            dc,
            "启动与外观即时生效 · 快捷键修改后保存",
            Area {
                x: x + 24,
                y: 303,
                w: 472,
                h: 21,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
    }
    fn draw_sync(&mut self, p: &Painter, dc: HDC) {
        let width = (self.width - 48).min(720);
        let x = (self.width - width) / 2;
        p.round(
            Area {
                x,
                y: 65,
                w: width,
                h: 267,
            },
            14,
            Some(self.palette.surface),
            Some((self.palette.border, 0.8)),
        );
        self.label(
            dc,
            "iCloud Drive 加密同步",
            Area {
                x: x + 20,
                y: 78,
                w: width - 40,
                h: 28,
            },
            self.palette.text,
            2,
            DT_SINGLELINE,
        );
        if let Dialog::SyncConflict(entity, index) = &self.dialog {
            let can_both = entity.starts_with("item:");
            let heads = self.model.sync.heads(entity);
            if let Some(op) = heads.get(*index % heads.len().max(1)) {
                if let pasteleaf::sync::Value::Record(pasteleaf::sync::Record {
                    content: Content::Image { file, .. },
                    ..
                }) = &op.value
                {
                    if let Ok(image) = self.store.read_image(file) {
                        let mut thumb = image::imageops::thumbnail(
                            &image,
                            self.px(width / 2 - 36) as u32,
                            self.px(136) as u32,
                        );
                        for pixel in thumb.pixels_mut() {
                            pixel.0.swap(0, 2);
                        }
                        draw_bitmap(
                            dc,
                            &thumb,
                            self.px(x + width / 2 + 12),
                            self.px(130),
                            thumb.width() as i32,
                            thumb.height() as i32,
                        );
                    }
                }
            }
            self.label(
                dc,
                "查看每个版本后选择保留；正文和标签按版本一起保留",
                Area {
                    x: x + 20,
                    y: 101,
                    w: width - 40,
                    h: 20,
                },
                self.palette.muted,
                3,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            for (i, (text, action)) in [
                ("下一版本", SyncAction::NextVersion),
                ("保留此版", SyncAction::Keep),
                (
                    if can_both {
                        "两版都留"
                    } else {
                        "返回设置"
                    },
                    if can_both {
                        SyncAction::KeepBoth
                    } else {
                        SyncAction::Open
                    },
                ),
            ]
            .into_iter()
            .enumerate()
            {
                self.badge(
                    p,
                    dc,
                    Area {
                        x: x + 20 + i as i32 * ((width - 48) / 3),
                        y: 280,
                        w: (width - 60) / 3,
                        h: 32,
                    },
                    text,
                    Hit::Sync(action),
                    false,
                );
            }
            self.badge(
                p,
                dc,
                Area {
                    x: x + width - 90,
                    y: 75,
                    w: 70,
                    h: 27,
                },
                "返回",
                Hit::Sync(SyncAction::Open),
                false,
            );
            return;
        }
        let connected = self.model.sync.config.is_some();
        self.label(
            dc,
            "专用同步文件夹",
            Area {
                x: x + 24,
                y: 106,
                w: width - 48,
                h: 18,
            },
            self.palette.muted,
            3,
            DT_SINGLELINE,
        );
        p.round(
            Area {
                x: x + 20,
                y: 121,
                w: width - 130,
                h: 30,
            },
            6,
            Some(self.palette.input),
            Some((self.palette.border, 0.8)),
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + width - 102,
                y: 121,
                w: 80,
                h: 30,
            },
            "选择…",
            Hit::Sync(SyncAction::Browse),
            false,
        );
        for (offset, title) in [
            (24, "同步密码（至少 12 个字符）"),
            (width / 2 + 7, "确认密码（创建时填写）"),
        ] {
            self.label(
                dc,
                title,
                Area {
                    x: x + offset,
                    y: 157,
                    w: (width - 62) / 2,
                    h: 18,
                },
                self.palette.muted,
                3,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            p.round(
                Area {
                    x: x + offset - 4,
                    y: 175,
                    w: (width - 62) / 2 + 8,
                    h: 30,
                },
                6,
                Some(self.palette.input),
                Some((self.palette.border, 0.8)),
            );
        }
        self.badge(
            p,
            dc,
            Area {
                x: x + 20,
                y: 214,
                w: (width - 48) / 2,
                h: 28,
            },
            if self.sync_runtime.all {
                "范围：全部记录"
            } else {
                "范围：仅收藏"
            },
            Hit::Sync(SyncAction::Scope),
            self.sync_runtime.all,
        );
        self.badge(
            p,
            dc,
            Area {
                x: x + width / 2 + 4,
                y: 214,
                w: (width - 48) / 2,
                h: 28,
            },
            if self.sync_runtime.remember {
                "记住本机密钥：开"
            } else {
                "记住本机密钥：关"
            },
            Hit::Sync(SyncAction::Remember),
            self.sync_runtime.remember,
        );
        let actions = if connected {
            vec![
                ("解锁", SyncAction::Unlock),
                ("立即同步", SyncAction::Now),
                (
                    if self.model.sync.config.as_ref().is_some_and(|c| c.paused) {
                        "恢复"
                    } else {
                        "暂停"
                    },
                    SyncAction::Pause,
                ),
                ("冲突", SyncAction::Conflicts),
                ("断开", SyncAction::Disconnect),
            ]
        } else {
            vec![
                ("创建并启用", SyncAction::Create),
                ("加入并启用", SyncAction::Join),
            ]
        };
        let count = actions.len() as i32;
        let step = (width - 40) / count;
        for (i, (title, action)) in actions.into_iter().enumerate() {
            self.badge(
                p,
                dc,
                Area {
                    x: x + 20 + i as i32 * step,
                    y: 250,
                    w: step - 5,
                    h: 30,
                },
                title,
                Hit::Sync(action),
                false,
            );
        }
        let status = if !connected {
            "未启用 · 仅同步文件加密，本地记录保持原样".to_string()
        } else if self.model.sync.config.as_ref().is_some_and(|c| c.paused) {
            "已暂停".into()
        } else if self.sync_runtime.key.is_none() {
            "等待解锁 · 本地记录仍可使用".into()
        } else {
            self.sync_runtime.status.clone()
        };
        let status = if let Some(last) = self.sync_runtime.last {
            format!("{status} · {} 秒前", timestamp().saturating_sub(last))
        } else {
            status
        };
        let area = Area {
            x: x + 20,
            y: 292,
            w: width - 40,
            h: 25,
        };
        self.label(
            dc,
            &status,
            area,
            self.palette.muted,
            3,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        self.tips.push((area, status));
        self.badge(
            p,
            dc,
            Area {
                x: x + width - 76,
                y: 77,
                w: 56,
                h: 25,
            },
            "关闭",
            Hit::Cancel,
            false,
        );
    }
    pub(super) fn paint_popup(&self) {
        let Some(menu) = self.popup.as_ref() else {
            return;
        };
        // SAFETY: the separate owned popup is double-buffered. Native EDITs
        // remain visible beneath it and retain their selection/undo state.
        unsafe {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(self.popup_hwnd, &mut ps);
            let mut bounds: RECT = zeroed();
            GetClientRect(self.popup_hwnd, &mut bounds);
            let mem = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, bounds.right.max(1), bounds.bottom.max(1));
            if mem.is_null() || bitmap.is_null() {
                if !mem.is_null() {
                    DeleteDC(mem);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                EndPaint(self.popup_hwnd, &ps);
                return;
            }
            let old = SelectObject(mem, bitmap);
            let brush = CreateSolidBrush(colorref(self.palette.surface));
            FillRect(mem, &bounds, brush);
            DeleteObject(brush);
            let p = Painter::new(mem, self.scale);
            p.round(
                Area {
                    x: 0,
                    y: 0,
                    w: menu.width(),
                    h: menu.height(),
                },
                10,
                Some(self.palette.surface),
                Some((self.palette.border, 1.0)),
            );
            let top = if menu.page().title.is_empty() { 8 } else { 40 };
            if top == 40 {
                let title = if menu.pages.len() > 1 {
                    format!("‹  {}", menu.page().title)
                } else {
                    menu.page().title.clone()
                };
                self.label(
                    mem,
                    &title,
                    Area {
                        x: 16,
                        y: 12,
                        w: 170,
                        h: 24,
                    },
                    self.palette.muted,
                    3,
                    DT_SINGLELINE | DT_END_ELLIPSIS,
                );
                if menu.page().items.len() > menu.rows() {
                    self.label(
                        mem,
                        "↑↓ / 滚轮",
                        Area {
                            x: 184,
                            y: 12,
                            w: 80,
                            h: 24,
                        },
                        self.palette.muted,
                        3,
                        DT_SINGLELINE | DT_RIGHT,
                    );
                }
            }
            for (row, (index, item)) in menu
                .page()
                .items
                .iter()
                .enumerate()
                .skip(menu.first)
                .take(menu.rows())
                .enumerate()
            {
                let y = top + row as i32 * 36;
                if item.label.is_empty() {
                    p.round(
                        Area {
                            x: 16,
                            y: y + 17,
                            w: menu.width() - 32,
                            h: 1,
                        },
                        0,
                        Some(self.palette.border),
                        None,
                    );
                    continue;
                }
                if item.enabled && menu.selected == Some(index) {
                    p.round(
                        Area {
                            x: 7,
                            y,
                            w: menu.width() - 14,
                            h: 34,
                        },
                        6,
                        Some(self.palette.hover),
                        None,
                    );
                }
                let hint = if item.children.is_empty() {
                    item.hint.as_str()
                } else {
                    "›"
                };
                let hint_width = if hint.is_empty() {
                    0
                } else if !item.children.is_empty() {
                    20
                } else {
                    110
                };
                self.label(
                    mem,
                    &item.label,
                    Area {
                        x: 18,
                        y: y + 8,
                        w: menu.width() - 38 - hint_width,
                        h: 24,
                    },
                    if item.enabled {
                        self.palette.text
                    } else {
                        self.palette.muted
                    },
                    0,
                    DT_SINGLELINE | DT_END_ELLIPSIS,
                );
                self.label(
                    mem,
                    hint,
                    Area {
                        x: menu.width() - 18 - hint_width,
                        y: y + 9,
                        w: hint_width,
                        h: 24,
                    },
                    self.palette.muted,
                    3,
                    DT_SINGLELINE | DT_RIGHT,
                );
            }
            BitBlt(dc, 0, 0, bounds.right, bounds.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bitmap);
            DeleteDC(mem);
            EndPaint(self.popup_hwnd, &ps);
        }
    }
    pub(super) fn draw_content(&mut self, mem: HDC) {
        // SAFETY: caller supplies a live paint or compatible memory DC.
        unsafe {
            let bounds = self.rect(Area {
                x: 0,
                y: 0,
                w: self.width,
                h: PANEL_H,
            });
            let brush = CreateSolidBrush(colorref(self.palette.background));
            FillRect(mem, &bounds, brush);
            DeleteObject(brush);
        }
        {
            let p = Painter::new(mem, self.scale);
            self.hits.clear();
            self.tips.clear();
            self.draw_header(&p, mem);
            if self.dialog == Dialog::None {
                self.draw_cards(&p, mem);
            } else {
                self.draw_dialog(&p, mem);
            }
            let status = if !self.status.is_empty() {
                self.status.clone()
            } else if self.model.paused {
                "●  已暂停记录".into()
            } else {
                format!("{} 条内容", self.visible.len())
            };
            self.label(
                mem,
                &status,
                Area {
                    x: 24,
                    y: 346,
                    w: if self.dialog == Dialog::None {
                        (self.width - 450).max(150)
                    } else {
                        self.width - 48
                    },
                    h: 19,
                },
                self.palette.muted,
                3,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            if self.dialog == Dialog::None {
                self.label(
                    mem,
                    "← → 选择     ↵ 粘贴     Space 预览",
                    Area {
                        x: self.width - 395,
                        y: 346,
                        w: 300,
                        h: 19,
                    },
                    self.palette.muted,
                    3,
                    DT_SINGLELINE | DT_RIGHT,
                );
                self.badge(
                    &p,
                    mem,
                    Area {
                        x: self.width - 78,
                        y: 338,
                        w: 27,
                        h: 26,
                    },
                    "‹",
                    Hit::Previous,
                    false,
                );
                self.badge(
                    &p,
                    mem,
                    Area {
                        x: self.width - 45,
                        y: 338,
                        w: 27,
                        h: 26,
                    },
                    "›",
                    Hit::Next,
                    false,
                );
            }
        }
        let p = Painter::new(mem, self.scale);
        self.draw_tooltip(&p, mem);
    }
    pub(super) fn paint(&mut self) {
        // SAFETY: Begin/EndPaint and memory DC/bitmap lifetimes are paired. Fonts
        // and bitmaps are restored before their owning objects are destroyed.
        unsafe {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(self.hwnd, &mut ps);
            let mut bounds: RECT = zeroed();
            GetClientRect(self.hwnd, &mut bounds);
            let mem = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, bounds.right.max(1), bounds.bottom.max(1));
            if mem.is_null() || bitmap.is_null() {
                if !mem.is_null() {
                    DeleteDC(mem);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                EndPaint(self.hwnd, &ps);
                return;
            }
            let old = SelectObject(mem, bitmap);
            let bg = CreateSolidBrush(colorref(self.palette.background));
            FillRect(mem, &bounds, bg);
            DeleteObject(bg);
            self.draw_content(mem);
            BitBlt(dc, 0, 0, bounds.right, bounds.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bitmap);
            DeleteDC(mem);
            EndPaint(self.hwnd, &ps);
        }
    }
}
fn draw_bitmap(dc: HDC, img: &image::RgbaImage, x: i32, y: i32, w: i32, h: i32) {
    // SAFETY: BGRA32 pixels and header dimensions agree; buffer lives through call.
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = img.width() as i32;
        info.bmiHeader.biHeight = -(img.height() as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(
            dc,
            x,
            y,
            w,
            h,
            0,
            0,
            img.width() as i32,
            img.height() as i32,
            img.as_ptr() as _,
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}
