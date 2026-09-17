// Contacts are generated before retained layer placement. Apply geometry at
// the raster boundary, preserving spacing, sensors and textured footprints.
fn brush_to_layer(p: vec2<f32>) -> vec2<f32> {
    return mat2x2<f32>(style.brush_to_layer_linear.xy, style.brush_to_layer_linear.zw) * p
        + style.brush_to_layer_offset.xy;
}

fn layer_to_brush(p: vec2<f32>) -> vec2<f32> {
    return mat2x2<f32>(style.layer_to_brush_linear.xy, style.layer_to_brush_linear.zw) * p
        + style.layer_to_brush_offset.xy;
}
