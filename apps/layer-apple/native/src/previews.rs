//! Small owned picker atlases. No per-channel JSON and no blocking GPU wait.
use super::*;
use layer_render::{CanvasRenderer, FilterPreviewImage};

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
