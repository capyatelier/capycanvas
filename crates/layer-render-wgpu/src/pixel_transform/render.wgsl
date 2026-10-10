@vertex fn vertex_main(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(p[index],0.,1.);
}
@fragment fn fragment_main(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    return layer_pixel(position.xy+transform.attachment.xy);
}
