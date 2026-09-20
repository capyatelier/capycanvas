//! One bounded, superseding analysis worker. Camera and recipe edits reuse the
//! full-image guide; immutable snapshots and cancellation belong to Rust.
use layer_core::{
    Layer,
    color::{DocumentColor, hdr::LocalToneGuide},
};
use layer_host::NativeHost;
use layer_render_wgpu::snapshot::CaptureControl;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq)]
struct Key {
    epoch: u64,
    device: wgpu::Device,
    color: DocumentColor,
    extent: [u32; 2],
    background: [f32; 4],
    layers: Vec<Layer>,
}
struct Pending {
    key: Key,
    time: f32,
    control: CaptureControl,
    receiver: mpsc::Receiver<Result<Arc<LocalToneGuide>, String>>,
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
    pub guide: Option<Arc<LocalToneGuide>>,
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
            let changed = self.guide.is_some();
            self.clear();
            return Ok(changed);
        };
        let stamp = (
            s.state().document_file.epoch,
            d.revision,
            gpu.device().clone(),
        );
        if self.stamp.as_ref() != Some(&stamp) {
            let key = Key {
                epoch: stamp.0,
                device: stamp.2.clone(),
                color: d.color,
                extent: [d.width, d.height],
                background: s.engine().view().background_rgba_linear,
                layers: d.layers.iter().map(Layer::composite_snapshot).collect(),
            };
            if self.wanted.as_ref() != Some(&key) {
                if let Some(p) = &self.pending {
                    p.control.cancel();
                }
                self.wanted = Some(key);
                self.published = None;
                self.guide = None;
                self.error = None;
                self.changed = Some(Instant::now());
            }
            self.stamp = Some(stamp);
        }
        let mut changed = false;
        if let Some(p) = &self.pending {
            match p.receiver.try_recv() {
                Err(mpsc::TryRecvError::Empty) => (),
                result => {
                    let p = self.pending.take().unwrap();
                    if !p.control.is_cancelled() && self.wanted.as_ref() == Some(&p.key) {
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
        if self.pending.is_none()
            && (self.wanted != self.published || refresh)
            && self
                .changed
                .is_none_or(|v| v.elapsed() >= Duration::from_millis(180))
            && s.require_document_snapshot_idle().is_ok()
        {
            let project = s.capture_project_recovery()?;
            let gpu = gpu.snapshot_gpu();
            let key = self.wanted.clone().unwrap();
            let background = key.background;
            let control = CaptureControl::default();
            let c = control.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("capy-local-tone".into())
                .spawn(move || {
                    let result = gpu
                        .capture(project, background, time, Default::default(), c)
                        .map_err(|e| e.to_string())
                        .and_then(|mut snapshot| snapshot.local_tone_guide());
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
