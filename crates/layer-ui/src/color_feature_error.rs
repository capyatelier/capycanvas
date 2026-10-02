use crate::{Localizer, MessageId};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorFeatureError {
    SelectImportedProfile, ProfileReadLimit, ProfileChanged, ProfileLibraryLimit, ProfileHiddenLimit,
    ExportDimensions, ExportResolution, ExportQuality, ExportCombination, HdrDocument, ProfileChannels, WebpLimit, DitherDepth,
    PresetProfileMissing, PresetSelectSaved, PresetChooseDestination, PresetLimit, PresetNameUsed, PresetEntryLimit, PresetProfileNameLimit, PresetBuiltinChannels, PresetEmptyProfile, PresetProfileBytesLimit, PresetDuplicateName, PresetFileLimit, PresetUnusedProfile, PresetNameInvalid,
    ProfileMissing,
    ProfileChooseFile,
    SourceChannels(layer_core::color::ProfileChannels),
    PresetChanged,
    HdrRangeBlocked,
    Diagnostic(String),
}
impl From<String> for ColorFeatureError { fn from(value: String) -> Self { Self::Diagnostic(value) } }
impl From<&str> for ColorFeatureError { fn from(value: &str) -> Self { Self::Diagnostic(value.into()) } }
impl ColorFeatureError {
    pub fn diagnostic(&self) -> String {
        match self { Self::Diagnostic(detail) => detail.clone(), _ => format!("{self:?}") }
    }
    fn operation_message(&self, localizer: &Localizer, context: MessageId) -> String {
        match self { Self::Diagnostic(detail) => format!("{}\n{}", localizer.text(context), detail), _ => self.message(localizer) }
    }
    pub fn profile_message(&self, localizer: &Localizer) -> String { self.operation_message(localizer, MessageId::COLOR_PROFILE_OPERATION_FAILED) }
    pub fn preset_message(&self, localizer: &Localizer) -> String { self.operation_message(localizer, MessageId::COLOR_PRESET_OPERATION_FAILED) }
    pub fn message(&self, localizer: &Localizer) -> String {
        let id = match self {
            Self::SelectImportedProfile => MessageId::COLOR_PROFILE_SELECT_IMPORTED,
            Self::ProfileReadLimit => MessageId::COLOR_PROFILE_READ_LIMIT,
            Self::ProfileChanged => MessageId::COLOR_PROFILE_CHANGED_STORAGE,
            Self::ProfileLibraryLimit => MessageId::COLOR_PROFILE_LIBRARY_LIMIT,
            Self::ProfileHiddenLimit => MessageId::COLOR_PROFILE_HIDDEN_LIMIT,
            Self::ExportDimensions => MessageId::COLOR_EXPORT_DIMENSIONS_RANGE,
            Self::ExportResolution => MessageId::COLOR_EXPORT_RESOLUTION_RANGE,
            Self::ExportQuality => MessageId::COLOR_EXPORT_QUALITY_RANGE,
            Self::ExportCombination => MessageId::COLOR_EXPORT_UNSUPPORTED_COMBINATION,
            Self::HdrDocument => MessageId::COLOR_EXPORT_HDR_DOCUMENT,
            Self::ProfileChannels => MessageId::COLOR_EXPORT_PROFILE_CHANNELS,
            Self::WebpLimit => MessageId::COLOR_EXPORT_WEBP_LIMIT,
            Self::DitherDepth => MessageId::COLOR_EXPORT_DITHER_DEPTH,
            Self::PresetProfileMissing => MessageId::COLOR_PRESET_PROFILE_MISSING,
            Self::PresetSelectSaved => MessageId::COLOR_PRESET_SELECT_SAVED,
            Self::PresetChooseDestination => MessageId::COLOR_PRESET_CHOOSE_DESTINATION,
            Self::PresetLimit => MessageId::COLOR_PRESET_LIMIT,
            Self::PresetNameUsed => MessageId::COLOR_PRESET_NAME_USED,
            Self::PresetEntryLimit => MessageId::COLOR_PRESET_ENTRY_LIMIT,
            Self::PresetProfileNameLimit => MessageId::COLOR_PRESET_PROFILE_NAME_LIMIT,
            Self::PresetBuiltinChannels => MessageId::COLOR_PRESET_BUILTIN_CHANNELS,
            Self::PresetEmptyProfile => MessageId::COLOR_PRESET_EMPTY_PROFILE,
            Self::PresetProfileBytesLimit => MessageId::COLOR_PRESET_PROFILE_BYTES_LIMIT,
            Self::PresetDuplicateName => MessageId::COLOR_PRESET_DUPLICATE_NAME,
            Self::PresetFileLimit => MessageId::COLOR_PRESET_FILE_LIMIT,
            Self::PresetUnusedProfile => MessageId::COLOR_PRESET_UNUSED_PROFILE,
            Self::PresetNameInvalid => MessageId::COLOR_PRESET_NAME_INVALID,
            Self::ProfileMissing => MessageId::COLOR_PROFILE_MISSING,
            Self::ProfileChooseFile => MessageId::COLOR_FEATURES_PROFILE_CHOOSE,
            Self::SourceChannels(channels) => {
                let channels = match channels { layer_core::color::ProfileChannels::Rgb => "RGB".into(), layer_core::color::ProfileChannels::Cmyk => "CMYK".into(), layer_core::color::ProfileChannels::Gray => localizer.text(MessageId::COLOR_FEATURES_PROFILE_GRAYSCALE) };
                let mut args = crate::FluentArgs::new(); args.set("channels", channels.as_ref());
                return localizer.format(MessageId::COLOR_PROFILE_SOURCE_CHANNELS, &args);
            }
            Self::PresetChanged => MessageId::COLOR_PRESET_CHANGED_STORAGE,
            Self::HdrRangeBlocked => MessageId::COLOR_FEATURES_EXPORT_OUTSIDE_RANGE,
            Self::Diagnostic(_) => return self.operation_message(localizer, MessageId::COLOR_EXPORT_OPERATION_FAILED),
        };
        localizer.text(id).to_string()
    }
}
