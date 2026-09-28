// Feather only the incoming mask, then combine it with the previous selection.
// Intermediates retain float precision; the durable result uses 8-bit coverage.
// Flags: 1 antialias, 2 erosion keeps the canvas edges.
// A pass covers rows [rows.y, rows.z) of the active area; the rest of the grid
// holds no coverage. Row y of an intermediate lives in slot y % rows.w.
struct Params { extent: vec2<u32>, mode: u32, flags: u32, inverse: vec4<f32>, offset_radius: vec4<f32>, rows: vec4<u32>, area: vec4<u32> }
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
        return select(select(0.,1.,value>=.5),value,(params.flags & 1u)!=0u);
    }
    let value = mix(mix(incoming_at(base),incoming_at(base+vec2<i32>(1,0)),f.x),
        mix(incoming_at(base+vec2<i32>(0,1)),incoming_at(base+vec2<i32>(1,1)),f.x),f.y);
    return select(select(0.,1.,value >= .5),value,(params.flags & 1u) != 0u);
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
fn inside(p: vec2<i32>) -> bool {
    return all(p >= vec2<i32>(params.area.xy)) && all(p < vec2<i32>(params.area.zw));
}
fn mask_at(p: vec2<i32>) -> f32 {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(params.extent)) { return 0.; }
    return sample_incoming(vec2<f32>(p)+vec2(.5));
}
fn extremum(a: f32, b: f32) -> f32 {
    return select(min(a,b),max(a,b),params.offset_radius.w > 0.);
}
fn level_word(level: u32, y: u32) -> u32 {
    let stride = (params.extent.x+3u)/4u;
    return ((level-1u)*params.rows.w+y%params.rows.w)*stride;
}
// Packed horizontal range extrema (powers of two). Each disk row is queried
// with two overlapping ranges, so soft masks cost O(radius), not O(radius²).
// Rounding commutes with min/max; byte intermediates preserve the final result.
fn range_at(p: vec2<i32>, level: u32) -> f32 {
    if !inside(p) { return 0.; }
    if level == 0u { return round(mask_at(p)*255.)/255.; }
    return f32((horizontal[level_word(level,u32(p.y))+u32(p.x)/4u] >> ((u32(p.x)%4u)*8u)) & 255u)/255.;
}
@compute @workgroup_size(64)
fn resize_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let y = params.rows.y+id.y;
    let word = params.area.x/4u+id.x;
    if word*4u >= params.area.z || y >= params.rows.z { return; }
    let level = params.rows.x;
    let offset = i32(1u << (level-1u));
    var packed = 0u;
    for (var i = 0u; i < 4u; i++) {
        let x = word*4u+i;
        if x >= params.extent.x { break; }
        let p = vec2<i32>(i32(x),i32(y));
        let value = extremum(range_at(p,level-1u),range_at(p+vec2(offset,0),level-1u));
        packed |= u32(round(value*255.)) << (i*8u);
    }
    horizontal[level_word(level,y)+word] = packed;
}
fn resize_at(p: vec2<i32>) -> f32 {
    let radius = i32(abs(params.offset_radius.w));
    let erode = params.offset_radius.w < 0.;
    let keep_edges = erode && (params.flags & 2u) != 0u;
    let area = vec4<i32>(params.area);
    let width = i32(params.extent.x);
    var value = range_at(p,0u);
    for (var dy = -radius; dy <= radius; dy++) {
        let span = i32(floor(sqrt(f32(radius*radius-dy*dy))));
        let beyond = p.y+dy < 0 || p.y+dy >= i32(params.extent.y);
        if beyond && keep_edges { continue; }
        // Every pixel beyond the active area is unselected: erosion
        // meets it, and dilation needs only the pixels inside.
        var left = max(0,p.x-span);
        var right = min(width-1,p.x+span);
        var outside = beyond || (erode && !keep_edges && (p.x-span < 0 || p.x+span >= width));
        if erode {
            outside = outside || (p.x-span < area.x && area.x > 0) || (p.x+span >= area.z && area.z < width);
        } else {
            left = max(left,area.x);
            right = min(right,area.z-1);
            outside = outside || left > right;
        }
        var row = 0.;
        if !outside {
            let level = 31u-countLeadingZeros(u32(right-left+1));
            let run = i32(1u << level);
            row = extremum(range_at(vec2(left,p.y+dy),level),range_at(vec2(right-run+1,p.y+dy),level));
        }
        value = extremum(value,row);
        if value == select(0.,1.,params.offset_radius.w > 0.) { break; }
    }
    return value;
}
// Each neighborhood is decoded once for the whole workgroup, including halo.
var<workgroup> feather_row: array<f32,556>;
@compute @workgroup_size(64)
fn feather_h(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let radius = i32(ceil(params.offset_radius.z*1.5));
    let y = params.rows.y+id.y;
    let start = params.area.x+(id.x-lane)*4u;
    for(var i=lane;i<256u+2u*u32(radius);i+=64u) {
        // Extend edge pixels at document boundaries, so selecting the entire
        // image doesn't introduce an unwanted fade at its outer border.
        let x=clamp(i32(start+i)-radius,0,i32(params.extent.x)-1);
        feather_row[i]=sample_incoming(vec2<f32>(f32(x)+.5,f32(y)+.5));
    }
    workgroupBarrier();
    let x = params.area.x+id.x*4u;
    if x>=params.area.z {return;}
    var value=vec4<f32>(0.);
    for(var d=-radius;d<=radius;d++) {
        let at=u32(i32(lane*4u)+d+radius);
        value+=weight(d)*vec4<f32>(feather_row[at],feather_row[at+1u],feather_row[at+2u],feather_row[at+3u]);
    }
    let row = (y%params.rows.w)*params.extent.x;
    for(var i=0u;i<4u && x+i<params.extent.x;i++) {horizontal[row+x+i]=bitcast<u32>(value[i]);}
}
@compute @workgroup_size(64)
fn combine(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    let y=params.rows.y+id.y;
    let word=params.area.x/4u+id.x;
    if word*4u >= params.area.z || y >= params.rows.z { return; }
    let radius = i32(ceil(params.offset_radius.z*1.5));
    // Share tap/weight calculations across a packed word's four neighboring
    // pixels; keep float precision until the final byte coverage is published.
    var blurred=vec4<f32>(0.);
    for(var d=-radius;radius>0 && d<=radius;d++) {
        let row=u32(clamp(i32(y)+d,0,i32(params.extent.y)-1));
        if row < params.area.y || row >= params.area.w { continue; }
        let at=(row%params.rows.w)*params.extent.x+word*4u;
        blurred+=weight(d)*bitcast<vec4<f32>>(vec4<u32>(horizontal[at],horizontal[at+1u],horizontal[at+2u],horizontal[at+3u]));
    }
    var packed = 0u;
    for(var i = 0u; i < 4u; i++) {
        let x = word*4u+i;
        if x >= params.extent.x { break; }
        var value = 0.;
        if params.offset_radius.w != 0. {
            value = resize_at(vec2<i32>(i32(x),i32(y)));
        } else if radius == 0 {
            value = sample_incoming(vec2<f32>(f32(x)+.5,f32(y)+.5));
        } else {
            value=blurred[i];
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
    atomicStore(&output.values[y*stride+word],packed);
}
// The mean coverage of cells offset_radius.z document pixels wide, from
// offset_radius.xy, over their part of the canvas area.zw.
@compute @workgroup_size(64)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let stride = (params.extent.x+3u)/4u;
    let y = params.rows.y+id.y;
    if id.x >= stride || y >= params.rows.z { return; }
    let scale = u32(params.offset_radius.z);
    let origin = vec2<u32>(params.offset_radius.xy);
    var packed = 0u;
    for (var i = 0u; i < 4u; i++) {
        let x = id.x*4u+i;
        if x >= params.extent.x { break; }
        let low = origin+vec2(x,y)*scale;
        let high = max(low,min(low+vec2(scale),params.area.zw));
        var sum = 0.;
        for (var v = low.y; v < high.y; v++) {
            for (var u = low.x; u < high.x; u++) { sum += incoming_at(vec2<i32>(i32(u),i32(v))); }
        }
        let count = f32(high.x-low.x)*f32(high.y-low.y);
        packed |= u32(round(sum/max(count,1.)*255.)) << (i*8u);
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
