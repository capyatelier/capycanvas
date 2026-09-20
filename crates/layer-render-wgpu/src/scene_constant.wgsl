// An aligned, constant-backdrop tile can write its destination directly.
// This avoids loading/storing a large render attachment on tile-based GPUs.
@group(2) @binding(0) var scene_output: texture_storage_2d<rgba32float, write>;
@compute @workgroup_size(8, 8)
fn compose_constant(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = vec2<f32>(id.xy) + vec2(.5);
    let p = settings.rect.xy + local;
    if any(local >= settings.rect.zw) || any(p >= settings.extent.xy) { return; }
    let v = Vertex(vec4(p,0.,1.), local/settings.rect.zw);
    textureStore(scene_output,vec2<i32>(p),scene_normal(scene_read(front,v),v));
}
