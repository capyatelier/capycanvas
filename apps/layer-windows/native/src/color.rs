//! Stateless native color presentation. CPU pixels never touch a canvas host.
use layer_ui::{
    ColorAction, ColorPanelLayout, ColorShape, ColorState, ColorWheelGeometry, ColorWheelPart,
};
use std::ffi::{CString, c_char};

/// Stateless shared numeric parsing and display projection. No document host is accessed.
/// # Safety
/// The input must be a readable, NUL-terminated UTF-8 JSON string for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_ui(input: *const c_char) -> *mut c_char {
    if input.is_null() { return std::ptr::null_mut(); }
    let result = std::panic::catch_unwind(|| {
        let input = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        if input.len() > 256 * 1024 { return Err("Color request is too large".into()); }
        layer_ui::color_ui(serde_json::from_str(input).map_err(|e| e.to_string())?)
    }).unwrap_or_else(|_| Err("Color request failed".into()));
    json(match result { Ok(value) => value, Err(error) => serde_json::json!({"error": error}) })
}

/// Shared export form normalization; file validation happens on the worker.
/// # Safety
/// input is a readable NUL-terminated UTF-8 JSON string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_export_draft(input: *const c_char) -> *mut c_char {
    if input.is_null() { return std::ptr::null_mut(); }
    let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct Request { recipe: layer_ui::ExportRecipe, action: layer_ui::ExportDraftAction, #[serde(default)] validate: bool, extent: Option<[u32;2]>, color: Option<layer_core::color::DocumentColor> }
        let text = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().map_err(|e| e.to_string())?;
        let request: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let draft = if let Some(color) = request.color {
            request.recipe.draft_for_color(color, request.action)
        } else { request.recipe.draft(request.action) };
        if request.validate {
            draft.recipe.validate()?;
            draft.recipe.size.extent(request.extent.ok_or("Export extent is missing")?)?;
            draft.recipe.output_resolution(None)?;
        }
        serde_json::to_value(draft).map_err(|e| e.to_string())
    }).unwrap_or_else(|_| Err("Export form failed".into()));
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

/// Return shared logical bounds; free a nonnull result with capy_string_free.
#[unsafe(no_mangle)]
pub extern "C" fn capy_color_layout(size: f32) -> *mut c_char {
    ColorPanelLayout::new(size).map_or(std::ptr::null_mut(), json)
}

/// Return projection-specific display hue stops; free with capy_string_free.
#[unsafe(no_mangle)]
pub extern "C" fn capy_color_hue_stops(projection: u32) -> *mut c_char {
    let Some(shape) = shape(projection) else {
        return std::ptr::null_mut();
    };
    let mut color = ColorState::default();
    if color.apply(ColorAction::Shape { shape }).is_err() {
        return std::ptr::null_mut();
    }
    json(color.wheel_hue_stops())
}

/// Shared SDR projection of the active HDR picker, including its EV and recipe.
/// # Safety
/// Input is a readable C string; output is writable for exactly length bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_mapped_field(side: u32, input: *const c_char, output: *mut u8, length: usize) -> bool {
    if input.is_null() || output.is_null() || !(1..=2048).contains(&side) || length != side as usize * side as usize * 4 { return false; }
    std::panic::catch_unwind(|| {
        #[derive(serde::Deserialize)]
        struct Request { state: ColorState, rendition: layer_core::color::hdr::SdrRendition }
        let text = unsafe { std::ffi::CStr::from_ptr(input) }.to_str().ok()?;
        if text.len()>256*1024 { return None; }
        let request: Request=serde_json::from_str(text).ok()?;
        request.rendition.validate().ok()?;
        Some(request.state.render_field_mapped(side, request.rendition, unsafe { std::slice::from_raw_parts_mut(output,length) }))
    }).ok().flatten().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

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
