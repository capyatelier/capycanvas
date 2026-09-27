//! Delivery choices describe a copy of the master. Hosts own dialogs and jobs.
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::color::{
    ColorProfile, DocumentColor, SampleDepth, OutputEncoding, ProfileChannels, RgbSpace,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Exr,
    PngHdr,
    PngHdrMapped,
    JpegHdr,
    JpegHdrMapped,
    AvifHdr,
    AvifHdrMapped,
    Png,
    Tiff,
    Jpeg,
    Webp,
}
impl ExportFormat {
    pub fn is_hdr(self) -> bool { matches!(self, Self::Exr | Self::PngHdr | Self::PngHdrMapped | Self::JpegHdr | Self::JpegHdrMapped | Self::AvifHdr | Self::AvifHdrMapped) }
    pub fn maps_hdr_range(self) -> bool { matches!(self, Self::PngHdrMapped | Self::JpegHdrMapped | Self::AvifHdrMapped) }
    pub fn with_hdr_range_mapping(self, mapped: bool) -> Self {
        match self {
            Self::PngHdr | Self::PngHdrMapped => if mapped {Self::PngHdrMapped} else {Self::PngHdr},
            Self::JpegHdr | Self::JpegHdrMapped => if mapped {Self::JpegHdrMapped} else {Self::JpegHdr},
            Self::AvifHdr | Self::AvifHdrMapped => if mapped {Self::AvifHdrMapped} else {Self::AvifHdr},
            _=>self,
        }
    }
    pub fn gainmap(self) -> Option<layer_color::photo::GainMapFormat> { match self {
        Self::JpegHdr | Self::JpegHdrMapped => Some(layer_color::photo::GainMapFormat::Jpeg),
        Self::AvifHdr | Self::AvifHdrMapped => Some(layer_color::photo::GainMapFormat::Avif), _ => None,
    }}
    pub fn extension(self) -> &'static str {
        match self {
            Self::Exr => "exr",
            Self::Png | Self::PngHdr | Self::PngHdrMapped => "png",
            Self::Tiff => "tif",
            Self::Jpeg | Self::JpegHdr | Self::JpegHdrMapped => "jpg",
            Self::AvifHdr | Self::AvifHdrMapped => "avif",
            Self::Webp => "webp",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Exr => "OpenEXR · 32-bit float",
            Self::Png => "PNG image",
            Self::PngHdr => "HDR PNG · BT.2020 PQ",
            Self::PngHdrMapped => "HDR PNG · clipped to PQ range",
            Self::Tiff => "TIFF image",
            Self::Jpeg => "JPEG image",
            Self::JpegHdr | Self::JpegHdrMapped => "HDR JPEG",
            Self::AvifHdr | Self::AvifHdrMapped => "HDR AVIF with transparency",
            Self::Webp => "WebP · lossless",
        }
    }
    /// SDR formats that can carry a delivery profile with these channels.
    fn sdr_choices(channels: ProfileChannels) -> Vec<Self> {
        match channels {
            ProfileChannels::Rgb => vec![Self::Png, Self::Tiff, Self::Jpeg, Self::Webp],
            ProfileChannels::Gray => vec![Self::Png, Self::Tiff, Self::Jpeg],
            ProfileChannels::Cmyk => vec![Self::Tiff, Self::Jpeg],
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
pub struct ExportProfile<P = ColorProfile> {
    pub profile: P,
    pub channels: ProfileChannels,
    pub name: String,
}
impl<P> ExportProfile<P> {
    pub fn with_profile<Q>(self, profile: Q) -> ExportProfile<Q> {
        ExportProfile { profile, channels: self.channels, name: self.name }
    }
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
    pub depth: SampleDepth,
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
    /// Validate the delivery transform from a document color on a file worker.
    pub fn validate_for_color(&self, color: DocumentColor) -> Result<(), String> {
        self.validate()?;
        if self.format.is_hdr() && !color.depth.is_float() { return Err("HDR delivery requires an HDR document".into()); }
        if layer_color::profile_channels(&self.profile.profile)? != self.profile.channels {
            return Err("Profile channels do not match the ICC data".into());
        }
        layer_color::WorkingEncoder::new(color.space, &self.interpretation(), self.encoding)?;
        Ok(())
    }
    /// Validate the complete delivery transform and size on a file worker.
    pub fn validate_for_document(&self, document: &layer_core::Document) -> Result<(), String> {
        self.validate_for_color(document.color)?;
        self.output_extent([document.width, document.height])?;
        self.output_resolution(document.resolution)?;
        Ok(())
    }
    /// Delivery pixel dimensions, refused when the format's encoder cannot
    /// write them.
    pub fn output_extent(&self, source: [u32; 2]) -> Result<[u32; 2], String> {
        let extent = self.size.extent(source)?;
        if self.format == ExportFormat::Webp && extent.iter().any(|v| *v > layer_color::photo::WEBP_MAX_DIMENSION) {
            return Err(format!(
                "{}. Fit the size within that or choose another format.",
                layer_color::photo::WEBP_SIZE_LIMIT
            ));
        }
        Ok(extent)
    }
    pub fn web_share() -> Self {
        Self {
            format: ExportFormat::Png,
            profile: ExportProfile::builtin(RgbSpace::Srgb),
            depth: SampleDepth::U8,
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
            format: if document.depth.is_float() { ExportFormat::Exr } else { ExportFormat::Tiff },
            profile: ExportProfile::builtin(document.space),
            depth: if document.depth.is_float() { SampleDepth::F32 } else { SampleDepth::U16 },
            background: ExportBackground::Preserve,
            jpeg_quality: 90,
            encoding: Default::default(),
            size: ExportSize::Original,
            resolution: ExportResolution::Master,
        }
    }
    pub fn for_color(self, color: DocumentColor) -> Self {
        if color.depth.is_float() || !self.format.is_hdr() {
            return self;
        }
        let (base, format) = match self.format {
            ExportFormat::Exr => (Self::further_editing(color), ExportFormat::Tiff),
            f if f.gainmap() == Some(layer_color::photo::GainMapFormat::Jpeg) => (Self::web_share(), ExportFormat::Jpeg),
            _ => (Self::web_share(), ExportFormat::Png),
        };
        Self {
            background: self.background,
            jpeg_quality: self.jpeg_quality,
            size: self.size,
            resolution: self.resolution,
            ..base
        }
        .draft(ExportDraftAction::Format(format))
        .recipe
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
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err("JPEG quality must be between 1 and 100".into());
        }
        if self.clone().draft(ExportDraftAction::Refresh).recipe != *self {
            return Err("Unsupported export combination".into());
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
                ExportFormat::Png | ExportFormat::PngHdr | ExportFormat::PngHdrMapped => {
                    value.png_density()?;
                }
                ExportFormat::Tiff | ExportFormat::Webp => {
                    value.tiff_density()?;
                }
                ExportFormat::Exr | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => (),
                ExportFormat::Jpeg | ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped => {
                    value.jfif_density()?;
                }
            }
        }
        Ok(value)
    }
}

