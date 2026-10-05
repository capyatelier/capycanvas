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
    EditorOpen {
        #[serde(default)]
        colors: ColorState,
        #[serde(default)]
        slot: Option<ColorSlot>,
        #[serde(default)]
        color: Option<RgbColor>,
        #[serde(default)]
        opaque: bool,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
    },
    Editor {
        editor: ColorEditor,
        #[serde(default)]
        action: Option<ColorEditorAction>,
        #[serde(default)]
        display_space: RgbSpace,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
    },
    EditorStrip { editor: ColorEditor, sample: RgbColor },
    HueStops { shape: ColorShape, space: RgbSpace },
    WheelHit { size: f32, point: [f32; 2], shape: ColorShape },
    StripPlacement { area: [f32; 4], size: [f32; 2], scale: f32, avoid: Vec<[f32; 2]>, corner: ColorStripCorner },
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
            super::hdr_picker::HdrPaint::validate_stops(stops).map_err(|reason|reason.message(localizer))?;
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
        ColorUiRequest::EditorOpen { colors, slot, color, opaque, display_space, rendition } => {
            colors.validate()?;
            let editor = match (slot, color) {
                (Some(slot), None) => ColorEditor::for_slot(&colors, slot),
                (None, Some(color)) => ColorEditor::for_color(&colors, color, opaque),
                _ => return Err("Edit either a paint slot or a color".into()),
            }
            .map_err(|reason| reason.message(localizer))?;
            editor_response(editor, None, display_space, rendition, localizer)
        }
        ColorUiRequest::Editor { editor, action, display_space, rendition } => {
            editor.validate()?;
            editor_response(editor, action, display_space, rendition, localizer)
        }
        ColorUiRequest::EditorStrip { editor, sample } => {
            editor.validate()?;
            serde_json::to_value(editor.strip(sample)?)
        }
        ColorUiRequest::HueStops { shape, space } => serde_json::to_value(ColorState::hue_stops(shape, space)),
        ColorUiRequest::WheelHit { size, point, shape } => {
            serde_json::to_value(ColorWheelGeometry::new(size).and_then(|g| g.hit_shape(point, shape)))
        }
        ColorUiRequest::StripPlacement { area, size, scale, avoid, corner } => {
            serde_json::to_value(ColorStripPlacement::new(area, size, scale, &avoid, corner))
        }
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

fn editor_response(
    mut editor: ColorEditor,
    action: Option<ColorEditorAction>,
    display: RgbSpace,
    rendition: Option<layer_core::color::hdr::SdrRendition>,
    localizer: &crate::Localizer,
) -> Result<serde_json::Value, serde_json::Error> {
    let error = action.and_then(|action| editor.apply(action).err()).map(|reason| reason.message(localizer));
    let view = editor.view(display, rendition, localizer).map_err(serde::ser::Error::custom)?;
    Ok(serde_json::json!({"editor": editor, "view": view, "error": error}))
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

pub fn color_validation_localized(color: RgbColor, document: RgbSpace, display: RgbSpace, hdr: bool, localizer: &crate::Localizer) -> Result<String, String> {
    Ok(ColorValidationCopy::new(color, document, display, hdr)?.message(localizer))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor_json(request: serde_json::Value) -> serde_json::Value {
        color_ui(serde_json::from_value(request).unwrap()).unwrap()
    }
    #[test]
    fn stateless_editor_round_trips_actions_errors_and_strips() {
        let colors = ColorState::default();
        let opened = editor_json(serde_json::json!({"type":"editor_open","colors":colors,"slot":"foreground","display_space":"Srgb"}));
        assert!(opened["error"].is_null());
        assert_eq!(opened["view"]["rows"].as_array().unwrap().len(), 3);
        let edited = editor_json(serde_json::json!({"type":"editor","editor":opened["editor"],"action":{"op":"text","text":"#3B7EA1"}}));
        assert_eq!(edited["view"]["hex"], "#3B7EA1");
        assert_eq!(edited["view"]["changed"], true);
        let refused = editor_json(serde_json::json!({"type":"editor","editor":edited["editor"],"action":{"op":"value","row":0,"index":0,"text":"lots"}}));
        assert!(refused["error"].as_str().unwrap().contains("Red"));
        assert_eq!(refused["editor"], edited["editor"]);
        let strip = editor_json(serde_json::json!({"type":"editor_strip","editor":edited["editor"],"sample":RgbColor::WHITE}));
        assert_eq!(strip["hex"], "#FFFFFF");
        assert_eq!(strip["label"], "OKLCH");
        let mut tampered = edited["editor"].clone();
        tampered["picker"]["editor"]["search"] = serde_json::json!("x".repeat(300));
        assert!(color_ui(serde_json::from_value(serde_json::json!({"type":"editor","editor":tampered})).unwrap()).is_err());
        assert!(color_ui(serde_json::from_value(serde_json::json!({"type":"editor_open","colors":colors,"slot":"foreground","color":RgbColor::WHITE})).unwrap()).is_err());
        let opaque = editor_json(serde_json::json!({"type":"editor_open","colors":colors,"color":RgbColor::new(RgbSpace::Srgb,[1.,0.,0.,0.25]).unwrap(),"opaque":true}));
        assert_eq!(opaque["view"]["value"]["rgba"][3], 1.);
        let wheel = |shape: &str| {
            let picker = &editor_json(serde_json::json!({"type":"editor","editor":edited["editor"],"action":{"op":"wheel","action":{"op":"shape","shape":shape}}}))["view"]["panel"];
            let stops = editor_json(serde_json::json!({"type":"hue_stops","shape":picker["shape"],"space":picker["rgb_space"]}));
            let size = 200.;
            let geometry = ColorWheelGeometry::new(size).unwrap();
            let center = editor_json(serde_json::json!({"type":"wheel_hit","size":size,"point":geometry.center,"shape":shape}));
            let ring = editor_json(serde_json::json!({"type":"wheel_hit","size":size,"point":[geometry.center[0] + (geometry.inner + geometry.outer) * 0.5, geometry.center[1]],"shape":shape}));
            let outside = editor_json(serde_json::json!({"type":"wheel_hit","size":size,"point":[0., 0.],"shape":shape}));
            (stops.as_array().unwrap().len(), center, ring, outside)
        };
        let (circle_stops, circle_center, circle_ring, outside) = wheel("circle");
        let (square_stops, square_center, square_ring, _) = wheel("square");
        assert_eq!(circle_stops, ColorState::hue_stops(ColorShape::Circle, RgbSpace::Srgb).len());
        assert_eq!(square_stops, ColorState::hue_stops(ColorShape::Square, RgbSpace::Srgb).len());
        assert_ne!(circle_stops, square_stops);
        assert!(!circle_center.is_null() && !square_center.is_null() && circle_center == square_center);
        assert!(!circle_ring.is_null() && circle_ring == square_ring && circle_ring != circle_center);
        assert!(outside.is_null());
        let strip = editor_json(serde_json::json!({"type":"strip_placement","area":[0., 0., 800., 600.],"size":[272., 64.],"scale":1.,"avoid":[[700., 40.]],"corner":"top_right"}));
        assert_eq!(strip, serde_json::to_value(ColorStripPlacement::new([0., 0., 800., 600.], [272., 64.], 1., &[[700., 40.]], ColorStripCorner::TopRight)).unwrap());
        assert_eq!(strip["corner"], "top_left");
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
