use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use layer_ui::{CanvasCursor, ContactPhase, PointerButton, PointerKind, UiInput, UiSession};
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
    // Surface drops before the session and instance. The native panel retains
    // its COM reference for the lifetime of the surface.
    poisoned: bool,
    target: Option<wgpu::SurfaceTexture>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    session: UiSession<WgpuRasterizer>,
    _instance: wgpu::Instance,
    cursor: CanvasCursor,
    logical: [f32; 2],
    scale: f32,
    sequence: u64,
    last_pen: Option<PenEvent>,
    snapshot_revision: Option<u64>,
    dirty: bool,
}
impl CapyHost {
    unsafe fn new(panel: *mut c_void, width: u32, height: u32, scale: f32) -> Result<Self, String> {
        if panel.is_null() || width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid Windows canvas surface".into());
        }
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SwapChainPanel(panel))
        }
        .map_err(err)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
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
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or("D3D12 surface unsupported")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        // SetSwapChain is performed here, on the XAML thread, exactly once.
        surface.configure(&device, &config);
        set_composition_scale(&surface, scale)?;
        let presenter = ViewportPresenter::new(&device, config.format);
        let renderer = WgpuRasterizer::from_wgpu(adapter, device, queue).map_err(err)?;
        let mut session = UiSession::blank(renderer, [width, height])?;
        session.set_platform(layer_ui::Platform::Windows);
        let logical = [width as f32 / scale, height as f32 / scale];
        session.set_viewport(logical, [width, height])?;

        Ok(Self {
            poisoned: false,
            surface,
            target: None,
            config,
            presenter,
            session,
            _instance: instance,
            cursor: CanvasCursor::default(),
            logical,
            scale,
            sequence: 0,
            last_pen: None,
            snapshot_revision: None,
            dirty: true,
        })
    }
    fn enqueue(&mut self, event: PenEvent) -> Result<(), String> {
        if let Err(event) = self.session.pen(event) {
            // Engine ingress is drained only by its exclusive owner.
            self.session.frame(event.timestamp_ns, event.timestamp_ns)?;
            self.session
                .pen(event)
                .map_err(|_| "Pen ingress stayed full")?;
        }
        self.dirty = true;
        Ok(())
    }
    fn input(&mut self, input: UiInput) -> Result<(), String> {
        let reply = self.session.input(input)?;
        self.dirty |= reply.change.canvas_wake;
        if reply.cancel_paint
            && let Some(mut event) = self.last_pen.take()
        {
            event.phase = PenPhase::Cancel;
            self.sequence += 1;
            event.sequence = self.sequence;
            self.enqueue(event)?;
        }
        Ok(())
    }
    fn pointer(&mut self, sample: CapyPointer) -> Result<(), String> {
        if sample.phase > 4
            || sample.tool > 3
            || sample.button > 2
            || ![
                sample.x,
                sample.y,
                sample.pressure,
                sample.tilt_x,
                sample.tilt_y,
                sample.twist,
                sample.distance,
            ]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err("Invalid Windows pointer record".into());
        }
        let phase = match sample.phase {
            0 => PenPhase::Hover,
            1 => PenPhase::Down,
            2 => PenPhase::Move,
            3 => PenPhase::Up,
            _ => PenPhase::Cancel,
        };
        let position = [sample.x, sample.y];
        let paint = if sample.phase != 0 {
            let contact = match sample.phase {
                1 => ContactPhase::Down,
                2 => ContactPhase::Move,
                3 => ContactPhase::Up,
                _ => ContactPhase::Cancel,
            };
            let reply = self.session.input(UiInput::Pointer {
                id: sample.id,
                phase: contact,
                kind: match sample.tool {
                    1 => PointerKind::Mouse,
                    3 => PointerKind::Touch,
                    _ => PointerKind::Pen,
                },
                button: match sample.button {
                    0 => PointerButton::Primary,
                    1 => PointerButton::Pan,
                    _ => PointerButton::Other,
                },
                position,
            })?;
            self.dirty |= reply.change.canvas_wake;
            reply.paint
        } else {
            false
        };
        self.sequence = self.sequence.max(sample.sequence);
        let event = PenEvent {
            device_id: sample.id,
            sequence: sample.sequence,
            timestamp_ns: sample.timestamp_ns,
            view_revision: sample.view_revision,
            surface_position: Point {
                x: sample.x,
                y: sample.y,
            },
            pressure: sample.pressure,
            tilt_radians: [sample.tilt_x, sample.tilt_y],
            twist_radians: sample.twist,
            distance: sample.distance,
            phase,
            tool: match sample.tool {
                1 => ToolKind::Mouse,
                2 => ToolKind::Eraser,
                _ => ToolKind::Pen,
            },
            flags: SampleFlags(sample.flags as u16),
        };
        if sample.tool != 3 {
            self.session
                .cursor_input(if sample.phase == 4 { None } else { Some(event) });
            self.dirty = true;
        }
        if paint {
            self.enqueue(event)?;
            self.last_pen = if sample.phase >= 3 { None } else { Some(event) };
        }
        Ok(())
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
        host.logical = [width as f32 / scale, height as f32 / scale];
        host.session.set_viewport(host.logical, [width, height])?;
        {
            host.config.width = width;
            host.config.height = height;
            host.surface
                .configure(host.session.engine().backend().device(), &host.config);
        }
        set_composition_scale(&host.surface, scale)?;
        host.dirty = true;
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
            for &sample in batch {
                host.pointer(sample)?;
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
        let action = serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?;
        host.dirty |= host.session.dispatch(action)?.canvas_wake;
        Ok(0)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_input(host: *mut CapyHost, json: *const c_char) -> i32 {
    guard(host, |host| {
        host.input(serde_json::from_str(unsafe { read_json(json) }?).map_err(err)?)?;
        Ok(0)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_acquire(host: *mut CapyHost) -> i32 {
    guard(host, |host| {
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
    guard(host, |host| {
        let Some(target) = host.target.take() else {
            return Ok(1);
        };
        let change = host.session.frame(now, presentation)?;
        host.session.update_canvas_cursor(&mut host.cursor, false);
        host.session.append_layer_overlay(&mut host.cursor.segments);
        let view = host.session.state().camera.view();
        let surround = host.session.state().palette.surround_linear;
        let gpu = host.session.engine().backend();
        host.presenter
            .set_cursor(gpu.device(), &host.cursor.segments, host.scale);
        host.presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        );
        gpu.queue().present(target);
        gpu.device().poll(wgpu::PollType::Poll).map_err(err)?;
        host.dirty = change.canvas_wake || host.session.wants_continuous_frames();
        Ok(i32::from(host.dirty))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_snapshot(host: *mut CapyHost) -> *mut c_char {
    let mut result = std::ptr::null_mut();
    guard(host, |host| {
        let revision = host.session.state().revision;
        if host.snapshot_revision == Some(revision) {
            return Ok(0);
        }
        host.snapshot_revision = Some(revision);
        let layout = host.session.layout(host.logical);
        let panels: Vec<_> = layout
            .groups
            .iter()
            .flat_map(|g| &g.panels)
            .filter_map(|&p| host.session.panel_view(p).ok())
            .collect();
        let json = serde_json::json!({"state":host.session.state(),"layout":layout,"panels":panels,
            "preferences":host.session.preferences(),"catalog":layer_ui::ui_catalog()});
        result = CString::new(json.to_string()).map_err(err)?.into_raw();
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
    unsafe { host.as_ref() }.map_or(0, |h| h.session.state().camera.revision)
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
