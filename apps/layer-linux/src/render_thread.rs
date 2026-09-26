//! Bounded frame handoff and canvas GPU/WSI owner. No GTK calls on the worker.
//! Small dab/layer records cross threads; live canvas pixels remain on the GPU.
use crate::wayland::{Child, Geometry, Parent};
use gtk::prelude::WidgetExt;
use layer_core::{AssetId, Layer};
use layer_render::{
    BackendError, BrushSource, CanvasRenderer, CursorSegment, Dab, DabBatch, FramePacket, HostImage,
    PixelFormat, ReadbackImage, TipOutline, ViewState,
};
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
mod color;
#[cfg(test)]
mod tests;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct Frame {
    time_seconds: f32,
    view: ViewState,
    extent: [u32; 2],
    layers: Vec<Layer>,
    dabs: Vec<Dab>,
    batches: Vec<DabBatch>,
    restore_rasters: Vec<(layer_core::LayerId, layer_core::raster::RasterRevision)>,
    reset: bool,
    composite: bool,
    geometry: Geometry,
    surround: [f32; 4],
    picker: Option<layer_render::ColorPickerOverlay>,
    cursor: Vec<CursorSegment>,
    overviews: Vec<layer_render_wgpu::OverviewPlacement>,
    backdrops: Vec<layer_render_wgpu::BackdropRegion>,
    backdrop_style: layer_render_wgpu::BackdropBlurStyle,
    backdrop_hold: bool,
    stroke_target: Option<crate::wayland::StrokeTarget>,
    // Every queued producer must resolve its immutable roots, even on failure.
    pending_rasters: Vec<layer_core::raster::RasterRevision>,
    #[cfg(test)]
    queued_ns: u64,
}
impl Drop for Frame {
    fn drop(&mut self) {
        for raster in &self.pending_rasters {
            if raster.try_data().is_none() {
                let _ = raster.publish(Err("GPU worker stopped before raster capture".into()));
            }
        }
    }
}
impl Frame {
    fn packet(&self) -> FramePacket<'_> {
        FramePacket {
            time_seconds: self.time_seconds,
            view: self.view,
            document_extent: self.extent,
            layers: &self.layers,
            dabs: &self.dabs,
            dab_batches: &self.batches,
            restore_rasters: &self.restore_rasters,
            reset_layers: self.reset,
            composite_all: self.composite,
        }
    }
}
enum Command {
    LocalTone(Option<Arc<layer_render_wgpu::local_tone::GpuToneGuide>>, mpsc::Sender<Result<(),String>>),
    Proof(Option<Arc<layer_color::ProofLut>>, bool, bool, mpsc::Sender<Result<(), String>>),
    HdrView(Option<layer_core::color::hdr::SdrRendition>, bool),
    PrepareColor(Box<color::Request>),
    AdoptColor(u64),
    DiscardColor(u64, mpsc::Sender<()>),
    TransformPreview(Option<layer_render::TransformPreview>),
    Startup(
        u64,
        Box<(layer_core::Document, layer_core::BrushSnapshot, bool)>,
    ),
    FinishStartupCache,
    Selection(Option<layer_core::Selection>),
    SelectionPaint(u64, layer_render::SelectionPaint),
    CancelSelectionPaint(u64),
    SelectionOverlay(Option<layer_render::SelectionOverlay>),
    QuickMaskThumbnail(Option<layer_core::Selection>),
    Region(layer_render::RegionRequest),
    EffectValidation(layer_render::EffectValidationRequest),
    Telemetry(bool),
    Thumbnail(u64, layer_core::LayerId),
    ColorSample(layer_render::ColorSampleRequest),
    FilterPreviews(u64, layer_render::FilterPreviewRequest),
    CancelFilterPreviews(u64),
    Frame(Box<Frame>),
    Asset(AssetId, layer_core::ProjectAsset),
    Release(AssetId),
    #[cfg(test)]
    DocumentPixels(u64, mpsc::Sender<Result<ReadbackImage, String>>),
    Capture(crate::display_color::ViewColor, mpsc::Sender<Result<ReadbackImage, String>>),
    Stop,
    #[cfg(test)]
    FailNextFrame,
}
enum Reply {
    ColorAdopted(u64, HashMap<AssetId, BrushSource>, layer_render_wgpu::snapshot::SnapshotGpu, Option<layer_render_wgpu::ShaderActivity>),
    Initialized(crate::display_color::ViewColor, layer_render_wgpu::snapshot::SnapshotGpu, Option<layer_render_wgpu::ShaderActivity>),
    Startup(
        u64,
        layer_render_wgpu::StartupProgress,
        HashMap<AssetId, BrushSource>,
    ),
    Region(Result<layer_render::RegionResult, String>),
    SelectionPaintAck(u64, Result<bool, String>),
    SelectionPaint(u64, Result<layer_render::SelectionPaintResult, String>),
    EffectValidation(layer_render::EffectValidationResult),
    Thumbnail(ReadbackImage),
    ColorSample(Result<layer_render::ColorSample, String>),
    FilterPreviews(u64, Result<layer_render::FilterPreviewImage, String>),
    DisplayHeadroom(f32, Option<layer_render_wgpu::SdrSurfaceColor>),
    Error(String),
}

#[cfg(test)]
static NEXT_STARTUP_PAUSE: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn pause_next_startup() -> Arc<AtomicBool> {
    let pause = Arc::new(AtomicBool::new(true));
    *NEXT_STARTUP_PAUSE.lock().unwrap() = Some(pause.clone());
    pause
}