/// Product-dependent draft transitions, shared by all native/browser forms.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ExportDraftAction {
    Refresh,
    ClipHdrRange(bool),
    Format(ExportFormat),
    Profile(ExportProfile),
    Depth(SampleDepth),
    Background(ExportBackground),
    Encoding(OutputEncoding),
}
#[derive(Serialize)]
pub struct ExportDraft {
    pub hdr: bool,
    pub clip_hdr_range: bool,
    pub format: ExportFormat,
    pub recipe: ExportRecipe,
    pub formats: Vec<ExportFormat>,
    pub depths: Vec<SampleDepth>,
    pub backgrounds: Vec<ExportBackground>,
    pub dithers: Vec<layer_core::color::OutputDither>,
}
impl ExportRecipe {
    pub fn draft_for_color(self, color: DocumentColor, action: ExportDraftAction) -> ExportDraft {
        let mut draft = self.draft(action);
        if draft.recipe.format == ExportFormat::Exr {
            draft.recipe.profile = ExportProfile::builtin(color.space);
        }
        draft.formats = ExportFormat::sdr_choices(draft.recipe.profile.channels);
        if color.depth.is_float() {
            draft.formats.extend([ExportFormat::PngHdr, ExportFormat::PngHdrMapped, ExportFormat::Exr,
                ExportFormat::JpegHdr, ExportFormat::JpegHdrMapped, ExportFormat::AvifHdr, ExportFormat::AvifHdrMapped]);
        }
        draft
    }
    pub fn draft(mut self, action: ExportDraftAction) -> ExportDraft {
        use layer_core::color::OutputDither;
        match action {
            ExportDraftAction::Refresh => (),
            ExportDraftAction::ClipHdrRange(v) => self.format=self.format.with_hdr_range_mapping(v),
            ExportDraftAction::Format(v) => self.format = v,
            ExportDraftAction::Profile(v) => self.profile = v,
            ExportDraftAction::Depth(v) => self.depth = v,
            ExportDraftAction::Background(v) => self.background = v,
            ExportDraftAction::Encoding(v) => self.encoding = v,
        }
        if self.format == ExportFormat::Exr {
            if !matches!(self.profile.profile, ColorProfile::Builtin(_)) { self.profile = ExportProfile::builtin(RgbSpace::Srgb); }
            self.depth = SampleDepth::F32;
            self.background = ExportBackground::Preserve;
            self.encoding = Default::default();
        } else if self.format.is_hdr() {
            self.profile = ExportProfile::builtin(RgbSpace::Srgb);
            self.depth = SampleDepth::U16;
            if self.format.gainmap()!=Some(layer_color::photo::GainMapFormat::Jpeg) { self.background = ExportBackground::Preserve; }
            self.encoding = Default::default();
        }
        if !self.format.is_hdr() && self.depth.is_float() { self.depth = SampleDepth::U16; }
        let cmyk = self.profile.channels == ProfileChannels::Cmyk;
        let choices = ExportFormat::sdr_choices(self.profile.channels);
        if !self.format.is_hdr() && !choices.contains(&self.format) {
            self.format = if cmyk { ExportFormat::Tiff } else { ExportFormat::Png };
        }
        let jpeg = self.format == ExportFormat::Jpeg;
        let eight_bit = jpeg || self.format == ExportFormat::Webp;
        if eight_bit { self.depth = SampleDepth::U8; }
        if (jpeg || cmyk) && self.background == ExportBackground::Preserve { self.background = ExportBackground::White; }
        if self.depth != SampleDepth::U8 { self.encoding.dither = OutputDither::None; }
        ExportDraft {
            hdr:self.format.is_hdr(),clip_hdr_range:self.format.maps_hdr_range(),format:self.format.with_hdr_range_mapping(false),
            formats: if self.format.is_hdr() { vec![ExportFormat::JpegHdr, ExportFormat::AvifHdr, ExportFormat::PngHdr, ExportFormat::Exr] } else { choices },
            depths: if self.format == ExportFormat::Exr { vec![SampleDepth::F32] } else if self.format.is_hdr() { vec![SampleDepth::U16] } else if eight_bit { vec![SampleDepth::U8] } else { vec![SampleDepth::U8, SampleDepth::U16] },
            backgrounds: if self.format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg) { vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black] } else if self.format.is_hdr() { vec![ExportBackground::Preserve] } else if jpeg || cmyk { vec![ExportBackground::White, ExportBackground::Black] } else { vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black] },
            dithers: if self.depth == SampleDepth::U8 { vec![OutputDither::None, OutputDither::Stochastic8] } else { vec![OutputDither::None] },
            recipe: self,
        }
    }
}

