//! HDR is a linear-light multiplier of the ordinary picker, never its HSV value.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HdrPaint {
    pub base: RgbColor,
    pub stops: f32,
}
impl HdrPaint {
    pub fn from_color(color: RgbColor, space: RgbSpace) -> Result<Self, String> {
        let stops = color.brightness_ev(space)?.unwrap_or(0.).max(0.);
        if stops == 0. {
            return Ok(Self { base: color, stops });
        }
        let mut p = color.linear_in(space)?;
        for v in &mut p[..3] {
            *v /= stops.exp2();
        }
        Ok(Self {
            base: RgbColor::from_linear(space, p)?,
            stops,
        })
    }
    fn color(self, space: RgbSpace) -> Result<RgbColor, String> {
        let mut p = self.base.linear_in(space)?;
        for v in &mut p[..3] {
            *v *= self.stops.exp2();
        }
        layer_core::color::hdr::encode_pixel(p).map_err(str::to_string)?;
        RgbColor::from_linear(space, p)
    }
}
impl ColorState {
    /// Called at document/workspace boundaries; changing mode never alters paint.
    pub fn set_hdr_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if enabled == self.hdr_picker.is_some() {
            return Ok(());
        }
        self.hdr_picker = if enabled {
            Some([
                HdrPaint::from_color(self.foreground, self.rgb_space)?,
                HdrPaint::from_color(self.background, self.rgb_space)?,
            ])
        } else {
            None
        };
        Ok(())
    }
    pub fn hdr_intensity(&self) -> f32 {
        self.hdr_picker.map_or(0., |p| p[self.index()].stops)
    }
    pub fn picker_base(&self) -> RgbColor {
        self.hdr_picker
            .map_or(self.definition(), |p| p[self.index()].base)
    }
    pub(super) fn set_hdr_intensity(&mut self, stops: f32) -> Result<(), String> {
        if !stops.is_finite() || !(-16. ..=65504f32.log2()).contains(&stops) {
            return Err("HDR intensity must be between −16 and +16 EV (half-float limit)".into());
        }
        let mut paint = self
            .hdr_picker
            .ok_or("HDR intensity requires an HDR drawing")?[self.index()];
        paint.stops = stops;
        let color = paint.color(self.rgb_space)?;
        // Base and remembered coordinates are unchanged, including at black.
        self.set_color_with_picker(color, Some(paint))
    }
    pub(super) fn set_picker_rgba(&mut self, rgba: [f32; 4]) -> Result<(), String> {
        if self.hdr_picker.is_none() {
            return self.set_rgba(rgba);
        }
        let paint = HdrPaint {
            base: RgbColor::new(self.rgb_space, rgba)?,
            stops: self.hdr_intensity(),
        };
        let color = paint.color(self.rgb_space)?;
        self.set_color_with_picker(color, Some(paint))
    }
    pub(super) fn validate_hdr_picker(&self) -> Result<(), String> {
        if let Some(paints) = self.hdr_picker {
            for (paint, actual) in paints.into_iter().zip([self.foreground, self.background]) {
                paint.base.validate_working_spaces()?;
                if !paint.stops.is_finite() || !(-16. ..=128.).contains(&paint.stops) {
                    return Err("Invalid HDR picker intensity".into());
                }
                let mut expected = paint.base.linear_in(actual.space)?;
                for v in &mut expected[..3] {
                    *v *= paint.stops.exp2();
                }
                if expected
                    .into_iter()
                    .zip(actual.linear_in(actual.space)?)
                    .any(|(a, b)| !a.is_finite() || (a - b).abs() > b.abs().max(1.) * 2e-5)
                {
                    return Err("HDR picker does not match the paint color".into());
                }
            }
        }
        Ok(())
    }
    /// Float32 document-linear field before display mapping. No 8-bit intermediate.
    /// The host clips the shape using the same geometry as ordinary SDR picking.
    pub fn render_field_linear(&self, side: u32, pixels: &mut [[f32; 4]]) -> bool {
        self.render_field_with_gain(side, pixels, self.hdr_intensity().exp2())
    }
    /// Hosts retain this Float32 base across EV and display-capability changes.
    pub fn render_field_base_linear(&self, side: u32, pixels: &mut [[f32; 4]]) -> bool {
        self.render_field_with_gain(side, pixels, 1.)
    }
    fn render_field_with_gain(&self, side: u32, pixels: &mut [[f32; 4]], gain: f32) -> bool {
        if side == 0 || (side as usize).checked_mul(side as usize) != Some(pixels.len()) {
            return false;
        }
        let g = ColorWheelGeometry::new(side as f32).unwrap();
        let hue = self.wheel_components()[0];
        let okhsv = okhsv::Hue::new_in(self.rgb_space, hue);
        for (i, p) in pixels.iter_mut().enumerate() {
            let point = [
                (i % side as usize) as f32 + 0.5,
                (i / side as usize) as f32 + 0.5,
            ];
            let rgb = match self.wheel_shape() {
                ColorShape::Circle => {
                    let [s, v] = g.disc_components(point);
                    okhsv.linear_rgb(s, v).map(|v| v.clamp(0., 1.) as f32)
                }
                ColorShape::Square => {
                    let [s, v] = g.square_components(point);
                    let rgba = from_components([hue, s * 100., v * 100.], ColorSpace::Hsv, 1.);
                    [rgba[0], rgba[1], rgba[2]].map(|v| self.rgb_space.decode(v as f64) as f32)
                }
                ColorShape::Triangle => {
                    let weights = triangle_weights(g.triangle, point);
                    hue_color(hue)
                        .map(|v| self.rgb_space.decode((weights[0] + weights[2] * v) as f64) as f32)
                }
            };
            *p = [rgb[0] * gain, rgb[1] * gain, rgb[2] * gain, 1.];
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intensity_multiplies_linear_rgb_and_preserves_markers_alpha_and_black() {
        for space in RgbSpace::ALL {
            let mut s = ColorState::default();
            s.set_rgb_space(space).unwrap();
            s.set_hdr_enabled(true).unwrap();
            s.set_rgba([0.3, 0.5, 0.2, 0.37]).unwrap();
            for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
                s.apply(ColorAction::Shape { shape }).unwrap();
                s.set_hdr_intensity(0.).unwrap();
                let base = s.definition().linear_in(space).unwrap();
                let marker = s.wheel_components();
                s.set_hdr_intensity(2.).unwrap();
                assert_eq!(s.wheel_components(), marker);
                let p = s.definition().linear_in(space).unwrap();
                for c in 0..3 {
                    assert!((p[c] - base[c] * 4.).abs() < 2e-6);
                }
                assert_eq!(p[3], 0.37);
                s.validate().unwrap();
            }
            s.set_color(RgbColor::BLACK).unwrap();
            s.set_hdr_intensity(2.).unwrap();
            assert_eq!(s.definition().linear_in(space).unwrap(), [0., 0., 0., 1.]);
            assert_eq!(s.hdr_intensity(), 2.);
            s.set_picker_rgba([1., 1., 1., 0.2]).unwrap();
            let p = s.definition().linear_in(space).unwrap();
            for v in &p[..3] {
                assert!((*v - 4.).abs() < 3e-6);
            }
            s.apply(ColorAction::RgbaComponent {
                index: 3,
                value: 0.5,
            })
            .unwrap();
            assert_eq!(s.hdr_intensity(), 2.);
            assert_eq!(s.definition().rgba[3], 0.5);
            s.validate().unwrap();
        }
    }
    #[test]
    fn all_hdr_fields_show_the_linear_color_they_pick_and_keep_intensity() {
        let side = 67;
        let g = ColorWheelGeometry::new(side as f32).unwrap();
        for space in RgbSpace::ALL {
            let mut s = ColorState::default();
            s.set_rgb_space(space).unwrap();
            s.set_hdr_enabled(true).unwrap();
            s.set_rgba([0.2, 0.7, 0.3, 0.4]).unwrap();
            s.set_hdr_intensity(2.).unwrap();
            for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
                s.apply(ColorAction::Shape { shape }).unwrap();
                let mut pixels = vec![[0.; 4]; side as usize * side as usize];
                assert!(s.render_field_linear(side, &mut pixels));
                for y in (12..side - 12).step_by(5) {
                    for x in (12..side - 12).step_by(5) {
                        let point = [x as f32 + 0.5, y as f32 + 0.5];
                        let inside = match shape {
                            ColorShape::Circle => {
                                (point[0] - g.center[0]).hypot(point[1] - g.center[1])
                                    < g.disc_radius() - 1.
                            }
                            ColorShape::Square => point
                                .iter()
                                .enumerate()
                                .all(|(i, v)| *v > g.square[i] && *v < g.square[i] + g.square[2]),
                            ColorShape::Triangle => {
                                barycentric(g.triangle, point).into_iter().all(|v| v > 0.01)
                            }
                        };
                        if !inside {
                            continue;
                        }
                        let mut picked = s.clone();
                        picked
                            .apply(ColorAction::PickWheel {
                                part: ColorWheelPart::Field,
                                point,
                                size: side as f32,
                            })
                            .unwrap();
                        assert_eq!(picked.hdr_intensity(), 2.);
                        let expected = picked.definition().linear_in(space).unwrap();
                        let actual = pixels[(y * side + x) as usize];
                        for c in 0..3 {
                            assert!(
                                (expected[c] - actual[c]).abs() < 5e-5,
                                "{shape:?} {space:?} {point:?}: {expected:?} != {actual:?}"
                            );
                        }
                        assert_eq!(expected[3], 0.4);
                        picked.validate().unwrap();
                    }
                }
            }
        }
    }
    #[test]
    fn hdr_picker_roundtrip_swap_exact_entry_and_sdr_transition() {
        let mut s = ColorState::default();
        assert!(s.set_hdr_intensity(2.).is_err());
        s.set_hdr_enabled(true).unwrap();
        s.set_rgba([0.4, 0.1, 0.2, 1.]).unwrap();
        s.set_hdr_intensity(2.).unwrap();
        let marker = s.wheel_components();
        let paint = s.definition();
        s = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        s.validate().unwrap();
        assert_eq!(s.definition(), paint);
        assert_eq!(s.wheel_components(), marker);
        s.apply(ColorAction::Swap).unwrap();
        assert_eq!(s.hdr_intensity(), 0.);
        s.apply(ColorAction::Swap).unwrap();
        assert_eq!(s.hdr_intensity(), 2.);
        let exact = RgbColor::from_linear(RgbSpace::Srgb, [65504., 2., -1., 0.25]).unwrap();
        s.set_color(exact).unwrap();
        assert_eq!(s.definition(), exact);
        assert!((s.hdr_intensity() - 65504f32.log2()).abs() < 1e-6);
        s.validate().unwrap();
        let old = s.clone();
        assert!(s.set_hdr_intensity(f32::NAN).is_err());
        assert_eq!(s, old);
        s.set_hdr_enabled(false).unwrap();
        assert_eq!(s.definition(), exact);
        assert_eq!(s.hdr_intensity(), 0.);
        let mut legacy = ColorState::default();
        legacy.set_color(exact).unwrap();
        assert_eq!(s.wheel_components(), legacy.wheel_components());
    }
}
