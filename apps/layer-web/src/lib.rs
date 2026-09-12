#![cfg(target_arch = "wasm32")]

mod documents;
mod editor;
mod workspaces;

use layer_core::{AssetId, Point};
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render::{CanvasRenderer, FramePacket, HostImage, ReadbackImage, TipOutline};
use layer_render_wgpu::{GpuRasterError, StartupProgress, ViewportPresenter, WgpuRasterizer};
use layer_ui::{UiAction, UiSession, ui_catalog};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WebApp {
    workspaces: Option<layer_workspace::WorkspaceController<workspaces::BrowserStore>>,
    session: UiSession<WebRenderer>,
    canvas: web_sys::HtmlCanvasElement,
    sequence: u64,
    startup: StartupProgress,
    deferred_contacts: std::collections::BTreeSet<u64>,
    overviews: std::collections::BTreeMap<u32, editor::NavigatorSurface>,
}

#[derive(Deserialize)]
struct ExpansionQuery {
    viewport: [f32; 2],
    panel: layer_ui::Panel,
    heights: [f32; 2],
    open: bool,
    progress: f32,
    #[serde(default)]
    from: Option<layer_ui::PanelExpansion>,
}

#[derive(Deserialize)]
struct DropQuery {
    viewport: [f32; 2],
    position: [f32; 2],
    tabs: Vec<layer_ui::TabHit>,
    item: layer_ui::DockItem,
    #[serde(default)]
    expansion: Option<layer_ui::PanelExpansion>,
}

/// Created separately so an adapter request never holds a mutable UI borrow
/// across await. Settings and layout remain usable throughout GPU startup.
#[wasm_bindgen]
pub struct WebGpu {
    renderer: WgpuRasterizer,
    instance: wgpu::Instance,
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    blank_presented: bool,
}

/// An unattached GPU, not a fallback rasterizer. Only viewport bookkeeping is
/// permitted before attachment; pixel operations fail instead of losing work.
#[derive(Default)]
struct WebRenderer(Option<WebGpu>);

impl WebRenderer {
    fn renderer(&mut self) -> Result<&mut WgpuRasterizer, GpuRasterError> {
        self.0
            .as_mut()
            .map(|gpu| &mut gpu.renderer)
            .ok_or(GpuRasterError::AdapterUnavailable)
    }
}

impl CanvasRenderer for WebRenderer {
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        self.renderer()?.request_color_sample(request)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.0.as_mut()?.renderer.take_color_sample()
    }
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        self.renderer()?.set_transform_preview(preview)
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        self.renderer()?.request_region(request)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        self.0.as_mut()?.renderer.take_region()
    }
    fn set_selection_outline(
        &mut self,
        selection: Option<&layer_core::Selection>,
    ) -> Result<(), Self::Error> {
        self.renderer()?.set_selection_outline(selection)
    }
    fn request_effect_validation(
        &mut self,
        request: layer_render::EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        self.renderer()?.request_effect_validation(request)
    }
    fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
        self.0.as_mut()?.renderer.take_effect_validation()
    }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        if let Some(gpu) = &mut self.0 {
            gpu.renderer.set_telemetry_enabled(enabled);
        }
    }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        self.0
            .as_ref()
            .map(|g| g.renderer.telemetry())
            .unwrap_or_default()
    }
    type Error = GpuRasterError;
    fn request_filter_previews(
        &mut self,
        request: layer_render::FilterPreviewRequest,
    ) -> Result<bool, Self::Error> {
        self.renderer()?.request_filter_previews(request)
    }
    fn take_filter_previews(
        &mut self,
    ) -> Option<Result<layer_render::FilterPreviewImage, Self::Error>> {
        self.0.as_mut()?.renderer.take_filter_previews()
    }
    fn request_thumbnail(
        &mut self,
        id: u64,
        target: layer_core::LayerId,
    ) -> Result<(), Self::Error> {
        self.renderer()?.request_thumbnail(id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.0.as_mut()?.renderer.take_thumbnail()
    }
    fn tip_outline(&self, asset: &AssetId) -> Option<&TipOutline> {
        self.0.as_ref()?.renderer.tip_outline(asset)
    }
    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
        if let Some(gpu) = &mut self.0 {
            gpu.renderer.resize_surface(width, height)?;
        }
        Ok(())
    }
    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        self.renderer()?.prepare_asset(asset, image)
    }
    fn release_asset(&mut self, asset: &AssetId) {
        if let Some(gpu) = &mut self.0 {
            gpu.renderer.release_asset(asset);
        }
    }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        self.renderer()?.submit(packet)
    }
    fn request_readback(&mut self, id: u64) -> Result<(), Self::Error> {
        self.renderer()?.request_readback(id)
    }
    fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.0.as_mut()?.renderer.take_readback()
    }
}

