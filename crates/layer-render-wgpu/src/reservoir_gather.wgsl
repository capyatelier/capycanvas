const EXCHANGE_GATHERED: u32 = 1u;

fn exchange_mode() -> u32 {
    return material_sources.header.x;
}

fn pickup_position(dab: Dab, texel: vec2<u32>, tap: u32) -> vec2<f32> {
    return pickup_tap_position(pulled_position(exchange_world(dab, reservoir_local(texel)), dab), tap);
}

fn gathered_tap(at: vec2<u32>) -> vec4<f32> {
    return textureLoad(stroke_coverage_texture, vec2<i32>(at), 0);
}

fn exchange_tap(position: vec2<f32>, gathered: vec2<u32>) -> vec4<f32> {
    if outside_drawing(position) { return vec4<f32>(0.0); }
    let slot = donor_slot(vec2<u32>(floor(position / 256.0)));
    if slot < material_sources.pages[1].z { return donor_load(slot, position); }
    if exchange_mode() == EXCHANGE_GATHERED { return gathered_tap(gathered); }
    return vec4<f32>(0.0);
}

fn pickup_tap(dab: Dab, offset: u32, texel: vec2<u32>, tap: u32) -> vec4<f32> {
    return exchange_tap(pickup_position(dab, texel, tap), texel + vec2<u32>(tap, offset) * RESERVOIR_TEXELS);
}

fn exchange_pickup(dab: Dab, offset: u32, texel: vec2<u32>, world: vec2<f32>) -> vec4<f32> {
    if pickup_radius() < 0.01 {
        return pickup_tap(dab, offset, texel, 0u);
    }
    return (pickup_tap(dab, offset, texel, 0u) * 4.0
        + pickup_tap(dab, offset, texel, 1u)
        + pickup_tap(dab, offset, texel, 2u)
        + pickup_tap(dab, offset, texel, 3u)
        + pickup_tap(dab, offset, texel, 4u)) / 8.0;
}

fn footprint_tap(dab: Dab, offset: u32, tap: u32, world: vec2<f32>) -> vec4<f32> {
    return exchange_tap(world, vec2<u32>(tap, style.operation.y * RESERVOIR_TEXELS + offset));
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn reservoir_gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = style.operation.y;
    let texels = material_sources.pages[0];
    let size = texels.zw - texels.xy;
    if id.y < count * size.y {
        if id.x >= PICKUP_TAPS * size.x { return; }
        let texel = texels.xy + vec2<u32>(id.x % size.x, id.y % size.y);
        let tap = id.x / size.x;
        let offset = id.y / size.y;
        store_donated(texel + vec2<u32>(tap, offset) * RESERVOIR_TEXELS, pickup_position(dabs[style.operation.x + offset], texel, tap));
        return;
    }
    let offset = id.y - count * size.y;
    if offset >= count || id.x >= AVERAGE_TAPS { return; }
    store_donated(vec2<u32>(id.x, count * RESERVOIR_TEXELS + offset), average_position(dabs[style.operation.x + offset], id.x));
}
