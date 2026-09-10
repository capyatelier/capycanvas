//! Bounded frame handoff and sole GPU/WSI owner. No GTK calls on the worker.
//! Small dab/layer records cross threads; live canvas pixels remain on the GPU.
use crate::wayland::{Child, Geometry, Parent};
use layer_core::{AssetId, Layer};
use layer_render::{
    BackendError, CanvasRenderer, CursorSegment, Dab, DabBatch, FramePacket, HostImage,
    PixelFormat, ReadbackImage, TipOutline, ViewState,
};
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
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
    reset: bool,
    composite: bool,
    geometry: Geometry,
    surround: [f32; 4],
    cursor: Vec<CursorSegment>,
    #[cfg(test)]
    queued_ns: u64,
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
            reset_layers: self.reset,
            composite_all: self.composite,
        }
    }
}
enum Command {
    EffectValidation(layer_render::EffectValidationRequest),
    Telemetry(bool),
    Thumbnail(u64, layer_core::LayerId),
    CanvasPreview(Option<u64>),
    ColorSample(layer_render::ColorSampleRequest),
    FilterPreviews(layer_render::FilterPreviewRequest),
    Frame(Box<Frame>),
    Asset(AssetId, [u32; 3], PixelFormat, Vec<u8>),
    Release(AssetId),
    Readback(u64),
    Capture(mpsc::Sender<Result<ReadbackImage, String>>),
    Stop,
}
enum Reply {
    EffectValidation(layer_render::EffectValidationResult),
    Thumbnail(ReadbackImage),
    CanvasPreview(Result<layer_render::CanvasPreview, String>),
    ColorSample(Result<layer_render::ColorSample, String>),
    FilterPreviews(Result<layer_render::FilterPreviewImage, String>),
    Error(String),
    Readback(ReadbackImage),
}

