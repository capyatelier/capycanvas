//! Window drawing ownership. Rust owns membership, admission, history and backing;
//! WinUI owns tab capture and presentation. The file worker builds renderers.
use super::*;
use layer_host::window::{Activation, OpenAdoption, TabRequest};
use serde_json::Value;

pub(super) struct Parked {
    pub session: UiSession<Renderer>,
    pub recovery: Option<crate::recovery::Service>,
}
impl layer_host::window::Parked for Parked {
    fn session(&self) -> &UiSession<Renderer> {
        &self.session
    }
    fn session_mut(&mut self) -> &mut UiSession<Renderer> {
        &mut self.session
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Select {
        id: u64,
    },
    Close {
        id: u64,
    },
    Adjacent {
        forward: bool,
    },
    Slide {
        id: u64,
        hits: Vec<layer_ui::DocumentTabHit>,
        clip: layer_ui::Bounds,
        press: [f32; 2],
        point: [f32; 2],
    },
    Step {
        id: u64,
        forward: bool,
    },
    Reorder {
        id: u64,
        before: Option<u64>,
    },
    History {
        redo: bool,
    },
    RetryStorage,
}
impl DocumentService {
    pub(crate) fn start_recovery(&mut self) -> Result<(), String> {
        if self.recovery.is_none() {
            self.recovery = Some(self.new_recovery()?);
        }
        Ok(())
    }
    fn new_recovery(&self) -> Result<crate::recovery::Service, String> {
        let wake = self.wake.clone();
        crate::recovery::Service::open(move || wake())
    }
    pub(crate) fn tab_device(&self) -> Option<&wgpu::Device> {
        self.window.gpu.as_ref().map(|g| &g.device)
    }
    pub(crate) fn window_close_ready(&self, host: &NativeHost) -> bool {
        self.window.documents.order().len() == 1 && host.session.state().document_file.close_ready
    }
    pub(crate) fn tabs_view(&self, host: &NativeHost) -> Value {
        let mut view = self.window.view(host, 0.);
        view["available"] = (self.idle()
            && !self.close_window
            && !host.session.state().document_file.close_ready
            && host.session.can_park_document())
        .into();
        view["activating"] = self.activating.into();
        view["window_ready"] = self.window_close_ready(host).into();
        view
    }
    fn idle(&self) -> bool {
        self.active.is_none()
            && self.workflow.is_none()
            && !self.workflow_running
            && !self.activating
            && !self.spilling
            && self.deferred_action.is_none()
            && self.recovery.as_ref().is_none_or(|r| !r.restoring())
    }
    fn document_retired(&mut self) -> Result<(), String> {
        self.tone.clear();
        self.proof.stop()?;
        self.proof.view = Default::default();
        Ok(())
    }
    fn activate(&mut self, host: &mut NativeHost, activation: Activation) {
        if self.window.gpu.is_none() {
            host.error =
                Some("Painting is unavailable. This drawing can still be saved or closed.".into());
            return;
        }
        self.activating = true;
        self.worker.submit(Job::Activate(Box::new(activation)));
    }
    fn switch(&mut self, host: &mut NativeHost, id: u64, closing: bool) -> Result<(), String> {
        let options = host.renderer_options(None);
        let recovery = &mut self.recovery;
        let Some((activation, closed)) =
            self.window.switch(host, id, closing, options, |incoming| {
                std::mem::swap(recovery, &mut incoming.recovery)
            })?
        else {
            return Ok(());
        };
        self.document_retired()?;
        if let Some(closed) = closed {
            self.worker.retire(Box::new(closed.session));
        }
        self.activate(host, activation);
        Ok(())
    }
    fn select(&mut self, host: &mut NativeHost, id: u64) -> Result<(), String> {
        if id != self.window.documents.selected() && !self.idle() {
            return Err("Finish the current operation before switching drawings".into());
        }
        self.switch(host, id, false)
    }
    fn adopt_candidate(
        &mut self,
        host: &mut NativeHost,
        active: Active,
        candidate: &mut Option<Box<UiSession<Renderer>>>,
    ) -> Result<Option<Box<WgpuRasterizer>>, String> {
        Self::matches(host, active.epoch, active.revision)?;
        let mut recovery = if self.recovery.is_some() {
            Some(self.new_recovery()?)
        } else {
            None
        };
        let open = OpenAdoption {
            epoch: active.epoch,
            revision: active.revision,
            location: active.location,
            recovered: false,
        };
        let slot = &mut self.recovery;
        self.window.adopt(
            host,
            candidate,
            open,
            || true,
            |session| {
                std::mem::swap(slot, &mut recovery);
                Parked { session, recovery }
            },
        )
    }
    pub(super) fn append_candidate(
        &mut self,
        host: &mut NativeHost,
        active: Active,
        candidate: Box<UiSession<Renderer>>,
    ) -> Result<(), String> {
        let id = active.id;
        let mut candidate = Some(candidate);
        match self.adopt_candidate(host, active, &mut candidate) {
            Ok(retired) => {
                self.document_retired()?;
                self.worker.retire_renderer(Renderer(retired));
                Ok(())
            }
            Err(error) => {
                if let Some(candidate) = candidate {
                    self.worker.retire(candidate);
                }
                Self::complete(host, id, Err(error))
            }
        }
    }
    fn adopt_recovery(
        &mut self,
        host: &mut NativeHost,
        restored: crate::recovery::Restored,
    ) -> Result<(), String> {
        let crate::recovery::Restored {
            token,
            identity,
            mut candidate,
        } = restored;
        let checked = (|| {
            if self.active.is_some() || self.activating || !host.session.can_park_document() {
                return Err("Finish the current operation before restoring this drawing".into());
            }
            Self::matches(host, identity.0, identity.1)?;
            if host
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .map(|g| g.device())
                != candidate.engine().backend().0.as_ref().map(|g| g.device())
            {
                return Err("The GPU changed during recovery; try again".into());
            }
            self.window.documents.admit(
                &host.session.retained_document_tiles(),
                &candidate.capture_project_recovery()?,
            )?;
            candidate.mark_recovered();
            candidate.inherit_window_state(&host.session)?;
            candidate.inherit_initial_drawing_tools(&host.session)?;
            let outgoing_recovery = self.new_recovery()?;
            let tiles = host.session.park_document()?;
            Ok((outgoing_recovery, tiles))
        })();
        let result = match checked {
            Ok((outgoing_recovery, tiles)) => {
                self.document_retired()?;
                self.worker
                    .retire_renderer(Renderer(self.window.retire_gpu(host)));
                let outgoing = Parked {
                    session: std::mem::replace(&mut host.session, *candidate),
                    recovery: Some(outgoing_recovery),
                };
                self.window.documents.append(outgoing, tiles);
                self.window.changed(host);
                Ok(())
            }
            Err(error) => {
                self.worker.retire(candidate);
                Err(error)
            }
        };
        self.recovery
            .as_mut()
            .unwrap()
            .complete_restore(&mut host.session, token, result)?;
        host.invalidate_snapshot();
        Ok(())
    }
    pub(super) fn activated(
        &mut self,
        host: &mut NativeHost,
        completed: Result<Completed, String>,
    ) -> Result<(), String> {
        self.activating = false;
        match completed {
            Ok(Completed::Activated(mut activation)) => {
                match self.window.resume(host, &mut activation) {
                    Ok(Some(gpu)) => {
                        #[cfg(target_os = "windows")]
                        if crate::device::removed(gpu.device()) {
                            host.error = Some(
                                "The GPU changed while switching drawings; restart the canvas"
                                    .into(),
                            );
                            host.invalidate_snapshot();
                            return Ok(());
                        }
                        let previous = host.session.state().revision;
                        let (retired, change) =
                            host.session.replace_renderer(Renderer(Some(gpu)))?;
                        self.worker.retire_renderer(retired);
                        host.apply_change(previous, change);
                        host.startup = Default::default();
                    }
                    Ok(None) => {}
                    Err(_) => self
                        .worker
                        .retire_renderer(Renderer(activation.take_renderer())),
                }
            }
            Err(error) => host.error = Some(error),
            _ => return Err("Unexpected drawing activation completion".into()),
        }
        host.invalidate_snapshot();
        Ok(())
    }
    pub(super) fn tab_action(
        &mut self,
        host: &mut NativeHost,
        action: Action,
    ) -> Result<(), String> {
        if !self.idle() || self.close_window || host.session.state().document_file.close_ready {
            return Err("Finish the current operation before changing drawings".into());
        }
        match action {
            Action::Select { id } => self.select(host, id)?,
            Action::Adjacent { forward } => {
                if let Some(id) = self.window.documents.adjacent(forward) {
                    self.select(host, id)?;
                }
            }
            Action::Close { id } => {
                self.select(host, id)?;
                if self.activating {
                    self.close_next = true;
                } else {
                    host.dispatch(layer_ui::UiAction::Invoke {
                        command: layer_ui::CommandId::CloseDocument,
                    })?;
                }
            }
            Action::RetryStorage => {
                self.window.documents.storage_completed(Ok(()));
            }
            Action::Slide {
                id,
                hits,
                clip,
                press,
                point,
            } => {
                if hits.len() > 1024 {
                    return Err("Too many drawing targets".into());
                }
                let before = self
                    .window
                    .documents
                    .drag(id, press, &hits, clip)
                    .and_then(|drag| drag.preview(point))
                    .filter(|slide| slide.attached)
                    .map(|slide| slide.before);
                if let Some(before) = before {
                    self.window
                        .request(host, TabRequest::Reorder { id, before })?;
                }
            }
            Action::Reorder { id, before } => {
                self.window
                    .request(host, TabRequest::Reorder { id, before })?;
            }
            Action::Step { id, forward } => {
                self.window
                    .request(host, TabRequest::Step { id, forward })?;
            }
            Action::History { redo } => {
                self.window.request(host, TabRequest::History { redo })?;
            }
        }
        host.invalidate_snapshot();
        Ok(())
    }
    pub(super) fn poll_tabs(&mut self, host: &mut NativeHost) -> Result<(), String> {
        if let Some(recovery) = &mut self.recovery
            && recovery.poll(&mut host.session, true)?
        {
            host.invalidate_snapshot();
        }
        if let Some(restored) = self.recovery.as_mut().and_then(|r| r.take_restored()) {
            self.adopt_recovery(host, restored)?;
        }
        for (_, p) in self.window.documents.parked_mut() {
            if let Some(r) = &mut p.owner.recovery {
                r.poll(&mut p.owner.session, false)?;
            }
        }
        if self.close_next && self.idle() && host.session.can_park_document() {
            self.close_next = false;
            host.dispatch(layer_ui::UiAction::Invoke {
                command: layer_ui::CommandId::CloseDocument,
            })?;
        }
        if self.close_window
            && !self.close_next
            && self.idle()
            && !host.session.state().document_file.busy
            && !host.session.state().document_file.close_ready
        {
            self.close_window = false;
        }
        if host.session.state().document_file.close_ready
            && self.window.documents.order().len() > 1
            && self.idle()
            && self.recovery.as_ref().is_none_or(|r| r.close_ready())
        {
            self.switch(host, 0, true)?;
            self.close_next = self.close_window;
        }
        if self.idle()
            && host.session.can_park_document()
            && self.window.documents.storage_error().is_none()
            && let Some(tiles) = self.window.documents.spill_candidate()
        {
            let directory = crate::settings::data_directory()?.join("drawing-backing");
            self.spilling = true;
            self.worker.submit(Job::Spill { tiles, directory });
            host.invalidate_snapshot();
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "document_tabs_tests.rs"]
mod tests;