fn js(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn serialize(value: &impl Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(
            &serde_wasm_bindgen::Serializer::new()
                .serialize_large_number_types_as_bigints(true)
                .serialize_maps_as_objects(true),
        )
        .map_err(js)
}
fn gpu_error(stage: &str, error: impl std::fmt::Display) -> JsValue {
    // Stable failure categories for platform-specific help, without parsing
    // browser/driver error strings or making a second adapter/device request.
    #[derive(Serialize)]
    struct Failure<'a> {
        stage: &'a str,
        message: String,
    }
    serialize(&Failure {
        stage,
        message: error.to_string(),
    })
    .unwrap_or_else(|error| error)
}
fn phase(value: u8) -> Result<PenPhase, JsValue> {
    match value {
        0 => Ok(PenPhase::Hover),
        1 => Ok(PenPhase::Down),
        2 => Ok(PenPhase::Move),
        3 => Ok(PenPhase::Up),
        4 => Ok(PenPhase::Cancel),
        _ => Err(js("Invalid pointer phase")),
    }
}

#[wasm_bindgen]
impl WebApp {
    pub fn filter_package_modules(&self, manifest: &str) -> Result<JsValue, JsValue> {
        serialize(
            &layer_core::EffectPackage::parse(manifest)
                .map_err(js)?
                .module_names()
                .map_err(js)?,
        )
    }
    pub fn load_filter_package(
        &mut self,
        manifest: &str,
        modules: JsValue,
        mode: JsValue,
    ) -> Result<JsValue, JsValue> {
        let modules: std::collections::BTreeMap<String, std::sync::Arc<str>> =
            serde_wasm_bindgen::from_value(modules).map_err(js)?;
        let mode = serde_wasm_bindgen::from_value(mode).map_err(js)?;
        let change = self
            .session
            .load_effect_package(
                manifest,
                |name| {
                    modules
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Missing filter module: {name}"))
                },
                mode,
            )
            .map_err(js)?;
        serialize(&change)
    }
    pub fn filter_preview_revision(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.filter_preview_revision())
    }
    pub fn request_filter_previews(
        &mut self,
        request: u64,
        filters: JsValue,
        width: u32,
        height: u32,
    ) -> Result<bool, JsValue> {
        if !self.gpu_ready() {
            return Ok(false);
        }
        let filters = serde_wasm_bindgen::from_value(filters).map_err(js)?;
        self.session
            .request_filter_previews(request, filters, [width, height])
            .map_err(js)
    }
    pub fn take_filter_previews(&mut self) -> Result<JsValue, JsValue> {
        let Some(result) = self.session.renderer_mut().take_filter_previews() else {
            return Ok(JsValue::NULL);
        };
        let result = result.map_err(js)?;
        let image = result.image;
        // One typed byte transfer, not a JavaScript number/object per channel.
        // Views of the returned atlas share this array in the browser.
        let header = serialize(&(image.request_id, image.width, image.height, result.filters))?;
        js_sys::Reflect::set(
            &header,
            &JsValue::from_str("bytes"),
            &js_sys::Uint8Array::from(image.bytes.as_slice()),
        )?;
        Ok(header)
    }
    pub fn action_tooltip(&self, label: &str, action: JsValue) -> Result<String, JsValue> {
        let action = serde_wasm_bindgen::from_value(action).map_err(js)?;
        let state = self.session.state();
        Ok(state
            .settings
            .action_tooltip(label, &action, state.platform))
    }
    pub fn layer_menu(&self, id: u64, mask: bool) -> Result<JsValue, JsValue> {
        serialize(&self.session.layer_menu(id, mask).map_err(js)?)
    }
    pub fn request_layer_thumbnail(&mut self, request: u64, target: u64) -> Result<bool, JsValue> {
        if !self.startup.complete || self.session.engine().has_pending_document_edits() {
            return Ok(false);
        }
        self.session
            .renderer_mut()
            .request_thumbnail(request, layer_core::LayerId(target))
            .map_err(js)?;
        Ok(true)
    }
    pub fn take_layer_thumbnail(&mut self) -> Result<JsValue, JsValue> {
        let Some(result) = self.session.renderer_mut().take_thumbnail() else {
            return Ok(JsValue::NULL);
        };
        let image = result.map_err(js)?;
        serialize(&(image.request_id, image.width, image.height, image.bytes))
    }
    pub fn import_layer_image(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        bytes: &[u8],
    ) -> Result<JsValue, JsValue> {
        self.session
            .import_layer_image(
                name,
                HostImage {
                    width,
                    height,
                    stride: width * 4,
                    format: layer_render::PixelFormat::Rgba8Srgb,
                    bytes,
                },
            )
            .map_err(js)?;
        serialize(&layer_ui::UiChange {
            regions: layer_ui::regions::DOCUMENT,
            canvas_wake: true,
            revision: self.session.state().revision,
        })
    }
    /// Display-only hover data, independent of the high-rate paint queue.
    pub fn cursor_input(&mut self, sample: &[f64]) {
        let event = (sample.len() == 7).then(|| PenEvent {
            device_id: 0,
            sequence: 0,
            timestamp_ns: (sample[6] * 1_000_000.0) as u64,
            view_revision: self.session.state().camera.revision,
            surface_position: Point {
                x: sample[0] as f32,
                y: sample[1] as f32,
            },
            pressure: sample[2] as f32,
            tilt_radians: [sample[3] as f32, sample[4] as f32],
            twist_radians: sample[5] as f32,
            distance: 0.0,
            phase: PenPhase::Hover,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        });
        self.session.cursor_input(event);
    }
    pub fn canvas_cursor(&mut self) -> Result<JsValue, JsValue> {
        if !self.gpu_ready() {
            return Ok(JsValue::NULL);
        }
        serialize(&self.session.canvas_cursor())
    }
    pub fn create(canvas: web_sys::HtmlCanvasElement) -> Result<WebApp, JsValue> {
        console_error_panic_hook::set_once();
        let mut session = UiSession::blank(
            WebRenderer::default(),
            [canvas.width().max(1), canvas.height().max(1)],
        )
        .map_err(js)?;
        session.set_platform(layer_ui::Platform::Web);
        session.set_document_replacement(true);
        session
            .dispatch(UiAction::RestoreWorkspace {
                workspace: layer_ui::WorkspaceState::for_platform(layer_ui::Platform::Web),
            })
            .map_err(js)?;
        Ok(Self {
            session,
            workspaces: None,
            canvas,
            sequence: 0,
            startup: StartupProgress::default(),
            deferred_contacts: Default::default(),
            overviews: Default::default(),
        })
    }
    pub fn gpu_ready(&self) -> bool {
        self.session.engine().backend().0.is_some()
    }
    pub fn brush_ready(&self) -> bool {
        self.startup.brush_ready
            && self
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .is_some_and(|gpu| {
                    !gpu.renderer.startup_needs_update(
                        self.session.engine().document(),
                        self.session.engine().brush(),
                        self.session.engine().transform_preview().is_some(),
                    )
                })
    }
    pub fn canvas_presented(&self) -> bool {
        self.session
            .engine()
            .backend()
            .0
            .as_ref()
            .is_some_and(|g| g.blank_presented)
    }
    pub fn shader_work_pending(&self) -> bool {
        self.session
            .engine()
            .backend()
            .0
            .as_ref()
            .is_some_and(|g| g.renderer.shader_work_pending())
    }
    pub fn wait_for_canvas(&self) -> Result<js_sys::Promise, JsValue> {
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("GPU unavailable"))?;
        let queue = gpu
            .renderer
            .queue()
            .as_webgpu()
            .ok_or_else(|| js("WebGPU queue unavailable"))?;
        Ok(queue.on_submitted_work_done().unchecked_into())
    }
    pub fn startup_progress(&mut self) -> Result<JsValue, JsValue> {
        if let Some(gpu) = &mut self.session.renderer_mut().0 {
            self.startup = gpu.renderer.poll_startup().map_err(js)?;
        }
        serialize(&[
            self.startup.canvas_ready,
            self.startup.brush_ready,
            self.startup.complete,
        ])
    }
    pub fn startup_catalog_submitted(&mut self) {
        if let Some(gpu) = &mut self.session.renderer_mut().0 {
            gpu.renderer.startup_catalog_submitted();
        }
    }
    /// Return an owned promise: UI/input may borrow the session while the
    /// browser validates this one job. No WebApp borrow survives an await.
    pub fn compile_startup_step(&mut self) -> Result<js_sys::Promise, JsValue> {
        self.prepare_startup()?;
        let gpu = self
            .session
            .renderer_mut()
            .0
            .as_ref()
            .ok_or_else(|| js("GPU unavailable"))?;
        let work = gpu.renderer.compile_startup_step();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            work.await.map_err(js)?;
            Ok(JsValue::UNDEFINED)
        }))
    }
    pub fn attach_gpu(&mut self, mut gpu: WebGpu) -> Result<(), JsValue> {
        if self.gpu_ready() {
            return Err(js("GPU is already attached"));
        }
        gpu.config.width = self.canvas.width().max(1);
        gpu.config.height = self.canvas.height().max(1);
        gpu.renderer
            .resize_surface(gpu.config.width, gpu.config.height)
            .map_err(js)?;
        gpu.surface
            .as_ref()
            .unwrap()
            .configure(gpu.renderer.device(), &gpu.config);
        self.session.renderer_mut().0 = Some(gpu);
        Ok(())
    }
}

