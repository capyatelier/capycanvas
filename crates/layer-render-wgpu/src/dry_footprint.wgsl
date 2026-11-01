fn brush_footprint(dab:Dab, world:vec2<f32>, field:vec2<f32>)->f32 {
    return relief_contact(dab,world,field.y,0.0,true).x;
}
fn footprint_and_start(dab:Dab, world:vec2<f32>, field:vec2<f32>, held:f32, required:bool)->vec2<f32> {
    let exposure=relief_contact(dab,world,field.y,held,required);
    return vec2<f32>(exposure.x,select(exposure.y,0.0,contact_uniform()));
}
