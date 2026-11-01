//! Bounded frame handoff and canvas GPU/WSI owner. No GTK calls on the worker.
//! Small dab/layer records cross threads; live canvas pixels remain on the GPU.
use crate::wayland::{Child, Geometry, Parent};
use gtk::prelude::WidgetExt;
use layer_core::{AssetId, authored::{SceneSnapshot, SceneScope, OccurrenceHandle, SourceTarget, EvaluationContext, ArtworkCapture}};
use layer_render::{
    BackendError, BrushSource, CanvasRenderer, CursorSegment, Dab, DabBatch, FramePacket, HostImage,
    ReadbackImage, TipOutline, ViewState,
};
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
/// Wake the canvas once GTK is next idle, from any thread.
fn wake_canvas(area: &gtk::glib::SendWeakRef<gtk::Picture>, status: &Arc<ChromeStatus>) {
    canvas_action(area, "canvas.wake", status);
}
fn canvas_action(area: &gtk::glib::SendWeakRef<gtk::Picture>, action: &'static str, status: &Arc<ChromeStatus>) {
    let area = area.clone();
    let status = status.clone();
    gtk::glib::idle_add_full(gtk::glib::Priority::HIGH_IDLE, move || {
        if !status.accepting.load(Ordering::Acquire) { return gtk::glib::ControlFlow::Break; }
        if let Some(area) = area.upgrade() {
            let parameter = (action == "canvas.chrome").then(|| gtk::glib::variant::ToVariant::to_variant(&status.owner));
            let _ = area.activate_action(action, parameter.as_ref());
        }
        gtk::glib::ControlFlow::Break
    });
}

