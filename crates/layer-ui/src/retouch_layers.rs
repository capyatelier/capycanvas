//! Layer › New › New Dodge & Burn Layer, and Filter › Frequency Separation…,
//! whose dialog previews the blur of its Low layer on the canvas while the
//! document stays as it is. The dialog shows its own value, published once it
//! rests, and closes when the drawing changes. Each inserts its layers in one
//! undo step.
use super::*;
use layer_core::{RetouchLayerRefusal, SeparationFilters};
use layer_engine::LayerPreview;
use std::collections::BTreeSet;

/// The radius Frequency Separation opens with.
const DEFAULT_RADIUS: f32 = 4.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FrequencySeparationAction {
    Radius { radius: f32 },
    Apply,
    Cancel,
}

/// The open Frequency Separation dialog: one radius, previewed on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FrequencySeparationView {
    pub title: &'static str,
    pub label: &'static str,
    pub radius: f32,
    pub numeric: NumericControl,
}

pub(super) struct SeparationDraft {
    target: LayerId,
    revision: u64,
    preview: LayerId,
    filters: SeparationFilters,
    view: FrequencySeparationView,
    /// A value not yet published, and when it last changed; None until the
    /// next frame.
    unpublished: Option<Option<u64>>,
}
impl SeparationDraft {
    pub(super) fn unpublished(&self) -> bool {
        self.unpublished.is_some()
    }
}

