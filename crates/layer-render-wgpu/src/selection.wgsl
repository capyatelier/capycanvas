struct Settings { origin_count_default: vec4<f32> }
@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var<storage, read> edges: array<vec4<f32>>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.,-1.), vec2<f32>(3.,-1.), vec2<f32>(-1.,3.));
    return vec4<f32>(p[i],0.,1.);
}
fn inside(p: vec2<f32>) -> f32 {
    var value = settings.origin_count_default.w > 0.5;
    for (var i = 0u; i < u32(settings.origin_count_default.z); i++) {
        let e = edges[i];
        if (e.y > p.y) != (e.w > p.y) {
            if p.x < (e.z-e.x)*(p.y-e.y)/(e.w-e.y)+e.x { value = !value; }
        }
    }
    return select(0., 1., value);
}
@fragment fn fragment_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy + settings.origin_count_default.xy;
    let a = (inside(p+vec2<f32>(-.25,-.25))+inside(p+vec2<f32>(.25,-.25))
        +inside(p+vec2<f32>(-.25,.25))+inside(p+vec2<f32>(.25,.25)))*.25;
    return vec4<f32>(a);
}