/// Two in-flight paint frames, including the frame being presented. GTK never
/// waits on a worker lock, Vulkan acquire, or a GPU completion fence.
pub struct RenderWorker {
    pub(super) clock: Arc<crate::wayland::FrameClock>,
    telemetry: Arc<std::sync::Mutex<layer_render::RendererTelemetry>>,
    telemetry_enabled: bool,
    commands: mpsc::Sender<Command>,
    replies: mpsc::Receiver<Reply>,
    in_flight: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
    outlines: HashMap<AssetId, TipOutline>,
    readbacks: VecDeque<ReadbackImage>,
    thumbnails: VecDeque<ReadbackImage>,
    canvas_preview: Option<Result<layer_render::CanvasPreview, String>>,
    canvas_preview_pending: bool,
    color_sample: Option<Result<layer_render::ColorSample, String>>,
    color_sample_pending: bool,
    filter_previews: VecDeque<Result<layer_render::FilterPreviewImage, String>>,
    filter_previews_pending: bool,
    effect_validation_pending: bool,
    effect_validation: Option<layer_render::EffectValidationResult>,
    pub(super) geometry: Option<Geometry>,
    pub(super) surround: [f32; 4],
    pub(super) cursor: Vec<CursorSegment>,
    #[cfg(test)]
    pub stats: Arc<std::sync::Mutex<crate::timing::Stats>>,
}
impl RenderWorker {
    pub(super) fn capture(&self) -> Result<ReadbackImage, String> {
        let (tx, rx) = mpsc::channel();
        self.send(Command::Capture(tx)).map_err(error)?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(error)?
    }
    pub(super) fn new(
        parent: Parent,
        area: gtk::glib::SendWeakRef<gtk::Picture>,
    ) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel();
        let (reply, replies) = mpsc::channel();
        let (started, start) = mpsc::sync_channel(1);
        let in_flight = Arc::new(AtomicUsize::new(0));
        let count = in_flight.clone();
        let telemetry = Arc::new(std::sync::Mutex::new(
            layer_render::RendererTelemetry::default(),
        ));
        let worker_telemetry = telemetry.clone();
        #[cfg(test)]
        let stats = Arc::new(std::sync::Mutex::new(crate::timing::Stats::default()));
        #[cfg(test)]
        let worker_stats = stats.clone();
        let thread = std::thread::Builder::new()
            .name("canvas-gpu".into())
            .spawn(move || {
                let result = Worker::new(parent, area).and_then(|mut worker| {
                    let outlines = worker.renderer.cursor_outlines();
                    if started.send(Ok((outlines, worker.child.clock()))).is_err() {
                        return Ok(());
                    }
                    #[cfg(test)]
                    let mut timing =
                        crate::timing::Timing::new(worker.renderer.device(), worker_stats);
                    let mut telemetry_enabled = false;
                    loop {
                        worker
                            .renderer
                            .device()
                            .poll(wgpu::PollType::Poll)
                            .map_err(error)?;
                        worker.child.dispatch()?;
                        if telemetry_enabled && let Ok(mut snapshot) = worker_telemetry.try_lock() {
                            *snapshot = worker.renderer.telemetry();
                        }
                        #[cfg(test)]
                        timing.presented(worker.child.take_presented());
                        while let Some(image) = worker.renderer.take_readback() {
                            reply
                                .send(Reply::Readback(image.map_err(error)?))
                                .map_err(error)?;
                        }
                        while let Some(image) = worker.renderer.take_thumbnail() {
                            reply
                                .send(Reply::Thumbnail(image.map_err(error)?))
                                .map_err(error)?;
                        }
                        if let Some(image) = worker.renderer.take_canvas_preview() {
                            reply
                                .send(Reply::CanvasPreview(image.map_err(error)))
                                .map_err(error)?;
                        }
                        if let Some(color) = worker.renderer.take_color_sample() {
                            reply
                                .send(Reply::ColorSample(color.map_err(error)))
                                .map_err(error)?;
                        }
                        while let Some(image) = worker.renderer.take_filter_previews() {
                            reply
                                .send(Reply::FilterPreviews(image.map_err(error)))
                                .map_err(error)?;
                        }
                        if let Some(result) = worker.renderer.take_effect_validation() {
                            reply.send(Reply::EffectValidation(result)).map_err(error)?;
                        }
                        let next = if cfg!(test)
                            || worker.renderer.effect_validation_pending()
                            || worker.renderer.filter_previews_pending()
                            || worker.pending_present
                            || worker.renderer.thumbnails_pending()
                            || worker.renderer.canvas_preview_pending()
                            || worker.renderer.color_sample_pending()
                            || worker.child.feedback_pending()
                        {
                            receiver.recv_timeout(Duration::from_millis(8))
                        } else {
                            receiver
                                .recv()
                                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                        };
                        let command = match next {
                            Ok(command) => command,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                if worker.pending_present
                                    && let Some(target) = worker.acquire()?
                                {
                                    worker.publish(
                                        target,
                                        #[cfg(test)]
                                        None,
                                    );
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };
                        match command {
                            Command::EffectValidation(request) => {
                                let request_id = request.request_id;
                                let result = worker.renderer.request_effect_validation(request);
                                if !matches!(result, Ok(true)) {
                                    reply
                                        .send(Reply::EffectValidation(
                                            layer_render::EffectValidationResult {
                                                request_id,
                                                result: Err(result
                                                    .err()
                                                    .map(error)
                                                    .unwrap_or_else(|| {
                                                        "Filter validator busy".into()
                                                    })),
                                            },
                                        ))
                                        .map_err(error)?;
                                }
                            }
                            Command::Telemetry(enabled) => {
                                telemetry_enabled = enabled;
                                worker.renderer.set_telemetry_enabled(enabled);
                            }
                            Command::FilterPreviews(request) => {
                                let result = worker
                                    .renderer
                                    .request_filter_previews(request)
                                    .map_err(error);
                                if !matches!(result, Ok(true)) {
                                    reply
                                        .send(Reply::FilterPreviews(Err(result
                                            .err()
                                            .unwrap_or_else(|| "Preview renderer busy".into()))))
                                        .map_err(error)?;
                                }
                            }
                            Command::Thumbnail(id, target) => worker
                                .renderer
                                .request_thumbnail(id, target)
                                .map_err(error)?,
                            Command::CanvasPreview(known) => {
                                let result = worker.renderer.request_canvas_preview(known);
                                if !matches!(result, Ok(true)) {
                                    reply
                                        .send(Reply::CanvasPreview(Err(result
                                            .err()
                                            .map(error)
                                            .unwrap_or_else(|| {
                                                "Canvas preview is not ready".into()
                                            }))))
                                        .map_err(error)?;
                                }
                            }
                            Command::ColorSample(request) => {
                                let result = worker.renderer.request_color_sample(request);
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
                                timing.begin(frame.queued_ns);
                                worker.draw(
                                    &frame,
                                    #[cfg(test)]
                                    &mut timing,
                                )?;
                                count.fetch_sub(1, Ordering::Release);
                            }
                            Command::Asset(id, [width, height, stride], format, bytes) => {
                                worker
                                    .renderer
                                    .prepare_asset(
                                        &id,
                                        HostImage {
                                            width,
                                            height,
                                            stride,
                                            format,
                                            bytes: &bytes,
                                        },
                                    )
                                    .map_err(error)?;
                            }
                            Command::Release(id) => worker.renderer.release_asset(&id),
                            Command::Readback(id) => {
                                worker.renderer.request_readback(id).map_err(error)?
                            }
                            Command::Capture(reply) => {
                                let _ = reply.send(worker.capture());
                            }
                            Command::Stop => break,
                        }
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    let _ = started.send(Err(error.clone()));
                    let _ = reply.send(Reply::Error(error));
                }
            })
            .map_err(error)?;
        // Initialization only; never wait this way during drawing.
        let (outlines, clock) = match start.recv().map_err(error).and_then(|result| result) {
            Ok(value) => value,
            Err(error) => {
                let _ = thread.join();
                return Err(error);
            }
        };
        Ok(Self {
            clock,
            telemetry,
            telemetry_enabled: false,
            commands,
            replies,
            in_flight,
            thread: Some(thread),
            outlines,
            readbacks: VecDeque::new(),
            thumbnails: VecDeque::new(),
            canvas_preview: None,
            canvas_preview_pending: false,
            color_sample: None,
            color_sample_pending: false,
            filter_previews: VecDeque::new(),
            filter_previews_pending: false,
            effect_validation_pending: false,
            effect_validation: None,
            geometry: None,
            surround: [0.033; 4],
            cursor: Vec::new(),
            #[cfg(test)]
            stats,
        })
    }
    fn send(&self, command: Command) -> Result<(), BackendError> {
        self.commands
            .send(command)
            .map_err(|_| BackendError("GPU worker stopped"))
    }
    pub(super) fn ready(&mut self) -> Result<bool, String> {
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::EffectValidation(result) => {
                    self.effect_validation_pending = false;
                    self.effect_validation = Some(result);
                }
                Reply::Thumbnail(image) => self.thumbnails.push_back(image),
                Reply::CanvasPreview(image) => {
                    self.canvas_preview = Some(image);
                    self.canvas_preview_pending = false;
                }
                Reply::ColorSample(color) => {
                    self.color_sample = Some(color);
                    self.color_sample_pending = false;
                }
                Reply::FilterPreviews(image) => {
                    self.filter_previews_pending = false;
                    self.filter_previews.push_back(image);
                }
                Reply::Error(error) => return Err(error),
                Reply::Readback(image) => self.readbacks.push_back(image),
            }
        }
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            return Err("GPU worker stopped".into());
        }
        Ok(self.in_flight.load(Ordering::Acquire) < 2)
    }
}
impl Drop for RenderWorker {
    fn drop(&mut self) {
        let _ = self.send(Command::Stop);
        // Lifecycle boundary only: Vulkan/child must die before GTK's parent.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl CanvasRenderer for RenderWorker {
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
    fn request_canvas_preview(&mut self, known_revision: Option<u64>) -> Result<bool, Self::Error> {
        if self.canvas_preview_pending {
            return Ok(false);
        }
        self.send(Command::CanvasPreview(known_revision))?;
        self.canvas_preview_pending = true;
        Ok(true)
    }
    fn take_canvas_preview(&mut self) -> Option<Result<layer_render::CanvasPreview, Self::Error>> {
        self.ready().ok()?;
        self.canvas_preview
            .take()
            .map(|result| result.map_err(|_| BackendError("Canvas preview unavailable")))
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
        self.send(Command::FilterPreviews(request))?;
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
    type Error = BackendError;
    fn tip_outline(&self, asset: &AssetId) -> Option<&TipOutline> {
        self.outlines.get(asset)
    }
    fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        if image.format == PixelFormat::R8Unorm {
            self.outlines.insert(
                asset.clone(),
                layer_render::mask_outline(image.width, image.height, image.stride, image.bytes),
            );
        }
        self.send(Command::Asset(
            asset.clone(),
            [image.width, image.height, image.stride],
            image.format,
            image.bytes.to_vec(),
        ))
    }
    fn release_asset(&mut self, asset: &AssetId) {
        self.outlines.remove(asset);
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
            // The renderer needs layer properties, not stroke-history lists.
            layers: packet
                .layers
                .iter()
                .map(|l| Layer {
                    id: l.id,
                    name: l.name.clone(),
                    kind: l.kind,
                    visible: l.visible,
                    opacity: l.opacity,
                    strokes: Vec::new(),
                    asset: l.asset.clone(),
                    source_revision: l.source_revision,
                    properties: l.properties.clone(),
                    operations: l.operations.clone(),
                    effect: l.effect.clone(),
                    mask: l.mask.as_ref().map(|m| {
                        let mut m = m.clone();
                        m.strokes = Default::default();
                        m
                    }),
                })
                .collect(),
            dabs: packet.dabs.to_vec(),
            batches: packet.dab_batches.to_vec(),
            reset: packet.reset_layers,
            composite: packet.composite_all,
            geometry,
            surround: self.surround,
            cursor: self.cursor.clone(),
            #[cfg(test)]
            queued_ns: gtk::glib::monotonic_time().max(0) as u64 * 1000,
        };
        self.in_flight.fetch_add(1, Ordering::Release);
        if let Err(e) = self.send(Command::Frame(Box::new(frame))) {
            self.in_flight.fetch_sub(1, Ordering::Release);
            return Err(e);
        }
        Ok(())
    }
    fn request_readback(&mut self, id: u64) -> Result<(), Self::Error> {
        self.send(Command::Readback(id))
    }
    fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.readbacks.pop_front().map(Ok)
    }
}

