//! Pure effect descriptions and parameters. No graphics API or UI widget types.
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const EFFECT_ABI: u32 = 1;
pub const EFFECT_LUT_SAMPLES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Adjustment,
    Generator,
}

/// ABI 1 is pointwise. Spatial/temporal graphs require a new capability contract;
/// never pretend a tile-local source can service arbitrary neighbor sampling.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProgram {
    pub abi: u32,
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub kind: EffectKind,
    /// Ordinary WGSL library with a uniquely named function matching the ABI.
    pub wgsl: Arc<str>,
    pub entry: Arc<str>,
    pub parameters: Arc<[EffectParameter]>,
    #[serde(default)]
    pub constraints: Arc<[EffectConstraint]>,
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
        let mut data = Vec::new();
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
        if data.is_empty() {
            data.push([0.; 4]);
        }
        data
    }
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
impl BuiltinEffect {
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
            wgsl: include_str!("effects.wgsl").into(),
            entry: format!("capy_{}", self.id()).into(),
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
            vec![[0., 0., 0., 0.]; 9]
                .into_iter()
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
