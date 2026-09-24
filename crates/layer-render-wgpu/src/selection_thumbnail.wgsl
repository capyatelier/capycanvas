struct Params { extent: vec2<u32>, unused: vec2<u32> }
@group(0) @binding(0) var<uniform> params: Params;
@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
    return vec4(vec2(f32((i<<1u)&2u),f32(i&2u))*vec2(2.,-2.)+vec2(-1.,1.),0.,1.);
}
@fragment fn fs(@builtin(position) pos:vec4<f32>)->@location(0) vec4<f32> {
    let extent = vec2<f32>(params.extent);
    let scale = max(extent.x,extent.y)/32.;
    let p = (pos.xy-vec2(16.))*scale+extent*.5;
    if any(p<vec2(0.)) || any(p>=extent) { return vec4(0.); }
    // Four document-space samples keep thin contours visible in small previews.
    var gray = 0.;
    for (var y=0u;y<2u;y++) { for (var x=0u;x<2u;x++) {
        gray += brush_selection_at(p+(vec2<f32>(f32(x),f32(y))-.5)*scale*.5)*.25;
    }}
    // Shared UI readback encodes sRGB; masks themselves have no color space.
    let linear = select(gray/12.92,pow((gray+.055)/1.055,2.4),gray>.04045);
    return vec4(vec3(linear),1.);
}
