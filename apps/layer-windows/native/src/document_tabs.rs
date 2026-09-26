//! Window drawing ownership. Rust owns membership, admission, history and backing;
//! WinUI owns tab capture and presentation. The file worker builds renderers.
use super::*;
use serde_json::{Value, json};

pub(super) struct Parked {
    pub session: UiSession<Renderer>,
    pub recovery: Option<crate::recovery::Service>,
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
        self.tab_gpu.as_ref().map(|g| &g.device)
    }
    pub(crate) fn window_close_ready(&self, host: &NativeHost) -> bool {
        self.tabs.order().len() == 1 && host.session.state().document_file.close_ready
    }
    fn tabs_changed(&self, host: &mut NativeHost) {
        host.document_count = self.tabs.order().len();
        host.document_adopted();
        host.invalidate_snapshot();
    }
    pub(crate) fn tabs_view(&self, host: &NativeHost) -> Value {
        json!({"tabs":self.tabs.labels(&host.session.state().document_file,|p| &p.session.state().document_file),
            "selected":self.tabs.selected(),"can_undo":self.tabs.can_undo(),"can_redo":self.tabs.can_redo(),
            "available":self.idle() && !self.close_window && !host.session.state().document_file.close_ready && host.session.can_park_document(),"activating":self.activating.is_some(),
            "window_ready":self.window_close_ready(host),"storage_error":self.tabs.storage_error(),
            "resident_bytes":self.tabs.resident_bytes(),"parked_renderers":self.tabs.parked().filter(|(_,p)|p.owner.session.engine().backend().0.is_some()).count()})
    }
    fn idle(&self) -> bool {
        self.active.is_none()
            && self.workflow.is_none()
            && !self.workflow_running
            && self.activating.is_none()
            && !self.spilling
            && self.deferred_action.is_none()
            && self.recovery.as_ref().is_none_or(|r| !r.restoring())
    }
    fn retire_gpu(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.tone.clear();
        self.proof.stop()?;
        self.proof.view = Default::default();
        if let Some(gpu) = host.session.renderer_mut().0.take() {
            self.tab_gpu = Some(layer_host::GpuContext::of(&gpu));
            self.worker.retire_renderer(Renderer(Some(gpu)));
        }
        Ok(())
    }
    fn activate(&mut self, host: &mut NativeHost) -> Result<(), String> {
        host.startup = Default::default();
        self.tabs_changed(host);
        let Some(gpu) = self.tab_gpu.clone() else {
            host.error =
                Some("Painting is unavailable. This drawing can still be saved or closed.".into());
            return Ok(());
        };
        self.activating = Some((
            self.tabs.selected(),
            host.session.state().document_file.epoch,
        ));
        self.worker.submit(Job::Activate {
            gpu,
            options: host.renderer_options(None),
            color: host.session.engine().document().color,
        });
        Ok(())
    }
    fn select(&mut self, host: &mut NativeHost, id: u64) -> Result<(), String> {
        if id == self.tabs.selected() {
            return Ok(());
        }
        if !self.idle() {
            return Err("Finish the current operation before switching drawings".into());
        }
        if !host.session.can_park_document()
            || (!host.session.rendering_suspended()
                && host
                    .session
                    .retained_document_tiles()
                    .try_blobs()?
                    .is_none())
        {
            return Err("Wait for drawing capture before switching drawings".into());
        }
        let next = self
            .tabs
            .parked_owner_mut(id)
            .ok_or("Drawing tab is no longer open")?;
        next.session.inherit_window_state(&host.session)?;
        let tiles = host.session.park_document()?;
        self.retire_gpu(host)?;
        // Exchange both the editor and its recovery lease with one membership change.
        self.tabs.exchange_with(id, tiles, |incoming| {
            std::mem::swap(&mut host.session, &mut incoming.session);
            std::mem::swap(&mut self.recovery, &mut incoming.recovery);
        })?;
        self.activate(host)
    }
    pub(super) fn append_candidate(
        &mut self,
        host: &mut NativeHost,
        active: Active,
        mut candidate: Box<UiSession<Renderer>>,
    ) -> Result<(), String> {
        let checked = (|| {
            Self::matches(host, active.epoch, active.revision)?;
            let tiles = host.session.retained_document_tiles();
            if tiles.try_blobs()?.is_none() {
                return Err("Wait for drawing capture before opening another drawing".into());
            }
            self.tabs
                .admit(&tiles, &candidate.capture_project_recovery()?)?;
            candidate.initialize_document_location(active.location)?;
            candidate.inherit_window_state(&host.session)?;
            candidate.inherit_initial_drawing_tools(&host.session)?;
            let recovery = if self.recovery.is_some() {
                Some(self.new_recovery()?)
            } else {
                None
            };
            Ok(recovery)
        })();
        let recovery = match checked {
            Ok(r) => r,
            Err(error) => {
                self.worker.retire(candidate);
                return Self::complete(host, active.id, Err(error));
            }
        };
        Self::complete(host, active.id, Ok(true))?;
        let tiles = host.session.park_document()?;
        self.retire_gpu(host)?;
        let outgoing = Parked {
            session: std::mem::replace(&mut host.session, *candidate),
            recovery: std::mem::replace(&mut self.recovery, recovery),
        };
        self.tabs.append(outgoing, tiles);
        self.tabs_changed(host);
        Ok(())
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
            if self.active.is_some()
                || self.activating.is_some()
                || !host.session.can_park_document()
            {
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
            self.tabs.admit(
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
                self.retire_gpu(host)?;
                let outgoing = Parked {
                    session: std::mem::replace(&mut host.session, *candidate),
                    recovery: Some(outgoing_recovery),
                };
                self.tabs.append(outgoing, tiles);
                self.tabs_changed(host);
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
        let identity = self.activating.take().unwrap();
        match completed {
            Ok(Completed::Activated(gpu)) => {
                if identity
                    != (
                        self.tabs.selected(),
                        host.session.state().document_file.epoch,
                    )
                    || host.session.engine().backend().0.is_some()
                {
                    self.worker.retire_renderer(Renderer(Some(gpu)));
                } else {
                    #[cfg(target_os = "windows")]
                    if crate::device::removed(gpu.device()) {
                        host.error = Some(
                            "The GPU changed while switching drawings; restart the canvas".into(),
                        );
                        host.invalidate_snapshot();
                        return Ok(());
                    }
                    let previous = host.session.state().revision;
                    let (retired, change) = host.session.replace_renderer(Renderer(Some(gpu)))?;
                    self.worker.retire_renderer(retired);
                    host.apply_change(previous, change);
                    host.startup = Default::default();
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
                if let Some(id) = self.tabs.adjacent(forward) {
                    self.select(host, id)?;
                }
            }
            Action::Close { id } => {
                self.select(host, id)?;
                if self.activating.is_some() {
                    self.close_next = true;
                } else {
                    host.dispatch(layer_ui::UiAction::Invoke {
                        command: layer_ui::CommandId::CloseDocument,
                    })?;
                }
            }
            Action::RetryStorage => {
                self.tabs.storage_completed(Ok(()));
            }
            other => {
                if !host.session.can_park_document() {
                    return Err("Finish the current operation before reordering drawings".into());
                }
                match other {
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
                        if let Some(slide) = self
                            .tabs
                            .drag(id, press, &hits, clip)
                            .and_then(|drag| drag.preview(point))
                            .filter(|slide| slide.attached)
                        {
                            self.tabs.reorder(id, slide.before);
                        }
                    }
                    Action::Reorder { id, before } => {
                        self.tabs.reorder(id, before);
                    }
                    Action::Step { id, forward } => {
                        if let Some(before) = self.tabs.step(id, forward) {
                            self.tabs.reorder(id, before);
                        }
                    }
                    Action::History { redo } => {
                        if redo {
                            self.tabs.redo()
                        } else {
                            self.tabs.undo()
                        }
                    }
                    _ => unreachable!(),
                }
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
        for (_, p) in self.tabs.parked_mut() {
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
            && self.tabs.order().len() > 1
            && self.idle()
            && self.recovery.as_ref().is_none_or(|r| r.close_ready())
        {
            let id = self.tabs.after_close().unwrap();
            self.tabs
                .parked_owner_mut(id)
                .unwrap()
                .session
                .inherit_window_state(&host.session)?;
            host.session.park_document()?;
            self.retire_gpu(host)?;
            let next = self.tabs.close_selected().unwrap();
            let previous = std::mem::replace(&mut host.session, next.session);
            self.worker.retire(Box::new(previous));
            self.recovery = next.recovery;
            self.close_next = self.close_window;
            self.activate(host)?;
        }
        if self.idle()
            && host.session.can_park_document()
            && self.tabs.storage_error().is_none()
            && let Some(tiles) = self.tabs.spill_candidate()
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
