fn scatter_hash(value: u32) -> u32 {
    var v = value * 747796405u + 2891336453u;
    v = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (v >> 22u) ^ v;
}

fn scatter_key(origin: vec2<f32>, domain: u32) -> u32 {
    let cell = bitcast<vec2<u32>>(vec2<i32>(origin));
    return scatter_hash(cell.x ^ scatter_hash(cell.y ^ scatter_hash(u32(style.transport_b.z) ^ domain)));
}

fn scatter_unit(origin: vec2<f32>, domain: u32) -> f32 {
    return (f32(scatter_key(origin, domain) >> 9u) + 0.5) / 8388608.0;
}

fn scatter_origin(id: vec2<u32>) -> vec2<f32> {
    let size = style.deformation.w;
    let seed = u32(style.transport_b.z);
    let phase_y = f32(scatter_hash(seed ^ 0x243f6a88u) % u32(size));
    let row = i32(floor((render_target.origin_extent.y - phase_y) / size)) + i32(id.y);
    let phase_x = f32(scatter_hash(bitcast<u32>(row) ^ scatter_hash(seed ^ 0x85a308d3u)) % u32(size));
    let column = floor((render_target.origin_extent.x - phase_x) / size) + f32(id.x);
    return vec2<f32>(column * size + phase_x, f32(row) * size + phase_y);
}

fn smudge_fragment(fragment_position: vec2<f32>, origin: vec2<f32>, first: u32, count: u32) -> MaterialOutput {
    let pixel = vec2<i32>(floor(fragment_position));
    var state = select(textureLoad(stroke_coverage_texture, pixel, 0), vec4<f32>(0.0), style.canvas_opacity.w > 0.5);
    let size = style.deformation.w;
    for (var i = 0u; i < count; i += 1u) {
        let dab = dabs[first + i];
        let coverage = blender_cell_coverage(dab, origin, size);
        if coverage <= 0.0 { continue; }
        let strength = coverage * dab.flow;
        let dose = strength * length(dab.motion) / max(min(dab.radii.x, dab.radii.y) * 2.0, 0.01) * max(style.deformation.y, 0.0);
        state.x = min(state.x + floor(min(dose, 16.0) * 1048576.0 + 0.5), 16777216.0);
        var pull = state.yz / 4096.0 + dab.motion * clamp(dab.material.y, 0.0, 1.0) * strength;
        pull *= min(1.0, (128.0 - size) / max(length(pull), 0.000001));
        let fixed_pull = round(pull * 4096.0);
        state.y = fixed_pull.x;
        state.z = fixed_pull.y;
    }
    return MaterialOutput(vec4<f32>(0.0), state, vec4<f32>(0.0));
}

fn scatter_source(origin: vec2<f32>, state: vec4<f32>) -> vec4<f32> {
    let radius = clamp(style.deformation.x * style.transport_b.y, 0.0, 112.0);
    if radius <= 0.0 && all(state.yz == vec2<f32>(0.0)) { return vec4<f32>(0.0); }
    let size = u32(style.deformation.w);
    let texel = vec2<u32>(scatter_key(origin, 0xa341316cu), scatter_key(origin, 0xc8013ea4u)) % vec2<u32>(size);
    let angle = scatter_unit(origin, 0xad90777du) * 6.28318530718;
    let offset = vec2<f32>(cos(angle), sin(angle)) * sqrt(scatter_unit(origin, 0x7e95761eu)) * radius;
    let point = floor(origin + vec2<f32>(texel) + 0.5 + offset - state.yz / 4096.0) + 0.5;
    if brush_selection_at(point) <= 0.0 { return vec4<f32>(0.0); }
    return canvas_load(point);
}

fn scatter_amount(state: vec4<f32>) -> f32 {
    return select(1.0 - exp(-state.x / 1048576.0), 1.0, state.x >= 16777216.0);
}

fn scatter_color(original: vec4<f32>, paint: vec4<f32>, amount: f32) -> vec4<f32> {
    if !working_has_color(paint.a) { return original; }
    if style.color.a > 0.5 && !working_has_color(original.a) { return original; }
    let influence = mix(amount, 1.0, clamp(style.deformation.z, 0.0, 1.0));
    let alpha = select(original.a + max(paint.a - original.a, 0.0) * influence, original.a, style.color.a > 0.5);
    if influence == 1.0 {
        if alpha == paint.a { return paint; }
        return vec4<f32>(working_unassociate(paint) * alpha, alpha);
    }
    let donor = working_unassociate(paint);
    let own = select(donor, working_unassociate(original), working_has_color(original.a));
    let color = mix_color(own, donor, working_ratio(influence * min(paint.a, 1.0), alpha));
    return vec4<f32>(color * alpha, alpha);
}
