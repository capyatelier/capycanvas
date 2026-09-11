//! Small owned picker atlases. No per-channel JSON and no blocking GPU wait.
use super::*;
use layer_render::{CanvasRenderer, FilterPreviewImage};

pub struct CapyPreviewImage {
    image: layer_render::ReadbackImage,
    epoch: u64,
}
#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapyNavigatorKey {
    pub epoch: u64,
    pub revision: u64,
}
#[repr(C)]
pub struct CapyPreviewImageInfo {
    pub key: CapyNavigatorKey,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: *const u8,
    pub count: usize,
}

/// # Safety
/// Valid session on its serial owner; output must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_key(
    app: *const CapyApple,
    output: *mut CapyNavigatorKey,
) {
    let (Some(app), Some(output)) = (unsafe { app.as_ref() }, unsafe { output.as_mut() }) else {
        return;
    };
    *output = CapyNavigatorKey {
        epoch: app.host.session.state().document_file.epoch,
        revision: app
            .host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .map_or(0, |gpu| gpu.canvas_preview_revision()),
    };
}

/// # Safety
/// Serial owner call. output is writable. Image ownership can move to a worker.
/// Returns 1 while a map or throttled final update remains, 0 idle, -1 failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_preview(
    app: *mut CapyApple,
    now: u64,
    visible: u32,
    output: *mut *mut CapyPreviewImage,
) -> i32 {
    let (Some(app), Some(output)) = (unsafe { app.as_mut() }, unsafe { output.as_mut() }) else {
        return -1;
    };
    *output = std::ptr::null_mut();
    app.perform(|app| {
        let Some(gpu) = &app.host.session.engine().backend().0 else {
            return Ok(0);
        };
        gpu.device()
            .poll(wgpu::PollType::Poll)
            .map_err(|e| e.to_string())?;
        // A newly adopted document may still have the previous composition on
        // the GPU. Wait for replay before requesting pixels for its epoch.
        if app.host.session.engine().has_pending_document_edits() {
            return Ok(i32::from(visible != 0));
        }
        let pending_before = gpu.canvas_preview_pending();
        let requested_epoch = app.navigator_preview_epoch;
        let visible = visible != 0 && app.host.startup.canvas_ready;
        let image = app.host.session.poll_navigator_preview(now, visible)?;
        let gpu = app.host.session.engine().backend().0.as_ref().unwrap();
        let epoch = app.host.session.state().document_file.epoch;
        if gpu.canvas_preview_pending() && (!pending_before || image.is_some()) {
            app.navigator_preview_epoch = Some(epoch);
        }
        if let Some(image) = image.filter(|_| requested_epoch == Some(epoch)) {
            *output = Box::into_raw(Box::new(CapyPreviewImage { image, epoch }));
        }
        Ok(i32::from(
            visible
                && (gpu.canvas_preview_pending()
                    || !app
                        .host
                        .session
                        .navigator_preview_current(gpu.canvas_preview_revision())),
        ))
    })
    .unwrap_or(-1)
}

/// # Safety
/// Image and output must be valid; returned views are borrowed until image_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_image_read(
    image: *const CapyPreviewImage,
    output: *mut CapyPreviewImageInfo,
) {
    let (Some(image), Some(output)) = (unsafe { image.as_ref() }, unsafe { output.as_mut() })
    else {
        return;
    };
    *output = CapyPreviewImageInfo {
        key: CapyNavigatorKey {
            epoch: image.epoch,
            revision: image.image.request_id,
        },
        width: image.image.width,
        height: image.image.height,
        stride: image.image.stride,
        pixels: image.image.bytes.as_ptr(),
        count: image.image.bytes.len(),
    };
}
/// # Safety
/// Free exactly once, after all borrowed views finish reading.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_image_free(image: *mut CapyPreviewImage) {
    if !image.is_null() {
        unsafe { drop(Box::from_raw(image)) };
    }
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
