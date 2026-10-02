//! Stateless Rust geometry for retained native Proof and drawing controls.
use std::ffi::{CStr, CString, c_char};
pub struct CapyLocalization {
    pub(crate) localizer: std::sync::Arc<layer_ui::Localizer>,
}
/// # Safety
/// Free a uniquely owned immutable context after its final borrowed call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_localization_free(context: *mut CapyLocalization) {
    if !context.is_null() { drop(unsafe { Box::from_raw(context) }); }
}
/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_proof_dial(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<serde_json::Value, String> {
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
    }))
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
/// Input is readable NUL-terminated UTF-8 JSON. The immutable context remains
/// alive and unmodified for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_toolbar_ui(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<serde_json::Value, String> {
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 64 * 1024 {
            return Err("Toolbar request is too large".into());
        }
        let context = unsafe { context.as_ref() }.ok_or("Missing toolbar localization")?;
        layer_ui::toolbar_ui(serde_json::from_str(text).map_err(|e| e.to_string())?, &context.localizer)
    }))
    .unwrap_or_else(|_| Err("Toolbar request failed".into()));
    CString::new(
        result
            .unwrap_or_else(|error| serde_json::json!({"error":error}))
            .to_string(),
    )
    .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_native_caption(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    unsafe { localized_json(context, input, |source, localization| {
        let request: layer_ui::NativeCaption = serde_json::from_str(source).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"text": request.message(localization)}))
    }) }
}
/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_numeric_labels(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    unsafe { localized_json(context, input, |source, localization| {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request { label: String }
        let request: Request = serde_json::from_str(source).map_err(|e| e.to_string())?;
        serde_json::to_value(layer_ui::NumericLabels::new(&request.label, localization)).map_err(|e| e.to_string())
    }) }
}
/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_appearance(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    unsafe { localized_json(context, input, |source, localization| {
        let options: layer_ui::NewDocumentOptions = serde_json::from_str(source).map_err(|e| e.to_string())?;
        serde_json::to_value(options.appearance(localization)).map_err(|e| e.to_string())
    }) }
}
unsafe fn localized_json(context: *const CapyLocalization, input: *const c_char,
    resolve: impl FnOnce(&str, &layer_ui::Localizer) -> Result<serde_json::Value, String> + std::panic::UnwindSafe,
) -> *mut c_char {
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { context.as_ref() }.ok_or("Missing launch localization")?;
        if input.is_null() { return Err("Missing localized view request".into()); }
        let source = unsafe { CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        if source.len() > 64 * 1024 { return Err("Localized view request is too large".into()); }
        resolve(source, &context.localizer)
    })).unwrap_or_else(|_| Err("Localized view request failed".into()));
    CString::new(value.unwrap_or_else(|error| serde_json::json!({"error":error})).to_string())
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
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<serde_json::Value, String> {
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
    }))
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
        drop(unsafe { CString::from_raw(reply) });
        value
    }
    #[test]
    fn toolbar_number_ffi_preserves_the_supplied_context_and_shared_expression_policy() {
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let context = CapyLocalization { localizer: localization.clone() };
        let invoke = |text: &str| {
            let input = CString::new(serde_json::json!({
                "type": "number", "compact": false, "units": false,
                "request": { "control": layer_ui::NumericControl::number(0., 100., 1., 0),
                    "value": 0., "operation": { "type": "expression", "text": text } }
            }).to_string()).unwrap();
            let reply = unsafe { capy_toolbar_ui(&context, input.as_ptr()) };
            assert!(!reply.is_null());
            let owned = unsafe { CString::from_raw(reply) };
            serde_json::from_str::<serde_json::Value>(owned.to_str().unwrap()).unwrap()
        };
        assert_eq!(invoke("12+3")["value"], 15.);
        assert_eq!(invoke("１２＋３")["error"], layer_ui::NumericError::InvalidExpression.message(&localization));
        assert_eq!(invoke("{ $literal } 日本語")["error"], layer_ui::NumericError::InvalidExpression.message(&localization));
        assert!(std::sync::Arc::ptr_eq(&context.localizer, &localization));
    }
    #[test]
    fn caption_and_appearance_ffi_retain_context_and_literal_user_text() {
        let l = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let context = CapyLocalization { localizer: l.clone() };
        let invoke = |f: unsafe extern "C" fn(*const CapyLocalization, *const c_char) -> *mut c_char, request: serde_json::Value| {
            let input = CString::new(request.to_string()).unwrap();
            let reply = unsafe { f(&context, input.as_ptr()) };
            assert!(!reply.is_null());
            let text = unsafe { CString::from_raw(reply) };
            serde_json::from_str::<serde_json::Value>(text.to_str().unwrap()).unwrap()
        };
        let title = "{ $name } 日本語🎨\u{2068}literal\u{2069}";
        let result = invoke(capy_native_caption, serde_json::json!({"type":"close_drawing","title":title}));
        assert_eq!(result["text"], layer_ui::NativeCaption::CloseDrawing { title: title.into() }.message(&l));
        assert!(result["text"].as_str().unwrap().contains(title));
        assert!(invoke(capy_native_caption, serde_json::json!({"type":"unknown"}))["error"].is_string());
        let default = invoke(capy_native_caption, serde_json::json!({"type":"shortcut_defaults","keys":[title]}));
        assert!(default["text"].as_str().unwrap().contains(title));
        let graph = invoke(capy_native_caption, serde_json::json!({"type":"inspection_graph","channel":title}));
        assert!(graph["text"].as_str().unwrap().contains(title));
        let counts = invoke(capy_native_caption, serde_json::json!({"type":"inspection_channel","below":17,"above":23,"black":31,"white":41}));
        for count in ["17", "23", "31", "41"] { assert!(counts["text"].as_str().unwrap().contains(count)); }

        let labels = invoke(capy_numeric_labels, serde_json::json!({"label":title}));
        for key in ["edit", "decrease", "increase"] {
            let text = labels[key].as_str().unwrap();
            assert!(text.contains(title));
            assert_eq!(text.matches('\u{2068}').count(), 1);
        }
        assert_ne!(labels["decrease"], format!("Decrease {title}"));
        assert!(invoke(capy_numeric_labels, serde_json::json!({"label":title,"unknown":true}))["error"].is_string());
        let appearance = invoke(capy_document_appearance, serde_json::to_value(layer_ui::NewDocumentOptions::default()).unwrap());
        assert_eq!(appearance["summary"], layer_ui::NewDocumentOptions::default().appearance(&l).summary);
        assert!(std::sync::Arc::ptr_eq(&context.localizer, &l));
    }
    #[test]
    fn drawing_strip_slide_uses_the_shared_gapped_preview() {
        let over = slide([97., 30.], [1, 2, 3]);
        assert_eq!(over["offsets"], serde_json::json!([0., -106., 0.]));
        assert_eq!(over["attached"], true);
        assert!(slide([500., 17.], [2, 1, 3]).is_null());
    }
}
