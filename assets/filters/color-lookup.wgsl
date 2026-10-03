fn capy_color_lookup(c:vec4<f32>,p:vec2<f32>,base:u32)->vec4<f32> {
    let intensity=fx_parameter(base,2u).x*.01;
    if intensity==0. || fx_parameter(base,0u).x==0. || c.a<=0. {return c;}
    let ceiling=3.401992e38;
    if fx_cube_max(c.rgb)>ceiling*c.a {return c;}
    let space=u32(fx_parameter(base,1u).x);
    let original=fx_unassociate(c);let scale=clamp(fx_cube_max(original),1.,8.507059e37);
    let converted=fx_cube_transform(original/scale,space,true);
    if fx_cube_max(converted)>ceiling/scale {return c;}
    let encoded=sdr_encode(converted*scale,space);
    let bounded=clamp(encoded,fx_auxiliary(1u).rgb,fx_auxiliary(2u).rgb);
    let correction=fx_cube(encoded)-bounded;
    if all(correction==vec3(0.)) {return c;}
    let adjusted=encoded+intensity*correction;
    if any(abs(adjusted)>vec3(fx_cube_limit(space))) {return c;}
    let linear=sdr_decode(adjusted,space);let output_scale=clamp(fx_cube_max(linear),1.,8.507059e37);
    let output=fx_cube_transform(linear/output_scale,space,false);
    if fx_cube_max(output)>ceiling/output_scale {return c;}
    return vec4(output*(output_scale*c.a),c.a);
}
