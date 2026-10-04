//! Window drawing ownership. Rust owns membership, admission, history and backing;
//! WinUI owns tab capture and presentation. The file worker builds renderers.
use super::*;
use layer_host::window::{Activation, OpenAdoption, TabRequest};
use serde_json::Value;

pub(super) struct Parked {
    pub session: UiSession<Renderer>,
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
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
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
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
    pub(crate) fn tab_device(&self) -> Option<&wgpu::Device> {
        self.window.gpu.as_ref().map(|g| &g.device)
    }
    pub(crate) fn window_close_ready(&self, host: &NativeHost) -> bool {
        (self.close_window || self.window.documents.order().len()==1) && host.session.state().document_file.close_ready
    }
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
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
    fn document_retired(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.tone.clear();
        self.proof.stop()?;
        host.proof = Default::default();
        Ok(())
    }
    fn activate(&mut self, host: &mut NativeHost, activation: Activation) {
        if self.window.gpu.is_none() {
            host.error =
                Some(layer_ui::DocumentTransportRefusal::PaintingUnavailable.message(host.session.localization()).to_string());
            return;
        }
        self.activating = true;
        self.worker.submit(Job::Activate(Box::new(activation)));
    }
    fn switch(&mut self, host: &mut NativeHost, id: u64, closing: bool) -> Result<(), String> {
        let options = host.renderer_options(None);
        let Some((activation, closed)) =
            self.window.switch(host, id, closing, options, |_| {})?
        else {
            return Ok(());
        };
        self.document_retired(host)?;
        if let Some(closed) = closed {
            self.worker.retire(Box::new(closed.session));
        }
        self.activate(host, activation);
        Ok(())
    }
    fn select(&mut self, host: &mut NativeHost, id: u64) -> Result<(), String> {
        if id != self.window.documents.selected() && !self.idle() {
            return Err(layer_ui::DocumentTransportRefusal::SwitchOperation.message(host.session.localization()).to_string());
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
        let open = OpenAdoption {
            epoch: active.epoch,
            revision: active.revision,
            location: active.location,
        };
        self.window.adopt(
            host,
            candidate,
            open,
            || true,
            |session| Parked { session },
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
                self.document_retired(host)?;
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
        let crate::recovery::Restored { mut candidates, active, stamp } = restored;
        let order=self.recovery.as_ref().unwrap().restore_order();
        let result=if self.window.documents.order().len()==1&&host.session.can_replace_startup_session(&stamp){
            self.window.restore_sessions(host,&mut candidates,active,stamp,|session|Parked{session}).and_then(|retired|{self.window.documents.restore_order(&order,active)?;Ok(retired)})
        }else{
            self.window.append_restored_sessions(host,&mut candidates,|session|Parked{session}).and_then(|(mapping,retired)|{self.recovery.as_mut().unwrap().remap_restored(mapping)?;Ok(retired)})
        };
        match result {
            Ok(retired)=>{for renderer in retired{self.worker.retire_renderer(Renderer(Some(renderer)));}self.document_retired(host)?;self.recovery.as_mut().unwrap().complete_restore(Ok(()))?;},
            Err(error)=>{for (_,candidate) in candidates{self.worker.retire(candidate);}self.recovery.as_mut().unwrap().complete_restore(Err(error))?;}
        }
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
                                layer_ui::DocumentTransportRefusal::SwitchGpuChanged.message(host.session.localization()).to_string(),
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
            return Err(layer_ui::DocumentTransportRefusal::ChangeInProgress.message(host.session.localization()).to_string());
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
        if self.prepared_close.is_some()&&!host.session.state().document_file.close_ready {let prepared=self.prepared_close.take().unwrap();self.window.cancel_close(host,prepared)?;}
        if !self.close_window && host.session.state().document_file.close_ready && self.prepared_close.is_none() {
            if !self.idle()||!self.window.adoption_ready(host)? {return Ok(());}
            let options=host.renderer_options(None);self.prepared_close=Some(self.window.prepare_close(host,options)?);
        }
        if let Some(prepared)=&self.prepared_close {self.window.validate_close(host,prepared)?;}
        if let Some(recovery) = &mut self.recovery
            && recovery.poll(host, &self.window, self.close_window && host.session.state().document_file.close_ready)?
        {
            host.invalidate_snapshot();
        }
        if self.prepared_close.is_some() && (!host.session.state().document_file.close_ready || self.recovery.as_ref().is_some_and(|recovery|recovery.failed())) {
            let prepared=self.prepared_close.take().unwrap();self.window.cancel_close(host,prepared)?;
        }
        if !self.activating && self.active.is_none() && host.session.can_park_document()
            && let Some(restored) = self.recovery.as_mut().and_then(|r| r.take_restored()) {
            self.adopt_recovery(host, restored)?;
        }
        if self.close_next && self.idle() && host.session.can_park_document() {
            self.close_next = false;
            host.dispatch(layer_ui::UiAction::Invoke {
                command: layer_ui::CommandId::CloseDocument,
            })?;
        }
        if self.close_window && self.idle() && !host.session.state().document_file.busy && !host.session.state().document_file.close_ready {
            if self.quit_pending {
                self.quit_pending=false;let previous=host.session.state().revision;let change=host.session.request_session_close()?;host.apply_change(previous,change);
            }else{self.close_window=false;}
        }
        if !self.close_window && host.session.state().document_file.close_ready
            && self.window.documents.order().len() > 1
            && self.idle()
            && self.recovery.as_ref().is_none_or(|r| r.close_ready())
        {
            let prepared=self.prepared_close.take().ok_or("The drawing close is not prepared")?;
            let (activation,closed)=self.window.commit_close(host,prepared,|_|{});
            self.document_retired(host)?;if let Some(closed)=closed{self.worker.retire(Box::new(closed.session));}self.activate(host,activation);
            self.close_next = false;
        }
        if self.idle()
            && host.session.can_park_document()
            && self.window.documents.storage_error().is_none()
            && let Some(tiles) = self.window.documents.spill_candidate()
        {
            self.spilling = true;
            self.worker.submit(Job::Spill { tiles });
            host.invalidate_snapshot();
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "document_tabs_tests.rs"]
mod tests;
