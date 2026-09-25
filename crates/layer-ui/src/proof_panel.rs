//! Portable Proof control definitions and print-option policy. Hosts supply
//! native widgets, profile I/O and asynchronous preparation, not option semantics.
use crate::{ExportProfile, NumericControl, NumericKind};
use layer_core::color::{ProofRecipe, RenderingIntent};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProofAction {
    Reveal,
    Number { key: String, value: f64 },
    Control { part: u8, edit: SdrControlEdit },
    Dial { phase: crate::ContactPhase, size: f32, origin: [f32; 2], point: [f32; 2] },
    Mode { mode: crate::ProofMode },
    Rendition { phase: crate::ContactPhase, recipe: layer_core::color::hdr::SdrRendition },
    Pad { phase: crate::ContactPhase, values: [f64;2] },
}

/// Value policy shared with GTK; the host owns key repeat, capture and history phases.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SdrControlEdit {
    Reset,
    Step { axis: usize, steps: f64 },
}
pub fn sdr_control(mut recipe: layer_core::color::hdr::SdrRendition, part: u8, edit: SdrControlEdit) -> Result<layer_core::color::hdr::SdrRendition,String> {
    use layer_core::color::hdr::SdrRendition;
    if part>3 {return Err("Invalid Proof control".into())}
    match edit {
        SdrControlEdit::Reset => match part {
            0=>{recipe.balance=0.;recipe.contrast=1.;},
            1=>recipe.exposure=0.,
            2=>recipe.highlight_color=SdrRendition::default().highlight_color,
            _=>recipe=SdrRendition{headroom:recipe.headroom,..Default::default()},
        },
        SdrControlEdit::Step {axis,steps} => {
            if axis>1||!steps.is_finite(){return Err("Invalid Proof adjustment".into())}
            if part==0 {
                let mut values=sdr_pad_values(recipe);let control=sdr_tone_pad().axes[axis].numeric.clone();
                values[axis]=(values[axis]+steps*control.step).clamp(control.min,control.max);
                recipe=sdr_from_pad(recipe,values);
            } else if part<3 {
                let control=sdr_number_controls()[part as usize-1].numeric.clone();
                let value=if part==1 {&mut recipe.exposure}else{&mut recipe.highlight_color};
                *value=(f64::from(*value)+steps*control.step).clamp(control.min,control.max) as f32;
            }
        }
    }
    Ok(recipe)
}
pub fn apply<R: layer_render::CanvasRenderer>(session: &mut crate::UiSession<R>, action: ProofAction) -> Result<crate::UiChange, String> {
    match action {
        ProofAction::Reveal => reveal(session),
        ProofAction::Number { key, value } => {
            let mut recipe = session.effective_sdr_rendition();
            if !value.is_finite() { return Err("Invalid Proof value".into()); }
            match key.as_str() {
                "exposure" => recipe.exposure = value as f32,
                "highlight_color" => recipe.highlight_color = value as f32,
                "balance" | "contrast" => {
                    if !(-1. ..=1.).contains(&value) { return Err("Invalid Proof value".into()); }
                    let mut pad = sdr_pad_values(recipe);
                    pad[usize::from(key == "contrast")] = value;
                    recipe = sdr_from_pad(recipe, pad);
                }
                _ => return Err("Unknown Proof value".into()),
            }
            commit_control(session, recipe)
        }
        ProofAction::Control { part, edit } => {
            let recipe = sdr_control(session.effective_sdr_rendition(), part, edit)?;
            commit_control(session, recipe)
        }
        ProofAction::Dial { phase, size, origin, point } => {
            if !origin.into_iter().chain(point).all(f32::is_finite) {
                return Err("Invalid Proof contact".into());
            }
            let recipe = session.effective_sdr_rendition();
            let hit = sdr_dial(size, recipe, Some(origin), None)?["hit"].as_u64();
            let Some(part) = hit else { return session.edit_sdr_rendition(crate::ContactPhase::Cancel, recipe); };
            let value = sdr_dial(size, recipe, Some(point), Some(part as u8))?;
            let recipe = serde_json::from_value(value["recipe"].clone()).map_err(|e| e.to_string())?;
            session.edit_sdr_rendition(phase, recipe)
        }
        ProofAction::Mode { mode } => session.select_proof_mode(mode),
        ProofAction::Rendition { phase, recipe } => session.edit_sdr_rendition(phase, recipe),
        ProofAction::Pad { phase, values } => {
            let recipe=sdr_from_pad(session.effective_sdr_rendition(),values);
            session.edit_sdr_rendition(phase,recipe)
        }
    }
}

