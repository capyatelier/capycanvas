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
    FormCopy { copy: ColorFormCopy },
    Preview {
        colors: Vec<RgbColor>,
        #[serde(default)]
        document_space: RgbSpace,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
    },
    Gradient {
        gradient: layer_core::GradientDefinition,
        document_space: RgbSpace,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
        image: Option<GradientPreview>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientPreview {
    size: [u32;2],
    depth: layer_core::color::SampleDepth,
}
#[cfg(test)]
fn color_ui(request: ColorUiRequest) -> Result<serde_json::Value, String> { color_ui_localized(request, &crate::Localizer::shared(crate::UiLanguage::English)) }

pub fn color_ui_localized(request: ColorUiRequest, localizer: &crate::localization::Localizer) -> Result<serde_json::Value, String> {
    let value = match request {
        ColorUiRequest::IntensityPoint {size,point,minimum,maximum} => {
            if !point.iter().all(|v|v.is_finite()) || !minimum.is_finite() || !maximum.is_finite() || minimum>=maximum || minimum < -149. || maximum>128. {return Err("Invalid intensity range".into());}
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            return Ok(serde_json::json!(minimum+a.fraction(point)*(maximum-minimum)));
        },
        ColorUiRequest::IntensityArc {size,stops,depth,base,document_space,recipe,headroom} => {
            super::hdr_picker::HdrPaint::validate_stops(stops).map_err(|reason|reason.message(ColorInputModel::DocumentRgb, localizer))?;
            let a=HdrIntensityArc::new(size).ok_or("Invalid picker extent")?;
            let minimum=(-2f32).min(stops.floor()); let limit=if depth==Some(layer_core::color::SampleDepth::F32) {128.} else {65504f32.log2()}; let maximum=6f32.max(stops.ceil()).min(limit);
            let samples=(0..=80).map(|i|{
                let t=i as f32/80.;
                let p=super::hdr_picker::scale_linear(base.linear_in(document_space)?,minimum+t*(maximum-minimum)).map(|c|c.clamp(-f32::MAX,f32::MAX));
                let color=RgbColor::from_linear(document_space,p)?;
                crate::color_management::picker_preview(color,document_space,recipe,headroom)
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
        ColorUiRequest::PrintProof {settings} => return serde_json::to_value(settings.recipe().map_err(|reason|reason.proof_message(localizer))?).map_err(|e|e.to_string()),
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
        ColorUiRequest::Form { request } => serde_json::to_value(color_form_localized(request, localizer)?),
        ColorUiRequest::FormCopy { copy } => serde_json::to_value(copy.localized(localizer)?),
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
            gradient,
            document_space,
            display_space,
            rendition,
            image,
        } => {
            if let Some(GradientPreview {size,depth})=image {
                if size[0]>2048 || size[1]>64 {return Err("Invalid gradient preview size".into());}
                let mut mapped=std::collections::HashMap::new();
                let pixels=gradient.preview(size,document_space,depth)?.into_iter().map(|color| {
                    let key=(color.space,color.rgba.map(f32::to_bits));
                    if let Some(pixel)=mapped.get(&key) {return Ok(*pixel);}
                    let rgba=mapped_preview(color,document_space,display_space,rendition)?.rgba.map(|v|(v.clamp(0.,1.)*255.).round() as u32);
                    let pixel=rgba[3]<<24|rgba[0]<<16|rgba[1]<<8|rgba[2];mapped.insert(key,pixel);Ok(pixel)
                }).collect::<Result<Vec<u32>,String>>()?;
                return Ok(serde_json::json!({"size":size,"argb":pixels}));
            }
            let samples=gradient.samples((0..=256).map(|i|i as f32/256.),document_space)?.into_iter()
                .map(|color|mapped_preview(color,document_space,display_space,rendition)).collect::<Result<Vec<_>,String>>()?;
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
    pub copy: ColorFormCopy,
    pub draft: ColorFormRequest,
    pub models: Vec<(ColorInputModel, std::sync::Arc<str>)>,
    pub labels: [std::sync::Arc<str>; 4],
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorValidationCopy {
    pub space: RgbSpace,
    pub display_space: RgbSpace,
    pub outside_document: bool,
    pub outside_display: bool,
    pub above_white: bool,
}
impl ColorValidationCopy {
    pub fn new(color: RgbColor, document: RgbSpace, display: RgbSpace, hdr: bool) -> Result<Self, String> {
        Ok(Self {
            space:color.space, display_space:display,
            outside_document:!if hdr { color.in_hdr_gamut(document)? } else { color.in_gamut(document)? },
            outside_display:!if hdr { color.in_hdr_gamut(display)? } else { color.in_gamut(display)? },
            above_white:hdr && color.brightness_ev(document)?.is_some_and(|v| v > 0.00001),
        })
    }
    pub fn message(&self, localizer: &crate::Localizer) -> String {
        use crate::MessageId;
        let defined = match self.space {
            RgbSpace::Srgb => MessageId::COLOR_DEFINED_SRGB,
            RgbSpace::DisplayP3 => MessageId::COLOR_DEFINED_DISPLAY_P3,
            RgbSpace::AdobeRgb => MessageId::COLOR_DEFINED_ADOBE_RGB,
            RgbSpace::ProPhoto => MessageId::COLOR_DEFINED_PROPHOTO,
        };
        let mut text = localizer.text(defined).to_string();
        let mut append = |id| { text.push(' '); text.push_str(&localizer.text(id)); };
        if self.outside_document { append(MessageId::COLOR_OUTSIDE_DOCUMENT_GAMUT); }
        if self.outside_display {
            append(match self.display_space {
                RgbSpace::Srgb => MessageId::COLOR_OUTSIDE_SRGB_PREVIEW_GAMUT,
                RgbSpace::DisplayP3 => MessageId::COLOR_OUTSIDE_DISPLAY_P3_PREVIEW_GAMUT,
                RgbSpace::AdobeRgb => MessageId::COLOR_OUTSIDE_ADOBE_RGB_PREVIEW_GAMUT,
                RgbSpace::ProPhoto => MessageId::COLOR_OUTSIDE_PROPHOTO_PREVIEW_GAMUT,
            });
        }
        if self.above_white { append(MessageId::COLOR_ABOVE_SDR_WHITE); }
        text
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorFormCopy {
    pub model: ColorInputModel,
    pub document_space: RgbSpace,
    pub validation: Option<ColorValidationCopy>,
    pub error: Option<ColorEditorError>,
}
#[derive(Serialize)]
pub struct ColorFormCopyView {
    pub models: Vec<(ColorInputModel, std::sync::Arc<str>)>,
    pub labels: [std::sync::Arc<str>;4],
    pub description: String,
    pub validation: Option<String>,
    pub error: Option<String>,
}
impl ColorFormCopy {
    pub fn localized(&self, localizer: &crate::Localizer) -> Result<ColorFormCopyView, String> {
        if self.error.as_ref().is_some_and(|reason| !reason.valid()) { return Err("Invalid color form copy".into()); }
        Ok(ColorFormCopyView {
            models:ColorInputModel::ALL.into_iter().map(|model|(model,model.localized_name(localizer))).collect(),
            labels:self.model.localized_labels(localizer),
            description:self.model.localized_description(self.document_space, localizer),
            validation:self.validation.as_ref().map(|copy|copy.message(localizer)),
            error:self.error.as_ref().map(|reason|reason.message(self.model, localizer)),
        })
    }
}

pub fn color_validation_localized(color: RgbColor, document: RgbSpace, display: RgbSpace, hdr: bool, localizer: &crate::Localizer) -> Result<String, String> {
    Ok(ColorValidationCopy::new(color, document, display, hdr)?.message(localizer))
}

#[cfg(test)]
fn color_form(request: ColorFormRequest) -> Result<ColorFormView, String> { color_form_localized(request, &crate::localization::Localizer::shared(crate::localization::UiLanguage::English)) }

pub fn color_form_localized(request: ColorFormRequest, localizer: &crate::localization::Localizer) -> Result<ColorFormView, String> {
    let mut editor = ColorEditor::new(request.color, request.document_space)?;
    if let Some(depth)=request.document_depth {editor.set_document_depth(depth);}
    editor.set_model(request.model).map_err(|reason|reason.message(editor.model(),localizer))?;
    let intensity=request.intensity.or_else(||request.document_depth.filter(|d|d.is_float()).map(|_|request.color.brightness_ev(request.document_space).ok().flatten().unwrap_or(0.).max(0.)));
    if let Some(stops) = intensity { editor.enable_hdr(stops).map_err(|reason|reason.message(editor.model(),localizer))?; }
    if let Some(fields) = request.fields {
        for (i, text) in fields.into_iter().enumerate() {
            editor.set_field(i, text)?;
        }
    }
    let mut error = None;
    let typed_intensity = request.change_intensity_text.as_deref().map(super::editor::color_intensity_input_typed).transpose();
    let change_intensity = match typed_intensity { Ok(value) => value.or(request.change_intensity), Err(message) => { error = Some(message); None } };
    let mut value = match editor.color() {
        Ok(color) => Some(color),
        Err(reason) => {
            error = Some(reason);
            None
        }
    };
    if let Some(color) = value {
        if let Some(stops) = change_intensity {
            match editor.set_intensity_color(stops,color) {
                Ok(color) => value=Some(color),
                Err(reason) => error=Some(reason),
            }
        }
        if let Some(model) = request.change_model {
            match editor.set_model_color(model,value.unwrap()) {
                Ok(()) => value=Some(editor.definition()),
                Err(reason) => error=Some(reason),
            }
        }
    }
    let base = value.and_then(|color|editor.intensity().and_then(|_|editor.base_for_color(color).ok()));
    let preview = value
        .map(|color| mapped_preview(color, request.document_space, request.display_space, request.rendition))
        .transpose()?;
    let has_error = error.is_some();
    let copy = ColorFormCopy {
        model:editor.model(), document_space:request.document_space,
        validation:value.map(|color|ColorValidationCopy::new(color, request.document_space, request.display_space, editor.intensity().is_some())).transpose()?,
        error,
    };
    let captions = copy.localized(localizer)?;
    Ok(ColorFormView {
        copy,
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
            change_intensity_text: request.change_intensity_text.clone().filter(|_| has_error),
            rendition: request.rendition,
        },
        models:captions.models,
        labels:captions.labels,
        description:captions.description,
        validation:captions.validation,
        value,
        preview,
        base,
        base_preview: base.map(|c| mapped_preview(c,request.document_space,request.display_space,request.rendition)).transpose()?,
        error:captions.error,
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
    fn copied(view: &ColorFormView, localizer: &crate::Localizer) -> serde_json::Value {
        let copy = serde_json::to_value(&view.copy).unwrap();
        assert!(copy.to_string().len() < 4096);
        let request = serde_json::json!({"type":"form_copy","copy":copy});
        color_ui_localized(serde_json::from_value(request).unwrap(), localizer).unwrap()
    }
    fn assert_copy(view: &ColorFormView, copy: &serde_json::Value) {
        for key in ["models","labels","description","validation","error"] {
            assert_eq!(copy[key], serde_json::to_value(view).unwrap()[key], "{key}");
        }
        assert_eq!(copy.as_object().unwrap().len(), 5);
        for key in ["fields","value","preview","base_preview","draft","base","copy"] { assert!(!copy.as_object().unwrap().contains_key(key)); }
    }
    #[test]
    fn color_form_copy_reprojects_every_model_without_draft_values_or_preview_work() {
        let english = crate::Localizer::shared(crate::UiLanguage::English);
        for model in ColorInputModel::ALL {
            for document in RgbSpace::ALL {
                for (color, hdr) in [
                    (RgbColor::new(RgbSpace::DisplayP3,[1.,0.01,0.23,1./65535.]).unwrap(),false),
                    (RgbColor::from_linear(RgbSpace::Srgb,[-0.2,4.,1.,0.3]).unwrap(),true),
                ] {
                    let mut source=request(color);source.document_space=document;source.model=model;
                    if hdr { source.document_depth=Some(layer_core::color::SampleDepth::F32); }
                    let view=color_form_localized(source.clone(),&english).unwrap();
                    let unchanged=serde_json::to_value(&view).unwrap();
                    for language in crate::UiLanguage::ALL {
                        let localizer=crate::Localizer::shared(language);
                        let formatted=copied(&view,&localizer);
                        let actual=color_form_localized(source.clone(),&localizer).unwrap();
                        assert_copy(&actual,&formatted);
                        assert_eq!(serde_json::to_value(&view).unwrap(),unchanged);
                        assert_eq!(serde_json::to_value(&actual.draft).unwrap(),unchanged["draft"]);
                        for key in ["value","preview","base_preview","base"] { assert_eq!(serde_json::to_value(&actual).unwrap()[key],unchanged[key]); }
                    }
                    let mut edited=view.draft.clone();edited.fields.as_mut().unwrap()[0]="literal İı ไทย { $field } unfinished".into();
                    let invalid=color_form_localized(edited.clone(),&english).unwrap();
                    assert!(invalid.copy.error.is_some());
                    let retained=serde_json::to_value(&invalid).unwrap();
                    for language in crate::UiLanguage::ALL {
                        let localizer=crate::Localizer::shared(language);
                        assert_copy(&color_form_localized(edited.clone(),&localizer).unwrap(),&copied(&invalid,&localizer));
                        assert_eq!(serde_json::to_value(&invalid).unwrap(),retained);
                    }
                }
            }
        }
    }
    #[test]
    fn color_form_copy_retains_invalid_and_range_refused_hdr_intensity_sources() {
        let english=crate::Localizer::shared(crate::UiLanguage::English);
        for text in ["literal İı ไทย { $stops }", "NaN", "18"] {
            let mut source=request(RgbColor::WHITE);source.document_depth=Some(layer_core::color::SampleDepth::F16);source.intensity=Some(0.);source.change_intensity_text=Some(text.into());
            let view=color_form_localized(source.clone(),&english).unwrap();
            assert!(view.copy.error.is_some());
            assert_eq!(view.draft.change_intensity_text.as_deref(),Some(text));
            let retained=serde_json::to_value(&view).unwrap();
            for language in crate::UiLanguage::ALL {
                let localizer=crate::Localizer::shared(language);
                assert_copy(&color_form_localized(source.clone(),&localizer).unwrap(),&copied(&view,&localizer));
                assert_eq!(serde_json::to_value(&view).unwrap(),retained);
            }
        }
    }

    #[test]
    fn color_form_copy_validates_typed_errors_and_preserves_literal_diagnostics() {
        let literal="literal İı ไทย Tie\u{302}\u{301}ng { $name }\n{\"type\":\"numeric_error\"} 🎨";
        let errors=[
            ColorEditorError::Numeric { field:2,reason:crate::NumericError::InvalidNumber },
            ColorEditorError::EntriesTooLong, ColorEditorError::AlphaRange, ColorEditorError::HexSyntax,
            ColorEditorError::PercentRange, ColorEditorError::NegativeLightnessChroma,
            ColorEditorError::Intensity(crate::NumericError::FiniteNumber),
            ColorEditorError::Hdr(layer_core::color::hdr::HdrPixelError::StorageRange),
            ColorEditorError::Detail(literal.into()),
        ];
        for model in ColorInputModel::ALL {
            for error in &errors {
                let copy=ColorFormCopy {model,document_space:RgbSpace::AdobeRgb,validation:None,error:Some(error.clone())};
                let wire=serde_json::json!({"type":"form_copy","copy":copy});
                for language in crate::UiLanguage::ALL {
                    let localizer=crate::Localizer::shared(language);
                    let formatted=color_ui_localized(serde_json::from_value(wire.clone()).unwrap(),&localizer).unwrap();
                    assert_eq!(formatted["error"],error.message(model,&localizer));
                    if matches!(error,ColorEditorError::Detail(_)) { assert_eq!(formatted["error"],literal); }
                    assert_eq!(serde_json::to_value(&copy).unwrap(),wire["copy"]);
                }
            }
        }
        let localizer=crate::Localizer::shared(crate::UiLanguage::English);
        for error in [
            ColorEditorError::Numeric {field:4,reason:crate::NumericError::InvalidNumber},
            ColorEditorError::Intensity(crate::NumericError::Range {label:"literal".into(),min:2.,max:1.}),
            ColorEditorError::Numeric {field:0,reason:crate::NumericError::WholePixels {label:layer_core::ResourceLabel::Message {message:"unknown-color-label".into()}}},
        ] {
            assert!(!error.valid());
            let copy=ColorFormCopy {model:ColorInputModel::DocumentRgb,document_space:RgbSpace::Srgb,validation:None,error:Some(error)};
            assert!(color_ui_localized(ColorUiRequest::FormCopy {copy},&localizer).is_err());
        }
        let copy=ColorFormCopy {model:ColorInputModel::Hls,document_space:RgbSpace::Srgb,validation:None,error:None};
        let valid=serde_json::json!({"type":"form_copy","copy":copy});
        let mut malformed=valid.clone();malformed["copy"]["fields"]=serde_json::json!(["bad","bad","bad","bad"]);
        assert!(serde_json::from_value::<ColorUiRequest>(malformed).is_err());
        let mut malformed=valid.clone();malformed["request"]=serde_json::json!({"color":"bad"});
        assert!(serde_json::from_value::<ColorUiRequest>(malformed).is_err());
        let mut malformed=valid.clone();malformed["copy"]["model"]=serde_json::json!("unknown");
        assert!(serde_json::from_value::<ColorUiRequest>(malformed).is_err());
        let mut malformed=valid.clone();malformed["copy"]["error"]=serde_json::json!({"type":"unknown"});
        assert!(serde_json::from_value::<ColorUiRequest>(malformed).is_err());
        let mut malformed=valid;malformed["copy"]["validation"]=serde_json::json!({"space":"Srgb","display_space":"Srgb","outside_document":false,"outside_display":false,"above_white":false,"color":"forbidden"});
        assert!(serde_json::from_value::<ColorUiRequest>(malformed).is_err());
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
        assert!(!color.in_hdr_gamut(RgbSpace::Srgb).unwrap());
        assert!(form.validation.as_deref().unwrap().contains("Above SDR white."));
        assert!(form.validation.as_deref().unwrap().contains("Outside the document gamut."));
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

    fn gradient_image(gradient:layer_core::GradientDefinition,size:[u32;2],depth:layer_core::color::SampleDepth,rendition:Option<layer_core::color::hdr::SdrRendition>)->Result<serde_json::Value,String> {
        color_ui(serde_json::from_value(serde_json::json!({"type":"gradient","gradient":gradient,"document_space":"Srgb","image":{"size":size,"depth":depth},"rendition":rendition})).unwrap())
    }
    #[test]
    fn gradient_image_transport_preserves_two_dimensional_noise_and_float_columns() {
        use layer_core::color::SampleDepth;
        let gradient=layer_core::GradientDefinition::default();
        let integer=gradient_image(gradient.clone(),[257,16],SampleDepth::U8,None).unwrap();
        let float=gradient_image(gradient,[257,16],SampleDepth::F32,None).unwrap();
        assert_eq!(integer["size"],serde_json::json!([257,16]));
        assert_eq!(integer.as_object().unwrap().len(),2);
        let pixels=integer["argb"].as_array().unwrap();let floats=float["argb"].as_array().unwrap();
        assert_eq!(pixels.len(),257*16);
        assert!(pixels.iter().all(|v|v.as_u64().is_some_and(|v|v<=u32::MAX as u64)));
        assert!(pixels.chunks(257).skip(1).any(|row|row!=&pixels[..257]));
        for row in floats.chunks(257) {assert_eq!(row,&floats[..257]);}
        for row in pixels.chunks(257) {assert_eq!(row[0],serde_json::json!(0xff000000u32));assert_eq!(row[256],serde_json::json!(0xffffffffu32));}
    }
    #[test]
    fn gradient_image_transport_keeps_constant_alpha_and_maps_hdr() {
        use layer_core::{GradientDefinition,GradientStop,color::SampleDepth};
        let color=RgbColor::new(RgbSpace::Srgb,[1.,0.,0.,0.5]).unwrap();
        let gradient=GradientDefinition::new(vec![GradientStop {position:0.,color},GradientStop {position:1.,color}]);
        let image=gradient_image(gradient,[9,4],SampleDepth::U8,None).unwrap();
        assert!(image["argb"].as_array().unwrap().iter().all(|pixel|*pixel==serde_json::json!(0x80ff0000u32)));
        let color=RgbColor::from_linear(RgbSpace::Srgb,[4.,2.,0.5,0.5]).unwrap();
        let gradient=GradientDefinition::new(vec![GradientStop {position:0.,color},GradientStop {position:1.,color}]);
        let mapped=gradient_image(gradient.clone(),[9,4],SampleDepth::F32,Some(Default::default())).unwrap();
        let raw=gradient_image(gradient,[9,4],SampleDepth::F32,None).unwrap();
        assert_ne!(mapped,raw);
        assert!(mapped["argb"].as_array().unwrap().iter().all(|pixel|pixel.as_u64().unwrap()>>24==128));
        assert_eq!(mapped["argb"][0],mapped["argb"][35]);
    }
    #[test]
    fn gradient_image_transport_bounds_size_and_preserves_array_api() {
        use layer_core::color::SampleDepth;
        for size in [[0,1],[1,0],[2049,1],[1,65]] {assert!(gradient_image(Default::default(),size,SampleDepth::F32,None).is_err());}
        for size in [[1,1],[2048,1],[1,64]] {assert_eq!(gradient_image(Default::default(),size,SampleDepth::F32,None).unwrap()["argb"].as_array().unwrap().len(),size[0] as usize*size[1] as usize);}
        let array=color_ui(serde_json::from_value(serde_json::json!({"type":"gradient","gradient":layer_core::GradientDefinition::default(),"document_space":"Srgb"})).unwrap()).unwrap();
        assert_eq!(array.as_array().unwrap().len(),257);assert!(array[0]["rgba"].is_array());
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
        let gradient=color_ui(serde_json::from_value(serde_json::json!({"type":"gradient","gradient":{"stops":[{"position":0.,"color":color},{"position":1.,"color":color}],"interpolation":"Oklab"},"document_space":"DisplayP3","rendition":recipe})).unwrap()).unwrap();
        // Gradient interpolation returns through encoded document RGB; allow Float32 roundoff.
        for at in [0,256] {for channel in 0..4 {assert!((gradient[at]["rgba"][channel].as_f64().unwrap()-mapped[0]["rgba"][channel].as_f64().unwrap()).abs()<1e-6);}}
        request["rendition"]["exposure"]=serde_json::json!(-1.);
        assert_ne!(color_ui(serde_json::from_value(request).unwrap()).unwrap(),mapped);
        for (actual,expected) in color.linear_in(RgbSpace::DisplayP3).unwrap().into_iter().zip([4.,2.,0.5,0.5]) {assert!((actual-expected).abs()<1e-6);}
    }

}
