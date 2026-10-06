use crate::{Affine, BlendSpace, ColorMixSpace, ImageResolution, LayerBlend,
    Point, ResolutionUnit, color::{ConversionOptions, DocumentColor, ProofRecipe,
    RenderingIntent, RgbColor, RgbSpace, SampleDepth, hdr::SdrRendition}};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError { Invalid(String), Unsupported(String) }
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::Invalid(message) | Self::Unsupported(message) => f.write_str(message) }
    }
}
impl std::error::Error for DecodeError {}
impl From<String> for DecodeError { fn from(message: String) -> Self { Self::Invalid(message) } }
impl From<&str> for DecodeError { fn from(message: &str) -> Self { Self::Invalid(message.into()) } }
impl From<DecodeError> for String { fn from(error: DecodeError) -> Self { error.to_string() } }
pub type DecodeResult<T> = Result<T, DecodeError>;

pub fn object<'a>(value: &'a Value, allowed: &[&str]) -> DecodeResult<&'a Map<String, Value>> {
    let fields = value.as_object().ok_or("Expected an object")?;
    if let Some(key) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(DecodeError::Unsupported(format!("Unknown field {key}")));
    }
    Ok(fields)
}
pub fn required<'a>(fields: &'a Map<String, Value>, key: &str) -> DecodeResult<&'a Value> {
    fields.get(key).ok_or_else(|| DecodeError::Invalid(format!("Missing field {key}")))
}
pub fn string(value: &Value) -> DecodeResult<&str> { value.as_str().ok_or_else(|| "Expected a string".into()) }
pub fn boolean(value: &Value) -> DecodeResult<bool> { value.as_bool().ok_or_else(|| "Expected a boolean".into()) }
pub fn array(value: &Value, length: usize) -> DecodeResult<&[Value]> {
    value.as_array().filter(|values| values.len() == length).map(Vec::as_slice)
        .ok_or_else(|| format!("Expected an array of {length} values").into())
}
pub fn finite_f32(value: &Value) -> DecodeResult<f32> {
    let number = value.as_f64().ok_or("Expected a finite number")?;
    let result = number as f32;
    if !result.is_finite() { return Err("Number exceeds finite F32 range".into()); }
    Ok(result)
}
pub fn u32_value(value: &Value) -> DecodeResult<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| "Expected a U32 integer".into())
}
fn floats<const N: usize>(value: &Value) -> DecodeResult<[f32; N]> {
    let values = array(value, N)?;
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) { *out = finite_f32(value)?; }
    Ok(result)
}
fn float_value(value: f32) -> Value { Value::from(f64::from(value)) }
fn float_array(values: &[f32]) -> Value { Value::Array(values.iter().copied().map(float_value).collect()) }
fn check_finite(values: &[f32]) -> Result<(), String> {
    if values.iter().all(|value| value.is_finite()) { Ok(()) } else { Err("Expected finite F32 values".into()) }
}
fn unsupported(name: &str, value: &str) -> DecodeError { DecodeError::Unsupported(format!("Unknown {name} {value}")) }
fn optional<T>(fields: &Map<String, Value>, key: &str, default: T, parse: impl FnOnce(&Value) -> DecodeResult<T>) -> DecodeResult<T> {
    fields.get(key).map_or(Ok(default), parse)
}
fn insert_float(fields: &mut Map<String, Value>, key: &str, value: f32, default: f32) {
    if value.to_bits() != default.to_bits() { fields.insert(key.into(), float_value(value)); }
}

