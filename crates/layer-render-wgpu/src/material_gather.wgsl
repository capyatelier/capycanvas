fn donor_slot(page: vec2<u32>) -> u32 {
    let donors = material_sources.pages[1].z;
    for (var slot = 0u; slot < donors; slot += 1u) {
        let pair = material_sources.pages[2u + slot / 2u];
        if all(page == select(pair.xy, pair.zw, slot % 2u == 1u)) { return slot; }
    }
    return donors;
}

fn donor_load(slot: u32, position: vec2<f32>) -> vec4<f32> {
    return load_neighborhood(i32(slot), vec2<i32>(floor(position)) - vec2<i32>(vec2<u32>(floor(position / 256.0)) * 256u));
}

fn outside_drawing(position: vec2<f32>) -> bool {
    return any(position < vec2<f32>(0.0)) || any(position >= render_target.document_extent.xy);
}

fn store_donated(at: vec2<u32>, position: vec2<f32>) {
    if outside_drawing(position) {
        if material_sources.header.w != 0u { store_dry_color(at, vec4<f32>(0.0)); }
        return;
    }
    let slot = donor_slot(vec2<u32>(floor(position / 256.0)));
    if slot < material_sources.pages[1].z {
        store_dry_color(at, donor_load(slot, position));
    }
}

fn gathered_page_tap(fragment_position: vec2<f32>, row: u32, tap: u32) -> vec4<f32> {
    return textureLoad(source_00, vec2<i32>(floor(fragment_position)) + vec2<i32>(i32(tap), i32(row)) * 256, 0);
}

fn pickup_tap_position(position: vec2<f32>, tap: u32) -> vec2<f32> {
    let radius = pickup_radius();
    var offsets = array<vec2<f32>, 5>(
        vec2<f32>(0.0), vec2<f32>(radius, 0.0), vec2<f32>(-radius, 0.0), vec2<f32>(0.0, radius), vec2<f32>(0.0, -radius),
    );
    return position + offsets[tap];
}

fn pulled_pickup(fragment_position: vec2<f32>, world: vec2<f32>, dab: Dab, row: u32) -> vec4<f32> {
    if pickup_radius() < 0.01 {
        return gathered_page_tap(fragment_position, row, 0u);
    }
    return (gathered_page_tap(fragment_position, row, 0u) * 4.0
        + gathered_page_tap(fragment_position, row, 1u)
        + gathered_page_tap(fragment_position, row, 2u)
        + gathered_page_tap(fragment_position, row, 3u)
        + gathered_page_tap(fragment_position, row, 4u)) / 8.0;
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

fn traced_sample(fragment_position: vec2<f32>, position: vec2<f32>) -> vec4<f32> {
    let base = floor(position - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let fraction = clamp(position - base, vec2<f32>(0.0), vec2<f32>(1.0));
    let sample = mix(
        mix(gathered_page_tap(fragment_position, 0u, 0u), gathered_page_tap(fragment_position, 0u, 1u), fraction.x),
        mix(gathered_page_tap(fragment_position, 0u, 2u), gathered_page_tap(fragment_position, 0u, 3u), fraction.x),
        fraction.y,
    );
    if pickup_radius() < 0.01 {
        return sample;
    }
    return (sample * 4.0
        + gathered_page_tap(fragment_position, 0u, 4u)
        + gathered_page_tap(fragment_position, 0u, 5u)
        + gathered_page_tap(fragment_position, 0u, 6u)
        + gathered_page_tap(fragment_position, 0u, 7u)) / 8.0;
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

fn gathered_position(world: vec2<f32>, row: u32) -> vec2<f32> {
    if MATERIAL_OPERATION == OP_WET {
        return pulled_position(world, dabs[style.operation.x + row]);
    }
    return watercolor_trace(world, style.operation.x, style.operation.y);
}

fn gathered_tap_position(position: vec2<f32>, tap: u32) -> vec2<f32> {
    if MATERIAL_OPERATION == OP_WET {
        return pickup_tap_position(position, tap);
    }
    return trace_tap_position(position, tap);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let bounds = material_sources.pages[0];
    let size = bounds.zw - bounds.xy;
    if id.x >= size.x || id.y >= size.y * material_sources.header.z { return; }
    let row = id.y / size.y;
    let pixel = bounds.xy + vec2<u32>(id.x, id.y % size.y);
    let position = gathered_position(vec2<f32>(material_sources.pages[1].xy * 256u + pixel) + 0.5, row);
    for (var tap = 0u; tap < material_sources.header.y; tap += 1u) {
        store_donated(pixel + vec2<u32>(tap, row) * 256u, gathered_tap_position(position, tap));
    }
}
