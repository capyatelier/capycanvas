// Resample immutable source coverage once per placement. Brush/mask consumers
// retain their compact packed-coverage interface and pay no per-dab transform
// cost. Rows x, y and w map an output pixel to homogeneous source pixels.
struct Params {
    rect: vec4<u32>, info: vec4<u32>,
    x: vec4<f32>, y: vec4<f32>, w: vec4<f32>,
}
struct Packed { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> source: Packed;
@group(0) @binding(2) var<storage, read_write> output: Packed;

fn at(p: vec2<i32>) -> f32 {
    let q = p - vec2<i32>(source.rect.xy);
    if any(q < vec2<i32>(0)) || any(q >= vec2<i32>(source.rect.zw)) { return 0.; }
    let bytes = source.info.y == 2u;
    let count = select(8u,4u,bytes);
    let bits = 32u/count;
    let word = u32(q.y) * ((source.rect.z + count-1u) / count) + u32(q.x) / count;
    return f32((source.values[word] >> ((u32(q.x) % count) * bits)) & select(15u,255u,bytes)) / select(4.,255.,bytes);
}

fn coverage(p: vec2<f32>) -> f32 {
    let h = vec3(p, 1.);
    let w = dot(params.w.xyz, h);
    let q = vec2(dot(params.x.xyz, h), dot(params.y.xyz, h)) / w - .5;
    if w <= 0. || any(abs(q) > vec2(16777216.)) { return 0.; }
    let base = vec2<i32>(floor(q));
    let f = fract(q);
    return mix(mix(at(base), at(base + vec2<i32>(1, 0)), f.x),
        mix(at(base + vec2<i32>(0, 1)), at(base + vec2<i32>(1, 1)), f.x), f.y);
}

@compute @workgroup_size(64)
fn resample(@builtin(global_invocation_id) id: vec3<u32>) {
    let bytes = params.info.y == 2u;
    let count = select(8u,4u,bytes);
    let maximum = select(4.,255.,bytes);
    let stride = (params.rect.z + count-1u) / count;
    if id.x >= stride || id.y >= params.rect.w { return; }
    var packed = 0u;
    for (var i = 0u; i < count; i++) {
        let x = id.x * count + i;
        if x >= params.rect.z { break; }
        let value = coverage(vec2<f32>(params.rect.xy + vec2<u32>(x, id.y)) + .5);
        // Preserve the source coverage precision and bounded storage.
        packed |= min(u32(maximum), u32(floor(value * maximum + .5))) << (i * (32u/count));
    }
    output.values[id.y * stride + id.x] = packed;
}

// A warp mesh maps each output pixel through the layer position rasterized at
// it, one window of rect at a time, with a one-pixel border, into an output
// info.z pixels wide. Rows x and y then map that position to source pixels.
@group(0) @binding(3) var positions: texture_2d<f32>;
const UNCOVERED = -1e38;
@compute @workgroup_size(64)
fn resample_mesh(@builtin(global_invocation_id) id: vec3<u32>) {
    let words = (params.rect.z + 3u) / 4u;
    if id.x >= words || id.y >= params.rect.w { return; }
    var packed = 0u;
    for (var i = 0u; i < 4u; i++) {
        let x = id.x * 4u + i;
        if x >= params.rect.z { break; }
        let s = textureLoad(positions, vec2<i32>(vec2(x, id.y)) + vec2(1), 0).xy;
        if s.x > UNCOVERED {
            packed |= min(255u, u32(floor(coverage(s) * 255. + .5))) << (i * 8u);
        }
    }
    let stride = (params.info.z + 3u) / 4u;
    output.values[(params.rect.y + id.y) * stride + params.rect.x / 4u + id.x] = packed;
}
