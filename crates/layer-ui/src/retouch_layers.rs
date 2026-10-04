//! Layer › New › New Dodge & Burn Layer, and Filter › Frequency Separation…,
//! whose dialog previews the blur of its Low layer on the canvas while the
//! document stays as it is. The dialog shows its own value, published once it
//! rests, and closes when the drawing changes. Each inserts its layers in one
//! undo step.
use super::*;
use layer_core::{RetouchLayerRefusal, SeparationFilters};
use layer_engine::ScenePreview;
use layer_core::authored::*;
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
    target: OccurrenceHandle,
    revision: u64,
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
        let plan = self.engine.document().dodge_burn_plan( self.localization().text(MessageId::RESOURCES_LAYER_DODGE_BURN)).map_err(|refusal| refusal_text(refusal, self.localization()).to_string())?;
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
        self.retouch_layer_refusal().or_else(|| doc.working.occurrence.and_then(|h| doc.separation_refusal(h)).or_else(|| doc.working.occurrence.is_none().then_some(RetouchLayerRefusal::NoLayer)).map(|refusal| refusal_text(refusal, l)))
    }

    pub fn frequency_separation_view(&self) -> Option<FrequencySeparationView> {
        self.frequency_separation.as_ref().map(|draft| draft.view.clone())
    }

    fn show_separation(&mut self) -> Result<(), String> {
        if let Some(draft) = &self.frequency_separation {
            let doc = self.engine.document();
            let mut artwork = doc.artwork.clone();
            let definition = artwork.definitions.insert(PortableId::random(), Definition {
                program: draft.filters.blur.program.clone(),
            })?;
            let effect = artwork.effects.insert(PortableId::random(), EffectApplication {
                definition, values: draft.filters.blur.values.clone(), domain: doc.composition().size,
            })?;
            let name = effects::resource_label(&draft.filters.blur.program.label, self.localization());
            let mut occurrence = Occurrence::new(OccurrenceContent::Effect(effect), name);
            occurrence.clipped = true;
            let handle = artwork.occurrences.insert(PortableId::random(), occurrence)?;
            let containing = doc.scene().stack(draft.target).ok_or("The preview layer was removed")?;
            let stack = artwork.stacks.get_mut(containing).ok_or("The preview group was removed")?;
            let index = stack.entries.iter().position(|h| *h == draft.target).ok_or("The preview layer was removed")?;
            stack.entries.insert(index, handle);
            let mut preview = Document::from_artwork(artwork).map_err(error)?;
            preview.owner = doc.owner;
            preview.revision = doc.revision;
            let context = self.engine.scene_snapshot().context.clone();
            self.engine.set_scene_preview(Some(ScenePreview { above: draft.target, scene: preview.snapshot_with_context(context) }));
        }
        self.state.layer_tools.frequency_separation = self.frequency_separation_view();
        Ok(())
    }

    pub(super) fn open_frequency_separation(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.separation_refusal())?;
        let parameter = SeparationFilters::radius_parameter(&self.effect_catalog).ok_or("Gaussian Blur is missing")?;
        let numeric = effects::number_control(parameter).ok_or("Gaussian Blur has no radius")?;
        let radius = DEFAULT_RADIUS.clamp(numeric.min as f32, numeric.max as f32);
        let filters = SeparationFilters::new(&self.effect_catalog, radius)?;
        let doc = self.engine.document();
        let (target, revision) = (doc.working.occurrence.ok_or("Select a paint layer first")?, doc.revision);
        self.frequency_separation = Some(SeparationDraft {
            target,
            revision,
            filters,
            view: FrequencySeparationView { title: "Frequency Separation", label: "Radius", radius, numeric },
            unpublished: None,
        });
        self.show_separation()?;
        Ok(())
    }

    pub(super) fn frequency_separation_action(&mut self, action: FrequencySeparationAction) -> Result<(), String> {
        match action {
            FrequencySeparationAction::Radius { radius } => {
                let draft = self.frequency_separation.as_mut().ok_or("Frequency Separation is not open")?;
                draft.view.numeric.validate(radius, draft.view.label).map_err(|reason| reason.message(&self.state.localization))?;
                if draft.view.radius != radius {
                    draft.filters = SeparationFilters::new(&self.effect_catalog, radius)?;
                    draft.view.radius = radius;
                    draft.unpublished = Some(None);
                    self.show_separation()?;
                }
                Ok(())
            }
            FrequencySeparationAction::Apply => {
                self.require_document_idle()?;
                refused(self.retouch_layer_refusal())?;
                let draft = self.close_frequency_separation().ok_or("Frequency Separation is not open")?;
                let plan = self.engine.document().separation_plan(draft.target, &draft.filters,
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
        self.engine.set_scene_preview(None);
        self.state.layer_tools.frequency_separation = None;
        Some(draft)
    }
}
