struct DitherSample {
    lightness: f32,
    index: u32,
}

const DITHER_OFFSETS = array<vec2<f32>, 7>(
    vec2<f32>(0.267261242, 0.000000000),
    vec2<f32>(-0.341335464, 0.312691246),
    vec2<f32>(0.052246864, -0.595326064),
    vec2<f32>(0.430231244, 0.561160473),
    vec2<f32>(-0.789527247, -0.139656254),
    vec2<f32>(0.747909132, -0.475758568),
    vec2<f32>(-0.250160969, 0.930586330)
);

fn dither_hash(value: u32) -> u32 {
    var v = value * 747796405u + 2891336453u;
    v = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (v >> 22u) ^ v;
}

fn dither_rank(world: vec2<f32>, seed: u32) -> f32 {
    var p = bitcast<vec2<u32>>(vec2<i32>(floor(world))) + vec2<u32>(seed & 255u, (seed >> 8u) & 255u);
    var rank = 0u;
    for (var bit = 0u; bit < 8u; bit += 1u) {
        let bx = p.x & 1u;
        let by = p.y & 1u;
        rank = (rank << 2u) | ((bx ^ by) << 1u) | by;
        p >>= vec2<u32>(1u);
    }
    return (f32(rank) + 0.5) / 65536.0;
}

fn dither_point(index: u32, world: vec2<f32>) -> vec2<f32> {
    let seed = dither_hash(u32(style.transport_b.z));
    let point = DITHER_OFFSETS[(index & 7u) - 1u];
    let swapped = select(point, point.yx, (seed & 65536u) != 0u);
    let offset = swapped * vec2<f32>(select(1.0, -1.0, (seed & 131072u) != 0u),
        select(1.0, -1.0, (seed & 262144u) != 0u));
    let reach = clamp(style.deformation.x * style.transport_b.y, 0.0, 112.0);
    if DITHER_CELLS {
        let size = style.deformation.w;
        let cell = floor(world / size);
        return dither_cell_point(cell, floor((cell * size + size * 0.5 + offset * reach) / size));
    }
    return floor(world + offset * reach) + 0.5;
}

fn dither_hash_pair(value: vec2<u32>) -> vec2<u32> {
    var v = value * 747796405u + 2891336453u;
    v = ((v >> ((v >> vec2<u32>(28u)) + vec2<u32>(4u))) ^ v) * 277803737u;
    return (v >> vec2<u32>(22u)) ^ v;
}

fn dither_cell_point(cell: vec2<f32>, source: vec2<f32>) -> vec2<f32> {
    let key = bitcast<vec4<u32>>(vec4<i32>(vec4<f32>(cell, source)));
    var salt = dither_hash_pair(vec2<u32>(u32(style.transport_b.z)) ^ vec2<u32>(0xa341316cu, 0xc8013ea4u));
    for (var i = 0u; i < 4u; i += 1u) { salt = dither_hash_pair(salt ^ vec2<u32>(dither_hash(key[i]))); }
    return source * style.deformation.w + vec2<f32>(salt % vec2<u32>(u32(style.deformation.w))) + 0.5;
}

fn dither_original(world: vec2<f32>) -> vec4<f32> {
    var point = world;
    if DITHER_CELLS {
        let cell = floor(world / style.deformation.w);
        point = clamp(dither_cell_point(cell, cell), vec2<f32>(0.5), render_target.document_extent.xy - 0.5);
    }
    var original = canvas_load(point);
    if DITHER_CELLS && style.color.a > 0.5 && working_has_color(original.a) {
        original = vec4<f32>(working_unassociate(original), 1.0);
    }
    return original;
}

fn dither_paint(index: u32, world: vec2<f32>, original: vec4<f32>) -> vec4<f32> {
    if (index & 8u) == 0u { return original; }
    var paint = canvas_load(dither_point(index, world));
    if style.color.a > 0.5 {
        if !working_has_color(paint.a) || (!DITHER_CELLS && !working_has_color(original.a)) { return original; }
        if DITHER_CELLS { paint = vec4<f32>(working_unassociate(paint), 1.0); }
        else if paint.a != original.a { paint = vec4<f32>(working_unassociate(paint) * original.a, original.a); }
    }
    return paint;
}

