//! Delivery choices describe a copy of the master. Hosts own dialogs and jobs.
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Tiff,
}
impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tiff => "tif",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG image",
            Self::Tiff => "TIFF image",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportBackground {
    Preserve,
    White,
    Black,
}
impl ExportBackground {
    /// Neutral endpoints are identical in each supported linear RGB space.
    pub fn matte(self) -> Option<[f32; 3]> {
        match self {
            Self::Preserve => None,
            Self::White => Some([1.; 3]),
            Self::Black => Some([0.; 3]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRecipe {
    pub format: ExportFormat,
    pub color: DocumentColor,
    pub background: ExportBackground,
}
impl ExportRecipe {
    pub fn web_share() -> Self {
        Self {
            format: ExportFormat::Png,
            color: DocumentColor::default(),
            background: ExportBackground::Preserve,
        }
    }
    pub fn wide_color() -> Self {
        Self {
            color: DocumentColor {
                space: RgbSpace::DisplayP3,
                depth: IntegerDepth::U8,
            },
            ..Self::web_share()
        }
    }
    pub fn further_editing(document: DocumentColor) -> Self {
        Self {
            format: ExportFormat::Tiff,
            color: DocumentColor {
                space: document.space,
                depth: IntegerDepth::U16,
            },
            background: ExportBackground::Preserve,
        }
    }
    pub fn interpretation(self) -> SourceInterpretation {
        SourceInterpretation {
            channels: if self.background == ExportBackground::Preserve {
                SourceChannels::Rgba
            } else {
                SourceChannels::Rgb
            },
            depth: self.color.depth,
            profile: ColorProfile::Builtin(self.color.space),
            profile_assumed: false,
        }
    }
    pub fn filename(self, suggested: &str) -> String {
        let stem = suggested
            .rsplit_once('.')
            .map_or(suggested, |(stem, _)| stem);
        format!("{stem}.{}", self.format.extension())
    }
}
