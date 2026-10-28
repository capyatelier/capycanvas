//! Stateless Rust geometry for retained native Proof and drawing controls.
use std::ffi::{CStr, CString, c_char};
pub struct CapyLocalization {
    pub(crate) localizer: std::sync::Arc<layer_ui::Localizer>,
}
pub(crate) fn store_localization(localizer:std::sync::Arc<layer_ui::Localizer>)->u64 {
    layer_host::handles::insert_shared(std::sync::Arc::new(CapyLocalization {localizer})).map_or(0,layer_host::handles::Handle::raw)
}
pub(crate) fn localization(value:u64)->Option<std::sync::Arc<CapyLocalization>> {
    layer_host::handles::Handle::from_raw(value).and_then(layer_host::handles::shared::<CapyLocalization>).ok()
}
/// # Safety
/// The input is a readable NUL-terminated shipped language tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_localization_for_tag(tag: *const c_char) -> u64 {
    if tag.is_null() { return 0; }
    let Ok(tag) = unsafe { CStr::from_ptr(tag) }.to_str() else { return 0; };
    let Some(language) = layer_ui::UiLanguage::ALL.into_iter().find(|language| language.tag() == tag) else { return 0; };
    store_localization(layer_ui::Localizer::shared(language))
}
/// # Safety
/// Free a uniquely owned immutable context after its final borrowed call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_localization_free(context: u64) {
    let _=layer_host::handles::Handle::from_raw(context).and_then(layer_host::handles::release::<CapyLocalization>);
}
/// # Safety
/// The key is a readable NUL-terminated UTF-8 shared key name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_control_takes_key(key: *const c_char, command: bool, alt: bool, keyboard_focus: bool) -> bool {
    if key.is_null() { return false; }
    let Ok(key) = unsafe { CStr::from_ptr(key) }.to_str() else { return false };
    layer_ui::control_takes_key(key, layer_ui::Modifiers { command, shift: false, alt }, keyboard_focus)
}
/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_edge_reach(input: *const c_char) -> u32 {
    #[derive(serde::Deserialize)]
    struct Request {
        layout: layer_ui::ResolvedLayout,
        position: [f32; 2],
        kind: layer_ui::PointerKind,
        contact: layer_ui::EdgeContact,
        #[serde(default)]
        reset: bool,
    }
    if input.is_null() {
        return 0;
    }
    std::panic::catch_unwind(|| {
        let request: Request = serde_json::from_str(unsafe { CStr::from_ptr(input) }.to_str().ok()?).ok()?;
        Some(if request.reset {
            u32::from(request.contact == layer_ui::EdgeContact::Free
                && request.layout.edge_reset_at(request.position, request.kind).is_some())
        } else {
            request.layout.edge_grab_at(request.position, request.kind, request.contact)
                .map_or(0, |grab| 1 | u32::from(grab.resizes(layer_ui::Axis::Vertical)) << 1)
        })
    })
    .ok()
    .flatten()
    .unwrap_or(0)
}
/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_proof_dial(input: *const c_char) -> u64 {
    if input.is_null() {
        return 0;
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
    .map_or(0, crate::owned::text)
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
pub unsafe extern "C" fn capy_toolbar_ui(context: u64, input: *const c_char) -> u64 {
    if input.is_null() {
        return 0;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<serde_json::Value, String> {
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if text.len() > 64 * 1024 {
            return Err("Toolbar request is too large".into());
        }
        let context = localization(context).ok_or("Missing toolbar localization")?;
        layer_ui::toolbar_ui(serde_json::from_str(text).map_err(|e| e.to_string())?, &context.localizer)
    }))
    .unwrap_or_else(|_| Err("Toolbar request failed".into()));
    CString::new(
        result
            .unwrap_or_else(|error| serde_json::json!({"error":error}))
            .to_string(),
    )
    .map_or(0, crate::owned::text)
}

/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_native_caption(context: u64, input: *const c_char) -> u64 {
    unsafe { localized_json(context, input, |source, localization| {
        let request: layer_ui::NativeCaption = serde_json::from_str(source).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"text": request.message(localization)}))
    }) }
}
/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_numeric_labels(context: u64, input: *const c_char) -> u64 {
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
pub unsafe extern "C" fn capy_header_frame(context:u64,input:*const c_char)->u64 {
    unsafe { localized_json(context,input,|source,_|layer_ui::header_frame(source)) }
}
/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_drawer_frame(context: u64, input: *const c_char) -> u64 {
    unsafe { localized_json(context, input, |source, _| layer_ui::drawer_frame(source)) }
}
/// # Safety
/// Both pointers remain readable for this call; the context remains immutable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_bubble_ui(context: u64, input: *const c_char) -> u64 {
    unsafe { localized_json(context, input, |source, _| layer_host::bubbles::query(
        serde_json::from_str(source).map_err(|error| error.to_string())?,
    )) }
}
unsafe fn localized_json(context: u64, input: *const c_char,
    resolve: impl FnOnce(&str, &layer_ui::Localizer) -> Result<serde_json::Value, String> + std::panic::UnwindSafe,
) -> u64 {
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = localization(context).ok_or("Missing launch localization")?;
        if input.is_null() { return Err("Missing localized view request".into()); }
        let source = unsafe { CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        if source.len() > 64 * 1024 { return Err("Localized view request is too large".into()); }
        resolve(source, &context.localizer)
    })).unwrap_or_else(|_| Err("Localized view request failed".into()));
    CString::new(value.unwrap_or_else(|error| serde_json::json!({"error":error})).to_string())
        .map_or(0, crate::owned::text)
}