/// Two in-flight paint frames, including the frame being presented. GTK never
/// waits on a worker lock, Vulkan acquire, or a GPU completion fence.
pub struct RenderWorker {
    shader_activity: Option<layer_render_wgpu::ShaderActivity>,
    #[cfg(test)]
    startup_pause: Option<Arc<AtomicBool>>,
    pub(crate) proof_owner: u64,
    hdr_view: Option<(Option<layer_core::color::hdr::SdrRendition>, bool)>,
    transform_preview: Option<layer_render::TransformPreview>,
    initialized: bool,
    pub(crate) display_headroom: f32,
    pub(crate) display_encoding: Option<layer_render_wgpu::SdrSurfaceColor>,
    snapshot_gpu: Option<layer_render_wgpu::snapshot::SnapshotGpu>,
    pub(crate) view_color: crate::display_color::ViewColor,
    first_frame_sent: bool,
    pub(super) startup: layer_render_wgpu::StartupProgress,
    startup_generation: u64,
    startup_key: Option<(layer_render_wgpu::ShaderDocument, layer_core::BrushSnapshot, bool)>,
    selection: Option<layer_core::Selection>,
    region: Option<Result<layer_render::RegionResult, String>>,
    region_pending: bool,
    selection_generation: u64,
    selection_update_pending: bool,
    selection_ack: Option<Result<bool, String>>,
    selection_paint: Option<Result<layer_render::SelectionPaintResult, String>>,
    selection_overlay: Option<layer_render::SelectionOverlay>,
    quick_thumbnail: Option<layer_core::Selection>,
    pub(super) clock: Arc<crate::wayland::FrameClock>,
    telemetry: Arc<std::sync::Mutex<layer_render::RendererTelemetry>>,
    telemetry_enabled: bool,
    commands: mpsc::Sender<Command>,
    replies: mpsc::Receiver<Reply>,
    failure: Arc<std::sync::OnceLock<String>>,
    in_flight: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
    color: layer_core::color::DocumentColor,
    next_color_request: u64,
    pending_color: Option<color::Pending>,
    awaiting_color_adoption: Option<u64>,
    brush_sources: HashMap<AssetId, BrushSource>,
    thumbnails: VecDeque<ReadbackImage>,
    color_sample: Option<Result<layer_render::ColorSample, String>>,
    color_sample_pending: bool,
    filter_previews: VecDeque<Result<layer_render::FilterPreviewImage, String>>,
    filter_previews_pending: bool,
    filter_preview_generation: u64,
    effect_validation_pending: bool,
    effect_validation: Option<layer_render::EffectValidationResult>,
    pub(super) geometry: Option<Geometry>,
    pub(super) surround: [f32; 4],
    pub(super) picker: Option<layer_render::ColorPickerOverlay>,
    pub(super) cursor: Vec<CursorSegment>,
    pub(super) overviews: Vec<layer_render_wgpu::OverviewPlacement>,
    pub(super) backdrops: Vec<layer_render_wgpu::BackdropRegion>,
    pub(super) backdrop_style: layer_render_wgpu::BackdropBlurStyle,
    pub(super) backdrop_hold: bool,
    pub(super) stroke_target: Option<crate::wayland::StrokeTarget>,
    #[cfg(test)]
    pub stats: Arc<std::sync::Mutex<crate::timing::Stats>>,
}
impl RenderWorker {
    pub(crate) fn set_local_tone(
        &self,
        guide: Option<Arc<layer_render_wgpu::local_tone::GpuToneGuide>>,
    ) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::LocalTone(guide, tx)).map_err(error)?;
        Ok(rx)
    }

    pub(crate) fn set_hdr_view(&mut self, rendition: Option<layer_core::color::hdr::SdrRendition>, preview_sdr: bool) -> Result<(), String> {
        if self.hdr_view != Some((rendition, preview_sdr)) {
            self.send(Command::HdrView(rendition, preview_sdr)).map_err(error)?;
            self.hdr_view = Some((rendition, preview_sdr));
        }
        Ok(())
    }

    pub(crate) fn set_proof(&self, lut: Option<Arc<layer_color::ProofLut>>, enabled: bool, gamut: bool)
        -> Result<mpsc::Receiver<Result<(), String>>, String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Proof(lut, enabled, gamut, tx)).map_err(error)?;
        Ok(rx)
    }
    #[cfg(test)]
    pub(super) fn document_pixels(&self, request_id: u64) -> Result<ReadbackImage, String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::DocumentPixels(request_id, tx)).map_err(error)?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(error)?
    }
    pub(super) fn capture(&self) -> Result<ReadbackImage, String> {
        self.capture_in(crate::display_color::ViewColor::Srgb)
    }
    pub(super) fn capture_in(&self, view: crate::display_color::ViewColor) -> Result<ReadbackImage, String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Capture(view, tx)).map_err(error)?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(error)?
    }
    pub(super) fn new(
        parent: Parent,
        area: gtk::glib::SendWeakRef<gtk::Picture>,
        color: layer_core::color::DocumentColor,
    ) -> Result<Self, String> {
        #[cfg(test)]
        let startup_pause = NEXT_STARTUP_PAUSE.lock().unwrap().take();
        #[cfg(test)]
        let pause = startup_pause.clone();
        static NEXT_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let proof_owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
        let (commands, receiver) = mpsc::channel();
        let (reply, replies) = mpsc::channel();
        let failure = Arc::new(std::sync::OnceLock::new());
        let worker_failure = failure.clone();
        let clock = Arc::new(crate::wayland::FrameClock::default());
        let worker_clock = clock.clone();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let count = in_flight.clone();
        let telemetry = Arc::new(std::sync::Mutex::new(
            layer_render::RendererTelemetry::default(),
        ));
        let worker_telemetry = telemetry.clone();
        let failure_area = area.clone();
        #[cfg(test)]
        let stats = Arc::new(std::sync::Mutex::new(crate::timing::Stats::default()));
        #[cfg(test)]
        let worker_stats = stats.clone();
        let thread = std::thread::Builder::new()
            .name("canvas-gpu".into())
            .spawn(move || {
                // A panic retires this entire owner; no encoder or renderer
                // state is reused after unwinding.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    #[cfg(test)]
                    if let Some(pause) = pause {
                        while pause.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    Worker::new(parent, area, worker_clock, color)?.run(
                        &receiver,
                        &reply,
                        &worker_telemetry,
                        &count,
                        #[cfg(test)]
                        worker_stats,
                    )
                }))
                .unwrap_or_else(|payload| {
                    Err(payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "GPU worker panicked".into()))
                });
                if let Err(error) = &result {
                    // Optional result polls may consume Reply::Error. Keep the
                    // cause available for every subsequent readiness check and
                    // record it before disconnecting pending command sends.
                    eprintln!("Canvas GPU worker failed: {error}");
                }
                // Resolve abandoned producers before the session observes loss.
                drop(receiver);
                if let Err(error) = result {
                    let _ = worker_failure.set(error.clone());
                    let _ = reply.send(Reply::Error(error));
                    // GTK may already be idle. Schedule, never invoke inline
                    // on the worker when the main context is temporarily free.
                    gtk::glib::idle_add_once(move || {
                        if let Some(area) = failure_area.upgrade() {
                            let _ = area.activate_action("canvas.worker-stopped", None);
                        }
                    });
                }
            })
            .map_err(error)?;
        Ok(Self {
            shader_activity: None,
            #[cfg(test)]
            startup_pause,
            proof_owner,
            hdr_view: None,
            transform_preview: None,
            initialized: false,
            display_headroom: 1.,
            display_encoding: None,
            snapshot_gpu: None,
            view_color: Default::default(),
            first_frame_sent: false,
            startup: Default::default(),
            startup_generation: 0,
            startup_key: None,
            selection: None,
            region: None,
            region_pending: false,
            selection_generation: 0,
            selection_update_pending: false,
            selection_ack: None,
            selection_paint: None,
            selection_overlay: None,
            quick_thumbnail: None,
            clock,
            stroke_target: None,
            telemetry,
            telemetry_enabled: false,
            commands,
            replies,
            failure,
            in_flight,
            thread: Some(thread),
            color,
            next_color_request: 0,
            pending_color: None,
            awaiting_color_adoption: None,
            brush_sources: HashMap::new(),
            thumbnails: VecDeque::new(),
            color_sample: None,
            color_sample_pending: false,
            filter_previews: VecDeque::new(),
            filter_previews_pending: false,
            filter_preview_generation: 0,
            effect_validation_pending: false,
            effect_validation: None,
            geometry: None,
            surround: [0.033; 4],
            picker: None,
            cursor: Vec::new(),
            overviews: Vec::new(),
            backdrops: Vec::new(),
            backdrop_style: Default::default(),
            backdrop_hold: false,
            #[cfg(test)]
            stats,
        })
    }
    #[cfg(test)]
    pub(super) fn fail_next_frame(&self) {
        self.send(Command::FailNextFrame).unwrap();
    }
    fn send(&self, command: Command) -> Result<(), BackendError> {
        self.commands
            .send(command)
            .map_err(|_| BackendError("GPU worker stopped"))
    }
    pub(super) fn finish_startup_cache(&self) -> Result<(), String> {
        self.send(Command::FinishStartupCache).map_err(error)
    }
    pub(super) fn startup_needs_update(
        &self,
        document: &layer_core::Document,
        brush: &layer_core::BrushSnapshot,
        transform: bool,
    ) -> bool {
        self.startup_key.as_ref().is_none_or(|(key, old, previous)| {
            !key.matches(document) || old != brush || *previous != transform
        })
    }
    pub(super) fn prepare_startup(
        &mut self,
        document: layer_core::Document,
        brush: layer_core::BrushSnapshot,
        transform: bool,
    ) -> Result<(), String> {
        self.startup_generation += 1;
        self.startup_key = Some((layer_render_wgpu::ShaderDocument::new(&document), brush.clone(), transform));
        self.startup = Default::default();
        self.send(Command::Startup(
            self.startup_generation,
            Box::new((document, brush, transform)),
        ))
        .map_err(error)
    }
    pub(super) fn paint_ready(
        &self,
        document: &layer_core::Document,
        brush: &layer_core::BrushSnapshot,
        transform: bool,
    ) -> bool {
        self.startup.brush_ready && !self.startup_needs_update(document, brush, transform)
    }
    pub(super) fn ready(&mut self) -> Result<bool, String> {
        if let Some(error) = self.failure.get() {
            self.snapshot_gpu = None;
            return Err(error.clone());
        }
        while let Ok(reply) = self.replies.try_recv() {
            if let Some(id) = self.awaiting_color_adoption {
                match reply {
                    Reply::ColorAdopted(current, brush_sources, gpu, activity) if current == id => {
                        self.shader_activity = activity;
                        // Keep the new renderer's pipeline/cache context. Exact
                        // source samples remain shared with existing file jobs.
                        self.awaiting_color_adoption = None;
                        self.brush_sources = brush_sources;
                        self.snapshot_gpu = Some(gpu);
                    }
                    Reply::DisplayHeadroom(headroom, encoding) => { self.display_headroom = headroom; self.display_encoding = encoding; },
                Reply::Error(error) => { self.snapshot_gpu = None; return Err(error); },
                    _ => (),
                }
                continue;
            }
            match reply {
                Reply::ColorAdopted(..) => (),
                Reply::Initialized(color, gpu, activity) => {
                    self.shader_activity = activity;
                    self.initialized = true;
                    self.view_color = color;
                    self.snapshot_gpu = Some(gpu);
                },
                Reply::Startup(generation, progress, brush_sources) => {
                    if generation == self.startup_generation {
                        self.startup = progress;
                        self.brush_sources = brush_sources;
                    }
                }
                Reply::SelectionPaintAck(generation, result) => {
                    if generation == self.selection_generation {
                        self.selection_update_pending = false; self.selection_ack = Some(result);
                    }
                }
                Reply::SelectionPaint(generation, result) => {
                    if generation == self.selection_generation { self.selection_paint = Some(result); }
                }
                Reply::Region(result) => {
                    self.region_pending = false;
                    self.region = Some(result);
                }
                Reply::EffectValidation(result) => {
                    self.effect_validation_pending = false;
                    self.effect_validation = Some(result);
                }
                Reply::Thumbnail(image) => self.thumbnails.push_back(image),
                Reply::ColorSample(color) => {
                    self.color_sample = Some(color);
                    self.color_sample_pending = false;
                }
                Reply::FilterPreviews(generation, image) => {
                    if generation == self.filter_preview_generation {
                        self.filter_previews_pending = false;
                        self.filter_previews.push_back(image);
                    }
                }
                Reply::DisplayHeadroom(headroom, encoding) => { self.display_headroom = headroom; self.display_encoding = encoding; },
                Reply::Error(error) => { self.snapshot_gpu = None; return Err(error); },
            }
        }
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            self.snapshot_gpu = None;
            return Err("GPU worker stopped".into());
        }
        Ok(self.initialized
            && (!self.first_frame_sent || self.startup.canvas_ready)
            && self.in_flight.load(Ordering::Acquire) < 2)
    }
    #[cfg(test)]
    pub(super) fn worker_is_joined(&self) -> bool {
        self.thread.is_none()
    }
    #[cfg(test)]
    pub(super) fn frames_idle(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) == 0
    }
    pub(crate) fn snapshot_gpu(&self) -> Result<layer_render_wgpu::snapshot::SnapshotGpu, String> {
        if self.thread.as_ref().is_none_or(|thread| thread.is_finished()) {
            return Err("Canvas renderer stopped".into());
        }
        self.snapshot_gpu.clone().ok_or_else(|| "Canvas renderer is still preparing".into())
    }
    pub(super) fn stop(&mut self) {
        #[cfg(test)]
        if let Some(pause) = &self.startup_pause { pause.store(false, Ordering::Release); }
        self.snapshot_gpu = None;
        let _ = self.discard_prepared_color();
        if let Some(thread) = self.thread.take() {
            let _ = self.send(Command::Stop);
            // Wayland children must die before GTK releases their parent.
            let _ = thread.join();
        }
        // Unconsumed initialization replies also own a device handle.
        for reply in self.replies.try_iter() { drop(reply); }
        self.thumbnails.clear();
        self.filter_previews.clear();
        self.color_sample = None;
        self.region = None;
        self.selection = None;
        self.brush_sources.clear();
        self.startup_key = None;
    }
}
impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
impl CanvasRenderer for RenderWorker {
    fn shader_input(&mut self) { if let Some(activity) = &self.shader_activity { activity.input(); } }
    fn shader_idle(&mut self, idle: bool) { if let Some(activity) = &self.shader_activity { activity.idle(idle); } }
    fn shaders_need_update(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.startup_needs_update(document, brush, transform)
    }
    fn document_color(&self) -> layer_core::color::DocumentColor { self.color }
    fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> { self.adopt_color(color) }
    fn supports_tiled_sources(&self) -> bool { true }
    fn supports_raster_damage(&self) -> bool { true }
    fn can_submit(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) < 2
    }
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        if self.transform_preview.as_ref() != preview {
            self.send(Command::TransformPreview(preview.cloned()))?;
            self.transform_preview = preview.cloned();
        }
        Ok(())
    }
    fn paint_selection(&mut self, update: &layer_render::SelectionPaint) -> Result<bool,Self::Error> {
        self.ready().map_err(|_| BackendError("Selection worker unavailable"))?;
        if let Some(ack) = self.selection_ack.take() { return ack.map_err(|message| {
            eprintln!("Selection paint: {message}"); BackendError("Selection painting failed") }); }
        if !self.selection_update_pending {
            self.send(Command::SelectionPaint(self.selection_generation, update.clone()))?;
            self.selection_update_pending = true;
        }
        Ok(false)
    }
    fn take_selection_paint(&mut self) -> Option<Result<layer_render::SelectionPaintResult,Self::Error>> {
        self.ready().ok()?;
        self.selection_paint.take().map(|r| r.map_err(|_| BackendError("Selection capture failed")))
    }
    fn cancel_selection_paint(&mut self) {
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.selection_update_pending = false; self.selection_ack = None; self.selection_paint = None;
        self.selection = None;
        let _ = self.send(Command::CancelSelectionPaint(self.selection_generation));
    }
    fn set_quick_mask_thumbnail(&mut self, selection: Option<&layer_core::Selection>) {
        if self.quick_thumbnail.as_ref() != selection {
            self.quick_thumbnail = selection.cloned();
            let _ = self.send(Command::QuickMaskThumbnail(self.quick_thumbnail.clone()));
        }
    }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) {
        if self.selection_overlay != overlay {
            let _ = self.send(Command::SelectionOverlay(overlay));
            self.selection_overlay = overlay; self.selection = None;
        }
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        if self.region_pending {
            return Ok(false);
        }
        self.send(Command::Region(request))?;
        self.region_pending = true;
        Ok(true)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        self.ready().ok()?;
        self.region
            .take()
            .map(|result| result.map_err(|_| BackendError("Region detection failed")))
    }
    fn set_selection_outline(
        &mut self,
        selection: Option<&layer_core::Selection>,
    ) -> Result<(), Self::Error> {
        if self.selection.as_ref() != selection {
            self.send(Command::Selection(selection.cloned()))?;
            self.selection = selection.cloned();
        }
        Ok(())
    }
    fn request_effect_validation(
        &mut self,
        request: layer_render::EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        if self.effect_validation_pending {
            return Ok(false);
        }
        self.send(Command::EffectValidation(request))?;
        self.effect_validation_pending = true;
        Ok(true)
    }
    fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
        self.ready().ok()?;
        self.effect_validation.take()
    }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        if self.telemetry_enabled != enabled {
            self.telemetry_enabled = enabled;
            let _ = self.send(Command::Telemetry(enabled));
        }
    }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        self.telemetry
            .try_lock()
            .map(|t| t.clone())
            .unwrap_or_default()
    }
    fn request_thumbnail(
        &mut self,
        id: u64,
        target: layer_core::LayerId,
    ) -> Result<(), Self::Error> {
        self.send(Command::Thumbnail(id, target))
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.thumbnails.pop_front().map(Ok)
    }
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        if self.color_sample_pending {
            return Ok(false);
        }
        self.send(Command::ColorSample(request))?;
        self.color_sample_pending = true;
        Ok(true)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.ready().ok()?;
        self.color_sample
            .take()
            .map(|result| result.map_err(|_| BackendError("Color sample unavailable")))
    }
    fn request_filter_previews(
        &mut self,
        request: layer_render::FilterPreviewRequest,
    ) -> Result<bool, Self::Error> {
        if self.filter_previews_pending {
            return Ok(false);
        }
        self.send(Command::FilterPreviews(self.filter_preview_generation, request))?;
        self.filter_previews_pending = true;
        Ok(true)
    }
    fn take_filter_previews(
        &mut self,
    ) -> Option<Result<layer_render::FilterPreviewImage, Self::Error>> {
        self.ready().ok()?;
        self.filter_previews.pop_front().map(|r| {
            r.map_err(|message| {
                eprintln!("Filter preview: {message}");
                BackendError("Filter preview failed")
            })
        })
    }
    fn cancel_filter_previews(&mut self) {
        self.filter_preview_generation = self.filter_preview_generation.wrapping_add(1);
        let _ = self.send(Command::CancelFilterPreviews(self.filter_preview_generation));
        self.filter_previews_pending = false;
        self.filter_previews.clear();
    }
    type Error = BackendError;
    fn tip_outline(&self, asset: &AssetId) -> Option<&TipOutline> {
        self.brush_sources.get(asset).map(|source| &source.outline)
    }
    fn tip_mask(&self, asset: &AssetId) -> Option<HostImage<'_>> {
        let source = &self.brush_sources.get(asset)?.image;
        Some(HostImage { width: source.extent[0], height: source.extent[1],
            stride: source.extent[0], format: source.format, bytes: &source.bytes })
    }
    fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        let source = layer_core::ProjectAsset::copy_rows(
            [image.width, image.height],
            image.format,
            image.stride as usize,
            image.bytes,
        )
        .map_err(|_| BackendError("Invalid source image"))?;
        self.prepare_owned_asset(asset, &source)
    }
    fn prepare_owned_asset(
        &mut self,
        id: &AssetId,
        asset: &layer_core::ProjectAsset,
    ) -> Result<(), Self::Error> {
        let [width, height] = asset.extent;
        if asset.format == PixelFormat::R8Unorm {
            self.brush_sources.insert(
                id.clone(),
                BrushSource { image: asset.clone(), outline: layer_render::mask_outline(width, height, width, &asset.bytes) },
            );
        }
        self.send(Command::Asset(id.clone(), asset.clone()))
    }
    fn release_asset(&mut self, asset: &AssetId) {
        self.brush_sources.remove(asset);
        let _ = self.send(Command::Release(asset.clone()));
    }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        let Some(geometry) = self.geometry else {
            return Err(BackendError("Canvas not allocated"));
        };
        if self.in_flight.load(Ordering::Acquire) >= 2 {
            return Err(BackendError("Canvas frame queue full"));
        }
        let frame = Frame {
            time_seconds: packet.time_seconds,
            view: packet.view,
            extent: packet.document_extent,
            // Immutable raster roots and sources cross threads by shared ownership.
            layers: packet.layers.to_vec(),
            pending_rasters: packet
                .layers
                .iter()
                .flat_map(|l| std::iter::once(&l.raster).chain(l.mask.iter().map(|m| &m.raster)))
                .filter(|r| r.try_data().is_none())
                .cloned()
                .collect(),
            dabs: packet.dabs.to_vec(),
            batches: packet.dab_batches.to_vec(),
            restore_rasters: packet.restore_rasters.to_vec(),
            reset: packet.reset_layers,
            composite: packet.composite_all,
            geometry,
            surround: self.surround,
            picker: self.picker,
            cursor: self.cursor.clone(),
            overviews: self.overviews.clone(),
            backdrops: self.backdrops.clone(),
            backdrop_style: self.backdrop_style,
            backdrop_hold: self.backdrop_hold,
            stroke_target: self.stroke_target,
            #[cfg(test)]
            queued_ns: gtk::glib::monotonic_time().max(0) as u64 * 1000,
        };
        self.in_flight.fetch_add(1, Ordering::Release);
        self.first_frame_sent = true;
        if let Err(e) = self.send(Command::Frame(Box::new(frame))) {
            self.in_flight.fetch_sub(1, Ordering::Release);
            return Err(e);
        }
        Ok(())
    }
}

