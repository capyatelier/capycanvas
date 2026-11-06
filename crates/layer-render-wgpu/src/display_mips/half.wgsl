fn half_bits(value: f32) -> u32 {
    let bits = pack2x16float(vec2(clamp(value, -65504., 65504.), 0.)) & 0xffffu;
    if (bits & 0x7fffu) == 0u && value != 0. { return select(0x8001u, 1u, value > 0.); }
    return bits;
}
fn half_value(bits: u32) -> f32 { return unpack2x16float(bits).x; }

fn half_keeping_clipping(color: vec4<f32>) -> vec4<f32> {
    let alpha = half_bits(color.a);
    var rgb: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        var bits = half_bits(color[i]);
        if color[i] < color.a && alpha != 0u && (alpha & 0x8000u) == 0u && (bits & 0x8000u) == 0u && bits >= alpha { bits = alpha - 1u; }
        rgb[i] = half_value(bits);
    }
    return vec4(rgb, half_value(alpha));
}