fn dither_sample(index: u32, world: vec2<f32>, original: vec4<f32>) -> DitherSample {
    let point = dither_point(index, world);
    if any(point < vec2<f32>(0.0)) || any(point >= render_target.document_extent.xy) || brush_selection_at(point) <= 0.0 {
        return DitherSample(dot(original.rgb, WORKING_LUMA), index);
    }
    var paint = canvas_load(point);
    if style.color.a > 0.5 {
        if !working_has_color(paint.a) || (!DITHER_CELLS && !working_has_color(original.a)) { paint = original; }
        else if DITHER_CELLS { paint = vec4<f32>(working_unassociate(paint), 1.0); }
        else if paint.a != original.a { paint = vec4<f32>(working_unassociate(paint) * original.a, original.a); }
    }
    return DitherSample(dot(paint.rgb, WORKING_LUMA), index | select(0u, 8u, any(paint != original)));
}

fn dither_before(a: DitherSample, b: DitherSample, world: vec2<f32>, original: vec4<f32>) -> bool {
    if a.lightness != b.lightness { return a.lightness < b.lightness; }
    if ((a.index | b.index) & 8u) == 0u { return false; }
    let ca = dither_paint(a.index, world, original);
    let cb = dither_paint(b.index, world, original);
    if ca.r != cb.r { return ca.r < cb.r; }
    if ca.g != cb.g { return ca.g < cb.g; }
    if ca.b != cb.b { return ca.b < cb.b; }
    return ca.a < cb.a;
}

fn dither_order(a: ptr<function, DitherSample>, b: ptr<function, DitherSample>, world: vec2<f32>, original: vec4<f32>) {
    if dither_before(*b, *a, world, original) {
        let saved = *a;
        *a = *b;
        *b = saved;
    }
}

fn dither_exposure(previous: vec4<f32>, dose: vec4<f32>) -> vec4<f32> {
    return min(previous + floor(min(dose, vec4<f32>(16.0)) * 1048576.0 + 0.5), vec4<f32>(16777216.0));
}

fn dither_weights(exposure: vec4<f32>) -> vec4<f32> {
    return select(1.0 - exp(-exposure / 1048576.0), vec4<f32>(1.0), exposure >= vec4<f32>(16777216.0))
        * (clamp(style.canvas_opacity.z, 0.0, 1.0) / 7.0);
}

fn dither_contact_dose(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    var coverage = 0.0;
    if DITHER_CELLS { coverage = dither_cell_coverage(dab, world); }
    else { coverage = contact_coverage_field(dab, world, field); }
    return coverage * dab.flow
        * length(dab.motion) / max(min(dab.radii.x, dab.radii.y) * 2.0, 0.01)
        * max(style.deformation.y, 0.0);
}

fn dither_cell_coverage(dab: Dab, world: vec2<f32>) -> f32 {
    let origin = floor(world / style.deformation.w) * style.deformation.w;
    let end = min(origin + style.deformation.w, render_target.document_extent.xy);
    if any(end <= origin) { return 0.0; }
    let low = origin + 0.5;
    let high = end - 0.5;
    if brush_selection.info.y == 0u && style.flags.x > 0.5 && !bristles_enabled()
        && !contact_feature(1u, style.contact_a.x > 0.5) && !contact_feature(64u, style.contact_a.z > 0.5) {
        return min(min(contact_coverage(dab, low), contact_coverage(dab, high)),
            min(contact_coverage(dab, vec2<f32>(low.x, high.y)), contact_coverage(dab, vec2<f32>(high.x, low.y))));
    }
    var coverage = 1.0;
    var selected = false;
    for (var y = low.y; y <= high.y; y += 1.0) {
        for (var x = low.x; x <= high.x; x += 1.0) {
            let point = vec2<f32>(x, y);
            if brush_selection_at(point) <= 0.0 { continue; }
            selected = true;
            coverage = min(coverage, contact_coverage(dab, point));
            if coverage <= 0.0 { return 0.0; }
        }
    }
    return select(0.0, coverage, selected);
}

