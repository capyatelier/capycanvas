// An aligned, constant-backdrop tile can write its destination directly.
// This avoids loading/storing a large render attachment on tile-based GPUs.
@group(2) @binding(0) var scene_output: texture_storage_2d<rgba32float, write>;
@group(1) @binding(3) var lower: texture_2d<f32>;
fn scene_normal_stack(preview: vec4<f32>, paint: vec4<f32>, v: Vertex) -> vec4<f32> {
    let top = (preview + paint * (1. - preview.a)) * settings.options.y;
    let bottom = scene_read(lower, v) * settings.source_over.x;
    let backdrop = bottom + settings.backdrop * (1. - bottom.a);
    return top + backdrop * (1. - top.a);
}

@compute @workgroup_size(32, 2)
fn compose_constant(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = vec2<f32>(id.xy) + vec2(.5);
    let p = settings.rect.xy + local;
    if any(local >= settings.rect.zw) || any(p >= settings.extent.xy) { return; }
    let v = Vertex(vec4(p,0.,1.), local/settings.rect.zw);
    var result: vec4<f32>;
    if u32(settings.options.x) == 15u {
        result = scene_normal_stack(scene_read(front,v), scene_read(back,v), v);
    } else {
        result = scene_normal(scene_read(front,v),v);
    }
    textureStore(scene_output,vec2<i32>(p),result);
}
