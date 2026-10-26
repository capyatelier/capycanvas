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
pub const TRANSPARENCY_CHECKER: [HexColor; 2] = [
    HexColor([layer_color::TRANSPARENCY_CHECKER_SRGB[0]; 3]),
    HexColor([layer_color::TRANSPARENCY_CHECKER_SRGB[1]; 3]),
];
pub const TRANSPARENCY_CHECKER_CELL: f32 = 5.;

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
        let [r, g, b] = self
            .0
            .map(|v| layer_core::color::srgb_decode(v as f32 / 255.0));
        [r, g, b, 1.0]
    }
    fn oklab(self) -> [f64; 3] {
        layer_core::color::oklab::to_lab(
            self.0
                .map(|v| layer_core::color::RgbSpace::Srgb.decode(f64::from(v) / 255.0)),
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
    fn relationship(self) -> Self {
        let [lightness, a, b] = self.oklab();
        Self::oklch((lightness - 0.06).max(0.), a.hypot(b), b.atan2(a))
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

type Rgb = [f64; 3];
const WHITE: Rgb = [255.; 3];
const BLACK: Rgb = [0.; 3];
const SHADE: Rgb = [0., 0., 6.];
const CARD_FILL: f64 = 0.08;
const CARD_RING: f64 = 0.03;
const CARD_SHADOW: f64 = 0.07;
const CARD_FAR_SHADOW: f64 = 0.03;
const SCROLL_LINE: f64 = 0.75;
const BORDER_OPACITY: f64 = 0.15;
const HIGHLIGHT: f64 = 0.07;
const POPOVER_BORDER: f64 = 0.14;
const DARK_ROW_DIVIDER: f64 = 0.05;

/// libadwaita 1.9's surfaces and line opacities, the reference our lines match.
struct Adwaita {
    dark: bool,
    window: Rgb,
    sidebar: Rgb,
    popover: Rgb,
    text: Rgb,
    text_alpha: f64,
    card_shade: f64,
    sidebar_border: f64,
    shade: f64,
}
const ADWAITA_DARK: Adwaita = Adwaita {
    dark: true,
    window: [34., 34., 38.],
    sidebar: [46., 46., 50.],
    popover: [54., 54., 58.],
    text: WHITE,
    text_alpha: 1.,
    card_shade: 0.36,
    sidebar_border: 0.36,
    shade: 0.25,
};
const ADWAITA_LIGHT: Adwaita = Adwaita {
    dark: false,
    window: [250., 250., 251.],
    sidebar: [235., 235., 237.],
    popover: WHITE,
    text: SHADE,
    text_alpha: 0.8,
    card_shade: 0.07,
    sidebar_border: 0.07,
    shade: 0.07,
};

fn over(ink: Rgb, alpha: f64, ground: Rgb) -> Rgb {
    std::array::from_fn(|i| ink[i] * alpha + ground[i] * (1. - alpha))
}
fn lightness(color: Rgb) -> f64 {
    layer_core::color::oklab::to_lab(
        color.map(|v| layer_core::color::RgbSpace::Srgb.decode(v / 255.)),
    )[0]
}
fn card(dark: bool, ground: Rgb) -> Rgb {
    if dark { over(WHITE, CARD_FILL, ground) } else { WHITE }
}
fn line(ink: Rgb, alpha: f64) -> crate::GlassColor {
    let [r, g, b] = ink.map(|v| (v / 255.) as f32);
    crate::GlassColor([r, g, b, alpha.min(1.) as f32])
}

/// Opacity of `ink` over `ground` whose smallest OKLab lightness step to any of
/// `touches` equals the step from libadwaita's composited `reference` line to the
/// nearest of `reference_touches`, on the same side. Blends in encoded sRGB, as GTK
/// and browsers composite.
fn matched_alpha(reference: Rgb, reference_touches: &[Rgb], ink: Rgb, ground: Rgb, touches: &[Rgb]) -> f64 {
    let reference = lightness(reference);
    let step = reference_touches
        .iter()
        .map(|&surface| (reference - lightness(surface)).abs())
        .fold(f64::INFINITY, f64::min);
    let darker = lightness(ink) < lightness(ground);
    let levels = touches.iter().map(|&surface| lightness(surface));
    let target = if darker {
        levels.fold(f64::INFINITY, f64::min) - step
    } else {
        levels.fold(f64::NEG_INFINITY, f64::max) + step
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..30 {
        let mid = (low + high) / 2.;
        if (lightness(over(ink, mid, ground)) > target) == darker {
            low = mid
        } else {
            high = mid
        }
    }
    (low + high) / 2.
}
fn same_step(ink: Rgb, alpha: f64, reference: Rgb, our_ink: Rgb, ground: Rgb) -> f64 {
    matched_alpha(over(ink, alpha, reference), &[reference], our_ink, ground, &[ground])
}

/// libadwaita's lines for one background: shades use near-black ink, `border` uses
/// the text color and binds as libadwaita's `--border-opacity`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SurfaceLines {
    pub card_shade: crate::GlassColor,
    pub card_ring: crate::GlassColor,
    pub card_shadow: crate::GlassColor,
    pub card_far_shadow: crate::GlassColor,
    pub shade: crate::GlassColor,
    pub border: crate::GlassColor,
    pub highlight: crate::GlassColor,
}
impl SurfaceLines {
    fn new(adwaita: &Adwaita, reference: Rgb, ground: Rgb, text: Rgb) -> Self {
        let shade = |alpha, reference, ground| line(SHADE, same_step(SHADE, alpha, reference, SHADE, ground));
        let dark = adwaita.dark;
        Self {
            card_shade: if dark {
                line(over(BLACK, DARK_ROW_DIVIDER, ground), 1.)
            } else {
                shade(adwaita.card_shade, card(dark, reference), card(dark, ground))
            },
            card_ring: shade(CARD_RING, reference, ground),
            card_shadow: shade(CARD_SHADOW, reference, ground),
            card_far_shadow: shade(CARD_FAR_SHADOW, reference, ground),
            shade: line(SHADE, same_step(SHADE, adwaita.shade * SCROLL_LINE, reference, SHADE, ground) / SCROLL_LINE),
            border: line(text, same_step(adwaita.text, BORDER_OPACITY * adwaita.text_alpha, reference, text, ground)),
            highlight: line(WHITE, same_step(WHITE, HIGHLIGHT, reference, WHITE, ground)),
        }
    }
}

/// Lines keep libadwaita's contrast against our surfaces for any base color.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Lines {
    pub settings: SurfaceLines,
    pub surround: SurfaceLines,
    pub panel: SurfaceLines,
    pub sidebar_border: crate::GlassColor,
    pub sidebar_shade: crate::GlassColor,
    pub popover_border: crate::GlassColor,
}
impl Lines {
    fn new(dark: bool, [settings, surround, panel, sidebar]: [HexColor; 4], text: HexColor) -> Self {
        let adwaita = if dark { &ADWAITA_DARK } else { &ADWAITA_LIGHT };
        let rgb = |color: HexColor| color.0.map(f64::from);
        let [settings, surround, panel, sidebar, text] = [settings, surround, panel, sidebar, text].map(rgb);
        let edge = over(SHADE, adwaita.sidebar_border, adwaita.sidebar);
        let sidebar_shade = same_step(SHADE, adwaita.shade * SCROLL_LINE, adwaita.sidebar, SHADE, sidebar);
        Self {
            settings: SurfaceLines::new(adwaita, adwaita.window, settings, text),
            surround: SurfaceLines::new(adwaita, adwaita.window, surround, text),
            panel: SurfaceLines::new(adwaita, adwaita.popover, panel, text),
            sidebar_border: line(SHADE, matched_alpha(edge, &[adwaita.sidebar, adwaita.window], SHADE, sidebar, &[sidebar, settings])),
            sidebar_shade: line(SHADE, sidebar_shade / SCROLL_LINE),
            popover_border: line(BLACK, same_step(BLACK, POPOVER_BORDER, adwaita.window, BLACK, surround)),
        }
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
    pub card: HexColor,
    pub thumb: HexColor,
    pub text: HexColor,
    pub settings_secondary: HexColor,
    pub button: HexColor,
    pub control_fill: crate::GlassColor,
    pub accent: HexColor,
    pub accent_foreground: HexColor,
    pub relationship: HexColor,
    pub selection: HexColor,
    pub header_selection: HexColor,
    pub header_selection_hover: HexColor,
    pub checker_light: HexColor,
    pub checker_dark: HexColor,
    pub lines: Lines,
    /// Prepared once when settings/theme change, not converted per frame.
    pub surround_linear: [f32; 4],
    pub glass: crate::GlassPalette,
}
impl ThemePalette {
    pub fn histogram_colors(self) -> [HexColor; 4] {
        if self.text.oklab()[0] > self.panel.oklab()[0] {
            [HexColor([240,110,100]), HexColor([100,210,140]), HexColor([110,155,250]), self.text]
        } else {
            [HexColor([185,40,35]), HexColor([25,120,65]), HexColor([45,85,190]), self.text]
        }
    }
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
        let input = surface([51; 3], [250; 3]);
        let view = surface([43; 3], [228; 3]);
        let tabbar = surface([46; 3], [210; 3]);
        let selection = surface([82; 3], [213; 3]).tint(accent, 0.0);
        let header_selection = header.tint(accent, 0.0);
        let settings = surface([51; 3], if android { [250; 3] } else { [250, 250, 251] });
        let sidebar = surface(
            if android { [46; 3] } else { [46, 46, 50] },
            if android { [237; 3] } else { [235, 235, 237] },
        );
        let text = HexColor(if dark { [250, 250, 251] } else { [46, 46, 50] });
        ThemePalette {
            bg,
            panel,
            tabbar,
            input,
            view,
            settings,
            sidebar,
            sidebar_backdrop: surface([40, 40, 44], [242, 242, 244]),
            card: surface([65; 3], [255; 3]),
            thumb: surface([211; 3], [250; 3]),
            text,
            settings_secondary: HexColor(if dark { [188; 3] } else { [102; 3] }),
            button: HexColor([if dark { 255 } else { 0 }; 3]),
            control_fill: line(text.0.map(f64::from), if dark { 0.06 } else { 0.16 }),
            accent,
            accent_foreground: accent.contrasting(),
            relationship: accent.relationship(),
            selection,
            header_selection,
            header_selection_hover: header.tint(accent, if dark { 0.03 } else { -0.03 }),
            checker_light: TRANSPARENCY_CHECKER[0],
            checker_dark: TRANSPARENCY_CHECKER[1],
            lines: Lines::new(dark, [settings, bg, panel, sidebar], text),
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
            assert_eq!(p.checker_light, HexColor([255; 3]));
            assert_eq!(p.checker_dark, HexColor([204; 3]));
            assert_eq!(p.bg, anchor);
            assert_eq!(
                p.panel,
                HexColor([if theme == Theme::Dark { 65 } else { 237 }; 3])
            );
            assert!(p.tabbar.0[0] < p.panel.0[0]);
        }
    }

    #[test]
    fn control_faces_share_text_overlay_across_hosts_and_bases() {
        for theme in [Theme::Dark, Theme::Light] {
            let alpha = if theme == Theme::Dark { 0.06 } else { 0.16 };
            for platform in Platform::ALL {
                for base in theme.base_choices() {
                    let mut settings = Settings::default();
                    match theme { Theme::Dark => settings.dark_base = base, Theme::Light => settings.light_base = base }
                    let palette = settings.palette(theme, platform, None);
                    let [r, g, b, a] = palette.control_fill.0;
                    assert_eq!([r, g, b], palette.text.0.map(|v| v as f32 / 255.));
                    assert_eq!(a, alpha);
                    assert_eq!(serde_json::to_value(palette).unwrap()["control_fill"], serde_json::json!([r, g, b, a]));
                }
            }
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
    fn relationships_are_darker_accent_shades_with_practical_surface_contrast() {
        let accents: Vec<_> = ACCENTS.into_iter().map(|(_, color)| color).chain([
            HexColor([0; 3]), HexColor([128; 3]), HexColor([255; 3]),
            HexColor([1, 0, 0]), HexColor([255, 254, 255]),
            HexColor([255, 0, 255]), HexColor([0, 255, 0]),
        ]).collect();
        let luminance = |color: HexColor| {
            let [r, g, b, _] = color.linear();
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let contrast = |color: HexColor, surface: HexColor| {
            let (indicator, background) = (luminance(color), luminance(surface));
            (indicator.max(background) + 0.05) / (indicator.min(background) + 0.05)
        };
        for theme in [Theme::Light, Theme::Dark] {
            for base in theme.base_choices() {
                for &accent in &accents {
                    let palette = Settings {
                        light_base: base, dark_base: base, accent: Some(accent),
                        ..Settings::default()
                    }.palette(theme, Platform::Gtk, None);
                    let [lightness, a, b] = accent.oklab();
                    let [darker, da, db] = palette.relationship.oklab();
                    assert!(darker <= lightness + 0.001, "accent {accent}: {}", palette.relationship);
                    if lightness > 0.06 {
                        assert!((lightness - darker - 0.06).abs() < 0.005, "accent {accent}: {}", palette.relationship);
                    }
                    if a.hypot(b) > 0.02 && da.hypot(db) > 0.02 {
                        let difference = (hue(palette.relationship) - hue(accent) + 180.).rem_euclid(360.) - 180.;
                        assert!(difference.abs() < 3., "accent {accent}: {} changes hue by {difference}", palette.relationship);
                    }
                    if accent.0[0] == accent.0[1] && accent.0[1] == accent.0[2] {
                        let [r, g, b] = palette.relationship.0;
                        assert_eq!([r, g, b], [r; 3], "neutral accent {accent}");
                    }
                    if theme == Theme::Light && ACCENTS.iter().any(|(_, color)| *color == accent) {
                        for surface in [palette.panel, palette.selection, palette.view, palette.input] {
                            let ratio = contrast(palette.relationship, surface);
                            assert!(ratio >= 2., "base {base} accent {accent}: {} against {surface} is {ratio}:1", palette.relationship);
                        }
                    }
                }
            }
        }
        let dark = Settings::default().palette(Theme::Dark, Platform::Gtk, None);
        let light = Settings::default().palette(Theme::Light, Platform::Gtk, None);
        for surface in [light.panel, light.selection, light.view, light.input] {
            assert!(contrast(light.relationship, surface) >= 3.);
        }
        assert!((1.5..2.).contains(&contrast(dark.relationship, dark.selection)));
        assert!(contrast(dark.accent, dark.selection) < 3.);
        assert!(contrast(HexColor([0; 3]), dark.view) < 3.);
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

    fn rgb(color: HexColor) -> Rgb {
        color.0.map(f64::from)
    }
    fn drawn(line: crate::GlassColor, ground: Rgb) -> Rgb {
        let [r, g, b, a] = line.0.map(f64::from);
        over([r * 255., g * 255., b * 255.], a, ground)
    }
    fn steps(line: crate::GlassColor, ground: Rgb, touches: &[Rgb]) -> Vec<f64> {
        let level = lightness(drawn(line, ground));
        touches.iter().map(|&surface| level - lightness(surface)).collect()
    }

    #[test]
    fn default_light_lines_are_libadwaitas() {
        let p = Settings::default().palette(Theme::Light, Platform::Gtk, None);
        for set in [p.lines.settings] {
            for (line, alpha) in [
                (set.card_shade, 0.07),
                (set.card_ring, 0.03),
                (set.card_shadow, 0.07),
                (set.card_far_shadow, 0.03),
                (set.shade, 0.07),
            ] {
                assert!((f64::from(line.0[3]) - alpha).abs() < 0.002, "{line} is not {alpha}");
            }
        }
        assert!((f64::from(p.lines.sidebar_border.0[3]) - 0.07).abs() < 0.002);
    }

    #[test]
    fn default_dark_dividers_are_no_stronger_than_libadwaitas() {
        let p = Settings::default().palette(Theme::Dark, Platform::Gtk, None);
        let (settings, sidebar) = (rgb(p.settings), rgb(p.sidebar));
        let edge = steps(p.lines.sidebar_border, sidebar, &[sidebar, settings]);
        assert!(edge.iter().all(|&step| step < -0.018), "sidebar edge {edge:?}");
        assert!(edge[1] > -0.040, "sidebar edge against the content {edge:?}");
        let rows = drawn(p.lines.settings.card_shade, card(true, settings));
        assert!(rows.iter().all(|&v| (v - 51. * 0.95).abs() < 0.5), "row divider {rows:?} is the background 5% darker");
    }

    #[test]
    fn lines_keep_libadwaitas_contrast_on_every_base() {
        for theme in [Theme::Dark, Theme::Light] {
            let dark = theme == Theme::Dark;
            let adwaita = if dark { &ADWAITA_DARK } else { &ADWAITA_LIGHT };
            let window = adwaita.window;
            let reference = |ink, alpha, ground: Rgb, touches: &[Rgb]| {
                let level = lightness(over(ink, alpha, ground));
                touches.iter().map(|&s| (level - lightness(s)).abs()).fold(f64::INFINITY, f64::min)
            };
            let row = reference(SHADE, adwaita.card_shade, card(dark, window), &[card(dark, window)]);
            let edge = reference(SHADE, adwaita.sidebar_border, adwaita.sidebar, &[adwaita.sidebar, window]);
            let separator = reference(adwaita.text, BORDER_OPACITY * adwaita.text_alpha, window, &[window]);
            let colored = HexColor(if dark { [28, 44, 60] } else { [192, 180, 156] });
            for base in theme.base_choices().into_iter().chain([colored]) {
                let p = Settings { dark_base: base, light_base: base, ..Settings::default() }
                    .palette(theme, Platform::Gtk, None);
                let (settings, sidebar) = (rgb(p.settings), rgb(p.sidebar));
                let near = |line, ground: Rgb, touches: &[Rgb]| {
                    steps(line, ground, touches).into_iter().map(f64::abs).fold(f64::INFINITY, f64::min)
                };
                for (set, ground) in [(p.lines.settings, p.settings), (p.lines.surround, p.bg)] {
                    let (ground, card) = (rgb(ground), card(dark, rgb(ground)));
                    if dark {
                        let rows = drawn(set.card_shade, card);
                        let darker = over(BLACK, DARK_ROW_DIVIDER, ground);
                        assert!(rows.iter().zip(darker).all(|(a, b)| (a - b).abs() < 0.5), "{theme:?} {base} rows");
                    } else {
                        assert!((near(set.card_shade, card, &[card]) - row).abs() < 0.002, "{theme:?} {base} rows");
                    }
                    assert!((near(set.border, ground, &[ground]) - separator).abs() < 0.002, "{theme:?} {base} separator");
                    assert!(steps(set.card_shade, card, &[card])[0] < 0., "{theme:?} {base} rows stay darker");
                }
                let sides = steps(p.lines.sidebar_border, sidebar, &[sidebar, settings]);
                assert!(sides.iter().all(|&step| step < 0.), "{theme:?} {base} edge {sides:?}");
                assert!((near(p.lines.sidebar_border, sidebar, &[sidebar, settings]) - edge).abs() < 0.002, "{theme:?} {base} edge {sides:?}");
            }
        }
    }
}
