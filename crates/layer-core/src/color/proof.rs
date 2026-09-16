//! Saved print simulation; delivery and temporary view state are separate.
use super::{ColorProfile, ConversionOptions, RenderingIntent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofRecipe {
    pub name: String,
    pub profile: ColorProfile,
    pub conversion: ConversionOptions,
    pub simulate_paper: bool,
    pub simulate_black_ink: bool,
}

impl ProofRecipe {
    pub fn new(name: String, profile: ColorProfile) -> Self {
        Self {
            name,
            profile,
            conversion: ConversionOptions {
                intent: RenderingIntent::RelativeColorimetric,
                black_point_compensation: true,
            },
            simulate_paper: false,
            simulate_black_ink: true,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 1024 {
            return Err("Give the proof target a name of at most 1024 bytes".into());
        }
        if self.simulate_paper && !self.simulate_black_ink {
            return Err("Paper simulation requires black-ink simulation".into());
        }
        if self.conversion.intent == RenderingIntent::AbsoluteColorimetric
            && self.conversion.black_point_compensation
        {
            return Err(
                "Absolute colorimetric proofing cannot use black point compensation".into(),
            );
        }
        Ok(())
    }
}
