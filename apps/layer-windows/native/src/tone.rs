//! Windows worker lifetime and publication around shared HDR snapshot analysis.
use layer_host::NativeHost;
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::proof_workflow::ToneKey;
use std::{
    sync::{Arc, mpsc},
    thread::JoinHandle,
};
type Guide = Arc<layer_core::color::hdr::LocalToneGuide>;
struct Pending {
    key: ToneKey,
    generation: u64,
    time: f32,
    control: CaptureControl,
    thread: JoinHandle<()>,
    result: mpsc::Receiver<Result<Guide, String>>,
}
pub(crate) struct Service {
    key: Option<ToneKey>,
    generation: u64,
    pub guide: Option<Guide>,
    pub error: Option<String>,
    pending: Option<Pending>,
    wake: Arc<dyn Fn() + Send + Sync>,
    analysed_time: f32,
}
impl Service {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            key: None,
            generation: 0,
            guide: None,
            error: None,
            pending: None,
            wake,
            analysed_time: 0.,
        }
    }
    pub fn poll(&mut self, host: &mut NativeHost, generation: u64) -> Result<(), String> {
        let key = ToneKey::current(&host.session);
        if key != self.key || generation != self.generation {
            if let Some(p) = &self.pending {
                p.control.cancel();
            }
            self.key = key;
            self.generation = generation;
            self.guide = None;
            self.error = None;
            host.dirty = true;
            host.invalidate_snapshot();
        }
        if (host.session.state().document_file.close_ready || host.session.rendering_suspended())
            && let Some(p) = &self.pending
        {
            p.control.cancel();
        }
        let completed = self
            .pending
            .as_ref()
            .and_then(|p| match p.result.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("HDR analysis worker stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = completed {
            let p = self.pending.take().unwrap();
            let _ = p.thread.join();
            if !p.control.is_cancelled()
                && self.key.as_ref() == Some(&p.key)
                && generation == p.generation
            {
                match result {
                    Ok(guide) => {
                        self.guide = Some(guide);
                        self.analysed_time = p.time;
                    }
                    Err(e) => self.error = Some(e),
                }
                host.dirty = true;
                host.invalidate_snapshot();
            }
        }
        let animated = host.session.engine().document().has_animated_effects()
            && (host.session.engine().animation_time() - self.analysed_time).abs() >= 0.5;
        if self.pending.is_some()
            || self.key.is_none()
            || (self.guide.is_some() && !animated)
            || self.error.is_some()
            || host.session.state().document_file.close_ready
            || host.session.require_document_snapshot_idle().is_err()
        {
            return Ok(());
        }
        let Some(gpu) = host.session.engine().backend().0.as_ref() else {
            return Ok(());
        };
        let gpu = gpu.snapshot_gpu();
        let project = host.session.capture_project_recovery()?;
        let background = host.session.engine().view().background_rgba_linear;
        let time = host.session.engine().animation_time();
        let control = CaptureControl::default();
        let cancel = control.clone();
        let wake = self.wake.clone();
        let (send, result) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("windows-hdr-analysis".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut snapshot = gpu
                        .capture(project, background, time, Default::default(), cancel)
                        .map_err(|e| e.to_string())?;
                    snapshot.local_tone_guide()
                }))
                .unwrap_or_else(|_| Err("HDR analysis worker failed".into()));
                let _ = send.send(result);
                wake();
            })
            .map_err(|e| e.to_string())?;
        self.pending = Some(Pending {
            key: self.key.clone().unwrap(),
            generation,
            time,
            control,
            thread,
            result,
        });
        host.invalidate_snapshot();
        Ok(())
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"ready":self.guide.is_some(),"pending":self.pending.is_some(),"error":self.error})
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(p) = self.pending.take() {
            p.control.cancel();
            p.thread.join().map_err(|_| "HDR worker shutdown failed")?;
        }
        Ok(())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
