struct ScreenCheck { red: vec4<f32>, green: vec4<f32>, blue: vec4<f32>, options: vec4<f32> }
@group(0) @binding(12) var<uniform> screen_check: ScreenCheck;
@group(1) @binding(0) var<storage, read_write> screen_counts: array<atomic<u32>, 64>;
const SCREEN_TOLERANCE: f32 = .01;
const SCREEN_MARK: vec3<f32> = vec3<f32>(0., .17, 1.);

fn screen_beyond(rgb: vec3<f32>, bounded: bool) -> bool {
    let top = max(rgb.r, max(rgb.g, rgb.b));
    let bottom = min(rgb.r, min(rgb.g, rgb.b));
    return bottom < -SCREEN_TOLERANCE * max(top, .05) || (bounded && top > 1. + SCREEN_TOLERANCE);
}
fn screen_clipped(paint: vec4<f32>) -> bool {
    if paint.a <= .004 { return false; }
    let rgb = view_working_rgb(paint.rgb / paint.a);
    if screen_check.options.y > .5 && screen_beyond(rgb, true) { return true; }
    if screen_check.options.x < .5 { return false; }
    let screen = vec3<f32>(dot(screen_check.red.xyz, rgb), dot(screen_check.green.xyz, rgb), dot(screen_check.blue.xyz, rgb));
    return screen_beyond(screen, screen_check.options.w > .5);
}
fn screen_marked(paint: vec4<f32>, rgb: vec3<f32>, checker: f32) -> vec3<f32> {
    if screen_check.options.z < .5 || !screen_clipped(paint) { return rgb; }
    return view_ui_rgb(SCREEN_MARK) * paint.a + vec3<f32>(checker) * (1. - paint.a);
}

var<workgroup> screen_group_clipped: atomic<u32>;

fn screen_sample_clipped(block: vec2<u32>) -> bool {
    let surface = vec2<f32>(block * SCREEN_SAMPLE_STRIDE + SCREEN_SAMPLE_STRIDE / 2u) + .5;
    if any(surface >= camera.viewport.xy) { return false; }
    let p = vec2<f32>(dot(camera.inverse.xz, surface), dot(camera.inverse.yw, surface)) + camera.offset_document.xy;
    if any(p < vec2<f32>(0.)) || any(p >= camera.offset_document.zw) { return false; }
    return screen_clipped(proof_artwork(artwork_at(p, camera.inverse.xy, camera.inverse.zw), p));
}

@compute @workgroup_size(8, 8)
fn screen_count(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    if screen_sample_clipped(id.xy) { atomicStore(&screen_group_clipped, 1u); }
    workgroupBarrier();
    if local == 0u && atomicLoad(&screen_group_clipped) != 0u {
        atomicAdd(&screen_counts[(group.x + group.y * 3u) % 64u], 1u);
    }
}
