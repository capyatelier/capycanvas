//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use crate::GradientDefinition;
#[cfg(test)]
use crate::GradientStop;
use crate::color::{RgbColor, RgbSpace};
use crate::authored::{Dimension, Resource};
use crate::effect_catalog::ResourceLabel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumericMapping {
    #[default]
    Linear,
    Log,
    Power {
        exponent: f64,
    },
}

pub const EFFECT_ABI: u32 = 5;
pub const MAX_PIXEL_LENGTH: f32 = 65536.;
/// Header plus two records for at most 32 control points/stops. Curves store
/// analytic Hermite segments; gradients store exact stops, never sampled LUTs.
pub const EFFECT_TABLE_VECTORS: usize = 65;

/// Inline WGSL or manifest-local module names. Catalog loading resolves module
/// lists into shared code; the renderer never performs I/O or source resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EffectShader {
    Code(Resource<str>),
    Modules(Arc<[Arc<str>]>),
    /// Resolved modules remain separate so fused programs can share helpers.
    Linked {
        sources: Arc<[Resource<str>]>,
    },
}
impl EffectShader {
    pub fn sources(&self) -> Result<&[Resource<str>], &'static str> {
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
        Self::Code(value.into())
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

/// The values a filter's input window holds and its output returns, both
/// premultiplied. Retouching filters follow the document's Blending, as in
/// Photoshop; filters that model light stay linear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectSpace {
    #[default]
    Linear,
    /// Encoded values in Perceptual documents, linear ones in Linear light
    /// documents. Pointwise fusion keeps programs with matching spaces together.
    Blending,
}
impl EffectSpace {
    fn is_linear(&self) -> bool {
        *self == Self::Linear
    }
    /// Whether the filter reads and writes encoded values in a document that
    /// blends in `blend`.
    pub fn encoded(self, blend: crate::BlendSpace) -> bool {
        self == Self::Blending && blend == crate::BlendSpace::Perceptual
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectResolution {
    #[default]
    Native,
    Display,
}

/// WGSL functions take premultiplied color in the program's `space`, document
/// position and a parameter offset. Empty `passes` means pointwise. Only
/// built-in pointwise programs without auxiliary resources permit fusion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProgram {
    pub abi: u32,
    pub id: Arc<str>,
    pub label: ResourceLabel,
    pub kind: EffectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constant_color: Option<Arc<str>>,
    #[serde(default)]
    pub alpha: EffectAlpha,
    #[serde(default, skip_serializing_if = "EffectSpace::is_linear")]
    pub space: EffectSpace,
    #[serde(default)]
    pub resolution: EffectResolution,
    /// WGSL source with an entry function matching the ABI.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auxiliary: Option<EffectAuxiliary>,
    #[serde(default)]
    pub pages: Arc<[EffectPage]>,
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
    pub fn radius(&self, effect: EffectView<'_>) -> Option<u32> {
        match self {
            Self::Neighborhood { radius } => Some(*radius),
            Self::Parameter {
                key,
                scale,
                padding,
            } => match effect.parameter(key)? {
                (_, EffectValue::Number(value)) => {
                    ((*value * scale).ceil() as u32).checked_add(*padding)
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectAuxiliary {
    Lut3d { resource: Arc<str>, color_space: Arc<str> },
    Analysis { analysis: EffectAnalysisKind },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectAnalysisKind { LocalIllumination, Dehaze }

impl EffectProgram {
    pub fn uses_spatial_reference(&self) -> bool {
        if crate::bundled_effect_catalog().get(&self.id).is_none_or(|builtin| builtin.program().as_ref() != self) {
            return true;
        }
        !self.passes.is_empty() || matches!(self.id.as_ref(), "vignette" | "film_grain" | "crosshatch" | "iridescence" | "gradient_fill")
    }

    pub fn analysis(&self) -> Option<EffectAnalysisKind> {
        match self.auxiliary { Some(EffectAuxiliary::Analysis {analysis}) => Some(analysis), _ => None }
    }
    pub fn image_boundary(&self) -> bool {
        !self.passes.is_empty() || self.time
    }
    pub fn literal_labels(&self) -> bool {
        let literal = |label: &ResourceLabel| matches!(label, ResourceLabel::Literal(_));
        literal(&self.label) && self.pages.iter().all(|p| literal(&p.label))
            && self.parameters.iter().all(|p| literal(&p.label) && p.section.as_ref().is_none_or(literal)
                && match &p.kind { EffectParameterKind::Choice {options} => options.iter().all(|option| match option {
                    EffectOption::Literal(_) => true, EffectOption::Labeled {label,..} => literal(label),
                }), _ => true })
    }
    pub fn fusion_boundary(&self) -> bool { self.image_boundary() || self.auxiliary.is_some() || crate::bundled_effect_catalog().get(&self.id).is_none() }
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
pub struct EffectPage {
    pub id: Arc<str>,
    pub label: ResourceLabel,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectVisibility {
    pub key: Arc<str>,
    pub value: EffectValue,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectParameter {
    #[serde(default)]
    pub dimension: Dimension,
    #[serde(default)]
    pub opaque: bool,
    pub key: Arc<str>,
    pub label: ResourceLabel,
    /// Consecutive parameters in the same section share one heading/divider.
    #[serde(default)]
    pub section: Option<ResourceLabel>,
    #[serde(default)]
    pub page: Option<Arc<str>>,
    #[serde(default)]
    pub visible_when: Option<EffectVisibility>,
    #[serde(default)]
    pub soft_bounds: Option<[f64; 2]>,
    #[serde(default)]
    pub mapping: NumericMapping,
    pub kind: EffectParameterKind,
    pub default: EffectValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum EffectOption {
    Literal(Arc<str>),
    Labeled { value: Arc<str>, label: ResourceLabel },
}
impl EffectOption {
    pub fn value(&self) -> &str {
        match self { Self::Literal(value) | Self::Labeled { value, .. } => value }
    }
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
        options: Arc<[EffectOption]>,
    },
    Color,
    Curve,
    Gradient,
    Lut3d,
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
    Gradient(GradientDefinition),
    Lut3d(Option<Arc<crate::Lut3d>>),
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
    pub fn at(effect: EffectView<'_>, elapsed: f32, phase: f32) -> Self {
        Self { previous: Some((elapsed, effect.playback_rate(), effect.animated())), phase }
    }
    pub fn advance(&mut self, effect: EffectView<'_>, elapsed: f32) -> f32 {
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectView<'a> {
    pub program: &'a EffectProgram,
    pub values: &'a [EffectValue],
    pub spatial: Option<&'a crate::authored::EffectSpatialReference>,
}
impl<'a> EffectView<'a> {
    pub fn new(program: &'a EffectProgram, values: &'a [EffectValue]) -> Self {
        Self { program, values, spatial: None }
    }
    pub fn with_spatial(mut self, spatial: Option<&'a crate::authored::EffectSpatialReference>) -> Self {
        self.spatial = spatial;
        self
    }
    pub fn spatial_radius(self, radius: u32) -> Option<u32> {
        let scale=self.spatial.map_or(1.,|spatial| {
            let [a,b,c,d,_,_]=spatial.mapping.0;
            (a.abs()+c.abs()).max(b.abs()+d.abs())
        });
        let mapped=(f64::from(radius)*scale).ceil();
        (mapped.is_finite() && mapped<=u32::MAX as f64).then_some(mapped as u32)
    }
    pub fn constant_color(&self) -> Option<RgbColor> {
        match self.value(self.program.constant_color.as_deref()?) {
            Some(EffectValue::Color(color)) => Some(*color),
            _ => None,
        }
    }
    pub fn lut3d(&self) -> Option<&'a Arc<crate::Lut3d>> {
        let EffectAuxiliary::Lut3d { resource, .. } = self.program.auxiliary.as_ref()? else { return None; };
        match self.value(resource) { Some(EffectValue::Lut3d(resource)) => resource.as_ref(), _ => None }
    }
    pub fn resources(self) -> impl Iterator<Item = &'a Arc<crate::Lut3d>> {
        self.values.iter().chain(self.program.parameters.iter().map(|p| &p.default))
            .filter_map(|v| match v { EffectValue::Lut3d(Some(r)) => Some(r), _ => None })
    }
    fn auxiliary_indices(&self) -> Result<Option<(usize,usize)>, &'static str> {
        if self.program.analysis().is_some() && self.program.kind != EffectKind::Adjustment {
            return Err("Source analysis requires an adjustment");
        }
        let count = self.program.parameters.iter().filter(|p| p.kind == EffectParameterKind::Lut3d).count();
        let Some(EffectAuxiliary::Lut3d {resource,color_space}) = &self.program.auxiliary else {
            return if count == 0 { Ok(None) } else { Err("Color lookup requires its auxiliary declaration") };
        };
        let parameter = self.program.parameters.iter().position(|p| p.key == *resource && p.kind == EffectParameterKind::Lut3d)
            .ok_or("Missing color lookup resource parameter")?;
        let space = self.program.parameters.iter().position(|p| p.key == *color_space)
            .ok_or("Missing color lookup color space parameter")?;
        if count != 1 || !matches!(&self.program.parameters[space].kind,
            EffectParameterKind::Choice {options} if !options.is_empty() && options.iter().all(|option| RgbSpace::from_id(option.value()).is_some())) {
            return Err("Invalid color lookup resource or color spaces");
        }
        Ok(Some((parameter,space)))
    }
    fn validate_resource<'v>(&self, value: impl Fn(usize) -> &'v EffectValue) -> Result<(), &'static str> {
        let Some((resource,space)) = self.auxiliary_indices()? else { return Ok(()); };
        let (EffectValue::Lut3d(resource),EffectValue::Choice(choice)) = (value(resource),value(space)) else {
            return Err("Invalid color lookup resource values");
        };
        let EffectParameterKind::Choice {options} = &self.program.parameters[space].kind else { return Err("Invalid color lookup color space"); };
        let space = options.get(*choice as usize).and_then(|option| RgbSpace::from_id(option.value())).ok_or("Invalid color lookup color space")?;
        if resource.as_ref().is_some_and(|r| !r.accepts(space)) { return Err("Color lookup exceeds the selected color space range"); }
        Ok(())
    }
    pub fn playback_rate(&self) -> f32 {
        match self.value("speed") { Some(EffectValue::Number(speed)) if self.program.time => *speed, _ => 1. }
    }
    pub fn damage_radius(&self) -> Option<u32> {
        self.program.passes.iter().try_fold(0u32, |radius, pass| {
            radius.checked_add(self.spatial_radius(pass.sampling.radius(*self)?)?)
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
    pub fn value(&self, key: &str) -> Option<&'a EffectValue> {
        self.parameter(key).map(|(_, value)| value)
    }
    pub fn parameter(&self, key: &str) -> Option<(&'a EffectParameter, &'a EffectValue)> {
        let i = self.program.parameters.iter().position(|p| &*p.key == key)?;
        Some((&self.program.parameters[i], self.values.get(i)?))
    }
    pub fn choice(&self, key: &str) -> Option<&'a str> {
        let i = self.program.parameters.iter().position(|p| &*p.key == key)?;
        match (&self.program.parameters[i].kind, self.values.get(i)?) {
            (EffectParameterKind::Choice { options }, EffectValue::Choice(v)) => {
                options.get(*v as usize).map(EffectOption::value)
            }
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(key) = &self.program.constant_color
            && (self.program.kind != EffectKind::Generator || self.program.time
                || !self.program.passes.is_empty() || !self.program.lookups.is_empty()
                || self.program.auxiliary.is_some()
                || !self.program.parameters.iter().any(|p| p.key == *key && p.kind == EffectParameterKind::Color))
        {
            return Err("Invalid constant color generator");
        }
        let source_len: usize = self.program.wgsl.sources()?.iter().map(|s| s.len()).sum();
        if source_len == 0 || source_len > 1024 * 1024 {
            return Err("Empty or oversized effect shader");
        }
        if self.program.abi != EFFECT_ABI
            || !self.program.label.valid(256)
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
        if self.program.pages.len() > 16 {return Err("Too many effect pages");}
        for (index,page) in self.program.pages.iter().enumerate() {
            if page.id.is_empty() || page.id.len()>128 || !page.label.valid(256)
                || self.program.pages[..index].iter().any(|old|old.id==page.id) {
                return Err("Invalid effect page");
            }
        }
        for parameter in self.program.parameters.iter() {
            if parameter.page.as_ref().is_some_and(|id|!self.program.pages.iter().any(|page|page.id==*id)) {
                return Err("Unknown effect page");
            }
            if let Some(condition)=&parameter.visible_when {
                let Some(other)=self.program.parameters.iter().find(|other|other.key==condition.key) else{return Err("Unknown effect visibility parameter");};
                if condition.key==parameter.key || !matches!((&other.kind,&condition.value),
                    (EffectParameterKind::Toggle,EffectValue::Toggle(_)) |
                    (EffectParameterKind::Choice{..},EffectValue::Choice(_))) || other.validate(&condition.value).is_err() {
                    return Err("Invalid effect visibility condition");
                }
            }
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
                    || !self.program.parameters.iter().any(|p| p.key == *key && p.kind != EffectParameterKind::Lut3d)
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
                || f64::from(number(&self.values[b])?) - f64::from(number(&self.values[a])?) < f64::from(*gap)
                || f64::from(number(&self.program.parameters[b].default)?) - f64::from(number(&self.program.parameters[a].default)?) < f64::from(*gap)
            {
                return Err("Invalid ordered parameter range");
            }
        }
        self.validate_resource(|i| &self.values[i])?;
        self.validate_resource(|i| &self.program.parameters[i].default)?;
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
    /// Small parameter upload, never image processing. Curves/gradients retain
    /// exact control data; shaders find the segment with a bounded binary search.
    pub fn gpu_parameters(&self, space: RgbSpace) -> Result<Vec<[f32; 4]>, String> {
        self.validate()?;
        let mut data = vec![[0.; 4]];
        for (parameter,value) in self.program.parameters.iter().zip(self.values) {
            match value {
                EffectValue::Number(v) => data.push([*v, 0., 0., 0.]),
                EffectValue::Toggle(v) => data.push([f32::from(*v), 0., 0., 0.]),
                EffectValue::Choice(v) => {
                    let code = if matches!(&self.program.auxiliary, Some(EffectAuxiliary::Lut3d {color_space,..}) if color_space == &parameter.key) {
                        RgbSpace::from_id(self.choice(&parameter.key).ok_or("Invalid color lookup color space")?).ok_or("Invalid color lookup color space")?.shader_code()
                    } else {
                        match (self.program.id.as_ref(), parameter.key.as_ref(), self.choice(&parameter.key)) {
                            ("curves", "domain", Some("encoded_rgb")) | ("selective_color", "mode", Some("relative"))
                                | ("gradient_fill", "style", Some("linear")) => 0,
                            ("curves", "domain", Some("log_hdr")) | ("selective_color", "mode", Some("absolute"))
                                | ("gradient_fill", "style", Some("radial")) => 1,
                            ("gradient_fill", "style", Some("reflected")) => 2,
                            ("threshold", "colors", Some("black_white")) | ("threshold", "transparency", Some("keep")) => 0,
                            ("threshold", "colors", Some("black")) | ("threshold", "transparency", Some("threshold")) => 1,
                            ("threshold", "colors", Some("white")) => 2,
                            ("threshold", "colors"|"transparency", _) => return Err("Unknown built-in choice".into()),
                            ("curves", "domain", _) | ("selective_color", "mode", _) | ("gradient_fill", "style", _) => return Err("Unknown built-in choice".into()),
                            _ => *v,
                        }
                    };
                    data.push([code as f32, 0., 0., 0.]);
                },
                EffectValue::Color(v) => data.push(v.encoded_in(space)?),
                EffectValue::Curve(points) => data.extend(curve_parameters(points)),
                EffectValue::Gradient(stops) => data.extend(stops.parameters(space)?),
                EffectValue::Lut3d(resource) => data.push([f32::from(resource.is_some()),0.,0.,0.]),
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
impl<'a> From<&'a EffectInstance> for EffectView<'a> {
    fn from(effect: &'a EffectInstance) -> Self {
        Self::new(&effect.program, &effect.values)
    }
}
impl EffectInstance {
    pub fn view(&self) -> EffectView<'_> { self.into() }
    pub fn lut3d(&self) -> Option<&Arc<crate::Lut3d>> { self.view().lut3d() }
    pub fn resources(&self) -> impl Iterator<Item = &Arc<crate::Lut3d>> { self.view().resources() }
    pub fn playback_rate(&self) -> f32 { self.view().playback_rate() }
    pub fn damage_radius(&self) -> Option<u32> { self.view().damage_radius() }
    pub fn animated(&self) -> bool { self.view().animated() }
    pub fn time_seconds(&self, elapsed: f32) -> f32 { self.view().time_seconds(elapsed) }
    pub fn value(&self, key: &str) -> Option<&EffectValue> { self.view().value(key) }
    pub fn choice(&self, key: &str) -> Option<&str> { self.view().choice(key) }
    pub fn constant_color(&self) -> Option<RgbColor> { self.view().constant_color() }
    pub fn validate(&self) -> Result<(), &'static str> { self.view().validate() }
    pub fn gpu_parameters(&self, space: RgbSpace) -> Result<Vec<[f32; 4]>, String> { self.view().gpu_parameters(space) }
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
    pub fn set_choice(&mut self, key: &str, choice: &str) -> Result<(), &'static str> {
        let parameter=self.program.parameters.iter().find(|p|p.key.as_ref()==key).ok_or("Unknown effect parameter")?;
        let EffectParameterKind::Choice {options}=&parameter.kind else {return Err("Expected a choice parameter");};
        let index=options.iter().position(|option|option.value()==choice).ok_or("Unknown effect choice")?;
        self.set(key,EffectValue::Choice(index as u32))
    }
    pub fn set(&mut self, key: &str, mut value: EffectValue) -> Result<(), &'static str> {
        let i = self
            .program
            .parameters
            .iter()
            .position(|p| p.key.as_ref() == key)
            .ok_or("Unknown effect parameter")?;
        self.program.parameters[i].validate(&value)?;
        if self.program.parameters[i].opaque && let EffectValue::Color(color) = &mut value { color.rgba[3] = 1.; }
        if let EffectValue::Number(v) = &mut value {
            let mut low = f32::NEG_INFINITY;
            let mut high = f32::INFINITY;
            for constraint in self.program.constraints.iter() {
                let EffectConstraint::OrderedNumbers { lower, upper, gap } = constraint;
                let (a, b) = self.view().ordered_indices(lower, upper)?;
                match (&self.values[a], &self.values[b]) {
                    (EffectValue::Number(x), EffectValue::Number(y)) => {
                        if i == a {
                            let limit=f64::from(*y)-f64::from(*gap);
                            let rounded=limit as f32;
                            high=high.min(if f64::from(rounded)>limit {rounded.next_down()} else {rounded});
                        }
                        if i == b {
                            let limit=f64::from(*x)+f64::from(*gap);
                            let rounded=limit as f32;
                            low=low.max(if f64::from(rounded)<limit {rounded.next_up()} else {rounded});
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
        self.view().validate_resource(|index| if index == i { &value } else { &self.values[index] })?;
        self.values[i] = value;
        Ok(())
    }
}

impl EffectParameter {
    fn pixel_length(&self) -> bool {
        matches!(self.dimension, Dimension::SourcePixels | Dimension::CompositionPixels)
    }
    fn accepted_range(&self) -> Option<[f32; 2]> {
        let EffectParameterKind::Number { min, max, .. } = self.kind else { return None };
        Some(if self.pixel_length() {
            [if min < 0. { min.min(-MAX_PIXEL_LENGTH) } else { 0. }, max.max(MAX_PIXEL_LENGTH)]
        } else { [min, max] })
    }
    pub fn accepts(&self, value: f32) -> bool {
        self.accepted_range().is_some_and(|[low, high]| value.is_finite() && (low..=high).contains(&value))
    }
    pub fn validate(&self, value: &EffectValue) -> Result<(), &'static str> {
        if self.key.is_empty()
            || self.key.len() > 128
            || !self.label.valid(256)
            || self.section.as_ref().is_some_and(|s| !s.valid(256))
        {
            return Err("Invalid effect parameter metadata");
        }
        if let EffectParameterKind::Number{min,max,..}=self.kind {
            if self.soft_bounds.is_some_and(|[low,high]|!low.is_finite() || !high.is_finite() || low>high || low<f64::from(min) || high>f64::from(max))
                || matches!(self.mapping,NumericMapping::Power{exponent} if !exponent.is_finite() || !(0.125..=8.).contains(&exponent))
                || matches!(self.mapping,NumericMapping::Log) && min<=0. {
                return Err("Invalid numeric effect presentation");
            }
        } else if self.soft_bounds.is_some() || self.mapping!=NumericMapping::Linear {
            return Err("Numeric presentation on nonnumeric effect parameter");
        }
        if self.opaque && self.kind != EffectParameterKind::Color { return Err("Opaque requires a color parameter"); }
        if self.dimension == Dimension::Count && !matches!(self.kind, EffectParameterKind::Number {..}) { return Err("Count requires a number parameter"); }
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
                    && self.accepts(*v)
                    && (self.dimension != Dimension::Count || [*min, *max, *v].into_iter().all(|v| v.fract() == 0.))
            }
            (EffectParameterKind::Toggle, EffectValue::Toggle(_)) => true,
            (EffectParameterKind::Choice { options }, EffectValue::Choice(v)) => {
                (*v as usize) < options.len()
                    && options.len() <= 256
                    && options.iter().enumerate().all(|(index, option)|
                        !option.value().is_empty() && option.value().len() <= 256
                        && match option { EffectOption::Literal(_) => true, EffectOption::Labeled { label, .. } => label.valid(256) }
                        && options[..index].iter().all(|other| other.value() != option.value()))
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
            (EffectParameterKind::Gradient, EffectValue::Gradient(s)) => s.validate().is_ok(),
            (EffectParameterKind::Lut3d, EffectValue::Lut3d(resource)) => resource.as_ref().is_none_or(|r|
                r.validate_descriptor().is_ok() && RgbSpace::ALL.into_iter().any(|space| r.accepts(space))),
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
fn curve_coefficients(points:&[[f32;2]],i:usize)->([f64;4],f64) {
    let h=f64::from(points[i+1][0])-f64::from(points[i][0]);
    let delta=f64::from(points[i+1][1])-f64::from(points[i][1]);
    let d0=h*curve_tangent(points,i);let d1=h*curve_tangent(points,i+1);
    ([f64::from(points[i][1]),d0,3.*delta-2.*d0-d1,-2.*delta+d0+d1],d1)
}
fn curve_segment(points: &[[f32; 2]], i: usize) -> [[f32; 4]; 2] {
    let (coefficient,d1)=curve_coefficients(points,i);
    [[points[i][0],points[i+1][0],points[i+1][1],d1 as f32],coefficient.map(|v|v as f32)]
}
pub fn curve_inverse(points:&[[f32;2]],target:f32,current:f32)->Option<f32> {
    if points.len()<2 || !target.is_finite() || !current.is_finite() {return None;}
    let target=f64::from(target);let current=f64::from(current);
    let root=(0..points.len()-1).filter_map(|i| {
        let [x0,y0]=points[i].map(f64::from);let [x1,y1]=points[i+1].map(f64::from);
        if target<y0.min(y1) || target>y0.max(y1) {return None;}
        if y0==y1 {return Some(current.clamp(x0,x1));}
        if target==y0 {return Some(x0);}
        if target==y1 {return Some(x1);}
        let (c,_)=curve_coefficients(points,i);let mut low=0.;let mut high=1.;
        for _ in 0..48 {
            let t=(low+high)*0.5;let y=((c[3]*t+c[2])*t+c[1])*t+c[0];
            if (y<target)==(y0<y1) {low=t;} else {high=t;}
        }
        Some(x0+(x1-x0)*(low+high)*0.5)
    }).min_by(|a,b|(a-current).abs().total_cmp(&(b-current).abs()).then(a.total_cmp(b)))? as f32;
    ((f64::from(curve_value(points,root))-target).abs()<=f64::from(8.*f32::EPSILON)).then_some(root)
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
pub const LOG_CURVE_FLOOR_STOPS: f32 = -8.;
pub const CURVE_KEYS: [&str; 4] = ["rgb", "red", "green", "blue"];

pub fn hdr_curve_white(space: &str, stops: f32) -> Option<f32> {
    match space {
        "log_hdr" => Some(-LOG_CURVE_FLOOR_STOPS / (stops - LOG_CURVE_FLOOR_STOPS)),
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_color_contract_rejects_nonconstant_definitions() {
        let fill = EffectInstance::new(fixture("solid_color").program());
        assert!(fill.constant_color().is_some());
        assert!(fill.validate().is_ok());
        for change in 0..4 {
            let mut invalid = fill.clone();
            let program = Arc::make_mut(&mut invalid.program);
            match change {
                0 => program.kind = EffectKind::Adjustment,
                1 => program.time = true,
                2 => program.constant_color = Some("missing".into()),
                _ => program.passes = vec![EffectPass { entry: program.entry.clone(), sampling: EffectSampling::Document }].into(),
            }
            assert_eq!(invalid.validate(), Err("Invalid constant color generator"));
        }
    }
    #[test]
    fn display_resolution_requires_an_explicit_program_declaration() {
        let program = fixture("exposure").program();
        let mut json = serde_json::to_value(&program).unwrap();
        let restored: EffectProgram = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(restored.resolution, EffectResolution::Display);
        assert_eq!(&restored, program.as_ref());
        json.as_object_mut().unwrap().remove("resolution");
        assert_eq!(serde_json::from_value::<EffectProgram>(json.clone()).unwrap().resolution, EffectResolution::Native);
        json["resolution"] = "unknown".into();
        assert!(serde_json::from_value::<EffectProgram>(json).is_err());
    }
    #[test]
    fn tagged_effect_colors_upload_document_coordinates_and_preserve_definitions() {
        let red = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 1.]).unwrap();
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
    fn mixed_gamut_gradient_insertion_preserves_alpha_weighted_classic_field() {
        let stops=vec![
            GradientStop{position:0.,color:RgbColor::new(RgbSpace::DisplayP3,[1.,0.,0.,0.125]).unwrap()},
            GradientStop{position:1.,color:RgbColor::new(RgbSpace::ProPhoto,[0.2,0.4,0.8,0.75]).unwrap()}];
        let gradient=GradientDefinition{stops,interpolation:crate::ColorMixSpace::Classic};
        let mut effect=EffectInstance::new(fixture("gradient_map").program());
        effect.set("gradient",EffectValue::Gradient(gradient.clone())).unwrap();
        for space in RgbSpace::ALL {
            let packed=effect.gpu_parameters(space).unwrap();
            let middle=gradient.sample(0.375,space).unwrap();
            let a=gradient.stops[0].color.encoded_in(space).unwrap();let b=gradient.stops[1].color.encoded_in(space).unwrap();
            let alpha=f64::from(a[3])*0.625+f64::from(b[3])*0.375;
            for channel in 0..3 {let expected=(f64::from(a[channel])*f64::from(a[3])*0.625+f64::from(b[channel])*f64::from(b[3])*0.375)/alpha;
                assert!((f64::from(middle.rgba[channel])-expected).abs()<2e-6);}
            assert_eq!(middle.rgba[3],alpha as f32);
            let linear=gradient.stops[0].color.linear_in(space).unwrap();assert_eq!(packed[2],[0.,linear[0],linear[1],linear[2]]);
            let mut inserted=gradient.clone();inserted.stops.insert(1,GradientStop{position:0.375,color:middle});
            for i in 0..=100 {let x=i as f32/100.;let before=gradient.sample(x,space).unwrap().linear_in(space).unwrap();let after=inserted.sample(x,space).unwrap().linear_in(space).unwrap();
                assert!(before.into_iter().zip(after).all(|(a,b)|(a-b).abs()<2e-6));}
        }
        assert_eq!(effect.value("gradient"),Some(&EffectValue::Gradient(gradient)));
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
    fn pixel_lengths_evaluate_full_authored_values_beyond_control_range() {
        let mut effect=EffectInstance::new(fixture("gaussian_blur").program());
        effect.set("sigma",EffectValue::Number(120.)).unwrap();
        let view=effect.view();
        assert_eq!(view.gpu_parameters(RgbSpace::Srgb).unwrap()[1][0],120.);
        assert_eq!(view.damage_radius(),Some(720));
    }
    #[test]
    fn count_parameters_accept_only_whole_authored_values() {
        for (id,key) in [("posterize","levels"),("kaleidoscope","segments")] {
            let mut effect=EffectInstance::new(fixture(id).program());
            effect.set(key,EffectValue::Number(7.)).unwrap();
            for invalid in [7.25,7.5,f32::NAN] {assert!(effect.set(key,EffectValue::Number(invalid)).is_err());}
            assert_eq!(effect.value(key),Some(&EffectValue::Number(7.)));
        }
    }
    #[test]
    fn borrowed_effect_reads_use_the_authored_values_and_definition() {
        let program = fixture("gaussian_blur").program();
        let mut values: Vec<_> = program.parameters.iter().map(|p| p.default.clone()).collect();
        let sigma = program.parameters.iter().position(|p| &*p.key == "sigma").unwrap();
        values[sigma] = EffectValue::Number(1.);
        let owners = Arc::strong_count(&program);
        let value = EffectView::new(&program, &values).value("sigma").unwrap();
        assert!(std::ptr::eq(value, &values[sigma]));
        let view = fixture("gaussian_blur").view(&values);
        view.validate().unwrap();
        assert_eq!(view.damage_radius(), Some(6));
        assert_eq!(program.passes[0].sampling.radius(view), Some(3));
        assert_eq!(view.gpu_parameters(RgbSpace::Srgb).unwrap()[1], [1.,0.,0.,0.]);
        assert_eq!(values[sigma], EffectValue::Number(1.));
        assert_eq!(Arc::strong_count(&program), owners);
    }
    #[test]
    fn borrowed_effect_validation_rejects_incomplete_and_constrained_values() {
        let program = fixture("levels").program();
        let mut values: Vec<_> = program.parameters.iter().map(|p| p.default.clone()).collect();
        let black = program.parameters.iter().position(|p| &*p.key == "black").unwrap();
        let white = program.parameters.iter().position(|p| &*p.key == "white").unwrap();
        values[black] = EffectValue::Number(0.8);
        values[white] = EffectValue::Number(0.2);
        assert_eq!(EffectView::new(&program, &values).validate(), Err("Invalid ordered parameter range"));
        let incomplete = EffectView::new(&program, &[]);
        assert_eq!(incomplete.value("black"), None);
        assert_eq!(incomplete.choice("black"), None);
        assert_eq!(incomplete.validate(), Err("Unsupported or invalid effect program"));
        assert!(incomplete.gpu_parameters(RgbSpace::Srgb).is_err());
    }
    #[test]
    fn borrowed_clock_preserves_captured_phase_and_integrates_the_previous_rate() {
        let program = fixture("domain_warp").program();
        let mut values: Vec<_> = program.parameters.iter().map(|p| p.default.clone()).collect();
        let animate = program.parameters.iter().position(|p| &*p.key == "animate").unwrap();
        let speed = program.parameters.iter().position(|p| &*p.key == "speed").unwrap();
        values[animate] = EffectValue::Toggle(true);
        values[speed] = EffectValue::Number(1.);
        let mut clock = EffectClock::at(EffectView::new(&program, &values), 0., 5.);
        for (elapsed, rate, phase) in [(1.,2.,6.), (2.,0.,8.), (9.,2.,8.), (10.,2.,10.)] {
            values[speed] = EffectValue::Number(rate);
            let effect = EffectView::new(&program, &values);
            assert_eq!(clock.advance(effect, elapsed), phase);
            let mut captured = EffectClock::at(effect, 0., phase);
            assert_eq!(captured.advance(effect, 0.), phase);
            assert_eq!(captured.advance(effect, 0.5), phase + rate * 0.5);
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
        let EffectValue::Number(white)=fx.values[1] else {panic!()};
        assert!(f64::from(white)-f64::from(0.8f32)>=f64::from(0.001f32));
        assert!(f64::from(white.next_down())-f64::from(0.8f32)<f64::from(0.001f32));
        fx.validate().unwrap();
    }
    #[test]
    fn literal_choice_metadata_preserves_its_value_and_wire_shape() {
        let mut program = (*fixture("curves").program()).clone();
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|p| p.key.as_ref() == "domain").unwrap();
        parameter.kind = EffectParameterKind::Choice { options: [
            EffectOption::Literal("Encoded RGB".into()), EffectOption::Literal("Log HDR".into()),
        ].into() };
        let mut instance = EffectInstance::new(Arc::new(program));
        instance.set("domain", EffectValue::Choice(1)).unwrap();
        let encoded = serde_json::to_string(&instance).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(wire["program"]["parameters"][4]["kind"]["options"], serde_json::json!(["Encoded RGB", "Log HDR"]));
        let copy: EffectInstance = serde_json::from_str(&encoded).unwrap();
        assert_eq!(copy, instance);
        assert_eq!(copy.choice("domain"), Some("Log HDR"));
        let literal: EffectOption = serde_json::from_str(r#""我的 { $name } 🎨""#).unwrap();
        assert_eq!(literal.value(), "我的 { $name } 🎨");
        assert_eq!(serde_json::to_string(&literal).unwrap(), r#""我的 { $name } 🎨""#);
    }
    #[test]
    fn pages_shorten_labels_without_changing_shader_parameters() {
        let program = fixture("color_balance").program();
        for (i, section) in ["Shadows", "Midtones", "Highlights"]
            .into_iter()
            .enumerate()
        {
            for (j, _) in ["Cyan — Red", "Magenta — Green", "Yellow — Blue"]
                .into_iter()
                .enumerate()
            {
                let parameter = &program.parameters[i * 3 + j];
                assert!(parameter.section.is_none());
                assert_eq!(parameter.page.as_deref(), Some(section.to_ascii_lowercase().as_str()));
                assert_eq!(program.pages[i].label, ResourceLabel::Message { message: format!("resources-section-color-balance-{}", section.to_ascii_lowercase()).into() });
                assert_eq!(parameter.label, ResourceLabel::Message { message: format!("resources-parameter-color-balance-{}", parameter.key.replace('_', "-")).into() });
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
        fx.set("rgb", EffectValue::Curve(points.clone()))
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

#[cfg(test)] mod presentation_tests {
    use super::*;
    fn schema()->EffectInstance {
        let program=serde_json::from_value(serde_json::json!({"abi":EFFECT_ABI,"id":"presentation","label":"Presentation","kind":"adjustment","wgsl":"fn capy_presentation() {}","entry":"capy_presentation",
            "pages":[{"id":"rgb","label":"RGB"}],"parameters":[
                {"key":"enabled","label":"Enabled","kind":{"kind":"toggle"},"default":{"kind":"toggle","value":false}},
                {"key":"channel","label":"Channel","kind":{"kind":"choice","options":["RGB","Red"]},"default":{"kind":"choice","value":0}},
                {"key":"gain","label":"Gain","page":"rgb","visible_when":{"key":"enabled","value":{"kind":"toggle","value":true}},"soft_bounds":[0.1,1.],"mapping":{"type":"power","exponent":0.5},
                    "kind":{"kind":"number","min":0.,"max":2.,"step":0.01,"decimals":2,"unit":""},"default":{"kind":"number","value":1.}}
            ]})).unwrap();
        EffectInstance::new(Arc::new(program))
    }
    #[test] fn presentation_does_not_change_parameter_payload_or_hidden_values() {
        let mut effect=schema();effect.validate().unwrap();let payload=effect.gpu_parameters(RgbSpace::Srgb).unwrap();
        Arc::make_mut(&mut effect.program).pages=Arc::from([]);
        Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[2].page=None;
        effect.validate().unwrap();assert_eq!(effect.gpu_parameters(RgbSpace::Srgb).unwrap(),payload);assert_eq!(effect.value("gain"),Some(&EffectValue::Number(1.)));
    }
    #[test] fn page_admission_rejects_unknown_duplicate_and_seventeenth_page() {
        let mut effect=schema();let original=effect.program.clone();
        Arc::make_mut(&mut effect.program).pages=vec![EffectPage{id:"other".into(),label:"Other".into()}].into();assert!(effect.validate().is_err());
        effect.program=original.clone();Arc::make_mut(&mut effect.program).pages=vec![original.pages[0].clone();2].into();assert!(effect.validate().is_err());
        effect.program=original;Arc::make_mut(&mut effect.program).pages=(0..17).map(|n|EffectPage{id:n.to_string().into(),label:"Page".into()}).collect::<Vec<_>>().into();assert!(effect.validate().is_err());
    }
    #[test] fn visibility_is_only_validated_toggle_or_choice_equality() {
        let mut effect=schema();
        let set=|effect:&mut EffectInstance,key:&str,value|Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[2].visible_when=Some(EffectVisibility{key:key.into(),value});
        set(&mut effect,"channel",EffectValue::Choice(1));effect.validate().unwrap();
        set(&mut effect,"channel",EffectValue::Choice(2));assert!(effect.validate().is_err());
        set(&mut effect,"gain",EffectValue::Number(1.));assert!(effect.validate().is_err());
        set(&mut effect,"missing",EffectValue::Toggle(true));assert!(effect.validate().is_err());
    }
    #[test] fn numeric_presentation_stays_finite_and_inside_hard_bounds() {
        let mut effect=schema();
        for bounds in [[f64::NAN,1.],[-0.1,1.],[0.1,2.1],[1.,0.1]] {
            Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[2].soft_bounds=Some(bounds);assert!(effect.validate().is_err());
        }
        Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[2].soft_bounds=Some([0.,2.]);
        for exponent in [f64::NAN,0.124,8.001] {
            Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[2].mapping=NumericMapping::Power{exponent};assert!(effect.validate().is_err());
        }
    }
}

pub fn log_curve_encode(value:f64, stops:f64)->f64 {
    let span=stops+8.;let toe=(-8f64).exp2()*std::f64::consts::E;
    if value<=toe {value/(toe*std::f64::consts::LN_2*span)}else{(value.log2()+8.)/span}
}
pub fn log_curve_decode(x:f64, stops:f64)->f64 {
    let span=stops+8.;let toe=(-8f64).exp2()*std::f64::consts::E;let knee=1./(std::f64::consts::LN_2*span);
    if x<=knee {x*toe*std::f64::consts::LN_2*span}else{(x*span-8.).exp2()}
}
