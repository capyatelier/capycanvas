struct Area {size:vec2<u32>,space:u32,padding:u32,ranges:array<vec4<f32>,3>}
@group(0) @binding(0) var pixels:texture_2d<f32>;
@group(0) @binding(1) var<uniform> area:Area;
@group(0) @binding(2) var<storage,read_write> summary:array<atomic<u32>>;
@group(0) @binding(3) var<storage,read_write> shards:array<atomic<u32>>;
var<workgroup> minima:array<vec3<u32>,64>;
var<workgroup> maxima:array<vec3<u32>,64>;
var<workgroup> totals:array<vec2<u32>,64>;
var<workgroup> bins:array<atomic<u32>,4096>;
fn ordered(value:f32)->u32 {let bits=bitcast<u32>(value);return select(bits^0x80000000u,~bits,(bits>>31u)!=0u);}
fn logarithm(value:u32)->f32 {
    var mantissa=(value&0x7fffffu)|0x800000u;var exponent=i32(value>>23u)-127;
    if value<0x800000u {let shift=countLeadingZeros(value)-8u;mantissa=value<<shift;exponent= -126-i32(shift);}
    return log2(f32(mantissa)/8388608.)+f32(exponent);
}
fn encoded(value:f32,alpha:f32)->f32 {
    let bits=bitcast<u32>(value);let absolute=bits&0x7fffffffu;
    if absolute==0u {return 0.;}
    let magnitude=logarithm(absolute)-logarithm(bitcast<u32>(alpha));
    var result:f32;
    if area.space<2u {result=select(1.055*exp2(magnitude/2.4)-0.055,exp2(magnitude)*12.92,magnitude<=log2(0.0031308));}
    else if area.space==2u {result=exp2(magnitude*(256./563.));}
    else {result=select(exp2(magnitude/1.8),exp2(magnitude)*16.,magnitude<= -9.);}
    return select(result,-result,bits>>31u!=0u);
}
fn valid(color:vec4<f32>)->bool {
    let bits=bitcast<vec4<u32>>(color);let absolute=bits&vec4(0x7fffffffu);
    return all(absolute<vec4(0x7f800000u)) && absolute.a<=0x3f800000u && (bits.a>>31u==0u || absolute.a==0u);
}
@compute @workgroup_size(64)
fn measure(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>) {
    var low=vec3(0xffffffffu);var high=vec3(0u);var count=0u;var invalid=0u;
    for(var i=group.x*64u+lane;i<area.size.x*area.size.y;i+=4096u) {
        let color=textureLoad(pixels,vec2<i32>(i32(i%area.size.x),i32(i/area.size.x)),0);
        if !valid(color) {invalid=1u;continue;}
        if (bitcast<u32>(color.a)&0x7fffffffu)==0u {continue;}
        let rgb=vec3(encoded(color.r,color.a),encoded(color.g,color.a),encoded(color.b,color.a));
        if any((bitcast<vec3<u32>>(rgb)&vec3(0x7fffffffu))>=vec3(0x7f800000u)) {invalid=1u;continue;}
        let values=vec3(ordered(rgb.r),ordered(rgb.g),ordered(rgb.b));low=min(low,values);high=max(high,values);count+=1u;
    }
    minima[lane]=low;maxima[lane]=high;totals[lane]=vec2(count,invalid);workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if lane<stride {minima[lane]=min(minima[lane],minima[lane+stride]);maxima[lane]=max(maxima[lane],maxima[lane+stride]);totals[lane]=vec2(totals[lane].x+totals[lane+stride].x,totals[lane].y|totals[lane+stride].y);}
        workgroupBarrier();
    }
    if lane==0u {
        for(var c=0u;c<3u;c++) {atomicMin(&summary[c],minima[0][c]);atomicMax(&summary[c+3u],maxima[0][c]);}
        atomicAdd(&summary[6],totals[0].x);atomicOr(&summary[7],totals[0].y);
    }
}
@compute @workgroup_size(64)
fn count(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>) {
    for(var i=lane;i<4096u;i+=64u) {atomicStore(&bins[i],0u);}workgroupBarrier();
    let channel=group.y;let range=area.ranges[channel];
    for(var i=group.x*64u+lane;i<area.size.x*area.size.y;i+=1024u) {
        let color=textureLoad(pixels,vec2<i32>(i32(i%area.size.x),i32(i/area.size.x)),0);
        if (bitcast<u32>(color.a)&0x7fffffffu)==0u {continue;}
        let value=encoded(color[channel],color.a)/range.z;
        var index=0u;
        if range.y>range.x {index=u32(clamp(floor((value-range.x)/(range.y-range.x)*4096.),0.,4095.));}
        atomicAdd(&bins[index],1u);
    }
    workgroupBarrier();
    for(var i=lane;i<4096u;i+=64u) {let n=atomicLoad(&bins[i]);if n!=0u {atomicAdd(&shards[(group.x*3u+channel)*4096u+i],n);}}
}
@compute @workgroup_size(64)
fn fold(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=12288u {return;}
    var sum=0u;for(var group=0u;group<16u;group++) {sum+=atomicLoad(&shards[group*12288u+id.x]);}
    atomicStore(&summary[8u+id.x],sum);
}
