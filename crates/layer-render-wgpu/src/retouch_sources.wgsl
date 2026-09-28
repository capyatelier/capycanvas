// Retouch sources drawn into a 256px field whose first pixel is `origin` in
// the target's pixels.
struct Gather {
    origin: vec4<f32>,
    mapping: array<vec4<u32>, 4>,
}

@group(0) @binding(0) var target0: texture_2d<f32>;
@group(0) @binding(1) var target1: texture_2d<f32>;
@group(0) @binding(2) var target2: texture_2d<f32>;
@group(0) @binding(3) var target3: texture_2d<f32>;
@group(0) @binding(4) var reference0: texture_2d<f32>;
@group(0) @binding(5) var reference1: texture_2d<f32>;
@group(0) @binding(6) var reference2: texture_2d<f32>;
@group(0) @binding(7) var reference3: texture_2d<f32>;
@group(0) @binding(8) var<uniform> gather: Gather;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

fn retouch_target_load(page: i32, texel: vec2<i32>) -> vec4<f32> {
    switch page {
        case 0: { return textureLoad(target0, texel, 0); }
        case 1: { return textureLoad(target1, texel, 0); }
        case 2: { return textureLoad(target2, texel, 0); }
        default: { return textureLoad(target3, texel, 0); }
    }
}

fn retouch_reference_load(page: i32, texel: vec2<i32>) -> vec4<f32> {
    switch page {
        case 0: { return textureLoad(reference0, texel, 0); }
        case 1: { return textureLoad(reference1, texel, 0); }
        case 2: { return textureLoad(reference2, texel, 0); }
        default: { return textureLoad(reference3, texel, 0); }
    }
}

@fragment
fn gather_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return retouch_source(retouch_mapping(gather.mapping), gather.origin.xy + position.xy);
}

@fragment
fn copy_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(target0, vec2<i32>(position.xy), 0);
}
