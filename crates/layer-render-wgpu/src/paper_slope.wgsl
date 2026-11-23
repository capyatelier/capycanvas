struct PaperSlope { scale: vec4<f32> }
@group(0) @binding(0) var<uniform> slope_parameters: PaperSlope;
@group(0) @binding(1) var slope_source: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn slope_texel(q: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(slope_source));
    return textureLoad(slope_source, ((q % size) + size) % size, 0);
}

@fragment
fn paper_slope(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(floor(position.xy));
    let h = array<f32, 9>(
        slope_texel(q + vec2(-1, -1)).r, slope_texel(q + vec2(0, -1)).r, slope_texel(q + vec2(1, -1)).r,
        slope_texel(q + vec2(-1, 0)).r, 0., slope_texel(q + vec2(1, 0)).r,
        slope_texel(q + vec2(-1, 1)).r, slope_texel(q + vec2(0, 1)).r, slope_texel(q + vec2(1, 1)).r);
    let x = (h[2] + 2. * h[5] + h[8]) - (h[0] + 2. * h[3] + h[6]);
    let y = (h[6] + 2. * h[7] + h[8]) - (h[0] + 2. * h[1] + h[2]);
    return vec4<f32>(vec2<f32>(x, y) * (slope_parameters.scale.x / 8.), 0., 1.);
}

@fragment
fn paper_slope_mip(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let source = vec2<i32>(textureDimensions(slope_source));
    let reduced = max(source / 2, vec2(1));
    let q = vec2<i32>(floor(position.xy));
    if all(source == reduced * 2) {
        let p = q * 2;
        let sum = slope_texel(p).xy + slope_texel(p + vec2(1, 0)).xy + slope_texel(p + vec2(0, 1)).xy + slope_texel(p + vec2(1, 1)).xy;
        return vec4<f32>(sum * .25, 0., 1.);
    }
    let low = vec2<f32>(q * source) / vec2<f32>(reduced);
    let high = vec2<f32>((q + 1) * source) / vec2<f32>(reduced);
    var sum = vec2(0.);
    for (var y = i32(floor(low.y)); f32(y) < high.y; y++) {
        let height = min(high.y, f32(y + 1)) - max(low.y, f32(y));
        for (var x = i32(floor(low.x)); f32(x) < high.x; x++) {
            sum += slope_texel(vec2(x, y)).xy * (min(high.x, f32(x + 1)) - max(low.x, f32(x))) * height;
        }
    }
    return vec4<f32>(sum / ((high.x - low.x) * (high.y - low.y)), 0., 1.);
}
