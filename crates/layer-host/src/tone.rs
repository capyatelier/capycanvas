//! One bounded, superseding GPU analysis worker for the HDR guide. Compatible
//! completed guides stay visible while drawing; new guides publish only at an
//! idle boundary.
use crate::NativeHost;
use layer_render_wgpu::{local_tone::GpuToneGuide, snapshot::CaptureControl};
use layer_ui::proof_workflow::ToneKey;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, PartialEq)]
struct Key {
    tone: ToneKey,
    device: wgpu::Device,
}
struct Pending {
    key: Key,
    time: f32,
    control: CaptureControl,
    receiver: mpsc::Receiver<Result<Arc<GpuToneGuide>, String>>,
}

#[derive(Default)]
pub struct ToneService {
    stamp: Option<(u64, u64, wgpu::Device)>,
    wanted: Option<Key>,
    published: Option<Key>,
    pending: Option<Pending>,
    changed: Option<Instant>,
    last_start: Option<Instant>,
    sampled_time: f32,
    publications: u64,
    wake: Option<Wake>,
    pub guide: Option<Arc<GpuToneGuide>>,
    pub error: Option<String>,
}

impl ToneService {
    /// `wake` runs on the worker after each result is sent.
    pub fn new(wake: Option<Wake>) -> Self {
        let mut service = Self::default();
        service.wake = wake;
        service
    }

    /// Cancel analysis and drop guides that retain a retired device.
    pub fn clear(&mut self) {
        if let Some(p) = self.pending.take() {
            p.control.cancel();
        }
        let mut cleared = Self::new(self.wake.take());
        cleared.publications = self.publications;
        *self = cleared;
    }

    pub fn current(&self, host: &NativeHost) -> Option<Arc<GpuToneGuide>> {
        let key = self.published.as_ref()?;
        let gpu = host.session.engine().backend().0.as_ref()?;
        (key.device == *gpu.device() && key.tone.can_preview_current(&host.session))
            .then(|| self.guide.clone())
            .flatten()
    }

    pub fn publications(&self) -> u64 {
        self.publications
    }

    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "ready": self.wanted.is_some() && self.published == self.wanted,
            "retained": self.guide.is_some(),
            "pending": self.pending.is_some(),
            "publications": self.publications,
            "error": self.error,
        })
    }

    /// Returns true when the guide or the status changed.
    pub fn tick(&mut self, host: &NativeHost) -> Result<bool, String> {
        let s = &host.session;
        let d = s.engine().document();
        let Some(gpu) = s
            .engine()
            .backend()
            .0
            .as_ref()
            .filter(|_| d.color.depth.is_float() && !s.rendering_suspended())
        else {
            let changed = self.wanted.is_some() || self.guide.is_some() || self.error.is_some();
            self.clear();
            return Ok(changed);
        };
        let mut changed = false;
        let stamp = (
            s.state().document_file.epoch,
            d.revision,
            gpu.device().clone(),
        );
        if self.stamp.as_ref() != Some(&stamp) {
            let key = Key {
                tone: ToneKey::current(s).ok_or("HDR analysis is unavailable")?,
                device: stamp.2.clone(),
            };
            if self.wanted.as_ref() != Some(&key) {
                if let Some(p) = &self.pending {
                    p.control.cancel();
                }
                if !self
                    .published
                    .as_ref()
                    .is_some_and(|old| old.device == key.device && old.tone.can_preview(&key.tone))
                {
                    self.published = None;
                    self.guide = None;
                }
                self.wanted = Some(key);
                self.error = None;
                self.changed = Some(Instant::now());
                changed = true;
            }
            self.stamp = Some(stamp);
        }
        let idle =
            !s.state().document_file.close_ready && s.require_document_snapshot_idle().is_ok();
        if !idle {
            if let Some(p) = &self.pending {
                p.control.cancel();
            }
            self.changed = Some(Instant::now());
        }
        if let Some(p) = &self.pending {
            match p.receiver.try_recv() {
                Err(mpsc::TryRecvError::Empty) => (),
                result => {
                    let p = self.pending.take().unwrap();
                    changed = true;
                    if idle && !p.control.is_cancelled() && self.wanted.as_ref() == Some(&p.key) {
                        match result.unwrap_or_else(|_| Err("SDR analysis worker stopped".into())) {
                            Ok(guide) => {
                                self.published = Some(p.key);
                                self.sampled_time = p.time;
                                self.publications += 1;
                                self.guide = Some(guide);
                                self.error = None;
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                }
            }
        }
        let time = if d.has_animated_effects() {
            s.engine().animation_time()
        } else {
            0.
        };
        let refresh = time != self.sampled_time
            && self
                .last_start
                .is_none_or(|v| v.elapsed() >= Duration::from_millis(500));
        if idle
            && self.pending.is_none()
            && self.error.is_none()
            && (self.wanted != self.published || refresh)
            && self
                .changed
                .is_none_or(|v| v.elapsed() >= Duration::from_millis(180))
        {
            let project = s.capture_project_recovery()?;
            let capture = gpu.snapshot_gpu();
            let key = self.wanted.clone().unwrap();
            let background = key.tone.background();
            let control = CaptureControl::default();
            let cancel = control.clone();
            let wake = self.wake.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("capy-local-tone".into())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        capture
                            .capture(project, background, time, Default::default(), cancel)
                            .map_err(|e| e.to_string())
                            .and_then(|mut snapshot| snapshot.gpu_local_tone_guide())
                    }))
                    .unwrap_or_else(|_| Err("SDR analysis worker failed".into()));
                    let _ = sender.send(result);
                    if let Some(wake) = wake {
                        wake();
                    }
                })
                .map_err(|e| e.to_string())?;
            self.pending = Some(Pending {
                key,
                time,
                control,
                receiver,
            });
            self.last_start = Some(Instant::now());
            changed = true;
        }
        Ok(changed)
    }
}

