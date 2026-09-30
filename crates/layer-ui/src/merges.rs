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

fn refusal_text(refusal: MergeRefusal) -> &'static str {
    use MergeRefusal as R;
    match refusal {
        R::NoLayer => "Select a layer first",
        R::Paper => "The paper can't be merged",
        R::SelectionLayer => "A selection layer holds a selection, not artwork",
        R::Hidden => "Show the layer before merging it",
        R::NotNormal => "Set the layer to Normal before merging it down",
        R::Locked => "Unlock the layers to merge first",
        R::NoLayerBelow => "There's no layer below to merge into",
        R::PaperBelow => "The paper can't receive merged pixels",
        R::BelowHidden => "Show the layer below first",
        R::BelowLocked => "The layer below is locked",
        R::BelowNotNormal => "Set the layer below to Normal before merging",
        R::BelowEffect => "The layer below is an effect layer, with no pixels to merge into",
        R::BelowClipped => "The layer below is clipped; merge its clipped layers first",
        R::BaseHidden => "Show the clipping base first",
        R::BaseNotNormal => "Set the clipping base to Normal before merging",
        R::ClipsHidden => "The clipped layers are hidden",
        R::NotGroup => "Select a group to merge",
        R::NothingVisible => "No visible layers to merge",
        R::SelectionLayersInside => "Move the selection layers out of the group first",
        R::TooLarge => "The merged layer would exceed the 1 GiB limit for one edit",
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn merge_down_label(&self) -> &'static str {
        match self.engine.document().merge_down() {
            MergeDown::Layer => CommandId::MergeDown.label(),
            MergeDown::ClippingStack => "Merge Clipped Layers",
            MergeDown::ApplyEffect => "Apply Effect to Layer Below",
        }
    }

    /// Why a merge can't run once the document is idle. The size limit is
    /// checked when it runs.
    pub(super) fn merge_refusal(&self, kind: MergeKind) -> Option<&'static str> {
        if self.selection_masks.target().is_some() {
            return Some("Return to the artwork first");
        }
        if self.operation.active() {
            return Some(self.operation_refusal());
        }
        self.engine.document().merge_refusal(kind).map(refusal_text)
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
        let plan = self.engine.document().merge_plan(kind, result, coverage).map_err(refusal_text)?;
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
                ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label, UiAction::Invoke { command }) }
            })
            .collect()
    }
}
