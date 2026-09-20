@group(0) @binding(0) var source: texture_2d<f32>;
// Original dimensions, original pixels per source texel, packed 256px tile XY.
// Scratch-tile reduction uses XY = 0; retained images use document coordinates.
@group(0) @binding(1) var<uniform> footprint: vec4<u32>;
@group(0) @binding(2) var destination: texture_storage_2d<rgba32float, write>;

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let span = footprint.z;
    let side = 128u / span;
    if any(invocation.xy >= vec2<u32>(side)) { return; }
    // A retained row run uses one Z workgroup per adjacent tile. Scratch
    // reduction and isolated tiles dispatch Z=1 and retain their original XY.
    let tile = vec2<u32>((footprint.w & 65535u) + invocation.z, footprint.w >> 16u);
    let position = tile * side + invocation.xy;
    if any(position >= textureDimensions(destination)) { return; }
    let start = position * 2u;
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
    textureStore(destination, vec2<i32>(position), total / f32(max(area, 1u)));
}
