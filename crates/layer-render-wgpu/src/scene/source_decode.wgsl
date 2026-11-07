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
    if settings.integer_max==0. {
        var decoded=vec4(half_value(samples.r),half_value(samples.g),half_value(samples.b),half_value(samples.a));
        if settings.float_bits!=0u {decoded=bitcast<vec4<f32>>(samples);}
        let alpha=decoded.a;
        if alpha==0. {return vec4(0.);}
        let linear=decoded.rgb;
        if settings.identity!=0u {return vec4(linear*alpha,alpha); }
        let rgb=vec3(dot(settings.red.xyz,linear),dot(settings.green.xyz,linear),dot(settings.blue.xyz,linear));
        return vec4(rgb*alpha,alpha);
    }
    // Keep opaque coverage exactly one despite approximate GPU reciprocals.
    let alpha = select(f32(samples.a) / settings.integer_max, 1., samples.a == u32(settings.integer_max));
    let scale = 65535u / u32(settings.integer_max);
    let code = samples.rgb * scale;
    let linear = vec3(transfer[code.r].x, transfer[code.g].x, transfer[code.b].x);
    let rgb = vec3<f32>(dot(settings.red.xyz,linear), dot(settings.green.xyz,linear), dot(settings.blue.xyz,linear));
    if alpha == 0. { return vec4<f32>(0.); }
    return vec4<f32>(select(rgb * alpha, rgb, settings.premultiplied != 0u), alpha);
}
