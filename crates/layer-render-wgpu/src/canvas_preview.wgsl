@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    let p = array(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.))[i];
    return Vertex(vec4(p,0.,1.), vec2((p.x+1.)*.5,(1.-p.y)*.5));
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    // Average premultiplied linear color before unpremultiplication/sRGB export.
    // Sixteen stratified bilinear samples reduce aliasing of fine line art;
    // bounded by preview size, not by document area or layer/filter count.
    let footprint = fwidth(v.uv);
    var color = vec4(0.);
    for (var y=0u; y<4u; y++) {
        for (var x=0u; x<4u; x++) {
            let offset = (vec2(f32(x),f32(y))+.5)/4.-.5;
            color += textureSampleLevel(source, source_sampler, v.uv+offset*footprint, 0.);
        }
    }
    color /= 16.;
    return vec4(color.rgb/max(color.a,.000001), color.a);
}
