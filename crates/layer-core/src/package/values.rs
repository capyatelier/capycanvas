use crate::{Affine, BlendSpace, ImageResolution, Interpolation, LayerBlend, LayerPlacement, MeshMap,
    Point, Projective, Rect, ResolutionUnit, color::{ConversionOptions, DocumentColor, ProofRecipe,
    RenderingIntent, RgbColor, RgbSpace, SampleDepth, hdr::SdrRendition}};
use serde_json::{Map, Value};
use std::{fmt, sync::Arc};

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
    Ok(match string(value)? {
        "srgb" => RgbSpace::Srgb, "display_p3" => RgbSpace::DisplayP3,
        "adobe_rgb" => RgbSpace::AdobeRgb, "pro_photo" => RgbSpace::ProPhoto,
        name => return Err(unsupported("RGB space", name)),
    })
}
pub fn encode_rgb_space(value: RgbSpace) -> Value {
    Value::from(match value { RgbSpace::Srgb => "srgb", RgbSpace::DisplayP3 => "display_p3",
        RgbSpace::AdobeRgb => "adobe_rgb", RgbSpace::ProPhoto => "pro_photo" })
}
pub fn parse_depth(value: &Value) -> DecodeResult<SampleDepth> {
    Ok(match string(value)? { "u8" => SampleDepth::U8, "u16" => SampleDepth::U16,
        "f16" => SampleDepth::F16, "f32" => SampleDepth::F32, name => return Err(unsupported("depth", name)) })
}
pub fn encode_depth(value: SampleDepth) -> Value {
    Value::from(match value { SampleDepth::U8 => "u8", SampleDepth::U16 => "u16", SampleDepth::F16 => "f16", SampleDepth::F32 => "f32" })
}
pub fn parse_rgb_color(value: &Value) -> DecodeResult<RgbColor> {
    let fields = object(value, &["rgba", "space", "linear_rgb"])?;
    let color = RgbColor { space: optional(fields, "space", RgbSpace::Srgb, parse_rgb_space)?,
        rgba: floats(required(fields, "rgba")?)?, linear_rgb: fields.get("linear_rgb").map(floats).transpose()? };
    color.validate()?;
    Ok(color)
}
pub fn encode_rgb_color(value: RgbColor) -> Result<Value, String> {
    value.validate()?;
    let mut fields = Map::new();
    fields.insert("rgba".into(), float_array(&value.rgba));
    if value.space != RgbSpace::Srgb { fields.insert("space".into(), encode_rgb_space(value.space)); }
    if let Some(linear) = value.linear_rgb { fields.insert("linear_rgb".into(), float_array(&linear)); }
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
    if result.inverse().is_none() { return Err("Invalid affine transform".into()); }
    Ok(result)
}
pub fn encode_affine(value: Affine) -> Result<Value, String> {
    if value.inverse().is_none() { return Err("Invalid affine transform".into()); }
    Ok(float_array(&value.0))
}
pub fn parse_projective(value: &Value) -> DecodeResult<Projective> {
    let result = Projective(floats(value)?);
    if result.inverse().is_none() { return Err("Invalid projective transform".into()); }
    Ok(result)
}
pub fn encode_projective(value: Projective) -> Result<Value, String> {
    if value.inverse().is_none() { return Err("Invalid projective transform".into()); }
    Ok(float_array(&value.0))
}
pub fn parse_interpolation(value: &Value) -> DecodeResult<Interpolation> {
    Ok(match string(value)? { "nearest" => Interpolation::Nearest, "linear" => Interpolation::Linear,
        "bicubic" => Interpolation::Bicubic, "lanczos" => Interpolation::Lanczos,
        name => return Err(unsupported("interpolation", name)) })
}
pub fn encode_interpolation(value: Interpolation) -> Value {
    Value::from(match value { Interpolation::Nearest => "nearest", Interpolation::Linear => "linear",
        Interpolation::Bicubic => "bicubic", Interpolation::Lanczos => "lanczos" })
}
pub fn parse_mesh(value: &Value) -> DecodeResult<MeshMap> {
    let fields = object(value, &["frame", "breakpoints", "net"])?;
    let axes = array(required(fields, "breakpoints")?, 2)?;
    let mut breakpoints: [Arc<[f32]>; 2] = [Arc::from([]), Arc::from([])];
    for (out, axis) in breakpoints.iter_mut().zip(axes) {
        let values = axis.as_array().ok_or("Invalid mesh breakpoints")?;
        if !(2..=usize::from(MeshMap::MAX_CELLS) + 1).contains(&values.len()) { return Err("Invalid mesh breakpoint count".into()); }
        *out = values.iter().map(finite_f32).collect::<DecodeResult<Vec<_>>>()?.into();
    }
    let values = required(fields, "net")?.as_array().ok_or("Invalid mesh net")?;
    let expected = (3 * (breakpoints[0].len() - 1) + 1) * (3 * (breakpoints[1].len() - 1) + 1);
    if values.len() != expected { return Err("Invalid mesh net size".into()); }
    let mesh = MeshMap { frame: parse_affine(required(fields, "frame")?)?, breakpoints,
        net: values.iter().map(parse_point).collect::<DecodeResult<Vec<_>>>()?.into() };
    if !mesh.valid() { return Err("Invalid mesh".into()); }
    Ok(mesh)
}
pub fn encode_mesh(value: &MeshMap) -> Result<Value, String> {
    if !value.valid() { return Err("Invalid mesh".into()); }
    let net = value.net.iter().copied().map(encode_point).collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::json!({"frame": encode_affine(value.frame)?, "breakpoints":value.breakpoints.each_ref().map(|axis| float_array(axis)), "net":net}))
}
pub fn parse_placement(value: &Value, source: Rect) -> DecodeResult<(Point, LayerPlacement)> {
    let fields = object(value, &["translation", "projective", "mesh", "interpolation"])?;
    let translation = optional(fields, "translation", Point::default(), parse_point)?;
    let placement = LayerPlacement { outer: optional(fields, "projective", Projective::IDENTITY, parse_projective)?,
        mesh: fields.get("mesh").map(parse_mesh).transpose()?.map(Arc::new),
        interpolation: optional(fields, "interpolation", Interpolation::Linear, parse_interpolation)? };
    placement.validate_for(source).map_err(|e| DecodeError::Invalid(e.to_string()))?;
    Ok((translation, placement))
}
pub fn encode_placement(translation: Point, value: &LayerPlacement, source: Rect) -> Result<Value, String> {
    value.validate_for(source).map_err(|e| e.to_string())?;
    let mut fields = placement_fields(translation, value.outer)?;
    if let Some(mesh) = &value.mesh { fields.insert("mesh".into(), encode_mesh(mesh)?); }
    if value.interpolation != Interpolation::Linear { fields.insert("interpolation".into(), encode_interpolation(value.interpolation)); }
    Ok(Value::Object(fields))
}
fn placement_fields(translation: Point, outer: Projective) -> Result<Map<String, Value>, String> {
    let mut fields = Map::new();
    let translation_value = encode_point(translation)?;
    if [translation.x.to_bits(), translation.y.to_bits()] != [0; 2] { fields.insert("translation".into(), translation_value); }
    let projective = encode_projective(outer)?;
    if outer.0.map(f32::to_bits) != Projective::IDENTITY.0.map(f32::to_bits) { fields.insert("projective".into(), projective); }
    Ok(fields)
}
pub fn parse_mask_placement(value: &Value, source: Rect) -> DecodeResult<(Point, Projective)> {
    let fields = object(value, &["translation", "projective"])?;
    let translation = optional(fields, "translation", Point::default(), parse_point)?;
    let projective = optional(fields, "projective", Projective::IDENTITY, parse_projective)?;
    if !projective.covers(source) { return Err("Invalid mask placement".into()); }
    Ok((translation, projective))
}
pub fn encode_mask_placement(translation: Point, projective: Projective, source: Rect) -> Result<Value, String> {
    if !projective.covers(source) { return Err("Invalid mask placement".into()); }
    Ok(Value::Object(placement_fields(translation, projective)?))
}

