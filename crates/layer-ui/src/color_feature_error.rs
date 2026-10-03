use crate::{Localizer, MessageId};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorFeatureError {
    SelectImportedProfile, ProfileReadLimit, ProfileChanged, ProfileLibraryLimit, ProfileHiddenLimit,
    ExportDimensions, ExportResolution, ExportQuality, ExportCombination, HdrDocument, ProfileChannels, WebpLimit, DitherDepth,
    PresetProfileMissing, PresetSelectSaved, PresetChooseDestination, PresetLimit, PresetNameUsed, PresetEntryLimit, PresetProfileNameLimit, PresetBuiltinChannels, PresetEmptyProfile, PresetProfileBytesLimit, PresetDuplicateName, PresetFileLimit, PresetUnusedProfile, PresetNameInvalid,
    ProfileMissing,
    ProfileChooseFile,
    ProofChooseProfile, ProofDrawingChanged, ProofInactive, ProofSetupInactive, ProofDocumentBusy, ProofPreserveOriginal,
    ProofChoosePrintProfile, ProofCancelled, ProofWorkingSpaceChanged, ProofPreviewNotPrepared, ProofValidateFirst, ProofPrepareBeforeApply,
    ProofAlreadySdr, ProofHdrArtwork, ProofInvalidRendition, ProofInvalidValue,
    ProofRecipe(layer_core::color::ProofRecipeError), ProofAdmission(crate::session::DocumentIdleReason),
    SourceChannels(layer_core::color::ProfileChannels),
    PresetChanged,
    HdrRangeBlocked,
    Diagnostic(String),
}
impl From<layer_core::color::ProofRecipeError> for ColorFeatureError {
    fn from(reason: layer_core::color::ProofRecipeError) -> Self { Self::ProofRecipe(reason) }
}
impl From<crate::session::DocumentIdleReason> for ColorFeatureError {
    fn from(reason: crate::session::DocumentIdleReason) -> Self { Self::ProofAdmission(reason) }
}
impl From<layer_color::ProofLutError> for ColorFeatureError {
    fn from(reason:layer_color::ProofLutError) -> Self { match reason {layer_color::ProofLutError::Cancelled=>Self::ProofCancelled, layer_color::ProofLutError::Diagnostic(detail)=>Self::Diagnostic(detail)} }
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
    pub fn proof_message(&self, localizer: &Localizer) -> String {
        match self { Self::Diagnostic(detail) => detail.clone(), _ => self.message(localizer) }
    }
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
            Self::ProofChooseProfile => MessageId::COLOR_PROOF_CHOOSE_PROFILE,
            Self::ProofDrawingChanged => MessageId::COLOR_PROOF_DRAWING_CHANGED,
            Self::ProofInactive => MessageId::COLOR_PROOF_INACTIVE,
            Self::ProofSetupInactive => MessageId::COLOR_PROOF_SETUP_INACTIVE,
            Self::ProofDocumentBusy => MessageId::COLOR_PROOF_DOCUMENT_BUSY,
            Self::ProofPreserveOriginal => MessageId::COLOR_PROOF_PRESERVE_ORIGINAL,
            Self::ProofChoosePrintProfile => MessageId::COLOR_PROOF_CHOOSE_PRINT_PROFILE,
            Self::ProofCancelled => MessageId::COLOR_PROOF_CANCELLED,
            Self::ProofWorkingSpaceChanged => MessageId::COLOR_PROOF_WORKING_SPACE_CHANGED,
            Self::ProofPreviewNotPrepared => MessageId::COLOR_PROOF_PREVIEW_NOT_PREPARED,
            Self::ProofValidateFirst => MessageId::COLOR_PROOF_VALIDATE_FIRST,
            Self::ProofPrepareBeforeApply => MessageId::COLOR_PROOF_PREPARE_BEFORE_APPLY,
            Self::ProofAlreadySdr => MessageId::COLOR_PROOF_ALREADY_SDR,
            Self::ProofHdrArtwork => MessageId::COLOR_PROOF_HDR_ARTWORK,
            Self::ProofInvalidRendition => MessageId::COLOR_PROOF_INVALID_RENDITION,
            Self::ProofInvalidValue => MessageId::COLOR_PROOF_INVALID_VALUE,
            Self::ProofRecipe(reason) => match reason {
                layer_core::color::ProofRecipeError::NameLimit => MessageId::COLOR_PROOF_NAME_LIMIT,
                layer_core::color::ProofRecipeError::PaperRequiresBlackInk => MessageId::COLOR_PROOF_PAPER_REQUIRES_BLACK_INK,
                layer_core::color::ProofRecipeError::AbsoluteBlackPoint => MessageId::COLOR_PROOF_ABSOLUTE_BLACK_POINT,
            },
            Self::ProofAdmission(reason) => return reason.message(localizer).to_string(),

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
