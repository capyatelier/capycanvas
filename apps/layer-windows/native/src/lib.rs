//! Windows application adapter. After UI-thread surface initialization, the
//! canvas thread exclusively owns this object; UI callbacks only enqueue work.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(any(target_os = "windows", test))]
mod document_io;
#[cfg(any(target_os = "windows", test))]
mod documents;
mod events;
#[cfg(any(target_os = "windows", test))]
mod settings;
pub use events::CapyPointer;
#[cfg(target_os = "windows")]
mod host;
#[cfg(target_os = "windows")]
pub use host::*;

#[unsafe(no_mangle)]
pub extern "C" fn capy_color_hit(x: f32, y: f32, size: f32, space: u32) -> u32 {
    let space = match space {
        0 => layer_ui::ColorSpace::Hsv,
        1 => layer_ui::ColorSpace::Hls,
        _ => return 0,
    };
    match layer_ui::ColorWheelGeometry::new(size).and_then(|g| g.hit([x, y], space)) {
        None => 0,
        Some(layer_ui::ColorWheelPart::Hue) => 1,
        Some(layer_ui::ColorWheelPart::Field) => 2,
    }
}

#[cfg(test)]
mod color_bridge_tests {
    use super::capy_color_hit;
    #[test]
    fn hit_bridge_distinguishes_spaces_and_rejects_invalid_coordinates() {
        assert_eq!(capy_color_hit(100., 8., 200., 0), 1);
        assert_eq!(capy_color_hit(100., 100., 200., 0), 2);
        assert_eq!(capy_color_hit(60., 60., 200., 0), 2);
        assert_eq!(capy_color_hit(60., 60., 200., 1), 0);
        assert_eq!(capy_color_hit(100., 100., 200., 1), 2);
        for (x, y, size, space) in [
            (0., 0., 200., 0),
            (f32::NAN, 100., 200., 0),
            (100., f32::INFINITY, 200., 0),
            (100., 100., 0., 0),
            (100., 100., 200., 2),
        ] {
            assert_eq!(capy_color_hit(x, y, size, space), 0);
        }
    }
}
