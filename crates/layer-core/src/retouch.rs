//! What a retouching stroke samples. The stroke fixes its source at pen-down so
//! replays and late corrections sample the same layers.
use crate::{Document, LayerId};
use std::{collections::BTreeSet, sync::Arc};

/// The pixels a retouching tool copies from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RetouchSource {
    /// The reference layers below the editing layer, with the editing layer
    /// over them, as the stroke found them.
    #[default]
    References,
    /// The editing layer alone, as the stroke found it.
    Editing,
}

/// A retouching stroke's source, fixed at pen-down.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Retouch {
    pub source: RetouchSource,
    /// Layers composing the reference pixels below the editing layer. Empty
    /// when the source is the editing layer alone.
    pub references: Arc<BTreeSet<LayerId>>,
}

impl Retouch {
    /// The source `source` samples for a stroke on `target`. With no reference
    /// below the target, the target alone is the source.
    pub fn for_target(document: &Document, target: LayerId, source: RetouchSource) -> Self {
        let references = match source {
            RetouchSource::References => document.references_below(target),
            RetouchSource::Editing => BTreeSet::new(),
        };
        Self { source, references: Arc::new(references) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EffectKind, Layer, LayerKind};

    fn document() -> Document {
        let mut doc = Document::new("retouch", 64, 64);
        let mut group = Layer::paint(LayerId(10), "Group");
        group.kind = LayerKind::Group;
        let mut above = Layer::paint(LayerId(11), "Above");
        above.properties.parent = Some(group.id);
        let mut target = Layer::paint(LayerId(12), "Target");
        target.properties.parent = Some(group.id);
        let mut below = Layer::paint(LayerId(13), "Below");
        below.properties.parent = Some(group.id);
        let photo = Layer::paint(LayerId(14), "Photo");
        doc.layers.splice(0..0, [group, above, target, below, photo]);
        doc
    }

    #[test]
    fn references_below_skip_the_target_and_everything_above_it() {
        let mut doc = document();
        let members = |doc: &Document| Retouch::for_target(doc, LayerId(12), RetouchSource::References).references.iter().map(|id| id.0).collect::<Vec<_>>();
        assert!(members(&doc).is_empty(), "nothing marked samples the target alone");
        doc.reference_layers = [LayerId(11), LayerId(12)].into();
        assert!(members(&doc).is_empty(), "references above the target and the target itself are ignored");
        doc.reference_layers = [LayerId(13), LayerId(14)].into();
        assert_eq!(members(&doc), [10, 13, 14], "a member's ancestor group composes it");
        doc.reference_layers = [LayerId(10)].into();
        assert_eq!(members(&doc), [10, 13], "a marked group contributes its children below the target");
        assert!(Retouch::for_target(&doc, LayerId(12), RetouchSource::Editing).references.is_empty());
    }

    #[test]
    fn a_clipping_stack_below_keeps_only_clips_below_the_target() {
        let mut doc = Document::new("clips", 64, 64);
        let mut top_clip = Layer::paint(LayerId(20), "Top clip");
        top_clip.properties.clipped = true;
        let mut target = Layer::paint(LayerId(21), "Target");
        target.properties.clipped = true;
        let mut low_clip = Layer::paint(LayerId(22), "Low clip");
        low_clip.properties.clipped = true;
        let base = Layer::paint(LayerId(23), "Base");
        doc.layers.splice(0..0, [top_clip, target, low_clip, base]);
        doc.reference_layers = [LayerId(23)].into();
        let members = doc.references_below(LayerId(21));
        assert_eq!(members.into_iter().map(|id| id.0).collect::<Vec<_>>(), [22, 23]);
    }

    #[test]
    fn an_adjustment_below_brings_its_inputs() {
        let mut doc = Document::new("adjust", 64, 64);
        let target = Layer::paint(LayerId(30), "Target");
        let mut curves = Layer::paint(LayerId(31), "Curves");
        curves.kind = LayerKind::Effect;
        let program = crate::bundled_effect_catalog()
            .filters()
            .iter()
            .find(|e| e.program.kind == EffectKind::Adjustment)
            .unwrap()
            .program
            .clone();
        curves.effect = Some(Arc::new(crate::EffectInstance::new(program)));
        let input = Layer::paint(LayerId(32), "Input");
        doc.layers.splice(0..0, [target, curves, input]);
        doc.reference_layers = [LayerId(31)].into();
        let members: Vec<_> = doc.references_below(LayerId(30)).into_iter().map(|id| id.0).collect();
        assert!(members.contains(&31) && members.contains(&32), "{members:?}");
    }
}
