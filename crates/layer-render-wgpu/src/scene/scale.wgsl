// The two kernels share queue-ordered region records. Reduction reads exact
// premultiplied paint; only presentation ever consumes the resulting pixels.
struct Region {
    origin: vec2<u32>,
    size: vec2<u32>,
    extent: vec2<u32>,
    side: u32,
    flags: u32,
    paper: vec4<f32>,
    opacity: vec4<f32>,
    source_origin: vec2<i32>,
    base_origin: vec2<i32>,
}
@group(0) @binding(0) var<uniform> region: Region;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var base: texture_2d<f32>;
@group(1) @binding(2) var output: texture_storage_2d<rgba32float, write>;
@group(1) @binding(3) var source_preview: texture_2d<f32>;
@group(1) @binding(4) var base_preview: texture_2d<f32>;
@group(1) @binding(5) var area_sampler: sampler;

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    if (region.flags & 4u) != 0u {
        textureStore(output, vec2<i32>(region.origin + id.xy), region.paper);
        return;
    }
    let start = vec2<u32>(vec2<i32>((id.xy + select(vec2<u32>(0u), region.origin, (region.flags & 8u) != 0u)) * region.side) - vec2<i32>(region.opacity.zw));
    let step = 1u << (region.flags >> 8u);
    let remaining = region.extent - start * step;
    let size = min(vec2<u32>(region.side), (remaining + step - 1u) / step);
    let dimensions = textureDimensions(source);
    let base_dimensions = textureDimensions(base);
    if (region.flags & ~128u) == 0u && region.side % 2u == 0u && all(size == vec2(region.side))
        && all((dimensions & (dimensions - 1u)) == vec2(0u))
        && all((base_dimensions & (base_dimensions - 1u)) == vec2(0u)) {
        var sum = vec4(0.);
        for (var y = 1u; y < region.side; y += 2u) {
            for (var x = 1u; x < region.side; x += 2u) {
                let p = vec2<f32>(start + vec2(x, y));
                var value = textureSampleLevel(source, area_sampler, p / vec2<f32>(dimensions), 0.);
                if (region.flags & 128u) != 0u {
                    value -= textureSampleLevel(base, area_sampler, p / vec2<f32>(base_dimensions), 0.);
                }
                sum += value;
            }
        }
        textureStore(output, vec2<i32>(region.origin + id.xy), sum * (4. / f32(region.side * region.side)));
        return;
    }
    var sum = vec4<f32>(0.);
    var weight = 0u;
    for (var y = 0u; y < size.y; y++) {
        for (var x = 0u; x < size.x; x++) {
            let p = vec2<i32>(start + vec2<u32>(x, y));
            var value = textureLoad(source, p, 0);
            if (region.flags & 32u) != 0u {
                value = vec4<f32>(select(value.r, 1. - value.r, (region.flags & 64u) != 0u));
            }
            // A flow preview is a transparent contribution. Resolve it before
            // reducing, so layer opacity is applied only once.
            if (region.flags & 1u) != 0u {
                value += textureLoad(base, p, 0) * (1. - value.a);
            }
            if (region.flags & 2u) != 0u { value = working_encode(value); }
            if (region.flags & 128u) != 0u {
                var pigment = textureLoad(base, p, 0);
                if (region.flags & 2u) != 0u { pigment = working_encode(pigment); }
                value -= pigment;
            }
            // A final compact texel may represent fewer document pixels.
            // Weight its average accordingly at odd document boundaries.
            let footprint = min(vec2<u32>(step), remaining - vec2<u32>(x, y) * step);
            let count = footprint.x * footprint.y;
            sum += value * f32(count);
            weight += count;
        }
    }
    textureStore(output, vec2<i32>(region.origin + id.xy), sum / f32(weight));
}

