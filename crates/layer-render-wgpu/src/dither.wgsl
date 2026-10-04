fn quantization_noise(position:vec2<f32>,salt:u32)->f32 {
    let point=bitcast<vec2<u32>>(vec2<i32>(floor(position)));
    let seed=point.x*0x9e3779b9u+point.y*0x85ebca6bu+salt;
    var a=seed;var b=seed^0xa511e9b3u;
    a=(a^(a>>16u))*0x7feb352du;a=(a^(a>>15u))*0x846ca68bu;a=a^(a>>16u);
    b=(b^(b>>16u))*0x7feb352du;b=(b^(b>>15u))*0x846ca68bu;b=b^(b>>16u);
    return (f32(a&65535u)+f32(b&65535u)+1.)/65536.-1.;
}
