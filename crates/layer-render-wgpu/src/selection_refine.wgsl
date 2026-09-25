// Feather only the incoming mask, then combine it with the previous selection.
// Intermediates retain float precision; the durable result uses 8-bit coverage.
struct Params { extent: vec2<u32>, mode: u32, antialias: u32, inverse: vec4<f32>, offset_radius: vec4<f32>, resize_level: vec4<u32> }
struct Packed { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
struct Output { rect: vec4<u32>, info: vec4<u32>, values: array<atomic<u32>> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> incoming: Packed;
@group(0) @binding(2) var<storage, read> previous: Packed;
@group(0) @binding(3) var<storage, read_write> horizontal: array<u32>;
@group(0) @binding(4) var<storage, read_write> output: Output;
@group(0) @binding(5) var<uniform> weights: array<vec4<f32>,38>;

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
    if all(f==vec2<f32>(0.)) {
        let value=incoming_at(base);
        return select(select(0.,1.,value>=.5),value,params.antialias!=0u);
    }
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
    let i=u32(abs(d));
    return weights[i/4u][i%4u];
}
fn mask_at(p: vec2<i32>) -> f32 {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(params.extent)) { return 0.; }
    return sample_incoming(vec2<f32>(p)+vec2(.5));
}
fn extremum(a: f32, b: f32) -> f32 {
    return select(min(a,b),max(a,b),params.offset_radius.w > 0.);
}
// Packed horizontal range extrema (powers of two). Each disk row is queried
// with two overlapping ranges, so soft masks cost O(radius), not O(radius²).
// Rounding commutes with min/max; byte intermediates preserve the final result.
fn range_at(p: vec2<i32>, level: u32) -> f32 {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(params.extent)) { return 0.; }
    if level == 0u { return round(mask_at(p)*255.)/255.; }
    let stride = (params.extent.x+3u)/4u;
    let index = (level-1u)*stride*params.extent.y+u32(p.y)*stride+u32(p.x)/4u;
    return f32((horizontal[index] >> ((u32(p.x)%4u)*8u)) & 255u)/255.;
}
@compute @workgroup_size(64)
fn resize_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    if id.x >= stride || id.y >= params.extent.y { return; }
    let level = params.resize_level.x;
    let offset = i32(1u << (level-1u));
    var packed = 0u;
    for (var i = 0u; i < 4u; i++) {
        let x = id.x*4u+i;
        if x >= params.extent.x { break; }
        let p = vec2<i32>(i32(x),i32(id.y));
        let value = extremum(range_at(p,level-1u),range_at(p+vec2(offset,0),level-1u));
        packed |= u32(round(value*255.)) << (i*8u);
    }
    horizontal[(level-1u)*stride*params.extent.y+id.y*stride+id.x] = packed;
}
fn resize_at(p: vec2<i32>) -> f32 {
    let radius = i32(abs(params.offset_radius.w));
    var value = range_at(p,0u);
    for (var dy = -radius; dy <= radius; dy++) {
        let span = i32(floor(sqrt(f32(radius*radius-dy*dy))));
        let left = max(0,p.x-span);
        let right = min(i32(params.extent.x)-1,p.x+span);
        var row = 0.;
        let outside = p.y+dy < 0 || p.y+dy >= i32(params.extent.y)
            || (params.offset_radius.w < 0. && (p.x-span < 0 || p.x+span >= i32(params.extent.x)));
        if !outside {
            let level = 31u-countLeadingZeros(u32(right-left+1));
            let width = i32(1u << level);
            row = extremum(range_at(vec2(left,p.y+dy),level),range_at(vec2(right-width+1,p.y+dy),level));
        }
        value = extremum(value,row);
        if value == select(0.,1.,params.offset_radius.w > 0.) { break; }
    }
    return value;
}
// Each neighborhood is decoded once for the whole workgroup, including halo.
var<workgroup> feather_row: array<f32,428>;
@compute @workgroup_size(128)
fn feather_h(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let radius = i32(ceil(params.offset_radius.z*1.5));
    let y=id.y+params.resize_level.w;
    for(var i=lane;i<128u+2u*u32(radius);i+=128u) {
        // Extend edge pixels at document boundaries, so selecting the entire
        // image doesn't introduce an unwanted fade at its outer border.
        let x=clamp(i32(id.x-lane+i)-radius,0,i32(params.extent.x)-1);
        feather_row[i]=sample_incoming(vec2<f32>(f32(x)+.5,f32(y)+.5));
    }
    workgroupBarrier();
    if id.x>=params.extent.x {return;}
    var value=0.;
    for(var d=-radius;d<=radius;d++) {value+=weight(d)*feather_row[u32(i32(lane)+d+radius)];}
    horizontal[id.y*params.extent.x+id.x] = bitcast<u32>(value);
}
@compute @workgroup_size(64)
fn combine(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    let y=id.y+params.resize_level.y;
    if id.x >= stride || y >= params.resize_level.z { return; }
    let radius = i32(ceil(params.offset_radius.z*1.5));
    var packed = 0u;
    for(var i = 0u; i < 4u; i++) {
        let x = id.x*4u+i;
        if x >= params.extent.x { break; }
        var value = 0.;
        if params.offset_radius.w != 0. {
            value = resize_at(vec2<i32>(i32(x),i32(y)));
        } else if radius == 0 {
            value = sample_incoming(vec2<f32>(f32(x)+.5,f32(y)+.5));
        } else {
            for(var d = -radius; d <= radius; d++) {
                let w = weight(d);
                let row = u32(clamp(i32(y)+d,0,i32(params.extent.y)-1))-params.resize_level.w;
                value += w*bitcast<f32>(horizontal[row*params.extent.x+x]);
            }
        }
        let old = previous_at(vec2<f32>(f32(x)+.5,f32(y)+.5));
        switch params.mode {
            case 1u: { value = max(old,value); }
            case 2u: { value = max(0.,old-value); }
            case 3u: { value = min(old,value); }
            default: {}
        }
        let coverage = u32(round(clamp(value,0.,1.)*255.));
        packed |= coverage << (i*8u);
    }
    atomicStore(&output.values[y*stride+id.x],packed);
}
// One workgroup scans one row, then publishes one set of global bounds. This
// reduces contention from millions of word updates to only thousands of rows.
var<workgroup> row_bounds: array<vec2<u32>,128>;
@compute @workgroup_size(128)
fn bounds(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let stride=(params.extent.x+3u)/4u;
    let y=group.x;
    var low=params.extent.x; var high=0u;
    for(var x=lane;x<stride;x+=128u) {
        let word=atomicLoad(&output.values[y*stride+x]);
        if word!=0u {
            low=min(low,x*4u+countTrailingZeros(word)/8u);
            high=max(high,x*4u+4u-countLeadingZeros(word)/8u);
        }
    }
    row_bounds[lane]=vec2<u32>(low,high);
    workgroupBarrier();
    for(var step=64u;step>0u;step/=2u) {
        if lane<step {row_bounds[lane]=vec2<u32>(min(row_bounds[lane].x,row_bounds[lane+step].x),max(row_bounds[lane].y,row_bounds[lane+step].y));}
        workgroupBarrier();
    }
    if lane==0u && row_bounds[0].y!=0u {
        let offset=stride*params.extent.y;
        atomicMin(&output.values[offset],row_bounds[0].x);atomicMin(&output.values[offset+1u],y);
        atomicMax(&output.values[offset+2u],row_bounds[0].y);atomicMax(&output.values[offset+3u],y+1u);
        atomicStore(&output.values[offset+4u],1u);
    }
}
