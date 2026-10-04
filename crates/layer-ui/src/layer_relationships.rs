use crate::{ContextMenuItem, LayerAction, LayerState, Localizer, MessageId, UiAction};
use crate::session::occurrence_token;
use fluent_bundle::FluentArgs;
use layer_core::{Attachment, Document, EffectKind, LayerBlend, LayerKind};
use layer_core::authored::OccurrenceHandle;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerRelationKind { Clip, Effect }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LayerRelation {
    pub kind: LayerRelationKind,
    pub target: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LayerConnection {
    pub kind: LayerRelationKind,
    pub from: u64,
    pub to: u64,
    pub depth: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerAttachmentControl {
    pub icon: &'static str,
    pub label: String,
    pub description: String,
    pub checked: bool,
    pub action: Option<LayerAction>,
}

impl Default for LayerAttachmentControl {
    fn default() -> Self {
        Self { icon: "layer-clip-symbolic", label: String::new(), description: String::new(), checked: false, action: None }
    }
}

impl LayerAttachmentControl {
    pub(super) fn menu_item(self) -> ContextMenuItem {
        let icon = self.icon.strip_prefix("layer-").unwrap_or(self.icon);
        let icon = icon.strip_suffix("-symbolic").unwrap_or(icon);
        ContextMenuItem {
            icon: Some(icon), label: self.label, selected: Some(self.checked),
            enabled: self.action.is_some(), action: self.action.map(|action| UiAction::Layer { action }),
            hint: self.description, bindings: Vec::new(), sections: Vec::new(),
        }
    }
}

pub(super) fn relation(doc: &Document, id: OccurrenceHandle) -> Option<LayerRelation> {
    let scene = doc.scene();
    scene.effect_owner(id).map(|owner| LayerRelation { kind: LayerRelationKind::Effect, target: occurrence_token(owner) })
        .or_else(|| scene.clipping_base(id).map(|base| LayerRelation { kind: LayerRelationKind::Clip, target: occurrence_token(base) }))
}

pub(super) fn right_swipe(doc: &Document, id: OccurrenceHandle) -> Option<LayerAction> {
    if doc.is_locked(id) { return None; }
    let row = doc.scene().occurrence(id)?;
    match row.kind() {
        LayerKind::Paint => Some(LayerAction::ToggleAlphaLock { id: occurrence_token(id) }),
        LayerKind::Group => {
            let next = if row.passes_through() { LayerBlend::Normal } else { LayerBlend::PassThrough };
            doc.group_blend_refusal(id, next).is_none().then(|| LayerAction::TogglePassThrough { id: occurrence_token(id) })
        }
        _ => None,
    }
}

pub(super) fn group_mode_menu(doc: &Document, id: OccurrenceHandle, l: &Localizer) -> Option<ContextMenuItem> {
    let row = doc.scene().occurrence(id)?;
    if row.kind() != LayerKind::Group { return None; }
    let action = right_swipe(doc, id);
    Some(ContextMenuItem {
        icon: Some("group-pass-through"),
        label: l.text(MessageId::RESOURCES_BLEND_PASS_THROUGH).to_string(),
        selected: Some(row.passes_through()), enabled: action.is_some(),
        action: action.map(|action| UiAction::Layer { action }),
        hint: String::new(), bindings: Vec::new(), sections: Vec::new(),
    })
}

pub(super) fn attachment_control(doc: &Document, id: Option<OccurrenceHandle>, l: &Localizer) -> LayerAttachmentControl {
    let scene = doc.scene();
    let unavailable = || {
        let label = l.text(MessageId::RESOURCES_LAYER_ATTACH_UNAVAILABLE).to_string();
        LayerAttachmentControl { description: label, label: l.text(MessageId::RESOURCES_LAYER_MENU_CLIP_TO_LAYER_BELOW).to_string(), ..Default::default() }
    };
    let Some((id, row)) = id.and_then(|id| scene.occurrence(id).map(|row| (id, row))) else { return unavailable(); };
    if row.kind() == LayerKind::Selection { return unavailable(); }
    let effect = scene.effect(id).is_some_and(|e| e.program.kind == EffectKind::Adjustment);
    let checked = row.attachment != Attachment::None;
    let target = scene.attachment_target(id).or_else(|| doc.attachment_candidate(id));
    let pass_through_group = if checked { None }
        else if row.passes_through() { Some(row) }
        else { target.and_then(|id| scene.occurrence(id).filter(|row| row.passes_through())) };
    let mut args = FluentArgs::new();
    if let Some(target) = target { args.set("target", scene.occurrence(target).unwrap().name.as_ref()); }
    let label = if !effect { l.text(MessageId::RESOURCES_LAYER_MENU_CLIP_TO_LAYER_BELOW).to_string() }
        else { l.format(if target.is_none() { MessageId::RESOURCES_LAYER_ATTACH_UNAVAILABLE }
            else if checked { MessageId::RESOURCES_LAYER_ATTACH_STACK } else { MessageId::RESOURCES_LAYER_ATTACH_EFFECT }, &args) };
    let description = if target.is_none() { l.text(MessageId::RESOURCES_LAYER_ATTACH_UNAVAILABLE).to_string() }
        else if let Some(group) = pass_through_group {
            args.set("group", group.name.as_ref());
            l.format(MessageId::RESOURCES_LAYER_ATTACH_PASS_THROUGH, &args)
        } else { l.format(match (checked, effect) {
            (true, true) => MessageId::RESOURCES_LAYER_EFFECT_OWNER,
            (true, false) => MessageId::RESOURCES_LAYER_CLIPPED_TO,
            (false, true) => MessageId::RESOURCES_LAYER_ATTACH_EFFECT,
            (false, false) => MessageId::RESOURCES_LAYER_ATTACH_CLIP,
        }, &args) };
    let action = (!doc.is_locked(id) && pass_through_group.is_none() && (checked || target.is_some()))
        .then(|| LayerAction::Clip { id: occurrence_token(id), value: !checked });
    LayerAttachmentControl {
        icon: if effect { "layer-effect-link-symbolic" } else { "layer-clip-symbolic" },
        label, description, checked, action,
    }
}

pub(super) fn connections(doc: &Document, rows: &[LayerState]) -> Vec<LayerConnection> {
    let scene = doc.scene();
    let visible: BTreeMap<_, _> = rows.iter().enumerate().map(|(index, row)| (row.id, index)).collect();
    let mut clips = BTreeMap::new();
    let mut connections = Vec::new();
    for row in rows {
        let Some(relation) = row.relationship else { continue; };
        if !visible.contains_key(&relation.target) { continue; }
        match relation.kind {
            LayerRelationKind::Clip => {
                let Ok(id) = crate::session::occurrence_handle(row.id) else { continue; };
                let from = scene.attached_effects(id).iter().rev().copied().map(occurrence_token)
                    .find(|effect| visible.contains_key(effect)).unwrap_or(row.id);
                clips.entry(relation.target).or_insert((from, row.depth));
            }
            LayerRelationKind::Effect => {
                let Ok(id) = crate::session::occurrence_handle(row.id) else { continue; };
                let owner = scene.effect_owner(id).unwrap();
                let chain = scene.attached_effects(owner);
                let at = chain.iter().position(|h| *h == id).unwrap();
                let to = occurrence_token(if at == 0 { owner } else { chain[at - 1] });
                if visible.get(&to).is_some_and(|index| *index == visible[&row.id] + 1) {
                    connections.push(LayerConnection { kind: LayerRelationKind::Effect, from: row.id, to, depth: row.depth });
                }
            }
        }
    }
    connections.extend(clips.into_iter().map(|(to, (from, depth))| LayerConnection { kind: LayerRelationKind::Clip, from, to, depth }));
    connections
}
