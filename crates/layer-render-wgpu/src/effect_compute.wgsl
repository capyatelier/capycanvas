@compute @workgroup_size(32, 2)
fn effect_compute(@builtin(global_invocation_id) id: vec3<u32>) {
    var p=settings.rect.xy+vec2<f32>(id.xy)+vec2(.5);
    if settings.query_grid.z!=0u {
        if any(id.xy>=query_grid_size()) {return;}
        p=query_grid_pixel(id.xy)+vec2(.5);
    }
    let local=p-settings.rect.xy;
    if any(local<vec2(0.)) || any(local>=settings.rect.zw) || any(p<vec2(0.)) || any(p>=settings.extent.xy) {return;}
    textureStore(effect_output,vec2<i32>(p),effect_result(Vertex(vec4(p,0.,1.),local/settings.rect.zw)));
}
