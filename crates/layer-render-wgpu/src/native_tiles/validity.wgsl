// Inspect bits before arithmetic: NaN comparisons and negative zero must not
// depend on optimizer floating-point assumptions.
fn unit_scalar(value:f32)->f32 {
    let bits=bitcast<u32>(value);
    if (bits>>31u)!=0u || (bits&0x7fffffffu)>0x7f800000u {return 0.;}
    if bits>=0x3f800000u {return 1.;}
    return value;
}
fn publishable(value:vec4<f32>)->vec4<f32> {
    let nan=(bitcast<vec4<u32>>(value).rgb&vec3(0x7fffffffu))>vec3(0x7f800000u);
    return vec4(select(value.rgb,vec3(0.),nan),unit_scalar(value.a));
}
// IEEE binary16 ties-to-even, including subnormals. Integer operations avoid
// implementation-dependent half arithmetic/denormal flushing.
fn half_bits(value:f32)->u32 {
    let bits=bitcast<u32>(value);let sign=(bits>>16u)&32768u;
    let exponent=(bits>>23u)&255u;let mantissa=bits&8388607u;
    if exponent<102u {return sign;}
    var shift=13u;var base=0u;var significand=mantissa;
    if exponent<113u {shift=126u-exponent;significand=mantissa|8388608u;}
    else {base=(exponent-112u)<<10u;}
    var rounded=significand>>shift;
    let remainder=significand&((1u<<shift)-1u);let midpoint=1u<<(shift-1u);
    if remainder>midpoint || (remainder==midpoint && (rounded&1u)!=0u) {rounded++;}
    return sign|(base+rounded);
}
fn half_value(bits:u32)->f32 {
    let magnitude=bits&32767u;let sign=select(1.,-1.,(bits&32768u)!=0u);
    if magnitude<1024u {return sign*f32(magnitude)*(1./16777216.);}
    return bitcast<f32>(((bits&32768u)<<16u)|((((bits>>10u)&31u)+112u)<<23u)|((bits&1023u)<<13u));
}
