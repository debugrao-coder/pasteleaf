//! Pure geometry shared by native layout and portable DPI/work-area regression tests.
use crate::design_tokens::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn right(self) -> f32 {
        self.x + self.width
    }
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub physical: PixelRect,
    pub logical_width: f32,
    pub logical_height: f32,
}

pub fn scale_for_dpi(dpi: u32) -> f32 {
    if dpi == 0 {
        1.
    } else {
        dpi as f32 / 96.
    }
}

pub fn placement(work: PixelRect, scale: f32, shelf: bool) -> Placement {
    let scale = if scale.is_finite() && scale > 0. {
        scale
    } else {
        1.
    };
    let work_width = work.width.max(1);
    let work_height = work.height.max(1);
    let (width, height) = if shelf {
        (
            work_width,
            ((PANEL_HEIGHT * scale).round() as i32).min(work_height),
        )
    } else {
        // Reading windows keep their own comfortable size and never inherit shelf width.
        let width = (work_width as f32 / scale - 48.).clamp(1., 820.);
        let height = (work_height as f32 / scale - 48.).clamp(1., 620.);
        (
            (width * scale).round() as i32,
            (height * scale).round() as i32,
        )
    };
    let x = if shelf {
        work.x
    } else {
        work.x + (work_width - width) / 2
    };
    let y = if shelf {
        work.y + work_height - height - (PANEL_BOTTOM_INSET * scale).round() as i32
    } else {
        work.y + (work_height - height) / 2
    };
    Placement {
        physical: PixelRect {
            x,
            y,
            width,
            height,
        },
        logical_width: width as f32 / scale,
        logical_height: height as f32 / scale,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub search: Rect,
    pub history: Option<Rect>,
    pub category_start: f32,
    pub category_end: f32,
    /// Collections, sync, settings, and hide; same order in native controls.
    pub right: [Rect; 4],
}
pub fn header(width: f32) -> Header {
    let widths = [56., 64., 64., 54.];
    let right_start = width - PANEL_PADDING - widths.iter().sum::<f32>() - 3. * CATEGORY_GAP;
    let mut x = right_start;
    let right = widths.map(|w| {
        let rect = Rect::new(x, PANEL_PADDING, w, HEADER_HEIGHT);
        x += w + CATEGORY_GAP;
        rect
    });
    let search = Rect::new(
        PANEL_PADDING,
        PANEL_PADDING,
        (right_start - PANEL_PADDING - GAP).clamp(1., 260.),
        HEADER_HEIGHT,
    );
    let history =
        (width >= 900.).then(|| Rect::new(search.right() + GAP, PANEL_PADDING, 66., HEADER_HEIGHT));
    Header {
        search,
        history,
        category_start: history
            .map(|r| r.right() + CATEGORY_GAP)
            .unwrap_or(search.right() + GAP),
        category_end: right_start - GAP,
        right,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Previous,
    Next,
    Paste,
    Plain,
    Preview,
    Favorite,
    Queue,
    Enable,
    More,
}
#[derive(Clone, Copy, Debug)]
pub struct Button {
    pub action: Action,
    pub rect: Rect,
}

pub fn footer(width: f32, height: f32, has_queue: bool, enable: bool) -> Vec<Button> {
    let mut candidates = vec![
        (Action::Previous, 36.),
        (Action::Next, 36.),
        (Action::Paste, 90.),
        (Action::Plain, 80.),
        (Action::Preview, 64.),
        (Action::Favorite, 64.),
    ];
    if has_queue {
        candidates.push((Action::Queue, 110.));
    }
    let y = height - FOOTER_HEIGHT - PANEL_BOTTOM_PADDING - PANEL_PADDING;
    let right = width - PANEL_PADDING - if enable { 98. + CATEGORY_GAP } else { 0. };
    let wanted = candidates.iter().map(|(_, w)| *w).sum::<f32>()
        + CATEGORY_GAP * (candidates.len() - 1) as f32;
    let overflow = PANEL_PADDING + wanted > right;
    let limit = right - if overflow { 64. + CATEGORY_GAP } else { 0. };
    let mut x = PANEL_PADDING;
    let mut result = Vec::new();
    for (action, w) in candidates {
        if x + w > limit {
            break;
        }
        result.push(Button {
            action,
            rect: Rect::new(x, y, w, FOOTER_HEIGHT),
        });
        x += w + CATEGORY_GAP;
    }
    if overflow {
        result.push(Button {
            action: Action::More,
            rect: Rect::new(right - 64., y, 64., FOOTER_HEIGHT),
        });
    }
    if enable {
        result.push(Button {
            action: Action::Enable,
            rect: Rect::new(width - PANEL_PADDING - 98., y, 98., FOOTER_HEIGHT),
        });
    }
    result
}

#[derive(Clone, Copy, Debug)]
pub struct Shelf {
    pub card_width: f32,
    pub card_height: f32,
    pub card_y: f32,
    pub visible_cards: usize,
    pub status: Rect,
}
impl Shelf {
    pub fn new(width: f32, height: f32) -> Self {
        let card_width = CARD_WIDTH.min((width - 2. * PANEL_PADDING).max(1.));
        let card_y = HEADER_HEIGHT + 2. * PANEL_PADDING;
        let action_y = height - FOOTER_HEIGHT - PANEL_BOTTOM_PADDING - PANEL_PADDING;
        Self {
            card_width,
            card_height: CARD_HEIGHT.min((action_y - CARD_RADIUS - card_y).max(1.)),
            card_y,
            visible_cards: ((width - 2. * PANEL_PADDING + GAP) / (card_width + GAP))
                .floor()
                .max(1.) as usize,
            status: Rect::new(PANEL_PADDING, height - 18., width - 2. * PANEL_PADDING, 18.),
        }
    }
    pub fn card(self, index: usize) -> Rect {
        Rect::new(
            PANEL_PADDING + index as f32 * (self.card_width + GAP),
            self.card_y,
            self.card_width,
            self.card_height,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub header: Rect,
    pub body: Rect,
    pub metadata: Rect,
    pub source: Rect,
    pub state: Rect,
}
impl Card {
    pub fn new(width: f32, height: f32, metadata: bool, state: bool) -> Self {
        let header = Rect::new(8., 8., width - 16., HEADER_HEIGHT);
        let source_y = height - FOOTER_HEIGHT;
        let metadata_y = source_y - 18. - FOOTER_GAP;
        let body_y = header.bottom() + GAP;
        Self {
            header,
            body: Rect::new(
                PANEL_PADDING,
                body_y,
                width - 2. * PANEL_PADDING,
                ((if metadata { metadata_y } else { source_y }) - FOOTER_GAP - body_y).max(1.),
            ),
            metadata: Rect::new(PANEL_PADDING, metadata_y, width - 2. * PANEL_PADDING, 18.),
            source: Rect::new(
                PANEL_PADDING,
                source_y,
                width - 2. * PANEL_PADDING - if state { 72. + FOOTER_GAP } else { 0. },
                18.,
            ),
            state: Rect::new(width - PANEL_PADDING - 72., source_y, 72., 18.),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disjoint(a: Rect, b: Rect) -> bool {
        a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y
    }
    fn inside(r: Rect, width: f32, height: f32) {
        assert!(
            r.x >= 0. && r.y >= 0. && r.width > 0. && r.height > 0.,
            "{r:?}"
        );
        assert!(
            r.right() <= width && r.bottom() <= height,
            "{r:?} in {width}×{height}"
        );
    }
    #[test]
    fn full_width_shelf_uses_exact_physical_work_area_at_100_150_200_percent() {
        for dpi in [96, 144, 192] {
            for work in [
                PixelRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1040,
                },
                PixelRect {
                    x: -2560,
                    y: -240,
                    width: 2560,
                    height: 1400,
                },
                PixelRect {
                    x: 80,
                    y: -1080,
                    width: 1840,
                    height: 1080,
                },
            ] {
                let p = placement(work, scale_for_dpi(dpi), true);
                assert_eq!(p.physical.x, work.x);
                assert_eq!(p.physical.width, work.width);
                assert_eq!(p.physical.y + p.physical.height, work.y + work.height);
                assert_eq!(
                    p.physical.height,
                    (PANEL_HEIGHT * scale_for_dpi(dpi)).round() as i32
                );
                assert_eq!(p.logical_height, PANEL_HEIGHT);
                println!("DPI {dpi}: work={work:?}; shelf={:?}", p.physical);
            }
        }
    }
    #[test]
    fn shelf_has_no_1400_width_cap_and_no_outer_side_gutters() {
        let work = PixelRect {
            x: 3200,
            y: 0,
            width: 5120,
            height: 1400,
        };
        let p = placement(work, 1., true);
        assert_eq!(p.logical_width, 5120.);
        assert_eq!(p.physical.x, 3200);
        assert_eq!(p.physical.y, 1072);
    }
    #[test]
    fn reading_windows_stay_centered_and_bounded_on_each_monitor() {
        for dpi in [96, 144, 192] {
            let work = PixelRect {
                x: -2560,
                y: 80,
                width: 2560,
                height: 1360,
            };
            let p = placement(work, scale_for_dpi(dpi), false);
            assert!(p.logical_width <= 820. && p.logical_height <= 620.);
            assert!(p.physical.width < work.width && p.physical.height < work.height);
            assert_eq!(p.physical.x, work.x + (work.width - p.physical.width) / 2);
            assert_eq!(p.physical.y, work.y + (work.height - p.physical.height) / 2);
        }
    }
    #[test]
    fn header_cards_footer_and_status_do_not_overlap_across_dpi_and_widths() {
        for dpi in [96, 144, 192] {
            for physical_width in [960, 1280, 1366, 1920, 2560, 3840] {
                let width = physical_width as f32 / scale_for_dpi(dpi);
                let shelf = Shelf::new(width, PANEL_HEIGHT);
                let top = header(width);
                let mut header_controls = vec![top.search];
                header_controls.extend(top.history);
                header_controls.extend(top.right);
                for (i, r) in header_controls.iter().enumerate() {
                    inside(*r, width, PANEL_HEIGHT);
                    for other in header_controls.iter().skip(i + 1) {
                        assert!(disjoint(*r, *other));
                    }
                    assert!(r.bottom() < shelf.card_y);
                }
                for queued in [false, true] {
                    for paused in [false, true] {
                        let buttons = footer(width, PANEL_HEIGHT, queued, paused);
                        for (i, b) in buttons.iter().enumerate() {
                            inside(b.rect, width, PANEL_HEIGHT);
                            assert_eq!(b.rect.y, 268.);
                            assert_eq!(b.rect.height, 32.);
                            assert!(b.rect.bottom() < shelf.status.y);
                            for other in buttons.iter().skip(i + 1) {
                                assert!(disjoint(b.rect, other.rect));
                            }
                            for n in 0..shelf.visible_cards {
                                assert!(disjoint(shelf.card(n), b.rect));
                            }
                        }
                    }
                }
                for n in 0..shelf.visible_cards {
                    inside(shelf.card(n), width, PANEL_HEIGHT);
                }
                assert_eq!(shelf.card_y, 72.);
                assert_eq!(shelf.card_height, 180.);
                assert_eq!(shelf.status.y, 310.);
                inside(shelf.status, width, PANEL_HEIGHT);
            }
        }
    }
    #[test]
    fn narrow_footer_exposes_overflow_without_overlapping_enable() {
        let buttons = footer(480., PANEL_HEIGHT, true, true);
        assert!(buttons.iter().any(|b| b.action == Action::More));
        assert!(buttons.iter().any(|b| b.action == Action::Enable));
        assert!(buttons.iter().any(|b| b.action == Action::Paste));
    }
    #[test]
    fn short_card_regions_keep_preview_metadata_and_source_separate() {
        for metadata in [false, true] {
            for state in [false, true] {
                let c = Card::new(CARD_WIDTH, CARD_HEIGHT, metadata, state);
                let mut regions = vec![c.header, c.body, c.source];
                if metadata {
                    regions.push(c.metadata);
                }
                if state {
                    regions.push(c.state);
                }
                for (i, r) in regions.iter().enumerate() {
                    inside(*r, CARD_WIDTH, CARD_HEIGHT);
                    for other in regions.iter().skip(i + 1) {
                        assert!(disjoint(*r, *other));
                    }
                }
                assert!(c.body.height >= 70.);
                assert!(c.source.height >= 18.);
            }
        }
    }
    #[test]
    fn card_count_uses_the_last_gap_only_between_cards() {
        let exact = 2. * PANEL_PADDING + 3. * CARD_WIDTH + 2. * GAP;
        assert_eq!(Shelf::new(exact, PANEL_HEIGHT).visible_cards, 3);
        assert_eq!(Shelf::new(exact - 1., PANEL_HEIGHT).visible_cards, 2);
    }
}
