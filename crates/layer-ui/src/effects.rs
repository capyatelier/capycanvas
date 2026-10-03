//! Effect catalog, properties and navigation policy shared by every native view.
use super::*;
#[path = "effects/curves.rs"]
mod curves;
pub use curves::{CurveAxis,CurveAxisView,CurveControls,CurveCoordinateControl,CurveDomain};
pub(super) use curves::PropertyEditorState;
use layer_core::{Edit, EffectInstance, EffectParameterKind, EffectValue, Layer, ResourceLabel};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) fn blend_label(blend: layer_core::LayerBlend, l: &Localizer) -> Arc<str> {
    l.text(match blend {
        layer_core::LayerBlend::Normal => MessageId::RESOURCES_BLEND_NORMAL,
        layer_core::LayerBlend::Multiply => MessageId::RESOURCES_BLEND_MULTIPLY,
        layer_core::LayerBlend::Screen => MessageId::RESOURCES_BLEND_SCREEN,
        layer_core::LayerBlend::Add => MessageId::RESOURCES_BLEND_ADD,
        layer_core::LayerBlend::Overlay => MessageId::RESOURCES_BLEND_OVERLAY,
        layer_core::LayerBlend::SoftLight => MessageId::RESOURCES_BLEND_SOFT_LIGHT,
        layer_core::LayerBlend::Color => MessageId::RESOURCES_BLEND_COLOR,
        layer_core::LayerBlend::Darken => MessageId::RESOURCES_BLEND_DARKEN,
        layer_core::LayerBlend::Lighten => MessageId::RESOURCES_BLEND_LIGHTEN,
        layer_core::LayerBlend::ColorBurn => MessageId::RESOURCES_BLEND_COLOR_BURN,
        layer_core::LayerBlend::LinearBurn => MessageId::RESOURCES_BLEND_LINEAR_BURN,
        layer_core::LayerBlend::ColorDodge => MessageId::RESOURCES_BLEND_COLOR_DODGE,
        layer_core::LayerBlend::HardLight => MessageId::RESOURCES_BLEND_HARD_LIGHT,
        layer_core::LayerBlend::VividLight => MessageId::RESOURCES_BLEND_VIVID_LIGHT,
        layer_core::LayerBlend::LinearLight => MessageId::RESOURCES_BLEND_LINEAR_LIGHT,
        layer_core::LayerBlend::PinLight => MessageId::RESOURCES_BLEND_PIN_LIGHT,
        layer_core::LayerBlend::HardMix => MessageId::RESOURCES_BLEND_HARD_MIX,
        layer_core::LayerBlend::Difference => MessageId::RESOURCES_BLEND_DIFFERENCE,
        layer_core::LayerBlend::Exclusion => MessageId::RESOURCES_BLEND_EXCLUSION,
        layer_core::LayerBlend::Subtract => MessageId::RESOURCES_BLEND_SUBTRACT,
        layer_core::LayerBlend::Divide => MessageId::RESOURCES_BLEND_DIVIDE,
        layer_core::LayerBlend::Hue => MessageId::RESOURCES_BLEND_HUE,
        layer_core::LayerBlend::Saturation => MessageId::RESOURCES_BLEND_SATURATION,
        layer_core::LayerBlend::Luminosity => MessageId::RESOURCES_BLEND_LUMINOSITY,
        layer_core::LayerBlend::PassThrough => MessageId::RESOURCES_BLEND_PASS_THROUGH,
    })
}
pub(super) fn resource_label(label: &ResourceLabel, l: &Localizer) -> Arc<str> {
    match label {
        ResourceLabel::Literal(text) => text.clone(),
        ResourceLabel::Message { message } => l.static_message(message)
            .map(|id| l.text(id)).unwrap_or_else(|| l.text(MessageId::COMMON_ERROR)),
    }
}
fn validate_label(label: &ResourceLabel, l: &Localizer) -> Result<(), String> {
    if let ResourceLabel::Message { message } = label
        && l.static_message(message).is_none()
    {
        return Err(l.text(MessageId::RESOURCES_INVALID_MESSAGE).to_string());
    }
    Ok(())
}
fn validate_program_labels(program: &layer_core::EffectProgram, l: &Localizer) -> Result<(), String> {
    validate_label(&program.label, l)?;
    for page in program.pages.iter(){validate_label(&page.label,l)?;}
    for parameter in program.parameters.iter() {
        validate_label(&parameter.label, l)?;
        if let Some(section) = &parameter.section { validate_label(section, l)?; }
        if let EffectParameterKind::Choice { options } = &parameter.kind {
            for option in options.iter() {
                if let layer_core::EffectOption::Labeled { label, .. } = option { validate_label(label, l)?; }
            }
        }
    }
    Ok(())
}
pub(super) fn validate_catalog_labels(catalog: &layer_core::EffectCatalog, l: &Localizer) -> Result<(), String> {
    for category in catalog.categories() { validate_label(&category.label, l)?; }
    for filter in catalog.filters() { validate_program_labels(&filter.program, l)?; }
    Ok(())
}
pub(super) fn validate_document_labels(document: &Document, l: &Localizer) -> Result<(), String> {
    for layer in &document.layers {
        if let Some(effect) = &layer.effect { validate_program_labels(&effect.program, l)?; }
    }
    Ok(())
}

impl<B: CanvasRenderer> UiSession<B> {
    pub(crate) fn update_shader_idle(&mut self) {
        let idle = self.filter_previews_idle() && !self.state.settings_open;
        self.engine.backend_mut().shader_idle(idle);
    }
    pub(crate) fn filter_drawer_open(&self) -> bool {
        self.state.customization.drawer.as_ref().is_some_and(|d|
            d.columns.iter().flatten().any(|p| *p == Panel::FilterTypes))
    }
    pub(crate) fn open_filter_picker(&mut self) {
        let current = self.engine.document().layer(self.engine.document().active_layer)
            .and_then(|l| l.effect.as_ref()).and_then(|e| self.effect_catalog.get(&e.program.id));
        self.state.filter_picker.category = current.map(|e| e.category.clone())
            .or_else(|| self.state.filter_picker.category.clone())
            .or_else(|| self.effect_catalog.categories().first().map(|c| c.id.clone()));
        self.state.filter_picker.search = None;
        self.state.adjustments = catalog(&self.effect_catalog, &self.state.filter_picker, &self.state.localization);
    }
    /// Optional preview work yields to delivered input and unfinished edits.
    /// Apply this to completion service as well as admission: taking a preview
    /// can submit the next source-probe chunk.
    pub fn filter_previews_idle(&self) -> bool {
        !self.rendering_suspended
            && !self.input_pending
            && !self.touch.is_active()
            && self.interaction.pointer.is_none()
            && self.effect_gesture.is_none()
            && !self.engine.has_active_stroke()
            && !self.engine.has_pending_document_edits()
    }
    pub fn background_readback_idle(&self) -> bool {
        self.filter_previews_idle() && !self.wants_continuous_frames()
    }
    pub fn filter_preview_revision(&self) -> (u64, u64, u64) {
        let doc = self.engine.document();
        (
            doc.revision,
            doc.active_layer.0,
            self.state.filter_catalog_revision ^ (u64::from(self.filter_drawer_open()) << 63),
        )
    }

