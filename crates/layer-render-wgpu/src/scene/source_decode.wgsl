// Integer source samples enter Float32 directly. No bounded intermediate or
// premultiplied integer representation is used for the immutable original.
struct Settings {
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    options: vec4<f32>, // space, integer maximum, valid width, valid height
    unused0: vec4<f32>,
    unused1: vec4<f32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(1) @binding(0) var encoded: texture_2d<u32>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(corners[i],0.,1.);
}
@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if any(position.xy >= settings.options.zw) { return vec4<f32>(0.); }
    let samples = textureLoad(encoded, vec2<i32>(position.xy), 0);
    // GPU division may approximate the reciprocal. Keep opaque coverage and
    // the RGB endpoint exactly one, including after repeated compositing.
    let value = select(vec4<f32>(samples) / settings.options.y, vec4<f32>(1.), samples == vec4<u32>(u32(settings.options.y)));
    let linear = sdr_decode(value.rgb, u32(settings.options.x));
    let rgb = vec3<f32>(dot(settings.red.xyz,linear), dot(settings.green.xyz,linear), dot(settings.blue.xyz,linear));
    return vec4<f32>(rgb * value.a, value.a);
}
