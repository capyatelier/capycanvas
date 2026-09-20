//! Apple host ABI. The same library serves UIKit and AppKit. Rust owns the
//! shared session; Swift owns UI and serial execution. No callbacks into Swift.
mod metal;
mod local_tone;
mod document_tabs;
pub use document_tabs::*;

/// SDR viewing contract shared by canvas, UI values and image transports.
/// Core Animation/ColorSync maps tagged P3 to the current screen, including sRGB.
const DISPLAY_SPACE: layer_core::color::RgbSpace = layer_core::color::RgbSpace::DisplayP3;
mod project;
pub use project::*;
mod previews;
pub use previews::*;
mod workspaces;
pub use workspaces::*;
#[cfg(test)]
mod tests;
use layer_host::{NativeHost, PointerBatch};
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub struct CapyApple {
    metal: metal::MetalHost,
    host: NativeHost,
    documents: document_tabs::Sessions,
    document_gpu: Option<document_tabs::Context>,
    error: Option<CString>,
    chrome_facts: layer_ui::ChromeFacts,
    dismissed_contacts: std::collections::BTreeSet<u64>,
}
impl CapyApple {
    fn perform<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Option<T> {
        self.error = None;
        match catch_unwind(AssertUnwindSafe(|| {
            self.metal.observe_failure(&mut self.host, false);
            work(self)
        })) {
            Ok(Ok(value)) => Some(value),
            result => {
                let message = match result {
                    Ok(Err(message)) => message,
                    _ => "Native operation panicked".into(),
                };
                self.error = CString::new(message.replace('\0', " ")).ok();
                None
            }
        }
    }
    fn gpu_operation<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let result = catch_unwind(AssertUnwindSafe(|| work(self)))
            .unwrap_or_else(|_| Err("Canvas rendering stopped unexpectedly".into()));
        if let Err(message) = &result {
            self.metal.stop(&mut self.host, message.clone());
            self.dismissed_contacts.clear();
        }
        result
    }
}
/// # Safety
/// Returned handle must have one serial owner and be destroyed exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_create(platform: u32) -> *mut CapyApple {
    catch_unwind(|| {
        let platform = match platform {
            0 => layer_ui::Platform::Ios,
            1 => layer_ui::Platform::Mac,
            _ => return None,
        };
        let mut host = NativeHost::new(platform).ok()?;
        host.ui_color_space = DISPLAY_SPACE;
        host.dispatch(layer_ui::UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState::for_platform(platform)),
        })
        .ok()?;
        host.session.set_document_replacement(false);
        Some(Box::into_raw(Box::new(CapyApple {
            host,
            documents: Default::default(),
            document_gpu: None,
            metal: metal::MetalHost::default(),
            error: None,
            chrome_facts: Default::default(),
            dismissed_contacts: Default::default(),
        })))
    })
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Handle must come from create, with no outstanding calls or borrowed strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_destroy(app: *mut CapyApple) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}
/// # Safety
/// Handle must remain alive throughout this call and use of the borrowed result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_error(app: *const CapyApple) -> *const c_char {
    unsafe { app.as_ref() }
        .and_then(|a| a.error.as_ref())
        .map_or(std::ptr::null(), |e| e.as_ptr())
}
/// # Safety
/// Text must be an unreleased owned result returned by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_string_free(text: *mut c_char) {
    if !text.is_null() {
        unsafe {
            drop(CString::from_raw(text));
        }
    }
}
/// # Safety
/// `json` is a NUL-terminated UTF-8 numeric request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_numeric(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing numeric request", |source| {
        let request: layer_ui::NumericRequest = serde_json::from_str(source).map_err(|e| e.to_string())?;
        serde_json::to_value(request.resolve()?).map_err(|e| e.to_string())
    }) }
}
/// Shared tagged color forms, display previews and sampled gradient ramps.
/// # Safety
/// `json` is a NUL-terminated UTF-8 color request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_color_ui(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing color request", |source| {
        layer_ui::color_ui(serde_json::from_str(source).map_err(|e| e.to_string())?)
    }) }
}
unsafe fn stateless_json(json: *const c_char, missing: &str,
    resolve: impl FnOnce(&str) -> Result<serde_json::Value, String> + std::panic::UnwindSafe) -> *mut c_char {
    catch_unwind(|| {
        let result = (|| {
            if json.is_null() {
                return Err(missing.to_string());
            }
            let source = unsafe { CStr::from_ptr(json) }
                .to_str()
                .map_err(|e| e.to_string())?;
            resolve(source)
        })();
        let response = result.unwrap_or_else(|error| serde_json::json!({"error": error}));
        CString::new(response.to_string())
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_color_hit(x: f32, y: f32, size: f32, shape: u32) -> u32 {
    let Some(shape) = color_shape(shape) else {
        return 0;
    };
    match layer_ui::ColorWheelGeometry::new(size).and_then(|g| g.hit_shape([x, y], shape)) {
        None => 0,
        Some(layer_ui::ColorWheelPart::Hue) => 1,
        Some(layer_ui::ColorWheelPart::Field) => 2,
    }
}
fn color_shape(shape: u32) -> Option<layer_ui::ColorShape> {
    match shape {
        0 => Some(layer_ui::ColorShape::Circle),
        1 => Some(layer_ui::ColorShape::Square),
        2 => Some(layer_ui::ColorShape::Triangle),
        _ => None,
    }
}
/// Shared geometry, fetched only when the panel allocation changes.
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_color_layout(size: f32) -> *mut c_char {
    catch_unwind(|| {
        let layout = layer_ui::ColorPanelLayout::new(size)?;
        CString::new(serde_json::to_string(&layout).ok()?).ok().map(CString::into_raw)
    })
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}
/// Shared dial hit testing: 0 misses, 1 center, 2 brightness, 3 color.
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_parameter_hit(x:f32,y:f32,size:f32,hdr:bool)->u32 {
    if hdr { u32::from(layer_ui::HdrIntensityArc::new(size).is_some_and(|a|a.contains([x,y]))) }
    else {layer_ui::color_management::dial_hit(size,[x,y])}
}
/// # Safety
/// Caller supplies exactly edge²*4 writable bytes. This is a disposable UI texture.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_texture(edge:u32,bytes:*mut u8,count:usize)->bool {
    if edge==0 || edge>512 || bytes.is_null() || count!=(edge as usize).pow(2)*4 {return false;}
    let texture=layer_ui::proof_panel::sdr_direction_texture(edge);
    unsafe {std::ptr::copy_nonoverlapping(texture.as_ptr(),bytes,count)};
    true
}
/// # Safety
/// Borrowed JSON and side²*4 Float32 output components, exclusively writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_hdr_field(side:u32,request:*const c_char,pixels:*mut f32,count:usize,operation:u32)->bool {
    if side==0 || side>2048 || pixels.is_null() || count!=(side as usize).pow(2)*4 || request.is_null(){return false;}
    let Ok(source)=unsafe{CStr::from_ptr(request)}.to_str() else{return false;};
    let Ok(request)=serde_json::from_str::<layer_ui::color_management::PickerField>(source) else{return false;};
    let pixels=unsafe{std::slice::from_raw_parts_mut(pixels.cast::<[f32;4]>(),count/4)};
    match operation {0=>request.render(side,pixels),1=>request.render_base(side,pixels),2=>request.map(pixels),_=>Err("Unknown field operation".into())}.is_ok()
}
/// Stateless display-encoded wheel field. No editor, GPU or file access.
/// # Safety
/// `rgba` must point to `count` writable bytes exclusively borrowed for this call.
/// `rgb_space` is a NUL-terminated shared RGB-space name valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_color_field(
    side: u32,
    hue: f32,
    shape: u32,
    rgb_space: *const c_char,
    guide: bool,
    rgba: *mut u8,
    count: usize,
) -> i32 {
    let length = (side as usize)
        .checked_mul(side as usize)
        .and_then(|n| n.checked_mul(4));
    if side == 0
        || !hue.is_finite()
        || rgba.is_null()
        || length != Some(count)
        || count > isize::MAX as usize
    {
        return 0;
    }
    if rgb_space.is_null() { return 0; }
    let Ok(name) = unsafe { CStr::from_ptr(rgb_space) }.to_str() else { return 0; };
    let Ok(space) = serde_json::from_value(serde_json::Value::String(name.into())) else { return 0; };
    let Some(shape) = color_shape(shape) else { return 0; };
    let pixels = unsafe { std::slice::from_raw_parts_mut(rgba, count) };
    i32::from(if guide {
        layer_ui::render_hue_guide_in(side, shape, space, DISPLAY_SPACE, pixels)
    } else {
        layer_ui::render_color_field(side, shape, hue, space, DISPLAY_SPACE, pixels)
    })
}
/// # Safety
/// Valid handle; json must be a NUL-terminated UTF-8 string except for requests 3, 5 and 7.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_request(
    app: *mut CapyApple,
    request: u32,
    json: *const c_char,
) -> *mut c_char {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|a| {
        if matches!(request, 3 | 5 | 7) {
            let snapshot = match request {
                7 => a.host.take_layout_update_bytes(),
                5 => a.host.take_update_bytes(),
                _ => a.host.take_snapshot_bytes(),
            };
            return snapshot
                .map_err(|e| e.to_string())?
                .map(|bytes| {
                    let mut snapshot: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    snapshot["display_status"] = a.metal.display_status(&a.host);
                    snapshot["document_tabs"] = a.tabs_view(a.host.logical[0]);
                    CString::new(snapshot.to_string())
                        .map(CString::into_raw)
                        .map_err(|e| e.to_string())
                })
                .transpose();
        }
        let value = {
            if json.is_null() {
                return Err("Missing request JSON".into());
            }
            serde_json::from_str(
                unsafe { CStr::from_ptr(json) }
                    .to_str()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
        };
        let result = match request {
            0 => {
                a.host
                    .dispatch(serde_json::from_value(value).map_err(|e| e.to_string())?)?;
                Some(serde_json::Value::Null)
            }
            1 => {
                let input: layer_ui::UiInput =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                let reply = a.host.input(input.clone())?;
                match input {
                    layer_ui::UiInput::Chrome { facts, .. } => a.chrome_facts = facts,
                    layer_ui::UiInput::Blur => a.dismissed_contacts.clear(),
                    _ => {}
                }
                Some(serde_json::to_value(reply).map_err(|e| e.to_string())?)
            }
            2 => Some(match value.get("type").and_then(serde_json::Value::as_str) {
                Some("display_headroom") => {
                    let headroom=value["value"].as_f64().ok_or("Missing display headroom")? as f32;
                    a.metal.set_headroom(&mut a.host,headroom)?;
                    serde_json::Value::Null
                },
                Some("document_tabs") => a.tabs_request(value)?,
                Some("proof_form") => layer_ui::proof_workflow::proof_form(&a.host.session),
                Some("proof_status") => serde_json::to_value(a.metal.proof.observe(&a.host.session)).map_err(|e| e.to_string())?,
                _ => a.host.query(value)?,
            }),
            6 => Some(workspaces::session_request(&mut a.host, value)?),
            4 => Some(
                serde_json::to_value(
                    serde_json::from_value::<layer_ui::NumericRequest>(value)
                        .map_err(|e| e.to_string())?
                        .resolve()?,
                )
                .map_err(|e| e.to_string())?,
            ),
            _ => return Err("Unknown Apple host request".into()),
        };
        result
            .map(|r| {
                CString::new(r.to_string())
                    .map(CString::into_raw)
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Valid exclusively owned handle, called on the serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_scroll(
    app: *mut CapyApple,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    scale: f32,
    zoom: u32,
    horizontal: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        a.host
            .scroll([x, y], [dx, dy], scale, zoom != 0, horizontal != 0)
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Valid exclusively owned handle, called on the serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_gesture(
    app: *mut CapyApple,
    x: f32,
    y: f32,
    scale: f32,
    rotation: f32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| a.host.gesture([x, y], scale, rotation))
        .map_or(-1, |_| 0)
}

/// # Safety
/// Valid handle and retained CAMetalLayer; layer outlives attach through detach.
/// Cache directory is a NUL-terminated UTF-8 path valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_attach(
    app: *mut CapyApple,
    layer: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
    cache_directory: *const c_char,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if layer.is_null() || cache_directory.is_null() {
            return Err("Missing Metal layer or cache directory".into());
        }
        let cache = unsafe { CStr::from_ptr(cache_directory) }
            .to_str()
            .map_err(|e| e.to_string())?;
        a.host.resize(width, height, scale)?;
        a.gpu_operation(|a| unsafe {
            a.metal.attach(&mut a.host, layer, std::path::Path::new(cache))
        })
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Call after submitting the bundled filters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_finish_startup_cache(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if let Some(gpu) = a.host.session.renderer_mut().0.as_mut() {
            gpu.finish_startup_cache();
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_resize(
    app: *mut CapyApple,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| a.host.resize(width, height, scale))
        .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Request presentation after native exposure
/// without changing document, camera or history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_redraw(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.host.dirty = true;
    0
}
/// # Safety
/// Valid exclusively owned handle; detach before releasing its native layer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_detach(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        a.host.input(layer_ui::UiInput::Blur)?;
        a.dismissed_contacts.clear();
        a.metal.detach();
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Retire GPU resources while retaining the CPU session;
/// attach the retained native layer again to restart rendering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_suspend_renderer(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        a.metal.stop(&mut a.host, "Canvas stopped. Save the drawing or restart the canvas to continue.".into());
        a.dismissed_contacts.clear();
        Ok(())
    }).map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Nonblocking health check, including when no frame is due.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_poll_renderer(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        a.gpu_operation(|a| {
            a.metal.observe_failure(&mut a.host, true);
            let changed=a.metal.poll_color(&mut a.host)?;
            if changed {a.host.invalidate_snapshot();}
            Ok(i32::from(changed || a.host.session.rendering_suspended()))
        })
    }).unwrap_or(-1)
}

/// # Safety
/// Debug fixtures and unit tests only; affects this editor's device, never the system GPU.
#[cfg(any(test, debug_assertions))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_test_gpu_fault(app: *mut CapyApple, validation: u32) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        let device = a.host.session.engine().backend().0.as_ref().ok_or("No test GPU")?.device();
        if validation == 1 {
            let _invalid = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("isolated invalid buffer"), size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
        } else if validation == 0 { device.destroy(); }
        else { return Err("Unknown GPU fault".into()); }
        Ok(())
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Records must contain count initialized doubles, alive for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_pointer(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    predicted: u32,
    view_revision: u64,
) -> i32 {
    unsafe {
        apple_pointer(
            app,
            id,
            tool,
            button,
            records,
            count,
            predicted,
            view_revision,
            std::ptr::null(),
            0,
        )
    }
}
/// # Safety
/// Records contain `count` doubles and updates contain `count / 9 * 2` u64s.
/// Both arrays must remain alive for this call. Updates are token/expecting pairs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_pointer_updates(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    updates: *const u64,
    correction: u32,
    view_revision: u64,
) -> i32 {
    if updates.is_null() {
        return -1;
    }
    unsafe {
        apple_pointer(
            app,
            id,
            tool,
            button,
            records,
            count,
            0,
            view_revision,
            updates,
            correction,
        )
    }
}
unsafe fn apple_pointer(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    predicted: u32,
    view_revision: u64,
    updates: *const u64,
    correction: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if records.is_null()
            || count == 0
            || count % 9 != 0
            || count > 8192 * 9
            || tool > 3
            || button > 2
            || predicted > 1
            || correction > 1
        {
            return Err("Invalid Apple pointer batch".into());
        }
        let records = unsafe { std::slice::from_raw_parts(records, count) };
        if !records.iter().all(|v| v.is_finite())
            || records
                .chunks_exact(9)
                .any(|r| r[7] < 0. || r[8] < 0. || r[8] > 4. || r[8].fract() != 0.)
        {
            return Err("Invalid Apple pointer sample".into());
        }
        if !a.host.accepts_pointer_input(view_revision) {
            return Ok(());
        }
        if a.dismissed_contacts.contains(&id) {
            if correction == 0 && predicted == 0 && records.chunks_exact(9).any(|r| r[8] >= 3.) {
                a.dismissed_contacts.remove(&id);
            }
            return Ok(());
        }
        let updates = if updates.is_null() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(updates, count / 9 * 2) }
        };
        if updates
            .chunks_exact(2)
            .any(|u| u[1] > 1 || (u[1] != 0 && u[0] == 0) || (correction != 0 && u[0] == 0))
        {
            return Err("Invalid Apple input estimates".into());
        }
        if correction == 0 && predicted == 0 && records[8] == 1. {
            let viewport = a.host.logical;
            let physical = a.host.session.state().camera.viewport;
            let position = [
                records[0] as f32 * viewport[0] / physical[0].max(1) as f32,
                records[1] as f32 * viewport[1] / physical[1].max(1) as f32,
            ];
            let reply = a.host.input(layer_ui::UiInput::Chrome {
                event: layer_ui::ChromeEvent::Contact {
                    position,
                    canvas: true,
                },
                facts: a.chrome_facts,
                viewport,
            })?;
            if reply.handled {
                if !records.chunks_exact(9).any(|r| r[8] >= 3.) {
                    a.dismissed_contacts.insert(id);
                }
                return Ok(());
            }
        }
        a.host.pointer_batch_updates(
            PointerBatch {
                id,
                tool: tool as u8,
                button: button as u8,
                records,
                predicted: predicted != 0,
                view_revision,
            },
            updates,
            correction != 0,
        )
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid handle; costs, if non-NULL, must point to five writable u64 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_frame(
    app: *mut CapyApple,
    now: u64,
    presentation: u64,
    costs: *mut u64,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if presentation < now {
            return Err("Presentation precedes observation time".into());
        }
        if a.host.session.rendering_suspended() { return Ok(0); }
        let (again, timing) = a.gpu_operation(|a| a.metal.frame(&mut a.host, now, presentation))?;
        if !costs.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(timing.as_ptr(), costs, 5);
            }
        }
        Ok(i32::from(again))
    })
    .unwrap_or(-1)
}
/// # Safety
/// Valid handle, read on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_camera_revision(app: *const CapyApple) -> u64 {
    unsafe { app.as_ref() }.map_or(0, |a| a.host.session.state().camera.revision)
}

/// # Safety
/// Valid handle on its serial owner. Timing is opt-in and disabled by default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_gpu_timing(app: *mut CapyApple, enabled: u32) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if enabled > 1 {
            return Err("Invalid GPU timing flag".into());
        }
        a.metal.set_timing_enabled(enabled != 0);
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Valid serial-owned handle, writable stats and samples (capacity <= 256).
/// A zero-capacity call may pass NULL samples. Never waits for GPU completion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_take_gpu_timing(
    app: *mut CapyApple,
    samples: *mut layer_render_wgpu::GpuFrameSample,
    capacity: usize,
    stats: *mut layer_render_wgpu::GpuFrameTimingStats,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if capacity > 256 || stats.is_null() || (capacity > 0 && samples.is_null()) {
            return Err("Invalid GPU timing output buffer".into());
        }
        let output = if capacity == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(samples, capacity) }
        };
        let (count, status) = a.metal.take_timing(&mut a.host, output)?;
        unsafe {
            *stats = status;
        }
        Ok(count as i32)
    })
    .unwrap_or(-1)
}
