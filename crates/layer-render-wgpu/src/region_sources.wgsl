// Each dispatch consumes one raw tile in queue order before cache reuse. One
// invocation writes 32 eligibility bits; no full-resolution color target exists.
struct Tile { extent: vec2<u32>, origin: vec2<u32>, position: vec2<u32>, size: vec2<u32>,
              fallback: vec4<f32>, options: vec4<f32> }
struct Batch { tiles: array<Tile,16> }
@group(0) @binding(16) var<uniform> batch: Batch;
@group(0) @binding(17) var<storage, read_write> seed: vec4<f32>;
@group(0) @binding(18) var<storage, read_write> eligibility: array<u32>;

fn raw_color(index: u32, p: vec2<u32>) -> vec4<f32> {
    let tile = batch.tiles[index];
    if tile.options.y == 0. { return tile.fallback; }
    return tile_load(index, vec2<i32>(p));
}
@compute @workgroup_size(1)
fn sample_seed() { seed = raw_color(0u, batch.tiles[0].position); }

@compute @workgroup_size(64)
fn classify_tile(@builtin(global_invocation_id) id: vec3<u32>) {
    let tile = batch.tiles[id.z];
    if tile.options.z > .5 {
        let stride = (tile.size.x + 3u)/4u;
        let p = vec2<u32>((id.x%stride)*4u,id.x/stride);
        if p.y >= tile.size.y { return; }
        var packed = 0u;
        for(var i=0u;i<4u && p.x+i<tile.size.x;i++) {
            let color = raw_color(id.z,p+vec2<u32>(i,0u));
            var value = color.a;
            if tile.options.z > 1.5 { value = color.r; }
            if tile.options.w > .5 { value=1.-value; }
            packed |= u32(round(clamp(value,0.,1.)*255.)) << (i*8u);
        }
        eligibility[8u+(tile.origin.y+p.y)*((tile.extent.x+3u)/4u)+(tile.origin.x+p.x)/4u]=packed;
        return;
    }
    let stride = (tile.size.x + 31u) / 32u;
    let p = vec2<u32>((id.x % stride) * 32u, id.x / stride);
    if p.y >= tile.size.y { return; }
    var packed = 0u;
    for (var i = 0u; i < 32u && p.x+i < tile.size.x; i++) {
        let local = p + vec2<u32>(i, 0);
        let world = tile.origin + local;
        let value = raw_color(id.z, local);
        // Compare the seed and candidates in the same shader. Encoding only
        // the seed in another pipeline can differ by one Float32 rounding step
        // and exclude even the seed itself at zero tolerance.
        let same = all(value == seed);
        let color = comparison_color(value);
        if (same || all(abs(color - comparison_color(seed)) <= vec4<f32>(tile.options.x)))
            && brush_selection_at(vec2<f32>(world) + .5) > 0. {
            packed |= 1u << i;
        }
    }
    eligibility[(tile.origin.y+p.y)*((tile.extent.x+31u)/32u)+(tile.origin.x+p.x)/32u] = packed;
}
