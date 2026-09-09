//! Platform-neutral number-field policy. Native widgets own focus, selection,
//! pointer capture and unfinished text, never numeric math or expression parsing.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericKind {
    Number,
    Slider,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumericMapping {
    Linear,
    Log,
    /// The control travels uniformly through value^exponent. Exponent 2 on
    /// brush diameter gives equal increments of brush area; 0.5 gives a
    /// quadratic diameter response. Signed powers also support negative ranges.
    Power {
        exponent: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumericControl {
    pub kind: NumericKind,
    pub mapping: NumericMapping,
    /// Hard bounds for typed input. Sliders use the soft bounds.
    pub min: f64,
    pub max: f64,
    pub soft_min: f64,
    pub soft_max: f64,
    pub step: f64,
    /// Smallest stored increment, separate from plus/minus stepping.
    pub resolution: f64,
    pub digits: u32,
    /// Displayed value = stored value * scale. E.g. opacity uses 100 and "%".
    pub scale: f64,
    pub unit: String,
    /// Settings may reset an empty committed expression; ordinary fields may
    /// not. The default is supplied by the settings schema, not by the host.
    #[serde(default)]
    pub default_value: Option<f64>,
}
impl NumericControl {
    pub fn number(min: f64, max: f64, step: f64, digits: u32) -> Self {
        Self {
            kind: if digits == 0 && (max - min) / step <= 64.0 {
                NumericKind::Number
            } else {
                NumericKind::Slider
            },
            mapping: NumericMapping::Linear,
            min,
            max,
            soft_min: min,
            soft_max: max,
            step,
            resolution: 10f64.powi(-(digits as i32)),
            digits,
            scale: 1.0,
            unit: String::new(),
            default_value: None,
        }
    }
    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = unit.into();
        self
    }
    pub fn percent() -> Self {
        Self {
            kind: NumericKind::Slider,
            scale: 100.0,
            unit: "%".into(),
            resolution: 0.001,
            digits: 1,
            ..Self::number(0.0, 1.0, 0.01, 2)
        }
    }
    pub fn brush_size() -> Self {
        Self {
            mapping: NumericMapping::Log,
            ..Self::number(0.5, 2048.0, 1.0, 1).unit("px")
        }
    }
    pub fn pressure() -> Self {
        Self::number(0.25, 4.0, 0.05, 2).unit("×")
    }
    pub fn validate(&self, value: f32, label: &str) -> Result<(), String> {
        if !value.is_finite() || !(self.min..=self.max).contains(&f64::from(value)) {
            Err(format!(
                "{label} must be between {} and {}",
                self.min, self.max
            ))
        } else {
            Ok(())
        }
    }
    fn check(&self) -> Result<(), String> {
        if ![
            self.min,
            self.max,
            self.soft_min,
            self.soft_max,
            self.step,
            self.resolution,
            self.scale,
        ]
        .into_iter()
        .all(f64::is_finite)
            || self.min >= self.max
            || self.soft_min < self.min
            || self.soft_max > self.max
            || self.soft_min >= self.soft_max
            || self.step <= 0.0
            || self.resolution <= 0.0
            || self.scale <= 0.0
            || self.digits > 9
            || self.unit.len() > 16
        {
            return Err("Invalid number-field definition".into());
        }
        match self.mapping {
            NumericMapping::Log if self.soft_min <= 0.0 => {
                return Err("Logarithmic bounds must be positive".into());
            }
            NumericMapping::Power { exponent }
                if !exponent.is_finite() || !(0.125..=8.0).contains(&exponent) =>
            {
                return Err("Invalid range exponent".into());
            }
            _ => (),
        }
        if !self.mapped(self.soft_min).is_finite() || !self.mapped(self.soft_max).is_finite() {
            return Err("Invalid mapped range".into());
        }
        Ok(())
    }
    fn mapped(&self, value: f64) -> f64 {
        match self.mapping {
            NumericMapping::Linear => value,
            NumericMapping::Log => value.ln(),
            NumericMapping::Power { exponent } => value.signum() * value.abs().powf(exponent),
        }
    }
    fn unmapped(&self, value: f64) -> f64 {
        match self.mapping {
            NumericMapping::Linear => value,
            NumericMapping::Log => value.exp(),
            NumericMapping::Power { exponent } => value.signum() * value.abs().powf(1.0 / exponent),
        }
    }
    pub fn resolve(&self, value: f64, operation: NumericOperation) -> Result<NumericValue, String> {
        self.check()?;
        if !value.is_finite() {
            return Err("Enter a finite number".into());
        }
        let formatting = matches!(operation, NumericOperation::Format);
        let mut resolved = match operation {
            NumericOperation::Format => value,
            NumericOperation::Step { steps } => {
                if !steps.is_finite() {
                    return Err("Invalid numeric step".into());
                }
                value + steps * self.step
            }
            NumericOperation::Value { value } => value,
            NumericOperation::Position { position } => {
                if !position.is_finite() {
                    return Err("Invalid slider position".into());
                }
                self.unmapped(
                    self.mapped(self.soft_min)
                        + position.clamp(0.0, 1.0)
                            * (self.mapped(self.soft_max) - self.mapped(self.soft_min)),
                )
            }
            NumericOperation::Expression { text } if text.trim().is_empty() => {
                self.default_value
                    .ok_or("Enter a mathematical expression")?
            }
            NumericOperation::Expression { text } => self.expression(&text)? / self.scale,
        };
        if !resolved.is_finite() {
            return Err("Enter a finite number".into());
        }
        // Formatting an externally supplied value must never change it.
        if !formatting {
            resolved =
                ((resolved / self.resolution).round() * self.resolution).clamp(self.min, self.max);
        }
        let shown = if resolved == 0.0 {
            0.0
        } else {
            resolved * self.scale
        };
        let edit = format!("{:.*}", self.digits as usize, shown);
        let text = if self.unit.is_empty() {
            edit.clone()
        } else {
            format!("{edit} {}", self.unit)
        };
        Ok(NumericValue {
            value: resolved,
            text,
            edit,
            fill: ((self.mapped(resolved.clamp(self.soft_min, self.soft_max))
                - self.mapped(self.soft_min))
                / (self.mapped(self.soft_max) - self.mapped(self.soft_min)))
            .clamp(0.0, 1.0),
        })
    }
    fn expression(&self, source: &str) -> Result<f64, String> {
        if source.len() > 256 {
            return Err("Expression is too long".into());
        }
        let text = source.trim();
        let text = if self.unit.is_empty() {
            text
        } else {
            text.strip_suffix(&self.unit).unwrap_or(text).trim_end()
        };
        // fasteval has a diagnostic print() builtin. Number fields expose only
        // math, not string literals or diagnostic output.
        if text.contains(['"', '\''])
            || text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|s| s == "print")
        {
            return Err("Enter a mathematical expression".into());
        }
        let mut namespace = |name: &str, args: Vec<f64>| match (name, args.as_slice()) {
            ("pi", []) => Some(std::f64::consts::PI),
            ("e", []) => Some(std::f64::consts::E),
            ("tau", []) => Some(std::f64::consts::TAU),
            ("sqrt", [value]) => Some(value.sqrt()),
            _ => None,
        };
        fasteval::ez_eval(text, &mut namespace).map_err(|_| "Invalid expression".into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumericOperation {
    Format,
    /// Native spin buttons report a value; native sliders report a normalized
    /// position. Both use the same rounding, bounds and mapping as typed input.
    Value {
        value: f64,
    },
    Position {
        position: f64,
    },
    Step {
        steps: f64,
    },
    Expression {
        text: String,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct NumericValue {
    pub value: f64,
    pub text: String,
    pub edit: String,
    pub fill: f64,
}
#[derive(Deserialize)]
pub struct NumericRequest {
    pub control: NumericControl,
    pub value: f64,
    pub operation: NumericOperation,
}
impl NumericRequest {
    pub fn resolve(self) -> Result<NumericValue, String> {
        self.control.resolve(self.value, self.operation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_settings_accept_empty_expressions_as_reset() {
        let mut control = NumericControl::percent();
        let empty = || NumericOperation::Expression { text: " ".into() };
        assert!(control.resolve(0.5, empty()).is_err());
        control.default_value = Some(1.0);
        let result = control.resolve(0.5, empty()).unwrap();
        assert_eq!(result.value, 1.0);
        assert_eq!(result.text, "100.0 %");
        assert!(
            control
                .resolve(
                    0.5,
                    NumericOperation::Expression {
                        text: "nonsense".into()
                    }
                )
                .is_err()
        );
    }
    fn expr(spec: &NumericControl, text: &str) -> Result<NumericValue, String> {
        spec.resolve(1.0, NumericOperation::Expression { text: text.into() })
    }
    #[test]
    fn expressions_units_limits_and_untrusted_input() {
        let spec = NumericControl::number(-1000.0, 1000.0, 1.0, 3).unit("px");
        for (text, expected) in [
            ("3*2", 6.0),
            ("10/5+4", 6.0),
            ("sqrt(2)", 1.414),
            ("pi", (std::f64::consts::PI * 1000.0).round() / 1000.0),
            ("2^3 px", 8.0),
            ("2000", 1000.0),
        ] {
            assert!(
                (expr(&spec, text).unwrap().value - expected).abs() < 1e-9,
                "{text}"
            );
        }
        for text in [
            "sqrt(-1)",
            "1/0",
            "1e999",
            "unknown(2)",
            "print(3)",
            "\"hello\"",
            "2+",
            "",
        ] {
            assert!(expr(&spec, text).is_err(), "{text}");
        }
        assert!(expr(&spec, &"1+".repeat(200)).is_err());
        let percent = expr(&NumericControl::percent(), "25+25%").unwrap();
        assert_eq!(percent.value, 0.5);
        assert_eq!(percent.text, "50.0 %");
    }
    #[test]
    fn slider_and_typed_limits_and_steps() {
        let spec = NumericControl {
            soft_max: 512.0,
            ..NumericControl::brush_size()
        };
        assert_eq!(expr(&spec, "1024").unwrap().value, 1024.0);
        assert_eq!(
            spec.resolve(500.0, NumericOperation::Position { position: 1.0 })
                .unwrap()
                .value,
            512.0
        );
        assert_eq!(
            spec.resolve(1024.0, NumericOperation::Format).unwrap().fill,
            1.0
        );
        assert_eq!(
            spec.resolve(10.0, NumericOperation::Step { steps: 1.0 })
                .unwrap()
                .value,
            11.0
        );
    }
    #[test]
    fn nonlinear_mapping_uses_the_same_curve_for_position_and_fill() {
        for mapping in [
            NumericMapping::Log,
            NumericMapping::Power { exponent: 2.0 },
            NumericMapping::Power { exponent: 0.5 },
        ] {
            let spec = NumericControl {
                mapping,
                kind: NumericKind::Slider,
                ..NumericControl::number(1.0, 100.0, 1.0, 6)
            };
            let result = spec
                .resolve(1.0, NumericOperation::Position { position: 0.5 })
                .unwrap();
            assert!((result.fill - 0.5).abs() < 1e-6);
            let expected = match mapping {
                NumericMapping::Log => 10.0,
                NumericMapping::Power { exponent: 2.0 } => 5000.5f64.sqrt(),
                _ => 30.25,
            };
            assert!((result.value - expected).abs() < 1e-6);
            assert_eq!(
                spec.resolve(10.0, NumericOperation::Step { steps: 1.0 })
                    .unwrap()
                    .value,
                11.0
            );
        }
    }
    #[test]
    fn touch_control_selection_and_slider_quantization() {
        assert_eq!(
            NumericControl::number(0.0, 64.0, 1.0, 0).kind,
            NumericKind::Number
        );
        for control in [
            NumericControl::brush_size(),
            NumericControl::percent(),
            NumericControl::pressure(),
            NumericControl::number(0.0, 256.0, 1.0, 0),
        ] {
            assert_eq!(control.kind, NumericKind::Slider);
            for position in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let result = control
                    .resolve(control.min, NumericOperation::Position { position })
                    .unwrap();
                assert!((control.min..=control.max).contains(&result.value));
                assert!((result.fill - position).abs() < 0.01);
            }
            assert!(
                control
                    .resolve(1.0, NumericOperation::Position { position: f64::NAN })
                    .is_err()
            );
        }
    }
}
