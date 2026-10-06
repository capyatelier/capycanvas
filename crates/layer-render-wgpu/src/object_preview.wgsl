struct PreviewObject { x: vec4<f32>, y: vec4<f32>, source: vec4<i32>, slot: vec4<u32> }
struct Collection { size: vec4<u32>, objects: array<PreviewObject, 16> }
@group(0) @binding(0) var<uniform> collection: Collection;
@group(0) @binding(1) var back: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba32float,write>;
@group(0) @binding(3) var s0: texture_2d<f32>;
@group(0) @binding(4) var s1: texture_2d<f32>;
@group(0) @binding(5) var s2: texture_2d<f32>;
@group(0) @binding(6) var s3: texture_2d<f32>;
@group(0) @binding(7) var s4: texture_2d<f32>;
@group(0) @binding(8) var s5: texture_2d<f32>;
@group(0) @binding(9) var s6: texture_2d<f32>;
@group(0) @binding(10) var s7: texture_2d<f32>;
fn load(slot: u32, p: vec2<i32>) -> vec4<f32> {
    switch slot {
        case 0u: { return textureLoad(s0, p, 0); }
        case 1u: { return textureLoad(s1, p, 0); }
        case 2u: { return textureLoad(s2, p, 0); }
        case 3u: { return textureLoad(s3, p, 0); }
        case 4u: { return textureLoad(s4, p, 0); }
        case 5u: { return textureLoad(s5, p, 0); }
        case 6u: { return textureLoad(s6, p, 0); }
        default: { return textureLoad(s7, p, 0); }
    }
}
fn sample(object: PreviewObject, position: vec2<u32>) -> vec4<f32> {
    let h = vec3(vec2<f32>(position) + vec2(0.5) - vec2<f32>(collection.size.xy) * 0.5, 1.);
    let point = vec2(dot(object.x.xyz, h), dot(object.y.xyz, h)) - vec2(0.5);
    let origin = object.source.xy;
    let extent = object.source.zw;
    if object.slot.y != 0u {
        let texel = vec2<i32>(floor(point + vec2(0.5)));
        if any(texel < origin) || any(texel >= origin + extent) { return vec4(0.); }
        return load(object.slot.x, texel - origin);
    }
    let low = vec2<i32>(floor(point));
    if any(low < origin - vec2(1)) || any(low >= origin + extent) { return vec4(0.); }
    let fraction = point - floor(point);
    var sum = vec4(0.);
    for (var y = 0; y < 2; y++) { for (var x = 0; x < 2; x++) {
        let p = low + vec2(x, y);
        if all(p >= origin) && all(p < origin + extent) {
            sum += load(object.slot.x, p - origin) * select(1. - fraction.x, fraction.x, x == 1) * select(1. - fraction.y, fraction.y, y == 1);
        }
    }}
    return sum;
}
@compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= collection.size.xy) { return; }
    var color = vec4(0.);
    if (collection.size.w & 1u) != 0u { color = textureLoad(back, vec2<i32>(id.xy), 0); }
    for (var index = 0u; index < collection.size.z; index++) {
        var front = sample(collection.objects[index], id.xy);
        if (collection.size.w & 2u) != 0u { front = working_encode(front); }
        color = front + color * (1. - front.a);
    }
    textureStore(output, vec2<i32>(id.xy), color);
}
