//! Draft numeric entry: switching readouts and accepting an untouched form must
//! not quantize or reinterpret the retained paint definition.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorInputModel {
    #[default]
    DocumentRgb,
    LinearRgb,
    SrgbHex,
    Hsv,
    Hls,
    Oklch,
}
impl ColorInputModel {
    pub const ALL: [Self; 6] = [
        Self::DocumentRgb,
        Self::LinearRgb,
        Self::SrgbHex,
        Self::Hsv,
        Self::Hls,
        Self::Oklch,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::DocumentRgb => "Document RGB",
            Self::LinearRgb => "Linear RGB",
            Self::SrgbHex => "sRGB hex",
            Self::Hsv => "HSV (document RGB)",
            Self::Hls => "HLS (document RGB)",
            Self::Oklch => "OKLCH",
        }
    }
    pub fn labels(self) -> [&'static str; 4] {
        match self {
            Self::DocumentRgb => ["Red (encoded)", "Green (encoded)", "Blue (encoded)", "Alpha (%)"],
            Self::LinearRgb => ["Red (linear)", "Green (linear)", "Blue (linear)", "Alpha (%)"],
            Self::SrgbHex => ["sRGB hex (#RRGGBB)", "", "", "Alpha (%)"],
            Self::Hsv => ["Hue (°)", "Saturation (%)", "Value (%)", "Alpha (%)"],
            Self::Hls => ["Hue (°)", "Lightness (%)", "Saturation (%)", "Alpha (%)"],
            Self::Oklch => ["Lightness (%)", "Chroma", "Hue (°)", "Alpha (%)"],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ColorEditor {
    definition: RgbColor,
    document_space: RgbSpace,
    model: ColorInputModel,
    fields: [String; 4],
    initial: [String; 4],
    hdr: Option<HdrPaint>,
    depth: layer_core::color::SampleDepth,
}
impl ColorEditor {
    pub fn new(definition: RgbColor, document_space: RgbSpace) -> Result<Self, String> {
        ColorState::validate_definition(definition)?;
        let mut editor = Self {
            definition,
            document_space,
            model: ColorInputModel::DocumentRgb,
            fields: Default::default(),
            initial: Default::default(),
            hdr: None,
            depth: layer_core::color::SampleDepth::F16,
        };
        editor.populate();
        Ok(editor)
    }
    pub fn set_document_depth(&mut self, depth: layer_core::color::SampleDepth) { self.depth = depth; }
    fn validate_range(&self, color: RgbColor) -> Result<(), String> {
        if self.hdr.is_some() { layer_core::color::hdr::validate_pixel(self.depth, color.linear_in(self.document_space)?).map_err(str::to_string)?; }
        Ok(())
    }
    pub fn enable_hdr(&mut self, stops: f32) -> Result<(), String> {
        self.hdr = Some(HdrPaint::at_intensity(self.color()?, self.document_space, stops)?);
        Ok(())
    }
    pub fn intensity(&self) -> Option<f32> { self.hdr.map(|p| p.stops) }
    /// Current draft before its EV multiplier, including pending component edits.
    pub fn base_color(&self) -> Result<RgbColor, String> {
        let color = self.color()?;
        match self.hdr {
            Some(paint) => Ok(HdrPaint::at_intensity(color, self.document_space, paint.stops)?.base),
            None => Ok(color),
        }
    }
    /// RGB fields describe the final color. EV multiplies its remembered base,
    /// while numeric RGB edits keep the explicitly selected EV.
    pub fn set_intensity(&mut self, stops: f32) -> Result<(), String> {
        super::hdr_picker::validate_intensity(self.depth, stops)?;
        let mut paint = self.hdr.ok_or("Intensity requires an HDR color draft")?;
        if paint.stops == stops { self.color()?; return Ok(()); }
        let color = self.color()?;
        if color != self.definition { paint = HdrPaint::at_intensity(color, self.document_space, paint.stops)?; }
        paint.stops = stops;
        let color = paint.color(self.document_space)?;
        ColorState::validate_definition(color)?;
        self.validate_range(color)?;
        self.hdr = Some(paint);
        self.definition = color;
        self.populate();
        Ok(())
    }
    pub fn model(&self) -> ColorInputModel {
        self.model
    }
    pub fn fields(&self) -> &[String; 4] {
        &self.fields
    }
    pub fn definition(&self) -> RgbColor {
        self.definition
    }
    pub fn set_field(&mut self, index: usize, text: String) -> Result<(), String> {
        if index >= 4 {
            return Err("Invalid color entry".into());
        }
        self.fields[index] = text;
        Ok(())
    }
    pub fn set_model(&mut self, model: ColorInputModel) -> Result<(), String> {
        let color = self.color()?;
        if color != self.definition && let Some(paint) = self.hdr {
            self.hdr = Some(HdrPaint::at_intensity(color, self.document_space, paint.stops)?);
        }
        self.definition = color;
        self.model = model;
        self.populate();
        Ok(())
    }
    fn populate(&mut self) {
        let rgba = self.definition.encoded_in(self.document_space).unwrap();
        let rgb = [rgba[0], rgba[1], rgba[2]];
        let values = match self.model {
            ColorInputModel::DocumentRgb | ColorInputModel::SrgbHex => rgb,
            ColorInputModel::LinearRgb => { let p=self.definition.linear_in(self.document_space).unwrap(); [p[0],p[1],p[2]] },
            ColorInputModel::Hsv => components(rgba, ColorSpace::Hsv, 0.),
            ColorInputModel::Hls => components(rgba, ColorSpace::Hls, 0.),
            ColorInputModel::Oklch => okhsv::to_oklch_in(self.document_space, rgb, 0.),
        };
        self.fields = [
            values[0].to_string(),
            values[1].to_string(),
            values[2].to_string(),
            format!("{:.6}", self.definition.rgba[3] as f64 * 100.)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
        ];
        if self.model == ColorInputModel::SrgbHex {
            let rgb = self.definition.encoded_in(RgbSpace::Srgb).unwrap();
            let rgb = rgb.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
            self.fields[0] = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
            self.fields[1].clear();
            self.fields[2].clear();
        }
        self.initial = self.fields.clone();
    }
    pub fn color(&self) -> Result<RgbColor, String> {
        if self.fields.iter().any(|text| text.len() > 128) {
            return Err("Color entries must be at most 128 bytes".into());
        }
        let parse = |i: usize| -> Result<f32, String> {
            self.fields[i]
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("Enter a finite number for {}", self.model.labels()[i]))
        };
        let alpha = if self.fields[3] == self.initial[3] {
            self.definition.rgba[3]
        } else {
            let percent = parse(3)?;
            if !(0.0..=100.).contains(&percent) {
                return Err("Alpha must be between 0 and 100%".into());
            }
            percent / 100.
        };
        // Readout formatting, hex previews, and alpha-only edits never rebuild RGB.
        if self.fields[..3] == self.initial[..3] {
            let mut color = self.definition;
            color.rgba[3] = alpha;
            color.validate()?;
            return Ok(color);
        }
        let color = match self.model {
            ColorInputModel::SrgbHex => {
                let text = self.fields[0].trim().trim_start_matches('#');
                if !matches!(text.len(), 3 | 6) || !text.bytes().all(|v| v.is_ascii_hexdigit()) {
                    return Err("Use #RGB or #RRGGBB; edit alpha separately".into());
                }
                let mut rgb = [0.; 3];
                for i in 0..3 {
                    let value = if text.len() == 3 {
                        u8::from_str_radix(&text[i..i + 1], 16).unwrap() * 17
                    } else {
                        u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap()
                    };
                    rgb[i] = value as f32 / 255.;
                }
                RgbColor::new(RgbSpace::Srgb, [rgb[0], rgb[1], rgb[2], alpha])?
            }
            model => {
                let values = [parse(0)?, parse(1)?, parse(2)?];
                match model {
                    ColorInputModel::LinearRgb => RgbColor::from_linear(self.document_space, [values[0], values[1], values[2], alpha])?,
                    ColorInputModel::DocumentRgb => RgbColor::new(
                        self.document_space,
                        [values[0], values[1], values[2], alpha],
                    )?,
                    ColorInputModel::Hsv | ColorInputModel::Hls => {
                        if !(0.0..=100.).contains(&values[1]) || !(0.0..=100.).contains(&values[2])
                        {
                            return Err(
                                "Saturation, lightness and value must be between 0 and 100%".into(),
                            );
                        }
                        let space = if model == ColorInputModel::Hsv {
                            ColorSpace::Hsv
                        } else {
                            ColorSpace::Hls
                        };
                        RgbColor::new(self.document_space, from_components(values, space, alpha))?
                    }
                    ColorInputModel::Oklch => {
                        if values[0] < 0. || values[1] < 0. {
                            return Err("Lightness and chroma cannot be negative".into());
                        }
                        let hue = (values[2] as f64).to_radians();
                        let rgb = gamut::Gamut::get(self.document_space).linear_rgb([
                            values[0] as f64 / 100.,
                            values[1] as f64 * hue.cos(),
                            values[1] as f64 * hue.sin(),
                        ]);
                        RgbColor::from_linear(
                            self.document_space,
                            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, alpha],
                        )?
                    }
                    ColorInputModel::SrgbHex => unreachable!(),
                }
            }
        };
        ColorState::validate_definition(color)?;
        self.validate_range(color)?;
        Ok(color)
    }
    pub fn description(&self) -> String {
        let mut text = format!("Document RGB: {}.", self.document_space.name());
        if self.model == ColorInputModel::LinearRgb { text.push_str(" 1 = reference white."); }
        if self.model == ColorInputModel::SrgbHex {
            text.push_str(" Hex uses 8-bit sRGB.");
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_numeric_draft_retains_exact_color_ev_black_and_alpha() {
        for space in RgbSpace::ALL {
            for linear in [[0.08, 0.02, 0.04, 0.37], [0., 0., 0., 0.25], [4., -0.2, 2., 0.8]] {
                let original = RgbColor::from_linear(space, linear).unwrap();
                let mut state = ColorState::default();
                state.set_rgb_space(space).unwrap();
                state.set_hdr_enabled(true).unwrap();
                state.apply(ColorAction::SetSlotIntensity { slot: ColorSlot::Foreground, color: original, stops: 2. }).unwrap();
                let mut editor = ColorEditor::new(original, space).unwrap();
                editor.enable_hdr(2.).unwrap();
                for model in ColorInputModel::ALL {
                    editor.set_model(model).unwrap();
                    assert_eq!(editor.color().unwrap(), original);
                    assert_eq!(editor.intensity(), Some(2.));
                }
                let untouched = state.clone();
                state.apply(ColorAction::SetSlotIntensity { slot: ColorSlot::Foreground, color: editor.color().unwrap(), stops: editor.intensity().unwrap() }).unwrap();
                assert_eq!(state, untouched);
                let base = editor.base_color().unwrap().linear_in(space).unwrap();
                for c in 0..3 { assert!((base[c] - linear[c] / 4.).abs() < 1e-5); }
                editor.set_intensity(3.).unwrap();
                let next_base = editor.base_color().unwrap().linear_in(space).unwrap();
                for c in 0..4 { assert!((base[c] - next_base[c]).abs() < 1e-5); }
                let color = editor.color().unwrap().linear_in(space).unwrap();
                for c in 0..3 { assert!((color[c] - linear[c] * 2.).abs() < 1e-5); }
                assert_eq!(color[3], linear[3]);
                state.apply(ColorAction::SetSlotIntensity { slot: ColorSlot::Background, color: editor.color().unwrap(), stops: 3. }).unwrap();
                state.validate().unwrap();
                assert_eq!(state.hdr_intensity(), 3.);
                assert_eq!(state.foreground, original);
                state.apply(ColorAction::Select { slot: ColorSlot::Foreground }).unwrap();
                state.apply(ColorAction::SetSlotIntensity { slot: ColorSlot::Background, color: editor.color().unwrap(), stops: 3. }).unwrap();
                assert_eq!(state.slot, ColorSlot::Background);
                assert_eq!(state.hdr_intensity(), 3.);
                state.validate().unwrap();
                editor.set_model(ColorInputModel::LinearRgb).unwrap();
                editor.set_field(0, "0.25".into()).unwrap();
                assert!((editor.base_color().unwrap().linear_in(space).unwrap()[0] - 0.25 / 8.).abs() < 1e-5);
                editor.set_intensity(4.).unwrap();
                assert!((editor.color().unwrap().linear_in(space).unwrap()[0] - 0.5).abs() < 1e-5);
                let accepted = editor.color().unwrap();
                for invalid in [f32::NAN, f32::INFINITY, -17., 17.] { assert!(editor.set_intensity(invalid).is_err()); }
                assert_eq!(editor.color().unwrap(), accepted);
                editor.set_field(0, "65504".into()).unwrap();
                assert!(editor.set_intensity(5.).is_err());
                assert_eq!(editor.intensity(), Some(4.));
            }
        }
    }
    #[test]
    fn model_changes_and_alpha_do_not_quantize_definitions() {
        for space in RgbSpace::ALL {
            let definition =
                RgbColor::new(space, [-0.12, 1.2, 31234. / 65535., 213. / 65535.]).unwrap();
            let mut editor = ColorEditor::new(definition, RgbSpace::Srgb).unwrap();
            for model in ColorInputModel::ALL {
                editor.set_model(model).unwrap();
                assert_eq!(editor.color().unwrap(), definition);
            }
            editor.set_field(3, "37".into()).unwrap();
            let color = editor.color().unwrap();
            assert_eq!(color.space, space);
            assert_eq!(color.rgba[..3], definition.rgba[..3]);
            assert_eq!(color.rgba[3], 0.37);
        }
    }
    #[test]
    fn numeric_entries_name_their_space_and_keep_extended_rgb() {
        let mut editor = ColorEditor::new(RgbColor::WHITE, RgbSpace::DisplayP3).unwrap();
        editor.set_field(0, "-0.1".into()).unwrap();
        assert_eq!(editor.color().unwrap().space, RgbSpace::DisplayP3);
        assert_eq!(editor.color().unwrap().rgba[0], -0.1);
        editor.set_model(ColorInputModel::SrgbHex).unwrap();
        editor.set_field(0, "#ff0033".into()).unwrap();
        assert_eq!(
            editor.color().unwrap(),
            RgbColor::new(RgbSpace::Srgb, [1., 0., 0.2, 1.]).unwrap()
        );
        editor.set_model(ColorInputModel::Hsv).unwrap();
        for (i, text) in ["120", "100", "100"].into_iter().enumerate() {
            editor.set_field(i, text.into()).unwrap();
        }
        assert_eq!(
            editor.color().unwrap(),
            RgbColor::new(RgbSpace::DisplayP3, [0., 1., 0., 1.]).unwrap()
        );
        editor.set_model(ColorInputModel::Oklch).unwrap();
        editor.set_field(1, "0.4".into()).unwrap();
        assert!(!editor.color().unwrap().in_gamut(RgbSpace::Srgb).unwrap());
        editor.set_field(1, "NaN".into()).unwrap();
        assert!(editor.set_model(ColorInputModel::DocumentRgb).is_err());
        assert_eq!(editor.model(), ColorInputModel::Oklch);
    }
}
