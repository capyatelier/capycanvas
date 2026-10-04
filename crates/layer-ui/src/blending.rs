//! The document's Blending, how its layers combine: Edit ▸ Blending.
use super::*;
use layer_core::{BlendSpace, Edit};

pub(super) fn blend_space(command: CommandId) -> Option<BlendSpace> {
    match command {
        CommandId::BlendPerceptual => Some(BlendSpace::Perceptual),
        CommandId::BlendLinear => Some(BlendSpace::Linear),
        _ => None,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn blending_refusal(&self) -> Option<std::sync::Arc<str>> {
        BlendSpace::unavailable_reason(self.engine.document().composition().color.depth).map(|_| self.localization().text(MessageId::COMMANDS_REFUSAL_BLENDING_FLOAT_DOCUMENTS_BLEND_IN_LINEAR_LIGHT))
    }
    pub(super) fn set_blend_space(&mut self, space: BlendSpace) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.blending_refusal())?;
        if self.engine.document().composition().blend != space {
            let doc = self.engine.document();
            let mut composition = doc.composition().clone();
            composition.blend = space;
            self.layer_edit(Edit::Composition(layer_core::authored::RecordChange::replace(&doc.artwork.compositions, doc.artwork.root, Some(composition))?))?;
        }
        Ok(())
    }
}
