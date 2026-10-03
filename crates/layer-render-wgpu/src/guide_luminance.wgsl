fn guide_normalized(value:FloatNumber,exponent:i32)->f32{
    if value.mantissa==0u{return 0.;}
    return select(1.,-1.,value.negative)*(f32(value.mantissa)/8388608.)*exp2(f32(value.exponent-exponent));
}
fn guide_luminance(pixel:vec4<f32>,weights:vec3<f32>)->vec4<f32> {
    let bits=bitcast<vec4<u32>>(pixel);let absolute=bits&vec4<u32>(0x7fffffffu);
    if any(absolute>=vec4<u32>(0x7f800000u)) || absolute.a>0x3f800000u || (bits.a>>31u!=0u && absolute.a!=0u){return vec4(-24.,0.,0.,4.);}
    if absolute.a==0u{return vec4(-24.,0.,0.,0.);}
    let alpha=float_number(pixel.a);let greatest=float_number(bitcast<f32>(max(max(absolute.r,absolute.g),absolute.b)));
    if greatest.mantissa!=0u{
        var ratio=f32(greatest.mantissa)/f32(alpha.mantissa);var exponent=greatest.exponent-alpha.exponent;
        if ratio<1.{ratio*=2.;exponent--;}
        if ratio>=2.{ratio*=.5;exponent++;}
        if exponent>127 || (exponent==127 && ratio>1.9999998807907104){return vec4(-24.,0.,0.,2.);}
    }
    let scale=clamp(greatest.exponent,-126,126);
    let rgb=vec3(guide_normalized(float_number(pixel.r),scale),guide_normalized(float_number(pixel.g),scale),guide_normalized(float_number(pixel.b),scale));
    let luminance=rgb.g+weights.x*(rgb.r-rgb.g)+weights.z*(rgb.b-rgb.g);
    if luminance<=0.{return vec4(-24.,0.,pixel.a,3.);}
    let alpha_log=log2(f32(alpha.mantissa)/8388608.)+f32(alpha.exponent);
    let log_y=log2(luminance)+f32(scale)-alpha_log;
    var peak=3.402823466e38;if log_y<128.{peak=exp2(log_y);}
    return vec4(max(log_y,-24.),peak,pixel.a,1.);
}
