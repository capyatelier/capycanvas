//! Raw artwork access shared by queries and pixel operations. Painted tiles
//! override an immutable original; absent pages are not always transparent.
use super::*;

impl WgpuRasterizer {
    /// Consume this texture in queue order before requesting more source tiles.
    /// It can be encoded sRGB8 paint or linear Float32 original-source data.
    pub(super) fn raw_layer_tile(
        &mut self,
        layer: LayerId,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<wgpu::Texture>, GpuRasterError> {
        if let Some(page) = self
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
        {
            return Ok(Some(page.active().texture.clone()));
        }
        let Some(source) = self.tiled_sources.get(&layer).cloned() else {
            return Ok(None);
        };
        if coordinate[0] >= source.extent[0].div_ceil(PAGE_SIZE)
            || coordinate[1] >= source.extent[1].div_ceil(PAGE_SIZE)
        {
            return Ok(None);
        }
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
        let result = scene.source_texture(self, &source, coordinate, encoder);
        self.scene = Some(scene);
        result.map(Some)
    }
}
