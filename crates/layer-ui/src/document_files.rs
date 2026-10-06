//! Document lifecycle policy. Hosts provide dialogs and asynchronous transport;
//! checkpoints, cancellation and close-after-save decisions remain shared.
use super::*;
use layer_core::{Document, authored::{ArtworkCapture, CaptureCheckpoint, OccurrenceContent, PortableId}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileFailure { InvalidLocation, EditedLocation, Busy, UnknownRequest, SaveNotCaptured, NotExportRequest, InvalidSaveRequest, RespondClose, NoSavedSnapshot, NotCloseRequest, SelectionCapture, CanvasOperation }
impl FileFailure {
    fn message(self, localization: &Localizer) -> String {
        localization.text(match self {
            Self::InvalidLocation => MessageId::DOCUMENTS_ERROR_INVALID_LOCATION,
            Self::EditedLocation => MessageId::DOCUMENTS_ERROR_EDITED_LOCATION,
            Self::Busy => MessageId::DOCUMENTS_ERROR_BUSY,
            Self::UnknownRequest => MessageId::DOCUMENTS_ERROR_UNKNOWN_REQUEST,
            Self::SaveNotCaptured => MessageId::DOCUMENTS_ERROR_SAVE_NOT_CAPTURED,
            Self::NotExportRequest => MessageId::DOCUMENTS_ERROR_NOT_EXPORT_REQUEST,
            Self::InvalidSaveRequest => MessageId::DOCUMENTS_ERROR_INVALID_SAVE_REQUEST,
            Self::RespondClose => MessageId::DOCUMENTS_ERROR_RESPOND_CLOSE,
            Self::NoSavedSnapshot => MessageId::DOCUMENTS_ERROR_NO_SAVED_SNAPSHOT,
            Self::NotCloseRequest => MessageId::DOCUMENTS_ERROR_NOT_CLOSE_REQUEST,
            Self::SelectionCapture => MessageId::DOCUMENTS_ERROR_SELECTION_CAPTURE,
            Self::CanvasOperation => MessageId::DOCUMENTS_ERROR_CANVAS_OPERATION,
        }).to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentIdleReason { CanvasInteraction, WaitTransform, ApplyCrop, ApplyTransform, CanvasOperation }
impl DocumentIdleReason {
    pub fn message(self, localizer: &Localizer) -> std::sync::Arc<str> {
        localizer.text(match self {
            Self::CanvasInteraction => MessageId::COLOR_PROOF_FINISH_CANVAS_INTERACTION,
            Self::WaitTransform => MessageId::COMMANDS_WAIT_FOR_TRANSFORM,
            Self::ApplyCrop => MessageId::COMMANDS_APPLY_OR_CANCEL_THE_CROP_FIRST,
            Self::ApplyTransform => MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST,
            Self::CanvasOperation => MessageId::DOCUMENTS_ERROR_CANVAS_OPERATION,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRequestFailure { ActionFailed, KeymapTooLarge }
impl HostRequestFailure {
    pub fn message(self, localization: &Localizer) -> String {
        localization.text(match self {
            Self::ActionFailed => MessageId::COMMON_ACTION_FAILED,
            Self::KeymapTooLarge => MessageId::DOCUMENTS_DELIVERY_KEYMAP_TOO_LARGE,
        }).to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "reason", rename_all = "snake_case")]
pub enum DocumentHostErrorCopy { Document(DocumentHostError), Delivery(crate::DocumentDeliveryMessage), Transport(crate::DocumentTransportRefusal), HostRequest(HostRequestFailure), Color(crate::ColorFeatureError), Proof(crate::ColorFeatureError), Profile(crate::ColorFeatureError), Preset(crate::ColorFeatureError), ExportPreferences(crate::ColorFeatureError) }
impl DocumentHostErrorCopy {
    pub fn message(&self, localization: &Localizer) -> String {
        match self {
            Self::Document(reason) => reason.message(localization),
            Self::Proof(reason) => reason.proof_message(localization),
            Self::Delivery(reason) => reason.message(localization),
            Self::Transport(reason) => reason.message(localization).to_string(),
            Self::HostRequest(reason) => reason.message(localization),
            Self::ExportPreferences(reason) => crate::DocumentDeliveryMessage::ExportPreferences { detail: reason.preset_message(localization) }.message(localization),
            Self::Color(crate::ColorFeatureError::Diagnostic(detail)) | Self::Profile(crate::ColorFeatureError::Diagnostic(detail)) | Self::Preset(crate::ColorFeatureError::Diagnostic(detail)) => detail.clone(),
            Self::Color(reason) => reason.message(localization),
            Self::Profile(reason) => reason.profile_message(localization),
            Self::Preset(reason) => reason.preset_message(localization),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentHostError {
    NewWindowUnavailable, DrawingTabsUnavailable, ChooseDeviceFile, ChooseFilename, ProjectWriterFailed, NoDrawingToOpen, LookupImportFailed,
}
impl DocumentHostError {
    pub fn message(self, localization: &Localizer) -> String {
        localization.text(match self {
            Self::NewWindowUnavailable => MessageId::DOCUMENTS_ERROR_NEW_WINDOW_UNAVAILABLE,
            Self::DrawingTabsUnavailable => MessageId::DOCUMENTS_ERROR_TABS_UNAVAILABLE,
            Self::ChooseDeviceFile => MessageId::DOCUMENTS_ERROR_DEVICE_FILE,
            Self::ChooseFilename => MessageId::DOCUMENTS_ERROR_FILENAME,
            Self::ProjectWriterFailed => MessageId::DOCUMENTS_ERROR_PROJECT_WRITER,
            Self::NoDrawingToOpen => MessageId::DOCUMENTS_ERROR_NO_DRAWING,
            Self::LookupImportFailed => MessageId::RESOURCES_LOOKUP_FAILED,
        }).to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentLocation {
    /// Opaque host handle/URI. Never serialized into the project itself.
    pub uri: String,
    pub name: String,
}
impl DocumentLocation {
    pub(super) fn validate(&self) -> Result<(), FileFailure> {
        if self.uri.is_empty()
            || self.uri.len() > 16_384
            || self.uri.contains('\0')
            || self.name.is_empty()
            || self.name.len() > 1024
            || self.name.chars().any(char::is_control)
        {
            Err(FileFailure::InvalidLocation)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DocumentFileState {
    pub epoch: u64,
    pub revision: u64,
    pub location: Option<DocumentLocation>,
    pub export_uri: Option<String>,
    /// Suggested master name for an opened photo; never a save destination.
    pub unsaved_name: Option<String>,
    pub modified: bool,
    pub recovered: bool,
    pub busy: bool,
    /// The host closes only after this authorization, not on an initial request.
    pub close_ready: bool,
    #[serde(skip)]
    untitled: std::sync::Arc<str>,
}
impl Default for DocumentFileState {
    fn default() -> Self { Self::localized(&Localizer::shared(UiLanguage::English)) }
}
impl DocumentFileState {
    pub fn localized(localization: &Localizer) -> Self {
        Self { epoch: 0, revision: 0, location: None, export_uri: None, unsaved_name: None, modified: false, recovered: false,
            busy: false, close_ready: false, untitled: localization.text(MessageId::DOCUMENTS_UNTITLED) }
    }
    pub fn set_localization(&mut self, localization: &Localizer) {
        self.untitled = localization.text(MessageId::DOCUMENTS_UNTITLED);
    }

    pub fn title(&self) -> &str {
        self.location.as_ref().map_or_else(
            || self.unsaved_name.as_deref().unwrap_or(&self.untitled),
            |location| location.name.as_str(),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentColorOperation { Assign, Convert, Depth }

#[derive(Clone, Debug, Serialize)]
pub struct LookupTarget {pub document: PortableId, pub activation:u64, pub layer:u64, pub epoch:u64, pub key:String}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRepeat<P = ExportProfile> {
    pub recipe: ExportRecipe<P>,
    pub location: DocumentLocation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentRequest {
    ImportLookup {target: LookupTarget},
    ChangeColor { operation: DocumentColorOperation },
    ColorHistory { redo: bool },
    Place,
    Paste { mode: PasteMode },
    /// Copy the active layer's pixels or selected images, with `merged` the
    /// visible image, with `pixels` an image layer's pixels; `cut` erases them
    /// once the host reports the copy complete.
    Copy { merged: bool, cut: bool, pixels: bool },
    Properties,
    RepairSourceProfile { layer: u64 },
    RasterizeSource { layer: u64 },
    New,
    Open,
    Save {
        location: Option<DocumentLocation>,
        name: String,
    },
    Export {
        name: String,
        owner: u64,
        epoch: u64,
        repeat: Option<ExportRepeat>,
    },
    ConfirmClose {
        title: String,
    },
}
impl DocumentRequest {
    pub fn title(&self, localization: &Localizer) -> std::sync::Arc<str> {
        let id = match self {
            Self::ImportLookup {..} => MessageId::RESOURCES_LOOKUP_IMPORT,
            Self::ChangeColor { operation: DocumentColorOperation::Assign } => MessageId::DOCUMENTS_ASSIGN_PROFILE,
            Self::ChangeColor { operation: DocumentColorOperation::Convert } => MessageId::DOCUMENTS_CONVERT_COLOR,
            Self::ChangeColor { operation: DocumentColorOperation::Depth } => MessageId::DOCUMENTS_CHANGE_DEPTH,
            Self::ColorHistory { redo: false } => MessageId::DOCUMENTS_UNDO_COLOR,
            Self::ColorHistory { redo: true } => MessageId::DOCUMENTS_REDO_COLOR,
            Self::Place => MessageId::DOCUMENTS_PLACE,
            Self::Paste { mode: PasteMode::Paste } => MessageId::COMMAND_PASTE_IMAGE,
            Self::Paste { mode: PasteMode::InPlace } => MessageId::COMMAND_PASTE_IN_PLACE,
            Self::Paste { mode: PasteMode::Into } => MessageId::COMMAND_PASTE_INTO,
            Self::Copy { cut: true, .. } => MessageId::COMMAND_CUT,
            Self::Copy { merged: true, .. } => MessageId::COMMAND_COPY_MERGED,
            Self::Copy { pixels: true, .. } => MessageId::COMMAND_COPY_PIXELS,
            Self::Copy { .. } => MessageId::COMMAND_COPY,
            Self::Properties => MessageId::DOCUMENTS_PROPERTIES,
            Self::RepairSourceProfile { .. } => MessageId::DOCUMENTS_REPAIR_SOURCE,
            Self::RasterizeSource { .. } => MessageId::DOCUMENTS_RASTERIZE_SOURCE,
            Self::New => MessageId::DOCUMENTS_NEW,
            Self::Open => MessageId::DOCUMENTS_OPEN,
            Self::Save { .. } => MessageId::DOCUMENTS_SAVE,
            Self::Export { .. } => MessageId::DOCUMENTS_EXPORT,
            Self::ConfirmClose { title } => return title.as_str().into(),
        };
        localization.text(id)
    }
    pub fn accept_label(&self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(match self {
            Self::ChangeColor { .. } | Self::ColorHistory { .. } | Self::RepairSourceProfile { .. } => MessageId::COMMON_APPLY,
            Self::Place | Self::ImportLookup {..} => MessageId::DOCUMENTS_IMPORT_ACCEPT,
            Self::Paste { .. } => MessageId::COMMAND_PASTE_IMAGE,
            Self::Copy { .. } => MessageId::COMMAND_COPY,
            Self::Properties => MessageId::COMMON_DONE,
            Self::RasterizeSource { .. } => MessageId::DOCUMENTS_RASTERIZE,
            Self::New => MessageId::DOCUMENTS_CREATE,
            Self::Open => MessageId::DOCUMENTS_OPEN_ACCEPT,
            Self::Export { .. } => MessageId::DOCUMENTS_EXPORT_ACCEPT,
            _ => MessageId::COMMON_SAVE,
        })
    }
    pub fn filter(&self, localization: &Localizer) -> (std::sync::Arc<str>, &'static str) {
        match self {
            Self::Export { .. } => (localization.text(MessageId::DOCUMENTS_PNG), "png"),
            Self::ImportLookup {..} => (localization.text(MessageId::RESOURCES_LOOKUP_FILES), "cube"),
            _ => (localization.text(MessageId::DOCUMENTS_CAPY), "capy"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseDecision {
    Save,
    Discard,
    Cancel,
}

pub const DEFAULT_DOCUMENT_EXTENT: [u32; 2] = [2048, 1536];
pub const MAX_NEW_DOCUMENT_DIMENSION: u32 = 8192;

#[derive(Clone, Debug, Serialize)]
pub struct NewDocumentSpec {
    pub title: std::sync::Arc<str>,
    pub labels: [std::sync::Arc<str>; 2],
    pub extent: [u32; 2],
    pub numeric: NumericControl,
    pub minimum: u32,
    pub maximum: u32,
    pub accept: std::sync::Arc<str>,
    pub cancel: std::sync::Arc<str>,
    pub discard: std::sync::Arc<str>,
    pub unsaved_description: std::sync::Arc<str>,
}
pub fn new_document_spec(localization: &Localizer) -> NewDocumentSpec {
    NewDocumentSpec {
        title: localization.text(MessageId::DOCUMENTS_NEW),
        labels: [localization.text(MessageId::DOCUMENTS_WIDTH), localization.text(MessageId::DOCUMENTS_HEIGHT)],
        extent: DEFAULT_DOCUMENT_EXTENT,
        numeric: NumericControl {
            kind: NumericKind::Number,
            ..NumericControl::number(1., f64::from(MAX_NEW_DOCUMENT_DIMENSION), 1., 0)
        },
        minimum: 1,
        maximum: MAX_NEW_DOCUMENT_DIMENSION,
        accept: localization.text(MessageId::DOCUMENTS_CREATE),
        cancel: localization.text(MessageId::COMMON_CANCEL),
        discard: localization.text(MessageId::DOCUMENTS_DISCARD),
        unsaved_description: localization.text(MessageId::DOCUMENTS_UNSAVED_DESCRIPTION),
    }
}

/// Shared new-document constraints; creation is a host operation so native
/// windows and future tabbed/mobile hosts can use different presentation.
pub fn new_drawing(width: u32, height: u32, localization: &Localizer) -> Result<Document, String> {
    NewDocumentOptions {
        extent: [width, height],
        ..Default::default()
    }
    .project(localization)
}

#[derive(Clone)]
pub struct DocumentExport {
    pub capture: ArtworkCapture,
    pub time: f32,
}
impl DocumentExport {
    pub fn composition(&self) -> &layer_core::Composition { self.capture.artwork.compositions.get(self.capture.artwork.root).expect("captured composition") }
    pub fn output(&self) -> &layer_core::Output { self.capture.artwork.outputs.get(self.capture.artwork.default_output).expect("captured output") }
    pub fn metadata(&self) -> &layer_core::PhotoMetadata { &self.capture.artwork.metadata }
}

enum DocumentRequestCopy {
    Filename(&'static str),
    Close(Option<String>),
}

#[derive(Default)]
pub(super) struct DocumentFiles {
    pub(super) saved_checkpoint: u64,
    pub(super) unpublished: bool,
    pub(super) destination: Option<super::session_recovery::DestinationFingerprint>,
    pub(super) check_destination: bool,
    pub(super) pending_modified_change: bool,
    replace_in_place: bool,
    replace_after: Option<bool>,
    pub(super) pending: Option<(u32, Option<(CaptureCheckpoint, DocumentLocation)>)>,
    close_after: bool,
    pending_copy: Option<DocumentRequestCopy>,
    pub(super) cut: Option<clipboard::PendingCut>,
    host_error_copy: Option<DocumentHostErrorCopy>,
    pub(super) last_export: Option<ExportRepeat>,
    pending_export: Option<(u64, u64, ExportRepeat)>,
}
impl DocumentFiles {
    pub(super) fn session_state(&self,file:&DocumentFileState,camera:super::session_recovery::SessionCamera)->super::session_recovery::SessionDocumentState {
        let Self {saved_checkpoint,unpublished,destination,last_export,pending_export:_,check_destination:_,pending_modified_change:_,replace_in_place:_,replace_after:_,pending:_,close_after:_,pending_copy:_,cut:_,host_error_copy:_}=self;
        let DocumentFileState {epoch:_,revision:_,location,export_uri:_,unsaved_name,modified:_,recovered,busy:_,close_ready:_,untitled:_}=file;
        super::session_recovery::SessionDocumentState {camera,location:location.clone(),unsaved_name:unsaved_name.clone(),saved_checkpoint:*saved_checkpoint,
            unpublished:*unpublished,recovered:*recovered,destination:destination.clone(),last_export:last_export.as_ref().map(super::session_recovery::detach_export)}
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn retire_host_request(&mut self, id: u32) -> Result<(), String> {
        let index = self.state.requests.iter().position(|request| request.id == id).ok_or("Unknown host request")?;
        if matches!(self.state.requests[index].kind, HostRequestKind::Document { .. }) {
            return Err("Complete document requests through the document service".into());
        }
        self.state.requests.remove(index);
        Ok(())
    }
    pub fn set_host_error(&mut self, error: Option<String>) {
        self.state.host_error=error;self.files.host_error_copy=None;
    }
    pub fn set_host_error_copy(&mut self, reason: Option<DocumentHostErrorCopy>) {
        self.set_host_error(reason.as_ref().map(|reason| reason.message(self.localization())));
        self.files.host_error_copy=reason;
    }
    pub fn complete_document_request_failed(&mut self, id: u32, reason: DocumentHostErrorCopy) -> Result<UiChange, String> {
        let change=self.complete_document_request(id, Err(reason.message(self.localization())))?;
        self.files.host_error_copy=Some(reason);
        Ok(change)
    }
    pub(super) fn refresh_document_file_localization(&mut self) {
        if let Some(reason)=&self.files.host_error_copy { self.state.host_error=Some(reason.message(self.localization())); }
        self.state.document_file.set_localization(&self.state.localization);
        let Some((id, _)) = &self.files.pending else { return; };
        let Some(copy) = &self.files.pending_copy else { return; };
        let text = match copy {
            DocumentRequestCopy::Filename(extension) => {
                format!("{}.{}", self.state.localization.text(MessageId::DOCUMENTS_UNTITLED), extension)
            }
            DocumentRequestCopy::Close(name) => {
                let mut args = FluentArgs::new();
                let untitled = self.state.localization.text(MessageId::DOCUMENTS_UNTITLED);
                args.set("name", name.as_deref().unwrap_or(&untitled));
                self.state.localization.format(MessageId::DOCUMENTS_CLOSE_CONFIRM, &args)
            }
        };
        if let Some(request) = self.state.requests.iter_mut().find(|request| request.id == *id) {
            match &mut request.kind {
                HostRequestKind::Document { request: DocumentRequest::Save { name, .. } | DocumentRequest::Export { name, .. } } => *name = text,
                HostRequestKind::Document { request: DocumentRequest::ConfirmClose { title } } => *title = text,
                _ => {},
            }
        }
    }

    pub(super) fn numbered_document_name(&self, id: MessageId, number: u64) -> String {
        let mut args = FluentArgs::new();
        args.set("number", number);
        self.localization().format(id, &args)
    }
    /// Hosts validate/decode projects off-thread first. A fresh session avoids
    /// replacing a working document or colliding with its live GPU resources.
    pub fn from_project(
        renderer: R,
        document: Document,
        location: Option<DocumentLocation>,
        viewport: [u32; 2],
        platform: Platform,
    ) -> Result<Self, String> {
        Self::from_project_localized(renderer, document, location, viewport, platform, Localizer::shared(UiLanguage::English))
    }

    pub fn from_project_localized(
        renderer: R,
        mut document: Document,
        location: Option<DocumentLocation>,
        viewport: [u32; 2],
        platform: Platform,
        localization: std::sync::Arc<Localizer>,
    ) -> Result<Self, String> {
        if let Some(location) = &location {
            location.validate().map_err(|error| error.message(&localization))?;
        }
        if document.working.occurrence.is_none() && document.working.target.is_none() && document.working.generation == 0 {
            let scene = document.scene();
            let occurrence = scene.order().iter().copied().find(|handle| matches!(scene.source_target(*handle), Some(SourceTarget::Paint(_)))).or_else(|| scene.order().first().copied());
            let target = occurrence.and_then(|handle| scene.source_target(handle));
            document.working.occurrence = occurrence;
            document.working.target = target;
            document.working.layer_selection = occurrence.into_iter().collect();
            document.working.layer_anchor = occurrence;
        }
        let photo_name = location.is_none().then(|| {
            document.scene().order().iter().find_map(|handle| {
                let occurrence = document.scene().occurrence(*handle)?;
                let OccurrenceContent::Paint(paint) = occurrence.content else { return None; };
                document.artwork.paint.get(paint)?.base.as_ref().map(|_| occurrence.name.to_string())
            })
        }).flatten();
        let mut session = Self::new_localized(renderer, document, viewport, platform, localization)?;
        session.state.document_file.location = location;
        if let Some(name) = photo_name {
            session.state.document_file.unsaved_name = Some(name);
            session.files.unpublished = true; // Imported content needs its own master save.
        }
        session.refresh_document();
        Ok(session)
    }

    /// A prepared, unpublished session receives its destination before joining
    /// a window's drawing collection. This is not a Save and cannot clear edits.
    pub fn initialize_document_location(&mut self, location: Option<DocumentLocation>) -> Result<(), String> {
        if self.state.document_file.busy || self.engine.checkpoint() != self.files.saved_checkpoint {
            return Err(FileFailure::EditedLocation.message(self.localization()));
        }
        if let Some(location) = &location { location.validate().map_err(|error| error.message(self.localization()))?; }
        if location.is_some() {
            self.files.unpublished = false;
            self.state.document_file.unsaved_name = None;
        }
        self.state.document_file.location = location;
        self.refresh_document();
        self.refresh_commands();
        Ok(())
    }

    pub(super) fn refresh_file_state(&mut self) {
        self.state.document_file.revision = self.engine.document().revision;
        self.state.document_file.modified = self.files.unpublished
            || self.input_pending
            || self.engine.has_active_stroke()
            || self.engine.checkpoint() != self.files.saved_checkpoint;
        self.state.document_file.busy = self.files.pending.is_some();
    }

    pub(super) fn request_document(&mut self, request: DocumentRequest) -> Result<(), String> {
        if matches!(request, DocumentRequest::Save { .. }) {
            self.require_raster_snapshot()?;
        } else if matches!(request, DocumentRequest::ConfirmClose { .. }) {
            self.require_document_snapshot_idle()?;
        } else {
            self.require_document_idle()?;
        }
        if self.files.pending.is_some() {
            return Err(FileFailure::Busy.message(self.localization()));
        }
        let pending_copy = if self.state.document_file.location.is_none() && self.state.document_file.unsaved_name.is_none() {
            match &request {
                DocumentRequest::Save { .. } => Some(DocumentRequestCopy::Filename("capy")),
                DocumentRequest::Export { repeat: None, .. } => Some(DocumentRequestCopy::Filename("png")),
                _ => None,
            }
        } else { None };
        let id = self.next_request;
        self.request(HostRequestKind::Document { request })?;
        self.files.pending_copy = pending_copy;
        self.set_host_error(None);
        self.files.pending = Some((id, None));
        self.refresh_file_state();
        Ok(())
    }

    pub fn document_request(&self, id: u32) -> Result<&DocumentRequest, String> {
        if self.files.pending.as_ref().map(|p| p.0) != Some(id) {
            return Err(FileFailure::UnknownRequest.message(self.localization()));
        }
        self.state
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if r.id == id => Some(request),
                _ => None,
            })
            .ok_or_else(|| FileFailure::UnknownRequest.message(self.localization()))
    }

    pub fn set_document_replacement(&mut self, enabled: bool) {
        self.files.replace_in_place = enabled;
    }
    pub(super) fn request_document_open(&mut self, opening: bool) -> Result<(), String> {
        if self.files.replace_in_place {
            self.require_document_idle()?;
            if self.files.pending.is_some() {
                return Err(FileFailure::Busy.message(self.localization()));
            }
            self.files.replace_after = Some(opening);
            self.request_document_close()?;
            Ok(())
        } else {
            self.request_document(if opening {
                DocumentRequest::Open
            } else {
                DocumentRequest::New
            })
        }
    }
    fn finish_close_or_replace(&mut self) -> Result<(), String> {
        if let Some(opening) = self.files.replace_after.take() {
            self.request_document(if opening {
                DocumentRequest::Open
            } else {
                DocumentRequest::New
            })
        } else {
            self.state.document_file.close_ready = true;
            Ok(())
        }
    }
    /// A cancelled application termination can keep an already approved window.
    pub fn reset_document_close(&mut self) {
        self.state.document_file.close_ready = false;
        self.files.close_after = false;
        self.files.replace_after = None;
        self.refresh_commands();
        self.changed(regions::DOCUMENT | regions::COMMANDS, false);
    }
    /// Export pickers may select a location only after the archive is ready.
    pub fn retarget_project_save(
        &mut self,
        id: u32,
        location: DocumentLocation,
    ) -> Result<(), String> {
        location.validate().map_err(|error| error.message(self.localization()))?;
        self.document_request(id)?;
        let localization = self.localization().clone();
        let (_, snapshot) = self.files.pending.as_mut().unwrap();
        snapshot.as_mut().ok_or_else(|| FileFailure::SaveNotCaptured.message(&localization))?.1 = location;
        Ok(())
    }

    pub(super) fn request_save(&mut self, save_as: bool) -> Result<(), String> {
        self.request_document(DocumentRequest::Save {
            location: (!save_as)
                .then(|| self.state.document_file.location.clone())
                .flatten(),
            name: self.document_filename("capy"),
        })
    }

    pub(super) fn document_filename(&self, extension: &str) -> String {
        let name = self.state.document_file.title();
        format!(
            "{}.{}",
            name.strip_suffix(".capy").unwrap_or(name),
            extension
        )
    }

    /// Capture committed artwork without private history or a save acknowledgement.
    pub fn capture_artwork(&self) -> Result<ArtworkCapture, String> {
        self.require_raster_snapshot()?;
        self.engine.capture_artwork(self.state.document_file.epoch).map_err(error)
    }

    pub fn document_snapshot(&self) -> Result<Document, String> {
        self.require_raster_snapshot()?;
        Ok(self.engine.document().clone())
    }

    /// Freeze the committed master and its rendering coordinates for an output
    /// worker. This never reserves or acknowledges a saved-project checkpoint.
    pub fn capture_project_export(&self, id: u32) -> Result<DocumentExport, String> {
        if !matches!(self.document_request(id)?, DocumentRequest::Export { owner, epoch, .. } if *owner == self.engine.document().owner && *epoch == self.state.document_file.epoch) {
            return Err(FileFailure::NotExportRequest.message(self.localization()));
        }
        self.require_document_idle()?;
        let capture = self.capture_artwork()?;
        let time = capture.artwork.outputs.get(capture.artwork.default_output).expect("captured output").context.elapsed;
        Ok(DocumentExport {
            capture,
            time,
        })
    }

    /// A manual save additionally reserves the checkpoint to acknowledge once
    /// the destination is durable. Recovery never changes that checkpoint.
    pub fn capture_project_save(
        &mut self,
        id: u32,
        location: DocumentLocation,
    ) -> Result<ArtworkCapture, String> {
        self.require_raster_snapshot()?;
        location.validate().map_err(|error| error.message(self.localization()))?;
        if !matches!(self.document_request(id)?, DocumentRequest::Save { .. })
            || self.files.pending.as_ref().is_some_and(|p| p.1.is_some())
        {
            return Err(FileFailure::InvalidSaveRequest.message(self.localization()));
        }
        let capture = self.capture_artwork()?;
        self.files.pending.as_mut().unwrap().1 = Some((capture.checkpoint, location));
        Ok(capture)
    }

    pub fn apply_lookup(&mut self, request:u32, resource: std::sync::Arc<layer_core::Lut3d>) -> Result<bool,String> {
        let DocumentRequest::ImportLookup {target} = self.document_request(request)?.clone() else {return Err("Invalid lookup request".into());};
        if self.engine.document().artwork.id != target.document || self.state.document_file.epoch != target.activation || !self.property_editor.accepts(target.layer,target.epoch)
            || self.state.layer_properties.layer != Some(target.layer) {return Ok(false);}
        self.effect_action(EffectAction::Set {layer:target.layer,key:target.key,value:layer_core::EffectValue::Lut3d(Some(resource))})?;
        Ok(true)
    }

    pub fn prepare_export(&mut self, id: u32, owner: u64, recipe: ExportRecipe, location: DocumentLocation) -> Result<(), String> {
        if !matches!(self.document_request(id)?, DocumentRequest::Export { owner: expected, epoch, .. } if *expected == owner && *epoch == self.state.document_file.epoch)
            || self.engine.document().owner != owner {
            return Err(FileFailure::UnknownRequest.message(self.localization()));
        }
        location.validate().map_err(|reason| reason.message(self.localization()))?;
        recipe.validate_for_document(self.engine.document()).map_err(|reason| reason.message(self.localization()))?;
        if self.state.document_file.location.as_ref().is_some_and(|master| master.uri == location.uri) {
            return Err(crate::color_feature_copy::DocumentColorCopy::new(self.localization()).choose_different.to_string());
        }
        self.files.pending_export = Some((owner, self.state.document_file.epoch, ExportRepeat { recipe, location }));
        Ok(())
    }

    /// Success acknowledges a completed write/open/export, never just a chosen
    /// filename. Cancellation is distinct from failure and does not clear dirty.
    pub fn complete_document_request(
        &mut self,
        id: u32,
        result: Result<bool, String>,
    ) -> Result<UiChange, String> {
        let request = self.document_request(id)?;
        if matches!(request, DocumentRequest::ConfirmClose { .. }) {
            return Err(FileFailure::RespondClose.message(self.localization()));
        }
        let save = matches!(request, DocumentRequest::Save { .. });
        let export = matches!(request, DocumentRequest::Export { .. });
        let lookup = matches!(request, DocumentRequest::ImportLookup { .. });
        let cutting = matches!(request, DocumentRequest::Copy { cut: true, .. });
        if save && result == Ok(true) && self.files.pending.as_ref().unwrap().1.is_none() {
            return Err(FileFailure::NoSavedSnapshot.message(self.localization()));
        }
        if save && result == Ok(true) && self.files.pending.as_ref().and_then(|(_, snapshot)| snapshot.as_ref()).is_some_and(|(checkpoint, _)| {
            checkpoint.document != self.engine.document().artwork.id || checkpoint.owner != self.engine.document().owner || checkpoint.session_generation != self.state.document_file.epoch
        }) {
            return Err(FileFailure::UnknownRequest.message(self.localization()));
        }
        if export && result == Ok(true) && self.files.pending_export.as_ref().is_some_and(|(owner, epoch, _)| {
            *owner != self.engine.document().owner || *epoch != self.state.document_file.epoch
        }) {
            return Err(FileFailure::UnknownRequest.message(self.localization()));
        }
        let (_, snapshot) = self.files.pending.take().unwrap();
        self.files.pending_copy = None;
        self.state.requests.retain(|r| r.id != id);
        let success = result == Ok(true);
        if let Some((_, _, last)) = self.files.pending_export.take().filter(|_| export && success) {
            self.state.document_file.export_uri = Some(last.location.uri.clone());
            self.files.last_export = Some(last);
        }
        let cut = self.files.cut.take().filter(|_| cutting && success);
        if let Err(diagnostic) = result {
            if lookup {
                eprintln!("Lookup import: {diagnostic}");
                self.set_host_error_copy(Some(DocumentHostErrorCopy::Document(DocumentHostError::LookupImportFailed)));
            } else { self.set_host_error(Some(diagnostic)); }
        } else { self.set_host_error(None); }
        if save
            && success
            && let Some((checkpoint, location)) = snapshot
        {
            self.files.saved_checkpoint = checkpoint.edit_checkpoint;
            self.files.unpublished = false;
            self.state.document_file.location = Some(location);
            self.state.document_file.recovered = false;
            self.files.destination = None;
            self.files.check_destination = false;
        }
        self.refresh_file_state();
        self.files.close_after &= success;
        if !success {
            self.files.replace_after = None;
        }
        if let Some(cut) = cut {
            self.finish_cut(cut);
        }
        self.poll_document_close();
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(
            regions::DOCUMENT | regions::COMMANDS | regions::HOST,
            self.files.close_after || self.engine.has_pending_document_edits(),
        ))
    }

    /// A save can complete in the middle of a new stroke. Defer the close
    /// decision until that stroke commits, then recheck the saved checkpoint.
    pub(super) fn poll_document_close(&mut self) -> u32 {
        if self.files.close_after
            && self.files.pending.is_none()
            && self.require_document_snapshot_idle().is_ok()
        {
            self.files.close_after = false;
            if let Err(error) = self.request_document_close() {
                self.set_host_error(Some(error));
            }
            regions::DOCUMENT | regions::COMMANDS | regions::HOST
        } else {
            0
        }
    }

    pub(super) fn opening_drawing(&self) -> bool {
        self.files.pending.as_ref().is_some_and(|(id, _)| matches!(self.document_request(*id), Ok(DocumentRequest::New | DocumentRequest::Open)))
    }

    pub fn request_document_close(&mut self) -> Result<UiChange, String> {
        self.require_document_snapshot_idle()?;
        if self.opening_drawing() {
            return Err(FileFailure::Busy.message(self.localization()));
        }
        self.refresh_file_state();
        if self.files.pending.is_some() {
            self.files.close_after = true;
        } else if self.state.document_file.modified {
            self.request_document(DocumentRequest::ConfirmClose {
                title: {
                    let mut args = FluentArgs::new();
                    args.set("name", self.state.document_file.title());
                    self.localization().format(MessageId::DOCUMENTS_CLOSE_CONFIRM, &args)
                },
            })?;
            self.files.pending_copy = Some(DocumentRequestCopy::Close(
                (self.state.document_file.location.is_some() || self.state.document_file.unsaved_name.is_some())
                    .then(|| self.state.document_file.title().to_owned()),
            ));
        } else {
            self.finish_close_or_replace()?;
        }
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::HOST, false))
    }

    pub fn respond_document_close(
        &mut self,
        id: u32,
        decision: CloseDecision,
    ) -> Result<UiChange, String> {
        if !matches!(
            self.document_request(id)?,
            DocumentRequest::ConfirmClose { .. }
        ) {
            return Err(FileFailure::NotCloseRequest.message(self.localization()));
        }
        self.files.pending = None;
        self.files.pending_copy = None;
        self.files.close_after = false;
        self.state.requests.retain(|r| r.id != id);
        match decision {
            CloseDecision::Cancel => self.files.replace_after = None,
            CloseDecision::Discard => self.finish_close_or_replace()?,
            CloseDecision::Save => {
                self.request_save(false)?;
                self.files.close_after = true;
            }
        }
        self.refresh_file_state();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::HOST, false))
    }

    pub(crate) fn recovery_file_busy(&self)->bool {
        self.state.document_file.busy && !self.files.pending.as_ref().is_some_and(|(id,_)|
            matches!(self.document_request(*id),Ok(DocumentRequest::Save {..})))
    }

    pub(crate) fn require_raster_snapshot(&self) -> Result<(), String> {
        if self.painted_selections.busy() { return Err(FileFailure::SelectionCapture.message(self.localization())); }
        if self.input_held || self.sdr_gesture.is_some() || self.object_motion.is_some()
            || self.targeted_curve_busy() || self.auto_levels.is_some()
            || self.eyedropper.calibration.as_ref().is_some_and(|calibration| calibration.request.is_some())
            || self.operation.active()
            || self.objects.placing()
            || self.objects.dragging()
            || (self.region_tools.busy() && !self.refine_previewing())
            || !self.layer_interaction.path.is_empty()
        {
            Err(FileFailure::CanvasOperation.message(self.localization()))
        } else {
            Ok(())
        }
    }

    fn document_interaction_idle_reason(&self) -> Option<DocumentIdleReason> {
        if !self.canvas_idle() { Some(DocumentIdleReason::CanvasInteraction) }
        else if self.content_bounds.baking() { Some(DocumentIdleReason::WaitTransform) }
        else if self.conversion_busy() { Some(DocumentIdleReason::CanvasOperation) }
        else if self.operation.active() || self.objects.placing() {
            Some(if self.cropping() { DocumentIdleReason::ApplyCrop } else { DocumentIdleReason::ApplyTransform })
        } else if (self.region_tools.busy() && !self.refine_previewing())
            || self.targeted_curve_busy() || self.auto_levels.is_some()
            || self.eyedropper.calibration.as_ref().is_some_and(|calibration| calibration.request.is_some()) {
            Some(DocumentIdleReason::CanvasOperation)
        } else { None }
    }
    fn require_document_interaction_idle(&self) -> Result<(), String> {
        self.document_interaction_idle_reason().map_or(Ok(()), |reason| Err(reason.message(self.localization()).to_string()))
    }
    pub fn document_idle_reason(&self) -> Option<DocumentIdleReason> {
        self.document_interaction_idle_reason().or_else(|| self.pending_filters.is_some().then_some(DocumentIdleReason::CanvasOperation))
    }
    pub(crate) fn require_proof_idle(&self) -> Result<(), crate::ColorFeatureError> {
        self.document_idle_reason().map_or(Ok(()), |reason| Err(reason.into()))
    }

    /// Catalog validation owns private GPU work and cannot change document
    /// or workspace values. Closing and immutable saves may proceed, including
    /// their unsaved-changes decisions. Replacing the document still waits.
    pub fn require_document_snapshot_idle(&self) -> Result<(), String> {
        if self.painted_selections.busy() { return Err(FileFailure::SelectionCapture.message(self.localization())); }
        self.require_document_interaction_idle()?;
        Ok(())
    }

    pub fn require_document_idle(&self) -> Result<(), String> {
        self.document_idle_reason().map_or(Ok(()), |reason| Err(reason.message(self.localization()).to_string()))
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::{Recorder, layer};

    #[test]
    fn retired_typed_host_request_failures_follow_language_without_changing_document_or_literal_errors() {
        let mut session = UiSession::blank_localized(Recorder::default(), [256,256], Platform::Windows, Localizer::shared(UiLanguage::English)).unwrap();
        layer(&mut session, LayerAction::New { group:false, clipped:false });
        session.dispatch(UiAction::OpenSettings { page:SettingsPage::Shortcuts }).unwrap();
        let document = session.engine.document().clone();
        let checkpoint = session.engine.checkpoint();
        let files = serde_json::to_value(&session.state.document_file).unwrap();
        let rendering = (session.engine.backend().composites, session.engine.backend().dabs, session.engine.backend().snapshot_requests.len());
        for reason in [HostRequestFailure::ActionFailed, HostRequestFailure::KeymapTooLarge] {
            session.dispatch(UiAction::Preferences { action:PreferenceAction::ChooseKeymapFile }).unwrap();
            let id = session.state.requests.last().unwrap().id;
            let action: UiAction = serde_json::from_value(serde_json::json!({"type":"complete_request_failure","id":id,"reason":reason})).unwrap();
            assert!(action.is_host_report());
            session.dispatch(action.clone()).unwrap();
            assert!(!session.state.requests.iter().any(|request| request.id == id));
            let requests = serde_json::to_value(&session.state.requests).unwrap();
            assert!(session.dispatch(action).is_err());
            for language in UiLanguage::ALL {
                session.set_localization(Localizer::shared(language));
                assert_eq!(session.state.host_error, Some(reason.message(session.localization())));
                assert_eq!(session.files.host_error_copy, Some(DocumentHostErrorCopy::HostRequest(reason)));
                assert_eq!(session.engine.document(), &document);
                assert_eq!(session.engine.checkpoint(), checkpoint);
                assert_eq!(serde_json::to_value(&session.state.document_file).unwrap(), files);
                assert_eq!(serde_json::to_value(&session.state.requests).unwrap(), requests);
                assert_eq!((session.engine.backend().composites, session.engine.backend().dabs, session.engine.backend().snapshot_requests.len()), rendering);
            }
        }
        session.request_document(DocumentRequest::Open).unwrap();
        let id = session.files.pending.as_ref().unwrap().0;
        let error = session.state.host_error.clone();
        assert!(session.dispatch(UiAction::CompleteRequestFailure { id, reason:HostRequestFailure::ActionFailed }).is_err());
        assert_eq!(session.state.host_error, error);
        assert!(session.state.requests.iter().any(|request| request.id == id));
        session.complete_document_request(id, Ok(false)).unwrap();
        session.dispatch(UiAction::Preferences { action:PreferenceAction::ChooseKeymapFile }).unwrap();
        let id = session.state.requests.last().unwrap().id;
        let literal = "literal İı ไทย { $name }\n{\"type\":\"complete_request_failure\",\"reason\":\"keymap_too_large\"}";
        session.dispatch(UiAction::CompleteRequest { id, error:Some(literal.into()) }).unwrap();
        for language in UiLanguage::ALL {
            session.set_localization(Localizer::shared(language));
            assert_eq!(session.state.host_error.as_deref(), Some(literal));
            assert!(session.files.host_error_copy.is_none());
            assert_eq!(session.engine.document(), &document);
            assert_eq!(session.engine.checkpoint(), checkpoint);
        }
        session.dispatch(UiAction::CloseSettings).unwrap();
        session.dispatch(UiAction::Invoke { command:CommandId::Undo }).unwrap();
        assert_eq!(session.engine.document().scene().order().len() + 1, document.scene().order().len());
        session.dispatch(UiAction::Invoke { command:CommandId::Redo }).unwrap();
        assert_eq!(session.engine.document().scene().order(), document.scene().order());
    }

    #[test]
    fn retained_export_preferences_warning_refreshes_after_task_retirement_without_document_changes() {
        let mut session = UiSession::blank_localized(Recorder::default(), [256,256], Platform::Windows, Localizer::shared(UiLanguage::English)).unwrap();
        session.state.document_file.unsaved_name = Some("Untitled { $name } 🖌".into());
        layer(&mut session, LayerAction::New { group: false, clipped: false });
        session.request_document(DocumentRequest::Export { name: "literal 🖌.png".into(), owner: session.engine.document().owner, epoch:session.state.document_file.epoch, repeat: None }).unwrap();
        let id = session.files.pending.as_ref().unwrap().0;
        session.complete_document_request(id, Ok(true)).unwrap();
        assert!(session.files.pending.is_none());
        assert!(!session.state.requests.iter().any(|request| request.id == id));
        let document = session.engine.document().clone();
        let checkpoint = session.engine.checkpoint();
        let files = serde_json::to_value(&session.state.document_file).unwrap();
        let requests = serde_json::to_value(&session.state.requests).unwrap();
        let rendering = (session.engine.backend().composites, session.engine.backend().dabs, session.engine.backend().snapshot_requests.len());
        for reason in [crate::ColorFeatureError::PresetNameInvalid, crate::ColorFeatureError::PresetChanged,
            crate::ColorFeatureError::Diagnostic("literal { $name } 🖌\n{\"color_feature_error\":\"PresetChanged\"}".into())] {
            session.set_host_error_copy(Some(DocumentHostErrorCopy::ExportPreferences(reason.clone())));
            for language in UiLanguage::ALL.into_iter().chain([UiLanguage::English]) {
                session.set_localization(Localizer::shared(language));
                let expected = crate::DocumentDeliveryMessage::ExportPreferences { detail: reason.preset_message(session.localization()) }.message(session.localization());
                assert_eq!(session.state.host_error.as_deref(), Some(expected.as_str()), "{}", language.tag());
                if let crate::ColorFeatureError::Diagnostic(detail) = &reason { assert!(session.state.host_error.as_ref().unwrap().contains(detail)); }
                assert_eq!(session.files.host_error_copy, Some(DocumentHostErrorCopy::ExportPreferences(reason.clone())));
                assert_eq!(session.engine.document(), &document);
                assert_eq!(session.engine.checkpoint(), checkpoint);
                assert_eq!(serde_json::to_value(&session.state.document_file).unwrap(), files);
                assert_eq!(serde_json::to_value(&session.state.requests).unwrap(), requests);
                assert!(session.files.pending.is_none());
                assert_eq!((session.engine.backend().composites, session.engine.backend().dabs, session.engine.backend().snapshot_requests.len()), rendering);
            }
        }
        let literal = "literal { $name } 🖌 {\"color_feature_error\":\"PresetChanged\"}";
        session.set_host_error(Some(literal.into()));
        for language in UiLanguage::ALL {
            session.set_localization(Localizer::shared(language));
            assert_eq!(session.state.host_error.as_deref(), Some(literal));
            assert!(session.files.host_error_copy.is_none());
        }
        session.set_host_error_copy(None);
        session.set_localization(Localizer::shared(UiLanguage::English));
        assert!(session.state.host_error.is_none());
        assert!(session.files.host_error_copy.is_none());
    }

    #[test]
    fn typed_host_failures_retain_semantic_reasons_and_literal_arguments_in_every_language() {
        use crate::{DocumentDeliveryMessage as Delivery, DocumentTransportRefusal as Transport};
        let literal="Éİı ไทย Tiếng Việt 雪 { $name }";
        let mut cases=vec![
            (DocumentHostErrorCopy::Delivery(Delivery::ClipboardUnavailable),MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_UNAVAILABLE,None),
            (DocumentHostErrorCopy::Delivery(Delivery::ClipboardTooLarge),MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_TOO_LARGE,None),
            (DocumentHostErrorCopy::Delivery(Delivery::ClipboardEmpty),MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_EMPTY,None),
            (DocumentHostErrorCopy::Delivery(Delivery::ConvertedFilenameInvalid),MessageId::DOCUMENTS_DELIVERY_CONVERTED_FILENAME_INVALID,None),
            (DocumentHostErrorCopy::Delivery(Delivery::ChooseDifferent),MessageId::COLOR_FEATURES_COLOR_CHOOSE_DIFFERENT,None),
            (DocumentHostErrorCopy::Delivery(Delivery::ClipboardFormats {formats:literal.into()}),MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_FORMATS,Some("formats")),
            (DocumentHostErrorCopy::Delivery(Delivery::ExportExtension {extension:literal.into()}),MessageId::DOCUMENTS_DELIVERY_EXPORT_EXTENSION,Some("extension")),
            (DocumentHostErrorCopy::Delivery(Delivery::ClipboardShared {detail:literal.into()}),MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_SHARED,Some("detail")),
            (DocumentHostErrorCopy::Delivery(Delivery::OutputCleanup {detail:literal.into()}),MessageId::DOCUMENTS_DELIVERY_OUTPUT_CLEANUP,Some("detail")),
            (DocumentHostErrorCopy::Delivery(Delivery::ExportPreferences {detail:literal.into()}),MessageId::DOCUMENTS_DELIVERY_EXPORT_PREFERENCES,Some("detail")),
        ];
        for (reason,id) in [
            (DocumentHostError::NewWindowUnavailable,MessageId::DOCUMENTS_ERROR_NEW_WINDOW_UNAVAILABLE),
            (DocumentHostError::DrawingTabsUnavailable,MessageId::DOCUMENTS_ERROR_TABS_UNAVAILABLE),
            (DocumentHostError::ChooseDeviceFile,MessageId::DOCUMENTS_ERROR_DEVICE_FILE),
            (DocumentHostError::ChooseFilename,MessageId::DOCUMENTS_ERROR_FILENAME),
            (DocumentHostError::ProjectWriterFailed,MessageId::DOCUMENTS_ERROR_PROJECT_WRITER),
            (DocumentHostError::NoDrawingToOpen,MessageId::DOCUMENTS_ERROR_NO_DRAWING),
            (DocumentHostError::LookupImportFailed,MessageId::RESOURCES_LOOKUP_FAILED),
        ] { cases.push((DocumentHostErrorCopy::Document(reason),id,None)); }
        for (reason,id) in [
            (Transport::SnapshotChanged,MessageId::DOCUMENTS_REFUSAL_SNAPSHOT_CHANGED),
            (Transport::OpenSnapshotChanged,MessageId::DOCUMENTS_REFUSAL_OPEN_SNAPSHOT_CHANGED),
            (Transport::RestoreOperation,MessageId::DOCUMENTS_REFUSAL_RESTORE_OPERATION),
            (Transport::RecoveryGpuChanged,MessageId::DOCUMENTS_REFUSAL_RECOVERY_GPU_CHANGED),
            (Transport::SwitchGpuChanged,MessageId::DOCUMENTS_REFUSAL_SWITCH_GPU_CHANGED),
            (Transport::PaintingUnavailable,MessageId::DOCUMENTS_REFUSAL_PAINTING_UNAVAILABLE),
            (Transport::RecoveryServiceUnavailable,MessageId::DOCUMENTS_REFUSAL_RECOVERY_SERVICE_UNAVAILABLE),
            (Transport::RecoveryInProgress,MessageId::DOCUMENTS_REFUSAL_RECOVERY_IN_PROGRESS),
            (Transport::SwitchOperation,MessageId::DOCUMENTS_REFUSAL_SWITCH_OPERATION),
            (Transport::ChangeInProgress,MessageId::DOCUMENTS_REFUSAL_CHANGE_IN_PROGRESS),
            (Transport::SwitchDialog,MessageId::DOCUMENTS_REFUSAL_SWITCH_DIALOG),
            (Transport::CloseOperation,MessageId::DOCUMENTS_REFUSAL_CLOSE_OPERATION),
            (Transport::SelectedChanged,MessageId::DOCUMENTS_REFUSAL_SELECTED_CHANGED),
            (Transport::OpenOperation,MessageId::DOCUMENTS_REFUSAL_OPEN_OPERATION),
            (Transport::BatchOpening,MessageId::DOCUMENTS_REFUSAL_BATCH_OPENING),
            (Transport::OpenDrawingsOperation,MessageId::DOCUMENTS_REFUSAL_OPEN_DRAWINGS_OPERATION),
        ] { cases.push((DocumentHostErrorCopy::Transport(reason),id,None)); }
        let mut session=UiSession::blank_localized(Recorder::default(),[96,96],Platform::Web,Localizer::shared(UiLanguage::English)).unwrap();
        let checkpoint=session.engine.checkpoint();let document=session.engine.document().clone();
        let rendering=(session.engine.backend().composites,session.engine.backend().dabs,session.engine.backend().snapshot_requests.len());
        for (reason,id,argument) in cases {
            let encoded=serde_json::to_value(&reason).unwrap();
            assert_eq!(serde_json::from_value::<DocumentHostErrorCopy>(encoded).unwrap(),reason);
            session.request_document(DocumentRequest::Open).unwrap();let request=session.files.pending.as_ref().unwrap().0;
            session.complete_document_request_failed(request,reason.clone()).unwrap();
            assert!(session.complete_document_request_failed(request,DocumentHostErrorCopy::HostRequest(HostRequestFailure::ActionFailed)).is_err());
            assert_eq!(session.files.host_error_copy,Some(reason.clone()));
            for language in UiLanguage::ALL {
                session.set_localization(Localizer::shared(language));
                let expected=if let Some(argument)=argument {let mut args=FluentArgs::new();args.set(argument,literal);session.localization().format(id,&args)}else{session.localization().text(id).to_string()};
                assert_eq!(session.state.host_error.as_deref(),Some(expected.as_str()));
                assert_eq!(session.files.host_error_copy,Some(reason.clone()));
                assert_eq!(session.engine.checkpoint(),checkpoint);assert_eq!(session.engine.document(),&document);
                assert_eq!((session.engine.backend().composites,session.engine.backend().dabs,session.engine.backend().snapshot_requests.len()),rendering);
                assert!(!session.state.requests.iter().any(|pending|pending.id==request));
            }
        }
        let diagnostic="{\"document_host_error\":{\"type\":\"transport\",\"reason\":\"switch_dialog\"}} literal 雪";
        session.set_host_error(Some(diagnostic.into()));
        for language in UiLanguage::ALL {session.set_localization(Localizer::shared(language));assert_eq!(session.state.host_error.as_deref(),Some(diagnostic));assert!(session.files.host_error_copy.is_none());}
    }

    #[test]
    fn completed_typed_host_failure_refreshes_and_literal_replacement_clears_its_source() {
        let english=Localizer::shared(UiLanguage::English);
        let japanese=Localizer::shared(UiLanguage::Japanese);
        let mut session=UiSession::blank_localized(Recorder::default(), [256,256], Platform::Windows, english.clone()).unwrap();
        session.request_document(DocumentRequest::Open).unwrap();
        let id=session.files.pending.as_ref().unwrap().0;
        let checkpoint=session.engine.checkpoint();let epoch=session.state.document_file.epoch;
        let reason=DocumentHostErrorCopy::Profile(crate::ColorFeatureError::SelectImportedProfile);
        session.complete_document_request_failed(id,reason.clone()).unwrap();
        session.set_localization(japanese.clone());
        assert_eq!(session.state.host_error,Some(reason.message(&japanese)));
        assert_eq!(session.engine.checkpoint(),checkpoint);assert_eq!(session.state.document_file.epoch,epoch);
        assert!(!session.state.requests.iter().any(|request| request.id==id));
        session.set_host_error(Some("literal diagnostic { $name }".into()));
        session.set_localization(english);
        assert_eq!(session.state.host_error.as_deref(),Some("literal diagnostic { $name }"));
    }
    #[test]
    fn document_language_refresh_retains_request_and_saved_name_provenance() {
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let english = Localizer::shared(UiLanguage::English);
        let mut session = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk, english.clone()).unwrap();
        session.request_save(false).unwrap();
        let id = session.files.pending.as_ref().unwrap().0;
        let epoch = session.state.document_file.epoch;
        let checkpoint = session.engine.checkpoint();
        assert!(session.set_localization(japanese.clone()));
        assert_eq!(session.state.document_file.title(), "無題");
        assert_eq!(session.state.document_file.epoch, epoch);
        assert_eq!(session.engine.checkpoint(), checkpoint);
        assert!(matches!(session.document_request(id).unwrap(), DocumentRequest::Save { name, .. } if name == "無題.capy"));
        session.complete_document_request(id, Ok(false)).unwrap();
        session.state.document_file.unsaved_name = Some("Untitled".into());
        layer(&mut session, LayerAction::New { group: false, clipped: false });
        session.request_document_close().unwrap();
        let id = session.files.pending.as_ref().unwrap().0;
        assert!(session.set_localization(english.clone()));
        let mut args = FluentArgs::new(); args.set("name", "Untitled");
        let expected = english.format(MessageId::DOCUMENTS_CLOSE_CONFIRM, &args);
        assert!(matches!(session.document_request(id).unwrap(), DocumentRequest::ConfirmClose { title } if title == &expected));
        assert_eq!(session.state.document_file.title(), "Untitled");
        session.respond_document_close(id, CloseDecision::Cancel).unwrap();
        session.request_save(false).unwrap();
        let id = session.files.pending.as_ref().unwrap().0;
        assert!(session.set_localization(japanese));
        assert!(matches!(session.document_request(id).unwrap(), DocumentRequest::Save { name, .. } if name == "Untitled.capy"));
    }

    #[test]
    fn untitled_is_presentation_and_literal_close_names_are_whole_message_arguments() {
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut session = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk, japanese.clone()).unwrap();
        assert_eq!(session.state().document_file.title(), "無題");
        assert!(session.state().document_file.unsaved_name.is_none());
        assert!(!session.state().document_file.modified);
        let name = "Untitled { $name }「絵」🖌️\u{2068}literal\u{2069}.capy";
        session.initialize_document_location(Some(DocumentLocation { uri: "private:literal".into(), name: name.into() })).unwrap();
        layer(&mut session, LayerAction::New { group: false, clipped: false });
        session.request_document_close().unwrap();
        let HostRequestKind::Document { request: DocumentRequest::ConfirmClose { title } } = &session.state().requests.last().unwrap().kind else { panic!("close request"); };
        assert_eq!(title, &format!("「{name}」の変更を保存しますか？"));
        assert_eq!(session.state().document_file.title(), name);
        let snapshot = serde_json::to_value(&session.state().document_file).unwrap();
        assert!(snapshot.get("untitled").is_none());
        assert_eq!(snapshot["location"]["name"], name);
    }
}


#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::session::test_support::{session, invoke, Recorder};

    #[test]
    fn failed_renderer_source_only_save_acknowledges_the_durable_capture() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::AddLayer);
        session.frame(0, 0).unwrap();
        session.suspend_renderer().unwrap();
        invoke(&mut session, CommandId::SaveDocument);
        let request = session.files.pending.as_ref().unwrap().0;
        let location = DocumentLocation { uri: "private:source-only".into(), name: "source-only.capy".into() };
        let capture = session.capture_project_save(request, location.clone()).unwrap();
        assert_eq!(capture.checkpoint.owner, session.engine.document().owner);
        assert_eq!(capture.checkpoint.edit_checkpoint, session.engine.checkpoint());
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let package = layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancelled).unwrap();
        assert_eq!(package.preview_status, layer_core::package::codec::PreviewStatus::Unavailable);
        assert!(session.state.document_file.modified);
        let mut bytes = Vec::new(); package.write(&mut bytes, &cancelled).unwrap();
        assert!(!bytes.is_empty());
        session.complete_document_request(request, Ok(true)).unwrap();
        assert!(!session.state.document_file.modified);
        assert_eq!(session.state.document_file.location, Some(location));
    }

    #[test]
    fn export_and_workflow_capture_actual_phases_without_authoring_them() {
        let mut session = session(Platform::Gtk);
        crate::session::test_support::insert_effect(&mut session, "unsharp_mask");
        session.frame(0, 0).unwrap();
        let occurrence = session.engine.document().working.occurrence.unwrap();
        let effect = session.engine.document().scene().effect_handle(occurrence).unwrap();
        let authored = session.engine.document().output().context.clone();
        let checkpoint = session.engine.checkpoint();
        let context = layer_core::EvaluationContext { elapsed: 3.5, phases: vec![(effect, 0.75)].into() };
        session.renderer_mut().evaluation = context.clone();
        session.engine.render_frame_at(1_000_000_000).unwrap();
        session.engine.render_frame_at(4_500_000_000).unwrap();
        invoke(&mut session, CommandId::ExportDocument);
        let request = session.files.pending.as_ref().unwrap().0;
        let export = session.capture_project_export(request).unwrap();
        assert_eq!(export.output().context, context);
        assert_eq!(export.time, 3.5);
        assert_eq!(session.engine.document().output().context, authored);
        assert_eq!(session.engine.checkpoint(), checkpoint);
        session.complete_document_request(request, Ok(false)).unwrap();
        invoke(&mut session, CommandId::AssignProfile);
        let request = session.files.pending.as_ref().unwrap().0;
        let workflow = crate::ColorWorkflow::begin(&session, request).unwrap();
        assert_eq!(workflow.context, context);
        assert_eq!(workflow.original.output().context, authored);
        session.renderer_mut().evaluation.elapsed = 9.;
        assert_eq!(workflow.context.elapsed, 3.5);
        assert_eq!(session.engine.document().output().context, authored);
        assert_eq!(session.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn fresh_editable_open_initializes_working_target_without_resetting_parked_state() {
        let initial = layer_core::Document::new(layer_core::PortableId::random(), 32, 24, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let neutral = layer_core::Document::from_artwork(initial.artwork.clone()).unwrap();
        assert!(neutral.working.occurrence.is_none());
        let opened = UiSession::from_project(Recorder::default(), neutral.clone(), None, [128, 128], Platform::Gtk).unwrap();
        assert_eq!(opened.engine.document().working.occurrence, initial.working.occurrence);
        assert_eq!(opened.engine.document().working.target, initial.working.target);
        assert_eq!(opened.engine.document().working.layer_selection, initial.working.layer_selection);
        assert_eq!(opened.engine.document().working.layer_anchor, initial.working.layer_anchor);
        assert_eq!(opened.state.layers.iter().filter(|row| row.selected).count(), 1);
        assert_eq!(opened.engine.document().working.generation, 0);
        assert_eq!(opened.engine.checkpoint(), 0);
        crate::session::test_support::assert_live_artwork_eq(opened.engine.document(), &neutral);
        let mut parked = initial;
        parked.working.generation = 1;
        parked.working.occurrence = None;
        parked.working.target = None;
        parked.working.selection = Some(crate::session::test_support::rectangle([1., 2., 6., 8.]));
        let working = parked.working.clone();
        let restored = UiSession::from_project(Recorder::default(), parked.clone(), None, [128, 128], Platform::Gtk).unwrap();
        assert_eq!(restored.engine.document().working, working);
        assert_eq!(restored.engine.checkpoint(), 0);
        crate::session::test_support::assert_live_artwork_eq(restored.engine.document(), &parked);
        let mut unchecked = opened.engine.document().clone();
        unchecked.working.layer_selection.clear();
        unchecked.working.generation = 1;
        let working = unchecked.working.clone();
        let restored = UiSession::from_project(Recorder::default(), unchecked, None, [128, 128], Platform::Gtk).unwrap();
        assert_eq!(restored.engine.document().working, working);
        assert!(restored.state.layers.iter().all(|row| !row.selected));
    }

    #[test]
    fn foreign_capture_completion_cannot_acknowledge_a_save() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::AddLayer);
        invoke(&mut session, CommandId::SaveDocument);
        let request = session.files.pending.as_ref().unwrap().0;
        session.capture_project_save(request, DocumentLocation { uri: "private:foreign".into(), name: "foreign.capy".into() }).unwrap();
        let saved = session.files.saved_checkpoint;
        session.files.pending.as_mut().unwrap().1.as_mut().unwrap().0.owner += 1;
        assert!(session.complete_document_request(request, Ok(true)).is_err());
        assert_eq!(session.files.saved_checkpoint, saved);
        assert!(session.state.document_file.modified);
        assert!(session.state.document_file.location.is_none());
        assert_eq!(session.files.pending.as_ref().unwrap().0, request);
        session.complete_document_request(request, Ok(false)).unwrap();
    }
}

#[cfg(test)]
mod export_again_tests {
    use super::*;
    use crate::session::test_support::{Recorder, layer};

    fn session() -> UiSession<Recorder> {
        UiSession::blank(Recorder::default(), [256,256], Platform::Gtk).unwrap()
    }
    fn location(name: &str) -> DocumentLocation {
        DocumentLocation { uri: format!("private:export:{name}"), name: name.into() }
    }
    fn request(session: &mut UiSession<Recorder>, again: bool) -> (u32,u64) {
        session.dispatch(UiAction::Invoke { command: if again { CommandId::ExportAgain } else { CommandId::ExportDocument } }).unwrap();
        let id=session.files.pending.as_ref().unwrap().0;
        let DocumentRequest::Export {owner,..}=session.document_request(id).unwrap() else {panic!("export request")};
        (id,*owner)
    }
    fn prepare(session: &mut UiSession<Recorder>, id:u32, owner:u64, destination:DocumentLocation) {
        session.dispatch(UiAction::PrepareExport {id,owner,recipe:ExportRecipe::web_share(),location:destination}).unwrap();
    }
    fn publish(session:&mut UiSession<Recorder>, destination:DocumentLocation) {
        let (id,owner)=request(session,false);
        prepare(session,id,owner,destination);
        session.complete_document_request(id,Ok(true)).unwrap();
    }

    #[test]
    fn export_again_remembers_only_success_and_never_acknowledges_saved_artwork() {
        let mut app=session();
        layer(&mut app,LayerAction::New {group:false,clipped:false});
        let checkpoint=app.engine.checkpoint();let saved=app.files.saved_checkpoint;
        let (id,owner)=request(&mut app,false);
        prepare(&mut app,id,owner,location("first.png"));
        assert!(app.files.last_export.is_none());assert!(app.state.document_file.export_uri.is_none());
        app.complete_document_request(id,Ok(true)).unwrap();
        let remembered=app.files.last_export.clone().unwrap();
        assert_eq!(remembered.location,location("first.png"));
        for result in [Ok(false),Err("private write failed".into())] {
            let (id,owner)=request(&mut app,false);
            prepare(&mut app,id,owner,location("cancelled.png"));
            app.complete_document_request(id,result).unwrap();
            assert_eq!(app.files.last_export.as_ref(),Some(&remembered));
            assert_eq!(app.state.document_file.export_uri.as_deref(),Some("private:export:first.png"));
        }
        assert_eq!(app.engine.checkpoint(),checkpoint);assert_eq!(app.files.saved_checkpoint,saved);
        assert!(app.state.document_file.modified);
    }

    #[test]
    fn export_again_captures_fresh_artwork_with_the_concrete_recipe_and_target() {
        let mut app=session();
        let (id,owner)=request(&mut app,false);
        let original=app.capture_project_export(id).unwrap();
        prepare(&mut app,id,owner,location("drawing.png"));app.complete_document_request(id,Ok(true)).unwrap();
        layer(&mut app,LayerAction::New {group:false,clipped:false});
        let (id,_)=request(&mut app,true);
        let DocumentRequest::Export {repeat:Some(repeat),name,..}=app.document_request(id).unwrap() else {panic!("repeat")};
        assert_eq!(repeat.recipe,ExportRecipe::web_share());assert_eq!(repeat.location,location("drawing.png"));assert_eq!(name,"drawing.png");
        let fresh=app.capture_project_export(id).unwrap();
        assert_ne!(fresh.capture.checkpoint.edit_checkpoint,original.capture.checkpoint.edit_checkpoint);
        assert_ne!(fresh.capture.artwork.occurrences.len(),original.capture.artwork.occurrences.len());
        app.complete_document_request(id,Ok(false)).unwrap();
        assert_eq!(app.files.last_export.as_ref().unwrap().location,location("drawing.png"));
    }

    #[test]
    fn export_again_rejects_stale_owner_and_activation_without_replacing_success() {
        for prepared in [false,true] {
            let mut app=session();publish(&mut app,location("accepted.png"));
            let remembered=app.files.last_export.clone();
            let (id,owner)=request(&mut app,false);
            assert!(app.prepare_export(id,owner.wrapping_add(1),ExportRecipe::web_share(),location("wrong.png")).is_err());
            if prepared {prepare(&mut app,id,owner,location("stale.png"));}
            let other=session();app.inherit_window_state(&other).unwrap();
            if prepared {assert!(app.complete_document_request(id,Ok(true)).is_err());}
            else {assert!(app.prepare_export(id,owner,ExportRecipe::web_share(),location("stale.png")).is_err());}
            assert_eq!(app.files.last_export,remembered);assert_eq!(app.files.pending.as_ref().unwrap().0,id);
        }
    }

    #[test]
    fn export_again_memory_belongs_to_the_drawing_across_parking_and_renderer_replacement() {
        let mut first=session();let mut second=session();
        publish(&mut first,location("first.png"));publish(&mut second,location("second.png"));
        first.frame(0,0).unwrap();first.park_document().unwrap();
        first.inherit_window_state(&second).unwrap();first.replace_renderer(Recorder::default()).unwrap();
        assert_eq!(first.files.last_export.as_ref().unwrap().location,location("first.png"));
        assert_eq!(second.files.last_export.as_ref().unwrap().location,location("second.png"));
        let fresh=UiSession::new(Recorder::default(),first.engine.document().clone(),[256,256],Platform::Gtk).unwrap();
        assert!(fresh.files.last_export.is_none());assert!(fresh.state.document_file.export_uri.is_none());
    }

    #[test]
    fn export_again_cannot_publish_over_the_editable_master() {
        let mut app=session();let master=location("master.capy");
        app.initialize_document_location(Some(master.clone())).unwrap();
        let (id,owner)=request(&mut app,false);
        assert!(app.prepare_export(id,owner,ExportRecipe::web_share(),master).is_err());
        assert!(app.files.pending_export.is_none());assert!(app.files.last_export.is_none());
        app.complete_document_request(id,Ok(false)).unwrap();
    }
}
