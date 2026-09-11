// Resample immutable source coverage once per placement. Brush/mask consumers
// retain their compact packed-coverage interface and pay no per-dab affine cost.
struct Params {
    rect: vec4<u32>, info: vec4<u32>,
    inverse: vec4<f32>, offset: vec4<f32>,
}
struct Packed { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> source: Packed;
@group(0) @binding(2) var<storage, read_write> output: Packed;

fn at(p: vec2<i32>) -> f32 {
    let q = p - vec2<i32>(source.rect.xy);
    if any(q < vec2<i32>(0)) || any(q >= vec2<i32>(source.rect.zw)) { return 0.; }
    let word = u32(q.y) * ((source.rect.z + 7u) / 8u) + u32(q.x) / 8u;
    return f32((source.values[word] >> ((u32(q.x) % 8u) * 4u)) & 15u);
}

@compute @workgroup_size(64)
fn resample(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.rect.z + 7u) / 8u;
    if id.x >= stride || id.y >= params.rect.w { return; }
    var packed = 0u;
    for (var i = 0u; i < 8u; i++) {
        let x = id.x * 8u + i;
        if x >= params.rect.z { break; }
        let p = vec2<f32>(params.rect.xy + vec2<u32>(x, id.y)) + .5;
        let q = vec2<f32>(dot(params.inverse.xz, p), dot(params.inverse.yw, p)) + params.offset.xy - .5;
        let base = vec2<i32>(floor(q));
        let f = fract(q);
        let value = mix(mix(at(base), at(base + vec2<i32>(1, 0)), f.x),
            mix(at(base + vec2<i32>(0, 1)), at(base + vec2<i32>(1, 1)), f.x), f.y);
        // Preserve the existing four-level coverage format and bounded storage.
        packed |= min(4u, u32(floor(value + .5))) << (i * 4u);
    }
    output.values[id.y * stride + id.x] = packed;
}
