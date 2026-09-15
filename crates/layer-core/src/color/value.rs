//! Portable paint/swatches. A color retains its defining RGB space; document
//! and display conversions derive coordinates without changing that definition.
use super::{RgbSpace, rgb};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RgbColor {
    pub space: RgbSpace,
    /// Straight, profile-encoded RGB and linear alpha. Finite extended RGB is
    /// retained: a P3 color expressed in sRGB need not fit the sRGB unit cube.
    pub rgba: [f32; 4],
}

impl RgbColor {
    pub const BLACK: Self = Self {
        space: RgbSpace::Srgb,
        rgba: [0., 0., 0., 1.],
    };
    pub const WHITE: Self = Self {
        space: RgbSpace::Srgb,
        rgba: [1.; 4],
    };

    pub fn new(space: RgbSpace, rgba: [f32; 4]) -> Result<Self, String> {
        let color = Self { space, rgba };
        color.validate()?;
        Ok(color)
    }

    pub fn validate(self) -> Result<(), String> {
        if !self.rgba.into_iter().all(f32::is_finite) || !(0.0..=1.0).contains(&self.rgba[3]) {
            return Err("Color requires finite RGB and alpha between 0 and 1".into());
        }
        Ok(())
    }

    /// Validate every supported conversion before accepting persistent state.
    /// Extreme finite values must not break a later document or view change.
    pub fn validate_working_spaces(self) -> Result<(), String> {
        self.validate()?;
        for space in RgbSpace::ALL {
            self.encoded_in(space)?;
            self.linear_in(space)?;
        }
        Ok(())
    }

    /// Capture a document sample without clipping its RGB or associating alpha.
    pub fn from_linear(space: RgbSpace, rgba: [f32; 4]) -> Result<Self, String> {
        Self::new(space, rgba)?;
        Self::new(
            space,
            [
                space.encode(f64::from(rgba[0])) as f32,
                space.encode(f64::from(rgba[1])) as f32,
                space.encode(f64::from(rgba[2])) as f32,
                rgba[3],
            ],
        )
    }

    pub fn encoded_in(self, destination: RgbSpace) -> Result<[f32; 4], String> {
        self.validate()?;
        let [r, g, b, _] = self.rgba;
        let rgb = self.space.convert(destination, [r, g, b].map(f64::from));
        Ok(Self::new(
            destination,
            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, self.rgba[3]],
        )?
        .rgba)
    }

    pub fn linear_in(self, destination: RgbSpace) -> Result<[f32; 4], String> {
        self.validate()?;
        let [r, g, b, _] = self.rgba;
        let linear = [r, g, b].map(|v| self.space.decode(f64::from(v)));
        let rgb = rgb::apply(self.space.linear_transform(destination), linear);
        let rgba = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, self.rgba[3]];
        // This is only finite/coverage validation, independent of transfer.
        Self::new(destination, rgba)?;
        Ok(rgba)
    }

    /// Allow only conversion roundoff at a boundary, below half an integer16
    /// code. This reports gamut; it never alters the selected color.
    pub fn in_gamut(self, destination: RgbSpace) -> Result<bool, String> {
        Ok(self.encoded_in(destination)?[..3]
            .iter()
            .all(|v| (-1e-6..=1.000001).contains(v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p3_red_keeps_extended_srgb_and_low_alpha() {
        let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 1. / 65535.]).unwrap();
        // Independent reference from the CSS Color 4 D65 P3/XYZ/sRGB matrices.
        // https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#color-conversion-code
        let expected = [1.2249402, -0.0420570, -0.0196376];
        let linear = color.linear_in(RgbSpace::Srgb).unwrap();
        for (actual, expected) in linear[..3].iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(linear[3], color.rgba[3]);
        assert!(color.in_gamut(RgbSpace::DisplayP3).unwrap());
        assert!(!color.in_gamut(RgbSpace::Srgb).unwrap());
        let srgb =
            RgbColor::new(RgbSpace::Srgb, color.encoded_in(RgbSpace::Srgb).unwrap()).unwrap();
        assert!(srgb.rgba[0] > 1. && srgb.rgba[1] < 0. && srgb.rgba[2] < 0.);
        for (actual, expected) in srgb
            .encoded_in(color.space)
            .unwrap()
            .into_iter()
            .zip(color.rgba)
        {
            assert!((actual - expected).abs() < 2e-6);
        }
        let bytes = serde_json::to_vec(&color).unwrap();
        assert_eq!(serde_json::from_slice::<RgbColor>(&bytes).unwrap(), color);
    }

    #[test]
    fn extended_samples_and_definitions_survive_all_builtin_spaces() {
        for source in RgbSpace::ALL {
            for rgba in [[-0.2, 1.2, 0.37, 0.], [0.2, 0.1, 0.8, 0.37], [1.; 4]] {
                let color = RgbColor::from_linear(source, rgba).unwrap();
                for destination in RgbSpace::ALL {
                    let value = color.linear_in(destination).unwrap();
                    let back = RgbColor::from_linear(destination, value)
                        .unwrap()
                        .linear_in(source)
                        .unwrap();
                    assert_eq!(back[3], rgba[3]);
                    for c in 0..3 {
                        assert!((back[c] - rgba[c]).abs() < 3e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_colors_and_conversion_overflow_fail_without_clamping() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(RgbColor::new(RgbSpace::Srgb, [invalid, 0., 0., 1.]).is_err());
        }
        for alpha in [-0.1, 1.1, f32::NAN] {
            assert!(RgbColor::new(RgbSpace::Srgb, [0., 0., 0., alpha]).is_err());
        }
        let huge = RgbColor::new(RgbSpace::ProPhoto, [f32::MAX, 0., 0., 1.]).unwrap();
        assert!(huge.linear_in(RgbSpace::Srgb).is_err());
    }
}
