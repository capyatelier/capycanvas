//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const EFFECT_ABI: u32 = 2;
pub const EFFECT_LUT_SAMPLES: usize = 256;

/// Inline WGSL or manifest-local module names. Catalog loading resolves module
/// lists into shared code; the renderer never performs I/O or source resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EffectShader {
    Code(Arc<str>),
    Modules(Arc<[Arc<str>]>),
    /// Resolved modules remain separate so fused programs can share helpers.
    Linked {
        sources: Arc<[Arc<str>]>,
    },
}
impl EffectShader {
    pub fn sources(&self) -> Result<&[Arc<str>], &'static str> {
        match self {
            Self::Code(code) => Ok(std::slice::from_ref(code)),
            Self::Linked { sources } => Ok(sources),
            Self::Modules(_) => Err("Unresolved effect shader modules"),
        }
    }
}
impl From<&str> for EffectShader {
    fn from(value: &str) -> Self {
        Self::Code(value.into())
    }
}
impl From<String> for EffectShader {
    fn from(value: String) -> Self {
        Self::Code(value.into())
    }
}
impl From<Arc<str>> for EffectShader {
    fn from(value: Arc<str>) -> Self {
        Self::Code(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Adjustment,
    Generator,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectAlpha {
    #[default]
    Preserve,
    /// Blur and remapping can alter coverage. Clipping still preserves the
    /// clipping base's alpha, independently of this declaration.
    Filter,
}

/// WGSL functions take premultiplied linear color, document position and a
/// parameter offset. Empty `passes` means pointwise and permits shader fusion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProgram {
    pub abi: u32,
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub kind: EffectKind,
    #[serde(default)]
    pub alpha: EffectAlpha,
    /// Ordinary WGSL library with a uniquely named function matching the ABI.
    pub wgsl: EffectShader,
    pub entry: Arc<str>,
    /// Ordered image passes. Each reads the previous pass and original input
    /// through fx_sample/fx_original; only the last applies layer properties.
    #[serde(default)]
    pub passes: Arc<[EffectPass]>,
    /// Time is supplied through fx_time. Such programs include the shared
    /// `animate` toggle and `time` (seconds) parameter declarations.
    #[serde(default)]
    pub time: bool,
    /// Small parameter-derived tables, computed on edits rather than per pixel.
    #[serde(default)]
    pub lookups: Arc<[EffectLookup]>,
    pub parameters: Arc<[EffectParameter]>,
    #[serde(default)]
    pub constraints: Arc<[EffectConstraint]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectPass {
    pub entry: Arc<str>,
    pub sampling: EffectSampling,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectSampling {
    /// Maximum footprint in document pixels, including bilinear support.
    Neighborhood {
        radius: u32,
    },
    /// Effective support of the current setting, not its maximum slider value.
    Parameter {
        key: Arc<str>,
        scale: f32,
        padding: u32,
    },
    Document,
}

impl EffectSampling {
    pub fn radius(&self, effect: &EffectInstance) -> Option<u32> {
        match self {
            Self::Neighborhood { radius } => Some(*radius),
            Self::Parameter {
                key,
                scale,
                padding,
            } => match effect.value(key) {
                Some(EffectValue::Number(value)) => {
                    ((value * scale).ceil() as u32).checked_add(*padding)
                }
                _ => None,
            },
            Self::Document => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectLookup {
    /// Preparation library. The wrapper owns bindings and the compute entry.
    pub wgsl: EffectShader,
    pub entry: Arc<str>,
    /// Parameters exposed through prep_parameter, in this order.
    pub dependencies: Arc<[Arc<str>]>,
    /// Fixed output capacity in vec4<f32> records, read through fx_lookup.
    pub values: u32,
    pub workgroup_size: [u32; 3],
    pub workgroups: [u32; 3],
}

impl EffectProgram {
    pub fn with_time_controls(mut self) -> Self {
        self.time = true;
        let mut parameters = self.parameters.to_vec();
        parameters.extend([
            EffectParameter {
                key: "animate".into(),
                label: "Animate".into(),
                section: Some("Animation".into()),
                kind: EffectParameterKind::Toggle,
                default: EffectValue::Toggle(true),
            },
            EffectParameter {
                key: "time".into(),
                label: "Frozen time".into(),
                section: Some("Animation".into()),
                kind: EffectParameterKind::Number {
                    min: 0.,
                    max: 3600.,
                    step: 0.1,
                    decimals: 2,
                    unit: "s".into(),
                },
                default: EffectValue::Number(0.),
            },
        ]);
        self.parameters = parameters.into();
        self
    }
    pub fn image_boundary(&self) -> bool {
        !self.passes.is_empty()
            || self.time
            || (self.kind == EffectKind::Adjustment && self.alpha == EffectAlpha::Filter)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectConstraint {
    OrderedNumbers {
        lower: Arc<str>,
        upper: Arc<str>,
        gap: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectParameter {
    pub key: Arc<str>,
    pub label: Arc<str>,
    /// Consecutive parameters in the same section share one heading/divider.
    #[serde(default)]
    pub section: Option<Arc<str>>,
    pub kind: EffectParameterKind,
    pub default: EffectValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectParameterKind {
    Number {
        min: f32,
        max: f32,
        step: f32,
        decimals: u8,
        unit: Arc<str>,
    },
    Toggle,
    Choice {
        options: Arc<[Arc<str>]>,
    },
    Color,
    Curve,
    Gradient,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EffectValue {
    Number(f32),
    Toggle(bool),
    Choice(u32),
    /// Straight sRGB UI color. The effect declares any conversion it needs.
    Color([f32; 4]),
    Curve(Vec<[f32; 2]>),
    Gradient(Vec<GradientStop>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub program: Arc<EffectProgram>,
    pub values: Vec<EffectValue>,
}
impl EffectInstance {
    pub fn damage_radius(&self) -> Option<u32> {
        self.program.passes.iter().try_fold(0u32, |radius, pass| {
            radius.checked_add(pass.sampling.radius(self)?)
        })
    }
    pub fn animated(&self) -> bool {
        self.program.time && self.value("animate") == Some(&EffectValue::Toggle(true))
    }
    pub fn time_seconds(&self, elapsed: f32) -> f32 {
        if self.animated() {
            elapsed
        } else if let Some(EffectValue::Number(time)) = self.value("time") {
            *time
        } else {
            0.
        }
    }
    pub fn value(&self, key: &str) -> Option<&EffectValue> {
        self.program
            .parameters
            .iter()
            .position(|p| &*p.key == key)
            .map(|i| &self.values[i])
    }
    pub fn new(program: Arc<EffectProgram>) -> Self {
        Self {
            values: program
                .parameters
                .iter()
                .map(|p| p.default.clone())
                .collect(),
            program,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let source_len: usize = self.program.wgsl.sources()?.iter().map(|s| s.len()).sum();
        if source_len == 0 || source_len > 1024 * 1024 {
            return Err("Empty or oversized effect shader");
        }
        if self.program.abi != EFFECT_ABI
            || self.program.passes.len() > 8
            || self.program.parameters.len() > 64
            || self.program.constraints.len() > 128
            || self.values.len() != self.program.parameters.len()
            || self.program.entry.is_empty()
            || self.program.entry.len() > 128
            || !self
                .program
                .entry
                .bytes()
                .enumerate()
                .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
        {
            return Err("Unsupported or invalid effect program");
        }
        for pass in self.program.passes.iter() {
            if pass.entry.is_empty()
                || pass.entry.len() > 128
                || !pass.entry.bytes().enumerate().all(|(i, c)| {
                    c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
                || matches!(pass.sampling, EffectSampling::Neighborhood { radius } if radius > 4096)
            {
                return Err("Invalid image pass");
            }
            if let EffectSampling::Parameter{key,scale,padding}=&pass.sampling
                && (!scale.is_finite() || *scale<0. || !self.program.parameters.iter().any(|p| p.key==*key && matches!(p.kind,EffectParameterKind::Number{min,max,..} if min>=0. && max*scale+*padding as f32<=4096.))) {
                return Err("Invalid parameter-derived sampling footprint");
            }
        }
        if self.program.lookups.len() > 8 {
            return Err("Too many effect lookup tables");
        }
        for lookup in self.program.lookups.iter() {
            let product = |v: [u32; 3]| v.into_iter().try_fold(1u32, u32::checked_mul);
            if !(1..=4096).contains(&lookup.values)
                || lookup
                    .wgsl
                    .sources()?
                    .iter()
                    .map(|s| s.len())
                    .sum::<usize>()
                    > 256 * 1024
                || lookup.entry.is_empty()
                || lookup.entry.len() > 128
                || !lookup.entry.bytes().enumerate().all(|(i, c)| {
                    c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
                || !matches!(product(lookup.workgroup_size), Some(1..=256))
                || !matches!(product(lookup.workgroups), Some(1..=256))
                || lookup.dependencies.len() > 64
            {
                return Err("Invalid or oversized effect preparation");
            }
            for (i, key) in lookup.dependencies.iter().enumerate() {
                if lookup.dependencies[..i].contains(key)
                    || !self.program.parameters.iter().any(|p| p.key == *key)
                {
                    return Err("Unknown or duplicate preparation dependency");
                }
            }
        }
        if self.program.time
            && (!matches!(self.value("animate"), Some(EffectValue::Toggle(_)))
                || !matches!(self.value("time"), Some(EffectValue::Number(v)) if v.is_finite() && *v >= 0.))
        {
            return Err("Time-aware effects require Animate and Time parameters");
        }
        for (i, p) in self.program.parameters.iter().enumerate() {
            if self.program.parameters[..i].iter().any(|q| q.key == p.key) {
                return Err("Duplicate effect parameter");
            }
            p.validate(&p.default)?;
            p.validate(&self.values[i])?;
        }
        for constraint in self.program.constraints.iter() {
            let EffectConstraint::OrderedNumbers { lower, upper, gap } = constraint;
            let (a, b) = self.ordered_indices(lower, upper)?;
            let number = |value: &EffectValue| {
                if let EffectValue::Number(v) = value {
                    Ok(*v)
                } else {
                    Err("Ordered parameters must be numeric")
                }
            };
            if !gap.is_finite()
                || *gap < 0.
                || number(&self.values[a])? + gap > number(&self.values[b])? + f32::EPSILON
                || number(&self.program.parameters[a].default)? + gap
                    > number(&self.program.parameters[b].default)? + f32::EPSILON
            {
                return Err("Invalid ordered parameter range");
            }
        }
        Ok(())
    }
    fn ordered_indices(&self, lower: &str, upper: &str) -> Result<(usize, usize), &'static str> {
        let index = |key| {
            self.program
                .parameters
                .iter()
                .position(|p| p.key.as_ref() == key)
                .ok_or("Unknown constrained parameter")
        };
        let (a, b) = (index(lower)?, index(upper)?);
        if a == b {
            return Err("A parameter cannot constrain itself");
        }
        Ok((a, b))
    }
    pub fn set(&mut self, key: &str, mut value: EffectValue) -> Result<(), &'static str> {
        let i = self
            .program
            .parameters
            .iter()
            .position(|p| p.key.as_ref() == key)
            .ok_or("Unknown effect parameter")?;
        self.program.parameters[i].validate(&value)?;
        if let EffectValue::Number(v) = &mut value {
            let mut low = f32::NEG_INFINITY;
            let mut high = f32::INFINITY;
            for constraint in self.program.constraints.iter() {
                let EffectConstraint::OrderedNumbers { lower, upper, gap } = constraint;
                let (a, b) = self.ordered_indices(lower, upper)?;
                match (&self.values[a], &self.values[b]) {
                    (EffectValue::Number(x), EffectValue::Number(y)) => {
                        if i == a {
                            high = high.min(y - gap);
                        }
                        if i == b {
                            low = low.max(x + gap);
                        }
                    }
                    _ => return Err("Ordered parameters must be numeric"),
                }
            }
            if low > high {
                return Err("Conflicting parameter constraints");
            }
            *v = v.clamp(low, high);
        }
        self.program.parameters[i].validate(&value)?;
        self.values[i] = value;
        Ok(())
    }
    /// Small parameter upload, never image processing. Curves/gradients become
    /// lookup data once per edit; shaders do constant-time interpolation.
    pub fn gpu_parameters(&self) -> Vec<[f32; 4]> {
        let mut data = vec![[0.; 4]];
        for value in &self.values {
            match value {
                EffectValue::Number(v) => data.push([*v, 0., 0., 0.]),
                EffectValue::Toggle(v) => data.push([f32::from(*v), 0., 0., 0.]),
                EffectValue::Choice(v) => data.push([*v as f32, 0., 0., 0.]),
                EffectValue::Color(v) => data.push(*v),
                EffectValue::Curve(points) => data.extend((0..EFFECT_LUT_SAMPLES).map(|i| {
                    [
                        curve_value(points, i as f32 / (EFFECT_LUT_SAMPLES - 1) as f32),
                        0.,
                        0.,
                        0.,
                    ]
                })),
                EffectValue::Gradient(stops) => data
                    .extend((0..EFFECT_LUT_SAMPLES).map(|i| {
                        gradient_value(stops, i as f32 / (EFFECT_LUT_SAMPLES - 1) as f32)
                    })),
            }
        }
        let directory = data.len();
        data[0] = [directory as f32, self.program.lookups.len() as f32, 0., 0.];
        data.resize(directory + self.program.lookups.len(), [0.; 4]);
        for (i, lookup) in self.program.lookups.iter().enumerate() {
            data[directory + i] = [data.len() as f32, lookup.values as f32, 0., 0.];
            // Reserve GPU-owned output. Upload only the prefix before these
            // tables on edits, preserving previously prepared results.
            data.resize(data.len() + lookup.values as usize, [0.; 4]);
        }
        data
    }
}

impl EffectParameter {
    pub fn validate(&self, value: &EffectValue) -> Result<(), &'static str> {
        if self.key.is_empty()
            || self.key.len() > 128
            || self.label.is_empty()
            || self.label.len() > 256
            || self.section.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err("Invalid effect parameter metadata");
        }
        let valid = match (&self.kind, value) {
            (
                EffectParameterKind::Number {
                    min,
                    max,
                    step,
                    decimals,
                    ..
                },
                EffectValue::Number(v),
            ) => {
                min.is_finite()
                    && max.is_finite()
                    && min <= max
                    && step.is_finite()
                    && *step > 0.
                    && *decimals <= 6
                    && v.is_finite()
                    && (*min..=*max).contains(v)
            }
            (EffectParameterKind::Toggle, EffectValue::Toggle(_)) => true,
            (EffectParameterKind::Choice { options }, EffectValue::Choice(v)) => {
                (*v as usize) < options.len()
                    && options.len() <= 256
                    && options.iter().all(|o| !o.is_empty() && o.len() <= 256)
            }
            (EffectParameterKind::Color, EffectValue::Color(c)) => valid_color(c),
            (EffectParameterKind::Curve, EffectValue::Curve(p)) => {
                p.len() >= 2
                    && p.len() <= 32
                    && p.first().unwrap()[0] == 0.
                    && p.last().unwrap()[0] == 1.
                    && p.iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && p.windows(2).all(|v| v[0][0] < v[1][0])
            }
            (EffectParameterKind::Gradient, EffectValue::Gradient(s)) => {
                s.len() >= 2
                    && s.len() <= 32
                    && s.first().unwrap().position == 0.
                    && s.last().unwrap().position == 1.
                    && s.iter().all(|v| {
                        v.position.is_finite()
                            && (0.0..=1.0).contains(&v.position)
                            && valid_color(&v.color)
                    })
                    && s.windows(2).all(|v| v[0].position < v[1].position)
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err("Invalid effect parameter value")
        }
    }
}
fn valid_color(c: &[f32; 4]) -> bool {
    c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

/// Shape-preserving cubic Hermite interpolation: smooth curves without the
/// overshoot/ringing of an unconstrained cubic spline.
pub fn curve_value(points: &[[f32; 2]], x: f32) -> f32 {
    let i = points
        .partition_point(|p| p[0] < x)
        .saturating_sub(1)
        .min(points.len() - 2);
    let slope = |j: usize| (points[j + 1][1] - points[j][1]) / (points[j + 1][0] - points[j][0]);
    let tangent = |j: usize| {
        if j == 0 {
            return slope(0);
        }
        if j == points.len() - 1 {
            return slope(j - 1);
        }
        let a = slope(j - 1);
        let b = slope(j);
        if a * b <= 0. {
            0.
        } else {
            let h0 = points[j][0] - points[j - 1][0];
            let h1 = points[j + 1][0] - points[j][0];
            let w0 = 2. * h1 + h0;
            let w1 = h1 + 2. * h0;
            (w0 + w1) / (w0 / a + w1 / b)
        }
    };
    let h = points[i + 1][0] - points[i][0];
    let t = ((x - points[i][0]) / h).clamp(0., 1.);
    let t2 = t * t;
    let t3 = t2 * t;
    ((2. * t3 - 3. * t2 + 1.) * points[i][1]
        + (t3 - 2. * t2 + t) * h * tangent(i)
        + (-2. * t3 + 3. * t2) * points[i + 1][1]
        + (t3 - t2) * h * tangent(i + 1))
    .clamp(0., 1.)
}
pub fn gradient_value(stops: &[GradientStop], x: f32) -> [f32; 4] {
    let i = stops
        .partition_point(|s| s.position < x)
        .saturating_sub(1)
        .min(stops.len() - 2);
    let t = ((x - stops[i].position) / (stops[i + 1].position - stops[i].position)).clamp(0., 1.);
    std::array::from_fn(|c| stops[i].color[c] * (1. - t) + stops[i + 1].color[c] * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures() -> &'static [crate::EffectDefinition] {
        crate::bundled_effect_catalog().filters()
    }
    fn fixture(id: &str) -> &'static crate::EffectDefinition {
        crate::bundled_effect_catalog().get(id).unwrap()
    }
    #[test]
    fn preview_presets_are_valid_and_do_not_change_insertion_defaults() {
        for id in fixtures() {
            let original = EffectInstance::new(id.program());
            let preview = id.preview().unwrap();
            preview.validate().unwrap();
            assert_eq!(original, EffectInstance::new(id.program()));
            assert!(!preview.animated());
        }
    }
    #[test]
    fn image_footprints_follow_parameters_and_validate_their_bounds() {
        let mut blur = EffectInstance::new(fixture("gaussian_blur").program());
        assert_eq!(blur.damage_radius(), Some(18));
        blur.set("sigma", EffectValue::Number(1.)).unwrap();
        assert_eq!(blur.damage_radius(), Some(6));
        blur.set("sigma", EffectValue::Number(0.)).unwrap();
        assert_eq!(blur.damage_radius(), Some(0));
        let mut invalid = (*blur.program).clone();
        Arc::make_mut(&mut invalid.passes)[0].sampling = EffectSampling::Parameter {
            key: "missing".into(),
            scale: 1.,
            padding: 0,
        };
        assert!(EffectInstance::new(Arc::new(invalid)).validate().is_err());
        assert_eq!(
            EffectInstance::new(fixture("kaleidoscope").program()).damage_radius(),
            None
        );
    }
    #[test]
    fn preparation_has_bounded_storage_and_known_dependencies() {
        let mut program = (*fixture("gaussian_blur").program()).clone();
        EffectInstance::new(Arc::new(program.clone()))
            .validate()
            .unwrap();
        Arc::make_mut(&mut program.lookups)[0].values = u32::MAX;
        assert!(
            EffectInstance::new(Arc::new(program.clone()))
                .validate()
                .is_err()
        );
        Arc::make_mut(&mut program.lookups)[0].values = 33;
        Arc::make_mut(&mut program.lookups)[0].dependencies = Arc::from([Arc::from("unknown")]);
        assert!(EffectInstance::new(Arc::new(program)).validate().is_err());
    }
    #[test]
    fn defaults_and_parameter_validation() {
        for kind in fixtures() {
            let fx = EffectInstance::new(kind.program());
            fx.validate().unwrap();
            assert!(!fx.gpu_parameters().is_empty());
        }
        let mut fx = EffectInstance::new(fixture("levels").program());
        assert!(fx.set("gamma", EffectValue::Number(f32::NAN)).is_err());
        assert!(fx.set("black", EffectValue::Toggle(true)).is_err());
        fx.set("black", EffectValue::Number(0.8)).unwrap();
        fx.set("white", EffectValue::Number(0.2)).unwrap();
        assert_eq!(fx.values[1], EffectValue::Number(0.801));
        fx.validate().unwrap();
    }
    #[test]
    fn sections_shorten_labels_without_changing_shader_parameters() {
        let program = fixture("color_balance").program();
        for (i, section) in ["Shadows", "Midtones", "Highlights"]
            .into_iter()
            .enumerate()
        {
            for (j, label) in ["Cyan — Red", "Magenta — Green", "Yellow — Blue"]
                .into_iter()
                .enumerate()
            {
                let parameter = &program.parameters[i * 3 + j];
                assert_eq!(parameter.section.as_deref(), Some(section));
                assert_eq!(parameter.label.as_ref(), label);
            }
        }
        assert!(program.parameters[9].section.is_none());
        let fx = EffectInstance::new(program);
        assert_eq!(
            fx.gpu_parameters(),
            [[11., 0., 0., 0.]]
                .into_iter()
                .chain(vec![[0., 0., 0., 0.]; 9])
                .chain([[1., 0., 0., 0.]])
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn curve_identity_and_no_overshoot() {
        for i in 0..1000 {
            let x = i as f32 / 999.;
            assert!((curve_value(&[[0., 0.], [1., 1.]], x) - x).abs() < 1e-6);
        }
        let p = [[0., 0.], [0.2, 0.8], [0.6, 0.2], [1., 1.]];
        for pair in p.windows(2) {
            for i in 0..100 {
                let x = pair[0][0] + (pair[1][0] - pair[0][0]) * i as f32 / 99.;
                let y = curve_value(&p, x);
                assert!(
                    y >= pair[0][1].min(pair[1][1]) - 1e-6
                        && y <= pair[0][1].max(pair[1][1]) + 1e-6
                );
            }
        }
    }
}
