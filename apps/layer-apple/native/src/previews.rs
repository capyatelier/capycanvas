//! Owned picker atlases and live overview layout. No per-channel JSON or GPU wait.
use super::*;
use layer_render::{CanvasRenderer, FilterPreviewImage};

/// # Safety
/// Serial owner call; JSON is an array of logical bounds/clip/order records.
/// The layout is immutable until the next call. No preview pixels cross the ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_placements(
    app: *mut CapyApple,
    json: *const c_char,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        if json.is_null() {
            return Err("Missing Navigator geometry".into());
        }
        let source = unsafe { CStr::from_ptr(json) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if source.len() > 16 * 1024 {
            return Err("Navigator geometry exceeds the layout transport limit".into());
        }
        let slots = serde_json::from_str(source).map_err(|e| e.to_string())?;
        if app.metal.set_overviews(slots)? {
            app.host.dirty = true;
        }
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Stateless geometry on any thread. JSON is [Camera, documentExtent, viewport].
/// The returned JSON is owned and uses the ordinary string-free function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_geometry(json: *const c_char) -> *mut c_char {
    catch_unwind(|| {
        let result = (|| {
            if json.is_null() {
                return Err("Missing Navigator geometry".to_string());
            }
            let source = unsafe { CStr::from_ptr(json) }
                .to_str()
                .map_err(|e| e.to_string())?;
            let (camera, document, viewport): (layer_ui::Camera, [u32; 2], [f32; 2]) =
                serde_json::from_str(source).map_err(|e| e.to_string())?;
            serde_json::to_value(layer_ui::NavigatorGeometry::new(
                &camera, document, viewport,
            ))
            .map_err(|e| e.to_string())
        })();
        let value = result.unwrap_or_else(|error| serde_json::json!({"error":error}));
        CString::new(value.to_string())
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}

pub struct CapyFilterPreviews {
    atlas: FilterPreviewImage,
    filters: CString,
}

#[repr(C)]
pub struct CapyFilterPreviewInfo {
    pub request: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: *const u8,
    pub count: usize,
    pub filters: *const c_char,
}

/// # Safety
/// Call on the serial editor owner. The returned allocation has no editor/GPU
/// references and can be read and freed on an image worker after editor teardown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_take_filter_previews(
    app: *mut CapyApple,
) -> *mut CapyFilterPreviews {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|app| {
        let renderer = app.host.session.renderer_mut();
        if let Some(gpu) = &renderer.0 {
            gpu.device()
                .poll(wgpu::PollType::Poll)
                .map_err(|e| e.to_string())?;
        }
        let Some(result) = renderer.take_filter_previews() else {
            return Ok(std::ptr::null_mut());
        };
        let atlas = result.map_err(|e| e.to_string())?;
        let filters =
            CString::new(serde_json::to_string(&atlas.filters).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        Ok(Box::into_raw(Box::new(CapyFilterPreviews {
            atlas,
            filters,
        })))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Both pointers must be valid. Output views are borrowed until previews_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews_read(
    previews: *const CapyFilterPreviews,
    output: *mut CapyFilterPreviewInfo,
) {
    let (Some(previews), Some(output)) = (unsafe { previews.as_ref() }, unsafe { output.as_mut() })
    else {
        return;
    };
    let image = &previews.atlas.image;
    *output = CapyFilterPreviewInfo {
        request: image.request_id,
        width: image.width,
        height: image.height,
        stride: image.stride,
        pixels: image.bytes.as_ptr(),
        count: image.bytes.len(),
        filters: previews.filters.as_ptr(),
    };
}

/// # Safety
/// Free exactly once, after all borrowed views have finished reading.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews_free(previews: *mut CapyFilterPreviews) {
    if !previews.is_null() {
        unsafe { drop(Box::from_raw(previews)) };
    }
}
