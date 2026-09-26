use crate::HexColor;
use layer_core::color::RgbSpace;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transparency {
    Off,
    #[default]
    Low,
    Medium,
    High,
}

impl Transparency {
    pub const CHOICES: [(Self, &'static str); 4] = [
        (Self::Off, "Off"),
        (Self::Low, "Low"),
        (Self::Medium, "Medium"),
        (Self::High, "High"),
    ];
    pub const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
    const fn alphas(self, dark: bool) -> [f32; 4] {
        match (self, dark) {
            (Self::Off, _) => [1.; 4],
            (Self::Low, false) => [0.94, 0.78, 0.78, 0.71],
            (Self::Medium, false) => [0.76, 0.54, 0.59, 0.51],
            (Self::High, false) => [0.56, 0.34, 0.4, 0.43],
            (Self::Low, true) => [0.96, 0.86, 0.86, 0.884],
            (Self::Medium, true) => [0.845, 0.705, 0.735, 0.77],
            (Self::High, true) => [0.72, 0.5, 0.55, 0.646],
        }
    }
    pub const fn surface_alpha(self, dark: bool) -> f32 {
        self.alphas(dark)[0]
    }
    pub const fn blur(self) -> BlurStyle {
        match self {
            Self::Off => BlurStyle { levels: 0, offset: 0. },
            Self::Low => BlurStyle { levels: 3, offset: 2.9 },
            Self::Medium => BlurStyle { levels: 3, offset: 3.4 },
            Self::High => BlurStyle { levels: 4, offset: 2.5 },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BlurStyle {
    pub levels: u32,
    pub offset: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GlassColor(pub [f32; 4]);

impl GlassColor {
    pub fn over(target: [f32; 3], under: [f32; 3], alpha: f32) -> Self {
        let [r, g, b] = std::array::from_fn(|i| {
            ((target[i] - (1. - alpha) * under[i]) / alpha).clamp(0., 1.)
        });
        Self([r, g, b, alpha])
    }
    pub fn composite(self, under: [f32; 3]) -> [f32; 3] {
        let [r, g, b, a] = self.0;
        let color = [r, g, b];
        std::array::from_fn(|i| color[i] * a + under[i] * (1. - a))
    }
    pub fn matches(self, rgba: [f32; 4]) -> bool {
        self.0.iter().zip(rgba).all(|(a, b)| (a - b).abs() < 1. / 512.)
    }
}

impl std::fmt::Display for GlassColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [r, g, b, a] = self.0;
        write!(f, "rgba({:.3}, {:.3}, {:.3}, {a:.4})", r * 255., g * 255., b * 255.)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GlassPalette {
    pub transparency: Transparency,
    pub blur: BlurStyle,
    pub panel: GlassColor,
    pub strip: GlassColor,
    pub tab: GlassColor,
    pub source: GlassColor,
    pub open_tile: GlassColor,
    pub chip: GlassColor,
    pub switcher: GlassColor,
    pub selection: GlassColor,
    pub header_selection: GlassColor,
    pub switcher_selection: GlassColor,
    pub document_tab: GlassColor,
}

fn rgb(color: HexColor) -> [f32; 3] {
    color.0.map(|v| f32::from(v) / 255.)
}

fn mix(a: [f32; 3], b: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] * (1. - amount) + b[i] * amount)
}

const DARK_GREY: [f32; 3] = [0.2; 3];
const SELECTED_CONTRAST: f32 = 0.6;

fn difference(a: [f32; 3], b: [f32; 3]) -> f32 {
    let lab = |c: [f32; 3]| layer_core::color::oklab::to_lab(c.map(|v| RgbSpace::Srgb.decode(f64::from(v))));
    let [a, b] = [lab(a), lab(b)];
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt() as f32
}

fn distinct(target: [f32; 3], opaque_parent: [f32; 3], parent: GlassColor, base: [f32; 3], alpha: f32) -> GlassColor {
    let opaque = difference(target, opaque_parent);
    let under = parent.composite(base);
    let contrast = |glass: GlassColor| {
        [[1.; 3], DARK_GREY]
            .into_iter()
            .map(|backdrop| {
                let below = parent.composite(backdrop);
                difference(glass.composite(below), below)
            })
            .fold(f32::INFINITY, f32::min)
    };
    let mut best = GlassColor::over(target, under, alpha);
    if alpha >= 1. || opaque <= 0. {
        return best;
    }
    for step in 0..=100 {
        let candidate = GlassColor::over(target, under, alpha + (1. - alpha) * step as f32 / 100.);
        if contrast(candidate) >= SELECTED_CONTRAST * opaque {
            return candidate;
        }
        if contrast(candidate) > contrast(best) {
            best = candidate;
        }
    }
    best
}

impl GlassPalette {
    pub(crate) fn new(
        transparency: Transparency,
        dark: bool,
        bg: HexColor,
        panel: HexColor,
        tabbar: HexColor,
        selection: HexColor,
        header_selection: HexColor,
    ) -> Self {
        let [surface, strip, inner, floating] = transparency.alphas(dark);
        let joined = if strip < 1. { 1. - (1. - surface) / (1. - strip) } else { 1. };
        let [bg, panel, tabbar] = [bg, panel, tabbar].map(rgb);
        let source = mix(panel, [0.; 3], 0.08);
        let switcher = mix(tabbar, bg, 0.25);
        let glass_panel = GlassColor::over(panel, bg, surface);
        let glass_strip = GlassColor::over(tabbar, bg, strip);
        let chip = GlassColor::over(bg, bg, floating);
        let glass_switcher = GlassColor::over(switcher, bg, floating);
        let [selection, header_selection] = [selection, header_selection].map(rgb);
        Self {
            transparency,
            blur: transparency.blur(),
            panel: glass_panel,
            strip: glass_strip,
            tab: GlassColor::over(panel, tabbar, joined),
            source: GlassColor::over(source, bg, strip),
            open_tile: GlassColor::over(panel, source, joined),
            chip,
            switcher: glass_switcher,
            selection: distinct(selection, panel, glass_panel, bg, inner),
            header_selection: distinct(header_selection, bg, chip, bg, inner),
            switcher_selection: distinct(header_selection, switcher, glass_switcher, bg, inner),
            document_tab: distinct(panel, bg, chip, bg, inner),
        }
    }
    pub fn surfaces(&self) -> [GlassColor; 5] {
        [self.panel, self.strip, self.source, self.chip, self.switcher]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Platform, Settings, Theme};

    fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() <= tolerance)
    }

    #[test]
    fn glass_over_the_base_reproduces_the_opaque_theme() {
        for (transparency, theme) in [Transparency::Low, Transparency::Medium, Transparency::High]
            .into_iter()
            .flat_map(|t| [(t, Theme::Dark), (t, Theme::Light)])
        {
            let settings = Settings { transparency, ..Settings::default() };
            let p = settings.palette(theme, Platform::Gtk, None);
            if theme == Theme::Light {
                assert!(p.glass.strip.0[3] < p.glass.panel.0[3]);
                continue;
            }
            let g = p.glass;
            let bg = rgb(p.bg);
            assert!(close(g.panel.composite(bg), rgb(p.panel), 1. / 255.));
            assert!(close(g.strip.composite(bg), rgb(p.tabbar), 1. / 255.));
            assert!(close(g.tab.composite(g.strip.composite(bg)), rgb(p.panel), 1. / 255.));
            assert!(close(g.selection.composite(g.panel.composite(bg)), rgb(p.selection), 1. / 255.));
            assert!(g.strip.0[3] < g.panel.0[3], "inactive tab strips show more of the canvas");
        }
    }

    #[test]
    fn selected_tabs_match_the_panel_body_over_any_backdrop() {
        for transparency in [Transparency::Low, Transparency::Medium, Transparency::High] {
            let g = Settings { transparency, ..Settings::default() }
                .palette(Theme::Dark, Platform::Gtk, None)
                .glass;
            for backdrop in [[0.; 3], [1.; 3], [0.9, 0.2, 0.1], [0.1, 0.4, 0.8]] {
                let body = g.panel.composite(backdrop);
                let tab = g.tab.composite(g.strip.composite(backdrop));
                let tile = g.open_tile.composite(g.source.composite(backdrop));
                assert!(close(body, tab, 1. / 255.), "{transparency:?} {backdrop:?}");
                assert!(close(body, tile, 1. / 255.), "{transparency:?} {backdrop:?}");
            }
        }
    }

    fn lightness(rgb: [f32; 3]) -> f32 {
        let [r, g, b] = rgb.map(layer_core::color::srgb_decode);
        (0.2126 * r + 0.7152 * g + 0.0722 * b).max(0.).cbrt()
    }

    fn visibility(glass: GlassColor) -> f32 {
        let veil = |backdrop: [f32; 3]| (lightness(glass.composite(backdrop)) - lightness(backdrop)).abs();
        let artwork = (0..64).map(|i| veil([i as f32 / 63.; 3])).sum::<f32>() / 64.;
        veil([1.; 3]) / 3. + artwork * 2. / 3.
    }

    fn best_alpha(cost: impl Fn(f32) -> f32) -> f32 {
        (5..=100)
            .map(|i| i as f32 / 100.)
            .min_by(|a, b| cost(*a).total_cmp(&cost(*b)))
            .unwrap()
    }

    #[test]
    fn floating_chrome_alphas_are_calibrated_and_invisible_over_the_base() {
        let glass = |transparency, theme| {
            Settings { transparency, ..Settings::default() }.palette(theme, Platform::Gtk, None)
        };
        for transparency in [Transparency::Low, Transparency::Medium, Transparency::High] {
            for theme in [Theme::Dark, Theme::Light] {
                let p = glass(transparency, theme);
                let (g, base) = (p.glass, rgb(p.bg));
                assert!(close(g.chip.composite(base), base, 1. / 255.), "invisible over the surround");
                assert!(g.chip.0[3] < g.panel.0[3]);
                let optimum = if theme == Theme::Dark {
                    best_alpha(|a| (visibility(GlassColor::over(base, base, a)) - visibility(g.panel)).abs())
                } else {
                    let paper = if transparency == Transparency::High { 1. } else { 2. };
                    best_alpha(|a| {
                        let chip = GlassColor::over(base, base, a);
                        paper * difference(chip.composite([1.; 3]), g.panel.composite([1.; 3])).powi(2)
                            + difference(chip.composite(DARK_GREY), g.strip.composite(DARK_GREY)).powi(2)
                    })
                };
                assert!((g.chip.0[3] - optimum).abs() <= 0.011, "{theme:?} {transparency:?}: {} vs {optimum}", g.chip.0[3]);
            }
        }
    }

    #[test]
    fn selected_states_keep_most_of_their_opaque_contrast() {
        for theme in [Theme::Dark, Theme::Light] {
            let opaque = Settings { transparency: Transparency::Off, ..Settings::default() }
                .palette(theme, Platform::Gtk, None);
            for transparency in [Transparency::Low, Transparency::Medium, Transparency::High] {
                let p = Settings { transparency, ..Settings::default() }.palette(theme, Platform::Gtk, None);
                let g = p.glass;
                for (selected, parent, target, under) in [
                    (g.document_tab, g.chip, opaque.panel, opaque.bg),
                    (g.selection, g.panel, opaque.selection, opaque.panel),
                ] {
                    let [target, under] = [rgb(target), rgb(under)];
                    let full = difference(target, under);
                    let seen = |glass: GlassColor| {
                        [[1.; 3], DARK_GREY]
                            .into_iter()
                            .map(|backdrop| {
                                let below = parent.composite(backdrop);
                                difference(glass.composite(below), below)
                            })
                            .fold(f32::INFINITY, f32::min)
                    };
                    let best = (0..=100)
                        .map(|i| seen(GlassColor::over(target, under, i as f32 / 100.)))
                        .fold(0., f32::max);
                    assert!(
                        seen(selected) >= (SELECTED_CONTRAST * full).min(best) - 1e-3,
                        "{theme:?} {transparency:?}: {} of {full}",
                        seen(selected)
                    );
                }
            }
        }
    }

    #[test]
    fn off_is_opaque() {
        assert_eq!(Settings::default().transparency, Transparency::Low);
        let p = Settings { transparency: Transparency::Off, ..Settings::default() }
            .palette(Theme::Light, Platform::Gtk, None);
        assert!(!p.glass.transparency.enabled());
        assert_eq!(p.glass.panel.0[3], 1.);
        assert!(close(p.glass.panel.composite([0.5; 3]), rgb(p.panel), 1e-6));
        for theme in [Theme::Dark, Theme::Light] {
            let p = Settings { transparency: Transparency::Off, ..Settings::default() }.palette(theme, Platform::Web, None);
            let g = p.glass;
            for (glass, opaque) in [
                (g.panel, p.panel),
                (g.strip, p.tabbar),
                (g.tab, p.panel),
                (g.open_tile, p.panel),
                (g.chip, p.bg),
                (g.selection, p.selection),
                (g.header_selection, p.header_selection),
                (g.switcher_selection, p.header_selection),
                (g.document_tab, p.panel),
            ] {
                let [r, g, b] = rgb(opaque);
                assert_eq!(glass.0, [r, g, b, 1.], "{theme:?}: hosts may apply glass colors in every mode");
            }
        }
    }
}
