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
    /// Exact authored linear RGB, when transfer encoding would lose precision.
    /// `rgba` remains the encoded UI/legacy readout; alpha has one owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_rgb: Option<[f32; 3]>,
}

impl RgbColor {
    pub const BLACK: Self = Self {
        space: RgbSpace::Srgb,
        rgba: [0., 0., 0., 1.],
        linear_rgb: None,
    };
    pub const WHITE: Self = Self {
        space: RgbSpace::Srgb,
        rgba: [1.; 4],
        linear_rgb: None,
    };

    pub fn new(space: RgbSpace, rgba: [f32; 4]) -> Result<Self, String> {
        let color = Self { space, rgba, linear_rgb: None };
        color.validate()?;
        Ok(color)
    }

    pub fn validate(self) -> Result<(), String> {
        if !self.rgba.into_iter().all(f32::is_finite) || !(0.0..=1.0).contains(&self.rgba[3]) {
            return Err("Color requires finite RGB and alpha between 0 and 1".into());
        }
        if let Some(linear) = self.linear_rgb {
            if linear.iter().any(|v| !v.is_finite()) || linear.map(|v| self.space.encode(f64::from(v)) as f32) != self.rgba[..3] {
                return Err("Linear color and encoded readout disagree".into());
            }
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
        let linear = [rgba[0], rgba[1], rgba[2]];
        let encoded = linear.map(|v| space.encode(f64::from(v)) as f32);
        let mut color = Self::new(space, [encoded[0], encoded[1], encoded[2], rgba[3]])?;
        if encoded.map(|v| (space.decode(f64::from(v)) as f32).to_bits()) != linear.map(f32::to_bits) {
            color.linear_rgb = Some(linear);
        }
        Ok(color)
    }

    pub fn encoded_in(self, destination: RgbSpace) -> Result<[f32; 4], String> {
        self.validate()?;
        if destination == self.space { return Ok(self.rgba); }
        let rgb = if let Some(linear) = self.linear_rgb {
            rgb::apply(self.space.linear_transform(destination), linear.map(f64::from)).map(|v| destination.encode(v))
        } else {
            self.space.convert(destination, [self.rgba[0], self.rgba[1], self.rgba[2]].map(f64::from))
        };
        Ok(Self::new(
            destination,
            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, self.rgba[3]],
        )?
        .rgba)
    }

    pub fn linear_in(self, destination: RgbSpace) -> Result<[f32; 4], String> {
        self.validate()?;
        let [r, g, b, _] = self.rgba;
        let linear = self.linear_rgb.map(|p| p.map(f64::from)).unwrap_or_else(|| [r, g, b].map(|v| self.space.decode(f64::from(v))));
        let rgb = if destination == self.space { linear } else { rgb::apply(self.space.linear_transform(destination), linear) };
        let rgba = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, self.rgba[3]];
        // This is only finite/coverage validation, independent of transfer.
        Self::new(destination, rgba)?;
        Ok(rgba)
    }

    /// HDR brightness does not put a color outside chromatic gamut. Negative
    /// components still indicate a chromaticity outside the destination primaries.
    pub fn in_hdr_gamut(self, destination: RgbSpace) -> Result<bool, String> {
        let p = self.linear_in(destination)?;
        let tolerance = p[..3].iter().copied().fold(1., f32::max) * 1e-6;
        Ok(p[..3].iter().all(|v| *v >= -tolerance))
    }

    /// Peak-channel brightness relative to reference white; black has no EV.
    pub fn brightness_ev(self, destination: RgbSpace) -> Result<Option<f32>, String> {
        let p = self.linear_in(destination)?;
        let peak = p[..3].iter().copied().fold(0., f32::max);
        Ok((peak > 0.).then(|| peak.log2()))
    }

    pub fn with_brightness_ev_at_depth(self, destination: RgbSpace, stops: f32, depth: super::SampleDepth) -> Result<Self, String> {
        let lower = if depth == super::SampleDepth::F32 { -149. } else { -16. };
        let upper = if depth == super::SampleDepth::F32 { 128. } else { 15. };
        if !stops.is_finite() || !(lower..=upper).contains(&stops) {
            return Err(format!("Brightness must be between {lower} and {upper} EV"));
        }
        let mut p = self.linear_in(destination)?;
        let peak = p[..3].iter().copied().fold(0., f32::max);
        if peak <= 0. { return Err("Choose a color brighter than black first".into()); }
        let scale = f64::from(stops).exp2() / f64::from(peak);
        for v in &mut p[..3] { *v = (f64::from(*v) * scale) as f32; }
        super::hdr::validate_pixel(depth, p).map_err(str::to_string)?;
        Self::from_linear(destination, p)
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
    use crate::color::SampleDepth;

    #[test]
    fn authored_linear_float32_survives_transfer_encoding_and_serialization() {
        for space in RgbSpace::ALL {
            for pixel in [
                [65504., 100000.125, -0.12345679, 0.25],
                [f32::MAX, -f32::MAX, f32::MIN_POSITIVE, 1.],
                [f32::from_bits(1), -f32::from_bits(1), -0., 0.],
            ] {
                let color = RgbColor::from_linear(space, pixel).unwrap();
                let restored: RgbColor = serde_json::from_slice(&serde_json::to_vec(&color).unwrap()).unwrap();
                assert_eq!(restored.linear_in(space).unwrap().map(f32::to_bits), pixel.map(f32::to_bits));
            }
        }
        let mut inconsistent = RgbColor::WHITE;
        inconsistent.linear_rgb = Some([2.; 3]);
        assert!(inconsistent.validate().is_err());
    }

    #[test]
    fn hdr_brightness_preserves_chromaticity_alpha_and_separates_gamut() {
        let color = RgbColor::from_linear(RgbSpace::Srgb, [8., 2., 1., 0.25]).unwrap();
        assert!(color.in_hdr_gamut(RgbSpace::Srgb).unwrap());
        assert!(!color.in_gamut(RgbSpace::Srgb).unwrap());
        assert!((color.brightness_ev(RgbSpace::Srgb).unwrap().unwrap() - 3.).abs() < 1e-6);
        let brighter = color.with_brightness_ev_at_depth(RgbSpace::Srgb, 4., SampleDepth::F16).unwrap().linear_in(RgbSpace::Srgb).unwrap();
        for (a, b) in brighter.into_iter().zip([16., 4., 2., 0.25]) { assert!((a-b).abs() < 2e-5); }
        let red = RgbColor::from_linear(RgbSpace::DisplayP3, [8., 0., 0., 1.]).unwrap();
        assert!(!red.in_hdr_gamut(RgbSpace::Srgb).unwrap());
        assert!(red.in_hdr_gamut(RgbSpace::DisplayP3).unwrap());
        assert!(RgbColor::BLACK.brightness_ev(RgbSpace::Srgb).unwrap().is_none());
        assert!(RgbColor::BLACK.with_brightness_ev_at_depth(RgbSpace::Srgb, 1., SampleDepth::F16).is_err());
        assert!(color.with_brightness_ev_at_depth(RgbSpace::Srgb, f32::NAN, SampleDepth::F16).is_err());
    }

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
