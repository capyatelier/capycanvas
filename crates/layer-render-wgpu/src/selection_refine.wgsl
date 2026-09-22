// Feather only the incoming mask, then combine it with the previous selection.
// Intermediates retain float precision; the durable result uses 8-bit coverage.
struct Params { extent: vec2<u32>, mode: u32, antialias: u32, inverse: vec4<f32>, offset_radius: vec4<f32> }
struct Packed { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
struct Output { rect: vec4<u32>, info: vec4<u32>, values: array<atomic<u32>> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> incoming: Packed;
@group(0) @binding(2) var<storage, read> previous: Packed;
@group(0) @binding(3) var<storage, read_write> horizontal: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: Output;

fn incoming_at(p: vec2<i32>) -> f32 {
    let q = p - vec2<i32>(incoming.rect.xy);
    var value = 0.;
    if all(q >= vec2<i32>(0)) && all(q < vec2<i32>(incoming.rect.zw)) {
        let bytes = incoming.info.y == 2u;
        let count = select(8u,4u,bytes);
        let word = u32(q.y)*((incoming.rect.z+count-1u)/count)+u32(q.x)/count;
        value = f32((incoming.values[word] >> ((u32(q.x)%count)*(32u/count))) & select(15u,255u,bytes))/select(4.,255.,bytes);
    }
    return select(value,1.-value,incoming.info.x != 0u);
}
fn sample_incoming(p: vec2<f32>) -> f32 {
    let q = vec2<f32>(dot(params.inverse.xz,p),dot(params.inverse.yw,p)) + params.offset_radius.xy
        - bitcast<vec2<f32>>(incoming.info.zw) - .5;
    let base = vec2<i32>(floor(q));
    let f = fract(q);
    let value = mix(mix(incoming_at(base),incoming_at(base+vec2<i32>(1,0)),f.x),
        mix(incoming_at(base+vec2<i32>(0,1)),incoming_at(base+vec2<i32>(1,1)),f.x),f.y);
    return select(select(0.,1.,value >= .5),value,params.antialias != 0u);
}
fn previous_at(p: vec2<f32>) -> f32 {
    if previous.info.y == 0u { return 0.; }
    let q = vec2<i32>(floor(p-bitcast<vec2<f32>>(previous.info.zw)))-vec2<i32>(previous.rect.xy);
    var value = 0.;
    if all(q >= vec2<i32>(0)) && all(q < vec2<i32>(previous.rect.zw)) {
        let bytes = previous.info.y == 2u;
        let count = select(8u,4u,bytes);
        let word = u32(q.y)*((previous.rect.z+count-1u)/count)+u32(q.x)/count;
        value = f32((previous.values[word] >> ((u32(q.x)%count)*(32u/count))) & select(15u,255u,bytes))/select(4.,255.,bytes);
    }
    return select(value,1.-value,previous.info.x != 0u);
}
fn weight(d: i32) -> f32 {
    let sigma = max(params.offset_radius.z*.5,.001);
    return exp(-.5*f32(d*d)/(sigma*sigma));
}
@compute @workgroup_size(8,8)
fn feather_h(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= params.extent) { return; }
    let radius = i32(ceil(params.offset_radius.z*1.5));
    var value = 0.; var total = 0.;
    for(var d = -radius; d <= radius; d++) {
        let w = weight(d);
        // Extend edge pixels at document boundaries, so selecting the entire
        // image doesn't introduce an unwanted fade at its outer border.
        let x = clamp(i32(id.x)+d,0,i32(params.extent.x)-1);
        value += w*sample_incoming(vec2<f32>(f32(x)+.5,f32(id.y)+.5)); total += w;
    }
    horizontal[id.y*params.extent.x+id.x] = value/total;
}
@compute @workgroup_size(64)
fn combine(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    if id.x >= stride || id.y >= params.extent.y { return; }
    let radius = i32(ceil(params.offset_radius.z*1.5));
    let bounds = stride*params.extent.y;
    var packed = 0u;
    var low = params.extent; var high = vec2<u32>(0); var nonempty = false;
    for(var i = 0u; i < 4u; i++) {
        let x = id.x*4u+i;
        if x >= params.extent.x { break; }
        var value = 0.;
        if radius == 0 {
            value = sample_incoming(vec2<f32>(f32(x)+.5,f32(id.y)+.5));
        } else {
            var total = 0.;
            for(var d = -radius; d <= radius; d++) {
                let w = weight(d);
                let y = u32(clamp(i32(id.y)+d,0,i32(params.extent.y)-1));
                value += w*horizontal[y*params.extent.x+x]; total += w;
            }
            value /= total;
        }
        let old = previous_at(vec2<f32>(f32(x)+.5,f32(id.y)+.5));
        switch params.mode {
            case 1u: { value = max(old,value); }
            case 2u: { value = max(0.,old-value); }
            case 3u: { value = min(old,value); }
            default: {}
        }
        let coverage = u32(round(clamp(value,0.,1.)*255.));
        packed |= coverage << (i*8u);
        if coverage != 0u { low = min(low,vec2<u32>(x,id.y)); high = max(high,vec2<u32>(x+1u,id.y+1u)); nonempty = true; }
    }
    atomicStore(&output.values[id.y*stride+id.x],packed);
    if nonempty {
        atomicMin(&output.values[bounds],low.x); atomicMin(&output.values[bounds+1u],low.y);
        atomicMax(&output.values[bounds+2u],high.x); atomicMax(&output.values[bounds+3u],high.y);
        atomicStore(&output.values[bounds+4u],1u);
    }
}
