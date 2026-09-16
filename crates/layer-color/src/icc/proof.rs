//! Reusable print simulation in D50 PCS, independent of artwork and delivery.
use super::*;
use layer_core::color::ProofRecipe;
mod black;
mod lut;
mod view_lut;
pub use view_lut::ProofLut;
mod pcs;
use pcs::*;

/// Straight appearance and target-gamut metric before display clipping.
#[derive(Clone, Copy, Debug)]
pub struct ProofSample {
    pub xyz: [f64; 3],
    /// Values above 5 indicate output-gamut loss after inverse-table correction.
    pub gamut_distance: f64,
    // Preserve the continuous distances when resampling. The classifier itself
    // is discontinuous at repeat distance 5 and cannot be interpolated safely.
    pub gamut_roundtrips: [f64; 2],
}

pub struct ProofTransform {
    source: Pcs,
    target: Pcs,
    relative: Pcs,
    recipe: ProofRecipe,
    image_black: Option<[f64; 3]>,
    viewing_black: [f64; 3],
    input_white_scale: [f64; 3],
    paper_scale: [f64; 3],
}

impl ProofTransform {
    /// Prepare off the UI/render owner. A successful parse alone does not prove
    /// a profile has both device directions needed to simulate printing.
    pub fn new(space: RgbSpace, recipe: &ProofRecipe) -> Result<Self, String> {
        recipe.validate()?;
        let profile = open(&recipe.profile)?;
        let source_profile = builtin(space)?;
        let intent = recipe.conversion.intent;
        let target = Pcs::new(&profile, intent)?;
        let relative = Pcs::new(&profile, RenderingIntent::RelativeColorimetric)?;
        let viewing_black =
            black::source_black(&profile, &relative, RenderingIntent::RelativeColorimetric)?;
        let bpc = recipe.conversion.black_point_compensation
            || (profile.version() >= moxcms::ProfileVersion::V4_0
                && matches!(
                    intent,
                    RenderingIntent::Perceptual | RenderingIntent::Saturation
                ));
        let image_black = if bpc {
            Some(black::destination_black(
                &profile, &target, &relative, intent,
            )?)
        } else {
            None
        };
        let input_white_scale = if intent == RenderingIntent::AbsoluteColorimetric {
            media_white_scale(&source_profile, &profile)?
        } else {
            [1.; 3]
        };
        let white = profile.media_white_point.unwrap_or(profile.white_point);
        let paper_scale = [white.x / D50[0], white.y / D50[1], white.z / D50[2]];
        if paper_scale.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err("Invalid proof media white point".into());
        }
        Ok(Self {
            source: Pcs::new(&source_profile, RenderingIntent::RelativeColorimetric)?,
            target,
            relative,
            recipe: recipe.clone(),
            image_black,
            viewing_black,
            input_white_scale,
            paper_scale,
        })
    }

    /// Encoded working RGB in [0,1]. Alpha does not enter the color evaluator.
    /// Extended composition is an explicit out-of-domain condition at viewing.
    pub fn sample(&self, rgb: [f32; 3]) -> Result<ProofSample, String> {
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Soft proof input is outside the bounded SDR domain".into());
        }
        let original = self
            .source
            .to_xyz([rgb[0] as f64, rgb[1] as f64, rgb[2] as f64, 0.]);
        let xyz = std::array::from_fn(|i| original[i] * self.input_white_scale[i]);
        let xyz = self
            .image_black
            .map_or(xyz, |black| black::compensate(xyz, [0.; 3], black));
        let mut xyz = self.relative.roundtrip(&self.target, xyz);
        if self.recipe.simulate_paper {
            xyz = std::array::from_fn(|i| xyz[i] * self.paper_scale[i]);
        } else if !self.recipe.simulate_black_ink {
            xyz = black::compensate(xyz, self.viewing_black, [0.; 3]);
        }
        let once = self.relative.roundtrip(&self.relative, original);
        let twice = self.relative.roundtrip(&self.relative, once);
        let first = distance(original, once);
        let second = distance(once, twice);
        let gamut_distance = if second < 5. { first } else { first / second };
        if xyz.iter().any(|v| !v.is_finite()) || !gamut_distance.is_finite() {
            return Err("The proof profile produced nonfinite appearance values".into());
        }
        Ok(ProofSample {
            xyz,
            gamut_distance,
            gamut_roundtrips: [first, second],
        })
    }
}

#[cfg(test)]
mod tests;