const CHROME_PREPARING: u64 = u64::MAX;
const CHROME_READY: u64 = u64::MAX - 1;
const CHROME_RETIRED: u64 = u64::MAX - 2;
pub(crate) struct ChromeStatus {
    owner: u64,
    accepting: AtomicBool,
    value: AtomicU64,
}
impl ChromeStatus {
    fn new(owner: u64) -> Self { Self {owner,accepting:AtomicBool::new(true),value:AtomicU64::new(CHROME_PREPARING)} }
    pub(crate) fn state(&self) -> crate::glass::ChromeState {
        use crate::glass::ChromeState;
        match self.value.load(Ordering::Acquire) {
            CHROME_PREPARING => ChromeState::Preparing(self.owner),
            CHROME_READY => ChromeState::Ready(self.owner),
            CHROME_RETIRED => ChromeState::Retired(self.owner),
            revision => ChromeState::Visible(self.owner, revision),
        }
    }
    fn publish(&self, revision: u64) {
        assert!(revision < CHROME_RETIRED);
        self.value.store(revision, Ordering::Release);
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
enum FirstChrome {
    #[default]
    Preparing,
    Ready,
    Granted,
    Submitted,
    Visible,
}

fn workspace_presenter(renderer: &WgpuRasterizer,format: wgpu::TextureFormat,color: layer_render_wgpu::SdrSurfaceColor) -> Result<ViewportPresenter,String> {
    let mut presenter=ViewportPresenter::for_surface(renderer,format,color).map_err(error)?;
    presenter.prepare_chrome(renderer);
    Ok(presenter)
}

#[derive(Default)]
struct ChromeHandoff {
    requested: u64,
    published: u64,
    hold: bool,
    synchronized: Option<u64>,
    first: FirstChrome,
}
impl ChromeHandoff {
    fn request(&mut self, revision: u64, hold: bool) -> bool {
        if matches!(self.first, FirstChrome::Granted | FirstChrome::Submitted) || revision < self.requested { return false; }
        self.hold = hold && (self.first != FirstChrome::Visible || revision != self.requested || self.hold);
        self.requested = revision;
        true
    }
    fn changed(&self) -> bool { self.requested != self.published }
    fn needs_publication(&self) -> bool { self.synchronized.is_none() && (self.changed() || self.first == FirstChrome::Granted) }
    fn ready(&mut self) { assert_eq!(self.first, FirstChrome::Preparing); self.first = FirstChrome::Ready; }
    fn grant(&mut self, revision: u64) -> bool {
        if self.first != FirstChrome::Ready { return false; }
        assert!(revision >= self.requested);
        self.requested = revision;
        self.hold = true;
        self.first = FirstChrome::Granted;
        true
    }
    fn can_present(&self) -> bool {
        matches!(self.first, FirstChrome::Granted | FirstChrome::Visible)
    }
    fn release_for_canvas(&mut self) -> bool {
        if self.first != FirstChrome::Visible { return false; }
        self.synchronized.take().is_some()
    }
    fn should_hold(&self, allow_sync: bool) -> bool { self.first == FirstChrome::Granted || allow_sync && self.changed() && self.hold }
    fn publish(&mut self, allow_sync: bool) -> Option<u64> {
        assert!(self.can_present());
        if !self.changed() && self.first == FirstChrome::Visible { return None; }
        if self.should_hold(allow_sync) { self.synchronized = Some(self.requested); }
        self.published = self.requested;
        self.first = if self.first == FirstChrome::Granted { FirstChrome::Submitted } else { FirstChrome::Visible };
        Some(self.published)
    }
    fn release(&mut self, revision: u64) -> bool {
        if self.first == FirstChrome::Submitted { return false; }
        if self.synchronized.is_some_and(|held| revision >= held) {
            self.synchronized = None;
            true
        } else { false }
    }
    fn initial_applied(&mut self) -> bool {
        if self.first != FirstChrome::Submitted { return false; }
        self.first = FirstChrome::Visible;
        self.release(self.published)
    }
}

struct ChromePresentation {
    revision: u64,
    hold: bool,
    surround: [f32; 4],
    backdrops: Vec<layer_render_wgpu::BackdropRegion>,
    panels: Vec<layer_render_wgpu::PanelSurface>,
    style: layer_render_wgpu::BackdropBlurStyle,
    backdrop_hold: bool,
    overviews: Vec<layer_render_wgpu::OverviewPlacement>,
}
enum ChromeUpdate {
    Current(ChromePresentation, u64),
    Initial(ChromePresentation),
}

#[derive(Default)]
struct ChromeRequests {
    considered_input: u64,
    first: Option<(u64, ChromePresentation)>,
    latest: Option<(u64, ChromePresentation)>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameReadiness { Waiting, Dependencies, Ready }
impl FrameReadiness {
    fn ready(self) -> bool { self == Self::Ready }
}
fn admit_frame<T, E>(frame: Option<T>, pending: &mut VecDeque<T>, mut draw: impl FnMut(&mut T) -> Result<bool, E>) -> Result<usize, E> {
    let mut admitted = 0;
    while let Some(frame) = pending.front_mut() {
        if !draw(frame)? { break; }
        pending.pop_front();
        admitted += 1;
    }
    if let Some(mut frame) = frame {
        if pending.is_empty() && draw(&mut frame)? { admitted += 1; }
        else { pending.push_back(frame); }
    }
    Ok(admitted)
}
impl ChromeRequests {
    fn request(&mut self, chrome: ChromePresentation, after_input: u64) {
        let newest = self.latest.as_ref().or(self.first.as_ref());
        if newest.is_some_and(|(_, newest)| newest.revision > chrome.revision) { return; }
        match &mut self.first {
            None => self.first = Some((after_input, chrome)),
            Some((after, current)) if *after == after_input => *current = chrome,
            Some(_) => self.latest = Some((after_input, chrome)),
        }
    }
    fn considered(&mut self, input: u64) { self.considered_input = self.considered_input.max(input); }
    fn take_ready(&mut self) -> Option<ChromePresentation> {
        if self.first.as_ref().is_none_or(|(after, _)| *after > self.considered_input) { return None; }
        let (_, chrome) = self.first.take().unwrap();
        self.first = self.latest.take();
        Some(chrome)
    }
}

fn input_frame_order(order: &mut u64, dabs: usize, terminal: bool) -> u64 {
    if dabs == 0 && !terminal { return 0; }
    *order += 1;
    *order
}

#[derive(Clone)]
pub(crate) struct ContextCapture(Arc<std::sync::OnceLock<Result<EvaluationContext, String>>>);
struct ContextRequest {value: Arc<std::sync::OnceLock<Result<EvaluationContext, String>>>}
impl Drop for ContextRequest {
    fn drop(&mut self) { let _ = self.value.set(Err("GPU worker stopped before capture".into())); }
}
impl ContextCapture {
    fn context(&self) -> Result<EvaluationContext, String> {
        while self.0.get().is_none() { std::thread::sleep(Duration::from_millis(1)); }
        self.0.get().unwrap().clone()
    }
    pub(crate) async fn resolve(self) -> Result<EvaluationContext, String> {
        gtk::gio::spawn_blocking(move || self.context()).await.map_err(|_| "Capture worker failed".to_string())?
    }
    pub(crate) fn install(&self, capture: &mut ArtworkCapture) -> Result<(), String> {
        let mut context = self.context()?;
        context.retain_effects(&capture.artwork);
        let artwork = Arc::make_mut(&mut capture.artwork);
        artwork.outputs.get_mut(artwork.default_output).ok_or("Captured output unavailable")?.context = context;
        Ok(())
    }
}
impl RenderWorker {
    pub(super) fn take_cancelled_thumbnail(&mut self) -> Option<u64> { self.cancelled_thumbnails.pop_front() }
    pub(crate) fn capture_context(&self) -> Result<ContextCapture, String> {
        let value = Arc::new(std::sync::OnceLock::new());
        if self.failure.get().is_some() {
            let _ = value.set(Ok(self.evaluation_context()));
        } else {
            self.send(Command::CaptureContext(ContextRequest {value: value.clone()})).map_err(error)?;
        }
        Ok(ContextCapture(value))
    }
}

#[derive(Clone, Default)]
struct Overlays {
    selection: Option<layer_core::Selection>,
    selection_overlay: Option<layer_render::SelectionOverlay>,
    crop: Option<layer_render::CropOverlay>,
    paper: Option<layer_render::PaperOverlay>,
    paper_generation: u64,
    vector_preview: Option<layer_render::VectorPreview>,
    vector_motion: Option<layer_render::VectorMotion>,
    clipping: [bool; 2],
}

struct Frame {
    overlays: Arc<Overlays>,
    time_seconds: f32,
    view: ViewState,
    extent: [u32; 2],
    scene: Arc<SceneSnapshot>,
    inspect_mask: Option<OccurrenceHandle>,
    transform_preview: Option<layer_render::TransformPreview>,
    selection_overlays: Arc<layer_core::authored::SelectionOverlays>,
    dabs: Vec<Dab>,
    batches: Vec<DabBatch>,
    restore_rasters: Vec<(SourceTarget, layer_core::raster::RasterRevision)>,
    reset: bool,
    input_order: u64,
    composite: bool,
    commit_rasters: bool,
    blend_space: layer_core::BlendSpace,
    geometry: Geometry,
    present: bool,
    picker: Option<layer_render::ColorPickerOverlay>,
    cursor: Vec<CursorSegment>,
    chrome: ChromePresentation,
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
    fn backdrop_packet<'a>(&'a self,scope:&'a SceneScope)->FramePacket<'a> {
        FramePacket {scene:self.scene.view().with_scope(scope),inspect_mask:None,selection_overlays:None,dabs:&[],dab_batches:&[],restore_rasters:&[],
            reset_layers:true,composite_all:true,..self.packet()}
    }
    fn packet(&self) -> FramePacket<'_> {
        FramePacket {
            commit_rasters: self.commit_rasters,
            time_seconds: self.time_seconds,
            view: self.view,
            document_extent: self.extent,
            scene: self.scene.view(),
            inspect_mask: self.inspect_mask,
            selection_overlays: Some(&self.selection_overlays),
            dabs: &self.dabs,
            dab_batches: &self.batches,
            restore_rasters: &self.restore_rasters,
            reset_layers: self.reset,
            composite_all: self.composite,
            blend_space: self.blend_space,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum StartupMode { VisibleFirst, Unpresented }

enum Command {
    SeedContext(EvaluationContext),
    CaptureContext(ContextRequest),
    LocalTone(Option<Arc<layer_render_wgpu::local_tone::GpuToneGuide>>, mpsc::Sender<Result<(),String>>),
    Proof(Option<Arc<layer_color::ProofLut>>, bool, bool, mpsc::Sender<Result<(), String>>),
    Screen(u64, f32, Option<layer_render_wgpu::ScreenCheck>),
    HdrView(Option<layer_core::color::hdr::SdrRendition>, bool),
    PrepareColor(Box<color::Request>),
    AdoptColor(u64),
    DiscardColor(u64, mpsc::Sender<()>),
    MovingLayer(Option<OccurrenceHandle>),
    MovingPixels(Option<(SourceTarget, layer_core::Selection)>),
    Retouch(Option<layer_render::RetouchPreparation>),
    Regions(Option<layer_render::RegionPreparation>),
    RetireStrokeSources,
    Startup(
        u64,
        StartupMode,
        Box<(layer_core::Document, layer_core::BrushSnapshot, bool)>,
    ),
    FinishStartupCatalog,
    SelectionPaint(u64, layer_render::SelectionPaint),
    CancelSelectionPaint(u64),
    QuickMaskThumbnail(Option<layer_core::Selection>),
    Region(u64, layer_render::RegionRequest),
    CancelRegion(u64),
    EffectValidation(layer_render::EffectValidationRequest),
    Telemetry(bool),
    Thumbnail(u64, layer_render::ThumbnailTarget),
    BrushStamp(u64, layer_render::BrushStampRequest),
    Analysis(layer_core::ArtworkQuery, mpsc::Sender<Result<layer_render_wgpu::effect_analysis::Job, String>>),
    AcceptAnalysis(layer_render_wgpu::effect_analysis::Candidate),
    RetainAnalyses(Vec<OccurrenceHandle>),
    Snapshot(layer_render::SnapshotRequest, mpsc::Sender<Result<layer_render_wgpu::snapshot::SnapshotJob, String>>),
    ColorSample(layer_render::ColorSampleRequest),
    FilterPreviews(u64, layer_render::FilterPreviewRequest),
    CancelFilterPreviews(u64),
    Frame(Box<Frame>),
    Wake,
    Present(ViewState, Geometry, Vec<CursorSegment>),
    ReleaseChrome(u64),
    Asset(AssetId, layer_core::ProjectAsset),
    Release(AssetId),
    #[cfg(test)]
    DocumentPixels(u64, mpsc::Sender<Result<ReadbackImage, String>>),
    Capture(crate::display_color::ViewColor, mpsc::Sender<Result<ReadbackImage, String>>),
    Stop,
    #[cfg(test)]
    FailNextFrame,
}
/// What the render thread reports after each frame for the session to read.
#[derive(Default)]
struct FrameReport {
    context: std::sync::RwLock<EvaluationContext>,
    pending_work: AtomicBool,
    retouch_miss: std::sync::atomic::AtomicU64,
    retouch_waiting: std::sync::atomic::AtomicU64,
    paper_drawn: std::sync::atomic::AtomicU64,
}
enum Reply {
    ColorAdopted(u64, HashMap<AssetId, BrushSource>, layer_render_wgpu::snapshot::SnapshotGpu, Option<layer_render_wgpu::ShaderActivity>),
    Initialized(crate::display_color::ViewColor, layer_render_wgpu::snapshot::SnapshotGpu, Option<layer_render_wgpu::ShaderActivity>),
    Startup(
        u64,
        layer_render_wgpu::StartupProgress,
        HashMap<AssetId, BrushSource>,
    ),
    Region(u64, Result<layer_render::RegionResult, String>),
    SelectionPaintAck(u64, Result<bool, String>),
    SelectionPaint(u64, Result<layer_render::SelectionPaintResult, String>),
    EffectValidation(layer_render::EffectValidationResult),
    Thumbnail(ReadbackImage),
    BrushStamp(Result<ReadbackImage, String>),
    ThumbnailCancelled(u64),
    ColorSample(Result<layer_render::ColorSample, String>),
    FilterPreviews(u64, Result<layer_render::FilterPreviewImage, String>),
    DisplayHeadroom(f32, Option<layer_render_wgpu::SdrSurfaceColor>),
    Screen(layer_color::screen::ScreenColor),
    ScreenClipped(u64, Option<bool>),
    Error(String),
}

const SCREEN_CHECK_DELAY: Duration = Duration::from_millis(250);

#[cfg(test)]
static NEXT_STARTUP_PAUSE: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn pause_next_startup() -> Arc<AtomicBool> {
    let pause = Arc::new(AtomicBool::new(true));
    *NEXT_STARTUP_PAUSE.lock().unwrap() = Some(pause.clone());
    pause
}

enum WorkerJob<T> { Preparing(mpsc::Receiver<Result<T, String>>), Running(T) }
impl<T> WorkerJob<T> {
    fn ready(&mut self) -> Result<Option<&mut T>, BackendError> {
        if let Self::Preparing(receiver) = self {
            match receiver.try_recv() {
                Ok(Ok(job)) => *self = Self::Running(job),
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                _ => return Err(BackendError("Could not start the operation")),
            }
        }
        let Self::Running(job) = self else { unreachable!() }; Ok(Some(job))
    }
}
/// Two in-flight paint frames, including the frame being presented. GTK never
/// waits on a worker lock, Vulkan acquire, or a GPU completion fence.
pub struct RenderWorker {
    parent: Option<NativeParent>,
    retirement: Option<crate::retirement::Retirement>,
    overlays: Arc<Overlays>,
    snapshot_job: Option<WorkerJob<layer_render_wgpu::snapshot::SnapshotJob>>,
    analysis_job: Option<WorkerJob<layer_render_wgpu::effect_analysis::Job>>,
    analysis_candidate: Option<layer_render_wgpu::effect_analysis::Candidate>,
    shader_activity: Option<layer_render_wgpu::ShaderActivity>,
    #[cfg(test)]
    startup_pause: Option<Arc<AtomicBool>>,
    pub(crate) proof_owner: u64,
    hdr_view: Option<(Option<layer_core::color::hdr::SdrRendition>, bool)>,
    transform_preview: Option<layer_render::TransformPreview>,
    moving_layer: Option<OccurrenceHandle>,
    moving_pixels: Option<(SourceTarget, layer_core::Selection)>,
    initialized: bool,
    pub(crate) display_headroom: f32,
    pub(crate) display_encoding: Option<layer_render_wgpu::SdrSurfaceColor>,
    pub(crate) screen_color: layer_color::screen::ScreenColor,
    pub(crate) screen_clipped: Option<bool>,
    screen: Option<(f32, Option<layer_render_wgpu::ScreenCheck>)>,
    screen_generation: u64,
    snapshot_gpu: Option<layer_render_wgpu::snapshot::SnapshotGpu>,
    pub(crate) view_color: crate::display_color::ViewColor,
    first_frame_sent: bool,
    context_seed: EvaluationContext,
    selection_overlays: Arc<layer_core::authored::SelectionOverlays>,
    settling_roots: Vec<layer_core::raster::RasterRevision>,
    settling_view: Option<(ViewState, Vec<CursorSegment>)>,
    pub(super) startup: layer_render_wgpu::StartupProgress,
    startup_generation: u64,
    startup_key: Option<(layer_render_wgpu::ShaderDocument, layer_core::BrushSnapshot, bool)>,
    region: Option<Result<layer_render::RegionResult, String>>,
    region_pending: bool,
    region_generation: u64,
    selection_generation: u64,
    selection_update_pending: bool,
    selection_ack: Option<Result<bool, String>>,
    selection_paint: Option<Result<layer_render::SelectionPaintResult, String>>,
    quick_thumbnail: Option<layer_core::Selection>,
    pub(super) clock: Arc<crate::wayland::FrameClock>,
    telemetry: Arc<std::sync::Mutex<layer_render::RendererTelemetry>>,
    telemetry_enabled: bool,
    commands: mpsc::Sender<Command>,
    chrome_updates: mpsc::Sender<ChromeUpdate>,
    chrome_sent: Option<(u64, bool)>,
    input_order: u64,
    replies: mpsc::Receiver<Reply>,
    failure: Arc<std::sync::OnceLock<String>>,
    in_flight: Arc<AtomicUsize>,
    report: Arc<FrameReport>,
    retouch: Option<layer_render::RetouchPreparation>,
    regions:Option<layer_render::RegionPreparation>,
    thread: Option<JoinHandle<()>>,
    color: layer_core::color::DocumentColor,
    next_color_request: u64,
    pending_color: Option<color::Pending>,
    awaiting_color_adoption: Option<u64>,
    brush_sources: HashMap<AssetId, BrushSource>,
    thumbnails: VecDeque<ReadbackImage>,
    brush_stamps: VecDeque<Result<ReadbackImage, String>>,
    cancelled_thumbnails: VecDeque<u64>,
    color_sample: Option<Result<layer_render::ColorSample, String>>,
    color_sample_pending: bool,
    filter_previews: VecDeque<Result<layer_render::FilterPreviewImage, String>>,
    filter_previews_pending: bool,
    filter_preview_generation: u64,
    effect_validation_pending: bool,
    effect_validation: Option<layer_render::EffectValidationResult>,
    pub(super) geometry: Option<Geometry>,
    pub(super) present_frames: bool,
    pub(super) surround: [f32; 4],
    pub(super) picker: Option<layer_render::ColorPickerOverlay>,
    pub(super) cursor: Vec<CursorSegment>,
    pub(super) overviews: Vec<layer_render_wgpu::OverviewPlacement>,
    pub(super) backdrops: Vec<layer_render_wgpu::BackdropRegion>,
    pub(super) panel_surfaces: Vec<layer_render_wgpu::PanelSurface>,
    pub(super) backdrop_style: layer_render_wgpu::BackdropBlurStyle,
    pub(super) backdrop_hold: bool,
    pub(super) chrome: u64,
    pub(super) chrome_hold: bool,
    presented_chrome: Arc<ChromeStatus>,
    pub(super) stroke_target: Option<crate::wayland::StrokeTarget>,
    #[cfg(test)]
    pub stats: Arc<std::sync::Mutex<crate::timing::Stats>>,
}
struct NativeParent {
    _window: crate::window::Window,
    _surface: gtk::gdk::Surface,
    _owner: crate::retirement::Owner,
    _hold: Option<gtk::gio::ApplicationHoldGuard>,
}
impl RenderWorker {
    fn chrome_presentation(&self) -> ChromePresentation {
        ChromePresentation {revision:self.chrome,hold:self.chrome_hold,surround:self.surround,
            backdrops:self.backdrops.clone(),panels:self.panel_surfaces.clone(),
            style:self.backdrop_style,backdrop_hold:self.backdrop_hold,overviews:self.overviews.clone()}
    }
    pub(super) fn present_chrome(&mut self) -> Result<(), BackendError> {
        if self.chrome_sent != Some((self.chrome, self.chrome_hold)) {
            self.chrome_updates.send(ChromeUpdate::Current(self.chrome_presentation(), self.input_order))
                .map_err(|_| BackendError("GPU worker stopped"))?;
            self.chrome_sent = Some((self.chrome, self.chrome_hold));
            self.send(Command::Wake)?;
        }
        Ok(())
    }
    pub(super) fn start_chrome(&mut self, revision: u64) -> Result<(), String> {
        assert_eq!(revision, self.chrome);
        self.chrome_updates.send(ChromeUpdate::Initial(self.chrome_presentation())).map_err(error)?;
        self.chrome_sent = Some((revision, self.chrome_hold));
        self.send(Command::Wake).map_err(error)
    }
    pub(crate) fn chrome_status(&self) -> &Arc<ChromeStatus> { &self.presented_chrome }
    pub(crate) fn release_chrome(&self, revision: u64) -> impl FnOnce() + 'static {
        let commands = self.commands.clone();
        move || { let _ = commands.send(Command::ReleaseChrome(revision)); }
    }
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

    pub(crate) fn set_screen(&mut self, headroom: f32, check: Option<layer_render_wgpu::ScreenCheck>) -> Result<(), String> {
        if self.screen == Some((headroom, check)) {
            return Ok(());
        }
        let unmarked = |c: Option<layer_render_wgpu::ScreenCheck>| c.map(|c| layer_render_wgpu::ScreenCheck { mark: false, ..c });
        if self.screen.is_none_or(|(_, previous)| unmarked(previous) != unmarked(check)) {
            self.screen_generation += 1;
            self.screen_clipped = None;
        }
        self.send(Command::Screen(self.screen_generation, headroom, check)).map_err(error)?;
        self.screen = Some((headroom, check));
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
        area: &gtk::Picture,
        color: layer_core::color::DocumentColor,
        localization: Arc<layer_ui::Localizer>,
    ) -> Result<Self, String> {
        use gtk::prelude::*;
        let window = area.root().and_downcast::<crate::window::Window>().ok_or("The canvas has no window")?;
        let surface = window.surface().ok_or("The canvas window has no native surface")?;
        let parent = Parent::new(&surface)?;
        let lease = NativeParent { _owner: window.renderer_owner()?,
            _hold: window.application().map(|app| app.hold()), _window: window, _surface: surface };
        let area: gtk::glib::SendWeakRef<gtk::Picture> = area.downgrade().into();
        let (mut renderer, receiver, chrome_requests, reply) = Self::channels(color);
        renderer.parent = Some(lease);
        #[cfg(test)]
        { renderer.startup_pause = NEXT_STARTUP_PAUSE.lock().unwrap().take(); }
        #[cfg(test)]
        let pause = renderer.startup_pause.clone();
        let worker_failure = renderer.failure.clone();
        let worker_clock = renderer.clock.clone();
        let count = renderer.in_flight.clone();
        let worker_report = renderer.report.clone();
        let worker_telemetry = renderer.telemetry.clone();
        let failure_area = area.clone();
        let worker_chrome = renderer.presented_chrome.clone();
        let finished_chrome = renderer.presented_chrome.clone();
        #[cfg(test)]
        let worker_stats = renderer.stats.clone();
        renderer.thread = Some(std::thread::Builder::new()
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
                    Worker::new(parent, area, worker_clock, worker_chrome, color, &localization)?.run(
                        &receiver,
                        &chrome_requests,
                        &reply,
                        &worker_telemetry,
                        &count,
                        &worker_report,
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
                finished_chrome.value.store(CHROME_RETIRED, Ordering::Release);
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
                    wake_canvas(&failure_area, &finished_chrome);
                }
            })
            .map_err(error)?);
        Ok(renderer)
    }
    fn channels(color: layer_core::color::DocumentColor) -> (Self, mpsc::Receiver<Command>, mpsc::Receiver<ChromeUpdate>, mpsc::Sender<Reply>) {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let proof_owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
        let (commands, receiver) = mpsc::channel();
        let (chrome_updates, chrome_requests) = mpsc::channel();
        let (reply, replies) = mpsc::channel();
        let presented_chrome = Arc::new(ChromeStatus::new(proof_owner));
        let renderer = Self {
            parent: None,
            retirement: None,
            overlays: Arc::default(),
            context_seed: EvaluationContext::default(),
            selection_overlays: Arc::default(),
            snapshot_job: None,
            analysis_job: None,
            analysis_candidate: None,
            shader_activity: None,
            #[cfg(test)]
            startup_pause: None,
            proof_owner,
            hdr_view: None,
            transform_preview: None,
            moving_layer: None,
            moving_pixels: None,
            initialized: false,
            display_headroom: 1.,
            display_encoding: None,
            screen_color: Default::default(),
            screen_clipped: None,
            screen: None,
            screen_generation: 0,
            snapshot_gpu: None,
            view_color: Default::default(),
            first_frame_sent: false,
            settling_roots: Vec::new(),
            settling_view: None,
            startup: Default::default(),
            startup_generation: 0,
            startup_key: None,
            region: None,
            region_pending: false,
            region_generation: 0,
            selection_generation: 0,
            selection_update_pending: false,
            selection_ack: None,
            selection_paint: None,
            quick_thumbnail: None,
            clock: Arc::default(),
            stroke_target: None,
            telemetry: Arc::default(),
            telemetry_enabled: false,
            commands,
            chrome_updates,
            chrome_sent: None,
            input_order: 0,
            replies,
            failure: Arc::default(),
            in_flight: Arc::default(),
            report: Arc::default(),
            retouch: None,
            regions:None,
            thread: None,
            color,
            next_color_request: 0,
            pending_color: None,
            awaiting_color_adoption: None,
            brush_sources: HashMap::new(),
            thumbnails: VecDeque::new(),
            brush_stamps: VecDeque::new(),
            cancelled_thumbnails: VecDeque::new(),
            color_sample: None,
            color_sample_pending: false,
            filter_previews: VecDeque::new(),
            filter_previews_pending: false,
            filter_preview_generation: 0,
            effect_validation_pending: false,
            effect_validation: None,
            geometry: None,
            present_frames: true,
            surround: [0.033; 4],
            picker: None,
            cursor: Vec::new(),
            overviews: Vec::new(),
            backdrops: Vec::new(),
            panel_surfaces: Vec::new(),
            backdrop_style: Default::default(),
            backdrop_hold: false,
            chrome: 0,
            chrome_hold: false,
            presented_chrome,
            #[cfg(test)]
            stats: Arc::default(),
        };
        (renderer, receiver, chrome_requests, reply)
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
    pub(super) fn finish_startup_catalog(&self) -> Result<(), String> {
        self.send(Command::FinishStartupCatalog).map_err(error)
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
        self.prepare_startup_mode(document, brush, transform, StartupMode::VisibleFirst)
    }
    pub(super) fn prepare_import(
        &mut self,
        document: layer_core::Document,
        brush: layer_core::BrushSnapshot,
    ) -> Result<(), String> {
        self.prepare_startup_mode(document, brush, false, StartupMode::Unpresented)
    }
    fn prepare_startup_mode(
        &mut self,
        document: layer_core::Document,
        brush: layer_core::BrushSnapshot,
        transform: bool,
        mode: StartupMode,
    ) -> Result<(), String> {
        self.present_frames=matches!(mode,StartupMode::VisibleFirst);
        self.startup_generation += 1;
        self.startup_key = Some((layer_render_wgpu::ShaderDocument::new(&document), brush.clone(), transform));
        self.startup = Default::default();
        self.send(Command::Startup(
            self.startup_generation,
            mode,
            Box::new((document, brush, transform)),
        ))
        .map_err(error)
    }
    #[cfg(test)]
    pub(super) fn has_paper_overlay(&self) -> bool {
        self.overlays.paper.is_some()
    }
    pub(super) fn started(&self) -> bool {
        self.initialized
    }
    pub(super) fn ready(&mut self) -> Result<bool, String> {
        if let Some(error) = self.failure.get().cloned() {
            self.stop();
            return Err(error);
        }
        if self.retirement.is_some() { return Err("GPU worker retired".into()); }
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
                    Reply::Screen(color) => self.screen_color = color,
                    Reply::ScreenClipped(generation, clipped) => if generation == self.screen_generation { self.screen_clipped = clipped },
                    Reply::BrushStamp(image) => self.brush_stamps.push_back(image),
                Reply::Error(error) => { self.stop(); return Err(error); },
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
                Reply::Region(generation, result) => {
                    if generation == self.region_generation {
                        self.region_pending = false;
                        self.region = Some(result);
                    }
                }
                Reply::EffectValidation(result) => {
                    self.effect_validation_pending = false;
                    self.effect_validation = Some(result);
                }
                Reply::Thumbnail(image) => self.thumbnails.push_back(image),
                Reply::BrushStamp(image) => self.brush_stamps.push_back(image),
                Reply::ThumbnailCancelled(id) => self.cancelled_thumbnails.push_back(id),
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
                Reply::Screen(color) => self.screen_color = color,
                Reply::ScreenClipped(generation, clipped) => if generation == self.screen_generation { self.screen_clipped = clipped },
                Reply::Error(error) => { self.stop(); return Err(error); },
            }
        }
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            self.stop();
            return Err("GPU worker stopped".into());
        }
        Ok(self.initialized
            && (!self.first_frame_sent || self.startup.canvas_ready)
            && self.in_flight.load(Ordering::Acquire) < 2)
    }
    #[cfg(test)]
    pub(super) fn worker_is_retired(&self) -> bool {
        self.retirement.as_ref().is_some_and(|retirement| retirement.is_complete())
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
    pub(super) fn stop(&mut self) -> crate::retirement::Retirement {
        if let Some(retirement) = &self.retirement { return retirement.clone(); }
        #[cfg(test)]
        if let Some(pause) = &self.startup_pause { pause.store(false, Ordering::Release); }
        let _ = self.discard_prepared_color();
        self.presented_chrome.accepting.store(false, Ordering::Release);
        let _ = self.send(Command::Stop);
        self.commands = mpsc::channel().0;
        self.chrome_updates = mpsc::channel().0;
        let replies = std::mem::replace(&mut self.replies, mpsc::channel().1);
        let resources = (replies, self.snapshot_gpu.take(), self.snapshot_job.take(), self.analysis_job.take(),
            self.analysis_candidate.take(), self.shader_activity.take(), std::mem::take(&mut self.brush_sources),
            std::mem::take(&mut self.thumbnails), std::mem::take(&mut self.brush_stamps),
            std::mem::take(&mut self.filter_previews), self.color_sample.take(), self.region.take(),
            std::mem::take(&mut self.overlays), self.startup_key.take());
        let retirement = if let Some(thread) = self.thread.take() {
            crate::retirement::reap(thread, resources, self.parent.take())
        } else {
            drop(resources);
            let finished = crate::retirement::Retirement::default();
            finished.complete();
            finished
        };
        self.retirement = Some(retirement.clone());
        self.startup = Default::default();
        retirement
    }
}
impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
impl CanvasRenderer for RenderWorker {
    fn shader_input(&mut self) { if let Some(activity) = &self.shader_activity { activity.input(); } }
    fn shader_idle(&mut self, idle: bool, speculative_idle: bool) { if let Some(activity) = &self.shader_activity { activity.idle(idle, speculative_idle); } }
    fn shaders_need_update(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.startup_needs_update(document, brush, transform)
    }
    fn paint_ready(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.startup.brush_ready && !self.startup_needs_update(document, brush, transform)
    }
    fn document_color(&self) -> layer_core::color::DocumentColor { self.color }
    fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> { self.adopt_color(color) }
    fn supports_tiled_sources(&self) -> bool { true }
    fn supports_raster_damage(&self) -> bool { true }
    fn max_document_dimension(&self) -> u32 {
        self.snapshot_gpu.as_ref().map_or(u32::MAX, |gpu| gpu.max_document_dimension())
    }
    fn memory_headroom(&self) -> Option<u64> {
        self.snapshot_gpu.as_ref().and_then(|gpu| gpu.memory_headroom())
    }
    fn preflight_image_object_affine(&self,scene:layer_core::SceneView<'_>,object:layer_core::ImageObjectHandle,
        affine:layer_core::Affine64,view:ViewState,
    )->Result<(),Self::Error> {
        self.snapshot_gpu.as_ref().ok_or(BackendError("Canvas renderer is unavailable"))?
            .preflight_image_object_affine(scene,object,affine,view).map_err(|_|BackendError("Image sampling request is unsupported"))
    }
    fn poll_pending(&mut self, view: ViewState) -> Result<(), Self::Error> {
        if self.has_pending_submission() && self.settling_view.as_ref().is_none_or(|(old, cursor)| *old != view || *cursor != self.cursor)
            && let Some(geometry) = self.geometry {
            self.send(Command::Present(view, geometry, self.cursor.clone()))?;
            self.settling_view = Some((view, self.cursor.clone()));
        }
        Ok(())
    }
    fn evaluation_context(&self) -> EvaluationContext {
        self.report.context.try_read().map(|context| context.clone()).unwrap_or_else(|_| self.context_seed.clone())
    }
    fn seed_evaluation_context(&mut self, context: EvaluationContext) {
        self.context_seed = context.clone();
        if let Ok(mut value) = self.report.context.try_write() { *value = context.clone(); }
        let _ = self.send(Command::SeedContext(context));
    }
    fn has_pending_submission(&self) -> bool { self.settling_roots.iter().any(|r| r.try_data().is_none()) }
    fn can_submit(&self) -> bool {
        !self.has_pending_submission() && self.in_flight.load(Ordering::Acquire) < 2
    }
    fn has_pending_work(&self) -> bool {
        self.report.pending_work.load(Ordering::Acquire)
    }
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        self.transform_preview = preview.cloned();
        Ok(())
    }
    fn prepare_moving_layer(&mut self, layer: Option<OccurrenceHandle>) {
        if self.moving_layer != layer && self.send(Command::MovingLayer(layer)).is_ok() {
            self.moving_layer = layer;
        }
    }
    fn prepare_moving_pixels(&mut self, pixels: Option<(SourceTarget, layer_core::Selection)>) {
        if self.moving_pixels != pixels && self.send(Command::MovingPixels(pixels.clone())).is_ok() {
            self.moving_pixels = pixels;
        }
    }
    fn prepare_retouch(&mut self, retouch: Option<&layer_render::RetouchPreparation>) {
        if self.retouch.as_ref() != retouch && self.send(Command::Retouch(retouch.cloned())).is_ok() {
            self.retouch = retouch.cloned();
        }
    }
    fn prepare_regions(&mut self, regions:Option<&layer_render::RegionPreparation>) {
        if self.regions.as_ref()!=regions && self.send(Command::Regions(regions.cloned())).is_ok() {self.regions=regions.cloned();}
    }
    fn take_retouch_miss(&mut self) -> Option<layer_core::StrokeId> {
        let stroke = self.report.retouch_miss.swap(0, Ordering::AcqRel);
        (stroke != 0).then_some(layer_core::StrokeId(stroke))
    }
    fn retouch_waiting(&self) -> Option<layer_core::StrokeId> {
        let stroke = self.report.retouch_waiting.load(Ordering::Acquire);
        (stroke != 0).then_some(layer_core::StrokeId(stroke))
    }
    fn retire_stroke_sources(&mut self) {
        let _ = self.send(Command::RetireStrokeSources);
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
        Arc::make_mut(&mut self.overlays).selection = None;
        let _ = self.send(Command::CancelSelectionPaint(self.selection_generation));
    }
    fn set_quick_mask_thumbnail(&mut self, selection: Option<&layer_core::Selection>) {
        if self.quick_thumbnail.as_ref() != selection {
            self.quick_thumbnail = selection.cloned();
            let _ = self.send(Command::QuickMaskThumbnail(self.quick_thumbnail.clone()));
        }
    }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) {
        if self.overlays.selection_overlay != overlay {
            let state = Arc::make_mut(&mut self.overlays);
            state.selection_overlay = overlay;
            state.selection = None;
        }
    }
    fn set_clipping_preview(&mut self, shadows: bool, highlights: bool) {
        if self.overlays.clipping != [shadows, highlights] {
            Arc::make_mut(&mut self.overlays).clipping = [shadows, highlights];
        }
    }
    fn set_crop_overlay(&mut self, overlay: Option<layer_render::CropOverlay>) {
        if self.overlays.crop != overlay { Arc::make_mut(&mut self.overlays).crop = overlay; }
    }
    fn set_vector_preview(&mut self, preview: Option<layer_render::VectorPreview>) {
        let key = |value: &layer_render::VectorPreview| (value.owner, Arc::as_ptr(&value.path), value.revision);
        if self.overlays.vector_preview.as_ref().map(key) != preview.as_ref().map(key) {
            Arc::make_mut(&mut self.overlays).vector_preview = preview;
        }
    }
    fn set_vector_motion(&mut self, motion: Option<layer_render::VectorMotion>) {
        let key = |value: &layer_render::VectorMotion| (value.owner, Arc::as_ptr(&value.items), value.transform);
        if self.overlays.vector_motion.as_ref().map(key) != motion.as_ref().map(key) {
            Arc::make_mut(&mut self.overlays).vector_motion = motion;
        }
    }
    fn set_paper_overlay(&mut self, overlay: Option<layer_render::PaperOverlay>) {
        if self.overlays.paper != overlay {
            let state = Arc::make_mut(&mut self.overlays);
            if state.paper.as_ref().map(|value| &value.paper) != overlay.as_ref().map(|value| &value.paper) {
                state.paper_generation += 1;
            }
            state.paper = overlay;
        }
    }
    fn paper_overlay_ready(&self, paper: &layer_core::paper::PaperBinding) -> bool {
        self.overlays.paper.as_ref().is_some_and(|value| value.paper == *paper)
            && self.report.paper_drawn.load(Ordering::Acquire) == self.overlays.paper_generation
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        if self.region_pending {
            return Ok(false);
        }
        self.send(Command::Region(self.region_generation, request))?;
        self.region_pending = true;
        Ok(true)
    }
    fn cancel_region(&mut self) {
        self.region_generation = self.region_generation.wrapping_add(1);
        self.region_pending = false;
        self.region = None;
        let _ = self.send(Command::CancelRegion(self.region_generation));
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        self.ready().ok()?;
        self.region
            .take()
            .map(|result| result.map_err(|_| BackendError("Region detection failed")))
    }
    fn set_selection_outline(&mut self, selection: Option<&layer_core::Selection>) -> Result<(), Self::Error> {
        if self.overlays.selection.as_ref() != selection {
            Arc::make_mut(&mut self.overlays).selection = selection.cloned();
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
        let mut telemetry = self.telemetry
            .try_lock()
            .map(|t| t.clone())
            .unwrap_or_default();
        telemetry.frame_rate = self.clock.presented_fps(gtk::glib::monotonic_time().max(0) as u64 * 1000)
            .map(|fps| layer_render::FrameRate { fps, source: layer_render::FrameRateSource::Presented });
        telemetry
    }
    fn request_thumbnail(
        &mut self,
        id: u64,
        target: layer_render::ThumbnailTarget,
    ) -> Result<(), Self::Error> {
        self.send(Command::Thumbnail(id, target))
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.thumbnails.pop_front().map(Ok)
    }
    fn request_brush_stamp(&mut self, id:u64, request:&layer_render::BrushStampRequest) -> Result<bool,Self::Error> {
        self.send(Command::BrushStamp(id,request.clone()))?;
        Ok(true)
    }
    fn take_brush_stamp(&mut self) -> Option<Result<ReadbackImage,Self::Error>> {
        if self.ready().is_err() { return Some(Err(BackendError("GPU worker failed"))); }
        self.brush_stamps.pop_front().map(|result|result.map_err(|_|BackendError("Brush preview failed")))
    }
    fn request_effect_analysis(&mut self, query: layer_core::ArtworkQuery) -> Result<bool, Self::Error> {
        if self.analysis_job.is_some() || self.analysis_candidate.is_some() { return Ok(false); }
        let (sender, receiver) = mpsc::channel();
        self.send(Command::Analysis(query, sender))?;
        self.analysis_job = Some(WorkerJob::Preparing(receiver));
        Ok(true)
    }
    fn take_effect_analysis(&mut self) -> Option<Result<(), Self::Error>> {
        let result = match self.analysis_job.as_mut()?.ready() {
            Ok(Some(job)) => job.take()?,
            Ok(None) => return None,
            Err(error) => { self.analysis_job = None; return Some(Err(error)); }
        };
        self.analysis_job = None;
        Some(result.map(|candidate| self.analysis_candidate = Some(candidate)).map_err(|_| BackendError("Could not update the adjustment")))
    }
    fn accept_effect_analysis(&mut self) -> Result<(), Self::Error> {
        if let Some(candidate) = self.analysis_candidate.take() { self.send(Command::AcceptAnalysis(candidate))?; }
        Ok(())
    }
    fn cancel_effect_analysis(&mut self) { self.analysis_job = None; self.analysis_candidate = None; }
    fn retain_effect_analyses(&mut self, layers: &[OccurrenceHandle]) -> Result<(), Self::Error> {
        self.send(Command::RetainAnalyses(layers.to_vec()))
    }
    fn request_snapshot(&mut self, request: layer_render::SnapshotRequest) -> Result<bool, Self::Error> {
        if self.snapshot_job.is_some() { return Ok(false); }
        let (sender, receiver) = mpsc::channel();
        self.send(Command::Snapshot(request, sender))?;
        self.snapshot_job = Some(WorkerJob::Preparing(receiver));
        Ok(true)
    }
    fn take_snapshot(&mut self) -> Option<Result<layer_render::SnapshotResult, Self::Error>> {
        let result = match self.snapshot_job.as_mut()?.ready() {
            Ok(Some(job)) => job.take()?,
            Ok(None) => return None,
            Err(error) => { self.snapshot_job = None; return Some(Err(error)); }
        };
        self.snapshot_job = None;
        Some(result.map_err(|_| BackendError("Could not complete the operation")))
    }
    fn cancel_snapshot(&mut self) { self.snapshot_job = None; }
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
        let [width, height] = source.extent;
        self.brush_sources.insert(
            asset.clone(),
            BrushSource { image: source.clone(), outline: layer_render::mask_outline(width, height, width, &source.bytes) },
        );
        self.send(Command::Asset(asset.clone(), source))
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
        match packet.selection_overlays {
            Some(visibility) if visibility != self.selection_overlays.as_ref() => self.selection_overlays = Arc::new(visibility.clone()),
            None if *self.selection_overlays != layer_core::authored::SelectionOverlays::default() => self.selection_overlays = Arc::default(),
            _ => (),
        }
        let input_order = if self.present_frames {
            input_frame_order(&mut self.input_order, packet.dabs.len(), packet.dab_batches.iter().any(|batch| batch.stroke_end))
        } else { 0 };
        let frame = Frame {
            overlays: self.overlays.clone(),
            time_seconds: packet.time_seconds,
            view: packet.view,
            extent: packet.document_extent,
            scene: Arc::new(packet.scene.snapshot()),
            inspect_mask: packet.inspect_mask,
            transform_preview: self.transform_preview.clone(),
            selection_overlays: self.selection_overlays.clone(),
            pending_rasters: packet.scene.targets().filter_map(|target| packet.scene.raster(target))
                .filter(|r| packet.commit_rasters && r.try_data().is_none()).cloned().collect(),
            dabs: packet.dabs.to_vec(),
            batches: packet.dab_batches.to_vec(),
            restore_rasters: packet.restore_rasters.to_vec(),
            reset: packet.reset_layers,
            input_order,
            composite: packet.composite_all,
            commit_rasters: packet.commit_rasters,
            blend_space: packet.blend_space,
            geometry,
            present: self.present_frames,
            picker: self.picker,
            cursor: self.cursor.clone(),
            chrome: self.chrome_presentation(),
            stroke_target: self.stroke_target,
            #[cfg(test)]
            queued_ns: gtk::glib::monotonic_time().max(0) as u64 * 1000,
        };
        self.in_flight.fetch_add(1, Ordering::Release);
        self.first_frame_sent = true;
        self.settling_view = None;
        self.settling_roots = if packet.dab_batches.iter().any(|b| b.stroke_end && b.style.execution.heals()) { frame.pending_rasters.clone() } else { Vec::new() };
        if let Err(e) = self.send(Command::Frame(Box::new(frame))) {
            self.in_flight.fetch_sub(1, Ordering::Release);
            return Err(e);
        }
        Ok(())
    }
}

