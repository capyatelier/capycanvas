struct ScatterCellOutput {
    low: vec2<i32>,
    high: vec2<i32>,
    state: vec4<f32>,
    dirty: u32,
}

fn scatter_cell_store(id: vec2<u32>) -> ScatterCellOutput {
    let origin = scatter_origin(id);
    let low = vec2<i32>(max(origin - render_target.origin_extent.xy, vec2<f32>(0.0)));
    let high = vec2<i32>(min(origin + style.deformation.w - render_target.origin_extent.xy,
        min(vec2<f32>(256.0), render_target.document_extent.xy - render_target.origin_extent.xy)));
    if any(high <= low) { return ScatterCellOutput(low, high, vec4<f32>(0.0), 0u); }
    let bounds = material_sources.pages[0];
    let dirty = all(high > vec2<i32>(bounds.xy)) && all(low < vec2<i32>(bounds.zw));
    var state = vec4<f32>(0.0);
    var paint = vec4<f32>(0.0);
    var amount = 0.0;
    var chosen = false;
    if dirty {
        state = smudge_fragment(vec2<f32>(low) + 0.5, origin, material_sources.header.x, material_sources.header.y).coverage;
        amount = scatter_amount(state);
        chosen = scatter_unit(origin, 0x9e3779b9u) < amount * clamp(style.canvas_opacity.z, 0.0, 1.0);
        if chosen { paint = scatter_source(origin, state); }
    }
    for (var y = low.y; y < high.y; y += 1) {
        for (var x = low.x; x < high.x; x += 1) {
            let pixel = vec2<i32>(x, y);
            var color = textureLoad(reservoir_texture, pixel, 0);
            if dirty {
                let point = render_target.origin_extent.xy + vec2<f32>(pixel) + 0.5;
                color = canvas_load(point);
                if chosen && brush_selection_at(point) > 0.0 { color = scatter_color(color, paint, amount); }
                color = working_clamp(layer_color(color, WORKING_LUMA, style.color_mode.x, style.color_mode.y));
            }
            store_dry_color(vec2<u32>(pixel), color);
        }
    }
    return ScatterCellOutput(low, high, state, u32(dirty));
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_color(@builtin(global_invocation_id) id: vec3<u32>) {
    scatter_cell_store(id.xy);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_coverage(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = scatter_cell_store(id.xy);
    for (var y = cell.low.y; y < cell.high.y; y += 1) {
        for (var x = cell.low.x; x < cell.high.x; x += 1) {
            let pixel = vec2<i32>(x, y);
            textureStore(material_coverage_output, pixel, select(textureLoad(stroke_coverage_texture, pixel, 0), cell.state, cell.dirty != 0u));
        }
    }
}
