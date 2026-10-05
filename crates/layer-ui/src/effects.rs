//! Effect catalog, properties and navigation policy shared by every native view.
use super::*;
#[path = "effects/curves.rs"]
mod curves;
#[path = "effects/gradient.rs"]
mod gradient;
pub use gradient::{GradientDestination,GradientEdit,GradientControls};
pub use curves::{CurveAxis,CurveAxisView,CurveControls,CurveCoordinateControl,CurveDomain};
pub(super) use curves::PropertyEditorState;
use layer_core::{Edit, EffectInstance, EffectParameterKind, EffectValue, ResourceLabel, authored::{Definition, EffectApplication, EffectBaseline, EffectHandle, Occurrence, OccurrenceContent, OccurrenceHandle, RecordChange, SceneScope, SelectionHandle, SourceTarget}};
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
        let document = self.engine.document();
        let current = document.working.occurrence.and_then(|handle| document.scene().effect(handle))
            .and_then(|effect| self.effect_catalog.get(&effect.program.id));
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
            doc.working.occurrence.map_or(0, occurrence_token),
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
        let scene = doc.scene();
        let Some(current) = doc.working.occurrence else { return Ok(false); };
        let replacing = self.filter_drawer_open() && scene.effect(current).is_some_and(|e|e.program.kind==layer_core::EffectKind::Adjustment);
        let source = if replacing {
            layer_render::FilterPreviewSource::EffectInput(current)
        } else if self.filter_drawer_open() && scene.eligible_target(current)
            && scene.occurrence(current).is_some_and(|o|o.attachment==layer_core::Attachment::Clip) {
            layer_render::FilterPreviewSource::OwnerContent(current)
        } else {
            layer_render::FilterPreviewSource::LayerStack(doc.clipping_stack_top(current)
                .ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_SELECT_LAYER).to_string())?)
        };
        let scope = SceneScope::All;
        let request = layer_render::FilterPreviewRequest {
            request_id,
            source,
            size,
            view: self.engine.view(),
            snapshot: doc.snapshot(),
            scope,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EffectAction {
    ImportLookup {layer:u64,epoch:u64},
    LookupPreset {layer:u64,epoch:u64,preset:Option<layer_core::lut3d::Look>},
    AutoLevels {layer:u64,epoch:u64},
    TargetCurve {layer:u64,epoch:u64},
    Calibrate { layer: u64, epoch: u64, role:layer_core::levels::CalibrationRole },
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
    InsertAttached {
        effect: Arc<str>,
        owner: u64,
        epoch: u64,
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
    Gradient {target:GradientDestination,edit:GradientEdit},
}
impl EffectAction {
    pub(super) fn property_owner(&self)->Option<(u64,u64)> {
        match self {
            Self::ImportLookup {layer,epoch}|Self::LookupPreset {layer,epoch,..}|Self::AutoLevels {layer,epoch}|Self::TargetCurve {layer,epoch}|Self::Calibrate {layer,epoch,..}
            |Self::CurveSelectPoint {layer,epoch,..}|Self::CurveRemoveAt {layer,epoch,..}
            |Self::CurveContact {layer,epoch,..}|Self::CurveKey {layer,epoch,..}|Self::CurveNumber {layer,epoch,..}=>Some((*layer,*epoch)),
            Self::Gradient {target:GradientDestination::Effect {layer,epoch,..},..}=>Some((*layer,*epoch)),
            Self::Gesture {action,..}=>action.property_owner(),
            _=>None,
        }
    }
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
            animated: id.program.time,
            tooltip: if id.program.time {
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
    pub add_filter: Option<ContextMenu>,
    pub histogram:bool,
    pub actions: Vec<PropertyActionView>,
    pub pages:Vec<PropertyPageView>,
    pub page:Option<String>,
    pub epoch:u64,
    pub layer: Option<u64>,
    pub title: String,
    pub name: String,
    pub layer_type: String,
    pub description: String,
    pub resource_name: Option<String>,
    pub resource_label: Option<String>,
    pub resource_selection: Option<usize>,
    pub enabled: bool,
    pub controls: Vec<PropertyControl>,
    /// Linear input/output range for HDR curve axes; absent for encoded curves.
    pub curve_max: Option<f32>,
    pub curve_white: Option<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyActionView { pub label: String, pub icon: Option<String>, pub group: Option<PropertyActionGroup>, pub action: EffectAction }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyActionGroup { pub id: &'static str, pub label: String }
#[derive(Clone,Debug,PartialEq,Serialize)]
pub struct PropertyPageView { pub id:String, pub label:String }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyControl {
    pub curve:Option<CurveControls>,
    pub gradient:Option<GradientControls>,
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
            curve:None,gradient:None,
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
    Color { opaque: bool },
    Curve,
    Gradient,
}
/// The number field for a numeric filter parameter.
pub(super) fn number_control(p: &layer_core::EffectParameter) -> Option<NumericControl> {
    let EffectParameterKind::Number { min, max, step, decimals, unit } = &p.kind else {
        return None;
    };
    let (step,decimals)=if p.dimension==layer_core::authored::Dimension::Count {(1.,0)} else {(*step,*decimals)};
    let mut numeric = NumericControl::number(*min as f64, *max as f64, step as f64, decimals as u32).unit(unit);
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
fn control(layer: u64, p: &layer_core::EffectParameter, value: EffectValue, l: &Localizer) -> Option<PropertyControl> {
    let kind = match &p.kind {
        EffectParameterKind::Number { .. } => PropertyKind::Number { numeric: number_control(p).unwrap() },
        EffectParameterKind::Toggle => PropertyKind::Toggle,
        EffectParameterKind::Choice { options } => PropertyKind::Choice {
            options: options.iter().map(|option| match option {
                layer_core::EffectOption::Literal(label) => label.clone(),
                layer_core::EffectOption::Labeled { label, .. } => resource_label(label, l),
            }).collect(),
        },
        EffectParameterKind::Color => PropertyKind::Color {opaque:p.opaque},
        EffectParameterKind::Curve => PropertyKind::Curve,
        EffectParameterKind::Gradient => PropertyKind::Gradient,
        EffectParameterKind::Lut3d => return None,
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
    let color_action = matches!(kind, PropertyKind::Color {..}).then(|| UiAction::Effect {
        action: EffectAction::UseCurrentColor { layer, key: p.key.to_string() },
    });
    Some(PropertyControl {
        plot,
        page:p.page.as_ref().map(|id|id.to_string()),
        section: p.section.as_ref().map(|s| resource_label(s, l).to_string()),
        section_id: p.section.clone(),
        color_action,
        ..PropertyControl::new(&p.key, &resource_label(&p.label, l), kind, value, p.default.clone())
    })
}
pub(super) fn effect_application(document: &Document, occurrence: OccurrenceHandle) -> Option<(EffectHandle, &EffectApplication)> {
    let scene = document.scene();
    Some((scene.effect_handle(occurrence)?, scene.effect_application(occurrence)?))
}
pub(super) fn effect_draft(document: &Document, occurrence: OccurrenceHandle) -> Result<EffectInstance, String> {
    let (_, application) = effect_application(document, occurrence).ok_or("Missing adjustment")?;
    let definition = document.artwork.definitions.get(application.definition).ok_or("Missing adjustment definition")?;
    Ok(EffectInstance {program: definition.program.clone(), values: application.values.clone()})
}
pub(super) fn effect_edit(document: &Document, occurrence: OccurrenceHandle, draft: EffectInstance) -> Result<Edit, String> {
    draft.validate().map_err(str::to_string)?;
    let (handle, application) = effect_application(document, occurrence).ok_or("Missing adjustment")?;
    let definition = document.artwork.definitions.get(application.definition).ok_or("Missing adjustment definition")?;
    let mut edits = Vec::new();
    if definition.program != draft.program {
        let mut definition = definition.clone();
        definition.program = draft.program;
        edits.push(Edit::Definition(RecordChange::replace(&document.artwork.definitions, application.definition, Some(definition)).map_err(str::to_string)?));
    }
    if application.values != draft.values {
        let application = EffectApplication {definition: application.definition, values: draft.values};
        edits.push(Edit::Effect(RecordChange::replace(&document.artwork.effects, handle, Some(application)).map_err(str::to_string)?));
    }
    Ok(Edit::Batch(edits))
}
fn property_effect(doc: &Document, handle: OccurrenceHandle) -> Option<layer_core::EffectView<'_>> {
    let scene = doc.scene();
    let occurrence = scene.occurrence(handle)?;
    if doc.working.inspect_mask == Some(handle) || occurrence.mask.as_ref().is_some_and(|mask| doc.working.target == Some(SourceTarget::Coverage(mask.source))) {
        return None;
    }
    scene.effect(handle)
}
const LAYER_COLOR_MODES: [layer_core::color::LayerColorMode; 3] = [
    layer_core::color::LayerColorMode::FullColor,
    layer_core::color::LayerColorMode::Grayscale,
    layer_core::color::LayerColorMode::TwoTone,
];
pub(super) fn properties(doc: &Document, painting: layer_core::SelectionPaintBehavior, l: &Localizer) -> LayerPropertiesView {
    let scene = doc.scene();
    let Some((handle, layer)) = doc.working.occurrence.and_then(|handle| scene.occurrence(handle).map(|layer| (handle, layer))) else {
        return LayerPropertiesView::default();
    };
    let layer_type = match layer.kind() {
        LayerKind::Paint => l.text(MessageId::RESOURCES_LAYER_TYPE_PAINT),
        LayerKind::Group => l.text(MessageId::RESOURCES_LAYER_TYPE_GROUP),
        LayerKind::Selection => l.text(MessageId::RESOURCES_LAYER_TYPE_SELECTION),
        LayerKind::Effect => scene.effect(handle).map_or_else(|| Arc::from(""), |effect| resource_label(&effect.program.label, l)),
    };
    let title = if layer.kind() != LayerKind::Paint && !layer_type.is_empty() && layer_type.as_ref() != layer.name.as_ref() {
        let mut args = FluentArgs::new();
        args.set("name", layer.name.as_ref());
        args.set("type", layer_type.as_ref());
        l.format(MessageId::RESOURCES_PROPERTIES_LAYER_TITLE, &args)
    } else { layer.name.to_string() };
    let header = LayerPropertiesView { title, name: layer.name.to_string(), layer_type: layer_type.to_string(), ..Default::default() };
    let token = occurrence_token(handle);
    if scene.effect(handle).is_some() && property_effect(doc, handle).is_none() {
        return LayerPropertiesView {layer: Some(token), enabled: !doc.is_locked(handle), ..header};
    }
    if let OccurrenceContent::Selection(selection) = layer.content {
        let properties = doc.working.selection_overlays.properties.get(&selection).cloned().unwrap_or_default();
        let mut view = super::selection_properties::properties(token, &header.name, &properties, painting, !doc.is_locked(handle), l);
        view.title = header.title;
        return view;
    }
    let mut controls = Vec::new();
    let mut curve_max = None;
    let mut curve_white = None;
    let description = if let Some(effect) = property_effect(doc, handle) {
        let application = effect_application(doc, handle).unwrap().1;
        let program = &doc.artwork.definitions.get(application.definition).unwrap().program;
        controls.extend(
            program
                .parameters
                .iter()
                .zip(effect.values)
                .filter(|(p,_)|p.visible_when.as_ref().is_none_or(|condition|effect.value(&condition.key)==Some(&condition.value)))
                .filter_map(|(p, v)| control(token, p, v.clone(), l)),
        );
        if effect.program.id.as_ref() == "curves" {
            if let (Some(space), Some(EffectValue::Number(stops))) = (effect.choice("domain"), effect.value("hdr_stops")) {
                curve_white = layer_core::hdr_curve_white(space, *stops);
                curve_max = curve_white.map(|_| stops.exp2());
            }
            let hdr = curve_white.is_some();
            controls.retain(|c| match c.key.as_str() {
                "domain" => doc.composition().color.depth.is_float() || hdr,
                "hdr_stops" => hdr,
                _ => true,
            });
        }
        resource_label(&effect.program.label, l).to_string()
    } else {
        let mut numeric = NumericControl::percent();
        numeric.default_value = Some(1.);
        controls.push(PropertyControl::new("opacity", &l.text(MessageId::RESOURCES_OPACITY), PropertyKind::Number { numeric },
            EffectValue::Number(layer.opacity), EffectValue::Number(1.)));
        let options = layer_core::LayerBlend::ALL
            .iter()
            .filter(|b| **b != layer_core::LayerBlend::PassThrough || matches!(layer.content, OccurrenceContent::Stack(_)))
            .map(|b| blend_label(*b, l))
            .collect();
        controls.push(PropertyControl::new("blend", &l.text(MessageId::RESOURCES_BLEND_MODE), PropertyKind::Choice { options },
            EffectValue::Choice(layer.blend.code()), EffectValue::Choice(0)));
        if let Some(source) = scene.paint_source(handle)
            && doc.working.inspect_mask != Some(handle)
            && !matches!(doc.working.target, Some(SourceTarget::Coverage(_))) {
            let options = LAYER_COLOR_MODES.iter().map(|mode| art_layers::color_mode_label(*mode, l)).collect();
            let value = LAYER_COLOR_MODES.iter().position(|mode| *mode == source.color_mode).unwrap() as u32;
            controls.push(PropertyControl::new("color_mode", &l.text(MessageId::RESOURCES_LAYER_COLOR_MODE), PropertyKind::Choice { options },
                EffectValue::Choice(value), EffectValue::Choice(0)));
        }
        String::new()
    };
    LayerPropertiesView {
        layer: Some(token),
        description,
        enabled: !doc.is_locked(handle),
        controls,
        curve_max,
        curve_white,
        ..header
    }
}
pub(super) fn publish_properties(view:&mut LayerPropertiesView,doc:&Document,state:&mut PropertyEditorState,gesture:Option<&EffectGesture>,l:&Localizer) {
    let effect=view.layer.and_then(|token| occurrence_handle(token).ok()).and_then(|handle| property_effect(doc, handle));
    view.histogram=effect.is_some_and(|effect|effect.program.id.as_ref()=="levels");
    view.pages=effect.map_or_else(Vec::new,|effect|effect.program.pages.iter().filter(|page|view.controls.iter().any(|control|control.page.as_deref()==Some(page.id.as_ref()))).map(|page|PropertyPageView{id:page.id.to_string(),label:resource_label(&page.label,l).to_string()}).collect());
    state.sync(doc.owner,view.layer,doc.revision,view.pages.iter().map(|page|page.id.clone()).collect(),gesture.is_some());
    view.epoch=state.epoch;view.page=state.page().map(str::to_string);
    view.actions=effect.map_or_else(Vec::new,|effect| {
        use layer_core::levels::CalibrationRole;
        let roles:&[_]=match effect.program.id.as_ref() {"white_balance"=>&[CalibrationRole::Gray],"levels"|"curves"=>&[CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White],_=>&[]};
        roles.iter().map(|role|PropertyActionView {
            label:l.text(match role {CalibrationRole::Black=>MessageId::RESOURCES_PICKER_BLACK,CalibrationRole::Gray=>MessageId::RESOURCES_PICKER_NEUTRAL,CalibrationRole::White=>MessageId::RESOURCES_PICKER_WHITE}).to_string(),
            icon:Some("layer-eyedropper-symbolic".into()),
            group:(roles.len()>1).then(||PropertyActionGroup {id:"calibration",label:l.text(MessageId::RESOURCES_PICKER_POINTS).to_string()}),
            action:EffectAction::Calibrate {layer:view.layer.unwrap(),epoch:view.epoch,role:*role},
        }).collect()
    });
    if let Some(effect) = effect.filter(|effect| matches!(effect.program.auxiliary, Some(layer_core::EffectAuxiliary::Lut3d {..}))) {
        view.resource_label = Some(l.text(MessageId::RESOURCES_LOOKUP_TABLE).to_string());
        if let Some(layer_core::EffectAuxiliary::Lut3d {color_space,..})=&effect.program.auxiliary
            && effect.lut3d().is_none_or(|resource|layer_core::lut3d::Look::for_resource(resource).is_some()
                && effect.choice(color_space)==Some("srgb")) {view.controls.retain(|control|control.key!=color_space.as_ref());}
        if let Some(resource) = effect.lut3d() {
            let name = layer_core::lut3d::Look::for_resource(resource).map(|look|lookup_preset_label(Some(look),l))
                .unwrap_or_else(|| if resource.title().is_empty() {l.text(MessageId::RESOURCES_LOOKUP_TABLE).to_string()} else {resource.title().to_string()});
            view.description = name.clone(); view.resource_name = Some(name);
        }
        if view.resource_name.is_none() {view.resource_name=Some(lookup_preset_label(None,l));}
        for preset in [None].into_iter().chain(layer_core::lut3d::Look::ALL.into_iter().map(Some)) {
            let selected=match (preset,effect.lut3d()) {
                (None,None)=>true,
                (Some(look),Some(resource))=>layer_core::lut3d::Look::for_resource(resource)==Some(look)
                    && matches!(&effect.program.auxiliary,Some(layer_core::EffectAuxiliary::Lut3d {color_space,..}) if effect.choice(color_space)==Some("srgb")),
                _=>false,
            };
            if selected {view.resource_selection=Some(view.actions.len());}
            view.actions.push(PropertyActionView {label:lookup_preset_label(preset,l),icon:None,group:None,
                action:EffectAction::LookupPreset {layer:view.layer.unwrap(),epoch:view.epoch,preset}});
        }
        view.actions.push(PropertyActionView {label:l.text(MessageId::RESOURCES_LOOKUP_IMPORT).to_string(),
            icon:Some("layer-folder-open-symbolic".into()),group:None,action:EffectAction::ImportLookup {layer:view.layer.unwrap(),epoch:view.epoch}});
    }
    if effect.is_some_and(|effect|effect.program.id.as_ref()=="levels") {
        view.actions.push(PropertyActionView {label:l.text(MessageId::RESOURCES_LEVELS_AUTO).to_string(),icon:None,group:None,action:EffectAction::AutoLevels {layer:view.layer.unwrap(),epoch:view.epoch}});
    }
    if effect.is_some_and(|effect|effect.program.id.as_ref()=="curves") {
        view.actions.push(PropertyActionView {label:l.text(MessageId::RESOURCES_CURVE_TARGETED).to_string(),icon:Some("layer-cursor-sight-symbolic".into()),group:None,action:EffectAction::TargetCurve {layer:view.layer.unwrap(),epoch:view.epoch}});
    }
    view.controls.retain(|control|control.page.as_deref().is_none_or(|page|Some(page)==state.page()));
    if effect.is_some_and(|effect|effect.program.id.as_ref()=="gradient_fill") {
        view.controls.sort_by_key(|control|control.key!="style");
    }
    let domain=effect.filter(|effect|effect.choice("domain")==Some("log_hdr")).and_then(|effect|match effect.value("hdr_stops"){Some(EffectValue::Number(stops))=>Some(CurveDomain::LogHdr{stops:*stops}),_=>None}).unwrap_or(CurveDomain::Encoded);
    for control in &mut view.controls {
        if matches!(control.value,EffectValue::Gradient(_)) {
            control.gradient=Some(GradientControls::new(GradientDestination::Effect {layer:view.layer.unwrap(),key:control.key.clone(),epoch:view.epoch},match &control.value {EffectValue::Gradient(value)=>value,_=>unreachable!()},l));
        }
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

fn lookup_preset_label(preset:Option<layer_core::lut3d::Look>,l:&Localizer)->String {
    use layer_core::lut3d::Look;
    l.text(match preset {None=>MessageId::RESOURCES_LOOKUP_ORIGINAL,Some(Look::Warm)=>MessageId::RESOURCES_LOOKUP_WARM,
        Some(Look::Cool)=>MessageId::RESOURCES_LOOKUP_COOL,Some(Look::Monochrome)=>MessageId::RESOURCES_LOOKUP_MONOCHROME}).to_string()
}
pub(super) fn effect_baseline(document: &Document, occurrence: OccurrenceHandle) -> Result<EffectBaseline, String> {
    let (effect, application) = effect_application(document, occurrence).ok_or("Missing adjustment")?;
    Ok(EffectBaseline {occurrence, effect, application: application.clone()})
}
#[derive(Clone, PartialEq)]
enum PropertyBaseline {
    Effect {value: EffectBaseline, definition: Definition},
    Occurrence {handle: OccurrenceHandle, value: Occurrence},
    SelectionDisplay {occurrence: OccurrenceHandle, handle: SelectionHandle, value: Option<layer_core::SelectionMaskProperties>},
}
impl PropertyBaseline {
    fn capture(document: &Document, handle: OccurrenceHandle) -> Result<Self, String> {
        if let Some(SourceTarget::Selection(selection)) = document.scene().source_target(handle) {
            let value = document.working.selection_overlays.properties.get(&selection).cloned();
            Ok(Self::SelectionDisplay {occurrence: handle, handle: selection, value})
        } else if let Ok(value) = effect_baseline(document, handle) {
            let definition = document.artwork.definitions.get(value.application.definition).ok_or("Missing adjustment definition")?.clone();
            Ok(Self::Effect {value, definition})
        } else {
            Ok(Self::Occurrence {handle, value: document.scene().occurrence(handle).ok_or("Unknown occurrence")?.clone()})
        }
    }
    fn occurrence(&self) -> OccurrenceHandle {
        match self {Self::Effect {value,..} => value.occurrence, Self::Occurrence {handle,..} => *handle, Self::SelectionDisplay {occurrence,..} => *occurrence}
    }
    fn edit(&self, document: &Document) -> Result<Edit, String> {
        let art = &document.artwork;
        match self {
            Self::Effect {value, definition} => Ok(Edit::Batch(vec![
                Edit::Definition(RecordChange::replace(&art.definitions, value.application.definition, Some(definition.clone())).map_err(str::to_string)?),
                Edit::Effect(RecordChange::replace(&art.effects, value.effect, Some(value.application.clone())).map_err(str::to_string)?),
            ])),
            Self::Occurrence {handle, value} => Ok(Edit::Occurrence(RecordChange::replace(&art.occurrences, *handle, Some(value.clone())).map_err(str::to_string)?)),
            Self::SelectionDisplay {handle, value, ..} => {
                let mut working=document.working.clone();
                match value {Some(value)=>{working.selection_overlays.properties.insert(*handle,value.clone());},None=>{working.selection_overlays.properties.remove(handle);}}
                Ok(Edit::Working(working))
            },
        }
    }
}
pub(super) struct EffectGesture {
    original: PropertyBaseline,
    key: String,
    detached_curve_point: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn cancel_effect_gesture(&mut self) -> Result<bool, String> {
        if let Some(original)=self.layer_interaction.gradient_before.take() {self.layer_interaction.gradient=original;self.refresh_document();return Ok(true);}
        if let Some((_, old)) = self.selection_masks.quick_property_gesture.take() {
            self.selection_masks.quick_properties = old;
            self.refresh_document();
            return Ok(true);
        }
        let Some(gesture) = self.effect_gesture.take() else {
            return Ok(false);
        };
        self.engine
            .preview_edit(gesture.original.edit(self.engine.document())?)
            .map_err(error)?;
        self.refresh_document();
        Ok(true)
    }

    pub(super) fn effect_gesture_action(
        &mut self,
        phase: ContactPhase,
        action: EffectAction,
    ) -> Result<(), String> {
        if let EffectAction::Gradient {target,..}=&action {
            let accepted=match target {
                GradientDestination::Tool {epoch}=>*epoch==self.state.document_file.epoch && matches!(self.layer_interaction.tool,LayerCanvasTool::Gradient {..}),
                GradientDestination::Effect {layer,epoch,..}=>self.property_editor.accepts(*layer,*epoch),
            };
            if !accepted {return Ok(());}
        }
        if matches!(&action,EffectAction::Gradient {target:GradientDestination::Tool {..},..}) {
            if phase==ContactPhase::Down {self.require_idle()?;self.layer_interaction.gradient_before=Some(self.layer_interaction.gradient.clone());}
            else if self.layer_interaction.gradient_before.is_none() {return Ok(());}
            if phase==ContactPhase::Cancel || self.workspace_read_only || self.workspace_transition || self.rendering_suspended {self.cancel_effect_gesture()?;return Ok(());}
            if let Err(reason)=self.effect_action(action) {self.cancel_effect_gesture()?;return Err(reason);}
            if phase==ContactPhase::Up {self.layer_interaction.gradient_before=None;}
            return Ok(());
        }
        if let EffectAction::CurveNumber{layer,key,epoch,..}=&action
            && !self.curve_action_target(*layer,key,*epoch) {return Ok(());}
        let (layer, key) = match &action {
            EffectAction::CurvePoint { layer, key, .. }
            | EffectAction::CurveNumber { layer, key, .. }
            | EffectAction::Number { layer, key, .. }
            | EffectAction::Gradient {target:GradientDestination::Effect {layer,key,..},..}
            | EffectAction::Set {
                layer,
                key,
                value: EffectValue::Number(_) | EffectValue::Color(_) | EffectValue::Curve(_) | EffectValue::Gradient(_),
            } => (*layer, key.clone()),
            _ => return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_PROPERTY_NOT_DRAGGABLE).to_string()),
        };
        if layer == 0 && self.selection_masks.quick() && key.starts_with("mask_") {
            if phase == ContactPhase::Down {
                self.require_idle()?;
                self.selection_masks.quick_property_gesture = Some((key.clone(), self.selection_masks.quick_properties.clone()));
            } else if self.selection_masks.quick_property_gesture.as_ref().is_none_or(|(k, _)| *k != key) {
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
            let handle = occurrence_handle(layer)?;
            let original = PropertyBaseline::capture(self.engine.document(), handle)?;
            if self.engine.document().is_locked(handle) {
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
            .is_some_and(|g| occurrence_token(g.original.occurrence()) == layer && g.key == key)
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
            let edited = PropertyBaseline::capture(self.engine.document(), gesture.original.occurrence())?;
            let changed = edited != gesture.original;
            let original_edit = gesture.original.edit(self.engine.document())?;
            self.engine.preview_edit(original_edit).map_err(error)?;
            if changed {
                self.layer_edit(edited.edit(self.engine.document())?)?;
            }
            self.property_editor.commit_revision(self.engine.document().revision);
        }
        Ok(())
    }

    fn effect_parameter(&self, layer: u64, key: &str) -> Result<EffectValue, String> {
        let document = self.engine.document();
        let handle = occurrence_handle(layer)?;
        if document.scene().occurrence(handle).is_none() {
            return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_LAYER).to_string());
        }
        let effect = document.scene().effect(handle).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_ADJUSTMENT_REQUIRED).to_string())?;
        effect.value(key).cloned().ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_PROPERTY).to_string())
    }

    fn curve_action_target(&self,layer:u64,key:&str,epoch:u64)->bool {
        self.property_editor.accepts(layer,epoch) && self.state.layer_properties.layer==Some(layer)
            && self.state.layer_properties.controls.iter().any(|control|control.key==key && matches!(control.kind,PropertyKind::Curve))
    }
    fn insert_effect(&mut self, effect: Arc<str>, destination: Option<(OccurrenceHandle, u64)>) -> Result<(), String> {
        if let Some((owner, epoch)) = destination {
            if epoch != self.state.document_file.epoch { return Ok(()); }
            let doc = self.engine.document();
            if !doc.scene().eligible_target(owner) || doc.is_locked(owner)
                || self.effect_catalog.get(&effect).is_none_or(|f| f.program.kind != layer_core::EffectKind::Adjustment) {
                return Err(self.localization().text(MessageId::RESOURCES_ERROR_INVALID_LAYER_PROPERTY).to_string());
            }
        }
        let choosing = destination.is_none() && self.filter_drawer_open();
        let catalog = self.effect_catalog.get(&effect).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_FILTER).to_string())?;
        let generator = catalog.program.kind == layer_core::EffectKind::Generator;
        let doc = self.engine.document();
        let scene = doc.scene();
        let current = destination.map(|(owner, _)| owner).or(doc.working.occurrence);
        let occurrence = current.and_then(|id| scene.occurrence(id));
        let replacing = choosing && current.and_then(|id| scene.effect(id)).is_some_and(|effect| effect.program.kind == catalog.program.kind);
        let masked = !replacing && doc.working.selection.is_some();
        if replacing && doc.is_locked(current.unwrap()) { return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_LAYER_LOCKED).to_string()); }
        if replacing && current.and_then(|id| scene.effect(id)).is_some_and(|effect| effect.program.id == catalog.program.id) { return Ok(()); }
        let attaches=choosing && current.is_some_and(|id| scene.eligible_target(id)) && occurrence.is_some_and(|o| o.attachment==layer_core::Attachment::Clip);
        let top = destination.map(|(owner, _)| scene.attached_effects(owner).last().copied().unwrap_or(owner)).or_else(|| current.map(|id| if choosing && (replacing || generator || attaches) { id } else { doc.clipping_stack_top(id).unwrap_or(id) }));
        let parent = current.and_then(|id| scene.parent(id));
        if !replacing && parent.is_some_and(|id| doc.is_locked(id)) { return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_LAYER_LOCKED).to_string()); }
        let stack_handle = top.and_then(|id| scene.stack(id)).unwrap_or(doc.composition().result);
        let mut stack = doc.artwork.stacks.get(stack_handle).ok_or("Missing containing stack")?.clone();
        let index = top.and_then(|id| stack.entries.iter().position(|handle| *handle == id)).unwrap_or(0);
        let (index,insertion_attachment)=if generator&&!replacing {doc.content_insertion(parent,index)}else{(index,layer_core::Attachment::None)};
        let depth = doc.composition().color.depth;
        let mut instance = EffectInstance::new(catalog.program());
        if depth.is_float() && instance.program.id.as_ref() == "curves" { instance.set_choice("domain", "log_hdr").map_err(str::to_string)?; }
        if generator && let Some(color) = instance.program.parameters.iter().find(|parameter| parameter.kind == EffectParameterKind::Color) {
            instance.set(&color.key.clone(), EffectValue::Color(self.state.colors.definition())).map_err(str::to_string)?;
        }
        let mut edits = Vec::new();
        let definition = if let Some((handle, _, _)) = doc.artwork.definitions.iter().find(|(_, _, definition)| definition.program == instance.program) {
            handle
        } else {
            let change = RecordChange::insert(&doc.artwork.definitions, Definition {program: instance.program});
            let handle = change.handle; edits.push(Edit::Definition(change)); handle
        };
        let application = EffectApplication {definition, values: instance.values};
        let effect_handle = if replacing {
            let handle = scene.effect_handle(current.unwrap()).ok_or("Missing adjustment")?;
            edits.extend(doc.effect_edits(vec![RecordChange::replace(&doc.artwork.effects, handle, Some(application)).map_err(str::to_string)?]).map_err(|error|error.to_string())?);
            handle
        } else {
            let change = RecordChange::insert(&doc.artwork.effects, application);
            let handle = change.handle; edits.push(Edit::Effect(change)); handle
        };
        let mut occurrence = if replacing { occurrence.unwrap().clone() } else {
            let mut value = Occurrence::new(OccurrenceContent::Effect(effect_handle), resource_label(catalog.label(), &self.state.localization));
            value.attachment = if choosing && generator {insertion_attachment} else if attaches || destination.is_some() {
                layer_core::Attachment::Effect
            } else { layer_core::Attachment::None }; value
        };
        occurrence.content = OccurrenceContent::Effect(effect_handle);
        if masked {
            let (coverage, mask) = self.selection_mask(&occurrence, false, parent, if replacing { scene.local_extent(current.unwrap()) } else { doc.composition().size })?;
            edits.push(Edit::Coverage(coverage)); occurrence.mask = Some(mask);
        }
        let handle = if replacing {
            edits.push(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, current.unwrap(), Some(occurrence)).map_err(str::to_string)?)); current.unwrap()
        } else {
            let change = RecordChange::insert(&doc.artwork.occurrences, occurrence);
            let handle = change.handle; edits.push(Edit::Occurrence(change));
            stack.entries.insert(index, handle);
            edits.push(Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack_handle, Some(stack)).map_err(str::to_string)?)); handle
        };
        let mut working = doc.working.clone();
        working.occurrence = Some(handle); working.target = None; working.inspect_mask = None;
        working.layer_selection = [handle].into(); working.layer_anchor = Some(handle);
        if masked { working.selection = None; }
        edits.push(Edit::Working(working));
        self.layer_edit(Edit::Batch(edits))?;
        if !choosing {
            self.state.customization.expanded = None;
            self.state.workspace.layout.reveal_after(Panel::Properties, Panel::Adjustments)?;
        }
        Ok(())
    }
    pub(super) fn effect_action(&mut self, action: EffectAction) -> Result<(), String> {
        if let Some(result) = self.mask_property_action(&action) { return result; }

        if self.selection_masks.target().is_some() && !matches!(action, EffectAction::Gesture { .. } | EffectAction::Gradient {target:GradientDestination::Tool {..},..}) {return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_ARTWORK_REQUIRED).to_string());}
        match action {
            EffectAction::LookupPreset {layer,epoch,preset} => {
                if !self.property_editor.accepts(layer,epoch) {return Ok(());}
                self.cancel_effect_gesture()?;
                self.editable_layer(layer)?;
                let handle=occurrence_handle(layer)?;
                let document=self.engine.document();
                let Some(original)=document.scene().effect(handle) else {return Ok(());};
                let Some(layer_core::EffectAuxiliary::Lut3d {resource,color_space})=original.program.auxiliary.clone() else {return Ok(());};
                let mut draft=effect_draft(document,handle)?;
                draft.set(&resource,EffectValue::Lut3d(None)).map_err(str::to_string)?;
                if let Some(preset)=preset {
                    draft.set_choice(&color_space,"srgb").map_err(str::to_string)?;
                    draft.set(&resource,EffectValue::Lut3d(Some(preset.resource()))).map_err(str::to_string)?;
                }
                if draft.view()==original {return Ok(());}
                let edit=effect_edit(document,handle,draft)?;
                self.layer_edit(edit)?;
            }
            EffectAction::ImportLookup {layer,epoch} => {
                if !self.property_editor.accepts(layer,epoch) {return Ok(());}
                self.cancel_effect_gesture()?;
                let Some(effect) = self.engine.document().scene().effect(occurrence_handle(layer)?) else {return Ok(());};
                let Some(layer_core::EffectAuxiliary::Lut3d {resource,..}) = &effect.program.auxiliary else {return Ok(());};
                return self.request_document(DocumentRequest::ImportLookup {target: LookupTarget {document:self.engine.document().artwork.id,activation:self.state.document_file.epoch,layer,epoch,key:resource.to_string()}});
            }
            EffectAction::TargetCurve {layer,epoch}=>return self.start_targeted_curve(layer,epoch),
            EffectAction::AutoLevels {layer,epoch}=>return self.start_auto_levels(layer,epoch),
            EffectAction::Calibrate { layer, epoch, role } => return self.start_calibration(layer, epoch, role),
            EffectAction::SelectPage{layer,page}=> {
                if self.state.layer_properties.layer!=Some(layer) || self.state.layer_properties.page.as_deref()==Some(&page)
                    || !self.state.layer_properties.pages.iter().any(|candidate|candidate.id==page){return Ok(());}
                self.cancel_targeted_contact()?;self.cancel_effect_gesture()?;
                self.property_editor.select_page(layer,&page);
                self.refresh_document();self.sync_targeted_page();return Ok(());
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
                    let index=curves::hit(&points,point,extent).or_else(||layer_core::curves::curve_reusable_knot(&points,point[0]/extent[0]));
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
                if let Some(handle) = doc.working.occurrence.filter(|handle| doc.scene().effect(*handle).is_some()) {
                    self.layer_action(LayerAction::Delete { id: occurrence_token(handle) })?;
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
            EffectAction::Gradient {target,edit}=>return self.gradient_action(target,edit),
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
                let dragging = self.effect_gesture.as_ref().filter(|g| occurrence_token(g.original.occurrence()) == layer && g.key == key);
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
                    let _=layer_core::curves::curve_place_knot(&mut points,[point[0].clamp(0.,1.),point[1].clamp(0.,1.)]);
                }
                return self.effect_action(EffectAction::Set {
                    layer,
                    key,
                    value: EffectValue::Curve(points),
                });
            }
            EffectAction::Insert { effect } => self.insert_effect(effect, None)?,
            EffectAction::InsertAttached { effect, owner, epoch } => {
                self.insert_effect(effect, Some((occurrence_handle(owner)?, epoch)))?;
            }
            EffectAction::Set {
                layer: id,
                key,
                value,
            } => {
                let handle = occurrence_handle(id)?;
                let document = self.engine.document();
                document.scene().occurrence(handle).ok_or_else(|| self.state.localization.text(MessageId::RESOURCES_ERROR_UNKNOWN_LAYER).to_string())?;
                if document.scene().effect(handle).is_none() {
                    return match (key.as_str(), value) {
                        ("opacity", EffectValue::Number(opacity)) => {
                            self.set_layer_opacity(Some(id), opacity)
                        }
                        ("blend", EffectValue::Choice(value)) => {
                            self.layer_action(LayerAction::Blend { id, value })
                        }
                        ("color_mode", EffectValue::Choice(value)) if LAYER_COLOR_MODES.get(value as usize).is_some() => {
                            self.layer_action(LayerAction::ColorMode { id, epoch: self.state.document_file.epoch, mode: LAYER_COLOR_MODES[value as usize] })
                        }
                        _ => Err(self.state.localization.text(MessageId::RESOURCES_ERROR_INVALID_LAYER_PROPERTY).to_string()),
                    };
                }
                if document.is_locked(handle) {
                    return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_LAYER_LOCKED).to_string());
                }
                let original = document.scene().effect(handle).ok_or("Missing adjustment")?;
                let mut draft = effect_draft(document, handle)?;
                draft.set(&key, value).map_err(str::to_string)?;
                if draft.view() == original { return Ok(()); }
                let edit = effect_edit(document, handle, draft)?;
                if self.effect_gesture.is_some() { self.engine.preview_edit(edit).map_err(error)?; }
                else { self.layer_edit(edit)?; }

            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use layer_core::{EffectCatalog, EffectInstallMode, EffectPackage};

    #[test]
    fn properties_titles_identify_nonpaint_layers_without_repeating_matching_names() {
        for (language, fill_type, group_type, selection_type, paper_title) in [
            (UiLanguage::English, "Solid Color", "Group", "Selection", "Paper (Solid Color)"),
            (UiLanguage::Japanese, "単色", "グループ", "選択範囲", "Paper（単色）"),
        ] {
            let l = Localizer::shared(language);
            let mut doc = Document::new(layer_core::authored::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
            let fill = occurrence_handle(2).unwrap();
            doc.apply(doc.select_occurrence_edit(fill).unwrap()).unwrap();
            assert_eq!(properties(&doc, Default::default(), &l).title, paper_title);
            for (kind, name, expected) in [
                (LayerKind::Paint, "Paper", "Paper"),
                (LayerKind::Effect, fill_type, fill_type),
                (LayerKind::Group, group_type, group_type),
                (LayerKind::Selection, selection_type, selection_type),
                (LayerKind::Group, "Sky", if language == UiLanguage::English { "Sky (Group)" } else { "Sky（グループ）" }),
                (LayerKind::Selection, "{ $type } 🎨", if language == UiLanguage::English { "{ $type } 🎨 (Selection)" } else { "{ $type } 🎨（選択範囲）" }),
            ] {
                let mut selected = doc.clone();
                let content = match kind {
                    LayerKind::Effect => doc.scene().occurrence(fill).unwrap().content.clone(),
                    LayerKind::Paint => doc.scene().occurrence(occurrence_handle(1).unwrap()).unwrap().content.clone(),
                    LayerKind::Group => {
                        let stack = selected.artwork.stacks.insert(layer_core::authored::PortableId::random(), Default::default()).unwrap();
                        OccurrenceContent::Stack(stack)
                    }
                    LayerKind::Selection => {
                        let selection = selected.artwork.selections.insert(layer_core::authored::PortableId::random(), layer_core::authored::SavedSelection {
                            selection: layer_core::Selection::empty(),
                        }).unwrap();
                        OccurrenceContent::Selection(selection)
                    }
                };
                *selected.artwork.occurrences.get_mut(fill).unwrap() = layer_core::authored::Occurrence::new(content, name);
                let view = properties(&selected, Default::default(), &l);
                assert_eq!(view.title, expected);
                assert_eq!(view.name, name);
                assert_eq!(view.layer_type.as_str(), match kind {
                    LayerKind::Paint => if language == UiLanguage::English { "Paint layer" } else { "ペイントレイヤー" },
                    LayerKind::Effect => fill_type,
                    LayerKind::Group => group_type,
                    LayerKind::Selection => selection_type,
                });
            }
        }
    }

    fn package() -> EffectPackage {
        let bundled = layer_core::bundled_effect_catalog();
        EffectPackage { format: 2, categories: bundled.categories().to_vec(), filters: bundled.filters().to_vec() }
    }
    #[test]
    fn custom_count_controls_step_in_whole_units_without_presentation_hints() {
        let mut parameter=layer_core::bundled_effect_catalog().get("kaleidoscope").unwrap().program().parameters[0].clone();
        let EffectParameterKind::Number {step,decimals,..}=&mut parameter.kind else {panic!()};
        *step=0.01;*decimals=6;
        let numeric=number_control(&parameter).unwrap();
        assert_eq!(numeric.step,1.);assert_eq!(numeric.digits,0);
        let value=numeric.resolve(6.,NumericOperation::Step {steps:1.}).unwrap().value;
        assert_eq!(value,7.);parameter.validate(&EffectValue::Number(value as f32)).unwrap();
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
    fn resource_custom_literal_definition_survives_localization_and_serialization() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut replacement = package();
        replacement.filters=vec![crate::session::test_support::custom_filter("curves")];
        let program = Arc::make_mut(&mut replacement.filters[0].program);
        program.label = "私の \"曲線\" {名前} 🎨".into();
        let parameters = Arc::make_mut(&mut program.parameters);
        parameters[0].label = "한글 설정".into();
        parameters[0].section = Some("我的分组".into());
        let candidate = layer_core::bundled_effect_catalog()
            .stage(resolve(replacement), EffectInstallMode::Add).unwrap();
        validate_catalog_labels(&candidate, &l).unwrap();
        let picker = FilterPickerState { search: Some("曲".into()), ..FilterPickerState::new(&l) };
        let choices = catalog(&candidate, &picker, &l);
        let choices:Vec<_>=choices.into_iter().filter(|c|c.id.as_ref()=="test:curves").collect();
        assert_eq!(choices.len(), 1);
        assert_eq!(&*choices[0].label, "私の \"曲線\" {名前} 🎨");
        let instance = EffectInstance::new(candidate.get("test:curves").unwrap().program());
        let copy: EffectInstance = serde_json::from_str(&serde_json::to_string(&instance).unwrap()).unwrap();
        assert_eq!(copy, instance);
        let control = control(7, &copy.program.parameters[0], copy.values[0].clone(), &l).unwrap();
        assert_eq!(control.label, "한글 설정");
        assert_eq!(control.section.as_deref(), Some("我的分组"));
    }

    #[test]
    fn resource_groups_and_choice_values_keep_identity_when_display_labels_match() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut custom = package();
        custom.categories[0].label = message(MessageId::COMMON_CANCEL);
        custom.categories[1].label = "キャンセル".into();
        let program = Arc::make_mut(&mut custom.filters.iter_mut().find(|filter| filter.id()=="curves").unwrap().program);
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
        let a = control(7, &instance.program.parameters[0], instance.values[0].clone(), &l).unwrap();
        let b = control(7, &instance.program.parameters[1], instance.values[1].clone(), &l).unwrap();
        assert_eq!(a.section, b.section);
        assert_ne!(a.section_id, b.section_id);
        let choice = control(7, &instance.program.parameters[4], EffectValue::Choice(1), &l).unwrap();
        let PropertyKind::Choice { options } = choice.kind else { panic!() };
        assert_eq!(&*options[1], "キャンセル");
        let mut instance = instance;
        instance.set("domain", EffectValue::Choice(1)).unwrap();
        assert_eq!(instance.choice("domain"), Some("log_hdr"));
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
                    let program = Arc::make_mut(&mut custom.filters.iter_mut().find(|filter| filter.id()=="curves").unwrap().program);
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

            }
        }
    }
}
