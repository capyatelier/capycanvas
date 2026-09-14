use super::RgbSpace;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegerDepth {
    #[default]
    U8,
    U16,
}
impl IntegerDepth {
    pub fn bits(self) -> u8 {
        if self == Self::U8 { 8 } else { 16 }
    }
    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }
    pub fn maximum(self) -> u32 {
        if self == Self::U8 { 255 } else { 65535 }
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
    pub black_point_compensation: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            intent: RenderingIntent::RelativeColorimetric,
            black_point_compensation: true,
        }
    }
}