#[wasm_bindgen]
impl WebGpu {
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<WebGpu, JsValue> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| gpu_error("renderer", error))?;
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        };
        // Chrome can return null while its hardware GPU process initializes.
        // Retry once; an unavailable GPU still becomes an explicit startup error.
        let adapter = match instance.request_adapter(&options).await {
            Ok(adapter) => adapter,
            Err(_) => instance
                .request_adapter(&options)
                .await
                .map_err(|error| gpu_error("adapter", error))?,
        };
        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| gpu_error("renderer", "WebGPU canvas format unavailable"))?;
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("web canvas GPU"),
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|error| gpu_error("device", error))?;
        device.on_uncaptured_error(std::sync::Arc::new(|error| {
            web_sys::console::error_1(&js(error))
        }));
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let presenter = ViewportPresenter::new(&device, config.format);
        let mut renderer = WgpuRasterizer::from_wgpu_staged(adapter, device, queue)
            .map_err(|error| gpu_error("renderer", error))?;
        renderer.wait_for_startup_catalog();
        if let Some(error) = validation.pop().await {
            return Err(gpu_error("renderer", error));
        }
        Ok(Self {
            renderer,
            instance,
            surface: Some(surface),
            config,
            presenter,
            blank_presented: false,
        })
    }
}

impl WebApp {
    fn prepare_startup(&mut self) -> Result<(), JsValue> {
        let engine = self.session.engine();
        let Some(gpu) = &engine.backend().0 else {
            return Ok(());
        };
        if !gpu.blank_presented {
            return Ok(());
        }
        let transform = engine.transform_preview().is_some();
        if gpu
            .renderer
            .startup_needs_update(engine.document(), engine.brush(), transform)
        {
            let (document, brush) = (engine.document().clone(), engine.brush().clone());
            self.session
                .renderer_mut()
                .0
                .as_mut()
                .unwrap()
                .renderer
                .prepare_startup(&document, &brush, transform)
                .map_err(js)?;
        }
        Ok(())
    }
}