struct Worker {
    paper_submitted: bool,
    paper_ready: Arc<AtomicBool>,
    // Drop Vulkan's surface before the wl_surface (field declaration order).
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    child: Child,
    renderer: WgpuRasterizer,
    presenter: ViewportPresenter,
    prepared_color: Option<color::Prepared>,
    config: wgpu::SurfaceConfiguration,
    view_color: crate::display_color::ViewColor,
    last_view: Option<(ViewState, [f32; 4])>,
    area: gtk::glib::SendWeakRef<gtk::Picture>,
    picker: Option<layer_render::ColorPickerOverlay>,
    cursor: Vec<CursorSegment>,
    cursor_scale: f32,
    overviews: Vec<layer_render_wgpu::OverviewPlacement>,
    pending_present: bool,
    hdr_encoding: Option<layer_render_wgpu::SdrSurfaceColor>,
    hdr_float_supported: bool,
    hdr_attempted: bool,
    hdr_rendition: Option<layer_core::color::hdr::SdrRendition>,
    preview_sdr: bool,
    display_headroom: f32,
}
impl Worker {
    #[cfg(test)]
    fn inject_validation_failure(&self) {
        let device = self.renderer.device();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("isolated failure probe"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("isolated invalid scissor"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_scissor_rect(257, 0, 1, 1);
        }
        // Exercise wgpu's real validation panic, without resetting the driver
        // or touching any other application's GPU device.
        self.renderer.queue().submit([encoder.finish()]);
    }
    fn run(
        mut self,
        receiver: &mpsc::Receiver<Command>,
        reply: &mpsc::Sender<Reply>,
        worker_telemetry: &std::sync::Mutex<layer_render::RendererTelemetry>,
        count: &AtomicUsize,
        #[cfg(test)] worker_stats: Arc<std::sync::Mutex<crate::timing::Stats>>,
    ) -> Result<(), String> {
        if reply.send(Reply::Initialized(self.view_color, self.renderer.snapshot_gpu(), self.renderer.shader_activity())).is_err() {
            return Ok(());
        }
        self.report_display(reply)?;
        #[cfg(test)]
        let mut timing = crate::timing::Timing::new(self.renderer.device(), worker_stats);
        let mut telemetry_enabled = false;
        let mut startup_input = None;
        let mut startup_progress = layer_render_wgpu::StartupProgress::default();
        let mut pending_frames: VecDeque<Box<Frame>> = VecDeque::new();
        let mut deferred = VecDeque::new();
        let mut selection_generation = 0;
        let mut pending_thumbnails = VecDeque::new();
        let mut last_canvas_frame = std::time::Instant::now();
        let mut filter_preview_generation = 0;
        let mut document_drawn = false;
        #[cfg(test)]
        let mut fail_next_frame = false;
        loop {
            self.renderer
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(error)?;
            self.child.dispatch()?;
            let headroom = if self.hdr_encoding.is_some() { self.child.hdr_headroom() } else { 1. };
            if headroom != self.display_headroom {
                self.display_headroom = headroom;
                self.update_hdr_view()?;
                self.report_display(reply)?;
            }
            if self.paper_ready.load(Ordering::Acquire)
                && let Some((generation, document, brush, transform)) = &startup_input
            {
                if self
                    .renderer
                    .startup_needs_update(document, brush, *transform)
                {
                    self.renderer
                        .prepare_startup(document, brush, *transform)
                        .map_err(error)?;
                }
                let progress = self.renderer.poll_startup().map_err(error)?;
                if progress != startup_progress {
                    startup_progress = progress;
                    reply
                        .send(Reply::Startup(
                            *generation,
                            progress,
                            self.renderer.brush_sources(),
                        ))
                        .map_err(error)?;
                }
                while progress.canvas_ready
                    && pending_frames
                        .front()
                        .is_some_and(|f| f.dabs.is_empty() || progress.brush_ready)
                {
                    let frame = pending_frames.pop_front().unwrap();
                    #[cfg(test)]
                    timing.begin(frame.queued_ns);
                    self.draw(
                        &frame,
                        false,
                        #[cfg(test)]
                        &mut timing,
                    )?;
                    document_drawn = true;
                    last_canvas_frame = std::time::Instant::now();
                    count.fetch_sub(1, Ordering::Release);
                }
                if progress.complete {
                    startup_input = None;
                }
            }
            if telemetry_enabled && let Ok(mut snapshot) = worker_telemetry.try_lock() {
                *snapshot = self.renderer.telemetry();
                snapshot.resident_bytes += self.presenter.proof_storage_bytes();
            }
            #[cfg(test)]
            timing.presented(self.child.take_presented());
            while let Some(image) = self.renderer.take_thumbnail() {
                reply
                    .send(Reply::Thumbnail(image.map_err(error)?))
                    .map_err(error)?;
            }
            if let Some(color) = self.renderer.take_color_sample() {
                reply
                    .send(Reply::ColorSample(color.map_err(error)))
                    .map_err(error)?;
            }
            if let Some(result) = self.renderer.take_selection_paint() {
                reply.send(Reply::SelectionPaint(selection_generation, result.map_err(error))).map_err(error)?;
            }
            if let Some(region) = self.renderer.take_region() {
                reply
                    .send(Reply::Region(region.map_err(error)))
                    .map_err(error)?;
            }
            // The worker can advance a chunk independently of GTK's UI poll.
            // Apply the same priority already used for optional thumbnails.
            if last_canvas_frame.elapsed() >= Duration::from_millis(50) {
                while let Some(image) = self.renderer.take_filter_previews() {
                    reply
                        .send(Reply::FilterPreviews(filter_preview_generation, image.map_err(error)))
                        .map_err(error)?;
                }
            }
            if let Some(result) = self.renderer.take_effect_validation() {
                reply.send(Reply::EffectValidation(result)).map_err(error)?;
            }
            // A small preview can wait through continuous drawing/navigation.
            // Batching bounds interruption when input resumes during idle work;
            // the quiet period keeps those batches from competing every frame.
            let thumbnail_ready = !pending_thumbnails.is_empty()
                && last_canvas_frame.elapsed() >= Duration::from_millis(50);
            let next = if document_drawn && !deferred.is_empty() {
                Ok(deferred.pop_front().unwrap())
            } else if !startup_progress.complete
                || self.renderer.effect_validation_pending()
                || self.renderer.filter_previews_pending()
                || self.pending_present
                || self.renderer.thumbnails_pending()
                || !pending_thumbnails.is_empty()
                || self.renderer.color_sample_pending()
                || self.renderer.selection_paint_pending()
                || self.renderer.region_pending()
                || self.child.feedback_pending()
            {
                receiver.recv_timeout(Duration::from_millis(if thumbnail_ready { 1 } else { 8 }))
            } else if self.hdr_encoding.is_some() {
                // Display changes arrive on Wayland even when artwork is idle.
                receiver.recv_timeout(Duration::from_millis(100))
            } else {
                receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            let command = match next {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.pending_present
                        && let Some(target) = self.acquire()?
                    {
                        self.publish(
                            target,
                            None,
                            #[cfg(test)]
                            None,
                        )?;
                    }
                    // Visible canvas work wins over background layer artwork.
                    // A whole-photo thumbnail scan used to block this owner for
                    // ~200 ms even though every camera frame rendered in <2 ms.
                    if thumbnail_ready && count.load(Ordering::Acquire) == 0
                        && let Some(&(id, target)) = pending_thumbnails.front()
                        && self.renderer.prepare_thumbnail_batch(target).map_err(error)?
                    {
                        self.renderer.request_thumbnail(id, target).map_err(error)?;
                        pending_thumbnails.pop_front();
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if !document_drawn
                && matches!(
                    command,
                    Command::Region(_)
                        | Command::SelectionPaint(..)
                        | Command::Thumbnail(..)
                        | Command::ColorSample(_)
                        | Command::FilterPreviews(..)
                )
            {
                deferred.push_back(command);
                continue;
            }
            match command {
                Command::LocalTone(guide, reply) => {
                    let result = self
                        .presenter
                        .set_gpu_local_tone_guide(&self.renderer, guide)
                        .map_err(error);
                    if result.is_ok() {
                        self.pending_present = self.last_view.is_some();
                    }
                    let _ = reply.send(result);
                }
                Command::HdrView(rendition, preview_sdr) => {
                    if rendition.is_some() && !self.hdr_attempted {
                        self.enable_hdr()?;
                        self.report_display(reply)?;
                    }
                    self.hdr_rendition = rendition;
                    self.renderer.set_ui_rendition(rendition).map_err(error)?;
                    self.preview_sdr = preview_sdr;
                    self.update_hdr_view()?;
                    self.pending_present = self.last_view.is_some();
                }
                Command::Proof(lut, enabled, gamut, reply) => {
                    let result = self.presenter.set_proof(&self.renderer, lut, enabled, gamut).map_err(error);
                    if result.is_ok() { self.pending_present = self.last_view.is_some(); }
                    let _ = reply.send(result);
                }
                Command::PrepareColor(request) => self.prepare_color(*request, !pending_frames.is_empty()),
                Command::AdoptColor(id) => {
                    self.adopt_color(id, telemetry_enabled)?;
                    startup_input = None;
                    startup_progress = color::complete();
                    document_drawn = true;
                    reply.send(Reply::ColorAdopted(id, self.renderer.brush_sources(), self.renderer.snapshot_gpu(), self.renderer.shader_activity())).map_err(error)?;
                }
                Command::DiscardColor(id, reply) => {
                    self.discard_color(id);
                    let _ = reply.send(());
                }
                #[cfg(test)]
                Command::FailNextFrame => fail_next_frame = true,
                Command::TransformPreview(preview) => self
                    .renderer
                    .set_transform_preview(preview.as_ref())
                    .map_err(error)?,
                Command::Startup(generation, inputs) => {
                    let (document, brush, transform) = *inputs;
                    startup_input = Some((generation, document, brush, transform));
                    startup_progress = Default::default();
                }
                Command::FinishStartupCache => self.renderer.finish_startup_cache(),
                Command::SelectionPaint(generation, update) => {
                    if generation != selection_generation { continue; }
                    let result = self.renderer.paint_selection(&update).map_err(error);
                    reply.send(Reply::SelectionPaintAck(generation, result)).map_err(error)?;
                }
                Command::CancelSelectionPaint(generation) => {
                    selection_generation = generation; self.renderer.cancel_selection_paint();
                }
                Command::QuickMaskThumbnail(selection) => self.renderer.set_quick_mask_thumbnail(selection.as_ref()),
                Command::SelectionOverlay(overlay) => self.renderer.set_selection_overlay(overlay),
                Command::Region(request) => {
                    let result = self.renderer.request_region(request);
                    if !matches!(result, Ok(true)) {
                        reply
                            .send(Reply::Region(Err(result
                                .err()
                                .map(error)
                                .unwrap_or_else(|| "Region detector is busy".into()))))
                            .map_err(error)?;
                    }
                }
                Command::EffectValidation(request) => {
                    let request_id = request.request_id;
                    let result = self.renderer.request_effect_validation(request);
                    if !matches!(result, Ok(true)) {
                        reply
                            .send(Reply::EffectValidation(
                                layer_render::EffectValidationResult {
                                    request_id,
                                    result: Err(result
                                        .err()
                                        .map(error)
                                        .unwrap_or_else(|| "Filter validator busy".into())),
                                },
                            ))
                            .map_err(error)?;
                    }
                }
                Command::Telemetry(enabled) => {
                    telemetry_enabled = enabled;
                    self.renderer.set_telemetry_enabled(enabled);
                }
                Command::CancelFilterPreviews(generation) => {
                    filter_preview_generation = generation;
                    self.renderer.cancel_filter_previews();
                }
                Command::FilterPreviews(generation, request) => {
                    if generation < filter_preview_generation { continue; }
                    filter_preview_generation = generation;
                    let result = self
                        .renderer
                        .request_filter_previews(request)
                        .map_err(error);
                    if !matches!(result, Ok(true)) {
                        reply
                            .send(Reply::FilterPreviews(generation, Err(result
                                .err()
                                .unwrap_or_else(|| "Preview renderer busy".into()))))
                            .map_err(error)?;
                    }
                }
                Command::Thumbnail(id, target) => {
                    pending_thumbnails.push_back((id, target));
                }
                Command::ColorSample(request) => {
                    let result = self.renderer.request_color_sample(request);
                    if !matches!(result, Ok(true)) {
                        reply
                            .send(Reply::ColorSample(Err(result
                                .err()
                                .map(error)
                                .unwrap_or_else(|| "Color sampler is busy".into()))))
                            .map_err(error)?;
                    }
                }
                Command::Frame(frame) => {
                    #[cfg(test)]
                    if fail_next_frame {
                        self.inject_validation_failure();
                    }
                    if self.paper_submitted
                        && (!startup_progress.canvas_ready
                            || (!frame.dabs.is_empty() && !startup_progress.brush_ready))
                    {
                        pending_frames.push_back(frame);
                        continue;
                    }
                    #[cfg(test)]
                    timing.begin(frame.queued_ns);
                    let paper = !self.paper_submitted;
                    self.draw(
                        &frame,
                        paper,
                        #[cfg(test)]
                        &mut timing,
                    )?;
                    last_canvas_frame = std::time::Instant::now();
                    if paper {
                        pending_frames.push_back(frame);
                    } else {
                        document_drawn = true;
                        count.fetch_sub(1, Ordering::Release);
                    }
                }
                Command::Asset(id, asset) => {
                    self.renderer
                        .prepare_owned_asset(&id, &asset)
                        .map_err(error)?;
                }
                Command::Selection(selection) => self
                    .renderer
                    .set_selection_outline(selection.as_ref())
                    .map_err(error)?,
                Command::Release(id) => self.renderer.release_asset(&id),
                #[cfg(test)]
                Command::DocumentPixels(request_id, reply) => {
                    let [width, height] = self.renderer.document_extent();
                    let pixels = self.renderer.readback_srgb_rgba8().map_err(error);
                    let _ = reply.send(pixels.map(|bytes| ReadbackImage {
                        request_id,
                        width,
                        height,
                        stride: width * 4,
                        bytes,
                    }));
                }
                Command::Capture(view, reply) => {
                    let _ = reply.send(self.capture(view));
                }
                Command::Stop => break,
            }
        }
        Ok(())
    }
    fn new(
        parent: Parent,
        area: gtk::glib::SendWeakRef<gtk::Picture>,
        clock: Arc<crate::wayland::FrameClock>,
        color: layer_core::color::DocumentColor,
    ) -> Result<Self, String> {
        let mut child = Child::new(parent, clock)?;
        // One process-lifetime loader/instance, not one per window. On the
        // tested NVIDIA driver, destroying our instance invalidates Wayland WSI
        // entry points still used by GTK's instance. Devices/surfaces/resources
        // are still destroyed with their window; only this instance is retained.
        static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
        let instance = INSTANCE
            .get_or_init(|| {
                let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
                descriptor.backends = wgpu::Backends::VULKAN;
                descriptor.flags = descriptor.flags.with_env();
                wgpu::Instance::new(descriptor)
            })
            .clone();
        // Worker owns child; GpuCanvas retains GTK parent and joins us on drop.
        let surface = unsafe { instance.create_surface_unsafe(child.target()) }.map_err(error)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(error)?;
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, 1, 1)
            .ok_or("Vulkan Wayland swapchain unavailable")?;
        let hdr_float_supported = caps.color_spaces(wgpu::TextureFormat::Rgba16Float).contains(wgpu::SurfaceColorSpaces::PASS_THROUGH);
        let hdr_encoding = if color.depth.is_float() && hdr_float_supported {
            child.describe_hdr()?
        } else { None };
        let hdr_surface = hdr_encoding.is_some();
        let (format, view_color) = if hdr_surface {
            (wgpu::TextureFormat::Rgba16Float, crate::display_color::ViewColor::Srgb)
        } else {
            (crate::display_color::ViewColor::format(&caps)?, child.describe_sdr()?)
        };
        config.color_space = wgpu::SurfaceColorSpace::PassThrough;
        config.format = format;
        config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        if !caps.alpha_modes.contains(&config.alpha_mode) {
            return Err(
                "Wayland swapchain requires premultiplied alpha for rounded corners".into(),
            );
        }
        // FIFO barriers stalled the tested NVIDIA/Mutter child when GTK was
        // idle. Never silently select that known-broken presentation mode.
        if !caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            return Err("The Wayland canvas requires Vulkan mailbox presentation".into());
        }
        config.present_mode = wgpu::PresentMode::Mailbox;
        config.desired_maximum_frame_latency = 2;
        let working_features =
            wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE;
        if !adapter.features().contains(working_features) {
            return Err("This GPU cannot sample and blend the SDR editing format".into());
        }
        let features = working_features
            | layer_render_wgpu::native_tiles::native_in_place_features(&adapter)
            | (adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PIPELINE_CACHE));
        #[cfg(test)]
        let features = features
            | (adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY
                    | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Wayland canvas GPU"),
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::Manual {
                suballocated_device_memory_block_size: (64 * 1024 * 1024)..(128 * 1024 * 1024),
            },
            ..Default::default()
        }))
        .map_err(error)?;
        eprintln!(
            "Wayland canvas GPU: {:?}, {:?}, {:?}",
            adapter.get_info(),
            config.present_mode,
            config.format
        );
        let cache = gtk::glib::user_cache_dir()
            .join("capycanvas")
            .join("shaders");
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged_cached(
            adapter, device, queue, &cache, color,
        ).map_err(error)?;
        renderer.configure_ui_previews(view_color.space()).map_err(error)?;
        eprintln!("Wayland canvas color: {:?}; available: {:?}", config.color_space, caps.format_capabilities);
        let mut presenter = ViewportPresenter::for_surface(&renderer, config.format, hdr_encoding.unwrap_or_else(|| view_color.surface()))
            .map_err(error)?;
        presenter.prepare_overviews(&renderer);
        Ok(Self {
            view_color,
            paper_submitted: false,
            paper_ready: Arc::new(AtomicBool::new(false)),
            surface,
            instance,
            child,
            renderer,
            presenter,
            prepared_color: None,
            config,
            last_view: None,
            area,
            picker: None,
            cursor: Vec::new(),
            cursor_scale: 1.0,
            overviews: Vec::new(),
            pending_present: false,
            hdr_encoding,
            hdr_float_supported,
            hdr_attempted: color.depth.is_float(),
            hdr_rendition: color.depth.is_float().then_some(Default::default()),
            preview_sdr: false,
            display_headroom: 1.,
        })
    }
    fn report_display(&self, reply: &mpsc::Sender<Reply>) -> Result<(), String> {
        reply.send(Reply::DisplayHeadroom(self.display_headroom, self.hdr_encoding)).map_err(error)?;
        let area = self.area.clone();
        gtk::glib::idle_add_once(move || {
            if let Some(area) = area.upgrade() {
                let _ = area.activate_action("canvas.display-changed", None);
            }
        });
        Ok(())
    }
    fn enable_hdr(&mut self) -> Result<(), String> {
        self.hdr_attempted = true;
        if !self.hdr_float_supported { return Ok(()); }
        let Some(encoding) = self.child.describe_hdr()? else { return Ok(()); };
        let mut presenter = ViewportPresenter::for_surface(&self.renderer, wgpu::TextureFormat::Rgba16Float, encoding).map_err(error)?;
        presenter.inherit_proof(&self.renderer, &self.presenter);
        presenter.prepare_overviews(&self.renderer);
        self.presenter = presenter;
        self.hdr_encoding = Some(encoding);
        self.config.format = wgpu::TextureFormat::Rgba16Float;
        // The next complete frame installs geometry, cursor and the new swapchain.
        self.last_view = None;
        Ok(())
    }
    fn update_hdr_view(&mut self) -> Result<(), String> {
        let headroom = if self.preview_sdr { 1. } else { self.display_headroom };
        self.presenter.set_hdr_view(&self.renderer, self.hdr_rendition, headroom).map_err(error)?;
        // Wayland may deliver an HDR display hint before GTK has supplied the
        // first canvas geometry. Update the transform now, but only redraw an
        // existing frame: draw() configures the swapchain before first acquire.
        self.pending_present = self.last_view.is_some();
        Ok(())
    }
    fn draw(
        &mut self,
        frame: &Frame,
        paper: bool,
        #[cfg(test)] timing: &mut crate::timing::Timing,
    ) -> Result<(), String> {
        let draw_start = std::time::Instant::now();
        while frame.layers.iter().any(|l| {
            l.raster.try_data().is_none() || l.masks().any(|m| m.raster.try_data().is_none())
        }) && !self.renderer.raster_ready()
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if self.child.geometry(frame.geometry) {
            // Geometry/stacking become visible on a parent commit. Queue that
            // only AFTER sending our child requests; never race GTK's commit.
            let area = self.area.clone();
            gtk::glib::idle_add_once(move || {
                if let Some(area) = area.upgrade() {
                    crate::wayland::request_parent_commit(&area);
                }
            });
        }
        if self.last_view.is_none()
            || [self.config.width, self.config.height]
                != [frame.view.width_px, frame.view.height_px]
        {
            self.config.width = frame.view.width_px;
            self.config.height = frame.view.height_px;
            self.surface.configure(self.renderer.device(), &self.config);
        }
        self.last_view = Some((frame.view, frame.surround));
        self.pending_present = true;
        self.renderer
            .resize_surface(frame.view.width_px, frame.view.height_px)
            .map_err(error)?;
        // Only this worker may wait in acquire. Always apply paint, even when
        // occluded; an idle retry presents the retained viewport without replay.
        let target = self.acquire()?;
        let _presentation = self.renderer.prioritize_raster_presentation();
        #[cfg(test)]
        if target.is_some() {
            timing.acquired(&self.renderer);
        }
        #[cfg(test)]
        if !paper && frame.layers.iter().any(|layer| layer.raster.try_data().is_none()
            || layer.masks().any(|mask| mask.raster.try_data().is_none())) {
            timing.raster_commit();
        }
        if paper {
            let layers: Vec<_> = frame
                .layers
                .iter()
                .filter(|l| l.kind == layer_core::LayerKind::Background)
                .cloned()
                .collect();
            self.renderer
                .submit(FramePacket {
                    layers: &layers,
                    dabs: &[],
                    dab_batches: &[],
                    restore_rasters: &[],
                    reset_layers: true,
                    composite_all: true,
                    ..frame.packet()
                })
                .map_err(error)?;
            self.paper_submitted = true;
        } else {
            self.renderer.submit(frame.packet()).map_err(error)?;
            #[cfg(test)]
            timing.photo_frame(&frame.layers);
        }
        #[cfg(test)]
        timing.mark(0);
        self.picker = frame.picker;
        self.presenter.set_color_picker(&self.renderer, self.picker);
        self.cursor.clone_from(&frame.cursor);
        self.overviews.clone_from(&frame.overviews);
        self.presenter
            .set_overviews(&self.renderer, &self.overviews);
        self.presenter.set_backdrop(&self.renderer, &frame.backdrops, frame.backdrop_style, frame.backdrop_hold);
        self.cursor_scale = frame.geometry.scale as f32;
        self.presenter
            .set_cursor(self.renderer.device(), &self.cursor, self.cursor_scale);
        self.presenter.set_corner_radius(if frame.geometry.rounded {
            12.0 * self.cursor_scale
        } else {
            0.0
        });
        if let Some(target) = target {
            self.publish(
                target,
                frame.stroke_target.filter(|_| !paper),
                #[cfg(test)]
                Some(timing),
            )?;
        }
        self.child.observe_draw_work(draw_start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        Ok(())
    }

    fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, String> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(self.renderer.device(), &self.config);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // Same owned child, adapter, device and document textures.
                self.surface = unsafe { self.instance.create_surface_unsafe(self.child.target()) }
                    .map_err(error)?;
                self.surface.configure(self.renderer.device(), &self.config);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                Err("Wayland surface validation failed".into())
            }
        }
    }

    fn publish(
        &mut self,
        target: wgpu::SurfaceTexture,
        stroke_target: Option<crate::wayland::StrokeTarget>,
        #[cfg(test)] timing: Option<&mut crate::timing::Timing>,
    ) -> Result<(), String> {
        let _presentation = self.renderer.prioritize_raster_presentation();
        let (camera, surround) = self.last_view.expect("rendered document");
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.camera_view(camera, &self.renderer);
            timing.hdr_view(self.hdr_rendition);
        }
        let view = target.texture.create_view(&Default::default());
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        self.presenter
            .encode(&self.renderer, &mut encoder, &view, camera, surround)
            .map_err(error)?;
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.backdrop(self.presenter.backdrop_frames());
            timing.overview(
                (!self.overviews.is_empty()).then(|| self.renderer.canvas_preview_revision()),
            );
        }
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.encoded(&mut encoder);
        }
        let commands = encoder.finish();
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.mark(1);
        }
        self.renderer.queue().submit([commands]);
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.mark(2);
        }
        #[cfg(test)]
        self.child.feedback(timing.as_ref().map_or(0, |t| t.id()), stroke_target);
        #[cfg(not(test))]
        self.child.feedback(0, stroke_target);
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.mark(3);
        }
        self.renderer.queue().present(target);
        if !self.paper_ready.load(Ordering::Acquire) {
            let ready = self.paper_ready.clone();
            self.renderer
                .queue()
                .on_submitted_work_done(move || ready.store(true, Ordering::Release));
        }
        #[cfg(test)]
        if let Some(timing) = timing {
            timing.end(&self.renderer);
        }
        self.pending_present = false;
        Ok(())
    }
    fn capture(&mut self, color: crate::display_color::ViewColor) -> Result<ReadbackImage, String> {
        let (view, surround) = self.last_view.ok_or("Canvas has not rendered")?;
        // Explicit screenshot only. No host pixels or texture imports in ink.
        let texture = self
            .renderer
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("explicit viewport capture"),
                size: wgpu::Extent3d {
                    width: view.width_px,
                    height: view.height_px,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let mut presenter = ViewportPresenter::for_surface(&self.renderer, texture.format(), color.surface()).map_err(error)?;
        presenter.inherit_proof(&self.renderer, &self.presenter);
        presenter.inherit_backdrop(&self.renderer, &self.presenter);
        presenter.set_hdr_view(&self.renderer, self.hdr_rendition, 1.).map_err(error)?;
        presenter.set_color_picker(&self.renderer, self.picker);
        presenter.set_cursor(self.renderer.device(), &self.cursor, self.cursor_scale);
        presenter.set_overviews(&self.renderer, &self.overviews);
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        presenter
            .encode(
                &self.renderer,
                &mut encoder,
                &texture.create_view(&Default::default()),
                view,
                surround,
            )
            .map_err(error)?;
        let stride = (view.width_px * 4).div_ceil(256) * 256;
        let buffer = self
            .renderer
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("explicit capture readback"),
                size: u64::from(stride) * u64::from(view.height_px),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        self.renderer.queue().submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.renderer
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .map_err(error)?;
        rx.recv().map_err(error)?.map_err(error)?;
        let pixels = buffer.get_mapped_range(..).map_err(error)?.to_vec();
        Ok(ReadbackImage {
            request_id: 0,
            width: view.width_px,
            height: view.height_px,
            stride,
            bytes: pixels,
        })
    }
}
