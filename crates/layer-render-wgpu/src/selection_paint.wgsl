struct Params {
    extent: vec2<u32>, origin: vec2<u32>,
    count: u32, mode: u32, fresh: u32, textured: u32,
    opacity: f32, gray: f32, enclosed: u32, unused: u32,
}
struct Contact { pose: vec4<f32>, rotation_flow: vec4<f32>, texture_alpha: vec4<f32> }
struct Contacts { values: array<Contact,256> }
struct Output { rect: vec4<u32>, info: vec4<u32>, values: array<atomic<u32>> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<uniform> contacts: Contacts;
@group(0) @binding(2) var<storage, read_write> footprint: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: Output;
@group(0) @binding(4) var tip: texture_2d<f32>;
@group(0) @binding(5) var tip_sampler: sampler;
// Bindings 6/7 and sampling helpers come from selection_clip.wgsl.

@compute @workgroup_size(64)
fn initialize(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    if id.x >= stride || id.y >= params.extent.y { return; }
    var packed = 0u;
    for(var i=0u;i<4u;i++) {
        let x = id.x*4u+i;
        if x < params.extent.x {
            packed |= u32(round(before_at(vec2<f32>(f32(x)+.5,f32(id.y)+.5))*255.)) << (i*8u);
        }
    }
    atomicStore(&output.values[id.y*stride+id.x],packed);
}

@compute @workgroup_size(64)
fn paint(@builtin(global_invocation_id) id: vec3<u32>) {
    let start = params.origin + vec2<u32>(id.x*4u,id.y);
    if id.x >= 64u || id.y >= 256u || any(start >= params.extent) { return; }
    var packed = 0u;
    for(var sample=0u;sample<4u;sample++) {
        let pixel = start + vec2<u32>(sample,0u);
        if pixel.x >= params.extent.x { continue; }
        let p = vec2<f32>(pixel)+.5;
        let index = id.y*256u+id.x*4u+sample;
        let base = before_at(p);
        var value = footprint[index];
        if params.fresh != 0u { value = select(0.,base,params.mode == 2u); }
        for(var i=0u;i<params.count;i++) {
            let d = contacts.values[i];
            let q = p-d.pose.xy;
            let local = vec2<f32>(dot(q,d.rotation_flow.xy),dot(q,vec2<f32>(-d.rotation_flow.y,d.rotation_flow.x)))/d.pose.zw;
            if dot(local,local) >= 1. { continue; }
            var coverage = analytic_coverage(local,d.rotation_flow.w,min(d.pose.z,d.pose.w));
            if params.textured != 0u {
                coverage = textureSampleLevel(tip,tip_sampler,local*d.texture_alpha.xy*.5+.5,0.).r;
            }
            let alpha = clamp(coverage*d.rotation_flow.z*d.texture_alpha.z,0.,1.);
            if params.mode == 2u { value = mix(value,params.gray,alpha*params.opacity); }
            else { value = max(value,alpha); }
        }
        if params.enclosed != 0u {
            let area = enclosed_at(p);
            if params.mode == 2u { value = mix(value,params.gray,area*params.opacity); }
            else { value = max(value,area); }
        }
        footprint[index] = value;
        var result = value;
        if params.mode == 0u { result = base+(1.-base)*value*params.opacity; }
        if params.mode == 1u { result = base*(1.-value*params.opacity); }
        packed |= u32(round(clamp(result,0.,1.)*255.)) << (sample*8u);
    }
    let stride = (params.extent.x+3u)/4u;
    atomicStore(&output.values[start.y*stride+start.x/4u],packed);
}

// Reduce locally before publishing bounds. A full mask must not send one
// contended global atomic per packed word to the same five addresses.
var<workgroup> bounds_values: array<vec4<u32>,64>;
var<workgroup> bounds_flags: array<u32,64>;
@compute @workgroup_size(64)
fn bounds(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let stride = (params.extent.x+3u)/4u;
    let tail = stride*params.extent.y;
    var rect = vec4<u32>(params.extent,0u,0u);
    var flags = 0u;
    if id.x < stride && id.y < params.extent.y {
        let word = atomicLoad(&output.values[id.y*stride+id.x]);
        var original = 0u;
        for(var i=0u;i<4u;i++) {
            let x = id.x*4u+i;
            if x < params.extent.x {
                original |= u32(round(before_at(vec2<f32>(f32(x)+.5,f32(id.y)+.5))*255.)) << (i*8u);
            }
        }
        if original != word { flags = 1u; }
        if word != 0u {
            rect = vec4<u32>(id.x*4u+countTrailingZeros(word)/8u,id.y,
                id.x*4u+4u-countLeadingZeros(word)/8u,id.y+1u);
            flags |= 2u;
        }
    }
    bounds_values[lane] = rect; bounds_flags[lane] = flags;
    workgroupBarrier();
    for(var step=32u;step>0u;step/=2u) {
        if lane < step {
            let a = bounds_values[lane]; let b = bounds_values[lane+step];
            bounds_values[lane] = vec4<u32>(min(a.xy,b.xy),max(a.zw,b.zw));
            bounds_flags[lane] |= bounds_flags[lane+step];
        }
        workgroupBarrier();
    }
    if lane == 0u {
        if (bounds_flags[0] & 1u) != 0u { atomicStore(&output.values[tail+5u],1u); }
        if (bounds_flags[0] & 2u) != 0u {
            let area = bounds_values[0];
            atomicMin(&output.values[tail],area.x);
            atomicMin(&output.values[tail+1u],area.y);
            atomicMax(&output.values[tail+2u],area.z);
            atomicMax(&output.values[tail+3u],area.w);
            atomicStore(&output.values[tail+4u],1u);
        }
    }
}
