//! Platform-neutral number-field policy. Native widgets own focus, selection,
//! pointer capture and unfinished text, never numeric math or expression parsing.
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::{Localizer, MessageId};
use layer_core::ResourceLabel;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum NumericError {
    InvalidDefinition,
    PositiveLogarithmicBounds,
    InvalidRangeExponent,
    InvalidMappedRange,
    FiniteNumber,
    InvalidStep,
    InvalidPosition,
    ExpressionRequired,
    ExpressionTooLong,
    InvalidExpression,
    InvalidNumber,
    Range { label: ResourceLabel, min: f64, max: f64 },
    WholePixels { label: ResourceLabel },
}
impl NumericError {
    pub fn valid(&self) -> bool {
        let label = match self {
            Self::Range { label, min, max } => {
                if !min.is_finite() || !max.is_finite() || min >= max { return false; }
                label
            }
            Self::WholePixels { label } => label,
            _ => return true,
        };
        match label {
            ResourceLabel::Literal(_) => true,
            ResourceLabel::Message { message } => MessageId::STATIC.iter().any(|id| id.key() == message.as_ref()),
        }
    }
    fn message_id(&self) -> MessageId {
        match self {
            Self::InvalidDefinition => MessageId::NUMERIC_INVALID_DEFINITION,
            Self::PositiveLogarithmicBounds => MessageId::NUMERIC_POSITIVE_LOGARITHMIC_BOUNDS,
            Self::InvalidRangeExponent => MessageId::NUMERIC_INVALID_RANGE_EXPONENT,
            Self::InvalidMappedRange => MessageId::NUMERIC_INVALID_MAPPED_RANGE,
            Self::FiniteNumber => MessageId::NUMERIC_FINITE_NUMBER,
            Self::InvalidStep => MessageId::NUMERIC_INVALID_STEP,
            Self::InvalidPosition => MessageId::NUMERIC_INVALID_POSITION,
            Self::ExpressionRequired => MessageId::NUMERIC_EXPRESSION_REQUIRED,
            Self::ExpressionTooLong => MessageId::NUMERIC_EXPRESSION_TOO_LONG,
            Self::InvalidExpression => MessageId::NUMERIC_INVALID_EXPRESSION,
            Self::InvalidNumber => MessageId::SETTINGS_EXPECTED_A_NUMBER,
            Self::Range { .. } => MessageId::NUMERIC_RANGE,
            Self::WholePixels { .. } => MessageId::NUMERIC_WHOLE_PIXELS,
        }
    }
    pub fn code(&self) -> &'static str { self.message_id().key() }
    pub fn message(&self, localization: &Localizer) -> String {
        if !self.valid() { return localization.text(MessageId::NUMERIC_INVALID_DEFINITION).to_string(); }
        match self {
            Self::Range { label, min, max } => {
                let label = numeric_label(label, localization);
                let mut args = crate::FluentArgs::new();
                args.set("label", label.as_ref()); args.set("min", *min); args.set("max", *max);
                localization.format(self.message_id(), &args)
            }
            Self::WholePixels { label } => {
                let label = numeric_label(label, localization);
                let mut args = crate::FluentArgs::new(); args.set("label", label.as_ref());
                localization.format(self.message_id(), &args)
            }
            _ => localization.text(self.message_id()).to_string(),
        }
    }
}

