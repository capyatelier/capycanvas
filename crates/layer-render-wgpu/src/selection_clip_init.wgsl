struct Params { rect: vec4<u32>, info: vec4<u32> }
struct PackedSelection { rect: vec4<u32>, info: vec4<u32>, values: array<atomic<u32>> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> edges: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: PackedSelection;

// Mark even/odd crossing events for each of the four coverage samples. Work
// scales with edge height, not with every pixel times every polygon edge.
@compute @workgroup_size(64)
fn crossings(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let e = edges[group.x];
    let first = u32(clamp(floor(min(e.y, e.w) - f32(params.rect.y)), 0., f32(params.rect.w)));
    let end = u32(clamp(ceil(max(e.y, e.w) - f32(params.rect.y)), 0., f32(params.rect.w)));
    let stride = (params.rect.z + 7u) / 8u;
    for (var y = first + lane; y < end; y += 64u) {
        for (var sy = 0u; sy < 2u; sy++) {
            let py = f32(y + params.rect.y) + .25 + f32(sy) * .5;
            if (e.y > py) == (e.w > py) { continue; }
            let cross = selection_crossing(e, py) - f32(params.rect.x);
            for (var sx = 0u; sx < 2u; sx++) {
                let x = u32(clamp(ceil(cross - .25 - f32(sx) * .5), 0., f32(params.rect.z)));
                if x >= params.rect.z { continue; }
                let bit = (x % 8u) * 4u + sy * 2u + sx;
                atomicXor(&output.values[y * stride + x / 8u], 1u << bit);
            }
        }
    }
}

var<workgroup> prefix: array<u32, 128>;
var<workgroup> carry: u32;

// One workgroup per row. Prefix XOR fills all four sample interiors, then
// popcount stores their 0..4 coverage in the same nibble (no second image).
@compute @workgroup_size(128)
fn fill(@builtin(local_invocation_index) lane: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let stride = (params.rect.z + 7u) / 8u;
    if lane == 0u { carry = 0u; }
    workgroupBarrier();
    for (var start = 0u; start < stride; start += 128u) {
        let x = start + lane;
        let index = group.x * stride + x;
        var value = 0u;
        if x < stride { value = atomicLoad(&output.values[index]); }
        value ^= value << 4u;
        value ^= value << 8u;
        value ^= value << 16u;
        prefix[lane] = value >> 28u;
        workgroupBarrier();
        for (var step = 1u; step < 128u; step *= 2u) {
            var previous = 0u;
            if lane >= step { previous = prefix[lane - step]; }
            workgroupBarrier();
            prefix[lane] ^= previous;
            workgroupBarrier();
        }
        var incoming = carry;
        if lane > 0u { incoming ^= prefix[lane - 1u]; }
        value ^= incoming * 0x11111111u;
        var packed = 0u;
        for (var sample = 0u; sample < 8u; sample++) {
            packed |= countOneBits((value >> (sample * 4u)) & 15u) << (sample * 4u);
        }
        if x < stride { atomicStore(&output.values[index], packed); }
        workgroupBarrier();
        if lane == 0u { carry ^= prefix[127]; }
        workgroupBarrier();
    }
}
