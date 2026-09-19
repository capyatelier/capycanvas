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
/// Smooth glass folds on the left acquire fine ripples to the right; the top
/// increases contrast and the bottom approaches neutral gray. Hosts cache it
/// as an immutable texture. Opaque RGBA8 transport, bounded to 512².
pub fn sdr_direction_texture(edge: u32) -> Vec<u8> {
    let edge = edge.clamp(1, 512);
    let mut bytes = Vec::with_capacity((edge * edge * 4) as usize);
    for j in 0..edge {
        for i in 0..edge {
            let x = (i as f32 + 0.5) / edge as f32 * 2. - 1.;
            let y = 1. - (j as f32 + 0.5) / edge as f32 * 2.;
            let radius2 = (x * x + y * y).min(1.);
            let dome = (1. - radius2).sqrt();
            // Convex refraction magnifies the center and gently twists the
            // flowing folds. Screen-space up still controls contrast.
            let lens = 1. - 0.36 * (1. - radius2);
            let px = x * lens + 0.055 * y * (1. - radius2);
            let py = y * lens - 0.055 * x * (1. - radius2);
            let right = (px + 1.) * 0.5;
            let up = (y + 1.) * 0.5;
            // One continuous glass fold family makes the axes legible at
            // small panel sizes. Spacing tightens to the right, instead of
            // adding unrelated high-frequency noise over the broad folds.
            let flow = right
                + 0.23 * (2.7 * py - 0.35).sin() * (1. - 0.25 * right)
                + 0.06 * (4.5 * py + 2. * right).sin();
            let phase = std::f32::consts::TAU * (0.4 * flow + 3.6 * flow.powi(3))
                + 0.35 * (3. * py + right).sin();
            let wave = 0.7 * phase.sin() + 0.16 * (2. * phase + 0.3).sin();
            let reflection = (phase - 0.7).cos().max(0.).powf(4. + 10. * right);
            // Keep the internal reflection faint so the folds stay clear.
            let inner_reflection = (phase + 0.5).cos().max(0.).powi(2);
            // Bound the amplitude separately: the bottom visibly converges
            // to gray even where a reflection would otherwise stay bright.
            let contrast = 0.49 * up.powf(1.05);
            let v = 0.5
                + contrast
                    * ((1.1 + 1.6 * up)
                        * (0.8 * wave + 0.55 * reflection + 0.07 * inner_reflection - 0.17))
                        .tanh();
            let tint = 0.085 * up * (reflection - 0.5 * wave);
            // A restrained, narrow glint and faint rim retain the lens shape
            // without a broad hazy reflection. Keep the lower interior gray.
            let light = (-0.5 * x + 0.45 * y + 0.73993 * dome).max(0.);
            let glint = 0.10 * light.powi(110);
            let rim = (1. - dome).powi(3);
            let rim_light = 0.07 + 0.18 * (-0.55 * x + 0.83 * y).max(0.);
            let shadow = 0.035 * (0.65 * x - 0.76 * y).max(0.) * radius2;
            let rgb = [v - 0.7 * tint, v + 0.05 * tint, v + tint];
            let rgb: [u8; 3] = std::array::from_fn(|c| {
                let lit = rgb[c] * (1. - shadow) + glint * (1. - rgb[c]);
                let lit = lit * (1. - rim * 0.2) + rim * rim_light * (0.88 + 0.06 * c as f32);
                (lit.clamp(0., 1.) * 255.).round() as u8
            });
            bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    bytes
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
