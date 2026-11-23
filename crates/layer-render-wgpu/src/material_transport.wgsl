const MIX_OKLAB:u32=1u;
const MIX_CLASSIC:u32=2u;

fn load_neighborhood(index: i32, coordinate: vec2<i32>) -> vec4<f32> {
    switch index {
        case 0: { return textureLoad(source_00, coordinate, 0); }
        case 1: { return textureLoad(source_10, coordinate, 0); }
        case 2: { return textureLoad(source_20, coordinate, 0); }
        case 3: { return textureLoad(source_01, coordinate, 0); }
        case 4: { return textureLoad(source_11, coordinate, 0); }
        case 5: { return textureLoad(source_21, coordinate, 0); }
        case 6: { return textureLoad(source_02, coordinate, 0); }
        case 7: { return textureLoad(source_12, coordinate, 0); }
        default: { return textureLoad(source_22, coordinate, 0); }
    }
}

fn local_canvas_load(document_position: vec2<f32>) -> vec4<f32> {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return vec4<f32>(0.0);
    }
    let relative = document_position - render_target.origin_extent.xy;
    if all(relative >= vec2<f32>(0.0)) && all(relative < vec2<f32>(256.0)) {
        return textureLoad(source_11, vec2<i32>(relative), 0);
    }
    let page_offset = vec2<i32>(floor(relative / 256.0));
    if any(page_offset < vec2<i32>(-1)) || any(page_offset > vec2<i32>(1)) {
        return vec4<f32>(0.0);
    }
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    let index = (page_offset.y + 1) * 3 + page_offset.x + 1;
    return load_neighborhood(index, coordinate);
}

fn canvas_load(brush_position: vec2<f32>) -> vec4<f32> {
    return local_canvas_load(brush_position);
}

