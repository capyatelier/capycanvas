//! Draft numeric entry: switching readouts and accepting an untouched form must
//! not quantize or reinterpret the retained paint definition.
use super::*;

crate::variants! {
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
}
impl ColorInputModel {
    pub fn localized_name(self, localizer: &crate::Localizer) -> std::sync::Arc<str> {
        localizer.text(match self {
            Self::DocumentRgb => crate::MessageId::COLOR_FORM_MODEL_DOCUMENT_RGB,
            Self::LinearRgb => crate::MessageId::COLOR_FORM_MODEL_LINEAR_RGB,
            Self::SrgbHex => crate::MessageId::COLOR_FORM_MODEL_SRGB_HEX,
            Self::Hsv => crate::MessageId::COLOR_FORM_MODEL_HSV,
            Self::Hls => crate::MessageId::COLOR_FORM_MODEL_HLS,
            Self::Oklch => crate::MessageId::COLOR_FORM_MODEL_OKLCH,
        })
    }
    pub fn localized_labels(self, localizer: &crate::Localizer) -> [std::sync::Arc<str>; 4] {
        let ids = match self {
            Self::DocumentRgb => [Some(crate::MessageId::COLOR_FORM_FIELD_RED_ENCODED), Some(crate::MessageId::COLOR_FORM_FIELD_GREEN_ENCODED), Some(crate::MessageId::COLOR_FORM_FIELD_BLUE_ENCODED), Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
            Self::LinearRgb => [Some(crate::MessageId::COLOR_FORM_FIELD_RED_LINEAR), Some(crate::MessageId::COLOR_FORM_FIELD_GREEN_LINEAR), Some(crate::MessageId::COLOR_FORM_FIELD_BLUE_LINEAR), Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
            Self::SrgbHex => [Some(crate::MessageId::COLOR_FORM_FIELD_HEX), None, None, Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
            Self::Hsv => [Some(crate::MessageId::COLOR_FORM_FIELD_HUE), Some(crate::MessageId::COLOR_FORM_FIELD_SATURATION), Some(crate::MessageId::COLOR_FORM_FIELD_VALUE), Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
            Self::Hls => [Some(crate::MessageId::COLOR_FORM_FIELD_HUE), Some(crate::MessageId::COLOR_FORM_FIELD_LIGHTNESS), Some(crate::MessageId::COLOR_FORM_FIELD_SATURATION), Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
            Self::Oklch => [Some(crate::MessageId::COLOR_FORM_FIELD_LIGHTNESS), Some(crate::MessageId::COLOR_FORM_FIELD_CHROMA), Some(crate::MessageId::COLOR_FORM_FIELD_HUE), Some(crate::MessageId::COLOR_FORM_FIELD_ALPHA)],
        };
        ids.map(|id| id.map_or_else(|| "".into(), |id|localizer.text(id)))
    }
}

#[derive(Clone, Debug)]
pub enum ColorEditorError {
    Numeric { field: usize, reason: crate::NumericError },
    EntriesTooLong,
    AlphaRange,
    HexSyntax,
    PercentRange,
    NegativeLightnessChroma,
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
    pub fn message(&self, model: ColorInputModel, localizer: &crate::Localizer) -> String {
        use crate::MessageId as M;
        let id = match self {
            Self::Numeric { field, reason } => return finite_field_message(localizer, model.localized_labels(localizer)[*field].as_ref(), reason),
            Self::EntriesTooLong => M::COLOR_FORM_ENTRIES_TOO_LONG,
            Self::AlphaRange => M::COLOR_FORM_ALPHA_RANGE,
            Self::HexSyntax => M::COLOR_FORM_HEX_SYNTAX,
            Self::PercentRange => M::COLOR_FORM_PERCENT_RANGE,
            Self::NegativeLightnessChroma => M::COLOR_FORM_NEGATIVE_LIGHTNESS_CHROMA,
            Self::Intensity(reason) => return finite_field_message(localizer, localizer.text(M::NATIVE_COLOR_INTENSITY_EV).as_ref(), reason),
            Self::Hdr(reason) => match reason {
                layer_core::color::hdr::HdrPixelError::ExpectedFloat => M::COLOR_HDR_EXPECTED_FLOAT,
                layer_core::color::hdr::HdrPixelError::FiniteCoverage => M::COLOR_HDR_FINITE_COVERAGE,
                layer_core::color::hdr::HdrPixelError::StorageRange => M::COLOR_HDR_STORAGE_RANGE,
            },
            Self::Detail(reason) => return reason.clone(),
        };
        localizer.text(id).to_string()
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
    fn validate_range(&self, color: RgbColor) -> Result<(), ColorEditorError> {
        if self.hdr.is_some() { layer_core::color::hdr::validate_pixel_typed(self.depth, color.linear_in(self.document_space)?)?; }
        Ok(())
    }
    pub fn enable_hdr(&mut self, stops: f32) -> Result<(), ColorEditorError> {
        self.hdr = Some(HdrPaint::at_intensity(self.color()?, self.document_space, stops)?);
        Ok(())
    }
    pub fn intensity(&self) -> Option<f32> { self.hdr.map(|p| p.stops) }
    /// Current draft before its EV multiplier, including pending component edits.
    pub fn base_color(&self) -> Result<RgbColor, ColorEditorError> {
        let color = self.color()?;
        self.base_for_color(color)
    }
    pub(super) fn base_for_color(&self, color: RgbColor) -> Result<RgbColor, ColorEditorError> {
        match self.hdr {
            Some(paint) => Ok(HdrPaint::at_intensity(color, self.document_space, paint.stops)?.base),
            None => Ok(color),
        }
    }
    /// RGB fields describe the final color. EV multiplies its remembered base,
    /// while numeric RGB edits keep the explicitly selected EV.
    pub fn set_intensity(&mut self, stops: f32) -> Result<(), ColorEditorError> {
        let color = self.color()?;
        self.set_intensity_color(stops, color).map(|_| ())
    }
    pub(super) fn set_intensity_color(&mut self, stops: f32, color: RgbColor) -> Result<RgbColor, ColorEditorError> {
        super::hdr_picker::validate_intensity(self.depth, stops)?;
        let mut paint = self.hdr.ok_or_else(|| ColorEditorError::Detail("Intensity requires an HDR color draft".into()))?;
        if paint.stops == stops { return Ok(color); }
        if color != self.definition { paint = HdrPaint::at_intensity(color, self.document_space, paint.stops)?; }
        paint.stops = stops;
        let color = paint.color(self.document_space)?;
        ColorState::validate_definition(color)?;
        self.validate_range(color)?;
        self.hdr = Some(paint);
        self.definition = color;
        self.populate();
        Ok(color)
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
    pub fn set_model(&mut self, model: ColorInputModel) -> Result<(), ColorEditorError> {
        let color = self.color()?;
        self.set_model_color(model, color)
    }
    pub(super) fn set_model_color(&mut self, model: ColorInputModel, color: RgbColor) -> Result<(), ColorEditorError> {
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
            self.fields[0] = ColorLibrary::hex_preview(self.definition);
            self.fields[1].clear();
            self.fields[2].clear();
        }
        self.initial = self.fields.clone();
    }
    pub fn color_localized(&self, localizer: &crate::Localizer) -> Result<RgbColor, String> {
        self.color().map_err(|reason| reason.message(self.model, localizer))
    }
    pub fn colors_localized(&self, localizer: &crate::Localizer) -> Result<(RgbColor, RgbColor), String> {
        let color = self.color_localized(localizer)?;
        let base = self.base_for_color(color).map_err(|reason|reason.message(self.model,localizer))?;
        Ok((color,base))
    }
    pub fn color(&self) -> Result<RgbColor, ColorEditorError> {
        if self.fields.iter().any(|text| text.len() > 128) {
            return Err(ColorEditorError::EntriesTooLong);
        }
        let parse = |i: usize| -> Result<f32, ColorEditorError> {
            crate::numeric::parse_numeric_text(&self.fields[i]).map_err(|reason| ColorEditorError::Numeric { field: i, reason })
        };
        let alpha = if self.fields[3] == self.initial[3] {
            self.definition.rgba[3]
        } else {
            let percent = parse(3)?;
            if !(0.0..=100.).contains(&percent) {
                return Err(ColorEditorError::AlphaRange);
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
                    return Err(ColorEditorError::HexSyntax);
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
                            return Err(ColorEditorError::PercentRange);
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
                            return Err(ColorEditorError::NegativeLightnessChroma);
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
    pub fn localized_description(&self, localizer: &crate::Localizer) -> String {
        use crate::MessageId as M;
        let mut text = localizer.text(match self.document_space { RgbSpace::Srgb => M::COLOR_FORM_DOCUMENT_SRGB, RgbSpace::DisplayP3 => M::COLOR_FORM_DOCUMENT_DISPLAY_P3, RgbSpace::AdobeRgb => M::COLOR_FORM_DOCUMENT_ADOBE_RGB, RgbSpace::ProPhoto => M::COLOR_FORM_DOCUMENT_PROPHOTO }).to_string();
        let extra = match self.model { ColorInputModel::LinearRgb => Some(M::COLOR_FORM_REFERENCE_WHITE), ColorInputModel::SrgbHex => Some(M::COLOR_FORM_HEX_DESCRIPTION), _ => None };
        if let Some(id) = extra { text.push(' '); text.push_str(&localizer.text(id)); }
        text
    }

}

pub fn color_intensity_input(text: &str, localizer: &crate::Localizer) -> Result<f32, String> {
    crate::numeric::parse_numeric_text(text).map_err(|reason| finite_field_message(localizer, localizer.text(crate::MessageId::NATIVE_COLOR_INTENSITY_EV).as_ref(), &reason))
}
pub(crate) fn finite_field_message(localizer: &crate::Localizer, label: &str, reason: &crate::NumericError) -> String {
    if !matches!(reason, crate::NumericError::InvalidNumber | crate::NumericError::FiniteNumber) { return reason.message(localizer); }
    let mut args = crate::FluentArgs::new(); args.set("label", label);
    localizer.format(crate::MessageId::COLOR_FORM_FINITE_FIELD, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_known_refusals_are_typed_and_retain_the_draft() {
        let localizer = crate::Localizer::shared(crate::UiLanguage::English);
        let mut editor = ColorEditor::new(RgbColor::WHITE, RgbSpace::Srgb).unwrap();
        editor.enable_hdr(0.).unwrap();
        let before = editor.definition();
        let reason = editor.set_intensity(16.).unwrap_err();
        assert!(matches!(reason, ColorEditorError::Intensity(crate::NumericError::Range { max, .. }) if max == f64::from(65504f32.log2())));
        assert_eq!(editor.definition(), before);
        assert!(!reason.message(ColorInputModel::DocumentRgb, &localizer).is_empty());
        assert!(matches!(editor.set_intensity(f32::NAN), Err(ColorEditorError::Intensity(crate::NumericError::FiniteNumber))));
        editor.set_document_depth(layer_core::color::SampleDepth::F32);
        editor.set_intensity(16.).unwrap();
        assert!(matches!(super::super::hdr_picker::HdrPaint { base: RgbColor::WHITE, stops: 128. }.color(RgbSpace::Srgb), Err(ColorEditorError::Hdr(layer_core::color::hdr::HdrPixelError::FiniteCoverage))));
        for reason in [layer_core::color::hdr::HdrPixelError::ExpectedFloat, layer_core::color::hdr::HdrPixelError::FiniteCoverage, layer_core::color::hdr::HdrPixelError::StorageRange] {
            assert!(!ColorEditorError::Hdr(reason).message(ColorInputModel::DocumentRgb, &localizer).is_empty());
        }
    }
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
