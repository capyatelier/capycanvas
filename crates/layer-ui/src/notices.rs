//! Transient notices: refusals and hints raised by canvas gestures. The core
//! authors the text and keeps any action; hosts show each new id once, hide it
//! after a short timeout or at the next canvas contact, and answer with
//! `UiAction::Notice`.
use super::*;
use layer_core::{DrawingRefusal, RetouchSource};
use layer_engine::StrokeRefusal;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Notice {
    /// Increases with every notice, so a repeated refusal is shown again.
    pub id: u64,
    pub text: String,
    pub action: Option<NoticeAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoticeAction {
    pub label: String,
}

#[derive(Default)]
pub(super) struct Notices {
    last_id: u64,
    action: Option<UiAction>,
    epoch: u64,
    changed: bool,
    /// The mask whose editing session already explained dry coverage.
    dry_mask: Option<LayerId>,
}

impl Notices {
    pub(super) fn publishing(&self) -> bool {
        self.changed
    }
}

pub(super) fn drawing_refusal_text(refusal: DrawingRefusal, l: &Localizer) -> std::sync::Arc<str> {
    match refusal {
        DrawingRefusal::NoLayer => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_SELECT_A_LAYER_TO_DRAW_ON),
        DrawingRefusal::Locked => l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED),
        DrawingRefusal::BaseLocked => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THE_LAYER_BELOW_THIS_EFFECT_IS_LOCKED),
        DrawingRefusal::Group => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_A_GROUP_HAS_NO_PIXELS_OF_ITS_OWN_SELECT_A_LAYER_INSIDE_IT),
        DrawingRefusal::Paper => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THE_PAPER_CAN_T_BE_DRAWN_ON_ADD_A_LAYER_ABOVE_IT),
        DrawingRefusal::SelectionLayer => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_A_SELECTION_LAYER_HOLDS_A_SELECTION_NOT_PAINT),
        DrawingRefusal::EffectWithoutBase => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_EFFECT_LAYER_HAS_NO_LAYER_BELOW_IT_TO_DRAW_ON),
        DrawingRefusal::Mask => l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST),
        DrawingRefusal::EffectMask => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_TOOL_DRAWS_ON_ARTWORK_NOT_ON_AN_EFFECT_LAYER_S_MASK),
    }
}

fn stroke_refusal_text(refusal: StrokeRefusal, l: &Localizer) -> std::sync::Arc<str> {
    match refusal {
        StrokeRefusal::Target(refusal) => drawing_refusal_text(refusal, l),
        StrokeRefusal::AlphaLocked => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_ALPHA_LOCK_KEEPS_THIS_LAYER_S_TRANSPARENCY_SO_ERASING_HAS_NO_EFFECT),
        StrokeRefusal::DryMask => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_MASKS_TAKE_DRY_COVERAGE_SO_THIS_BRUSH_PAINTS_WITHOUT_ITS_WET_OR_BLENDING_BEHAVIOR),
        StrokeRefusal::EmptySource(RetouchSource::Editing) => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_LAYER_IS_EMPTY_SO_THERE_S_NOTHING_TO_COPY),
        StrokeRefusal::EmptySource(RetouchSource::References) => {
            l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_LAYER_IS_EMPTY_AND_THERE_S_NO_LAYER_BELOW_IT_TO_COPY_FROM)
        }
        StrokeRefusal::NoCloneSource => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_CHOOSE_WHERE_TO_COPY_FROM_FIRST),
        StrokeRefusal::TransformedLayer => {
            l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_LAYER_IS_SCALED_OR_ROTATED_SO_IT_CAN_T_BE_RETOUCHED_DIRECTLY_RETOUCH_ON_A_NEW_LAYER_ABOVE_IT)
        }
    }
}

pub(super) const NO_REFERENCE_BELOW: MessageId = MessageId::COMMANDS_REFUSAL_NOTICES_NO_VISIBLE_PHOTO_OR_PAINT_LAYER_BELOW;

impl<R: CanvasRenderer> UiSession<R> {
    pub fn notify(&mut self, text: impl Into<String>) {
        self.raise_notice(text.into(), None);
    }

    pub(super) fn raise_notice(&mut self, text: String, action: Option<(String, UiAction)>) {
        let (label, action) = action.unzip();
        let notices = &mut self.notices;
        notices.last_id += 1;
        notices.epoch = self.state.document_file.epoch;
        notices.changed = true;
        notices.action = action;
        self.state.notice = Some(Notice {
            id: notices.last_id,
            text,
            action: label.map(|label| NoticeAction { label }),
        });
    }

    pub(super) fn dismiss_notice(&mut self) {
        if self.state.notice.take().is_some() {
            self.notices.action = None;
            self.notices.changed = true;
        }
    }

    /// Identifies the current notice so a contact can clear it unless the
    /// contact raised a new one.
    pub(super) fn notice_id(&self) -> Option<u64> {
        self.state.notice.as_ref().map(|n| n.id)
    }

