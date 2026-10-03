struct Area { origin:vec2<i32>, center:vec2<i32>, extent:vec2<u32>, width:u32, padding:u32 }
struct Summary { maximum:u32, alpha_maximum:u32, count:u32, invalid:u32, sum:vec4<f32> }
@group(0) @binding(0) var pixels:texture_2d<f32>;
@group(0) @binding(1) var<uniform> area:Area;
@group(0) @binding(2) var<storage,read_write> summary:Summary;
var<workgroup> maxima:array<vec2<u32>,64>;
var<workgroup> counts:array<u32,64>;
var<workgroup> invalid:array<u32,64>;
var<workgroup> sums:array<vec4<f32>,64>;

fn included(i:u32)->bool {
    let p=area.origin+vec2<i32>(i32(i%area.extent.x),i32(i/area.extent.x))-area.center;
    return 4*(p.x*p.x+p.y*p.y)<=i32(area.width*area.width);
}
fn normalized(value:f32,maximum:u32)->f32 {
    let bits=bitcast<u32>(value);let absolute=bits&0x7fffffffu;
    if absolute==0u || maximum==0u {return 0.;}
    var mantissa=(absolute&0x7fffffu)|0x800000u;var exponent=i32(absolute>>23u)-127;
    if absolute<0x800000u {let shift=countLeadingZeros(absolute)-8u;mantissa=absolute<<shift;exponent= -126-i32(shift);}
    var denominator=(maximum&0x7fffffu)|0x800000u;var scale=i32(maximum>>23u)-127;
    if maximum<0x800000u {let shift=countLeadingZeros(maximum)-8u;denominator=maximum<<shift;scale= -126-i32(shift);}
    return select(1.,-1.,bits>>31u!=0u)*(f32(mantissa)/f32(denominator))*exp2(f32(exponent-scale));
}
@compute @workgroup_size(64)
fn measure(@builtin(local_invocation_index) lane:u32) {
    var maximum=vec2<u32>(0u);var count=0u;var bad=0u;
    for(var i=lane;i<area.extent.x*area.extent.y;i+=64u) {
        if !included(i) {continue;}
        count+=1u;
        let rgba=textureLoad(pixels,vec2<i32>(i32(i%area.extent.x),i32(i/area.extent.x)),0);
        let bits=bitcast<vec4<u32>>(rgba);let absolute=bits&vec4<u32>(0x7fffffffu);
        if any(absolute>=vec4<u32>(0x7f800000u)) || absolute.a>0x3f800000u || (bits.a>>31u!=0u && absolute.a!=0u) {bad=1u;continue;}
        if absolute.a!=0u {maximum=max(maximum,vec2<u32>(max(max(absolute.r,absolute.g),absolute.b),absolute.a));}
    }
    maxima[lane]=maximum;counts[lane]=count;invalid[lane]=bad;
    workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if lane<stride {maxima[lane]=max(maxima[lane],maxima[lane+stride]);counts[lane]+=counts[lane+stride];invalid[lane]|=invalid[lane+stride];}
        workgroupBarrier();
    }
    if lane==0u {summary.maximum=maxima[0].x;summary.alpha_maximum=maxima[0].y;summary.count=counts[0];summary.invalid=invalid[0];}
}
@compute @workgroup_size(64)
fn accumulate(@builtin(local_invocation_index) lane:u32) {
    var sum=vec4<f32>(0.);
    if summary.invalid==0u {
        for(var i=lane;i<area.extent.x*area.extent.y;i+=64u) {
            if !included(i) {continue;}
            let rgba=textureLoad(pixels,vec2<i32>(i32(i%area.extent.x),i32(i/area.extent.x)),0);
            if (bitcast<u32>(rgba.a)&0x7fffffffu)!=0u {
                sum+=vec4<f32>(normalized(rgba.r,summary.maximum),normalized(rgba.g,summary.maximum),normalized(rgba.b,summary.maximum),normalized(rgba.a,summary.alpha_maximum));
            }
        }
    }
    sums[lane]=sum;workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if lane<stride {sums[lane]+=sums[lane+stride];}
        workgroupBarrier();
    }
    if lane==0u {summary.sum=sums[0];}
}
