#![cfg(target_arch = "wasm32")]

mod clipboard;
mod documents;
mod document_tabs;
mod document_storage;
mod session_restart;
mod image_import;
mod color_edit;
mod source_edit;
mod color_preferences;
mod proof;
mod hdr;
mod screen;
mod output;
mod editor;
mod header;
mod artwork_transfer;
mod raster_worker;
mod scopes;
mod workspaces;

use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render::CanvasRenderer;
use layer_render_wgpu::{AttachedRenderer, GpuRasterError, SdrSurfaceColor, StartupProgress, ViewportPresenter, WgpuRasterizer};
use layer_ui::{UiAction, UiSession, ui_catalog_localized};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WebApp {
    language_transition: layer_ui::LanguageTransition,
    language_preparation: Option<(layer_ui::LanguageRequest, layer_ui::LocalizerPreparation)>,
    preferred_tags: Vec<String>,
    published_language: Option<u64>,
    scopes: scopes::Cache,
    state_cache: std::collections::BTreeMap<&'static str, Vec<u8>>,
    preferences_cache: Option<(u64, JsValue)>,
    tone: hdr::ToneState,
    proof: layer_ui::proof_workflow::ProofView,
    workspaces: Option<layer_workspace::WorkspaceController<workspaces::BrowserStore>>,
    session: UiSession<AttachedRenderer>,
    documents: layer_ui::DocumentSessions<UiSession<AttachedRenderer>>,
    document_gpu: Option<document_tabs::DocumentGpu>,
    surface: Option<WebSurface>,
    canvas: web_sys::HtmlCanvasElement,
    viewport_scale: f32,
    cursor: layer_ui::CanvasCursor,
    sequence: u64,
    startup: StartupProgress,
    deferred_contacts: layer_engine::DeferredContacts,
    overviews: std::collections::BTreeMap<u32, editor::NavigatorSurface>,
    header_drag: Option<layer_ui::HeaderDrag>,
    glass: Vec<layer_render_wgpu::BackdropRegion>,
    screen_presented_ms: f64,
    thumbnails: layer_ui::ThumbnailRequests,
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
    surface: WebSurface,
}

