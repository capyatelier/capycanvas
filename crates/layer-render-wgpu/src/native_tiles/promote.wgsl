struct PublicationStatus { invalid: u32, clipped: u32 }
@group(0) @binding(0) var canonical: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> status: PublicationStatus;

@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(points[index], 0., 1.);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    if status.invalid != 0u { discard; }
    return textureLoad(canonical, vec2<i32>(p.xy), 0);
}
