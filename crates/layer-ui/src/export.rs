//! Delivery choices describe a copy of the master. Hosts own dialogs and jobs.
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::color::{
    ColorProfile, DocumentColor, IntegerDepth, OutputEncoding, ProfileChannels, RgbSpace,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Tiff,
    Jpeg,
}
impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tiff => "tif",
            Self::Jpeg => "jpg",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG image",
            Self::Tiff => "TIFF image",
            Self::Jpeg => "JPEG image",
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

/// Display names are descriptive only; embedded bytes define the output color.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProfile {
    pub profile: ColorProfile,
    pub channels: ProfileChannels,
    pub name: String,
}
impl ExportProfile {
    pub fn builtin(space: RgbSpace) -> Self {
        Self {
            profile: ColorProfile::Builtin(space),
            channels: ProfileChannels::Rgb,
            name: space.name().into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportSize {
    Original,
    /// Preserve proportions inside a pixel box; never crop or stretch the copy.
    Fit {
        bounds: [u32; 2],
        enlarge: bool,
    },
}
impl ExportSize {
    pub fn extent(&self, source: [u32; 2]) -> Result<[u32; 2], String> {
        let validate = |size: [u32; 2]| {
            if size.into_iter().all(|v| (1..=32768).contains(&v)) {
                Ok(())
            } else {
                Err("Image dimensions must be between 1 and 32768 pixels".to_string())
            }
        };
        validate(source)?;
        let Self::Fit { bounds, enlarge } = self else {
            return Ok(source);
        };
        validate(*bounds)?;
        let axis = usize::from(
            u64::from(bounds[0]) * u64::from(source[1])
                > u64::from(bounds[1]) * u64::from(source[0]),
        );
        let numerator = if *enlarge {
            bounds[axis]
        } else {
            bounds[axis].min(source[axis])
        };
        let denominator = source[axis];
        Ok(source.map(|value| {
            ((u64::from(value) * u64::from(numerator) + u64::from(denominator) / 2)
                / u64::from(denominator))
            .max(1) as u32
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportResolution {
    Master,
    Ppi(u32),
    Omit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRecipe<P = ExportProfile> {
    pub format: ExportFormat,
    pub profile: P,
    pub depth: IntegerDepth,
    pub background: ExportBackground,
    pub jpeg_quality: u8,
    pub encoding: OutputEncoding,
    pub size: ExportSize,
    pub resolution: ExportResolution,
}
impl<P> ExportRecipe<P> {
    /// Storage can intern large ICC profiles without duplicating delivery policy.
    pub fn with_profile<Q>(self, profile: Q) -> ExportRecipe<Q> {
        ExportRecipe {
            profile,
            format: self.format,
            depth: self.depth,
            background: self.background,
            jpeg_quality: self.jpeg_quality,
            encoding: self.encoding,
            size: self.size,
            resolution: self.resolution,
        }
    }
}
impl ExportRecipe {
    pub fn web_share() -> Self {
        Self {
            format: ExportFormat::Png,
            profile: ExportProfile::builtin(RgbSpace::Srgb),
            depth: IntegerDepth::U8,
            background: ExportBackground::Preserve,
            jpeg_quality: 90,
            encoding: Default::default(),
            size: ExportSize::Original,
            resolution: ExportResolution::Master,
        }
    }
    pub fn wide_color() -> Self {
        Self {
            profile: ExportProfile::builtin(RgbSpace::DisplayP3),
            ..Self::web_share()
        }
    }
    pub fn further_editing(document: DocumentColor) -> Self {
        Self {
            format: ExportFormat::Tiff,
            profile: ExportProfile::builtin(document.space),
            depth: IntegerDepth::U16,
            background: ExportBackground::Preserve,
            jpeg_quality: 90,
            encoding: Default::default(),
            size: ExportSize::Original,
            resolution: ExportResolution::Master,
        }
    }
    pub fn interpretation(&self) -> SourceInterpretation {
        SourceInterpretation {
            channels: match (
                self.profile.channels,
                self.background == ExportBackground::Preserve,
            ) {
                (ProfileChannels::Rgb, true) => SourceChannels::Rgba,
                (ProfileChannels::Rgb, false) => SourceChannels::Rgb,
                (ProfileChannels::Gray, true) => SourceChannels::GrayAlpha,
                (ProfileChannels::Gray, false) => SourceChannels::Gray,
                (ProfileChannels::Cmyk, _) => SourceChannels::Cmyk,
            },
            depth: self.depth,
            profile: self.profile.profile.clone(),
            profile_assumed: false,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if let ExportResolution::Ppi(value) = self.resolution
            && !(1..=65535).contains(&value)
        {
            return Err("Resolution must be between 1 and 65535 pixels per inch".into());
        }
        self.size.extent([1, 1])?;
        self.encoding.validate(self.depth)?;
        if self.profile.channels == ProfileChannels::Cmyk {
            if self.format == ExportFormat::Png {
                return Err("Choose TIFF or JPEG for a CMYK profile".into());
            }
            if self.background == ExportBackground::Preserve {
                return Err("Choose a background for CMYK transparency".into());
            }
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err("JPEG quality must be between 1 and 100".into());
        }
        if self.format == ExportFormat::Jpeg {
            if self.depth != IntegerDepth::U8 {
                return Err("JPEG output requires 8-bit samples".into());
            }
            if self.background == ExportBackground::Preserve {
                return Err("Choose a background for JPEG transparency".into());
            }
        }
        Ok(())
    }
    pub fn filename(&self, suggested: &str) -> String {
        let stem = suggested
            .rsplit_once('.')
            .map_or(suggested, |(stem, _)| stem);
        format!("{stem}.{}", self.format.extension())
    }
    pub fn output_resolution(
        &self,
        master: Option<layer_core::ImageResolution>,
    ) -> Result<Option<layer_core::ImageResolution>, String> {
        let value = match self.resolution {
            ExportResolution::Master => master,
            ExportResolution::Ppi(value) => Some(layer_core::ImageResolution::ppi(value)),
            ExportResolution::Omit => None,
        };
        if let Some(value) = value {
            match self.format {
                ExportFormat::Png => {
                    value.png_density()?;
                }
                ExportFormat::Tiff => {
                    value.tiff_density()?;
                }
                ExportFormat::Jpeg => {
                    value.jfif_density()?;
                }
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_size_fits_orientation_without_distorting_or_changing_the_master() {
        for (source, bounds, enlarge, expected) in [
            ([6000, 4000], [2048, 2048], false, [2048, 1365]),
            ([4000, 6000], [2048, 2048], false, [1365, 2048]),
            ([33, 17], [100, 100], false, [33, 17]),
            ([33, 17], [100, 100], true, [100, 52]),
            ([32768, 1], [1, 32768], false, [1, 1]),
            ([1, 32768], [32768, 1], true, [1, 1]),
            ([1, 1], [32768, 32768], true, [32768, 32768]),
        ] {
            let size = ExportSize::Fit { bounds, enlarge };
            assert_eq!(size.extent(source).unwrap(), expected);
            assert_eq!(ExportSize::Original.extent(source).unwrap(), source);
            let mut recipe = ExportRecipe::further_editing(DocumentColor {
                space: RgbSpace::ProPhoto,
                depth: IntegerDepth::U16,
            });
            recipe.size = size;
            recipe.validate().unwrap();
            assert_eq!(
                serde_json::from_slice::<ExportRecipe>(&serde_json::to_vec(&recipe).unwrap())
                    .unwrap(),
                recipe
            );
            assert_eq!(recipe.depth, IntegerDepth::U16);
            assert_eq!(recipe.profile, ExportProfile::builtin(RgbSpace::ProPhoto));
        }
        for size in [[0, 1], [1, 32769]] {
            assert!(ExportSize::Original.extent(size).is_err());
            let mut recipe = ExportRecipe::web_share();
            recipe.size = ExportSize::Fit {
                bounds: size,
                enlarge: false,
            };
            assert!(recipe.validate().is_err());
        }
    }

    #[test]
    fn destination_channels_depth_and_transparency_follow_the_delivery_recipe() {
        for (model, format, depth, background, expected_channels, valid) in [
            (
                ProfileChannels::Rgb,
                ExportFormat::Png,
                IntegerDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::Rgba,
                true,
            ),
            (
                ProfileChannels::Gray,
                ExportFormat::Png,
                IntegerDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::GrayAlpha,
                true,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Png,
                IntegerDepth::U8,
                ExportBackground::White,
                SourceChannels::Cmyk,
                false,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Tiff,
                IntegerDepth::U16,
                ExportBackground::White,
                SourceChannels::Cmyk,
                true,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Tiff,
                IntegerDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::Cmyk,
                false,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Jpeg,
                IntegerDepth::U8,
                ExportBackground::Black,
                SourceChannels::Cmyk,
                true,
            ),
            (
                ProfileChannels::Gray,
                ExportFormat::Jpeg,
                IntegerDepth::U8,
                ExportBackground::White,
                SourceChannels::Gray,
                true,
            ),
            (
                ProfileChannels::Rgb,
                ExportFormat::Jpeg,
                IntegerDepth::U16,
                ExportBackground::White,
                SourceChannels::Rgb,
                false,
            ),
            (
                ProfileChannels::Rgb,
                ExportFormat::Jpeg,
                IntegerDepth::U8,
                ExportBackground::Preserve,
                SourceChannels::Rgba,
                false,
            ),
        ] {
            let recipe = ExportRecipe {
                format,
                depth,
                background,
                profile: ExportProfile {
                    channels: model,
                    profile: ColorProfile::Icc(std::sync::Arc::from([1, 2, 3])),
                    name: "profile label".into(),
                },
                ..ExportRecipe::web_share()
            };
            assert_eq!(recipe.validate().is_ok(), valid);
            let interpretation = recipe.interpretation();
            assert_eq!(interpretation.channels, expected_channels);
            assert_eq!(interpretation.depth, depth);
            assert_eq!(interpretation.profile, recipe.profile.profile);
            assert!(!interpretation.profile_assumed);
            // The host validates ICC contents separately; serialization must
            // retain the opaque payload/channel metadata rather than its label.
            let json = serde_json::to_vec(&recipe).unwrap();
            assert_eq!(
                serde_json::from_slice::<ExportRecipe>(&json).unwrap(),
                recipe
            );
        }
    }
}
