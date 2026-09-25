use layer_host::NativeHost;
use layer_render::CanvasRenderer;
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use layer_ui::{CanvasCursor, PointerButton};
use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
};

thread_local! { static ERROR: RefCell<CString> = RefCell::new(CString::default()); }
fn fail(e: impl std::fmt::Display) {
    ERROR.with(|slot| *slot.borrow_mut() = CString::new(e.to_string().replace('\0', " ")).unwrap());
}
fn guard(host: *mut CapyHost, f: impl FnOnce(&mut CapyHost) -> Result<i32, String>) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        fail("Null canvas");
        return -1;
    };
    if host.poisoned {
        fail("Canvas stopped after a native panic");
        return -1;
    }
    match catch_unwind(AssertUnwindSafe(|| f(host))) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            host.native.error = Some(error.clone());
            fail(error);
            -1
        }
        Err(_) => {
            host.poisoned = true;
            fail("Native canvas panic; this host is stopped");
            -1
        }
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

use crate::events::{CapyPointer, validate_batch};
pub struct CapyHost {
    // An acquired image drops before its surface; the panel is detached on the
    // UI thread before destruction. The surface retains its native COM reference.
    poisoned: bool,
    gpu: std::sync::Arc<crate::device::DeviceState>,
    gpu_generation: u64,
    document_epoch: u64,
    window: usize,
    display: crate::display::Display,
    display_checked: Option<std::time::Instant>,
    test_display: Option<crate::display::Display>,
    presenter_key: Option<(layer_core::color::DocumentColor, layer_render_wgpu::SdrSurfaceColor)>,
    target: Option<wgpu::SurfaceTexture>,
    surface: wgpu::Surface<'static>,
    config: Option<wgpu::SurfaceConfiguration>,
    presenter: Option<ViewportPresenter>,
    native: NativeHost,
    instance: wgpu::Instance,
    cursor: CanvasCursor,
    chrome_facts: layer_ui::ChromeFacts,
    navigator: crate::navigator::Navigator,
    scale: f32,
    blank_presented: bool,
    prediction_frames: Option<[u64; 2]>, // opt-in, accumulated across document switches
    services: Option<crate::settings::SettingsService>,
    filters: Option<crate::filter_packages::FilterService>,
    documents: Option<crate::documents::DocumentService>,
    workspaces: Option<crate::workspace_service::WorkspaceService<layer_workspace::StoreWorker>>,
    blocked_contacts: std::collections::BTreeSet<u64>,
    live_contacts: std::collections::BTreeMap<u64, CapyPointer>,
}
fn pointer_button(sample: &CapyPointer) -> PointerButton {
    match sample.button {
        0 => PointerButton::Primary,
        1 => PointerButton::Pan,
        _ => PointerButton::Other,
    }
}
impl CapyHost {
    fn cancel_contacts(&mut self) -> Result<(), String> {
        for (id, mut sample) in std::mem::take(&mut self.live_contacts) {
            sample.phase = 4;
            sample.flags &= !1;
            self.blocked_contacts.insert(id);
            self.native.pointer_event(sample.event(), pointer_button(&sample))?;
        }
        Ok(())
    }
    fn track_contact(&mut self, sample: &CapyPointer) {
        if sample.phase == 0 {
            return;
        }
        if matches!(sample.phase, 3 | 4) {
            self.live_contacts.remove(&sample.id);
        } else {
            self.live_contacts.insert(sample.id, *sample);
        }
    }
    unsafe fn new(panel: *mut c_void, width: u32, height: u32, scale: f32) -> Result<Self, String> {
        if panel.is_null() || width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid Windows canvas surface".into());
        }
        let mut native = NativeHost::new(layer_ui::Platform::Windows)?;
        crate::workspace::initialize(&mut native)?;
        native.startup = Default::default();
        native.resize(width, height, scale)?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        if std::env::var_os("CAPY_LATENCY_TRACE").is_some()
            && std::env::var_os("CAPY_WINDOWS_NO_VSYNC_WAIT").is_some() {
            descriptor.backend_options.dx12.latency_waitable_object = wgpu::Dx12UseFrameLatencyWaitableObject::DontWait;
        }
        // GPU optimization is independent of Rust/C++ debugging. DXC's -Od
        // fragment storage-buffer code can be rejected by drivers; retain API
        // validation while using the same optimized shaders as Release.
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SwapChainPanel(panel))
        }
        .map_err(err)?;
        // GPU allocation and shader work happen after ownership moves to the
        // render worker. No swap chain is attached during CPU/UI initialization.
        Ok(Self {
            poisoned: false,
            gpu: Default::default(),
            gpu_generation: 0,
            document_epoch: native.session.state().document_file.epoch,
            window: 0,
            display: Default::default(),
            display_checked: None,
            test_display: None,
            presenter_key: None,
            target: None,
            surface,
            config: None,
            presenter: None,
            native,
            instance,
            cursor: CanvasCursor::default(),
            chrome_facts: layer_ui::ChromeFacts::default(),
            navigator: Default::default(),
            scale,
            blank_presented: false,
            prediction_frames: std::env::var_os("CAPY_LATENCY_TRACE").map(|_| [0; 2]),
            services: None,
            filters: None,
            documents: None,
            workspaces: None,
            blocked_contacts: Default::default(),
            live_contacts: Default::default(),
        })
    }

    fn device_is_lost(&self) -> bool {
        self.gpu.is_lost(
            self.native
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .map(|gpu| gpu.device()),
        )
    }

    fn accepts_workspace_input(&self) -> bool {
        self.services
            .as_ref()
            .is_none_or(|s| !s.close_status().requested)
            && self.workspaces
                .as_ref()
                .is_none_or(|s| s.accepts_input(crate::workspace_service::now_ms()))
    }
    fn sync_document(&mut self) {
        let epoch = self.native.session.state().document_file.epoch;
        if epoch != self.document_epoch {
            self.document_epoch = epoch;
            self.target = None;
            self.presenter = None;
            self.presenter_key = None;
            self.cursor = Default::default();
            // This window already presented. A prepared candidate owns its
            // composite; submitting a background-only startup frame would clear it.
        }
    }
    fn poll_services(&mut self) -> Result<(), String> {
        if self.display_checked.is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(500)) {
            self.display_checked = Some(std::time::Instant::now());
            let display = self.test_display.clone().unwrap_or_else(|| crate::display::probe(self.window));
            if display != self.display {
                self.display = display;
                self.native.dirty = true;
                self.native.invalidate_snapshot();
            }
            let headroom = self.config.as_ref().map_or(1., |c| self.display.available_headroom(c.format));
            if self.native.session.set_hdr_display_available(headroom > 1.) {
                self.native.dirty = true;
                self.native.invalidate_snapshot();
            }
        }
        if let Some(service) = self.documents.as_mut() {
            service.poll(&mut self.native)?;
            service.proof.poll(&mut self.native)?;
            if !self.gpu.is_lost(self.native.session.engine().backend().0.as_ref().map(|g| g.device())) {
                service.tone.poll(&mut self.native, self.gpu_generation)?;
            }
        }
        self.sync_document();
        if let Some(service) = self.services.as_mut()
            && (!self.native.session.state().document_file.close_ready || self.documents.as_ref().is_none_or(|d| d.window_close_ready(&self.native))) {
            service.poll(&mut self.native)?;
        }
        if let Some(service) = self.filters.as_mut() {
            service.poll(&mut self.native);
        }
        if let Some(service) = self.workspaces.as_mut() {
            if self.documents.as_ref().is_some_and(|d| d.window_close_ready(&self.native))
                && self.services.as_ref().is_none_or(|s| s.close_status().ready)
                && self.documents.as_ref().and_then(|d| d.recovery.as_ref()).is_none_or(|s| s.close_ready())
                && !service.status().close_requested
            {
                service.request_close(&mut self.native);
            }
            service.poll(
                &mut self.native,
                std::time::Instant::now(),
                crate::workspace_service::now_ms(),
            );
        }
        Ok(())
    }

    fn prepare_gpu(&mut self) -> Result<(), String> {
        if self.device_is_lost() && std::env::var_os("CAPY_TEST_GPU_UNAVAILABLE").is_some()
            && std::env::var_os("CAPY_SMOKE_TEST").is_some()
            && std::env::var_os("CAPY_SETTINGS_DIRECTORY").map(std::path::PathBuf::from).is_some_and(|p| p.is_absolute())
        {
            return Err("Test GPU remains unavailable".into());
        }
        if self.config.is_some() {
            return Ok(());
        }
        if self.device_is_lost() {
            // D3D12 devices are process/adapter singletons. Release the removed
            // renderer and presenter before requesting another device. Retained
            // CPU assets, history and input remain in the same UiSession.
            if let Some(service) = &mut self.documents { service.renderer_unavailable(&mut self.native)?; }
            self.presenter = None;
            self.presenter_key = None;
            let renderer = layer_host::Renderer(self.native.session.renderer_mut().0.take());
            if let Some(service) = self.documents.as_mut().and_then(|d| d.recovery.as_mut()) { service.retire_renderer(renderer); } else { drop(renderer); }
            self.native.startup = Default::default();
            // Completed document candidates may still own the removed device;
            // reject those while allowing an in-flight CPU Save to complete.
            self.poll_services()?;
        }
        let adapter =
            pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&self.surface),
                force_fallback_adapter: false,
                power_preference: wgpu::PowerPreference::HighPerformance,
                apply_limit_buckets: false,
            }))
            .map_err(err)?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err("Painting requires a hardware D3D12 GPU".into());
        }
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Capy Canvas Windows"),
            required_features: adapter.features() & (wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(err)?;
        let gpu_state = crate::device::DeviceState::observe(&device);
        let [width, height] = self.native.session.state().camera.viewport;
        let mut config = self
            .surface
            .get_default_config(&adapter, width, height)
            .ok_or("D3D12 surface unsupported")?;
        // Present completed ink without waiting for vblank when DXGI supports
        // tearing. Keep the one-frame queue and acquire-before-input ordering.
        // See docs/development/windows-pen-latency-20260920.md for measurements.
        config.present_mode = if self.surface.get_capabilities(&adapter).present_modes
            .contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else { wgpu::PresentMode::Fifo };
        if std::env::var_os("CAPY_LATENCY_TRACE").is_some() {
            let mode = match std::env::var("CAPY_WINDOWS_PRESENT_MODE").as_deref() {
                Ok("immediate") => wgpu::PresentMode::Immediate,
                Ok("mailbox") => wgpu::PresentMode::Mailbox,
                Ok("fifo") => wgpu::PresentMode::Fifo,
                _ => config.present_mode,
            };
            if self.surface.get_capabilities(&adapter).present_modes.contains(&mode) {
                config.present_mode = mode;
            }
        }
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        // Keep scRGB across monitor moves; only viewing changes, never artwork.
        if self.surface.get_capabilities(&adapter).format_capabilities.iter().any(|f|
            f.format == wgpu::TextureFormat::Rgba16Float
                && f.color_spaces.contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)) {
            config.format = wgpu::TextureFormat::Rgba16Float;
            config.color_space = wgpu::SurfaceColorSpace::ExtendedSrgbLinear;
            config.view_formats.clear();
        }
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue,
            self.native.session.engine().document().color).map_err(err)?;
        renderer.configure_ui_previews(layer_core::color::RgbSpace::Srgb).map_err(err)?;
        let encoding = self.display.encoding(config.format);
        let mut presenter = ViewportPresenter::for_surface(&renderer, config.format, encoding).map_err(err)?;
        self.presenter_key = Some((renderer.document_color(), encoding));
        // Prepare the optional overview pipeline during GPU startup, before input is live.
        presenter.prepare_overviews(&renderer);
        gpu_state.check()?;
        let revision = self.native.session.state().revision;
        let (retired, change) = self
            .native
            .session
            .replace_renderer(layer_host::Renderer(Some(renderer.into())))?;
        self.native.apply_change(revision, change);
        if let Some(service) = self.documents.as_mut().and_then(|d| d.recovery.as_mut()) { service.retire_renderer(retired); } else { drop(retired); }
        if self.device_is_lost() {
            self.native.error = None;
        }
        self.gpu = gpu_state;
        self.gpu_generation = self.gpu_generation.saturating_add(1);
        self.native.invalidate_snapshot();
        self.native.startup = Default::default();
        self.presenter = Some(presenter);
        self.config = Some(config);
        // Surface::configure calls SetSwapChain. Only capy_resize may do that,
        // on the UI thread while this worker is parked.
        Ok(())
    }

    fn frame(&mut self, now: u64, presentation: u64) -> Result<i32, String> {
        self.gpu.check()?;
        // A tab command can arrive after DXGI acquisition. Retire that image
        // before touching the newly selected editor, including failed activation.
        self.sync_document();
        if self.native.session.engine().backend().0.is_none() {
            self.target = None;
            self.native.dirty = false;
            return Ok(0);
        }
        let Some(target) = self.target.take() else {
            return Ok(1);
        };
        let prediction_before = self.prediction_frames.map(|_| self.native.session.engine().metrics());
        self.native
            .prepare_canvas_frame(now, presentation, self.blank_presented)?;
        if let (Some(total), Some(before)) = (&mut self.prediction_frames, prediction_before) {
            let after = self.native.session.engine().metrics();
            total[0] += after.platform_prediction_frames.saturating_sub(before.platform_prediction_frames);
            total[1] += after.engine_prediction_frames.saturating_sub(before.engine_prediction_frames);
        }
        self.gpu.check()?;
        self.native.dirty |= self.native.session.wants_continuous_frames();
        self.native
            .session
            .update_canvas_cursor(&mut self.cursor);
        self.native
            .session
            .append_layer_overlay(&mut self.cursor.segments);
        let picker = self.native.session.color_picker_overlay();
        let view = self.native.session.state().camera.view();
        let surround = self.native.session.state().palette.surround_linear;
        let proof = self.documents.as_mut().and_then(|s| s.proof.view.lut(&self.native.session));
        let gpu = self.native.session.engine().backend().0.as_ref().unwrap();
        let config = self.config.as_ref().ok_or("Missing surface configuration")?;
        let encoding = self.display.encoding(config.format);
        let key = (gpu.document_color(), encoding);
        if self.presenter_key != Some(key) {
            let mut presenter = ViewportPresenter::for_surface(gpu, config.format, encoding).map_err(err)?;
            presenter.prepare_overviews(gpu);
            self.presenter = Some(presenter);
            self.presenter_key = Some(key);
        }
        let presenter = self
            .presenter
            .as_mut()
            .ok_or("Viewport presenter is not prepared")?;
        let state = self.native.session.state();
        let headroom = if state.preview_sdr || state.soft_proof || state.gamut_warning || state.sdr_appearance_preview.is_some() { 1. }
            else { self.display.available_headroom(config.format) };
        presenter.set_hdr_view(gpu, gpu.document_color().depth.is_float().then(|| self.native.session.effective_sdr_rendition()), headroom).map_err(err)?;
        presenter.set_gpu_local_tone_guide(gpu, self.documents.as_ref().and_then(|s| s.tone.preview(&self.native, self.gpu_generation))).map_err(err)?;
        presenter.set_proof(gpu, proof, self.native.session.state().soft_proof, self.native.session.state().gamut_warning).map_err(err)?;
        presenter.set_cursor(gpu.device(), &self.cursor.segments, self.scale);
        presenter.set_color_picker(gpu, picker);
        presenter.set_overviews(gpu, self.navigator.placements(&self.native, self.scale));
        presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        ).map_err(err)?;
        gpu.queue().present(target);
        self.blank_presented = true;
        gpu.device().poll(wgpu::PollType::Poll).map_err(err)?;
        self.gpu.check()?;
        if let Some(service) = self.documents.as_mut() {
            service.after_frame(&mut self.native)?;
        }
        self.poll_services()?;
        Ok(i32::from(self.native.dirty))
    }
}
/// # Safety
/// Call on the panel's XAML thread. `panel` must point to a live
/// ISwapChainPanelNative interface. Keep the panel alive until the swap chain
/// is detached on that thread and the returned host is destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_create(
    panel: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) -> *mut CapyHost {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        CapyHost::new(panel, width, height, scale)
    })) {
        Ok(Ok(host)) => Box::into_raw(Box::new(host)),
        Ok(Err(e)) => {
            fail(e);
            std::ptr::null_mut()
        }
        Err(_) => {
            fail("Canvas initialization panic");
            std::ptr::null_mut()
        }
    }
}
/// Associate the HWND before transferring ownership to the render worker.
/// # Safety
/// Exclusive access to a live host; window remains valid for its lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_set_window(host: *mut CapyHost, window: *mut c_void) -> i32 {
    guard(host, |host| { host.window = window as usize; host.display_checked = None; Ok(0) })
}
/// Destroy after stopping callers and detaching the panel.
/// # Safety
/// The pointer must be uniquely owned and freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_destroy(host: *mut CapyHost) {
    if !host.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let host = unsafe { Box::from_raw(host) };
            // One opt-in write after input/rendering stop, never in the pen path.
            if let Some([platform, engine]) = host.prediction_frames {
                let report = serde_json::json!({
                    "process_id": std::process::id(),
                    "window": host.window,
                    "platform_prediction_frames": platform,
                    "engine_prediction_frames": engine,
                });
                let path = format!("prediction-{}-{}.json", std::process::id(), host.window);
                let _ = std::fs::write(path, report.to_string());
            }
        }));
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_error() -> *const c_char {
    ERROR.with(|v| v.borrow().as_ptr())
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `wake` and its `context` must stay valid until `capy_finish_services` returns.
/// The callback must tolerate concurrent worker calls and must not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_start_services(
    host: *mut CapyHost,
    context: *mut c_void,
    wake: Option<extern "C" fn(*mut c_void)>,
) -> i32 {
    guard(host, |host| {
        if host.documents.is_none() {
            let context = context as usize;
            host.documents = Some(crate::documents::DocumentService::open(move || {
                if let Some(wake) = wake {
                    wake(context as *mut c_void);
                }
            })?);
        }
        if host.services.is_none() {
            let context = context as usize;
            host.services = Some(crate::settings::SettingsService::open(
                &mut host.native,
                move || {
                    if let Some(wake) = wake {
                        wake(context as *mut c_void);
                    }
                },
            ));
        }
        host.documents.as_mut().unwrap().start_recovery()?;
        if host.filters.is_none() {
            let context = context as usize;
            let mut service = crate::filter_packages::FilterService::new(move || {
                if let Some(wake) = wake {
                    wake(context as *mut c_void);
                }
            });
            service.startup(&mut host.native);
            host.filters = Some(service);
        }
        if host.workspaces.is_none() {
            let context = context as usize;
            let directory = crate::settings::data_directory()?;
            let worker = layer_workspace::StoreWorker::shared(&directory).map_err(err)?;
            let mut service =
                crate::workspace_service::WorkspaceService::new(worker, directory, move || {
                    if let Some(wake) = wake {
                        wake(context as *mut c_void);
                    }
                });
            service.start(crate::workspace_service::now_ms());
            host.workspaces = Some(service);
            host.poll_services()?;
        }
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_poll_services(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.poll_services()?;
        Ok(i32::from(host.native.dirty))
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Keep service callback code and context alive until this function returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_finish_services(host: *mut CapyHost) -> i32 {
    let Some(host) = (unsafe { host.as_mut() }) else {
        fail("Null canvas");
        return -1;
    };
    let filters = catch_unwind(AssertUnwindSafe(|| {
        if let Some(mut service) = host.filters.take() {
            service.stop();
        }
    }))
    .map_err(|_| "Filter transport shutdown failed".to_string());
    let workspaces = catch_unwind(AssertUnwindSafe(|| {
        if let Some(mut service) = host.workspaces.take() {
            service.stop();
        }
    }))
    .map_err(|_| "Workspace shutdown failed".to_string());
    // Cleanup must join the storage callback even when another host operation
    // poisoned the renderer. Do not dispatch more actions into a poisoned host.
    let documents = catch_unwind(AssertUnwindSafe(|| {
        host.documents
            .as_mut()
            .map_or(Ok(()), |service| service.stop_worker())
    }))
    .unwrap_or_else(|_| Err("Document shutdown failed".into()));
    match catch_unwind(AssertUnwindSafe(|| {
        let settings = if let Some(service) = host.services.as_mut() {
            if host.poisoned {
                service.stop_worker()
            } else {
                service.finish(&mut host.native)
            }
        } else {
            Ok(())
        };
        documents.and(settings).and(workspaces).and(filters)
    })) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            fail(error);
            -1
        }
        Err(_) => {
            fail("Preferences shutdown failed");
            -1
        }
    }
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Call from the canvas owner, before acquiring surface images.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_prepare_gpu(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.prepare_gpu()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Call on the panel's XAML thread with the canvas owner paused and no acquired
/// image; surface configuration attaches the swap chain to that panel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_resize(
    host: *mut CapyHost,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    guard(host, |host| {
        if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid viewport".into());
        }
        host.target = None;
        host.scale = scale;
        host.native.resize(width, height, scale)?;
        if let Some(config) = &mut host.config {
            config.width = width;
            config.height = height;
            host.surface.configure(
                host.native
                    .session
                    .engine()
                    .backend()
                    .0
                    .as_ref().map(|g| g.device())
                    .or_else(|| host.documents.as_ref().and_then(|d| d.tab_device()))
                    .ok_or("GPU is not prepared")?,
                config,
            );
            set_composition_scale(&host.surface, scale)?;
        }
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// For nonzero `count`, `records` must point to that many initialized, aligned
/// CapyPointer values, readable and unchanged throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_pointer(
    host: *mut CapyHost,
    records: *const CapyPointer,
    count: usize,
) -> i32 {
    guard(host, |host| {
        if count > 32768 || (count > 0 && records.is_null()) {
            return Err("Invalid pointer batch".into());
        }
        if count > 0 {
            let batch = unsafe { std::slice::from_raw_parts(records, count) };
            validate_batch(batch).map_err(err)?;
            host.poll_services()?;
            let blocked = !host.accepts_workspace_input();
            let mut delivered = None;
            for sample in batch {
                let terminal = matches!(sample.phase, 3 | 4);
                if !blocked && sample.phase == 1 {
                    host.blocked_contacts.remove(&sample.id);
                }
                if blocked && sample.phase != 0 {
                    host.blocked_contacts.insert(sample.id);
                }
                if blocked || host.blocked_contacts.contains(&sample.id) {
                    if terminal {
                        host.blocked_contacts.remove(&sample.id);
                    }
                    if !(terminal && host.live_contacts.contains_key(&sample.id)) {
                        continue;
                    }
                }
                host.native.pointer_event(sample.event(), pointer_button(sample))?;
                if sample.flags & 1 == 0 {
                    delivered = Some(*sample);
                }
            }
            if let Some(sample) = delivered {
                host.track_contact(&sample);
            }
        }
        Ok(0)
    })
}
/// Workspace recovery responses are serialized with document and canvas work.
/// # Safety
/// Exclusive access to a live host; json is a readable NUL-terminated buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_action(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        use crate::workspace_service::{WorkspaceAction, now_ms};
        let action = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        if matches!(
            action,
            WorkspaceAction::PreferencesRetry
                | WorkspaceAction::PreferencesKeepOpen
                | WorkspaceAction::PreferencesDiscardClose
        ) {
            let service = host
                .services
                .as_mut()
                .ok_or("Preferences service is unavailable")?;
            match action {
                WorkspaceAction::PreferencesRetry => service.retry_close(&mut host.native)?,
                WorkspaceAction::PreferencesKeepOpen => service.keep_open(&mut host.native),
                WorkspaceAction::PreferencesDiscardClose => service.discard_close(&mut host.native),
                _ => unreachable!(),
            }
            host.poll_services()?;
            return Ok(0);
        }
        if host.native.session.rendering_suspended() {
            fail("Workspace changes are unavailable. Save the drawing and reopen it.");
            return Ok(1);
        }
        let service = host
            .workspaces
            .as_mut()
            .ok_or("Workspace service is unavailable")?;
        let result = match action {
            WorkspaceAction::PreferencesRetry
            | WorkspaceAction::PreferencesKeepOpen
            | WorkspaceAction::PreferencesDiscardClose => unreachable!(),
            WorkspaceAction::Manager { dialog, command } => {
                service.manager_input(&mut host.native, dialog, command)
            }
            WorkspaceAction::RefreshSwitcher => {
                service.refresh_switcher();
                Ok(())
            }
            WorkspaceAction::Retry => {
                service.retry(&mut host.native, now_ms());
                Ok(())
            }
            WorkspaceAction::KeepOpen => {
                service.keep_open(&mut host.native);
                Ok(())
            }
            WorkspaceAction::DiscardClose => service.discard_close(&mut host.native),
            WorkspaceAction::SaveAsNew { name } => {
                service.save_as_new(&mut host.native, name, now_ms())
            }
            WorkspaceAction::ExportBackup { path } => service.export_backup(&mut host.native, path),
            WorkspaceAction::Failure { error } => Err(layer_workspace::StoreError::new(
                layer_workspace::ErrorKind::Unavailable,
                error,
            )),
            WorkspaceAction::BackupDatabase { path } => {
                service.backup_database(&mut host.native, path)
            }
        };
        if let Err(error) = result {
            service.report_error(&mut host.native, error);
        }
        host.poll_services()?;
        Ok(0)
    })
}

unsafe fn read_json<'a>(json: *const c_char) -> Result<&'a str, String> {
    if json.is_null() {
        return Err("Null JSON".into());
    }
    unsafe { CStr::from_ptr(json) }.to_str().map_err(err)
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_load_filter_directory(
    host: *mut CapyHost,
    json: *const c_char,
) -> i32 {
    guard(host, |host| {
        let text = unsafe { read_json(json) }?;
        if text.len() > 8192 {
            return Err("Filter directory request is too large.".into());
        }
        let request = serde_json::from_str(text).map_err(err)?;
        let service = host
            .filters
            .as_mut()
            .ok_or("Filter file transport is not started.")?;
        if let Err(error) = service.load(&mut host.native, request) {
            fail(error);
            return Ok(1);
        }
        service.poll(&mut host.native);
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_action(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        let action: crate::actions::Action =
            serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        host.poll_services()?;
        if !host.accepts_workspace_input() && !action.allowed_while_workspace_blocked() {
            return Ok(0); // A queued widget from before a workspace transition.
        }
        if let Err(error) = action.dispatch(&mut host.native) {
            fail(error);
            return Ok(1); // A valid action can be unavailable in the current state.
        }
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_action(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        let action = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        let result = host.documents.as_mut().ok_or("Document service is unavailable")?.dispatch(&mut host.native, action);
        if let Err(error) = result {
            fail(error);
            return Ok(1);
        }
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_native_prediction(host: *mut CapyHost, available: bool) -> i32 {
    guard(host, |host| {
        host.native.session.set_platform_prediction_available(available);
        host.native.invalidate_snapshot();
        Ok(0)
    })
}
/// # Safety
/// Exclusive access to a live host; json is a readable NUL-terminated buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_input(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        let input = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        host.poll_services()?;
        if !host.accepts_workspace_input()
            && !matches!(
                &input,
                layer_ui::UiInput::Blur | layer_ui::UiInput::Chrome { .. }
            )
        {
            return Ok(0);
        }
        if let layer_ui::UiInput::Chrome { facts, .. } = &input {
            host.chrome_facts = *facts;
            // This hit belongs only to that UI contact, never later canvas input.
            host.chrome_facts.contact_tab = None;
        }
        if matches!(input, layer_ui::UiInput::Blur) {
            host.cancel_contacts()?;
        }
        host.native.input(input)?;
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_scroll(
    host: *mut CapyHost,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    density: f32,
    zoom: bool,
    horizontal: bool,
) -> i32 {
    guard(host, |host| {
        host.poll_services()?;
        if !host.accepts_workspace_input() {
            return Ok(0);
        }
        host.native
            .scroll([x, y], [dx, dy], density, zoom, horizontal)?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_chrome(
    host: *mut CapyHost,
    kind: u32,
    x: f32,
    y: f32,
    canvas: bool,
    popup_open: bool,
    touch: bool,
) -> i32 {
    guard(host, |host| {
        let position = [x / host.scale, y / host.scale];
        let event = match kind {
            0 => layer_ui::ChromeEvent::Refresh,
            1 => layer_ui::ChromeEvent::Motion { position },
            2 => layer_ui::ChromeEvent::Contact { position, canvas },
            3 => layer_ui::ChromeEvent::Leave { touch },
            _ => return Err("Invalid chrome event".into()),
        };
        let reply = host.native.input(layer_ui::UiInput::Chrome {
            event,
            viewport: host.native.logical,
            facts: layer_ui::ChromeFacts {
                popup_open: popup_open
                    || host.native.session.state().customization.control.is_some(),
                ..host.chrome_facts
            },
        })?;
        Ok(i32::from(reply.handled) | (i32::from(reply.dismiss_popups) << 1))
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_acquire(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        if host.device_is_lost() {
            return Err("The GPU device was lost".into());
        }
        host.gpu.check()?;
        if host.native.session.engine().backend().0.is_none() { return Ok(3); }
        if host.config.is_none() {
            return Ok(2);
        }
        if host.target.is_some() {
            return Ok(1);
        }
        host.target = match host.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(0);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return Ok(2);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("D3D12 surface validation failed".into());
            }
        };
        Ok(1)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_frame(host: *mut CapyHost, now: u64, presentation: u64) -> i32 {
    guard(host, |host| host.frame(now, presentation))
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_snapshot(host: *mut CapyHost) -> *mut c_char {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        let metadata = crate::snapshots::WindowsMetadata {
            windows_gpu_generation: host.gpu_generation,
            windows_display: serde_json::json!({"output": host.display, "format": host.config.as_ref().map(|c| format!("{:?}", c.format)), "headroom": host.config.as_ref().map_or(1., |c| host.display.available_headroom(c.format)), "analysis": host.documents.as_ref().map(|s| s.tone.status())}),
            windows_rendering_suspended: host.native.session.rendering_suspended(),
            windows_importing: host
                .documents
                .as_ref()
                .is_some_and(|service| service.importing()),
            windows_image_import: host
                .documents
                .as_ref()
                .and_then(|service| service.import_request()),
            windows_recovery: host.documents.as_ref().and_then(|d| d.recovery.as_ref()).map(|service| service.status()),
            windows_tabs: host.documents.as_ref().map(|d| d.tabs_view(&host.native)),
            windows_palettes: host.documents.as_ref().map(|d| d.palettes.status()),
            windows_document: host.documents.as_ref().and_then(|service| service.status()),
            windows_proof_form: layer_ui::proof_workflow::proof_form(&host.native.session),
            windows_proof: host.documents.as_mut().map(|s| s.proof.view.observe(&host.native.session)),
            windows_workspace: host.workspaces.as_ref().map(|s| s.status().clone()),
            windows_settings_close: host.services.as_ref().map(|s| s.close_status().clone()),
            windows_filter_load: host.filters.as_ref().map(|s| s.status().clone()),
            windows_workspace_manager: host
                .workspaces
                .as_ref()
                .and_then(|s| s.manager_view().cloned()),
            windows_isolated_settings: std::env::var_os("CAPY_SETTINGS_DIRECTORY")
                .map(std::path::PathBuf::from)
                .is_some_and(|path| path.is_absolute()),
        };
        if let Some(snapshot) = crate::snapshots::take(&mut host.native, &metadata).map_err(err)? {
            result = CString::new(snapshot).map_err(err)?.into_raw();
        }
        Ok(0)
    });
    result
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_query(host: *mut CapyHost, json: *const c_char) -> *mut c_char {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        let query = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        result = CString::new(host.native.query(query)?.to_string())
            .map_err(err)?
            .into_raw();
        Ok(0)
    });
    result
}
/// Diagnostic identity for app-only ETW correlation. Never treat submission
/// metadata or this steady-content probe as physical input/display timing.
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_surface_info(host: *mut CapyHost) -> *mut c_char {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        use windows::core::Interface;
        let native = unsafe { host.surface.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or("Missing D3D12 surface")?;
        let swapchain = native.swap_chain().ok_or("Missing DXGI swap chain")?;
        let base = swapchain
            .cast::<windows::Win32::Graphics::Dxgi::IDXGISwapChain>()
            .map_err(err)?;
        let config = host
            .config
            .as_ref()
            .ok_or("Missing surface configuration")?;
        let info = serde_json::json!({
            "scope": "steady_canvas_presentation_probe",
            "swap_chain_addresses": [
                format!("{:#x}", base.as_raw() as usize),
                format!("{:#x}", swapchain.as_raw() as usize)
            ],
            "viewport": [config.width, config.height],
            "density": host.scale,
            "present_mode": format!("{:?}", config.present_mode),
            "format": format!("{:?}", config.format),
            "maximum_frame_latency": config.desired_maximum_frame_latency,
            "no_vsync_wait": std::env::var_os("CAPY_LATENCY_TRACE").is_some() && std::env::var_os("CAPY_WINDOWS_NO_VSYNC_WAIT").is_some()
        });
        result = CString::new(info.to_string()).map_err(err)?.into_raw();
        Ok(0)
    });
    result
}
/// Binary, display-only comparison pixels for a prepared document candidate.
/// # Safety
/// The host is exclusively owned by the caller; json is a readable C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_preview(host: *mut CapyHost, json: *const c_char) -> *mut crate::previews::CapyPreview {
    let mut packet = std::ptr::null_mut();
    guard(host, |host| {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Query { id: u32, index: usize }
        let query: Query = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        let result = host.documents.as_ref().ok_or("Documents unavailable")?.preview(query.id, query.index)?;
        packet = Box::into_raw(Box::new(result));
        Ok(0)
    });
    packet
}
/// Stateless shared numeric policy; safe on the UI thread without a host.
/// # Safety
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call. Free a nonnull result with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_number(json: *const c_char) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<CString, String> {
        let request: layer_ui::NumericRequest =
            serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        CString::new(serde_json::to_string(&request.resolve()?).map_err(err)?).map_err(err)
    }));
    match result {
        Ok(Ok(value)) => value.into_raw(),
        Ok(Err(error)) => {
            fail(error);
            std::ptr::null_mut()
        }
        Err(_) => {
            fail("Numeric policy panic");
            std::ptr::null_mut()
        }
    }
}
/// # Safety
/// `value` must be null or an unmodified, still-owned string returned by
/// `capy_snapshot`, `capy_query`, `capy_surface_info` or `capy_number`, freed once.
/// Never pass the borrowed pointer returned by `capy_error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_string_free(value: *mut c_char) {
    if !value.is_null() {
        unsafe { drop(CString::from_raw(value)) }
    }
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_suspend(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.target = None;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a valid host with no concurrent mutation or destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_view_revision(host: *const CapyHost) -> u64 {
    unsafe { host.as_ref() }.map_or(0, |h| h.native.session.state().camera.revision)
}

// The backbuffer is sized in physical pixels. SwapChainPanel otherwise scales
// it again by the XAML composition scale, displacing both the image and input.
fn set_composition_scale(surface: &wgpu::Surface<'_>, scale: f32) -> Result<(), String> {
    let native = unsafe { surface.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or("The Windows surface has no D3D12 backend")?;
    let swapchain = native
        .swap_chain()
        .ok_or("The Windows swap chain is not configured")?;
    let transform = windows::Win32::Graphics::Dxgi::DXGI_MATRIX_3X2_F {
        _11: 1.0 / scale,
        _22: 1.0 / scale,
        ..Default::default()
    };
    unsafe { swapchain.SetMatrixTransform(&transform) }.map_err(err)
}

/// # Safety
/// Exclusive host access, or the UI owner while the render worker is parked.
/// Poisoned hosts cannot resume after an ABI panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_device_lost(host: *const CapyHost) -> bool {
    unsafe { host.as_ref() }.is_some_and(|host| !host.poisoned && host.device_is_lost())
}

/// # Safety
/// Call on the XAML thread after detaching the old swap chain, with the render
/// worker parked. Keep the same panel alive through the replacement surface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_reset_surface(host: *mut CapyHost, panel: *mut c_void) -> i32 {
    guard(host, |host| {
        if panel.is_null() || host.target.is_some() {
            return Err("Surface replacement requires a live panel and no acquired image".into());
        }
        let surface = unsafe {
            host.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SwapChainPanel(panel))
        }
        .map_err(err)?;
        host.surface = surface;
        host.config = None;
        host.blank_presented = false;
        host.cancel_contacts()?;
        Ok(0)
    })
}

/// # Safety
/// Exclusive render-owner access. Available only in an isolated smoke-test host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_test_device_loss(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        if std::env::var_os("CAPY_SMOKE_TEST").is_none()
            || !std::env::var_os("CAPY_SETTINGS_DIRECTORY")
                .map(std::path::PathBuf::from)
                .is_some_and(|p| p.is_absolute())
        {
            return Err("Device-loss injection requires an isolated smoke test".into());
        }
        host.target = None;
        let gpu = host
            .native
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("GPU is not ready")?;
        // Remove the D3D12 device shared by this process's windows on the adapter.
        // The COM object remains owned by wgpu. Ordinary loss detection must
        // observe removal; this hook never sets the flag or resets the adapter.
        {
            use windows::{Win32::Graphics::Direct3D12::ID3D12Device5, core::Interface};
            let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }
                .ok_or("Device removal requires D3D12")?;
            let device: ID3D12Device5 = native.raw_device().cast().map_err(err)?;
            unsafe { device.RemoveDevice() };
        }
        // Drop the HAL guard before polling wgpu's device-loss notification.
        let _ = gpu.device().poll(wgpu::PollType::Poll);
        Ok(0)
    })
}

