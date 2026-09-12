//! Document lifecycle policy. Hosts provide dialogs and asynchronous transport;
//! checkpoints, cancellation and close-after-save decisions remain shared.
use super::*;
use layer_core::{AssetId, Project, ProjectAsset};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentLocation {
    /// Opaque host handle/URI. Never serialized into the project itself.
    pub uri: String,
    pub name: String,
}
impl DocumentLocation {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.uri.is_empty()
            || self.uri.len() > 16_384
            || self.uri.contains('\0')
            || self.name.is_empty()
            || self.name.len() > 1024
            || self.name.chars().any(char::is_control)
        {
            Err("Invalid document location".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DocumentFileState {
    pub epoch: u64,
    pub revision: u64,
    pub location: Option<DocumentLocation>,
    pub modified: bool,
    pub busy: bool,
    /// The host closes only after this authorization, not on an initial request.
    pub close_ready: bool,
}
impl DocumentFileState {
    pub fn title(&self) -> &str {
        self.location
            .as_ref()
            .map_or("Untitled", |location| location.name.as_str())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentRequest {
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
    pub fn title(&self) -> &str {
        match self {
            Self::New => "New drawing",
            Self::Open => "Open drawing",
            Self::Save { .. } => "Save drawing",
            Self::Export { .. } => "Export PNG",
            Self::ConfirmClose { title } => title,
        }
    }
    pub fn accept_label(&self) -> &'static str {
        match self {
            Self::New => "Create",
            Self::Open => "Open",
            Self::Export { .. } => "Export",
            _ => "Save",
        }
    }
    pub fn filter(&self) -> (&'static str, &'static str) {
        match self {
            Self::Export { .. } => ("PNG image", "png"),
            _ => ("Capy Canvas drawing", "capy"),
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

pub const UNSAVED_DESCRIPTION: &str = "Changes will be lost if you close without saving.";
pub const DISCARD_DOCUMENT_LABEL: &str = "Discard Changes";
pub const CANCEL_DOCUMENT_LABEL: &str = "Cancel";
pub const DOCUMENT_WIDTH_LABEL: &str = "Width (px)";
pub const DOCUMENT_HEIGHT_LABEL: &str = "Height (px)";
pub const DEFAULT_DOCUMENT_EXTENT: [u32; 2] = [2048, 1536];
pub const MAX_NEW_DOCUMENT_DIMENSION: u32 = 8192;

#[derive(Clone, Debug, Serialize)]
pub struct NewDocumentSpec {
    pub title: &'static str,
    pub labels: [&'static str; 2],
    pub extent: [u32; 2],
    pub numeric: NumericControl,
    pub minimum: u32,
    pub maximum: u32,
    pub accept: &'static str,
    pub cancel: &'static str,
}
pub fn new_document_spec() -> NewDocumentSpec {
    NewDocumentSpec {
        title: "New drawing",
        labels: [DOCUMENT_WIDTH_LABEL, DOCUMENT_HEIGHT_LABEL],
        extent: DEFAULT_DOCUMENT_EXTENT,
        numeric: NumericControl {
            kind: NumericKind::Number,
            ..NumericControl::number(1., f64::from(MAX_NEW_DOCUMENT_DIMENSION), 1., 0)
        },
        minimum: 1,
        maximum: MAX_NEW_DOCUMENT_DIMENSION,
        accept: "Create",
        cancel: CANCEL_DOCUMENT_LABEL,
    }
}

/// Shared new-document constraints; creation is a host operation so native
/// windows and future tabbed/mobile hosts can use different presentation.
pub fn new_drawing(width: u32, height: u32) -> Result<Project, String> {
    if width == 0
        || height == 0
        || width > MAX_NEW_DOCUMENT_DIMENSION
        || height > MAX_NEW_DOCUMENT_DIMENSION
    {
        return Err("Choose a canvas size from 1 to 8192 pixels".into());
    }
    Ok(Project {
        document: Document::new("untitled", width, height),
        assets: BTreeMap::new(),
    })
}

#[derive(Default)]
pub(super) struct DocumentFiles {
    pub assets: BTreeMap<AssetId, ProjectAsset>,
    pub(super) saved_checkpoint: u64,
    pub(super) recovered: bool,
    replace_in_place: bool,
    replace_after: Option<bool>,
    pub(super) pending: Option<(u32, Option<(u64, DocumentLocation)>)>,
    close_after: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Hosts validate/decode projects off-thread first. A fresh session avoids
    /// replacing a working document or colliding with its live GPU resources.
    pub fn from_project(
        renderer: R,
        project: Project,
        location: Option<DocumentLocation>,
        viewport: [u32; 2],
    ) -> Result<Self, String> {
        if let Some(location) = &location {
            location.validate()?;
        }
        let mut session = Self::new(renderer, project.document, viewport)?;
        for (id, asset) in &project.assets {
            session
                .renderer_mut()
                .prepare_owned_asset(id, asset)
                .map_err(error)?;
        }
        session.files.assets = project.assets;
        session.state.document_file.location = location;
        session.refresh_document();
        Ok(session)
    }

    pub(super) fn refresh_file_state(&mut self) {
        self.state.document_file.revision = self.engine.document().revision;
        self.state.document_file.modified =
            self.files.recovered || self.engine.checkpoint() != self.files.saved_checkpoint;
        self.state.document_file.busy = self.files.pending.is_some();
    }

    pub(super) fn request_document(&mut self, request: DocumentRequest) -> Result<(), String> {
        if matches!(
            request,
            DocumentRequest::ConfirmClose { .. } | DocumentRequest::Save { .. }
        ) {
            self.require_document_snapshot_idle()?;
        } else {
            self.require_document_idle()?;
        }
        if self.files.pending.is_some() {
            return Err("A file operation is already in progress".into());
        }
        let id = self.next_request;
        self.request(HostRequestKind::Document { request })?;
        self.state.host_error = None;
        self.files.pending = Some((id, None));
        self.refresh_file_state();
        Ok(())
    }

    fn document_request(&self, id: u32) -> Result<&DocumentRequest, String> {
        if self.files.pending.as_ref().map(|p| p.0) != Some(id) {
            return Err("Unknown document request".into());
        }
        self.state
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if r.id == id => Some(request),
                _ => None,
            })
            .ok_or("Unknown document request".into())
    }

    pub fn set_document_replacement(&mut self, enabled: bool) {
        self.files.replace_in_place = enabled;
    }
    pub(super) fn request_document_open(&mut self, opening: bool) -> Result<(), String> {
        if self.files.replace_in_place {
            self.require_document_idle()?;
            if self.files.pending.is_some() {
                return Err("A file operation is already in progress".into());
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
        location.validate()?;
        self.document_request(id)?;
        let (_, snapshot) = self.files.pending.as_mut().unwrap();
        snapshot.as_mut().ok_or("Save has not been captured")?.1 = location;
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

    /// Capture only immutable source state. The host prunes, validates and
    /// compresses this on a worker. No GPU readback or full image copy.
    pub fn capture_project_recovery(&self) -> Result<Project, String> {
        self.require_document_snapshot_idle()?;
        Ok(Project {
            document: self.engine.document().clone(),
            assets: self.files.assets.clone(),
        })
    }

    /// A manual save additionally reserves the checkpoint to acknowledge once
    /// the destination is durable. Recovery never changes that checkpoint.
    pub fn capture_project_save(
        &mut self,
        id: u32,
        location: DocumentLocation,
    ) -> Result<Project, String> {
        self.require_document_snapshot_idle()?;
        location.validate()?;
        if !matches!(self.document_request(id)?, DocumentRequest::Save { .. })
            || self.files.pending.as_ref().is_some_and(|p| p.1.is_some())
        {
            return Err("This save was already captured or is not a save request".into());
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
            return Err("Respond to the unsaved changes dialog".into());
        }
        let save = matches!(request, DocumentRequest::Save { .. });
        if save && result == Ok(true) && self.files.pending.as_ref().unwrap().1.is_none() {
            return Err("No project snapshot was saved".into());
        }
        let (_, snapshot) = self.files.pending.take().unwrap();
        self.state.requests.retain(|r| r.id != id);
        let success = result == Ok(true);
        self.state.host_error = result.err();
        if save
            && success
            && let Some((checkpoint, location)) = snapshot
        {
            self.files.saved_checkpoint = checkpoint;
            self.files.recovered = false;
            self.state.document_file.location = Some(location);
        }
        self.refresh_file_state();
        self.files.close_after &= success;
        if !success {
            self.files.replace_after = None;
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
                self.state.host_error = Some(error);
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
                title: format!("Save changes to “{}”?", self.state.document_file.title()),
            })?;
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
            return Err("Not an unsaved changes request".into());
        }
        self.files.pending = None;
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

    fn require_document_interaction_idle(&self) -> Result<(), String> {
        self.require_idle()?;
        if self.operation.active() || self.region_tools.busy() {
            Err("Finish the current canvas operation first".into())
        } else {
            Ok(())
        }
    }

    /// Library-only validation owns private GPU work and cannot change document
    /// or workspace values. Closing and immutable saves may proceed, including
    /// their unsaved-changes decisions. Replacing the document still waits.
    pub fn require_document_snapshot_idle(&self) -> Result<(), String> {
        self.require_document_interaction_idle()?;
        if self
            .pending_filters
            .as_ref()
            .is_some_and(|p| !p.library_only())
        {
            Err("Finish the current canvas operation first".into())
        } else {
            Ok(())
        }
    }

    pub fn require_document_idle(&self) -> Result<(), String> {
        self.require_document_interaction_idle()?;
        if self.pending_filters.is_some() {
            Err("Finish the current canvas operation first".into())
        } else {
            Ok(())
        }
    }
}
