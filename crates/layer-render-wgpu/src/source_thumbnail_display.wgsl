@group(0) @binding(0) var<storage, read> sums: array<vec4<f32>>;
struct Options {orientation:vec4<f32>,rendition:vec4<f32>,highlight:vec4<f32>}
@group(0) @binding(1) var<uniform> options: Options;
fn sample_overview(p: vec2<i32>) -> vec4<f32> {
    if any(p < vec2(0)) || any(p >= vec2(32)) { return vec4(0.); }
    return sums[u32(p.y) * 32u + u32(p.x)];
}
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(p[i], 0., 1.);
}
@fragment fn fragment_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<u32>(p.xy);
    let q = mat2x2<f32>(options.orientation.xy, options.orientation.zw) * (p.xy - 16.) + 15.5;
    // Branch before float-to-int conversion for subpixel-thin placements.
    var raw = vec4<f32>(0.);
    if all(q > vec2(-1.)) && all(q < vec2(32.)) {
        let low = vec2<i32>(floor(q));
        let t = fract(q);
        raw = mix(mix(sample_overview(low), sample_overview(low + vec2(1, 0)), t.x),
            mix(sample_overview(low + vec2(0, 1)), sample_overview(low + vec2(1, 1)), t.x), t.y);
    }
    raw=hdr_map_sdr(raw,options.rendition,options.highlight.x);
    let gray = select(.497, .855, (xy.x / 4u + xy.y / 4u) % 2u == 0u);
    return vec4(raw.rgb + gray * (1. - clamp(raw.a, 0., 1.)), 1.);
}
