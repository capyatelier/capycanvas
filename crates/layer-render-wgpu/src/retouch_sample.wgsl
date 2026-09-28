// What a retouching stroke copies at a destination pixel: the target as the
// stroke found it, over the reference composite below it. Each 2x2 block of
// pages covers the 512px a 256px region can reach. A shader that includes this
// defines retouch_target_load and retouch_reference_load for the block pages.
struct RetouchMapping {
    scale: vec2<f32>,
    offset: vec2<f32>,
    to_document: vec2<f32>,
    document: vec2<i32>,
    target_block: vec2<i32>,
    reference_block: vec2<i32>,
    opacity: f32,
    mode: u32,
    exact: bool,
}

const RETOUCH_NONE: u32 = 0u;
const RETOUCH_TARGET: u32 = 1u;
const RETOUCH_REFERENCES: u32 = 2u;
const RETOUCH_TINT: u32 = 3u;
// Spot Healing's live tint: a translucent mid grey, premultiplied.
const RETOUCH_TINT_COLOR: vec4<f32> = vec4<f32>(0.107, 0.107, 0.107, 0.5);

fn retouch_mapping(words: array<vec4<u32>, 4>) -> RetouchMapping {
    return RetouchMapping(
        bitcast<vec2<f32>>(words[0].xy),
        bitcast<vec2<f32>>(words[0].zw),
        bitcast<vec2<f32>>(words[1].xy),
        bitcast<vec2<i32>>(words[1].zw),
        bitcast<vec2<i32>>(words[2].xy),
        bitcast<vec2<i32>>(words[2].zw),
        bitcast<f32>(words[3].x),
        words[3].y,
        words[3].z != 0u,
    );
}

fn retouch_block_page(p: vec2<i32>, block: vec2<i32>) -> i32 {
    let local = p - block;
    if (any(local < vec2<i32>(0)) || any(local >= vec2<i32>(512))) {
        return -1;
    }
    let page = local / 256;
    return page.x + page.y * 2;
}

fn retouch_target_texel(m: RetouchMapping, p: vec2<i32>) -> vec4<f32> {
    let page = retouch_block_page(p, m.target_block);
    if page < 0 {
        return vec4<f32>(0.0);
    }
    return retouch_target_load(page, (p - m.target_block) % 256);
}

fn retouch_reference_texel(m: RetouchMapping, p: vec2<i32>) -> vec4<f32> {
    let page = retouch_block_page(p, m.reference_block);
    if page < 0 || any(p < vec2<i32>(0)) || any(p >= m.document) {
        return vec4<f32>(0.0);
    }
    return retouch_reference_load(page, (p - m.reference_block) % 256);
}

// Bilinear taps at texel centers. A whole-pixel mapping lands on texel
// centers, where one tap is the bilinear result.
fn retouch_sample_target(m: RetouchMapping, point: vec2<f32>) -> vec4<f32> {
    let position = point - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    if m.exact {
        return retouch_target_texel(m, base);
    }
    let f = position - floor(position);
    let top = mix(retouch_target_texel(m, base), retouch_target_texel(m, base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(retouch_target_texel(m, base + vec2<i32>(0, 1)), retouch_target_texel(m, base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

fn retouch_sample_reference(m: RetouchMapping, point: vec2<f32>) -> vec4<f32> {
    let position = point - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    if m.exact {
        return retouch_reference_texel(m, base);
    }
    let f = position - floor(position);
    let top = mix(retouch_reference_texel(m, base), retouch_reference_texel(m, base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(retouch_reference_texel(m, base + vec2<i32>(0, 1)), retouch_reference_texel(m, base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

// The premultiplied source for the destination pixel centered at `destination`
// in the target's pixels.
fn retouch_source(m: RetouchMapping, destination: vec2<f32>) -> vec4<f32> {
    if m.mode == RETOUCH_TINT {
        return RETOUCH_TINT_COLOR;
    }
    if m.mode == RETOUCH_NONE {
        return vec4<f32>(0.0);
    }
    let source = m.scale * destination + m.offset;
    let current = retouch_sample_target(m, source);
    if m.mode == RETOUCH_TARGET {
        return current;
    }
    let below = retouch_sample_reference(m, source + m.to_document);
    let over = current * m.opacity;
    return over + below * (1.0 - over.a);
}
