fn gathered_position(world: vec2<f32>, row: u32, tap: u32) -> vec2<f32> {
    if MATERIAL_OPERATION == OP_WET {
        return pickup_tap_position(pulled_position(world, dabs[style.operation.x + row]), tap);
    }
    return trace_tap_position(watercolor_trace(world, style.operation.x, style.operation.y), tap);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.xy % vec2<u32>(256u);
    let bounds = material_sources.pages[0];
    if id.x / 256u >= material_sources.header.y || id.y / 256u >= material_sources.header.z
        || any(pixel < bounds.xy) || any(pixel >= bounds.zw) { return; }
    let position = gathered_position(vec2<f32>(material_sources.pages[1].xy * 256u + pixel) + 0.5, id.y / 256u, id.x / 256u);
    let outside = any(position < vec2<f32>(0.0)) || any(position >= render_target.document_extent.xy);
    let owned = all(floor(position / 256.0) == floor(render_target.origin_extent.xy / 256.0));
    if (!outside && owned) || (outside && material_sources.header.w != 0u) {
        store_dry_color(id.xy, canvas_load(position));
    }
}