pub fn parse_sdr(value: &Value) -> DecodeResult<SdrRendition> {
    let fields = object(value, &["exposure", "contrast", "headroom", "highlight_color", "balance"])?;
    let result = SdrRendition { exposure: optional(fields, "exposure", 0., finite_f32)?,
        contrast: optional(fields, "contrast", 1., finite_f32)?, headroom: optional(fields, "headroom", 2.3004484, finite_f32)?,
        highlight_color: optional(fields, "highlight_color", 0.3, finite_f32)?, balance: optional(fields, "balance", 0., finite_f32)? };
    result.validate()?;
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
    result.validate().map_err(String::from)?;
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
        let source = Rect::from_extent([11,17]);
        assert_eq!(parse_placement(&json!({}), source).unwrap(), (Point::default(), LayerPlacement::IDENTITY));
        assert_eq!(encode_placement(Point::default(), &LayerPlacement::IDENTITY, source).unwrap(), json!({}));
        assert_eq!(parse_mask_placement(&json!({}), source).unwrap(), (Point::default(), Projective::IDENTITY));
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
        for interpolation in [Interpolation::Nearest, Interpolation::Linear, Interpolation::Bicubic, Interpolation::Lanczos] {
            assert_eq!(parse_interpolation(&encode_interpolation(interpolation)).unwrap(), interpolation);
        }
        for unit in [ResolutionUnit::Inch, ResolutionUnit::Centimetre, ResolutionUnit::Metre] {
            let resolution = ImageResolution { unit, density:[[u32::MAX,123],[24001,80]] };
            assert_eq!(parse_resolution(&encode_resolution(resolution).unwrap()).unwrap(), resolution);
        }
    }

    #[test]
    fn malformed_known_values_are_invalid_and_additions_are_unsupported() {
        invalid(parse_rgb_color(&json!({"rgba":[0,0,0,1.1]})));
        invalid(parse_rgb_color(&json!({"rgba":[0,0,0,1], "linear_rgb":[1,0,0]})));
        invalid(parse_rgb_color(&json!({"space":"srgb"})));
        invalid(parse_point(&json!([1,2,3])));
        invalid(parse_point(&json!([1e100,0])));
        invalid(parse_domain(&json!({"size":[0,17]})));
        invalid(parse_size(&json!([1.0,2])));
        invalid(parse_resolution(&json!({"unit":"inch", "density":[[300,0],[300,1]]})));
        invalid(parse_sdr(&json!({"exposure":13})));
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
                let restored = parse_rgb_color(&wire(encode_rgb_color(color).unwrap())).unwrap();
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
        let source = Rect::from_extent([11,17]);
        let mut placement = LayerPlacement::IDENTITY;
        placement.outer.0[1] = -0.;
        let restored = parse_placement(&wire(encode_placement(origin,&placement,source).unwrap()),source).unwrap();
        assert_eq!(restored.1.outer.0.map(f32::to_bits),placement.outer.0.map(f32::to_bits));
        assert_eq!(restored.0.x.to_bits(),origin.x.to_bits());
    }

    #[test]
    fn projective_and_mesh_validation_keeps_source_domain_constraints() {
        let source = Rect::from_extent([100,80]);
        let mesh = MeshMap::identity(source,[2,3]).unwrap().split(0,0.3).unwrap();
        let placement = LayerPlacement { outer:Projective::from_affine(Affine([1.,0.25,-0.5,2.,7.,9.])),
            mesh:Some(Arc::new(mesh)), interpolation:Interpolation::Lanczos };
        let translation = Point { x:2.1234567, y:-0. };
        let restored = parse_placement(&wire(encode_placement(translation,&placement,source).unwrap()),source).unwrap();
        assert_eq!(restored.1,placement);
        assert_eq!([restored.0.x.to_bits(),restored.0.y.to_bits()], [translation.x.to_bits(),translation.y.to_bits()]);
        invalid(parse_placement(&json!({"projective":[1,0,0,2,0,0,0,0,1]}),source));
        invalid(parse_placement(&json!({"projective":[1,0,0,0,1,0,-0.02,0,1]}),source));
        invalid(parse_mask_placement(&json!({"projective":[1,0,0,0,1,0,-0.02,0,1]}),source));
        unsupported(parse_mask_placement(&json!({"interpolation":"nearest"}),source));
        unsupported(parse_mask_placement(&json!({"mesh":{}}),source));
        unsupported(parse_placement(&json!({"interpolation":"future"}),source));
        let mut bad_mesh = encode_mesh(placement.mesh.as_ref().unwrap()).unwrap();
        bad_mesh["breakpoints"][0] = json!([0,0,1]);
        invalid(parse_mesh(&bad_mesh));
        let mut future_mesh = encode_mesh(placement.mesh.as_ref().unwrap()).unwrap();
        future_mesh["gpu_grid"] = json!([1,1]);
        unsupported(parse_mesh(&future_mesh));
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
            (json!({"name":"x".repeat(1025), "profile":{}}), crate::color::ProofRecipeError::NameLimit),
        ] {
            assert_eq!(parse_proof(&value,decode_profile).unwrap_err(), DecodeError::Invalid(reason.diagnostic().into()));
        }
        assert!(parse_proof(&json!({"name":"", "profile":{}}),decode_profile).unwrap().name.is_empty());
        unsupported(parse_proof(&json!({"name":"Printer", "profile":{}, "intent":"future"}),decode_profile));
        unsupported(parse_proof(&json!({"name":"Printer", "profile":{}, "future":true}),decode_profile));
        let absolute = parse_proof(&json!({"name":"Printer", "profile":{}, "intent":"absolute_colorimetric", "black_point_compensation":false}),decode_profile).unwrap();
        assert_eq!(parse_proof(&encode_proof(&absolute,|v|Ok(v.clone())).unwrap(),decode_profile).unwrap(),absolute);
    }
}