pub fn parse_rgb_space(value: &Value) -> DecodeResult<RgbSpace> {
    let name=string(value)?;
    RgbSpace::from_id(name).ok_or_else(||unsupported("RGB space",name))
}
pub fn encode_rgb_space(value: RgbSpace) -> Value { Value::from(value.id()) }
pub fn parse_depth(value: &Value) -> DecodeResult<SampleDepth> {
    Ok(match string(value)? { "u8" => SampleDepth::U8, "u16" => SampleDepth::U16,
        "f16" => SampleDepth::F16, "f32" => SampleDepth::F32, name => return Err(unsupported("depth", name)) })
}
pub fn encode_depth(value: SampleDepth) -> Value {
    Value::from(match value { SampleDepth::U8 => "u8", SampleDepth::U16 => "u16", SampleDepth::F16 => "f16", SampleDepth::F32 => "f32" })
}
pub fn parse_mix_space(value: &Value) -> DecodeResult<ColorMixSpace> {
    Ok(match string(value)? { "classic" => ColorMixSpace::Classic, "linear_rgb" => ColorMixSpace::LinearRgb,
        "oklab" => ColorMixSpace::Oklab, name => return Err(unsupported("gradient interpolation", name)) })
}
pub fn encode_mix_space(value: ColorMixSpace) -> Value {
    Value::from(match value { ColorMixSpace::Classic => "classic", ColorMixSpace::LinearRgb => "linear_rgb", ColorMixSpace::Oklab => "oklab" })
}
pub fn parse_rgb_color(value: &Value) -> DecodeResult<RgbColor> {
    let fields = object(value, &["rgba", "linear_rgba", "space"])?;
    let space = optional(fields, "space", RgbSpace::Srgb, parse_rgb_space)?;
    Ok(match (fields.get("rgba"), fields.get("linear_rgba")) {
        (Some(rgba), None) => RgbColor::new(space, floats(rgba)?)?,
        (None, Some(linear)) => RgbColor::from_linear(space, floats(linear)?)?,
        _ => return Err("A color requires exactly one of rgba or linear_rgba".into()),
    })
}
pub fn encode_rgb_color(value: RgbColor) -> Result<Value, String> {
    value.validate()?;
    let mut fields = Map::new();
    match value.linear_rgb {
        Some([r, g, b]) => fields.insert("linear_rgba".into(), float_array(&[r, g, b, value.rgba[3]])),
        None => fields.insert("rgba".into(), float_array(&value.rgba)),
    };
    if value.space != RgbSpace::Srgb { fields.insert("space".into(), encode_rgb_space(value.space)); }
    Ok(Value::Object(fields))
}
pub fn parse_document_color(value: &Value) -> DecodeResult<DocumentColor> {
    let fields = object(value, &["space", "depth"])?;
    Ok(DocumentColor { space: optional(fields, "space", RgbSpace::Srgb, parse_rgb_space)?,
        depth: optional(fields, "depth", SampleDepth::U8, parse_depth)? })
}
pub fn encode_document_color(value: DocumentColor) -> Result<Value, String> {
    let mut fields = Map::new();
    if value.space != RgbSpace::Srgb { fields.insert("space".into(), encode_rgb_space(value.space)); }
    if value.depth != SampleDepth::U8 { fields.insert("depth".into(), encode_depth(value.depth)); }
    Ok(Value::Object(fields))
}
pub fn parse_blend_space(value: &Value) -> DecodeResult<BlendSpace> {
    Ok(match string(value)? { "linear" => BlendSpace::Linear, "perceptual" => BlendSpace::Perceptual,
        name => return Err(unsupported("blend space", name)) })
}
pub fn encode_blend_space(value: BlendSpace) -> Value { Value::from(match value { BlendSpace::Linear => "linear", BlendSpace::Perceptual => "perceptual" }) }
macro_rules! blend_names {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        pub fn parse_layer_blend(value: &Value) -> DecodeResult<LayerBlend> {
            Ok(match string(value)? { $($name => LayerBlend::$variant,)+ name => return Err(unsupported("layer blend", name)) })
        }
        pub fn encode_layer_blend(value: LayerBlend) -> Value { Value::from(match value { $(LayerBlend::$variant => $name,)+ }) }
    };
}
blend_names! { Normal => "normal", Multiply => "multiply", Screen => "screen", Add => "add", Overlay => "overlay",
    SoftLight => "soft_light", Color => "color", Darken => "darken", Lighten => "lighten", ColorBurn => "color_burn",
    LinearBurn => "linear_burn", ColorDodge => "color_dodge", HardLight => "hard_light", VividLight => "vivid_light",
    LinearLight => "linear_light", PinLight => "pin_light", HardMix => "hard_mix", Difference => "difference",
    Exclusion => "exclusion", Subtract => "subtract", Divide => "divide", Hue => "hue", Saturation => "saturation",
    Luminosity => "luminosity", PassThrough => "pass_through" }

