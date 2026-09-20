//! Stateless Rust geometry for retained native Proof and drawing controls.
use std::ffi::{CStr, CString, c_char};
/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_proof_dial(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let value = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            size: f32,
            recipe: layer_core::color::hdr::SdrRendition,
            point: Option<[f32; 2]>,
        }
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 4096 {
            return Err("Proof geometry request is too large".into());
        }
        let request: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
        layer_ui::proof_panel::sdr_dial(request.size, request.recipe, request.point, None)
    })
    .unwrap_or_else(|_| Err("Proof geometry failed".into()));
    CString::new(
        value
            .unwrap_or_else(|error| serde_json::json!({"error":error}))
            .to_string(),
    )
    .map_or(std::ptr::null_mut(), CString::into_raw)
}
/// # Safety
/// Output is writable for length bytes for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_proof_texture(edge: u32, output: *mut u8, length: usize) -> bool {
    if !(1..=512).contains(&edge) || output.is_null() || length != edge as usize * edge as usize * 4
    {
        return false;
    }
    let bytes = layer_ui::proof_panel::sdr_direction_texture(edge);
    unsafe { std::slice::from_raw_parts_mut(output, length) }.copy_from_slice(&bytes);
    true
}

/// Native layout projection uses the same compact threshold as other hosts.
#[unsafe(no_mangle)]
pub extern "C" fn capy_document_tabs_compact(width: f32, count: usize) -> bool {
    layer_ui::DocumentTabs::compact(width, count)
}
/// # Safety
/// Input is readable NUL-terminated JSON, released after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_tab_drop(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct Request {
            order: Vec<u64>,
            hits: Vec<layer_ui::DocumentTabHit>,
            point: [f32; 2],
            vertical: bool,
        }
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 256 * 1024 {
            return Err("Too many drawing targets".into());
        }
        let r: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(
            layer_ui::DocumentTabs::drop_target_in_order(&r.order, &r.hits, r.point, r.vertical)
                .map_or(
                    serde_json::Value::Null,
                    |before| serde_json::json!({"before":before}),
                ),
        )
    })
    .unwrap_or_else(|_| Err("Drawing target failed".into()));
    CString::new(result.unwrap_or(serde_json::Value::Null).to_string())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