/// # Safety
/// Exclusive render-owner access after canvas input has stopped. Unsubmitted
/// contacts are canceled; keep services alive until approved close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_suspend_renderer(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.target = None;
        host.cancel_contacts()?;
        host.native.suspend_renderer()?;
        host.presenter = None;
        drop(host.native.session.renderer_mut().0.take());
        host.config = None;
        if let Some(service) = host.documents.as_mut() {
            service.renderer_unavailable(&mut host.native)?;
        }
        if let Some(mut service) = host.filters.take() {
            service.stop();
        }
        host.native.invalidate_snapshot();
        Ok(0)
    })
}

/// Native overview geometry, serialized on the same owner as canvas actions.
/// # Safety
/// `host` must be null or a live exclusively accessed host. `json` must be null
/// or a readable, unchanged NUL-terminated buffer throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_overviews(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        match host.navigator.set(unsafe { read_json(json) }?) {
            Ok(changed) => {
                host.native.dirty |= changed;
                Ok(0)
            }
            Err(error) => {
                fail(error);
                Ok(1)
            }
        }
    })
}

/// # Safety
/// Exclusive render-owner access to host; json is a readable NUL-terminated
/// buffer. Transfer/free a nonnull CPU packet using capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews(
    host: *mut CapyHost,
    json: *const c_char,
) -> *mut crate::previews::CapyPreview {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        let packet = crate::previews::query(&mut host.native, unsafe { read_json(json) }?)?;
        result = Box::into_raw(Box::new(packet));
        Ok(0)
    });
    result
}

