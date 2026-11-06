@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> region: vec4<u32>;
@group(0) @binding(2) var destination: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn copy(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.xy) { return; }
    textureStore(destination, vec2<i32>(region.zw + id.xy), half_keeping_clipping(textureLoad(source, vec2<i32>(id.xy), 0)));
}
