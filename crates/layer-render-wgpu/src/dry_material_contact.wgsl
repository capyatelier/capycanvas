fn material_contact(dab:Dab, world:vec2<f32>, field:vec2<f32>, prior:f32, selected:f32, held:f32)->MaterialContact {
    let footprint=footprint_and_start(dab,world,field,held,!pointwise_aggregate())*selected;
    return MaterialContact(dab.color.rgb,footprint.x,footprint.y,0u);
}
