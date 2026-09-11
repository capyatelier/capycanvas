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
    target: Option<wgpu::SurfaceTexture>,
    surface: wgpu::Surface<'static>,
    config: Option<wgpu::SurfaceConfiguration>,
    presenter: Option<ViewportPresenter>,
    native: NativeHost,
    instance: wgpu::Instance,
    cursor: CanvasCursor,
    scale: f32,
    blank_presented: bool,
}
impl CapyHost {
    unsafe fn new(panel: *mut c_void, width: u32, height: u32, scale: f32) -> Result<Self, String> {
        if panel.is_null() || width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid Windows canvas surface".into());
        }
        let mut native = NativeHost::new(layer_ui::Platform::Windows)?;
        native.startup = Default::default();
        native.resize(width, height, scale)?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SwapChainPanel(panel))
        }
        .map_err(err)?;
        // GPU allocation and shader work happen after ownership moves to the
        // render worker. No swap chain is attached during CPU/UI initialization.
        Ok(Self {
            poisoned: false,
            target: None,
            surface,
            config: None,
            presenter: None,
            native,
            instance,
            cursor: CanvasCursor::default(),
            scale,
            blank_presented: false,
        })
    }

    fn prepare_gpu(&mut self) -> Result<(), String> {
        if self.config.is_some() {
            return Ok(());
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
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(err)?;
        let [width, height] = self.native.session.state().camera.viewport;
        let mut config = self
            .surface
            .get_default_config(&adapter, width, height)
            .ok_or("D3D12 surface unsupported")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        let presenter = ViewportPresenter::new(&device, config.format);
        let renderer = WgpuRasterizer::from_wgpu_staged(adapter, device, queue).map_err(err)?;
        self.native.session.renderer_mut().0 = Some(renderer);
        self.native.startup = Default::default();
        self.presenter = Some(presenter);
        self.config = Some(config);
        // Surface::configure calls SetSwapChain. Only capy_resize may do that,
        // on the UI thread while this worker is parked.
        Ok(())
    }

    fn frame(&mut self, now: u64, presentation: u64) -> Result<i32, String> {
        let Some(target) = self.target.take() else {
            return Ok(1);
        };
        if self.blank_presented {
            let engine = self.native.session.engine();
            let gpu = engine.backend().0.as_ref().ok_or("GPU is not prepared")?;
            if gpu.startup_needs_update(engine.document(), engine.brush()) {
                let (document, brush) = (engine.document().clone(), engine.brush().clone());
                self.native
                    .session
                    .renderer_mut()
                    .0
                    .as_mut()
                    .unwrap()
                    .prepare_startup(&document, &brush)
                    .map_err(err)?;
            }
            self.native.startup = self
                .native
                .session
                .renderer_mut()
                .0
                .as_mut()
                .unwrap()
                .poll_startup()
                .map_err(err)?;
            if self.native.startup.canvas_ready {
                let previous = self.native.session.state().revision;
                let change = self.native.session.frame(now, presentation)?;
                self.native.dirty = change.canvas_wake;
                self.native.apply_change(previous, change);
            }
        } else {
            // Show paper before background shader preparation, retaining the
            // engine's pending replay for the first fully initialized frame.
            let view = self.native.session.state().camera.view();
            let document = self.native.session.engine().document();
            let extent = [document.width, document.height];
            let layers: Vec<_> = document
                .layers
                .iter()
                .filter(|l| l.kind == layer_core::LayerKind::Background)
                .cloned()
                .collect();
            self.native
                .session
                .renderer_mut()
                .0
                .as_mut()
                .ok_or("GPU is not prepared")?
                .submit(layer_render::FramePacket {
                    time_seconds: 0.,
                    view,
                    document_extent: extent,
                    layers: &layers,
                    dabs: &[],
                    dab_batches: &[],
                    reset_layers: true,
                    composite_all: true,
                })
                .map_err(err)?;
        }
        self.native.dirty |=
            !self.native.startup.complete || self.native.session.wants_continuous_frames();
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
        presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        );
        gpu.queue().present(target);
        self.blank_presented = true;
        gpu.device().poll(wgpu::PollType::Poll).map_err(err)?;
        Ok(i32::from(self.native.dirty))
    }
}
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_prepare_gpu(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.prepare_gpu()?;
        Ok(0)
    })
}
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
            for sample in batch {
                host.native.pointer_event(
                    sample.event(),
                    match sample.button {
                        0 => PointerButton::Primary,
                        1 => PointerButton::Pan,
                        _ => PointerButton::Other,
                    },
                )?;
            }
        }
        Ok(0)
    })
}
unsafe fn read_json<'a>(json: *const c_char) -> Result<&'a str, String> {
    if json.is_null() {
        return Err("Null JSON".into());
    }
    unsafe { CStr::from_ptr(json) }.to_str().map_err(err)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_action(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        host.native
            .dispatch(serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?)?;
        Ok(0)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_input(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        host.native
            .input(serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?)?;
        Ok(0)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_acquire(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_frame(host: *mut CapyHost, now: u64, presentation: u64) -> i32 {
    guard(host, |host| host.frame(now, presentation))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_snapshot(host: *mut CapyHost) -> *mut c_char {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        if let Some(snapshot) = host.native.take_snapshot() {
            result = CString::new(snapshot.to_string()).map_err(err)?.into_raw();
        }
        Ok(0)
    });
    result
}
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_string_free(value: *mut c_char) {
    if !value.is_null() {
        unsafe { drop(CString::from_raw(value)) }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_suspend(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
        host.target = None;
        Ok(0)
    })
}
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
