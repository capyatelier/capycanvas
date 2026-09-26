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
    result: mpsc::Receiver<Result<Arc<layer_color::ProofLut>, String>>,
}
pub(crate) struct Service {
    pending: Option<Pending>,
    wake: Arc<dyn Fn() + Send + Sync>,
    status: String,
}
impl Service {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            pending: None,
            wake,
            status: String::new(),
        }
    }
    pub fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        let status = host.proof.observe(&host.session);
        if let Some(pending) = &self.pending
            && (!status.needed
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
                    Ok(lut) => host.proof.retain(&pending.job, lut)?,
                    Err(error) => host.proof.fail(&host.session, &pending.job, error),
                }
                host.dirty = true;
                host.invalidate_snapshot();
            }
        }
        if self.pending.is_none()
            && host.proof.observe(&host.session).needed
            && !host.session.state().document_file.close_ready
        {
            let job = ProofPreparation::begin(&host.session, None, None)?;
            let control = CaptureControl::default();
            let (work, cancel, wake) = (job.clone(), control.clone(), self.wake.clone());
            let (send, result) = mpsc::channel();
            match std::thread::Builder::new()
                .name("windows-proof".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        work.build(|| cancel.is_cancelled())
                    }))
                    .unwrap_or_else(|_| Err("Proof worker failed".into()));
                    let _ = send.send(result);
                    wake();
                }) {
                Ok(thread) => {
                    self.pending = Some(Pending {
                        job,
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
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(pending) = self.pending.take() {
            pending.control.cancel();
            pending
                .thread
                .join()
                .map_err(|_| "Proof worker shutdown failed")?;
        }
        Ok(())
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
        assert_eq!(host.session.engine().document().proof, Some(original));
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