#[derive(Serialize)]
pub struct ExportForm {
    pub profiles: Vec<ExportProfile>,
    pub extent: [u32; 2],
}
impl ExportForm {
    pub fn new(document: &layer_core::Document) -> Self {
        let mut profiles: Vec<_> = RgbSpace::ALL
            .into_iter()
            .map(ExportProfile::builtin)
            .collect();
        for layer in &document.layers {
            if let Some(source) = &layer.source {
                let profile = &source.interpretation.profile;
                if profiles.iter().any(|p| p.profile == *profile) {
                    continue;
                }
                let channels = match source.interpretation.channels {
                    SourceChannels::Rgb | SourceChannels::Rgba => ProfileChannels::Rgb,
                    SourceChannels::Gray | SourceChannels::GrayAlpha => ProfileChannels::Gray,
                    SourceChannels::Cmyk => ProfileChannels::Cmyk,
                };
                profiles.push(ExportProfile {
                    profile: profile.clone(),
                    channels,
                    name: format!("Original: {}", layer.name),
                });
            }
        }
        Self {
            profiles,
            extent: [document.width, document.height],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_choices_describe_only_the_fixed_delivery_contract() {
        for format in [ExportFormat::PngHdr, ExportFormat::PngHdrMapped, ExportFormat::JpegHdr, ExportFormat::JpegHdrMapped, ExportFormat::AvifHdr, ExportFormat::AvifHdrMapped] {
            let draft = ExportRecipe::web_share().draft(ExportDraftAction::Format(format));
            assert_eq!(draft.formats, [ExportFormat::JpegHdr, ExportFormat::AvifHdr, ExportFormat::PngHdr, ExportFormat::Exr]);
            assert_eq!(draft.depths, [SampleDepth::U16]);
            if format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg){assert_eq!(draft.backgrounds,[ExportBackground::Preserve,ExportBackground::White,ExportBackground::Black]);}else{assert_eq!(draft.backgrounds, [ExportBackground::Preserve]);}
            assert_eq!(draft.dithers, [layer_core::color::OutputDither::None]);
            assert_eq!(draft.recipe.format, format);
            draft.recipe.validate().unwrap();
            for depth in [SampleDepth::F16, SampleDepth::F32] {
                let color = DocumentColor { space: RgbSpace::Srgb, depth };
                let choices = draft.recipe.clone().draft_for_color(color, ExportDraftAction::Refresh);
                assert!(choices.formats.contains(&format), "Authored range policy must remain selectable");
            }
            let integer = ExportRecipe::web_share().draft_for_color(DocumentColor::default(), ExportDraftAction::Refresh);
            assert!(!integer.formats.contains(&format));
        }
        let sdr = ExportRecipe::web_share().draft(ExportDraftAction::Refresh);
        assert_eq!(sdr.formats, [ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg, ExportFormat::Webp]);
        assert_eq!(sdr.depths, [SampleDepth::U8, SampleDepth::U16]);
    }

    #[test]
    fn draft_transitions_keep_supported_depth_alpha_and_dither_choices() {
        let mut recipe = ExportRecipe::further_editing(DocumentColor::default());
        recipe.encoding.dither = layer_core::color::OutputDither::Stochastic8;
        let jpeg = recipe.draft(ExportDraftAction::Format(ExportFormat::Jpeg));
        assert_eq!(jpeg.depths, [SampleDepth::U8]);
        assert_eq!(jpeg.recipe.depth, SampleDepth::U8);
        assert_eq!(jpeg.recipe.background, ExportBackground::White);
        jpeg.recipe.validate().unwrap();
        let png = jpeg.recipe.draft(ExportDraftAction::Format(ExportFormat::Png));
        let deep = png.recipe.draft(ExportDraftAction::Depth(SampleDepth::U16));
        assert_eq!(deep.recipe.encoding.dither, layer_core::color::OutputDither::None);
        deep.recipe.validate().unwrap();
        let cmyk = deep.recipe.draft(ExportDraftAction::Profile(ExportProfile { profile: ColorProfile::Icc(vec![1].into()), channels: ProfileChannels::Cmyk, name: "CMYK".into() }));
        assert_eq!(cmyk.formats, [ExportFormat::Tiff, ExportFormat::Jpeg]);
        assert_eq!(cmyk.recipe.format, ExportFormat::Tiff);
        assert!(!cmyk.backgrounds.contains(&ExportBackground::Preserve));
        cmyk.recipe.validate().unwrap();
        let mut invalid = cmyk.recipe;invalid.jpeg_quality = 0;
        assert!(invalid.draft(ExportDraftAction::Refresh).recipe.validate().is_err());
    }

    #[test]
    fn webp_drafts_as_lossless_eight_bit_rgb_with_transparency() {
        let mut recipe = ExportRecipe::further_editing(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
        recipe.encoding.dither = layer_core::color::OutputDither::Stochastic8;
        let webp = recipe.draft(ExportDraftAction::Format(ExportFormat::Webp));
        assert_eq!(webp.recipe.format, ExportFormat::Webp);
        assert_eq!((webp.recipe.depth, webp.depths.as_slice()), (SampleDepth::U8, [SampleDepth::U8].as_slice()));
        assert_eq!(webp.recipe.background, ExportBackground::Preserve);
        assert_eq!(webp.backgrounds, [ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black]);
        assert_eq!(webp.dithers, [layer_core::color::OutputDither::None, layer_core::color::OutputDither::Stochastic8]);
        assert_eq!(webp.formats, [ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg, ExportFormat::Webp]);
        assert_eq!(webp.recipe.interpretation().channels, SourceChannels::Rgba);
        webp.recipe.validate().unwrap();
        assert_eq!((ExportFormat::Webp.extension(), ExportFormat::Webp.name()), ("webp", "WebP · lossless"));
        assert_eq!(webp.recipe.filename("Sunset.capy"), "Sunset.webp");
        let json = serde_json::to_value(&webp.recipe).unwrap();
        assert_eq!(json["format"], "Webp");
        assert_eq!(serde_json::from_value::<ExportRecipe>(json).unwrap(), webp.recipe);
        let deep = ExportRecipe { depth: SampleDepth::U16, encoding: Default::default(), ..webp.recipe.clone() };
        assert_eq!(deep.validate().unwrap_err(), "Unsupported export combination");
        for (channels, format, formats) in [
            (ProfileChannels::Gray, ExportFormat::Png, vec![ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg]),
            (ProfileChannels::Cmyk, ExportFormat::Tiff, vec![ExportFormat::Tiff, ExportFormat::Jpeg]),
        ] {
            let profile = ExportProfile { profile: ColorProfile::Icc(vec![1].into()), channels, name: "Print".into() };
            let other = webp.recipe.clone().draft(ExportDraftAction::Profile(profile.clone()));
            assert_eq!((other.recipe.format, &other.formats), (format, &formats), "{channels:?} cannot be WebP");
            let refused = ExportRecipe { profile, ..webp.recipe.clone() }.draft(ExportDraftAction::Format(ExportFormat::Webp));
            assert_eq!(refused.recipe.format, format);
            for depth in [SampleDepth::U8, SampleDepth::F32] {
                let color = DocumentColor { space: RgbSpace::Srgb, depth };
                assert!(!refused.recipe.clone().draft_for_color(color, ExportDraftAction::Refresh).formats.contains(&ExportFormat::Webp));
            }
        }
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16] {
            let color = DocumentColor { space: RgbSpace::DisplayP3, depth };
            let choices = ExportRecipe::web_share().draft_for_color(color, ExportDraftAction::Format(ExportFormat::Webp));
            assert!(choices.formats.contains(&ExportFormat::Webp));
            assert_eq!(choices.recipe.format, ExportFormat::Webp);
        }
    }

    #[test]
    fn webp_output_is_refused_beyond_the_encoder_dimension_limit() {
        let webp = ExportRecipe::web_share().draft(ExportDraftAction::Format(ExportFormat::Webp)).recipe;
        assert_eq!(webp.output_extent([16384, 16384]).unwrap(), [16384, 16384]);
        for source in [[16385, 1], [1, 16385], [32768, 32768]] {
            let error = webp.output_extent(source).unwrap_err();
            assert!(error.starts_with("WebP export is limited to 16,384 pixels per side."), "{error}");
            assert_eq!(ExportRecipe { format: ExportFormat::Png, ..webp.clone() }.output_extent(source).unwrap(), source);
        }
        let fitted = ExportRecipe { size: ExportSize::Fit { bounds: [4096, 4096], enlarge: false }, ..webp.clone() };
        assert_eq!(fitted.output_extent([32768, 16384]).unwrap(), [4096, 2048]);
        let enlarged = ExportRecipe { size: ExportSize::Fit { bounds: [20000, 20000], enlarge: true }, ..webp.clone() };
        assert!(enlarged.output_extent([100, 50]).is_err());
        let mut document = layer_core::Document::new("Panorama", 16385, 2);
        assert!(webp.validate_for_document(&document).is_err());
        assert!(fitted.validate_for_document(&document).is_ok());
        document.resolution = Some(layer_core::ImageResolution::ppi(300));
        document.width = 16384;
        webp.validate_for_document(&document).unwrap();
    }

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
                depth: SampleDepth::U16,
            });
            recipe.size = size;
            recipe.validate().unwrap();
            assert_eq!(
                serde_json::from_slice::<ExportRecipe>(&serde_json::to_vec(&recipe).unwrap())
                    .unwrap(),
                recipe
            );
            assert_eq!(recipe.depth, SampleDepth::U16);
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
                SampleDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::Rgba,
                true,
            ),
            (
                ProfileChannels::Gray,
                ExportFormat::Png,
                SampleDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::GrayAlpha,
                true,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Png,
                SampleDepth::U8,
                ExportBackground::White,
                SourceChannels::Cmyk,
                false,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Tiff,
                SampleDepth::U16,
                ExportBackground::White,
                SourceChannels::Cmyk,
                true,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Tiff,
                SampleDepth::U16,
                ExportBackground::Preserve,
                SourceChannels::Cmyk,
                false,
            ),
            (
                ProfileChannels::Cmyk,
                ExportFormat::Jpeg,
                SampleDepth::U8,
                ExportBackground::Black,
                SourceChannels::Cmyk,
                true,
            ),
            (
                ProfileChannels::Gray,
                ExportFormat::Jpeg,
                SampleDepth::U8,
                ExportBackground::White,
                SourceChannels::Gray,
                true,
            ),
            (
                ProfileChannels::Rgb,
                ExportFormat::Jpeg,
                SampleDepth::U16,
                ExportBackground::White,
                SourceChannels::Rgb,
                false,
            ),
            (
                ProfileChannels::Rgb,
                ExportFormat::Jpeg,
                SampleDepth::U8,
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
    #[test]
    fn document_delivery_switches_between_hdr_and_sdr_without_relabelling_primaries() {
        for space in RgbSpace::ALL {
            let color=DocumentColor {space,depth:SampleDepth::F32};
            let exr=ExportRecipe::web_share().draft_for_color(color,ExportDraftAction::Format(ExportFormat::Exr));
            assert_eq!(exr.recipe.profile,ExportProfile::builtin(space));assert_eq!(exr.recipe.depth,SampleDepth::F32);
            let sdr=exr.recipe.draft_for_color(color,ExportDraftAction::Format(ExportFormat::Png));
            assert_eq!(sdr.recipe.depth,SampleDepth::U16);assert!(sdr.formats.contains(&ExportFormat::Exr));sdr.recipe.validate().unwrap();
            let integer=ExportRecipe::web_share().draft_for_color(DocumentColor{space,depth:SampleDepth::U8},ExportDraftAction::Refresh);
            assert!(integer.formats.iter().all(|f|!f.is_hdr()));
        }
    }
    #[test]
    fn exr_validates_for_any_document_primaries() {
        let mut document=layer_core::Document::new("P3",8,8);
        document.color=DocumentColor{space:RgbSpace::DisplayP3,depth:SampleDepth::F32};
        let recipe=ExportRecipe::web_share().draft(ExportDraftAction::Format(ExportFormat::Exr)).recipe;
        assert_eq!(recipe.profile,ExportProfile::builtin(RgbSpace::Srgb));
        recipe.validate_for_document(&document).unwrap();
    }
    #[test]
    fn validate_for_color_rejects_mismatched_profile_channels_and_sdr_documents_for_hdr() {
        let exr = ExportRecipe::web_share().draft(ExportDraftAction::Format(ExportFormat::Exr)).recipe;
        assert_eq!(exr.validate_for_color(DocumentColor::default()).unwrap_err(), "HDR delivery requires an HDR document");
        let mut gray = ExportRecipe::web_share();
        gray.profile.channels = ProfileChannels::Gray;
        assert_eq!(gray.validate_for_color(DocumentColor::default()).unwrap_err(), "Profile channels do not match the ICC data");
    }
}
