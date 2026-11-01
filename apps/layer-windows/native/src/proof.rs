//! Native scheduling for the shared view cache. CPU work never borrows the editor.
use layer_host::NativeHost;
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::proof_workflow::ProofPreparation;
use std::{
    sync::{Arc, mpsc},
    thread::JoinHandle,
};

struct Pending {
    job: ProofPreparation,
    control: CaptureControl,
    thread: JoinHandle<()>,
    result: mpsc::Receiver<Result<(ProofPreparation, Arc<layer_color::ProofLut>), layer_ui::ColorFeatureError>>,
    edit: bool,
}
struct Inventory {
    control: CaptureControl,
    thread: JoinHandle<()>,
    result: mpsc::Receiver<Result<Vec<layer_ui::profile_library::ProfileEntry>,String>>,
}
struct Retired {
    pending: Option<Pending>,
    inventory: Option<Inventory>,
    finished: mpsc::Sender<Result<(), String>>,
}
impl Drop for Retired {
    fn drop(&mut self) {
        let mut result = Ok(());
        if let Some(inventory) = self.inventory.take() { result = result.and(inventory.thread.join().map_err(|_| "Profile inventory worker shutdown failed".into())); }
        if let Some(pending) = self.pending.take() { result = result.and(pending.thread.join().map_err(|_| "Proof worker shutdown failed".into())); }
        let _ = self.finished.send(result);
    }
}
pub(crate) struct Service {
    pending: Option<Pending>,
    inventory: Option<Inventory>,
    retired: Vec<mpsc::Receiver<Result<(), String>>>,
    library: Vec<layer_ui::profile_library::ProfileEntry>,
    library_ready: bool,
    wake: Arc<dyn Fn() + Send + Sync>,
    status: String,
    edit: Option<(ProofPreparation, layer_ui::proof_panel::PrintProofSettings, Option<String>)>,
}
impl Service {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            pending: None,
            inventory: None,
            retired: Vec::new(),
            library: Vec::new(),
            library_ready: false,
            wake,
            status: String::new(),
            edit: None,
        }
    }
    pub fn edit(&mut self, host: &NativeHost, epoch: u64, mut settings: layer_ui::proof_panel::PrintProofSettings, profile_id: Option<String>) -> Result<(), String> {
        if epoch != host.session.state().document_file.epoch { return Ok(()); }
        if profile_id.as_deref()==Some("document") {
            settings.profile=host.session.engine().document().output().proof.as_ref().map(layer_ui::proof_panel::PrintProofSettings::from_recipe).transpose()?.and_then(|settings|settings.profile);
        }
        let recipe = if profile_id.as_deref().is_some_and(|id|id!="document") {
            host.session.engine().document().output().proof.clone().unwrap_or_else(||layer_core::color::ProofRecipe::new("sRGB".into(),layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::Srgb)))
        } else {settings.recipe().map_err(|e|e.proof_message(host.session.localization()))?};
        let job = ProofPreparation::panel(&host.session, recipe).map_err(|e|e.proof_message(host.session.localization()))?;
        if let Some(pending) = &self.pending { pending.control.cancel(); }
        self.edit = Some((job,settings,profile_id));
        Ok(())
    }
    pub fn library(&mut self, entries: Vec<layer_ui::profile_library::ProfileEntry>) {
        if let Some(inventory)=&self.inventory {inventory.control.cancel();}
        self.library=entries;self.library_ready=true;
    }
    pub fn form(&self, host: &NativeHost) -> serde_json::Value {
        use layer_ui::proof_panel::PrintProofSettings;
        let session=&host.session;
        let mut form=serde_json::json!(layer_ui::proof_workflow::proof_copy(session));
        let mut settings=session.engine().document().output().proof.as_ref().and_then(|recipe|PrintProofSettings::from_recipe(recipe).ok()).unwrap_or_default();
        let document=settings.profile.take().map(|profile|serde_json::json!({"name":profile.name,"channels":profile.channels,"profile":{"Icc":[]},"library":"document"}));
        form["print_settings"]=serde_json::json!(settings);
        form["intents"]=serde_json::json!(layer_ui::proof_panel::proof_intents(session.localization()).map(|choice|serde_json::json!({"value":choice.value,"label":choice.label,"bpc_available":PrintProofSettings{intent:choice.value,..Default::default()}.bpc_available()})));
        form["document_profile"]=serde_json::json!(document);
        form["profiles"]=serde_json::json!(layer_core::color::RgbSpace::ALL.map(layer_ui::ExportProfile::builtin));
        form["library"]=serde_json::json!(self.library.iter().map(|entry|entry.localized_view(session.localization())).collect::<Vec<_>>());
        form["preparing"]=serde_json::json!(self.edit.is_some() || self.pending.as_ref().is_some_and(|pending|pending.edit));
        form
    }
    pub fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        let mut failure = None;
        self.retired.retain(|finished| match finished.try_recv() {
            Ok(Ok(())) => false,
            Ok(Err(error)) => { failure.get_or_insert(error); false }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => { failure.get_or_insert("Proof worker shutdown failed".into()); false }
        });
        if let Some(error) = failure { return Err(error); }
        if let Some(result)=self.inventory.as_ref().and_then(|inventory|match inventory.result.try_recv(){Ok(result)=>Some(result),Err(mpsc::TryRecvError::Disconnected)=>Some(Err("Profile inventory worker stopped".into())),Err(mpsc::TryRecvError::Empty)=>None}) {
            let inventory=self.inventory.take().unwrap();let _=inventory.thread.join();
            if !inventory.control.is_cancelled() {if let Ok(entries)=result {self.library=entries;}self.library_ready=true;host.invalidate_snapshot();}
        }
        if !self.library_ready && self.inventory.is_none() && host.session.proof_panel_mode()==layer_ui::ProofMode::Print {
            let control=CaptureControl::default();let cancel=control.clone();let wake=self.wake.clone();let(send,result)=mpsc::channel();
            let thread=std::thread::Builder::new().name("windows-proof-profiles".into()).spawn(move||{let result=crate::color_storage::list(cancel.cancellation_flag());let _=send.send(result);wake();}).map_err(|error|error.to_string())?;
            self.inventory=Some(Inventory{control,thread,result});
        }
        let status = host.proof.observe(&host.session);
        if let Some(pending) = &self.pending
            && ((!status.needed && !pending.edit)
                || pending.job.validate(&host.session).is_err()
                || host.session.state().document_file.close_ready)
        {
            pending.control.cancel();
        }
        let completed = self
            .pending
            .as_ref()
            .and_then(|p| match p.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Proof worker stopped".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = completed {
            let pending = self.pending.take().unwrap();
            let _ = pending.thread.join();
            if !pending.control.is_cancelled() && pending.job.validate(&host.session).is_ok() {
                match result {
                    Ok((job,lut)) => {
                        if pending.edit {
                            let previous=host.session.state().revision;
                            match job.apply(&mut host.session,true) { Ok(change)=>host.apply_change(previous,change),Err(reason)=>host.proof.fail_reason(&host.session,&job,reason) }
                        }
                        if let Err(reason)=host.proof.retain(&job,lut) { host.proof.fail_reason(&host.session,&job,reason); }
                    },
                    Err(reason) => host.proof.fail_reason(&host.session, &pending.job, reason),
                }
                host.dirty = true;
                host.invalidate_snapshot();
            }
            host.invalidate_snapshot();
        }
        if self.pending.is_none()
            && (self.edit.is_some() || host.proof.observe(&host.session).needed)
            && !host.session.state().document_file.close_ready
        {
            let edit=self.edit.take();
            let editing=edit.is_some();
            let job=if let Some((job,_,_))=&edit {job.clone()} else {ProofPreparation::begin(&host.session,None,None).map_err(|reason|reason.proof_message(host.session.localization()))?};
            if job.validate(&host.session).is_err() {host.invalidate_snapshot();return Ok(());}
            let control = CaptureControl::default();
            let (mut work, cancel, wake) = (job.clone(), control.clone(), self.wake.clone());
            let localization=host.session.localization().clone();
            let (send, result) = mpsc::channel();
            match std::thread::Builder::new()
                .name("windows-proof".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if let Some((_,mut settings,profile_id))=edit {
                            if let Some(id)=profile_id.filter(|id|id!="document") {settings.profile=Some(crate::color_storage::export_profile(&id,cancel.cancellation_flag(),&localization)?);}
                            work.recipe=settings.recipe()?;
                        }
                        let lut=work.build(|| cancel.is_cancelled())?;
                        if editing && let Some(bytes)=work.preservation() { crate::color_storage::preserve(bytes,cancel.cancellation_flag(),&localization)?; }
                        Ok((work,lut))
                    }))
                    .unwrap_or_else(|_| Err("Proof worker failed".into()));
                    let _ = send.send(result);
                    wake();
                }) {
                Ok(thread) => {
                    self.pending = Some(Pending {
                        job,
                        edit: editing,
                        control,
                        thread,
                        result,
                    })
                }
                Err(error) => host.proof.fail(&host.session, &job, error.to_string()),
            }
        }
        let status =
            serde_json::to_string(&host.proof.observe(&host.session)).map_err(|e| e.to_string())?;
        if status != self.status {
            self.status = status;
            host.invalidate_snapshot();
        }
        Ok(())
    }
    pub fn retire(&mut self) -> Result<(), String> {
        self.edit = None;
        let pending = self.pending.take();
        let inventory = self.inventory.take();
        if let Some(pending) = &pending { pending.control.cancel(); }
        if let Some(inventory) = &inventory { inventory.control.cancel(); }
        if pending.is_none() && inventory.is_none() { return Ok(()); }
        let (finished, completion) = mpsc::channel();
        layer_host::window::retire_render_resources(Box::new(Retired { pending, inventory, finished }))?;
        self.retired.push(completion);
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.edit = None;
        let inventory = self.inventory.take();
        let pending = self.pending.take();
        if let Some(inventory) = &inventory { inventory.control.cancel(); }
        if let Some(pending) = &pending { pending.control.cancel(); }
        let mut result = Ok(());
        for finished in self.retired.drain(..) {
            result = result.and(finished.recv().unwrap_or_else(|_| Err("Proof worker shutdown failed".into())));
        }
        if let Some(inventory) = inventory { result = result.and(inventory.thread.join().map_err(|_| "Profile inventory worker shutdown failed".into())); }
        if let Some(pending) = pending { result = result.and(pending.thread.join().map_err(|_| "Proof worker shutdown failed".into())); }
        result
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
    #[test]
    fn explicit_stop_waits_for_a_retired_worker_before_releasing_its_wake_owner() {
        let mut service = Service::new(Arc::new(|| {}));
        let (release, waiting) = mpsc::channel();
        let (send, result) = mpsc::channel();
        service.inventory = Some(Inventory {
            control: CaptureControl::default(), result,
            thread: std::thread::spawn(move || { let _ = waiting.recv(); drop(send); }),
        });
        service.retire().unwrap();
        let (started, stopping) = mpsc::channel();
        let (finished, completed) = mpsc::channel();
        let shutdown = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(service.stop()).unwrap();
        });
        stopping.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let blocked = completed.recv_timeout(std::time::Duration::from_millis(50));
        release.send(()).unwrap();
        let waited = matches!(blocked, Err(mpsc::RecvTimeoutError::Timeout));
        blocked.unwrap_or_else(|_| completed.recv_timeout(std::time::Duration::from_secs(5)).unwrap()).unwrap();
        shutdown.join().unwrap();
        assert!(waited, "stop released the wake owner before the retired worker completed");
    }
    #[test]
    fn departing_inventory_is_cancelled_without_waiting_for_its_file_read() {
        let mut service = Service::new(Arc::new(|| {}));
        let control = CaptureControl::default();
        let (release, waiting) = mpsc::channel();
        let (send, result) = mpsc::channel();
        service.inventory = Some(Inventory {
            control: control.clone(), result,
            thread: std::thread::spawn(move || { let _ = waiting.recv(); drop(send); }),
        });
        let (finished, completed) = mpsc::channel();
        let retirement = std::thread::spawn(move || { let _ = finished.send(service.retire()); });
        let accepted = completed.recv_timeout(std::time::Duration::from_secs(2));
        release.send(()).unwrap();
        retirement.join().unwrap();
        accepted.unwrap().unwrap();
        assert!(control.is_cancelled());
    }

    use layer_ui::{CommandId, Platform, UiAction};
    use std::time::{Duration, Instant};
    fn wait(service: &mut Service, host: &mut NativeHost) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            service.poll(host).unwrap();
            if service.pending.is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "proof worker did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn live_print_edits_coalesce_and_retire_when_the_page_changes() {
        use layer_ui::proof_panel::{PrintProofSettings,ProofSimulation};
        let mut host=NativeHost::new(Platform::Windows).unwrap();
        host.session.select_proof_mode(layer_ui::ProofMode::Print).unwrap();
        let mut service=Service::new(Arc::new(||{}));
        let epoch=host.session.state().document_file.epoch;
        let first=PrintProofSettings {profile:Some(layer_ui::ExportProfile::builtin(RgbSpace::Srgb)),..Default::default()};
        let last=PrintProofSettings {profile:Some(layer_ui::ExportProfile::builtin(RgbSpace::DisplayP3)),simulation:ProofSimulation::Colors,..Default::default()};
        service.edit(&host,epoch,first.clone(),None).unwrap();service.poll(&mut host).unwrap();
        assert!(service.pending.as_ref().unwrap().edit);
        service.edit(&host,epoch,last.clone(),None).unwrap();
        assert!(service.pending.as_ref().unwrap().control.is_cancelled());
        wait(&mut service,&mut host);
        assert_eq!(host.session.engine().document().output().proof,Some(last.recipe().unwrap()));
        service.edit(&host,epoch,first,None).unwrap();service.poll(&mut host).unwrap();
        host.session.select_proof_mode(layer_ui::ProofMode::Off).unwrap();
        wait(&mut service,&mut host);
        assert_eq!(host.session.engine().document().output().proof,Some(last.recipe().unwrap()));
        assert!(!service.form(&host)["preparing"].as_bool().unwrap());
    }
    #[test]
    fn view_worker_rebuilds_after_recipe_history_and_cancels_superseded_work() {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        let original = ProofRecipe::new("sRGB".into(), ColorProfile::Builtin(RgbSpace::Srgb));
        let replacement = ProofRecipe::new("P3".into(), ColorProfile::Builtin(RgbSpace::DisplayP3));
        host.session
            .set_proof_recipe(Some(original.clone()))
            .unwrap();
        let mut service = Service::new(Arc::new(|| {}));
        service.poll(&mut host).unwrap();
        assert!(service.pending.is_some());
        // Supersede the job before owner adoption. A completed stale result is
        // rejected even when cancellation races the worker finishing.
        host.session.set_proof_recipe(Some(replacement)).unwrap();
        wait(&mut service, &mut host);
        assert_eq!(host.proof.observe(&host.session).text, "Proof: P3");
        assert!(host.proof.observe(&host.session).bytes > 0);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        wait(&mut service, &mut host);
        assert_eq!(host.proof.observe(&host.session).text, "Proof: sRGB");
        assert_eq!(host.session.engine().document().output().proof, Some(original));
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        service.poll(&mut host).unwrap();
        assert!(service.pending.is_some());
        let checkpoint = host.session.engine().checkpoint();
        service.stop().unwrap();
        assert!(service.pending.is_none());
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        wait(&mut service, &mut host);
        assert_eq!(host.proof.observe(&host.session).text, "Proof: P3");
    }
}
