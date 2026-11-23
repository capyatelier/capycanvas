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
