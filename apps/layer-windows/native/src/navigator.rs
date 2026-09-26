//! Native UI geometry only. The shared presenter samples the live GPU composition.

/// Shared image bounds for the transparent native UI cutout. No live host access.
///
/// # Safety
/// `output` must point to four writable, aligned floats for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_navigator_image(
    width: f32,
    height: f32,
    document_width: u32,
    document_height: u32,
    output: *mut f32,
) -> bool {
    if output.is_null() {
        return false;
    }
    let document = [document_width, document_height];
    let camera = layer_ui::Camera::new(document, document);
    let Some(g) = layer_ui::NavigatorGeometry::new(&camera, document, [width, height]) else {
        return false;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            [g.image.x, g.image.y, g.image.width, g.image.height].as_ptr(),
            output,
            4,
        );
    }
    true
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_navigator_aspect(document_width: u32, document_height: u32) -> f32 {
    layer_ui::NavigatorGeometry::overview_aspect([document_width, document_height])
}
