//! Stateless native color presentation. CPU pixels never touch a canvas host.
use layer_ui::{ColorShape, ColorState, ColorWheelGeometry, ColorWheelPart};
use std::ffi::{CString, c_char};
use crate::shared_controls::CapyLocalization;

/// Stateless shared numeric parsing and display projection. No document host is accessed.
/// # Safety
/// The input must be a readable, NUL-terminated UTF-8 JSON string for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_ui(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    if input.is_null() { return std::ptr::null_mut(); }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let input = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        if input.len() > 256 * 1024 { return Err("Color request is too large".into()); }
        let context = unsafe { context.as_ref() }.ok_or("Missing color localization")?;
        layer_ui::color_ui_localized(serde_json::from_str(input).map_err(|e| e.to_string())?, &context.localizer)
    })).unwrap_or_else(|_| Err("Color request failed".into()));
    json(match result { Ok(value) => value, Err(error) => serde_json::json!({"error": error}) })
}

/// Shared export form normalization; file validation happens on the worker.
/// # Safety
/// input is a readable NUL-terminated UTF-8 JSON string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_export_draft(context: *const CapyLocalization, input: *const c_char) -> *mut c_char {
    if input.is_null() { return std::ptr::null_mut(); }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct DraftRequest { recipe: layer_ui::ExportRecipe, action: layer_ui::ExportDraftAction, #[serde(default)] validate: bool, extent: Option<[u32;2]>, color: Option<layer_core::color::DocumentColor> }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct MetadataCopy { format: layer_ui::ExportFormat, keep: layer_ui::MetadataKeep }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CopyRequest { choices: Option<layer_ui::ExportChoices>, error_reason: Option<layer_ui::ColorFeatureError>, profile_captions: Option<Vec<layer_ui::ExportProfileCaption>>, metadata: Option<MetadataCopy> }
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Request { Draft(DraftRequest), Copy { copy: CopyRequest } }
        let text = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        let context = unsafe { context.as_ref() }.ok_or("Missing export localization")?;
        let request: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let request = match request {
            Request::Draft(request) => request,
            Request::Copy { copy } => {
                if copy.choices.as_ref().is_some_and(|choices| choices.formats.len()>11 || choices.depths.len()>4 || choices.backgrounds.len()>3 || choices.dithers.len()>2) { return Err("Export choices are too large".into()); }
                if copy.profile_captions.as_ref().is_some_and(|values| values.len()>layer_ui::profile_library::PROFILE_LIBRARY_ENTRIES+16) { return Err("Export captions are too large".into()); }
                return Ok(serde_json::json!({"metadata":copy.metadata.map(|metadata|layer_ui::ExportMetadataView::localized_for(metadata.format,metadata.keep,&context.localizer)),"choices":copy.choices.map(|choices|choices.localized(&context.localizer)),
                    "error":copy.error_reason.as_ref().map(|reason|reason.message(&context.localizer)),"error_reason":copy.error_reason,
                    "profile_names":copy.profile_captions.map(|captions|captions.iter().map(|caption|caption.message(&context.localizer)).collect::<Vec<_>>())}));
            },
        };
        let draft = if let Some(color) = request.color {
            request.recipe.draft_for_color_localized(color, request.action, &context.localizer)
        } else { request.recipe.draft_localized(request.action, &context.localizer) };
        if request.validate {
            let extent = request.extent.ok_or("Export extent is missing")?;
            let validation = draft.recipe.validate().and_then(|_| draft.recipe.output_extent(extent).map(|_| ())).and_then(|_| draft.recipe.output_resolution(None).map(|_| ()));
            if let Err(reason) = validation { return Ok(serde_json::json!({"error":reason.message(&context.localizer),"error_reason":reason})); }
        }
        serde_json::to_value(draft).map_err(|e| e.to_string())
    })).unwrap_or_else(|_| Err("Export form failed".into()));
    json(result.unwrap_or_else(|error| serde_json::json!({"error":error})))
}
/// Shared picker pixels in the document's RGB coordinates, projected to sRGB.
/// # Safety
/// output is writable for length bytes for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_raster(side: u32, hue: f32, projection: u32, space: u32, guide: bool, output: *mut u8, length: usize) -> bool {
    use layer_core::color::RgbSpace;
    let (Some(shape), Some(space)) = (shape(projection), RgbSpace::ALL.get(space as usize).copied()) else { return false; };
    if !(1..=2048).contains(&side) || !hue.is_finite() || output.is_null() || length != side as usize * side as usize * 4 { return false; }
    let bytes = unsafe { std::slice::from_raw_parts_mut(output, length) };
    if guide { layer_ui::render_hue_guide_in(side, shape, space, RgbSpace::Srgb, bytes) }
    else { layer_ui::render_color_field(side, shape, hue, space, RgbSpace::Srgb, bytes) }
}
fn shape(value: u32) -> Option<ColorShape> {
    match value {
        0 => Some(ColorShape::Square),
        1 => Some(ColorShape::Triangle),
        2 => Some(ColorShape::Circle),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_color_hit(x: f32, y: f32, size: f32, projection: u32) -> u32 {
    match shape(projection).and_then(|shape| {
        ColorWheelGeometry::new(size).and_then(|geometry| geometry.hit_shape([x, y], shape))
    }) {
        None => 0,
        Some(ColorWheelPart::Hue) => 1,
        Some(ColorWheelPart::Field) => 2,
    }
}

fn json(value: impl serde::Serialize) -> *mut c_char {
    serde_json::to_string(&value)
        .ok()
        .and_then(|text| CString::new(text).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Shared SDR projection of the active HDR picker, including its EV and recipe.
/// # Safety
/// Input is a readable C string; output is writable for exactly length bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_mapped_field(side: u32, input: *const c_char, output: *mut u8, length: usize) -> bool {
    if input.is_null() || output.is_null() || !(1..=2048).contains(&side) || length != side as usize * side as usize * 4 { return false; }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        #[derive(serde::Deserialize)]
        struct Request { state: ColorState, rendition: layer_core::color::hdr::SdrRendition }
        let text = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().ok()?;
        if text.len()>256*1024 { return None; }
        let request: Request=serde_json::from_str(text).ok()?;
        request.rendition.validate().ok()?;
        Some(request.state.render_field_mapped(side, request.rendition, unsafe { std::slice::from_raw_parts_mut(output,length) }))
    })).ok().flatten().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export_reply(context: &CapyLocalization, input: &serde_json::Value) -> serde_json::Value {
        let input = CString::new(input.to_string()).unwrap();
        let raw = unsafe { capy_export_draft(context, input.as_ptr()) };
        assert!(!raw.is_null());
        let output = unsafe { CString::from_raw(raw) };
        serde_json::from_slice(output.as_bytes()).unwrap()
    }

    #[test]
    fn export_validation_retains_a_bounded_reason_across_every_language() {
        let mut recipe = layer_ui::ExportRecipe::web_share();
        recipe.profile.name = "Tiếng Việt İı ไทย {literal}".into();
        recipe.profile.profile = layer_core::color::ColorProfile::Icc(vec![19;8192].into());
        recipe.jpeg_quality = 0;
        let retained = serde_json::to_value(&recipe).unwrap();
        let context = CapyLocalization { localizer: layer_ui::Localizer::shared(layer_ui::UiLanguage::English) };
        let failed = export_reply(&context, &serde_json::json!({"recipe":retained,"action":{"type":"refresh"},"validate":true,"extent":[64,64]}));
        assert_eq!(failed["error_reason"], "ExportQuality");
        let copy = serde_json::json!({"copy":{"error_reason":failed["error_reason"]}});
        assert!(copy.to_string().len()<80);
        for language in layer_ui::UiLanguage::ALL {
            let context = CapyLocalization { localizer: layer_ui::Localizer::shared(language) };
            let projected = export_reply(&context,&copy);
            assert_eq!(projected["error_reason"],failed["error_reason"]);
            assert_eq!(projected["error"],layer_ui::ColorFeatureError::ExportQuality.message(&context.localizer));
            assert!(!projected.as_object().unwrap().contains_key("recipe"));
            assert_eq!(serde_json::to_value(&recipe).unwrap(),retained);
        }
    }

    #[test]
    fn export_copy_relabels_scalar_choices_and_preserves_literal_profile_names() {
        let draft = layer_ui::ExportRecipe::web_share().draft_canonical(layer_ui::ExportDraftAction::Refresh);
        let choices = serde_json::to_value(&draft.choices).unwrap();
        let literal = "Embedded ICC profile · Tiếng Việt İı ไทย {literal}";
        let input = serde_json::json!({"copy":{"choices":choices,"profile_captions":[{"type":"literal","name":literal},{"type":"embedded"}]}});
        assert!(input.to_string().len()<4096);
        for language in layer_ui::UiLanguage::ALL {
            let context = CapyLocalization { localizer: layer_ui::Localizer::shared(language) };
            let projected = export_reply(&context,&input);
            assert_eq!(projected["choices"],serde_json::to_value(draft.choices.localized(&context.localizer)).unwrap());
            assert_eq!(projected["profile_names"][0],literal);
            assert_eq!(projected["profile_names"][1],context.localizer.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).as_ref());
            assert!(!projected.as_object().unwrap().contains_key("recipe"));
        }
        let context = CapyLocalization { localizer: layer_ui::Localizer::shared(layer_ui::UiLanguage::English) };
        assert!(export_reply(&context,&serde_json::json!({"copy":{"recipe":draft.recipe}}))["error"].is_string());
    }

    #[test]
    fn export_metadata_copy_keeps_scalar_selection_and_uses_current_copy() {
        for language in layer_ui::UiLanguage::ALL {
            let context = CapyLocalization { localizer: layer_ui::Localizer::shared(language) };
            for (format, keep, location, available) in [("Jpeg","All",true,true),("Jpeg","CopyrightContact",false,true),("Exr","All",false,false)] {
                let input=serde_json::json!({"copy":{"metadata":{"format":format,"keep":keep}}});
                assert!(input.to_string().len()<96);
                let value=export_reply(&context,&input);let metadata=&value["metadata"];
                assert_eq!(metadata["label"],context.localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_METADATA).as_ref());
                assert_eq!(metadata["choices"][0]["value"],"All");
                assert_eq!(metadata["choices"][1]["value"],"CopyrightContact");
                assert_eq!(metadata["choices"][2]["value"],"None");
                assert_eq!(metadata["choices"][0]["label"],context.localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_METADATA_ALL).as_ref());
                assert_eq!(metadata["choices"][1]["label"],context.localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_METADATA_COPYRIGHT).as_ref());
                assert_eq!(metadata["choices"][2]["label"],context.localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_METADATA_NONE).as_ref());
                assert_eq!(metadata["location"],location);assert_eq!(metadata["available"],available);
                assert!(!value.as_object().unwrap().contains_key("recipe"));
            }
        }
    }

    #[test]
    fn hit_distinguishes_projections_and_rejects_invalid_coordinates() {
        for projection in 0..3 {
            assert_eq!(capy_color_hit(100., 8., 200., projection), 1);
            assert_eq!(capy_color_hit(100., 100., 200., projection), 2);
        }
        assert_eq!(capy_color_hit(60., 60., 200., 0), 2);
        assert_eq!(capy_color_hit(60., 60., 200., 1), 0);
        assert_eq!(capy_color_hit(60., 60., 200., 2), 2);
        for (x, y, size, projection) in [
            (0., 0., 200., 0),
            (f32::NAN, 100., 200., 0),
            (100., f32::INFINITY, 200., 0),
            (100., 100., 0., 0),
            (100., 100., 200., 3),
        ] {
            assert_eq!(capy_color_hit(x, y, size, projection), 0);
        }
    }

    #[test]
    fn invalid_raster_requests_leave_the_callers_buffer_untouched() {
        let mut bytes = [93; 16];
        for (side, hue, projection, space, length) in [
            (0, 60., 2, 0, 16),
            (2049, 60., 2, 0, 16),
            (2, f32::NAN, 2, 0, 16),
            (2, 60., 2, 0, 15),
            (2, 60., 3, 0, 16),
            (2, 60., 2, u32::MAX, 16),
        ] {
            assert!(!unsafe {
                capy_color_raster(side, hue, projection, space, false, bytes.as_mut_ptr(), length)
            });
            assert_eq!(bytes, [93; 16]);
        }
        assert!(!unsafe { capy_color_raster(2, 60., 2, 0, false, std::ptr::null_mut(), 16) });
        assert!(unsafe { capy_color_raster(2, 60., 2, 0, false, bytes.as_mut_ptr(), 16) });
        assert!(bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
}
