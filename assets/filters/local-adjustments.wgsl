fn fx_local_gain(value:f32,gain:f32)->f32 {
    let bits=bitcast<u32>(value);let magnitude=bits&0x7fffffffu;
    if magnitude==0u || magnitude>=0x01800000u {return value*gain;}
    let number=float_number(value);
    var mantissa=f32(number.mantissa)/8388608.*gain;var exponent=number.exponent;
    for(var i=0u;i<2u;i++){if mantissa<1.{mantissa*=2.;exponent--;}else if mantissa>=2.{mantissa*=.5;exponent++;}}
    if exponent>=-126 {return select(1.,-1.,number.negative)*ldexp(mantissa,exponent);}
    let scaled=mantissa*exp2(f32(exponent+149));
    var rounded=u32(floor(scaled));let fraction=scaled-f32(rounded);
    if fraction>.5 || (fraction==.5 && (rounded&1u)!=0u) {rounded++;}
    return bitcast<f32>((bits&0x80000000u)|rounded);
}
fn fx_illumination(position:vec2<f32>,log_y:f32)->f32 {
    let size=bitcast<vec4<u32>>(fx_auxiliary(0u));
    let p=clamp(position,vec2(0.),vec2<f32>(size.zw));
    let scaled=vec2<u32>(floor(p))*size.xy;
    let remainder=(vec2<f32>(scaled%size.zw)+fract(p)*vec2<f32>(size.xy))/vec2<f32>(size.zw)-.5;
    let origin=vec2<i32>(scaled/size.zw)+vec2<i32>(floor(remainder));
    let fraction=fract(remainder);
    var mean=vec3(0.);
    for(var y=0u;y<2u;y++){for(var x=0u;x<2u;x++){
        let at=vec2<u32>(clamp(origin+vec2<i32>(i32(x),i32(y)),vec2<i32>(0),vec2<i32>(size.xy)-1));
        let sample=fx_auxiliary(1u+at.y*size.x+at.x);
        let difference=(sample.x-log_y)/1.5;
        let weight=select(1.-fraction.x,fraction.x,x==1u)*select(1.-fraction.y,fraction.y,y==1u)/(1.+difference*difference*difference*difference);
        mean=guide_mean(mean,sample.yzw,weight);
    }}
    if mean.y==0. {return log_y;}
    return mean.x;
}
fn fx_local_adjustment(c:vec4<f32>,position:vec2<f32>,amounts:vec3<f32>,clarity:bool)->vec4<f32> {
    if (bitcast<u32>(c.a)&0x7fffffffu)==0u || all(amounts==vec3(0.)) || bitcast<vec4<u32>>(fx_auxiliary(0u)).x==0u {return c;}
    let maximum=max(max(abs(c.r),abs(c.g)),abs(c.b));
    let measure=guide_luminance(c,FX_LUMA);
    if measure.w!=1. {return c;}
    let log_y=measure.x;
    let illumination=fx_illumination(position,log_y);
    var delta=clamp(amounts.z*(log_y-illumination),-2.,2.);
    if !clarity {
        let middle=log2(.18);
        delta=2.*amounts.x*(1.-smoothstep(middle-4.,middle,illumination))-2.*amounts.y*smoothstep(middle,middle+4.,illumination);
    }
    if delta==0. {return c;}
    let gain=exp2(delta);
    if maximum>3.401992e38/gain {return c;}
    return vec4(fx_local_gain(c.r,gain),fx_local_gain(c.g,gain),fx_local_gain(c.b,gain),c.a);
}
fn capy_shadows_highlights(c:vec4<f32>,position:vec2<f32>,base:u32)->vec4<f32> {
    return fx_local_adjustment(c,position,vec3(fx_parameter(base,0u).x,fx_parameter(base,1u).x,0.)*.01,false);
}
fn capy_clarity(c:vec4<f32>,position:vec2<f32>,base:u32)->vec4<f32> {
    return fx_local_adjustment(c,position,vec3(0.,0.,fx_parameter(base,0u).x*.01),true);
}