fn canvas_sample(brush_position: vec2<f32>) -> vec4<f32> {
    let document_position = brush_position;
    let base = floor(document_position - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let fraction = clamp(document_position - base, vec2<f32>(0.0), vec2<f32>(1.0));
    let top = mix(
        local_canvas_load(base),
        local_canvas_load(base + vec2<f32>(1.0, 0.0)),
        fraction.x,
    );
    let bottom = mix(
        local_canvas_load(base + vec2<f32>(0.0, 1.0)),
        local_canvas_load(base + vec2<f32>(1.0, 1.0)),
        fraction.x,
    );
    return mix(top, bottom, fraction.y);
}

fn donor_slot(page: vec2<u32>) -> u32 {
    let donors = material_sources.pages[1].z;
    for (var slot = 0u; slot < donors; slot += 1u) {
        let pair = material_sources.pages[2u + slot / 2u];
        if all(page == select(pair.xy, pair.zw, slot % 2u == 1u)) { return slot; }
    }
    return donors;
}

fn store_donated(at: vec2<u32>, position: vec2<f32>) {
    if any(position < vec2<f32>(0.0)) || any(position >= render_target.document_extent.xy) {
        if material_sources.header.w != 0u { store_dry_color(at, vec4<f32>(0.0)); }
        return;
    }
    let page = vec2<u32>(floor(position / 256.0));
    let slot = donor_slot(page);
    if slot < material_sources.pages[1].z {
        store_dry_color(at, load_neighborhood(i32(slot), vec2<i32>(floor(position)) - vec2<i32>(page * 256u)));
    }
}

fn gathered_pickups() -> bool {
    return material_sources.header.z != 0u;
}

fn gathered_page_tap(fragment_position: vec2<f32>, row: u32, tap: u32) -> vec4<f32> {
    return textureLoad(source_00, vec2<i32>(floor(fragment_position)) + vec2<i32>(i32(tap), i32(row)) * 256, 0);
}

fn pickup_radius() -> f32 {
    return clamp(style.material_b.z, 0.0, 1.0) * 4.0;
}

fn pickup_tap_position(position: vec2<f32>, tap: u32) -> vec2<f32> {
    let radius = pickup_radius();
    var offsets = array<vec2<f32>, 5>(
        vec2<f32>(0.0), vec2<f32>(radius, 0.0), vec2<f32>(-radius, 0.0), vec2<f32>(0.0, radius), vec2<f32>(0.0, -radius),
    );
    return position + offsets[tap];
}

fn pulled_position(world: vec2<f32>, dab: Dab) -> vec2<f32> {
    return world - dab.motion * clamp(dab.material.y, 0.0, 1.0);
}

fn pulled_tap(fragment_position: vec2<f32>, world: vec2<f32>, dab: Dab, row: u32, tap: u32) -> vec4<f32> {
    if gathered_pickups() {
        return gathered_page_tap(fragment_position, row, tap);
    }
    return canvas_load(pickup_tap_position(pulled_position(world, dab), tap));
}

fn pulled_pickup(fragment_position: vec2<f32>, world: vec2<f32>, dab: Dab, row: u32) -> vec4<f32> {
    if pickup_radius() < 0.01 {
        return pulled_tap(fragment_position, world, dab, row, 0u);
    }
    return (pulled_tap(fragment_position, world, dab, row, 0u) * 4.0
        + pulled_tap(fragment_position, world, dab, row, 1u)
        + pulled_tap(fragment_position, world, dab, row, 2u)
        + pulled_tap(fragment_position, world, dab, row, 3u)
        + pulled_tap(fragment_position, world, dab, row, 4u)) / 8.0;
}

fn trace_tap_position(position: vec2<f32>, tap: u32) -> vec2<f32> {
    let base = floor(position - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let radius = pickup_radius();
    var taps = array<vec2<f32>, 8>(
        base, base + vec2<f32>(1.0, 0.0), base + vec2<f32>(0.0, 1.0), base + vec2<f32>(1.0, 1.0),
        position + vec2<f32>(radius, 0.0), position + vec2<f32>(-radius, 0.0), position + vec2<f32>(0.0, radius), position + vec2<f32>(0.0, -radius),
    );
    return taps[tap];
}

fn traced_tap(fragment_position: vec2<f32>, position: vec2<f32>, tap: u32) -> vec4<f32> {
    if gathered_pickups() {
        return gathered_page_tap(fragment_position, 0u, tap);
    }
    return canvas_load(trace_tap_position(position, tap));
}

fn traced_sample(fragment_position: vec2<f32>, position: vec2<f32>) -> vec4<f32> {
    let base = floor(position - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let fraction = clamp(position - base, vec2<f32>(0.0), vec2<f32>(1.0));
    let sample = mix(
        mix(traced_tap(fragment_position, position, 0u), traced_tap(fragment_position, position, 1u), fraction.x),
        mix(traced_tap(fragment_position, position, 2u), traced_tap(fragment_position, position, 3u), fraction.x),
        fraction.y,
    );
    if pickup_radius() < 0.01 {
        return sample;
    }
    return (sample * 4.0
        + traced_tap(fragment_position, position, 4u)
        + traced_tap(fragment_position, position, 5u)
        + traced_tap(fragment_position, position, 6u)
        + traced_tap(fragment_position, position, 7u)) / 8.0;
}

fn trace_back(trace_coordinate: vec2<f32>, dab: Dab) -> vec2<f32> {
    let trace_delta = trace_coordinate - dab.center;
    let trace_local = rotate(trace_delta, dab.rotation.x, -dab.rotation.y)
        / max(dab.radii, vec2<f32>(0.005));
    let trace_coverage = analytic_coverage(
        trace_local,
        dab.hardness,
        min(dab.radii.x, dab.radii.y),
    ) * brush_selection_at(trace_coordinate);
    if trace_coverage > 0.0 {
        return trace_coordinate - dab.motion
            * clamp(dab.material.y, 0.0, 1.0)
            * trace_coverage;
    }
    return trace_coordinate;
}

fn watercolor_trace(world: vec2<f32>, first: u32, count: u32) -> vec2<f32> {
    var trace_coordinate = world;
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        trace_coordinate = trace_back(trace_coordinate, dabs[first + remaining]);
    }
    return trace_coordinate;
}

fn contact_segment_progress(dab: Dab, world: vec2<f32>) -> f32 {
    let motion_squared = dot(dab.motion, dab.motion);
    if motion_squared <= 0.000001 {
        return 1.0;
    }
    let previous_center = dab.center - dab.motion;
    return clamp(dot(world - previous_center, dab.motion) / motion_squared, 0.0, 1.0);
}

fn mix_color(a: vec3<f32>, b: vec3<f32>, amount: f32) -> vec3<f32> {
    if amount==0. || all(a==b) {return a;}
    if amount==1. {return b;}
    let space = u32(style.render_mode.z);
    if space == MIX_OKLAB {
        return working_from_oklab(mix(working_to_oklab(a), working_to_oklab(b), amount));
    }
    if space == MIX_CLASSIC {
        return sdr_decode(mix(sdr_encode(a, WORKING_SPACE), sdr_encode(b, WORKING_SPACE), amount), WORKING_SPACE);
    }
    return working_mix(a, b, amount);
}

const GOLDEN_ANGLE: f32 = 2.39996323;

fn to_mix_space(color: vec3<f32>) -> vec3<f32> {
    let space = u32(style.render_mode.z);
    if space == MIX_OKLAB { return working_to_oklab(color); }
    if space == MIX_CLASSIC { return sdr_encode(color, WORKING_SPACE); }
    return color;
}

fn from_mix_space(value: vec3<f32>) -> vec3<f32> {
    let space = u32(style.render_mode.z);
    if space == MIX_OKLAB { return working_from_oklab(value); }
    if space == MIX_CLASSIC { return sdr_decode(value, WORKING_SPACE); }
    return value;
}

fn mix_space_tap(paint: vec4<f32>) -> vec4<f32> {
    if !working_has_color(paint.a) { return vec4<f32>(0.0); }
    return vec4<f32>(to_mix_space(working_unassociate(paint)) * paint.a, paint.a);
}

fn resolve_mix_space(field: vec4<f32>) -> vec4<f32> {
    if !working_has_color(field.a) { return vec4<f32>(0.0); }
    let alpha = min(field.a, 1.0);
    return vec4<f32>(from_mix_space(field.rgb / field.a) * alpha, alpha);
}

fn paint_share(incoming: f32, kept: f32) -> f32 {
    return working_ratio(incoming, incoming + kept);
}

fn vogel(index: u32, count: u32, rotation: f32) -> vec2<f32> {
    let radius = sqrt((f32(index) + 0.5) / f32(count));
    let angle = f32(index) * GOLDEN_ANGLE + rotation;
    return vec2<f32>(cos(angle), sin(angle)) * radius;
}

fn random_unit(value: vec2<f32>) -> f32 {
    return fract(sin(dot(value, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn transport_conductance(world: vec2<f32>) -> f32 {
    if style.transport_a.x <= 0.0 {
        return 1.0;
    }
    let coordinate = rotate(
        world / 256.0,
        style.transport_a.y,
        style.transport_a.z,
    ) * style.transport_a.x;
    let sampled = textureSampleLevel(transport_texture, brush_sampler, coordinate, 0.0).r;
    let shaped = pow(clamp(sampled, 0.0, 1.0), 0.72);
    return mix(1.0, shaped, style.transport_a.w);
}

fn wet_average() -> f32 {
    return clamp(style.deformation.w, 0.0, 1.0);
}

fn reservoir_load(dab: Dab, world: vec2<f32>) -> vec4<f32> {
    let delta = world - dab.center;
    let local = rotate(delta, dab.rotation.x, -dab.rotation.y)
        / max(dab.radii, vec2<f32>(0.005));
    let size = vec2<i32>(textureDimensions(reservoir_texture));
    let coordinate = clamp(
        vec2<i32>(floor((local * 0.5 + vec2<f32>(0.5)) * vec2<f32>(size))),
        vec2<i32>(0),
        size - vec2<i32>(1),
    );
    return textureLoad(reservoir_texture, coordinate, 0);
}

fn watercolor_fragment(
    fragment_position: vec2<f32>,
    world: vec2<f32>,
    first: u32,
    count: u32,
) -> MaterialOutput {
    let stored = canvas_load(world);
    let original = working_clamp(stored);
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = dry_coverage(state_coordinate);
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }

    let prior_coverage = stroke_coverage;
    var pigment_color = dabs[first].color.rgb;
    var strongest_request = 0.0;
    let prior_wetness = textureLoad(reservoir_texture, state_coordinate, 0).r;
    var trace_coordinate = world;
    let field = contact_field(world);
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[first + remaining];
        let coverage = contact_coverage_field(dab, world, field);
        if coverage > 0.0 {
            let requested = clamp(
                coverage * dab.flow * dab.color.a * style.material_a.y
                    * style.material_b.x * dab.material.z,
                0.0,
                1.0,
            );
            if requested > strongest_request {
                strongest_request = requested;
                pigment_color = dab.color.rgb;
            }
            stroke_coverage += (1.0 - stroke_coverage) * requested;
        }
        trace_coordinate = trace_back(trace_coordinate, dab);
    }
    let water_load = select(1.0, style.transport_b.w, style.transport_a.x > 0.0);
    let water_charge = clamp(
        stroke_coverage * water_load
            * (0.38 + 0.62 * transport_conductance(world)),
        0.0,
        1.0,
    );
    let deposited = strongest_request > 0.0;
    let deposited_wetness = select(prior_wetness, prior_wetness + (1.0 - prior_wetness) * water_charge,
        deposited && (style.color.a <= 0.5 || stored.a > 0.0));
    let coverage_increment = stroke_increment(prior_coverage, stroke_coverage);
    var result = original;
    if working_has_color(coverage_increment) && style.operation.w == 0u {
        let dragged = traced_sample(fragment_position, trace_coordinate);
        let mixing = clamp(style.material_a.w * coverage_increment, 0.0, 1.0);
        if working_has_color(original.a) && working_has_color(dragged.a) && working_has_color(mixing) {
            let original_color = original.rgb / original.a;
            let dragged_color = dragged.rgb / dragged.a;
            let mixed_color = mix_color(
                original_color,
                dragged_color,
                mixing * dragged.a,
            );
            result = vec4<f32>(mixed_color * original.a, original.a);
        }
        let paint_load = mix(0.42, 1.0, style.material_a.x);
        let prior_pigment = prior_coverage * paint_load;
        let next_pigment = stroke_coverage * paint_load;
        let pigment = clamp(
            stroke_increment(prior_pigment, next_pigment),
            0.0,
            1.0,
        );
        result = stored_paint(source_over(deposit_paint(result), deposit_color(pigment_color), pigment));
    }
    if style.operation.w != 0u {
        result *= 1.0 - coverage_increment;
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(deposited_wetness, 0.0, 0.0, select(0.0, 1.0, deposited)),
    );
}

fn wet_fragment(
    fragment_position: vec2<f32>,
    world: vec2<f32>,
    first: u32,
    count: u32,
) -> MaterialOutput {
    let original = working_clamp(canvas_load(world));
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = dry_coverage(state_coordinate);
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }
    var batch_color = vec3<f32>(0.0);
    var batch_alpha = 0.0;
    let average = wet_average();
    let pure_blend = style.material_a.x <= 0.0 && style.deformation.z <= 0.0;
    var replaced = original;
    let field = contact_field(world);
    for (var offset = 0u; offset < count; offset += 1u) {
        let dab = dabs[first + offset];
        var contact_dab = dab;
        var contact_progress = 1.0;
        if contact_uniform() {
            contact_progress = contact_segment_progress(dab, world);
            contact_dab.center = dab.center - dab.motion * (1.0 - contact_progress);
        }
        let coverage = contact_coverage_field(contact_dab, world, field);
        if coverage <= 0.0 { continue; }
        let wet_jitter = mix(1.0, random_unit(world + dab.center), style.material_b.w);
        var carried = reservoir_load(contact_dab, world);
        if style.canvas_opacity.w > 0.5 {
            carried = vec4<f32>(dab.color.rgb, style.material_a.x * dab.material.z);
        }
        let pickup = pulled_pickup(fragment_position, world, dab, offset);
        let empty_color = select(dab.color.rgb, carried.rgb, working_has_color(carried.a));
        let pickup_color = select(
            empty_color,
            working_unassociate(pickup),
            working_has_color(pickup.a),
        );
        let carried_weight = max(carried.a * (1.0 - style.material_a.w), select(0.0, wet_average(), working_has_color(carried.a)));
        let pickup_weight = pickup.a * (1.0 - carried_weight);
        let paint_color = mix_color(pickup_color, empty_color, paint_share(carried_weight, pickup_weight));
        let available_material = max(
            pickup.a,
            carried.a * (1.0 - style.material_a.w * 0.5),
        );
        var source_alpha = clamp(
            coverage * dab.flow * dab.color.a * style.material_a.y
                * style.material_b.x * dab.material.z * wet_jitter * available_material,
            0.0,
            1.0,
        );
        if contact_uniform() || style.canvas_opacity.z < 1.0 {
            let next_coverage = stroke_coverage + (1.0 - stroke_coverage) * source_alpha;
            source_alpha = stroke_increment(stroke_coverage, next_coverage);
            stroke_coverage = next_coverage;
        }
        batch_color = batch_color * (1.0 - source_alpha) + deposit_color(paint_color) * source_alpha;
        batch_alpha = source_alpha + batch_alpha * (1.0 - source_alpha);
        if (average > 0.0 || pure_blend) && working_has_color(available_material) {
            let weight = clamp(working_ratio(source_alpha, available_material), 0.0, 1.0);
            let laid = select(available_material, carried.a, average > 0.0);
            let pigment = select(paint_color, carried.rgb, average > 0.0);
            let alpha = replaced.a + max(laid - replaced.a, 0.0) * weight;
            let kept = select(pigment, working_unassociate(replaced), working_has_color(replaced.a));
            let color = mix_color(kept, pigment, working_ratio(weight * laid, alpha));
            replaced = vec4<f32>(color * alpha, alpha);
        }
    }
    var result = original;
    if style.operation.w != 0u {
        result *= 1.0 - batch_alpha;
    } else if working_has_color(batch_alpha) {
        result = stored_paint(source_over(deposit_paint(original), batch_color / batch_alpha, batch_alpha));
    }
    if (average > 0.0 || pure_blend) && style.operation.w == 0u {
        result = mix(result, replaced, select(average, 1.0, pure_blend));
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(0.0),
    );
}

fn material_result(fragment_position:vec4<f32>)->MaterialOutput {
    let world=render_target.origin_extent.xy+fragment_position.xy;
    let first=select(style.operation.x,material_sources.header.x,DITHER_BLENDER);
    let count=select(style.operation.y,material_sources.header.y,DITHER_BLENDER);
    var result=operation_fragment(fragment_position.xy,world,first,count);
    if style.color.a>0.5 && !DITHER_BLENDER {
        result=material_alpha_locked(result,canvas_load(world));
    }
    if !DITHER_BLENDER {
        result.color=layer_color(result.color,WORKING_LUMA,style.color_mode.x,style.color_mode.y);
        result.color=working_clamp(result.color);
    }
    return result;
}
