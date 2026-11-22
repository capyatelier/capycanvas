alias Settings = SourceColor;
@group(0) @binding(0) var<uniform> settings: Settings;
SOURCE_INPUT
@group(1) @binding(1) var<storage,read> transfer: array<vec2<f32>>;
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(corners[i],0.,1.);
}
@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if any(position.xy >= settings.extent) { return vec4<f32>(0.); }
    if settings.display != 0u { return display_decode(display_samples(vec2<i32>(position.xy)), settings); }
    let samples = native_samples(vec2<i32>(position.xy));
    if settings.scalar != 0u { return vec4(f32(samples.r) * settings.scalar_scale, 0., 0., 1.); }
    let integer = settings.integer_max != 0.;
    var color: vec4<f32>;
    if integer { color = native_color(samples, u32(settings.integer_max), 65535u / u32(settings.integer_max)); }
    else { color = native_float(samples, settings.float_bits != 0u); }
    let rgb = vec3(dot(settings.red.xyz,color.rgb), dot(settings.green.xyz,color.rgb), dot(settings.blue.xyz,color.rgb));
    return native_associated(vec4(select(rgb, color.rgb, settings.identity != 0u), color.a), !integer || settings.premultiplied == 0u);
}
