fn quantize_coverage(value:f32, maximum:u32)->u32 {
    // Exact integer rounding of the submitted Float32 coverage. Form the
    // 40-bit product significand*(2^depth-1) as two uint32 words; ordinary float
    // multiplication can round a value just below a half-code up to the tie.
    let bits=bitcast<u32>(value);
    let exponent=(bits>>23u)&255u;
    let shift=150u-exponent;
    if shift>40u {return 0u;}
    let mantissa=(bits&0x7fffffu)|0x800000u;
    let depth=select(16u,8u,maximum==255u);
    let upper=mantissa>>(32u-depth);
    let lower=mantissa<<depth;
    let low=lower-mantissa;
    let high=upper-u32(lower<mantissa);
    if shift>32u {
        return (high>>(shift-32u))+((high>>(shift-33u))&1u);
    }
    if shift==32u {return high+(low>>31u);}
    return ((low>>shift)|(high<<(32u-shift)))+((low>>(shift-1u))&1u);
}
