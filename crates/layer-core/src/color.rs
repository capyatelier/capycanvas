//! Document interpretation and byte layout. GPU formats are a separate choice.
use serde::{Deserialize, Serialize};
pub mod rgb;
pub use rgb::RgbSpace;
mod profile;
pub use profile::{ColorProfile, ConversionOptions, IntegerDepth, RenderingIntent};
mod output;
pub use output::{OutputDither, OutputEncoding};
pub mod source;

/// Native SDR coordinates and committed integer precision. Working math and
/// per-operation blend domains are independent of these storage choices.
/// The archive version fixes the built-in RGB definitions; monitor state never
/// changes the permanent interpretation of artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentColor {
    pub space: RgbSpace,
    pub depth: IntegerDepth,
}
impl DocumentColor {
    pub fn paint_descriptor(self) -> PixelDescriptor {
        PixelDescriptor {
            channels: 4,
            bits_per_channel: self.depth.bits(),
            encoding: if self == Self::default() {
                TransferEncoding::Srgb
            } else {
                TransferEncoding::Profile
            },
            // New native modes retain straight RGB codes at low coverage.
            // The currently exposed sRGB8 renderer still stores encoded linear
            // premultiplication; its replacement is a separate mode adoption.
            alpha: if self == Self::default() {
                AlphaAssociation::PremultipliedLinear
            } else {
                AlphaAssociation::Straight
            },
        }
    }
    pub fn coverage_descriptor(self) -> PixelDescriptor {
        PixelDescriptor {
            bits_per_channel: self.depth.bits(),
            ..PixelDescriptor::COVERAGE8
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaAssociation {
    None,
    Straight,
    /// Decode RGB first, then divide by linear coverage. Stored RGB is
    /// encode(linear_RGB * alpha), not encoded_RGB * alpha.
    PremultipliedLinear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferEncoding {
    Linear,
    Srgb,
    /// Encoded channels interpreted by the source/document profile.
    Profile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelDescriptor {
    pub channels: u8,
    pub bits_per_channel: u8,
    pub encoding: TransferEncoding,
    pub alpha: AlphaAssociation,
}

impl PixelDescriptor {
    pub const SRGB8_STRAIGHT: Self = Self {
        channels: 4,
        bits_per_channel: 8,
        encoding: TransferEncoding::Srgb,
        alpha: AlphaAssociation::Straight,
    };
    pub const SRGB8_PAINT: Self = Self {
        alpha: AlphaAssociation::PremultipliedLinear,
        ..Self::SRGB8_STRAIGHT
    };
    pub const COVERAGE8: Self = Self {
        channels: 1,
        bits_per_channel: 8,
        encoding: TransferEncoding::Linear,
        alpha: AlphaAssociation::None,
    };

    pub fn bytes_per_pixel(self) -> Option<usize> {
        if !matches!(self.bits_per_channel, 8 | 16) {
            return None;
        }
        let valid_channels = match self.alpha {
            AlphaAssociation::None => matches!(self.channels, 1 | 3 | 4),
            AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear => {
                matches!(self.channels, 2 | 4)
            }
        };
        valid_channels
            .then_some(usize::from(self.channels) * usize::from(self.bits_per_channel / 8))
    }

    pub fn byte_len(self, extent: [u32; 2]) -> Option<usize> {
        if extent.contains(&0) {
            return None;
        }
        usize::try_from(extent[0])
            .ok()?
            .checked_mul(usize::try_from(extent[1]).ok()?)?
            .checked_mul(self.bytes_per_pixel()?)
    }
}

pub fn srgb_decode(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub fn srgb_encode(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

/// A raw sRGB paint texel becomes straight linear color for brush/UI consumers.
/// Alpha-zero is canonical transparent black, independent of unused RGB bits.
pub fn decode_paint_texel(texel: [u8; 4]) -> [f32; 4] {
    let alpha = f32::from(texel[3]) / 255.;
    if alpha == 0. {
        return [0.; 4];
    }
    [
        srgb_decode(f32::from(texel[0]) / 255.) / alpha,
        srgb_decode(f32::from(texel[1]) / 255.) / alpha,
        srgb_decode(f32::from(texel[2]) / 255.) / alpha,
        alpha,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoded8_preserves_every_opaque_gray_code() {
        for value in 0..=255 {
            let decoded = srgb_decode(value as f32 / 255.);
            assert_eq!((srgb_encode(decoded) * 255.).round() as u32, value);
        }
        let old: std::collections::BTreeSet<_> = (0..=50)
            .map(|v| (srgb_decode(v as f32 / 255.) * 255.).round() as u8)
            .collect();
        assert_eq!(old.len(), 9);
    }
    #[test]
    fn alpha_is_linear_and_association_precedes_transfer() {
        assert_eq!(decode_paint_texel([255, 200, 128, 0]), [0.; 4]);
        for alpha in [1u8, 2, 8, 32, 128, 255] {
            let a = f32::from(alpha) / 255.;
            let code = (srgb_encode(0.25 * a) * 255.).round() as u8;
            let decoded = decode_paint_texel([code, code, code, alpha]);
            assert_eq!(decoded[3], a);
            // One encoded half-code, decoded at its endpoints, is the declared
            // straight-color tolerance. A fixed epsilon hides low-alpha loss.
            let lo = srgb_decode((f32::from(code) - 0.5).max(0.) / 255.) / a;
            let hi = srgb_decode((f32::from(code) + 0.5).min(255.) / 255.) / a;
            assert!(lo <= 0.25 && hi >= 0.25);
        }
    }
    #[test]
    fn unsupported_layouts_and_invalid_extents_fail() {
        assert_eq!(
            PixelDescriptor::SRGB8_PAINT.byte_len([256, 256]),
            Some(262144)
        );
        assert_eq!(PixelDescriptor::COVERAGE8.byte_len([256, 256]), Some(65536));
        assert_eq!(PixelDescriptor::SRGB8_PAINT.byte_len([0, 1]), None);
        assert_eq!(
            PixelDescriptor {
                bits_per_channel: 32,
                ..PixelDescriptor::SRGB8_PAINT
            }
            .byte_len([1, 1]),
            None
        );
        assert_eq!(
            PixelDescriptor::SRGB8_PAINT.byte_len([u32::MAX, u32::MAX]),
            None
        );
    }
}
