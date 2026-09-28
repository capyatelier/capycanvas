// Retouch sources: the target as a stroke found it, over the reference
// composite below it. Each block binds the 2x2 pages a 256px output can reach.
struct Gather {
    origin: vec2<f32>,
    scale: vec2<f32>,
    offset: vec2<f32>,
    to_document: vec2<f32>,
    target_block: vec2<i32>,
    reference_block: vec2<i32>,
    document: vec2<i32>,
    opacity: f32,
    references: u32,
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

fn block_page(p: vec2<i32>, block: vec2<i32>) -> i32 {
    let local = p - block;
    if (any(local < vec2<i32>(0)) || any(local >= vec2<i32>(512))) {
        return -1;
    }
    let page = local / 256;
    return page.x + page.y * 2;
}

fn target_texel(p: vec2<i32>) -> vec4<f32> {
    let texel = (p - gather.target_block) % 256;
    switch block_page(p, gather.target_block) {
        case 0: { return textureLoad(target0, texel, 0); }
        case 1: { return textureLoad(target1, texel, 0); }
        case 2: { return textureLoad(target2, texel, 0); }
        case 3: { return textureLoad(target3, texel, 0); }
        default: { return vec4<f32>(0.0); }
    }
}

fn reference_texel(p: vec2<i32>) -> vec4<f32> {
    if (any(p < vec2<i32>(0)) || any(p >= gather.document)) {
        return vec4<f32>(0.0);
    }
    let texel = (p - gather.reference_block) % 256;
    switch block_page(p, gather.reference_block) {
        case 0: { return textureLoad(reference0, texel, 0); }
        case 1: { return textureLoad(reference1, texel, 0); }
        case 2: { return textureLoad(reference2, texel, 0); }
        case 3: { return textureLoad(reference3, texel, 0); }
        default: { return vec4<f32>(0.0); }
    }
}

// Bilinear taps at texel centers; whole-pixel mappings read one texel exactly.
fn sample_target(point: vec2<f32>) -> vec4<f32> {
    let position = point - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    let f = position - floor(position);
    let top = mix(target_texel(base), target_texel(base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(target_texel(base + vec2<i32>(0, 1)), target_texel(base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

fn sample_reference(point: vec2<f32>) -> vec4<f32> {
    let position = point - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    let f = position - floor(position);
    let top = mix(reference_texel(base), reference_texel(base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(reference_texel(base + vec2<i32>(0, 1)), reference_texel(base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

@fragment
fn gather_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let destination = gather.origin + position.xy;
    let source = gather.scale * destination + gather.offset;
    let current = sample_target(source);
    if (gather.references == 0u) {
        return current;
    }
    let below = sample_reference(source + gather.to_document);
    let over = current * gather.opacity;
    return over + below * (1.0 - over.a);
}

@fragment
fn copy_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(target0, vec2<i32>(position.xy), 0);
}