/// Native layout projection uses the same compact threshold as other hosts.
#[unsafe(no_mangle)]
pub extern "C" fn capy_document_tabs_compact(width: f32, count: usize) -> bool {
    layer_ui::DocumentTabs::compact(width, count)
}
/// # Safety
/// Input is readable NUL-terminated JSON, released after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_tab_slide(input: *const c_char) -> u64 {
    if input.is_null() {
        return 0;
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
        .map_or(0, crate::owned::text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_geometry_ffi_is_stateless_and_uses_the_supplied_model() {
        let context=crate::test_support::Localization::new(layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        for (width,documents,size) in [(1600.,1,layer_ui::HeaderSize::Large),(640.,3,layer_ui::HeaderSize::Small)] {
            let mut model=layer_ui::HeaderLayout::default();model.size=size;
            let request=serde_json::json!({"op":"geometry","model":model,"editing":false,"documents":documents,
                "width":width,"insets":[0.,168.],"metrics":[]}).to_string();
            let input=CString::new(request.clone()).unwrap();
            let response=unsafe {capy_header_frame(context.raw(),input.as_ptr())};
            let actual:serde_json::Value=serde_json::from_str(crate::owned::take_text(response).to_str().unwrap()).unwrap();
            assert_eq!(actual,layer_ui::header_frame(&request).unwrap());
        }
        let response=unsafe {capy_header_frame(context.raw(),std::ptr::null())};
        let error:serde_json::Value=serde_json::from_str(crate::owned::take_text(response).to_str().unwrap()).unwrap();
        assert!(error.get("error").is_some());
    }
    #[test]
    fn button_activation_keys_follow_the_shared_focus_rule() {
        let key = |name: &str, keyboard| unsafe { capy_control_takes_key(CString::new(name).unwrap().as_ptr(), false, false, keyboard) };
        assert!(key(" ", true) && !key(" ", false) && key("enter", false));
        assert!(!unsafe { capy_control_takes_key(std::ptr::null(), false, false, true) });
    }
    #[test]
    fn edge_reach_names_a_vertical_gap_and_leaves_swipe_controls_alone() {
        let layout = layer_ui::WorkspacePreset::Illustrator.layout(layer_ui::Platform::Windows).resolve(1600., 1000.);
        let gap = layout.dividers.iter().find(|d| d.axis == layer_ui::Axis::Vertical && !d.band && !d.fixed).unwrap();
        let reach = |position: [f32; 2], contact: &str| {
            let request = serde_json::json!({"layout": layout, "position": position, "kind": "touch", "contact": contact}).to_string();
            unsafe { capy_edge_reach(CString::new(request).unwrap().as_ptr()) }
        };
        let above = [gap.bounds.x + 30., gap.bounds.y - 10.];
        assert_eq!(reach(above, "free") & 3, 3);
        assert_eq!(reach(above, "input"), 0);
        assert_eq!(unsafe { capy_edge_reach(std::ptr::null()) }, 0);
    }
    #[test]
    fn edge_reset_admission_uses_shared_column_ownership_and_contact_class() {
        let layout = layer_ui::WorkspacePreset::Illustrator.layout(layer_ui::Platform::Windows).resolve(1600., 1000.);
        let column = layout.dividers.iter().find(|d| d.axis == layer_ui::Axis::Horizontal
            && d.owner_side == Some(layer_ui::Edge::Left) && !d.fixed
            && layout.work_area.contains(d.bounds.x + d.bounds.width + 20., d.bounds.y + d.bounds.height * 0.5)).unwrap();
        let point = [column.bounds.x + column.bounds.width + 20., column.bounds.y + column.bounds.height * 0.5];
        for (kind, contact, expected) in [("touch", "free", 1), ("mouse", "free", 0), ("touch", "input", 0), ("touch", "pickup", 0)] {
            let request = serde_json::json!({"layout": layout, "position": point, "kind": kind, "contact": contact, "reset": true}).to_string();
            assert_eq!(unsafe { capy_edge_reach(CString::new(request).unwrap().as_ptr()) }, expected);
        }
    }
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
        serde_json::from_str(crate::owned::take_text(reply).to_str().unwrap()).unwrap()
    }
    #[test]
    fn toolbar_number_ffi_preserves_the_supplied_context_and_shared_expression_policy() {
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let context = crate::test_support::Localization::new(localization.clone());
        let invoke = |text: &str| {
            let input = CString::new(serde_json::json!({
                "type": "number", "compact": false, "units": false,
                "request": { "control": layer_ui::NumericControl::number(0., 100., 1., 0),
                    "value": 0., "operation": { "type": "expression", "text": text } }
            }).to_string()).unwrap();
            let reply = unsafe { capy_toolbar_ui(context.raw(), input.as_ptr()) };
            assert_ne!(reply,0);
            let owned = crate::owned::take_text(reply);
            serde_json::from_str::<serde_json::Value>(owned.to_str().unwrap()).unwrap()
        };
        assert_eq!(invoke("12+3")["value"], 15.);
        assert_eq!(invoke("１２＋３")["error"], layer_ui::NumericError::InvalidExpression.message(&localization));
        assert_eq!(invoke("{ $literal } 日本語")["error"], layer_ui::NumericError::InvalidExpression.message(&localization));
        assert!(std::sync::Arc::ptr_eq(&context.localizer, &localization));
    }
    #[test]
    fn caption_ffi_retains_context_and_literal_user_text() {
        let l = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let context = crate::test_support::Localization::new(l.clone());
        let invoke = |f: unsafe extern "C" fn(u64, *const c_char) -> u64, request: serde_json::Value| {
            let input = CString::new(request.to_string()).unwrap();
            let reply = unsafe { f(context.raw(), input.as_ptr()) };
            assert_ne!(reply,0);
            let text = crate::owned::take_text(reply);
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
