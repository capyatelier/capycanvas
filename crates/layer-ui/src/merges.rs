//! Merge Down (Merge Clipped Layers, Apply Effect to Layer Below), Merge
//! Group, Merge Visible, Flatten Image and Stamp Visible. Each bakes into one
//! new layer in one undo step; placed photos become document pixels.
use super::*;
use layer_core::{MergeDown, MergeKind, MergeRefusal};
use std::collections::BTreeSet;

pub(super) fn merge_kind(command: CommandId) -> Option<MergeKind> {
    Some(match command {
        CommandId::MergeDown => MergeKind::Down,
        CommandId::MergeGroup => MergeKind::Group,
        CommandId::MergeVisible => MergeKind::Visible,
        CommandId::FlattenImage => MergeKind::Flatten,
        CommandId::StampVisible => MergeKind::Stamp,
        _ => return None,
    })
}

fn refusal_text(refusal: MergeRefusal, l: &Localizer) -> std::sync::Arc<str> {
    use MergeRefusal as R;
    match refusal {
        R::NoLayer => l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST),
        R::Paper => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_PAPER_CAN_T_BE_MERGED),
        R::SelectionLayer => l.text(MessageId::COMMANDS_REFUSAL_MERGES_A_SELECTION_LAYER_HOLDS_A_SELECTION_NOT_ARTWORK),
        R::Hidden => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SHOW_THE_LAYER_BEFORE_MERGING_IT),
        R::NotNormal => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SET_THE_LAYER_TO_NORMAL_BEFORE_MERGING_IT_DOWN),
        R::Locked => l.text(MessageId::COMMANDS_REFUSAL_MERGES_UNLOCK_THE_LAYERS_TO_MERGE_FIRST),
        R::NoLayerBelow => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THERE_S_NO_LAYER_BELOW_TO_MERGE_INTO),
        R::PaperBelow => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_PAPER_CAN_T_RECEIVE_MERGED_PIXELS),
        R::BelowHidden => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SHOW_THE_LAYER_BELOW_FIRST),
        R::BelowLocked => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_LAYER_BELOW_IS_LOCKED),
        R::BelowNotNormal => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SET_THE_LAYER_BELOW_TO_NORMAL_BEFORE_MERGING),
        R::BelowEffect => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_LAYER_BELOW_IS_AN_EFFECT_LAYER_WITH_NO_PIXELS_TO_MERGE_INTO),
        R::BelowClipped => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_LAYER_BELOW_IS_CLIPPED_MERGE_ITS_CLIPPED_LAYERS_FIRST),
        R::BaseHidden => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SHOW_THE_CLIPPING_BASE_FIRST),
        R::BaseNotNormal => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SET_THE_CLIPPING_BASE_TO_NORMAL_BEFORE_MERGING),
        R::ClipsHidden => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_CLIPPED_LAYERS_ARE_HIDDEN),
        R::NotGroup => l.text(MessageId::COMMANDS_REFUSAL_MERGES_SELECT_A_GROUP_TO_MERGE),
        R::NothingVisible => l.text(MessageId::COMMANDS_REFUSAL_MERGES_NO_VISIBLE_LAYERS_TO_MERGE),
        R::SelectionLayersInside => l.text(MessageId::COMMANDS_REFUSAL_MERGES_MOVE_THE_SELECTION_LAYERS_OUT_OF_THE_GROUP_FIRST),
        R::TooLarge => l.text(MessageId::COMMANDS_REFUSAL_MERGES_THE_MERGED_LAYER_WOULD_EXCEED_THE_1_GIB_LIMIT_FOR_ONE_EDIT),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn merge_down_label(&self) -> std::sync::Arc<str> {
        match self.engine.document().merge_down() {
            MergeDown::Layer => CommandId::MergeDown.localized_label(self.localization()),
            MergeDown::ClippingStack => self.localization().text(MessageId::COMMAND_MERGE_CLIPPED_LAYERS),
            MergeDown::ApplyEffect => self.localization().text(MessageId::COMMAND_APPLY_EFFECT_TO_LAYER_BELOW),
        }
    }

    /// Why a merge can't run once the document is idle. The size limit is
    /// checked when it runs.
    pub(super) fn merge_refusal(&self, kind: MergeKind) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        if self.operation.active() {
            return Some(self.operation_refusal());
        }
        self.engine.document().merge_refusal(kind).map(|refusal| refusal_text(refusal, l))
    }

    /// Flatten Image asks, through the notice, before it discards hidden layers.
    pub(super) fn merge(&mut self, kind: MergeKind) -> Result<(), String> {
        refused(self.merge_refusal(kind))?;
        let hidden = if kind == MergeKind::Flatten { self.engine.document().flatten_discards() } else { 0 };
        if hidden == 0 {
            return self.bake(kind);
        }
        let text = match hidden {
            1 => "Flattening discards 1 hidden layer".into(),
            n => format!("Flattening discards {n} hidden layers"),
        };
        self.raise_notice(text, Some(("Flatten".into(), UiAction::Layer { action: LayerAction::Flatten })));
        Ok(())
    }

    pub(super) fn bake(&mut self, kind: MergeKind) -> Result<(), String> {
        refused(self.merge_refusal(kind))?;
        let result = self.engine.allocate_layer_id();
        let coverage = self.engine.allocate_layer_id();
        let plan = self.engine.document().merge_plan(kind, result, coverage).map_err(|refusal| refusal_text(refusal, self.localization()).to_string())?;
        self.engine
            .insert_with_operations(plan.edits, vec![(result, plan.operation)], None)
            .map_err(error)?;
        self.layer_interaction.editing = Some(result);
        self.layer_interaction.selected = BTreeSet::from([result]);
        self.layer_interaction.changed = true;
        Ok(())
    }

    /// Merge commands for a layer's menu: Merge Down, or Merge Group for a
    /// group, on the active layer, then the whole-image merges.
    pub(super) fn merge_menu_items(&self, layer: &layer_core::Layer) -> Vec<ContextMenuItem> {
        let active = [if layer.kind == LayerKind::Group { CommandId::MergeGroup } else { CommandId::MergeDown }];
        let own = layer.id == self.engine.document().active_layer && layer.kind != LayerKind::Background;
        let active = if own { &active[..] } else { &[] };
        active
            .iter()
            .chain(&[CommandId::MergeVisible, CommandId::StampVisible, CommandId::FlattenImage])
            .map(|&command| {
                let state = self.command(command);
                ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label.to_string(), UiAction::Invoke { command }) }
            })
            .collect()
    }
}
