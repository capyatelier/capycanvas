
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DisplayPrecision { Half, Full }
impl DisplayPrecision {
    pub fn for_scene(scene: layer_core::SceneView<'_>) -> Self {
        if scene.composition().color.depth.is_float() || scene.order().iter().any(|&owner|
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
        DisplayPrecision::for_scene(scene)
    }
}
