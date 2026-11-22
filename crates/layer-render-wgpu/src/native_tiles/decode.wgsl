fn native_color(samples:vec4<u32>,maximum:u32,scale:u32)->vec4<f32> {
    let code=samples.rgb*scale;
    let alpha=select(f32(samples.a)/f32(maximum),1.,samples.a==maximum);
    return vec4(transfer[code.r].x,transfer[code.g].x,transfer[code.b].x,alpha);
}
fn native_float(samples:vec4<u32>,float_bits:bool)->vec4<f32> {
    if float_bits {return bitcast<vec4<f32>>(samples);}
    return vec4(half_value(samples.r),half_value(samples.g),half_value(samples.b),half_value(samples.a));
}
fn native_associated(color:vec4<f32>,straight:bool)->vec4<f32> {
    if color.a==0. {return vec4(0.);}
    return vec4(select(color.rgb,color.rgb*color.a,straight),color.a);
}
