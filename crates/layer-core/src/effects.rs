//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use crate::color::{RgbColor, RgbSpace};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const EFFECT_ABI: u32 = 3;
/// Header plus two records for at most 32 control points/stops. Curves store
/// analytic Hermite segments; gradients store exact stops, never sampled LUTs.
pub const EFFECT_TABLE_VECTORS: usize = 65;

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
    /// Portable straight color. GPU parameters use encoded document RGB.
    Color(RgbColor),
    Curve(Vec<[f32; 2]>),
    Gradient(Vec<GradientStop>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: RgbColor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub program: Arc<EffectProgram>,
    pub values: Vec<EffectValue>,
}
/// Playback integrates elapsed time at the previous rate. Changing a speed
/// therefore changes future motion without seeking through the animation.
#[derive(Clone, Default)]
pub struct EffectClock {
    previous: Option<(f32, f32, bool)>,
    phase: f32,
}
impl EffectClock {
    pub fn advance(&mut self, effect: &EffectInstance, elapsed: f32) -> f32 {
        let animated = effect.animated();
        let rate = effect.playback_rate();
        match self.previous {
            Some((time, speed, true)) if animated && elapsed >= time => self.phase += (elapsed - time) * speed,
            Some((time, _, false)) if animated && elapsed >= time => {},
            _ => self.phase = effect.time_seconds(elapsed),
        }
        self.previous = Some((elapsed, rate, animated));
        self.phase
    }
}
impl EffectInstance {
    pub fn playback_rate(&self) -> f32 {
        match self.value("speed") { Some(EffectValue::Number(speed)) if self.program.time => *speed, _ => 1. }
    }
    pub fn damage_radius(&self) -> Option<u32> {
        self.program.passes.iter().try_fold(0u32, |radius, pass| {
            radius.checked_add(pass.sampling.radius(self)?)
        })
    }
    pub fn animated(&self) -> bool {
        self.program.time && self.value("animate") == Some(&EffectValue::Toggle(true))
    }
    pub fn time_seconds(&self, elapsed: f32) -> f32 {
        let seconds = if self.animated() {
            elapsed
        } else if let Some(EffectValue::Number(time)) = self.value("time") {
            *time
        } else {
            0.
        };
        seconds * self.playback_rate()
    }
    pub fn value(&self, key: &str) -> Option<&EffectValue> {
        self.program
            .parameters
            .iter()
            .position(|p| &*p.key == key)
            .map(|i| &self.values[i])
    }
    pub fn choice(&self, key: &str) -> Option<&str> {
        let i = self.program.parameters.iter().position(|p| &*p.key == key)?;
        match (&self.program.parameters[i].kind, &self.values[i]) {
            (EffectParameterKind::Choice { options }, EffectValue::Choice(v)) => {
                options.get(*v as usize).map(|o| &**o)
            }
            _ => None,
        }
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
    /// Runtime schema replacement preserves compatible values by key, not by
    /// position. New or individually incompatible fields use their defaults.
    /// Conflicting joint constraints reject publication instead of silently
    /// changing otherwise valid user values.
    pub fn rebind(&self, program: Arc<EffectProgram>) -> Result<Self, &'static str> {
        let mut next = Self::new(program);
        for (parameter, value) in next.program.parameters.iter().zip(&mut next.values) {
            if let Some(old) = self.value(&parameter.key)
                && parameter.validate(old).is_ok()
            {
                *value = old.clone();
            }
        }
        next.validate()?;
        Ok(next)
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
    /// Small parameter upload, never image processing. Curves/gradients retain
    /// exact control data; shaders find the segment with a bounded binary search.
    pub fn gpu_parameters(&self, space: RgbSpace) -> Result<Vec<[f32; 4]>, String> {
        self.validate()?;
        let mut data = vec![[0.; 4]];
        for value in &self.values {
            match value {
                EffectValue::Number(v) => data.push([*v, 0., 0., 0.]),
                EffectValue::Toggle(v) => data.push([f32::from(*v), 0., 0., 0.]),
                EffectValue::Choice(v) => data.push([*v as f32, 0., 0., 0.]),
                EffectValue::Color(v) => data.push(v.encoded_in(space)?),
                EffectValue::Curve(points) => data.extend(curve_parameters(points)),
                EffectValue::Gradient(stops) => data.extend(gradient_parameters(stops, space)?),
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
        Ok(data)
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
fn valid_color(c: &RgbColor) -> bool {
    c.validate_working_spaces().is_ok()
}

fn curve_tangent(points: &[[f32; 2]], j: usize) -> f64 {
    let slope = |i: usize| {
        (f64::from(points[i + 1][1]) - f64::from(points[i][1]))
            / (f64::from(points[i + 1][0]) - f64::from(points[i][0]))
    };
    if j == 0 {
        return slope(0);
    }
    if j == points.len() - 1 {
        return slope(j - 1);
    }
    let a = slope(j - 1);
    let b = slope(j);
    if a * b <= 0. {
        return 0.;
    }
    let h0 = f64::from(points[j][0]) - f64::from(points[j - 1][0]);
    let h1 = f64::from(points[j + 1][0]) - f64::from(points[j][0]);
    let w0 = 2. * h1 + h0;
    let w1 = h1 + 2. * h0;
    (w0 + w1) / (w0 / a + w1 / b)
}
fn curve_segment(points: &[[f32; 2]], i: usize) -> [[f32; 4]; 2] {
    let h = f64::from(points[i + 1][0]) - f64::from(points[i][0]);
    let delta = f64::from(points[i + 1][1]) - f64::from(points[i][1]);
    let d0 = h * curve_tangent(points, i);
    let d1 = h * curve_tangent(points, i + 1);
    [
        [points[i][0], points[i + 1][0], points[i + 1][1], d1 as f32],
        [
            points[i][1],
            d0 as f32,
            (3. * delta - 2. * d0 - d1) as f32,
            (-2. * delta + d0 + d1) as f32,
        ],
    ]
}
fn curve_parameters(points: &[[f32; 2]]) -> [[f32; 4]; EFFECT_TABLE_VECTORS] {
    let mut data = [[0.; 4]; EFFECT_TABLE_VECTORS];
    data[0] = [
        (points.len() - 1) as f32,
        f32::from(points.iter().all(|p| p[0] == p[1])),
        1.,
        0.,
    ];
    for i in 0..points.len() - 1 {
        data[1 + i * 2..3 + i * 2].copy_from_slice(&curve_segment(points, i));
    }
    data
}
fn gradient_parameters(
    stops: &[GradientStop],
    space: RgbSpace,
) -> Result<[[f32; 4]; EFFECT_TABLE_VECTORS], String> {
    let mut data = [[0.; 4]; EFFECT_TABLE_VECTORS];
    data[0] = [stops.len() as f32, 0., 2., 0.];
    for (i, stop) in stops.iter().enumerate() {
        let color = stop.color.encoded_in(space)?;
        data[1 + i * 2] = [stop.position, color[0], color[1], color[2]];
        data[2 + i * 2] = [color[3], 0., 0., 0.];
    }
    Ok(data)
}

pub const LOG_CURVE_FLOOR_STOPS: f32 = -8.;

pub fn hdr_curve_white(space: &str, stops: f32) -> Option<f32> {
    match space {
        "Log HDR" => Some(-LOG_CURVE_FLOOR_STOPS / (stops - LOG_CURVE_FLOOR_STOPS)),
        "Linear HDR" => Some(stops.exp2().recip()),
        _ => None,
    }
}

/// Shape-preserving cubic Hermite interpolation in [0,1], with linear endpoint
/// continuation outside it. RGB identity curves preserve extended input exactly.
pub fn curve_value(points: &[[f32; 2]], x: f32) -> f32 {
    if points.iter().all(|p| p[0] == p[1]) {
        return x;
    }
    let i = points
        .partition_point(|p| p[0] < x)
        .saturating_sub(1)
        .min(points.len() - 2);
    let [bounds, coefficient] = curve_segment(points, i);
    let t = (x - bounds[0]) / (bounds[1] - bounds[0]);
    if t < 0. {
        return coefficient[0] + t * coefficient[1];
    }
    if t > 1. {
        return bounds[2] + (t - 1.) * bounds[3];
    }
    if t == 0. {
        return coefficient[0];
    }
    if t == 1. {
        return bounds[2];
    }
    (((coefficient[3] * t + coefficient[2]) * t + coefficient[1]) * t + coefficient[0])
        .clamp(coefficient[0].min(bounds[2]), coefficient[0].max(bounds[2]))
}
/// Interpolate straight, encoded document RGB and alpha, matching the effect
/// table shader. The returned definition records those interpolation coordinates.
pub fn gradient_value(stops: &[GradientStop], x: f32, space: RgbSpace) -> Result<RgbColor, String> {
    if stops.len() < 2 || !x.is_finite() {
        return Err("Invalid gradient sample".into());
    }
    let i = stops
        .partition_point(|s| s.position < x)
        .saturating_sub(1)
        .min(stops.len() - 2);
    let t = ((x - stops[i].position) / (stops[i + 1].position - stops[i].position)).clamp(0., 1.);
    let a = stops[i].color.encoded_in(space)?;
    let b = stops[i + 1].color.encoded_in(space)?;
    RgbColor::new(space, std::array::from_fn(|c| a[c] * (1. - t) + b[c] * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tagged_effect_colors_upload_document_coordinates_and_preserve_definitions() {
        let red = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 123. / 65535.]).unwrap();
        let mut effect = EffectInstance::new(fixture("black_white").program());
        effect.set("tint_color", EffectValue::Color(red)).unwrap();
        let original = serde_json::to_vec(&effect).unwrap();
        let restored: EffectInstance = serde_json::from_slice(&original).unwrap();
        assert_eq!(restored, effect);
        // Independent CSS Color 4 D65 P3/XYZ/sRGB matrix reference, including
        // the negative coordinates that an untagged or clamped UI loses.
        let uploaded = effect.gpu_parameters(RgbSpace::Srgb).unwrap()[8];
        for (actual, expected) in uploaded[..3]
            .iter()
            .zip([1.2249402, -0.0420570, -0.0196376])
        {
            assert!((RgbSpace::Srgb.decode(f64::from(*actual)) - expected).abs() < 2e-6);
        }
        assert_eq!(uploaded[3], red.rgba[3]);
        for space in RgbSpace::ALL {
            assert_eq!(
                effect.gpu_parameters(space).unwrap()[8],
                red.encoded_in(space).unwrap()
            );
            assert_eq!(serde_json::to_vec(&effect).unwrap(), original);
        }
        for color in [
            RgbColor { linear_rgb: None,
                space: RgbSpace::ProPhoto,
                rgba: [f32::MAX, 0., 0., 1.],
            },
            RgbColor { linear_rgb: None,
                space: RgbSpace::Srgb,
                rgba: [0., 0., 0., -0.1],
            },
        ] {
            assert!(effect.set("tint_color", EffectValue::Color(color)).is_err());
            assert_eq!(effect, restored);
        }
        assert!(
            serde_json::from_str::<EffectValue>(r#"{"kind":"color","value":[1,0,0,1]}"#).is_err()
        );
    }

    #[test]
    fn mixed_gamut_gradient_insertion_matches_encoded_document_interpolation() {
        let stops = vec![
            GradientStop {
                position: 0.,
                color: RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.125]).unwrap(),
            },
            GradientStop {
                position: 1.,
                color: RgbColor::new(RgbSpace::ProPhoto, [0.2, 0.4, 0.8, 0.75]).unwrap(),
            },
        ];
        let mut effect = EffectInstance::new(fixture("gradient_map").program());
        effect
            .set("gradient", EffectValue::Gradient(stops.clone()))
            .unwrap();
        for space in RgbSpace::ALL {
            let original = effect.gpu_parameters(space).unwrap();
            let middle = gradient_value(&stops, 0.375, space).unwrap();
            let a = stops[0].color.encoded_in(space).unwrap();
            let b = stops[1].color.encoded_in(space).unwrap();
            assert_eq!(middle.space, space);
            for c in 0..4 {
                assert!((middle.rgba[c] - (a[c] * 0.625 + b[c] * 0.375)).abs() < 1e-7);
            }
            assert_eq!(original[2], [0., a[0], a[1], a[2]]);
            assert_eq!(original[3][0], a[3]);
            let inserted = [
                stops[0].clone(),
                GradientStop {
                    position: 0.375,
                    color: middle,
                },
                stops[1].clone(),
            ];
            for i in 0..=100 {
                let x = i as f32 / 100.;
                let before = gradient_value(&stops, x, space).unwrap();
                let after = gradient_value(&inserted, x, space).unwrap();
                assert!(
                    before
                        .rgba
                        .into_iter()
                        .zip(after.rgba)
                        .all(|(a, b)| (a - b).abs() < 5e-7)
                );
            }
        }
        assert_eq!(
            effect.value("gradient"),
            Some(&EffectValue::Gradient(stops))
        );
    }
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
            assert!(!fx.gpu_parameters(RgbSpace::Srgb).unwrap().is_empty());
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
    fn schema_rebinding_uses_keys_and_rejects_conflicting_constraints() {
        let mut instance = EffectInstance::new(fixture("levels").program());
        instance.set("black", EffectValue::Number(0.4)).unwrap();
        instance.set("white", EffectValue::Number(0.8)).unwrap();
        let mut program = (*instance.program).clone();
        Arc::make_mut(&mut program.parameters).swap(0, 1);
        let rebound = instance.rebind(Arc::new(program.clone())).unwrap();
        assert_eq!(rebound.value("black"), instance.value("black"));
        assert_eq!(rebound.value("white"), instance.value("white"));
        let EffectConstraint::OrderedNumbers { gap, .. } =
            &mut Arc::make_mut(&mut program.constraints)[0];
        *gap = 0.5;
        assert!(
            instance.rebind(Arc::new(program)).is_err(),
            "do not silently clamp compatible user values on reload"
        );
        let mut program = (*instance.program).clone();
        let black = &mut Arc::make_mut(&mut program.parameters)[0];
        let EffectParameterKind::Number { max, .. } = &mut black.kind else {
            panic!()
        };
        *max = 0.2;
        let rebound = instance.rebind(Arc::new(program)).unwrap();
        assert_eq!(rebound.value("black"), Some(&EffectValue::Number(0.)));
        assert_eq!(rebound.value("white"), instance.value("white"));
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
            fx.gpu_parameters(RgbSpace::Srgb).unwrap(),
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

    #[test]
    fn analytic_parameter_tables_preserve_knots_and_reject_the_sampled_abi() {
        let points = vec![[0., 0.], [0.40003, 0.1], [0.40007, 0.9], [1., 1.]];
        let mut fx = EffectInstance::new(fixture("curves").program());
        fx.set("curve_0", EffectValue::Curve(points.clone()))
            .unwrap();
        let data = fx.gpu_parameters(RgbSpace::Srgb).unwrap();
        assert_eq!(data.len(), 3 + 4 * EFFECT_TABLE_VECTORS);
        assert_eq!(data[1], [3., 0., 1., 0.]);
        for p in &points {
            assert_eq!(curve_value(&points, p[0]), p[1]);
        }
        for v in [-2., -0.125, 0., 0.37, 1., 1.5] {
            assert_eq!(curve_value(&[[0., 0.], [0.25, 0.25], [1., 1.]], v), v);
        }
        Arc::make_mut(&mut fx.program).abi = 2;
        assert!(
            fx.validate().is_err(),
            "sampled ABI must not be interpreted as analytic controls"
        );
        let mut levels = EffectInstance::new(fixture("levels").program());
        assert_eq!(
            levels.value("clamp_input"),
            Some(&EffectValue::Toggle(false))
        );
        assert_eq!(
            levels.value("clamp_output"),
            Some(&EffectValue::Toggle(false))
        );
        levels
            .set("clamp_input", EffectValue::Toggle(true))
            .unwrap();
        levels
            .set("clamp_output", EffectValue::Toggle(true))
            .unwrap();
        levels.validate().unwrap();
    }
}
