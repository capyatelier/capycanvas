use super::*;
use layer_core::authored::{Occurrence, EffectApplication, MaskUse};
use std::sync::Weak;

#[derive(Clone, PartialEq)]
pub(super) struct MaskMetadata {
    pub use_: MaskUse,
    pub domain: [u32; 2],
    pub initial: Option<layer_core::Selection>,
    pub default_coverage: f32,
}
pub(super) fn mask_metadata(scene: layer_core::SceneView<'_>, handle: OccurrenceHandle) -> Option<MaskMetadata> {
    scene.mask(handle).map(|(use_, source)| MaskMetadata {
        use_: use_.clone(), domain: source.domain, initial: source.initial.clone(), default_coverage: source.default_coverage,
    })
}

#[derive(Clone)]
pub(super) struct Metadata {
    pub id: OccurrenceHandle,
    pub parent: Option<OccurrenceHandle>,
    pub evaluation_offset: layer_core::Point,
    pub occurrence: Occurrence,
    pub effect: Option<Weak<EffectApplication>>,
    pub effect_contract: Option<(layer_core::EffectKind, layer_core::EffectSpace, bool)>,
    definition: Option<Weak<layer_core::authored::Definition>>,
    pub mask: Option<MaskMetadata>,
    source: Option<Weak<layer_core::color::source::SourceImage>>,
}
impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.parent == other.parent && self.evaluation_offset == other.evaluation_offset && self.occurrence == other.occurrence
            && match (&self.effect, &other.effect) {
                (None, None) => true, (Some(a), Some(b)) => Weak::ptr_eq(a, b), _ => false,
            } && self.effect_contract == other.effect_contract
            && match (&self.definition, &other.definition) { (None, None) => true, (Some(a), Some(b)) => Weak::ptr_eq(a, b), _ => false }
            && self.mask == other.mask
            && match (&self.source, &other.source) {
                (None, None) => true,
                (Some(a), Some(b)) => Weak::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl Metadata {
    pub(super) fn same_content(&self, other: &Self) -> bool {
        let normalize = |metadata: &Self| {
            let mut metadata = metadata.clone();
            metadata.occurrence.opacity = 1.;
            metadata.occurrence.blend = layer_core::LayerBlend::Normal;
            metadata.occurrence.attachment = layer_core::Attachment::None;
            metadata
        };
        normalize(self) == normalize(other)
    }
    pub(super) fn new(scene: layer_core::SceneView<'_>, handle: OccurrenceHandle) -> Self {
        let mut occurrence = scene.occurrence(handle).unwrap().clone();
        occurrence.name = Arc::from("");
        occurrence.locked = false;
        occurrence.alpha_locked = false;
        occurrence.reference = false;
        if occurrence.attachment.is_clip() && !scene.effective_clipped(handle) { occurrence.attachment = layer_core::Attachment::None; }
        occurrence.isolated_blend = layer_core::LayerBlend::Normal;
        let effect = match occurrence.content {
            layer_core::authored::OccurrenceContent::Effect(effect) => scene.artwork().effects.shared(effect).map(Arc::downgrade),
            _ => None,
        };
        Self {
            id: handle, parent: scene.evaluation_parent(handle), evaluation_offset: scene.occurrence_offset(handle), occurrence, effect,
            effect_contract: scene.effect(handle).map(|effect| (effect.program.kind, effect.program.space, effect.program.image_boundary())),
            definition: scene.effect_application(handle).and_then(|application| scene.artwork().definitions.shared(application.definition)).map(Arc::downgrade),
            mask: mask_metadata(scene, handle), source: scene.paint_source(handle).and_then(|paint| paint.original.as_ref()).map(Arc::downgrade),
        }
    }
}

#[derive(PartialEq)]
pub(super) struct PreviewMetadata {
    metadata: Metadata,
    raster: Option<u64>,
    mask_raster: Option<u64>,
}
impl PreviewMetadata {
    pub(super) fn new(scene: layer_core::SceneView<'_>, handle: OccurrenceHandle) -> Self {
        Self {
            metadata: Metadata::new(scene, handle), raster: scene.paint_source(handle).map(|paint| paint.raster.identity()),
            mask_raster: scene.mask(handle).map(|(_, source)| source.raster.identity()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{SampleDepth, source::*};

    #[test]
    fn cache_keys_release_source_and_raster_backing_and_track_replacement() {
        let mut builder = SourceBuilder::new(
            [1, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: SampleDepth::U16,
                profile: Default::default(),
                profile_assumed: false,
            },
            1024 * 1024,
        )
        .unwrap();
        builder.push_row(&[255; 8]).unwrap();
        let mut artwork = layer_core::authored::Artwork::new([1, 1]).unwrap();
        let paint = artwork.paint.insert(layer_core::authored::PortableId::random(), layer_core::authored::PaintSource {
            domain: [1, 1], raster: Default::default(), original: Some(Arc::new(builder.finish().unwrap())), operations: Arc::default(),
        }).unwrap();
        let handle = artwork.occurrences.insert(layer_core::authored::PortableId::random(), layer_core::authored::Occurrence::new(
            layer_core::authored::OccurrenceContent::Paint(paint), "original")).unwrap();
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
        let index = Arc::new(layer_core::SceneIndex::build(&artwork).unwrap());
        let source = Arc::downgrade(artwork.paint.get(paint).unwrap().original.as_ref().unwrap());
        let raster = Arc::downgrade(&artwork.paint.get(paint).unwrap().raster.wait_data().unwrap());
        let key = Metadata::new(SceneView::new(&artwork, &index), handle);
        let preview = PreviewMetadata::new(SceneView::new(&artwork, &index), handle);
        assert!(key == Metadata::new(SceneView::new(&artwork, &index), handle));
        assert!(preview == PreviewMetadata::new(SceneView::new(&artwork, &index), handle));
        assert_eq!(source.strong_count(), 1);
        let original = artwork.paint.get(paint).unwrap().original.as_ref().unwrap();
        let replacement = Arc::new((**original).clone());
        artwork.paint.get_mut(paint).unwrap().original = Some(replacement);
        assert!(key != Metadata::new(SceneView::new(&artwork, &index), handle), "new source interpretation invalidates pixels");
        assert!(preview != PreviewMetadata::new(SceneView::new(&artwork, &index), handle));
        assert_eq!(source.strong_count(), 0);
        artwork.paint.get_mut(paint).unwrap().raster = Default::default();
        assert_eq!(
            raster.strong_count(),
            0,
            "keys must not own historical tile maps"
        );
    }
}
