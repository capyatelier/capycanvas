//! Cache comparisons must not own retired originals or raster history.
use super::*;
use std::sync::Weak;

pub(super) struct Metadata {
    layer: Layer,
    source: Option<Weak<layer_core::color::source::SourceImage>>,
}
impl std::ops::Deref for Metadata {
    type Target = Layer;
    fn deref(&self) -> &Layer {
        &self.layer
    }
}
impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        self.layer == other.layer
            && match (&self.source, &other.source) {
                (None, None) => true,
                (Some(a), Some(b)) => Weak::ptr_eq(a, b),
                _ => false,
            }
    }
}
fn empty_raster() -> layer_core::raster::RasterRevision {
    static EMPTY: std::sync::OnceLock<layer_core::raster::RasterRevision> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(Default::default).clone()
}
pub(super) fn mask_metadata(mask: &Option<layer_core::LayerMask>) -> Option<layer_core::LayerMask> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<layer_core::LayerOperation>>> =
        std::sync::OnceLock::new();
    mask.as_ref().map(|mask| {
        let mut mask = mask.clone();
        mask.raster = empty_raster();
        mask.pending_operations = EMPTY.get_or_init(Default::default).clone();
        mask
    })
}
impl Metadata {
    pub(super) fn new(layer: &Layer) -> Self {
        let mut metadata = layer.composite_snapshot();
        metadata.source = None;
        metadata.raster = empty_raster();
        metadata.mask = mask_metadata(&layer.mask);
        Self {
            layer: metadata,
            source: layer.source.as_ref().map(Arc::downgrade),
        }
    }
}

/// Preview queries also distinguish raster publication identities. Unlike the
/// live compositor, they do not receive an individual damage list with requests.
#[derive(PartialEq)]
pub(super) struct PreviewMetadata {
    metadata: Metadata,
    raster: u64,
    mask_raster: Option<u64>,
}
impl PreviewMetadata {
    pub(super) fn new(layer: &Layer) -> Self {
        Self {
            metadata: Metadata::new(layer),
            raster: layer.raster.identity(),
            mask_raster: layer.mask.as_ref().map(|m| m.raster.identity()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{IntegerDepth, source::*};

    #[test]
    fn cache_keys_release_source_and_raster_backing_and_track_replacement() {
        let mut builder = SourceBuilder::new(
            [1, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: IntegerDepth::U16,
                profile: Default::default(),
                profile_assumed: false,
            },
            1024 * 1024,
        )
        .unwrap();
        builder.push_row(&[255; 8]).unwrap();
        let mut layer = Layer::paint(LayerId(1), "original");
        layer.source = Some(Arc::new(builder.finish().unwrap()));
        let source = Arc::downgrade(layer.source.as_ref().unwrap());
        let raster = Arc::downgrade(&layer.raster.wait_data().unwrap());
        let key = Metadata::new(&layer);
        let preview = PreviewMetadata::new(&layer);
        assert!(key == Metadata::new(&layer));
        assert!(preview == PreviewMetadata::new(&layer));
        assert_eq!(source.strong_count(), 1);
        layer.source = Some(Arc::new((**layer.source.as_ref().unwrap()).clone()));
        assert!(
            key != Metadata::new(&layer),
            "new source interpretation invalidates pixels"
        );
        assert!(preview != PreviewMetadata::new(&layer));
        assert_eq!(source.strong_count(), 0);
        layer.raster = Default::default();
        assert_eq!(
            raster.strong_count(),
            0,
            "keys must not own historical tile maps"
        );
    }
}
