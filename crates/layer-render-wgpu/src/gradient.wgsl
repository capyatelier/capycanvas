struct GradientSample { color:vec4<f32>, dither:bool }
fn gradient_interval(base:u32,x:f32)->u32 {
    var low=0u;var high=u32(gradient_record(base,0u).x)-2u;
    for(var step=0u;step<5u;step++) {
        if low>=high {break;}
        let mid=(low+high)/2u;
        if x<=gradient_record(base,3u+2u*mid).x {high=mid;} else {low=mid+1u;}
    }
    return low;
}
fn gradient_sample(base:u32,value:f32)->GradientSample {
    let header=gradient_record(base,0u);let x=clamp(value,0.,1.);
    let i=2u*gradient_interval(base,x);
    let a=gradient_record(base,1u+i);let b=gradient_record(base,3u+i);
    let qa=gradient_record(base,2u+i);let qb=gradient_record(base,4u+i);
    let ca=vec4(a.yzw,qa.x);let cb=vec4(b.yzw,qb.x);
    if x<=a.x {return GradientSample(ca,false);}
    if x>=b.x {return GradientSample(cb,false);}
    let t=(x-a.x)/(b.x-a.x);
    let na=float_number(qa.x);let nb=float_number(qb.x);
    if na.mantissa==0u && nb.mantissa==0u {return GradientSample(vec4(0.),false);}
    let exponent=max(select(nb.exponent,na.exponent,na.mantissa!=0u),select(na.exponent,nb.exponent,nb.mantissa!=0u));
    let aa=ldexp(f32(na.mantissa)/8388608.,na.exponent-exponent);
    let ab=ldexp(f32(nb.mantissa)/8388608.,nb.exponent-exponent);
    let coverage=aa*(1.-t)+ab*t;
    let alpha=packed_number(coverage,exponent);
    if all(ca.rgb==cb.rgb) {return GradientSample(vec4(ca.rgb,alpha),false);}
    var mixed=qa.yzw*(aa*(1.-t)/coverage)+qb.yzw*(ab*t/coverage);
    if header.y==1. {mixed=working_from_oklab(mixed);}
    else if header.y==2. {mixed=working_decode(vec4(mixed,1.)).rgb;}
    return GradientSample(vec4(mixed,alpha),true);
}
fn gradient_shape(relative:vec2<f32>,axis:vec2<f32>,shape:u32,reverse:bool)->f32 {
    let magnitude=max(abs(axis.x),abs(axis.y));
    if magnitude==0. {return 0.;}
    let direction=axis/magnitude;let offset=clamp(relative/magnitude,vec2(-1.e15),vec2(1.e15));
    let length2=dot(direction,direction);var t=dot(offset,direction)/length2;
    if shape==1u {t=length(offset)*inverseSqrt(length2);}
    else if shape==2u {t=abs(t);}
    t=clamp(t,0.,1.);
    return select(t,1.-t,reverse);
}
fn gradient_dither(color:vec4<f32>,position:vec2<f32>,step:f32,enabled:bool)->vec4<f32> {
    if !enabled || step==0. || color.a==0. {return color;}
    let encoded=working_encode(vec4(color.rgb,1.)).rgb;
    return vec4(working_decode(vec4(encoded+quantization_noise(position,0x632be59bu)*step,1.)).rgb,color.a);
}
fn gradient_scalar(base:u32,value:f32,position:vec2<f32>)->vec2<f32> {
    let x=clamp(value,0.,1.);let i=2u*gradient_interval(base,x);
    let a=gradient_record(base,1u+i);let b=gradient_record(base,3u+i);
    let aa=gradient_record(base,2u+i).x;let ab=gradient_record(base,4u+i).x;
    let t=clamp((x-a.x)/(b.x-a.x),0.,1.);
    let alpha=mix(aa,ab,t);var gray=mix(a.y*aa,b.y*ab,t);
    if t>0. && t<1. && alpha>0. && a.y*aa!=b.y*ab {
        gray=clamp(gray+quantization_noise(position,0x632be59bu)/255.,0.,alpha);
    }
    return vec2(gray,alpha);
}