/// Join retired shader workers after every host is destroyed, before process
/// runtime teardown. Ordinary surface/document replacement stays asynchronous.
#[unsafe(no_mangle)]
pub extern "C" fn capy_finish_process() -> i32 {
    match catch_unwind(layer_render_wgpu::finish_shader_compiler_shutdown) {
        Ok(()) => 0,
        Err(_) => {
            fail("Shader compiler shutdown panic");
            -1
        }
    }
}

/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layer_menu(
    host: *mut CapyHost,
    json: *const c_char,
) -> *mut crate::previews::CapyPreview {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        result = Box::into_raw(Box::new(crate::previews::layer_menu(
            &mut host.native,
            unsafe { read_json(json) }?,
        )?));
        Ok(0)
    });
    result
}
/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layer_thumbnails(
    host: *mut CapyHost,
    json: *const c_char,
) -> *mut crate::previews::CapyPreview {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        result = Box::into_raw(Box::new(crate::previews::layer_thumbnails(
            &mut host.native,
            unsafe { read_json(json) }?,
        )?));
        Ok(0)
    });
    result
}

/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_query(
    host: *mut CapyHost,
    json: *const c_char,
) -> *mut crate::previews::CapyPreview {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        result = Box::into_raw(Box::new(crate::workspace::query(
            &mut host.native,
            unsafe { read_json(json) }?,
        )?));
        Ok(0)
    });
    result
}

