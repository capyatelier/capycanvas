//! Effect catalog, properties and navigation policy shared by every native view.
use super::*;
use layer_core::{BuiltinEffect, Edit, EffectInstance, EffectParameterKind, EffectValue, Layer};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

impl<B: CanvasRenderer> UiSession<B> {
    pub fn filter_preview_revision(&self) -> (u64, u64) {
        let doc = self.engine.document();
        (doc.revision, doc.active_layer.0)
    }

    /// Hosts request only rows on screen, after painting and pending edits end.
    pub fn request_filter_previews(
        &mut self,
        request_id: u64,
        filters: Vec<BuiltinEffect>,
        size: [u32; 2],
    ) -> Result<bool, String> {
        if self.input_pending
            || self.engine.has_active_stroke()
            || self.engine.has_pending_document_edits()
        {
            return Ok(false);
        }
        let doc = self.engine.document();
        let request = layer_render::FilterPreviewRequest {
            request_id,
            target: doc.active_layer,
            size,
            extent: [doc.width, doc.height],
            view: self.engine.view(),
            layers: doc.layers.iter().map(Layer::composite_snapshot).collect(),
            filters: filters.into_iter().take(8).collect(),
        };
        self.engine
            .backend_mut()
            .request_filter_previews(request)
            .map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FilterPickerState {
    pub category: Option<layer_core::FilterCategory>,
    pub search: Option<String>,
    pub search_label: &'static str,
    pub empty_label: &'static str,
}
impl Default for FilterPickerState {
    fn default() -> Self {
        Self {
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
    Category {
        category: Option<layer_core::FilterCategory>,
    },
    Search {
        query: String,
    },
    ToggleSearch,
}
#[derive(Clone, Debug, Serialize)]
pub struct FilterCategoryChoice {
    pub id: Option<layer_core::FilterCategory>,
    pub label: &'static str,
}
impl FilterPickerState {
    pub fn categories(&self) -> Vec<FilterCategoryChoice> {
        std::iter::once(FilterCategoryChoice {
            id: None,
            label: "All filters",
        })
        .chain(
            layer_core::FilterCategory::ALL
                .into_iter()
                .map(|id| FilterCategoryChoice {
                    id: Some(id),
                    label: id.label(),
                }),
        )
        .collect()
    }
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
    Insert {
        effect: BuiltinEffect,
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
        color: Option<[f32; 4]>,
        remove: bool,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct AdjustmentChoice {
    pub id: BuiltinEffect,
    pub label: &'static str,
    pub icon: &'static str,
    pub action: UiAction,
    pub tile_cells: [u32; 2],
    pub category: layer_core::FilterCategory,
    pub category_label: &'static str,
}
pub(super) fn catalog(picker: &FilterPickerState) -> Vec<AdjustmentChoice> {
    let query = picker.search.as_deref().unwrap_or("").trim().to_lowercase();
    layer_core::FilterCategory::ALL
        .into_iter()
        .flat_map(|category| {
            BuiltinEffect::ALL
                .into_iter()
                .filter(move |id| id.category() == category)
        })
        .filter(|id| picker.category.is_none_or(|c| id.category() == c))
        .filter(|id| {
            query
                .split_whitespace()
                .all(|word| id.label().to_lowercase().contains(word))
        })
        .map(|id| AdjustmentChoice {
            id,
            label: id.label(),
            icon: id.id(),
            action: UiAction::Effect {
                action: EffectAction::Insert { effect: id },
            },
            tile_cells: [3, 2],
            category: id.category(),
            category_label: id.category().label(),
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
    }
}
pub(super) fn properties(doc: &Document) -> LayerPropertiesView {
    let Some(layer) = doc.layer(doc.active_layer) else {
        return LayerPropertiesView::default();
    };
    let mut controls = Vec::new();
    let description = if let Some(effect) = &layer.effect {
        controls.extend(
            effect
                .program
                .parameters
                .iter()
                .zip(&effect.values)
                .map(|(p, v)| control(p, v.clone())),
        );
        effect.program.label.to_string()
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
        });
        if layer.kind != LayerKind::Background {
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
            });
        }
        String::new()
    };
    LayerPropertiesView {
        layer: Some(layer.id.0),
        title: layer.name.to_string(),
        description,
        enabled: !doc.is_locked(layer.id),
        controls,
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn effect_action(&mut self, action: EffectAction) -> Result<(), String> {
        match action {
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
                        let color =
                            color.unwrap_or_else(|| layer_core::gradient_value(&stops, position));
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
                let doc = self.engine.document();
                let current = doc.layer(doc.active_layer).ok_or("Select a layer first")?;
                let index = doc.layers.iter().position(|l| l.id == current.id).unwrap();
                let parent = current.properties.parent;
                let id = self.engine.allocate_layer_id();
                let mut layer = Layer::paint(id, effect.label());
                layer.kind = LayerKind::Effect;
                layer.properties.parent = parent;
                layer.effect = Some(Arc::new(EffectInstance::new(effect.program())));
                self.layer_edit(Edit::Batch(vec![
                    Edit::InsertLayer { index, layer },
                    Edit::SetActiveLayer { id },
                ]))?;
                self.state.customization.expanded = None;
                let layout = &mut self.state.workspace.layout;
                layout.reveal_after(Panel::Properties, Panel::Adjustments)?;
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
                        ("opacity", EffectValue::Number(opacity)) => {
                            self.layer_edit(Edit::SetLayerOpacity {
                                id: LayerId(id),
                                opacity,
                            })
                        }
                        ("blend", EffectValue::Choice(value)) => {
                            self.layer_action(LayerAction::Blend { id, value })
                        }
                        _ => Err("Invalid layer property".into()),
                    };
                }
                let mut layer = self.editable_layer(id)?;
                Arc::make_mut(layer.effect.as_mut().ok_or("Not an effect layer")?)
                    .set(&key, value)
                    .map_err(str::to_string)?;
                self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))?;
            }
        }
        Ok(())
    }
}
