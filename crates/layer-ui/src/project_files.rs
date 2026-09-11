//! Shared document service policy. Hosts own URLs, dialogs and background I/O;
//! these tokens prevent a late save/open from replacing newer document state.
use super::*;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProjectFileState {
    pub title: String,
    pub epoch: u64,
    pub revision: u64,
    pub saved_revision: u64,
    pub modified: bool,
    pub supported: bool,
}
impl Default for ProjectFileState {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            epoch: 1,
            revision: 0,
            saved_revision: 0,
            modified: false,
            supported: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFileAction {
    New,
    Open,
    Save,
    SaveAs,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_project_files_available(&mut self, available: bool) {
        self.state.project_file.supported = available;
        self.refresh_commands();
    }
    pub fn project_ready(&self) -> Result<(), String> {
        self.require_idle()?;
        if self.operation.active() || self.region_tools.busy() || self.pending_filters.is_some() {
            return Err("Finish or cancel the current operation first".into());
        }
        Ok(())
    }
    /// Metadata-only capture; finish/validate/serialize it on a worker.
    pub fn project_snapshot(&self) -> Result<layer_core::ProjectSnapshot, String> {
        self.project_ready()?;
        layer_core::ProjectSnapshot::capture(self.engine.document(), |id| {
            self.engine.backend().source_asset(id)
        })
    }
    pub fn project_saved(&mut self, epoch: u64, revision: u64, title: &str) -> Result<(), String> {
        if epoch != self.state.project_file.epoch || revision > self.engine.document().revision {
            return Err("The save belongs to a different document".into());
        }
        let title = project_title(title)?;
        self.state.project_file.title = title;
        self.state.project_file.saved_revision = revision;
        self.refresh_document();
        self.changed(regions::DOCUMENT, false);
        Ok(())
    }
    /// Adopt a fully prepared candidate. Retain settings/workspace/brushes in
    /// this window, reset document gestures/caches and start fresh undo history.
    /// Return the retired session so a host can release it off the input queue.
    pub fn adopt_project(
        &mut self,
        mut candidate: Self,
        epoch: u64,
        revision: u64,
        title: &str,
    ) -> Result<Self, (String, Self)> {
        let checked = (|| {
            self.project_ready()?;
            if epoch != self.state.project_file.epoch || revision != self.engine.document().revision
            {
                return Err(
                    "The document changed while opening; review those changes first".into(),
                );
            }
            let title = project_title(title)?;
            let next = epoch
                .checked_add(1)
                .ok_or("Document generation exhausted")?;
            candidate
                .engine
                .set_brush(self.engine.configured_brush().clone())
                .map_err(error)?;
            candidate.apply_settings(self.state.settings.clone())?;
            Ok((title, next))
        })();
        let (title, next) = match checked {
            Ok(v) => v,
            Err(e) => return Err((e, candidate)),
        };
        std::mem::swap(&mut self.engine, &mut candidate.engine);
        std::mem::swap(&mut self.pen, &mut candidate.pen);
        self.input_pending = false;
        self.touch.clear();
        self.navigator_drag = None;
        self.navigator_preview = Default::default();
        self.eyedropper = Default::default();
        self.region_tools = Default::default();
        self.operation = Default::default();
        self.rulers.selected = None;
        self.layer_interaction = Default::default();
        self.cursor = Default::default();
        // Keep the latest host viewport and monotonic input revision, including
        // any resize that occurred while the candidate was being prepared.
        self.state.camera.flipped = [false; 2];
        self.initial_fit = false;
        self.sync_work_area();
        let d = self.engine.document();
        self.state.camera.fit([d.width, d.height]);
        let revision = d.revision;
        self.state.project_file = ProjectFileState {
            title,
            epoch: next,
            revision,
            saved_revision: revision,
            modified: false,
            supported: self.state.project_file.supported,
        };
        self.engine.start_document_view(
            self.state.camera.view(),
            self.state.camera.input_transform(),
        );
        self.sync_camera();
        // The current brush/tool and pressure/feedback settings remain shared
        // policy; the candidate carries only document/render resources.
        self.apply_brush()
            .expect("previously validated brush state");
        self.refresh_document();
        self.refresh_commands();
        self.changed(
            regions::DOCUMENT | regions::CAMERA | regions::COMMANDS,
            true,
        );
        Ok(candidate)
    }
}
fn project_title(title: &str) -> Result<String, String> {
    if title.is_empty() || title.len() > 4096 || title.contains(['\0', '\n', '\r']) {
        Err("Invalid document title".into())
    } else {
        Ok(title.into())
    }
}
