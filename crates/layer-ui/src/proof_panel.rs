//! Portable Proof control definitions and print-option policy. Hosts supply
//! native widgets, profile I/O and asynchronous preparation, not option semantics.
use crate::{ExportProfile, NumericControl, NumericKind};
use layer_core::color::{ProofRecipe, RenderingIntent};
use serde::{Deserialize, Serialize};

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

/// Fixed directional illustration, never a sampled/modified document preview.
/// Broad liquid cells on the left become finer to the right; the top
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
    let cell = liquid_cells(px, py);
    // Clean cell centers without a broad white reflection or surface glow. Contrast
    // follows screen-space up so the bottom still fades toward neutral gray.
    let contrast = 0.49 * up.powf(0.8);
    let v = 0.5 + contrast * (1.9 * cell).tanh();
    let tint = 0.015 * up * cell;
    // A narrow, angle-dependent rim defines the dome without a drop shadow.
    // Let it dominate at the silhouette, where compressed cells are subpixel.
    let rim = (1. - dome).powi(3);
    let rim_light = 0.12 + 0.70 * (-0.55 * x + 0.83 * y).clamp(0., 1.).powi(6);
    [v - 0.7 * tint, v + 0.05 * tint, v + tint].map(|c| c * (1. - rim) + rim_light * rim)
}

fn liquid_cells(x: f32, y: f32) -> f32 {
    // A gentle flow bends the cell boundaries. The complex exponential then
    // increases density toward the right without stretching cells into stripes.
    let xw = x + 0.12 * (2.2 * y + 0.8 * x.sin()).sin();
    let yw = y + 0.10 * (2.5 * x - 0.65 * (2. * y).sin()).sin();
    let radius = 4.5 * (0.75 * xw).exp();
    let u = radius * (0.75 * yw).cos() + 8.1;
    let v = radius * (0.75 * yw).sin() + 5.7;
    let ix = u.floor() as i32;
    let iy = v.floor() as i32;
    let mut sum = 0.;
    let mut strongest: f32 = 0.;
    let mut shade = 0.;
    for j in -1..=1 {
        for i in -1..=1 {
            let h = liquid_cell_hash(ix + i, iy + j);
            let sx = (ix + i) as f32 + 0.18 + 0.64 * (h & 1023) as f32 / 1023.;
            let sy = (iy + j) as f32 + 0.18 + 0.64 * ((h >> 10) & 1023) as f32 / 1023.;
            let d2 = (u - sx).powi(2) + (v - sy).powi(2);
            let w = (-5.5 * d2).exp();
            sum += w;
            strongest = strongest.max(w);
            shade += w * ((h >> 20) & 1023) as f32 / 1023.;
        }
    }
    // Soft ownership rounds the corners into liquid patches and leaves narrow
    // connecting channels. Small shade variations keep the centers uncluttered.
    let ownership = strongest / sum;
    let t = ((ownership - 0.47) / 0.24).clamp(0., 1.);
    let core = t * t * (3. - 2. * t);
    (1. - core) * 0.9 + core * (-0.75 + 0.45 * shade / sum)
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
    let mut contrast = NumericControl::number(-1., 1., 0.01, 0).unit("%");
    contrast.scale = 100.;
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