struct Worker {
    watch: layer_host::DeviceWatch,
    backdrop_submitted: bool,
    paper_ready: Arc<AtomicBool>,
    overlays: Option<Arc<Overlays>>,
    // Drop Vulkan's surface before the wl_surface (field declaration order).
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    child: Child,
    renderer: WgpuRasterizer,
    presenter: ViewportPresenter,
    prepared_color: Option<color::Prepared>,
    config: wgpu::SurfaceConfiguration,
    surface_configured: bool,
    view_color: crate::display_color::ViewColor,
    last_view: Option<ViewState>,
    surround: [f32; 4],
    geometry: Option<Geometry>,
    geometry_changed: bool,
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
    screen_headroom: f32,
    screen_check: Option<layer_render_wgpu::ScreenCheck>,
    screen_generation: u64,
    checked_generation: u64,
    screen_reported: Option<u64>,
    last_publish: std::time::Instant,
    chrome: ChromeHandoff,
    chrome_requests: ChromeRequests,
    presented_chrome: Arc<ChromeStatus>,
    initial_completion: Option<Arc<AtomicBool>>,
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
        // Exercise a real wgpu validation failure, without resetting the driver
        // or touching any other application's GPU device.
        self.renderer.queue().submit([encoder.finish()]);
    }
    fn run(
        mut self,
        receiver: &mpsc::Receiver<Command>,
        chrome_requests: &mpsc::Receiver<ChromeUpdate>,
        reply: &mpsc::Sender<Reply>,
        worker_telemetry: &std::sync::Mutex<layer_render::RendererTelemetry>,
        count: &AtomicUsize,
        report: &FrameReport,
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
        let mut context_capture: Option<ContextRequest> = None;
        let mut deferred = VecDeque::new();
        let mut selection_generation = 0;
        let mut region_generation = 0;
        let mut pending_thumbnails = VecDeque::new();
        let mut pending_stamp = None;
        let mut stamp_flight = false;
        let mut last_canvas_frame = std::time::Instant::now();
        let mut filter_preview_generation = 0;
        let mut document_drawn = false;
        #[cfg(test)]
        let mut fail_next_frame = false;
        loop {
            if !matches!(self.chrome.first, FirstChrome::Granted | FirstChrome::Submitted) {
                for request in chrome_requests.try_iter() {
                    match request {
                        ChromeUpdate::Current(chrome, after_input) => {
                            if self.chrome.first == FirstChrome::Visible {
                                self.chrome_requests.request(chrome, after_input);
                            } else {
                                self.set_chrome(&chrome)?;
                            }
                        },
                        ChromeUpdate::Initial(chrome) => if self.chrome.grant(chrome.revision) {
                            self.apply_chrome(&chrome);
                            self.pending_present = true;
                            break;
                        },
                    }
                }
            }
            self.apply_pending_chrome()?;
            if let Some(failure) = self.watch.failure() {
                return Err(failure);
            }
            self.renderer
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(error)?;
            self.child.dispatch()?;
            if self.child.take_initial_commit() && self.chrome.initial_applied() {
                self.child.synchronize(false)?;
                self.initial_completion = None;
                self.presented_chrome.publish(self.chrome.published);
                canvas_action(&self.area, "canvas.chrome", &self.presented_chrome);
            }
            self.prepare_initial_chrome();
            let (revision, screen) = self.child.screen_color();
            if self.screen_reported != Some(revision) {
                self.screen_reported = Some(revision);
                reply.send(Reply::Screen(screen)).map_err(error)?;
                wake_canvas(&self.area, &self.presented_chrome);
            }
            if let Some(result) = self.presenter.screen_check_result() {
                if let Err(e) = &result { eprintln!("Screen gamut check: {e}"); }
                reply.send(Reply::ScreenClipped(self.checked_generation, result.ok())).map_err(error)?;
                wake_canvas(&self.area, &self.presented_chrome);
            }
            let headroom = if self.hdr_encoding.is_some() { self.screen_headroom } else { 1. };
            if headroom != self.display_headroom {
                self.display_headroom = headroom;
                self.update_hdr_view()?;
                self.report_display(reply)?;
            }
            let mut report_startup = None;
            if let Some((generation, document, brush, transform, mode)) = &startup_input
                && (*mode == StartupMode::Unpresented || self.paper_ready.load(Ordering::Acquire))
            {
                if self
                    .renderer
                    .startup_needs_update(document, brush, *transform)
                {
                    self.renderer
                        .prepare_startup(document, brush, *transform)
                        .map_err(error)?;
                }
                report_startup = Some(*generation);
            }
            let presenter_ready=self.presenter.prepare_chrome(&self.renderer);
            let mut progress = self.renderer.poll_startup().map_err(error)?;
            progress.canvas_ready &= presenter_ready;
            progress.brush_ready &= presenter_ready;
            progress.complete &= presenter_ready;
            if let Some(generation) = report_startup {
                if progress != startup_progress {
                    startup_progress = progress;
                    reply
                        .send(Reply::Startup(
                            generation,
                            progress,
                            self.renderer.brush_sources(),
                        ))
                        .map_err(error)?;
                }
                if progress.complete {
                    startup_input = None;
                }
            }
            if self.initial_completion.is_none() && self.renderer.has_pending_submission() && let Some(view) = self.last_view {
                self.renderer.poll_pending(view).map_err(error)?;
                if !self.renderer.has_pending_submission() {
                    self.pending_present |= self.presenter.needs_present(&self.renderer, view, self.surround);
                }
                self.report_frame(report);
            }
            let admitted = admit_frame(None, &mut pending_frames,
                |frame| self.draw_ready_frame(frame, &startup_progress, report, #[cfg(test)] &mut timing))?;
            if admitted != 0 {
                last_canvas_frame = std::time::Instant::now();
                document_drawn = true;
                count.fetch_sub(admitted, Ordering::Release);
            }
            if context_capture.is_some() && pending_frames.is_empty() && !self.renderer.has_pending_submission() {
                let request = context_capture.take().unwrap();
                let _ = request.value.set(Ok(self.renderer.evaluation_context()));
            }
            let queued = if context_capture.is_none() && self.initial_completion.is_none() && document_drawn && !deferred.is_empty() {
                Some(deferred.pop_front().unwrap())
            } else {
                match receiver.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            };
            if queued.is_none() {
                if self.pending_present && self.renderer.has_presentable_frame() {
                    if self.chrome.release_for_canvas() { self.child.synchronize(false)?; }
                    if let Some(target) = self.acquire()? { self.publish(target, None, false, #[cfg(test)] None, #[cfg(test)] None)?; }
                } else if pending_frames.is_empty() {
                    self.publish_chrome(#[cfg(test)] count.load(Ordering::Acquire))?;
                }
            }
            if queued.is_none() {
                if telemetry_enabled && let Ok(mut snapshot) = worker_telemetry.try_lock() {
                    *snapshot = self.renderer.telemetry();
                    snapshot.resident_bytes += self.presenter.proof_storage_bytes();
                }
                #[cfg(test)]
                timing.presented(self.child.take_presented());
                #[cfg(test)]
                timing.camera_feedback(self.child.take_camera_feedback());
                while let Some(image) = self.renderer.take_thumbnail() {
                    reply
                        .send(Reply::Thumbnail(image.map_err(error)?))
                        .map_err(error)?;
                }
                if let Some(image) = self.renderer.take_brush_stamp() {
                    stamp_flight = false;
                    reply.send(Reply::BrushStamp(image.map_err(error))).map_err(error)?;
                }
                if !stamp_flight && pending_frames.is_empty()
                    && let Some((id,request)) = pending_stamp.as_ref()
                {
                    match self.renderer.request_brush_stamp(*id,request) {
                        Ok(true) => { pending_stamp=None; stamp_flight=true; },
                        Ok(false) => (),
                        Err(failure) => { pending_stamp=None; reply.send(Reply::BrushStamp(Err(error(failure)))).map_err(error)?; },
                    }
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
                        .send(Reply::Region(region_generation, region.map_err(error)))
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
            }
            // A small preview can wait through continuous drawing/navigation.
            // Batching bounds interruption when input resumes during idle work;
            // the quiet period keeps those batches from competing every frame.
            let thumbnail_ready = queued.is_none() && !pending_thumbnails.is_empty()
                && last_canvas_frame.elapsed() >= Duration::from_millis(50);
            if context_capture.is_some() {
                match queued.map(Ok).unwrap_or_else(|| receiver.recv_timeout(Duration::from_millis(1))) {
                    Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(command) => deferred.push_back(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
                continue;
            }
            let next = if let Some(command) = queued {
                Ok(command)
            } else if !startup_progress.complete
                || !pending_frames.is_empty()
                || self.renderer.has_pending_submission()
                || self.renderer.effect_validation_pending()
                || self.renderer.filter_previews_pending()
                || self.pending_present
                || !self.chrome.can_present()
                || self.chrome.first != FirstChrome::Visible
                || stamp_flight || pending_stamp.is_some()
                || self.renderer.thumbnails_pending()
                || !pending_thumbnails.is_empty()
                || self.renderer.color_sample_pending()
                || self.renderer.selection_paint_pending()
                || self.renderer.region_pending()
                || self.child.feedback_pending()
                || self.presenter.screen_check_busy()
            {
                receiver.recv_timeout(Duration::from_millis(if thumbnail_ready || self.renderer.region_pending() { 1 } else { 8 }))
            } else if self.hdr_encoding.is_some() {
                // Display changes arrive on Wayland even when artwork is idle.
                receiver.recv_timeout(Duration::from_millis(100))
            } else if self.child.watching_display() || self.screen_check.is_some() {
                receiver.recv_timeout(SCREEN_CHECK_DELAY)
            } else {
                receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            let command = match next {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !self.pending_present
                        && pending_frames.is_empty()
                        && count.load(Ordering::Acquire) == 0
                        && self.last_publish.elapsed() >= SCREEN_CHECK_DELAY
                        && self.presenter.check_screen(&self.renderer)
                    {
                        self.checked_generation = self.screen_generation;
                    }
                    // Visible canvas work wins over background layer artwork.
                    // A whole-photo thumbnail scan used to block this owner for
                    // ~200 ms even though every camera frame rendered in <2 ms.
                    if thumbnail_ready && count.load(Ordering::Acquire) == 0
                        && let Some(&(id, target)) = pending_thumbnails.front()
                    {
                        let result = self.renderer.prepare_thumbnail_batch(target).and_then(|ready| {
                            if ready { self.renderer.request_thumbnail(id, target).map(|_| true) } else { Ok(false) }
                        });
                        self.report_frame(report);
                        match result {
                            Ok(false) => (),
                            Ok(true) => { pending_thumbnails.pop_front(); },
                            Err(layer_render_wgpu::GpuRasterError::ThumbnailUnavailable(_)) => {
                                pending_thumbnails.pop_front();
                                reply.send(Reply::ThumbnailCancelled(id)).map_err(error)?;
                                wake_canvas(&self.area, &self.presented_chrome);
                            }
                            Err(failure) => return Err(error(failure)),
                        }
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if self.initial_completion.is_some() && !matches!(command, Command::Stop | Command::Wake | Command::ReleaseChrome(_)) {
                deferred.push_back(command);
                continue;
            }
            if !document_drawn
                && matches!(
                    command,
                    Command::Region(..)
                        | Command::SelectionPaint(..)
                        | Command::Thumbnail(..)
                        | Command::Analysis(..)
                        | Command::AcceptAnalysis(..)
                        | Command::RetainAnalyses(..)
                        | Command::Snapshot(..)
                        | Command::ColorSample(_)
                        | Command::FilterPreviews(..)
                )
            {
                deferred.push_back(command);
                continue;
            }
            match command {
                Command::SeedContext(context) => self.renderer.seed_evaluation_context(context),
                Command::CaptureContext(request) => context_capture = Some(request),
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
                Command::Screen(generation, headroom, check) => {
                    self.screen_generation = generation;
                    self.screen_headroom = headroom;
                    self.screen_check = check;
                    self.presenter.set_screen_check(&self.renderer, check);
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
                    startup_progress = layer_render_wgpu::StartupProgress::COMPLETE;
                    document_drawn = true;
                    reply.send(Reply::ColorAdopted(id, self.renderer.brush_sources(), self.renderer.snapshot_gpu(), self.renderer.shader_activity())).map_err(error)?;
                }
                Command::DiscardColor(id, reply) => {
                    self.discard_color(id);
                    let _ = reply.send(());
                }
                #[cfg(test)]
                Command::FailNextFrame => fail_next_frame = true,
                Command::MovingLayer(layer) => self.renderer.prepare_moving_layer(layer),
                Command::MovingPixels(pixels) => self.renderer.prepare_moving_pixels(pixels),
                Command::Retouch(retouch) => self.renderer.prepare_retouch(retouch.as_ref()),
                Command::Regions(regions)=>self.renderer.prepare_regions(regions.as_ref()),
                Command::RetireStrokeSources => self.renderer.retire_stroke_sources(),
                Command::Startup(generation, mode, inputs) => {
                    let (document, brush, transform) = *inputs;
                    startup_input = Some((generation, document, brush, transform, mode));
                    startup_progress = Default::default();
                }
                Command::FinishStartupCatalog => self.renderer.finish_startup_catalog(),
                Command::SelectionPaint(generation, update) => {
                    if generation != selection_generation { continue; }
                    let result = self.renderer.paint_selection(&update).map_err(error);
                    reply.send(Reply::SelectionPaintAck(generation, result)).map_err(error)?;
                }
                Command::CancelSelectionPaint(generation) => {
                    selection_generation = generation; self.renderer.cancel_selection_paint();
                    self.overlays = None;
                }
                Command::QuickMaskThumbnail(selection) => self.renderer.set_quick_mask_thumbnail(selection.as_ref()),
                Command::CancelRegion(generation) => {
                    region_generation = generation;
                    deferred.retain(|c| !matches!(c, Command::Region(..)));
                    self.renderer.cancel_region();
                }
                Command::Region(generation, request) => {
                    region_generation = generation;
                    let result = self.renderer.request_region(request);
                    if !matches!(result, Ok(true)) {
                        reply
                            .send(Reply::Region(generation, Err(result
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
                Command::BrushStamp(id, request) => { pending_stamp=Some((id,request)); }
                Command::Thumbnail(id, target) => {
                    pending_thumbnails.push_back((id, target));
                }
                Command::Analysis(query, sender) => {
                    let _ = sender.send(layer_render_wgpu::effect_analysis::Job::start(self.renderer.snapshot_gpu(), query));
                }
                Command::AcceptAnalysis(candidate) => { self.renderer.apply_effect_analysis(candidate); self.report_frame(report); },
                Command::RetainAnalyses(layers) => { self.renderer.retain_effect_analyses(&layers).map_err(error)?; self.report_frame(report); },
                Command::Snapshot(request, sender) => {
                    let _ = sender.send(layer_render_wgpu::snapshot::SnapshotJob::start(self.renderer.snapshot_gpu(), request));
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
                Command::Wake => (),
                Command::ReleaseChrome(revision) => {
                    if self.chrome.release(revision) {
                        self.child.synchronize(false)?;
                    }
                }
                Command::Present(view, geometry, cursor) => {
                    self.set_view(view, geometry)?;
                    self.cursor = cursor;
                    self.presenter.set_cursor(self.renderer.device(), &self.cursor, geometry.scale as f32);
                    if let Some(target) = self.acquire()? { self.publish(target, None, false, #[cfg(test)] None, #[cfg(test)] None)?; }
                }
                Command::Frame(frame) => {
                    #[cfg(test)]
                    if fail_next_frame {
                        self.inject_validation_failure();
                    }
                    self.chrome_requests.considered(frame.input_order);
                    let admitted = admit_frame(Some(frame), &mut pending_frames,
                        |frame| self.draw_ready_frame(frame, &startup_progress, report, #[cfg(test)] &mut timing))?;
                    if admitted != 0 {
                        last_canvas_frame = std::time::Instant::now();
                        document_drawn = true;
                        count.fetch_sub(admitted, Ordering::Release);
                    }
                }
                Command::Asset(id, asset) => {
                    let [width, height] = asset.extent;
                    self.renderer
                        .prepare_asset(
                            &id,
                            HostImage { width, height, stride: width, format: asset.format, bytes: &asset.bytes },
                        )
                        .map_err(error)?;
                }
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
        presented_chrome: Arc<ChromeStatus>,
        color: layer_core::color::DocumentColor,
        localization: &layer_ui::Localizer,
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
        let limits = layer_render_wgpu::rendering_limits(adapter.limits());
        layer_host::admit_adapter(localization, &adapter, &limits)?;
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
        let (device, queue, watch) = pollster::block_on(layer_host::request_device(&adapter, &wgpu::DeviceDescriptor {
            label: Some("Wayland canvas GPU"),
            required_features: layer_render_wgpu::rendering_features(&adapter) | adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            required_limits: limits,
            memory_hints: layer_render_wgpu::memory_hints(),
            ..Default::default()
        }))
        .map_err(error)?;
        eprintln!(
            "Wayland canvas GPU: {:?}, {:?}, {:?}",
            adapter.get_info(),
            config.present_mode,
            config.format
        );
        let mut renderer = *WgpuRasterizer::from_wgpu_native_staged_boxed(
            adapter, device, queue, color,
        ).map_err(error)?;
        renderer.configure_ui_previews(view_color.space()).map_err(error)?;
        eprintln!("Wayland canvas color: {:?}; available: {:?}", config.color_space, caps.format_capabilities);
        let presenter = workspace_presenter(&renderer, config.format, hdr_encoding.unwrap_or_else(|| view_color.surface()))?;
        Ok(Self {
            watch,
            view_color,
            backdrop_submitted: false,
            paper_ready: Arc::new(AtomicBool::new(false)),
            overlays: None,
            surface,
            instance,
            child,
            renderer,
            presenter,
            prepared_color: None,
            config,
            surface_configured: false,
            last_view: None,
            surround: [0.; 4],
            geometry: None,
            geometry_changed: false,
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
            screen_headroom: 1.,
            screen_check: None,
            screen_generation: 0,
            checked_generation: 0,
            screen_reported: None,
            last_publish: std::time::Instant::now(),
            chrome: ChromeHandoff::default(),
            chrome_requests: ChromeRequests::default(),
            presented_chrome,
            initial_completion: None,
        })
    }
    /// Publish whether the renderer has work for later frames, and wake an
    /// idle canvas when it newly does. A retouching stroke that missed its
    /// source is kept for the session to replay.
    fn report_frame(&mut self, report: &FrameReport) {
        if let Ok(mut context) = report.context.try_write() { *context = self.renderer.evaluation_context(); }
        if let Some(stroke) = self.renderer.take_retouch_miss() {
            report.retouch_miss.store(stroke.0, Ordering::Release);
        }
        report.retouch_waiting.store(self.renderer.retouch_waiting().map_or(0, |stroke| stroke.0), Ordering::Release);
        if let Some(state) = &self.overlays && let Some(overlay) = &state.paper
            && self.renderer.paper_overlay_ready(&overlay.paper) {
            report.paper_drawn.store(state.paper_generation, Ordering::Release);
        }
        let pending = self.renderer.has_pending_work();
        if pending && !report.pending_work.swap(pending, Ordering::AcqRel) {
            wake_canvas(&self.area, &self.presented_chrome);
        }
        report.pending_work.store(pending, Ordering::Release);
    }
    fn report_display(&self, reply: &mpsc::Sender<Reply>) -> Result<(), String> {
        reply.send(Reply::DisplayHeadroom(self.display_headroom, self.hdr_encoding)).map_err(error)?;
        wake_canvas(&self.area, &self.presented_chrome);
        Ok(())
    }
    fn enable_hdr(&mut self) -> Result<(), String> {
        self.hdr_attempted = true;
        if !self.hdr_float_supported { return Ok(()); }
        let Some(encoding) = self.child.describe_hdr()? else { return Ok(()); };
        let mut presenter = workspace_presenter(&self.renderer, wgpu::TextureFormat::Rgba16Float, encoding)?;
        presenter.inherit_proof(&self.renderer, &self.presenter);
        presenter.set_screen_check(&self.renderer, self.screen_check);
        self.presenter = presenter;
        self.hdr_encoding = Some(encoding);
        self.config.format = wgpu::TextureFormat::Rgba16Float;
        self.surface_configured = false;
        self.last_view = None;
        self.pending_present = false;
        Ok(())
    }
    fn update_hdr_view(&mut self) -> Result<(), String> {
        let headroom = if self.preview_sdr { 1. } else { self.display_headroom };
        self.presenter.set_hdr_view(&self.renderer, self.hdr_rendition, headroom).map_err(error)?;
        // Wayland may deliver an HDR display hint before GTK has supplied the
        // first canvas geometry. Update the transform now, but only redraw an
        // existing frame: acquire() configures the swapchain before publication.
        self.pending_present = self.last_view.is_some();
        Ok(())
    }
    fn frame_readiness(&mut self, frame: &Frame, canvas_ready: bool, brush_ready: bool) -> FrameReadiness {
        if self.initial_completion.is_some() || self.backdrop_submitted && !canvas_ready
            || !self.renderer.can_submit() || self.backdrop_submitted && !frame.dabs.is_empty() && !brush_ready {
            FrameReadiness::Waiting
        } else if !self.dependencies_ready(frame) {
            FrameReadiness::Dependencies
        } else { FrameReadiness::Ready }
    }
    fn draw_ready_frame(
        &mut self,
        frame: &mut Frame,
        startup: &layer_render_wgpu::StartupProgress,
        report: &FrameReport,
        #[cfg(test)] timing: &mut crate::timing::Timing,
    ) -> Result<bool, String> {
        let readiness = self.frame_readiness(frame, startup.canvas_ready, startup.brush_ready);
        if !readiness.ready() {
            if readiness == FrameReadiness::Dependencies { self.report_frame(report); }
            return Ok(false);
        }
        #[cfg(test)]
        timing.begin(frame.queued_ns);
        let backdrop = !self.backdrop_submitted;
        self.draw(frame, backdrop, #[cfg(test)] timing)?;
        if !backdrop { self.report_frame(report); }
        Ok(!backdrop)
    }
    fn apply_pending_chrome(&mut self) -> Result<(), String> {
        while let Some(chrome) = self.chrome_requests.take_ready() { self.set_chrome(&chrome)?; }
        Ok(())
    }
    fn publish_chrome(&mut self, #[cfg(test)] queued_frames_at_admission: usize) -> Result<(), String> {
        self.apply_pending_chrome()?;
        if self.chrome.needs_publication()
            && self.renderer.has_presentable_frame()
            && let Some(target) = self.acquire()? {
            self.publish(target, None, true, #[cfg(test)] None, #[cfg(test)] Some(queued_frames_at_admission))?;
        }
        Ok(())
    }
    fn set_chrome(&mut self, chrome: &ChromePresentation) -> Result<(), String> {
        if !self.chrome.request(chrome.revision, chrome.hold) { return Ok(()); }
        if !chrome.hold && self.chrome.release_for_canvas() { self.child.synchronize(false)?; }
        self.apply_chrome(chrome);
        Ok(())
    }
    fn apply_chrome(&mut self, chrome: &ChromePresentation) {
        self.surround = chrome.surround;
        self.presenter.set_backdrop(&self.renderer, &chrome.backdrops, chrome.style, chrome.backdrop_hold);
        self.presenter.set_panel_surfaces(&self.renderer, &chrome.panels);
        self.overviews.clone_from(&chrome.overviews);
        self.presenter.set_overviews(&self.renderer, &self.overviews);
    }
    fn prepare_initial_chrome(&mut self) {
        if self.chrome.first != FirstChrome::Preparing { return; }
        if let Some(done) = &self.initial_completion {
            if done.load(Ordering::Acquire) {
                self.chrome.ready();
                self.presented_chrome.value.store(CHROME_READY, Ordering::Release);
                canvas_action(&self.area, "canvas.chrome", &self.presented_chrome);
            }
        } else if self.last_view.is_some() && self.renderer.has_presentable_frame()
            && self.presenter.pipelines_ready(&self.renderer) && self.presenter.chrome_ready() {
            let done = Arc::new(AtomicBool::new(false));
            let completed = done.clone();
            self.renderer.queue().on_submitted_work_done(move || completed.store(true, Ordering::Release));
            self.initial_completion = Some(done);
        }
    }
    fn set_view(&mut self, view: ViewState, geometry: Geometry) -> Result<(), String> {
        if self.chrome.release_for_canvas() { self.child.synchronize(false)?; }
        self.presenter.set_corner_radius(geometry.corner_radius * geometry.scale as f32);
        self.last_view = Some(view);
        self.geometry_changed |= self.geometry != Some(geometry);
        self.geometry = Some(geometry);
        self.pending_present = true;
        Ok(())
    }
    fn dependencies_ready(&mut self,frame:&Frame)->bool {
        if self.backdrop_submitted {self.renderer.raster_dependencies_ready(frame.packet(),frame.transform_preview.as_ref())}else {
            let scope=SceneScope::Members(Arc::from(frame.scene.view().constant_backdrop()));
            self.renderer.raster_dependencies_ready(frame.backdrop_packet(&scope),None)
        }
    }
    fn apply_overlays(&mut self, state: &Arc<Overlays>, #[cfg(test)] timing: &mut crate::timing::Timing) -> Result<(), String> {
        if self.overlays.as_ref().is_some_and(|current| Arc::ptr_eq(current, state)) { return Ok(()); }
        self.renderer.set_selection_overlay(state.selection_overlay);
        self.renderer.set_selection_outline(state.selection.as_ref()).map_err(error)?;
        self.renderer.set_crop_overlay(state.crop);
        self.renderer.set_vector_preview(state.vector_preview.clone());
        self.renderer.set_vector_motion(state.vector_motion.clone());
        self.renderer.set_clipping_preview(state.clipping[0], state.clipping[1]);
        self.renderer.set_paper_overlay(state.paper.clone());
        #[cfg(test)]
        if self.overlays.as_ref().and_then(|current| current.paper.as_ref()) != state.paper.as_ref() { timing.paper_overlay(); }
        self.overlays = Some(state.clone());
        Ok(())
    }
    fn draw(
        &mut self,
        frame: &mut Frame,
        backdrop: bool,
        #[cfg(test)] timing: &mut crate::timing::Timing,
    ) -> Result<(), String> {
        if let Some(failure) = self.watch.failure() {
            return Err(failure);
        }
        let draw_start = std::time::Instant::now();
        while frame.scene.view().targets().any(|t| frame.scene.view().raster(t).is_some_and(|r| r.try_data().is_none())) && !self.renderer.raster_ready()
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if frame.present {
            self.set_chrome(&frame.chrome)?;
            self.apply_pending_chrome()?;
            self.set_view(frame.view, frame.geometry)?;
        }
        #[cfg(test)]
        timing.rendering(&self.renderer);
        self.renderer
            .resize_surface(frame.view.width_px, frame.view.height_px)
            .map_err(error)?;
        self.apply_overlays(&frame.overlays, #[cfg(test)] timing)?;
        let _presentation = self.renderer.prioritize_raster_presentation();
        #[cfg(test)]
        if !backdrop && frame.scene.view().targets().any(|t| frame.scene.view().raster(t).is_some_and(|r| r.try_data().is_none())) {
            timing.raster_commit();
        }
        if backdrop {
            let scene = frame.scene.view();
            let scope = SceneScope::Members(Arc::from(scene.constant_backdrop()));
            self.renderer.submit(frame.backdrop_packet(&scope))                .map_err(error)?;
            self.backdrop_submitted = true;
        } else {
            self.renderer.set_transform_preview(frame.transform_preview.as_ref()).map_err(error)?;
            self.renderer.submit(frame.packet()).map_err(error)?;
            if self.renderer.has_pending_submission() { frame.pending_rasters.clear(); }
            #[cfg(test)]
            timing.photo_frame(frame.scene.view());
        }
        #[cfg(test)]
        timing.mark(0);
        if !frame.present {return Ok(());}
        self.picker = frame.picker;
        self.presenter.set_color_picker(&self.renderer, self.picker);
        self.cursor.clone_from(&frame.cursor);
        self.cursor_scale = frame.geometry.scale as f32;
        self.presenter
            .set_cursor(self.renderer.device(), &self.cursor, self.cursor_scale);
        self.pending_present = self.chrome.changed() || self.geometry_changed
            || self.presenter.needs_present(&self.renderer, frame.view, self.surround);
        let target = if self.pending_present {
            #[cfg(test)]
            let target = timing.acquire(|| self.acquire())?;
            #[cfg(not(test))]
            let target = self.acquire()?;
            target
        } else { None };
        if let Some(target) = target {
            self.publish(
                target,
                frame.stroke_target.filter(|_| !backdrop),
                false,
                #[cfg(test)]
                Some(timing),
                #[cfg(test)]
                None,
            )?;
        }
        self.child.observe_draw_work(draw_start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        Ok(())
    }
    fn configure_surface(&mut self) {
        self.surface.configure(self.renderer.device(), &self.config);
        self.surface_configured = true;
        self.presenter.set_target_retention(false);
    }

    fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, String> {
        if !self.chrome.hold && self.chrome.release_for_canvas() { self.child.synchronize(false)?; }
        if !self.chrome.can_present() { return Ok(None); }
        if !self.presenter.pipelines_ready(&self.renderer) || !self.presenter.chrome_ready() {return Ok(None);}
        if !self.renderer.has_presentable_frame() { return Ok(None); }
        let view = self.last_view.expect("rendered document");
        if !self.surface_configured || [self.config.width, self.config.height] != [view.width_px, view.height_px] {
            self.config.width = view.width_px;
            self.config.height = view.height_px;
            self.configure_surface();
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // Same owned child, adapter, device and document textures.
                self.surface = unsafe { self.instance.create_surface_unsafe(self.child.target()) }
                    .map_err(error)?;
                self.configure_surface();
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
        allow_sync: bool,
        #[cfg(test)] timing: Option<&mut crate::timing::Timing>,
        #[cfg(test)] queued_frames_at_chrome_admission: Option<usize>,
    ) -> Result<(), String> {
        let _presentation = self.renderer.prioritize_raster_presentation();
        self.last_publish = std::time::Instant::now();
        let camera = self.last_view.expect("rendered document");
        let surround = self.surround;
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.camera_work(&self.renderer);
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
        if self.chrome.should_hold(allow_sync) { self.child.synchronize(true)?; }
        if self.chrome.first == FirstChrome::Granted { self.child.initial_commit(); }
        let moved = self.child.geometry(self.geometry.expect("rendered geometry"));
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
        self.child.feedback_camera(timing.as_ref().map_or(0, |t| t.id()), stroke_target, camera.document_to_surface, self.renderer.canvas_preview_revision(), self.chrome.requested, queued_frames_at_chrome_admission);
        #[cfg(not(test))]
        self.child.feedback(0, stroke_target);
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.mark(3);
        }
        self.renderer.queue().present(target);
        if moved {
            let area = self.area.clone();
            let status = self.presented_chrome.clone();
            gtk::glib::idle_add_once(move || {
                if !status.accepting.load(Ordering::Acquire) { return; }
                if let Some(area) = area.upgrade() {
                    crate::wayland::request_parent_commit(&area);
                }
            });
        }
        if let Some(revision) = self.chrome.publish(allow_sync) {
            if self.chrome.first == FirstChrome::Visible { self.presented_chrome.publish(revision); }
            if self.chrome.hold { canvas_action(&self.area, "canvas.chrome", &self.presented_chrome); }
        }
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
        self.geometry_changed = false;
        Ok(())
    }
    fn capture(&mut self, color: crate::display_color::ViewColor) -> Result<ReadbackImage, String> {
        let view = self.last_view.ok_or("Canvas has not rendered")?;
        let surround = self.surround;
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
        presenter.set_screen_check(&self.renderer, self.screen_check);
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