struct WebSurface {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    sdr_format: wgpu::TextureFormat,
    color: SdrSurfaceColor,
    hdr_capable: bool,
    presenter: ViewportPresenter,
    presenter_color: layer_core::color::DocumentColor,
    blank_presented: bool,
    lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

fn js(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn numeric_error(reason: layer_ui::NumericError, localizer: &layer_ui::Localizer) -> JsValue {
    serialize(&serde_json::json!({"message":reason.message(localizer),"numeric_error":reason})).unwrap_or_else(std::convert::identity)
}
#[wasm_bindgen]
pub fn automatic_tab_names(available: f32, widths: &[f32]) -> Vec<u8> {
    let widths: Vec<[f32; 2]> = widths.chunks_exact(2).map(|w| [w[0], w[1]]).collect();
    layer_ui::TabStyle::automatic_names(available, &widths)
        .into_iter()
        .map(u8::from)
        .collect()
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
    pub fn poll_filter_previews(
        &mut self,
        filters: JsValue,
        cache: JsValue,
        width: u32,
        height: u32,
        now_ms: f64,
    ) -> Result<JsValue, JsValue> {
        let filters = if self.gpu_ready() {
            serde_wasm_bindgen::from_value(filters).map_err(js)?
        } else { Vec::new() };
        self.prepare_ui_previews()?;
        let cache = serde_wasm_bindgen::from_value(cache).map_err(js)?;
        let update = self.session.poll_filter_previews(
            (now_ms.max(0.) * 1_000_000.) as u64, filters, [width, height],
            cache,
        ).map_err(js)?;
        let result = serialize(&update.status)?;
        if let Some(atlas) = update.image {
            let image = atlas.image;
            let header = serialize(&(image.request_id, image.width, image.height, atlas.filters))?;
            js_sys::Reflect::set(&result, &JsValue::from_str("atlas"), &header)?;
            js_sys::Reflect::set(&result, &JsValue::from_str("bytes"),
                &js_sys::Uint8Array::from(image.bytes.as_slice()))?;
        }
        Ok(result)
    }
    pub fn action_tooltip(&self, label: &str, action: JsValue) -> Result<String, JsValue> {
        let action = serde_wasm_bindgen::from_value(action).map_err(js)?;
        let state = self.session.state();
        Ok(state
            .settings
            .action_tooltip_localized(label, &action, state.platform, self.session.localization()))
    }
    pub fn selection_menu(&self, kind: JsValue) -> Result<JsValue, JsValue> {
        serialize(&self.session.selection_menu(serde_wasm_bindgen::from_value(kind).map_err(js)?))
    }
    pub fn layer_menu(&self, id: u64, mask: bool) -> Result<JsValue, JsValue> {
        serialize(&self.session.layer_menu(id, mask).map_err(js)?)
    }
    pub fn object_menu(&self, id: u64) -> Result<JsValue, JsValue> {
        serialize(&self.session.object_menu(id).map_err(js)?)
    }
    pub fn layer_blend_menu(&self, id: u64) -> Result<JsValue, JsValue> {
        serialize(&self.session.layer_blend_menu(id).map_err(js)?)
    }
    pub fn layer_drop_preview(&self, id: u64, target: u64, fraction: f32, surface: JsValue) -> Result<JsValue, JsValue> {
        serialize(&self.session.layer_drop_preview(id, target, fraction, serde_wasm_bindgen::from_value(surface).map_err(js)?))
    }
    /// Places the canvas action bar from DOM control sizes; null for a stale context.
    pub fn canvas_bar_layout(&self, measure: JsValue) -> Result<JsValue, JsValue> {
        let measure: layer_ui::CanvasBarMeasure =
            serde_wasm_bindgen::from_value(measure).map_err(js)?;
        serialize(&self.session.canvas_bar_layout(&measure))
    }
    pub fn canvas_bar_menu(&self, context: JsValue, shown: usize) -> Result<JsValue, JsValue> {
        let context = serde_wasm_bindgen::from_value(context).map_err(js)?;
        serialize(&self.session.canvas_bar_menu(context, shown))
    }
    /// A bar dropdown or bar menu item's menu; undefined for a stale context or id.
    pub fn canvas_bar_choice_menu(&self, context: JsValue, id: &str) -> Result<JsValue, JsValue> {
        let context = serde_wasm_bindgen::from_value(context).map_err(js)?;
        serialize(&self.session.canvas_bar_choice_menu(context, id))
    }
    pub fn canvas_bar_hold(&self) -> u32 {
        self.session.canvas_bar_hold()
    }
    pub fn zoom_menu(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.zoom_menu())
    }
    pub fn palette_menu(&self, target: JsValue) -> Result<JsValue, JsValue> {
        let target = serde_wasm_bindgen::from_value(target).map_err(js)?;
        serialize(
            &self
                .session
                .state()
                .color_library
                .menu(target, self.session.localization())
                .map_err(js)?,
        )
    }
    pub fn palette_reorder_preview(
        &self,
        palette: u64,
        id: u64,
        slot: usize,
    ) -> Result<JsValue, JsValue> {
        let preview = self
            .session
            .state()
            .color_library
            .preview_reorder(palette, id, slot);
        serialize(&preview)
    }
    pub fn palette_action_error(&self, action: JsValue) -> Result<Option<String>, JsValue> {
        let action: layer_ui::ColorLibraryAction =
            serde_wasm_bindgen::from_value(action).map_err(js)?;
        Ok(self.session.state().color_library.check(action, self.session.localization()).err())
    }
    pub fn group_tab_style(&self, group: u32) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .state()
                .workspace
                .layout
                .group_tab_style(group)
                .map_err(js)?,
        )
    }
    pub fn request_layer_thumbnail(&mut self, request: u64, target: u64) -> Result<bool, JsValue> {
        let target = layer_render::ThumbnailTarget::from_wire_id(target).ok_or_else(|| js("Invalid thumbnail identity"))?;
        self.submit_thumbnail(Some((request, target)))
    }
    fn submit_thumbnail(&mut self, next: Option<(u64, layer_render::ThumbnailTarget)>) -> Result<bool, JsValue> {
        if !self.startup.canvas_ready || !self.session.background_readback_idle() {
            return Ok(false);
        }
        self.prepare_ui_previews()?;
        if !self.rasterizer()?.ui_readback_ready() {
            return Ok(false);
        }
        let retry = self.thumbnails.retry(&self.session);
        let Some((request, target)) = retry.or(next) else { return Ok(false) };
        let submitted = match self.rasterizer()?.prepare_thumbnail_batch(target) {
            Ok(true) => self.session.renderer_mut().request_thumbnail(request, target),
            Ok(false) => return Ok(false),
            Err(error) => Err(error),
        };
        match submitted {
            Ok(()) => {
                self.thumbnails.submitted(request, target, retry.is_some());
                Ok(retry.is_none())
            }
            Err(error) if retry.is_some() => { self.thumbnails.refused(&self.session, matches!(error, layer_render_wgpu::GpuRasterError::ThumbnailUnavailable(_))); Ok(false) }
            Err(layer_render_wgpu::GpuRasterError::ThumbnailUnavailable(_)) => Ok(false),
            Err(error) => Err(js(error)),
        }
    }
    pub fn canvas_work_pending(&self) -> bool {
        self.session.engine().wants_continuous_frames()
    }
    pub fn take_layer_thumbnail(&mut self) -> Result<JsValue, JsValue> {
        self.submit_thumbnail(None)?;
        let Some(result) = self.session.renderer_mut().take_thumbnail() else {
            return Ok(JsValue::NULL);
        };
        let image = result.map_err(js)?;
        self.thumbnails.completed(image.request_id);
        serialize(&(image.request_id, image.width, image.height, image.bytes))
    }
    /// Display-only hover data, independent of the high-rate paint queue.
    pub fn cursor_input(&mut self, sample: &[f64]) {
        let event = (sample.len() == 11).then(|| PenEvent {
            device_id: sample[7] as u64,
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
            phase: if sample[8] != 0. {
                PenPhase::Move
            } else {
                PenPhase::Hover
            },
            tool: match sample[9] as u8 {
                1 => ToolKind::Mouse,
                2 => ToolKind::Eraser,
                _ => ToolKind::Pen,
            },
            flags: SampleFlags(sample[10] as u16),
        });
        self.session.cursor_input(event);
    }
    pub fn create(canvas: web_sys::HtmlCanvasElement, saved: Option<String>, preferred_tags: JsValue) -> Result<WebApp, JsValue> {
        console_error_panic_hook::set_once();
        let tags: Vec<String> = serde_wasm_bindgen::from_value(preferred_tags).map_err(js)?;
        let localization = layer_ui::launch_localization(saved.as_deref().unwrap_or(""), &tags.iter().map(String::as_str).collect::<Vec<_>>());
        let mut session = UiSession::blank_localized(
            AttachedRenderer::default(),
            [canvas.width().max(1), canvas.height().max(1)],
            layer_ui::Platform::Web,
            localization,
        )
        .map_err(js)?;
        if let Some(saved) = saved {
            session.dispatch(UiAction::RestoreSavedSettings { saved }).map_err(js)?;
        }
        session.set_document_replacement(false);
        session
            .dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(layer_ui::WorkspaceState::for_platform(
                    layer_ui::Platform::Web,
                )),
            })
            .map_err(js)?;
        let documents = layer_ui::DocumentSessions::localized(session.localization());
        Ok(Self {
            language_transition: layer_ui::LanguageTransition::new(session.localization().clone()),
            language_preparation: None,
            preferred_tags: tags,
            published_language: None,
            proof: Default::default(),
            tone: Default::default(),
            session,
            documents,
            document_gpu: None,
            surface: None,
            workspaces: None,
            canvas,
            viewport_scale: 1.,
            cursor: Default::default(),
            sequence: 0,
            startup: StartupProgress::default(),
            scopes: Default::default(),
            state_cache: Default::default(),
            preferences_cache: None,
            deferred_contacts: Default::default(),
            overviews: Default::default(),
            header_drag: None,
            glass: Vec::new(),
            screen_presented_ms: 0.,
            thumbnails: Default::default(),
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
                    !gpu.startup_needs_update(
                        self.session.engine().document(),
                        self.session.engine().brush(),
                        self.session.engine().transform_preview().is_some(),
                    )
                })
    }
    pub fn canvas_presented(&self) -> bool {
        self.gpu_ready() && self.surface.as_ref().is_some_and(|s| s.blank_presented)
    }
    pub fn shader_work_pending(&self, allow_optional: bool) -> bool {
        self.session
            .engine()
            .backend()
            .0
            .as_ref()
            .is_some_and(|g| g.shader_work_pending(allow_optional && self.session.filter_previews_idle()))
    }
    pub fn shader_input(&mut self) { self.session.renderer_mut().shader_input(); }
    pub fn shader_wait_ms(&self) -> f64 {
        self.session.engine().backend().0.as_ref().map_or(0., |g| g.shader_wait_ms())
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
            .queue()
            .as_webgpu()
            .ok_or_else(|| js("WebGPU queue unavailable"))?;
        Ok(queue.on_submitted_work_done().unchecked_into())
    }
    pub fn startup_progress(&mut self) -> Result<JsValue, JsValue> {
        if let Some(gpu) = &mut self.session.renderer_mut().0 {
            self.startup = gpu.poll_startup().map_err(js)?;
        }
        serialize(&[
            self.startup.canvas_ready,
            self.startup.brush_ready,
            self.startup.complete,
        ])
    }
    /// Return an owned promise: UI/input may borrow the session while the
    /// browser validates this one job. No WebApp borrow survives an await.
    pub fn compile_startup_step(&mut self, allow_optional: bool) -> Result<js_sys::Promise, JsValue> {
        self.prepare_startup()?;
        let allow_optional = allow_optional && self.session.filter_previews_idle();
        let gpu = self
            .session
            .renderer_mut()
            .0
            .as_ref()
            .ok_or_else(|| js("GPU unavailable"))?;
        let work = gpu.compile_startup_step(allow_optional);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            work.await.map_err(js)?;
            Ok(JsValue::UNDEFINED)
        }))
    }
    pub fn gpu_failure(&self) -> Option<String> {
        self.gpu_owner()?.lock().ok()?.clone()
    }
    pub fn suspend_gpu(&mut self) -> Result<JsValue, JsValue> {
        if let Some(context) = self.document_gpu.take() { context.device.destroy(); }
        let change = self.session.suspend_renderer().map_err(js)?;
        if let Some(gpu) = self.session.renderer_mut().0.take() {
            // Dropping WebGPU handles leaves release to JavaScript GC. Retire
            // the failed device explicitly before recovery allocates another
            // complete photo cache. Document adoption shares a device and must
            // not use this path; suspension ends all work on this device.
            gpu.device().destroy();
        }
        self.surface = None;
        self.deferred_contacts.clear();
        // Retained DOM navigators keep their registration and geometry through
        // device replacement; only resources owned by the retired GPU expire.
        for slot in self.overviews.values_mut() { slot.gpu = None; }
        serialize(&change)
    }
    pub fn attach_gpu(&mut self, gpu: WebGpu) -> Result<(), JsValue> {
        if self.gpu_ready() {
            return Err(js("GPU is already attached"));
        }
        let WebGpu { mut renderer, mut surface } = gpu;
        surface.config.width = self.canvas.width().max(1);
        surface.config.height = self.canvas.height().max(1);
        renderer
            .resize_surface(surface.config.width, surface.config.height)
            .map_err(js)?;
        surface.surface.configure(renderer.device(), &surface.config);
        surface.presenter.set_target_retention(false);
        self.session
            .replace_renderer(AttachedRenderer(Some(Box::new(renderer))))
            .map_err(js)?;
        self.surface = Some(surface);
        self.session.set_hdr_display_available(false);
        self.startup = StartupProgress::default();
        Ok(())
    }
}

