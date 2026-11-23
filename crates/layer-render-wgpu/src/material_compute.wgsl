fn dry_block_result(id: vec2<u32>) -> MaterialOutput {
    let block = style.operation.z;
    return material_result(vec4<f32>(vec2<f32>(id * block) + 0.5 * f32(block), 0.0, 1.0));
}

fn cropped_color() -> bool {
    return MATERIAL_OPERATION==OP_DEPOSIT && contact_feature(64u,style.contact_a.z>0.5) && !contact_uniform();
}

@compute @workgroup_size(32, 2)
fn copy_color(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(material_color_output)) {return;}
    let block=style.operation.z;
    let bounds=material_sources.pages[0];
    if all(id.xy>=bounds.xy/block) && all(id.xy*block<bounds.zw) && !dry_start_stored() {return;}
    load_dry_destination(id.xy);
    store_dry_color(id.xy,sampled_original(vec2<i32>(id.xy*block)));
}

fn saturated_dry_coverage(p: vec2<i32>) -> f32 {
    if !grouped_opacity() && dry_start_stored() && style.canvas_opacity.w < 0.5
        && contact_uniform() && MATERIAL_NORMAL_BLEND && !bristles_enabled() {
        return dry_coverage(p);
    }
    return 0.0;
}

@compute @workgroup_size(32, 2)
fn compute_color(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let block=style.operation.z;
    let cropped=cropped_color();
    let bounds=material_sources.pages[0];
    let id=invocation.xy+select(vec2<u32>(0u),bounds.xy/block,cropped);
    if any(id>=textureDimensions(material_color_output)) || (cropped && any(id*block>=bounds.zw)) {return;}
    load_dry_destination(id);
    if saturated_dry_coverage(vec2<i32>(id*block))>=1.0 {return;}
    store_dry_color(id,dry_block_result(id).color);
}

@compute @workgroup_size(32, 2)
fn compute_coverage(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let id=invocation.xy;
    if any(id>=vec2<u32>(256u)) {return;}
    let p=vec2<i32>(id);
    load_dry_destination(id);
    let coverage=saturated_dry_coverage(p);
    if coverage>=1.0 {
        textureStore(material_coverage_output,p,vec4<f32>(coverage,0.0,0.0,1.0));
        return;
    }
    let result=material_result(vec4<f32>(vec2<f32>(id)+0.5,0.0,1.0));
    store_dry_color(id,result.color);
    textureStore(material_coverage_output,p,result.coverage);
}
