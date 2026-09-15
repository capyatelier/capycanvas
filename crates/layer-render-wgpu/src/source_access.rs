//! Raw artwork access shared by queries and pixel operations. Painted tiles
//! override an immutable original; absent pages are not always transparent.
use super::*;

pub(super) struct RawTile {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl WgpuRasterizer {
    /// Prepare at most one job's neighborhood before borrowing its views.
    /// Ordinary paint has no source preparation or resource-handle cloning.
    pub(super) fn prepare_raw_neighborhood<const N: usize>(
        &mut self,
        layer: LayerId,
        coordinate: [u32; 2],
        offsets: [[i32; 2]; N],
        preview: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        assert!(N <= SOURCE_SLOTS);
        if !self.tiled_sources.contains_key(&layer) && self.native_backing(layer).is_none() {
            return Ok(());
        }
        for [dx, dy] in offsets {
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            if x < 0 || y < 0 {
                continue;
            }
            let neighbor = [x as u32, y as u32];
            if preview
                && !self
                    .preview_damage
                    .intersect(page_rect(neighbor))
                    .is_empty()
                && self.preview_pages.iter().any(|p| p.coordinate == neighbor)
            {
                continue;
            }
            self.raw_layer_tile(layer, neighbor, encoder)?;
        }
        Ok(())
    }

    /// Consume the resulting binding before preparing another neighborhood;
    /// no cache eviction may intervene. All returned resources are borrowed.
    pub(super) fn raw_layer_neighborhood<'a, const N: usize>(
        &'a self,
        layer: &'a PaintLayer,
        coordinate: [u32; 2],
        offsets: [[i32; 2]; N],
        preview: bool,
    ) -> [&'a wgpu::TextureView; N] {
        offsets.map(|[dx, dy]| {
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            if x < 0 || y < 0 {
                return &self.empty_view;
            }
            let neighbor = [x as u32, y as u32];
            let predicted = if preview
                && !self
                    .preview_damage
                    .intersect(page_rect(neighbor))
                    .is_empty()
            {
                self.preview_pages.iter().find(|p| p.coordinate == neighbor)
            } else {
                None
            };
            if let Some(page) =
                predicted.or_else(|| layer.pages.iter().find(|p| p.coordinate == neighbor))
            {
                return &page.active().view;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Ok(Some(blob)) = self.native_color_tile(layer.id, neighbor)
                && let Some(view) = self
                    .scene
                    .as_ref()
                    .and_then(|s| s.prepared_raster_view(&blob, self.document_color().space))
            {
                return view;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(source) = self.tiled_sources.get(&layer.id)
                && let Some(view) = self
                    .scene
                    .as_ref()
                    .and_then(|s| s.prepared_source_view(source, neighbor))
            {
                return view;
            }
            &self.empty_view
        })
    }

    /// Consume this texture in queue order before the source cache can evict it.
    /// It can be encoded sRGB8 paint or linear Float32 original-source data.
    pub(super) fn raw_layer_tile(
        &mut self,
        layer: LayerId,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<RawTile>, GpuRasterError> {
        if let Some(page) = self
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
        {
            return Ok(Some(RawTile {
                texture: page.active().texture.clone(),
                view: page.active().view.clone(),
            }));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(blob) = self.native_color_tile(layer, coordinate)? {
            return self
                .backed_raster_tile(&blob, self.document_color().space, encoder)
                .map(Some);
        }
        let Some(source) = self.tiled_sources.get(&layer).cloned() else {
            return Ok(None);
        };
        self.original_source_tile(&source, coordinate, encoder)
    }

    pub(super) fn original_source_tile(
        &mut self,
        source: &std::sync::Arc<layer_core::color::source::SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<RawTile>, GpuRasterError> {
        if coordinate[0] >= source.extent[0].div_ceil(PAGE_SIZE)
            || coordinate[1] >= source.extent[1].div_ceil(PAGE_SIZE)
        {
            return Ok(None);
        }
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
        let result = scene.source_tile_for_query(self, source, coordinate, encoder);
        self.scene = Some(scene);
        result.map(Some)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn backed_raster_tile(
        &mut self,
        blob: &std::sync::Arc<layer_core::raster::TileBlob>,
        space: layer_core::color::RgbSpace,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<RawTile, GpuRasterError> {
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
        let result = scene.raster_tile_for_query(self, blob, space, encoder);
        self.scene = Some(scene);
        result
    }
}
