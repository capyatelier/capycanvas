// Rasterize a tessellated mesh's source positions into one window of
// destination pixels. Later triangles overwrite earlier ones where it folds.
// Rows x and y first map destination pixels into the window's space, and
// x.w scales the source positions stored.
struct Window { origin:vec2<f32>, size:vec2<f32>, x:vec4<f32>, y:vec4<f32> }
@group(0) @binding(0) var<uniform> window:Window;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) source:vec2<f32> }

@vertex fn vertex_main(@location(0) destination:vec2<f32>, @location(1) source:vec2<f32>, @location(2) weight:f32)->Vertex {
    let h=vec3(destination,1.);
    let p=(vec2(dot(window.x.xyz,h),dot(window.y.xyz,h))-window.origin)/window.size;
    return Vertex(vec4((p.x*2.-1.)*weight,(1.-p.y*2.)*weight,0.,weight),source*window.x.w);
}
@fragment fn fragment_main(vertex:Vertex)->@location(0) vec4<f32> {
    return vec4(vertex.source,0.,0.);
}
