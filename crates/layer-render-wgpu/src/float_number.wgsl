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