fn dither_sources_in_solid_core(dab: Dab, world: vec2<f32>) -> bool {
    if style.flags.x <= 0.5 || bristles_enabled()
        || contact_feature(1u, style.contact_a.x > 0.5) || contact_feature(64u, style.contact_a.z > 0.5) { return false; }
    let radius = max(min(dab.radii.x, dab.radii.y), 0.005);
    let solid = max(1.0 - max(1.0 - clamp(dab.hardness, 0.0, 1.0), 1.0 / radius), 0.0);
    let reach = clamp(style.deformation.x * style.transport_b.y, 0.0, 112.0)
        + select(1.0, 2.0 * style.deformation.w, DITHER_CELLS);
    let core = solid - reach / radius;
    if core <= 0.0 { return false; }
    let local = rotate(world - dab.center, dab.rotation.x, -dab.rotation.y) / max(dab.radii, vec2<f32>(0.005));
    return dot(local, local) <= core * core;
}

fn smudge_fragment(fragment_position: vec2<f32>, world: vec2<f32>, first: u32, count: u32) -> MaterialOutput {
    let pixel = vec2<i32>(floor(fragment_position));
    var exposure = select(textureLoad(stroke_coverage_texture, pixel, 0), vec4<f32>(0.0), style.canvas_opacity.w > 0.5);
    let edges = style.transport_b.w > 0.5;
    var total = 0.0;
    if edges { total = select(textureLoad(stroke_coverage_texture, pixel + vec2<i32>(0, 512), 0).x, 0.0, style.canvas_opacity.w > 0.5); }
    if exposure.x < 0.0 && !edges { return MaterialOutput(vec4<f32>(0.0), exposure, vec4<f32>(0.0)); }
    let field = contact_field(world);
    var begin = 0u;
    if exposure.x == 0.0 {
        while begin < count && dither_contact_dose(dabs[first + begin], world, field) <= 0.0 { begin += 1u; }
        if begin == count { return MaterialOutput(vec4<f32>(total, 0.0, 0.0, 0.0), exposure, vec4<f32>(0.0)); }
        exposure.x = dither_palette(world);
    }
    if exposure.x < 0.0 {
        if edges {
            for (var index = begin; index < count; index += 1u) {
                total = dither_exposure(vec4<f32>(total), vec4<f32>(dither_contact_dose(dabs[first + index], world, field))).x;
            }
        }
        return MaterialOutput(vec4<f32>(total, 0.0, 0.0, 0.0), exposure, vec4<f32>(0.0));
    }
    var pairs = select(textureLoad(stroke_coverage_texture, pixel + vec2<i32>(0, 256), 0), vec4<f32>(0.0), style.canvas_opacity.w > 0.5);
    let p1 = dither_point(1u, world);
    let p2 = dither_point(2u, world);
    let p3 = dither_point(3u, world);
    let p4 = dither_point(4u, world);
    let p5 = dither_point(5u, world);
    let p6 = dither_point(6u, world);
    let p7 = dither_point(7u, world);
    let f1 = contact_field(p1);
    let f2 = contact_field(p2);
    let f3 = contact_field(p3);
    let f4 = contact_field(p4);
    let f5 = contact_field(p5);
    let f6 = contact_field(p6);
    let f7 = contact_field(p7);
    for (var index = begin; index < count; index += 1u) {
        let dab = dabs[first + index];
        let amount = dither_contact_dose(dab, world, field);
        if amount <= 0.0 { continue; }
        if edges { total = dither_exposure(vec4<f32>(total), vec4<f32>(amount)).x; }
        if dither_sources_in_solid_core(dab, world) {
            exposure = dither_exposure(exposure, amount * vec4<f32>(0.0, 1.0, 1.0, 1.0));
            pairs = dither_exposure(pairs, vec4<f32>(amount));
        } else {
            exposure = dither_exposure(exposure, amount * vec4<f32>(0.0,
                brush_footprint(dab, p1, f1), brush_footprint(dab, p2, f2), brush_footprint(dab, p3, f3)));
            pairs = dither_exposure(pairs, amount * vec4<f32>(
                brush_footprint(dab, p4, f4), brush_footprint(dab, p5, f5),
                brush_footprint(dab, p6, f6), brush_footprint(dab, p7, f7)));
        }
    }
    return MaterialOutput(vec4<f32>(total, 0.0, 0.0, 0.0), exposure, pairs);
}

