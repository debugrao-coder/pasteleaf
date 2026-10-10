use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    #[default]
    Dark,
    System,
}

impl Theme {
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::System => system_dark,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub background: u32,
    pub surface: u32,
    pub hover: u32,
    pub input: u32,
    pub border: u32,
    pub shadow: u32,
    pub text: u32,
    pub muted: u32,
    pub code: u32,
    pub selected: u32,
    pub accent: u32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark: true,
                background: 0x17191F,
                surface: 0x252830,
                hover: 0x303440,
                input: 0x1D2027,
                border: 0x414754,
                shadow: 0x101217,
                text: 0xF0F2F6,
                muted: 0xADB5C5,
                code: 0xC8BAF0,
                selected: 0x293C5C,
                accent: 0x83B2FF,
            }
        } else {
            Self {
                dark: false,
                background: 0xEDEEF3,
                surface: 0xFFFFFF,
                hover: 0xE2E3E9,
                input: 0xFAFAFC,
                border: 0xD7D9E0,
                shadow: 0xD8DBE2,
                text: 0x30323A,
                muted: 0x686E7C,
                code: 0x655B85,
                selected: 0xDCEAFE,
                accent: 0x488CFA,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_theme_overrides_system() {
        assert!(Theme::Dark.is_dark(false));
        assert!(!Theme::Light.is_dark(true));
        assert!(Theme::System.is_dark(true));
        assert!(!Theme::System.is_dark(false));
    }
    #[test]
    fn existing_library_without_theme_opens_in_dark_mode() {
        let mut old = serde_json::to_value(crate::model::Model::default()).unwrap();
        old.as_object_mut().unwrap().remove("theme");
        let restored: crate::model::Model = serde_json::from_value(old).unwrap();
        assert_eq!(restored.theme, Theme::Dark);
        restored.validate().unwrap();
    }
    #[test]
    fn dark_text_has_readable_contrast() {
        fn luminance(c: u32) -> f64 {
            let linear = |n: u32| {
                let v = n as f64 / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c >> 16) + 0.7152 * linear((c >> 8) & 255) + 0.0722 * linear(c & 255)
        }
        let p = Palette::new(true);
        for foreground in [p.text, p.muted, p.code] {
            for background in [p.background, p.surface, p.input, p.hover] {
                assert!((luminance(foreground) + 0.05) / (luminance(background) + 0.05) >= 4.5);
            }
        }
    }
}
