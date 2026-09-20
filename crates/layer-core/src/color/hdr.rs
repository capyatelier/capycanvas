//! Portable display-referred HDR semantics. Displays never redefine RGB 1.
use super::f16;

mod local;
mod sdr;
pub use local::{LOCAL_GUIDE_EDGE, LocalToneBuilder, LocalToneGuide};
pub use sdr::{
    BT2020_LUMA, SdrMapper, SdrRendition, compress_sdr_gamut, sdr_luminance_weights,
    to_bt2020, unified_sdr_gamut,
};

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
    validate_pixel(super::SampleDepth::F16, pixel)?;
    Ok(pixel.map(|v| f16::from_f32(v).to_bits()))
}

/// Validate without quantizing. Signed RGB, subnormals and hidden RGB are valid;
/// alpha is finite linear coverage in [0, 1]. Storage never silently clamps.
pub fn validate_pixel(depth: super::SampleDepth, pixel: [f32; 4]) -> Result<(), &'static str> {
    if !depth.is_float() { return Err("Expected floating-point storage"); }
    if pixel.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&pixel[3]) {
        return Err("HDR requires finite RGB and coverage between zero and one");
    }
    if pixel[..3].iter().any(|v| v.abs() > depth.max_linear()) {
        return Err("HDR RGB exceeds the selected storage range");
    }
    Ok(())
}

/// Decode one little-endian RGB/RGBA pixel at its declared precision.
pub fn decode_samples(depth: super::SampleDepth, bytes: &[u8]) -> Result<[f32; 4], &'static str> {
    let step = depth.bytes();
    if !depth.is_float() || ![3 * step, 4 * step].contains(&bytes.len()) {
        return Err("Incomplete floating-point RGB/RGBA pixel");
    }
    let mut pixel = [0., 0., 0., 1.];
    for (channel, sample) in bytes.chunks_exact(step).enumerate() {
        pixel[channel] = if depth == super::SampleDepth::F32 {
            f32::from_le_bytes(sample.try_into().unwrap())
        } else { f16::from_bits(u16::from_le_bytes(sample.try_into().unwrap())).to_f32() };
    }
    validate_pixel(depth, pixel)?;
    Ok(pixel)
}

pub fn decode_pixel(bits: [u16; 4]) -> Result<[f32; 4], &'static str> {
    let pixel = bits.map(|v| f16::from_bits(v).to_f32());
    encode_pixel(pixel)?;
    Ok(pixel)
}

/// HDR display shoulder, matching `hdr_view.wgsl`. Signed RGB is scaled together
/// and coverage is unchanged. This is a viewing derivative, never editing data.
pub fn map_display_premultiplied(p: [f32; 4], headroom: f32) -> [f32; 4] {
    if p[3] <= 0. { return p; }
    let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
    let peak = rgb.into_iter().map(f32::abs).fold(0., f32::max);
    if peak == 0. { return [0., 0., 0., p[3]]; }
    let knee = headroom * 0.75;
    let mapped = if peak <= knee { peak }
        else { headroom - (headroom - knee).powi(2) / (peak + headroom - 2. * knee) };
    let rgb = rgb.map(|v| v / peak * mapped * p[3]);
    [rgb[0], rgb[1], rgb[2], p[3]]
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
}
