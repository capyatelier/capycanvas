@group(0) @binding(0) var<storage, read> sums: array<vec4<f32>>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(p[i], 0., 1.);
}
@fragment fn fragment_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<u32>(p.xy);
    let raw = sums[xy.y * 32u + xy.x];
    let gray = select(.497, .855, (xy.x / 4u + xy.y / 4u) % 2u == 0u);
    return vec4(raw.rgb + gray * (1. - clamp(raw.a, 0., 1.)), 1.);
}
