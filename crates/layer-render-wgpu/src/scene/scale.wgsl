// The two kernels share queue-ordered region records. Reduction reads exact
// premultiplied paint; only presentation ever consumes the resulting pixels.
struct Region {
    origin: vec2<u32>,
    size: vec2<u32>,
    extent: vec2<u32>,
    side: u32,
    flags: u32,
    paper: vec4<f32>,
    opacity: vec4<f32>,
    source_origin: vec2<i32>,
    base_origin: vec2<i32>,
}
@group(0) @binding(0) var<uniform> region: Region;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var base: texture_2d<f32>;
@group(1) @binding(2) var output: texture_storage_2d<rgba32float, write>;
@group(1) @binding(3) var source_preview: texture_2d<f32>;
@group(1) @binding(4) var base_preview: texture_2d<f32>;

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    if (region.flags & 4u) != 0u {
        textureStore(output, vec2<i32>(region.origin + id.xy), region.paper);
        return;
    }
    let start = vec2<u32>(vec2<i32>((id.xy + select(vec2<u32>(0u), region.origin, (region.flags & 8u) != 0u)) * region.side) - vec2<i32>(region.opacity.zw));
    let step = 1u << (region.flags >> 8u);
    let remaining = region.extent - start * step;
    let size = min(vec2<u32>(region.side), (remaining + step - 1u) / step);
    var sum = vec4<f32>(0.);
    var weight = 0u;
    for (var y = 0u; y < size.y; y++) {
        for (var x = 0u; x < size.x; x++) {
            let p = vec2<i32>(start + vec2<u32>(x, y));
            var value = textureLoad(source, p, 0);
            if (region.flags & 32u) != 0u {
                value = vec4<f32>(select(value.r, 1. - value.r, (region.flags & 64u) != 0u));
            }
            // A flow preview is a transparent contribution. Resolve it before
            // reducing, so layer opacity is applied only once.
            if (region.flags & 1u) != 0u {
                value += textureLoad(base, p, 0) * (1. - value.a);
            }
            // A final compact texel may represent fewer document pixels.
            // Weight its average accordingly at odd document boundaries.
            let footprint = min(vec2<u32>(step), remaining - vec2<u32>(x, y) * step);
            let count = footprint.x * footprint.y;
            sum += value * f32(count);
            weight += count;
        }
    }
    textureStore(output, vec2<i32>(region.origin + id.xy), sum / f32(weight));
}

@compute @workgroup_size(8, 8)
fn reduce_pair(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    let dst = region.origin + id.xy;
    let start = dst * 2u - vec2<u32>(region.opacity.zw);
    let step = 1u << (region.flags >> 8u);
    let footprint = min(vec2<u32>(step * 2u), region.extent - start * step);
    let first = min(vec2<u32>(step), footprint);
    let second = footprint - first;
    let weight = vec4<f32>(vec4<u32>(first.x * first.y, second.x * first.y, first.x * second.y, second.x * second.y));
    let p = vec2<i32>(start);
    let sum = textureLoad(source, p, 0) * weight.x
        + textureLoad(source, p + vec2(1, 0), 0) * weight.y
        + textureLoad(source, p + vec2(0, 1), 0) * weight.z
        + textureLoad(source, p + vec2(1, 1), 0) * weight.w;
    textureStore(output, vec2<i32>(dst), sum / f32(footprint.x * footprint.y));
}
