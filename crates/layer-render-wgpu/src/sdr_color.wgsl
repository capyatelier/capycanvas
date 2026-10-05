// Encoded SDR components and linear working values are distinct. Negative
// extended RGB is required for wide-gamut values expressed in sRGB primaries.
// IDs: 0 sRGB, 1 Display P3, 2 Adobe RGB (1998), 3 ProPhoto RGB.
fn sdr_decode_component(value: f32, space: u32) -> f32 {
    let magnitude = abs(value);
    var decoded: f32;
    if space < 2u {
        if magnitude<=0.04045 {return value/12.92;}
        decoded = pow((magnitude + 0.055) / 1.055, 2.4);
    } else if space == 2u {
        decoded = pow(magnitude, 563.0 / 256.0);
    } else {
        if magnitude<=1.0/32.0 {return value/16.0;}
        decoded = pow(magnitude, 1.8);
    }
    return sign(value) * decoded;
}
fn sdr_encode_component(value: f32, space: u32) -> f32 {
    let magnitude = abs(value);
    if magnitude==1. {return value;}
    var encoded: f32;
    if space < 2u {
        if magnitude<=0.0031308 {return value*12.92;}
        encoded = 1.055 * pow(magnitude, 1.0 / 2.4) - 0.055;
    } else if space == 2u {
        encoded = pow(magnitude, 256.0 / 563.0);
    } else {
        if magnitude<=1.0/512.0 {return value*16.0;}
        encoded = pow(magnitude, 1.0 / 1.8);
    }
    return sign(value) * encoded;
}
fn sdr_decode(value: vec3<f32>, space: u32) -> vec3<f32> {
    return vec3<f32>(sdr_decode_component(value.r, space), sdr_decode_component(value.g, space), sdr_decode_component(value.b, space));
}
fn sdr_encode(value: vec3<f32>, space: u32) -> vec3<f32> {
    return vec3<f32>(sdr_encode_component(value.r, space), sdr_encode_component(value.g, space), sdr_encode_component(value.b, space));
}
