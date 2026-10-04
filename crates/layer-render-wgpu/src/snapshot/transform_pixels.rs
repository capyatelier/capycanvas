use super::*;
use layer_core::raster::{RasterRevision, TileKey};

impl SnapshotGpu {
    pub async fn transform_pixels(&self, plan: layer_core::TransformPixelsPlan, control: CaptureControl) -> Result<Layer, String> {
        let mut output = plan.output;
        let scalar = plan.scope == layer_core::TransformPixelsScope::Mask;
        let linked_mask = matches!(plan.scope, layer_core::TransformPixelsScope::Paint { linked_mask: true });
        let mut snapshot = self.capture(plan.input, 0., control.clone()).map_err(|e| e.to_string())?;
        let extent = snapshot.extent;
        let mut color = RasterData { watercolor: snapshot.backing[&output.id].watercolor, ..Default::default() };
        let mut mask = RasterData::default();
        let mut empty = std::collections::BTreeMap::new();
        for plane in [RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness, RasterPlane::Mask] {
            if plane == RasterPlane::Mask && ((!linked_mask && !scalar) || output.mask.as_ref().unwrap().default_coverage != 0.) { continue; }
            let descriptor = plane.descriptor(snapshot.color());
            let tile = layer_core::raster::TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([PAGE_SIZE; 2]).unwrap()])?;
            empty.insert(plane, tile.digest);
        }
        for coordinate in page_coordinates(PixelRect::full(extent)) {
            control.check().map_err(|e| e.to_string())?;
            let region = page_rect(coordinate).intersect(PixelRect::full(extent));
            let (tiles, capture) = snapshot.with_region_gpu(
                [region.min_x(), region.min_y(), region.width(), region.height()], 8 * 1024 * 1024,
                |r, packet, _, encoder| {
                    let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
                    let result = scene.capture_raw_tile(r, packet, coordinate, &plan.geometry, plan.scope, encoder);
                    r.scene = Some(scene);
                    result
                }).map_err(|e| e.to_string())?;
            #[cfg(not(target_arch = "wasm32"))]
            capture.finish()?;
            #[cfg(target_arch = "wasm32")]
            capture.finish_browser(self.encoder.as_ref().ok_or("Raster worker is unavailable")?).await?;
            for (plane, tile) in tiles {
                let backing = tile.try_backing().ok_or("Transformed pixels are not ready")??;
                if empty.get(&plane) == Some(&backing.digest) { continue; }
                let data = if plane == RasterPlane::Mask { &mut mask } else { &mut color };
                data.tiles.insert(TileKey { plane, coordinate }, tile);
            }
        }
        control.check().map_err(|e| e.to_string())?;
        if !scalar { output.raster = RasterRevision::backed(color); }
        if linked_mask || scalar { output.mask.as_mut().unwrap().raster = RasterRevision::backed(mask); }
        Ok(output)
    }
}