fn dither_palette(world: vec2<f32>) -> f32 {
    let original = dither_original(world);
    var s0 = DitherSample(dot(original.rgb, WORKING_LUMA), 0u);
    var s1 = dither_sample(1u, world, original);
    var s2 = dither_sample(2u, world, original);
    var s3 = dither_sample(3u, world, original);
    var s4 = dither_sample(4u, world, original);
    var s5 = dither_sample(5u, world, original);
    var s6 = dither_sample(6u, world, original);
    var s7 = dither_sample(7u, world, original);
    if !DITHER_CELLS && ((s1.index | s2.index | s3.index | s4.index | s5.index | s6.index | s7.index) & 8u) == 0u {
        return -1.0;
    }
    dither_order(&s0, &s1, world, original);
    dither_order(&s2, &s3, world, original);
    dither_order(&s4, &s5, world, original);
    dither_order(&s6, &s7, world, original);
    dither_order(&s0, &s2, world, original);
    dither_order(&s1, &s3, world, original);
    dither_order(&s4, &s6, world, original);
    dither_order(&s5, &s7, world, original);
    dither_order(&s1, &s2, world, original);
    dither_order(&s5, &s6, world, original);
    dither_order(&s0, &s4, world, original);
    dither_order(&s3, &s7, world, original);
    dither_order(&s1, &s5, world, original);
    dither_order(&s2, &s6, world, original);
    dither_order(&s1, &s4, world, original);
    dither_order(&s3, &s6, world, original);
    dither_order(&s2, &s4, world, original);
    dither_order(&s3, &s5, world, original);
    dither_order(&s3, &s4, world, original);
    return f32((s0.index & 7u) | ((s1.index & 7u) << 3u) | ((s2.index & 7u) << 6u) | ((s3.index & 7u) << 9u)
        | ((s4.index & 7u) << 12u) | ((s5.index & 7u) << 15u) | ((s6.index & 7u) << 18u) | ((s7.index & 7u) << 21u));
}

fn dither_source_weight(index: u32, world: vec2<f32>, weight: f32) -> f32 {
    let point = dither_point(index, world);
    if any(point < vec2<f32>(0.0)) || any(point >= render_target.document_extent.xy) { return 0.0; }
    if DITHER_CELLS && (all(floor(point / style.deformation.w) == floor(world / style.deformation.w))
        || (style.color.a > 0.5 && !working_has_color(canvas_load(point).a))) { return 0.0; }
    return weight * brush_selection_at(point);
}

fn dither_weight(index: u32, weights: vec4<f32>, pairs: vec4<f32>) -> f32 {
    let low = select(weights.xy, weights.zw, (index & 2u) != 0u);
    let high = select(pairs.xy, pairs.zw, (index & 2u) != 0u);
    let pair = select(low, high, (index & 4u) != 0u);
    return select(pair.x, pair.y, (index & 1u) != 0u);
}

fn dither_choice(world: vec2<f32>, state: MaterialOutput) -> u32 {
    if state.coverage.x <= 0.0 || style.canvas_opacity.z <= 0.0 { return 0u; }
    var weights = dither_weights(vec4<f32>(0.0, state.coverage.yzw));
    let pairs = dither_weights(state.wetness);
    let w1 = dither_source_weight(1u, world, weights[1u]);
    let w2 = dither_source_weight(2u, world, weights[2u]);
    let w3 = dither_source_weight(3u, world, weights[3u]);
    let w4 = dither_source_weight(4u, world, pairs[0u]);
    let w5 = dither_source_weight(5u, world, pairs[1u]);
    let w6 = dither_source_weight(6u, world, pairs[2u]);
    let w7 = dither_source_weight(7u, world, pairs[3u]);
    weights = vec4<f32>(max(1.0 - (w1 + w2 + w3 + w4 + w5 + w6 + w7), 0.0), w1, w2, w3);
    let selected_pairs = vec4<f32>(w4, w5, w6, w7);
    let order = u32(state.coverage.x);
    let threshold = dither_rank(select(world, floor(world / style.deformation.w), DITHER_CELLS), dither_hash(u32(style.transport_b.z)));
    var cumulative = 0.0;
    var chosen = 0u;
    for (var slot = 0u; slot < 8u; slot += 1u) {
        let reference = (order >> (slot * 3u)) & 7u;
        let index = reference & 7u;
        let weight = dither_weight(index, weights, selected_pairs);
        if weight <= 0.0 { continue; }
        chosen = reference;
        cumulative += weight;
        if threshold < cumulative { break; }
    }
    return chosen;
}

fn dither_color(world: vec2<f32>, state: MaterialOutput) -> vec4<f32> {
    let chosen = dither_choice(world, state);
    return dither_paint(chosen | select(0u, 8u, chosen != 0u), world, dither_original(world));
}
