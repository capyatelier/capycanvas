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
    pub const fn base_choices(self) -> [HexColor; 4] {
        match self {
            Self::Dark => [
                HexColor([31; 3]),
                HexColor([41; 3]),
                HexColor([51; 3]),
                HexColor([61; 3]),
            ],
            Self::Light => [
                HexColor([164; 3]),
                HexColor([184; 3]),
                HexColor([204; 3]),
                HexColor([222; 3]),
            ],
        }
    }
}

pub const ACCENTS: [(&str, HexColor); 9] = [
    ("Blue", HexColor([0x35, 0x84, 0xe4])),
    ("Teal", HexColor([0x21, 0x90, 0xa4])),
    ("Green", HexColor([0x3a, 0x94, 0x4a])),
    ("Yellow", HexColor([0xc8, 0x88, 0x00])),
    ("Orange", HexColor([0xed, 0x5b, 0x00])),
    ("Red", HexColor([0xe6, 0x2d, 0x42])),
    ("Pink", HexColor([0xd5, 0x61, 0x99])),
    ("Purple", HexColor([0x91, 0x41, 0xac])),
    ("Slate", HexColor([0x6f, 0x83, 0x96])),
];
pub const DEFAULT_ACCENT: HexColor = ACCENTS[0].1;
const TINT_CHROMA: f64 = 0.05;

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
    fn oklab(self) -> [f64; 3] {
        layer_core::color::oklab::to_lab(
            self.0
                .map(|v| crate::srgb_to_linear(v as f32 / 255.0) as f64),
        )
    }
    fn oklch(lightness: f64, chroma: f64, hue: f64) -> Self {
        let rgb =
            |c: f64| layer_core::color::oklab::from_lab([lightness, c * hue.cos(), c * hue.sin()]);
        let fits = |c| rgb(c).iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v));
        let chroma = if fits(chroma) {
            chroma
        } else {
            let (mut fitting, mut over) = (0.0, chroma);
            for _ in 0..24 {
                let mid = (fitting + over) / 2.0;
                if fits(mid) { fitting = mid } else { over = mid }
            }
            fitting
        };
        Self(rgb(chroma).map(|v| {
            (layer_core::color::srgb_encode(v.clamp(0.0, 1.0) as f32) * 255.0).round() as u8
        }))
    }
    fn tint(self, accent: HexColor, shift: f64) -> Self {
        let [_, a, b] = accent.oklab();
        Self::oklch(
            (self.oklab()[0] + shift).clamp(0.0, 1.0),
            a.hypot(b).min(TINT_CHROMA),
            b.atan2(a),
        )
    }
    pub fn contrasting(self) -> Self {
        Self(if self.oklab()[0] > 0.72 {
            [46, 46, 50]
        } else {
            [255; 3]
        })
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
    pub accent: HexColor,
    pub accent_foreground: HexColor,
    pub selection: HexColor,
    pub header_selection: HexColor,
    pub header_selection_hover: HexColor,
    /// Prepared once when settings/theme change, not converted per frame.
    pub surround_linear: [f32; 4],
    pub glass: crate::GlassPalette,
}
impl Settings {
    pub fn palette(
        &self,
        theme: Theme,
        platform: Platform,
        system_accent: Option<HexColor>,
    ) -> ThemePalette {
        let dark = theme == Theme::Dark;
        let bg = if dark {
            self.dark_base
        } else {
            self.light_base
        };
        let anchor = theme.default_base().0[0];
        let surface = |d, l| bg.surface(anchor, if dark { d } else { l });
        let android = platform == Platform::Android;
        let accent = self.accent.or(system_accent).unwrap_or(DEFAULT_ACCENT);
        let header = surface([82; 3], [196; 3]);
        let panel = surface([65; 3], [237; 3]);
        let tabbar = surface([46; 3], [210; 3]);
        let selection = surface([82; 3], [213; 3]).tint(accent, 0.0);
        let header_selection = header.tint(accent, 0.0);
        ThemePalette {
            bg,
            panel,
            tabbar,
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
            accent,
            accent_foreground: accent.contrasting(),
            selection,
            header_selection,
            header_selection_hover: header.tint(accent, if dark { 0.03 } else { -0.03 }),
            surround_linear: bg.linear(),
            glass: crate::GlassPalette::new(self.transparency, dark, bg, panel, tabbar, selection, header_selection),
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
            let p = Settings::default().palette(theme, Platform::Web, None);
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
            let p = settings.palette(theme, Platform::Web, None);
            let old = Settings::default().palette(theme, Platform::Web, None);
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

    fn hue(color: HexColor) -> f64 {
        let [_, a, b] = color.oklab();
        b.atan2(a).to_degrees()
    }

    #[test]
    fn default_accent_tints_keep_the_accent_hue() {
        let dark = Settings::default().palette(Theme::Dark, Platform::Gtk, None);
        let light = Settings::default().palette(Theme::Light, Platform::Gtk, None);
        assert_eq!(dark.accent, DEFAULT_ACCENT);
        assert_eq!(dark.selection, dark.header_selection);
        assert_eq!(dark.selection.to_string(), "#40546e");
        assert_eq!(light.header_selection.to_string(), "#afc6e5");
        assert_eq!(light.selection.to_string(), "#c0d7f6");
        for tint in [dark.selection, light.header_selection, light.selection] {
            assert!((hue(tint) - hue(DEFAULT_ACCENT)).abs() < 1.0);
        }
        assert!(dark.header_selection_hover.oklab()[0] > dark.header_selection.oklab()[0]);
        assert!(light.header_selection_hover.oklab()[0] < light.header_selection.oklab()[0]);
    }

    #[test]
    fn accents_resolve_saved_then_system_then_default() {
        let teal = ACCENTS[1].1;
        let red = ACCENTS[5].1;
        let settings = Settings::default();
        assert_eq!(
            settings
                .palette(Theme::Dark, Platform::Gtk, Some(teal))
                .accent,
            teal
        );
        let saved = Settings {
            accent: Some(red),
            ..Settings::default()
        };
        let p = saved.palette(Theme::Light, Platform::Gtk, Some(teal));
        assert_eq!(p.accent, red);
        assert!((hue(p.selection) - hue(red)).abs() < 1.0);
        for (_, accent) in ACCENTS {
            for theme in [Theme::Dark, Theme::Light] {
                let p = Settings {
                    accent: Some(accent),
                    ..Settings::default()
                }
                .palette(theme, Platform::Gtk, None);
                let lightness = |c: HexColor| c.oklab()[0];
                let blue = Settings::default().palette(theme, Platform::Gtk, None);
                assert!((lightness(p.selection) - lightness(blue.selection)).abs() < 0.01);
                assert!(
                    (lightness(p.header_selection) - lightness(blue.header_selection)).abs() < 0.01
                );
            }
        }
        for (_, accent) in ACCENTS {
            let p = Settings {
                accent: Some(accent),
                ..Settings::default()
            }
            .palette(Theme::Light, Platform::Gtk, None);
            assert_eq!(
                p.accent_foreground,
                HexColor([255; 3]),
                "libadwaita keeps white text on {accent}"
            );
        }
        let pale = Settings {
            accent: Some(HexColor([0xf6, 0xe0, 0x7a])),
            ..Settings::default()
        }
        .palette(Theme::Dark, Platform::Gtk, None);
        assert_eq!(pale.accent_foreground, HexColor([46, 46, 50]));
        let grey = Settings {
            accent: Some(HexColor([128; 3])),
            ..Settings::default()
        }
        .palette(Theme::Light, Platform::Gtk, None);
        let [r, g, b] = grey.selection.0;
        assert!(
            r == g && g == b,
            "grey accent keeps grey tints: {}",
            grey.selection
        );
    }

    #[test]
    fn light_tints_follow_the_light_base() {
        let darker = Settings {
            light_base: HexColor([0x90; 3]),
            ..Settings::default()
        };
        let p = darker.palette(Theme::Light, Platform::Gtk, None);
        let old = Settings::default().palette(Theme::Light, Platform::Gtk, None);
        assert!(p.header_selection.oklab()[0] < old.header_selection.oklab()[0] - 0.05);
        assert!(p.selection.oklab()[0] > p.header_selection.oklab()[0]);
    }
}
