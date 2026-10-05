//! One property model for temporary and stored selection masks.
use super::*;
use layer_core::{Edit, EffectValue, SelectionMaskProperties, SelectionPaintBehavior};

pub(super) fn properties(
    id: u64,
    title: &str,
    p: &SelectionMaskProperties,
    painting: SelectionPaintBehavior,
    enabled: bool,
    l: &Localizer,
) -> LayerPropertiesView {
    let defaults = SelectionMaskProperties::default();
    let controls = vec![
        PropertyControl::new(
            "mask_mode",
            &l.text(MessageId::RESOURCES_MASK_MODE),
            PropertyKind::Choice {
                options: [MessageId::RESOURCES_MASK_PAINT_SELECTION, MessageId::RESOURCES_MASK_GRAYSCALE]
                    .map(|id| l.text(id))
                    .into(),
            },
            EffectValue::Choice(u32::from(painting == SelectionPaintBehavior::BlackWhite)),
            EffectValue::Choice(0),
        ),
        PropertyControl {
            color_action: Some(UiAction::Effect {
                action: EffectAction::UseCurrentColor {
                    layer: id,
                    key: "mask_color".into(),
                },
            }),
            ..PropertyControl::new(
                "mask_color",
                &l.text(MessageId::RESOURCES_MASK_OVERLAY_COLOR),
                PropertyKind::Color {opaque:false},
                EffectValue::Color(p.color),
                EffectValue::Color(defaults.color),
            )
        },
        PropertyControl::new(
            "mask_opacity",
            &l.text(MessageId::RESOURCES_MASK_OVERLAY_OPACITY),
            PropertyKind::Number {
                numeric: NumericControl::percent(),
            },
            EffectValue::Number(p.opacity),
            EffectValue::Number(defaults.opacity),
        ),
    ];
    LayerPropertiesView {
        layer: Some(id),
        title: title.into(),
        name: title.into(),
        layer_type: l.text(MessageId::RESOURCES_LAYER_TYPE_SELECTION).to_string(),
        description: String::new(),
        enabled,
        controls,
        curve_max: None,
        curve_white: None,
        ..Default::default()
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn mask_properties(&self) -> SelectionMaskProperties {
        match self.selection_masks.target() {
            Some(layer_core::SelectionTarget::Saved(id)) => self
                .engine
                .document()
                .scene().source_target(id)
                .and_then(|target| match target { layer_core::authored::SourceTarget::Selection(h) => self.engine.document().working.selection_overlays.properties.get(&h).cloned(), _ => None })
                .unwrap_or_default(),
            _ => self.selection_masks.quick_properties.clone(),
        }
    }
    pub(super) fn grayscale_masks(&self) -> bool {
        self.state.settings.selection_painting == SelectionPaintBehavior::BlackWhite
    }
    pub(super) fn mask_paint_value(&self, eraser: bool) -> f32 {
        let grayscale = self.grayscale_masks();
        if eraser || self.selection_masks.erases() {
            if grayscale { 1. } else { 0. }
        } else if !grayscale {
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
                    .saved_selection(super::session::occurrence_handle(id)?)
                    .map_err(error)?;
                layer_core::SelectionTarget::Saved(super::session::occurrence_handle(id)?)
            };
            if self.selection_masks.target() != Some(target) {
                return Err(self.localization().text(MessageId::RESOURCES_MASK_SELECT_BEFORE_EDIT).to_string());
            }
            let mut p = self.mask_properties();
            let view = properties(id, "", &p, self.state.settings.selection_painting, true, self.localization());
            let value = super::effects::property_value(&view, key, action, self.selection_masks.colors.definition(), self.localization())?;
            match (key.as_str(), value) {
                ("mask_mode", EffectValue::Choice(v @ 0..=1)) => {
                    self.state.settings.selection_painting = if v == 0 {
                        SelectionPaintBehavior::ColorTransparency
                    } else {
                        SelectionPaintBehavior::BlackWhite
                    };
                    self.refresh_document();
                    return Ok(());
                }
                ("mask_color", EffectValue::Color(c)) => p.color = c,
                ("mask_opacity", EffectValue::Number(v)) => p.opacity = v,
                _ => return Err(self.localization().text(MessageId::RESOURCES_MASK_INVALID_PROPERTY).to_string()),
            }
            p.validate().map_err(|_| self.localization().text(MessageId::RESOURCES_MASK_INVALID_PROPERTIES).to_string())?;
            if id == 0 {
                self.selection_masks.quick_properties = p;
            } else {
                let doc = self.engine.document();
                if doc.is_locked(super::session::occurrence_handle(id)?) {
                    return Err(self.localization().text(MessageId::COMMANDS_THIS_SELECTION_LAYER_IS_LOCKED).to_string());
                }
                let occurrence = super::session::occurrence_handle(id)?;
                let Some(layer_core::authored::SourceTarget::Selection(handle))=doc.scene().source_target(occurrence) else {return Err("Choose a Selection Layer".into());};
                let mut working=doc.working.clone();
                working.selection_overlays.properties.insert(handle,p);
                let edit=Edit::Working(working);
                if self.effect_gesture.is_some() {
                    self.engine.preview_edit(edit).map_err(error)?;
                } else {
                    self.layer_edit(edit)?;
                }
            }
            self.refresh_document();
            Ok(())
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_property_localization_preserves_identity_values_and_literal_titles() {
        let en = Localizer::shared(UiLanguage::English);
        let ja = Localizer::shared(UiLanguage::Japanese);
        let properties_value = SelectionMaskProperties { opacity: 0.42, ..Default::default() };
        for title in ["Quick Mask", "ユーザーの選択 {name} 🎨", "  mask name  ", "\u{2068}literal\u{2069}"] {
            let english = properties(41, title, &properties_value, SelectionPaintBehavior::BlackWhite, false, &en);
            let japanese = properties(41, title, &properties_value, SelectionPaintBehavior::BlackWhite, false, &ja);
            assert_eq!(english.title, title);
            assert_eq!(japanese.title, title);
            assert_eq!(japanese.layer, Some(41));
            assert!(!japanese.enabled);
            assert_eq!(japanese.controls.iter().map(|control| control.key.as_str()).collect::<Vec<_>>(),
                ["mask_mode", "mask_color", "mask_opacity"]);
            for (a, b) in english.controls.iter().zip(&japanese.controls) {
                assert_eq!(a.value, b.value);
                assert_eq!(a.default, b.default);
                assert_eq!(a.modified, b.modified);
                assert_eq!(a.color_action, b.color_action);
                assert_eq!(b.section_id, None);
            }
            let mode = &japanese.controls[0];
            assert_eq!(mode.label, "モード");
            assert_eq!(mode.value, EffectValue::Choice(1));
            assert_eq!(mode.default, EffectValue::Choice(0));
            let PropertyKind::Choice { options } = &mode.kind else { panic!("mask mode choices") };
            assert_eq!(options.iter().map(|label| label.as_ref()).collect::<Vec<_>>(),
                ["選択範囲を描く", "グレースケールマスク"]);
            assert!(std::sync::Arc::ptr_eq(&options[0], &ja.text(MessageId::RESOURCES_MASK_PAINT_SELECTION)));
            assert!(std::sync::Arc::ptr_eq(&options[1], &ja.text(MessageId::RESOURCES_MASK_GRAYSCALE)));
        }
    }
}
