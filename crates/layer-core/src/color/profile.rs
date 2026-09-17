use super::RgbSpace;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfileChannels {
    Rgb,
    Gray,
    Cmyk,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SampleDepth {
    #[default]
    U8,
    U16,
    /// Display-referred linear binary16; RGB 1 is 203 cd/m².
    F16,
}
impl SampleDepth {
    pub fn is_float(self) -> bool { self == Self::F16 }
    pub fn coverage(self) -> Self { if self.is_float() { Self::U16 } else { self } }
    pub fn label(self) -> &'static str { match self { Self::U8 => "8-bit integer SDR", Self::U16 => "16-bit integer SDR", Self::F16 => "16-bit float HDR" } }
    pub fn bits(self) -> u8 {
        if self == Self::U8 { 8 } else { 16 }
    }
    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }
    pub fn maximum(self) -> u32 {
        match self { Self::U8 => 255, Self::U16 => 65535, Self::F16 => panic!("Float samples have no integer code maximum") }
    }
}

/// Embedded bytes are authoritative and survive saving exactly. A human label
/// is never used as a profile identity. Hosts validate ICC support before use.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorProfile {
    Builtin(RgbSpace),
    Icc(Arc<[u8]>),
}

impl Default for ColorProfile {
    fn default() -> Self {
        Self::Builtin(RgbSpace::Srgb)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RenderingIntent {
    Perceptual,
    #[default]
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}

/// Conversion policy is independent of profile, channel depth and processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversionOptions {
    pub intent: RenderingIntent,
    /// Reserved for document compatibility. The portable CMM currently rejects
    /// explicit black-point compensation rather than silently ignoring it.
    pub black_point_compensation: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            intent: RenderingIntent::RelativeColorimetric,
            black_point_compensation: false,
        }
    }
}
