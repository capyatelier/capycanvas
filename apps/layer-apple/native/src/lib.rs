//! Apple host ABI. The same library serves UIKit and AppKit. Rust owns the
//! shared session; Swift owns UI and serial execution. No callbacks into Swift.
mod metal;
#[cfg(test)]
mod tests;
use layer_host::{NativeHost, PointerBatch};
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub struct CapyApple {
    metal: metal::MetalHost,
    host: NativeHost,
    error: Option<CString>,
}
impl CapyApple {
    fn perform<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Option<T> {
        self.error = None;
        match catch_unwind(AssertUnwindSafe(|| work(self))) {
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
        Some(Box::into_raw(Box::new(CapyApple {
            host: NativeHost::new(platform).ok()?,
            metal: metal::MetalHost::default(),
            error: None,
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
    catch_unwind(|| {
        let result = (|| {
            if json.is_null() {
                return Err("Missing numeric request".to_string());
            }
            let source = unsafe { CStr::from_ptr(json) }
                .to_str()
                .map_err(|e| e.to_string())?;
            let request: layer_ui::NumericRequest =
                serde_json::from_str(source).map_err(|e| e.to_string())?;
            serde_json::to_value(request.resolve()?).map_err(|e| e.to_string())
        })();
        let response = result.unwrap_or_else(|error| serde_json::json!({"error": error}));
        CString::new(response.to_string())
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_color_hit(x: f32, y: f32, size: f32, space: u32) -> u32 {
    let space = match space {
        0 => layer_ui::ColorSpace::Hsv,
        1 => layer_ui::ColorSpace::Hls,
        _ => return 0,
    };
    match layer_ui::ColorWheelGeometry::new(size).and_then(|g| g.hit([x, y], space)) {
        None => 0,
        Some(layer_ui::ColorWheelPart::Hue) => 1,
        Some(layer_ui::ColorWheelPart::Field) => 2,
    }
}
/// # Safety
/// Valid handle; json must be a NUL-terminated UTF-8 string when request != 3.
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
        let value = if request == 3 {
            serde_json::Value::Null
        } else {
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
            1 => Some(
                serde_json::to_value(
                    a.host
                        .input(serde_json::from_value(value).map_err(|e| e.to_string())?)?,
                )
                .map_err(|e| e.to_string())?,
            ),
            2 => Some(a.host.query(value)?),
            3 => a.host.take_snapshot(),
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
        unsafe {
            a.metal
                .attach(&mut a.host, layer, std::path::Path::new(cache))
        }
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
/// Valid exclusively owned handle; detach before releasing its native layer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_detach(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        a.host.input(layer_ui::UiInput::Blur)?;
        a.metal.detach();
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle, UTF-8 NUL-terminated name and `count` readable
/// image bytes. Neither input is retained after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_import_layer(
    app: *mut CapyApple,
    name: *const c_char,
    width: u32,
    height: u32,
    rgba: *const u8,
    count: usize,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if name.is_null()
            || rgba.is_null()
            || width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || count != width as usize * height as usize * 4
        {
            return Err("Import an image up to 8192 × 8192 pixels with complete RGBA data".into());
        }
        let name = unsafe { CStr::from_ptr(name) }
            .to_str()
            .map_err(|e| e.to_string())?;
        a.host.import_layer_image(
            name,
            layer_render::HostImage {
                width,
                height,
                stride: width * 4,
                format: layer_render::PixelFormat::Rgba8Srgb,
                bytes: unsafe { std::slice::from_raw_parts(rgba, count) },
            },
        )
    })
    .map_or(-1, |_| 0)
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
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if records.is_null()
            || count == 0
            || count > 8192 * 9
            || tool > 3
            || button > 2
            || predicted > 1
        {
            return Err("Invalid Apple pointer batch".into());
        }
        a.host.pointer_batch(PointerBatch {
            id,
            tool: tool as u8,
            button: button as u8,
            records: unsafe { std::slice::from_raw_parts(records, count) },
            predicted: predicted != 0,
            view_revision,
        })
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
        let (again, timing) = a.metal.frame(&mut a.host, now, presentation)?;
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
