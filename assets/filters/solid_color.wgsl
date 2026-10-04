fn capy_solid_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
    let color=fx_parameter(b,0u);return fx_rgba(color.rgb,color.a);
}
