//! Shared presentation and interaction policy for native and browser color controls.
//! Hosts own pointer capture, widget drawing, display observations and profile I/O.
use crate::UiSession;
use layer_core::color::{RgbSpace, hdr::SdrRendition};
use layer_render::CanvasRenderer;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn proof_view<R: CanvasRenderer>(session: &UiSession<R>) -> Value {
    let document = session.engine().document();
    let recipe = session.effective_sdr_rendition();
    json!({"mode":session.proof_panel_mode(), "hdr":document.color.depth.is_float(), "depth":document.color.depth,
        "recipe":recipe, "pad":crate::proof_panel::sdr_pad_values(recipe),
        "readouts":[format!("{:.0}%",recipe.contrast*100.), format!("{:+.0}%",recipe.balance*100.),
            format!("{:+.0}%",recipe.exposure*25.),format!("{:.0}%",recipe.highlight_color*100.)],
        "numbers":crate::proof_panel::sdr_tone_pad().axes.iter().map(|a|json!({"key":a.key,"label":a.label,"numeric":a.numeric,"value":if a.key=="contrast" {f64::from(recipe.contrast.log2())} else {f64::from(recipe.balance)}}))
            .chain(crate::proof_panel::sdr_number_controls().iter().map(|a|json!({"key":a.key,"label":a.label,"numeric":a.numeric,"value":if a.key=="exposure" {recipe.exposure} else {recipe.highlight_color}}))).collect::<Vec<_>>(), "icons":crate::proof_panel::SDR_READOUT_ICONS,
        "print":document.proof.as_ref().map(|p|json!({"name":p.name})), "gamut_warning":session.state().gamut_warning,
        "intents":crate::proof_panel::PROOF_INTENTS, "simulations":crate::proof_panel::ProofSimulation::CHOICES})
}

/// Hosts plot the shared bins and axis; nonpositive HDR values have no stop coordinate.
pub fn histogram_axis(h: &layer_core::color::histogram::Histogram) -> Value {
    let range = h.plot_bins();
    let fraction = |bin: usize| (bin as f64 - range.start as f64) / (range.len() - 1) as f64;
    let ticks = if h.color.depth.is_float() {
        let span = layer_core::color::histogram::hdr_bin_stops(range.end - 1)
            - layer_core::color::histogram::hdr_bin_stops(range.start);
        let step = if span > 16. { 4 } else { 2 };
        (-12..=16).filter(|stop|stop%step==0).filter_map(|stop|{
            let bin=layer_core::color::histogram::hdr_bin(2f64.powi(stop));
            range.contains(&bin).then(||json!({"fraction":fraction(bin),"label":if stop==0{"SDR white".into()}else{format!("{stop:+}")},"white":stop==0}))
        }).collect::<Vec<_>>()
    } else {
        vec![
            json!({"fraction":0.,"label":"0"}),
            json!({"fraction":1.,"label":"1"}),
        ]
    };
    json!({"start":range.start,"end":range.end,"ticks":ticks,"hdr":h.color.depth.is_float(),
        "description":if h.color.depth.is_float(){"Linear RGB and luminance · stops relative to SDR white"}else{"Encoded document RGB · linear luminance Y"}})
}

