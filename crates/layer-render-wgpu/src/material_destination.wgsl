@group(0) @binding(3) var material_wetness_output: texture_storage_2d<r32float, write>;

fn transport_dirty(id: vec2<u32>) -> bool {
    let bounds = material_sources.pages[0];
    return all(id >= bounds.xy) && all(id < bounds.zw);
}

fn transport_result(id: vec2<u32>) -> MaterialOutput {
    return material_result(vec4<f32>(vec2<f32>(id) + 0.5, 0.0, 1.0));
}

fn transport_original(p: vec2<i32>) -> vec4<f32> {
    if DITHER_BLENDER { return textureLoad(reservoir_texture, p, 0); }
    return textureLoad(source_11, p, 0);
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_color(@builtin(global_invocation_id) id: vec3<u32>) {
    if transport_dirty(id.xy) {
        store_dry_color(id.xy, operation_color(render_target.origin_extent.xy + vec2<f32>(id.xy) + 0.5, transport_result(id.xy)));
    } else {
        store_dry_color(id.xy, transport_original(vec2<i32>(id.xy)));
    }
}

fn store_transport_coverage(id: vec2<u32>, dirty: bool) -> MaterialOutput {
    let p = vec2<i32>(id);
    if dirty {
        let result = transport_result(id);
        textureStore(material_coverage_output, p, result.coverage);
        if DITHER_BLENDER {
            if result.coverage.x >= 0.0 { textureStore(material_coverage_output, p + vec2<i32>(0, 256), result.wetness); }
            if style.transport_b.w > 0.5 { textureStore(material_coverage_output, p + vec2<i32>(0, 512), result.color); }
            store_dry_color(id, operation_color(render_target.origin_extent.xy + vec2<f32>(id) + 0.5, result));
            return MaterialOutput(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
        }
        store_dry_color(id, result.color);
        return result;
    }
    store_dry_color(id, transport_original(p));
    if DITHER_BLENDER {
        let exposure = textureLoad(stroke_coverage_texture, p, 0);
        textureStore(material_coverage_output, p, exposure);
        if exposure.x >= 0.0 { textureStore(material_coverage_output, p + vec2<i32>(0, 256), textureLoad(stroke_coverage_texture, p + vec2<i32>(0, 256), 0)); }
        if style.transport_b.w > 0.5 { textureStore(material_coverage_output, p + vec2<i32>(0, 512), textureLoad(stroke_coverage_texture, p + vec2<i32>(0, 512), 0)); }
    } else { textureStore(material_coverage_output, p, vec4<f32>(dry_coverage(p), 0.0, 0.0, 1.0)); }
    return MaterialOutput(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_coverage(@builtin(global_invocation_id) id: vec3<u32>) {
    store_transport_coverage(id.xy, transport_dirty(id.xy));
}

@compute @workgroup_size(TRANSPORT_WORKGROUP_X, TRANSPORT_WORKGROUP_Y)
fn transport_watercolor(@builtin(global_invocation_id) id: vec3<u32>) {
    let wetness = store_transport_coverage(id.xy, transport_dirty(id.xy)).wetness;
    if wetness.a > 0.5 {
        textureStore(material_wetness_output, vec2<i32>(id.xy), wetness);
    } else if material_sources.header.w != 0u {
        textureStore(material_wetness_output, vec2<i32>(id.xy), textureLoad(reservoir_texture, vec2<i32>(id.xy), 0));
    }
}
