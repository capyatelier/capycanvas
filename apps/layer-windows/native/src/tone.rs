//! Windows worker lifetime and publication around shared HDR snapshot analysis.
use layer_host::NativeHost;
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::proof_workflow::ToneKey;
use std::{
    sync::{Arc, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};
type Guide = Arc<layer_render_wgpu::local_tone::GpuToneGuide>;
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
    published: Option<ToneKey>,
    changed: Instant,
    last_start: Option<Instant>,
    publications: u64,
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
            published: None,
            changed: Instant::now(),
            last_start: None,
            publications: 0,
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
            let retain = generation == self.generation
                && self
                    .published
                    .as_ref()
                    .zip(key.as_ref())
                    .is_some_and(|(old, next)| old.can_preview(next));
            if !retain {
                self.guide = None;
                self.published = None;
            }
            self.changed = Instant::now();
            self.key = key;
            self.generation = generation;
            self.error = None;
            host.dirty = true;
            host.invalidate_snapshot();
        }
        let idle = !host.session.state().document_file.close_ready
            && !host.session.rendering_suspended()
            && host.session.require_document_snapshot_idle().is_ok();
        if !idle {
            if let Some(p) = &self.pending {
                p.control.cancel();
            }
            self.changed = Instant::now();
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
            host.invalidate_snapshot();
            let p = self.pending.take().unwrap();
            let _ = p.thread.join();
            if idle
                && !p.control.is_cancelled()
                && self.key.as_ref() == Some(&p.key)
                && generation == p.generation
            {
                match result {
                    Ok(guide) => {
                        self.guide = Some(guide);
                        self.published = Some(p.key);
                        self.publications += 1;
                        self.analysed_time = p.time;
                    }
                    Err(e) => self.error = Some(e),
                }
                host.dirty = true;
                host.invalidate_snapshot();
            }
        }
        let animated = host.session.engine().document().has_animated_effects()
            && host.session.engine().animation_time() != self.analysed_time
            && self
                .last_start
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(500));
        if self.pending.is_some()
            || self.key.is_none()
            || (self.published == self.key && !animated)
            || !idle
            || self.changed.elapsed() < Duration::from_millis(180)
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
        self.last_start = Some(Instant::now());
        let thread = std::thread::Builder::new()
            .name("windows-hdr-analysis".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut snapshot = gpu
                        .capture(project, background, time, Default::default(), cancel)
                        .map_err(|e| e.to_string())?;
                    snapshot.gpu_local_tone_guide()
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
    pub fn preview(&self, host: &NativeHost, generation: u64) -> Option<Guide> {
        (self.generation == generation
            && self
                .published
                .as_ref()
                .is_some_and(|key| key.can_preview_current(&host.session)))
        .then(|| self.guide.clone())
        .flatten()
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"ready":self.key.is_some() && self.published == self.key,"retained":self.guide.is_some(),"pending":self.pending.is_some(),"publications":self.publications,"error":self.error})
    }
    pub fn stop(&mut self) -> Result<(), String> {
        let result = if let Some(p) = self.pending.take() {
            p.control.cancel();
            p.thread
                .join()
                .map_err(|_| "HDR worker shutdown failed".to_string())
        } else {
            Ok(())
        };
        // GPU guides retain the removed device. Release them before replacement.
        self.guide = None;
        self.published = None;
        self.key = None;
        self.error = None;
        result
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
