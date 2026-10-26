struct Target { origin_extent: vec4<f32>, document_extent: vec4<f32> }
struct Sources { header: vec4<u32>, pages: array<vec4<u32>, 9> }
@group(0) @binding(0) var<uniform> style: Style;
@group(0) @binding(1) var output_map: texture_storage_2d<rgba32uint, write>;
@group(0) @binding(2) var<storage, read> dabs: array<Dab>;
@group(1) @binding(0) var<uniform> render_target: Target;

fn contact_coverage(dab: Dab, at: vec2<f32>) -> f32 {
    return brush_footprint(dab, at, vec2(0.)) * brush_selection_at(at);
}

fn map_correction(pixel: vec2<i32>) -> MapSample {
    if any(pixel < vec2(0)) { return MapSample(vec2(0.), mat2x2(vec2(0.), vec2(0.))); }
    let page = vec2<u32>(pixel) / 256u;
    for (var i = 0u; i < sources.header.x; i++) {
        var coordinate: vec2<u32>;
        if MAP_BINDINGS == 9u { coordinate = sources.pages[i].xy; }
        else {
            let pair = sources.pages[i / 2u];
            coordinate = select(pair.xy, pair.zw, i % 2u == 1u);
        }
        if all(page == coordinate) {
            let value = unpack_map(load_source(i, pixel % 256));
            return MapSample(value.displacement, value.footprint - IDENTITY);
        }
    }
    return MapSample(vec2(0.), mat2x2(vec2(0.), vec2(0.)));
}

fn map_bilinear(at: vec2<f32>) -> MapSample {
    let p = vec2<i32>(floor(at - .5));
    let t = fract(at - .5);
    var displacement = vec2(0.);
    var footprint = mat2x2(vec2(0.), vec2(0.));
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let weight = select(1. - t.x, t.x, x == 1) * select(1. - t.y, t.y, y == 1);
            if weight == 0. { continue; }
            let value = map_correction(p + vec2(x, y));
            displacement += value.displacement * weight;
            footprint += value.footprint * weight;
        }
    }
    return MapSample(displacement, footprint);
}

fn unchanged_map(pixel: vec2<i32>) -> vec4<u32> {
    var packed = vec4(0u, 0u, 16256u, 1065353216u);
    if sources.header.y != 0u { packed = textureLoad(previous_map, pixel, 0); }
    if sources.header.w != 0u { packed = load_source(sources.header.w - 1u, pixel); }
    return packed;
}

@compute @workgroup_size(16, 8)
fn update_map(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let pixel = vec2<i32>(invocation.xy);
    let bounds = select(sources.pages[0].zw, sources.pages[6].xy, MAP_BINDINGS == 12u);
    let minimum = vec2(bounds.x & 65535u, bounds.x >> 16u);
    let maximum = vec2(bounds.y & 65535u, bounds.y >> 16u);
    if style.operation.z == 1u && (any(invocation.xy < minimum) || any(invocation.xy >= maximum)) {
        textureStore(output_map, pixel, unchanged_map(pixel));
        return;
    }
    let initial = render_target.origin_extent.xy + (vec2<f32>(invocation.xy) + .5) * f32(style.operation.z);
    let trace = trace_liquify(initial);
    if style.operation.z == 1u && all(trace.position == initial) && trace.restore == 1.
        && all(trace.footprint[0] == IDENTITY[0]) && all(trace.footprint[1] == IDENTITY[1]) && all(trace.gradient == vec2(0.)) {
        textureStore(output_map, pixel, unchanged_map(pixel));
        return;
    }
    var value = map_bilinear(trace.position);
    if sources.header.y != 0u {
        let previous = unpack_map(textureLoad(previous_map, pixel, 0));
        value = MapSample(value.displacement + previous.displacement, value.footprint + previous.footprint);
    }
    if sources.header.z != 0u {
        value = compose_liquify(initial, trace, MapSample(value.displacement, value.footprint + IDENTITY));
    }
    textureStore(output_map, pixel, pack_map(value));
}
