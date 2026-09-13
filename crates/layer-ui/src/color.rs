//! Shared paint colors and custom wheel geometry. Frontends only draw the
//! wheel and deliver coordinates; they do not own color conversion or policy.
use serde::{Deserialize, Serialize};

mod okhsv;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    Hsv,
    Hls,
}
/// Field projection is independent of the readout's color model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorShape {
    #[default]
    Circle,
    Square,
    Triangle,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorReadout {
    Hsb,
    #[default]
    #[serde(alias = "lab")]
    Oklch,
    Rgb,
}
impl ColorReadout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hsb => "HSB",
            Self::Oklch => "OKLCH",
            Self::Rgb => "RGB",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Hsb => Self::Oklch,
            Self::Oklch => Self::Rgb,
            Self::Rgb => Self::Hsb,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSlot {
    #[default]
    Foreground,
    Background,
    Transparent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorWheelPart {
    Hue,
    Field,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ColorAction {
    Select {
        slot: ColorSlot,
    },
    Swap,
    ToggleSpace,
    ToggleShape,
    Shape {
        shape: ColorShape,
    },
    ToggleReadout,
    /// Projection-aware picking. The older Pick action retains its square / triangle contract.
    PickWheel {
        part: ColorWheelPart,
        point: [f32; 2],
        size: f32,
    },
    Space {
        space: ColorSpace,
    },
    Component {
        index: usize,
        value: f32,
    },
    RgbaComponent {
        index: usize,
        value: f32,
    },
    Pick {
        part: ColorWheelPart,
        point: [f32; 2],
        size: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorState {
    /// Display-encoded sRGB, not linear canvas pigment.
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    pub slot: ColorSlot,
    pub space: ColorSpace,
    #[serde(default)]
    pub shape: ColorShape,
    #[serde(default)]
    pub readout: ColorReadout,
    paint_slot: ColorSlot,
    hues: [f32; 2],
    // RGB cannot identify a unique point at black, white, or an achromatic hue.
    // Retain both projections, independently for each paint, including across saves.
    #[serde(default)]
    coordinates: [Option<ColorCoordinates>; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorCoordinates {
    rgb: [f32; 3],
    hsv: [f32; 3],
    hls: [f32; 3],
    #[serde(default)]
    okhsv: Option<[f32; 3]>,
}

/// Derived presentation data for native hosts. Geometry is normalized to a
/// unit square; hosts scale it and render gradients without converting colors.
#[derive(Clone, Debug, Serialize)]
pub struct ColorPanelView {
    pub space: ColorSpace,
    pub shape: ColorShape,
    pub other_shapes: [ColorShape; 2],
    pub readout: ColorReadout,
    pub readout_label: &'static str,
    pub readout_text: [String; 3],
    /// Leading spaces reserve fixed digit cells; hosts use tabular advances.
    pub readout_layout_text: [String; 3],
    pub readout_description: String,
    pub wheel_marker: [f32; 2],
    pub wheel_components: [f32; 3],
    pub wheel_hue_color: [f32; 3],
    pub wheel_hue_marker: [f32; 2],
    pub wheel_hue_start_degrees: f32,
    pub geometry: ColorWheelGeometry,
    pub hue_color: [f32; 3],
    pub hue_stops: [[f32; 3]; 7],
    pub hue_start_degrees: f32,
    pub hue_marker: [f32; 2],
    pub field_marker: [f32; 2],
    /// Opaque marker preview of the remembered paint, including transparent mode.
    pub marker_color: [f32; 3],
    pub components: [ColorComponentView; 3],
    pub swatches: [ColorSwatchView; 3],
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ColorHueStop {
    pub offset: f32,
    pub color: [f32; 3],
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorComponentView {
    pub label: &'static str,
    pub name: &'static str,
    pub value: f32,
    pub numeric: crate::NumericControl,
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorSwatchView {
    pub slot: ColorSlot,
    pub label: &'static str,
    pub rgba: [f32; 4],
    pub selected: bool,
}
impl Default for ColorState {
    fn default() -> Self {
        Self {
            foreground: [0.075, 0.075, 0.07, 1.],
            background: [1.; 4],
            slot: ColorSlot::Foreground,
            space: ColorSpace::Hsv,
            shape: ColorShape::Circle,
            readout: ColorReadout::Oklch,
            paint_slot: ColorSlot::Foreground,
            hues: [60., 0.],
            coordinates: [None; 2],
        }
    }
}
impl ColorState {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self
            .foreground
            .iter()
            .chain(&self.background)
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self
                .hues
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=360.0).contains(v))
            || self.paint_slot == ColorSlot::Transparent
        {
            return Err("Invalid workspace colors".into());
        }
        for c in self.coordinates.iter().flatten() {
            for (v, space) in [(c.hsv, ColorSpace::Hsv), (c.hls, ColorSpace::Hls)] {
                if v.iter().enumerate().any(|(i, v)| {
                    !v.is_finite() || !(0.0..=if i == 0 { 360. } else { 100. }).contains(v)
                }) || c
                    .rgb
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || from_components(v, space, 1.)[..3]
                        .iter()
                        .zip(c.rgb)
                        .any(|(a, b)| (a - b).abs() > 0.00002)
                {
                    return Err("Invalid color picker coordinates".into());
                }
            }
            if let Some(v) = c.okhsv {
                if v.iter().enumerate().any(|(i, v)| {
                    !v.is_finite() || !(0.0..=if i == 0 { 360. } else { 100. }).contains(v)
                }) || okhsv::to_rgb(v)
                    .iter()
                    .zip(c.rgb)
                    .any(|(a, b)| (a - b).abs() > 0.00002)
                {
                    return Err("Invalid Okhsv picker coordinates".into());
                }
            }
        }
        Ok(())
    }
    pub fn view(&self) -> ColorPanelView {
        let geometry = ColorWheelGeometry::new(1.).unwrap();
        let components = self.components();
        let names = match self.space {
            ColorSpace::Hsv => ["Hue", "Saturation", "Value"],
            ColorSpace::Hls => ["Hue", "Lightness", "Saturation"],
        };
        ColorPanelView {
            space: self.space,
            shape: self.wheel_shape(),
            other_shapes: self.other_shapes(),
            readout: self.readout,
            readout_label: self.readout_label(),
            readout_text: self.readout_text(),
            readout_layout_text: self.readout_layout_text(),
            readout_description: self.readout_description(),
            wheel_marker: self.wheel_marker(&geometry),
            wheel_components: self.wheel_components(),
            wheel_hue_color: self.wheel_hue_color(self.wheel_components()[0]),
            wheel_hue_marker: self.wheel_hue_marker(&geometry, self.wheel_components()[0]),
            wheel_hue_start_degrees: self.wheel_hue_start_degrees(),
            geometry,
            hue_color: hue_color(components[0]),
            hue_stops: std::array::from_fn(|i| hue_color(i as f32 * 60.)),
            hue_start_degrees: ColorWheelGeometry::HUE_START_DEGREES,
            hue_marker: geometry.hue_marker(components[0]),
            field_marker: self.marker(&geometry),
            marker_color: self.rgba()[..3].try_into().unwrap(),
            components: std::array::from_fn(|i| ColorComponentView {
                label: self.labels()[i],
                name: names[i],
                value: components[i],
                numeric: Self::component_control(i).unwrap(),
            }),
            swatches: [
                (ColorSlot::Foreground, "Foreground color", self.foreground),
                (ColorSlot::Background, "Background color", self.background),
                (ColorSlot::Transparent, "Transparent paint", [0.; 4]),
            ]
            .map(|(slot, label, rgba)| ColorSwatchView {
                slot,
                label,
                rgba,
                selected: self.slot == slot,
            }),
        }
    }
    pub fn rgba(&self) -> [f32; 4] {
        if self.paint_slot == ColorSlot::Background {
            self.background
        } else {
            self.foreground
        }
    }
    pub fn transparent(&self) -> bool {
        self.slot == ColorSlot::Transparent
    }
    fn index(&self) -> usize {
        usize::from(self.paint_slot == ColorSlot::Background)
    }
    pub fn components(&self) -> [f32; 3] {
        self.components_in(self.space)
    }
    fn components_in(&self, space: ColorSpace) -> [f32; 3] {
        if let Some(c) = self.coordinates[self.index()]
            && c.rgb == self.rgba()[..3]
        {
            return if space == ColorSpace::Hsv {
                c.hsv
            } else {
                c.hls
            };
        }
        components(self.rgba(), space, self.hues[self.index()])
    }
    /// Circle controls use Okhsv; the legacy numeric / Pick API stays HSV/HLS.
    pub fn wheel_components(&self) -> [f32; 3] {
        if self.wheel_shape() == ColorShape::Circle {
            self.okhsv_components()
        } else {
            self.components()
        }
    }
    fn okhsv_components(&self) -> [f32; 3] {
        if let Some(c) = self.coordinates[self.index()]
            && c.rgb == self.rgba()[..3]
            && let Some(v) = c.okhsv
        {
            return v;
        }
        // Old documents contain conventional HSV hue memory. Convert the hue
        // anchor, not its numeric angle, when first opening the Okhsv disc.
        let fallback = okhsv::from_rgb(hue_color(self.hues[self.index()]), 0.)[0];
        okhsv::from_rgb(self.rgba()[..3].try_into().unwrap(), fallback)
    }
    pub fn wheel_hue_color(&self, hue: f32) -> [f32; 3] {
        if self.wheel_shape() == ColorShape::Circle {
            okhsv::hue_preview(hue)
        } else {
            hue_color(hue)
        }
    }
    /// Only the Okhsv circle rotates: its RGB blue hue is about 264 degrees,
    /// compared with HSV's 240. Keep legacy host geometry and HSV/HLS unchanged.
    pub fn wheel_hue_start_degrees(&self) -> f32 {
        ColorWheelGeometry::HUE_START_DEGREES
            - if self.wheel_shape() == ColorShape::Circle { 24. } else { 0. }
    }
    pub fn wheel_hue_marker(&self, geometry: &ColorWheelGeometry, hue: f32) -> [f32; 2] {
        geometry.hue_marker(hue + self.wheel_hue_start_degrees() - ColorWheelGeometry::HUE_START_DEGREES)
    }
    pub fn wheel_hue_at(&self, geometry: &ColorWheelGeometry, point: [f32; 2]) -> f32 {
        (geometry.hue_at(point) - self.wheel_hue_start_degrees() + ColorWheelGeometry::HUE_START_DEGREES).rem_euclid(360.)
    }
    pub fn wheel_hue_stops(&self) -> &'static [ColorHueStop] {
        static OKHSV: std::sync::LazyLock<Vec<ColorHueStop>> =
            std::sync::LazyLock::new(okhsv::hue_stops);
        static HSV: std::sync::LazyLock<Vec<ColorHueStop>> = std::sync::LazyLock::new(|| {
            (0..=6)
                .map(|i| ColorHueStop {
                    offset: i as f32 / 6.,
                    color: hue_color(i as f32 * 60.),
                })
                .collect()
        });
        if self.wheel_shape() == ColorShape::Circle {
            &OKHSV
        } else {
            &HSV
        }
    }
    pub fn labels(&self) -> [&'static str; 3] {
        match self.space {
            ColorSpace::Hsv => ["H", "S", "V"],
            ColorSpace::Hls => ["H", "L", "S"],
        }
    }
    pub fn component_control(index: usize) -> Option<crate::NumericControl> {
        (index < 3).then(|| {
            crate::NumericControl::number(0.0, if index == 0 { 360.0 } else { 100.0 }, 1.0, 0)
        })
    }
    pub fn set_rgba(&mut self, rgba: [f32; 4]) -> Result<(), String> {
        if !rgba
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        {
            return Err("Color components must be between 0 and 1".into());
        }
        let index = self.index();
        let old_hsv = self.components_in(ColorSpace::Hsv);
        let old_hls = self.components_in(ColorSpace::Hls);
        let old_okhsv = self.okhsv_components();
        let same_rgb = rgba[..3] == self.rgba()[..3];
        let mut hsv = if same_rgb {
            old_hsv
        } else {
            components(rgba, ColorSpace::Hsv, old_hsv[0])
        };
        let mut hls = if same_rgb {
            old_hls
        } else {
            components(rgba, ColorSpace::Hls, old_hls[0])
        };
        let mut okhsv = if same_rgb {
            old_okhsv
        } else {
            okhsv::from_rgb(rgba[..3].try_into().unwrap(), old_okhsv[0])
        };
        if okhsv[2] == 0. {
            okhsv[1] = old_okhsv[1];
        }
        if hsv[2] == 0. {
            hsv[1] = old_hsv[1];
        }
        if hls[1] == 0. || hls[1] == 100. {
            hls[2] = old_hls[2];
        }
        self.hues[index] = hsv[0];
        self.coordinates[index] = Some(ColorCoordinates {
            rgb: rgba[..3].try_into().unwrap(),
            hsv,
            hls,
            okhsv: Some(okhsv),
        });
        if self.paint_slot == ColorSlot::Background {
            self.background = rgba;
        } else {
            self.foreground = rgba;
        }
        self.slot = self.paint_slot;
        Ok(())
    }
    fn set_components(&mut self, mut values: [f32; 3]) -> Result<(), String> {
        values[0] = values[0].rem_euclid(360.);
        self.set_rgba(from_components(values, self.space, self.rgba()[3]))?;
        let index = self.index();
        let c = self.coordinates[index].as_mut().unwrap();
        if self.space == ColorSpace::Hsv {
            c.hsv = values;
        } else {
            c.hls = values;
        }
        // Conversion noise and powerless hues must not move the hue marker.
        c.hsv[0] = values[0];
        c.hls[0] = values[0];
        self.hues[index] = values[0];
        Ok(())
    }
    fn set_okhsv(&mut self, mut values: [f32; 3]) -> Result<(), String> {
        values[0] = values[0].rem_euclid(360.);
        let [r, g, b] = okhsv::to_rgb(values);
        self.set_rgba([r, g, b, self.rgba()[3]])?;
        let index = self.index();
        let coordinates = self.coordinates[index].as_mut().unwrap();
        coordinates.okhsv = Some(values);
        if values[1] == 0. || values[2] == 0. {
            // Gray/black cannot carry hue in RGB. Keep the ordinary HSB/HLS
            // readout in step with explicit hue changes made on the Okhsv ring.
            let [r, g, b] = okhsv::to_rgb([values[0], 100., 100.]);
            let hue = components([r, g, b, 1.], ColorSpace::Hsv, self.hues[index])[0];
            coordinates.hsv[0] = hue;
            coordinates.hls[0] = hue;
            self.hues[index] = hue;
        }
        Ok(())
    }
    pub fn apply(&mut self, action: ColorAction) -> Result<(), String> {
        match action {
            ColorAction::ToggleReadout => self.readout = self.readout.next(),
            ColorAction::ToggleShape => self.apply(ColorAction::Shape {
                shape: match self.wheel_shape() {
                    ColorShape::Circle => ColorShape::Square,
                    ColorShape::Square => ColorShape::Triangle,
                    ColorShape::Triangle => ColorShape::Circle,
                },
            })?,
            ColorAction::Shape { shape } => {
                self.readout = if shape == ColorShape::Circle {
                    ColorReadout::Oklch
                } else {
                    ColorReadout::Hsb
                };
                self.shape = shape;
                self.space = if shape == ColorShape::Triangle {
                    ColorSpace::Hls
                } else {
                    ColorSpace::Hsv
                };
            }
            ColorAction::PickWheel { part, point, size } => {
                let g = ColorWheelGeometry::new(size).ok_or("Invalid color wheel size")?;
                if !point.into_iter().all(f32::is_finite) {
                    return Err("Invalid color wheel position".into());
                }
                if self.wheel_shape() == ColorShape::Circle {
                    let mut values = self.okhsv_components();
                    match part {
                        ColorWheelPart::Hue => values[0] = self.wheel_hue_at(&g, point),
                        ColorWheelPart::Field => {
                            let [s, v] = g.disc_components(point);
                            values[1] = s * 100.;
                            values[2] = v * 100.;
                        }
                    }
                    self.set_okhsv(values)?;
                } else {
                    self.apply(ColorAction::Pick { part, point, size })?;
                }
            }
            ColorAction::ToggleSpace => {
                self.space = match self.space {
                    ColorSpace::Hsv => ColorSpace::Hls,
                    ColorSpace::Hls => ColorSpace::Hsv,
                }
            }
            ColorAction::Select { slot } => {
                self.slot = slot;
                if slot != ColorSlot::Transparent {
                    self.paint_slot = slot;
                }
            }
            ColorAction::Swap => {
                std::mem::swap(&mut self.foreground, &mut self.background);
                self.hues.swap(0, 1);
                self.coordinates.swap(0, 1);
            }
            ColorAction::Space { space } => self.space = space,
            ColorAction::RgbaComponent { index, value } => {
                if index > 3 || !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err("Invalid RGBA component".into());
                }
                let mut rgba = self.rgba();
                rgba[index] = value;
                self.set_rgba(rgba)?;
            }
            ColorAction::Component { index, value } => {
                if index > 2
                    || !value.is_finite()
                    || !(0.0..=if index == 0 { 360. } else { 100. }).contains(&value)
                {
                    return Err("Invalid color component".into());
                }
                let mut values = self.components();
                values[index] = value;
                self.set_components(values)?;
            }
            ColorAction::Pick { part, point, size } => {
                let geometry = ColorWheelGeometry::new(size).ok_or("Invalid color wheel size")?;
                if !point.into_iter().all(f32::is_finite) {
                    return Err("Invalid color wheel position".into());
                }
                match part {
                    ColorWheelPart::Hue => {
                        let h = geometry.hue_at(point);
                        self.apply(ColorAction::Component { index: 0, value: h })?;
                    }
                    ColorWheelPart::Field => {
                        let mut values = self.components();
                        match self.space {
                            ColorSpace::Hsv => {
                                let [x, y, side] = geometry.square;
                                values[1] = ((point[0] - x) / side).clamp(0., 1.) * 100.;
                                values[2] = (1. - (point[1] - y) / side).clamp(0., 1.) * 100.;
                                self.set_components(values)?;
                            }
                            ColorSpace::Hls => {
                                let weights = triangle_weights(geometry.triangle, point);
                                let hue = hue_color(values[0]);
                                let mut rgba = self.rgba();
                                for c in 0..3 {
                                    rgba[c] = weights[0] + weights[2] * hue[c];
                                }
                                self.set_rgba(rgba)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    /// Older hosts still use `space` to choose between square and triangle.
    /// An old HLS workspace therefore opens as a triangle even without a saved shape.
    pub fn wheel_shape(&self) -> ColorShape {
        if self.space == ColorSpace::Hls {
            ColorShape::Triangle
        } else if self.shape == ColorShape::Square {
            ColorShape::Square
        } else {
            ColorShape::Circle
        }
    }
    pub fn other_shapes(&self) -> [ColorShape; 2] {
        match self.wheel_shape() {
            ColorShape::Circle => [ColorShape::Square, ColorShape::Triangle],
            ColorShape::Square => [ColorShape::Circle, ColorShape::Triangle],
            ColorShape::Triangle => [ColorShape::Circle, ColorShape::Square],
        }
    }
    pub fn readout_label(&self) -> &'static str {
        if self.readout == ColorReadout::Hsb {
            match self.wheel_shape() {
                ColorShape::Circle | ColorShape::Square => "HSB",
                ColorShape::Triangle => "HLS",
            }
        } else {
            self.readout.label()
        }
    }
    pub fn wheel_marker(&self, g: &ColorWheelGeometry) -> [f32; 2] {
        if self.wheel_shape() == ColorShape::Circle {
            let [_, s, v] = self.wheel_components();
            g.disc_marker([s / 100., v / 100.])
        } else {
            self.marker(g)
        }
    }
    pub fn readout_values(&self) -> [f32; 3] {
        match self.readout {
            // HSB remains ordinary HSV/HLS, independent of the Okhsv projection.
            ColorReadout::Hsb => self.components(),
            ColorReadout::Rgb => self.rgba()[..3]
                .try_into()
                .map(|c: [f32; 3]| c.map(|v| v * 255.))
                .unwrap(),
            ColorReadout::Oklch => okhsv::to_oklch(
                self.rgba()[..3].try_into().unwrap(), self.okhsv_components()[0]),
        }
    }
    pub fn readout_text(&self) -> [String; 3] {
        let values = self.readout_values();
        let v = values.map(|v| v.round() as i32);
        match self.readout {
            ColorReadout::Hsb => [
                format!("{}°", v[0] % 360),
                format!("{}%", v[1]),
                format!("{}%", v[2]),
            ],
            ColorReadout::Oklch => [
                format!("{}%", v[0]),
                format!("{:.3}", values[1]),
                format!("{}°", v[2] % 360),
            ],
            ColorReadout::Rgb => v.map(|v| v.to_string()),
        }
    }
    pub fn readout_description(&self) -> String {
        let v = self.readout_text();
        let names = match self.readout {
            ColorReadout::Hsb if self.wheel_shape() == ColorShape::Triangle => {
                ["Hue", "Lightness", "Saturation"]
            }
            ColorReadout::Hsb => ["Hue", "Saturation", "Brightness"],
            ColorReadout::Oklch => ["Lightness", "Chroma", "Hue"],
            ColorReadout::Rgb => ["Red", "Green", "Blue"],
        };
        format!(
            "{}: {} {}, {} {}, {} {}. Switch to {}",
            self.readout_label(),
            names[0],
            v[0],
            names[1],
            v[1],
            names[2],
            v[2],
            if self.readout.next() == ColorReadout::Hsb {
                match self.wheel_shape() {
                    ColorShape::Circle | ColorShape::Square => "HSB",
                    ColorShape::Triangle => "HLS",
                }
            } else {
                self.readout.next().label()
            }
        )
    }
    pub fn readout_layout_text(&self) -> [String; 3] {
        let widths = match self.readout {
            ColorReadout::Hsb => [4; 3],
            ColorReadout::Rgb => [3; 3],
            ColorReadout::Oklch => [4, 5, 4],
        };
        let texts = self.readout_text();
        std::array::from_fn(|i| format!("{:>width$}", texts[i], width = widths[i]))
    }
    pub fn marker(&self, geometry: &ColorWheelGeometry) -> [f32; 2] {
        let [_, a, b] = self.components();
        match self.space {
            ColorSpace::Hsv => [
                geometry.square[0] + a / 100. * geometry.square[2],
                geometry.square[1] + (1. - b / 100.) * geometry.square[2],
            ],
            ColorSpace::Hls => {
                let light = a / 100.;
                let chroma = b / 100. * (1. - (2. * light - 1.).abs());
                let white = light - chroma * 0.5;
                let weights = [white, 1. - white - chroma, chroma];
                std::array::from_fn(|axis| {
                    (0..3)
                        .map(|i| weights[i] * geometry.triangle[i][axis])
                        .sum()
                })
            }
        }
    }
}

/// Hue in degrees, remaining components in percent; alpha is independent.
fn components(rgba: [f32; 4], space: ColorSpace, previous_hue: f32) -> [f32; 3] {
    let [r, g, b, _] = rgba;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        previous_hue
    } else if max == r {
        60. * ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / d + 2.)
    } else {
        60. * ((r - g) / d + 4.)
    };
    match space {
        ColorSpace::Hsv => [h, if max > 0. { d / max * 100. } else { 0. }, max * 100.],
        ColorSpace::Hls => {
            let light = (max + min) * 0.5;
            [
                h,
                light * 100.,
                if d > 1e-6 {
                    d / (1. - (2. * light - 1.).abs()) * 100.
                } else {
                    0.
                },
            ]
        }
    }
}
fn from_components([h, a, b]: [f32; 3], space: ColorSpace, alpha: f32) -> [f32; 4] {
    let (chroma, offset) = match space {
        ColorSpace::Hsv => {
            let c = a / 100. * b / 100.;
            (c, b / 100. - c)
        }
        ColorSpace::Hls => {
            let c = (1. - (2. * a / 100. - 1.).abs()) * b / 100.;
            (c, a / 100. - c * 0.5)
        }
    };
    let hue = hue_color(h);
    [
        hue[0] * chroma + offset,
        hue[1] * chroma + offset,
        hue[2] * chroma + offset,
        alpha,
    ]
    .map(|v| v.clamp(0., 1.))
}
pub fn hue_color(hue: f32) -> [f32; 3] {
    let h = hue.rem_euclid(360.) / 60.;
    let x = 1. - (h.rem_euclid(2.) - 1.).abs();
    match h as u32 {
        0 => [1., x, 0.],
        1 => [x, 1., 0.],
        2 => [0., 1., x],
        3 => [0., x, 1.],
        4 => [x, 0., 1.],
        _ => [1., 0., x],
    }
}

/// Display-encoded RGBA8 for the HLS field, at physical pixel centers. Hosts
/// cache this by hue and pixel size; markers and the hue ring stay independent.
/// Transparent pixels outside the triangle have zero RGB. No allocation occurs.
pub fn render_hls_field(side: u32, hue: f32, rgba: &mut [u8]) -> bool {
    let Some(length) = (side as usize)
        .checked_mul(side as usize)
        .and_then(|n| n.checked_mul(4))
    else {
        return false;
    };
    if side == 0 || !hue.is_finite() || rgba.len() != length {
        return false;
    }
    rgba.fill(0);
    let scale = side as f32;
    let triangle = ColorWheelGeometry::new(scale).unwrap().triangle;
    let color = hue_color(hue);
    let left = triangle[0][0].floor() as u32;
    let right = triangle[2][0].ceil().min(scale) as u32;
    let top = triangle[0][1].floor() as u32;
    let bottom = triangle[1][1].ceil().min(scale) as u32;
    for y in top..bottom {
        for x in left..right {
            let weights = barycentric(triangle, [x as f32 + 0.5, y as f32 + 0.5]);
            if weights.into_iter().any(|w| w < -1e-5) {
                continue;
            }
            let pixel = &mut rgba[((y as usize * side as usize + x as usize) * 4)..][..4];
            for channel in 0..3 {
                pixel[channel] = ((weights[0] + weights[2] * color[channel]) * 255.)
                    .round()
                    .clamp(0., 255.) as u8;
            }
            pixel[3] = 255;
        }
    }
    true
}

/// Opaque sRGB pixels for the Okhsv disc. The host clips the smooth circle edge
/// and caches by hue and physical size. A two-pixel apron extends to the rim;
/// pixels farther outside the host clip are opaque black.
pub fn render_okhsv_disc(side: u32, hue: f32, rgba: &mut [u8]) -> bool {
    if side == 0
        || !hue.is_finite()
        || (side as usize)
            .checked_mul(side as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(rgba.len())
    {
        return false;
    }
    let g = ColorWheelGeometry::new(side as f32).unwrap();
    let hue = okhsv::RasterHue::new(hue);
    for p in rgba.chunks_exact_mut(4) {
        p.copy_from_slice(&[0, 0, 0, 255]);
    }
    let radius = g.disc_radius() + 2.;
    let start = (g.center[0] - radius).floor().max(0.) as u32;
    let end = (g.center[0] + radius).ceil().min(side as f32) as u32;
    for y in start..end {
        for x in start..end {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            if (point[0] - g.center[0]).powi(2) + (point[1] - g.center[1]).powi(2) > radius * radius
            {
                continue;
            }
            let [s, v] = g.disc_components(point);
            let [r, g, b] = hue.rgb8(s, v);
            let offset = (y as usize * side as usize + x as usize) * 4;
            rgba[offset..offset + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    true
}

/// A square color panel, down to four tiles (128px after content insets).
/// Arrays are x/y/width/height in host logical pixels. Only the paint pair overlaps.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ColorPanelLayout {
    pub wheel: [f32; 4],
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    pub transparent: [f32; 4],
    pub swap: [f32; 4],
    pub shapes: [[f32; 4]; 2],
    pub shape_rotations: [f32; 2],
    pub readout: [f32; 4],
    pub readout_radius: f32,
}
impl ColorPanelLayout {
    pub fn new(size: f32) -> Option<Self> {
        if !size.is_finite() || size < 128. {
            return None;
        }
        let inset = 14. + ((176. - size) / 12.).clamp(0., 4.);
        let wheel = [inset, inset, size - 2. * inset, size - 2. * inset];
        let outer = wheel[2] * 0.49;
        let fg = (size * 0.17).round().clamp(30., 44.);
        let bg = (fg * 0.8).round();
        let background = [(fg * 0.54).round(), size - bg, bg, bg];
        let c = size * 0.5;
        let distance = (background[0] + bg * 0.5 - c).hypot(background[1] + bg * 0.5 - c);
        let transparent = c + distance / std::f32::consts::SQRT_2 - bg * 0.5;
        let swap = (size * 0.085).round().clamp(20., 24.);
        let shape = (size * 0.1).round().clamp(24., 28.);
        let angles = [-57_f32, -33.];
        Some(Self {
            wheel,
            foreground: [0., (size - fg - bg * 0.26).round(), fg, fg],
            background,
            transparent: [transparent.round(), transparent.round(), bg, bg],
            swap: [background[0] + bg + 2., size - swap, swap, swap],
            shapes: angles.map(|angle| {
                let r = outer + shape * 0.5 + 0.5;
                let a = angle.to_radians();
                [
                    (c + r * a.cos() - shape * 0.5).round(),
                    (c + r * a.sin() - shape * 0.5).round(),
                    shape,
                    shape,
                ]
            }),
            shape_rotations: angles.map(|a| a + 90.),
            readout: [0., 0., c.round(), c.round()],
            readout_radius: (size - 28.) * 0.49 + 6.,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ColorWheelGeometry {
    pub center: [f32; 2],
    pub outer: f32,
    pub inner: f32,
    pub square: [f32; 3],
    /// White, black, pure hue. The triangle does not rotate with hue.
    pub triangle: [[f32; 2]; 3],
}
impl ColorWheelGeometry {
    pub const HUE_START_DEGREES: f32 = -150.;
    pub fn new(size: f32) -> Option<Self> {
        if !size.is_finite() || size < 1. {
            return None;
        }
        let c = size * 0.5;
        let r = size * 0.38 * 0.94;
        let half = r / std::f32::consts::SQRT_2;
        Some(Self {
            center: [c; 2],
            outer: size * 0.49,
            inner: size * 0.38,
            square: [c - half, c - half, half * 2.],
            triangle: [
                [c - r * 0.5, c - r * 3_f32.sqrt() * 0.5],
                [c - r * 0.5, c + r * 3_f32.sqrt() * 0.5],
                [c + r, c],
            ],
        })
    }
    pub fn disc_radius(&self) -> f32 {
        self.inner * 0.94
    }
    /// Smooth elliptical square-to-disc map. Full S/V range, including corners.
    /// https://arxiv.org/abs/1509.06344 (elliptical grid mapping)
    pub fn disc_marker(&self, [s, v]: [f32; 2]) -> [f32; 2] {
        let (a, b) = (2. * s - 1., 1. - 2. * v);
        [
            self.center[0] + self.disc_radius() * a * (1. - b * b * 0.5).sqrt(),
            self.center[1] + self.disc_radius() * b * (1. - a * a * 0.5).sqrt(),
        ]
    }
    /// The analytical inverse uses f64 to avoid cancellation near rim corners.
    /// Outside drags clamp to the rim without losing pointer capture.
    pub fn disc_components(&self, point: [f32; 2]) -> [f32; 2] {
        let mut x = (point[0] as f64 - self.center[0] as f64) / self.disc_radius() as f64;
        let mut y = (point[1] as f64 - self.center[1] as f64) / self.disc_radius() as f64;
        let radius_squared = x * x + y * y;
        if radius_squared > (1_f64 - 2e-7).powi(2) {
            let r = radius_squared.sqrt();
            x /= r;
            y /= r;
        }
        let t = x * x - y * y;
        let k = 2. * std::f64::consts::SQRT_2;
        let sqrt = |v: f64| v.max(0.).sqrt();
        let a = (sqrt(2. + t + k * x) - sqrt(2. + t - k * x)) * 0.5;
        let b = (sqrt(2. - t + k * y) - sqrt(2. - t - k * y)) * 0.5;
        [
            ((a + 1.) * 0.5).clamp(0., 1.) as f32,
            ((1. - b) * 0.5).clamp(0., 1.) as f32,
        ]
    }
    pub fn hue_at(&self, point: [f32; 2]) -> f32 {
        ((point[1] - self.center[1])
            .atan2(point[0] - self.center[0])
            .to_degrees()
            - Self::HUE_START_DEGREES)
            .rem_euclid(360.)
    }
    pub fn hit_shape(&self, point: [f32; 2], shape: ColorShape) -> Option<ColorWheelPart> {
        if !point.into_iter().all(f32::is_finite) {
            return None;
        }
        if shape != ColorShape::Circle {
            return self.hit(
                point,
                if shape == ColorShape::Square {
                    ColorSpace::Hsv
                } else {
                    ColorSpace::Hls
                },
            );
        }
        let r = (point[0] - self.center[0]).hypot(point[1] - self.center[1]);
        if (self.inner..=self.outer).contains(&r) {
            Some(ColorWheelPart::Hue)
        } else if r <= self.disc_radius() {
            Some(ColorWheelPart::Field)
        } else {
            None
        }
    }
    pub fn hue_marker(&self, hue: f32) -> [f32; 2] {
        let angle = (hue + Self::HUE_START_DEGREES).to_radians();
        let r = (self.outer + self.inner) * 0.5;
        [
            self.center[0] + r * angle.cos(),
            self.center[1] + r * angle.sin(),
        ]
    }
    pub fn hit(&self, point: [f32; 2], space: ColorSpace) -> Option<ColorWheelPart> {
        if !point.into_iter().all(f32::is_finite) {
            return None;
        }
        let radius = (point[0] - self.center[0]).hypot(point[1] - self.center[1]);
        if (self.inner..=self.outer).contains(&radius) {
            return Some(ColorWheelPart::Hue);
        }
        let inside = match space {
            ColorSpace::Hsv => {
                let [x, y, side] = self.square;
                (x..=x + side).contains(&point[0]) && (y..=y + side).contains(&point[1])
            }
            ColorSpace::Hls => barycentric(self.triangle, point)
                .into_iter()
                .all(|v| v >= -1e-5),
        };
        inside.then_some(ColorWheelPart::Field)
    }
}
fn barycentric([a, b, c]: [[f32; 2]; 3], p: [f32; 2]) -> [f32; 3] {
    let cross = |u: [f32; 2], v: [f32; 2]| u[0] * v[1] - u[1] * v[0];
    let sub = |u: [f32; 2], v: [f32; 2]| [u[0] - v[0], u[1] - v[1]];
    let area = cross(sub(b, a), sub(c, a));
    let w = cross(sub(b, p), sub(c, p)) / area;
    let k = cross(sub(c, p), sub(a, p)) / area;
    [w, k, 1. - w - k]
}
fn triangle_weights(triangle: [[f32; 2]; 3], p: [f32; 2]) -> [f32; 3] {
    let weights = barycentric(triangle, p);
    if weights.into_iter().all(|v| v >= 0.) {
        return weights;
    }
    let mut closest = triangle[0];
    let mut distance = f32::INFINITY;
    for i in 0..3 {
        let a = triangle[i];
        let b = triangle[(i + 1) % 3];
        let d = [b[0] - a[0], b[1] - a[1]];
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]))
            .clamp(0., 1.);
        let q = [a[0] + t * d[0], a[1] + t * d[1]];
        let squared = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
        if squared < distance {
            closest = q;
            distance = squared;
        }
    }
    // Roundoff at corners must not leak values outside the sRGB range.
    let weights = barycentric(triangle, closest).map(|v| v.clamp(0., 1.));
    let total: f32 = weights.iter().sum();
    weights.map(|v| v / total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn circle_rotates_its_guide_and_hits_together_and_reports_ordinary_hsb() {
        let g = ColorWheelGeometry::new(236.).unwrap();
        let mut state = ColorState::default();
        state.readout = ColorReadout::Hsb;
        for rgb in [[1.,0.,0.], [0.,1.,0.], [0.,0.,1.]] {
            state.set_rgba([rgb[0],rgb[1],rgb[2],1.]).unwrap();
            assert_eq!(state.readout_label(), "HSB");
            assert_eq!(state.readout_values(), state.components());
            let model = state.view();
            assert_eq!(model.hue_start_degrees, -150.);
            assert_eq!(model.wheel_hue_start_degrees, -174.);
            let ok_angle = state.wheel_components()[0] + state.wheel_hue_start_degrees();
            let hsv_angle = state.components()[0] + ColorWheelGeometry::HUE_START_DEGREES;
            assert!((ok_angle-hsv_angle).abs() < 7.);
        }
        let blue = state.wheel_hue_marker(&g, state.wheel_components()[0]);
        assert!((blue[0]-g.center[0]).abs() < 0.2 && blue[1] > g.center[1]);
        for hue in (0..360).step_by(3) {
            state.apply(ColorAction::PickWheel { part: ColorWheelPart::Hue,
                point: state.wheel_hue_marker(&g, hue as f32), size: 236. }).unwrap();
            let delta = (state.wheel_components()[0]-hue as f32+180.).rem_euclid(360.)-180.;
            assert!(delta.abs() < 0.001);
        }
        state.set_okhsv([137., 0., 70.]).unwrap();
        let gray = state.rgba();
        let readout = state.readout_values();
        state.set_okhsv([29.23, 0., 70.]).unwrap();
        assert_eq!(state.rgba(), gray);
        assert_ne!(state.readout_values()[0], readout[0]);
        state.validate().unwrap();
    }
    #[test]
    fn readout_reserves_fixed_digit_cells_and_keeps_accessible_text_unpadded() {
        let mut state = ColorState::default();
        state.readout = ColorReadout::Hsb;
        for values in [[9.,9.,9.], [10.,10.,10.], [100.,100.,100.]] {
            state.set_rgba(from_components(values, ColorSpace::Hsv, 1.)).unwrap();
            let display = state.readout_layout_text();
            let cells = display.map(|s| s.chars().map(|c| if c.is_ascii_digit() || c == ' ' {'#'} else {c}).collect::<String>());
            assert_eq!(cells, ["###°", "###%", "###%"]);
            assert!(state.readout_text().iter().all(|s| !s.starts_with(' ')));
        }
    }
    #[test]
    fn oklch_readout_keeps_fixed_cells_and_chroma_precision() {
        let mut state = ColorState::default();
        for rgb in [[0.; 3], [1.; 3], [1., 0., 0.], [0., 0., 1.], [0.2, 0.72, 0.58]] {
            state.set_rgba([rgb[0], rgb[1], rgb[2], 1.]).unwrap();
            let cells = state.readout_layout_text().map(|s| s.chars().map(|c| {
                if c.is_ascii_digit() || c == ' ' { '#' } else { c }
            }).collect::<String>());
            assert_eq!(cells, ["###%", "#.###", "###°"]);
            assert!(state.readout_text().iter().all(|s| !s.starts_with(' ')));
        }
        state.set_rgba([1., 0., 0., 1.]).unwrap();
        assert_eq!(state.readout_text(), ["63%", "0.258", "29°"]);
        assert!(state.readout_description().contains("Chroma 0.258"));
        for value in [0., 70., 100.] {
            state.set_okhsv([9., 0., value]).unwrap();
            state.set_okhsv([100., 0., value]).unwrap();
            assert_eq!(state.readout_text()[2], "100°");
            assert_eq!(state.readout_text()[1], "0.000");
        }
    }
    #[test]
    fn powerless_coordinates_survive_picking_hue_alpha_slots_and_persistence() {
        for shape in [ColorShape::Circle, ColorShape::Square] {
            let mut state = ColorState::default();
            state.apply(ColorAction::Shape { shape }).unwrap();
            let g = ColorWheelGeometry::new(236.).unwrap();
            for saturation in [0.15, 0.85, 0.4] {
                let point = if shape == ColorShape::Circle {
                    g.disc_marker([saturation, 0.])
                } else {
                    [
                        g.square[0] + saturation * g.square[2],
                        g.square[1] + g.square[2],
                    ]
                };
                state
                    .apply(ColorAction::PickWheel {
                        part: ColorWheelPart::Field,
                        point,
                        size: 236.,
                    })
                    .unwrap();
                assert!(state.rgba()[..3].iter().all(|v| *v < 1e-6));
                assert!((state.wheel_components()[1] - saturation * 100.).abs() < 0.001);
                let marker = state.wheel_marker(&g);
                assert!((point[0] - marker[0]).hypot(point[1] - marker[1]) < 0.001);
                state
                    .apply(ColorAction::PickWheel {
                        part: ColorWheelPart::Hue,
                        point: state.wheel_hue_marker(&g, 237.),
                        size: 236.,
                    })
                    .unwrap();
                state
                    .apply(ColorAction::RgbaComponent {
                        index: 3,
                        value: 0.4,
                    })
                    .unwrap();
                assert!((state.wheel_components()[1] - saturation * 100.).abs() < 0.001);
                assert!((state.wheel_components()[0] - 237.).abs() < 0.001);
                state.validate().unwrap();
            }
            let before = state.wheel_components();
            state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
            state
                .apply(ColorAction::Select {
                    slot: ColorSlot::Background,
                })
                .unwrap();
            state
                .apply(ColorAction::Component {
                    index: 0,
                    value: 120.,
                })
                .unwrap();
            state.apply(ColorAction::Swap).unwrap();
            assert_eq!(state.wheel_components(), before);
            state
                .apply(ColorAction::PickWheel {
                    part: ColorWheelPart::Field,
                    point: if shape == ColorShape::Circle {
                        g.disc_marker([0.4, 0.75])
                    } else {
                        [
                            g.square[0] + g.square[2] * 0.4,
                            g.square[1] + g.square[2] * 0.25,
                        ]
                    },
                    size: 236.,
                })
                .unwrap();
            assert!((state.wheel_components()[1] - 40.).abs() < 0.001);
            let expected = if shape == ColorShape::Circle {
                let [r, g, b] = okhsv::to_rgb([237., 40., 75.]);
                [r, g, b, 0.4]
            } else {
                from_components([237., 40., 75.], ColorSpace::Hsv, 0.4)
            };
            for (a, b) in state.rgba().into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn hls_readout_and_powerless_saturation_follow_triangle() {
        let mut state = ColorState::default();
        state
            .apply(ColorAction::Shape {
                shape: ColorShape::Triangle,
            })
            .unwrap();
        for lightness in [0., 100.] {
            for (index, value) in [(0, 275.), (1, lightness), (2, 73.)] {
                state
                    .apply(ColorAction::Component { index, value })
                    .unwrap();
            }
            assert_eq!(state.readout_label(), "HLS");
            assert_eq!(state.readout_values(), [275., lightness, 73.]);
            assert!(state.readout_description().contains("Lightness"));
            state.validate().unwrap();
            state
                .apply(ColorAction::Component {
                    index: 1,
                    value: 50.,
                })
                .unwrap();
            assert_eq!(state.components(), [275., 50., 73.]);
        }
        state
            .apply(ColorAction::Shape {
                shape: ColorShape::Circle,
            })
            .unwrap();
        assert_eq!(state.readout_label(), "OKLCH");
        // Public color replacements must never expose stale coordinates.
        state.foreground = [1., 0., 0., 1.];
        assert_eq!(state.components(), [0., 100., 100.]);
        state.validate().unwrap();
    }
    #[test]
    fn saved_coordinates_are_validated_and_legacy_colors_still_load() {
        let mut state = ColorState::default();
        state
            .apply(ColorAction::Component {
                index: 1,
                value: 80.,
            })
            .unwrap();
        let mut json = serde_json::to_value(&state).unwrap();
        json["coordinates"][0]["hsv"][1] = 101.into();
        assert!(
            serde_json::from_value::<ColorState>(json.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        json["coordinates"][0]["hsv"][1] = 0.into();
        assert!(
            serde_json::from_value::<ColorState>(json.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        json.as_object_mut().unwrap().remove("coordinates");
        serde_json::from_value::<ColorState>(json)
            .unwrap()
            .validate()
            .unwrap();
    }
    #[test]
    fn curved_controls_fit_four_tiles_without_covering_the_hue_ring() {
        for size in 128..=600 {
            let l = ColorPanelLayout::new(size as f32).unwrap();
            let c = size as f32 * 0.5;
            let outer = l.wheel[2] * 0.49;
            for b in [
                l.foreground,
                l.background,
                l.transparent,
                l.swap,
                l.shapes[0],
                l.shapes[1],
            ] {
                assert!(
                    b[0] >= 0.
                        && b[1] >= 0.
                        && b[0] + b[2] <= size as f32
                        && b[1] + b[3] <= size as f32,
                    "{size}: {b:?}"
                );
                let d = (b[0] + b[2] * 0.5 - c).hypot(b[1] + b[3] * 0.5 - c);
                assert!(d - b[2] * 0.5 >= outer - 0.5, "{size}: {b:?} covers ring");
            }
            let [a, b] = l.shapes;
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) >= a[2] - 0.5);
            assert_eq!(l.background[2], l.transparent[2]);
            assert!(l.foreground[2] > l.background[2]);
        }
    }

    #[test]
    fn disc_projection_is_reversible_at_edges_center_and_interior() {
        for size in [1., 100., 236., 472.] {
            let g = ColorWheelGeometry::new(size).unwrap();
            for si in 0..=40 {
                for vi in 0..=40 {
                    let sv = [si as f32 / 40., vi as f32 / 40.];
                    let p = g.disc_marker(sv);
                    let actual = g.disc_components(p);
                    assert!(
                        (sv[0] - actual[0]).abs() < 2e-6 && (sv[1] - actual[1]).abs() < 2e-6,
                        "{sv:?} -> {actual:?}"
                    );
                }
            }
            let mut state = ColorState::default();
            state.set_rgba([0.2, 0.72, 0.58, 0.4]).unwrap();
            let before = state.rgba();
            state
                .apply(ColorAction::PickWheel {
                    part: ColorWheelPart::Field,
                    point: state.wheel_marker(&g),
                    size,
                })
                .unwrap();
            for (a, b) in state.rgba().into_iter().zip(before) {
                assert!((a - b).abs() < 1e-6);
            }
            for p in [[-size, -size], [2. * size, 2. * size], [size * 0.5, -size]] {
                let sv = g.disc_components(p);
                assert!(sv.into_iter().all(|c| (0.0..=1.0).contains(&c)));
                assert_eq!(g.hit_shape(p, ColorShape::Circle), None);
                state
                    .apply(ColorAction::PickWheel {
                        part: ColorWheelPart::Field,
                        point: p,
                        size,
                    })
                    .unwrap();
                state.validate().unwrap();
                assert_eq!(state.rgba()[3], 0.4);
            }
        }
    }
    #[test]
    fn disc_raster_matches_shared_picking() {
        for side in [31, 100, 236] {
            let g = ColorWheelGeometry::new(side as f32).unwrap();
            for hue in [0., 60., 163., 240., 340.] {
                let mut rgba = vec![0; side as usize * side as usize * 4];
                assert!(render_okhsv_disc(side, hue, &mut rgba));
                let mut base = ColorState::default();
                base.apply(ColorAction::PickWheel {
                    part: ColorWheelPart::Hue,
                    point: base.wheel_hue_marker(&g, hue),
                    size: side as f32,
                })
                .unwrap();
                for y in (0..side).step_by(7) {
                    for x in (0..side).step_by(7) {
                        let p = [x as f32 + 0.5, y as f32 + 0.5];
                        if g.hit_shape(p, ColorShape::Circle) != Some(ColorWheelPart::Field) {
                            continue;
                        }
                        let mut state = base.clone();
                        state
                            .apply(ColorAction::PickWheel {
                                part: ColorWheelPart::Field,
                                point: p,
                                size: side as f32,
                            })
                            .unwrap();
                        let expected = state.rgba().map(|c| (c * 255.).round() as u8);
                        let offset = ((y * side + x) * 4) as usize;
                        for (a, b) in rgba[offset..offset + 4].iter().zip(expected) {
                            assert!(a.abs_diff(b) <= 1);
                        }
                    }
                }
            }
        }
        let mut invalid = [17; 4];
        for (size, hue) in [(0, 0.), (2, 0.), (1, f32::NAN)] {
            assert!(!render_okhsv_disc(size, hue, &mut invalid));
            assert_eq!(invalid, [17; 4]);
        }
    }
    #[test]
    fn readout_and_shape_cycles_preserve_paint_and_survive_serialization() {
        let mut state = ColorState::default();
        assert_eq!(state.wheel_shape(), ColorShape::Circle);
        state.set_rgba([0.2, 0.72, 0.58, 0.4]).unwrap();
        let paint = state.rgba();
        for shape in [ColorShape::Square, ColorShape::Triangle, ColorShape::Circle] {
            state.apply(ColorAction::ToggleShape).unwrap();
            assert_eq!(state.wheel_shape(), shape);
            let initial = if shape == ColorShape::Circle { ColorReadout::Oklch } else { ColorReadout::Hsb };
            assert_eq!(state.readout, initial);
            for model in [initial.next(), initial.next().next(), initial] {
                state.apply(ColorAction::ToggleReadout).unwrap();
                assert_eq!(state.readout, model);
                assert_eq!(state.rgba(), paint);
                let saved = serde_json::to_string(&state).unwrap();
                assert_eq!(serde_json::from_str::<ColorState>(&saved).unwrap(), state);
            }
        }
        let mut old = serde_json::to_value(&state).unwrap();
        old.as_object_mut().unwrap().remove("shape");
        old.as_object_mut().unwrap().remove("readout");
        let loaded: ColorState = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(loaded.wheel_shape(), ColorShape::Circle);
        old["readout"] = "lab".into();
        let migrated: ColorState = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(migrated.readout, ColorReadout::Oklch);
        assert_eq!(serde_json::to_value(&migrated).unwrap()["readout"], "oklch");
        old["space"] = "hls".into();
        let loaded: ColorState = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.wheel_shape(), ColorShape::Triangle);
        let before = state.clone();
        assert!(
            state
                .apply(ColorAction::PickWheel {
                    part: ColorWheelPart::Field,
                    point: [f32::NAN, 0.],
                    size: 100.
                })
                .is_err()
        );
        assert_eq!(state, before);
    }
    #[test]
    fn okhsv_circle_keeps_color_when_switching_models_and_loading_old_memory() {
        let g = ColorWheelGeometry::new(236.).unwrap();
        for rgb in [
            [0., 0., 0.],
            [1., 1., 1.],
            [0.5; 3],
            [1., 0., 0.],
            [0., 0., 1.],
            [0.2, 0.72, 0.58],
            [0.01, 0.02, 0.04],
        ] {
            let mut state = ColorState::default();
            state.set_rgba([rgb[0], rgb[1], rgb[2], 0.35]).unwrap();
            let original = state.rgba();
            let circle = state.wheel_components();
            for shape in [ColorShape::Square, ColorShape::Triangle, ColorShape::Circle] {
                state.apply(ColorAction::Shape { shape }).unwrap();
                assert_eq!(state.rgba(), original);
                if shape != ColorShape::Circle {
                    assert_eq!(state.wheel_components(), state.components());
                }
            }
            assert_eq!(state.wheel_components(), circle);
            let mut saved = serde_json::to_value(&state).unwrap();
            saved["coordinates"][0]
                .as_object_mut()
                .unwrap()
                .remove("okhsv");
            let mut loaded: ColorState = serde_json::from_value(saved).unwrap();
            loaded.validate().unwrap();
            assert_eq!(loaded.rgba(), original);
            loaded
                .apply(ColorAction::PickWheel {
                    part: ColorWheelPart::Field,
                    point: loaded.wheel_marker(&g),
                    size: 236.,
                })
                .unwrap();
            for (a, b) in loaded.rgba().into_iter().zip(original) {
                assert!((a - b).abs() < 0.00002);
            }
        }
        // Okhsv has its own hue angles. Legacy host payloads remain ordinary HSV.
        let mut red = ColorState::default();
        red.set_rgba([1., 0., 0., 1.]).unwrap();
        assert_eq!(red.view().components[0].value, 0.);
        assert!((red.view().wheel_components[0] - 29.23).abs() < 0.01);
        assert_eq!(red.view().hue_stops.len(), 7);
        assert!(red.wheel_hue_stops().len() >= 361);
        assert_eq!(red.readout_label(), "OKLCH");
    }
    #[test]
    fn okhsv_memory_is_validated_and_keeps_gray_hue_and_black_saturation() {
        let mut state = ColorState::default();
        state.set_okhsv([263.5, 84., 0.]).unwrap();
        let black_marker = state.wheel_marker(&ColorWheelGeometry::new(236.).unwrap());
        state
            .apply(ColorAction::RgbaComponent {
                index: 3,
                value: 0.2,
            })
            .unwrap();
        state
            .apply(ColorAction::Shape {
                shape: ColorShape::Square,
            })
            .unwrap();
        state
            .apply(ColorAction::Shape {
                shape: ColorShape::Circle,
            })
            .unwrap();
        assert_eq!(
            state.wheel_marker(&ColorWheelGeometry::new(236.).unwrap()),
            black_marker
        );
        state.set_okhsv([137., 0., 70.]).unwrap();
        state.set_rgba([0.5, 0.5, 0.5, 0.2]).unwrap();
        assert_eq!(state.wheel_components()[0], 137.);
        state.validate().unwrap();
        for invalid in [
            [361., 0., 70.],
            [137., 101., 70.],
            [137., 0., 101.],
            [137., 95., 70.],
        ] {
            let mut saved = serde_json::to_value(&state).unwrap();
            saved["coordinates"][0]["okhsv"] = serde_json::json!(invalid);
            assert!(
                serde_json::from_value::<ColorState>(saved)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
    #[test]
    fn oklch_readout_matches_reference_primaries_and_retains_neutral_hue() {
        // Oklab reference primaries, in cylindrical coordinates.
        // https://bottosson.github.io/posts/oklab/#table-of-example-colors
        for (rgb, expected) in [
            ([0., 0., 0.], [0., 0., 137.]),
            ([1., 1., 1.], [100., 0., 137.]),
            ([1., 0., 0.], [62.795536, 0.2576833, 29.233885]),
            ([0., 1., 0.], [86.64396, 0.2948272, 142.49534]),
            ([0., 0., 1.], [45.20137, 0.3132144, 264.05203]),
        ] {
            let mut state = ColorState::default();
            state.set_okhsv([137., 50., 70.]).unwrap();
            state.set_rgba([rgb[0], rgb[1], rgb[2], 1.]).unwrap();
            state.readout = ColorReadout::Oklch;
            for (a, b) in state.readout_values().into_iter().zip(expected) {
                assert!((a - b).abs() < 0.0001, "{rgb:?}: {a} != {b}");
            }
            state
                .apply(ColorAction::Select {
                    slot: ColorSlot::Transparent,
                })
                .unwrap();
            for (a, b) in state.readout_values().into_iter().zip(expected) {
                assert!((a - b).abs() < 0.0001);
            }
        }
    }
    #[test]
    fn hls_raster_matches_picker_at_physical_pixel_centers() {
        for side in [1, 31, 160, 320, 452] {
            for hue in [0., 60., 140.000015, 150., 240., 340., 360.] {
                let mut pixels = vec![17; side as usize * side as usize * 4];
                assert!(render_hls_field(side, hue, &mut pixels));
                let mut state = ColorState {
                    space: ColorSpace::Hls,
                    ..Default::default()
                };
                state
                    .apply(ColorAction::Component {
                        index: 0,
                        value: hue,
                    })
                    .unwrap();
                let geometry = ColorWheelGeometry::new(side as f32).unwrap();
                for y in 0..side {
                    for x in 0..side {
                        let point = [x as f32 + 0.5, y as f32 + 0.5];
                        let pixel = &pixels[((y * side + x) * 4) as usize..][..4];
                        if geometry.hit(point, ColorSpace::Hls) != Some(ColorWheelPart::Field) {
                            assert_eq!(pixel, [0; 4]);
                            continue;
                        }
                        // Avoid accumulating tiny hue conversion errors across picks.
                        let mut picked = state.clone();
                        picked
                            .apply(ColorAction::Pick {
                                part: ColorWheelPart::Field,
                                point,
                                size: side as f32,
                            })
                            .unwrap();
                        assert_eq!(pixel[3], 255, "missing pixel {side}/{hue}/{point:?}");
                        for (actual, expected) in pixel[..3].iter().zip(picked.rgba()) {
                            assert!(
                                (*actual as f32 - expected * 255.).abs() <= 0.501,
                                "{side}/{hue}/{point:?}: {pixel:?} != {:?}",
                                picked.rgba()
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn hls_raster_invalid_requests_preserve_caller_buffer() {
        for (side, hue) in [
            (0, 0.),
            (2, f32::NAN),
            (2, f32::INFINITY),
            (3, 60.),
            (u32::MAX, 0.),
        ] {
            let mut bytes = [19; 16];
            assert!(!render_hls_field(side, hue, &mut bytes));
            assert_eq!(bytes, [19; 16]);
        }
    }
    fn close(a: [f32; 4], b: [f32; 4]) {
        assert!(
            a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5),
            "{a:?} != {b:?}"
        );
    }
    #[test]
    fn rgba_channel_edits_preserve_other_channels_and_reject_invalid_values() {
        let mut state = ColorState::default();
        state.set_rgba([0.2, 0.4, 0.6, 0.8]).unwrap();
        state
            .apply(ColorAction::RgbaComponent {
                index: 0,
                value: 0.3,
            })
            .unwrap();
        state
            .apply(ColorAction::RgbaComponent {
                index: 3,
                value: 0.5,
            })
            .unwrap();
        close(state.rgba(), [0.3, 0.4, 0.6, 0.5]);
        state
            .apply(ColorAction::Select {
                slot: ColorSlot::Transparent,
            })
            .unwrap();
        let before = state.clone();
        for (index, value) in [
            (4, 0.5),
            (0, -0.1),
            (1, 1.1),
            (2, f32::NAN),
            (3, f32::INFINITY),
        ] {
            assert!(
                state
                    .apply(ColorAction::RgbaComponent { index, value })
                    .is_err()
            );
            assert_eq!(state, before);
        }
        state
            .apply(ColorAction::RgbaComponent {
                index: 1,
                value: 0.7,
            })
            .unwrap();
        assert_eq!(state.slot, ColorSlot::Foreground);
        close(state.rgba(), [0.3, 0.7, 0.6, 0.5]);
    }
    #[test]
    fn both_spaces_and_wheel_markers_roundtrip() {
        for space in [ColorSpace::Hsv, ColorSpace::Hls] {
            for r in [0., 0.2, 0.5, 1.] {
                for g in [0., 0.2, 0.5, 1.] {
                    for b in [0., 0.2, 0.5, 1.] {
                        let rgba = [r, g, b, 0.7];
                        close(
                            rgba,
                            from_components(components(rgba, space, 123.), space, 0.7),
                        );
                        let mut state = ColorState {
                            space,
                            ..Default::default()
                        };
                        state.set_rgba(rgba).unwrap();
                        let geometry = ColorWheelGeometry::new(212.).unwrap();
                        let point = state.marker(&geometry);
                        state
                            .apply(ColorAction::Pick {
                                part: ColorWheelPart::Field,
                                point,
                                size: 212.,
                            })
                            .unwrap();
                        close(rgba, state.rgba());
                    }
                }
            }
        }
    }
    #[test]
    fn hue_survives_white_black_and_slot_switches() {
        let mut state = ColorState::default();
        state.set_rgba([0., 0., 1., 1.]).unwrap();
        state.set_rgba([1.; 4]).unwrap();
        assert_eq!(state.components()[0], 240.);
        state
            .apply(ColorAction::Select {
                slot: ColorSlot::Background,
            })
            .unwrap();
        state.set_rgba([0., 1., 0., 1.]).unwrap();
        state.set_rgba([0., 0., 0., 1.]).unwrap();
        assert_eq!(state.components()[0], 120.);
        state
            .apply(ColorAction::Select {
                slot: ColorSlot::Transparent,
            })
            .unwrap();
        assert!(state.transparent());
        state
            .apply(ColorAction::Component {
                index: 2,
                value: 50.,
            })
            .unwrap();
        assert_eq!(state.slot, ColorSlot::Background);
        state.apply(ColorAction::Swap).unwrap();
        assert_eq!(state.components()[0], 240.);
    }
    #[test]
    fn hue_geometry_and_outside_drags_are_bounded() {
        let geometry = ColorWheelGeometry::new(128.).unwrap();
        let mut state = ColorState::default();
        for hue in (0..360).step_by(15) {
            let point = geometry.hue_marker(hue as f32);
            assert_eq!(
                geometry.hit(point, ColorSpace::Hsv),
                Some(ColorWheelPart::Hue)
            );
            state
                .apply(ColorAction::Pick {
                    part: ColorWheelPart::Hue,
                    point,
                    size: 128.,
                })
                .unwrap();
            let delta = (state.components()[0] - hue as f32 + 180.).rem_euclid(360.) - 180.;
            assert!(delta.abs() < 0.001);
        }
        state.space = ColorSpace::Hls;
        for point in [[-100., -100.], [1000., 50.], [64., 500.]] {
            state
                .apply(ColorAction::Pick {
                    part: ColorWheelPart::Field,
                    point,
                    size: 128.,
                })
                .unwrap();
            assert!(state.rgba().into_iter().all(|v| (0.0..=1.0).contains(&v)));
        }
        let before = state.clone();
        assert!(
            state
                .apply(ColorAction::Component {
                    index: 3,
                    value: 0.
                })
                .is_err()
        );
        assert!(
            state
                .apply(ColorAction::Pick {
                    part: ColorWheelPart::Hue,
                    point: [f32::NAN, 0.],
                    size: 128.
                })
                .is_err()
        );
        assert_eq!(state, before);
    }
}
