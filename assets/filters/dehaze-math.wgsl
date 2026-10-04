fn dehaze_direct_math(pixel:vec4<f32>)->bool {
    let magnitude=abs(pixel.rgb);
    return pixel.a>=exp2(-32.) && pixel.a<=1. && all(magnitude<=vec3(exp2(32.)))
        && all((magnitude==vec3(0.)) | (magnitude>=vec3(exp2(-32.))));
}
fn unassociated_component(value:f32,alpha:FloatNumber)->f32 {
    let number=float_number(value);if number.mantissa==0u{return 0.;}
    let magnitude=packed_number(f32(number.mantissa)/f32(alpha.mantissa),number.exponent-alpha.exponent);
    return bitcast<f32>((bitcast<u32>(magnitude)&0x7fffffffu)|select(0u,0x80000000u,number.negative));
}
fn bounded_source(pixel:vec4<f32>)->vec3<f32>{
    if dehaze_direct_math(pixel) {return clamp(dehaze_to_srgb(pixel.rgb/pixel.a),vec3(0.),vec3(1.));}
    let alpha=float_number(pixel.a);let bits=bitcast<vec3<u32>>(pixel.rgb)&vec3<u32>(0x7fffffffu);
    let scale=float_number(bitcast<f32>(max(max(bits.r,bits.g),bits.b))).exponent;
    let scaled=vec3(guide_normalized(float_number(pixel.r),scale),guide_normalized(float_number(pixel.g),scale),guide_normalized(float_number(pixel.b),scale));
    let converted=dehaze_to_srgb(scaled);
    var result=vec3(0.);for(var i=0u;i<3u;i++){let number=float_number(converted[i]);if !number.negative && number.mantissa!=0u {let ratio=f32(number.mantissa)/f32(alpha.mantissa);let exponent=number.exponent+scale-alpha.exponent;if exponent>0 || (exponent==0 && ratio>=1.){result[i]=1.;}else{result[i]=packed_number(ratio,exponent);}}}return result;
}
fn packed_scaled(value:f32,scale:i32)->f32 {
    let number=float_number(value);if number.mantissa==0u{return 0.;}
    return packed_number(f32(number.mantissa)/8388608.,number.exponent+scale);
}
fn safe_luma(rgb:vec3<f32>)->f32 {
    let bits=bitcast<vec3<u32>>(rgb)&vec3<u32>(0x7fffffffu);
    let scale=float_number(bitcast<f32>(max(max(bits.r,bits.g),bits.b))).exponent;
    let normalized=vec3(guide_normalized(float_number(rgb.r),scale),guide_normalized(float_number(rgb.g),scale),guide_normalized(float_number(rgb.b),scale));
    return packed_scaled(dot(normalized,DEHAZE_LUMA),scale);
}
fn scaled_bounded_source(pixel:vec4<f32>,scale:i32)->vec3<f32>{
    let alpha=float_number(pixel.a);
    let bits=bitcast<vec3<u32>>(pixel.rgb)&vec3<u32>(0x7fffffffu);let original_scale=float_number(bitcast<f32>(max(max(bits.r,bits.g),bits.b))).exponent;
    let normalized=vec3(guide_normalized(float_number(pixel.r),original_scale),guide_normalized(float_number(pixel.g),original_scale),guide_normalized(float_number(pixel.b),original_scale));
    let values=dehaze_to_srgb(normalized);
    var result=vec3(0.);
    for(var i=0u;i<3u;i++){let number=float_number(values[i]);if number.negative || number.mantissa==0u{continue;}
        let ratio=f32(number.mantissa)/f32(alpha.mantissa);let exponent=number.exponent+original_scale-alpha.exponent;
        if exponent>0 || (exponent==0 && ratio>=1.){result[i]=exp2(f32(-scale));}else{result[i]=ldexp(ratio,exponent-scale);}
    }return result;
}
fn premult_correction(value:f32,alpha:f32)->f32 {
    if alpha>=exp2(-32.) && alpha<=1. && abs(value)>=exp2(-32.) && abs(value)<=exp2(32.) {return value*alpha;}
    let number=float_number(value);let coverage=float_number(alpha);
    if number.mantissa==0u{return 0.;}
    let magnitude=packed_number((f32(number.mantissa)/8388608.)*(f32(coverage.mantissa)/8388608.),number.exponent+coverage.exponent);
    return bitcast<f32>((bitcast<u32>(magnitude)&0x7fffffffu)|select(0u,0x80000000u,number.negative));
}
fn exact_add(a:f32,b:f32)->f32 {
    if (bitcast<u32>(a)&0x7fffffffu)==0u{return b;}if (bitcast<u32>(b)&0x7fffffffu)==0u{return a;}
    if (bitcast<u32>(a)&0x7fffffffu)>=0x00800000u && (bitcast<u32>(b)&0x7fffffffu)>=0x00800000u{return a+b;}
    let na=float_number(a);let nb=float_number(b);let exponent=max(na.exponent,nb.exponent);
    let value=guide_normalized(na,exponent)+guide_normalized(nb,exponent);
    let magnitude=packed_number(abs(value),exponent);
    return bitcast<f32>((bitcast<u32>(magnitude)&0x7fffffffu)|select(0u,0x80000000u,value<0.));
}
