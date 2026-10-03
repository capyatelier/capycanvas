//! Document lifecycle policy. Hosts provide dialogs and asynchronous transport;
//! checkpoints, cancellation and close-after-save decisions remain shared.
use super::*;
use layer_core::Project;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentHostErrorCopy { Color(crate::ColorFeatureError), Profile(crate::ColorFeatureError), Preset(crate::ColorFeatureError) }
impl DocumentHostErrorCopy {
    pub fn message(&self, localization: &Localizer) -> String {
        let reason=match self { Self::Color(reason)|Self::Profile(reason)|Self::Preset(reason)=>reason };
        if let crate::ColorFeatureError::Diagnostic(detail)=reason { return detail.clone(); }
        match self { Self::Color(reason)=>reason.message(localization),Self::Profile(reason)=>reason.profile_message(localization),Self::Preset(reason)=>reason.preset_message(localization) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentHostError {
    NewWindowUnavailable, DrawingTabsUnavailable, ChooseDeviceFile, ChooseFilename, ProjectWriterFailed, NoDrawingToOpen,
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
    /// Suggested master name for an opened photo; never a save destination.
    pub unsaved_name: Option<String>,
    pub modified: bool,
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
        Self { epoch: 0, revision: 0, location: None, unsaved_name: None, modified: false,
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentRequest {
    ChangeColor { operation: DocumentColorOperation },
    ColorHistory { redo: bool },
    Place,
    Paste { mode: PasteMode },
    /// Copy the active layer's pixels, or with `merged` the visible image;
    /// `cut` erases them once the host reports the copy complete.
    Copy { merged: bool, cut: bool },
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
    },
    ConfirmClose {
        title: String,
    },
}
impl DocumentRequest {
    pub fn title(&self, localization: &Localizer) -> std::sync::Arc<str> {
        let id = match self {
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
            Self::Place => MessageId::DOCUMENTS_IMPORT_ACCEPT,
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
pub fn new_drawing(width: u32, height: u32, localization: &Localizer) -> Result<Project, String> {
    NewDocumentOptions {
        extent: [width, height],
        ..Default::default()
    }
    .project(localization)
}

#[derive(Clone)]
pub struct DocumentExport {
    pub project: Project,
    pub background: [f32; 4],
    pub time: f32,
}

enum DocumentRequestCopy {
    Filename(&'static str),
    Close(Option<String>),
}

#[derive(Default)]
pub(super) struct DocumentFiles {
    pub(super) saved_checkpoint: u64,
    pub(super) unpublished: bool,
    pub(super) pending_modified_change: bool,
    replace_in_place: bool,
    replace_after: Option<bool>,
    pub(super) pending: Option<(u32, Option<(u64, DocumentLocation)>)>,
    close_after: bool,
    pending_copy: Option<DocumentRequestCopy>,
    pub(super) cut: Option<clipboard::PendingCut>,
    host_error_copy: Option<DocumentHostErrorCopy>,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_host_error(&mut self, error: Option<String>) {
        self.state.host_error=error;self.files.host_error_copy=None;
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
                HostRequestKind::Document { request: DocumentRequest::Save { name, .. } | DocumentRequest::Export { name } } => *name = text,
                HostRequestKind::Document { request: DocumentRequest::ConfirmClose { title } } => *title = text,
                _ => {},
            }
        }
    }

    pub(super) fn numbered_document_name(&self, id: MessageId, number: u64) -> String {
        let mut args = FluentArgs::new();
        args.set("number", number.to_string());
        self.localization().format(id, &args)
    }
    /// Hosts validate/decode projects off-thread first. A fresh session avoids
    /// replacing a working document or colliding with its live GPU resources.
    pub fn from_project(
        renderer: R,
        project: Project,
        location: Option<DocumentLocation>,
        viewport: [u32; 2],
        platform: Platform,
    ) -> Result<Self, String> {
        Self::from_project_localized(renderer, project, location, viewport, platform, Localizer::shared(UiLanguage::English))
    }

    pub fn from_project_localized(
        renderer: R,
        project: Project,
        location: Option<DocumentLocation>,
        viewport: [u32; 2],
        platform: Platform,
        localization: std::sync::Arc<Localizer>,
    ) -> Result<Self, String> {
        if let Some(location) = &location {
            location.validate().map_err(|error| error.message(&localization))?;
        }
        let photo_name = location
            .is_none()
            .then(|| {
                project
                    .document
                    .layers
                    .iter()
                    .find(|l| l.source.is_some())
                    .map(|l| l.name.to_string())
            })
            .flatten();
        let mut session = Self::new_localized(renderer, project.document, viewport, platform, localization)?;
        session.state.document_file.location = location;
        if let Some(name) = photo_name {
            session.state.document_file.unsaved_name = Some(name);
            session.files.unpublished = true; // Imported content needs its own master save.
        }
        session.refresh_document();
        Ok(session)
    }

    /// A recovered private checkpoint still needs an explicit user save.
    pub fn mark_recovered(&mut self) {
        self.files.unpublished = true;
        self.state.document_file.location = None;
        self.refresh_document();
        self.refresh_commands();
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
                DocumentRequest::Export { .. } => Some(DocumentRequestCopy::Filename("png")),
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

    /// Capture the last committed raster boundary, including while drawing.
    /// Pending tile backing is awaited by the file worker. Active contact pixels
    /// are excluded; recovery never acknowledges a manual save checkpoint.
    pub fn capture_project_recovery(&self) -> Result<Project, String> {
        self.require_raster_snapshot()?;
        Ok(Project {
            document: self.engine.document().clone(),
        })
    }

    /// Freeze the committed master and its rendering coordinates for an output
    /// worker. This never reserves or acknowledges a saved-project checkpoint.
    pub fn capture_project_export(&self, id: u32) -> Result<DocumentExport, String> {
        if !matches!(self.document_request(id)?, DocumentRequest::Export { .. }) {
            return Err(FileFailure::NotExportRequest.message(self.localization()));
        }
        self.require_document_idle()?;
        Ok(DocumentExport {
            project: self.capture_project_recovery()?,
            background: self.engine.view().background_rgba_linear,
            time: self.engine.animation_time(),
        })
    }

    /// A manual save additionally reserves the checkpoint to acknowledge once
    /// the destination is durable. Recovery never changes that checkpoint.
    pub fn capture_project_save(
        &mut self,
        id: u32,
        location: DocumentLocation,
    ) -> Result<Project, String> {
        self.require_raster_snapshot()?;
        location.validate().map_err(|error| error.message(self.localization()))?;
        if !matches!(self.document_request(id)?, DocumentRequest::Save { .. })
            || self.files.pending.as_ref().is_some_and(|p| p.1.is_some())
        {
            return Err(FileFailure::InvalidSaveRequest.message(self.localization()));
        }
        self.files.pending.as_mut().unwrap().1 = Some((self.engine.checkpoint(), location));
        self.capture_project_recovery()
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
        let cutting = matches!(request, DocumentRequest::Copy { cut: true, .. });
        if save && result == Ok(true) && self.files.pending.as_ref().unwrap().1.is_none() {
            return Err(FileFailure::NoSavedSnapshot.message(self.localization()));
        }
        let (_, snapshot) = self.files.pending.take().unwrap();
        self.files.pending_copy = None;
        self.state.requests.retain(|r| r.id != id);
        let success = result == Ok(true);
        let cut = self.files.cut.take().filter(|_| cutting && success);
        self.set_host_error(result.err());
        if save
            && success
            && let Some((checkpoint, location)) = snapshot
        {
            self.files.saved_checkpoint = checkpoint;
            self.files.unpublished = false;
            self.state.document_file.location = Some(location);
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
            self.files.close_after,
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

    pub fn request_document_close(&mut self) -> Result<UiChange, String> {
        self.require_document_snapshot_idle()?;
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

    pub(crate) fn require_raster_snapshot(&self) -> Result<(), String> {
        if self.painted_selections.busy() { return Err(FileFailure::SelectionCapture.message(self.localization())); }
        if self.sdr_gesture.is_some()
            || self.targeted_curve_busy() || self.auto_levels.is_some()
            || self.eyedropper.calibration.as_ref().is_some_and(|calibration| calibration.request.is_some())
            || self.operation.active()
            || (self.region_tools.busy() && !self.refine_previewing())
            || !self.layer_interaction.path.is_empty()
            || self
                .pending_filters
                .as_ref()
                .is_some_and(|p| !p.library_only())
        {
            Err(FileFailure::CanvasOperation.message(self.localization()))
        } else {
            Ok(())
        }
    }

    fn require_document_interaction_idle(&self) -> Result<(), String> {
        self.require_idle()?;
        if self.content_bounds.baking() {
            Err(self.localization().text(MessageId::COMMANDS_WAIT_FOR_TRANSFORM).to_string())
        } else if self.operation.active() {
            Err(self.operation_refusal().to_string())
        } else if (self.region_tools.busy() && !self.refine_previewing())
            || self.targeted_curve_busy() || self.auto_levels.is_some()
            || self.eyedropper.calibration.as_ref().is_some_and(|calibration| calibration.request.is_some()) {
            Err(FileFailure::CanvasOperation.message(self.localization()))
        } else {
            Ok(())
        }
    }

    /// Library-only validation owns private GPU work and cannot change document
    /// or workspace values. Closing and immutable saves may proceed, including
    /// their unsaved-changes decisions. Replacing the document still waits.
    pub fn require_document_snapshot_idle(&self) -> Result<(), String> {
        if self.painted_selections.busy() { return Err(FileFailure::SelectionCapture.message(self.localization())); }
        self.require_document_interaction_idle()?;
        if self
            .pending_filters
            .as_ref()
            .is_some_and(|p| !p.library_only())
        {
            Err(FileFailure::CanvasOperation.message(self.localization()))
        } else {
            Ok(())
        }
    }

    pub fn require_document_idle(&self) -> Result<(), String> {
        self.require_document_interaction_idle()?;
        if self.pending_filters.is_some() {
            Err(FileFailure::CanvasOperation.message(self.localization()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::{Recorder, layer};

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