struct Worker {
    // Drop Vulkan's surface before the wl_surface (field declaration order).
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    child: Child,
    renderer: WgpuRasterizer,
    presenter: ViewportPresenter,
    config: wgpu::SurfaceConfiguration,
    last_view: Option<(ViewState, [f32; 4])>,
    area: gtk::glib::SendWeakRef<gtk::Picture>,
    cursor: Vec<CursorSegment>,
    cursor_scale: f32,
    pending_present: bool,
}
impl Worker {
    fn new(parent: Parent, area: gtk::glib::SendWeakRef<gtk::Picture>) -> Result<Self, String> {
        let child = Child::new(parent)?;
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
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(config.format);
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
        let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        #[cfg(test)]
        let features = features
            | (adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY
                    | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Wayland canvas GPU"),
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(error)?;
        eprintln!(
            "Wayland canvas GPU: {:?}, {:?}, {:?}",
            adapter.get_info(),
            config.present_mode,
            config.format
        );
        let presenter = ViewportPresenter::new(&device, config.format);
        let renderer = WgpuRasterizer::from_wgpu(adapter, device, queue).map_err(error)?;
        Ok(Self {
            surface,
            instance,
            child,
            renderer,
            presenter,
            config,
            last_view: None,
            area,
            cursor: Vec::new(),
            cursor_scale: 1.0,
            pending_present: false,
        })
    }
    fn draw(
        &mut self,
        frame: &Frame,
        #[cfg(test)] timing: &mut crate::timing::Timing,
    ) -> Result<(), String> {
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
        #[cfg(test)]
        if target.is_some() {
            timing.acquired(&self.renderer);
        }
        self.renderer.submit(frame.packet()).map_err(error)?;
        self.cursor.clone_from(&frame.cursor);
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
                #[cfg(test)]
                Some(timing),
            );
        }
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
        #[cfg(test)] timing: Option<&mut crate::timing::Timing>,
    ) {
        let (camera, surround) = self.last_view.expect("rendered document");
        let view = target.texture.create_view(&Default::default());
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        self.presenter
            .encode(&self.renderer, &mut encoder, &view, camera, surround);
        #[cfg(test)]
        if let Some(timing) = &timing {
            timing.encoded(&mut encoder);
        }
        self.renderer.queue().submit([encoder.finish()]);
        #[cfg(test)]
        self.child.feedback(timing.as_ref().map_or(0, |t| t.id()));
        #[cfg(not(test))]
        self.child.feedback(0);
        self.renderer.queue().present(target);
        #[cfg(test)]
        if let Some(timing) = timing {
            timing.end(&self.renderer);
        }
        self.pending_present = false;
    }
    fn capture(&mut self) -> Result<ReadbackImage, String> {
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
        let mut presenter = ViewportPresenter::new(self.renderer.device(), texture.format());
        presenter.set_cursor(self.renderer.device(), &self.cursor, self.cursor_scale);
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        presenter.encode(
            &self.renderer,
            &mut encoder,
            &texture.create_view(&Default::default()),
            view,
            surround,
        );
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
