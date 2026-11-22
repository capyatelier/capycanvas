// Prediction evaluates the existing contact/coverage model at display texel
// centres. Its starting paint and stroke coverage average the same footprint.
// This is a presentation approximation; exact queries replay the retained tail.
@group(0) @binding(2) var material_coverage_output: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var display_original: texture_storage_2d<rgba32float, write>;
@group(0) @binding(5) var display_coverage: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var display_start: texture_storage_2d<rgba32float, write>;

fn dry_texel(p: vec2<i32>) -> vec2<i32> {
    return p >> vec2<u32>(countTrailingZeros(style.operation.z));
}
fn sampled_original(p: vec2<i32>) -> vec4<f32> {
    if style.flags.w > 1.5 { return vec4<f32>(0.); }
    return textureLoad(source_11, dry_texel(p), 0);
}
fn dry_coverage(p: vec2<i32>) -> f32 {
    return textureLoad(stroke_coverage_texture, dry_texel(p), 0).r;
}
fn dry_stroke_start(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(reservoir_texture, dry_texel(p), 0);
}

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
            sum += sample_float(source, brush_sampler, center / vec2<f32>(textureDimensions(source)), 0.)
                * f32(footprint.x * footprint.y);
        }
    }
    return sum / f32(max(size.x * size.y, 1));
}
fn display_average(source: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    if !paint_stored_encoded() { return preview_average(source, p); }
    if all(textureDimensions(source) == vec2<u32>(1u)) { return vec4<f32>(0.); }
    let start = preview_origin(p);
    let size = preview_size(start);
    var sum = vec4<f32>(0.);
    for (var y = 0; y < size.y; y++) {
        for (var x = 0; x < size.x; x++) { sum += working_encode(textureLoad(source, start + vec2<i32>(x, y), 0)); }
    }
    return sum / f32(max(size.x * size.y, 1));
}

@compute @workgroup_size(32, 2)
fn gather_display(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(256u / style.operation.z)) { return; }
    let p = vec2<i32>(id.xy * style.operation.z);
    let texel = vec2<i32>(id.xy);
    if style.flags.w < 1.5 { textureStore(display_original, texel, display_average(source_11, p)); }
    textureStore(display_coverage, texel, vec4<f32>(preview_average(stroke_coverage_texture, p).r, 0., 0., 1.));
    if grouped_opacity() { textureStore(display_start, texel, display_average(reservoir_texture, p)); }
}
