//! Output reduction is independent of the CMM and the editable document depth.
use super::{ConversionOptions, IntegerDepth};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputDither {
    #[default]
    None,
    /// Coordinate-stable stochastic rounding of 8-bit color, never coverage.
    Stochastic8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputEncoding {
    pub conversion: ConversionOptions,
    pub dither: OutputDither,
}
impl OutputEncoding {
    pub fn validate(self, depth: IntegerDepth) -> Result<(), String> {
        if self.dither != OutputDither::None && depth != IntegerDepth::U8 {
            return Err("Output dithering requires 8-bit delivery".into());
        }
        Ok(())
    }
}
