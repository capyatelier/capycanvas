//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const EFFECT_ABI: u32 = 2;
pub const EFFECT_LUT_SAMPLES: usize = 256;

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
    pub wgsl: Arc<str>,
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
    Document,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectLookup {
    /// Normalized discrete Gaussian, packed into bilinear positive tap pairs.
    /// Sigma is constrained to 0..21 px (at most 63 px / 32 pairs per side).
    Gaussian { sigma: Arc<str> },
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
    pub fn damage_radius(&self) -> Option<u32> {
        self.passes.iter().try_fold(0u32, |r, p| match p.sampling {
            EffectSampling::Neighborhood { radius } => r.checked_add(radius),
            EffectSampling::Document => None,
        })
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
        if self.program.abi != EFFECT_ABI
            || self.program.passes.len() > 8
            || self.program.parameters.len() > 64
            || self.values.len() != self.program.parameters.len()
            || self.program.entry.is_empty()
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
                || !pass.entry.bytes().enumerate().all(|(i, c)| {
                    c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
                || matches!(pass.sampling, EffectSampling::Neighborhood { radius } if radius > 4096)
            {
                return Err("Invalid image pass");
            }
        }
        if self.program.lookups.len() > 8 {
            return Err("Too many effect lookup tables");
        }
        for lookup in self.program.lookups.iter() {
            let EffectLookup::Gaussian { sigma } = lookup;
            if !self.program.parameters.iter().any(|p| p.key == *sigma && matches!(p.kind, EffectParameterKind::Number { min, max, .. } if min >= 0. && max <= 21.)) {
                return Err("Gaussian sigma must be a numeric parameter within 0..21 px");
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
            let EffectLookup::Gaussian { sigma } = lookup;
            let sigma = match self.value(sigma) {
                Some(EffectValue::Number(v)) => *v,
                _ => 0.,
            };
            data[directory + i] = [data.len() as f32, 33., 0., 0.];
            data.extend(gaussian_taps(sigma));
        }
        data
    }
}

fn gaussian_taps(sigma: f32) -> [[f32; 4]; 33] {
    let mut taps = [[0.; 4]; 33];
    if sigma <= 0. {
        taps[0][0] = 1.;
        return taps;
    }
    let radius = (sigma * 3.).ceil().min(63.) as usize;
    let mut weights = [0.; 65];
    for (i, w) in weights.iter_mut().enumerate().take(radius + 1) {
        *w = (-0.5 * (i as f32 / sigma).powi(2)).exp();
    }
    let total = weights[0] + 2. * weights[1..=radius].iter().sum::<f32>();
    taps[0] = [weights[0] / total, 0., 0., 0.];
    for (pair, i) in (1..=radius).step_by(2).enumerate() {
        let weight = weights[i] + weights[i + 1];
        if weight <= 1e-20 {
            break;
        }
        taps[pair + 1] = [i as f32 + weights[i + 1] / weight, weight / total, 0., 0.];
        taps[0][1] += 1.;
    }
    taps
}
impl EffectParameter {
    fn in_section(mut self, section: &str) -> Self {
        self.section = Some(section.into());
        self
    }
    pub fn validate(&self, value: &EffectValue) -> Result<(), &'static str> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinEffect {
    Curves,
    Levels,
    BrightnessContrast,
    HueSaturation,
    ColorBalance,
    Exposure,
    Vibrance,
    BlackWhite,
    GradientMap,
    Posterize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterCategory {
    Tone,
    Color,
    Artistic,
}
impl FilterCategory {
    pub const ALL: [Self; 3] = [Self::Tone, Self::Color, Self::Artistic];
    pub fn label(self) -> &'static str {
        match self {
            Self::Tone => "Tone",
            Self::Color => "Color",
            Self::Artistic => "Artistic",
        }
    }
}
impl BuiltinEffect {
    pub fn category(self) -> FilterCategory {
        match self {
            Self::Curves | Self::Levels | Self::BrightnessContrast | Self::Exposure => {
                FilterCategory::Tone
            }
            Self::HueSaturation | Self::ColorBalance | Self::Vibrance => FilterCategory::Color,
            Self::BlackWhite | Self::GradientMap | Self::Posterize => FilterCategory::Artistic,
        }
    }
    pub const ALL: [Self; 10] = [
        Self::Curves,
        Self::Levels,
        Self::BrightnessContrast,
        Self::HueSaturation,
        Self::ColorBalance,
        Self::Exposure,
        Self::Vibrance,
        Self::BlackWhite,
        Self::GradientMap,
        Self::Posterize,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Curves => "curves",
            Self::Levels => "levels",
            Self::BrightnessContrast => "brightness_contrast",
            Self::HueSaturation => "hue_saturation",
            Self::ColorBalance => "color_balance",
            Self::Exposure => "exposure",
            Self::Vibrance => "vibrance",
            Self::BlackWhite => "black_white",
            Self::GradientMap => "gradient_map",
            Self::Posterize => "posterize",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Curves => "Curves",
            Self::Levels => "Levels",
            Self::BrightnessContrast => "Brightness / Contrast",
            Self::HueSaturation => "Hue / Saturation",
            Self::ColorBalance => "Color Balance",
            Self::Exposure => "Exposure",
            Self::Vibrance => "Vibrance",
            Self::BlackWhite => "Black & White",
            Self::GradientMap => "Gradient Map",
            Self::Posterize => "Posterize",
        }
    }
    pub fn program(self) -> Arc<EffectProgram> {
        static PROGRAMS: std::sync::OnceLock<Vec<Arc<EffectProgram>>> = std::sync::OnceLock::new();
        PROGRAMS.get_or_init(|| Self::ALL.into_iter().map(Self::build_program).collect())
            [self as usize]
            .clone()
    }
    /// Illustrative presets are separate from the defaults used on insertion.
    pub fn preview(self) -> EffectInstance {
        let mut effect = EffectInstance::new(self.program());
        let values: Vec<(&str, EffectValue)> = match self {
            Self::Curves => vec![(
                "curve_0",
                EffectValue::Curve(vec![[0., 0.], [0.25, 0.15], [0.75, 0.85], [1., 1.]]),
            )],
            Self::Levels => vec![
                ("black", EffectValue::Number(0.1)),
                ("white", EffectValue::Number(0.9)),
            ],
            Self::BrightnessContrast => vec![("contrast", EffectValue::Number(25.))],
            Self::HueSaturation => vec![
                ("hue", EffectValue::Number(35.)),
                ("saturation", EffectValue::Number(18.)),
            ],
            Self::ColorBalance => vec![
                ("shadows_blue", EffectValue::Number(20.)),
                ("highlights_red", EffectValue::Number(20.)),
            ],
            Self::Exposure => vec![("exposure", EffectValue::Number(0.8))],
            Self::Vibrance => vec![("vibrance", EffectValue::Number(45.))],
            _ => Vec::new(),
        };
        for (key, value) in values {
            effect
                .set(key, value)
                .expect("valid built-in preview preset");
        }
        if effect.program.time {
            effect.set("animate", EffectValue::Toggle(false)).unwrap();
            effect.set("time", EffectValue::Number(1.25)).unwrap();
        }
        effect
    }
    fn build_program(self) -> Arc<EffectProgram> {
        use EffectParameterKind as K;
        let number = |key: &str, label: &str, min, max, default, step, decimals, unit: &str| {
            EffectParameter {
                key: key.into(),
                label: label.into(),
                section: None,
                kind: K::Number {
                    min,
                    max,
                    step,
                    decimals,
                    unit: unit.into(),
                },
                default: EffectValue::Number(default),
            }
        };
        let parameters = match self {
            Self::Exposure => vec![
                number("exposure", "Exposure", -10., 10., 0., 0.1, 2, "EV"),
                number("offset", "Offset", -0.5, 0.5, 0., 0.01, 3, ""),
                number("gamma", "Gamma", 0.1, 10., 1., 0.05, 2, ""),
            ],
            Self::Vibrance => vec![
                number("vibrance", "Vibrance", -100., 100., 0., 1., 0, "%"),
                number("saturation", "Saturation", -100., 100., 0., 1., 0, "%"),
                EffectParameter {
                    key: "protect_skin".into(),
                    label: "Protect skin tones".into(),
                    section: None,
                    kind: K::Toggle,
                    default: EffectValue::Toggle(true),
                },
            ],
            Self::BlackWhite => {
                let mut p: Vec<_> = [
                    ("reds", "Reds", 40.),
                    ("yellows", "Yellows", 60.),
                    ("greens", "Greens", 40.),
                    ("cyans", "Cyans", 60.),
                    ("blues", "Blues", 20.),
                    ("magentas", "Magentas", 80.),
                ]
                .into_iter()
                .map(|(key, label, v)| number(key, label, -100., 200., v, 1., 0, "%"))
                .collect();
                p.push(EffectParameter {
                    key: "tint".into(),
                    label: "Tint".into(),
                    section: None,
                    kind: K::Toggle,
                    default: EffectValue::Toggle(false),
                });
                p.push(EffectParameter {
                    key: "tint_color".into(),
                    label: "Tint color".into(),
                    section: None,
                    kind: K::Color,
                    default: EffectValue::Color([0.75, 0.53, 0.3, 1.]),
                });
                p
            }
            Self::GradientMap => vec![
                EffectParameter {
                    key: "gradient".into(),
                    label: "Gradient".into(),
                    section: None,
                    kind: K::Gradient,
                    default: EffectValue::Gradient(vec![
                        GradientStop {
                            position: 0.,
                            color: [0., 0., 0., 1.],
                        },
                        GradientStop {
                            position: 1.,
                            color: [1.; 4],
                        },
                    ]),
                },
                EffectParameter {
                    key: "reverse".into(),
                    label: "Reverse".into(),
                    section: None,
                    kind: K::Toggle,
                    default: EffectValue::Toggle(false),
                },
                number("amount", "Amount", 0., 100., 100., 1., 0, "%"),
            ],
            Self::Posterize => vec![number("levels", "Levels", 2., 256., 6., 1., 0, "")],
            Self::Curves => ["RGB", "Red", "Green", "Blue"]
                .into_iter()
                .enumerate()
                .map(|(i, label)| EffectParameter {
                    key: format!("curve_{i}").into(),
                    label: label.into(),
                    section: None,
                    kind: K::Curve,
                    default: EffectValue::Curve(vec![[0., 0.], [1., 1.]]),
                })
                .collect(),
            Self::Levels => vec![
                number("black", "Black", 0., 0.999, 0., 0.01, 3, "").in_section("Input"),
                number("white", "White", 0.001, 1., 1., 0.01, 3, "").in_section("Input"),
                number("gamma", "Midtones", 0.1, 10., 1., 0.05, 2, "").in_section("Input"),
                number("output_black", "Black", 0., 1., 0., 0.01, 3, "").in_section("Output"),
                number("output_white", "White", 0., 1., 1., 0.01, 3, "").in_section("Output"),
            ],
            Self::BrightnessContrast => vec![
                number("brightness", "Brightness", -100., 100., 0., 1., 0, ""),
                number("contrast", "Contrast", -100., 100., 0., 1., 0, ""),
            ],
            Self::HueSaturation => vec![
                number("hue", "Hue", -180., 180., 0., 1., 0, "°"),
                number("saturation", "Saturation", -100., 100., 0., 1., 0, "%"),
                number("lightness", "Lightness", -100., 100., 0., 1., 0, "%"),
            ],
            Self::ColorBalance => {
                let mut p = Vec::new();
                for (range, label) in [
                    ("shadows", "Shadows"),
                    ("midtones", "Midtones"),
                    ("highlights", "Highlights"),
                ] {
                    for (axis, name) in [
                        ("red", "Cyan — Red"),
                        ("green", "Magenta — Green"),
                        ("blue", "Yellow — Blue"),
                    ] {
                        p.push(
                            number(&format!("{range}_{axis}"), name, -100., 100., 0., 1., 0, "")
                                .in_section(label),
                        );
                    }
                }
                p.push(EffectParameter {
                    key: "preserve_luminance".into(),
                    label: "Preserve luminosity".into(),
                    section: None,
                    kind: K::Toggle,
                    default: EffectValue::Toggle(true),
                });
                p
            }
        };
        Arc::new(EffectProgram {
            abi: EFFECT_ABI,
            id: self.id().into(),
            label: self.label().into(),
            kind: EffectKind::Adjustment,
            alpha: EffectAlpha::Preserve,
            wgsl: include_str!("effects.wgsl").into(),
            entry: format!("capy_{}", self.id()).into(),
            passes: Arc::from([]),
            time: false,
            lookups: Arc::from([]),
            parameters: parameters.into(),
            constraints: if self == Self::Levels {
                Arc::from([EffectConstraint::OrderedNumbers {
                    lower: "black".into(),
                    upper: "white".into(),
                    gap: 0.001,
                }])
            } else {
                Arc::from([])
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_presets_are_valid_and_do_not_change_insertion_defaults() {
        for id in BuiltinEffect::ALL {
            let original = EffectInstance::new(id.program());
            let preview = id.preview();
            preview.validate().unwrap();
            assert_eq!(original, EffectInstance::new(id.program()));
            assert!(!preview.animated());
        }
    }
    #[test]
    fn gaussian_lookup_is_finite_normalized_and_fixed_size() {
        for sigma in [0., 0.000001, 0.1, 0.5, 1., 3., 12., 21.] {
            let taps = gaussian_taps(sigma);
            assert!(taps.iter().flatten().all(|v| v.is_finite()));
            let weight = taps[0][0] + 2. * taps[1..].iter().map(|v| v[1]).sum::<f32>();
            assert!((weight - 1.).abs() < 0.00001, "sigma={sigma}: {weight}");
            assert!(taps[0][1] <= 32.);
        }
    }
    #[test]
    fn defaults_and_parameter_validation() {
        for kind in BuiltinEffect::ALL {
            let fx = EffectInstance::new(kind.program());
            fx.validate().unwrap();
            assert!(!fx.gpu_parameters().is_empty());
        }
        let mut fx = EffectInstance::new(BuiltinEffect::Levels.program());
        assert!(fx.set("gamma", EffectValue::Number(f32::NAN)).is_err());
        assert!(fx.set("black", EffectValue::Toggle(true)).is_err());
        fx.set("black", EffectValue::Number(0.8)).unwrap();
        fx.set("white", EffectValue::Number(0.2)).unwrap();
        assert_eq!(fx.values[1], EffectValue::Number(0.801));
        fx.validate().unwrap();
    }
    #[test]
    fn sections_shorten_labels_without_changing_shader_parameters() {
        let program = BuiltinEffect::ColorBalance.program();
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
