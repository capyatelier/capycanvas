// SPDX-License-Identifier: MIT OR Apache-2.0
// One bit per pixel. Horizontal shifts and vertical reductions process 32
// pixels at once. Parent storage is reused only outside component labeling.
@group(0) @binding(5) var<storage, read_write> region_mask: array<u32>;

fn mask_stride() -> u32 { return (params.extent_seed.x + 31u) / 32u; }
fn mask_word_id(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * 64u;
}
fn mask_bit(p: vec2<i32>) -> bool {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(params.extent_seed.xy)) { return false; }
    return (region_mask[u32(p.y) * mask_stride() + u32(p.x)/32u] & (1u << (u32(p.x)%32u))) != 0u;
}
fn valid_bits(word: u32) -> u32 {
    let remaining = params.extent_seed.x - word * 32u;
    if remaining >= 32u { return NONE; }
    return (1u << remaining) - 1u;
}
fn read_word(x: i32, y: i32, parent_input: bool, extend: bool) -> u32 {
    let w = i32(params.extent_seed.x);
    let h = i32(params.extent_seed.y);
    let stride = i32(mask_stride());
    if !extend && (x < 0 || x >= stride || y < 0 || y >= h) { return 0u; }
    let xx = clamp(x, 0, stride-1);
    let index = u32(clamp(y, 0, h-1) * stride + xx);
    var value = 0u;
    if parent_input { value = atomicLoad(&parents[index]); }
    else { value = region_mask[index]; }
    if x < 0 { return select(0u, NONE, (value & 1u) != 0u); }
    if x >= stride { return select(0u, NONE, (value & (1u << (u32(w-1)%32u))) != 0u); }
    let valid = valid_bits(u32(x));
    if extend && (value & (1u << (u32(w-1)%32u))) != 0u && x == stride-1 {
        value |= ~valid;
    }
    return select(value & valid, value, extend);
}
fn shifted_word(x: i32, y: i32, extend: bool) -> u32 {
    let word = i32(floor(f32(x) / 32.));
    let shift = u32(x - word * 32);
    let a = read_word(word, y, false, extend);
    if shift == 0u { return a; }
    return (a >> shift) | (read_word(word+1, y, false, extend) << (32u-shift));
}
fn morph(word: u32, horizontal: bool, erode: bool, lower: i32, upper: i32, extend: bool) {
    let stride = mask_stride();
    let p = vec2<u32>(word % stride, word / stride);
    if p.y >= params.extent_seed.y { return; }
    var result = select(0u, NONE, erode);
    for (var d = lower; d <= upper; d++) {
        var sample = 0u;
        if horizontal { sample = shifted_word(i32(p.x)*32+d, i32(p.y), extend); }
        else { sample = read_word(i32(p.x), i32(p.y)+d, true, extend); }
        result = select(result | sample, result & sample, erode);
    }
    result &= valid_bits(p.x);
    if horizontal { atomicStore(&parents[word], result); }
    else { region_mask[word] = result; }
}

// Opening the eligible region closes holes in its complementary barriers.
// Mirrored even-length footprints avoid shifting odd-width gap settings.
@compute @workgroup_size(64)
fn close_h(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let gap = i32(params.options.y);
    morph(mask_word_id(id, groups), true, true, -gap/2, (gap+1)/2, true);
}
@compute @workgroup_size(64)
fn close_v(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let gap = i32(params.options.y);
    morph(mask_word_id(id, groups), false, true, -gap/2, (gap+1)/2, true);
}
@compute @workgroup_size(64)
fn reopen_h(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let gap = i32(params.options.y);
    morph(mask_word_id(id, groups), true, false, -(gap+1)/2, gap/2, true);
}
@compute @workgroup_size(64)
fn reopen_v(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let gap = i32(params.options.y);
    morph(mask_word_id(id, groups), false, false, -(gap+1)/2, gap/2, true);
}
fn window_word(id: vec3<u32>, groups: vec3<u32>) -> vec2<u32> {
    let word = mask_word_id(id, groups);
    let stride = (size().x + 31u) / 32u;
    return vec2<u32>((word % stride) * 32u, word / stride);
}
@compute @workgroup_size(64)
fn mark_exterior(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let p = window_word(id, groups);
    let extent = params.extent_seed.xy;
    if p.y >= size().y { return; }
    for (var i = 0u; i < 32u && p.x+i < size().x; i++) {
        let local = p + vec2<u32>(i, 0u);
        let q = origin() + local;
        if any(q == vec2<u32>(0)) || any(q+vec2<u32>(1) == extent)
            || brush_selection_at(vec2<f32>(q)+.5) < 1. {
            let label = root(local_index(local, size()));
            if label != NONE { set_bit(marks(), label); }
        }
    }
}
fn selected_component(local: vec2<u32>) -> bool {
    let label = root(local_index(local, size()));
    if label == NONE { return false; }
    let border = label < perimeter(size());
    let base = slots() + slot_base(window_index());
    if params.input.z != 0u {
        if border { return !bit_set(flags(), root(base + label) - slots()); }
        return !bit_set(marks(), label);
    }
    let reached = atomicLoad(&parents[seed() + 3u]) != 0u;
    if border { return reached && root(base + label) == atomicLoad(&parents[seed() + 2u]); }
    return !reached && atomicLoad(&parents[seed()]) == window_index() && atomicLoad(&parents[seed() + 1u]) == label;
}
@compute @workgroup_size(64)
fn component_mask(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let p = window_word(id, groups);
    if p.y >= size().y { return; }
    var packed = 0u;
    for (var i = 0u; i < 32u && p.x+i < size().x; i++) {
        if selected_component(p + vec2<u32>(i, 0u)) { packed |= 1u << i; }
    }
    let q = origin() + p;
    region_mask[q.y * mask_stride() + q.x / 32u] = packed;
}
@compute @workgroup_size(64)
fn expand_h(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let radius = i32(abs(params.options.z));
    morph(mask_word_id(id, groups), true, params.options.z < 0., -radius, radius, false);
}
@compute @workgroup_size(64)
fn expand_v(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let radius = i32(abs(params.options.z));
    morph(mask_word_id(id, groups), false, params.options.z < 0., -radius, radius, false);
}

fn refined_coverage(p: vec2<i32>) -> f32 {
    let center = mask_bit(p);
    if params.options.w <= 0. { return select(0., 1., center); }
    // Four corner samples reconstructed from 2x2 neighborhoods. Straight edges
    // stay sharp; diagonal stair corners get quarter coverage. Keep at least
    // one sample for thin selected features, and one hole sample for thin gaps.
    var samples = 0u;
    for (var y = -1; y <= 1; y += 2) {
        for (var x = -1; x <= 1; x += 2) {
            let last = vec2<i32>(params.extent_seed.xy)-1;
            let n = u32(center) + u32(mask_bit(clamp(p+vec2<i32>(x,0), vec2<i32>(0), last)))
                + u32(mask_bit(clamp(p+vec2<i32>(0,y), vec2<i32>(0), last)))
                + u32(mask_bit(clamp(p+vec2<i32>(x,y), vec2<i32>(0), last)));
            samples += u32(n > 2u || (n == 2u && center));
        }
    }
    samples = select(min(samples, 3u), max(samples, 1u), center);
    return f32(samples)*.25;
}
