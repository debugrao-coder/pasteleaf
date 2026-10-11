//! One Direct2D/DirectWrite component renderer shared by panels and native controls.
use crate::{design_tokens::*, layout, model::Item};
use windows::{
    core::{w, Result},
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
            Gdi::HDC,
        },
    },
};
pub struct Renderer {
    target: ID2D1DCRenderTarget,
    write: IDWriteFactory,
}
fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.,
        g: ((rgb >> 8) & 255) as f32 / 255.,
        b: (rgb & 255) as f32 / 255.,
        a: 1.,
    }
}
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
impl Renderer {
    pub unsafe fn new() -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.,
            dpiY: 96.,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        })?;
        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        Ok(Self { target, write })
    }
    pub unsafe fn begin(&self, dc: HDC, bounds: &RECT, scale: f32) -> Result<()> {
        self.target.BindDC(dc, bounds)?;
        self.target.SetDpi(96. * scale, 96. * scale);
        self.target.BeginDraw();
        Ok(())
    }
    pub unsafe fn end(&self) -> Result<()> {
        self.target.EndDraw(None, None)
    }
    pub unsafe fn fill(&self, r: D2D_RECT_F, rgb: u32) {
        if let Ok(b) = self.target.CreateSolidColorBrush(&color(rgb), None) {
            self.target.FillRectangle(&r, &b);
        }
    }
    pub unsafe fn round(&self, r: D2D_RECT_F, radius: f32, rgb: u32, border: Option<(u32, f32)>) {
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: radius,
            radiusY: radius,
        };
        if let Ok(b) = self.target.CreateSolidColorBrush(&color(rgb), None) {
            self.target.FillRoundedRectangle(&rr, &b);
        }
        if let Some((c, w)) = border {
            if let Ok(b) = self.target.CreateSolidColorBrush(&color(c), None) {
                self.target.DrawRoundedRectangle(&rr, &b, w, None);
            }
        }
    }
    pub unsafe fn text(
        &self,
        text: &str,
        r: D2D_RECT_F,
        size: f32,
        rgb: u32,
        bold: bool,
        mono: bool,
    ) {
        self.draw_text(text, r, size, rgb, bold, mono, false);
    }
    pub unsafe fn text_line(
        &self,
        text: &str,
        r: D2D_RECT_F,
        size: f32,
        rgb: u32,
        bold: bool,
        mono: bool,
    ) {
        self.draw_text(text, r, size, rgb, bold, mono, true);
    }
    // Drawing inputs retain font size; compact layouts change regions, not typography.
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_text(
        &self,
        text: &str,
        r: D2D_RECT_F,
        size: f32,
        rgb: u32,
        bold: bool,
        mono: bool,
        single_line: bool,
    ) {
        if let (Ok(format), Ok(brush)) = (
            self.write.CreateTextFormat(
                if mono {
                    w!("Cascadia Code")
                } else {
                    w!("Segoe UI")
                },
                None,
                if bold {
                    DWRITE_FONT_WEIGHT_SEMI_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("zh-CN"),
            ),
            self.target.CreateSolidColorBrush(&color(rgb), None),
        ) {
            let _ = format.SetWordWrapping(if single_line {
                DWRITE_WORD_WRAPPING_NO_WRAP
            } else {
                DWRITE_WORD_WRAPPING_WRAP
            });
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            if let Ok(ellipsis) = self.write.CreateEllipsisTrimmingSign(&format) {
                let _ = format.SetTrimming(
                    &DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    },
                    &ellipsis,
                );
            }
            let chars: Vec<u16> = text.encode_utf16().collect();
            self.target.DrawText(
                &chars,
                &format,
                &r,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
    // Drawing inputs mirror the native control geometry and state bits.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn button(
        &self,
        label: &str,
        width: f32,
        height: f32,
        p: Palette,
        primary: bool,
        focused: bool,
        pressed: bool,
        disabled: bool,
    ) {
        self.fill(rect(0., 0., width, height), p.background);
        self.round(
            rect(1., 1., width - 2., height - 2.),
            CONTROL_RADIUS,
            if primary {
                p.accent
            } else if pressed {
                p.selection
            } else {
                p.elevated
            },
            Some((
                if focused { p.accent } else { p.border },
                if focused { 2. } else { 1. },
            )),
        );
        self.text(
            label,
            rect(12., (height - 19.) / 2., width - 24., 24.),
            12.,
            if disabled {
                p.muted
            } else if primary {
                p.accent_text
            } else {
                p.text
            },
            primary,
            false,
        );
    }
    // Drawing inputs mirror the native control geometry and state bits.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn card(
        &self,
        item: &Item,
        width: f32,
        height: f32,
        p: Palette,
        selected: bool,
        focused: bool,
        thumbnail: Option<&image::RgbaImage>,
    ) {
        self.fill(rect(0., 0., width, height), p.background);
        self.round(
            rect(1., 2., width - 2., height - 5.),
            CARD_RADIUS,
            p.surface,
            Some((
                if selected || focused {
                    p.accent
                } else {
                    p.border
                },
                if selected || focused { 2. } else { 1. },
            )),
        );
        let has_metadata = matches!(item.kind.as_str(), "image" | "files") || !item.tags.is_empty();
        let has_state = selected || !item.available;
        let regions = layout::Card::new(width, height, has_metadata, has_state);
        let bounds = |r: layout::Rect| rect(r.x, r.y, r.width, r.height);
        let head = match item.type_name() {
            "图片" => p.image_header,
            "文件" => p.file_header,
            "链接" => p.link_header,
            "富文本" => p.rich_header,
            _ => p.text_header,
        };
        self.round(bounds(regions.header), CONTROL_RADIUS, head, None);
        self.text_line(
            item.type_name(),
            rect(20., 15., width - 72., 22.),
            11.,
            p.text,
            true,
            false,
        );
        self.text_line(
            if item.category.is_some() {
                "★"
            } else {
                "↗"
            },
            rect(width - 35., 13., 20., 24.),
            13.,
            p.text,
            false,
            false,
        );
        if item.kind == "image" {
            if let Some(image) = thumbnail {
                self.bitmap(image, bounds(regions.body));
            } else {
                self.text(
                    if item.available {
                        "图片预览加载中"
                    } else {
                        "等待图片附件"
                    },
                    bounds(regions.body),
                    14.,
                    p.muted,
                    false,
                    false,
                );
            }
        } else if item.kind == "files" {
            self.text(
                &item.title(),
                bounds(regions.body),
                17.,
                p.text,
                true,
                false,
            );
        } else {
            let code = item.source.to_lowercase().contains("code")
                || item.text.starts_with("const ")
                || item.text.starts_with("fn ");
            self.text(
                &item.text,
                bounds(regions.body),
                if code { 12. } else { 15. },
                p.text,
                false,
                code,
            );
        }
        if has_metadata {
            let tags = item
                .tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(" · ");
            let metadata = if matches!(item.kind.as_str(), "image" | "files") {
                let value = crate::presentation::content_metadata(item);
                if tags.is_empty() {
                    value
                } else {
                    format!("{value} · {tags}")
                }
            } else {
                tags
            };
            self.text_line(
                &metadata,
                bounds(regions.metadata),
                if item.kind == "files" { 12. } else { 11. },
                p.muted,
                false,
                false,
            );
        }
        self.text_line(
            if item.source.is_empty() {
                "未知来源"
            } else {
                &item.source
            },
            bounds(regions.source),
            10.5,
            p.muted,
            false,
            false,
        );
        if has_state {
            self.text_line(
                if !item.available {
                    "等待附件"
                } else {
                    "已选择"
                },
                bounds(regions.state),
                10.5,
                if item.available { p.muted } else { p.warning },
                false,
                false,
            );
        }
    }
    pub unsafe fn bitmap(&self, image: &image::RgbaImage, r: D2D_RECT_F) {
        let (w, h) = image.dimensions();
        let mut bgra = Vec::with_capacity(image.as_raw().len());
        for p in image.pixels() {
            let a = p[3] as u32;
            bgra.extend([
                (p[2] as u32 * a / 255) as u8,
                (p[1] as u32 * a / 255) as u8,
                (p[0] as u32 * a / 255) as u8,
                p[3],
            ]);
        }
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.,
            dpiY: 96.,
        };
        if let Ok(bitmap) = self.target.CreateBitmap(
            D2D_SIZE_U {
                width: w,
                height: h,
            },
            Some(bgra.as_ptr().cast()),
            w * 4,
            &props,
        ) {
            let fit = ((r.right - r.left) / w as f32).min((r.bottom - r.top) / h as f32);
            let nw = w as f32 * fit;
            let nh = h as f32 * fit;
            let dst = rect(
                r.left + (r.right - r.left - nw) / 2.,
                r.top + (r.bottom - r.top - nh) / 2.,
                nw,
                nh,
            );
            self.target.DrawBitmap(
                &bitmap,
                Some(&dst),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
    }
}
