// One workgroup reduces a 16x16 source patch through four levels. Intermediate
// samples stay in shared memory; every retained level still receives exactly
// the same area-weighted reduction, including partial document edges.
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> footprint: vec4<u32>;
@group(0) @binding(2) var level1: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var level2: texture_storage_2d<rgba32float, write>;
@group(0) @binding(4) var level3: texture_storage_2d<rgba32float, write>;
@group(0) @binding(5) var level4: texture_storage_2d<rgba32float, write>;
var<workgroup> samples: array<vec4<f32>, 64>;

fn pixel_area(p: vec2<u32>, span: u32) -> u32 {
    let edge = min(p * span, footprint.xy);
    let size = min(footprint.xy - edge, vec2<u32>(span));
    return size.x * size.y;
}

@compute @workgroup_size(8, 8)
fn reduce_four(@builtin(workgroup_id) group: vec3<u32>,
               @builtin(local_invocation_id) local: vec3<u32>) {
    let tile = vec2<u32>((footprint.w & 65535u) + group.z, footprint.w >> 16u);
    let origin = tile * (128u / footprint.z) + group.xy * 8u;
    let p = origin + local.xy;
    var total = vec4<f32>(0.0);
    var area = 0u;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let q = p * 2u + vec2<u32>(x, y);
            let weight = pixel_area(q, footprint.z);
            if weight > 0u {
                total += textureLoad(source, vec2<i32>(q), 0) * f32(weight);
                area += weight;
            }
        }
    }
    var value = total / f32(max(area, 1u));
    if all(p < textureDimensions(level1)) { textureStore(level1, vec2<i32>(p), value); }
    samples[local.y * 8u + local.x] = value;
    workgroupBarrier();
    for (var level = 1u; level < 4u; level++) {
        let side = 8u >> level;
        let participating = all(local.xy < vec2<u32>(side));
        let position = (origin >> vec2<u32>(level)) + local.xy;
        if participating {
            total = vec4<f32>(0.0);
            area = 0u;
            for (var y = 0u; y < 2u; y++) {
                for (var x = 0u; x < 2u; x++) {
                    let offset = vec2<u32>(x, y);
                    let weight = pixel_area(position * 2u + offset, footprint.z << level);
                    if weight > 0u {
                        let q = local.xy * 2u + offset;
                        total += samples[q.y * 8u + q.x] * f32(weight);
                        area += weight;
                    }
                }
            }
            value = total / f32(max(area, 1u));
            if level == 1u && all(position < textureDimensions(level2)) {
                textureStore(level2, vec2<i32>(position), value);
            }
            if level == 2u && all(position < textureDimensions(level3)) {
                textureStore(level3, vec2<i32>(position), value);
            }
            if level == 3u && all(position < textureDimensions(level4)) {
                textureStore(level4, vec2<i32>(position), value);
            }
        }
        // Readers finish before this level reuses the shared array.
        workgroupBarrier();
        if participating { samples[local.y * 8u + local.x] = value; }
        workgroupBarrier();
    }
}
