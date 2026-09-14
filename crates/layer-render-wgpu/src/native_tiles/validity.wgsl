// Inspect bits before arithmetic: NaN comparisons and negative zero must not
// depend on optimizer floating-point assumptions. Used by preflight and encode.
fn scalar_error(value:f32)->u32 {
    let bits=bitcast<u32>(value);
    let magnitude=bits&0x7fffffffu;
    if magnitude>=0x7f800000u {return 1u;}
    if magnitude>0x3f800000u || ((bits>>31u)!=0u && magnitude!=0u) {return 2u;}
    return 0u;
}
fn color_error(value:vec4<f32>)->u32 {
    let bits=bitcast<vec4<u32>>(value);
    if any((bits&vec4(0x7f800000u))==vec4(0x7f800000u)) {return 1u;}
    return scalar_error(value.a);
}
