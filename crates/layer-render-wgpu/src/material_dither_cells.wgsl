struct DitherCellOutput {
    low: vec2<i32>,
    high: vec2<i32>,
    state: MaterialOutput,
    dirty: u32,
}

fn dither_cell_store(id: vec2<u32>) -> DitherCellOutput {
    let size = style.deformation.w;
    let origin = (floor(render_target.origin_extent.xy / size) + vec2<f32>(id)) * size;
    let low = vec2<i32>(max(origin - render_target.origin_extent.xy, vec2<f32>(0.0)));
    let high = vec2<i32>(min(origin + size - render_target.origin_extent.xy, vec2<f32>(256.0)));
    if any(high <= low) {
        return DitherCellOutput(low, high, MaterialOutput(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0)), 0u);
    }
    let bounds = material_sources.pages[0];
    let dirty = all(high > vec2<i32>(bounds.xy)) && all(low < vec2<i32>(bounds.zw));
    let world = origin + size * 0.5;
    var result = MaterialOutput(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    var chosen = 0u;
    var paint = vec4<f32>(0.0);
    if dirty {
        result = smudge_fragment(vec2<f32>(low) + 0.5, world, material_sources.header.x, material_sources.header.y);
        chosen = dither_choice(world, result);
        if chosen != 0u { paint = dither_paint(chosen | 8u, world, dither_original(world)); }
    }
    for (var y = low.y; y < high.y; y += 1) {
        for (var x = low.x; x < high.x; x += 1) {
            let pixel = vec2<i32>(x, y);
            let point = render_target.origin_extent.xy + vec2<f32>(pixel) + 0.5;
            var color = textureLoad(reservoir_texture, pixel, 0);
            if dirty {
                color = canvas_load(point);
                if chosen != 0u && brush_selection_at(point) > 0.0 {
                    if style.color.a <= 0.5 { color = paint; }
                    else if working_has_color(paint.a) && working_has_color(color.a) {
                        color = vec4<f32>(working_unassociate(paint) * color.a, color.a);
                    }
                }
                color = working_clamp(layer_color(color, WORKING_LUMA, style.color_mode.x, style.color_mode.y));
            }
            store_dry_color(vec2<u32>(pixel), color);
        }
    }
    return DitherCellOutput(low, high, result, u32(dirty));
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_color(@builtin(global_invocation_id) id: vec3<u32>) {
    dither_cell_store(id.xy);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_coverage(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = dither_cell_store(id.xy);
    for (var y = cell.low.y; y < cell.high.y; y += 1) {
        for (var x = cell.low.x; x < cell.high.x; x += 1) {
            let pixel = vec2<i32>(x, y);
            if cell.dirty != 0u {
                textureStore(material_coverage_output, pixel, cell.state.coverage);
                textureStore(material_coverage_output, pixel + vec2<i32>(0, 256), cell.state.wetness);
                if style.transport_b.w > 0.5 { textureStore(material_coverage_output, pixel + vec2<i32>(0, 512), cell.state.color); }
            } else {
                textureStore(material_coverage_output, pixel, textureLoad(stroke_coverage_texture, pixel, 0));
                textureStore(material_coverage_output, pixel + vec2<i32>(0, 256), textureLoad(stroke_coverage_texture, pixel + vec2<i32>(0, 256), 0));
                if style.transport_b.w > 0.5 {
                    textureStore(material_coverage_output, pixel + vec2<i32>(0, 512), textureLoad(stroke_coverage_texture, pixel + vec2<i32>(0, 512), 0));
                }
            }
        }
    }
}
