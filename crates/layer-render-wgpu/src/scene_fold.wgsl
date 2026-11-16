const FOLD_BASE: u32 = 1u;
const FOLD_NATIVE: u32 = 2u;
const FOLD_OVERLAY: u32 = 4u;
const FOLD_CONTRIBUTION: u32 = 8u;
const FOLD_MASK: u32 = 16u;
const FOLD_INVERTED: u32 = 32u;
const FOLD_END: u32 = 64u;
struct Fold {
    count: u32, backdrop: u32, convert: u32, output_convert: u32,
    constant: vec4<f32>,
    origin: vec2<i32>, integer_max: f32, padding: u32,
    clip: vec4<i32>,
    roles: array<vec4<u32>, 4>,
    opacity: array<vec4<f32>, 4>,
    coverage: array<vec4<f32>, 4>,
}
@group(0) @binding(0) var<uniform> fold: Fold;
FOLD_SLOT_BINDINGS
@group(1) @binding(FOLD_TRANSFER) var<storage, read> transfer: array<vec2<f32>>;
@group(2) @binding(0) var fold_output: texture_storage_2d<rgba32float, write>;

fn fold_native(value: vec4<f32>) -> vec4<f32> {
    let samples = vec4<u32>(round(clamp(value, vec4(0.), vec4(1.)) * 255.));
    let alpha = select(f32(samples.a) / fold.integer_max, 1., samples.a == u32(fold.integer_max));
    let code = samples.rgb * (65535u / u32(fold.integer_max));
    let linear = vec3(transfer[code.r].x, transfer[code.g].x, transfer[code.b].x);
    if alpha == 0. { return vec4(0.); }
    return vec4(linear * alpha, alpha);
}

@compute @workgroup_size(32, 2)
fn fold_layers(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = vec2<i32>(id.xy);
    let destination = p + fold.origin;
    if any(destination < fold.clip.xy) || any(destination >= fold.clip.zw) { return; }
    var acc = fold.constant;
    if fold.backdrop != 0u { acc = textureLoad(slot0, p, 0); }
    var raw = vec4(0.);
    var mask = 1.;
    for (var k = fold.backdrop; k < fold.count; k++) {
        let role = fold.roles[k / 4u][k % 4u];
        var value: vec4<f32>;
        switch k {
FOLD_SLOT_CASES        }
        if (role & FOLD_NATIVE) != 0u { value = fold_native(value); }
        if (role & FOLD_BASE) != 0u { raw = vec4(0.); mask = fold.coverage[k / 4u][k % 4u]; }
        if (role & FOLD_MASK) != 0u {
            mask = mix(value.r, 1. - value.r, f32((role & FOLD_INVERTED) != 0u));
        } else if (role & FOLD_OVERLAY) != 0u {
            if (role & FOLD_CONTRIBUTION) == 0u { raw = value + raw * (1. - value.a); }
        } else {
            raw = value;
        }
        if (role & FOLD_END) != 0u {
            var color = raw;
            if (role & FOLD_CONTRIBUTION) != 0u { color = working_encode(value) + working_encode(raw) * (1. - value.a); }
            else if fold.convert != 0u { color = working_encode(raw); }
            let src = color * mask * fold.opacity[k / 4u][k % 4u];
            acc = src + acc * (1. - src.a);
        }
    }
    textureStore(fold_output, destination, scene_convert(acc, f32(fold.output_convert)));
}