    /// Hosts request only rows on screen, after painting and pending edits end.
    pub fn request_filter_previews(
        &mut self,
        request_id: u64,
        filters: Vec<Arc<str>>,
        size: [u32; 2],
    ) -> Result<bool, String> {
        if !self.filter_previews_idle() {
            return Ok(false);
        }
        let doc = self.engine.document();
        let replacing = self.filter_drawer_open() && doc.layer(doc.active_layer).is_some_and(|l| l.effect.is_some());
        let Some(current) = doc.layer(doc.active_layer) else { return Ok(false); };
        let target = if replacing {
            doc.layers.iter().skip_while(|l| l.id != current.id).skip(1)
                .find(|l| l.properties.parent == current.properties.parent).map(|l| l.id)
        } else if self.filter_drawer_open() {
            Some(current.id)
        } else { doc.clipping_stack_top(current.id) }.ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_SELECT_LAYER).to_string())?;
        let request = layer_render::FilterPreviewRequest {
            request_id,
            target,
            size,
            extent: [doc.width, doc.height],
            view: self.engine.view(),
            blend_space: doc.blend_space,
            layers: doc.layers.iter().filter(|l| !replacing || l.id != current.id)
                .map(Layer::composite_snapshot).collect(),
            filters: filters
                .into_iter()
                .take(8)
                .map(|id| {
                    self.effect_catalog
                        .get(&id)
                        .ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_FILTER).to_string())?
                        .preview()
                        .map(Arc::new)
                })
                .collect::<Result<_, _>>()?,
        };
        self.engine
            .backend_mut()
            .request_filter_previews(request)
            .map_err(|error| {
                eprintln!("Filter preview request: {error}");
                self.state.localization.text(MessageId::RESOURCES_PREVIEW_FAILED).to_string()
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FilterPickerState {
    pub selected: Option<Arc<str>>,
    pub category: Option<Arc<str>>,
    pub search: Option<String>,
    pub search_label: Arc<str>,
    pub empty_label: Arc<str>,
}
impl FilterPickerState {
    pub fn new(l: &Localizer) -> Self {
        Self {
            selected: None,
            category: None,
            search: None,
            search_label: l.text(MessageId::RESOURCES_SEARCH_FILTERS),
            empty_label: l.text(MessageId::RESOURCES_NO_MATCHING_FILTERS),
        }
    }
}
impl Default for FilterPickerState {
    fn default() -> Self { Self::new(&Localizer::shared(UiLanguage::English)) }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FilterPickerAction {
    Category { category: Option<Arc<str>> },
    Search { query: String },
    ToggleSearch,
}
#[derive(Clone, Debug, Serialize)]
pub struct FilterCategoryChoice {
    pub id: Option<Arc<str>>,
    pub label: Arc<str>,
    pub icon: &'static str,
}
fn category_icon(id: &str) -> &'static str {
    match id {
        "tone" => "levels",
        "color" => "hue_saturation",
        "detail" => "sharpen",
        "blur" => "blur",
        "artistic" => "paint",
        "distort" => "domain-warp",
        "texture" => "grain",
        "fill" => "fill",
        _ => "adjustments",
    }
}
pub(super) fn categories(catalog: &layer_core::EffectCatalog, l: &Localizer) -> Vec<FilterCategoryChoice> {
    std::iter::once(FilterCategoryChoice {
        id: None,
        label: l.text(MessageId::RESOURCES_ALL_FILTERS),
        icon: "adjustments",
    })
    .chain(catalog.categories().iter().map(|c| FilterCategoryChoice {
        id: Some(c.id.clone()),
        label: resource_label(&c.label, l),
        icon: category_icon(&c.id),
    }))
    .collect()
}
impl FilterPickerState {
    pub fn apply(&mut self, action: FilterPickerAction) {
        match action {
            FilterPickerAction::Category { category } => self.category = category,
            FilterPickerAction::Search { query } => {
                self.search = Some(query.chars().take(120).collect())
            }
            FilterPickerAction::ToggleSearch => {
                self.search = if self.search.is_some() {
                    None
                } else {
                    Some(String::new())
                }
            }
        }
    }
}

const CURVE_DETACH_MARGIN: f32 = 0.1;

fn point_between(value: f32, lower: f32, upper: f32) -> f32 {
    let gap = ((upper - lower) * 0.25).min(0.001);
    value.clamp(lower + gap, upper - gap)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EffectAction {
    WhiteBalancePicker { layer: u64, epoch: u64 },
    SelectPage { layer:u64, page:String },
    CurveSelectPoint { layer:u64, key:String, epoch:u64, index:Option<usize> },
    CurveRemoveAt { layer:u64, key:String, epoch:u64, point:[f32;2], extent:[f32;2], point_count:Option<usize> },
    CurveContact { layer:u64, key:String, epoch:u64, phase:ContactPhase, point:[f32;2], extent:[f32;2] },
    CurveKey { layer:u64, key:String, epoch:u64, key_event:String, pressed:bool, repeat:bool, modifiers:Modifiers },
    CurveNumber { layer:u64, key:String, epoch:u64, axis:CurveAxis, operation:NumericOperation },
    CancelFilter,
    UseCurrentColor { layer: u64, key: String },
    /// Live property editing uses the same validation as individual actions,
    /// with one history entry on release and restoration on cancellation.
    Gesture {
        phase: ContactPhase,
        action: Box<EffectAction>,
    },
    Insert {
        effect: Arc<str>,
    },
    Set {
        layer: u64,
        key: String,
        value: EffectValue,
    },
    Reset {
        layer: u64,
        key: String,
    },
    Number {
        layer: u64,
        key: String,
        operation: NumericOperation,
    },
    CurvePoint {
        layer: u64,
        key: String,
        index: Option<usize>,
        point: [f32; 2],
        remove: bool,
    },
    GradientStop {
        layer: u64,
        key: String,
        index: Option<usize>,
        position: f32,
        color: Option<layer_core::color::RgbColor>,
        remove: bool,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct AdjustmentChoice {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub icon: Arc<str>,
    pub action: UiAction,
    pub category: Arc<str>,
    pub category_label: Arc<str>,
    pub category_icon: &'static str,
    /// Capability, independent of whether a particular layer has frozen time.
    pub animated: bool,
    pub tooltip: String,
}
pub(super) fn catalog(
    catalog: &layer_core::EffectCatalog,
    picker: &FilterPickerState,
    l: &Localizer,
) -> Vec<AdjustmentChoice> {
    let query = crate::search::normalize(picker.search.as_deref().unwrap_or(""));
    let english = Localizer::shared(UiLanguage::English);
    catalog
        .categories()
        .iter()
        .flat_map(|category| {
            catalog
                .filters()
                .iter()
                .filter(move |definition| definition.category == category.id)
        })
        .filter(|definition| {
            picker
                .category
                .as_ref()
                .is_none_or(|c| definition.category == *c)
        })
        .filter(|id| {
            query.split_whitespace().all(|word|
                crate::search::normalize(&resource_label(id.label(), l)).contains(word)
                || crate::search::normalize(&resource_label(id.label(), &english)).contains(word))
        })
        .map(|id| AdjustmentChoice {
            id: id.program.id.clone(),
            label: resource_label(&id.program.label, l),
            icon: id.icon.clone(),
            action: UiAction::Effect {
                action: EffectAction::Insert {
                    effect: id.program.id.clone(),
                },
            },
            category: id.category.clone(),
            category_icon: category_icon(&id.category),
            category_label: resource_label(&catalog.categories().iter()
                .find(|c| c.id == id.category).unwrap().label, l),
            animated: id.program().time,
            tooltip: if id.program().time {
                { let mut args = fluent_bundle::FluentArgs::new();
                args.set("name", resource_label(id.label(), l).to_string());
                l.format(MessageId::RESOURCES_ANIMATED_TOOLTIP, &args) }
            } else {
                resource_label(id.label(), l).to_string()
            },
        })
        .collect()
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayerPropertiesView {
    pub actions: Vec<PropertyActionView>,
    pub pages:Vec<PropertyPageView>,
    pub page:Option<String>,
    pub epoch:u64,
    pub layer: Option<u64>,
    pub title: String,
    pub description: String,
    pub enabled: bool,
    pub controls: Vec<PropertyControl>,
    /// Linear input/output range for HDR curve axes; absent for encoded curves.
    pub curve_max: Option<f32>,
    pub curve_white: Option<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyActionView { pub label: String, pub action: EffectAction }
#[derive(Clone,Debug,PartialEq,Serialize)]
pub struct PropertyPageView { pub id:String, pub label:String }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyControl {
    pub curve:Option<CurveControls>,
    pub page:Option<String>,
    pub plot: Vec<[f32; 2]>,
    pub key: String,
    pub label: String,
    pub section: Option<String>,
    pub section_id: Option<ResourceLabel>,
    pub kind: PropertyKind,
    pub value: EffectValue,
    pub default: EffectValue,
    pub modified: bool,
    /// Optional shortcut alongside a color editor, supplied by shared policy.
    pub color_action: Option<UiAction>,
}
impl PropertyControl {
    pub(super) fn new(key: &str, label: &str, kind: PropertyKind, value: EffectValue, default: EffectValue) -> Self {
        Self {
            curve:None,
            page:None,
            plot: Vec::new(),
            key: key.into(),
            label: label.into(),
            section: None,
            section_id: None,
            kind,
            modified: value != default,
            value,
            default,
            color_action: None,
        }
    }
}
pub(super) fn property_value(
    view: &LayerPropertiesView,
    key: &str,
    action: &EffectAction,
    color: layer_core::color::RgbColor,
    l: &Localizer,
) -> Result<EffectValue, String> {
    let control = view.controls.iter().find(|c| c.key == key).ok_or_else(|| l.text(MessageId::RESOURCES_ERROR_UNKNOWN_PROPERTY).to_string())?;
    Ok(match action {
        EffectAction::Set { value, .. } => value.clone(),
        EffectAction::Reset { .. } => control.default.clone(),
        EffectAction::UseCurrentColor { .. } => EffectValue::Color(color),
        EffectAction::Number { operation, .. } => {
            let (PropertyKind::Number { numeric }, EffectValue::Number(value)) = (&control.kind, &control.value) else {
                return Err(l.text(MessageId::RESOURCES_ERROR_NUMERIC_PROPERTY_REQUIRED).to_string());
            };
            EffectValue::Number(numeric.resolve(*value as f64, operation.clone()).map_err(|reason| reason.message(l))?.value as f32)
        }
        _ => return Err(l.text(MessageId::RESOURCES_ERROR_PROPERTY_EDIT_REQUIRED).to_string()),
    })
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropertyKind {
    Number { numeric: NumericControl },
    Toggle,
    Choice { options: Arc<[Arc<str>]> },
    Color,
    Curve,
    Gradient,
}
/// The number field for a numeric filter parameter.
pub(super) fn number_control(p: &layer_core::EffectParameter) -> Option<NumericControl> {
    let EffectParameterKind::Number { min, max, step, decimals, unit } = &p.kind else {
        return None;
    };
    let mut numeric = NumericControl::number(*min as f64, *max as f64, *step as f64, *decimals as u32).unit(unit);
    numeric.mapping=p.mapping;
    if let Some([low,high])=p.soft_bounds {numeric.soft_min=low;numeric.soft_max=high;}
    // Percentages express an amount, not an item count, even when the
    // displayed range is short. Keep their compact slider presentation.
    if unit.as_ref() == "%" {
        numeric.kind = NumericKind::Slider;
    }
    if let EffectValue::Number(v) = p.default {
        numeric.default_value = Some(v as f64);
    }
    Some(numeric)
}
fn control(layer: u64, p: &layer_core::EffectParameter, value: EffectValue, l: &Localizer) -> PropertyControl {
    let kind = match &p.kind {
        EffectParameterKind::Number { .. } => PropertyKind::Number { numeric: number_control(p).unwrap() },
        EffectParameterKind::Toggle => PropertyKind::Toggle,
        EffectParameterKind::Choice { options } => PropertyKind::Choice {
            options: options.iter().map(|option| match option {
                layer_core::EffectOption::Literal(label) => label.clone(),
                layer_core::EffectOption::Labeled { label, .. } => resource_label(label, l),
            }).collect(),
        },
        EffectParameterKind::Color => PropertyKind::Color,
        EffectParameterKind::Curve => PropertyKind::Curve,
        EffectParameterKind::Gradient => PropertyKind::Gradient,
    };
    let plot = if let EffectValue::Curve(points) = &value {
        (0..=128)
            .map(|i| {
                let x = i as f32 / 128.;
                [x, layer_core::curve_value(points, x)]
            })
            .collect()
    } else {
        Vec::new()
    };
    let color_action = matches!(kind, PropertyKind::Color).then(|| UiAction::Effect {
        action: EffectAction::UseCurrentColor { layer, key: p.key.to_string() },
    });
    PropertyControl {
        plot,
        page:p.page.as_ref().map(|id|id.to_string()),
        section: p.section.as_ref().map(|s| resource_label(s, l).to_string()),
        section_id: p.section.clone(),
        color_action,
        ..PropertyControl::new(&p.key, &resource_label(&p.label, l), kind, value, p.default.clone())
    }
}
/// Extend only the bundled linear algorithms, whose math is independent of
/// sample depth. Embedded/custom programs retain their own declared contracts.
fn float32_program(program: &Arc<layer_core::EffectProgram>, depth: layer_core::color::SampleDepth) -> Arc<layer_core::EffectProgram> {
    if depth != layer_core::color::SampleDepth::F32 { return program.clone(); }
    let (key, lower, upper) = match program.id.as_ref() {
        "curves" => ("hdr_stops", 0., 127.),
        "exposure" => ("exposure", -126., 126.),
        _ => return program.clone(),
    };
    let Some(bundled) = layer_core::bundled_effect_catalog().get(&program.id) else { return program.clone(); };
    if program.wgsl != bundled.program().wgsl || program.entry != bundled.program().entry { return program.clone(); }
    let mut result = program.clone();
    let parameters = &mut Arc::make_mut(&mut result).parameters;
    for parameter in Arc::make_mut(parameters) {
        if parameter.key.as_ref() == key && let EffectParameterKind::Number { min, max, .. } = &mut parameter.kind { *min = lower; *max = upper; }
    }
    result
}

pub(super) fn properties(doc: &Document, painting: layer_core::SelectionPaintBehavior, l: &Localizer) -> LayerPropertiesView {
    let Some(layer) = doc.layer(doc.active_layer) else {
        return LayerPropertiesView::default();
    };
    if layer.kind == LayerKind::Selection {
        return super::selection_properties::properties(layer.id.0, &layer.name, &layer.properties.selection_mask.clone().unwrap_or_default(), painting, !doc.is_locked(layer.id), l);
    }
    let mut controls = Vec::new();
    let mut curve_max = None;
    let mut curve_white = None;
    let description = if let Some(effect) = &layer.effect {
        let program = float32_program(&effect.program, doc.color.depth);
        controls.extend(
            program
                .parameters
                .iter()
                .zip(&effect.values)
                .filter(|(p,_)|p.visible_when.as_ref().is_none_or(|condition|effect.value(&condition.key)==Some(&condition.value)))
                .map(|(p, v)| control(layer.id.0, p, v.clone(), l)),
        );
        if effect.program.id.as_ref() == "curves" {
            if let (Some(space), Some(EffectValue::Number(stops))) = (effect.choice("domain"), effect.value("hdr_stops")) {
                curve_white = layer_core::hdr_curve_white(space, *stops);
                curve_max = curve_white.map(|_| stops.exp2());
            }
            let hdr = curve_white.is_some();
            controls.retain(|c| match c.key.as_str() {
                "domain" => doc.color.depth.is_float() || hdr,
                "hdr_stops" => hdr,
                _ => true,
            });
        }
        resource_label(&effect.program.label, l).to_string()
    } else if layer.kind == LayerKind::Background {
        let paper = layer.properties.paper_color.unwrap_or(layer_core::color::RgbColor::WHITE);
        controls.push(PropertyControl {
            color_action: Some(UiAction::Effect { action: EffectAction::UseCurrentColor {
                layer: layer.id.0, key: "paper_color".into(),
            } }),
            ..PropertyControl::new("paper_color", &l.text(MessageId::RESOURCES_PAPER_COLOR), PropertyKind::Color,
                EffectValue::Color(paper), EffectValue::Color(layer_core::color::RgbColor::WHITE))
        });
        String::new()
    } else {
        let mut numeric = NumericControl::percent();
        numeric.default_value = Some(1.);
        controls.push(PropertyControl::new("opacity", &l.text(MessageId::RESOURCES_OPACITY), PropertyKind::Number { numeric },
            EffectValue::Number(layer.opacity), EffectValue::Number(1.)));
        let options = layer_core::LayerBlend::ALL
            .iter()
            .filter(|b| **b != layer_core::LayerBlend::PassThrough || layer.kind == LayerKind::Group)
            .map(|b| blend_label(*b, l))
            .collect();
        controls.push(PropertyControl::new("blend", &l.text(MessageId::RESOURCES_BLEND_MODE), PropertyKind::Choice { options },
            EffectValue::Choice(layer.properties.blend.code()), EffectValue::Choice(0)));
        String::new()
    };
    LayerPropertiesView {
        layer: Some(layer.id.0),
        title: layer.name.to_string(),
        description,
        enabled: !doc.is_locked(layer.id),
        controls,
        curve_max,
        curve_white,
        ..LayerPropertiesView::default()
    }
}
pub(super) fn publish_properties(view:&mut LayerPropertiesView,doc:&Document,state:&mut PropertyEditorState,gesture:Option<&EffectGesture>,l:&Localizer) {
    let effect=if doc.active_mask {None} else {view.layer.and_then(|id|doc.layer(layer_core::LayerId(id))).and_then(|layer|layer.effect.as_ref())};
    view.pages=effect.map_or_else(Vec::new,|effect|effect.program.pages.iter().map(|page|PropertyPageView{id:page.id.to_string(),label:resource_label(&page.label,l).to_string()}).collect());
    state.sync(doc.id.clone(),view.layer,doc.revision,view.pages.iter().map(|page|page.id.clone()).collect(),gesture.is_some());
    view.epoch=state.epoch;view.page=state.page().map(str::to_string);
    view.actions = effect.filter(|effect| effect.program.id.as_ref() == "white_balance").map(|_| PropertyActionView {
        label: l.text(MessageId::RESOURCES_PICKER_NEUTRAL).to_string(),
        action: EffectAction::WhiteBalancePicker { layer: view.layer.unwrap(), epoch: view.epoch },
    }).into_iter().collect();
    view.controls.retain(|control|control.page.as_deref().is_none_or(|page|Some(page)==state.page()));
    let domain=effect.filter(|effect|effect.choice("domain")==Some("Log HDR")).and_then(|effect|match effect.value("hdr_stops"){Some(EffectValue::Number(stops))=>Some(CurveDomain::LogHdr{stops:*stops}),_=>None}).unwrap_or(CurveDomain::Encoded);
    for control in &mut view.controls {
        let EffectValue::Curve(points)=&control.value else{continue;};
        let selected=if gesture.is_some_and(|gesture|gesture.key==control.key && gesture.detached_curve_point){None}else{state.selected(&control.key,points)};
        let coordinate=|axis:CurveAxis,index:usize| {
            let graph=points[index][usize::from(axis==CurveAxis::Output)];let value=domain.decode(f64::from(graph));
            CurveCoordinateControl{value,text:domain.text(graph),read_only:axis==CurveAxis::Input && (index==0 || index+1==points.len()),
                ev:matches!(domain,CurveDomain::LogHdr{..}).then(||if value==0.{l.text(MessageId::RESOURCES_PARAMETER_LEVELS_BLACK).to_string()}else{format!("{:.2} EV",value.log2())})}
        };
        let axis=|label|CurveAxisView{label:l.text(label).to_string(),minimum:domain.axis_text(0.),maximum:domain.axis_text(1.),white:matches!(domain,CurveDomain::LogHdr{..}).then(||domain.encode(1.) as f32)};
        control.curve=Some(CurveControls{epoch:state.epoch,numeric:domain.numeric(),selected,input:selected.map(|index|coordinate(CurveAxis::Input,index)),output:selected.map(|index|coordinate(CurveAxis::Output,index)),
            axes:[axis(MessageId::RESOURCES_SECTION_LEVELS_INPUT),axis(MessageId::RESOURCES_SECTION_LEVELS_OUTPUT)],domain,
            help:l.text(MessageId::RESOURCES_CURVES_HELP).to_string(),reset_label:l.text(MessageId::RESOURCES_CURVES_RESET).to_string()});
    }
}
pub(super) struct EffectGesture {
    original: Layer,
    key: String,
    detached_curve_point: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn cancel_effect_gesture(&mut self) -> Result<bool, String> {
        if let Some((_, old)) = self.selection_masks.quick_property_gesture.take() {
            self.selection_masks.quick_properties = old;
            self.refresh_document();
            return Ok(true);
        }
        let Some(gesture) = self.effect_gesture.take() else {
            return Ok(false);
        };
        self.engine
            .preview_edit(Edit::ReplaceLayer(Box::new(gesture.original)))
            .map_err(error)?;
        self.refresh_document();
        Ok(true)
    }

    fn effect_gesture_action(
        &mut self,
        phase: ContactPhase,
        action: EffectAction,
    ) -> Result<(), String> {
        if let EffectAction::CurveNumber{layer,key,epoch,..}=&action
            && !self.curve_action_target(*layer,key,*epoch) {return Ok(());}
        let (layer, key) = match &action {
            EffectAction::CurvePoint { layer, key, .. }
            | EffectAction::CurveNumber { layer, key, .. }
            | EffectAction::Number { layer, key, .. }
            | EffectAction::GradientStop { layer, key, .. }
            | EffectAction::Set {
                layer,
                key,
                value: EffectValue::Number(_) | EffectValue::Color(_),
            } => (*layer, key.clone()),
            _ => return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_PROPERTY_NOT_DRAGGABLE).to_string()),
        };
        if layer == 0 && self.selection_masks.quick() && key.starts_with("mask_") {
            if phase == ContactPhase::Down {
                self.require_idle()?;
                self.selection_masks.quick_property_gesture = Some((key.clone(), self.selection_masks.quick_properties.clone()));
            } else if !self.selection_masks.quick_property_gesture.as_ref().is_some_and(|(k, _)| *k == key) {
                return Ok(());
            }
            if phase == ContactPhase::Cancel || self.workspace_read_only || self.workspace_transition || self.rendering_suspended {
                self.cancel_effect_gesture()?;
                return Ok(());
            }
            if let Err(error) = self.effect_action(action) {
                self.cancel_effect_gesture()?;
                return Err(error);
            }
            if phase == ContactPhase::Up { self.selection_masks.quick_property_gesture = None; }
            return Ok(());
        }
        if phase == ContactPhase::Down {
            self.require_idle()?;
            let original = self
                .engine
                .document()
                .layer(LayerId(layer))
                .ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_LAYER).to_string())?
                .clone();
            if self.engine.document().is_locked(original.id) {
                return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_LAYER_LOCKED).to_string());
            }
            self.effect_gesture = Some(EffectGesture {
                original,
                key: key.clone(),
                detached_curve_point: false,
            });
        } else if !self
            .effect_gesture
            .as_ref()
            .is_some_and(|g| g.original.id.0 == layer && g.key == key)
        {
            // A cancelled native contact may still deliver its terminal event.
            return Ok(());
        }
        if phase == ContactPhase::Cancel
            || self.workspace_read_only
            || self.workspace_transition
            || self.rendering_suspended
        {
            self.cancel_effect_gesture()?;
            return Ok(());
        }
        if let Err(error) = self.effect_action(action) {
            self.cancel_effect_gesture()?;
            return Err(error);
        }
        if phase == ContactPhase::Up {
            let gesture = self.effect_gesture.take().unwrap();
            if gesture.detached_curve_point {self.property_editor.clear_selection(&gesture.key);}
            let edited = self
                .engine
                .document()
                .layer(gesture.original.id)
                .unwrap()
                .clone();
            let changed = edited.effect != gesture.original.effect
                || edited.opacity != gesture.original.opacity
                || edited.properties != gesture.original.properties;
            self.engine
                .preview_edit(Edit::ReplaceLayer(Box::new(gesture.original)))
                .map_err(error)?;
            if changed {
                self.layer_edit(Edit::ReplaceLayer(Box::new(edited)))?;
            }
            self.property_editor.commit_revision(self.engine.document().revision);
        }
        Ok(())
    }

    fn effect_parameter(&self, layer: u64, key: &str) -> Result<EffectValue, String> {
        let layer = self.engine.document().layer(LayerId(layer)).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_LAYER).to_string())?;
        let effect = layer.effect.as_ref().ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_ADJUSTMENT_REQUIRED).to_string())?;
        effect.value(key).cloned().ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_PROPERTY).to_string())
    }

    fn curve_action_target(&self,layer:u64,key:&str,epoch:u64)->bool {
        self.property_editor.accepts(layer,epoch) && self.state.layer_properties.layer==Some(layer)
            && self.state.layer_properties.controls.iter().any(|control|control.key==key && matches!(control.kind,PropertyKind::Curve))
    }
    pub(super) fn effect_action(&mut self, action: EffectAction) -> Result<(), String> {
        if let Some(result) = self.mask_property_action(&action) { return result; }

        if self.selection_masks.target().is_some() && !matches!(action, EffectAction::Gesture { .. }) {return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_ARTWORK_REQUIRED).to_string());}
        match action {
            EffectAction::WhiteBalancePicker { layer, epoch } => return self.start_white_balance_picker(layer, epoch),
            EffectAction::SelectPage{layer,page}=> {
                if self.state.layer_properties.layer!=Some(layer) || self.state.layer_properties.page.as_deref()==Some(&page)
                    || !self.state.layer_properties.pages.iter().any(|candidate|candidate.id==page){return Ok(());}
                self.cancel_effect_gesture()?;
                self.property_editor.select_page(layer,&page);
                self.refresh_document();return Ok(());
            }
            EffectAction::CurveSelectPoint{layer,key,epoch,index}=> {
                if !self.curve_action_target(layer,&key,epoch){return Ok(());}
                let EffectValue::Curve(points)=self.effect_parameter(layer,&key)? else{return Ok(());};
                self.property_editor.select(layer,epoch,&key,index,&points);self.refresh_document();return Ok(());
            }
            EffectAction::CurveNumber{layer,key,epoch,axis,operation}=> {
                if !self.curve_action_target(layer,&key,epoch){return Ok(());}
                let EffectValue::Curve(points)=self.effect_parameter(layer,&key)? else{return Ok(());};
                let Some(index)=self.property_editor.selected(&key,&points) else{return Ok(());};
                let Some(control)=self.state.layer_properties.controls.iter().find(|control|control.key==key).and_then(|control|control.curve.as_ref()) else{return Ok(());};
                if matches!(operation,NumericOperation::Format){return Ok(());}
                let domain=control.domain;let old=domain.decode(f64::from(points[index][usize::from(axis==CurveAxis::Output)]));
                if matches!(operation,NumericOperation::Value{value} if value==old) {return Ok(());}
                let resolved=domain.numeric().resolve(old,operation).map_err(|_|self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_CURVE_COORDINATE).to_string())?;
                let Some(point)=curves::numeric_point(&points,index,axis,domain,resolved.value) else{return Ok(());};
                return self.effect_action(EffectAction::CurvePoint{layer,key,index:Some(index),point,remove:false});
            }
            EffectAction::CurveRemoveAt{layer,key,epoch,point,extent,point_count}=> {
                if !self.curve_action_target(layer,&key,epoch){return Ok(());}
                let EffectValue::Curve(points)=self.effect_parameter(layer,&key)? else{return Ok(());};
                if point_count.is_some_and(|count|count!=points.len()){return Ok(());}
                let Some(index)=curves::hit(&points,point,extent) else{return Ok(());};
                if index==0 || index+1==points.len(){return Ok(());}
                self.cancel_effect_gesture()?;
                self.effect_action(EffectAction::CurvePoint{layer,key,index:Some(index),point:points[index],remove:true})?;
                self.refresh_document();return Ok(());
            }
            EffectAction::CurveContact{layer,key,epoch,phase,point,extent}=> {
                if !self.curve_action_target(layer,&key,epoch){return Ok(());}
                if phase==ContactPhase::Cancel {self.property_editor.end_contact();self.cancel_effect_gesture()?;return Ok(());}
                if point.iter().any(|v|!v.is_finite()) || extent.iter().any(|v|!v.is_finite() || *v<=0.) {return Ok(());}
                let EffectValue::Curve(points)=self.effect_parameter(layer,&key)? else{return Ok(());};
                if phase==ContactPhase::Down {
                    self.cancel_effect_gesture()?;
                    let index=curves::hit(&points,point,extent).or_else(||points.iter().enumerate().filter(|(_,p)|(p[0]-point[0]/extent[0]).abs()<=0.002).min_by(|a,b|(a.1[0]-point[0]/extent[0]).abs().total_cmp(&(b.1[0]-point[0]/extent[0]).abs())).map(|(index,_)|index));
                    if let Some(index)=index {
                        self.property_editor.begin_contact(&key,index,point,extent,points[index]);
                        return self.effect_gesture_action(phase,EffectAction::CurvePoint{layer,key,index:Some(index),point:points[index],remove:false});
                    }
                    let graph=[(point[0]/extent[0]).clamp(0.,1.),(1.-point[1]/extent[1]).clamp(0.,1.)];
                    if points.len()>=32 || graph[0]<=0. || graph[0]>=1. {return Ok(());}
                    self.effect_gesture_action(phase,EffectAction::CurvePoint{layer,key:key.clone(),index:None,point:graph,remove:false})?;
                    let EffectValue::Curve(inserted)=self.effect_parameter(layer,&key)? else{return Ok(());};
                    let Some(index)=inserted.iter().position(|p|p[0]==graph[0]) else{return Ok(());};
                    self.property_editor.begin_contact(&key,index,point,extent,inserted[index]);return Ok(());
                }
                let Some(index)=self.property_editor.contact_index(&key) else{return Ok(());};
                let Some(graph)=self.property_editor.contact_point(&key,point) else{return Ok(());};
                self.effect_gesture_action(phase,EffectAction::CurvePoint{layer,key,index:Some(index),point:graph,remove:false})?;
                if phase==ContactPhase::Up {self.property_editor.end_contact();}return Ok(());
            }
            EffectAction::CurveKey{layer,key,epoch,key_event,pressed,repeat:_,modifiers}=> {
                if !self.curve_action_target(layer,&key,epoch){return Ok(());}
                if key_event=="Escape" && pressed {self.property_editor.end_contact();self.cancel_effect_gesture()?;return Ok(());}
                let EffectValue::Curve(points)=self.effect_parameter(layer,&key)? else{return Ok(());};
                let Some(index)=self.property_editor.selected(&key,&points) else{return Ok(());};
                let action=|point,remove|EffectAction::CurvePoint{layer,key:key.clone(),index:Some(index),point,remove};
                if !pressed {
                    if self.property_editor.release_key(&key_event){self.effect_gesture_action(ContactPhase::Up,action(points[index],false))?;}return Ok(());
                }
                if modifiers.command || modifiers.alt{return Ok(());}
                if matches!(key_event.as_str(),"Delete"|"Backspace") {
                    self.cancel_effect_gesture()?;
                    return self.effect_action(action(points[index],true));
                }
                let (axis,direction)=match key_event.as_str(){"ArrowLeft"=>(0,-1.),"ArrowRight"=>(0,1.),"ArrowUp"=>(1,1.),"ArrowDown"=>(1,-1.),_=>return Ok(())};
                if axis==0 && (index==0 || index+1==points.len()){return Ok(());}
                if self.property_editor.key().is_some_and(|old|old!=key_event) {
                    self.effect_gesture_action(ContactPhase::Up,action(points[index],false))?;
                }
                let phase=if self.property_editor.key()==Some(key_event.as_str()){ContactPhase::Move}else{ContactPhase::Down};
                let mut point=points[index];point[axis]+=(if modifiers.shift{10.}else{1.})*direction/255.;
                if axis==0 {let Some(x)=curves::point_between(point[0],points[index-1][0],points[index+1][0]) else{return Ok(());};point[0]=x;}else{point[1]=point[1].clamp(0.,1.);}
                self.property_editor.press_key(&key_event);return self.effect_gesture_action(phase,action(point,false));
            }
            EffectAction::CancelFilter => {
                let doc = self.engine.document();
                if let Some(layer) = doc.layer(doc.active_layer).filter(|l| l.effect.is_some()) {
                    self.layer_action(LayerAction::Delete { id: layer.id.0 })?;
                }
                self.state.customization.drawer = None;
                self.state.customization.expanded = None;
            }
            EffectAction::UseCurrentColor { layer, ref key }
            | EffectAction::Number { layer, ref key, .. }
            | EffectAction::Reset { layer, ref key } => {
                let view = properties(self.engine.document(), self.state.settings.selection_painting, &self.state.localization);
                if view.layer != Some(layer) {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_SELECT_PROPERTY_LAYER).to_string());
                }
                let value = property_value(&view, key, &action, self.state.colors.definition(), &self.state.localization)?;
                return self.effect_action(EffectAction::Set { layer, key: key.clone(), value });
            }
            EffectAction::Gesture { phase, action } => return self.effect_gesture_action(phase, *action),
            EffectAction::GradientStop {
                layer,
                key,
                index,
                position,
                color,
                remove,
            } => {
                if !position.is_finite() {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_GRADIENT_POSITION).to_string());
                }
                let EffectValue::Gradient(mut stops) = self.effect_parameter(layer, &key)? else {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_GRADIENT_REQUIRED).to_string());
                };
                if let Some(i) = index {
                    if i >= stops.len() {
                        return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_GRADIENT_STOP).to_string());
                    }
                    if remove {
                        if i > 0 && i + 1 < stops.len() {
                            stops.remove(i);
                        }
                    } else {
                        stops[i].position = if i == 0 {
                            0.
                        } else if i + 1 == stops.len() {
                            1.
                        } else {
                            point_between(position, stops[i - 1].position, stops[i + 1].position)
                        };
                        if let Some(color) = color {
                            stops[i].color = color;
                        }
                    }
                } else if stops.len() < 32 && !remove {
                    let position = position.clamp(0., 1.);
                    if stops.iter().all(|s| (s.position - position).abs() > 0.002) {
                        let color = match color {
                            Some(color) => color,
                            None => layer_core::gradient_value(&stops, position, self.engine.document().color.space)?,
                        };
                        stops.push(layer_core::GradientStop { position, color });
                        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
                    }
                }
                return self.effect_action(EffectAction::Set {
                    layer,
                    key,
                    value: EffectValue::Gradient(stops),
                });
            }
            EffectAction::CurvePoint {
                layer,
                key,
                index,
                point,
                remove,
            } => {
                if !point.iter().all(|x| x.is_finite()) {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_CURVE_COORDINATE).to_string());
                }
                let EffectValue::Curve(mut points) = self.effect_parameter(layer, &key)? else {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_CURVE_REQUIRED).to_string());
                };
                let dragging = self.effect_gesture.as_ref().filter(|g| g.original.id.0 == layer && g.key == key);
                let detached = dragging.is_some_and(|g| g.detached_curve_point);
                if !detached && !remove && index.is_some_and(|index| points.get(index)==Some(&point)) {return Ok(());}
                let off_graph = dragging.is_some()
                    && point.iter().any(|v| !(-CURVE_DETACH_MARGIN..=1. + CURVE_DETACH_MARGIN).contains(v));
                if let Some(i) = index {
                    if i >= points.len() || (detached && i == 0) {
                        return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_CURVE_POINT).to_string());
                    }
                    if detached {
                        if off_graph {
                            return Ok(());
                        }
                        points.insert(i, [curves::point_between(point[0], points[i - 1][0], points[i][0]).ok_or_else(||self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_CURVE_COORDINATE).to_string())?, point[1].clamp(0., 1.)]);
                        self.effect_gesture.as_mut().unwrap().detached_curve_point = false;
                    } else if remove {
                        if i > 0 && i + 1 < points.len() {
                            points.remove(i);
                            self.property_editor.clear_selection(&key);
                        }
                    } else if off_graph && i > 0 && i + 1 < points.len() {
                        points.remove(i);
                        self.effect_gesture.as_mut().unwrap().detached_curve_point = true;
                    } else {
                        let x = if point[0] == points[i][0] {
                            points[i][0]
                        } else if i == 0 {
                            0.
                        } else if i + 1 == points.len() {
                            1.
                        } else {
                            curves::point_between(point[0], points[i - 1][0], points[i + 1][0]).unwrap_or(points[i][0])
                        };
                        points[i] = [x, point[1].clamp(0., 1.)];
                    }
                } else if points.len() < 32 && !remove {
                    let x = point[0].clamp(0., 1.);
                    if points.iter().all(|p| (p[0] - x).abs() > 0.002) {
                        points.push([x, point[1].clamp(0., 1.)]);
                        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
                    }
                }
                return self.effect_action(EffectAction::Set {
                    layer,
                    key,
                    value: EffectValue::Curve(points),
                });
            }
            EffectAction::Insert { effect } => {
                let choosing = self.filter_drawer_open();
                let effect = self.effect_catalog.get(&effect).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_FILTER).to_string())?;
                let generator = effect.program.kind == layer_core::EffectKind::Generator;
                let doc = self.engine.document();
                let current = doc.layer(doc.active_layer).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_SELECT_LAYER).to_string())?;
                let replacing = choosing && current.effect.as_ref().is_some_and(|fx| fx.program.kind == effect.program.kind);
                let masked = !replacing && doc.selection.is_some();
                if replacing && doc.is_locked(current.id) { return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_LAYER_LOCKED).to_string()); }
                if replacing && current.effect.as_ref().is_some_and(|fx| fx.program.id == effect.program.id) {
                    return Ok(());
                }
                let top = if choosing { current.id } else { doc.clipping_stack_top(current.id).unwrap() };
                let index = doc.layers.iter().position(|l| l.id == top).unwrap();
                let parent = current.properties.parent;
                let depth = doc.color.depth;
                let hdr = depth.is_float();
                let mut layer = if replacing { current.clone() } else {
                    let clipped = choosing && current.properties.clipped;
                    let mut layer = Layer::paint(self.engine.allocate_layer_id(), resource_label(effect.label(), &self.state.localization));
                    layer.properties.clipped = clipped;
                    layer
                };
                let id = layer.id;
                layer.kind = LayerKind::Effect;
                layer.properties.parent = parent;
                let mut instance = EffectInstance::new(float32_program(&effect.program(), depth));
                if hdr && instance.program.id.as_ref() == "curves" { instance.set("domain", EffectValue::Choice(1)).map_err(str::to_string)?; }
                if generator && let Some(color) = instance.program.parameters.iter().find(|p| p.kind == EffectParameterKind::Color) {
                    instance.set(&color.key.clone(), EffectValue::Color(self.state.colors.definition())).map_err(str::to_string)?;
                }
                layer.effect = Some(Arc::new(instance));
                if masked || (generator && layer.mask.is_none()) {
                    layer.mask = Some(self.selection_mask(&layer, false)?);
                }
                self.layer_edit(if replacing {
                    Edit::ReplaceLayer(Box::new(layer))
                } else {
                    let mut edits = vec![Edit::InsertLayer { index, layer }, Edit::SetActiveLayer { id }];
                    if masked {
                        edits.push(Edit::SetSelection(None));
                    }
                    Edit::Batch(edits)
                })?;
                if !choosing {
                    self.state.customization.expanded = None;
                    self.state.workspace.layout.reveal_after(Panel::Properties, Panel::Adjustments)?;
                }
            }
            EffectAction::Set {
                layer: id,
                key,
                value,
            } => {
                if self
                    .engine
                    .document()
                    .layer(LayerId(id))
                    .is_some_and(|l| l.effect.is_none())
                {
                    return match (key.as_str(), value) {
                        ("paper_color", EffectValue::Color(color)) => {
                            color.validate_working_spaces()?;
                            let doc = self.engine.document();
                            let mut layer = doc.layer(LayerId(id)).unwrap().clone();
                            if layer.kind != LayerKind::Background || doc.is_locked(layer.id) {
                                return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_PAPER_LOCKED).to_string());
                            }
                            layer.properties.paper_color = Some(color);
                            let edit = Edit::ReplaceLayer(Box::new(layer));
                            if self.effect_gesture.is_some() {
                                self.engine.preview_edit(edit).map_err(error)?;
                            } else {
                                self.layer_edit(edit)?;
                            }
                            Ok(())
                        }
                        ("opacity", EffectValue::Number(opacity)) => {
                            self.set_layer_opacity(Some(id), opacity)
                        }
                        ("blend", EffectValue::Choice(value)) => {
                            self.layer_action(LayerAction::Blend { id, value })
                        }
                        _ => Err(self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_LAYER_PROPERTY).to_string()),
                    };
                }
                let mut layer = self.editable_layer(id)?;
                let effect = layer.effect.as_mut().ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_EFFECT_LAYER_REQUIRED).to_string())?;
                let original = effect.clone();
                let changed = Arc::make_mut(effect);
                changed.program = float32_program(&changed.program, self.engine.document().color.depth);
                changed.set(&key, value)
                    .map_err(str::to_string)?;
                if *effect == original {
                    return Ok(());
                }
                let edit = Edit::ReplaceLayer(Box::new(layer));
                if self.effect_gesture.is_some() {
                    self.engine.preview_edit(edit).map_err(error)?;
                } else {
                    self.layer_edit(edit)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use layer_core::{EffectCatalog, EffectInstallMode, EffectPackage};

    fn package() -> EffectPackage {
        let bundled = layer_core::bundled_effect_catalog();
        EffectPackage { format: 2, categories: bundled.categories().to_vec(), filters: bundled.filters().to_vec() }
    }
    fn resolve(package: EffectPackage) -> EffectCatalog {
        package.resolve(|_| Err("resolved fixture has no module imports".into())).unwrap()
    }
    fn message(id: MessageId) -> ResourceLabel {
        ResourceLabel::Message { message: id.key().into() }
    }

    #[test]
    fn resource_builtin_references_resolve_without_changing_programs() {
        let bundled = layer_core::bundled_effect_catalog();
        for language in [UiLanguage::English, UiLanguage::Japanese, UiLanguage::SimplifiedChinese,
            UiLanguage::TraditionalChinese, UiLanguage::Korean]
        {
            let l = Localizer::shared(language);
            validate_catalog_labels(bundled, &l).unwrap();
            let before = bundled.get("curves").unwrap().preview().unwrap();
            let _ = catalog(bundled, &FilterPickerState::new(&l), &l);
            let program = before.program.clone();
            for parameter in program.parameters.iter() {
                let _ = control(7, parameter, parameter.default.clone(), &l);
            }
            assert_eq!(before.program, program);
            assert_eq!(before.gpu_parameters(layer_core::color::RgbSpace::Srgb).unwrap(),
                bundled.get("curves").unwrap().preview().unwrap().gpu_parameters(layer_core::color::RgbSpace::Srgb).unwrap());
        }
    }

    #[test]
    fn resource_builtin_id_literal_replacement_survives_localization_and_serialization() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut replacement = package();
        replacement.filters.retain(|f| f.id() == "curves");
        let program = Arc::make_mut(&mut replacement.filters[0].program);
        program.label = "私の \"曲線\" {名前} 🎨".into();
        let parameters = Arc::make_mut(&mut program.parameters);
        parameters[0].label = "한글 설정".into();
        parameters[0].section = Some("我的分组".into());
        let candidate = layer_core::bundled_effect_catalog()
            .stage(resolve(replacement), EffectInstallMode::Replace).unwrap();
        validate_catalog_labels(&candidate, &l).unwrap();
        let picker = FilterPickerState { search: Some("曲".into()), ..FilterPickerState::new(&l) };
        let choices = catalog(&candidate, &picker, &l);
        assert_eq!(choices.len(), 1);
        assert_eq!(&*choices[0].label, "私の \"曲線\" {名前} 🎨");
        let instance = EffectInstance::new(candidate.get("curves").unwrap().program());
        let copy: EffectInstance = serde_json::from_str(&serde_json::to_string(&instance).unwrap()).unwrap();
        assert_eq!(copy, instance);
        let control = control(7, &copy.program.parameters[0], copy.values[0].clone(), &l);
        assert_eq!(control.label, "한글 설정");
        assert_eq!(control.section.as_deref(), Some("我的分组"));
    }

    #[test]
    fn resource_groups_and_choice_values_keep_identity_when_display_labels_match() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut custom = package();
        custom.categories[0].label = message(MessageId::COMMON_CANCEL);
        custom.categories[1].label = "キャンセル".into();
        let program = Arc::make_mut(&mut custom.filters[0].program);
        let parameters = Arc::make_mut(&mut program.parameters);
        parameters[0].section = Some(message(MessageId::COMMON_CANCEL));
        parameters[1].section = Some("キャンセル".into());
        let EffectParameterKind::Choice { options } = &mut parameters[4].kind else { panic!() };
        let layer_core::EffectOption::Labeled { label, .. } = &mut Arc::make_mut(options)[1] else { panic!() };
        *label = message(MessageId::COMMON_CANCEL);
        let custom = resolve(custom);
        let categories = categories(&custom, &l);
        assert_eq!(categories[1].label, categories[2].label);
        assert_ne!(categories[1].id, categories[2].id);
        let choices = catalog(&custom, &FilterPickerState::new(&l), &l);
        assert_eq!(choices.len(), custom.filters().len());
        assert_eq!(choices.iter().map(|c| &c.id).collect::<std::collections::HashSet<_>>().len(), choices.len());
        let tone = catalog(&custom, &FilterPickerState { category: Some("tone".into()), ..FilterPickerState::new(&l) }, &l);
        assert!(tone.iter().all(|choice| choice.category.as_ref() == "tone"));
        assert_eq!(tone.len(), custom.filters().iter().filter(|f| f.category.as_ref() == "tone").count());
        let instance = custom.get("curves").unwrap().preview().unwrap();
        let a = control(7, &instance.program.parameters[0], instance.values[0].clone(), &l);
        let b = control(7, &instance.program.parameters[1], instance.values[1].clone(), &l);
        assert_eq!(a.section, b.section);
        assert_ne!(a.section_id, b.section_id);
        let choice = control(7, &instance.program.parameters[4], EffectValue::Choice(1), &l);
        let PropertyKind::Choice { options } = choice.kind else { panic!() };
        assert_eq!(&*options[1], "キャンセル");
        let mut instance = instance;
        instance.set("domain", EffectValue::Choice(1)).unwrap();
        assert_eq!(instance.choice("domain"), Some("Log HDR"));
    }

    #[test]
    fn resource_unknown_or_dynamic_references_fail_admission() {
        let l = Localizer::shared(UiLanguage::English);
        for key in ["resources-does-not-exist", MessageId::RESOURCES_ANIMATED_TOOLTIP.key()] {
            for field in 0..5 {
                let invalid = ResourceLabel::Message { message: key.into() };
                let mut custom = package();
                if field == 0 { custom.categories[0].label = invalid; }
                else {
                    let program = Arc::make_mut(&mut custom.filters[0].program);
                    match field {
                        1 => program.label = invalid,
                        2 => Arc::make_mut(&mut program.parameters)[0].label = invalid,
                        3 => Arc::make_mut(&mut program.parameters)[0].section = Some(invalid),
                        _ => {
                            let EffectParameterKind::Choice { options } = &mut Arc::make_mut(&mut program.parameters)[4].kind else { panic!() };
                            let layer_core::EffectOption::Labeled { label, .. } = &mut Arc::make_mut(options)[0] else { panic!() };
                            *label = invalid;
                        }
                    }
                }
                let custom = resolve(custom);
                assert!(validate_catalog_labels(&custom, &l).is_err(), "field {field}: {key}");
                if field != 0 {
                    let mut document = Document::new("resource", 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
                    document.layers[0].effect = Some(Arc::new(EffectInstance::new(custom.filters()[0].program())));
                    assert!(validate_document_labels(&document, &l).is_err(), "embedded field {field}: {key}");
                }
            }
        }
    }
}