impl Drop for ToneService {
    fn drop(&mut self) {
        if let Some(p) = &self.pending {
            p.control.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use layer_core::color::SampleDepth;
    use layer_render_wgpu::WgpuRasterizer;
    use layer_ui::{CommandId, UiAction, UiSession};

    fn settle(tone: &mut ToneService, host: &mut NativeHost) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while tone.status()["ready"] != true {
            host.session.frame(0, 0).unwrap();
            tone.tick(host).unwrap();
            assert!(tone.error.is_none(), "{:?}", tone.error);
            assert!(Instant::now() < deadline, "HDR analysis timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn tone_service_publishes_only_the_current_key_and_retains_compatible_guides() {
        let mut document = layer_core::Document::new("HDR", 32, 24);
        document.color.depth = SampleDepth::F32;
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session =
            UiSession::new(Renderer(Some(gpu.into())), document.clone(), [32, 24], layer_ui::Platform::Mac).unwrap();
        let mut tone = ToneService::new(None);
        settle(&mut tone, &mut host);
        let guide = tone.current(&host).unwrap();
        host.dispatch(UiAction::Invoke {
            command: CommandId::AddLayer,
        })
        .unwrap();
        assert!(tone.tick(&host).unwrap());
        assert_eq!(tone.status()["ready"], false);
        assert!(Arc::ptr_eq(&tone.current(&host).unwrap(), &guide));
        settle(&mut tone, &mut host);
        assert_eq!(tone.status()["publications"], 2);
        assert!(!Arc::ptr_eq(&tone.current(&host).unwrap(), &guide));
        let context = crate::GpuContext::of(host.session.engine().backend().0.as_ref().unwrap());
        let renderer = context
            .rasterizer(document.color, &Default::default(), true)
            .unwrap();
        let mut replacement =
            UiSession::new(Renderer(Some(renderer.into())), document, [32, 24], layer_ui::Platform::Mac).unwrap();
        replacement.inherit_window_state(&host.session).unwrap();
        host.session = replacement;
        assert!(tone.current(&host).is_none());
        assert!(tone.tick(&host).unwrap());
        assert!(tone.guide.is_none());
        assert_eq!(tone.status()["retained"], false);
    }
}
