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
            *v = (f64::from(*v) / f64::from(stops).exp2()) as f32;
        }
        Ok(Self {
            base: RgbColor::from_linear(space, p)?,
            stops,
        })
    }
    pub fn at_intensity(color: RgbColor, space: RgbSpace, stops: f32) -> Result<Self, String> {
        Self::validate_stops(stops)?;
        let mut p = color.linear_in(space)?;
        for v in &mut p[..3] { *v = (f64::from(*v) / f64::from(stops).exp2()) as f32; }
        Ok(Self { base: if stops == 0. { color } else { RgbColor::from_linear(space, p)? }, stops })
    }
    pub fn validate_stops(stops: f32) -> Result<(), String> {
        if !stops.is_finite() || !(-149. ..=128.).contains(&stops) {
            return Err("Intensity must be between −149 and +128 EV; the color must fit the document precision".into());
        }
        Ok(())
    }
    pub fn color(self, space: RgbSpace) -> Result<RgbColor, String> {
        let mut p = self.base.linear_in(space)?;
        for v in &mut p[..3] {
            *v = (f64::from(*v) * f64::from(self.stops).exp2()) as f32;
        }
        layer_core::color::hdr::validate_pixel(layer_core::color::SampleDepth::F32, p).map_err(str::to_string)?;
        RgbColor::from_linear(space, p)
    }
}
pub(super) fn validate_intensity(depth: layer_core::color::SampleDepth, stops: f32) -> Result<(), String> {
    HdrPaint::validate_stops(stops)?;
    if depth != layer_core::color::SampleDepth::F32 && !(-16. ..=65504f32.log2()).contains(&stops) {
        return Err("Intensity must be between −16 and +16 EV (half-float limit)".into());
    }
    Ok(())
}
impl ColorState {
    pub fn view_mapped(&self, recipe: layer_core::color::hdr::SdrRendition) -> ColorPanelView {
        let mut view=self.view();
        if self.hdr_picker.is_none() {return view;}
        view.rendition=Some(recipe);
        let base=self.picker_base().linear_in(self.rgb_space).unwrap();
        let mapper=recipe.mapper(self.rgb_space,RgbSpace::Srgb);
        view.intensity_ramp=(0..=64).map(|i| {
            let gain=(-2.+8.*i as f64/64.).exp2();
            let rgb=mapper.map_rgb([base[0],base[1],base[2]].map(|v|
                (f64::from(v)*gain).clamp(-f64::from(f32::MAX),f64::from(f32::MAX)) as f32));
            [RgbSpace::Srgb.encode(rgb[0] as f64) as f32,RgbSpace::Srgb.encode(rgb[1] as f64) as f32,RgbSpace::Srgb.encode(rgb[2] as f64) as f32,1.]
        }).collect();
        let preview=|color| super::form::mapped_preview(color,self.rgb_space,RgbSpace::Srgb,Some(recipe)).unwrap().rgba;
        view.marker_color=preview(self.definition())[..3].try_into().unwrap();
        for swatch in &mut view.swatches {swatch.rgba=match swatch.slot {ColorSlot::Foreground=>preview(self.foreground),ColorSlot::Background=>preview(self.background),ColorSlot::Transparent=>[0.;4]};}
        view.outside_document_gamut=!self.definition().in_hdr_gamut(self.rgb_space).unwrap();
        view.outside_display_gamut=!self.definition().in_hdr_gamut(RgbSpace::Srgb).unwrap();
        view
    }
    pub fn render_field_mapped(&self, side:u32, recipe:layer_core::color::hdr::SdrRendition, bytes:&mut [u8]) -> bool {
        if bytes.len()!=side as usize*side as usize*4{return false;}
        let mut pixels=vec![[0.;4];side as usize*side as usize];
        if !self.render_field_linear(side,&mut pixels){return false;}
        let mapper=recipe.mapper(self.rgb_space,RgbSpace::Srgb);
        for (out,p) in bytes.chunks_exact_mut(4).zip(pixels){let rgb=mapper.map_rgb([p[0],p[1],p[2]]);for c in 0..3{out[c]=(RgbSpace::Srgb.encode(rgb[c] as f64).clamp(0.,1.)*255.).round() as u8;}out[3]=255;}
        true
    }
    /// Called at document/workspace boundaries; changing mode never alters paint.
    pub fn set_document_depth(&mut self, depth: layer_core::color::SampleDepth) -> Result<(), String> {
        self.set_hdr_enabled(depth.is_float())?;
        self.hdr_depth = depth;
        Ok(())
    }
    pub fn hdr_depth(&self) -> layer_core::color::SampleDepth { self.hdr_depth }
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
        validate_intensity(self.hdr_depth, stops)?;
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
                if !paint.stops.is_finite() || !(-149. ..=128.).contains(&paint.stops) {
                    return Err("Invalid HDR picker intensity".into());
                }
                let mut expected = paint.base.linear_in(actual.space)?;
                for v in &mut expected[..3] {
                    *v = (f64::from(*v) * f64::from(paint.stops).exp2()) as f32;
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
        self.render_field_with_gain(side, pixels, f64::from(self.hdr_intensity()).exp2())
    }
    /// Hosts retain this Float32 base across EV and display-capability changes.
    pub fn render_field_base_linear(&self, side: u32, pixels: &mut [[f32; 4]]) -> bool {
        self.render_field_with_gain(side, pixels, 1.)
    }
    fn render_field_with_gain(&self, side: u32, pixels: &mut [[f32; 4]], gain: f64) -> bool {
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
            // The field previews colors beyond the current selection. Saturate
            // only this display buffer; accepting a color still validates range.
            let rgb = rgb.map(|v| (f64::from(v) * gain).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32);
            *p = [rgb[0], rgb[1], rgb[2], 1.];
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

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn hdr_entry_at_half_limit_keeps_serializable_picker_coordinates() {
        for space in RgbSpace::ALL {
            let mut s=ColorState::default();s.set_rgb_space(space).unwrap();s.set_hdr_enabled(true).unwrap();
            for p in [[8.,2.,1.,1.],[65504.,2.,1.,1.],[1.,65504.,2.,0.5]] {
                s.set_color(RgbColor::from_linear(space,p).unwrap()).unwrap();
                assert!(s.validate().is_ok(),"{space:?} {p:?}: {:?} {:?}",s.validate(),s.coordinates);
                let recovered:ColorState=serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
                recovered.validate().unwrap();
            }
        }
    }
}

#[cfg(test)]
mod float32_tests {
    use super::*;
    use layer_core::color::SampleDepth;
    #[test]
    fn float32_intensity_extremes_keep_state_and_preview_finite() {
        let mut state = ColorState::default();
        state.set_document_depth(SampleDepth::F32).unwrap();
        state.set_color(RgbColor::from_linear(RgbSpace::Srgb, [0.25; 4]).unwrap()).unwrap();
        state.set_hdr_intensity(128.).unwrap();
        state.validate().unwrap();
        let restored: ColorState = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        restored.validate().unwrap();
        let mut pixels = vec![[0.; 4]; 49];
        assert!(state.render_field_linear(7, &mut pixels));
        assert!(pixels.into_iter().flatten().all(f32::is_finite));
        assert!(state.view_mapped(Default::default()).intensity_ramp.into_iter().flatten().all(f32::is_finite));
        state.set_hdr_intensity(-149.).unwrap();
        state.validate().unwrap();
        let before = state.clone();
        assert!(state.set_hdr_intensity(-150.).is_err());
        assert_eq!(state, before);
    }
    #[test]
    fn float32_color_entry_uses_document_range_and_roundtrips_workspace() {
        let mut state = ColorState::default();
        state.set_document_depth(SampleDepth::F32).unwrap();
        let color = RgbColor::from_linear(RgbSpace::Srgb, [100000.125,-1.,0.000000123,0.25]).unwrap();
        state.set_color(color).unwrap();
        assert!(state.hdr_intensity() > 16.);
        let mut draft = ColorEditor::new(color, RgbSpace::Srgb).unwrap();
        draft.set_document_depth(SampleDepth::F32);
        draft.enable_hdr(state.hdr_intensity()).unwrap();
        draft.set_model(ColorInputModel::LinearRgb).unwrap();
        draft.set_field(0, "200000.125".into()).unwrap();
        assert!(draft.color().unwrap().linear_in(RgbSpace::Srgb).unwrap()[0] > 200000.);
        draft.set_document_depth(SampleDepth::F16);
        assert!(draft.color().is_err());
        let restored: ColorState = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored, state);
        state.apply(ColorAction::HdrIntensity { stops: 30. }).unwrap();
        let before = state.clone();
        assert!(state.apply(ColorAction::HdrIntensity { stops: 129. }).is_err());
        assert_eq!(state, before);
        state.set_document_depth(SampleDepth::F16).unwrap();
        assert!(state.set_color(color).is_err());
    }
}
