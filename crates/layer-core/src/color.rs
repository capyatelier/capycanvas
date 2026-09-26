//! Document interpretation and byte layout. GPU formats are a separate choice.
use serde::{Deserialize, Serialize};
pub mod rgb;
pub mod oklab;
pub use rgb::RgbSpace;
mod value;
pub use value::RgbColor;
mod profile;
pub use profile::{ColorProfile, ProfileReference, ConversionOptions, SampleDepth, ProfileChannels, RenderingIntent};
mod output;
pub use output::{OutputDither, OutputEncoding};
mod proof;
pub use proof::ProofRecipe;
pub mod source;
pub mod histogram;
pub mod hdr;
pub use half::f16;

/// Native RGB coordinates and committed sample precision. Working math and
/// per-operation blend domains are independent of these storage choices.
/// The archive version fixes the built-in RGB definitions; monitor state never
/// changes the permanent interpretation of artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentColor {
    pub space: RgbSpace,
    pub depth: SampleDepth,
}
impl DocumentColor {
    pub fn paint_descriptor(self) -> PixelDescriptor {
        PixelDescriptor {
            channels: 4,
            bits_per_channel: self.depth.bits(),
            sample: if self.depth.is_float() { SampleType::Float } else { SampleType::Unsigned },
            encoding: if self.depth.is_float() { TransferEncoding::Linear } else { TransferEncoding::Profile },
            alpha: AlphaAssociation::Straight,
        }
    }
    pub fn coverage_descriptor(self) -> PixelDescriptor {
        PixelDescriptor {
            bits_per_channel: self.depth.coverage().bits(),
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
    #[serde(default, skip_serializing_if = "SampleType::is_unsigned")]
    pub sample: SampleType,
    pub channels: u8,
    pub bits_per_channel: u8,
    pub encoding: TransferEncoding,
    pub alpha: AlphaAssociation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleType { #[default] Unsigned, Float }
impl SampleType { pub fn is_unsigned(&self) -> bool { *self == Self::Unsigned } }

impl PixelDescriptor {
    pub fn depth(self) -> SampleDepth {
        if self.sample == SampleType::Float { if self.bits_per_channel == 32 { SampleDepth::F32 } else { SampleDepth::F16 } } else if self.bits_per_channel == 16 { SampleDepth::U16 } else { SampleDepth::U8 }
    }
    pub const SRGB8_STRAIGHT: Self = Self {
        sample: SampleType::Unsigned,
        channels: 4,
        bits_per_channel: 8,
        encoding: TransferEncoding::Srgb,
        alpha: AlphaAssociation::Straight,
    };
    /// Explicit attachment layout used by hosts awaiting native SDR adoption.
    /// Canonical document paint uses `DocumentColor::paint_descriptor()`.
    pub const SRGB8_PAINT: Self = Self {
        alpha: AlphaAssociation::PremultipliedLinear,
        ..Self::SRGB8_STRAIGHT
    };
    pub const COVERAGE8: Self = Self {
        sample: SampleType::Unsigned,
        channels: 1,
        bits_per_channel: 8,
        encoding: TransferEncoding::Linear,
        alpha: AlphaAssociation::None,
    };

    pub fn validate_samples(self, bytes: &[u8]) -> Result<(), String> {
        if self.sample != SampleType::Float { return Ok(()); }
        let bpp = self.bytes_per_pixel().ok_or("Invalid float descriptor")?;
        if bytes.len() % bpp != 0 { return Err("Incomplete float samples".into()); }
        for input in bytes.chunks_exact(bpp) {
            hdr::decode_samples(self.depth(), input).map_err(str::to_string)?;
        }
        Ok(())
    }
    pub fn bytes_per_pixel(self) -> Option<usize> {
        if self.sample == SampleType::Float && (!matches!(self.bits_per_channel, 16 | 32) || self.encoding != TransferEncoding::Linear || !matches!((self.channels, self.alpha), (3, AlphaAssociation::None) | (4, AlphaAssociation::Straight))) { return None; }
        if self.sample == SampleType::Unsigned && !matches!(self.bits_per_channel, 8 | 16) {
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
