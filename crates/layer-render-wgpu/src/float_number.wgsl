struct FloatNumber { mantissa:u32, exponent:i32, negative:bool }
fn float_number(value:f32)->FloatNumber {
    let bits=bitcast<u32>(value);let absolute=bits&0x7fffffffu;
    if absolute==0u {return FloatNumber(0u,0,false);}
    if absolute<0x800000u {
        let shift=countLeadingZeros(absolute)-8u;
        return FloatNumber(absolute<<shift,-126-i32(shift),bits>>31u!=0u);
    }
    return FloatNumber((absolute&0x7fffffu)|0x800000u,i32(absolute>>23u)-127,bits>>31u!=0u);
}

fn packed_number(mantissa:f32,exponent:i32)->f32 {
    if mantissa==0. {return 0.;}
    var m=mantissa;var e=exponent;
    for(var i=0u;i<2u;i++){if m<1.{m*=2.;e--;}else if m>=2.{m*=.5;e++;}}
    if e>=-126{return ldexp(m,e);}
    if e< -150{return 0.;}
    let scaled=m*exp2(f32(e+149));var bits=u32(floor(scaled));let fraction=scaled-f32(bits);
    if fraction>.5 || (fraction==.5 && (bits&1u)!=0u){bits++;}
    return bitcast<f32>(bits);
}