fn phased_load(page: u32, texel: vec2<i32>) -> vec4<f32> {
    switch page {
        case 0u: { return textureLoad(source, texel, 0); }
        case 1u: { return textureLoad(base, texel, 0); }
        case 2u: { return textureLoad(source_preview, texel, 0); }
        default: { return textureLoad(base_preview, texel, 0); }
    }
}
fn phased_sample(page: u32, texel: vec2<f32>) -> vec4<f32> {
    switch page {
        case 0u: { return textureSampleLevel(source, area_sampler, texel / vec2<f32>(textureDimensions(source)), 0.); }
        case 1u: { return textureSampleLevel(base, area_sampler, texel / vec2<f32>(textureDimensions(base)), 0.); }
        case 2u: { return textureSampleLevel(source_preview, area_sampler, texel / vec2<f32>(textureDimensions(source_preview)), 0.); }
        default: { return textureSampleLevel(base_preview, area_sampler, texel / vec2<f32>(textureDimensions(base_preview)), 0.); }
    }
}
// Level pixels lie `opacity.xy` pixels after the layer's own, so a level page
// spans four layer pages; `paper` holds each page's level, or none. Each
// thread reduces a texel between `opacity.z` and `opacity.w` (16-bit pairs):
// over reduced predictions from the prediction texels nearest its pixels,
// otherwise with filtered taps inside a page or pixel by pixel across edges.
@compute @workgroup_size(8, 8)
fn reduce_phased(@builtin(global_invocation_id) id: vec3<u32>) {
    let texel = vec2(bitcast<u32>(region.opacity.z) & 0xffffu, bitcast<u32>(region.opacity.z) >> 16u) + id.xy;
    if any(texel >= vec2(bitcast<u32>(region.opacity.w) & 0xffffu, bitcast<u32>(region.opacity.w) >> 16u)) { return; }
    let side = region.side;
    let first = texel * side;
    let size = min(vec2(side), region.extent - first);
    let start = vec2<i32>(256u - vec2(bitcast<u32>(region.opacity.x), bitcast<u32>(region.opacity.y)) + first);
    let words = bitcast<vec4<u32>>(region.paper);
    let full = all(size == vec2(side));
    let cell = 1u << select(0u, max(max(words.x, words.y), max(words.z, words.w)) & 15u, full);
    let page = start / 256;
    var sum = vec4(0.);
    var count: f32;
    if cell == 1u && (region.flags & 2u) == 0u && full && all(page == (start + vec2<i32>(size) - 1) / 256) {
        let index = u32(page.y * 2 + page.x);
        count = f32(side * side / 4u);
        if (bitcast<u32>(region.paper[index]) & 16u) != 0u {
            let at = vec2<f32>(start - page * 256) + 1.;
            for (var y = 0u; y < side; y += 2u) {
                for (var x = 0u; x < side; x += 2u) { sum += phased_sample(index, at + vec2<f32>(vec2(x, y))); }
            }
        }
    } else {
        let snapped = (start + i32(cell / 2u)) & vec2(-i32(cell));
        let units = (size + cell - 1u) / cell;
        for (var y = 0u; y < units.y; y++) {
            for (var x = 0u; x < units.x; x++) {
                let local = snapped + vec2<i32>(vec2(x, y) * cell);
                let at = local / 256;
                let index = u32(at.y * 2 + at.x);
                let word = bitcast<u32>(region.paper[index]);
                if (word & 16u) == 0u { continue; }
                var value = phased_load(index, (local - at * 256 + i32(cell / 2u)) >> vec2(word & 15u));
                if (region.flags & 2u) != 0u { value = working_encode(value); }
                sum += value;
            }
        }
        count = f32(units.x * units.y);
    }
    textureStore(output, vec2<i32>(region.origin + texel), sum / count);
}

@compute @workgroup_size(8, 8)
fn reduce_pair(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    let dst = region.origin + id.xy;
    let start = dst * 2u - vec2<u32>(region.opacity.zw);
    let step = 1u << (region.flags >> 8u);
    let footprint = min(vec2<u32>(step * 2u), region.extent - start * step);
    let first = min(vec2<u32>(step), footprint);
    let second = footprint - first;
    let weight = vec4<f32>(vec4<u32>(first.x * first.y, second.x * first.y, first.x * second.y, second.x * second.y));
    let p = vec2<i32>(start);
    let sum = textureLoad(source, p, 0) * weight.x
        + textureLoad(source, p + vec2(1, 0), 0) * weight.y
        + textureLoad(source, p + vec2(0, 1), 0) * weight.z
        + textureLoad(source, p + vec2(1, 1), 0) * weight.w;
    textureStore(output, vec2<i32>(dst), sum / f32(footprint.x * footprint.y));
}
