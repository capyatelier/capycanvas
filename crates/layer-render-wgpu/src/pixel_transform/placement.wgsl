@group(2) @binding(BINDING) var placed:texture_storage_2d<FORMAT,write>;
fn placement_contains(pixel:vec2<u32>)->bool {
    return all(pixel>=vec2<u32>(transform.texels.xy)) && all(pixel<vec2<u32>(transform.texels.xy+transform.texels.zw));
}
fn placement_pixel(pixel:vec2<u32>)->vec4<f32> {
    if !placement_contains(pixel) {return vec4(0.);}
    return layer_pixel(vec2<f32>(pixel)+.5+transform.attachment.xy);
}
@compute @workgroup_size(8,8)
fn placement_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(placed)) || ((flags()&CLEAR)==0u && !placement_contains(id.xy)) {return;}
    textureStore(placed,vec2<i32>(id.xy),placement_pixel(id.xy));
}
