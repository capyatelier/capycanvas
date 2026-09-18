use layer_host::NativeHost;
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
    services: Option<crate::settings::SettingsService>,
    filters: Option<crate::filter_packages::FilterService>,
    documents: Option<crate::documents::DocumentService>,
    recovery: Option<crate::recovery::Service>,
    workspaces: Option<crate::workspace_service::WorkspaceService<layer_workspace::StoreWorker>>,
    blocked_contacts: std::collections::BTreeSet<u64>,
}
impl CapyHost {
    unsafe fn new(panel: *mut c_void, width: u32, height: u32, scale: f32) -> Result<Self, String> {
        if panel.is_null() || width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid Windows canvas surface".into());
        }
        let mut native = NativeHost::new(layer_ui::Platform::Windows)?;
        crate::workspace::initialize(&mut native)?;
        native.session.set_document_replacement(true);
        native.startup = Default::default();
        native.resize(width, height, scale)?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
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
            services: None,
            filters: None,
            documents: None,
            recovery: None,
            workspaces: None,
            blocked_contacts: Default::default(),
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
    fn poll_services(&mut self) -> Result<(), String> {
        if let Some(service) = self.documents.as_mut() {
            service.poll(&mut self.native)?;
        }
        if let Some(service) = self.recovery.as_mut() { service.poll(&mut self.native)?; }
        if let Some(service) = self.services.as_mut() {
            service.poll(&mut self.native)?;
        }
        if let Some(service) = self.filters.as_mut() {
            service.poll(&mut self.native);
        }
        if let Some(service) = self.workspaces.as_mut() {
            if self.native.session.state().document_file.close_ready
                && self.services.as_ref().is_none_or(|s| s.close_status().ready)
                && self.recovery.as_ref().is_none_or(|s| s.close_ready())
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
            self.presenter = None;
            let renderer = layer_host::Renderer(self.native.session.renderer_mut().0.take());
            if let Some(service) = &mut self.recovery { service.retire_renderer(renderer); } else { drop(renderer); }
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
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        let mut presenter = ViewportPresenter::new(&device, config.format);
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue,
            self.native.session.engine().document().color).map_err(err)?;
        renderer.configure_ui_previews(layer_core::color::RgbSpace::Srgb).map_err(err)?;
        // Prepare the optional overview pipeline during GPU startup, before input is live.
        presenter.prepare_overviews(&renderer);
        gpu_state.check()?;
        let revision = self.native.session.state().revision;
        let (retired, change) = self
            .native
            .session
            .replace_renderer(layer_host::Renderer(Some(renderer)))?;
        self.native.apply_change(revision, change);
        if let Some(service) = &mut self.recovery { service.retire_renderer(retired); } else { drop(retired); }
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
        let Some(target) = self.target.take() else {
            return Ok(1);
        };
        self.native
            .prepare_canvas_frame(now, presentation, self.blank_presented)?;
        self.gpu.check()?;
        self.native.dirty |= self.native.session.wants_continuous_frames();
        self.native
            .session
            .update_canvas_cursor(&mut self.cursor, false);
        self.native
            .session
            .append_layer_overlay(&mut self.cursor.segments);
        let view = self.native.session.state().camera.view();
        let surround = self.native.session.state().palette.surround_linear;
        let gpu = self.native.session.engine().backend().0.as_ref().unwrap();
        let presenter = self
            .presenter
            .as_mut()
            .ok_or("Viewport presenter is not prepared")?;
        presenter.set_cursor(gpu.device(), &self.cursor.segments, self.scale);
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
/// # Safety
/// `host` must be null or the uniquely owned pointer from `capy_create`, freed
/// exactly once. Stop input/render callers and finish service callbacks first;
/// detach the swap chain on the panel's XAML thread before destroying the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_destroy(host: *mut CapyHost) {
    if !host.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { drop(Box::from_raw(host)) }));
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
        if host.recovery.is_none() {
            let context = context as usize;
            host.recovery = Some(crate::recovery::Service::open(move || { if let Some(wake) = wake { wake(context as *mut c_void); } })?);
        }
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
        let recovery = host.recovery.as_mut().map_or(Ok(()), |s| s.stop());
        documents.and(settings).and(workspaces).and(filters).and(recovery)
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
                    .as_ref()
                    .ok_or("GPU is not prepared")?
                    .device(),
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
            for sample in batch {
                if blocked && sample.phase != 0 {
                    host.blocked_contacts.insert(sample.id);
                }
                if blocked || host.blocked_contacts.contains(&sample.id) {
                    if sample.phase == 3 || sample.phase == 4 {
                        host.blocked_contacts.remove(&sample.id);
                    }
                    continue;
                }
                let button = match sample.button {
                    0 => PointerButton::Primary,
                    1 => PointerButton::Pan,
                    _ => PointerButton::Other,
                };
                host.native.pointer_event(sample.event(), button)?;
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
        // The restore sheet owns the window until its candidate returns. A
        // queued title-bar close must not retire that still-unadopted origin.
        if matches!(action, crate::documents::DocumentAction::Close) && host.recovery.as_ref().is_some_and(|s| s.restoring()) { return Ok(0); }
        let result = if let crate::documents::DocumentAction::Recovery { action } = action {
            host.recovery.as_mut().ok_or("Recovery service is unavailable")?.dispatch(&mut host.native, action)
        } else {
            host.documents.as_mut().ok_or("Document service is unavailable")?.dispatch(&mut host.native, action)
        };
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
            windows_rendering_suspended: host.native.session.rendering_suspended(),
            windows_importing: host
                .documents
                .as_ref()
                .is_some_and(|service| service.importing()),
            windows_image_import: host
                .documents
                .as_ref()
                .and_then(|service| service.import_request()),
            windows_recovery: host.recovery.as_ref().map(|service| service.status()),
            windows_document: host.documents.as_ref().and_then(|service| service.status()),
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
            "maximum_frame_latency": config.desired_maximum_frame_latency
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
