//! Transport for native and browser numeric color forms. Hosts retain drafts;
//! the existing editor owns parsing, coordinates and untouched-value precision.
use super::*;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorUiRequest {
    ProofGeometry { size: f32 },
    PrintRecipe { settings: crate::proof_panel::PrintProofSettings },
    IntensityArc { size:f32, stops:f32, base:RgbColor, document_space:RgbSpace, recipe:layer_core::color::hdr::SdrRendition, headroom:f32 },
    IntensityPoint {size:f32, point:[f32;2], minimum:f32, maximum:f32},
    ProofMarkers { size: f32, recipe: layer_core::color::hdr::SdrRendition },
    PickerLayout { size: f32, #[serde(default)] hdr: bool },
    HdrPreview { color: RgbColor, document_space: RgbSpace, recipe: layer_core::color::hdr::SdrRendition, headroom: f32 },
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
        ColorUiRequest::PrintRecipe {settings} => return Ok(serde_json::json!(settings.recipe()?)),
        ColorUiRequest::IntensityPoint {size,point,minimum,maximum} => {
            if !point.iter().all(|v|v.is_finite()) || !minimum.is_finite() || !maximum.is_finite() || minimum>=maximum || minimum < -16. || maximum>65504f32.log2() {return Err("Invalid intensity range".into());}
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            return Ok(serde_json::json!(minimum+a.fraction(point)*(maximum-minimum)));
        },
        ColorUiRequest::IntensityArc {size,stops,base,document_space,recipe,headroom} => {
            super::hdr_picker::HdrPaint::validate_stops(stops)?;
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            let minimum=(-2f32).min(stops.floor());let maximum=6f32.max(stops.ceil()).min(65504f32.log2());
            let samples=(0..=80).map(|i|{
                let t=i as f32/80.;let mut p=base.linear_in(document_space)?;let gain=(minimum+t*(maximum-minimum)).exp2();
                for c in &mut p[..3]{*c*=gain;}
                let color=RgbColor::from_linear(document_space,p)?;
                Ok(crate::color_management::picker_preview(color,document_space,recipe,headroom)?)
            }).collect::<Result<Vec<_>,String>>()?;
            return Ok(serde_json::json!({"minimum":minimum,"maximum":maximum,"colors":samples,"marker":a.point((stops-minimum)/(maximum-minimum)),"zero":a.point(-minimum/(maximum-minimum))}));
        },
        ColorUiRequest::ProofGeometry {size} => return crate::color_management::dial_geometry(size),
        ColorUiRequest::ProofMarkers {size,recipe} => return crate::color_management::dial_markers(size,recipe),
        ColorUiRequest::PickerLayout {size,hdr} => {
            let layout=if hdr {ColorPanelLayout::with_hdr(size)} else {ColorPanelLayout::new(size)}.ok_or("Invalid picker size")?;
            let arc=HdrIntensityArc::new(size).ok_or("Invalid picker size")?;
            return Ok(serde_json::json!({"layout":layout,"height":layout.height(),"arc":{"width":arc.width,"marker_radius":arc.marker_radius,
                "points":(0..=80).map(|i|arc.point(i as f32/80.)).collect::<Vec<_>>()}}));
        },
        ColorUiRequest::HdrPreview {color,document_space,recipe,headroom} => {
            return Ok(serde_json::json!({"linear":crate::color_management::picker_preview(color,document_space,recipe,headroom)?}));
        },
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
    #[serde(default)]
    pub intensity: Option<f32>,
    #[serde(default)]
    pub change_intensity: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColorFormView {
    pub draft: ColorFormRequest,
    pub models: Vec<(ColorInputModel, &'static str)>,
    pub labels: [&'static str; 4],
    pub description: String,
    pub value: Option<RgbColor>,
    pub preview: Option<ColorPreview>,
    pub base: Option<RgbColor>,
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
    if let Some(stops)=request.intensity {editor.enable_hdr(stops)?;}
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
    let mut invalid_intensity = None;
    if let Some(text)=request.change_intensity {
        if let Err(message)=text.trim().parse::<f32>().map_err(|_| "Enter a finite EV value".to_string()).and_then(|v|editor.set_intensity(v)) {error=Some(message);invalid_intensity=Some(text);}
    }
    let base=editor.base_color().ok();
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
            intensity: editor.intensity(),
            change_intensity: invalid_intensity,
        },
        models: ColorInputModel::ALL
            .into_iter()
            .map(|model| (model, model.name()))
            .collect(),
        labels: editor.model().labels(),
        description: editor.description(),
        value,
        preview,
        base,
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
            intensity: None,
            change_intensity: None,
        }
    }
    #[test]
    fn hdr_invalid_intensity_draft_survives_other_edits() {
        let color=RgbColor::new(RgbSpace::DisplayP3,[1.8,1.2,0.4,0.5]).unwrap();
        let mut input=request(color);input.intensity=Some(2.);input.change_intensity=Some("bad EV".into());
        let mut view=color_form(input).unwrap();assert!(view.error.is_some());
        view.draft.change_model=Some(ColorInputModel::SrgbHex);
        view=color_form(view.draft).unwrap();assert!(view.error.is_some());assert_eq!(view.draft.change_intensity.as_deref(),Some("bad EV"));
        view.draft.change_intensity=Some("3".into());view=color_form(view.draft).unwrap();assert!(view.error.is_none());assert_eq!(view.draft.intensity,Some(3.));
        assert!((view.value.unwrap().linear_in(RgbSpace::DisplayP3).unwrap()[3]-0.5).abs()<1e-6);
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
