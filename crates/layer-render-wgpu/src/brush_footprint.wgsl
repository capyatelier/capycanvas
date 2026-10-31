// Pure brush coverage shared by artwork and scalar mask painting.
override FOOTPRINT_GRAIN: bool = true;

fn brush_footprint(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    if !contact_feature(64u,style.contact_a.z>0.5) && bristles_enabled() { return bristle_footprint(dab, world, field.y); }
    return unbristled_footprint(dab, world, field);
}

fn footprint_and_start(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> vec2<f32> {
    if !contact_feature(64u,style.contact_a.z>0.5) && contact_feature(1u, style.contact_a.x > 0.5) {
        return evolving_contact(dab, world, field);
    }
    return vec2<f32>(unbristled_footprint(dab, world, field), 0.0);
}

fn evolving_contact(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> vec2<f32> {
    return evolving_contact_prepared(world, dab.center, dab.radii, dab.rotation, dab.motion,
        dab.previous, dab.contact, dab.previous_contact, dab.hardness, field,
        dab.metric, dab.invariants.xy);
}

fn unbristled_footprint(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    if contact_feature(64u,style.contact_a.z>0.5) { return relief_contact(dab,world,field.y); }
    if contact_feature(1u, style.contact_a.x > 0.5) { return evolving_contact(dab, world, field).x; }
    let delta = world - dab.center;
    let local = rotate(delta, dab.rotation.x, -dab.rotation.y) / max(dab.radii, vec2<f32>(0.005));
    var coverage = tip_coverage(
        style.flags.x > 0.5,
        primary_texture,
        local,
        dab.texture_sign,
        dab.hardness,
        min(dab.radii.x, dab.radii.y),
    );
    if coverage <= 0.0 { return 0.0; }
    if FOOTPRINT_GRAIN && style.flags.y > 0.5 {
        coverage *= mix(1.0, paper_sample(grain_texture, brush_sampler, world, style.grain),
            clamp(dab.material.x * style.flags.z, 0.0, 1.0));
    }
    if coverage < style.edges.w { return 0.0; }
    return coverage;
}
