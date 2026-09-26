// Pure brush coverage shared by artwork and scalar mask painting.
fn brush_footprint(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    if contact_feature(1u, style.contact_a.x > 0.5) {
        return evolving_contact_prepared(world, dab.center, dab.radii, dab.rotation, dab.motion,
            dab.previous, dab.contact, dab.previous_contact, dab.hardness, field,
            dab.metric, dab.invariants.xy);
    }
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
    if style.flags.y > 0.5 {
        let uv = grain_uv(
            local,
            world,
            style.grain,
            style.flags.z > 0.5,
            dab.center,
            style.flags.w,
        );
        coverage *= mix(1.0, textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r,
            clamp(dab.material.x, 0.0, 1.0));
    }
    if coverage < style.edges.w { return 0.0; }
    return coverage;
}
