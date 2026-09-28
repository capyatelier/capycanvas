// Prediction evaluates the existing contact/coverage model at display texel
// centres. Its starting paint and stroke coverage average the same footprint.
// This is a presentation approximation; exact queries replay the retained tail.
@group(0) @binding(1) var material_color_output: texture_storage_2d<rgba32float, write>;

fn preview_origin(p: vec2<i32>) -> vec2<i32> {
    let side = i32(style.operation.z);
    return p / side * side;
}
fn preview_size(start: vec2<i32>) -> vec2<i32> {
    let remaining = vec2<i32>(style.canvas_opacity.xy - render_target.origin_extent.xy) - start;
    return clamp(remaining, vec2<i32>(0), vec2<i32>(i32(style.operation.z)));
}
fn dry_original(p: vec2<i32>) -> vec4<f32> {
    if all(textureDimensions(source_11) == vec2<u32>(1u)) { return vec4<f32>(0.); }
    let start = preview_origin(p);
    let size = preview_size(start);
    var sum = vec4<f32>(0.);
    for (var y = 0; y < size.y; y++) {
        for (var x = 0; x < size.x; x++) {
            sum += textureLoad(source_11, start + vec2<i32>(x, y), 0);
        }
    }
    return sum / f32(max(size.x * size.y, 1));
}
fn dry_coverage(p: vec2<i32>) -> f32 {
    if all(textureDimensions(stroke_coverage_texture) == vec2<u32>(1u)) { return 0.; }
    let start = preview_origin(p);
    let size = preview_size(start);
    var sum = 0.;
    for (var y = 0; y < size.y; y++) {
        for (var x = 0; x < size.x; x++) {
            sum += textureLoad(stroke_coverage_texture, start + vec2<i32>(x, y), 0).r;
        }
    }
    return sum / f32(max(size.x * size.y, 1));
}
