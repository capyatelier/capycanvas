// SPDX-License-Identifier: MIT OR Apache-2.0
// One bit per pixel. Horizontal shifts and vertical reductions process 32
// pixels at once. Parent storage is reused only outside component labeling.
@group(0) @binding(5) var<storage, read_write> region_mask: array<u32>;

fn mask_stride() -> u32 { return (params.extent_seed.x + 31u) / 32u; }
fn mask_word_id(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * 64u;
}
fn base_word(word:u32)->u32 {
    let words=mask_stride()*params.extent_seed.y;var bits=0xffffffffu;
    for(var plane=0u;plane<attempt_count();plane++){bits&=atomicLoad(&coverage.values[plane*words+word]);}
    return bits;
}
fn mask_word(word:u32)->u32 {
    if params.input.x==3u {return base_word(word);}
    if params.input.x!=0u && params.input.x!=4u {return atomicLoad(&coverage.values[attempt()*mask_stride()*params.extent_seed.y+word]);}
    return region_mask[word];
}
fn mask_bit(p: vec2<i32>) -> bool {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(params.extent_seed.xy)) { return false; }
    return (mask_word(u32(p.y)*mask_stride()+u32(p.x)/32u) & (1u << (u32(p.x)%32u))) != 0u;
}
fn open_word(p:vec2<i32>)->u32 {
    if any(p<vec2<i32>(0)) || p.x>=i32(mask_stride()) || p.y>=i32(params.extent_seed.y) {return 0u;}
    var bits=mask_word(u32(p.y)*mask_stride()+u32(p.x));let tail=params.extent_seed.x%32u;
    if p.x+1==i32(mask_stride()) && tail!=0u {bits&=(1u<<tail)-1u;}
    return bits;
}
fn eroded_row(p:vec2<i32>)->u32 {
    let center=open_word(p);
    return center&((center<<1u)|(open_word(p-vec2(1,0))>>31u))&((center>>1u)|(open_word(p+vec2(1,0))<<31u));
}
fn window_word(id: vec3<u32>, groups: vec3<u32>) -> vec2<u32> {
    let word = mask_word_id(id, groups);
    let stride = (size().x + 31u) / 32u;
    return vec2<u32>((word % stride) * 32u, word / stride);
}
fn freed_word(p:vec2<u32>)->u32 {
    let q=origin()+p;let word=q.y*mask_stride()+q.x/32u;let words=mask_stride()*params.extent_seed.y;
    let base=base_word(word);
    return atomicLoad(&coverage.values[attempt()*words+word])&~base;
}
@compute @workgroup_size(64)
fn mark_exterior(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let p = window_word(id, groups);
    let extent = params.extent_seed.xy;
    if p.y >= size().y { return; }
    let q=origin()+p;let word=vec2<i32>(i32(q.x/32u),i32(q.y));
    let wide_bits=eroded_row(word-vec2(0,1))&eroded_row(word)&eroded_row(word+vec2(0,1));
    var marked=NONE;var previous=NONE;var label=NONE;
    for (var i = 0u; i < 32u && p.x+i < size().x; i++) {
        let local = p + vec2<u32>(i, 0u);
        let q = origin() + local;
        let parent=atomicLoad(&parents[local_index(local,size())]);
        if parent!=previous {previous=parent;label=root(parent);}
        if label==NONE {continue;}
        if (wide_bits&(1u<<i))!=0u && label!=marked {set_bit(wide_marks(),label);marked=label;}
        if any(q==vec2<u32>(0)) || any(q+vec2<u32>(1)==extent)
            || (params.input.z!=0u && brush_selection_at(vec2<f32>(q)+.5)<1.) {set_bit(marks(),label);}
    }
}
fn wide_component(label:u32,global:u32)->bool {
    if label==NONE {return false;}
    if label<perimeter(size()){
        return global!=NONE && bit_set(wide_flags(),global-slots());
    }
    return bit_set(wide_marks(),label);
}
fn selected_component(label:u32,global:u32,wide:bool)->bool {
    if label==NONE{return false;}
    if params.input.y!=0u {return true;}
    if !wide && (params.input.z!=0u || (atomicLoad(&parents[seed()+6u])&0x80000000u)!=0u){return false;}
    let border=label<perimeter(size());
    if params.input.z!=0u {
        if border{return !bit_set(flags(),global-slots());}
        return !bit_set(marks(),label);
    }
    let reached=atomicLoad(&parents[seed()+3u])!=0u;
    if border {return reached && global==atomicLoad(&parents[seed()+2u]);}
    return !reached && atomicLoad(&parents[seed()])==window_index() && atomicLoad(&parents[seed()+1u])==label;
}
@compute @workgroup_size(64)
fn component_mask(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let p = window_word(id, groups);
    if p.y >= size().y { return; }
    var packed = 0u;var thin=0u;var previous=NONE;var selected=false;var is_thin=false;
    for (var i = 0u; i < 32u && p.x+i < size().x; i++) {
        let local=p+vec2<u32>(i,0u);let parent=atomicLoad(&parents[local_index(local,size())]);
        if parent!=previous {
            previous=parent;let label=root(parent);var global=NONE;
            if label!=NONE && label<perimeter(size()) {global=root(slots()+slot_base(window_index())+label);}
            let wide=wide_component(label,global);selected=selected_component(label,global,wide);is_thin=label!=NONE && !wide && !selected;
        }
        if selected{packed|=1u<<i;}
        if is_thin{thin|=1u<<i;}
    }
    let q = origin() + p;
    let word=q.y*mask_stride()+q.x/32u;
    region_mask[word]=packed;
    let words=mask_stride()*params.extent_seed.y;
    if thin!=0u && (params.input.x==0u || params.input.z==0u || params.input.w+1u==attempt_count()) {
        atomicOr(&coverage.values[select(0u,attempt_count(),params.input.x!=0u && params.input.z==0u)*words+word],thin);
    }
    if params.input.z!=0u {atomicOr(&coverage.values[select(1u,attempt_count(),params.input.x!=0u)*words+word],packed);}
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