#[wasm_bindgen]
impl WebGpu {
    pub async fn create(canvas: web_sys::HtmlCanvasElement, color: JsValue) -> Result<WebGpu, JsValue> {
        let color = serde_wasm_bindgen::from_value(color).map_err(js)?;
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| gpu_error("renderer", error))?;
        let (adapter, device, queue) = request_device(&instance, Some(&surface)).await?;
        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| gpu_error("renderer", "WebGPU canvas format unavailable"))?;
        let lost = std::sync::Arc::new(std::sync::Mutex::new(None));
        let failure = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            *failure.lock().unwrap() = Some(format!("Canvas GPU stopped ({reason:?}): {message}"));
        });
        device.on_uncaptured_error(std::sync::Arc::new(|error| {
            web_sys::console::error_1(&js(error))
        }));
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, color)
            .map_err(|error| gpu_error("renderer", error))?;
        let hdr_capable = surface.get_capabilities(renderer.adapter())
            .color_spaces(wgpu::TextureFormat::Rgba16Float)
            .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB);
        let presenter = ViewportPresenter::for_renderer(&renderer, config.format);
        let presenter_color = renderer.document_color();
        raster_worker::install(&mut renderer);
        if let Some(error) = validation.pop().await {
            return Err(gpu_error("renderer", error));
        }
        Ok(Self {
            renderer,
            surface: WebSurface {
                instance,
                surface,
                sdr_format: config.format,
                color: SdrSurfaceColor::Srgb,
                hdr_capable,
                config,
                presenter,
                presenter_color,
                blank_presented: false,
                lost,
            },
        })
    }
}

