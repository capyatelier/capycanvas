fn reservoir_exchange_amount(dab: Dab, coverage: f32) -> f32 {
    let pull = clamp(dab.material.y, 0.0, 1.0);
    return clamp(
        ((1.0 - style.material_a.x) * style.material_b.x + pull * 0.25)
            * coverage * dab.flow,
        0.0,
        1.0,
    );
}

fn replenished_reservoir_alpha(carried: f32, pickup: f32, exchange: f32) -> f32 {
    return max(carried, mix(carried, pickup, exchange));
}

fn wet_refill(dab: Dab) -> f32 {
    let travel = length(dab.motion) / max(min(dab.radii.x, dab.radii.y) * 2.0, 0.01);
    return 1.0 - exp(-4.0 * clamp(style.deformation.z, 0.0, 1.0) * smoothstep(0.55, 1.0, dab.contact.x) * travel);
}

fn reservoir_local(texel: vec2<u32>) -> vec2<f32> {
    return (vec2<f32>(texel) + 0.5) / f32(RESERVOIR_TEXELS) * 2.0 - vec2<f32>(1.0);
}

fn exchange_world(dab: Dab, local: vec2<f32>) -> vec2<f32> {
    return dab.center + rotate(local * dab.radii, dab.rotation.x, dab.rotation.y);
}

fn average_position(dab: Dab, tap: u32) -> vec2<f32> {
    return dab.center + rotate(vogel(tap, AVERAGE_TAPS, 0.0) * dab.radii, dab.rotation.x, dab.rotation.y);
}

fn footprint_average(dab: Dab, offset: u32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var index = 0u; index < AVERAGE_TAPS; index += 1u) {
        let world = average_position(dab, index);
        let inside = all(world >= vec2<f32>(0.0)) && all(world < render_target.document_extent.xy);
        let weight = select(0.0, contact_coverage(dab, world), inside);
        sum += mix_space_tap(footprint_tap(dab, offset, index, world)) * weight;
        weights += weight;
    }
    return resolve_mix_space(sum / max(weights, 0.000001));
}

fn carried_toward(carried: vec4<f32>, goal: vec4<f32>, exchange: f32, keep_alpha: bool) -> vec4<f32> {
    if !working_has_color(goal.a) {
        return vec4<f32>(carried.rgb, select(mix(carried.a, 0.0, exchange), carried.a, keep_alpha));
    }
    let incoming = exchange * goal.a;
    let color = mix_color(carried.rgb, goal.rgb / goal.a, paint_share(incoming, (1.0 - exchange) * carried.a));
    let alpha = select(mix(carried.a, goal.a, exchange), replenished_reservoir_alpha(carried.a, goal.a, exchange), keep_alpha);
    return vec4<f32>(color, alpha);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn reservoir_exchange(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = reservoir_local(id.xy);
    var carried = textureLoad(reservoir_texture, vec2<i32>(id.xy), 0);
    if style.canvas_opacity.w > 0.5 {
        carried = bitcast<vec4<f32>>(material_sources.pages[0]);
    }
    let average = wet_average();
    for (var offset = 0u; offset < style.operation.y; offset += 1u) {
        let dab = dabs[style.operation.x + offset];
        let world = exchange_world(dab, local);
        let coverage = contact_coverage(dab, world);
        if average > 0.0 {
            let diameter = max(max(dab.radii.x, dab.radii.y) * 2.0, 0.01);
            let half_life = max(style.deformation.x, 0.0) * diameter;
            let kept = select(0.0, exp2(-length(dab.motion) / max(half_life, 0.0001)), half_life > 0.0 && working_has_color(carried.a));
            carried = carried_toward(carried, footprint_average(dab, offset), average * (1.0 - kept), false);
        }
        if coverage <= 0.0 || average >= 1.0 { continue; }
        let refill = wet_refill(dab) * coverage;
        if refill > 0.000001 {
            let loaded = paint_share(refill, (1.0 - refill) * carried.a);
            carried = vec4<f32>(mix_color(carried.rgb, dab.color.rgb, loaded), max(carried.a, refill));
        }
        let pickup = exchange_pickup(dab, offset, id.xy, world);
        if !working_has_color(pickup.a) { continue; }
        let exchange = reservoir_exchange_amount(dab, coverage) * (1.0 - average);
        let alpha_exchange = exchange * (1.0 - style.material_a.w * 0.5);
        let incoming = exchange * pickup.a;
        carried = vec4<f32>(
            mix_color(carried.rgb, pickup.rgb / pickup.a, paint_share(incoming, (1.0 - exchange) * carried.a)),
            replenished_reservoir_alpha(carried.a, pickup.a, alpha_exchange),
        );
    }
    store_dry_color(id.xy, carried);
}