#[wasm_bindgen]
impl WebApp {
    pub fn state(&self) -> Result<JsValue, JsValue> {
        serialize(self.session.state())
    }
    /// Retain UI models by model_revision; ordinary workspace motion only
    /// publishes absolute native geometry, tab presentation and drop feedback.
    pub fn workspace_update(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.workspace_update())
    }
    /// Shared live reflow packet; panel content stays at content_revision.
    pub fn layout_update(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        serialize(&self.session.workspace_layout_update([width, height]))
    }
    pub fn camera(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().camera)
    }
    pub fn catalog(&self) -> Result<JsValue, JsValue> {
        serialize(&ui_catalog())
    }
    pub fn number_input(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request: layer_ui::NumericRequest =
            serde_wasm_bindgen::from_value(request).map_err(js)?;
        serialize(&request.resolve().map_err(js)?)
    }
    pub fn preferences(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.preferences())
    }
    pub fn renderer_stats(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.renderer_stats())
    }
    pub fn panel_view(&self, panel: JsValue) -> Result<JsValue, JsValue> {
        let panel = serde_wasm_bindgen::from_value(panel).map_err(js)?;
        serialize(&self.session.panel_view(panel).map_err(js)?)
    }
    pub fn tool_picker(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.tool_picker())
    }
    pub fn toolbar_prompt(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.toolbar_prompt())
    }
    pub fn toolbar_manager(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.toolbar_manager())
    }
    pub fn workspace_menu(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.workspace_menu())
    }
    pub fn panel_handle_target(&self, item: JsValue) -> Result<JsValue, JsValue> {
        let item = serde_wasm_bindgen::from_value(item).map_err(js)?;
        serialize(
            &self
                .session
                .state()
                .workspace
                .layout
                .panel_handle_target(item),
        )
    }
    pub fn context_menu(&self, target: JsValue) -> Result<JsValue, JsValue> {
        let target = serde_wasm_bindgen::from_value(target).map_err(js)?;
        serialize(&self.session.context_menu(target).map_err(js)?)
    }
    pub fn expanded_panel(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: ExpansionQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let layout = &self.session.state().workspace.layout;
        let target = layout.expanded_panel(
            q.viewport,
            q.panel,
            q.heights,
            if q.open { 1.0 } else { 0.0 },
        );
        let from = q
            .from
            .or_else(|| layout.expanded_panel(q.viewport, q.panel, q.heights, 0.0));
        serialize(
            &target
                .zip(from)
                .filter(|_| q.progress.is_finite())
                .map(|(to, from)| to.interpolate_from(from, q.progress)),
        )
    }
    pub fn panel_tiles(
        &self,
        panel: JsValue,
        width: f32,
        height: f32,
        axis: JsValue,
        standalone: bool,
    ) -> Result<JsValue, JsValue> {
        let panel = serde_wasm_bindgen::from_value(panel).map_err(js)?;
        let axis = serde_wasm_bindgen::from_value(axis).map_err(js)?;
        let config = self
            .session
            .state()
            .workspace
            .layout
            .panel(panel)
            .map_err(js)?;
        serialize(&layer_ui::toolbar_tile_layout(
            width,
            height,
            axis,
            config.tiles(),
            standalone,
            config.tile_style,
        ))
    }
    pub fn dispatch(&mut self, action: JsValue) -> Result<JsValue, JsValue> {
        let action: UiAction = serde_wasm_bindgen::from_value(action).map_err(js)?;
        serialize(&self.session.dispatch(action).map_err(js)?)
    }
    pub fn input(&mut self, input: JsValue) -> Result<JsValue, JsValue> {
        let input = serde_wasm_bindgen::from_value(input).map_err(js)?;
        if !self.gpu_ready() && matches!(input, layer_ui::UiInput::Pointer { .. }) {
            return serialize(&layer_ui::InputReply::default());
        }
        if let layer_ui::UiInput::Pointer { id, phase, .. } = &input {
            use layer_ui::ContactPhase;
            if *phase == ContactPhase::Down {
                self.deferred_contacts.remove(id);
                if !self.brush_ready() {
                    self.deferred_contacts.insert(*id);
                }
            }
            if self.deferred_contacts.contains(id) {
                if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                    self.deferred_contacts.remove(id);
                }
                return serialize(&layer_ui::InputReply::default());
            }
        }
        if matches!(input, layer_ui::UiInput::Blur) {
            self.deferred_contacts.clear();
        }
        serialize(&self.session.input(input).map_err(js)?)
    }
    pub fn layout(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        serialize(&self.session.layout([width, height]))
    }
    pub fn dragging_attached_tab(&self) -> bool {
        self.session.dragging_attached_tab()
    }
    pub fn begin_tab_drag(&mut self, tabs: JsValue, clip: JsValue) -> Result<(), JsValue> {
        let tabs: Vec<layer_ui::TabHit> = serde_wasm_bindgen::from_value(tabs).map_err(js)?;
        let clip = serde_wasm_bindgen::from_value(clip).map_err(js)?;
        self.session.begin_tab_drag(&tabs, clip);
        Ok(())
    }
    pub fn tab_drag_preview(&self, position: JsValue) -> Result<JsValue, JsValue> {
        let position = serde_wasm_bindgen::from_value(position).map_err(js)?;
        serialize(&self.session.tab_drag_preview(position))
    }

    pub fn drop_hint(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: DropQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        match self
            .session
            .drop_hint(q.viewport, q.position, &q.tabs, q.item, q.expansion)
        {
            Some(hint) => serialize(&hint),
            None => Ok(JsValue::NULL),
        }
    }
    pub fn scroll(
        &mut self,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        dpi: f32,
        modifiers: u8,
    ) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .scroll(
                    [x, y],
                    [dx, dy],
                    dpi,
                    modifiers & 1 != 0,
                    modifiers & 2 != 0,
                )
                .map_err(js)?,
        )
    }
    pub fn viewport(
        &mut self,
        logical_width: f32,
        logical_height: f32,
        width: u32,
        height: u32,
    ) -> Result<JsValue, JsValue> {
        let change = self
            .session
            .set_viewport([logical_width, logical_height], [width, height])
            .map_err(js)?;
        if let Some(gpu) = &mut self.session.renderer_mut().0
            && [width, height] != [gpu.config.width, gpu.config.height]
        {
            gpu.config.width = width;
            gpu.config.height = height;
            gpu.surface
                .as_ref()
                .unwrap()
                .configure(gpu.renderer.device(), &gpu.config);
        }
        serialize(&change)
    }

    /// Packed history records: id, phase, x, y, pressure, tilt x/y, twist,
    /// timestamp milliseconds, flags, device kind (0 pen / 1 mouse / 2 eraser).
    /// Returns consumed record count if the bounded input queue fills.
    pub fn pen(&mut self, records: &[f64], view_revision: u64) -> Result<u32, JsValue> {
        if !self.gpu_ready() {
            return Err(js("Drawing is unavailable until the GPU is connected"));
        }
        if !records.len().is_multiple_of(11) {
            return Err(js("Invalid pen batch length"));
        }
        if !self.brush_ready() {
            return Ok((records.len() / 11) as u32);
        }
        for (index, item) in records.chunks_exact(11).enumerate() {
            if !item.iter().all(|n| n.is_finite()) {
                return Err(js("Invalid pen sample"));
            }
            let event = PenEvent {
                device_id: item[0] as u64,
                sequence: self.sequence + 1,
                timestamp_ns: (item[8].max(0.0) * 1_000_000.0) as u64,
                view_revision,
                surface_position: Point {
                    x: item[2] as f32,
                    y: item[3] as f32,
                },
                pressure: item[4] as f32,
                tilt_radians: [item[5] as f32, item[6] as f32],
                twist_radians: item[7] as f32,
                distance: 0.0,
                phase: phase(item[1] as u8)?,
                flags: SampleFlags(item[9] as u16),
                tool: match item[10] as u8 {
                    1 => ToolKind::Mouse,
                    2 => ToolKind::Eraser,
                    _ => ToolKind::Pen,
                },
            };
            if self.session.pen(event).is_err() {
                return Ok(index as u32);
            }
            self.sequence += 1;
        }
        Ok((records.len() / 11) as u32)
    }
    pub fn gesture(
        &mut self,
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
        scale: f32,
        rotation: f32,
    ) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .gesture([from_x, from_y], [to_x, to_y], scale, rotation)
                .map_err(js)?,
        )
    }
    pub fn frame(&mut self, now_ms: f64, presentation_ms: f64) -> Result<JsValue, JsValue> {
        if !self.gpu_ready() {
            return serialize(&layer_ui::UiChange::default());
        }
        let first = !self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .unwrap()
            .blank_presented;
        let mut change = layer_ui::UiChange::default();
        if first {
            // Present only paper without consuming the engine's pending replay.
            // Loaded strokes and effects retain their initial reset/history.
            let view = self.session.state().camera.view();
            let doc = self.session.engine().document();
            let extent = [doc.width, doc.height];
            let layers: Vec<_> = doc
                .layers
                .iter()
                .filter(|l| l.kind == layer_core::LayerKind::Background)
                .cloned()
                .collect();
            self.session
                .renderer_mut()
                .renderer()
                .map_err(js)?
                .submit(FramePacket {
                    time_seconds: 0.,
                    view,
                    document_extent: extent,
                    layers: &layers,
                    dabs: &[],
                    dab_batches: &[],
                    reset_layers: true,
                    composite_all: true,
                })
                .map_err(js)?;
        } else {
            self.prepare_startup()?;
            self.startup = self
                .session
                .renderer_mut()
                .0
                .as_mut()
                .unwrap()
                .renderer
                .poll_startup()
                .map_err(js)?;
            if self.startup.canvas_ready {
                change = self
                    .session
                    .frame(
                        (now_ms * 1_000_000.) as u64,
                        (presentation_ms * 1_000_000.) as u64,
                    )
                    .map_err(js)?;
            }
        }
        change.canvas_wake |= !self.startup.complete;
        let view = self.session.state().camera.view();
        let surround = self.session.state().palette.surround_linear;
        let mut overlay = Vec::new();
        self.session.append_layer_overlay(&mut overlay);
        let scale = self.canvas.width() as f32 / self.canvas.client_width().max(1) as f32;
        change.canvas_wake |= self.present_navigators()?;
        let gpu = self.session.renderer_mut().0.as_mut().unwrap();
        gpu.presenter
            .set_cursor(gpu.renderer.device(), &overlay, scale);
        let target = match gpu.surface.as_ref().unwrap().get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface = Some(
                    gpu.instance
                        .create_surface(wgpu::SurfaceTarget::Canvas(self.canvas.clone()))
                        .map_err(js)?,
                );
                gpu.surface
                    .as_ref()
                    .unwrap()
                    .configure(gpu.renderer.device(), &gpu.config);
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                gpu.surface
                    .as_ref()
                    .unwrap()
                    .configure(gpu.renderer.device(), &gpu.config);
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Occluded => return serialize(&change),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(js("WebGPU surface validation failed"));
            }
        };
        gpu.presenter.present(
            &gpu.renderer,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        );
        gpu.renderer.queue().present(target);
        gpu.blank_presented = true;
        serialize(&change)
    }
}