pub(crate) fn parse_numeric_text(source: &str) -> Result<f32, NumericError> {
    let text = bounded_numeric_text(source)?;
    let value: f32 = text.trim().parse().map_err(|_| NumericError::InvalidNumber)?;
    if !value.is_finite() { return Err(NumericError::FiniteNumber); }
    Ok(value)
}
fn bounded_numeric_text(source: &str) -> Result<&str, NumericError> {
    if source.len() > 256 { Err(NumericError::ExpressionTooLong) } else { Ok(source) }
}
fn numeric_label(label: &ResourceLabel, localization: &Localizer) -> Arc<str> {
    match label {
        ResourceLabel::Literal(text) => text.clone(),
        ResourceLabel::Message { message } => localization.text(*MessageId::STATIC.iter().find(|id| id.key() == message.as_ref()).expect("validated numeric label")),
    }
}
impl From<MessageId> for ResourceLabel {
    fn from(id: MessageId) -> Self { Self::Message { message: id.key().into() } }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceValidationError {
    Numeric(NumericError),
    ToolbarName(crate::ToolbarNameRefusal),
    Diagnostic(String),
}
impl WorkspaceValidationError {
    pub fn diagnostic(self) -> String {
        match self { Self::Numeric(reason) => reason.code().into(), Self::ToolbarName(reason) => reason.diagnostic(), Self::Diagnostic(message) => message }
    }
    pub fn message(&self, localization: &Localizer) -> String {
        match self {
            Self::Numeric(reason) => reason.message(localization),
            Self::ToolbarName(reason) => reason.message(localization).to_string(),
            Self::Diagnostic(message) => message.clone(),
        }
    }
}
impl From<NumericError> for WorkspaceValidationError {
    fn from(reason: NumericError) -> Self { Self::Numeric(reason) }
}
impl From<String> for WorkspaceValidationError {
    fn from(message: String) -> Self { Self::Diagnostic(message) }
}
impl From<&str> for WorkspaceValidationError {
    fn from(message: &str) -> Self { Self::Diagnostic(message.into()) }
}

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
    /// Above this value edits use whole units; formatting never mutates state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integer_above: Option<f64>,
    pub digits: u32,
    /// Displayed value = stored value * scale. E.g. opacity uses 100 and "%".
    pub scale: f64,
    pub unit: String,
    /// Settings may reset an empty committed expression; ordinary fields may
    /// not. The default is supplied by the settings schema, not by the host.
    #[serde(default)]
    pub default_value: Option<f64>,
    /// Optional words for exact slider endpoints. Editing still uses numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_labels: Option<[String; 2]>,
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
            integer_above: None,
            resolution: 10f64.powi(-(digits as i32)),
            digits,
            scale: 1.0,
            unit: String::new(),
            default_value: None,
            endpoint_labels: None,
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
            integer_above: Some(32.),
            ..Self::number(0.5, 2048.0, 1.0, 1).unit("px")
        }
    }
    /// Compact layer header: display 0–100 without a unit; keep stored precision.
    pub fn layer_opacity() -> Self {
        Self {
            digits: 0,
            unit: String::new(),
            ..Self::percent()
        }
    }
    /// View zoom in percent; the slider travels evenly through each doubling.
    pub fn zoom() -> Self {
        Self {
            kind: NumericKind::Slider,
            mapping: NumericMapping::Log,
            scale: 100.0,
            unit: "%".into(),
            resolution: 0.0001,
            ..Self::number(f64::from(crate::MIN_ZOOM), f64::from(crate::MAX_ZOOM), 0.1, 0)
        }
    }
    pub fn pressure() -> Self {
        Self::number(0.25, 4.0, 0.05, 2).unit("×")
    }
    pub fn validate(&self, value: f32, label: impl Into<ResourceLabel>) -> Result<(), NumericError> {
        // Actions and engine parameters are f32, including their endpoints.
        if !value.is_finite() || !(self.min as f32..=self.max as f32).contains(&value) {
            Err(NumericError::Range { label: label.into(), min: self.min, max: self.max })
        } else {
            Ok(())
        }
    }
    fn check(&self) -> Result<(), NumericError> {
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
            || self.integer_above.is_some_and(|v| !v.is_finite())
            || self.digits > 9
            || self.unit.len() > 16
        {
            return Err(NumericError::InvalidDefinition);
        }
        match self.mapping {
            NumericMapping::Log if self.soft_min <= 0.0 => {
                return Err(NumericError::PositiveLogarithmicBounds);
            }
            NumericMapping::Power { exponent }
                if !exponent.is_finite() || !(0.125..=8.0).contains(&exponent) =>
            {
                return Err(NumericError::InvalidRangeExponent);
            }
            _ => (),
        }
        if !self.mapped(self.soft_min).is_finite() || !self.mapped(self.soft_max).is_finite() {
            return Err(NumericError::InvalidMappedRange);
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
    pub fn resolve(&self, value: f64, operation: NumericOperation) -> Result<NumericValue, NumericError> {
        self.check()?;
        if !value.is_finite() {
            return Err(NumericError::FiniteNumber);
        }
        let formatting = matches!(operation, NumericOperation::Format);
        let mut resolved = match operation {
            NumericOperation::Format => value,
            NumericOperation::Step { steps } => {
                if !steps.is_finite() {
                    return Err(NumericError::InvalidStep);
                }
                value + steps * self.step
            }
            NumericOperation::Value { value } => value,
            NumericOperation::Position { position } => {
                if !position.is_finite() {
                    return Err(NumericError::InvalidPosition);
                }
                self.unmapped(
                    self.mapped(self.soft_min)
                        + position.clamp(0.0, 1.0)
                            * (self.mapped(self.soft_max) - self.mapped(self.soft_min)),
                )
            }
            NumericOperation::Expression { text } => {
                let text = bounded_numeric_text(&text)?;
                if text.trim().is_empty() { self.default_value.ok_or(NumericError::ExpressionRequired)? }
                else { self.expression(text)? / self.scale }
            }
        };
        if !resolved.is_finite() {
            return Err(NumericError::FiniteNumber);
        }
        // Formatting an externally supplied value must never change it.
        if !formatting {
            resolved =
                ((resolved / self.resolution).round() * self.resolution).clamp(self.min, self.max);
            if self.integer_above.is_some_and(|limit| resolved > limit) {
                resolved = resolved.round().clamp(self.min, self.max);
            }
        }
        let shown = if resolved == 0.0 {
            0.0
        } else {
            resolved * self.scale
        };
        let edit = format!("{:.*}", self.digits as usize, shown);
        let text = if let Some(labels) = &self.endpoint_labels
            && (resolved == self.min || resolved == self.max)
        {
            labels[usize::from(resolved == self.max)].clone()
        } else if self.unit.is_empty() {
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
    /// Short toolbar readout. Omit fractional digits at three digits and above;
    /// the editable expression and stored value retain their full precision.
    pub fn compact_text(&self, value: f64) -> String {
        let text = self.compact_value(value);
        if self.unit.is_empty() {
            text
        } else {
            format!("{text} {}", self.unit)
        }
    }
    /// Representative widest readouts, including signs, fractional values just
    /// below a digit/precision boundary, endpoint words, and units. Hosts measure
    /// these with their native font to reserve a stable input footprint.
    pub fn width_samples(&self, compact: bool) -> Vec<String> {
        let mut values = vec![self.min, self.max];
        if self.digits > 0 {
            values.extend([
                ((self.max * self.scale).ceil() - 0.1) / self.scale,
                ((self.min * self.scale).floor() + 0.1) / self.scale,
                99.9 / self.scale,
                -99.9 / self.scale,
            ]);
            if let Some(limit) = self.integer_above {
                values.push(limit - 0.1 / self.scale);
            }
        }
        values
            .into_iter()
            .filter(|v| *v >= self.min && *v <= self.max)
            .filter_map(|v| self.resolve(v, NumericOperation::Format).ok())
            .map(|v| {
                if compact {
                    self.compact_text(v.value)
                } else {
                    v.text
                }
            })
            .collect()
    }
    /// Value only, for hosts that present the unit separately.
    pub fn compact_value(&self, value: f64) -> String {
        let shown = value * self.scale;
        let digits = if (shown * 10.).round().abs() >= 1000.
            || self.integer_above.is_some_and(|v| value > v)
        {
            0
        } else {
            self.digits.min(1)
        };
        let text = format!("{:.*}", digits as usize, shown);
        if digits > 0 {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    }
    fn expression(&self, source: &str) -> Result<f64, NumericError> {
        let text = bounded_numeric_text(source)?.trim();
        let text = if self.unit.is_empty() { text }
            else { text.strip_suffix(&self.unit).unwrap_or(text).trim_end() };
        // fasteval has a diagnostic print() builtin. Number fields expose only
        // math, not string literals or diagnostic output.
        if text.contains(['"', '\''])
            || text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|s| s == "print")
        {
            return Err(NumericError::ExpressionRequired);
        }
        let mut namespace = |name: &str, args: Vec<f64>| match (name, args.as_slice()) {
            ("pi", []) => Some(std::f64::consts::PI),
            ("e", []) => Some(std::f64::consts::E),
            ("tau", []) => Some(std::f64::consts::TAU),
            ("sqrt", [value]) => Some(value.sqrt()),
            _ => None,
        };
        fasteval::ez_eval(text, &mut namespace).map_err(|_| NumericError::InvalidExpression)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub fn resolve(self) -> Result<NumericValue, NumericError> {
        self.control.resolve(self.value, self.operation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_words_preserve_numeric_editing_and_slider_policy() {
        let mut spec = NumericControl::percent();
        spec.digits = 0;
        spec.endpoint_labels = Some(["White".into(), "Color".into()]);
        for (position, label, edit) in [
            (0., "White", "0"),
            (0.5, "50 %", "50"),
            (1., "Color", "100"),
        ] {
            let result = spec
                .resolve(0., NumericOperation::Position { position })
                .unwrap();
            assert_eq!(result.value, position);
            assert_eq!(result.text, label);
            assert_eq!(result.edit, edit);
            assert_eq!(expr(&spec, &result.edit).unwrap().value, position);
        }
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<NumericControl>(&json).unwrap(), spec);
        assert!(
            !NumericControl::percent()
                .resolve(0., NumericOperation::Format)
                .unwrap()
                .text
                .contains("White")
        );
    }

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
    fn expr(spec: &NumericControl, text: &str) -> Result<NumericValue, NumericError> {
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

#[cfg(test)]
mod compact_tests {
    use super::*;
    #[test]
    fn size_rounds_edits_above_32_and_compact_readouts_preserve_values() {
        let spec = NumericControl::brush_size();
        for (value, expected) in [
            (31.8, 31.8),
            (32., 32.),
            (32.2, 32.),
            (32.6, 33.),
            (517.6, 518.),
            (2047.9, 2048.),
        ] {
            let result = spec
                .resolve(value, NumericOperation::Value { value })
                .unwrap();
            assert!((result.value - expected).abs() < 0.0001);
            assert_eq!(
                spec.resolve(value, NumericOperation::Format).unwrap().value,
                value
            );
        }
        for (value, expected) in [
            (9.5, "9.5"),
            (32., "32"),
            (99., "99"),
            (999.2, "999"),
            (2048., "2048"),
        ] {
            assert_eq!(spec.compact_text(value), format!("{expected} px"));
            assert_eq!(spec.compact_value(value), expected);
        }
        assert_eq!(NumericControl::percent().compact_text(1.), "100 %");
        assert_eq!(NumericControl::percent().compact_text(0.999), "99.9 %");
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn compatibility_numeric_input_is_refused_without_rewriting_literal_text() {
        let control = NumericControl::number(-100., 100., 0.1, 1).unit("px");
        for text in ["１２．５ ｐｘ", "－２＋３", "＋５＊２", "ｓｑｒｔ（９）", "２＾３", "５０％", "１２＋", "２＋３", "２３", "１２３", "١٢", "12,5", "−2", "2²"] {
            let operation = NumericOperation::Expression { text: text.into() };
            let encoded = serde_json::to_string(&operation).unwrap();
            assert_eq!(serde_json::from_str::<NumericOperation>(&encoded).unwrap(), operation);
            assert!(control.resolve(32., operation.clone()).is_err(), "{text}");
            assert_eq!(operation, NumericOperation::Expression { text: text.into() });
        }
        for (text, value) in [("12.5 px", 12.5), ("-2+3", 1.), ("+5*2", 10.), ("sqrt(9)", 3.), ("2^3", 8.)] {
            assert_eq!(control.resolve(0., NumericOperation::Expression { text: text.into() }).unwrap().value, value);
        }
        let pressure = NumericControl::number(0.1, 5., 0.1, 1).unit("×");
        assert_eq!(pressure.resolve(0., NumericOperation::Expression { text: "　1.5 × ".into() }).unwrap().value, 1.5);
        let percent = NumericControl::percent();
        assert_eq!(percent.resolve(0., NumericOperation::Expression { text: "50%".into() }).unwrap().value, 0.5);
    }

    #[test]
    fn raw_expression_work_is_bounded_and_nonfinite_values_stay_refused() {
        let control = NumericControl::number(0., 100., 0.1, 1);
        for text in ["1".repeat(257), "１".repeat(86)] {
            assert_eq!(control.resolve(0., NumericOperation::Expression { text }).unwrap_err(), NumericError::ExpressionTooLong);
        }
        for text in ["1/0", "sqrt(-1)", "1e999"] {
            assert_eq!(control.resolve(0., NumericOperation::Expression { text: text.into() }).unwrap_err(), NumericError::FiniteNumber);
        }
        assert_eq!(control.resolve(0., NumericOperation::Expression { text: "2000".into() }).unwrap().value, 100.);
        assert_eq!(control.resolve(42.125, NumericOperation::Format).unwrap().value, 42.125);
        let mut default = control.clone();
        default.default_value = Some(7.);
        assert_eq!(default.resolve(0., NumericOperation::Expression { text: "　 ".into() }).unwrap().value, 7.);
    }

    #[test]
    fn numeric_refusals_retain_literal_labels_and_unrounded_bounds() {
        let control = NumericControl::number(0., 1., 0.1, 1);
        let label = "日本語 { $literal }\u{202e}１２";
        let reason = control.validate(f32::NAN, label).unwrap_err();
        assert_eq!(reason, NumericError::Range { label: label.into(), min: 0., max: 1. });
        assert!(reason.message(&Localizer::shared(crate::UiLanguage::English)).contains(label));
        assert!(control.validate(0., label).is_ok());
        assert!(control.validate(1., label).is_ok());
        assert!(control.validate(1.1, label).is_err());
    }
}
