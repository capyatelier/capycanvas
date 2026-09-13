//! Stateless native color presentation. CPU pixels never touch a canvas host.
use layer_ui::{
    ColorAction, ColorPanelLayout, ColorShape, ColorState, ColorWheelGeometry, ColorWheelPart,
};
use std::ffi::{CString, c_char};

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

/// Write display-encoded RGBA8 into a caller-owned field raster.
/// # Safety
/// A nonnull output must be writable for length bytes with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_field(
    side: u32,
    hue: f32,
    projection: u32,
    output: *mut u8,
    length: usize,
) -> bool {
    if !(1..=2048).contains(&side)
        || !hue.is_finite()
        || output.is_null()
        || length != side as usize * side as usize * 4
        || !matches!(
            shape(projection),
            Some(ColorShape::Circle | ColorShape::Triangle)
        )
    {
        return false;
    }
    let rgba = unsafe { std::slice::from_raw_parts_mut(output, length) };
    if projection == 2 {
        layer_ui::render_okhsv_disc(side, hue, rgba)
    } else {
        layer_ui::render_hls_field(side, hue, rgba)
    }
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
        for (side, hue, projection, length) in [
            (0, 60., 2, 16),
            (2049, 60., 2, 16),
            (2, f32::NAN, 2, 16),
            (2, 60., 2, 15),
            (2, 60., 0, 16),
            (2, 60., 3, 16),
        ] {
            assert!(!unsafe {
                capy_color_field(side, hue, projection, bytes.as_mut_ptr(), length)
            });
            assert_eq!(bytes, [93; 16]);
        }
        assert!(!unsafe { capy_color_field(2, 60., 2, std::ptr::null_mut(), 16) });
        assert!(unsafe { capy_color_field(2, 60., 2, bytes.as_mut_ptr(), 16) });
        assert!(bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
}
