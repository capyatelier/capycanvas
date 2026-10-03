//! Saved print simulation; delivery and temporary view state are separate.
use super::{ColorProfile, ConversionOptions, RenderingIntent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofRecipeError { NameLimit, PaperRequiresBlackInk, AbsoluteBlackPoint }
impl ProofRecipeError {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::NameLimit => "The proof target name must be at most 1024 bytes",
            Self::PaperRequiresBlackInk => "Paper simulation requires black-ink simulation",
            Self::AbsoluteBlackPoint => "Absolute colorimetric proofing cannot use black point compensation",
        }
    }
}
impl From<ProofRecipeError> for String {
    fn from(reason: ProofRecipeError) -> Self { reason.diagnostic().into() }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofRecipe<P = ColorProfile> {
    pub name: String,
    pub profile: P,
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
}

impl<P> ProofRecipe<P> {
    pub fn with_profile<Q>(self, profile: Q) -> ProofRecipe<Q> {
        ProofRecipe {
            name: self.name,
            profile,
            conversion: self.conversion,
            simulate_paper: self.simulate_paper,
            simulate_black_ink: self.simulate_black_ink,
        }
    }

    pub fn validate(&self) -> Result<(), ProofRecipeError> {
        if self.name.len() > 1024 {
            return Err(ProofRecipeError::NameLimit);
        }
        if self.simulate_paper && !self.simulate_black_ink {
            return Err(ProofRecipeError::PaperRequiresBlackInk);
        }
        if self.conversion.intent == RenderingIntent::AbsoluteColorimetric
            && self.conversion.black_point_compensation
        {
            return Err(ProofRecipeError::AbsoluteBlackPoint);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_name_is_optional_display_metadata_with_bounded_bytes() {
        for name in ["", " ", "โปรไฟล์ 🖌", "Embedded profile"] {
            let recipe = ProofRecipe::new(name.into(), ColorProfile::default());
            recipe.validate().unwrap();
            let restored: ProofRecipe = serde_json::from_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
            assert_eq!(restored, recipe);
        }
        let mut recipe = ProofRecipe::new("é".repeat(512), ColorProfile::default());
        recipe.validate().unwrap();
        recipe.name.push('a');
        assert!(recipe.validate().is_err());
        recipe.name.clear();
        recipe.simulate_paper = true;
        recipe.simulate_black_ink = false;
        assert!(recipe.validate().is_err());
        recipe.simulate_black_ink = true;
        recipe.conversion.intent = RenderingIntent::AbsoluteColorimetric;
        assert!(recipe.validate().is_err());
    }
}
