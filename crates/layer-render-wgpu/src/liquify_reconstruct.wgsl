struct Target { origin_extent: vec4<f32>, document_extent: vec4<f32> }
struct Sources { header: vec4<u32>, pages: array<vec4<u32>, 9> }
@group(0) @binding(0) var<uniform> style: Style;
@group(0) @binding(1) var output_color: texture_storage_2d<rgba32float, write>;
@group(1) @binding(0) var<uniform> render_target: Target;
@group(2) @binding(11) var<uniform> sources: Sources;
@group(2) @binding(12) var field: texture_2d<u32>;
@group(2) @binding(13) var previous_color: texture_2d<f32>;
@group(2) @binding(14) var original_color: texture_2d<f32>;
struct SourceIndices { extent: vec4<u32>, tiles: array<u32> }
@group(2) @binding(15) var<storage, read> source_indices: SourceIndices;
@group(2) @binding(16) var current_color: texture_2d<f32>;
@group(2) @binding(17) var prior_map: texture_2d<u32>;

fn source_pixel(pixel: vec2<i32>, level: u32) -> vec4<f32> {
    if any(pixel < vec2(0)) { return vec4(0.); }
    let tile_size = 256u >> level;
    let page = vec2<u32>(pixel) / tile_size;
    if any(page >= source_indices.extent.xy) { return vec4(0.); }
    let stored = source_indices.tiles[page.y * source_indices.extent.x + page.x];
    if stored == 0u { return vec4(0.); }
    let slot = stored - 1u;
    let atlas = slot / sources.pages[0].y;
    let first = sources.pages[0].x;
    if atlas < first || atlas >= first + sources.header.x { return vec4(0.); }
    return load_source(atlas - first, pixel % i32(tile_size), i32(slot % sources.pages[0].y), i32(level));
}

fn source_bilinear(at: vec2<f32>, level: u32) -> vec4<f32> {
    let coordinate = at / f32(1u << level) - .5;
    let pixel = vec2<i32>(floor(coordinate));
    let t = fract(coordinate);
    if all(t == vec2(0.)) { return source_pixel(pixel, level); }
    let tile_size = i32(256u >> level);
    let local = pixel % tile_size;
    if all(pixel >= vec2(0)) && all(local + vec2(1) < vec2(tile_size)) {
        let page = vec2<u32>(pixel / tile_size);
        if any(page >= source_indices.extent.xy) { return vec4(0.); }
        let stored = source_indices.tiles[page.y * source_indices.extent.x + page.x];
        if stored == 0u { return vec4(0.); }
        let slot = stored - 1u;
        let atlas = slot / sources.pages[0].y;
        let first = sources.pages[0].x;
        if atlas < first || atlas >= first + sources.header.x { return vec4(0.); }
        return sample_source(atlas - first, local, t, i32(slot % sources.pages[0].y), level);
    }
    return mix(mix(source_pixel(pixel, level), source_pixel(pixel + vec2(1, 0), level), t.x),
        mix(source_pixel(pixel + vec2(0, 1), level), source_pixel(pixel + vec2(1, 1), level), t.x), t.y);
}

fn footprint_length(v: vec2<f32>) -> f32 {
    let scale = max(max(abs(v.x), abs(v.y)), 1.);
    return min(scale, 1.e20) * length(v / scale);
}

fn prior_value(pixel: vec2<i32>, block: u32) -> vec4<u32> {
    if sources.pages[0].w == 0u { return vec4(0u, 0u, 16256u, 1065353216u); }
    if block == 1u { return textureLoad(prior_map, pixel, 0); }
    let at = pixel * i32(block) + i32(block / 2u) - 1;
    var value = MapSample(vec2(0.), mat2x2(vec2(0.), vec2(0.)));
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let sample = unpack_map(textureLoad(prior_map, at + vec2(x, y), 0));
            value = MapSample(value.displacement + sample.displacement * .25, value.footprint + (sample.footprint - IDENTITY) * .25);
        }
    }
    return pack_map(MapSample(value.displacement, value.footprint + IDENTITY));
}

fn current_average(pixel: vec2<i32>, block: u32) -> vec4<f32> {
    var color = vec4(0.);
    for (var y = 0u; y < block; y++) {
        for (var x = 0u; x < block; x++) {
            color += textureLoad(current_color, pixel * i32(block) + vec2<i32>(i32(x), i32(y)), 0);
        }
    }
    return color / f32(block * block);
}

@compute @workgroup_size(16, 8)
fn reconstruct(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let pixel = vec2<i32>(invocation.xy);
    let block = style.operation.z;
    if sources.pages[0].z == 0u && (any(invocation.xy < sources.pages[1].xy) || any(invocation.xy >= sources.pages[1].zw)) { return; }
    let packed = textureLoad(field, pixel, 0);
    if sources.pages[0].z == 1u && style.deformation.z < 0. {
        if all(packed == prior_value(pixel, block)) {
            textureStore(output_color, pixel, current_average(pixel, block));
            return;
        }
    }
    let displacement = bitcast<vec2<f32>>(packed.xy);
    var dx = unpack_footprint(packed.z);
    var dy = unpack_footprint(packed.w);
    let at = render_target.origin_extent.xy + (vec2<f32>(invocation.xy) + .5) * f32(block) + displacement;
    let bounds = bitcast<vec4<f32>>(sources.pages[2]);
    let radius = .5 * abs(dx) + .5 * abs(dy);
    let available = max(min(at - bounds.xy, bounds.zw - at), vec2(0.));
    let fit = min(vec2(f32(block)), available / max(radius, vec2(1.e-20)));
    let factor = min(fit.x, fit.y);
    dx *= factor;
    dy *= factor;
    let lengths = vec2(footprint_length(dx), footprint_length(dy));
    let cap = f32(max(sources.header.w, 1u));
    let scale = max(max(lengths.x, lengths.y) / cap, sqrt(lengths.x) * sqrt(lengths.y) / sqrt(cap));
    var level = u32(clamp(ceil(log2(max(scale, 1.))), 0., 8.));
    var count = vec2<u32>(clamp(floor(lengths / f32(1u << level) + .5), vec2(1.), vec2(cap)));
    if count.x * count.y > u32(cap) {
        level = min(level + 1u, 8u);
        count = vec2<u32>(clamp(floor(lengths / f32(1u << level) + .5), vec2(1.), vec2(cap)));
        count.y = min(count.y, max(u32(cap) / count.x, 1u));
    }
    var color = vec4(0.);
    for (var y = 0u; y < count.y; y++) {
        for (var x = 0u; x < count.x; x++) {
            let offset = (vec2(f32(x), f32(y)) + .5) / vec2<f32>(count) - .5;
            color += source_bilinear(at + dx * offset.x + dy * offset.y, level);
        }
    }
    color /= f32(count.x * count.y);
    if sources.header.y != 0u { color += textureLoad(previous_color, pixel, 0); }
    if sources.header.z != 0u {
        if style.color.a > .5 {
            let original = textureLoad(original_color, pixel, i32(log2(f32(block))));
            if color.a > 0. { color = vec4(color.rgb / color.a * original.a, original.a); }
            else { color = original; }
        }
        color = layer_color(color, WORKING_LUMA, style.color_mode.x, style.color_mode.y);
        color = working_clamp(color);
    }
    textureStore(output_color, pixel, color);
}
