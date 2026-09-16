//! Transport for native and browser numeric color forms. Hosts retain drafts;
//! the existing editor owns parsing, coordinates and untouched-value precision.
use super::*;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorUiRequest {
    Form {
        request: ColorFormRequest,
    },
    Preview {
        colors: Vec<RgbColor>,
        #[serde(default)]
        display_space: RgbSpace,
    },
    Gradient {
        stops: Vec<layer_core::GradientStop>,
        document_space: RgbSpace,
        #[serde(default)]
        display_space: RgbSpace,
    },
}
pub fn color_ui(request: ColorUiRequest) -> Result<serde_json::Value, String> {
    let value = match request {
        ColorUiRequest::Form { request } => serde_json::to_value(color_form(request)?),
        ColorUiRequest::Preview {
            colors,
            display_space,
        } => {
            if colors.len() > 1024 {
                return Err("Too many color previews".into());
            }
            serde_json::to_value(
                colors
                    .into_iter()
                    .map(|color| color_preview(color, display_space))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        ColorUiRequest::Gradient {
            stops,
            document_space,
            display_space,
        } => {
            if stops.len() > 64 {
                return Err("Too many gradient stops".into());
            }
            // Preserve the shader's encoded document interpolation before
            // transforming each display sample. Hosts only draw these samples.
            let samples = (0..=256)
                .map(|i| {
                    let color =
                        layer_core::gradient_value(&stops, i as f32 / 256., document_space)?;
                    color_preview(color, display_space)
                })
                .collect::<Result<Vec<_>, String>>()?;
            serde_json::to_value(samples)
        }
    };
    value.map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorFormRequest {
    pub color: RgbColor,
    pub document_space: RgbSpace,
    #[serde(default)]
    pub display_space: RgbSpace,
    #[serde(default)]
    pub model: ColorInputModel,
    pub fields: Option<[String; 4]>,
    pub change_model: Option<ColorInputModel>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColorFormView {
    pub draft: ColorFormRequest,
    pub models: Vec<(ColorInputModel, &'static str)>,
    pub labels: [&'static str; 4],
    pub description: String,
    pub value: Option<RgbColor>,
    pub preview: Option<ColorPreview>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ColorPreview {
    pub space: RgbSpace,
    pub rgba: [f32; 4],
    pub in_gamut: bool,
}
pub fn color_preview(color: RgbColor, display_space: RgbSpace) -> Result<ColorPreview, String> {
    Ok(ColorPreview {
        space: display_space,
        rgba: color.encoded_in(display_space)?.map(|v| v.clamp(0., 1.)),
        in_gamut: color.in_gamut(display_space)?,
    })
}

pub fn color_form(request: ColorFormRequest) -> Result<ColorFormView, String> {
    let mut editor = ColorEditor::new(request.color, request.document_space)?;
    editor.set_model(request.model)?;
    if let Some(fields) = request.fields {
        for (i, text) in fields.into_iter().enumerate() {
            editor.set_field(i, text)?;
        }
    }
    let mut error = None;
    if let Some(model) = request.change_model {
        if let Err(message) = editor.set_model(model) {
            error = Some(message);
        }
    }
    let value = match editor.color() {
        Ok(color) => Some(color),
        Err(message) => {
            error = Some(message);
            None
        }
    };
    let preview = value
        .map(|color| color_preview(color, request.display_space))
        .transpose()?;
    Ok(ColorFormView {
        draft: ColorFormRequest {
            color: editor.definition(),
            document_space: request.document_space,
            display_space: request.display_space,
            model: editor.model(),
            fields: Some(editor.fields().clone()),
            change_model: None,
        },
        models: ColorInputModel::ALL
            .into_iter()
            .map(|model| (model, model.name()))
            .collect(),
        labels: editor.model().labels(),
        description: editor.description(),
        value,
        preview,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(color: RgbColor) -> ColorFormRequest {
        ColorFormRequest {
            color,
            document_space: RgbSpace::ProPhoto,
            display_space: RgbSpace::Srgb,
            model: Default::default(),
            fields: None,
            change_model: None,
        }
    }
    #[test]
    fn transported_drafts_keep_native_precision_and_reject_invalid_edits() {
        let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0.01, 0.23, 1. / 65535.]).unwrap();
        let mut form = color_form(request(color)).unwrap();
        assert!(!form.preview.unwrap().in_gamut);
        for model in ColorInputModel::ALL {
            form.draft.change_model = Some(model);
            let wire = serde_json::to_string(&form.draft).unwrap();
            form = color_form(serde_json::from_str(&wire).unwrap()).unwrap();
            assert_eq!(form.value, Some(color));
        }
        form.draft.fields.as_mut().unwrap()[0] = "not a number".into();
        form.draft.change_model = Some(ColorInputModel::SrgbHex);
        form = color_form(form.draft).unwrap();
        assert!(form.value.is_none());
        assert!(form.error.is_some());
        assert_eq!(form.draft.model, ColorInputModel::Oklch);
        assert_eq!(form.draft.color, color);
    }
}
