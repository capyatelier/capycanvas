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

pub(super) fn drawing_refusal_text(refusal: DrawingRefusal) -> &'static str {
    match refusal {
        DrawingRefusal::NoLayer => "Select a layer to draw on",
        DrawingRefusal::Locked => "The active layer is locked",
        DrawingRefusal::BaseLocked => "The layer below this effect is locked",
        DrawingRefusal::Group => "A group has no pixels of its own. Select a layer inside it.",
        DrawingRefusal::Paper => "The paper can't be drawn on. Add a layer above it.",
        DrawingRefusal::SelectionLayer => "A selection layer holds a selection, not paint",
        DrawingRefusal::EffectWithoutBase => "This effect layer has no layer below it to draw on",
        DrawingRefusal::Mask => "Return to the layer's artwork first",
        DrawingRefusal::EffectMask => "This tool draws on artwork, not on an effect layer's mask",
    }
}

fn stroke_refusal_text(refusal: StrokeRefusal) -> &'static str {
    match refusal {
        StrokeRefusal::Target(refusal) => drawing_refusal_text(refusal),
        StrokeRefusal::AlphaLocked => "Alpha lock keeps this layer's transparency, so erasing has no effect",
        StrokeRefusal::DryMask => "Masks take dry coverage, so this brush paints without its wet or blending behavior",
        StrokeRefusal::EmptySource(RetouchSource::Editing) => "This layer is empty, so there's nothing to copy",
        StrokeRefusal::EmptySource(RetouchSource::References) => {
            "This layer is empty, and there's no layer below it to copy from"
        }
    }
}

pub(super) const NO_REFERENCE_BELOW: &str = "No visible photo or paint layer below";

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
            Some(refusal) => self.notify(stroke_refusal_text(refusal)),
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
            self.notify(drawing_refusal_text(refusal));
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

    pub(super) fn use_reference_below_reason(&self) -> Option<&'static str> {
        match self.reference_below() {
            None => Some(NO_REFERENCE_BELOW),
            Some(layer) if self.engine.document().reference_layers.contains(&layer.id) => {
                Some("The layer below is already a reference")
            }
            Some(_) => None,
        }
    }

    pub(super) fn use_reference_below(&mut self) -> Result<(), String> {
        if let Some(reason) = self.use_reference_below_reason() {
            return Err(reason.into());
        }
        let id = self.reference_below().ok_or(NO_REFERENCE_BELOW)?.id;
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
