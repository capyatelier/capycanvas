use super::*;
use layer_core::color::hdr::SdrRendition;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PaintPairView {
    pub front_swatch: ColorSlot,
    pub definition: RgbColor,
    pub rgba: [f32; 4],
    pub in_gamut: bool,
    pub checker_cell: f32,
    pub swatches: [PaintSwatchView; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PaintSwatchView {
    pub slot: ColorSlot,
    pub definition: RgbColor,
    pub rgba: [f32; 4],
    pub checker: [[f32; 4]; 2],
}

impl ColorState {
    pub fn paint_pair(&self, display: RgbSpace, rendition: Option<SdrRendition>) -> PaintPairView {
        let mapper = rendition.filter(|_| self.hdr_picker.is_some()).map(|recipe| recipe.mapper(self.rgb_space, display));
        let linear = |color: RgbColor| {
            if let Some(mapper) = &mapper {
                let p = color.linear_in(self.rgb_space).unwrap();
                let rgb = mapper.map_rgb([p[0], p[1], p[2]]);
                [rgb[0], rgb[1], rgb[2], p[3]]
            } else {
                color.linear_in(display).unwrap()
            }
        };
        let encode = |p: [f32; 4]| [
            (display.encode(p[0] as f64) as f32).clamp(0., 1.),
            (display.encode(p[1] as f64) as f32).clamp(0., 1.),
            (display.encode(p[2] as f64) as f32).clamp(0., 1.),
            p[3],
        ];
        PaintPairView {
            front_swatch: self.front_swatch(),
            definition: self.definition(),
            rgba: encode(linear(self.definition())),
            in_gamut: self.definition().in_gamut(display).unwrap(),
            checker_cell: crate::TRANSPARENCY_CHECKER_CELL,
            swatches: [(ColorSlot::Foreground, self.foreground), (ColorSlot::Background, self.background)].map(|(slot, definition)| {
                let p = linear(definition);
                PaintSwatchView {
                    slot,
                    definition,
                    rgba: encode(p),
                    checker: crate::TRANSPARENCY_CHECKER.map(|gray| {
                        let g = gray.linear()[0];
                        encode([p[0] * p[3] + g * (1. - p[3]), p[1] * p[3] + g * (1. - p[3]), p[2] * p[3] + g * (1. - p[3]), 1.])
                    }),
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(colors: &ColorState) -> PaintPairView {
        colors.paint_pair(RgbSpace::Srgb, Some(SdrRendition::default()))
    }

    #[test]
    fn paint_pair_selection_preserves_definitions_and_remembers_transparency() {
        let mut colors = ColorState::default();
        let initial = pair(&colors);
        colors.apply(ColorAction::Select { slot: ColorSlot::Background }).unwrap();
        let background = pair(&colors);
        assert_eq!(background.front_swatch, ColorSlot::Background);
        assert_eq!(background.swatches, initial.swatches);
        assert_eq!(background.definition, colors.background);
        colors.apply(ColorAction::ToggleTransparent).unwrap();
        assert_eq!(pair(&colors), background);
        colors.apply(ColorAction::QuickColor { white: false }).unwrap();
        assert_eq!(pair(&colors).front_swatch, ColorSlot::Foreground);
        assert_eq!(pair(&colors).swatches, initial.swatches);
        assert_eq!(pair(&colors).definition, RgbColor::BLACK);
        colors.apply(ColorAction::QuickColor { white: true }).unwrap();
        assert_eq!(pair(&colors).definition, RgbColor::WHITE);
        assert_eq!(pair(&colors).rgba, [1.; 4]);
        let edited = RgbColor::new(RgbSpace::DisplayP3, [0.1, 0.4, 0.7, 0.3]).unwrap();
        colors.apply(ColorAction::Definition { color: edited }).unwrap();
        assert_eq!(pair(&colors).definition, edited);
        assert_eq!(pair(&colors).swatches, initial.swatches);
    }

    #[test]
    fn paint_pair_checker_is_opaque_and_composites_in_linear_light() {
        let mut colors = ColorState::default();
        for alpha in [0., 0.5, 1.] {
            colors.set_color(RgbColor::new(RgbSpace::Srgb, [0., 0., 0., alpha]).unwrap()).unwrap();
            let view = pair(&colors);
            for (actual, gray) in view.swatches[0].checker.into_iter().zip(crate::TRANSPARENCY_CHECKER) {
                assert_eq!(actual[3], 1.);
                let expected = RgbSpace::Srgb.encode(RgbSpace::Srgb.decode(gray.0[0] as f64 / 255.) * (1. - alpha as f64)) as f32;
                for value in &actual[..3] { assert!((value - expected).abs() < 1e-6); }
            }
        }
    }

    #[test]
    fn paint_pair_tracks_hdr_rendition_without_changing_paint() {
        let mut colors = ColorState::default();
        colors.set_hdr_enabled(true).unwrap();
        colors.set_color(RgbColor::from_linear(RgbSpace::Srgb, [3., 1., 0.25, 0.5]).unwrap()).unwrap();
        let original = colors.clone();
        let initial = pair(&colors);
        let recipe = SdrRendition { exposure: -2., ..Default::default() };
        let changed = colors.paint_pair(RgbSpace::Srgb, Some(recipe));
        assert_ne!(initial.rgba, changed.rgba);
        assert_ne!(initial.swatches[0].checker, changed.swatches[0].checker);
        let panel = colors.view_mapped(recipe, &crate::Localizer::shared(crate::UiLanguage::English));
        for (actual, expected) in changed.swatches[0].rgba.into_iter().zip(panel.swatches[0].rgba) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(changed.definition, original.definition());
        assert_eq!(colors, original);
    }

    #[test]
    fn paint_pair_follows_mask_context_and_leaves_artwork_intact() {
        use crate::{CommandId, Platform, UiAction};
        use crate::session::test_support::{invoke, session};
        let mut session = session(Platform::Gtk);
        session.dispatch(UiAction::Color { action: ColorAction::Definition {
            color: RgbColor::new(RgbSpace::Srgb, [0.9, 0.2, 0.1, 1.]).unwrap(),
        }}).unwrap();
        let artwork = pair(session.state().display_colors());
        invoke(&mut session, CommandId::QuickMask);
        session.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background }}).unwrap();
        session.dispatch(UiAction::Color { action: ColorAction::Definition {
            color: RgbColor::new(RgbSpace::Srgb, [0.25, 0.25, 0.25, 1.]).unwrap(),
        }}).unwrap();
        let mask = pair(session.state().display_colors());
        assert_eq!(mask.front_swatch, ColorSlot::Background);
        assert_ne!(mask.swatches, artwork.swatches);
        session.dispatch(UiAction::Color { action: ColorAction::ToggleTransparent }).unwrap();
        assert_eq!(pair(session.state().display_colors()), mask);
        session.dispatch(UiAction::Color { action: ColorAction::Definition { color: RgbColor::BLACK }}).unwrap();
        assert_eq!(pair(session.state().display_colors()).definition, RgbColor::BLACK);
        assert_eq!(pair(&session.state().colors), artwork);
        invoke(&mut session, CommandId::QuickMask);
        assert_eq!(pair(session.state().display_colors()), artwork);
    }
}
