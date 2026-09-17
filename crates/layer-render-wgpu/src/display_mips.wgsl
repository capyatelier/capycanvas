@group(0) @binding(0) var source: texture_2d<f32>;
// Original tile dimensions and the number of original pixels per source texel.
@group(0) @binding(1) var<uniform> footprint: vec4<u32>;
@group(0) @binding(2) var destination: texture_storage_2d<rgba32float, write>;

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) position: vec3<u32>) {
    if any(position.xy >= textureDimensions(destination)) { return; }
    let start = position.xy * 2u;
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
    textureStore(destination, vec2<i32>(position.xy), total / f32(max(area, 1u)));
}
