// Four-connected, fixed-seed color region. Original WGSL implementation;
// local union-find, boundary merge, then packed coverage/bounds reduction.
struct Params { extent_seed: vec4<u32>, options: vec4<f32> }
struct Coverage { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
struct Bounds {
    min_x: atomic<u32>, min_y: atomic<u32>, max_x: atomic<u32>, max_y: atomic<u32>,
    count: atomic<u32>,
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read_write> parents: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> coverage: Coverage;
@group(0) @binding(4) var<storage, read_write> bounds: Bounds;

const NONE = 0xffffffffu;
var<workgroup> local_parent: array<atomic<u32>, 256>;

fn comparison_color(value: vec4<f32>) -> vec4<f32> {
    // Ignore hidden RGB in transparent pixels; tolerance includes opacity.
    let straight = value.rgb / max(value.a, .000001);
    let srgb = select(1.055 * pow(max(straight, vec3<f32>(0.)), vec3<f32>(1./2.4)) - .055,
        straight * 12.92, straight <= vec3<f32>(.0031308));
    return vec4<f32>(srgb * value.a, value.a);
}
fn local_root(start: u32) -> u32 {
    var node = start;
    loop {
        let parent = atomicLoad(&local_parent[node]);
        if parent == node { return node; }
        let grand = atomicLoad(&local_parent[parent]);
        atomicMin(&local_parent[node], grand);
        node = grand;
    }
    return node;
}
fn join_local(first: u32, second: u32) {
    var a = first;
    var b = second;
    loop {
        a = local_root(a);
        b = local_root(b);
        if a == b { return; }
        let low = min(a,b);
        let high = max(a,b);
        let previous = atomicMin(&local_parent[high], low);
        if previous == high { return; }
        a = low;
        b = previous;
    }
}

@compute @workgroup_size(16,16)
fn initialize(@builtin(global_invocation_id) id: vec3<u32>,
              @builtin(local_invocation_index) lane: u32,
              @builtin(workgroup_id) group: vec3<u32>) {
    let extent = params.extent_seed.xy;
    let inside = all(id.xy < extent);
    var eligible = false;
    if inside {
        let seed = comparison_color(textureLoad(source, vec2<i32>(params.extent_seed.zw), 0));
        let color = comparison_color(textureLoad(source, vec2<i32>(id.xy), 0));
        eligible = all(abs(color-seed) <= vec4<f32>(params.options.x)) && brush_selection_at(vec2<f32>(id.xy)+.5) > 0.;
    }
    atomicStore(&local_parent[lane], select(NONE, lane, eligible));
    if all(id.xy == vec2<u32>(0)) {
        coverage.rect = vec4<u32>(0,0,extent);
        coverage.info = vec4<u32>(0,1,0,0);
        atomicStore(&bounds.min_x, extent.x);
        atomicStore(&bounds.min_y, extent.y);
        atomicStore(&bounds.max_x, 0);
        atomicStore(&bounds.max_y, 0);
        atomicStore(&bounds.count, 0);
    }
    workgroupBarrier();
    if eligible {
        if lane % 16u != 0u && atomicLoad(&local_parent[lane-1u]) != NONE { join_local(lane,lane-1u); }
        if lane >= 16u && atomicLoad(&local_parent[lane-16u]) != NONE { join_local(lane,lane-16u); }
    }
    workgroupBarrier();
    if inside {
        var label = NONE;
        if eligible {
            let root = local_root(lane);
            let p = group.xy * 16u + vec2<u32>(root % 16u, root / 16u);
            label = p.y * extent.x + p.x;
        }
        atomicStore(&parents[id.y * extent.x + id.x], label);
    }
}

fn root(start: u32) -> u32 {
    var node = start;
    loop {
        let parent = atomicLoad(&parents[node]);
        if parent == node || parent == NONE { return parent; }
        let grand = atomicLoad(&parents[parent]);
        atomicMin(&parents[node], grand);
        node = grand;
    }
    return node;
}
fn join(first: u32, second: u32) {
    var a = first;
    var b = second;
    loop {
        a = root(a);
        b = root(b);
        if a == b || a == NONE || b == NONE { return; }
        let low = min(a,b);
        let high = max(a,b);
        let previous = atomicMin(&parents[high], low);
        if previous == high { return; }
        a = low;
        b = previous;
    }
}

@compute @workgroup_size(64)
fn merge(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let w = params.extent_seed.x;
    let h = params.extent_seed.y;
    let horizontal = w * ((h-1u)/16u);
    let vertical = h * ((w-1u)/16u);
    let i = id.x + id.y * groups.x * 64u;
    if i < horizontal {
        let y = (i/w+1u)*16u;
        let below = y*w + i%w;
        join(below, below-w);
    } else if i < horizontal+vertical {
        let j = i-horizontal;
        let x = (j/h+1u)*16u;
        let right = (j%h)*w+x;
        join(right, right-1u);
    }
}

var<workgroup> lows: array<vec2<u32>,64>;
var<workgroup> highs: array<vec2<u32>,64>;
var<workgroup> counts: array<u32,64>;

@compute @workgroup_size(64)
fn pack(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane: u32,
        @builtin(num_workgroups) groups: vec3<u32>) {
    let extent = params.extent_seed.xy;
    let stride = (extent.x+7u)/8u;
    let word = id.x + id.y * groups.x * 64u;
    let y = word/stride;
    let x = (word%stride)*8u;
    let selected = root(params.extent_seed.w * extent.x + params.extent_seed.z);
    var packed = 0u;
    var low = extent;
    var high = vec2<u32>(0);
    var count = 0u;
    if y < extent.y {
        for (var i = 0u; i < 8u && x+i < extent.x; i++) {
            if selected != NONE && root(y*extent.x+x+i) == selected {
                packed |= u32(round(brush_selection_at(vec2<f32>(f32(x+i)+.5, f32(y)+.5))*4.)) << (i*4u);
                low = min(low, vec2<u32>(x+i,y));
                high = max(high, vec2<u32>(x+i+1u,y+1u));
                count++;
            }
        }
        coverage.values[word] = packed;
    }
    lows[lane] = low;
    highs[lane] = high;
    counts[lane] = count;
    workgroupBarrier();
    for (var step = 32u; step > 0u; step /= 2u) {
        if lane < step {
            lows[lane] = min(lows[lane], lows[lane+step]);
            highs[lane] = max(highs[lane], highs[lane+step]);
            counts[lane] += counts[lane+step];
        }
        workgroupBarrier();
    }
    if lane == 0u && counts[0] != 0u {
        atomicMin(&bounds.min_x, lows[0].x);
        atomicMin(&bounds.min_y, lows[0].y);
        atomicMax(&bounds.max_x, highs[0].x);
        atomicMax(&bounds.max_y, highs[0].y);
        atomicAdd(&bounds.count, counts[0]);
    }
}
