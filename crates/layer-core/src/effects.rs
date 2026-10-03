//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use crate::color::{RgbColor, RgbSpace};
use crate::authored::Resource;
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

pub const EFFECT_ABI: u32 = 4;
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
    /// documents. Only filters with passes declare it; pointwise filters
    /// convert per pixel themselves.
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
/// position and a parameter offset. Empty `passes` means pointwise and permits
/// shader fusion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProgram {
    pub abi: u32,
    pub id: Arc<str>,
    pub label: ResourceLabel,
    pub kind: EffectKind,
    #[serde(default)]
    pub alpha: EffectAlpha,
    #[serde(default, skip_serializing_if = "EffectSpace::is_linear")]
    pub space: EffectSpace,
    #[serde(default)]
    pub resolution: EffectResolution,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectAuxiliary {
    Lut3d { resource: Arc<str>, color_space: Arc<str> },
    Analysis { analysis: EffectAnalysisKind },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectAnalysisKind { LocalIllumination }

impl EffectProgram {
    pub fn analysis(&self) -> Option<EffectAnalysisKind> {
        match self.auxiliary { Some(EffectAuxiliary::Analysis {analysis}) => Some(analysis), _ => None }
    }
    pub fn image_boundary(&self) -> bool {
        !self.passes.is_empty()
            || self.time
            || (self.kind == EffectKind::Adjustment && self.alpha == EffectAlpha::Filter)
    }
    pub fn fusion_boundary(&self) -> bool { self.image_boundary() || self.auxiliary.is_some() }
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
    Gradient(Vec<GradientStop>),
    Lut3d(Option<Arc<crate::Lut3d>>),
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
    pub fn at(effect: &EffectInstance, elapsed: f32, phase: f32) -> Self {
        Self { previous: Some((elapsed, effect.playback_rate(), effect.animated())), phase }
    }
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
    pub fn lut3d(&self) -> Option<&Arc<crate::Lut3d>> {
        let EffectAuxiliary::Lut3d { resource, .. } = self.program.auxiliary.as_ref()? else { return None; };
        match self.value(resource) { Some(EffectValue::Lut3d(resource)) => resource.as_ref(), _ => None }
    }
    pub fn resources(&self) -> impl Iterator<Item = &Arc<crate::Lut3d>> {
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
            EffectParameterKind::Choice {options} if options.iter().map(EffectOption::value).eq(RgbSpace::ALL.map(RgbSpace::name))) {
            return Err("Invalid color lookup resource or color spaces");
        }
        Ok(Some((parameter,space)))
    }
    fn validate_resource<'a>(&self, value: impl Fn(usize) -> &'a EffectValue) -> Result<(), &'static str> {
        let Some((resource,space)) = self.auxiliary_indices()? else { return Ok(()); };
        let (EffectValue::Lut3d(resource),EffectValue::Choice(space)) = (value(resource),value(space)) else {
            return Err("Invalid color lookup resource values");
        };
        let space = RgbSpace::ALL.get(*space as usize).ok_or("Invalid color lookup color space")?;
        if resource.as_ref().is_some_and(|r| !r.accepts(*space)) { return Err("Color lookup exceeds the selected color space range"); }
        Ok(())
    }
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
                options.get(*v as usize).map(EffectOption::value)
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
    /// Runtime schema replacement preserves parameters by key and choices by
    /// stable value. New or individually incompatible fields use their defaults.
    /// Conflicting joint constraints reject publication instead of silently
    /// changing otherwise valid user values.
    pub fn rebind(&self, program: Arc<EffectProgram>) -> Result<Self, &'static str> {
        let mut next = Self::new(program);
        for (parameter, value) in next.program.parameters.iter().zip(&mut next.values) {
            if let EffectParameterKind::Choice { options } = &parameter.kind {
                if let Some(index) = self.choice(&parameter.key)
                    .and_then(|selected| options.iter().position(|option| option.value() == selected))
                {
                    *value = EffectValue::Choice(index as u32);
                }
            } else if let Some(old) = self.value(&parameter.key)
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
        if self.program.space == EffectSpace::Blending && self.program.passes.is_empty() {
            return Err("Only filters with passes follow the document's Blending");
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
        self.validate_resource(|index| if index == i { &value } else { &self.values[index] })?;
        self.values[i] = value;
        Ok(())
    }
    /// Distances declared in pixels scaled with the image, rounded to their
    /// decimals and clamped to their range; None when nothing changes or the
    /// scaled values would break the effect's constraints.
    pub fn scaled_px(&self, factor: f32) -> Option<Self> {
        let mut scaled = self.clone();
        for (parameter, value) in self.program.parameters.iter().zip(&mut scaled.values) {
            if let (EffectParameterKind::Number { min, max, decimals, unit, .. }, EffectValue::Number(v)) = (&parameter.kind, value)
                && &**unit == "px"
            {
                let places = 10f32.powi(i32::from(*decimals));
                *v = ((*v * factor * places).round() / places).clamp(*min, *max);
            }
        }
        (scaled.values != self.values && scaled.validate().is_ok()).then_some(scaled)
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

impl EffectParameter {
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
        let EffectValue::Number(white)=fx.values[1] else {panic!()};
        assert!(f64::from(white)-f64::from(0.8f32)>=f64::from(0.001f32));
        assert!(f64::from(white.next_down())-f64::from(0.8f32)<f64::from(0.001f32));
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
        black.soft_bounds=Some([0.,0.2]);
        let rebound = instance.rebind(Arc::new(program)).unwrap();
        assert_eq!(rebound.value("black"), Some(&EffectValue::Number(0.)));
        assert_eq!(rebound.value("white"), instance.value("white"));
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
    fn choice_rebinding_preserves_stable_values_and_defaults_when_values_are_removed() {
        let mut instance = EffectInstance::new(fixture("curves").program());
        instance.set("domain", EffectValue::Choice(1)).unwrap();
        let mut program = (*instance.program).clone();
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|p| p.key.as_ref() == "domain").unwrap();
        let option = |value: &str| EffectOption::Labeled { value: value.into(), label: "同じ表示名 🎨".into() };
        parameter.kind = EffectParameterKind::Choice { options: [option("Log HDR"), option("Encoded RGB")].into() };
        parameter.default = EffectValue::Choice(1);
        let rebound = instance.rebind(Arc::new(program.clone())).unwrap();
        assert_eq!(rebound.value("domain"), Some(&EffectValue::Choice(0)));
        assert_eq!(rebound.choice("domain"), Some("Log HDR"));
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|p| p.key.as_ref() == "domain").unwrap();
        parameter.kind = EffectParameterKind::Choice { options: [option("Encoded RGB"), option("Linear HDR")].into() };
        parameter.default = EffectValue::Choice(0);
        let rebound = instance.rebind(Arc::new(program.clone())).unwrap();
        assert_eq!(rebound.value("domain"), Some(&EffectValue::Choice(0)));
        assert_eq!(rebound.choice("domain"), Some("Encoded RGB"));
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|p| p.key.as_ref() == "domain").unwrap();
        parameter.default = EffectValue::Choice(1);
        let rebound = instance.rebind(Arc::new(program.clone())).unwrap();
        assert_eq!(rebound.value("domain"), Some(&EffectValue::Choice(1)));
        assert_eq!(rebound.choice("domain"), Some("Linear HDR"));
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|p| p.key.as_ref() == "domain").unwrap();
        parameter.kind = EffectParameterKind::Choice { options: [option("Encoded RGB"), option("Encoded RGB")].into() };
        assert!(instance.rebind(Arc::new(program)).is_err());
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

impl EffectProgram {
    pub fn for_depth(self:&Arc<Self>,depth:crate::color::SampleDepth)->Arc<Self> {
        let selected=match self.id.as_ref() {
            "levels" if depth.is_float()=>None,
            "threshold" if depth.is_float()=>Some(("threshold",-65504.,65504.)),
            "curves" if depth==crate::color::SampleDepth::F32=>Some(("hdr_stops",0.,127.)),
            "exposure" if depth==crate::color::SampleDepth::F32=>Some(("exposure",-126.,126.)),
            _=>return self.clone(),
        };
        let Some(bundled)=crate::bundled_effect_catalog().get(&self.id) else {return self.clone();};
        if self.wgsl!=bundled.program().wgsl || self.entry!=bundled.program().entry {return self.clone();}
        let mut result=self.clone();
        for parameter in Arc::make_mut(&mut Arc::make_mut(&mut result).parameters) {
            if let EffectParameterKind::Number {min,max,..}=&mut parameter.kind {
                if let Some((key,lower,upper))=selected {
                    if parameter.key.as_ref()==key {*min=lower;*max=upper;}
                } else if !parameter.key.ends_with("gamma") {*min= -65504.;*max=65504.;}
            }
        }
        result
    }
}
