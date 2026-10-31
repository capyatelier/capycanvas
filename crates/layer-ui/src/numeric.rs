//! Platform-neutral number-field policy. Native widgets own focus, selection,
//! pointer capture and unfinished text, never numeric math or expression parsing.
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, sync::Arc};
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
    WholeNumber { label: ResourceLabel },
}
impl NumericError {
    pub fn valid(&self) -> bool {
        let label = match self {
            Self::Range { label, min, max } => {
                if !min.is_finite() || !max.is_finite() || min >= max { return false; }
                label
            }
            Self::WholePixels { label } | Self::WholeNumber { label } => label,
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
            Self::WholeNumber { .. } => MessageId::NUMERIC_WHOLE_NUMBER,
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
            Self::WholePixels { label } | Self::WholeNumber { label } => {
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
pub(crate) fn bounded_numeric_text(source: &str) -> Result<Cow<'_, str>, NumericError> {
    if source.len() > 256 { return Err(NumericError::ExpressionTooLong); }
    let ascii = |character| match character {
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(character as u32 - 0xfee0).unwrap(),
        '。' => '.',
        'ー' => '-',
        character => character,
    };
    Ok(if source.chars().any(|character| ascii(character) != character) {
        Cow::Owned(source.chars().map(ascii).collect())
    } else { Cow::Borrowed(source) })
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
    Dial,
}

pub use layer_core::NumericMapping;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericSliderSnap {
    #[default]
    Resolution,
    Pixels,
    Percent,
    Whole,
}

const FRACTIONAL_PIXEL_LIMIT: f64 = 32.;
const DIAL_SNAP_DEGREES: f64 = 15.;
const DIAL_SNAP_RADII: f64 = 2.;
const DIAL_CENTER_RADII: f64 = 0.15;
const FRACTIONAL_PIXEL_SCRUB_LIMIT: f64 = 1000.;
const SCRUB_PIXELS_PER_STEP: f64 = 4.;
const FINE_SCRUB_SLOWDOWN: f64 = 10.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumericControl {
    pub kind: NumericKind,
    pub mapping: NumericMapping,
    #[serde(default)]
    pub log_offset: f64,
    /// Hard bounds for typed input. Sliders use the soft bounds.
    pub min: f64,
    pub max: f64,
    pub soft_min: f64,
    pub soft_max: f64,
    pub step: f64,
    /// Smallest stored increment, separate from plus/minus stepping.
    pub resolution: f64,
    #[serde(default)]
    pub slider_snap: NumericSliderSnap,
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
    /// Increment a drag without Shift lands on when typed values keep finer precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrub_snap: Option<f64>,
}
impl NumericControl {
    pub fn same_rules(&self, other: &Self) -> bool {
        (self.kind, &self.mapping, self.min, self.max, self.soft_min, self.soft_max, self.step, self.log_offset)
            == (other.kind, &other.mapping, other.min, other.max, other.soft_min, other.soft_max, other.step, other.log_offset)
            && (self.resolution, self.slider_snap, self.digits, self.scale, &self.unit, self.default_value, self.scrub_snap)
                == (other.resolution, other.slider_snap, other.digits, other.scale, &other.unit, other.default_value, other.scrub_snap)
    }

    pub fn number(min: f64, max: f64, step: f64, digits: u32) -> Self {
        Self {
            kind: if digits == 0 && (max - min) / step <= 64.0 {
                NumericKind::Number
            } else {
                NumericKind::Slider
            },
            mapping: NumericMapping::Linear,
            log_offset: 0.,
            min,
            max,
            soft_min: min,
            soft_max: max,
            step,
            slider_snap: NumericSliderSnap::Resolution,
            resolution: 10f64.powi(-(digits as i32)),
            digits,
            scale: 1.0,
            unit: String::new(),
            default_value: None,
            endpoint_labels: None,
            scrub_snap: None,
        }
    }
    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = unit.into();
        self.slider_snap = match unit {
            "px" => NumericSliderSnap::Pixels,
            "%" => NumericSliderSnap::Percent,
            "°" => NumericSliderSnap::Whole,
            _ => NumericSliderSnap::Resolution,
        };
        self
    }
    pub fn percent() -> Self {
        Self {
            kind: NumericKind::Slider,
            scale: 100.0,
            unit: "%".into(),
            slider_snap: NumericSliderSnap::Percent,
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
    pub fn whole_pixels(min: f64, max: f64) -> Self {
        Self {
            slider_snap: NumericSliderSnap::Resolution,
            ..Self::number(min, max, 1., 0).unit("px")
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
            slider_snap: NumericSliderSnap::Percent,
            resolution: 0.0001,
            ..Self::number(f64::from(crate::MIN_ZOOM), f64::from(crate::MAX_ZOOM), 0.1, 0)
        }
    }
    pub fn rotation() -> Self {
        Self {
            kind: NumericKind::Slider,
            scale: 180.0 / std::f64::consts::PI,
            resolution: std::f64::consts::PI / 1800.0,
            unit: "°".into(),
            slider_snap: NumericSliderSnap::Whole,
            ..Self::number(-std::f64::consts::PI, std::f64::consts::PI, std::f64::consts::PI / 180.0, 1)
        }
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
            self.log_offset,
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
            || self.mapping != NumericMapping::Log && self.log_offset != 0.
        {
            return Err(NumericError::InvalidDefinition);
        }
        match self.mapping {
            NumericMapping::Log if self.soft_min + self.log_offset <= 0.0 => {
                return Err(NumericError::PositiveLogarithmicBounds);
            }
            NumericMapping::Power { exponent }
                if !exponent.is_finite() || !(0.125..=8.0).contains(&exponent) =>
            {
                return Err(NumericError::InvalidRangeExponent);
            }
            _ => (),
        }
        let mapped_min = self.mapped(self.soft_min);
        let mapped_max = self.mapped(self.soft_max);
        if !mapped_min.is_finite() || !mapped_max.is_finite() || mapped_min >= mapped_max {
            return Err(NumericError::InvalidMappedRange);
        }
        Ok(())
    }
    fn mapped(&self, value: f64) -> f64 {
        match self.mapping {
            NumericMapping::Linear => value,
            NumericMapping::Log => (value + self.log_offset).ln(),
            NumericMapping::Power { exponent } => value.signum() * value.abs().powf(exponent),
        }
    }
    fn unmapped(&self, value: f64) -> f64 {
        match self.mapping {
            NumericMapping::Linear => value,
            NumericMapping::Log => value.exp() - self.log_offset,
            NumericMapping::Power { exponent } => value.signum() * value.abs().powf(1.0 / exponent),
        }
    }
    fn fine_resolution(&self) -> f64 {
        if self.kind == NumericKind::Slider && self.slider_snap != NumericSliderSnap::Resolution {
            self.resolution.min(0.1 / self.scale)
        } else { self.resolution }
    }
    fn display_digits(&self) -> u32 {
        if self.fine_resolution() * self.scale < 1. && self.slider_snap != NumericSliderSnap::Resolution {
            self.digits.max(1)
        } else { self.digits }
    }
    fn slider_resolution(&self, value: f64) -> f64 {
        let shown = value * self.scale;
        let increment = match self.slider_snap {
            NumericSliderSnap::Resolution => return self.resolution,
            NumericSliderSnap::Pixels if shown.abs() <= FRACTIONAL_PIXEL_LIMIT => 0.1,
            NumericSliderSnap::Percent if shown <= self.soft_min * self.scale + 5.
                || shown >= self.soft_max * self.scale - 5. => 0.1,
            _ => 1.,
        };
        (increment / self.scale).max(self.fine_resolution())
    }
    fn scrub_resolution(&self, value: f64) -> f64 {
        if self.unit == "px" && (value * self.scale).abs() > FRACTIONAL_PIXEL_SCRUB_LIMIT {
            self.fine_resolution().max(1. / self.scale)
        } else { self.fine_resolution() }
    }
    fn quantized(&self, value: f64, resolution: f64) -> f64 {
        let value = (value / resolution).round() * resolution;
        if self.kind == NumericKind::Dial { self.min + (value - self.min).rem_euclid(self.max - self.min) }
        else { value.clamp(self.min, self.max) }
    }
    fn dial_value(&self, value: f64, x: f64, y: f64, radius: f64) -> Result<f64, NumericError> {
        if ![x, y, radius].into_iter().all(f64::is_finite) || radius <= 0. { return Err(NumericError::FiniteNumber); }
        let distance = x.hypot(y);
        if distance < DIAL_CENTER_RADII * radius { return Ok(value); }
        let degrees = y.atan2(x).to_degrees();
        let degrees = if distance <= DIAL_SNAP_RADII * radius { (degrees / DIAL_SNAP_DEGREES).round() * DIAL_SNAP_DEGREES } else { degrees };
        Ok(degrees / self.scale)
    }
    fn value_at_position(&self, position: f64) -> f64 {
        self.unmapped(self.mapped(self.soft_min)
            + position.clamp(0., 1.) * (self.mapped(self.soft_max) - self.mapped(self.soft_min)))
    }
    pub fn position_matches_value(&self, position: f64, value: f64) -> bool {
        self.check().is_ok() && position.is_finite() && value.is_finite()
            && self.quantized(self.value_at_position(position), self.fine_resolution())
                == self.quantized(value.clamp(self.soft_min, self.soft_max), self.fine_resolution())
    }
    pub fn values_equal(&self, left: f64, right: f64) -> bool {
        self.check().is_ok() && left.is_finite() && right.is_finite()
            && self.quantized(left, self.fine_resolution()) == self.quantized(right, self.fine_resolution())
    }
    pub fn resolve(&self, value: f64, operation: NumericOperation) -> Result<NumericValue, NumericError> {
        self.check()?;
        if !value.is_finite() {
            return Err(NumericError::FiniteNumber);
        }
        let formatting = matches!(operation, NumericOperation::Format);
        let slider = matches!(operation, NumericOperation::Position { .. });
        let fine_scrub = matches!(operation, NumericOperation::Scrub { fine: true, .. });
        let coarse_scrub = matches!(operation, NumericOperation::Scrub { fine: false, .. });
        let same_position = (self.min..=self.max).contains(&value)
            && matches!(&operation, NumericOperation::Position { position } if self.position_matches_value(*position, value));
        let unmoved_scrub = matches!(&operation, NumericOperation::Scrub { pixels, .. } if *pixels == 0.);
        let mut resolved = match operation {
            NumericOperation::Format => value,
            NumericOperation::Step { steps } => {
                if !steps.is_finite() {
                    return Err(NumericError::InvalidStep);
                }
                value + steps * self.step
            }
            NumericOperation::Scrub { origin, pixels, fine } => {
                if !origin.is_finite() || !pixels.is_finite() { return Err(NumericError::FiniteNumber); }
                origin + pixels * self.step / SCRUB_PIXELS_PER_STEP / if fine { FINE_SCRUB_SLOWDOWN } else { 1. }
            }
            NumericOperation::Value { value } => value,
            NumericOperation::Position { position } => {
                if !position.is_finite() {
                    return Err(NumericError::InvalidPosition);
                }
                self.value_at_position(position)
            }
            NumericOperation::Expression { text } => self.expression(&text)?,
            NumericOperation::Dial { x, y, radius } => self.dial_value(value, x, y, radius)?,
        };
        if !resolved.is_finite() {
            return Err(NumericError::FiniteNumber);
        }
        // Formatting an externally supplied value must never change it.
        if same_position { resolved = value; }
        else if !formatting && !unmoved_scrub {
            let resolution = if coarse_scrub && let Some(snap) = self.scrub_snap { snap.max(self.fine_resolution()) }
                else if slider || coarse_scrub { self.slider_resolution(resolved) }
                else if fine_scrub { self.scrub_resolution(resolved) } else { self.fine_resolution() };
            resolved = self.quantized(resolved, resolution);
        }
        let shown = if resolved == 0.0 {
            0.0
        } else {
            resolved * self.scale
        };
        let fixed = format!("{:.*}", self.display_digits() as usize, shown);
        let edit = trim_fractional_zeros(&fixed);
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
    /// Short toolbar readout with at most one fractional digit.
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
        if self.display_digits() > 0 {
            values.extend([
                ((self.max * self.scale).ceil() - 0.1) / self.scale,
                ((self.min * self.scale).floor() + 0.1) / self.scale,
                99.9 / self.scale,
                -99.9 / self.scale,
            ]);
            if self.slider_snap == NumericSliderSnap::Pixels {
                values.push((FRACTIONAL_PIXEL_LIMIT - 0.1) / self.scale);
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
        let digits = self.display_digits().min(1);
        let text = format!("{:.*}", digits as usize, shown);
        trim_fractional_zeros(&text)
    }
    fn expression(&self, source: &str) -> Result<f64, NumericError> {
        let text = bounded_numeric_text(source)?;
        let text = text.trim();
        if text.is_empty() { return self.default_value.ok_or(NumericError::ExpressionRequired); }
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
        fasteval::ez_eval(text, &mut namespace).map(|value| value / self.scale).map_err(|_| NumericError::InvalidExpression)
    }
}

fn trim_fractional_zeros(text: &str) -> String {
    let trimmed = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { text };
    if trimmed == "-0" { "0".into() } else { trimmed.into() }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumericOperation {
    Format,
    /// Native spin buttons report a value; sliders report a normalized position.
    Value {
        value: f64,
    },
    Position {
        position: f64,
    },
    Step {
        steps: f64,
    },
    Scrub {
        origin: f64,
        pixels: f64,
        #[serde(default)]
        fine: bool,
    },
    Expression {
        text: String,
    },
    Dial {
        x: f64,
        y: f64,
        radius: f64,
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
    fn value_scrubs_snap_like_sliders_unless_the_precision_key_is_held() {
        let brush = NumericControl::brush_size();
        let scrub = |spec: &NumericControl, origin, pixels, fine| spec.resolve(origin, NumericOperation::Scrub { origin, pixels, fine }).unwrap().value;
        assert_eq!(scrub(&brush, 120., 4., false), 121.);
        assert_eq!(scrub(&brush, 120., -4., false), 119.);
        assert_eq!(scrub(&brush, 120., 1., false), 120., "a wiggle keeps the whole pixel");
        let percent = NumericControl::percent();
        assert_eq!(scrub(&percent, 0.5, 1., false), 0.5, "a wiggle keeps the whole percent");
        assert_eq!(scrub(&percent, 0.5, 4., false), 0.51);
        assert_eq!(scrub(&percent, 0.5, 20., false), 0.55);
        assert_eq!(scrub(&percent, 0.5, 4., true), 0.501, "the precision key moves ten times slower in tenths");
        assert_eq!(scrub(&percent, 0.5, 40., true), 0.51);
        assert!((scrub(&brush, 120., 2., true) - 120.1).abs() < 1e-9);
        let mut extended = percent.clone();
        extended.min = -100.; extended.max = 100.;
        assert!((scrub(&extended, 2., 4., false) - 2.01).abs() < 1e-12);
        assert_eq!(scrub(&percent, 0.999, 400., false), 1.);
        assert_eq!(scrub(&brush, 1., -400., false), 0.5);
        assert_eq!(brush.resolve(1005., NumericOperation::Scrub { origin: 999.5, pixels: 0., fine: false }).unwrap().value, 999.5,
            "returning to the start keeps its exact value");
        for (origin, pixels) in [(f64::NAN, 1.), (1., f64::INFINITY)] {
            assert_eq!(percent.resolve(1., NumericOperation::Scrub { origin, pixels, fine: false }).unwrap_err(), NumericError::FiniteNumber);
        }
    }
    #[test]
    fn large_pixel_scrubs_snap_to_integers_without_restricting_typed_fractions() {
        let brush = NumericControl::brush_size();
        for (origin, pixels, expected, readout) in [
            (999., 20., 999.5, "999.5 px"),
            (999., 40., 1000., "1000 px"),
            (1000., 20., 1001., "1001 px"),
            (1000., -20., 999.5, "999.5 px"),
            (1200., 20., 1201., "1201 px"),
            (1200., -20., 1200., "1200 px"),
            (1200.5, 40., 1202., "1202 px"),
            (1200.5, -40., 1200., "1200 px"),
            (2047.5, 200., 2048., "2048 px"),
        ] {
            let result = brush.resolve(origin, NumericOperation::Scrub { origin, pixels, fine: true }).unwrap();
            assert_eq!(result.value, expected, "{origin} with {pixels}px motion");
            assert_eq!(result.text, readout);
            assert_eq!(brush.resolve(result.value, NumericOperation::Format).unwrap().text, readout);
        }
        for operation in [NumericOperation::Format, NumericOperation::Expression { text: "1200.5 px".into() }] {
            let result = brush.resolve(1200.5, operation).unwrap();
            assert_eq!(result.value, 1200.5);
            assert_eq!(result.text, "1200.5 px");
            assert_eq!(result.edit, "1200.5");
        }
        let mut scaled = NumericControl::number(-20., 20., 0.01, 3).unit("px");
        scaled.scale = 100.;
        for origin in [-12., 12.] {
            let result = scaled.resolve(origin, NumericOperation::Scrub { origin, pixels: 20. * origin.signum(), fine: true }).unwrap();
            assert!((result.value - origin - 0.01 * origin.signum()).abs() < 1e-12);
            assert_eq!(result.text, format!("{:.0} px", result.value * scaled.scale));
        }
        let degrees = NumericControl::number(-2000., 2000., 1., 1).unit("°");
        assert_eq!(degrees.resolve(1200., NumericOperation::Scrub { origin: 1200., pixels: 20., fine: true }).unwrap().value, 1200.5);
    }
    #[test]
    fn dials_point_where_pressed_snap_near_the_dial_and_wrap() {
        let dial = NumericControl { kind: NumericKind::Dial, ..NumericControl::number(0., 360., 1., 0).unit("°") };
        let at = |value, x, y| dial.resolve(value, NumericOperation::Dial { x, y, radius: 20. }).unwrap().value;
        assert_eq!(at(0., -10., -10.), 225., "upper left, y down");
        assert_eq!(at(0., 30., 9.), 15., "within twice the radius the direction snaps to 15°");
        assert_eq!(at(0., 100., 30.), 17., "farther out it moves freely in whole degrees");
        assert_eq!(at(225., 1., 1.), 225., "a press on the center keeps the direction");
        assert_eq!(at(0., 10., -0.1), 0.);
        let step = |value, steps| dial.resolve(value, NumericOperation::Step { steps }).unwrap().value;
        assert_eq!(step(359., 1.), 0.);
        assert_eq!(step(0., -15.), 345.);
        assert_eq!(dial.resolve(0., NumericOperation::Value { value: 360. }).unwrap().value, 0.);
        assert_eq!(dial.resolve(0., NumericOperation::Expression { text: "-90".into() }).unwrap().value, 270.);
        assert!(dial.resolve(0., NumericOperation::Dial { x: 1., y: 1., radius: 0. }).is_err());
    }
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
        assert_eq!(result.text, "100 %");
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
        assert_eq!(percent.text, "50 %");
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
        for (mapping, log_offset, min, expected) in [
            (NumericMapping::Log, 0., 1., 10.),
            (NumericMapping::Log, 1., 0., 101f64.sqrt() - 1.),
            (NumericMapping::Power { exponent: 2.0 }, 0., 1., 5000.5f64.sqrt()),
            (NumericMapping::Power { exponent: 0.5 }, 0., 1., 30.25),
        ] {
            let spec = NumericControl {
                mapping,
                log_offset,
                kind: NumericKind::Slider,
                ..NumericControl::number(min, 100.0, 1.0, 6)
            };
            let result = spec
                .resolve(1.0, NumericOperation::Position { position: 0.5 })
                .unwrap();
            assert!((result.fill - 0.5).abs() < 1e-6);
            assert!((result.value - expected).abs() < 1e-6);
            assert_eq!(
                spec.resolve(10.0, NumericOperation::Step { steps: 1.0 })
                    .unwrap()
                    .value,
                11.0
            );
        }
        let first = NumericControl {
            mapping: NumericMapping::Log, log_offset: 1., kind: NumericKind::Slider,
            ..NumericControl::number(0., 100., 1., 6)
        };
        let second = NumericControl { log_offset: 4., ..first.clone() };
        let midpoint = |spec: &NumericControl| spec.resolve(0., NumericOperation::Position { position: 0.5 }).unwrap().value;
        assert_ne!(midpoint(&first), midpoint(&second));
        assert!(!first.same_rules(&second));
    }
    #[test]
    fn logarithmic_offsets_reject_invalid_domains() {
        for (mapping, log_offset, min) in [
            (NumericMapping::Log, 0., 0.),
            (NumericMapping::Log, 1., -1.),
            (NumericMapping::Log, f64::NAN, 0.),
            (NumericMapping::Log, f64::INFINITY, 0.),
            (NumericMapping::Log, 1e308, 0.),
            (NumericMapping::Linear, 1., 0.),
        ] {
            let spec = NumericControl { mapping, log_offset, ..NumericControl::number(min, 100., 1., 0) };
            assert!(spec.resolve(min, NumericOperation::Format).is_err());
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
    fn unchanged_slider_positions_preserve_fine_values_and_extended_authored_values() {
        for (spec, value) in [(NumericControl::brush_size(), 120.5), (NumericControl::percent(), 0.355),
            (NumericControl::rotation(), 12.6_f64.to_radians()),
            (NumericControl { max: 10., ..NumericControl::percent() }, 2.3456)] {
            let value = f64::from(value as f32);
            let fill = spec.resolve(value, NumericOperation::Format).unwrap().fill;
            assert_eq!(spec.resolve(value, NumericOperation::Position { position: fill }).unwrap().value, value);
        }
        assert_eq!(NumericControl::percent().resolve(10., NumericOperation::Position { position: 1. }).unwrap().value, 1.);
    }
    #[test]
    fn size_slider_uses_the_existing_32_pixel_threshold_and_fine_edits_keep_decimals() {
        let spec = NumericControl::brush_size();
        for (value, expected) in [
            (31.8, 31.8),
            (32., 32.),
            (32.2, 32.),
            (32.6, 33.),
            (517.6, 518.),
            (2047.9, 2048.),
        ] {
            let fill = spec.resolve(value, NumericOperation::Format).unwrap().fill;
            let result = spec
                .resolve(spec.min, NumericOperation::Position { position: fill })
                .unwrap();
            assert!((result.value - expected).abs() < 0.0001);
            for operation in [NumericOperation::Value { value }, NumericOperation::Expression { text: value.to_string() }] {
                assert!((spec.resolve(value, operation).unwrap().value - value).abs() < 0.0001);
            }
            assert_eq!(
                spec.resolve(value, NumericOperation::Format).unwrap().value,
                value
            );
        }
        for (value, expected) in [
            (9.5, "9.5"),
            (32., "32"),
            (99., "99"),
            (999.2, "999.2"),
            (2048., "2048"),
        ] {
            assert_eq!(spec.compact_text(value), format!("{expected} px"));
            assert_eq!(spec.compact_value(value), expected);
        }
        assert_eq!(NumericControl::percent().compact_text(1.), "100 %");
        assert_eq!(NumericControl::percent().compact_text(0.999), "99.9 %");
    }

    #[test]
    fn percentage_sliders_keep_tenths_within_five_displayed_points_of_soft_edges() {
        let mut extended = NumericControl::percent(); extended.max = 10.;
        for spec in [NumericControl::percent(), NumericControl::number(-100., 100., 1., 0).unit("%"), extended] {
            let value_at = |shown: f64| shown / spec.scale;
            for (shown, expected) in [(spec.soft_min * spec.scale + 4.3, spec.soft_min * spec.scale + 4.3),
                (spec.soft_min * spec.scale + 5.6, spec.soft_min * spec.scale + 6.),
                (spec.soft_max * spec.scale - 5.6, spec.soft_max * spec.scale - 6.),
                (spec.soft_max * spec.scale - 4.3, spec.soft_max * spec.scale - 4.3)] {
                let value = value_at(shown);
                let fill = spec.resolve(value, NumericOperation::Format).unwrap().fill;
                let slider = spec.resolve(spec.min, NumericOperation::Position { position: fill }).unwrap();
                assert!((slider.value * spec.scale - expected).abs() < 1e-8);
                assert!((spec.resolve(value, NumericOperation::Expression { text: shown.to_string() }).unwrap().value - value).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn degree_sliders_snap_in_displayed_units_and_fine_scrubs_survive_model_echoes() {
        let spec = NumericControl::rotation();
        let value = 12.6 / spec.scale;
        let fill = spec.resolve(value, NumericOperation::Format).unwrap().fill;
        let slider = spec.resolve(spec.min, NumericOperation::Position { position: fill }).unwrap();
        assert!((slider.value * spec.scale - 13.).abs() < 1e-10);
        assert!(spec.values_equal(slider.value, f64::from(slider.value as f32)));
        let scrub = NumericControl::brush_size().resolve(120., NumericOperation::Scrub { origin: 120., pixels: 20., fine: true }).unwrap();
        assert_eq!(scrub.value, 120.5);
        assert_eq!(scrub.text, "120.5 px");
        for (spec, value) in [(NumericControl::brush_size(), 120.5), (NumericControl::percent(), 0.355),
            (spec.clone(), value)] {
            let value = f64::from(value as f32);
            let fill = spec.resolve(value, NumericOperation::Format).unwrap().fill;
            assert!(spec.position_matches_value(fill, value));
            assert!(!spec.position_matches_value(fill + 0.02, value));
        }
        let extended = NumericControl { max: 10., ..NumericControl::percent() };
        assert!(extended.position_matches_value(1., 2.3456));
        assert!(!extended.position_matches_value(0.98, 2.3456));
    }

    #[test]
    fn numeric_readouts_omit_padding_and_keep_actual_fractions() {
        for (spec, origin, pixels, fine, text) in [
            (NumericControl::brush_size(), 120., 4., false, "121 px"),
            (NumericControl::brush_size(), 32., 4., false, "33 px"),
            (NumericControl::brush_size(), 32., -2., false, "31.5 px"),
            (NumericControl::brush_size(), 120., 20., true, "120.5 px"),
            (NumericControl::brush_size(), 1200.5, 0., false, "1200.5 px"),
            (NumericControl::percent(), 0.5, 4., false, "51 %"),
            (NumericControl::percent(), 0.04, 2., false, "4.5 %"),
            (NumericControl::percent(), 0.95, 2., false, "95.5 %"),
            (NumericControl::percent(), 0.5, 20., true, "50.5 %"),
            (NumericControl::number(-180., 180., 1., 0).unit("°"), 10., 4., false, "11 °"),
            (NumericControl::number(0.25, 4., 0.05, 2).unit("×"), 1., 0., false, "1 ×"),
            (NumericControl::number(0.25, 4., 0.05, 2).unit("×"), 1., 4., false, "1.05 ×"),
            (NumericControl::whole_pixels(0., 64.), 12., 4., false, "13 px"),
        ] {
            let result = spec.resolve(origin, NumericOperation::Scrub { origin, pixels, fine }).unwrap();
            assert_eq!(result.text, text);
            assert_eq!(spec.resolve(result.value, NumericOperation::Format).unwrap().text, text);
        }
        assert_eq!(NumericControl::brush_size().resolve(120.5, NumericOperation::Format).unwrap().text, "120.5 px");
        let count = NumericControl::number(0., 16., 1., 0).unit("px");
        assert_eq!(count.resolve(0., NumericOperation::Expression { text: "5.5".into() }).unwrap().value, 6.);
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn numeric_width_conversion_borrows_unchanged_text_and_maps_all_fullwidth_ascii() {
        for text in ["12.5", "-12", "日本", "١٢", "−2"] {
            assert!(matches!(bounded_numeric_text(text).unwrap(), Cow::Borrowed(value) if value == text));
        }
        let fullwidth: String = ('\u{ff01}'..='\u{ff5e}').collect();
        assert!(fullwidth.len() > 256);
        for character in fullwidth.chars() {
            let text = character.to_string();
            assert_eq!(bounded_numeric_text(&text).unwrap(), char::from_u32(character as u32 - 0xfee0).unwrap().to_string());
        }
        assert_eq!(bounded_numeric_text(&fullwidth).unwrap_err(), NumericError::ExpressionTooLong);
    }

    #[test]
    fn compatibility_numeric_input_is_accepted_without_rewriting_literal_text() {
        let control = NumericControl::number(-100., 100., 0.1, 1).unit("px");
        for (text, value) in [("１２．５ ｐｘ", 12.5), ("１２。５", 12.5), ("ー１２", -12.), ("－２＋３", 1.),
            ("＋５＊２", 10.), ("ｓｑｒｔ（９）", 3.), ("２＾３", 8.), ("２＋３", 5.), ("２３", 23.), ("１２３", 100.)] {
            let operation = NumericOperation::Expression { text: text.into() };
            let encoded = serde_json::to_string(&operation).unwrap();
            assert_eq!(serde_json::from_str::<NumericOperation>(&encoded).unwrap(), operation);
            assert_eq!(control.resolve(32., operation.clone()).unwrap().value, value, "{text}");
            assert_eq!(operation, NumericOperation::Expression { text: text.into() });
        }
        for text in ["１２＋", "١٢", "12,5", "−2", "2²", "ｐｒｉｎｔ（１）", "＂１２＂"] {
            assert!(control.resolve(32., NumericOperation::Expression { text: text.into() }).is_err(), "{text}");
        }
        let pressure = NumericControl::number(0.1, 5., 0.1, 1).unit("×");
        assert_eq!(pressure.resolve(0., NumericOperation::Expression { text: "　1.5 × ".into() }).unwrap().value, 1.5);
        let percent = NumericControl::percent();
        for text in ["50%", "５０％"] {
            assert_eq!(percent.resolve(0., NumericOperation::Expression { text: text.into() }).unwrap().value, 0.5);
        }
        for (text, value) in [("１２．５", 12.5), ("１２。５", 12.5), ("ー１２", -12.), ("－１２", -12.)] {
            assert_eq!(parse_numeric_text(text).unwrap(), value);
        }
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
