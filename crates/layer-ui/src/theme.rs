//! Base-relative sRGB surfaces. Only UI appearance changes, never artwork.
use crate::{Platform, Settings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
}
impl Theme {
    pub const fn default_base(self) -> HexColor {
        HexColor([if matches!(self, Self::Dark) { 51 } else { 184 }; 3])
    }
}

/// Opaque, validated six-digit sRGB. Serialized as a canonical #rrggbb string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HexColor(pub [u8; 3]);
impl TryFrom<String> for HexColor {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let digits = value.strip_prefix('#').unwrap_or("");
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Enter a hex color such as #333333.".into());
        }
        Ok(Self(std::array::from_fn(|i| {
            u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap()
        })))
    }
}
impl From<HexColor> for String {
    fn from(color: HexColor) -> Self {
        color.to_string()
    }
}
impl std::fmt::Display for HexColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}
impl HexColor {
    pub fn linear(self) -> [f32; 4] {
        let [r, g, b] = self.0.map(|v| crate::srgb_to_linear(v as f32 / 255.0));
        [r, g, b, 1.0]
    }
    /// Reconstruct the old role's black/white mix relative to its default base,
    /// then apply that mix to the new base. Always in gamut, no clipping needed.
    fn surface(self, anchor: u8, reference: [u8; 3]) -> Self {
        Self(std::array::from_fn(|i| {
            let base = self.0[i] as f64;
            let anchor = anchor as f64;
            let target = reference[i] as f64;
            let value = if target <= anchor {
                base * target / anchor
            } else {
                base + (255.0 - base) * (target - anchor) / (255.0 - anchor)
            };
            value.round() as u8
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ThemePalette {
    pub bg: HexColor,
    pub panel: HexColor,
    pub tabbar: HexColor,
    pub input: HexColor,
    pub view: HexColor,
    pub settings: HexColor,
    pub sidebar: HexColor,
    pub sidebar_backdrop: HexColor,
    pub dialog: HexColor,
    pub card: HexColor,
    pub thumb: HexColor,
    pub text: HexColor,
    pub settings_secondary: HexColor,
    pub button: HexColor,
    /// Prepared once when settings/theme change, not converted per frame.
    pub surround_linear: [f32; 4],
}
impl Settings {
    pub fn palette(&self, theme: Theme, platform: Platform) -> ThemePalette {
        let dark = theme == Theme::Dark;
        let bg = if dark {
            self.dark_base
        } else {
            self.light_base
        };
        let anchor = theme.default_base().0[0];
        let surface = |d, l| bg.surface(anchor, if dark { d } else { l });
        let android = platform == Platform::Android;
        ThemePalette {
            bg,
            panel: surface([65; 3], [237; 3]),
            tabbar: surface([46; 3], [222; 3]),
            input: surface([51; 3], [250; 3]),
            view: surface([43; 3], [228; 3]),
            settings: surface([51; 3], if android { [250; 3] } else { [250, 250, 251] }),
            sidebar: surface(
                if android { [46; 3] } else { [46, 46, 50] },
                if android { [237; 3] } else { [235, 235, 237] },
            ),
            sidebar_backdrop: surface([40, 40, 44], [242, 242, 244]),
            dialog: surface([54, 54, 58], [250, 250, 251]),
            card: surface([65; 3], [255; 3]),
            thumb: surface([211; 3], [250; 3]),
            text: HexColor(if dark { [250, 250, 251] } else { [46, 46, 50] }),
            settings_secondary: HexColor(if dark { [188; 3] } else { [102; 3] }),
            button: HexColor([if dark { 255 } else { 0 }; 3]),
            surround_linear: bg.linear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_strict_and_canonical() {
        for invalid in [
            "",
            "123456",
            "#abc",
            "#11223344",
            "#12345g",
            " #123456",
            "#ééé",
        ] {
            assert!(HexColor::try_from(invalid.to_owned()).is_err());
        }
        let color: HexColor = serde_json::from_str("\"#aAbBcC\"").unwrap();
        assert_eq!(color.to_string(), "#aabbcc");
        assert_eq!(serde_json::to_string(&color).unwrap(), "\"#aabbcc\"");
    }

    #[test]
    fn default_surfaces_are_exact_and_ordered() {
        for theme in [Theme::Dark, Theme::Light] {
            let anchor = theme.default_base();
            for target in 0..=255 {
                assert_eq!(
                    anchor.surface(anchor.0[0], [target; 3]),
                    HexColor([target; 3])
                );
            }
            let p = Settings::default().palette(theme, Platform::Web);
            assert_eq!(p.bg, anchor);
            assert_eq!(
                p.panel,
                HexColor([if theme == Theme::Dark { 65 } else { 237 }; 3])
            );
            assert!(p.tabbar.0[0] < p.panel.0[0]);
        }
    }

    #[test]
    fn colored_bases_preserve_roles_and_leave_foregrounds_alone() {
        let settings = Settings {
            dark_base: HexColor([28, 44, 60]),
            light_base: HexColor([192, 180, 156]),
            ..Settings::default()
        };
        for theme in [Theme::Dark, Theme::Light] {
            let p = settings.palette(theme, Platform::Web);
            let old = Settings::default().palette(theme, Platform::Web);
            assert_eq!(p.text, old.text);
            assert_ne!(p.panel, old.panel);
            for i in 0..3 {
                assert!(p.tabbar.0[i] < p.panel.0[i]);
                assert!(p.panel.0[i] > p.bg.0[i]);
            }
            assert_eq!(p.surround_linear, p.bg.linear());
        }
        assert_eq!(HexColor([0; 3]).linear(), [0.0, 0.0, 0.0, 1.0]);
        assert!((HexColor([1; 3]).linear()[0] - 1.0 / 255.0 / 12.92).abs() < 1e-7);
    }
}
