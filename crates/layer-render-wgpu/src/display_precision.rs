
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DisplayPrecision { Half, Full }
impl DisplayPrecision {
    pub fn for_scene(scene: layer_core::SceneView<'_>, clipping: bool) -> Self {
        let color = scene.composition().color;
        if clipping || color.depth.is_float() || color.space != layer_core::color::RgbSpace::Srgb || scene.order().iter().any(|&owner|
            scene.effect(owner).is_some()
                || scene.object_layer(owner).is_some_and(|object|object.image.interpretation.depth.is_float())
                || scene.paint_source(owner).is_some_and(|paint|paint.base.as_ref().is_some_and(|base|base.image.interpretation.depth.is_float()))) {
            Self::Full
        } else { Self::Half }
    }
    pub fn format(self) -> wgpu::TextureFormat {
        match self { Self::Half => wgpu::TextureFormat::Rgba16Float, Self::Full => wgpu::TextureFormat::Rgba32Float }
    }
}

impl crate::WgpuRasterizer {
    pub(super) fn display_precision(&self,scene:layer_core::SceneView<'_>)->DisplayPrecision {
        #[cfg(test)]
        if self.test.reference {return DisplayPrecision::Full;}
        DisplayPrecision::for_scene(scene, self.clipping_preview.iter().any(|enabled| *enabled))
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayPrecision;
    fn document(space: layer_core::color::RgbSpace) -> layer_core::Document {
        let extent = [512, 512];
        let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
        crate::test_support::add_paint(&mut artwork, "Pixels", extent);
        let composition = artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color.depth = layer_core::color::SampleDepth::U8;
        composition.color.space = space;
        layer_core::Document::from_artwork(artwork).unwrap()
    }
    #[test]
    fn half_display_needs_an_srgb_working_space_and_no_clipping_preview() {
        use layer_core::color::RgbSpace;
        assert_eq!(DisplayPrecision::for_scene(document(RgbSpace::Srgb).scene(), false), DisplayPrecision::Half);
        for space in [RgbSpace::DisplayP3, RgbSpace::AdobeRgb, RgbSpace::ProPhoto] {
            assert_eq!(DisplayPrecision::for_scene(document(space).scene(), false), DisplayPrecision::Full,
                "converting {space:?} to the view cancels rounded channels");
        }
        assert_eq!(DisplayPrecision::for_scene(document(RgbSpace::Srgb).scene(), true), DisplayPrecision::Full,
            "clipping previews test exact zeros, which half floats create");
    }
}