pub fn picker_preview(
    color: layer_core::color::RgbColor,
    space: RgbSpace,
    recipe: SdrRendition,
    headroom: f32,
) -> Result<[f32; 4], String> {
    recipe.validate().map_err(str::to_string)?;
    if !headroom.is_finite() || !(1. ..=100.).contains(&headroom) {
        return Err("Invalid headroom".into());
    }
    let p = color.linear_in(space)?;
    let rgb = display_mapper(recipe, space, headroom)([p[0], p[1], p[2], 1.]);
    Ok([rgb[0], rgb[1], rgb[2], p[3]])
}
fn display_mapper(recipe: SdrRendition, space: RgbSpace, headroom: f32) -> impl Fn([f32; 4]) -> [f32; 3] {
    let mapper = recipe.mapper(space, RgbSpace::Srgb);
    let matrix = space.linear_transform(RgbSpace::Srgb);
    move |p| {
        if headroom > 1. {
            let m = layer_core::color::hdr::map_display_premultiplied(p, headroom);
            layer_core::color::rgb::apply(matrix, [m[0], m[1], m[2]].map(f64::from)).map(|v| v as f32)
        } else {
            mapper.map_rgb([p[0], p[1], p[2]])
        }
    }
}
#[derive(Deserialize)]
pub struct PickerField {
    pub space: RgbSpace,
    pub hue: f32,
    pub shape: crate::ColorShape,
    pub stops: f32,
    pub recipe: SdrRendition,
    pub headroom: f32,
}
impl PickerField {
    pub fn render(&self, side: u32, pixels: &mut [[f32; 4]]) -> Result<(), String> {
        self.render_base(side, pixels)?;
        self.map(pixels)
    }
    pub fn render_base(&self, side: u32, pixels: &mut [[f32; 4]]) -> Result<(), String> {
        self.recipe.validate().map_err(str::to_string)?;
        if !self.hue.is_finite()
            || !self.headroom.is_finite()
            || !(1. ..=100.).contains(&self.headroom)
        {
            return Err("Invalid HDR field viewing conditions".into());
        }
        let mut colors = crate::ColorState::default();
        colors.set_rgb_space(self.space)?;
        colors.apply(crate::ColorAction::Shape { shape: self.shape })?;
        let geometry = crate::ColorWheelGeometry::new(128.).unwrap();
        colors.apply(crate::ColorAction::PickWheel {
            part: crate::ColorWheelPart::Hue,
            point: colors.wheel_hue_marker(&geometry, self.hue),
            size: 128.,
        })?;
        if !colors.render_field_base_linear(side, pixels) {
            return Err("Invalid picker extent".into());
        }
        Ok(())
    }
    pub fn map(&self, pixels: &mut [[f32; 4]]) -> Result<(), String> {
        self.recipe.validate().map_err(str::to_string)?;
        if !self.headroom.is_finite()
            || !(1. ..=100.).contains(&self.headroom)
            || !self.stops.is_finite()
            || !(-149. ..=128.).contains(&self.stops)
        {
            return Err("Invalid HDR field viewing conditions".into());
        }
        let map = display_mapper(self.recipe, self.space, self.headroom);
        let gain = f64::from(self.stops).exp2();
        for p in pixels {
            for c in &mut p[..3] {
                *c = (f64::from(*c) * gain).clamp(-(f32::MAX as f64),f32::MAX as f64) as f32;
            }
            let rgb = map(*p);
            *p = [rgb[0], rgb[1], rgb[2], p[3]];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorAction, ColorShape, ColorState, ColorWheelGeometry, ColorWheelPart};
    use layer_core::color::{
        DocumentColor, RgbSpace, SampleDepth, hdr::SdrRendition, histogram::Histogram,
    };
    #[test]
    fn transported_hdr_field_matches_live_picker_hue_and_cached_mapping() {
        for space in RgbSpace::ALL {
            for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
                for hue in [0., 137., 237.] {
                    let field = PickerField {
                        space,
                        shape,
                        hue,
                        stops: 2.,
                        recipe: SdrRendition::default(),
                        headroom: 4.,
                    };
                    let mut state = ColorState::default();
                    state.set_rgb_space(space).unwrap();
                    state.apply(ColorAction::Shape { shape }).unwrap();
                    let g = ColorWheelGeometry::new(128.).unwrap();
                    state
                        .apply(ColorAction::PickWheel {
                            part: ColorWheelPart::Hue,
                            point: state.wheel_hue_marker(&g, hue),
                            size: 128.,
                        })
                        .unwrap();
                    state.set_hdr_enabled(true).unwrap();
                    state
                        .apply(ColorAction::HdrIntensity { stops: 2. })
                        .unwrap();
                    let mut live = vec![[0.; 4]; 32 * 32];
                    assert!(state.render_field_base_linear(32, &mut live));
                    let mut transported = vec![[0.; 4]; 32 * 32];
                    field.render_base(32, &mut transported).unwrap();
                    assert_eq!(live, transported);
                    field.map(&mut live).unwrap();
                    field.render(32, &mut transported).unwrap();
                    assert_eq!(live, transported);
                    assert!(live.iter().flatten().all(|v| v.is_finite()));
                }
            }
        }
    }
    #[test]
    fn hdr_histogram_axis_keeps_white_and_has_bounded_readable_ticks() {
        let mut histogram = Histogram::new(DocumentColor {
            space: RgbSpace::Srgb,
            depth: SampleDepth::F16,
        });
        histogram.add(&[[-1., 0.000001, 65504., 1.]]).unwrap();
        let axis = histogram_axis(&histogram);
        let ticks = axis["ticks"].as_array().unwrap();
        assert!(ticks.len() <= 8);
        assert!(ticks.iter().any(|t| t["white"] == true));
        assert!(
            ticks
                .iter()
                .all(|t| (0. ..=1.).contains(&t["fraction"].as_f64().unwrap()))
        );
    }
}