impl WebApp {
    fn deliver(&mut self, event: PenEvent) -> Result<(), JsValue> {
        if let Err(event) = self.session.pen(event) {
            self.session.frame(event.timestamp_ns, event.timestamp_ns).map_err(js)?;
            self.session.pen(event).map_err(|_| js("Pen queue remained full"))?;
        }
        Ok(())
    }
    fn rasterizer(&mut self) -> Result<&mut WgpuRasterizer, JsValue> {
        self.session
            .renderer_mut()
            .0
            .as_deref_mut()
            .ok_or_else(|| js(GpuRasterError::AdapterUnavailable))
    }
    pub(crate) fn gpu_owner(&self) -> Option<std::sync::Arc<std::sync::Mutex<Option<String>>>> {
        self.session.engine().backend().0.as_ref()?;
        Some(self.surface.as_ref()?.lost.clone())
    }
    fn prepare_ui_previews(&mut self) -> Result<(), JsValue> {
        let rendition = self.session.engine().document().composition().color.depth.is_float()
            .then(|| self.session.effective_sdr_rendition());
        if let Some(gpu) = self.session.renderer_mut().0.as_mut() {
            gpu.set_ui_rendition(rendition).map_err(js)?;
        }
        Ok(())
    }
    fn prepare_startup(&mut self) -> Result<(), JsValue> {
        let engine = self.session.engine();
        let Some(gpu) = &engine.backend().0 else {
            return Ok(());
        };
        if !self.surface.as_ref().is_some_and(|s| s.blank_presented) {
            return Ok(());
        }
        let transform = engine.transform_preview().is_some();
        if gpu.startup_needs_update(engine.document(), engine.brush(), transform) {
            let (document, brush) = (engine.document().clone(), engine.brush().clone());
            self.rasterizer()?
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
    /// Search-only publications avoid serializing unchanged editor controls.
    pub fn command_search(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().command_search)
    }
    /// Incremental UI transport. Each field retains its original Serde type
    /// (including u64 BigInts); unchanged catalogs never cross the Wasm/JS
    /// boundary again. `state()` remains an independent full snapshot.
    pub fn state_update(&mut self) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let layer_ui::UiState {
            command_search,
            soft_proof,
            preview_sdr,
            hdr_display_available,
            screen,
            gamut_warning,
            revision,
            fullscreen,
            workspace,
            brush,
            colors,
            color_library,
            color_picker,
            tool_settings,
            tool_extra,
            tool_actions,
            tool_set,
            tool_panels,
            canvas_bar,
            layers,
            layer_tools,
            adjustments,
            filter_picker,
            filter_categories,
            filter_catalog_revision,
            filter_load,
            layer_properties,
            tabs,
            document_file,
            commands,
            settings,
            theme,
            palette,
            settings_open,
            preferences,
            customization,
            platform,
            requests,
            host_error,
            notice,
            camera,
            toolbar_context_generation,
            ..
        } = state;
        let result = js_sys::Object::new();
        macro_rules! field {
            ($name:ident) => {{
                let bytes = serde_json::to_vec($name).map_err(js)?;
                if self.state_cache.get(stringify!($name)) != Some(&bytes) {
                    js_sys::Reflect::set(&result, &JsValue::from_str(stringify!($name)), &serialize($name)?)?;
                    self.state_cache.insert(stringify!($name), bytes);
                }
            }};
        }
        field!(command_search);
        field!(soft_proof);
        field!(preview_sdr);
        field!(hdr_display_available);
        field!(screen);
        field!(gamut_warning);
        field!(revision);
        field!(fullscreen);
        field!(workspace);
        field!(brush);
        field!(colors);
        field!(color_library);
        field!(color_picker);
        field!(tool_settings);
        field!(tool_extra);
        field!(toolbar_context_generation);
        field!(tool_actions);
        field!(tool_set);
        field!(tool_panels);
        field!(canvas_bar);
        field!(layers);
        field!(layer_tools);
        field!(adjustments);
        field!(filter_picker);
        field!(filter_categories);
        field!(filter_catalog_revision);
        field!(filter_load);
        field!(layer_properties);
        field!(tabs);
        field!(document_file);
        field!(commands);
        field!(settings);
        field!(theme);
        field!(palette);
        field!(settings_open);
        field!(preferences);
        field!(customization);
        field!(platform);
        field!(requests);
        field!(host_error);
        field!(notice);
        field!(camera);
        scopes::publish(state, &mut self.scopes, &result)?;
        let generation = self.session.localization_generation();
        if self.published_language != Some(generation) {
            js_sys::Reflect::set(&result, &JsValue::from_str("localization"), &serialize(&serde_json::json!({"generation":generation,"tag":self.session.localization().language().tag()}))?)?;
            self.published_language = Some(generation);
        }
        Ok(result.into())
    }
    pub fn preferred_languages(&mut self, tags: JsValue) -> Result<(), JsValue> {
        self.preferred_tags = serde_wasm_bindgen::from_value(tags).map_err(js)?;
        self.request_language();
        Ok(())
    }
    pub fn language_pending(&self) -> bool {
        self.language_preparation.is_some() || self.language_transition.pending()
    }
    pub fn prepare_language(&mut self, input_busy: bool) -> Result<JsValue, JsValue> {
        if let Some((_, preparation)) = &mut self.language_preparation {
            let started = js_sys::Date::now();
            let ready = loop {
                if preparation.step(1) { break true; }
                if js_sys::Date::now() - started >= 2. { break false; }
            };
            if ready {
                let (request, preparation) = self.language_preparation.take().unwrap();
                if let Some(localization) = preparation.finish() { self.language_transition.prepared(request, localization); }
            }
        }
        if let Some(localization) = self.language_transition.publish(input_busy || self.session.localization_input_busy()) {
            if self.session.set_localization(localization.clone()) {
                self.documents.set_localization(&localization);
                if let Some(controller) = &mut self.workspaces { controller.set_localization(localization); }
                self.preferences_cache = None;
                return serialize(&layer_ui::UiChange { revision:self.session.state().revision, regions:layer_ui::regions::ALL, canvas_wake:false });
            }
        }
        Ok(JsValue::NULL)
    }
    pub fn document_color(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.engine().document().composition().color)
    }
    pub fn color_ui(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request = serde_wasm_bindgen::from_value(request).map_err(js)?;
        if let layer_ui::ColorUiRequest::PrintProof {settings} = request {
            serialize(&settings.recipe().map_err(proof::proof_rejection)?)
        } else { serialize(&layer_ui::color_ui_localized(request, self.session.localization()).map_err(js)?) }
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
    pub fn panel_measurements(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().workspace.layout.measurements)
    }
    pub fn camera(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().camera)
    }
    pub fn language_tag(&self) -> String {
        self.session.localization().language().tag().into()
    }
    pub fn bootstrap_view(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::bootstrap_view(self.session.localization()))
    }
    pub fn document_delivery_copy(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::DocumentDeliveryCopy::new(self.session.localization()))
    }
    pub fn native_caption(&self, request: JsValue) -> Result<String, JsValue> {
        let request: layer_ui::NativeCaption = serde_wasm_bindgen::from_value(request).map_err(js)?;
        Ok(request.message(self.session.localization()))
    }
    pub fn document_delivery_message(&self, request: JsValue) -> Result<String, JsValue> {
        let request: layer_ui::DocumentDeliveryMessage = serde_wasm_bindgen::from_value(request).map_err(js)?;
        Ok(request.message(self.session.localization()))
    }
    pub fn document_storage_retained(&self, detail: &str) -> String {
        layer_ui::document_storage_retained(self.session.localization(), detail)
    }
    pub fn document_recovery_unavailable(&self, detail: &str) -> String {
        layer_ui::document_recovery_unavailable(self.session.localization(), detail)
    }
    pub fn file_open_failure(&self, name: &str) -> String {
        layer_ui::file_open_failure(self.session.localization(), name)
    }
    pub fn catalog(&self) -> Result<JsValue, JsValue> {
        serialize(&ui_catalog_localized(self.session.localization()))
    }
    pub fn numeric_labels(&self, label: &str) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::NumericLabels::new(label, self.session.localization()))
    }
    pub fn number_input(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request: layer_ui::NumericRequest =
            serde_wasm_bindgen::from_value(request).map_err(js)?;
        serialize(&request.resolve().map_err(|reason| numeric_error(reason, self.session.localization()))?)
    }
    pub fn toolbar_ui(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request = serde_wasm_bindgen::from_value(request).map_err(js)?;
        let result = match request {
            layer_ui::ToolbarUiRequest::Number { request, compact, units } => serde_json::json!(
                layer_ui::toolbar_number(request, compact, units).map_err(|reason| numeric_error(reason, self.session.localization()))?),
            request => layer_ui::toolbar_ui(request, self.session.localization()).map_err(js)?,
        };
        // These JSON queries contain bounded UI numbers, not document IDs.
        // Preserve numbers when a returned numeric spec is sent back to Rust.
        js_sys::JSON::parse(&serde_json::to_string(&result).map_err(js)?)
    }
    pub fn toolbar_stamp(&self, context: JsValue) -> Result<JsValue, JsValue> {
        let context = serde_wasm_bindgen::from_value(context).map_err(js)?;
        serialize(&self.session.toolbar_stamp(context).map_err(js)?)
    }
    pub fn preferences(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.preferences())
    }
    /// Retained, read-only view for the DOM adapter. `preferences()` still
    /// returns an independent snapshot for callers that need ownership.
    pub fn preferences_cached(&mut self) -> Result<JsValue, JsValue> {
        if !self.session.state().settings_open { return Ok(JsValue::UNDEFINED); }
        let key = self.session.preferences_revision();
        if let Some((previous, value)) = &self.preferences_cache
            && *previous == key
        {
            return Ok(value.clone());
        }
        let value = serialize(&self.session.preferences().unwrap())?;
        self.preferences_cache = Some((key, value.clone()));
        Ok(value)
    }
    pub fn renderer_stats(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.renderer_stats())
    }
    pub fn backdrop_frames(&self) -> Vec<f64> {
        self.surface.as_ref().filter(|_| self.gpu_ready()).map_or([0; 2], |s| s.presenter.backdrop_frames()).map(|v| v as f64).to_vec()
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
        style: JsValue,
    ) -> Result<JsValue, JsValue> {
        let panel = serde_wasm_bindgen::from_value(panel).map_err(js)?;
        let axis = serde_wasm_bindgen::from_value(axis).map_err(js)?;
        let style = serde_wasm_bindgen::from_value(style).map_err(js)?;
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
            style,
        ))
    }
    pub fn dispatch(&mut self, action: JsValue) -> Result<JsValue, JsValue> {
        let action: UiAction = serde_wasm_bindgen::from_value(action).map_err(js)?;
        let change = self.session.dispatch(action).map_err(js)?;
        self.request_language();
        serialize(&change)
    }
    pub fn native_paste_input(&mut self) -> Result<JsValue, JsValue> {
        let change = self.session.native_paste_input().map_err(js)?;
        serialize(&change)
    }
    pub fn input(&mut self, input: JsValue) -> Result<JsValue, JsValue> {
        let input = serde_wasm_bindgen::from_value(input).map_err(js)?;
        if !self.gpu_ready() && matches!(input, layer_ui::UiInput::Pointer { .. }) {
            return serialize(&layer_ui::InputReply::default());
        }
        if let layer_ui::UiInput::Pointer { phase: layer_ui::ContactPhase::Down, .. } = &input {
            if let Some(control) = self.tone.pending.take() { control.cancel(); }
            if !self.brush_ready() {
                self.prepare_startup()?;
                self.startup = self.rasterizer()?.poll_startup().map_err(js)?;
            }
        }
        serialize(&self.session.input(input).map_err(js)?)
    }
    pub fn layout(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        serialize(&self.session.layout([width, height]))
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
        let scale = width as f32 / logical_width.max(1.);
        for region in &mut self.glass {
            let ratio = scale / self.viewport_scale;
            region.bounds = region.bounds.map(|v| v * ratio);
            region.radii = region.radii.map(|v| v * ratio);
        }
        self.viewport_scale = scale;
        if let (Some(gpu), Some(surface)) = (self.session.engine().backend().0.as_deref(), self.surface.as_mut())
            && [width, height] != [surface.config.width, surface.config.height]
        {
            surface.config.width = width;
            surface.config.height = height;
            surface.surface.configure(gpu.device(), &surface.config);
            surface.presenter.set_target_retention(false);
        }
        serialize(&change)
    }

    pub fn stroke_recording_status(&mut self) -> Result<JsValue, JsValue> {
        serialize(&self.session.stroke_recording().status())
    }
    pub fn start_stroke_recording(&mut self) -> Result<(), JsValue> {
        self.session.stroke_recording().start("web").map_err(js)
    }
    pub fn stop_stroke_recording(&mut self) {
        self.session
            .stroke_recording()
            .stop(layer_engine::recording::StopReason::Manual);
    }
    pub fn stroke_recording_data(&mut self) -> Result<Vec<u8>, JsValue> {
        self.session.stroke_recording().snapshot().map_err(js)
    }
    pub fn stroke_recording_saved(&mut self) {
        self.session.stroke_recording().saved();
    }

    /// Transient browser API capability; the preference remains persisted.
    pub fn prediction_availability(&mut self, available: bool) {
        self.session.set_platform_prediction_available(available);
    }

    /// Packed history records: id, phase, x, y, pressure, tilt x/y, twist,
    /// timestamp milliseconds, flags, device kind (0 pen / 1 mouse / 2 eraser).
    /// A contact that begins before painting is ready is held whole and
    /// delivered once it is.
    pub fn pen(&mut self, records: &[f64], view_revision: u64) -> Result<u32, JsValue> {
        if !self.gpu_ready() {
            return Err(js("Drawing is unavailable until the GPU is connected"));
        }
        if !records.len().is_multiple_of(11) {
            return Err(js("Invalid pen batch length"));
        }
        for item in records.chunks_exact(11) {
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
            if event.phase == PenPhase::Down {
                if let Some(control) = self.tone.pending.take() { control.cancel(); }
            }
            let admitted = self.deferred_contacts.admit(event, self.brush_ready(), web_time::Instant::now());
            self.session.set_input_held(self.deferred_contacts.holding());
            for event in admitted {
                self.deliver(event)?;
            }
            self.sequence += 1;
        }
        Ok((records.len() / 11) as u32)
    }
    pub fn begin_view_gesture(&mut self) { self.session.begin_view_gesture(); }
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
        self.session.set_input_held(self.deferred_contacts.holding());
        if !self.gpu_ready() {
            return serialize(&layer_ui::UiChange::default());
        }
        let first = !self.surface.as_ref().is_some_and(|s| s.blank_presented);
        let mut change = layer_ui::UiChange::default();
        if first {
            self.session.submit_backdrop_frame().map_err(js)?;
        } else {
            self.prepare_startup()?;
            self.startup = self.rasterizer()?.poll_startup().map_err(js)?;
            let released = self.deferred_contacts.release(self.brush_ready(), web_time::Instant::now());
            self.session.set_input_held(self.deferred_contacts.holding());
            for event in released {
                self.deliver(event)?;
            }
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
        // Compiler completions wake the browser explicitly. Once the brush is
        // usable, optional compilation does not require continuous redraws.
        change.canvas_wake |= !self.startup.brush_ready || !self.deferred_contacts.is_empty();
        let rendition = self.session.engine().document().composition().color.depth.is_float().then(|| self.session.effective_sdr_rendition());
        let hdr_output = self.hdr_output();
        let lut = self.proof.lut(&self.session);
        let (enabled, gamut) = (self.session.state().soft_proof, self.session.state().gamut_warning);
        let screen = &self.session.state().screen;
        let screen_check = layer_render_wgpu::ScreenCheck::for_view(
            &screen.assessment,
            layer_core::color::RgbSpace::Srgb,
            hdr_output,
            screen.show_clipped,
        );
        self.clear_incompatible_tone()?;
        if let (Some(gpu), Some(surface)) = (self.session.engine().backend().0.as_deref(), self.surface.as_mut()) {
            let color = if hdr_output { SdrSurfaceColor::ExtendedSrgb } else { SdrSurfaceColor::Srgb };
            if surface.color != color {
                surface.config.format = if hdr_output { wgpu::TextureFormat::Rgba16Float } else { surface.sdr_format };
                surface.config.color_space = color.surface_color_space();
                let mut presenter = ViewportPresenter::for_surface(gpu, surface.config.format, color).map_err(js)?;
                presenter.inherit_proof(gpu, &surface.presenter);
                surface.presenter = presenter;
                surface.presenter_color = gpu.document_color();
                surface.color = color;
                surface.surface.configure(gpu.device(), &surface.config);
                surface.presenter.set_target_retention(false);
            } else if surface.presenter_color != gpu.document_color() {
                surface.presenter = ViewportPresenter::for_surface(gpu, surface.config.format, color).map_err(js)?;
                surface.presenter_color = gpu.document_color();
            }
            surface.presenter.set_proof(gpu, lut, enabled, gamut).map_err(js)?;
            surface.presenter.set_screen_check(gpu, screen_check);
            if hdr_output {
                surface.presenter.set_compositor_hdr_view(gpu, rendition.unwrap()).map_err(js)?;
            } else {
                surface.presenter.set_hdr_view(gpu, rendition, 1.).map_err(js)?;
            }
        }
        let view = self.session.state().camera.view();
        let surround = self.session.state().palette.surround_linear;
        // Reuse the same retained GPU cursor as native hosts. Updating a
        // full-window SVG overlay made every pen frame repaint DOM artwork.
        self.session.update_canvas_cursor(&mut self.cursor);
        self.session.append_layer_overlay(&mut self.cursor.segments);
        let picker = self.session.color_picker_overlay();
        let glass = self.session.state().palette.glass;
        let backdrop_hold = self.session.hold_canvas_backdrop();
        let scale = self.viewport_scale;
        change.canvas_wake |= self.present_navigators()?;
        let (Some(gpu), Some(surface)) = (self.session.engine().backend().0.as_deref(), self.surface.as_mut()) else {
            return serialize(&change);
        };
        surface.presenter
            .set_cursor(gpu.device(), &self.cursor.segments, scale);
        surface.presenter.set_color_picker(gpu, picker);
        surface.presenter.set_backdrop(
            gpu,
            if glass.transparency.enabled() { &self.glass } else { &[] },
            layer_render_wgpu::BackdropBlurStyle { levels: glass.blur.levels, offset: glass.blur.offset },
            backdrop_hold,
        );
        if !surface.presenter.needs_present(gpu, view, surround) {
            return serialize(&change);
        }
        let target = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Lost => {
                surface.surface = surface
                    .instance
                    .create_surface(wgpu::SurfaceTarget::Canvas(self.canvas.clone()))
                    .map_err(js)?;
                surface.surface.configure(gpu.device(), &surface.config);
                surface.presenter.set_target_retention(false);
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                surface.surface.configure(gpu.device(), &surface.config);
                surface.presenter.set_target_retention(false);
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
        surface.presenter
            .present(
                gpu,
                &target.texture.create_view(&Default::default()),
                view,
                surround,
            )
            .map_err(js)?;
        gpu.queue().present(target);
        surface.blank_presented = true;
        self.screen_presented_ms = now_ms;
        serialize(&change)
    }
}

async fn request_device(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>)
    -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), JsValue> {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: surface,
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
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("web canvas GPU"),
                required_features: adapter.features() & (wgpu::Features::TIMESTAMP_QUERY
                    | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|error| gpu_error("device", error))?;
        Ok((adapter, device, queue))
}

impl WebApp {
    fn request_language(&mut self) {
        let tags: Vec<&str> = self.preferred_tags.iter().map(String::as_str).collect();
        if let Some(request) = self.language_transition.request(self.session.state().settings.language, &tags) {
            self.language_preparation = Some((request, layer_ui::LocalizerPreparation::new(request.language)));
        } else if !self.language_transition.pending() { self.language_preparation = None; }
    }
}
