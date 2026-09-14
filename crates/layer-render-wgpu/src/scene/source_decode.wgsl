// Native integer samples enter Float32 directly. Original images retain
// straight RGB; committed raster tiles declare their alpha association.
struct Settings {
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    options: vec4<f32>, // premultiplied storage, integer maximum, valid width, valid height
    unused0: vec4<f32>,
    unused1: vec4<f32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(1) @binding(0) var encoded: texture_2d<u32>;
@group(1) @binding(1) var<storage,read> transfer: array<vec2<f32>>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(corners[i],0.,1.);
}
@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if any(position.xy >= settings.options.zw) { return vec4<f32>(0.); }
    let samples = textureLoad(encoded, vec2<i32>(position.xy), 0);
    // Keep opaque coverage exactly one despite approximate GPU reciprocals.
    let alpha = select(f32(samples.a) / settings.options.y, 1., samples.a == u32(settings.options.y));
    let scale = 65535u / u32(settings.options.y);
    let code = samples.rgb * scale;
    let linear = vec3(transfer[code.r].x, transfer[code.g].x, transfer[code.b].x);
    let rgb = vec3<f32>(dot(settings.red.xyz,linear), dot(settings.green.xyz,linear), dot(settings.blue.xyz,linear));
    if alpha == 0. { return vec4<f32>(0.); }
    return vec4<f32>(select(rgb * alpha, rgb, settings.options.x != 0.), alpha);
}