    pub(super) fn dismiss_notice_unless_raised(&mut self, before: Option<u64>) {
        if before.is_some() && self.notice_id() == before {
            self.dismiss_notice();
        }
    }

    /// HOST when the notice changed since the last publication. A notice
    /// never outlives the document activation that raised it.
    pub(super) fn notice_regions(&mut self) -> u32 {
        if self.notices.epoch != self.state.document_file.epoch {
            self.dismiss_notice();
            self.notices.epoch = self.state.document_file.epoch;
        }
        if std::mem::take(&mut self.notices.changed) { regions::HOST } else { 0 }
    }

    /// Keep notice ids increasing across the documents of one window.
    pub(super) fn inherit_notice_ids(&mut self, previous: &Self) {
        self.notices.last_id = self.notices.last_id.max(previous.notices.last_id);
    }

    pub(super) fn notice_action(&mut self, id: u64, accept: bool) -> Result<UiChange, String> {
        if self.notice_id() != Some(id) {
            return Err("This notice was already dismissed".into());
        }
        let action = self.notices.action.take().filter(|_| accept);
        self.dismiss_notice();
        match action {
            Some(action) => self.dispatch(action),
            None => Ok(self.changed(0, false)),
        }
    }

    /// At pen-down, explain a brush stroke that will not paint, or will paint
    /// differently than configured.
    pub(super) fn notify_stroke_refusal(&mut self, event: &PenEvent) {
        let refusal = self.engine.stroke_refusal(event);
        if refusal == Some(StrokeRefusal::DryMask) {
            let mask = self.engine.document().drawing_target();
            if self.notices.dry_mask == mask {
                return;
            }
            self.notices.dry_mask = mask;
        }
        match refusal {
            Some(StrokeRefusal::EmptySource(RetouchSource::References)) if self.reference_below().is_some() => self
                .offer_reference_below("This layer is empty, and no reference layer below it is marked"),
            Some(StrokeRefusal::NoCloneSource) => self.raise_notice(
                stroke_refusal_text(StrokeRefusal::NoCloneSource, self.localization()).to_string(),
                Some((CommandId::CloneSourceArm.localized_label(self.localization()).to_string(), UiAction::Invoke { command: CommandId::CloneSourceArm })),
            ),
            Some(refusal) => self.notify(stroke_refusal_text(refusal, self.localization()).to_string()),
            None => {}
        }
    }

    /// A mask-editing session ends when the brush target is no longer its mask.
    pub(super) fn end_dry_mask_session(&mut self, drawing_target: Option<LayerId>) {
        if self.notices.dry_mask != drawing_target {
            self.notices.dry_mask = None;
        }
    }

    /// Explain why a content tool (Move, Lasso Fill, Gradient, Figure, Fill)
    /// has nothing to act on.
    pub(super) fn notify_drawing_refusal(&mut self) {
        if let Some(refusal) = self.engine.document().drawing_refusal() {
            self.notify(drawing_refusal_text(refusal, self.localization()).to_string());
        }
    }

    /// The nearest visible Paint layer, photo or painting, below the active
    /// layer.
    pub(super) fn reference_below(&self) -> Option<&layer_core::Layer> {
        let doc = self.engine.document();
        let index = doc.layers.iter().position(|l| l.id == doc.active_layer)?;
        doc.layers[index + 1..]
            .iter()
            .find(|l| l.kind == LayerKind::Paint && doc.layer_is_visible(l.id))
    }

    pub(super) fn use_reference_below_reason(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        match self.reference_below() {
            None => Some(l.text(MessageId::COMMANDS_REFUSAL_NOTICES_NO_VISIBLE_PHOTO_OR_PAINT_LAYER_BELOW)),
            Some(layer) if self.engine.document().reference_layers.contains(&layer.id) => {
                Some(l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THE_LAYER_BELOW_IS_ALREADY_A_REFERENCE))
            }
            Some(_) => None,
        }
    }

    pub(super) fn use_reference_below(&mut self) -> Result<(), String> {
        refused(self.use_reference_below_reason())?;
        let id = self.reference_below().ok_or_else(|| self.localization().text(NO_REFERENCE_BELOW).to_string())?.id;
        let mut references = self.engine.document().reference_layers.clone();
        references.insert(id);
        self.set_references(references)
    }

    /// Wand and Fill sampling reference layers when none is marked.
    pub(super) fn notify_missing_reference(&mut self) {
        if self.reference_below().is_some() {
            self.offer_reference_below("This tool samples reference layers, and none is marked");
        } else {
            self.notify("This tool samples reference layers. Mark one in the Layers panel first.");
        }
    }

    /// Explain `text` and offer to mark the nearest layer below as a
    /// reference, in one undo step.
    fn offer_reference_below(&mut self, text: &str) {
        if let Some(layer) = self.reference_below() {
            let label = format!("Use {} as Reference", layer.name);
            self.raise_notice(text.into(), Some((label, UiAction::Invoke { command: CommandId::UseReferenceBelow })));
        }
    }
}
