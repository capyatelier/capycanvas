@group(0) @binding(0) var source: texture_2d<f32>;
// Original tile dimensions and the number of original pixels per source texel.
@group(0) @binding(1) var<uniform> footprint: vec4<u32>;

@vertex fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0., 1.);
}
@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let start = vec2<u32>(position.xy) * 2u;
    let span = footprint.z;
    var total = vec4<f32>(0.);
    var area = 0u;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let p = start + vec2<u32>(x, y);
            let edge = min(p * span, footprint.xy);
            let size = min(footprint.xy - edge, vec2<u32>(span));
            let weight = size.x * size.y;
            if weight > 0u {
                total += textureLoad(source, vec2<i32>(p), 0) * f32(weight);
                area += weight;
            }
        }
    }
    return total / f32(max(area, 1u));
}