/// Synthetic capability changes exercise native presentation, never hardware acceptance.
/// # Safety
/// Exclusive access to the render-owned host. Only an isolated smoke test can call it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_test_display(host: *mut CapyHost, hdr: bool) -> i32 {
    guard(host, |host| {
        if std::env::var_os("CAPY_SMOKE_TEST").is_none() || std::env::var_os("CAPY_TEST_HDR").is_none()
            || !std::env::var_os("CAPY_SETTINGS_DIRECTORY").map(std::path::PathBuf::from).is_some_and(|p| p.is_absolute()) {
            return Err("Display injection requires an isolated HDR smoke test".into());
        }
        host.test_display = Some(crate::display::Display::reported(hdr, if hdr { 1015. } else { 80. }, "Synthetic test output".into()));
        host.display_checked = None;
        host.poll_services()?;
        Ok(0)
    })
}

/// Optional read-only presentation counters. A failed query is diagnostic only;
/// it must not change document state or stop the renderer.
/// # Safety
/// `host` is exclusively owned by the render thread; `values` writes six u64s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_presentation_stats(host: *const CapyHost, values: *mut u64) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else { return -1; };
    if values.is_null() { return -1; }
    let Some(native) = (unsafe { host.surface.as_hal::<wgpu::hal::api::Dx12>() }) else { return -1; };
    let Some(swapchain) = native.swap_chain() else { return -1; };
    let output = unsafe { &mut *values.cast::<[u64; 6]>() };
    *output = [0; 6];
    let last = match unsafe { swapchain.GetLastPresentCount() } {
        Ok(value) => value, Err(error) => return error.code().0,
    };
    output[0] = u64::from(last);
    let mut stats = windows::Win32::Graphics::Dxgi::DXGI_FRAME_STATISTICS::default();
    if let Err(error) = unsafe { swapchain.GetFrameStatistics(&mut stats) } { return error.code().0; }
    output[1..].copy_from_slice(&[u64::from(stats.PresentCount), u64::from(stats.PresentRefreshCount),
        u64::from(stats.SyncRefreshCount), stats.SyncQPCTime as u64, stats.SyncGPUTime as u64]);
    0
}
