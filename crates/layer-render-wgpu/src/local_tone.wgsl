// Coverage-aware local-Laplacian analysis. All image arithmetic stays Float32.
// Four storage bindings, no atomics, subgroups or filterable float textures.
struct Params { size: vec4<u32>, aux: vec4<u32>, values: vec4<f32>, weights: vec4<f32> }
struct Plane { pixels: array<vec4<f32>> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> a: Plane;
@group(0) @binding(2) var<storage, read> b: Plane;
@group(0) @binding(3) var<storage, read> c: Plane;
@group(0) @binding(4) var<storage, read_write> dst: Plane;
@group(0) @binding(5) var source: texture_2d<f32>;

@compute @workgroup_size(8, 8)
fn reduce_source(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = id.xy + p.size.zw;
    if any(cell >= p.size.xy) { return; }
    let scale = vec2<f32>(p.size.xy) / vec2<f32>(p.aux.zw);
    // Inverse coordinates can round across a cell boundary. Include one extra
    // source pixel at each edge; the exact forward area gives it zero weight.
    let start = max(vec2<u32>(max(vec2<i32>(floor(vec2<f32>(cell) / scale)) - 1, vec2<i32>(0))), p.aux.xy);
    let end = min(vec2<u32>(ceil(vec2<f32>(cell + 1u) / scale)) + 1u, p.aux.xy + textureDimensions(source));
    var sum = dst.pixels[cell.y * p.size.x + cell.x];
    for (var y = start.y; y < end.y; y++) {
        let wy = max(0.0, min(f32(y + 1u) * scale.y, f32(cell.y + 1u)) - max(f32(y) * scale.y, f32(cell.y)));
        for (var x = start.x; x < end.x; x++) {
            let pixel = textureLoad(source, vec2<i32>(vec2<u32>(x, y) - p.aux.xy), 0);
            if any((bitcast<vec4<u32>>(pixel) & vec4<u32>(0x7fffffffu)) >= vec4<u32>(0x7f800000u)) || pixel.a < 0.0 || pixel.a > 1.0 {
                sum.w = 1.0;
                continue;
            }
            if pixel.a == 0.0 { continue; }
            let luminance = dot(pixel.rgb, p.weights.xyz) / pixel.a;
            let weight = wy * max(0.0, min(f32(x + 1u) * scale.x, f32(cell.x + 1u)) - max(f32(x) * scale.x, f32(cell.x)));
            sum.x += log2(max(luminance, exp2(-24.0))) * weight * pixel.a;
            sum.y += weight * pixel.a;
            sum.z = max(sum.z, luminance);
        }
    }
    dst.pixels[cell.y * p.size.x + cell.x] = sum;
}

@compute @workgroup_size(8, 8)
fn normalize(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    let s = a.pixels[i];
    var value = -24.0;
    if s.y > 0.0 { value = s.x / s.y; }
    dst.pixels[i] = vec4<f32>(value, min(s.y, 1.0), max(1.0, s.z), s.w);
}

var<workgroup> ranges: array<vec4<f32>, 256>;
@compute @workgroup_size(256)
fn range_reduce(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane: u32, @builtin(workgroup_id) group: vec3<u32>) {
    var v = vec4<f32>(3.402823e38, -3.402823e38, 1.0, 0.0);
    if id.x < p.size.x {
        let sample = a.pixels[id.x];
        if p.aux.x == 0u {
            if sample.y > 0.0 { v = vec4<f32>(sample.xx, v.zw); }
            v = vec4<f32>(v.xy, sample.zw);
        } else { v = sample; }
    }
    ranges[lane] = v;
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride /= 2u) {
        if lane < stride {
            let rhs = ranges[lane + stride];
            ranges[lane] = vec4<f32>(min(ranges[lane].x, rhs.x), max(ranges[lane].yzw, rhs.yzw));
        }
        workgroupBarrier();
    }
    if lane == 0u { dst.pixels[group.x] = ranges[0]; }
}

// p.aux.xy is the source extent; aux.z selects horizontal/vertical.
@compute @workgroup_size(8, 8)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let kernel = array<f32, 5>(1.0, 4.0, 6.0, 4.0, 1.0);
    var value = 0.0;
    var coverage = 0.0;
    for (var k = 0u; k < 5u; k++) {
        var point = vec2<i32>(id.xy);
        if p.aux.z == 0u { point.x = point.x * 2 + i32(k) - 2; }
        else { point.y = point.y * 2 + i32(k) - 2; }
        point = clamp(point, vec2<i32>(0), vec2<i32>(p.aux.xy) - 1);
        let s = a.pixels[u32(point.y) * p.aux.x + u32(point.x)];
        let w = s.y * kernel[k];
        value += s.x * w;
        coverage += w;
    }
    if coverage > 0.0 { value /= coverage; }
    dst.pixels[id.y * p.size.x + id.x] = vec4<f32>(value, coverage / 16.0, 0.0, 0.0);
}

@compute @workgroup_size(8, 8)
fn remap(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    let s = a.pixels[i];
    let d = (s.x - p.values.x) / 1.5;
    dst.pixels[i] = vec4<f32>(1.5 * d / (1.0 + abs(d)), s.y, 0.0, 0.0);
}

// Coarse plane is always binding c, including its coverage.
fn expanded(point: vec2<f32>) -> f32 {
    let position = clamp(point, vec2<f32>(0.0), vec2<f32>(p.aux.xy - 1u));
    let base = vec2<u32>(floor(position));
    let fraction = fract(position);
    var value = 0.0;
    var coverage = 0.0;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let at = min(base + vec2<u32>(x, y), p.aux.xy - 1u);
            let s = c.pixels[at.y * p.aux.x + at.x];
            let w = select(1.0 - fraction.x, fraction.x, x == 1u) * select(1.0 - fraction.y, fraction.y, y == 1u) * s.y;
            value += s.x * w;
            coverage += w;
        }
    }
    if coverage > 0.0 { return value / coverage; }
    return 0.0;
}

@compute @workgroup_size(8, 8)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    let original = a.pixels[i];
    let q = clamp((original.x - p.values.x) / p.values.y, 0.0, p.values.z);
    let weight = max(1.0 - abs(q - p.values.w), 0.0);
    var detail = dst.pixels[i].x;
    if weight > 0.0 { detail += weight * (b.pixels[i].x - expanded(vec2<f32>(id.xy) * 0.5)); }
    dst.pixels[i] = vec4<f32>(detail, original.y, 0.0, 0.0);
}

@compute @workgroup_size(8, 8)
fn reconstruct(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    dst.pixels[i] = vec4<f32>(dst.pixels[i].x + expanded(vec2<f32>(id.xy) * 0.5), a.pixels[i].y, 0.0, 0.0);
}

@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    let s = a.pixels[i];
    // The first vec4 contains the present shader's integer geometry header.
    if i == 0u { dst.pixels[0] = bitcast<vec4<f32>>(vec4<u32>(p.size.xy, p.aux.xy)); }
    dst.pixels[i + 1u] = vec4<f32>(s.x, s.x - b.pixels[i].x, s.y, 0.0);
}

@compute @workgroup_size(8, 8)
fn init_detail(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= p.size.xy) { return; }
    let i = id.y * p.size.x + id.x;
    dst.pixels[i] = vec4<f32>(0.0, a.pixels[i].y, 0.0, 0.0);
}
