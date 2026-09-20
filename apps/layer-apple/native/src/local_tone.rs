//! One bounded, superseding GPU analysis worker. Compatible completed guides
//! stay visible while drawing; new guides publish only at an idle boundary.
use layer_host::NativeHost;
use layer_render_wgpu::{local_tone::GpuToneGuide, snapshot::CaptureControl};
use layer_ui::proof_workflow::ToneKey;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

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
pub(crate) struct LocalTone {
    stamp: Option<(u64, u64, wgpu::Device)>,
    wanted: Option<Key>,
    published: Option<Key>,
    pending: Option<Pending>,
    changed: Option<Instant>,
    last_start: Option<Instant>,
    sampled_time: f32,
    pub guide: Option<Arc<GpuToneGuide>>,
    pub error: Option<String>,
    pub completed: u64,
}
impl LocalTone {
    pub fn clear(&mut self) {
        if let Some(p) = self.pending.take() {
            p.control.cancel();
        }
        *self = Self::default();
    }
    pub fn current(&self, host: &NativeHost) -> Option<Arc<GpuToneGuide>> {
        let key = self.published.as_ref()?;
        let gpu = host.session.engine().backend().0.as_ref()?;
        (key.device == *gpu.device() && key.tone.can_preview_current(&host.session))
            .then(|| self.guide.clone())
            .flatten()
    }
    pub fn tick(&mut self, host: &mut NativeHost) -> Result<bool, String> {
        let s = &host.session;
        let d = s.engine().document();
        let Some(gpu) = s
            .engine()
            .backend()
            .0
            .as_ref()
            .filter(|_| d.color.depth.is_float() && !s.rendering_suspended())
        else {
            let changed = self.guide.is_some() || self.error.is_some();
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
        let idle = s.require_document_snapshot_idle().is_ok();
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
                    if idle && !p.control.is_cancelled() && self.wanted.as_ref() == Some(&p.key) {
                        self.published = Some(p.key);
                        self.sampled_time = p.time;
                        self.completed += 1;
                        match result.unwrap_or_else(|_| Err("SDR analysis worker stopped".into())) {
                            Ok(guide) => {
                                self.guide = Some(guide);
                                self.error = None;
                            }
                            Err(error) => self.error = Some(error),
                        }
                        changed = true;
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
            && (self.wanted != self.published || refresh)
            && self
                .changed
                .is_none_or(|v| v.elapsed() >= Duration::from_millis(180))
        {
            let project = s.capture_project_recovery()?;
            let gpu = gpu.snapshot_gpu();
            let key = self.wanted.clone().unwrap();
            let background = key.tone.background();
            let control = CaptureControl::default();
            let c = control.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("capy-local-tone".into())
                .spawn(move || {
                    let result = gpu
                        .capture(project, background, time, Default::default(), c)
                        .map_err(|e| e.to_string())
                        .and_then(|mut snapshot| snapshot.gpu_local_tone_guide());
                    let _ = sender.send(result);
                })
                .map_err(|e| e.to_string())?;
            self.pending = Some(Pending {
                key,
                time,
                control,
                receiver,
            });
            self.last_start = Some(Instant::now());
        }
        Ok(changed)
    }
}
impl Drop for LocalTone {
    fn drop(&mut self) {
        if let Some(p) = &self.pending {
            p.control.cancel();
        }
    }
}
