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
            Self::LinearRgb => "Linear RGB / HDR",
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
        };
        editor.populate();
        Ok(editor)
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
            return RgbColor::new(
                self.definition.space,
                [
                    self.definition.rgba[0],
                    self.definition.rgba[1],
                    self.definition.rgba[2],
                    alpha,
                ],
            );
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
        Ok(color)
    }
    pub fn description(&self) -> String {
        let mut text = format!("Document RGB: {}.", self.document_space.name());
        if self.model == ColorInputModel::LinearRgb { text.push_str(" Linear 1 is reference white (203 cd/m² in HDR). Negative values and values above 1 are supported; alpha is separate."); }
        if self.model == ColorInputModel::SrgbHex {
            text.push_str(" Hex uses sRGB and rounds its preview to 8-bit. An unchanged entry keeps the original color.");
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
