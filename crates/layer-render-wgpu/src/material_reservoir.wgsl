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

const EXCHANGE_GATHERED: u32 = 1u;
const EXCHANGE_GATHER: u32 = 2u;
const AVERAGE_TAPS: u32 = 24u;

fn exchange_mode() -> u32 {
    return material_sources.header.x;
}

fn exchange_world(dab: Dab, texel: vec2<u32>) -> vec2<f32> {
    let local = (vec2<f32>(texel) + 0.5) / f32(RESERVOIR_TEXELS) * 2.0 - vec2<f32>(1.0);
    return dab.center + rotate(local * dab.radii, dab.rotation.x, dab.rotation.y);
}

fn pickup_position(dab: Dab, texel: vec2<u32>, tap: u32) -> vec2<f32> {
    return pickup_tap_position(pulled_position(exchange_world(dab, texel), dab), tap);
}

fn average_position(dab: Dab, tap: u32) -> vec2<f32> {
    return dab.center + rotate(vogel(tap, AVERAGE_TAPS, 0.0) * dab.radii, dab.rotation.x, dab.rotation.y);
}

fn gathered_tap(at: vec2<u32>) -> vec4<f32> {
    return textureLoad(stroke_coverage_texture, vec2<i32>(at), 0);
}

fn pickup_tap(dab: Dab, offset: u32, texel: vec2<u32>, tap: u32) -> vec4<f32> {
    if exchange_mode() == EXCHANGE_GATHERED {
        return gathered_tap(texel + vec2<u32>(tap, offset) * RESERVOIR_TEXELS);
    }
    return canvas_load(pickup_position(dab, texel, tap));
}

fn average_tap(dab: Dab, offset: u32, tap: u32) -> vec4<f32> {
    if exchange_mode() == EXCHANGE_GATHERED {
        return gathered_tap(vec2<u32>(tap, style.operation.y * RESERVOIR_TEXELS + offset));
    }
    return canvas_load(average_position(dab, tap));
}

fn exchange_pickup(dab: Dab, offset: u32, texel: vec2<u32>) -> vec4<f32> {
    if pickup_radius() < 0.01 {
        return pickup_tap(dab, offset, texel, 0u);
    }
    return (pickup_tap(dab, offset, texel, 0u) * 4.0
        + pickup_tap(dab, offset, texel, 1u)
        + pickup_tap(dab, offset, texel, 2u)
        + pickup_tap(dab, offset, texel, 3u)
        + pickup_tap(dab, offset, texel, 4u)) / 8.0;
}

fn footprint_average(dab: Dab, offset: u32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var index = 0u; index < AVERAGE_TAPS; index += 1u) {
        let world = average_position(dab, index);
        let inside = all(world >= vec2<f32>(0.0)) && all(world < render_target.document_extent.xy);
        let weight = select(0.0, contact_coverage(dab, world), inside);
        sum += mix_space_tap(average_tap(dab, offset, index)) * weight;
        weights += weight;
    }
    return resolve_mix_space(sum / max(weights, 0.000001));
}

fn gather_tap(at: vec2<u32>) {
    let count = style.operation.y;
    let rows = count * RESERVOIR_TEXELS;
    var position: vec2<f32>;
    if at.y < rows {
        if at.x >= PICKUP_TAPS * RESERVOIR_TEXELS { return; }
        position = pickup_position(dabs[style.operation.x + at.y / RESERVOIR_TEXELS], at % vec2<u32>(RESERVOIR_TEXELS), at.x / RESERVOIR_TEXELS);
    } else {
        if at.y >= rows + count || at.x >= AVERAGE_TAPS { return; }
        position = average_position(dabs[style.operation.x + at.y - rows], at.x);
    }
    let outside = any(position < vec2<f32>(0.0)) || any(position >= render_target.document_extent.xy);
    let owned = all(floor(position / 256.0) == floor(render_target.origin_extent.xy / 256.0));
    if (!outside && owned) || (outside && material_sources.header.w != 0u) {
        store_dry_color(at, canvas_load(position));
    }
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
    if exchange_mode() == EXCHANGE_GATHER {
        gather_tap(id.xy);
        return;
    }
    var carried = textureLoad(reservoir_texture, vec2<i32>(id.xy), 0);
    if style.canvas_opacity.w > 0.5 {
        carried = bitcast<vec4<f32>>(material_sources.pages[0]);
    }
    let average = wet_average();
    for (var offset = 0u; offset < style.operation.y; offset += 1u) {
        let dab = dabs[style.operation.x + offset];
        let world = exchange_world(dab, id.xy);
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
        let pickup = exchange_pickup(dab, offset, id.xy);
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
