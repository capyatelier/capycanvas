// Exact box footprints in document coordinates, with linear premultiplied sums.
// Separable integration bounds each serial loop to one 256px source tile.
struct Tile { origin_extent: vec4<u32>, contribution: vec4<u32>, options: vec4<u32> }
@group(0) @binding(0) var<uniform> tile: Tile;
@group(0) @binding(1) var pixels: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> contributions: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> rows: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> sums: array<vec4<f32>>;
fn footprint(index: u32, axis: u32) -> vec2<f32> {
    let extent = vec2<f32>(tile.origin_extent.zw);
    let side = max(extent.x, extent.y);
    let step = side / 32.;
    let start = f32(index) * step + (extent[axis] - side) * .5;
    return vec2(max(start, 0.), min(start + step, extent[axis]));
}
@compute @workgroup_size(8, 8)
fn horizontal(@builtin(global_invocation_id) gid: vec3<u32>) {
    let span = footprint(gid.x, 0u) - f32(tile.origin_extent.x);
    let low = max(i32(floor(span.x)), 0);
    let high = min(i32(ceil(span.y)), 256);
    var sum = vec4<f32>(0.);
    if gid.y + tile.origin_extent.y < tile.origin_extent.w {
        for (var x = low; x < high; x++) {
            let weight = max(0., min(span.y, f32(x + 1)) - max(span.x, f32(x)));
            let p = vec2<i32>(x, i32(gid.y));
            let value = textureLoad(pixels, p, 0);
            sum += value * weight;
        }
    }
    rows[gid.y * 32u + gid.x] = sum;
}
@compute @workgroup_size(8, 8)
fn vertical(@builtin(global_invocation_id) gid: vec3<u32>) {
    let span = footprint(gid.y, 1u) - f32(tile.origin_extent.y);
    let low = max(i32(floor(span.x)), 0);
    let high = min(i32(ceil(span.y)), 256);
    var sum = vec4<f32>(0.);
    for (var y = low; y < high; y++) {
        let weight = max(0., min(span.y, f32(y + 1)) - max(span.x, f32(y)));
        sum += rows[u32(y) * 32u + gid.x] * weight;
    }
    let step = f32(max(tile.origin_extent.z, tile.origin_extent.w)) / 32.;
    let at = gid.y * 32u + gid.x;
    var value = sum / (step * step);
    let relative = gid.xy - tile.contribution.xy;
    if all(gid.xy >= tile.contribution.xy) && all(relative < tile.contribution.zw) {
        let index = tile.options.y + relative.y * tile.contribution.z + relative.x;
        if tile.options.x == 0u { contributions[index] = value; }
        else { value -= contributions[index]; }
    }
    sums[at] += value;
}