pub fn parse_point(value: &Value) -> DecodeResult<Point> { let [x, y] = floats(value)?; Ok(Point { x, y }) }
pub fn encode_point(value: Point) -> Result<Value, String> { check_finite(&[value.x, value.y])?; Ok(float_array(&[value.x, value.y])) }
pub fn parse_size(value: &Value) -> DecodeResult<[u32; 2]> {
    let values = array(value, 2)?;
    let result = [u32_value(&values[0])?, u32_value(&values[1])?];
    if result.contains(&0) { return Err("Pixel size must be positive".into()); }
    Ok(result)
}
pub fn encode_size(value: [u32; 2]) -> Result<Value, String> {
    if value.contains(&0) { return Err("Pixel size must be positive".into()); }
    Ok(Value::Array(value.into_iter().map(Value::from).collect()))
}
pub fn parse_domain(value: &Value) -> DecodeResult<[u32; 2]> { parse_size(required(object(value, &["size"])?, "size")?) }
pub fn encode_domain(value: [u32; 2]) -> Result<Value, String> { Ok(serde_json::json!({"size": encode_size(value)?})) }
pub fn parse_frame(value: &Value) -> DecodeResult<(Point, [u32; 2])> {
    let fields = object(value, &["size", "origin"])?;
    Ok((optional(fields, "origin", Point::default(), parse_point)?, parse_size(required(fields, "size")?)?))
}
pub fn encode_frame(origin: Point, size: [u32; 2]) -> Result<Value, String> {
    let mut fields = Map::new();
    fields.insert("size".into(), encode_size(size)?);
    let point = encode_point(origin)?;
    if [origin.x.to_bits(), origin.y.to_bits()] != [0; 2] { fields.insert("origin".into(), point); }
    Ok(Value::Object(fields))
}
pub fn parse_resolution(value: &Value) -> DecodeResult<ImageResolution> {
    let fields = object(value, &["unit", "density"])?;
    let unit = match string(required(fields, "unit")?)? { "inch" => ResolutionUnit::Inch,
        "centimetre" => ResolutionUnit::Centimetre, "metre" => ResolutionUnit::Metre,
        name => return Err(unsupported("resolution unit", name)) };
    let density = array(required(fields, "density")?, 2)?;
    let density = [parse_size(&density[0])?, parse_size(&density[1])?];
    Ok(ImageResolution { unit, density })
}
pub fn encode_resolution(value: ImageResolution) -> Result<Value, String> {
    value.validate()?;
    let unit = match value.unit { ResolutionUnit::Inch => "inch", ResolutionUnit::Centimetre => "centimetre", ResolutionUnit::Metre => "metre" };
    Ok(serde_json::json!({"unit":unit, "density":[encode_size(value.density[0])?, encode_size(value.density[1])?]}))
}

