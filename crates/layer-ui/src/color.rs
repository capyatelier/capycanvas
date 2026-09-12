//! Shared paint colors and custom wheel geometry. Frontends only draw the
//! wheel and deliver coordinates; they do not own color conversion or policy.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    Hsv,
    Hls,
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
    paint_slot: ColorSlot,
    hues: [f32; 2],
}

/// Derived presentation data for native hosts. Geometry is normalized to a
/// unit square; hosts scale it and render gradients without converting colors.
#[derive(Clone, Debug, Serialize)]
pub struct ColorPanelView {
    pub space: ColorSpace,
    pub geometry: ColorWheelGeometry,
    pub hue_color: [f32; 3],
    pub hue_stops: [[f32; 3]; 7],
    pub hue_start_degrees: f32,
    pub hue_marker: [f32; 2],
    pub field_marker: [f32; 2],
    pub components: [ColorComponentView; 3],
    pub swatches: [ColorSwatchView; 3],
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
            paint_slot: ColorSlot::Foreground,
            hues: [60., 0.],
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
            geometry,
            hue_color: hue_color(components[0]),
            hue_stops: std::array::from_fn(|i| hue_color(i as f32 * 60.)),
            hue_start_degrees: ColorWheelGeometry::HUE_START_DEGREES,
            hue_marker: geometry.hue_marker(components[0]),
            field_marker: self.marker(&geometry),
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
        components(self.rgba(), self.space, self.hues[self.index()])
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
        self.hues[index] = components(rgba, ColorSpace::Hsv, self.hues[index])[0];
        if self.paint_slot == ColorSlot::Background {
            self.background = rgba;
        } else {
            self.foreground = rgba;
        }
        self.slot = self.paint_slot;
        Ok(())
    }
    pub fn apply(&mut self, action: ColorAction) -> Result<(), String> {
        match action {
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
                self.hues[self.index()] = values[0].rem_euclid(360.);
                self.set_rgba(from_components(values, self.space, self.rgba()[3]))?;
            }
            ColorAction::Pick { part, point, size } => {
                let geometry = ColorWheelGeometry::new(size).ok_or("Invalid color wheel size")?;
                if !point.into_iter().all(f32::is_finite) {
                    return Err("Invalid color wheel position".into());
                }
                match part {
                    ColorWheelPart::Hue => {
                        let h = ((point[1] - geometry.center[1])
                            .atan2(point[0] - geometry.center[0])
                            .to_degrees()
                            - ColorWheelGeometry::HUE_START_DEGREES)
                            .rem_euclid(360.);
                        self.apply(ColorAction::Component { index: 0, value: h })?;
                    }
                    ColorWheelPart::Field => {
                        let mut values = self.components();
                        match self.space {
                            ColorSpace::Hsv => {
                                let [x, y, side] = geometry.square;
                                values[1] = ((point[0] - x) / side).clamp(0., 1.) * 100.;
                                values[2] = (1. - (point[1] - y) / side).clamp(0., 1.) * 100.;
                                self.set_rgba(from_components(values, self.space, self.rgba()[3]))?;
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
        let r = size * 0.405 * 0.94;
        let half = r / std::f32::consts::SQRT_2;
        Some(Self {
            center: [c; 2],
            outer: size * 0.49,
            inner: size * 0.405,
            square: [c - half, c - half, half * 2.],
            triangle: [
                [c - r * 0.5, c - r * 3_f32.sqrt() * 0.5],
                [c - r * 0.5, c + r * 3_f32.sqrt() * 0.5],
                [c + r, c],
            ],
        })
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
