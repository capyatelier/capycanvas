// Prediction evaluates the existing contact/coverage model at display texel
// centres. Its starting paint and stroke coverage average the same footprint.
// This is a presentation approximation; exact queries replay the retained tail.

fn preview_origin(p: vec2<i32>) -> vec2<i32> {
    let side = i32(style.operation.z);
    return p / side * side;
}
fn preview_size(start: vec2<i32>) -> vec2<i32> {
    let remaining = vec2<i32>(style.canvas_opacity.xy - render_target.origin_extent.xy) - start;
    return clamp(remaining, vec2<i32>(0), vec2<i32>(i32(style.operation.z)));
}
fn preview_average(source: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    if all(textureDimensions(source) == vec2<u32>(1u)) { return vec4<f32>(0.); }
    let start = preview_origin(p);
    let size = preview_size(start);
    var sum = vec4<f32>(0.);
    for (var y = 0; y < size.y; y += 2) {
        for (var x = 0; x < size.x; x += 2) {
            let footprint = min(vec2<i32>(2), size - vec2<i32>(x, y));
            let center = vec2<f32>(start + vec2<i32>(x, y)) + 0.5 * vec2<f32>(footprint);
            sum += textureSampleLevel(source, brush_sampler, center / vec2<f32>(textureDimensions(source)), 0.)
                * f32(footprint.x * footprint.y);
        }
    }
    return sum / f32(max(size.x * size.y, 1));
}
fn dry_original(p: vec2<i32>) -> vec4<f32> {
    if MATERIAL_PREVIEW_CONTRIBUTION {return vec4(0.);}
    return preview_average(source_11, p);
}
fn dry_coverage(p: vec2<i32>) -> f32 {
    return preview_average(stroke_coverage_texture, p).r;
}
