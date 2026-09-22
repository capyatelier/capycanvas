//! Effect catalog, properties and navigation policy shared by every native view.
use super::*;
use layer_core::{Edit, EffectInstance, EffectParameterKind, EffectValue, Layer};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

impl<B: CanvasRenderer> UiSession<B> {
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
        self.state.adjustments = catalog(&self.effect_catalog, &self.state.filter_picker);
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
        } else { doc.clipping_stack_top(current.id) }.ok_or("Select a layer first")?;
        let request = layer_render::FilterPreviewRequest {
            request_id,
            target,
            size,
            extent: [doc.width, doc.height],
            view: self.engine.view(),
            layers: doc.layers.iter().filter(|l| !replacing || l.id != current.id)
                .map(Layer::composite_snapshot).collect(),
            filters: filters
                .into_iter()
                .take(8)
                .map(|id| {
                    self.effect_catalog
                        .get(&id)
                        .ok_or_else(|| format!("Unknown filter: {id}"))?
                        .preview()
                        .map(Arc::new)
                })
                .collect::<Result<_, _>>()?,
        };
        self.engine
            .backend_mut()
            .request_filter_previews(request)
            .map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FilterPickerState {
    pub selected: Option<Arc<str>>,
    pub category: Option<Arc<str>>,
    pub search: Option<String>,
    pub search_label: &'static str,
    pub empty_label: &'static str,
}
impl Default for FilterPickerState {
    fn default() -> Self {
        Self {
            selected: None,
            category: None,
            search: None,
            search_label: "Search filters",
            empty_label: "No matching filters",
        }
    }
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
        _ => "adjustments",
    }
}
pub(super) fn categories(catalog: &layer_core::EffectCatalog) -> Vec<FilterCategoryChoice> {
    std::iter::once(FilterCategoryChoice {
        id: None,
        label: "All filters".into(),
        icon: "adjustments",
    })
    .chain(catalog.categories().iter().map(|c| FilterCategoryChoice {
        id: Some(c.id.clone()),
        label: c.label.clone(),
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

fn point_between(value: f32, lower: f32, upper: f32) -> f32 {
    let gap = ((upper - lower) * 0.25).min(0.001);
    value.clamp(lower + gap, upper - gap)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EffectAction {
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
) -> Vec<AdjustmentChoice> {
    let query = picker.search.as_deref().unwrap_or("").trim().to_lowercase();
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
            query
                .split_whitespace()
                .all(|word| id.label().to_lowercase().contains(word))
        })
        .map(|id| AdjustmentChoice {
            id: id.program.id.clone(),
            label: id.program.label.clone(),
            icon: id.icon.clone(),
            action: UiAction::Effect {
                action: EffectAction::Insert {
                    effect: id.program.id.clone(),
                },
            },
            category: id.category.clone(),
            category_icon: category_icon(&id.category),
            category_label: catalog
                .categories()
                .iter()
                .find(|c| c.id == id.category)
                .unwrap()
                .label
                .clone(),
            animated: id.program().time,
            tooltip: if id.program().time {
                format!("{} · Animated", id.label())
            } else {
                id.label().into()
            },
        })
        .collect()
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayerPropertiesView {
    pub layer: Option<u64>,
    pub title: String,
    pub description: String,
    pub enabled: bool,
    pub controls: Vec<PropertyControl>,
    /// Linear input/output range for HDR curve axes; absent for encoded curves.
    pub curve_max: Option<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyControl {
    pub plot: Vec<[f32; 2]>,
    pub key: String,
    pub label: String,
    pub section: Option<String>,
    pub kind: PropertyKind,
    pub value: EffectValue,
    pub default: EffectValue,
    /// Optional shortcut alongside a color editor, supplied by shared policy.
    pub color_action: Option<UiAction>,
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
fn control(p: &layer_core::EffectParameter, value: EffectValue) -> PropertyControl {
    let kind = match &p.kind {
        EffectParameterKind::Number {
            min,
            max,
            step,
            decimals,
            unit,
        } => {
            let mut numeric =
                NumericControl::number(*min as f64, *max as f64, *step as f64, *decimals as u32)
                    .unit(unit);
            // Percentages express an amount, not an item count, even when the
            // displayed range is short. Keep their compact slider presentation.
            if unit.as_ref() == "%" {
                numeric.kind = NumericKind::Slider;
            }
            if let EffectValue::Number(v) = p.default {
                numeric.default_value = Some(v as f64);
            }
            PropertyKind::Number { numeric }
        }
        EffectParameterKind::Toggle => PropertyKind::Toggle,
        EffectParameterKind::Choice { options } => PropertyKind::Choice {
            options: options.clone(),
        },
        EffectParameterKind::Color => PropertyKind::Color,
        EffectParameterKind::Curve => PropertyKind::Curve,
        EffectParameterKind::Gradient => PropertyKind::Gradient,
    };
    PropertyControl {
        plot: if let EffectValue::Curve(points) = &value {
            (0..=128)
                .map(|i| {
                    let x = i as f32 / 128.;
                    [x, layer_core::curve_value(points, x)]
                })
                .collect()
        } else {
            Vec::new()
        },
        key: p.key.to_string(),
        label: p.label.to_string(),
        section: p.section.as_ref().map(ToString::to_string),
        kind,
        value,
        default: p.default.clone(),
        color_action: None,
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

pub(super) fn properties(doc: &Document) -> LayerPropertiesView {
    let Some(layer) = doc.layer(doc.active_layer) else {
        return LayerPropertiesView::default();
    };
    let mut controls = Vec::new();
    let mut curve_max = None;
    let description = if let Some(effect) = &layer.effect {
        let program = float32_program(&effect.program, doc.color.depth);
        controls.extend(
            program
                .parameters
                .iter()
                .zip(&effect.values)
                .map(|(p, v)| control(p, v.clone())),
        );
        if effect.program.id.as_ref() == "curves" {
            let linear = effect.value("domain") == Some(&EffectValue::Choice(1));
            if linear {
                if let Some(EffectValue::Number(stops)) = effect.value("hdr_stops") { curve_max = Some(stops.exp2()); }
            }
            controls.retain(|c| match c.key.as_str() {
                "domain" => doc.color.depth.is_float() || linear,
                "hdr_stops" => linear,
                _ => true,
            });
            for c in &mut controls {
                if matches!(c.key.as_str(), "domain" | "hdr_stops") {
                    c.section = Some("Advanced".into());
                    // Older masters embed their original parameter labels.
                    c.label = if c.key == "domain" { "Curve space" } else { "HDR range" }.into();
                }
            }
        }
        effect.program.label.to_string()
    } else if layer.kind == LayerKind::Background {
        controls.push(PropertyControl {
            plot: Vec::new(), key: "paper_color".into(), label: "Paper color".into(),
            section: None, kind: PropertyKind::Color,
            value: EffectValue::Color(layer.properties.paper_color.unwrap_or(layer_core::color::RgbColor::WHITE)),
            default: EffectValue::Color(layer_core::color::RgbColor::WHITE),
            color_action: Some(UiAction::Effect { action: EffectAction::UseCurrentColor {
                layer: layer.id.0, key: "paper_color".into(),
            } }),
        });
        String::new()
    } else {
        let mut numeric = NumericControl::percent();
        numeric.default_value = Some(1.);
        controls.push(PropertyControl {
            plot: Vec::new(),
            key: "opacity".into(),
            label: "Opacity".into(),
            section: None,
            kind: PropertyKind::Number { numeric },
            value: EffectValue::Number(layer.opacity),
            default: EffectValue::Number(1.),
            color_action: None,
        });
        controls.push(PropertyControl {
                plot: Vec::new(),
                key: "blend".into(),
                label: "Blend mode".into(),
                section: None,
                kind: PropertyKind::Choice {
                    options: layer_core::LayerBlend::ALL
                        .iter()
                        .map(|b| Arc::from(b.label()))
                        .collect(),
                },
                value: EffectValue::Choice(layer.properties.blend as u32),
                default: EffectValue::Choice(0),
                color_action: None,
            });
        String::new()
    };
    LayerPropertiesView {
        layer: Some(layer.id.0),
        title: layer.name.to_string(),
        description,
        enabled: !doc.is_locked(layer.id),
        controls,
        curve_max,
    }
}
pub(super) struct EffectGesture {
    original: Layer,
    key: String,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn cancel_effect_gesture(&mut self) -> Result<bool, String> {
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
        let (layer, key) = match &action {
            EffectAction::CurvePoint { layer, key, .. }
            | EffectAction::GradientStop { layer, key, .. }
            | EffectAction::Set {
                layer,
                key,
                value: EffectValue::Number(_) | EffectValue::Color(_),
            } => (*layer, key.clone()),
            _ => return Err("Not a draggable effect property".into()),
        };
        if phase == ContactPhase::Down {
            self.require_idle()?;
            let original = self
                .engine
                .document()
                .layer(LayerId(layer))
                .ok_or("Unknown layer")?
                .clone();
            if self.engine.document().is_locked(original.id) {
                return Err("This layer is locked".into());
            }
            self.effect_gesture = Some(EffectGesture {
                original,
                key: key.clone(),
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
        }
        Ok(())
    }

    pub(super) fn effect_action(&mut self, action: EffectAction) -> Result<(), String> {
        match action {
            EffectAction::CancelFilter => {
                let doc = self.engine.document();
                if let Some(layer) = doc.layer(doc.active_layer).filter(|l| l.effect.is_some()) {
                    self.layer_action(LayerAction::Delete { id: layer.id.0 })?;
                }
                self.state.customization.drawer = None;
                self.state.customization.expanded = None;
            }
            EffectAction::UseCurrentColor { layer, key } => {
                return self.effect_action(EffectAction::Set {
                    layer, key, value: EffectValue::Color(self.state.colors.definition()),
                });
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
                    return Err("Invalid gradient position".into());
                }
                let l = self
                    .engine
                    .document()
                    .layer(LayerId(layer))
                    .ok_or("Unknown layer")?;
                let effect = l.effect.as_ref().ok_or("Not an adjustment")?;
                let i = effect
                    .program
                    .parameters
                    .iter()
                    .position(|p| p.key.as_ref() == key)
                    .ok_or("Unknown gradient")?;
                let EffectValue::Gradient(mut stops) = effect.values[i].clone() else {
                    return Err("Not a gradient".into());
                };
                if let Some(i) = index {
                    if i >= stops.len() {
                        return Err("Unknown gradient stop".into());
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
                    return Err("Invalid curve coordinate".into());
                }
                let l = self
                    .engine
                    .document()
                    .layer(LayerId(layer))
                    .ok_or("Unknown layer")?;
                let effect = l.effect.as_ref().ok_or("Not an adjustment")?;
                let i = effect
                    .program
                    .parameters
                    .iter()
                    .position(|p| p.key.as_ref() == key)
                    .ok_or("Unknown curve")?;
                let EffectValue::Curve(mut points) = effect.values[i].clone() else {
                    return Err("Not a curve".into());
                };
                if let Some(i) = index {
                    if i >= points.len() {
                        return Err("Unknown curve point".into());
                    }
                    if remove {
                        if i > 0 && i + 1 < points.len() {
                            points.remove(i);
                        }
                    } else {
                        let x = if i == 0 {
                            0.
                        } else if i + 1 == points.len() {
                            1.
                        } else {
                            point_between(point[0], points[i - 1][0], points[i + 1][0])
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
                let effect = self.effect_catalog.get(&effect).ok_or("Unknown filter")?;
                let doc = self.engine.document();
                let current = doc.layer(doc.active_layer).ok_or("Select a layer first")?;
                let replacing = choosing && current.effect.is_some();
                if replacing && doc.is_locked(current.id) { return Err("This layer is locked".into()); }
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
                    let mut layer = Layer::paint(self.engine.allocate_layer_id(), effect.label());
                    layer.properties.clipped = clipped;
                    layer
                };
                let id = layer.id;
                layer.name = effect.label().into();
                layer.kind = LayerKind::Effect;
                layer.properties.parent = parent;
                let mut instance = EffectInstance::new(float32_program(&effect.program(), depth));
                if hdr && instance.program.id.as_ref() == "curves" { instance.set("domain", EffectValue::Choice(1)).map_err(str::to_string)?; }
                layer.effect = Some(Arc::new(instance));
                self.layer_edit(if replacing { Edit::ReplaceLayer(Box::new(layer)) } else { Edit::Batch(vec![
                    Edit::InsertLayer { index, layer },
                    Edit::SetActiveLayer { id },
                ]) })?;
                if !choosing {
                    self.state.customization.expanded = None;
                    self.state.workspace.layout.reveal_after(Panel::Properties, Panel::Adjustments)?;
                }
            }
            EffectAction::Number {
                layer,
                key,
                operation,
            } => {
                let view = properties(self.engine.document());
                if view.layer != Some(layer) {
                    return Err("Select this layer before editing its properties".into());
                }
                let c = view
                    .controls
                    .iter()
                    .find(|c| c.key == key)
                    .ok_or("Unknown property")?;
                let (PropertyKind::Number { numeric }, EffectValue::Number(value)) =
                    (&c.kind, &c.value)
                else {
                    return Err("Not a numeric property".into());
                };
                let result = numeric.resolve(*value as f64, operation)?;
                return self.effect_action(EffectAction::Set {
                    layer,
                    key,
                    value: EffectValue::Number(result.value as f32),
                });
            }
            EffectAction::Reset { layer, key } => {
                let l = self
                    .engine
                    .document()
                    .layer(LayerId(layer))
                    .ok_or("Unknown layer")?;
                let value = if let Some(fx) = &l.effect {
                    fx.program
                        .parameters
                        .iter()
                        .find(|p| p.key.as_ref() == key)
                        .ok_or("Unknown property")?
                        .default
                        .clone()
                } else if key == "opacity" {
                    EffectValue::Number(1.)
                } else if key == "blend" {
                    EffectValue::Choice(0)
                } else if key == "paper_color" {
                    EffectValue::Color(layer_core::color::RgbColor::WHITE)
                } else {
                    return Err("Unknown property".into());
                };
                return self.effect_action(EffectAction::Set { layer, key, value });
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
                                return Err("Select unlocked paper to change its color".into());
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
                        _ => Err("Invalid layer property".into()),
                    };
                }
                let mut layer = self.editable_layer(id)?;
                let effect = layer.effect.as_mut().ok_or("Not an effect layer")?;
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
