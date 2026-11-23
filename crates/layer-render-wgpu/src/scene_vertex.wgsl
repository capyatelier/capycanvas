struct Settings {
    rect: vec4<f32>,
    extent: vec4<f32>,
    options: vec4<f32>,
    color: vec4<f32>,
    source_over: vec4<f32>,
    backdrop: vec4<f32>,
    operation_linear: vec4<f32>,
    operation_offset: vec4<f32>,
    query_grid: vec4<u32>,
    paper_axes: vec4<f32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(1) @binding(0) var front: texture_2d<f32>;
@group(1) @binding(1) var back: texture_2d<f32>;
@group(1) @binding(2) var sampling: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
fn query_grid_begin(origin:vec2<u32>)->vec2<u32> {
    let extent=settings.query_grid.zw;let size=min(extent,vec2(256u));
    return vec2<u32>(max(vec2(0), (2*vec2<i32>(size*origin)-vec2<i32>(extent)+2*vec2<i32>(extent)-1)/(2*vec2<i32>(extent))));
}
fn query_grid_size()->vec2<u32> {
    return min(query_grid_begin(settings.query_grid.xy+vec2(256u)),min(settings.query_grid.zw,vec2(256u)))-query_grid_begin(settings.query_grid.xy);
}
fn query_grid_pixel(index:vec2<u32>)->vec2<f32> {
    let extent=settings.query_grid.zw;let size=min(extent,vec2(256u));
    return vec2<f32>(((2u*(query_grid_begin(settings.query_grid.xy)+index)+1u)*extent)/(2u*size)-settings.query_grid.xy);
}
@vertex fn vertex_main(@builtin(vertex_index) i: u32, @builtin(instance_index) instance:u32) -> Vertex {
    if settings.query_grid.z!=0u {
        let size=query_grid_size();
        let corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
        let p=query_grid_pixel(vec2(instance%size.x,instance/size.x))+corners[i];
        return Vertex(vec4(p.x/settings.extent.x*2.-1.,1.-p.y/settings.extent.y*2.,0.,1.),(p-settings.rect.xy)/settings.rect.zw);
    }
    var corners = array<vec2<f32>,3>(vec2<f32>(0.,0.),vec2<f32>(2.,0.),vec2<f32>(0.,2.));
    let uv = corners[i];
    let p = settings.rect.xy + uv*settings.rect.zw;
    var o: Vertex;
    o.position = vec4<f32>(p.x/settings.extent.x*2.-1.,1.-p.y/settings.extent.y*2.,0.,1.);
    o.uv = uv; return o;
}
