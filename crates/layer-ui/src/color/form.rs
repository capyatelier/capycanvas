//! Transport for native and browser numeric color forms. Hosts retain drafts;
//! the existing editor owns parsing, coordinates and untouched-value precision.
use super::*;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorUiRequest {
    IntensityArc { size:f32, stops:f32, #[serde(default)] depth:Option<layer_core::color::SampleDepth>, base:RgbColor, document_space:RgbSpace, recipe:layer_core::color::hdr::SdrRendition, headroom:f32 },
    IntensityPoint {size:f32, point:[f32;2], minimum:f32, maximum:f32},
    PickerLayout { size: f32, #[serde(default)] hdr: bool },
    HdrPreview { color: RgbColor, document_space: RgbSpace, recipe: layer_core::color::hdr::SdrRendition, headroom: f32 },
    Layout { size: f32, #[serde(default)] hdr: bool },
    Arc { size: f32, point: Option<[f32;2]>, #[serde(default)] fraction: f32 },
    ProofDial { size: f32, recipe: layer_core::color::hdr::SdrRendition, point: Option<[f32;2]>, part: Option<u8> },
    ProofControl { recipe: layer_core::color::hdr::SdrRendition, part: u8, edit: crate::proof_panel::SdrControlEdit },
    PrintProof { settings: crate::proof_panel::PrintProofSettings },
    Form {
        request: ColorFormRequest,
    },
    Preview {
        colors: Vec<RgbColor>,
        #[serde(default)]
        document_space: RgbSpace,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
    },
    Gradient {
        stops: Vec<layer_core::GradientStop>,
        document_space: RgbSpace,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
    },
}
pub fn color_ui(request: ColorUiRequest) -> Result<serde_json::Value, String> {
    let value = match request {
        ColorUiRequest::IntensityPoint {size,point,minimum,maximum} => {
            if !point.iter().all(|v|v.is_finite()) || !minimum.is_finite() || !maximum.is_finite() || minimum>=maximum || minimum < -149. || maximum>128. {return Err("Invalid intensity range".into());}
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            return Ok(serde_json::json!(minimum+a.fraction(point)*(maximum-minimum)));
        },
        ColorUiRequest::IntensityArc {size,stops,depth,base,document_space,recipe,headroom} => {
            super::hdr_picker::HdrPaint::validate_stops(stops)?;
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            let minimum=(-2f32).min(stops.floor()); let limit=if depth==Some(layer_core::color::SampleDepth::F32) {128.} else {65504f32.log2()}; let maximum=6f32.max(stops.ceil()).min(limit);
            let samples=(0..=80).map(|i|{
                let t=i as f32/80.;let mut p=base.linear_in(document_space)?;let gain=f64::from(minimum+t*(maximum-minimum)).exp2();
                for c in &mut p[..3]{*c=(f64::from(*c)*gain).clamp(-f64::from(f32::MAX),f64::from(f32::MAX)) as f32;}
                let color=RgbColor::from_linear(document_space,p)?;
                Ok(crate::color_management::picker_preview(color,document_space,recipe,headroom)?)
            }).collect::<Result<Vec<_>,String>>()?;
            return Ok(serde_json::json!({"minimum":minimum,"maximum":maximum,"colors":samples,"marker":a.point((stops-minimum)/(maximum-minimum)),"zero":a.point(-minimum/(maximum-minimum))}));
        },
        ColorUiRequest::PickerLayout {size,hdr} => {
            let layout=if hdr {ColorPanelLayout::with_hdr(size)} else {ColorPanelLayout::new(size)}.ok_or("Invalid picker size")?;
            let arc=HdrIntensityArc::new(size).ok_or("Invalid picker size")?;
            return Ok(serde_json::json!({"layout":layout,"height":layout.height(),"arc":{"width":arc.width,"marker_radius":arc.marker_radius,
                "points":(0..=80).map(|i|arc.point(i as f32/80.)).collect::<Vec<_>>()}}));
        },
        ColorUiRequest::HdrPreview {color,document_space,recipe,headroom} => {
            return Ok(serde_json::json!({"linear":crate::color_management::picker_preview(color,document_space,recipe,headroom)?}));
        },
        ColorUiRequest::ProofDial {size,recipe,point,part} => return crate::proof_panel::sdr_dial(size,recipe,point,part),
        ColorUiRequest::ProofControl {recipe,part,edit} => return serde_json::to_value(crate::proof_panel::sdr_control(recipe,part,edit)?).map_err(|e|e.to_string()),
        ColorUiRequest::PrintProof {settings} => return serde_json::to_value(settings.recipe()?).map_err(|e|e.to_string()),
        ColorUiRequest::Layout {size,hdr} => {
            let layout=if hdr {ColorPanelLayout::with_hdr(size)}else{ColorPanelLayout::new(size)}.ok_or("Invalid color panel size")?;
            let mut value=serde_json::to_value(layout).map_err(|e|e.to_string())?;
            value["height"]=serde_json::json!(layout.height().max(size));
            return Ok(value);
        }
        ColorUiRequest::Arc {size,point,fraction} => {
            let arc=HdrIntensityArc::new(size).ok_or("Invalid HDR arc size")?;
            return Ok(serde_json::json!({"geometry":arc,"point":arc.point(fraction),"path":(0..=64).map(|i|arc.point(i as f32/64.)).collect::<Vec<_>>(),"hit":point.is_some_and(|p|arc.contains(p)),"fraction":point.map(|p|arc.fraction(p))}));
        }
        ColorUiRequest::Form { request } => serde_json::to_value(color_form(request)?),
        ColorUiRequest::Preview {
            colors,
            document_space,
            display_space,
            rendition,
        } => {
            if colors.len() > 1024 {
                return Err("Too many color previews".into());
            }
            serde_json::to_value(
                colors
                    .into_iter()
                    .map(|color| mapped_preview(color, document_space, display_space, rendition))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        ColorUiRequest::Gradient {
            stops,
            document_space,
            display_space,
            rendition,
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
                    mapped_preview(color, document_space, display_space, rendition)
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
    pub document_depth: Option<layer_core::color::SampleDepth>,
    #[serde(default)]
    pub display_space: RgbSpace,
    #[serde(default)]
    pub model: ColorInputModel,
    pub fields: Option<[String; 4]>,
    pub change_model: Option<ColorInputModel>,
    pub intensity: Option<f32>,
    pub change_intensity: Option<f32>,
    #[serde(default)]
    pub change_intensity_text: Option<String>,
    pub rendition: Option<layer_core::color::hdr::SdrRendition>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColorFormView {
    pub draft: ColorFormRequest,
    pub models: Vec<(ColorInputModel, &'static str)>,
    pub labels: [&'static str; 4],
    pub description: String,
    pub validation: Option<String>,
    pub value: Option<RgbColor>,
    pub preview: Option<ColorPreview>,
    pub base_preview: Option<ColorPreview>,
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

pub(super) fn mapped_preview(color:RgbColor, document:RgbSpace, display:RgbSpace, rendition:Option<layer_core::color::hdr::SdrRendition>) -> Result<ColorPreview,String> {
    let Some(recipe)=rendition else {return color_preview(color,display)};
    recipe.validate().map_err(str::to_string)?;
    let p=color.linear_in(document)?;
    let rgb=recipe.mapper(document,display).map_rgb([p[0],p[1],p[2]]);
    Ok(ColorPreview {space:display,rgba:[display.encode(rgb[0] as f64) as f32,display.encode(rgb[1] as f64) as f32,display.encode(rgb[2] as f64) as f32,p[3]],in_gamut:color.in_hdr_gamut(display)?})
}

/// Color definition and gamut feedback shared with GTK's Edit Color dialog.
pub fn color_validation(color: RgbColor, document: RgbSpace, display: RgbSpace, hdr: bool) -> Result<String, String> {
    let mut text = format!("Defined in {}", color.space.name());
    if !if hdr { color.in_hdr_gamut(document)? } else { color.in_gamut(document)? } {
        text.push_str(" · Outside document gamut");
    }
    if !if hdr { color.in_hdr_gamut(display)? } else { color.in_gamut(display)? } {
        text.push_str(&format!(" · Outside {} preview gamut", display.name()));
    }
    if hdr && color.brightness_ev(document)?.is_some_and(|v| v > 0.00001) { text.push_str(" · Above SDR white"); }
    Ok(text)
}

pub fn color_form(request: ColorFormRequest) -> Result<ColorFormView, String> {
    let mut editor = ColorEditor::new(request.color, request.document_space)?;
    if let Some(depth)=request.document_depth {editor.set_document_depth(depth);}
    editor.set_model(request.model)?;
    let intensity=request.intensity.or_else(||request.document_depth.filter(|d|d.is_float()).map(|_|request.color.brightness_ev(request.document_space).ok().flatten().unwrap_or(0.).max(0.)));
    if let Some(stops) = intensity { editor.enable_hdr(stops)?; }
    if let Some(fields) = request.fields {
        for (i, text) in fields.into_iter().enumerate() {
            editor.set_field(i, text)?;
        }
    }
    let mut error = None;
    let typed_intensity = request.change_intensity_text.as_deref().map(|s| s.trim().parse::<f32>().map_err(|_| "Enter a finite HDR intensity in EV".to_string())).transpose();
    let change_intensity = match typed_intensity { Ok(value) => value.or(request.change_intensity), Err(message) => { error = Some(message); None } };
    if let Some(stops) = change_intensity {
        if let Err(message) = editor.set_intensity(stops) { error = Some(message); }
    }
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
        .map(|color| mapped_preview(color, request.document_space, request.display_space, request.rendition))
        .transpose()?;
    Ok(ColorFormView {
        draft: ColorFormRequest {
            color: editor.definition(),
            document_space: request.document_space,
            document_depth: request.document_depth,
            display_space: request.display_space,
            model: editor.model(),
            fields: Some(editor.fields().clone()),
            change_model: None,
            intensity: editor.intensity(),
            change_intensity: None,
            change_intensity_text: request.change_intensity_text.clone().filter(|_| error.is_some()),
            rendition: request.rendition,
        },
        models: ColorInputModel::ALL
            .into_iter()
            .map(|model| (model, model.name()))
            .collect(),
        labels: editor.model().labels(),
        description: editor.description(),
        validation: value.map(|c|color_validation(c,request.document_space,request.display_space,editor.intensity().is_some())).transpose()?,
        value,
        preview,
        base: editor.intensity().and_then(|_| editor.base_color().ok()),
        base_preview: editor.intensity().and_then(|_| editor.base_color().ok()).map(|c| mapped_preview(c,request.document_space,request.display_space,request.rendition)).transpose()?,
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
            document_depth: None,
            display_space: RgbSpace::Srgb,
            model: Default::default(),
            fields: None,
            change_model: None,
            intensity: None,
            change_intensity: None,
            change_intensity_text: None,
            rendition: None,
        }
    }
    #[test]
    fn hdr_property_color_uses_the_gtk_initial_ev_and_preserves_samples() {
        let color=RgbColor::from_linear(RgbSpace::Srgb,[-0.2,4.,1.,0.3]).unwrap();
        let mut draft=request(color);
        draft.document_space=RgbSpace::Srgb;
        draft.document_depth=Some(layer_core::color::SampleDepth::F16);
        draft.model=ColorInputModel::LinearRgb;
        let form=color_form(draft).unwrap();
        assert_eq!(form.value,Some(color));
        assert_eq!(form.draft.intensity,Some(2.));
        assert!(form.base_preview.is_some());
        assert!(form.validation.as_deref().unwrap().contains("Above SDR white"));
        assert!(form.validation.as_deref().unwrap().contains("Outside document gamut"));
        assert_eq!(color_form(form.draft).unwrap().value,Some(color));
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
    #[test]
    fn native_hdr_intensity_text_preserves_precision_and_invalid_drafts() {
        let color=RgbColor::new(RgbSpace::ProPhoto,[1.;4]).unwrap();
        for depth in [layer_core::color::SampleDepth::F16,layer_core::color::SampleDepth::F32] {
            let mut draft=request(color);draft.document_depth=Some(depth);draft.intensity=Some(0.);
            draft.change_intensity_text=Some("18".into());
            let form=color_form(draft).unwrap();
            if depth==layer_core::color::SampleDepth::F32 {
                assert!(form.error.is_none());assert_eq!(form.value.unwrap().linear_in(RgbSpace::ProPhoto).unwrap()[0],262144.);
            } else {assert!(form.error.is_some());assert_eq!(form.draft.change_intensity_text.as_deref(),Some("18"));}
        }
        for text in ["not a number","NaN","inf"] {
            let mut draft=request(color);draft.document_depth=Some(layer_core::color::SampleDepth::F32);draft.intensity=Some(0.);draft.change_intensity_text=Some(text.into());
            let form=color_form(draft).unwrap();assert!(form.error.is_some());assert_eq!(form.draft.change_intensity_text.as_deref(),Some(text));
        }
    }

    #[test]
    fn hdr_palette_and_gradient_previews_follow_the_saved_appearance() {
        let color=RgbColor::from_linear(RgbSpace::DisplayP3,[4.,2.,0.5,0.5]).unwrap();
        let mut request=serde_json::json!({"type":"preview","colors":[color],"document_space":"DisplayP3"});
        let unmapped=color_ui(serde_json::from_value(request.clone()).unwrap()).unwrap();
        let recipe=layer_core::color::hdr::SdrRendition::default();
        request["rendition"]=serde_json::to_value(recipe).unwrap();
        let mapped=color_ui(serde_json::from_value(request.clone()).unwrap()).unwrap();
        assert_ne!(mapped,unmapped);
        assert_eq!(mapped[0]["rgba"][3],serde_json::json!(0.5));
        let gradient=color_ui(serde_json::from_value(serde_json::json!({"type":"gradient","stops":[{"position":0.,"color":color},{"position":1.,"color":color}],"document_space":"DisplayP3","rendition":recipe})).unwrap()).unwrap();
        // Gradient interpolation returns through encoded document RGB; allow Float32 roundoff.
        for at in [0,256] {for channel in 0..4 {assert!((gradient[at]["rgba"][channel].as_f64().unwrap()-mapped[0]["rgba"][channel].as_f64().unwrap()).abs()<1e-6);}}
        request["rendition"]["exposure"]=serde_json::json!(-1.);
        assert_ne!(color_ui(serde_json::from_value(request).unwrap()).unwrap(),mapped);
        for (actual,expected) in color.linear_in(RgbSpace::DisplayP3).unwrap().into_iter().zip([4.,2.,0.5,0.5]) {assert!((actual-expected).abs()<1e-6);}
    }

}
