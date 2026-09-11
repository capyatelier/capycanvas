struct Settings { origin_count_default: vec4<f32> }
@group(0) @binding(0) var<uniform> settings: Settings;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.,-1.), vec2<f32>(3.,-1.), vec2<f32>(-1.,3.));
    return vec4<f32>(p[i],0.,1.);
}
@fragment fn fragment_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy + settings.origin_count_default.xy;
    if settings.origin_count_default.z < .5 { return vec4<f32>(settings.origin_count_default.w); }
    return vec4<f32>(brush_selection_at(p));
}