fn commit_control<R: layer_render::CanvasRenderer>(session: &mut crate::UiSession<R>, recipe: layer_core::color::hdr::SdrRendition) -> Result<crate::UiChange, String> {
    recipe.validate().map_err(str::to_string)?;
    session.edit_sdr_rendition(crate::ContactPhase::Down, recipe)?;
    session.edit_sdr_rendition(crate::ContactPhase::Up, recipe)
}

/// The reviewed GTK reveal behavior, shared by the other retained workspaces.
/// Showing a panel preserves its placement and opens its collapsed-column view.
pub fn reveal<R: layer_render::CanvasRenderer>(session: &mut crate::UiSession<R>) -> Result<crate::UiChange,String> {
    session.reveal_panel(crate::Panel::Proof)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProofChoice<T> {
    pub value: T,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintProofControl {
    Profile,
    Simulation,
    Intent,
    BlackPointCompensation,
    GamutWarning,
}
impl PrintProofControl {
    pub const ALL: [Self; 5] = [
        Self::Profile,
        Self::Simulation,
        Self::Intent,
        Self::BlackPointCompensation,
        Self::GamutWarning,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Simulation => "Simulate",
            Self::Intent => "Intent",
            Self::BlackPointCompensation => "Black point compensation",
            Self::GamutWarning => "Gamut warning",
        }
    }
}

pub const PROOF_INTENTS: [ProofChoice<RenderingIntent>; 4] = [
    ProofChoice {
        value: RenderingIntent::RelativeColorimetric,
        label: "Relative",
    },
    ProofChoice {
        value: RenderingIntent::Perceptual,
        label: "Perceptual",
    },
    ProofChoice {
        value: RenderingIntent::Saturation,
        label: "Saturation",
    },
    ProofChoice {
        value: RenderingIntent::AbsoluteColorimetric,
        label: "Absolute",
    },
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofSimulation {
    Colors,
    #[default]
    BlackInk,
    PaperAndInk,
}
impl ProofSimulation {
    pub const CHOICES: [ProofChoice<Self>; 3] = [
        ProofChoice {
            value: Self::Colors,
            label: "Colors",
        },
        ProofChoice {
            value: Self::BlackInk,
            label: "Black ink",
        },
        ProofChoice {
            value: Self::PaperAndInk,
            label: "Paper & ink",
        },
    ];
    pub fn from_recipe(recipe: &ProofRecipe) -> Self {
        if recipe.simulate_paper {
            Self::PaperAndInk
        } else if recipe.simulate_black_ink {
            Self::BlackInk
        } else {
            Self::Colors
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrintProofSettings {
    pub profile: Option<ExportProfile>,
    pub intent: RenderingIntent,
    pub bpc: bool,
    pub simulation: ProofSimulation,
}
impl Default for PrintProofSettings {
    fn default() -> Self {
        Self {
            profile: None,
            intent: RenderingIntent::RelativeColorimetric,
            bpc: true,
            simulation: ProofSimulation::default(),
        }
    }
}
impl PrintProofSettings {
    pub fn from_recipe(recipe: &ProofRecipe) -> Result<Self, String> {
        Ok(Self {
            profile: Some(ExportProfile {
                name: recipe.name.clone(),
                channels: layer_color::profile_channels(&recipe.profile)?,
                profile: recipe.profile.clone(),
            }),
            intent: recipe.conversion.intent,
            bpc: recipe.conversion.black_point_compensation,
            simulation: ProofSimulation::from_recipe(recipe),
        })
    }
    pub fn bpc_available(&self) -> bool {
        self.intent != RenderingIntent::AbsoluteColorimetric
    }
    pub fn recipe(&self) -> Result<ProofRecipe, String> {
        let p = self.profile.as_ref().ok_or("Choose a print profile")?;
        let mut recipe = ProofRecipe::new(p.name.clone(), p.profile.clone());
        recipe.conversion.intent = self.intent;
        recipe.conversion.black_point_compensation = self.bpc && self.bpc_available();
        recipe.simulate_paper = self.simulation == ProofSimulation::PaperAndInk;
        recipe.simulate_black_ink = self.simulation != ProofSimulation::Colors;
        recipe.validate()?;
        Ok(recipe)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProofNumberControl {
    pub key: &'static str,
    pub label: &'static str,
    pub numeric: NumericControl,
}
/// Reuse the application icon vocabulary on every host.
pub const SDR_READOUT_ICONS: [&str; 4] = [
    "layer-appearance-symbolic",
    "layer-grain-symbolic",
    "layer-brightness_contrast-symbolic",
    "layer-hue_saturation-symbolic",
];

/// GTK's dial geometry and square/disc mapping, transported to Web/Compose.
/// Part 0 is the field, 1/2 the arcs and 3 reset. A captured part stays fixed
/// while its pointer moves outside the original hit area.
pub fn sdr_dial(size: f32, mut recipe: layer_core::color::hdr::SdrRendition,
    point: Option<[f32;2]>, part: Option<u8>) -> Result<serde_json::Value,String> {
    use crate::parameter_pad::ParameterDialGeometry;
    let g=ParameterDialGeometry::new(size).ok_or("Invalid Proof dial size")?;
    let pad=sdr_tone_pad();
    let hit=point.and_then(|p| {
        let [x,y,w,h]=g.reset;
        if p[0]>=x&&p[0]<=x+w&&p[1]>=y&&p[1]<=y+h {Some(3)}
        else if let Some(i)=g.arcs.iter().position(|a|a.contains(p)){Some(i as u8+1)}
        else if (p[0]-g.field.center[0]).hypot(p[1]-g.field.center[1])<=g.field.disc_radius(){Some(0)}else{None}
    });
    if let (Some(p),Some(part))=(point,part.or(hit)) {
        match part {
            0=>recipe=sdr_from_pad(recipe,pad.values(g.field.disc_components(p).map(f64::from))),
            1|2=>{
                let control=&sdr_number_controls()[part as usize-1].numeric;
                let value=control.min+(control.max-control.min)*f64::from(g.arcs[part as usize-1].fraction(p));
                let value=((value/control.step).round()*control.step).clamp(control.min,control.max) as f32;
                if part==1 {recipe.exposure=value}else{recipe.highlight_color=value}
            },
            3=>recipe=layer_core::color::hdr::SdrRendition{headroom:recipe.headroom,..Default::default()},
            _=>return Err("Invalid Proof dial control".into()),
        }
    }
    let values=sdr_pad_values(recipe);
    let fractions=[(recipe.exposure+2.)/4.,recipe.highlight_color];
    Ok(serde_json::json!({"center":g.field.center,"radius":g.field.disc_radius(),
        "marker_radius":g.field.marker_radius(),"marker":g.field.disc_marker(pad.fractions(values).map(|v|v as f32)),
        "reset":g.reset,"readouts":g.readouts(size),"icons":SDR_READOUT_ICONS,"text_size":ParameterDialGeometry::text_size(size),
        "percentages":[recipe.contrast*100.,recipe.balance*100.,recipe.exposure*25.,recipe.highlight_color*100.],
        "arcs":g.arcs.iter().zip(fractions).map(|(a,f)|serde_json::json!({"geometry":a,"point":a.point(f),"path":(0..=64).map(|i|a.point(i as f32/64.)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "hit":hit,"recipe":recipe,"pad_values":values}))
}

/// Fixed directional illustration, never a sampled/modified document preview.
/// Broad flowing pools on the left become fine, defined cells to the right; the top
/// increases contrast and the bottom approaches neutral gray. Hosts cache it
/// as an immutable texture. Opaque RGBA8 transport, bounded to 512².
pub fn sdr_direction_texture(edge: u32) -> Vec<u8> {
    let edge = edge.clamp(1, 512);
    let mut bytes = Vec::with_capacity((edge * edge * 4) as usize);
    for j in 0..edge {
        for i in 0..edge {
            let x = (i as f32 + 0.5) / edge as f32 * 2. - 1.;
            let y = 1. - (j as f32 + 0.5) / edge as f32 * 2.;
            // Integrate the tightly bent perimeter over a pixel footprint.
            // This happens only during the host's one-time texture generation.
            let rgb = if x * x + y * y > 0.8 {
                let d = 0.5 / edge as f32;
                let samples = [(-d, -d), (d, -d), (-d, d), (d, d)]
                    .map(|(dx, dy)| sdr_glass_sample(x + dx, y + dy));
                std::array::from_fn(|c| samples.iter().map(|s| s[c]).sum::<f32>() * 0.25)
            } else {
                sdr_glass_sample(x, y)
            };
            let rgb = rgb.map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
            bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    bytes
}

fn sdr_glass_sample(x: f32, y: f32) -> [f32; 3] {
    let radius2 = (x * x + y * y).min(1.);
    let dome = (1. - radius2).sqrt();
    // Orthographic ray through an air/glass hemisphere (IOR 1.52) to its
    // base plane. Unlike a polynomial bulge, the spherical normal turns
    // sharply at the silhouette. Amplify that edge bend for this small guide;
    // this is an illustration, not a physically calibrated glass renderer.
    let eta = 1. / 1.52;
    let bend = (1. - eta * eta * radius2).sqrt() - eta * dome;
    let snell = eta / (eta + bend * dome);
    let lens = eta + 2.8 * (snell - eta);
    let px = x * lens + 0.04 * y * (1. - radius2);
    let py = y * lens - 0.04 * x * (1. - radius2);
    let up = ((y + 1.) * 0.5).clamp(0., 1.);
    let cell = liquid_cells(px, py, ((x + 1.) * 0.5).clamp(0., 1.));
    // Clean cell centers without a broad white reflection or surface glow. Contrast
    // follows screen-space up so the bottom still fades toward neutral gray.
    let contrast = 0.49 * up.powf(0.8);
    let v = 0.5 + contrast * (1.9 * cell).tanh();
    let tint = 0.010 * up * cell.tanh();
    // A narrow, angle-dependent rim defines the dome without a drop shadow.
    // Let it dominate at the silhouette, where compressed cells are subpixel.
    let rim = (1. - dome).powi(3);
    let rim_light = 0.12 + 0.70 * (-0.55 * x + 0.83 * y).clamp(0., 1.).powi(6);
    [v - 0.7 * tint, v + 0.05 * tint, v + tint].map(|c| c * (1. - rim) + rim_light * rim)
}

fn liquid_cells(x: f32, y: f32, across: f32) -> f32 {
    // Anchor the flow/detail transition to the visible horizontal direction,
    // even where refraction pulls the pattern coordinates toward the rim.
    let across = ((across - 0.225) / 0.5).clamp(0., 1.);
    let detail = across * across * (3. - 2. * across);
    // Two broad counter-currents deform neighboring regions together. Stronger
    // leftward bending creates necks and winding pools instead of isolated tiles.
    let mut p = [x, y];
    for (center, turn) in [([-0.45, 0.3], 1.8), ([0.1, -0.45], -1.6)] {
        let dx = p[0] - center[0];
        let dy = p[1] - center[1];
        let angle = turn * (1. - 0.9 * detail) * (-1.5 * (dx * dx + dy * dy)).exp();
        let (sn, cs) = angle.sin_cos();
        p = [center[0] + cs * dx - sn * dy, center[1] + sn * dx + cs * dy];
    }
    // Increase point density toward the right while keeping small cells rounded.
    let radius = 4.5 * (0.75 * p[0]).exp();
    let u = radius * (0.75 * p[1]).cos() + 8.1;
    let v = radius * (0.75 * p[1]).sin() + 5.7;
    let ix = u.floor() as i32;
    let iy = v.floor() as i32;
    let falloff = 1.4 + 4.1 * detail;
    let mut sum = 0.;
    let mut strongest: f32 = 0.;
    let mut field = 0.;
    for j in -2..=2 {
        for i in -2..=2 {
            // The grid only bounds the search. Vary its population rather than
            // assigning one identical point to every square: gaps, pairs and
            // wider jitter break up rows. Fade these changes in with detail so
            // the broad left-hand flow keeps its established shape.
            let hashes = [
                liquid_cell_hash(ix + i, iy + j),
                liquid_cell_hash(ix + i + 97, iy + j - 61),
            ];
            for (site, h) in hashes.into_iter().enumerate() {
                let weight = if site == 0 {
                    if h >> 28 < 3 { 1. - detail } else { 1. }
                } else if h >> 28 < 5 {
                    detail
                } else {
                    0.
                };
                if weight == 0. {
                    continue;
                }
                let margin = 0.18 - 0.13 * detail;
                let jitter = 1. - 2. * margin;
                let sx = (ix + i) as f32 + margin + jitter * (h & 1023) as f32 / 1023.;
                let sy = (iy + j) as f32 + margin + jitter * ((h >> 10) & 1023) as f32 / 1023.;
                let dx = u - sx;
                let dy = v - sy;
                let d2 = dx * dx + dy * dy;
                if d2 >= 4. {
                    continue;
                }
                // Different widths and oriented oval influences produce curved,
                // uneven boundaries. The quadratic stays positive definite.
                let shape = h.rotate_left(11).wrapping_mul(0x9e3779b9);
                let size = 1. + detail * ((shape & 255) as f32 / 255. - 0.5) * 0.45;
                let a = detail * (((shape >> 8) & 255) as f32 / 255. - 0.5) * 0.65;
                let b = detail * (((shape >> 16) & 255) as f32 / 255. - 0.5) * 0.65;
                let distance = ((1. + a) * dx * dx + (1. - a) * dy * dy + 2. * b * dx * dy)
                    / (size * size);
                // Keep support circular and bounded despite oval influences,
                // so entering/leaving search buckets creates no discontinuity.
                let tail = (d2 - 3.).clamp(0., 1.);
                let support = 1. - tail * tail * (3. - 2. * tail);
                let w = (-falloff * distance).exp() * support * weight;
                sum += w;
                strongest = strongest.max(w);
                let strength = 0.35 + 1.1 * ((h >> 20) & 1023) as f32 / 1023.;
                field += w * strength;
            }
        }
    }
    // Overlapping influences join into flowing contours on the left. Local
    // ownership separates them into defined cells on the right. Interpolate
    // the implicit field before shading: no image blur, haze or opacity layer.
    let pooled = (0.9 * std::f32::consts::PI / falloff - field) * 1.8;
    let ownership = if sum > 0. { strongest / sum } else { 0. };
    let edge = ((ownership - 0.47) / 0.24).clamp(0., 1.);
    let core = edge * edge * (3. - 2. * edge);
    let separated = 0.7 - 1.25 * core;
    pooled * (1. - detail) + separated * detail
}

fn liquid_cell_hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb352d);
    h ^ (h >> 15)
}
pub fn sdr_tone_pad() -> crate::parameter_pad::ParameterPadSpec {
    use crate::parameter_pad::{ParameterPadAxis, ParameterPadSpec};
    let mut balance = NumericControl::number(-1., 1., 0.01, 0).unit("%");
    balance.scale = 100.;
    balance.resolution = 0.01;
    let mut contrast = NumericControl::number(-1., 1., 0.01, 0).unit("%");
    contrast.scale = 100.;
    contrast.resolution = 0.01;
    ParameterPadSpec {
        axes: [
            ParameterPadAxis {
                key: "balance",
                label: "Balance",
                numeric: balance,
                default: 0.,
            },
            ParameterPadAxis {
                key: "contrast",
                label: "Contrast",
                numeric: contrast,
                default: 0.,
            },
        ],
    }
}
pub fn sdr_number_controls() -> [ProofNumberControl; 2] {
    [
        (
            "exposure",
            "Brightness",
            -2.,
            2.,
            0.04,
            0,
            "%",
            25.,
            -2.,
            2.,
        ),
        (
            "highlight_color",
            "Color intensity",
            0.,
            1.,
            0.01,
            0,
            "%",
            100.,
            0.,
            1.,
        ),
    ]
    .map(
        |(key, label, min, max, step, digits, unit, scale, soft_min, soft_max)| {
            let mut numeric = NumericControl::number(min, max, step, digits).unit(unit);
            numeric.kind = NumericKind::Slider;
            numeric.scale = scale;
            numeric.resolution = step;
            numeric.soft_min = soft_min;
            numeric.soft_max = soft_max;
            if key == "highlight_color" {
                numeric.resolution = 0.01;
            }
            if key == "highlight_color" {
                numeric.endpoint_labels = Some(["White".into(), "Color".into()]);
            }
            ProofNumberControl {
                key,
                label,
                numeric,
            }
        },
    )
}

/// Contrast is logarithmic: bottom 0.5x, center 1x, top 2x. Balance trades
/// macro and micro gain reciprocally; the fixed baseline never changes here.
pub fn sdr_from_pad(
    mut recipe: layer_core::color::hdr::SdrRendition,
    [balance, contrast]: [f64; 2],
) -> layer_core::color::hdr::SdrRendition {
    recipe.balance = if balance.is_nan() {
        0.
    } else {
        balance.clamp(-1., 1.) as f32
    };
    recipe.contrast = if contrast.is_nan() {
        1.
    } else {
        (contrast.clamp(-1., 1.) as f32).exp2()
    };
    recipe
}
pub fn sdr_pad_values(recipe: layer_core::color::hdr::SdrRendition) -> [f64; 2] {
    [f64::from(recipe.balance), f64::from(recipe.contrast.log2())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::RgbSpace;
    #[test]
    fn keyboard_steps_and_resets_match_native_controls() {
        use layer_core::color::hdr::SdrRendition;
        let initial=SdrRendition{headroom:12.,balance:0.2,contrast:1.2,exposure:-1.,highlight_color:0.7};
        let step=|part,axis,steps|sdr_control(initial,part,SdrControlEdit::Step{axis,steps}).unwrap();
        assert!((step(0,0,1.).balance-0.21).abs()<1e-6);
        assert!((step(0,1,1.).contrast-initial.contrast*2f32.powf(0.01)).abs()<1e-6);
        assert!((step(1,0,10.).exposure+0.6).abs()<1e-6);
        assert!((step(2,1,-1.).highlight_color-0.69).abs()<1e-6);
        assert_eq!(step(1,1,1000.).exposure,2.);
        for part in 0..=3 {
            let reset=sdr_control(initial,part,SdrControlEdit::Reset).unwrap();
            assert_eq!(reset.headroom,12.);
            if part==0 {assert_eq!((reset.balance,reset.contrast),(0.,1.));assert_eq!(reset.exposure,initial.exposure);}
            if part==1 {assert_eq!(reset.exposure,0.);assert_eq!(reset.balance,initial.balance);}
            if part==2 {assert_eq!(reset.highlight_color,SdrRendition::default().highlight_color);}
            if part==3 {assert_eq!(reset,SdrRendition{headroom:12.,..Default::default()});}
        }
        assert!(sdr_control(initial,4,SdrControlEdit::Reset).is_err());
        assert!(sdr_control(initial,0,SdrControlEdit::Step{axis:2,steps:1.}).is_err());
        assert!(sdr_control(initial,0,SdrControlEdit::Step{axis:0,steps:f64::NAN}).is_err());
    }
    #[test]
    fn dial_transport_reuses_gtk_hits_mapping_and_preserves_headroom() {
        let recipe=layer_core::color::hdr::SdrRendition{headroom:12.,..Default::default()};
        for size in [128.,256.,400.] {
            let g=crate::parameter_pad::ParameterDialGeometry::new(size).unwrap();
            for (part,arc) in g.arcs.iter().enumerate() {
                let p=arc.point(0.75);
                let v=sdr_dial(size,recipe,Some(p),None).unwrap();
                assert_eq!(v["hit"],part+1);
                let r:layer_core::color::hdr::SdrRendition=serde_json::from_value(v["recipe"].clone()).unwrap();
                assert_eq!(r.headroom,12.);
                assert!((if part==0 {r.exposure-1.}else{r.highlight_color-0.75}).abs()<1e-5);
            }
            let p=g.field.disc_marker([1.,1.]);
            let v=sdr_dial(size,recipe,Some(p),Some(0)).unwrap();
            assert_eq!(v["pad_values"],serde_json::json!([1.,1.]));
            let v=sdr_dial(size,recipe,Some(g.field.center),Some(3)).unwrap();
            assert_eq!(v["recipe"],serde_json::to_value(recipe).unwrap());
        }
    }
    #[test]
    fn scaled_numeric_edits_preserve_single_percentage_steps() {
        use crate::numeric::NumericOperation;
        let numbers = sdr_number_controls();
        for (spec, text, expected, stepped) in [
            (&numbers[0].numeric, "-10", -0.4, -0.36),
            (&numbers[1].numeric, "42", 0.42, 0.43),
        ] {
            let value = spec.resolve(0., NumericOperation::Expression { text: text.into() }).unwrap();
            assert!((value.value - expected).abs() < 1e-6);
            let next = spec.resolve(value.value, NumericOperation::Step { steps: 1. }).unwrap();
            assert!((next.value - stepped).abs() < 1e-6);
        }
        for axis in sdr_tone_pad().axes {
            let value = axis.numeric.resolve(0., NumericOperation::Expression { text: "25".into() }).unwrap();
            assert!((value.value - 0.25).abs() < 1e-6);
            let next = axis.numeric.resolve(value.value, NumericOperation::Step { steps: -1. }).unwrap();
            assert!((next.value - 0.24).abs() < 1e-6);
        }
    }
    #[test]
    fn circular_controls_are_bounded_invertible_and_preserve_delivery_settings() {
        use layer_core::color::hdr::SdrRendition;
        let original = SdrRendition {
            headroom: 12.,
            exposure: -1.,
            highlight_color: 0.7,
            ..Default::default()
        };
        for b in -100..=100 {
            for c in -100..=100 {
                let values = [b as f64 / 100., c as f64 / 100.];
                let r = sdr_from_pad(original, values);
                r.validate().unwrap();
                assert_eq!(
                    (r.headroom, r.exposure, r.highlight_color),
                    (
                        original.headroom,
                        original.exposure,
                        original.highlight_color
                    )
                );
                let v = sdr_pad_values(r);
                assert!((v[0] - values[0]).abs() < 1e-5 && (v[1] - values[1]).abs() < 1e-5);
                let gains = r.gains();
                assert!(((gains[0] * gains[1]).sqrt() - r.contrast * 1.3).abs() < 1e-6);
                assert_eq!(
                    serde_json::from_str::<SdrRendition>(&serde_json::to_string(&r).unwrap())
                        .unwrap(),
                    r
                );
            }
        }
        assert_eq!(sdr_from_pad(original, [0., 0.]), original);
        assert_eq!(
            sdr_from_pad(original, [0., -1.]).gains(),
            original.gains().map(|v| v * 0.5)
        );
        assert_eq!(
            sdr_from_pad(original, [0., 1.]).gains(),
            original.gains().map(|v| v * 2.)
        );
        assert!(sdr_from_pad(original, [-1., 0.]).gains()[0] > 1.);
        assert!(sdr_from_pad(original, [1., 0.]).gains()[1] > 1.);
        assert_eq!(sdr_from_pad(original, [f64::NAN; 2]), original);
    }
    #[test]
    fn proof_defaults_and_brightness_range_match_the_reviewed_treatment() {
        let recipe = layer_core::color::hdr::SdrRendition::default();
        assert_eq!(recipe.highlight_color, 0.3);
        assert_eq!(sdr_pad_values(recipe), [0., 0.]);
        let controls = sdr_number_controls();
        assert_eq!(
            (
                controls[0].numeric.min * controls[0].numeric.scale,
                controls[0].numeric.max * controls[0].numeric.scale
            ),
            (-50., 50.)
        );
        assert_eq!(sdr_direction_texture(128).len(), 128 * 128 * 4);
    }
    #[test]
    fn print_options_round_trip_and_absolute_intent_disables_bpc() {
        let mut settings = PrintProofSettings {
            profile: Some(ExportProfile::builtin(RgbSpace::Srgb)),
            ..Default::default()
        };
        for intent in PROOF_INTENTS {
            settings.intent = intent.value;
            for simulation in ProofSimulation::CHOICES {
                settings.simulation = simulation.value;
                let recipe = settings.recipe().unwrap();
                assert_eq!(ProofSimulation::from_recipe(&recipe), simulation.value);
                assert_eq!(
                    recipe.conversion.black_point_compensation,
                    settings.bpc_available()
                );
                let restored: PrintProofSettings =
                    serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
                assert_eq!(restored.recipe().unwrap(), recipe);
            }
        }
    }
}