pub fn parse_affine(value: &Value) -> DecodeResult<Affine> {
    let result = Affine(floats(value)?);
    let [a,b,c,d,_,_] = result.0.map(f64::from);
    if a*d-b*c == 0. { return Err("Invalid affine transform".into()); }
    if result.inverse().is_none() { return Err(DecodeError::Unsupported("Affine transform exceeds evaluator precision".into())); }
    Ok(result)
}
pub fn encode_affine(value: Affine) -> Result<Value, String> {
    if value.inverse().is_none() { return Err("Invalid affine transform".into()); }
    Ok(float_array(&value.0))
}
pub fn parse_sdr(value: &Value) -> DecodeResult<SdrRendition> {
    let fields = object(value, &["exposure", "contrast", "headroom", "highlight_color", "balance"])?;
    let result = SdrRendition { exposure: optional(fields, "exposure", 0., finite_f32)?,
        contrast: optional(fields, "contrast", 1., finite_f32)?, headroom: optional(fields, "headroom", 2.3004484, finite_f32)?,
        highlight_color: optional(fields, "highlight_color", 0.3, finite_f32)?, balance: optional(fields, "balance", 0., finite_f32)? };
    result.validate().map_err(|error| DecodeError::Unsupported(error.into()))?;
    Ok(result)
}
pub fn encode_sdr(value: SdrRendition) -> Result<Value, String> {
    value.validate().map_err(str::to_string)?;
    let mut fields = Map::new();
    for (key, actual, default) in [("exposure", value.exposure, 0.), ("contrast", value.contrast, 1.),
        ("headroom", value.headroom, 2.3004484), ("highlight_color", value.highlight_color, 0.3), ("balance", value.balance, 0.)] {
        insert_float(&mut fields, key, actual, default);
    }
    Ok(Value::Object(fields))
}
pub fn parse_intent(value: &Value) -> DecodeResult<RenderingIntent> {
    Ok(match string(value)? { "perceptual" => RenderingIntent::Perceptual,
        "relative_colorimetric" => RenderingIntent::RelativeColorimetric, "saturation" => RenderingIntent::Saturation,
        "absolute_colorimetric" => RenderingIntent::AbsoluteColorimetric, name => return Err(unsupported("rendering intent", name)) })
}
pub fn encode_intent(value: RenderingIntent) -> Value {
    Value::from(match value { RenderingIntent::Perceptual => "perceptual", RenderingIntent::RelativeColorimetric => "relative_colorimetric",
        RenderingIntent::Saturation => "saturation", RenderingIntent::AbsoluteColorimetric => "absolute_colorimetric" })
}
pub fn parse_proof<P>(value: &Value, profile: impl FnOnce(&Value) -> DecodeResult<P>) -> DecodeResult<ProofRecipe<P>> {
    let fields = object(value, &["name", "profile", "intent", "black_point_compensation", "simulate_paper", "simulate_black_ink"])?;
    let result = ProofRecipe { name: string(required(fields, "name")?)?.into(), profile: profile(required(fields, "profile")?)?,
        conversion: ConversionOptions { intent: optional(fields, "intent", RenderingIntent::RelativeColorimetric, parse_intent)?,
            black_point_compensation: optional(fields, "black_point_compensation", true, boolean)? },
        simulate_paper: optional(fields, "simulate_paper", false, boolean)?, simulate_black_ink: optional(fields, "simulate_black_ink", true, boolean)? };
    result.validate().map_err(|error| match error {
        crate::color::ProofRecipeError::NameLimit => DecodeError::Unsupported(error.diagnostic().into()),
        _ => DecodeError::Invalid(error.diagnostic().into()),
    })?;
    Ok(result)
}
pub fn encode_proof<P>(value: &ProofRecipe<P>, profile: impl FnOnce(&P) -> Result<Value, String>) -> Result<Value, String> {
    value.validate()?;
    let mut fields = Map::new();
    fields.insert("name".into(), Value::from(value.name.clone()));
    fields.insert("profile".into(), profile(&value.profile)?);
    if value.conversion.intent != RenderingIntent::RelativeColorimetric { fields.insert("intent".into(), encode_intent(value.conversion.intent)); }
    if !value.conversion.black_point_compensation { fields.insert("black_point_compensation".into(), Value::Bool(false)); }
    if value.simulate_paper { fields.insert("simulate_paper".into(), Value::Bool(true)); }
    if !value.simulate_black_ink { fields.insert("simulate_black_ink".into(), Value::Bool(false)); }
    Ok(Value::Object(fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire(value: Value) -> Value { serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap() }
    fn invalid<T>(result: DecodeResult<T>) { assert!(matches!(result, Err(DecodeError::Invalid(_)))); }
    fn unsupported<T>(result: DecodeResult<T>) { assert!(matches!(result, Err(DecodeError::Unsupported(_)))); }

    #[test]
    fn scalar_wire_names_and_frozen_defaults() {
        assert_eq!(parse_document_color(&json!({})).unwrap(), DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U8 });
        assert_eq!(encode_document_color(DocumentColor::default()).unwrap(), json!({}));
        assert_eq!(parse_frame(&json!({"size":[11,17]})).unwrap(), (Point::default(), [11,17]));
        assert_eq!(encode_frame(Point::default(), [11,17]).unwrap(), json!({"size":[11,17]}));
        assert_eq!(parse_sdr(&json!({})).unwrap(), SdrRendition { exposure:0., contrast:1., headroom:2.3004484, highlight_color:0.3, balance:0. });
        assert_eq!(encode_sdr(parse_sdr(&json!({})).unwrap()).unwrap(), json!({}));
        for space in RgbSpace::ALL {
            assert_eq!(parse_rgb_space(&encode_rgb_space(space)).unwrap(), space);
            for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
                let color = DocumentColor { space, depth };
                assert_eq!(parse_document_color(&encode_document_color(color).unwrap()).unwrap(), color);
            }
        }
        for blend in LayerBlend::ALL { assert_eq!(parse_layer_blend(&encode_layer_blend(blend)).unwrap(), blend); }
        assert_eq!(encode_layer_blend(LayerBlend::SoftLight), json!("soft_light"));
        assert_eq!(encode_layer_blend(LayerBlend::PassThrough), json!("pass_through"));
        for blend in BlendSpace::ALL { assert_eq!(parse_blend_space(&encode_blend_space(blend)).unwrap(), blend); }
        for unit in [ResolutionUnit::Inch, ResolutionUnit::Centimetre, ResolutionUnit::Metre] {
            let resolution = ImageResolution { unit, density:[[u32::MAX,123],[24001,80]] };
            assert_eq!(parse_resolution(&encode_resolution(resolution).unwrap()).unwrap(), resolution);
        }
    }

    #[test]
    fn malformed_known_values_are_invalid_and_additions_are_unsupported() {
        invalid(parse_rgb_color(&json!({"rgba":[0,0,0,1.1]})));
        invalid(parse_rgb_color(&json!({"rgba":[0,0,0,1], "linear_rgba":[1,0,0,1]})));
        unsupported(parse_rgb_color(&json!({"rgba":[0,0,0,1], "linear_rgb":[1,0,0]})));
        invalid(parse_rgb_color(&json!({"space":"srgb"})));
        invalid(parse_point(&json!([1,2,3])));
        invalid(parse_point(&json!([1e100,0])));
        invalid(parse_domain(&json!({"size":[0,17]})));
        invalid(parse_size(&json!([1.0,2])));
        invalid(parse_resolution(&json!({"unit":"inch", "density":[[300,0],[300,1]]})));
        unsupported(parse_sdr(&json!({"exposure":13})));
        invalid(parse_sdr(&json!({"contrast":null})));
        unsupported(parse_rgb_color(&json!({"rgba":[0,0,0,1], "space":"xyz"})));
        unsupported(parse_rgb_color(&json!({"rgba":[0,0,0,1], "future":true})));
        unsupported(parse_document_color(&json!({"depth":"u32"})));
        unsupported(parse_blend_space(&json!("Linear")));
        unsupported(parse_layer_blend(&json!("SoftLight")));
        unsupported(parse_domain(&json!({"size":[11,17], "origin":[0,0]})));
        unsupported(parse_resolution(&json!({"unit":"foot", "density":[[1,1],[1,1]]})));
        unsupported(parse_sdr(&json!({"method":"future"})));
        assert!(encode_point(Point { x:f32::INFINITY, y:0. }).is_err());
        assert!(encode_rgb_color(RgbColor { rgba:[f32::NAN,0.,0.,1.], ..RgbColor::BLACK }).is_err());
    }

    #[test]
    fn exact_authored_float_components_survive_json_and_omission() {
        for space in RgbSpace::ALL {
            for rgba in [[65504.,100000.125,-0.12345679,0.25], [f32::MAX,-f32::MAX,f32::MIN_POSITIVE,1.],
                [f32::from_bits(1),-f32::from_bits(1),-0.,-0.]] {
                let color = RgbColor::from_linear(space, rgba).unwrap();
                let encoded = encode_rgb_color(color).unwrap();
                assert_ne!(encoded.get("rgba").is_some(), encoded.get("linear_rgba").is_some());
                let restored = parse_rgb_color(&wire(encoded)).unwrap();
                assert_eq!(restored.rgba.map(f32::to_bits), color.rgba.map(f32::to_bits));
                assert_eq!(restored.linear_rgb.map(|v|v.map(f32::to_bits)), color.linear_rgb.map(|v|v.map(f32::to_bits)));
                assert_eq!(restored.linear_in(space).unwrap().map(f32::to_bits), rgba.map(f32::to_bits));
            }
        }
        let origin = Point { x:-0., y:f32::from_bits(1) };
        let restored = parse_frame(&wire(encode_frame(origin,[11,17]).unwrap())).unwrap().0;
        assert_eq!([restored.x.to_bits(),restored.y.to_bits()], [origin.x.to_bits(),origin.y.to_bits()]);
        let sdr = SdrRendition { exposure:-0., balance:-0., ..parse_sdr(&json!({})).unwrap() };
        let restored = parse_sdr(&wire(encode_sdr(sdr).unwrap())).unwrap();
        assert_eq!(restored.exposure.to_bits(),sdr.exposure.to_bits());
        assert_eq!(restored.balance.to_bits(),sdr.balance.to_bits());
    }

    #[test]
    fn finite_transform_precision_is_an_admission_limit() {
        unsupported(parse_affine(&json!([1e-40,0,0,1,0,0])));
        invalid(parse_affine(&json!([0,0,0,1,0,0])));
    }

    #[test]
    fn proof_frozen_defaults_and_known_semantic_validation() {
        let decode_profile = |v:&Value| Ok(v.clone());
        let minimal = json!({"name":"Printer", "profile":{"builtin":"srgb"}});
        let proof = parse_proof(&minimal,decode_profile).unwrap();
        assert_eq!(proof.conversion.intent,RenderingIntent::RelativeColorimetric);
        assert!(proof.conversion.black_point_compensation);
        assert!(!proof.simulate_paper);
        assert!(proof.simulate_black_ink);
        assert_eq!(encode_proof(&proof,|v|Ok(v.clone())).unwrap(),minimal);
        for (value, reason) in [
            (json!({"name":"Printer", "profile":{}, "simulate_paper":true, "simulate_black_ink":false}), crate::color::ProofRecipeError::PaperRequiresBlackInk),
            (json!({"name":"Printer", "profile":{}, "intent":"absolute_colorimetric"}), crate::color::ProofRecipeError::AbsoluteBlackPoint),
        ] {
            assert_eq!(parse_proof(&value,decode_profile).unwrap_err(), DecodeError::Invalid(reason.diagnostic().into()));
        }
        assert!(parse_proof(&json!({"name":"", "profile":{}}),decode_profile).unwrap().name.is_empty());
        unsupported(parse_proof(&json!({"name":"x".repeat(1025), "profile":{}}),decode_profile));
        unsupported(parse_proof(&json!({"name":"Printer", "profile":{}, "intent":"future"}),decode_profile));
        unsupported(parse_proof(&json!({"name":"Printer", "profile":{}, "future":true}),decode_profile));
        let absolute = parse_proof(&json!({"name":"Printer", "profile":{}, "intent":"absolute_colorimetric", "black_point_compensation":false}),decode_profile).unwrap();
        assert_eq!(parse_proof(&encode_proof(&absolute,|v|Ok(v.clone())).unwrap(),decode_profile).unwrap(),absolute);
    }
}
