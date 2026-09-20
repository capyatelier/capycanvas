// New continuous-coverage ink/pencil, used only by the explicit test switch.
// Reuses real source tiles, stroke coverage, selection and disposable prediction.
fn swept_result(position: vec4<f32>) -> MaterialOutput {
    let world = layer_to_brush(render_target.origin_extent.xy + position.xy);
    let xy = vec2<i32>(position.xy);
    let original = textureLoad(source_11, xy, 0);
    var old_coverage = textureLoad(stroke_coverage_texture, xy, 0).r;
    if style.canvas_opacity.w > 0.5 { old_coverage = 0.0; }
    let range = material_sources.header.zw;
    var best = 1e20;
    var radius = 1.0;
    var pressure = 1.0;
    var color = vec4<f32>(0.0);
    var flow = 0.0;
    for (var i = 0u; i < range.y; i += 1u) {
        let d = dabs[range.x + i];
        let t = clamp(dot(world - d.center + d.motion, d.motion)
            / max(dot(d.motion, d.motion), 0.000001), 0.0, 1.0);
        let from_radius = select(d.radii.x, d.previous.x, d.previous.x > 0.0);
        let r = mix(from_radius, d.radii.x, t);
        let distance = length(world - d.center + d.motion * (1.0 - t)) - r;
        if distance < best {
            best = distance;
            radius = r;
            pressure = mix(d.previous_contact.x, d.contact.x, t);
            color = d.color;
            flow = d.flow;
        }
    }
    let pencil = style.contact_a.y > 0.0;
    let feather = select(1.0, max(1.0, radius * 0.25), pencil);
    var alpha = 1.0 - smoothstep(-feather, 0.5, best);
    if pencil && alpha > 0.0 {
        let uv = world / vec2<f32>(textureDimensions(grain_texture));
        let tooth = textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r;
        let threshold = 0.7 - pressure * 0.9 * 0.58;
        alpha *= smoothstep(threshold - 0.12, threshold + 0.12, tooth) * (0.48 + tooth * 0.52);
        alpha = 1.0 - exp(-alpha * 1.5);
    }
    alpha *= clamp(flow * color.a, 0.0, 1.0) * brush_selection_at(brush_to_layer(world));
    if !WORKING_EXTENDED { alpha = round(alpha * 255.0) / 255.0; }
    let next_coverage = max(old_coverage, alpha);
    let delta = clamp(working_ratio(next_coverage - old_coverage, 1.0 - old_coverage), 0.0, 1.0);
    var result = source_over(original, color.rgb, delta);
    if style.operation.w != 0u { result = original * (1.0 - delta); }
    return MaterialOutput(result, vec4<f32>(next_coverage, 0.0, 0.0, 1.0), vec4<f32>(0.0));
}
