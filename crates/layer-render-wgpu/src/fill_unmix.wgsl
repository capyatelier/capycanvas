struct FillPrecision {source:mat3x3<f32>,codes:vec4<u32>}
@group(2) @binding(4) var<uniform> fill_precision:FillPrecision;
struct FillSample {value:vec4<f32>,low:vec4<f32>,high:vec4<f32>}

fn fill_storage_ulp(value:vec4<f32>)->vec4<f32> {
    let exponent=vec4<i32>((bitcast<vec4<u32>>(abs(value))>>vec4<u32>(23))&vec4<u32>(255))-127;
    return exp2(vec4<f32>(max(exponent-i32(fill_precision.codes.z),vec4(-i32(fill_precision.codes.w)))));
}
fn fill_sample(value:vec4<f32>)->FillSample {
    let matrix=fill_precision.source;
    let absolute=mat3x3(abs(matrix[0]),abs(matrix[1]),abs(matrix[2]));
    let positive=mat3x3(max(matrix[0],vec3(0.)),max(matrix[1],vec3(0.)),max(matrix[2],vec3(0.)));
    let negative=absolute-positive;
    let storage=fill_storage_ulp(value);
    let bits=bitcast<vec4<u32>>(abs(value));
    let exponent=vec4<i32>((bits>>vec4<u32>(23))&vec4<u32>(255))-127;
    let power=(bits&vec4<u32>(8388607))==vec4<u32>(0);
    let previous=select(storage,storage*.5,power&(exponent>vec4(i32(fill_precision.codes.z)-i32(fill_precision.codes.w))));
    let below=select(previous,storage,value<vec4(0.))*.5;
    let above=select(storage,previous,value<vec4(0.))*.5;
    let color=matrix*value.rgb;
    var low_error=vec4(positive*below.rgb+negative*above.rgb,below.a);
    var high_error=vec4(positive*above.rgb+negative*below.rgb,above.a);
    if fill_precision.codes.x!=0u {
        let maximum=f32(fill_precision.codes.x);
        let encoded=round(sdr_encode(color,fill_precision.codes.y)*maximum);
        let low=sdr_decode((encoded-.5)/maximum,fill_precision.codes.y);
        let high=sdr_decode((encoded+.5)/maximum,fill_precision.codes.y);
        low_error+=vec4(color-low,.5/maximum);high_error+=vec4(high-color,.5/maximum);
    }
    return FillSample(vec4(color,value.a),low_error,high_error);
}
fn fill_integer_product(a:u32,b:u32)->vec2<u32> {
    let low=(a&65535u)*(b&65535u);let x=(a>>16u)*(b&65535u);let y=(b>>16u)*(a&65535u);
    let first=low+(x<<16u);let second=first+(y<<16u);
    return vec2((a>>16u)*(b>>16u)+(x>>16u)+(y>>16u)+u32(first<low)+u32(second<first),second);
}
fn fill_ratio_greater(a:vec2<i32>,b:vec2<i32>)->bool {
    if (a.x<0)!=(b.x<0) {return a.x>=0;}
    let left=fill_integer_product(u32(abs(a.x)),u32(b.y));let right=fill_integer_product(u32(abs(b.x)),u32(a.y));
    if a.x<0 {return right.x>left.x || (right.x==left.x && right.y>left.y);}
    return left.x>right.x || (left.x==right.x && left.y>right.y);
}
fn fill_fraction(pixel:vec4<f32>,seed:vec4<f32>,neighbour:vec4<f32>)->f32 {
    let p=fill_sample(pixel);let s=fill_sample(seed);let n=fill_sample(neighbour);
    let largest=max(max(abs(p.value)+max(p.low,p.high),abs(s.value)+max(s.low,s.high)),abs(n.value)+max(n.low,n.high));
    let exponent=vec4<i32>((bitcast<vec4<u32>>(largest)>>vec4<u32>(23))&vec4<u32>(255))-127;
    let scale=exp2(vec4<f32>(min(vec4(127),vec4(27)-exponent)));
    let qp=vec4<i32>(round(p.value*scale));let qs=vec4<i32>(round(s.value*scale));let qn=vec4<i32>(round(n.value*scale));
    let pl=vec4<i32>(ceil(p.low*scale+.5));let ph=vec4<i32>(ceil(p.high*scale+.5));
    let sl=vec4<i32>(ceil(s.low*scale+.5));let sh=vec4<i32>(ceil(s.high*scale+.5));
    let nl=vec4<i32>(ceil(n.low*scale+.5));let nh=vec4<i32>(ceil(n.high*scale+.5));
    var lower=vec2<i32>(0,1);var upper=vec2<i32>(1,1);
    for(var channel=0u;channel<4u;channel++) {
        let a=qs[channel]+sh[channel]-qn[channel]-nh[channel];
        let b=qp[channel]-pl[channel]-qn[channel]-nh[channel];
        if a==0 && b>0 {return -1.;}
        if a>0 && fill_ratio_greater(vec2(b,a),lower) {lower=vec2(b,a);}
        if a<0 && fill_ratio_greater(upper,vec2(-b,-a)) {upper=vec2(-b,-a);}
        let c=qs[channel]-sl[channel]-qn[channel]+nl[channel];
        let d=qp[channel]+ph[channel]-qn[channel]+nl[channel];
        if c==0 && d<0 {return -1.;}
        if c>0 && fill_ratio_greater(upper,vec2(d,c)) {upper=vec2(d,c);}
        if c<0 && fill_ratio_greater(vec2(-d,-c),lower) {lower=vec2(-d,-c);}
    }
    if fill_ratio_greater(lower,upper) {return -1.;}
    return .5*(f32(lower.x)/f32(lower.y)+f32(upper.x)/f32(upper.y));
}
fn fill_reconstruct(pixel:vec4<f32>,seed:vec4<f32>,neighbour:vec4<f32>,paint:vec4<f32>,amount:f32)->vec4<f32> {
    let fraction=fill_fraction(pixel,seed,neighbour);
    if fraction<0. {return pixel;}
    return mix(pixel,mix(neighbour,paint,fraction),amount);
}
