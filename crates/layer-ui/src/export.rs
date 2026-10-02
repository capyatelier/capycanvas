//! Delivery choices describe a copy of the master. Hosts own dialogs and jobs.
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::color::{
    ColorProfile, DocumentColor, SampleDepth, OutputEncoding, ProfileChannels, RgbSpace,
};
use layer_color::photo::{DeliveryMetadata, ExportMetadata, MetadataKeep};
use serde::{Deserialize, Serialize};
use crate::color_feature_error::ColorFeatureError;

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
    pub fn localized_name(self, localizer: &crate::Localizer) -> std::sync::Arc<str> {
        use crate::MessageId as M;
        localizer.text(match self { Self::Exr => M::COLOR_FEATURES_EXPORT_FORMAT_EXR,
            Self::Png => M::COLOR_FEATURES_EXPORT_FORMAT_PNG, Self::Tiff => M::COLOR_FEATURES_EXPORT_FORMAT_TIFF,
            Self::Jpeg => M::COLOR_FEATURES_EXPORT_FORMAT_JPEG, Self::Webp => M::COLOR_FEATURES_EXPORT_FORMAT_WEBP,
            Self::PngHdr => M::COLOR_FEATURES_EXPORT_FORMAT_PQ, Self::PngHdrMapped => M::COLOR_FEATURES_EXPORT_FORMAT_PQ_CLIPPED,
            Self::JpegHdr | Self::JpegHdrMapped => M::COLOR_FEATURES_EXPORT_FORMAT_JPEG_HDR,
            Self::AvifHdr | Self::AvifHdrMapped => M::COLOR_FEATURES_EXPORT_FORMAT_AVIF_HDR })
    }
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
    pub fn extent(&self, source: [u32; 2]) -> Result<[u32; 2], ColorFeatureError> {
        let validate = |size: [u32; 2]| {
            if size.into_iter().all(|v| (1..=32768).contains(&v)) {
                Ok(())
            } else {
                Err(ColorFeatureError::ExportDimensions)
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
    #[serde(default)]
    pub metadata: ExportMetadata,
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
            metadata: self.metadata,
        }
    }
}
impl ExportRecipe {
    /// Validate the delivery transform from a document color on a file worker.
    pub fn validate_for_color(&self, color: DocumentColor) -> Result<(), ColorFeatureError> {
        self.validate()?;
        if self.format.is_hdr() && !color.depth.is_float() { return Err(ColorFeatureError::HdrDocument); }
        if layer_color::profile_channels(&self.profile.profile)? != self.profile.channels {
            return Err(ColorFeatureError::ProfileChannels);
        }
        layer_color::WorkingEncoder::new(color.space, &self.interpretation(), self.encoding)?;
        Ok(())
    }
    /// Validate the complete delivery transform and size on a file worker.
    pub fn validate_for_document(&self, document: &layer_core::Document) -> Result<(), ColorFeatureError> {
        self.validate_for_color(document.color)?;
        self.output_extent([document.width, document.height])?;
        self.output_resolution(document.resolution)?;
        Ok(())
    }
    /// Delivery pixel dimensions, refused when the format's encoder cannot
    /// write them.
    pub fn output_extent(&self, source: [u32; 2]) -> Result<[u32; 2], ColorFeatureError> {
        let extent = self.size.extent(source)?;
        if self.format == ExportFormat::Webp && extent.iter().any(|v| *v > layer_color::photo::WEBP_MAX_DIMENSION) {
            return Err(ColorFeatureError::WebpLimit);
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
            metadata: ExportMetadata::default(),
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
            metadata: ExportMetadata::default(),
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
            metadata: self.metadata,
            ..base
        }
        .normalized(ExportDraftAction::Format(format))
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
    pub fn validate(&self) -> Result<(), ColorFeatureError> {
        if let ExportResolution::Ppi(value) = self.resolution
            && !(1..=65535).contains(&value)
        {
            return Err(ColorFeatureError::ExportResolution);
        }
        self.size.extent([1, 1])?;
        self.encoding.validate(self.depth).map_err(|_| ColorFeatureError::DitherDepth)?;
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(ColorFeatureError::ExportQuality);
        }
        if self.clone().normalized(ExportDraftAction::Refresh) != *self {
            return Err(ColorFeatureError::ExportCombination);
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
    ) -> Result<Option<layer_core::ImageResolution>, ColorFeatureError> {
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
    /// The resolution and photo metadata this recipe writes for `document`.
    pub fn delivery_metadata(&self, document: &layer_core::Document) -> Result<DeliveryMetadata, ColorFeatureError> {
        Ok(DeliveryMetadata {
            resolution: self.output_resolution(document.resolution)?,
            photo: document.metadata.clone(),
            policy: self.metadata,
        })
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
    Metadata(ExportMetadata),
}
#[derive(Serialize)]
pub struct MetadataChoice {
    pub value: MetadataKeep,
    pub label: std::sync::Arc<str>,
}
/// The export dialog's Metadata row.
#[derive(Serialize)]
pub struct ExportMetadataView {
    pub label: std::sync::Arc<str>,
    pub choices: Vec<MetadataChoice>,
    pub remove_location: std::sync::Arc<str>,
    /// Remove location applies only when everything else is kept.
    pub location: bool,
    /// Whether the format carries photo metadata at all.
    pub available: bool,
    pub note: Option<std::sync::Arc<str>>,
}
impl ExportMetadataView {
    pub fn new_localized(recipe: &ExportRecipe, localizer: &crate::Localizer) -> Self {
        use crate::MessageId as M;
        let available = recipe.format != ExportFormat::Exr;
        Self {
            label: localizer.text(M::COLOR_FEATURES_EXPORT_METADATA),
            choices: MetadataKeep::ALL.into_iter().map(|value| MetadataChoice { value,
                label: localizer.text(match value { MetadataKeep::All => M::COLOR_FEATURES_EXPORT_METADATA_ALL,
                    MetadataKeep::CopyrightContact => M::COLOR_FEATURES_EXPORT_METADATA_COPYRIGHT,
                    MetadataKeep::None => M::COLOR_FEATURES_EXPORT_METADATA_NONE }) }).collect(),
            remove_location: localizer.text(M::COLOR_FEATURES_EXPORT_REMOVE_LOCATION),
            location: available && recipe.metadata.keep == MetadataKeep::All, available,
            note: (!available).then(|| localizer.text(M::COLOR_FEATURES_EXPORT_METADATA_EXR)),
        }
    }
}
#[derive(Serialize)]
pub struct ExportChoice<T> { pub value: T, pub label: std::sync::Arc<str> }
#[derive(Serialize)]
pub struct ExportChoices {
    pub formats: Vec<ExportChoice<ExportFormat>>,
    pub depths: Vec<ExportChoice<SampleDepth>>,
    pub backgrounds: Vec<ExportChoice<ExportBackground>>,
    pub dithers: Vec<ExportChoice<layer_core::color::OutputDither>>,
}
#[derive(Serialize)]
pub struct ExportDraft {
    pub choices: ExportChoices,
    pub hdr: bool,
    pub clip_hdr_range: bool,
    pub format: ExportFormat,
    pub recipe: ExportRecipe,
    pub formats: Vec<ExportFormat>,
    pub depths: Vec<SampleDepth>,
    pub backgrounds: Vec<ExportBackground>,
    pub dithers: Vec<layer_core::color::OutputDither>,
    pub metadata: ExportMetadataView,
}
impl ExportRecipe {
    pub fn draft_localized(self, action: ExportDraftAction, localizer: &crate::Localizer) -> ExportDraft {
        self.draft_impl(action, localizer)
    }
    pub fn draft_for_color_localized(self, color: DocumentColor, action: ExportDraftAction, localizer: &crate::Localizer) -> ExportDraft {
        let mut draft = self.draft_impl(action, localizer);
        if draft.recipe.format == ExportFormat::Exr {
            draft.recipe.profile = ExportProfile::builtin(color.space);
        }
        draft.formats = ExportFormat::sdr_choices(draft.recipe.profile.channels);
        if color.depth.is_float() {
            draft.formats.extend([ExportFormat::PngHdr, ExportFormat::PngHdrMapped, ExportFormat::Exr,
                ExportFormat::JpegHdr, ExportFormat::JpegHdrMapped, ExportFormat::AvifHdr, ExportFormat::AvifHdrMapped]);
        }
        draft.choices.formats = draft.formats.iter().map(|value| ExportChoice { value: *value, label: value.localized_name(localizer) }).collect();
        draft
    }
    pub fn draft_for_color_canonical(self, color: DocumentColor, action: ExportDraftAction) -> ExportDraft { self.draft_for_color_localized(color, action, &crate::Localizer::shared(crate::UiLanguage::English)) }
    pub fn draft_canonical(self, action: ExportDraftAction) -> ExportDraft { self.draft_impl(action, &crate::Localizer::shared(crate::UiLanguage::English)) }
    fn normalized(mut self, action: ExportDraftAction) -> Self {
        use layer_core::color::OutputDither;
        match action {
            ExportDraftAction::Refresh => (),
            ExportDraftAction::ClipHdrRange(v) => self.format=self.format.with_hdr_range_mapping(v),
            ExportDraftAction::Format(v) => self.format = v,
            ExportDraftAction::Profile(v) => self.profile = v,
            ExportDraftAction::Depth(v) => self.depth = v,
            ExportDraftAction::Background(v) => self.background = v,
            ExportDraftAction::Encoding(v) => self.encoding = v,
            ExportDraftAction::Metadata(v) => self.metadata = v,
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
        self
    }
    fn draft_impl(mut self, action: ExportDraftAction, localizer: &crate::Localizer) -> ExportDraft {
        self = self.normalized(action);
        use layer_core::color::OutputDither;
        let choices = ExportFormat::sdr_choices(self.profile.channels);
        let cmyk = self.profile.channels == ProfileChannels::Cmyk;
        let jpeg = self.format == ExportFormat::Jpeg;
        let eight_bit = jpeg || self.format == ExportFormat::Webp;
        let formats: Vec<_> = if self.format.is_hdr() { vec![ExportFormat::JpegHdr, ExportFormat::AvifHdr, ExportFormat::PngHdr, ExportFormat::Exr] } else { choices };
        let depths: Vec<_> = if self.format == ExportFormat::Exr { vec![SampleDepth::F32] } else if self.format.is_hdr() { vec![SampleDepth::U16] } else if eight_bit { vec![SampleDepth::U8] } else { vec![SampleDepth::U8, SampleDepth::U16] };
        let backgrounds: Vec<_> = if self.format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg) { vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black] } else if self.format.is_hdr() { vec![ExportBackground::Preserve] } else if jpeg || cmyk { vec![ExportBackground::White, ExportBackground::Black] } else { vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black] };
        let dithers: Vec<_> = if self.depth == SampleDepth::U8 { vec![OutputDither::None, OutputDither::Stochastic8] } else { vec![OutputDither::None] };
        let choices = ExportChoices {
            formats: formats.iter().map(|value| ExportChoice {value:*value, label:value.localized_name(localizer)}).collect(),
            depths: depths.iter().map(|value| ExportChoice {value:*value, label:localizer.text(match value {SampleDepth::U8=>crate::MessageId::COLOR_FEATURES_EXPORT_DEPTH_8, SampleDepth::U16=>crate::MessageId::COLOR_FEATURES_EXPORT_DEPTH_16, SampleDepth::F16=>crate::MessageId::COLOR_FEATURES_COLOR_DEPTH_FLOAT16, SampleDepth::F32=>crate::MessageId::COLOR_FEATURES_EXPORT_DEPTH_FLOAT32})}).collect(),
            backgrounds: backgrounds.iter().map(|value| ExportChoice {value:*value, label:localizer.text(match value {ExportBackground::Preserve=>crate::MessageId::COLOR_FEATURES_EXPORT_KEEP_TRANSPARENCY, ExportBackground::White=>crate::MessageId::COLOR_FEATURES_EXPORT_WHITE_BACKGROUND, ExportBackground::Black=>crate::MessageId::COLOR_FEATURES_EXPORT_BLACK_BACKGROUND})}).collect(),
            dithers: dithers.iter().map(|value| ExportChoice {value:*value, label:localizer.text(match value {OutputDither::None=>crate::MessageId::COLOR_FEATURES_EXPORT_DITHER_NONE, OutputDither::Stochastic8=>crate::MessageId::COLOR_FEATURES_EXPORT_DITHER_STOCHASTIC})}).collect(),
        };
        ExportDraft {
            choices,
            hdr:self.format.is_hdr(),clip_hdr_range:self.format.maps_hdr_range(),format:self.format.with_hdr_range_mapping(false),
            formats,
            depths,
            backgrounds,
            dithers,
            metadata: ExportMetadataView::new_localized(&self, localizer),
            recipe: self,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ExportNumericControls {
    pub dimension: crate::NumericControl,
    pub ppi: crate::NumericControl,
    pub quality: crate::NumericControl,
}
impl Default for ExportNumericControls {
    fn default() -> Self {
        Self {
            dimension: crate::NumericControl::number(1., 32768., 1., 0),
            ppi: crate::NumericControl::number(1., 65535., 1., 0),
            quality: crate::NumericControl::number(1., 100., 1., 0),
        }
    }
}

#[derive(Serialize)]
pub struct ExportForm {
    pub numeric: ExportNumericControls,
    pub copy: crate::color_feature_copy::ExportCopy,
    pub profiles: Vec<ExportProfile>,
    pub extent: [u32; 2],
    /// The document keeps metadata from an opened photo, so the Metadata row applies.
    pub metadata: bool,
}
impl ExportForm {
    pub fn new_canonical(document: &layer_core::Document) -> Self {
        Self::new_localized(document, &crate::Localizer::shared(crate::UiLanguage::English))
    }
    pub fn new_localized(document: &layer_core::Document, localizer: &crate::Localizer) -> Self {
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
                    name: crate::color_feature_copy::named(localizer, crate::MessageId::COLOR_FEATURES_EXPORT_ORIGINAL_PROFILE, &layer.name),
                });
            }
        }
        Self {
            numeric: ExportNumericControls::default(),
            copy: crate::color_feature_copy::ExportCopy::new(localizer),
            profiles,
            extent: [document.width, document.height],
            metadata: !document.metadata.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_choices_describe_only_the_fixed_delivery_contract() {
        for format in [ExportFormat::PngHdr, ExportFormat::PngHdrMapped, ExportFormat::JpegHdr, ExportFormat::JpegHdrMapped, ExportFormat::AvifHdr, ExportFormat::AvifHdrMapped] {
            let draft = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(format));
            assert_eq!(draft.formats, [ExportFormat::JpegHdr, ExportFormat::AvifHdr, ExportFormat::PngHdr, ExportFormat::Exr]);
            assert_eq!(draft.depths, [SampleDepth::U16]);
            if format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg){assert_eq!(draft.backgrounds,[ExportBackground::Preserve,ExportBackground::White,ExportBackground::Black]);}else{assert_eq!(draft.backgrounds, [ExportBackground::Preserve]);}
            assert_eq!(draft.dithers, [layer_core::color::OutputDither::None]);
            assert_eq!(draft.recipe.format, format);
            draft.recipe.validate().unwrap();
            for depth in [SampleDepth::F16, SampleDepth::F32] {
                let color = DocumentColor { space: RgbSpace::Srgb, depth };
                let choices = draft.recipe.clone().draft_for_color_canonical(color, ExportDraftAction::Refresh);
                assert!(choices.formats.contains(&format), "Authored range policy must remain selectable");
            }
            let integer = ExportRecipe::web_share().draft_for_color_canonical(DocumentColor::default(), ExportDraftAction::Refresh);
            assert!(!integer.formats.contains(&format));
        }
        let sdr = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Refresh);
        assert_eq!(sdr.formats, [ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg, ExportFormat::Webp]);
        assert_eq!(sdr.depths, [SampleDepth::U8, SampleDepth::U16]);
    }

    #[test]
    fn draft_transitions_keep_supported_depth_alpha_and_dither_choices() {
        let mut recipe = ExportRecipe::further_editing(DocumentColor::default());
        recipe.encoding.dither = layer_core::color::OutputDither::Stochastic8;
        let jpeg = recipe.draft_canonical(ExportDraftAction::Format(ExportFormat::Jpeg));
        assert_eq!(jpeg.depths, [SampleDepth::U8]);
        assert_eq!(jpeg.recipe.depth, SampleDepth::U8);
        assert_eq!(jpeg.recipe.background, ExportBackground::White);
        jpeg.recipe.validate().unwrap();
        let png = jpeg.recipe.draft_canonical(ExportDraftAction::Format(ExportFormat::Png));
        let deep = png.recipe.draft_canonical(ExportDraftAction::Depth(SampleDepth::U16));
        assert_eq!(deep.recipe.encoding.dither, layer_core::color::OutputDither::None);
        deep.recipe.validate().unwrap();
        let cmyk = deep.recipe.draft_canonical(ExportDraftAction::Profile(ExportProfile { profile: ColorProfile::Icc(vec![1].into()), channels: ProfileChannels::Cmyk, name: "CMYK".into() }));
        assert_eq!(cmyk.formats, [ExportFormat::Tiff, ExportFormat::Jpeg]);
        assert_eq!(cmyk.recipe.format, ExportFormat::Tiff);
        assert!(!cmyk.backgrounds.contains(&ExportBackground::Preserve));
        cmyk.recipe.validate().unwrap();
        let mut invalid = cmyk.recipe;invalid.jpeg_quality = 0;
        assert!(invalid.draft_canonical(ExportDraftAction::Refresh).recipe.validate().is_err());
    }

    #[test]
    fn webp_drafts_as_lossless_eight_bit_rgb_with_transparency() {
        let mut recipe = ExportRecipe::further_editing(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
        recipe.encoding.dither = layer_core::color::OutputDither::Stochastic8;
        let webp = recipe.draft_canonical(ExportDraftAction::Format(ExportFormat::Webp));
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
        assert_eq!(deep.validate().unwrap_err(), ColorFeatureError::ExportCombination);
        for (channels, format, formats) in [
            (ProfileChannels::Gray, ExportFormat::Png, vec![ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg]),
            (ProfileChannels::Cmyk, ExportFormat::Tiff, vec![ExportFormat::Tiff, ExportFormat::Jpeg]),
        ] {
            let profile = ExportProfile { profile: ColorProfile::Icc(vec![1].into()), channels, name: "Print".into() };
            let other = webp.recipe.clone().draft_canonical(ExportDraftAction::Profile(profile.clone()));
            assert_eq!((other.recipe.format, &other.formats), (format, &formats), "{channels:?} cannot be WebP");
            let refused = ExportRecipe { profile, ..webp.recipe.clone() }.draft_canonical(ExportDraftAction::Format(ExportFormat::Webp));
            assert_eq!(refused.recipe.format, format);
            for depth in [SampleDepth::U8, SampleDepth::F32] {
                let color = DocumentColor { space: RgbSpace::Srgb, depth };
                assert!(!refused.recipe.clone().draft_for_color_canonical(color, ExportDraftAction::Refresh).formats.contains(&ExportFormat::Webp));
            }
        }
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16] {
            let color = DocumentColor { space: RgbSpace::DisplayP3, depth };
            let choices = ExportRecipe::web_share().draft_for_color_canonical(color, ExportDraftAction::Format(ExportFormat::Webp));
            assert!(choices.formats.contains(&ExportFormat::Webp));
            assert_eq!(choices.recipe.format, ExportFormat::Webp);
        }
    }

    #[test]
    fn webp_output_is_refused_beyond_the_encoder_dimension_limit() {
        let webp = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::Webp)).recipe;
        assert_eq!(webp.output_extent([16384, 16384]).unwrap(), [16384, 16384]);
        for source in [[16385, 1], [1, 16385], [32768, 32768]] {
            let error = webp.output_extent(source).unwrap_err();
            assert_eq!(error, ColorFeatureError::WebpLimit);
            assert_eq!(ExportRecipe { format: ExportFormat::Png, ..webp.clone() }.output_extent(source).unwrap(), source);
        }
        let fitted = ExportRecipe { size: ExportSize::Fit { bounds: [4096, 4096], enlarge: false }, ..webp.clone() };
        assert_eq!(fitted.output_extent([32768, 16384]).unwrap(), [4096, 2048]);
        let enlarged = ExportRecipe { size: ExportSize::Fit { bounds: [20000, 20000], enlarge: true }, ..webp.clone() };
        assert!(enlarged.output_extent([100, 50]).is_err());
        let mut document = layer_core::Document::new("Panorama", 16385, 2, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
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
            let exr=ExportRecipe::web_share().draft_for_color_canonical(color,ExportDraftAction::Format(ExportFormat::Exr));
            assert_eq!(exr.recipe.profile,ExportProfile::builtin(space));assert_eq!(exr.recipe.depth,SampleDepth::F32);
            let sdr=exr.recipe.draft_for_color_canonical(color,ExportDraftAction::Format(ExportFormat::Png));
            assert_eq!(sdr.recipe.depth,SampleDepth::U16);assert!(sdr.formats.contains(&ExportFormat::Exr));sdr.recipe.validate().unwrap();
            let integer=ExportRecipe::web_share().draft_for_color_canonical(DocumentColor{space,depth:SampleDepth::U8},ExportDraftAction::Refresh);
            assert!(integer.formats.iter().all(|f|!f.is_hdr()));
        }
    }
    #[test]
    fn metadata_defaults_to_all_without_location_and_drafts_through_the_shared_view() {
        let recipe = ExportRecipe::web_share();
        assert_eq!(recipe.metadata, ExportMetadata { keep: MetadataKeep::All, remove_location: true });
        let mut json = serde_json::to_value(&recipe).unwrap();
        assert_eq!(json["metadata"], serde_json::json!({"keep": "All", "remove_location": true}));
        json.as_object_mut().unwrap().remove("metadata");
        assert_eq!(serde_json::from_value::<ExportRecipe>(json.clone()).unwrap(), recipe, "recipes saved before metadata read as the default");
        json["metadata"] = serde_json::json!({"keep": "CopyrightContact"});
        assert_eq!(serde_json::from_value::<ExportRecipe>(json).unwrap().metadata, ExportMetadata { keep: MetadataKeep::CopyrightContact, remove_location: true });

        let draft = recipe.clone().draft_canonical(ExportDraftAction::Refresh);
        let view = serde_json::to_value(&draft.metadata).unwrap();
        assert_eq!(view["label"], "Metadata");
        assert_eq!(view["choices"], serde_json::json!([
            {"value": "All", "label": "All"},
            {"value": "CopyrightContact", "label": "Copyright & Contact"},
            {"value": "None", "label": "None"},
        ]));
        assert_eq!((view["remove_location"].as_str(), view["location"].as_bool(), view["available"].as_bool()), (Some("Remove location"), Some(true), Some(true)));
        assert!(view["note"].is_null());
        let action: ExportDraftAction = serde_json::from_value(serde_json::json!({"type": "metadata", "value": {"keep": "CopyrightContact", "remove_location": false}})).unwrap();
        let rights = recipe.clone().draft_canonical(action);
        assert_eq!(rights.recipe.metadata, ExportMetadata { keep: MetadataKeep::CopyrightContact, remove_location: false });
        assert!(!rights.metadata.location, "Remove location applies only when everything is kept");
        rights.recipe.validate().unwrap();
        for format in [ExportFormat::Tiff, ExportFormat::Jpeg, ExportFormat::Webp] {
            let draft = rights.recipe.clone().draft_canonical(ExportDraftAction::Format(format));
            assert_eq!(draft.recipe.metadata, rights.recipe.metadata, "{format:?} keeps the choice");
            assert!(draft.metadata.available);
        }
        let exr = rights.recipe.clone().draft_canonical(ExportDraftAction::Format(ExportFormat::Exr));
        assert!(!exr.metadata.available && !exr.metadata.location);
        assert!(exr.metadata.note.unwrap().starts_with("OpenEXR keeps no camera or copyright details."));
        let back = exr.recipe.draft_for_color_canonical(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 }, ExportDraftAction::Format(ExportFormat::Png));
        assert_eq!(back.recipe.metadata, rights.recipe.metadata);
        let sdr = ExportRecipe { metadata: rights.recipe.metadata, ..ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::JpegHdr)).recipe }
            .for_color(DocumentColor::default());
        assert_eq!(sdr.metadata, rights.recipe.metadata, "an SDR fallback keeps the choice");

        let mut document = layer_core::Document::new("Photo", 40, 30, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        assert!(!ExportForm::new_canonical(&document).metadata, "a new drawing has no photo metadata");
        document.metadata.exif = Some(vec![1, 2, 3].into());
        document.resolution = Some(layer_core::ImageResolution::ppi(300));
        assert!(ExportForm::new_canonical(&document).metadata);
        let delivery = ExportRecipe { resolution: ExportResolution::Ppi(72), ..rights.recipe }.delivery_metadata(&document).unwrap();
        assert_eq!(delivery.resolution, Some(layer_core::ImageResolution::ppi(72)));
        assert_eq!(delivery.photo, document.metadata);
        assert_eq!(delivery.policy, rights.recipe.metadata);
    }
    #[test]
    fn exr_validates_for_any_document_primaries() {
        let mut document=layer_core::Document::new("P3",8,8, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.color=DocumentColor{space:RgbSpace::DisplayP3,depth:SampleDepth::F32};
        let recipe=ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::Exr)).recipe;
        assert_eq!(recipe.profile,ExportProfile::builtin(RgbSpace::Srgb));
        recipe.validate_for_document(&document).unwrap();
    }
    #[test]
    fn validate_for_color_rejects_mismatched_profile_channels_and_sdr_documents_for_hdr() {
        let exr = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::Exr)).recipe;
        assert_eq!(exr.validate_for_color(DocumentColor::default()).unwrap_err(), ColorFeatureError::HdrDocument);
        let mut gray = ExportRecipe::web_share();
        gray.profile.channels = ProfileChannels::Gray;
        assert_eq!(gray.validate_for_color(DocumentColor::default()).unwrap_err(), ColorFeatureError::ProfileChannels);
    }
}
