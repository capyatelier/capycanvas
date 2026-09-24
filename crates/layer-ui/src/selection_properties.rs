//! One property model for temporary and stored selection masks.
use super::*;
use layer_core::{Edit, EffectValue, SelectionMaskProperties, SelectionPaintBehavior};

pub(super) fn properties(
    id: u64,
    title: &str,
    p: &SelectionMaskProperties,
    enabled: bool,
) -> LayerPropertiesView {
    let defaults = SelectionMaskProperties::default();
    let mut controls = Vec::new();
    let mut add = |key: &str, label: &str, kind, value, default| {
        controls.push(PropertyControl {
            plot: Vec::new(),
            key: key.into(),
            label: label.into(),
            section: None,
            kind,
            value,
            default,
            color_action: None,
        })
    };
    add(
        "mask_painting",
        "Painting",
        PropertyKind::Choice {
            options: ["Color / transparent", "Black / white"]
                .map(std::sync::Arc::from)
                .into(),
        },
        EffectValue::Choice(u32::from(p.painting == SelectionPaintBehavior::BlackWhite)),
        EffectValue::Choice(0),
    );
    add(
        "mask_color",
        "Overlay color",
        PropertyKind::Color,
        EffectValue::Color(p.color),
        EffectValue::Color(defaults.color),
    );
    add(
        "mask_opacity",
        "Overlay opacity",
        PropertyKind::Number {
            numeric: NumericControl::percent(),
        },
        EffectValue::Number(p.opacity),
        EffectValue::Number(defaults.opacity),
    );
    add(
        "mask_side",
        "Overlay",
        PropertyKind::Choice {
            options: ["Selected areas", "Protected areas"]
                .map(std::sync::Arc::from)
                .into(),
        },
        EffectValue::Choice(u32::from(p.protected)),
        EffectValue::Choice(0),
    );
    controls[1].color_action = Some(UiAction::Effect {
        action: EffectAction::UseCurrentColor {
            layer: id,
            key: "mask_color".into(),
        },
    });
    LayerPropertiesView {
        layer: Some(id),
        title: title.into(),
        description: String::new(),
        enabled,
        controls,
        curve_max: None,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn mask_properties(&self) -> SelectionMaskProperties {
        match self.selection_masks.target() {
            Some(layer_core::SelectionTarget::Saved(id)) => self
                .engine
                .document()
                .layer(id)
                .and_then(|l| l.properties.selection_mask.clone())
                .unwrap_or_default(),
            _ => self.selection_masks.quick_properties.clone(),
        }
    }
    pub(super) fn mask_paint_value(&self, eraser: bool) -> f32 {
        if eraser || self.selection_masks.erases() {
            0.
        } else if self.mask_properties().painting == SelectionPaintBehavior::ColorTransparency {
            1.
        } else {
            self.selection_masks.gray()
        }
    }
    pub(super) fn mask_property_action(
        &mut self,
        action: &EffectAction,
    ) -> Option<Result<(), String>> {
        let (id, key) = match action {
            EffectAction::Set { layer, key, .. }
            | EffectAction::Reset { layer, key }
            | EffectAction::Number { layer, key, .. }
            | EffectAction::UseCurrentColor { layer, key } => (*layer, key),
            _ => return None,
        };
        if !key.starts_with("mask_") {
            return None;
        }
        Some((|| {
            let target = if id == 0 && self.selection_masks.quick() {
                layer_core::SelectionTarget::Current
            } else {
                self.engine
                    .document()
                    .saved_selection(LayerId(id))
                    .map_err(error)?;
                layer_core::SelectionTarget::Saved(LayerId(id))
            };
            if self.selection_masks.target() != Some(target) {
                return Err("Select this mask before editing its properties".into());
            }
            let mut p = self.mask_properties();
            let view = properties(id, "", &p, true);
            let field = view
                .controls
                .iter()
                .find(|c| c.key == *key)
                .ok_or("Unknown mask property")?;
            let value = match action {
                EffectAction::Set { value, .. } => value.clone(),
                EffectAction::Reset { .. } => field.default.clone(),
                EffectAction::UseCurrentColor { .. } => {
                    EffectValue::Color(self.state.colors.definition())
                }
                EffectAction::Number { operation, .. } => {
                    let (PropertyKind::Number { numeric }, EffectValue::Number(value)) =
                        (&field.kind, &field.value)
                    else {
                        return Err("Not a number".into());
                    };
                    EffectValue::Number(
                        numeric.resolve(*value as f64, operation.clone())?.value as f32,
                    )
                }
                _ => unreachable!(),
            };
            match (key.as_str(), value) {
                ("mask_painting", EffectValue::Choice(v @ 0..=1)) => {
                    p.painting = if v == 0 {
                        SelectionPaintBehavior::ColorTransparency
                    } else {
                        SelectionPaintBehavior::BlackWhite
                    }
                }
                ("mask_color", EffectValue::Color(c)) => p.color = c,
                ("mask_opacity", EffectValue::Number(v)) => p.opacity = v,
                ("mask_side", EffectValue::Choice(v @ 0..=1)) => p.protected = v == 1,
                _ => return Err("Invalid mask property".into()),
            }
            p.validate().map_err(error)?;
            let grayscale = p.painting == SelectionPaintBehavior::BlackWhite;
            if id == 0 {
                self.selection_masks.quick_properties = p;
            } else {
                let doc = self.engine.document();
                if doc.is_locked(LayerId(id)) {
                    return Err("This selection layer is locked".into());
                }
                let mut layer = doc.layer(LayerId(id)).unwrap().clone();
                layer.properties.selection_mask = Some(p);
                let edit = Edit::ReplaceLayer(Box::new(layer));
                if self.effect_gesture.is_some() {
                    self.engine.preview_edit(edit).map_err(error)?;
                } else {
                    self.layer_edit(edit)?;
                }
            }
            if grayscale {
                self.selection_masks.colors.constrain_grayscale()?;
            }
            self.refresh_document();
            Ok(())
        })())
    }
}
