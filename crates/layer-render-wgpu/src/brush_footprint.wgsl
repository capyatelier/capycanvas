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
            style.advanced.y > 0.5,
            dab.center,
            style.advanced.w,
        );
        coverage *= mix(1.0, textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r,
            clamp(dab.material.x, 0.0, 1.0));
    }
    if style.flags.w > 0.5 {
        let shifted = local - style.dual_offset_flags.xy;
        let dual_local = rotate(shifted, style.dual.z, -style.dual.w)
            / vec2<f32>(max(style.dual.x * style.dual.y, 0.0001), max(style.dual.x, 0.0001));
        var secondary = tip_coverage(
            style.flags.z > 0.5,
            dual_texture,
            dual_local,
            dab.texture_sign,
            dab.hardness,
            min(dab.radii.x, dab.radii.y) * style.dual.x,
        );
        if style.advanced.x > 0.5 {
            let uv = grain_uv(
                dual_local,
                world,
                style.dual_grain,
                style.advanced.z > 0.5,
                dab.center + vec2<f32>(31.7, 19.3),
                style.edges.w,
            );
            secondary *= mix(1.0, textureSampleLevel(dual_grain_texture, brush_sampler, uv, 0.0).r,
                clamp(style.dual_grain.y, 0.0, 1.0));
        }
        coverage = combine_coverage(coverage, secondary, style.dual_offset_flags.z);
    }
    if coverage < style.dual_offset_flags.w { return 0.0; }
    return coverage;
}
