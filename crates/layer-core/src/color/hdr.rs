//! Portable display-referred HDR semantics. Displays never redefine RGB 1.
use super::f16;
use serde::{Deserialize, Serialize};

pub const REFERENCE_WHITE_NITS: f32 = 203.;
pub const MAX_LINEAR: f32 = 65504.;
pub fn bt2020_to_srgb() -> super::rgb::Matrix3 {
    super::rgb::linear_rgb_transform(
        [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        [0.3127, 0.3290],
        super::RgbSpace::Srgb,
    )
}
pub fn srgb_to_bt2020() -> super::rgb::Matrix3 {
    super::rgb::inverse(bt2020_to_srgb())
}

/// Validate straight RGB before half quantization. Zero-alpha hidden RGB may be
/// retained in source files; edited pixels use canonical transparent black.
pub fn encode_pixel(pixel: [f32; 4]) -> Result<[u16; 4], &'static str> {
    if pixel.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&pixel[3]) {
        return Err("HDR requires finite RGB and coverage between zero and one");
    }
    if pixel[..3].iter().any(|v| v.abs() > MAX_LINEAR) {
        return Err("HDR RGB exceeds the supported half-float range");
    }
    Ok(pixel.map(|v| f16::from_f32(v).to_bits()))
}

pub fn decode_pixel(bits: [u16; 4]) -> Result<[f32; 4], &'static str> {
    let pixel = bits.map(|v| f16::from_bits(v).to_f32());
    encode_pixel(pixel)?;
    Ok(pixel)
}

/// A deliberate SDR rendition, owned by the document and shared by viewing,
/// proofing and delivery. The shoulder begins at `knee` and tends toward white;
/// negative channels are mapped to black in this rendition only.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdrRendition {
    pub exposure: f32,
    pub contrast: f32,
    pub knee: f32,
}
impl Default for SdrRendition {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 1.,
            knee: 0.75,
        }
    }
}
impl SdrRendition {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.exposure.is_finite()
            || !(-12. ..=12.).contains(&self.exposure)
            || !self.contrast.is_finite()
            || !(0.25..=4.).contains(&self.contrast)
            || !self.knee.is_finite()
            || !(0.25..=0.95).contains(&self.knee)
        {
            return Err("Invalid SDR rendition settings");
        }
        Ok(())
    }
    /// Linear straight document RGB. Max-channel scaling preserves chromaticity
    /// of nonnegative colors without introducing a per-channel shoulder hue shift.
    pub fn map_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let rgb = rgb.map(|v| v.max(0.));
        let peak = rgb.into_iter().fold(0., f32::max);
        if peak == 0. {
            return [0.; 3];
        }
        let x = 0.18 * (peak * self.exposure.exp2() / 0.18).powf(self.contrast);
        let mapped = if x <= self.knee {
            x
        } else {
            1. - (1. - self.knee).powi(2) / (x + 1. - 2. * self.knee)
        };
        rgb.map(|v| v / peak * mapped)
    }
    pub fn map_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.map_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
}

/// SMPTE ST 2084 (PQ), absolute luminance in cd/m². Float64 is used only at
/// interchange boundaries, not as a separate document processing mode.
pub fn pq_decode(code: f64) -> f64 {
    let p = code.powf(32. / 2523.);
    10000. * ((p - 3424. / 4096.).max(0.) / (2413. / 128. - 2392. / 128. * p)).powf(16384. / 2610.)
}
pub fn pq_encode(nits: f64) -> f64 {
    let p = (nits / 10000.).powf(2610. / 16384.);
    ((3424. / 4096. + 2413. / 128. * p) / (1. + 2392. / 128. * p)).powf(2523. / 32.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_finite_half_code_round_trips_with_subnormals_and_signed_zero() {
        for bits in 0..=u16::MAX {
            let value = f16::from_bits(bits).to_f32();
            if value.is_finite() {
                assert_eq!(encode_pixel([value, 0., 0., 1.]).unwrap()[0], bits);
            } else {
                assert!(encode_pixel([value, 0., 0., 1.]).is_err());
            }
        }
        assert!(encode_pixel([65505., 0., 0., 1.]).is_err());
        assert!(encode_pixel([1., 0., 0., -0.1]).is_err());
    }
    #[test]
    fn pq_absolute_landmarks_and_every_16_bit_code() {
        assert!((pq_decode(0.508078421517399) - 100.).abs() < 1e-8);
        assert!((pq_decode(0.751827096247041) - 1000.).abs() < 1e-7);
        assert_eq!(pq_decode(1.), 10000.);
        for code in 0..=65535 {
            assert_eq!(
                (pq_encode(pq_decode(code as f64 / 65535.)) * 65535.).round() as u32,
                code
            );
        }
    }
    #[test]
    fn rendition_is_monotone_bounded_and_preserves_alpha_and_rgb_ratios() {
        let r = SdrRendition::default();
        let mut previous = 0.;
        for i in 0..=65504 {
            let p = i as f32 / 16.;
            let mapped = r.map_rgb([p, p / 2., -p]);
            assert!((previous..=1.).contains(&mapped[0]));
            assert_eq!(mapped[1], mapped[0] / 2.);
            assert_eq!(mapped[2], 0.);
            previous = mapped[0];
        }
        assert_eq!(r.map_premultiplied([2., -1., 0., 0.]), [0.; 4]);
        assert_eq!(r.map_premultiplied([1., 0.5, 0., 0.25])[3], 0.25);
    }
}
