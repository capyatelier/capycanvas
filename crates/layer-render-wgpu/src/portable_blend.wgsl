// Full precision replacement for fixed-function Float32 attachment blending.
struct Settings { origin: vec2<u32>, extent: vec2<u32>, mode: u32, pad: vec3<u32> }
@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var backdrop: texture_2d<f32>;
@group(0) @binding(3) var destination: texture_storage_2d<OUTPUT_FORMAT, write>;
@compute @workgroup_size(8, 8)
fn blend(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= settings.extent) { return; }
    let p = vec2<i32>(settings.origin + id.xy);
    let src = textureLoad(source, p, 0);
    let dst = textureLoad(backdrop, p, 0);
    var result = src + dst * (1.0 - src.a);
    if settings.mode == 1u { result = dst * (1.0 - src.a); }
    if settings.mode == 2u { result = max(src, dst); }
    textureStore(destination, p, result);
}
