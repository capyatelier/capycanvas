struct Settings { size: vec2<u32>, side: u32, radius: u32 }
@group(0) @binding(0) var front: texture_2d<f32>;
@group(0) @binding(1) var back: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<uniform> settings: Settings;

@compute @workgroup_size(8, 8)
fn copy(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= settings.size) { return; }
    textureStore(output, vec2<i32>(id.xy), textureLoad(front, vec2<i32>(id.xy), 0));
}

@compute @workgroup_size(8, 8)
fn blend(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= settings.size) { return; }
    let p = vec2<i32>(id.xy);
    let f = textureLoad(front, p, 0);
    textureStore(output, p, f + textureLoad(back, p, 0) * (1. - f.a));
}

@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= settings.size) { return; }
    let start = id.xy * settings.side;
    let size = min(vec2<u32>(settings.side), textureDimensions(front) - start);
    var sum = vec4<f32>(0.);
    for (var y = 0u; y < size.y; y++) {
        for (var x = 0u; x < size.x; x++) {
            sum += textureLoad(front, vec2<i32>(start + vec2<u32>(x, y)), 0);
        }
    }
    textureStore(output, vec2<i32>(id.xy), sum / f32(size.x * size.y));
}

@compute @workgroup_size(8, 8)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= settings.size) { return; }
    let p = vec2<i32>(id.xy);
    var sum = vec4<f32>(0.);
    for (var x = -i32(settings.radius); x <= i32(settings.radius); x++) {
        let q = clamp(p + vec2<i32>(x, 0), vec2<i32>(0), vec2<i32>(settings.size) - 1);
        sum += textureLoad(front, q, 0);
    }
    textureStore(output, p, sum / f32(2u * settings.radius + 1u));
}
