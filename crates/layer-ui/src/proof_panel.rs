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
pub fn sdr_tone_pad() -> crate::parameter_pad::ParameterPadSpec {
    use crate::parameter_pad::{ParameterPadAxis, ParameterPadSpec};
    let mut balance = NumericControl::number(-1., 1., 0.01, 0).unit("%");
    balance.scale = 100.;
    let mut strength = NumericControl::number(0., 1., 0.01, 0).unit("%");
    strength.scale = 100.;
    ParameterPadSpec {
        axes: [
            ParameterPadAxis {
                key: "balance",
                label: "Balance",
                numeric: balance,
                default: 0.,
            },
            ParameterPadAxis {
                key: "strength",
                label: "Strength",
                numeric: strength,
                default: 0.75,
            },
        ],
    }
}
pub fn sdr_number_controls() -> [ProofNumberControl; 2] {
    [
        (
            "exposure",
            "Brightness",
            -4.,
            4.,
            0.04,
            0,
            "%",
            25.,
            -4.,
            4.,
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

/// Portable control mapping. At zero strength, local processing is neutral.
/// Balance left favors illumination compression/softer texture; right favors
/// texture while retaining useful illumination compression. No file migration:
/// the existing physical tone/detail recipe remains the saved representation.
pub fn sdr_from_pad(
    mut recipe: layer_core::color::hdr::SdrRendition,
    [balance, strength]: [f64; 2],
) -> layer_core::color::hdr::SdrRendition {
    let balance = if balance.is_nan() { 0. } else { balance };
    let strength = if strength.is_nan() { 0.75 } else { strength };
    let balance = balance.clamp(-1., 1.) as f32;
    let strength = strength.clamp(0., 1.) as f32;
    recipe.tone = strength * (0.8 - balance * if balance < 0. { 0.05 } else { 0.35 });
    recipe.detail = 1. + strength * balance * if balance < 0. { 0.5 } else { 1. };
    recipe.method = layer_core::color::hdr::SdrMethod::LocalLaplacian;
    recipe.contrast = 1.;
    recipe.highlights = 0.;
    recipe
}

/// Older recipes outside the new control envelope remain exact. Hosts show a
/// custom position (no fabricated marker/value) until an explicit pad edit.
pub fn sdr_pad_values(recipe: layer_core::color::hdr::SdrRendition) -> Option<[f64; 2]> {
    if !recipe.is_local() {
        return None;
    }
    let delta = f64::from(recipe.detail) - 1.;
    let strength = (f64::from(recipe.tone) + delta * if delta < 0. { 0.1 } else { 0.35 }) / 0.8;
    let balance = if strength.abs() < 1e-6 {
        if delta.abs() > 1e-6 {
            return None;
        }
        0.
    } else {
        delta / (strength * if delta < 0. { 0.5 } else { 1. })
    };
    if !(-1e-6..=1.000001).contains(&strength) || !(-1.000001..=1.000001).contains(&balance) {
        return None;
    }
    Some([balance.clamp(-1., 1.), strength.clamp(0., 1.)])
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
            for s in 0..=100 {
                let values = [b as f64 / 100., s as f64 / 100.];
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
                if s == 0 {
                    assert_eq!((r.tone, r.detail), (0., 1.));
                }
                let restored = sdr_pad_values(r).unwrap();
                assert!((restored[1] - values[1]).abs() < 1e-5);
                if s > 0 {
                    assert!((restored[0] - values[0]).abs() < 2e-5);
                }
                let restored: SdrRendition =
                    serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
                assert_eq!(r, restored);
            }
        }
        let left = sdr_from_pad(original, [-1., 1.]);
        let right = sdr_from_pad(original, [1., 1.]);
        assert!(left.tone > right.tone && left.detail < right.detail && right.tone > 0.);
        assert!(
            sdr_pad_values(SdrRendition {
                tone: 0.85,
                detail: 2.,
                ..original
            })
            .is_none()
        );
        assert!(
            sdr_from_pad(original, [f64::NAN, f64::NAN])
                .validate()
                .is_ok()
        );
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
