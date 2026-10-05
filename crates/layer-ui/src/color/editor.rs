use super::*;
use super::text::parse_color_text;
use std::sync::Arc;

const MAX_SEARCH: usize = 256;
const SCRUB_PIXELS: f64 = 2.;
const SCRUB_INTENSITY_STEP: f64 = 0.05;
const SCRUB_INTENSITY_RANGE: (f64, f64) = (-2., 6.);

crate::variants! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ColorForm {
        #[default]
        Rgb,
        RgbUnit,
        LinearRgb,
        Hsb,
        Hsl,
        Oklch,
        Oklab,
    }
}

pub const COLOR_FORM_FAMILIES: [&[ColorForm]; 3] = [
    &[ColorForm::Rgb, ColorForm::RgbUnit, ColorForm::LinearRgb],
    &[ColorForm::Hsb, ColorForm::Hsl],
    &[ColorForm::Oklch, ColorForm::Oklab],
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorEditorMemory {
    pub forms: [ColorForm; 3],
    pub search: String,
}
impl Default for ColorEditorMemory {
    fn default() -> Self {
        Self { forms: [ColorForm::Rgb, ColorForm::Hsb, ColorForm::Oklch], search: String::new() }
    }
}
impl ColorEditorMemory {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.search.len() > MAX_SEARCH || self.forms.iter().enumerate().any(|(row, form)| form.family() != row) {
            return Err("Invalid color editor memory".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValueName {
    Message(crate::MessageId),
    Literal(&'static str),
}
impl ColorValueName {
    pub fn text(self, localizer: &crate::Localizer) -> Arc<str> {
        match self {
            Self::Message(id) => localizer.text(id),
            Self::Literal(text) => text.into(),
        }
    }
}

#[derive(Clone, Copy)]
struct Component {
    name: ColorValueName,
    unit: &'static str,
    min: f64,
    max: f64,
    wrap: bool,
    step: f64,
    digits: usize,
    edit_digits: usize,
}
impl Component {
    const fn new(name: ColorValueName, unit: &'static str, max: f64, digits: usize, edit_digits: usize) -> Self {
        Self { name, unit, min: 0., max, wrap: false, step: 1., digits, edit_digits }
    }
    const fn fine(self, min: f64, step: f64) -> Self {
        Self { min, step, ..self }
    }
    const fn hue(self) -> Self {
        Self { wrap: true, ..self }
    }
    fn bound(self, value: f64) -> f64 {
        if self.wrap { value.rem_euclid(360.) } else { value.clamp(self.min, self.max) }
    }
    fn display(self, value: f64) -> String {
        let value = if self.wrap && self.digits == 0 { value.round().rem_euclid(360.) } else { value };
        format!("{}{}", fixed(value, self.digits), self.unit)
    }
    fn edit(self, value: f64) -> String {
        let text = fixed(value, self.edit_digits);
        if self.edit_digits == 0 { text } else { text.trim_end_matches('0').trim_end_matches('.').to_string() }
    }
}

fn fixed(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    if text.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { text.trim_start_matches('-').to_string() } else { text }
}

const M: fn(crate::MessageId) -> ColorValueName = ColorValueName::Message;
use crate::MessageId as Id;

impl ColorForm {
    pub fn family(self) -> usize {
        match self {
            Self::Rgb | Self::RgbUnit | Self::LinearRgb => 0,
            Self::Hsb | Self::Hsl => 1,
            Self::Oklch | Self::Oklab => 2,
        }
    }
    pub fn label(self) -> ColorValueName {
        match self {
            Self::Rgb => ColorValueName::Literal("RGB"),
            Self::RgbUnit => ColorValueName::Literal("RGB 0–1"),
            Self::LinearRgb => M(Id::COLOR_FORM_MODEL_LINEAR_RGB),
            Self::Hsb => ColorValueName::Literal("HSB"),
            Self::Hsl => ColorValueName::Literal("HSL"),
            Self::Oklch => ColorValueName::Literal("OKLCH"),
            Self::Oklab => ColorValueName::Literal("OKLab"),
        }
    }
    fn components(self) -> [Component; 3] {
        let rgb = [Id::SETTINGS_RED, Id::SETTINGS_GREEN, Id::SETTINGS_BLUE];
        let hue = Component::new(M(Id::COLOR_FORM_FIELD_HUE), "°", 360., 0, 2).hue();
        let percent = |id| Component::new(M(id), "%", 100., 0, 2);
        let lightness = Component::new(M(Id::COLOR_FORM_FIELD_LIGHTNESS), "%", 100., 1, 2);
        match self {
            Self::Rgb => rgb.map(|id| Component::new(M(id), "", 255., 0, 2)),
            Self::RgbUnit => rgb.map(|id| Component::new(M(id), "", 1., 3, 5).fine(0., 0.001)),
            Self::LinearRgb => rgb.map(|id| Component::new(M(id), "", 1., 3, 5).fine(0., 0.001)),
            Self::Hsb => [hue, percent(Id::COLOR_FORM_FIELD_SATURATION), percent(Id::COLOR_FORM_FIELD_VALUE)],
            Self::Hsl => [hue, percent(Id::COLOR_FORM_FIELD_SATURATION), percent(Id::COLOR_FORM_FIELD_LIGHTNESS)],
            Self::Oklch => [lightness, Component::new(M(Id::COLOR_FORM_FIELD_CHROMA), "", 0.5, 3, 4).fine(0., 0.001), hue],
            Self::Oklab => [
                lightness,
                Component::new(ColorValueName::Literal("a"), "", 0.5, 3, 4).fine(-0.5, 0.001),
                Component::new(ColorValueName::Literal("b"), "", 0.5, 3, 4).fine(-0.5, 0.001),
            ],
        }
    }
    pub(super) fn values(self, color: RgbColor, space: RgbSpace, memory: Option<&ColorState>) -> Result<[f64; 3], String> {
        let encoded = color.encoded_in(space)?;
        let rgb = [encoded[0], encoded[1], encoded[2]].map(f64::from);
        let clamped = encoded.map(|v| v.clamp(0., 1.));
        let remembered = memory.filter(|picker| picker.picker_base() == color);
        let lab = || -> Result<[f64; 3], String> {
            let linear = color.linear_in(space)?;
            Ok(gamut::Gamut::get(space).lab([linear[0], linear[1], linear[2]].map(f64::from)))
        };
        Ok(match self {
            Self::Rgb => rgb.map(|v| v * 255.),
            Self::RgbUnit => rgb,
            Self::LinearRgb => {
                let linear = color.linear_in(space)?;
                [linear[0], linear[1], linear[2]].map(f64::from)
            }
            Self::Hsb => remembered.map_or_else(|| components(clamped, ColorSpace::Hsv, 0.), |p| p.components_in(ColorSpace::Hsv)).map(f64::from),
            Self::Hsl => {
                let [h, l, s] = remembered.map_or_else(|| components(clamped, ColorSpace::Hls, 0.), |p| p.components_in(ColorSpace::Hls));
                [h, s, l].map(f64::from)
            }
            Self::Oklch => {
                let [l, a, b] = lab()?;
                let chroma = a.hypot(b);
                let hue = if chroma < 1e-6 {
                    remembered.map_or(0., |p| f64::from(p.okhsv_components()[0]))
                } else {
                    b.atan2(a).to_degrees().rem_euclid(360.)
                };
                [l * 100., chroma, hue]
            }
            Self::Oklab => {
                let [l, a, b] = lab()?;
                [l * 100., a, b]
            }
        })
    }
    pub(super) fn color(self, values: [f64; 3], space: RgbSpace, alpha: f32) -> Result<RgbColor, ColorEditorError> {
        let [a, b, c] = values;
        let lab = |lab: [f64; 3]| -> Result<RgbColor, ColorEditorError> {
            let rgb = gamut::Gamut::get(space).linear_rgb(lab);
            Ok(RgbColor::from_linear(space, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, alpha])?)
        };
        let color = match self {
            Self::Rgb => RgbColor::new(space, [(a / 255.) as f32, (b / 255.) as f32, (c / 255.) as f32, alpha])?,
            Self::RgbUnit => RgbColor::new(space, [a as f32, b as f32, c as f32, alpha])?,
            Self::LinearRgb => RgbColor::from_linear(space, [a as f32, b as f32, c as f32, alpha])?,
            Self::Hsb => RgbColor::new(space, from_components([a, b, c].map(|v| v as f32), ColorSpace::Hsv, alpha))?,
            Self::Hsl => RgbColor::new(space, from_components([a, c, b].map(|v| v as f32), ColorSpace::Hls, alpha))?,
            Self::Oklch => {
                let hue = c.to_radians();
                lab([a / 100., b * hue.cos(), b * hue.sin()])?
            }
            Self::Oklab => lab([a / 100., b, c])?,
        };
        ColorState::validate_definition(color)?;
        Ok(color)
    }
    fn copy(self, values: [f64; 3], space: RgbSpace) -> String {
        let css = match space {
            RgbSpace::Srgb => "srgb",
            RgbSpace::DisplayP3 => "display-p3",
            RgbSpace::AdobeRgb => "a98-rgb",
            RgbSpace::ProPhoto => "prophoto-rgb",
        };
        let list = |digits: usize, units: [&str; 3]| -> String {
            values.iter().zip(units).map(|(v, unit)| format!("{}{unit}", trimmed(*v, digits))).collect::<Vec<_>>().join(" ")
        };
        let srgb = space == RgbSpace::Srgb;
        match self {
            Self::Rgb if srgb => format!("rgb({})", list(0, ["", "", ""])),
            Self::Rgb => format!("color({css} {})", values.map(|v| trimmed(v / 255., 4)).join(" ")),
            Self::RgbUnit => format!("color({css} {})", list(4, ["", "", ""])),
            Self::LinearRgb if srgb => format!("color(srgb-linear {})", list(5, ["", "", ""])),
            Self::LinearRgb => list(5, ["", "", ""]),
            Self::Hsb if srgb => format!("hsb({})", list(0, ["", "%", "%"])),
            Self::Hsl if srgb => format!("hsl({})", list(0, ["", "%", "%"])),
            Self::Hsb | Self::Hsl => list(0, ["°", "%", "%"]),
            Self::Oklch => format!("oklch({})", [trimmed(values[0], 2) + "%", trimmed(values[1], 4), trimmed(values[2], 2)].join(" ")),
            Self::Oklab => format!("oklab({})", [trimmed(values[0], 2) + "%", trimmed(values[1], 4), trimmed(values[2], 4)].join(" ")),
        }
    }
}

fn trimmed(value: f64, digits: usize) -> String {
    let text = fixed(value, digits);
    if digits == 0 { text } else { text.trim_end_matches('0').trim_end_matches('.').to_string() }
}

fn parse_value(text: &str, component: Component) -> Result<f64, crate::NumericError> {
    let text = text.trim().trim_end_matches(component.unit).trim_end_matches("deg").trim();
    Ok(f64::from(crate::numeric::parse_numeric_text(text)?))
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColorEditorError {
    Numeric { name: ColorValueName, reason: crate::NumericError },
    ColorSyntax,
    Intensity(crate::NumericError),
    Hdr(layer_core::color::hdr::HdrPixelError),
    Detail(String),
}
impl From<String> for ColorEditorError {
    fn from(value: String) -> Self { Self::Detail(value) }
}
impl From<&str> for ColorEditorError {
    fn from(value: &str) -> Self { Self::Detail(value.into()) }
}
impl From<layer_core::color::hdr::HdrPixelError> for ColorEditorError {
    fn from(value: layer_core::color::hdr::HdrPixelError) -> Self { Self::Hdr(value) }
}
impl ColorEditorError {
    pub fn message(&self, localizer: &crate::Localizer) -> String {
        match self {
            Self::Numeric { name, reason } => finite_field_message(localizer, name.text(localizer).as_ref(), reason),
            Self::ColorSyntax => localizer.text(Id::COLOR_FORM_COLOR_SYNTAX).to_string(),
            Self::Intensity(reason) => finite_field_message(localizer, localizer.text(Id::NATIVE_COLOR_INTENSITY_EV).as_ref(), reason),
            Self::Hdr(reason) => localizer.text(match reason {
                layer_core::color::hdr::HdrPixelError::ExpectedFloat => Id::COLOR_HDR_EXPECTED_FLOAT,
                layer_core::color::hdr::HdrPixelError::FiniteCoverage => Id::COLOR_HDR_FINITE_COVERAGE,
                layer_core::color::hdr::HdrPixelError::StorageRange => Id::COLOR_HDR_STORAGE_RANGE,
            }).to_string(),
            Self::Detail(reason) => reason.clone(),
        }
    }
}
pub(crate) fn finite_field_message(localizer: &crate::Localizer, label: &str, reason: &crate::NumericError) -> String {
    if !matches!(reason, crate::NumericError::InvalidNumber | crate::NumericError::FiniteNumber) { return reason.message(localizer); }
    let mut args = crate::FluentArgs::new(); args.set("label", label);
    localizer.format(Id::COLOR_FORM_FINITE_FIELD, &args)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorEditorTarget {
    Value { row: usize, index: usize },
    Intensity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScrubSpeed {
    #[default]
    Normal,
    Fast,
    Fine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorEditorAction {
    Wheel { action: ColorAction },
    Value { row: usize, index: usize, text: String },
    Scrub { target: ColorEditorTarget, pixels: f32, #[serde(default)] speed: ColorScrubSpeed },
    EndScrub { cancel: bool },
    Intensity { text: String },
    Text { text: String, #[serde(default)] row: Option<usize> },
    Form { row: usize, form: ColorForm },
    Color { color: RgbColor },
    Revert,
    Search { text: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scrub {
    target: ColorEditorTarget,
    picker: ColorState,
    values: [f64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorEditor {
    picker: ColorState,
    start: ColorState,
    opaque: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scrub: Option<Scrub>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColorShapeChoice {
    pub shape: ColorShape,
    pub label: &'static str,
    pub name: Arc<str>,
    pub selected: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorHexNoteKind {
    Nearest,
    Base,
    Srgb,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorHexNote {
    pub kind: ColorHexNoteKind,
    pub text: Arc<str>,
    pub tip: Option<Arc<str>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorValueView {
    pub text: String,
    pub edit: String,
    pub name: Arc<str>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorFormChoice {
    pub form: ColorForm,
    pub label: Arc<str>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorRowView {
    pub form: ColorForm,
    pub label: Arc<str>,
    pub forms: Vec<ColorFormChoice>,
    pub space: Option<&'static str>,
    pub values: [ColorValueView; 3],
    pub copy: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorEditorView {
    pub panel: ColorPanelView,
    pub shapes: [ColorShapeChoice; 3],
    pub current: ColorPreview,
    pub new: ColorPreview,
    pub hex: String,
    pub hex_note: Option<ColorHexNote>,
    pub rows: [ColorRowView; 3],
    pub intensity: Option<ColorValueView>,
    pub value: RgbColor,
    pub current_value: RgbColor,
    pub stops: Option<f32>,
    pub changed: bool,
    pub search: String,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ColorStripView {
    pub hex: String,
    pub label: &'static str,
    pub values: [String; 3],
    pub intensity: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorStripCorner {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ColorStripPlacement {
    pub origin: [f32; 2],
    pub corner: ColorStripCorner,
}
impl ColorStripPlacement {
    pub fn new(area: [f32; 4], size: [f32; 2], scale: f32, avoid: &[[f32; 2]], corner: ColorStripCorner) -> Self {
        use ColorStripCorner::*;
        let [margin, clearance] = [12. * scale, 8. * scale];
        let [left, top] = [area[0] + margin, area[1] + margin];
        let [right, bottom] = [(area[0] + area[2] - size[0] - margin).max(left), (area[1] + area[3] - size[1] - margin).max(top)];
        let origin = |corner| match corner { TopRight => [right, top], TopLeft => [left, top], BottomRight => [right, bottom], BottomLeft => [left, bottom] };
        let free = |corner| {
            let [x, y] = origin(corner);
            !avoid.iter().any(|&[px, py]| (x - clearance..=x + size[0] + clearance).contains(&px) && (y - clearance..=y + size[1] + clearance).contains(&py))
        };
        let corner = [corner, TopRight, TopLeft, BottomRight, BottomLeft].into_iter().find(|&corner| free(corner)).unwrap_or(corner);
        Self { origin: origin(corner), corner }
    }
}

const SHAPE_CHOICES: [(ColorShape, &str, crate::MessageId); 3] = [
    (ColorShape::Circle, "OKLCH", Id::NATIVE_COLOR_USE_CIRCLE),
    (ColorShape::Square, "HSB", Id::NATIVE_COLOR_USE_SQUARE),
    (ColorShape::Triangle, "HLS", Id::NATIVE_COLOR_USE_TRIANGLE),
];

impl ColorEditor {
    pub fn for_slot(colors: &ColorState, slot: ColorSlot) -> Result<Self, ColorEditorError> {
        if slot == ColorSlot::Transparent {
            return Err("Choose foreground or background".into());
        }
        let mut picker = colors.clone();
        picker.apply(ColorAction::Select { slot })?;
        Ok(Self::with_picker(picker, false))
    }
    pub fn for_color(colors: &ColorState, color: RgbColor, opaque: bool) -> Result<Self, ColorEditorError> {
        let mut picker = colors.clone();
        picker.apply(ColorAction::Select { slot: ColorSlot::Foreground })?;
        let mut color = color;
        if opaque {
            color.rgba[3] = 1.;
        }
        picker.set_color(color)?;
        Ok(Self::with_picker(picker, opaque))
    }
    fn with_picker(picker: ColorState, opaque: bool) -> Self {
        Self { start: picker.clone(), picker, opaque, scrub: None }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.picker.validate()?;
        self.start.validate()?;
        if self.opaque && self.picker.definition().rgba[3] != 1. {
            return Err("Opaque colors keep full alpha".into());
        }
        Ok(())
    }
    pub fn picker(&self) -> &ColorState {
        &self.picker
    }
    pub fn value(&self) -> RgbColor {
        self.picker.definition()
    }
    pub fn intensity(&self) -> Option<f32> {
        self.picker.hdr_picker.map(|_| self.picker.hdr_intensity())
    }
    pub fn changed(&self) -> bool {
        self.picker.definition() != self.start.definition() || self.picker.hdr_intensity() != self.start.hdr_intensity()
    }
    pub fn memory(&self) -> &ColorEditorMemory {
        &self.picker.editor
    }
    pub fn memory_changed(&self) -> bool {
        self.picker.editor != self.start.editor
    }
    fn space(&self) -> RgbSpace {
        self.picker.rgb_space
    }
    fn base(&self) -> RgbColor {
        self.picker.picker_base()
    }
    fn alpha(&self) -> f32 {
        self.picker.definition().rgba[3]
    }
    fn form(&self, row: usize) -> Result<ColorForm, ColorEditorError> {
        self.picker.editor.forms.get(row).copied().ok_or_else(|| "Invalid color row".into())
    }
    fn row_values(&self, row: usize) -> Result<[f64; 3], ColorEditorError> {
        Ok(self.form(row)?.values(self.base(), self.space(), Some(&self.picker))?)
    }
    pub fn apply(&mut self, action: ColorEditorAction) -> Result<(), ColorEditorError> {
        let mut next = self.clone();
        next.apply_action(action)?;
        *self = next;
        Ok(())
    }
    fn apply_action(&mut self, action: ColorEditorAction) -> Result<(), ColorEditorError> {
        match action {
            ColorEditorAction::Wheel { action } => match action {
                ColorAction::PickWheel { .. } | ColorAction::Shape { .. } | ColorAction::HdrIntensity { .. } => self.picker.apply(action),
                _ => Err("Unsupported color editor action".into()),
            },
            ColorEditorAction::Value { row, index, text } => {
                let form = self.form(row)?;
                let component = *form.components().get(index).ok_or("Invalid color value")?;
                match parse_value(&text, component) {
                    Ok(value) => {
                        let mut values = self.row_values(row)?;
                        values[index] = component.bound(value);
                        self.set_row(form, values)
                    }
                    Err(reason) => match parse_color_text(&text, self.space(), Some(form)) {
                        Ok(color) => self.set_base(color),
                        Err(_) => Err(ColorEditorError::Numeric { name: component.name, reason }),
                    },
                }
            }
            ColorEditorAction::Scrub { target, pixels, speed } => self.scrub(target, pixels, speed),
            ColorEditorAction::EndScrub { cancel } => {
                if let Some(scrub) = self.scrub.take() && cancel {
                    self.picker = scrub.picker;
                }
                Ok(())
            }
            ColorEditorAction::Intensity { text } => {
                let stops = crate::numeric::parse_numeric_text(text.trim().trim_end_matches("EV").trim()).map_err(ColorEditorError::Intensity)?;
                self.picker.apply(ColorAction::HdrIntensity { stops })
            }
            ColorEditorAction::Text { text, row } => {
                let form = row.map(|row| self.form(row)).transpose()?;
                let color = parse_color_text(&text, self.space(), form)?;
                self.set_base(color)
            }
            ColorEditorAction::Form { row, form } => {
                if form.family() != row {
                    return Err("That format belongs to another row".into());
                }
                self.picker.editor.forms[row] = form;
                Ok(())
            }
            ColorEditorAction::Color { mut color } => {
                color.rgba[3] = self.alpha();
                self.picker.set_color(color)?;
                Ok(())
            }
            ColorEditorAction::Revert => {
                let (shape, memory) = (self.picker.shape, self.picker.editor.clone());
                self.picker = self.start.clone();
                self.picker.shape = shape;
                self.picker.editor = memory;
                Ok(())
            }
            ColorEditorAction::Search { text } => {
                if text.len() > MAX_SEARCH {
                    return Err("Search text is too long".into());
                }
                self.picker.editor.search = text;
                Ok(())
            }
        }
    }
    fn scrub(&mut self, target: ColorEditorTarget, pixels: f32, speed: ColorScrubSpeed) -> Result<(), ColorEditorError> {
        if !pixels.is_finite() {
            return Err("Invalid drag distance".into());
        }
        if self.scrub.as_ref().is_none_or(|scrub| scrub.target != target) {
            let values = match target {
                ColorEditorTarget::Value { row, .. } => self.row_values(row)?,
                ColorEditorTarget::Intensity => [f64::from(self.intensity().ok_or("HDR intensity requires an HDR drawing")?), 0., 0.],
            };
            self.scrub = Some(Scrub { target, picker: self.picker.clone(), values });
        }
        let scrub = self.scrub.clone().unwrap();
        self.picker = scrub.picker;
        let steps = (f64::from(pixels) / SCRUB_PIXELS).round() * match speed {
            ColorScrubSpeed::Normal => 1.,
            ColorScrubSpeed::Fast => 10.,
            ColorScrubSpeed::Fine => 0.1,
        };
        match target {
            ColorEditorTarget::Value { row, index } => {
                let form = self.form(row)?;
                let component = *form.components().get(index).ok_or("Invalid color value")?;
                let mut values = scrub.values;
                values[index] = component.bound(values[index] + steps * component.step);
                self.set_row(form, values)
            }
            ColorEditorTarget::Intensity => {
                let stops = (scrub.values[0] + steps * SCRUB_INTENSITY_STEP).clamp(SCRUB_INTENSITY_RANGE.0, SCRUB_INTENSITY_RANGE.1);
                self.picker.apply(ColorAction::HdrIntensity { stops: ((stops * 100.).round() / 100.) as f32 })
            }
        }
    }
    fn set_base(&mut self, mut base: RgbColor) -> Result<(), ColorEditorError> {
        base.rgba[3] = self.alpha();
        ColorState::validate_definition(base)?;
        match self.intensity() {
            Some(stops) => {
                let paint = HdrPaint { base, stops };
                let color = paint.color(self.space())?;
                self.picker.set_color_with_picker(color, Some(paint))?;
            }
            None => self.picker.set_color(base)?,
        }
        Ok(())
    }
    fn set_row(&mut self, form: ColorForm, values: [f64; 3]) -> Result<(), ColorEditorError> {
        self.set_base(form.color(values, self.space(), self.alpha())?)?;
        let index = self.picker.index();
        let hue = values[0] as f32;
        let coordinates = self.picker.coordinates[index].as_mut().ok_or("Missing picker coordinates")?;
        match form {
            ColorForm::Hsb | ColorForm::Hsl => {
                if form == ColorForm::Hsb {
                    coordinates.hsv = values.map(|v| v as f32);
                } else {
                    coordinates.hls = [values[0], values[2], values[1]].map(|v| v as f32);
                }
                coordinates.hsv[0] = hue;
                coordinates.hls[0] = hue;
                self.picker.hues[index] = hue;
            }
            ColorForm::Oklch if values[1] < 1e-6 => coordinates.okhsv[0] = values[2] as f32,
            _ => {}
        }
        Ok(())
    }
    pub fn view(&self, display: RgbSpace, rendition: Option<layer_core::color::hdr::SdrRendition>, localizer: &crate::Localizer) -> Result<ColorEditorView, String> {
        let space = self.space();
        let hdr = self.picker.hdr_picker.is_some();
        let panel = match rendition {
            Some(recipe) if hdr => self.picker.view_mapped(recipe, localizer),
            _ => self.picker.view_in_localized(display, localizer),
        };
        let base = self.base();
        let hex_note = if !base.in_gamut(RgbSpace::Srgb)? {
            Some(ColorHexNote { kind: ColorHexNoteKind::Nearest, text: "≈".into(), tip: Some(localizer.text(Id::NATIVE_COLOR_NEAREST_SRGB)) })
        } else if hdr {
            Some(ColorHexNote { kind: ColorHexNoteKind::Base, text: localizer.text(Id::NATIVE_COLOR_BASE), tip: None })
        } else if space != RgbSpace::Srgb {
            Some(ColorHexNote { kind: ColorHexNoteKind::Srgb, text: "sRGB".into(), tip: None })
        } else {
            None
        };
        let rows = std::array::from_fn(|row| -> Result<ColorRowView, String> {
            let form = self.picker.editor.forms[row];
            let values = form.values(base, space, Some(&self.picker))?;
            let components = form.components();
            Ok(ColorRowView {
                form,
                label: form.label().text(localizer),
                forms: COLOR_FORM_FAMILIES[row].iter().map(|form| ColorFormChoice { form: *form, label: form.label().text(localizer) }).collect(),
                space: (row == 0).then(|| space.name()),
                values: std::array::from_fn(|i| ColorValueView {
                    text: components[i].display(values[i]),
                    edit: components[i].edit(values[i]),
                    name: components[i].name.text(localizer),
                }),
                copy: form.copy(values, space),
            })
        });
        let [a, b, c] = rows;
        Ok(ColorEditorView {
            panel,
            shapes: SHAPE_CHOICES.map(|(shape, label, name)| ColorShapeChoice { shape, label, name: localizer.text(name), selected: self.picker.shape == shape }),
            current: form::mapped_preview(self.start.definition(), space, display, rendition)?,
            new: form::mapped_preview(self.value(), space, display, rendition)?,
            hex: ColorLibrary::hex_preview(base),
            hex_note,
            rows: [a?, b?, c?],
            intensity: self.intensity().map(|stops| ColorValueView {
                text: format!("{stops:+.2} EV"),
                edit: trimmed(f64::from(stops), 2),
                name: localizer.text(Id::NATIVE_COLOR_INTENSITY_EV),
            }),
            value: self.value(),
            current_value: self.start.definition(),
            stops: self.intensity(),
            changed: self.changed(),
            search: self.picker.editor.search.clone(),
        })
    }
    pub fn strip(&self, sample: RgbColor) -> Result<ColorStripView, String> {
        let space = self.space();
        let (base, stops) = if self.picker.hdr_picker.is_some() {
            let paint = HdrPaint::from_color(sample, space)?;
            (paint.base, Some(paint.stops))
        } else {
            (sample, None)
        };
        let (label, form) = match self.picker.shape {
            ColorShape::Circle => ("OKLCH", ColorForm::Oklch),
            ColorShape::Square => ("HSB", ColorForm::Hsb),
            ColorShape::Triangle => ("HLS", ColorForm::Hsl),
        };
        let mut values = form.values(base, space, None)?;
        if form == ColorForm::Hsl {
            values.swap(1, 2);
        }
        let components = form.components();
        Ok(ColorStripView {
            hex: ColorLibrary::hex_preview(base),
            label,
            values: std::array::from_fn(|i| components[if form == ColorForm::Hsl && i > 0 { 3 - i } else { i }].display(values[i])),
            intensity: stops.map(|stops| format!("{stops:+.2} EV")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn english() -> std::sync::Arc<crate::Localizer> { crate::Localizer::shared(crate::UiLanguage::English) }
    fn colors(space: RgbSpace) -> ColorState {
        let mut state = ColorState::default();
        state.set_rgb_space(space).unwrap();
        state
    }
    fn editor(hex: u32) -> ColorEditor {
        let rgb = [16, 8, 0].map(|shift| ((hex >> shift) & 0xff) as f32 / 255.);
        ColorEditor::for_color(&colors(RgbSpace::Srgb), RgbColor::new(RgbSpace::Srgb, [rgb[0], rgb[1], rgb[2], 1.]).unwrap(), false).unwrap()
    }
    fn view(editor: &ColorEditor) -> ColorEditorView { editor.view(RgbSpace::Srgb, None, &english()).unwrap() }
    fn texts(row: &ColorRowView) -> [&str; 3] { [0, 1, 2].map(|i| row.values[i].text.as_str()) }

    #[test]
    fn rows_use_painter_units_and_copy_standard_notations() {
        let view = view(&editor(0x3B7EA1));
        assert_eq!(view.hex, "#3B7EA1");
        assert_eq!(texts(&view.rows[0]), ["59", "126", "161"]);
        assert_eq!(texts(&view.rows[1]), ["201°", "63%", "63%"]);
        assert_eq!(texts(&view.rows[2]), ["56.5%", "0.087", "234°"]);
        assert_eq!(view.rows[0].copy, "rgb(59 126 161)");
        assert_eq!(view.rows[1].copy, "hsb(201 63% 63%)");
        assert!(view.rows[2].copy.starts_with("oklch(56.4"));
        assert_eq!(view.rows[0].space, Some("sRGB"));
        assert!(view.hex_note.is_none());
        let original = editor(0x3B7EA1).value().encoded_in(RgbSpace::Srgb).unwrap();
        for row in &view.rows {
            let pasted = parse_color_text(&row.copy, RgbSpace::Srgb, None).unwrap().encoded_in(RgbSpace::Srgb).unwrap();
            assert!((0..3).all(|c| (pasted[c] - original[c]).abs() <= 1. / 255.), "{}", row.copy);
        }
        assert_eq!(ColorLibrary::hex_preview(parse_color_text(&view.rows[2].copy, RgbSpace::Srgb, None).unwrap()), "#3B7EA1");
    }

    #[test]
    fn untouched_rows_keep_the_exact_definition_and_one_edit_changes_one_channel() {
        let definition = RgbColor::new(RgbSpace::DisplayP3, [0.23137, 0.4941177, 0.631, 1.]).unwrap();
        let mut editor = ColorEditor::for_color(&colors(RgbSpace::DisplayP3), definition, false).unwrap();
        for row in 0..3 {
            for form in COLOR_FORM_FAMILIES[row] {
                editor.apply(ColorEditorAction::Form { row, form: *form }).unwrap();
                assert_eq!(editor.value(), definition);
            }
        }
        editor.apply(ColorEditorAction::Form { row: 0, form: ColorForm::Rgb }).unwrap();
        editor.apply(ColorEditorAction::Value { row: 0, index: 0, text: "100".into() }).unwrap();
        let encoded = editor.value().encoded_in(RgbSpace::DisplayP3).unwrap();
        assert!((encoded[0] - 100. / 255.).abs() < 1e-6);
        assert!((encoded[1] - 0.4941177).abs() < 1e-6 && (encoded[2] - 0.631).abs() < 1e-6);
        assert!(editor.changed());
        editor.apply(ColorEditorAction::Revert).unwrap();
        assert_eq!(editor.value(), definition);
        assert!(!editor.changed());
    }

    #[test]
    fn pasting_a_whole_color_into_a_value_reads_it_in_full_and_refusals_keep_the_draft() {
        let mut editor = editor(0x3B7EA1);
        editor.apply(ColorEditorAction::Value { row: 0, index: 0, text: "rgb(202, 75, 53)".into() }).unwrap();
        assert_eq!(view(&editor).hex, "#CA4B35");
        editor.apply(ColorEditorAction::Value { row: 1, index: 0, text: "120 100 100".into() }).unwrap();
        assert_eq!(view(&editor).hex, "#00FF00");
        let before = editor.clone();
        let refused = editor.apply(ColorEditorAction::Value { row: 0, index: 1, text: "lots".into() }).unwrap_err();
        assert!(matches!(refused, ColorEditorError::Numeric { .. }));
        assert!(refused.message(&english()).contains("Green"));
        assert!(matches!(editor.apply(ColorEditorAction::Text { text: "#12".into(), row: None }), Err(ColorEditorError::ColorSyntax)));
        assert_eq!(editor, before);
        editor.apply(ColorEditorAction::Value { row: 1, index: 1, text: "250%".into() }).unwrap();
        assert_eq!(texts(&view(&editor).rows[1])[1], "100%");
    }

    #[test]
    fn scrubbing_steps_from_the_drag_start_without_drift_and_cancel_restores() {
        let mut editor = editor(0x3B7EA1);
        let target = ColorEditorTarget::Value { row: 0, index: 0 };
        for pixels in [4., 10., 40.] {
            editor.apply(ColorEditorAction::Scrub { target, pixels, speed: ColorScrubSpeed::Normal }).unwrap();
        }
        assert_eq!(texts(&view(&editor).rows[0]), ["79", "126", "161"]);
        editor.apply(ColorEditorAction::Scrub { target, pixels: 40., speed: ColorScrubSpeed::Fast }).unwrap();
        assert_eq!(texts(&view(&editor).rows[0])[0], "255");
        editor.apply(ColorEditorAction::EndScrub { cancel: true }).unwrap();
        assert_eq!(view(&editor).hex, "#3B7EA1");
        let hue = ColorEditorTarget::Value { row: 1, index: 0 };
        editor.apply(ColorEditorAction::Scrub { target: hue, pixels: 400., speed: ColorScrubSpeed::Normal }).unwrap();
        editor.apply(ColorEditorAction::EndScrub { cancel: false }).unwrap();
        assert_eq!(texts(&view(&editor).rows[1])[0], "41°");
    }

    #[test]
    fn neutral_rows_keep_their_typed_hue() {
        let mut editor = editor(0x808080);
        editor.apply(ColorEditorAction::Value { row: 1, index: 0, text: "210".into() }).unwrap();
        assert_eq!(texts(&view(&editor).rows[1])[0], "210°");
        assert_eq!(view(&editor).hex, "#808080");
        editor.apply(ColorEditorAction::Value { row: 2, index: 2, text: "33".into() }).unwrap();
        assert_eq!(texts(&view(&editor).rows[2])[2], "33°");
    }

    #[test]
    fn picking_strip_keeps_a_free_work_area_corner_without_oscillating() {
        use ColorStripCorner::*;
        let place = |area, avoid: &[[f32; 2]], corner| ColorStripPlacement::new(area, [272., 64.], 2., avoid, corner);
        let wide = [100., 50., 800., 600.];
        assert_eq!(place(wide, &[], TopRight), ColorStripPlacement { origin: [604., 74.], corner: TopRight });
        assert_eq!(place(wide, &[[700., 100.]], TopRight), ColorStripPlacement { origin: [124., 74.], corner: TopLeft });
        assert_eq!(place(wide, &[[700., 100.]], TopLeft).corner, TopLeft, "a strip that moved away stays put");
        assert_eq!(place(wide, &[[200., 300.]], TopLeft).corner, TopLeft);
        assert_eq!(place(wide, &[[200., 300.], [200., 100.]], TopLeft).corner, TopRight, "the pointer over the strip moves it too");
        let narrow = [0., 0., 560., 800.];
        let covered = place(narrow, &[[300., 60.]], TopRight);
        assert_eq!(covered, ColorStripPlacement { origin: [264., 712.], corner: BottomRight }, "both top corners overlap the sample");
        assert_eq!(place(narrow, &[[300., 60.]], covered.corner).corner, BottomRight);
        assert_eq!(place([0., 0., 200., 100.], &[[100., 50.]], TopRight).corner, TopRight, "with no free corner the strip stays");
    }

    #[test]
    fn wide_gamut_and_hdr_hex_notes_and_strip_follow_the_shape() {
        let p3 = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 1.]).unwrap();
        let editor = ColorEditor::for_color(&colors(RgbSpace::DisplayP3), p3, false).unwrap();
        let view = view(&editor);
        assert_eq!(view.hex_note.as_ref().map(|n| n.kind), Some(ColorHexNoteKind::Nearest));
        assert_eq!(view.rows[0].space, Some("Display P3"));
        assert!(view.rows[0].copy.starts_with("color(display-p3 1 0 0"));
        let mut hdr = colors(RgbSpace::Srgb);
        hdr.set_document_depth(layer_core::color::SampleDepth::F16).unwrap();
        let bright = RgbColor::from_linear(RgbSpace::Srgb, [2., 1., 0.5, 1.]).unwrap();
        let mut editor = ColorEditor::for_color(&hdr, bright, false).unwrap();
        let view = editor.view(RgbSpace::Srgb, None, &english()).unwrap();
        assert_eq!(view.hex_note.as_ref().map(|n| n.kind), Some(ColorHexNoteKind::Base));
        assert_eq!(view.intensity.as_ref().unwrap().text, "+1.00 EV");
        editor.apply(ColorEditorAction::Text { text: "#FFFFFF".into(), row: None }).unwrap();
        assert_eq!(editor.intensity(), Some(1.));
        assert!((editor.value().linear_in(RgbSpace::Srgb).unwrap()[0] - 2.).abs() < 1e-5);
        let strip = editor.strip(bright).unwrap();
        assert_eq!((strip.label, strip.intensity.as_deref()), ("OKLCH", Some("+1.00 EV")));
        editor.apply(ColorEditorAction::Wheel { action: ColorAction::Shape { shape: ColorShape::Triangle } }).unwrap();
        let strip = editor.strip(RgbColor::new(RgbSpace::Srgb, [0.2313725, 0.4941176, 0.6313725, 1.]).unwrap()).unwrap();
        assert_eq!((strip.label, strip.values.clone()), ("HLS", ["201°".into(), "43%".into(), "46%".into()]));
    }

    #[test]
    fn slots_opaque_targets_and_memory_are_validated() {
        let mut state = colors(RgbSpace::Srgb);
        state.apply(ColorAction::SetSlot { slot: ColorSlot::Background, color: RgbColor::BLACK }).unwrap();
        let editor = ColorEditor::for_slot(&state, ColorSlot::Background).unwrap();
        assert_eq!(editor.value(), RgbColor::BLACK);
        assert!(ColorEditor::for_slot(&state, ColorSlot::Transparent).is_err());
        let half = RgbColor::new(RgbSpace::Srgb, [1., 0., 0., 0.5]).unwrap();
        let mut opaque = ColorEditor::for_color(&state, half, true).unwrap();
        assert_eq!(opaque.value().rgba[3], 1.);
        opaque.apply(ColorEditorAction::Color { color: half }).unwrap();
        assert_eq!(opaque.value().rgba[3], 1.);
        assert!(opaque.apply(ColorEditorAction::Form { row: 0, form: ColorForm::Hsl }).is_err());
        opaque.apply(ColorEditorAction::Form { row: 1, form: ColorForm::Hsl }).unwrap();
        opaque.apply(ColorEditorAction::Search { text: "sand".into() }).unwrap();
        assert!(opaque.memory_changed());
        assert_eq!(opaque.memory().forms[1], ColorForm::Hsl);
        let mut json = serde_json::to_value(&opaque).unwrap();
        json["picker"]["editor"]["forms"][1] = serde_json::json!("oklch");
        let tampered: ColorEditor = serde_json::from_value(json).unwrap();
        assert!(tampered.validate().is_err());
    }
}
