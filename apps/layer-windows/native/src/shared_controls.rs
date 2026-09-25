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

/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_toolbar_ui(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 64 * 1024 {
            return Err("Toolbar request is too large".into());
        }
        layer_ui::toolbar_ui(serde_json::from_str(text).map_err(|e| e.to_string())?)
    })
    .unwrap_or_else(|_| Err("Toolbar request failed".into()));
    CString::new(
        result
            .unwrap_or_else(|error| serde_json::json!({"error":error}))
            .to_string(),
    )
    .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Native layout projection uses the same compact threshold as other hosts.
#[unsafe(no_mangle)]
pub extern "C" fn capy_document_tabs_compact(width: f32, count: usize) -> bool {
    layer_ui::DocumentTabs::compact(width, count)
}
/// # Safety
/// Input is readable NUL-terminated JSON, released after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_tab_slide(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct Request {
            order: Vec<u64>,
            id: u64,
            hits: Vec<layer_ui::DocumentTabHit>,
            clip: layer_ui::Bounds,
            press: [f32; 2],
            point: [f32; 2],
        }
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 256 * 1024 {
            return Err("Too many drawing targets".into());
        }
        let r: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
        layer_ui::DocumentTabDrag::new(&r.order, r.id, r.press, &r.hits, r.clip)
            .and_then(|drag| drag.preview(r.point))
            .map_or(Ok(serde_json::Value::Null), |slide| {
                serde_json::to_value(slide).map_err(|e| e.to_string())
            })
    })
    .unwrap_or_else(|_| Err("Drawing target failed".into()));
    CString::new(result.unwrap_or(serde_json::Value::Null).to_string())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn slide(point: [f32; 2], order: [u64; 3]) -> serde_json::Value {
        let hits: Vec<_> = [1u64, 2, 3]
            .iter()
            .enumerate()
            .map(|(i, id)| serde_json::json!({"id":id,"bounds":{"x":10. + i as f32 * 106.,"y":1.,"width":100.,"height":32.}}))
            .collect();
        let request = serde_json::json!({
            "order": order, "id": 1, "hits": hits, "press": [40., 17.], "point": point,
            "clip": {"x":10.,"y":1.,"width":312.,"height":32.},
        })
        .to_string();
        let input = CString::new(request).unwrap();
        let reply = unsafe { capy_document_tab_slide(input.as_ptr()) };
        let value = serde_json::from_str(unsafe { CStr::from_ptr(reply) }.to_str().unwrap()).unwrap();
        unsafe { crate::capy_string_free(reply) };
        value
    }
    #[test]
    fn drawing_strip_slide_uses_the_shared_gapped_preview() {
        let over = slide([97., 30.], [1, 2, 3]);
        assert_eq!(over["offsets"], serde_json::json!([0., -106., 0.]));
        assert_eq!(over["bounds"]["x"], 67.);
        assert_eq!(over["before"], 3);
        assert_eq!(over["attached"], true);
        let end = slide([500., 17.], [1, 2, 3]);
        assert_eq!(end["offsets"], serde_json::json!([0., -106., -106.]));
        assert!(end["before"].is_null());
        let away = slide([500., 50.], [1, 2, 3]);
        assert_eq!(away["attached"], false);
        assert_eq!(away["offsets"], serde_json::json!([0., 0., 0.]));
        assert!(slide([500., 17.], [2, 1, 3]).is_null());
    }
}
