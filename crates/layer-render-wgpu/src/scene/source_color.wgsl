const SOURCE_PACKED16: u32 = 2u;
const SOURCE_SRGB8: u32 = 3u;
const TRANSFER_SRGB: u32 = 0u;
const TRANSFER_ADOBE_RGB: u32 = 2u;
struct SourceColor {
    red: vec4<f32>, green: vec4<f32>, blue: vec4<f32>,
    premultiplied: u32, integer_max: f32, extent: vec2<f32>,
    float_bits: u32, identity: u32, scalar: u32, scalar_scale: f32,
    representation: u32, display: u32, transfer: u32, padding: u32,
}
fn display_decode(raw: vec4<f32>, color: SourceColor) -> vec4<f32> {
    if color.display == 0u { return raw; }
    let alpha = raw.a;
    if alpha == 0. { return vec4(0.); }
    var linear = raw.rgb;
    if color.representation != SOURCE_SRGB8 {
        if color.transfer == TRANSFER_SRGB {
            linear = select(raw.rgb / 12.92, pow((raw.rgb + .055) / 1.055, vec3(2.4)), raw.rgb > vec3(.04045));
        } else if color.transfer == TRANSFER_ADOBE_RGB {
            linear = pow(raw.rgb, vec3(563. / 256.));
        } else {
            linear = select(raw.rgb / 16., pow(raw.rgb, vec3(1.8)), raw.rgb > vec3(1. / 32.));
        }
    }
    let rgb = select(vec3(dot(color.red.xyz,linear), dot(color.green.xyz,linear), dot(color.blue.xyz,linear)), linear, color.identity != 0u);
    return vec4(select(rgb * alpha, rgb, color.premultiplied != 0u), alpha);
}
