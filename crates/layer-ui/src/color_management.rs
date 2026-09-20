//! Shared presentation and interaction policy for native and browser color controls.
//! Hosts own pointer capture, widget drawing, display observations and profile I/O.
use crate::{ContactPhase, Platform, ProofMode, UiChange, UiSession};
use layer_render::CanvasRenderer;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn enabled(platform: Platform) -> bool {
    matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Windows | Platform::Mac | Platform::Ios)
}

pub fn proof_view<R: CanvasRenderer>(session: &UiSession<R>) -> Value {
    let document = session.engine().document();
    let recipe = session.effective_sdr_rendition();
    json!({"mode":session.proof_panel_mode(), "hdr":document.color.depth.is_float(),
        "recipe":recipe, "pad":crate::proof_panel::sdr_pad_values(recipe),
        "readouts":[format!("{:.0}%",recipe.contrast*100.), format!("{:+.0}%",recipe.balance*100.),
            format!("{:+.0}%",recipe.exposure*25.),format!("{:.0}%",recipe.highlight_color*100.)],
        "numbers":crate::proof_panel::sdr_tone_pad().axes.iter().map(|a|json!({"key":a.key,"label":a.label,"numeric":a.numeric,"value":if a.key=="contrast" {f64::from(recipe.contrast.log2())} else {f64::from(recipe.balance)}}))
            .chain(crate::proof_panel::sdr_number_controls().iter().map(|a|json!({"key":a.key,"label":a.label,"numeric":a.numeric,"value":if a.key=="exposure" {recipe.exposure} else {recipe.highlight_color}}))).collect::<Vec<_>>(), "icons":crate::proof_panel::SDR_READOUT_ICONS,
        "print":document.proof.as_ref().map(|p|json!({"name":p.name})), "gamut_warning":session.state().gamut_warning,
        "intents":crate::proof_panel::PROOF_INTENTS, "simulations":crate::proof_panel::ProofSimulation::CHOICES})
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProofAction {
    Mode {
        mode: ProofMode,
    },
    Reveal,
    Edit {
        phase: ContactPhase,
        control: String,
        value: f64,
    },
    Nudge {
        phase: ContactPhase,
        part: u32,
        delta: [f64; 2],
    },
    Point {
        phase: ContactPhase,
        part: u32,
        point: [f32; 2],
        size: f32,
    },
    Reset {
        #[serde(default)]
        part: Option<u32>,
    },
}
pub fn proof_action<R: CanvasRenderer>(
    session: &mut UiSession<R>,
    action: ProofAction,
) -> Result<UiChange, String> {
    let mut recipe = session.effective_sdr_rendition();
    let phase = match action {
        ProofAction::Reveal => {
            use crate::{CustomizationAction, DrawerAnchor, Panel, UiAction};
            let mut change = session.dispatch(UiAction::Customize {
                action: CustomizationAction::SetPanelVisible {
                    panel: Panel::Proof,
                    visible: true,
                },
            })?;
            let state = session.state();
            let layout = &state.workspace.layout;
            let next=layout.panel_group(Panel::Proof).and_then(|group| {
                if let Some(column)=layout.collapsed_column_for_group(group) {
                    let settings=layout.column_stack(column);
                    let open=if settings.drawers {state.customization.column_drawers.iter().any(|d|matches!(d.anchor,DrawerAnchor::Column {group:g,origin:Panel::Proof,..} if g==group))}
                        else {settings.open_column==Some(column) && layout.active_panel(Panel::Proof)==Some(Panel::Proof)};
                    (!open).then_some(UiAction::Customize {action:CustomizationAction::ToggleColumnDrawer {group,panel:Panel::Proof}})
                } else {(layout.active_panel(Panel::Proof)!=Some(Panel::Proof)).then_some(UiAction::SelectPanelTab {group,panel:Panel::Proof})}
            });
            if let Some(next) = next {
                let c = session.dispatch(next)?;
                change.regions |= c.regions;
                change.revision = c.revision;
                change.canvas_wake |= c.canvas_wake;
            }
            return Ok(change);
        }
        ProofAction::Mode { mode } => return session.select_proof_mode(mode),
        ProofAction::Reset { part } => {
            match part {
                Some(1) => {
                    recipe.balance = 0.;
                    recipe.contrast = 1.;
                }
                Some(2) => recipe.exposure = 0.,
                Some(3) => recipe.highlight_color = 0.3,
                _ => {
                    recipe = layer_core::color::hdr::SdrRendition {
                        headroom: recipe.headroom,
                        ..Default::default()
                    }
                }
            }
            return session.set_sdr_rendition(recipe);
        }
        ProofAction::Edit {
            phase,
            control,
            value,
        } => {
            if !value.is_finite() {
                return Err("Enter a finite value".into());
            }
            match control.as_str() {
                "contrast" => {
                    recipe =
                        crate::proof_panel::sdr_from_pad(recipe, [f64::from(recipe.balance), value])
                }
                "balance" => {
                    recipe = crate::proof_panel::sdr_from_pad(
                        recipe,
                        [value, f64::from(recipe.contrast.log2())],
                    )
                }
                "exposure" => recipe.exposure = value as f32,
                "highlight_color" => recipe.highlight_color = value as f32,
                _ => return Err("Unknown proof control".into()),
            }
            phase
        }
        ProofAction::Point {
            phase,
            part,
            point,
            size,
        } => {
            if !point.iter().all(|v| v.is_finite()) {
                return Err("Invalid proof position".into());
            }
            let g = crate::parameter_pad::ParameterDialGeometry::new(size)
                .ok_or("Invalid proof size")?;
            match part {
                1 => {
                    let p = g.field.disc_components(point);
                    recipe = crate::proof_panel::sdr_from_pad(
                        recipe,
                        crate::proof_panel::sdr_tone_pad().values(p.map(f64::from)),
                    );
                }
                2 | 3 => {
                    let t = g.arcs[(part - 2) as usize].fraction(point);
                    if part == 2 {
                        recipe.exposure = t * 4. - 2.;
                    } else {
                        recipe.highlight_color = t;
                    }
                }
                _ => return Err("Unknown proof target".into()),
            }
            phase
        }
        ProofAction::Nudge { phase, part, delta } => {
            if !delta.iter().all(|v| v.is_finite()) {
                return Err("Invalid proof increment".into());
            }
            match part {
                1 => {
                    let pad = crate::proof_panel::sdr_pad_values(recipe);
                    recipe = crate::proof_panel::sdr_from_pad(
                        recipe,
                        std::array::from_fn(|i| (pad[i] + delta[i] * 0.01).clamp(-1., 1.)),
                    );
                }
                2 => {
                    recipe.exposure =
                        (recipe.exposure + (delta[0] + delta[1]) as f32 * 0.04).clamp(-2., 2.)
                }
                3 => {
                    recipe.highlight_color =
                        (recipe.highlight_color + (delta[0] + delta[1]) as f32 * 0.01).clamp(0., 1.)
                }
                _ => return Err("Unknown proof target".into()),
            }
            phase
        }
    };
    session.edit_sdr_rendition(phase, recipe)
}

/// Immutable geometry is reusable at a given allocation, independent of artwork.
pub fn dial_geometry(size: f32) -> Result<Value, String> {
    let g = crate::parameter_pad::ParameterDialGeometry::new(size).ok_or("Invalid proof size")?;
    let arcs=g.arcs.map(|a|json!({"center":a.center,"radius":a.radius,"width":a.width,
        "marker_radius":a.marker_radius,"points":(0..=80).map(|i|a.point(i as f32/80.)).collect::<Vec<_>>()}));
    let readouts = g
        .readouts(size)
        .map(|r| json!({"icon":r.icon,"text":r.text,"curve":r.curve}));
    Ok(
        json!({"field":g.field,"arcs":arcs,"readouts":readouts,"text_size":crate::parameter_pad::ParameterDialGeometry::text_size(size),"reset":g.reset}),
    )
}

pub fn dial_hit(size: f32, point: [f32; 2]) -> u32 {
    let Some(g) = crate::parameter_pad::ParameterDialGeometry::new(size) else {
        return 0;
    };
    if (point[0] - g.field.center[0]).hypot(point[1] - g.field.center[1]) <= g.field.disc_radius() {
        return 1;
    }
    g.arcs
        .iter()
        .position(|a| a.contains(point))
        .map_or(0, |i| i as u32 + 2)
}

pub fn dial_markers(
    size: f32,
    recipe: layer_core::color::hdr::SdrRendition,
) -> Result<Value, String> {
    let g = crate::parameter_pad::ParameterDialGeometry::new(size).ok_or("Invalid proof size")?;
    let p =
        crate::proof_panel::sdr_tone_pad().fractions(crate::proof_panel::sdr_pad_values(recipe));
    Ok(json!([
        g.field.disc_marker(p.map(|v| v as f32)),
        g.arcs[0].point((recipe.exposure + 2.) / 4.),
        g.arcs[1].point(recipe.highlight_color)
    ]))
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
    space: layer_core::color::RgbSpace,
    recipe: layer_core::color::hdr::SdrRendition,
    headroom: f32,
) -> Result<[f32; 4], String> {
    use layer_core::color::{RgbSpace, hdr, rgb};
    recipe.validate().map_err(str::to_string)?;
    if !headroom.is_finite() || !(1. ..=100.).contains(&headroom) {
        return Err("Invalid headroom".into());
    }
    let p = color.linear_in(space)?;
    let rgb = if headroom > 1. {
        let mapped = hdr::map_display_premultiplied([p[0], p[1], p[2], 1.], headroom);
        rgb::apply(
            space.linear_transform(RgbSpace::Srgb),
            [mapped[0], mapped[1], mapped[2]].map(f64::from),
        )
        .map(|v| v as f32)
    } else {
        recipe
            .mapper(space, RgbSpace::Srgb)
            .map_rgb([p[0], p[1], p[2]])
    };
    Ok([rgb[0], rgb[1], rgb[2], p[3]])
}
#[derive(Deserialize)]
pub struct PickerField {
    pub space: layer_core::color::RgbSpace,
    pub hue: f32,
    pub shape: crate::ColorShape,
    pub stops: f32,
    pub recipe: layer_core::color::hdr::SdrRendition,
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
            || !(-16. ..=65504f32.log2()).contains(&self.stops)
        {
            return Err("Invalid HDR field viewing conditions".into());
        }
        let mapper = self
            .recipe
            .mapper(self.space, layer_core::color::RgbSpace::Srgb);
        let matrix = self
            .space
            .linear_transform(layer_core::color::RgbSpace::Srgb);
        let gain = self.stops.exp2();
        for p in pixels {
            for c in &mut p[..3] {
                *c *= gain;
            }
            let rgb = if self.headroom > 1. {
                let m = layer_core::color::hdr::map_display_premultiplied(*p, self.headroom);
                layer_core::color::rgb::apply(matrix, [m[0], m[1], m[2]].map(f64::from))
                    .map(|v| v as f32)
            } else {
                mapper.map_rgb([p[0], p[1], p[2]])
            };
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