fn refusal_text(refusal: RetouchLayerRefusal, l: &Localizer) -> std::sync::Arc<str> {
    use RetouchLayerRefusal as R;
    match refusal {
        R::NoLayer => l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST),
        R::NotPaint => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_SELECT_A_PAINT_LAYER_FIRST),
        R::Hidden => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_SHOW_THE_LAYER_FIRST),
        R::NotNormal => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_SET_THE_LAYER_TO_NORMAL_FIRST),
        R::Linear => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_FREQUENCY_SEPARATION_NEEDS_PERCEPTUAL_BLENDING_CHANGE_IT_IN_EDIT_BLENDING),
        R::GroupLocked => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_THE_DESTINATION_GROUP_IS_LOCKED),
        R::TooLarge => l.text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_THE_SEPARATED_LAYERS_WOULD_EXCEED_THE_1_GIB_LIMIT_FOR_ONE_EDIT),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    fn retouch_layer_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.selection_masks.target().is_some() {
            Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST))
        } else if self.operation.active() {
            Some(self.operation_refusal())
        } else {
            None
        }
    }

    pub(super) fn dodge_burn_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        self.retouch_layer_refusal().or_else(|| self.engine.document().dodge_burn_refusal().map(|refusal| refusal_text(refusal, l)))
    }

    pub(super) fn new_dodge_burn_layer(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.dodge_burn_refusal())?;
        let ids = std::array::from_fn(|_| self.engine.allocate_layer_id());
        let plan = self.engine.document().dodge_burn_plan(ids, self.localization().text(MessageId::RESOURCES_LAYER_DODGE_BURN)).map_err(|refusal| refusal_text(refusal, self.localization()).to_string())?;
        self.insert_retouch_layers(plan)
    }

    fn insert_retouch_layers(&mut self, plan: layer_core::RetouchLayerPlan) -> Result<(), String> {
        self.engine.insert_with_operations(plan.edits, plan.operations, None).map_err(error)?;
        self.layer_interaction.editing = Some(plan.active);
        self.layer_interaction.selected = BTreeSet::from([plan.active]);
        self.layer_interaction.changed = true;
        Ok(())
    }

    pub(super) fn separation_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let doc = self.engine.document();
        self.retouch_layer_refusal().or_else(|| doc.separation_refusal(doc.active_layer).map(|refusal| refusal_text(refusal, l)))
    }

    pub fn frequency_separation_view(&self) -> Option<FrequencySeparationView> {
        self.frequency_separation.as_ref().map(|draft| draft.view.clone())
    }

    fn show_separation(&mut self) {
        if let Some(draft) = &self.frequency_separation {
            let name = effects::resource_label(&draft.filters.blur.program.label, self.localization());
            let layer = SeparationFilters::clipped(draft.preview, &draft.filters.blur, name);
            self.engine.set_layer_preview(Some(LayerPreview { above: draft.target, layer }));
        }
        self.state.layer_tools.frequency_separation = self.frequency_separation_view();
    }

    pub(super) fn open_frequency_separation(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.separation_refusal())?;
        let parameter = SeparationFilters::radius_parameter(&self.effect_catalog).ok_or("Gaussian Blur is missing")?;
        let numeric = effects::number_control(parameter).ok_or("Gaussian Blur has no radius")?;
        let radius = DEFAULT_RADIUS.clamp(numeric.min as f32, numeric.max as f32);
        let filters = SeparationFilters::new(&self.effect_catalog, radius)?;
        let doc = self.engine.document();
        let (target, revision) = (doc.active_layer, doc.revision);
        self.frequency_separation = Some(SeparationDraft {
            target,
            revision,
            preview: self.engine.allocate_layer_id(),
            filters,
            view: FrequencySeparationView { title: "Frequency Separation", label: "Radius", radius, numeric },
            unpublished: None,
        });
        self.show_separation();
        Ok(())
    }

    pub(super) fn frequency_separation_action(&mut self, action: FrequencySeparationAction) -> Result<(), String> {
        match action {
            FrequencySeparationAction::Radius { radius } => {
                let draft = self.frequency_separation.as_mut().ok_or("Frequency Separation is not open")?;
                draft.view.numeric.validate(radius, &*draft.view.label).map_err(|reason| reason.message(&self.state.localization))?;
                if draft.view.radius != radius {
                    draft.filters = SeparationFilters::new(&self.effect_catalog, radius)?;
                    draft.view.radius = radius;
                    draft.unpublished = Some(None);
                    self.show_separation();
                }
                Ok(())
            }
            FrequencySeparationAction::Apply => {
                self.require_document_idle()?;
                refused(self.retouch_layer_refusal())?;
                let draft = self.close_frequency_separation().ok_or("Frequency Separation is not open")?;
                let ids = std::array::from_fn(|_| self.engine.allocate_layer_id());
                let plan = self.engine.document().separation_plan(draft.target, &draft.filters, ids,
                    [MessageId::RESOURCES_LAYER_FREQUENCY_SEPARATION, MessageId::RESOURCES_LAYER_LOW, MessageId::RESOURCES_LAYER_HIGH]
                        .map(|id| self.localization().text(id))).map_err(|refusal| refusal_text(refusal, self.localization()).to_string())?;
                self.insert_retouch_layers(plan)
            }
            FrequencySeparationAction::Cancel => {
                self.close_frequency_separation();
                Ok(())
            }
        }
    }

    /// Regions to publish: the rested value, or the dialog closed because the
    /// drawing changed under it.
    pub(super) fn advance_frequency_separation(&mut self, now_ns: u64) -> u32 {
        let doc = self.engine.document();
        let Some(draft) = self.frequency_separation.as_mut() else {
            return 0;
        };
        if draft.revision != doc.revision {
            self.close_frequency_separation();
            self.notify("The drawing changed, so Frequency Separation was closed");
            return regions::DOCUMENT | regions::BRUSH | regions::COMMANDS;
        }
        let Some(changed) = &mut draft.unpublished else {
            return 0;
        };
        if now_ns.saturating_sub(*changed.get_or_insert(now_ns)) < super::selection_refine::SETTLE_NS {
            return 0;
        }
        draft.unpublished = None;
        regions::BRUSH
    }

    fn close_frequency_separation(&mut self) -> Option<SeparationDraft> {
        let draft = self.frequency_separation.take()?;
        self.engine.set_layer_preview(None);
        self.state.layer_tools.frequency_separation = None;
        Some(draft)
    }
}
