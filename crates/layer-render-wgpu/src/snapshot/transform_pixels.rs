use super::*;
use layer_core::raster::{RasterRevision, TileKey};

impl SnapshotGpu {
    pub async fn transform_pixels(&self, plan: layer_core::TransformPixelsPlan, control: CaptureControl) -> Result<layer_core::Edit, String> {
        let scalar = plan.scope == layer_core::TransformPixelsScope::Mask;
        let linked_mask = matches!(plan.scope, layer_core::TransformPixelsScope::Paint { linked_mask: true });
        let target = plan.paint.map(SourceTarget::Paint).unwrap_or(plan.target);
        let mut snapshot = SnapshotRenderer::construct(plan.scene.clone(), SceneScope::Raw(target), control.clone(), self).map_err(|e| e.to_string())?;
        if let Some(handle) = plan.coverage {
            let target = SourceTarget::Coverage(handle);
            let data = plan.scene.view().raster(target).ok_or("Missing linked coverage")?.wait_data_cancellable(control.cancellation_flag())?;
            snapshot.backing.insert(target, data);
        }
        let extent = plan.extent;
        snapshot.extent = extent;
        snapshot.offset = layer_core::offsets::point(plan.origin.map(|v| -v));
        snapshot.raw_plan = Some(plan.clone());
        snapshot.renderer.ensure_document_metadata(extent, snapshot.scene.view().with_scope(&snapshot.scope).with_offset(snapshot.offset)).map_err(|e| e.to_string())?;
        let mut color = RasterData { watercolor: snapshot.backing[&target].watercolor, ..Default::default() };
        let mut mask = RasterData::default();
        let mut empty = std::collections::BTreeMap::new();
        for plane in [RasterPlane::Color, RasterPlane::WatercolorWetness, RasterPlane::Mask] {
            if plane == RasterPlane::Mask && ((!linked_mask && !scalar) || plan.coverage.and_then(|h| plan.scene.view().coverage(h)).map_or(0., |c| c.default_coverage) != 0.) { continue; }
            let descriptor = plane.descriptor(snapshot.color());
            let tile = layer_core::raster::TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([PAGE_SIZE; 2]).unwrap()])?;
            empty.insert(plane, tile.content_digest()?);
        }
        for coordinate in page_coordinates(PixelRect::full(extent)) {
            control.check().map_err(|e| e.to_string())?;
            let region = page_rect(coordinate).intersect(PixelRect::full(extent));
            let (tiles, capture) = snapshot.with_region_gpu(
                [region.min_x(), region.min_y(), region.width(), region.height()], 8 * 1024 * 1024,
                |r, packet, _, encoder| {
                    let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
                    let result = scene.capture_raw_tile(r, packet, coordinate, &plan, encoder);
                    r.scene = Some(scene);
                    result
                }).await.map_err(|e| e.to_string())?;
            #[cfg(not(target_arch = "wasm32"))]
            capture.finish()?;
            #[cfg(target_arch = "wasm32")]
            capture.finish_browser(self.encoder.as_ref().ok_or("Raster worker is unavailable")?).await?;
            for (plane, tile) in tiles {
                let backing = tile.try_backing().ok_or("Transformed pixels are not ready")??;
                if empty.get(&plane) == Some(&backing.content_digest()?) { continue; }
                let data = if plane == RasterPlane::Mask { &mut mask } else { &mut color };
                data.tiles.insert(TileKey { plane, coordinate }, tile);
            }
        }
        control.check().map_err(|e| e.to_string())?;
        fn install(edit: &mut layer_core::Edit, paint: Option<layer_core::authored::PaintHandle>, coverage: Option<layer_core::authored::CoverageHandle>, color: &RasterRevision, mask: &RasterRevision) {
            match edit {
                layer_core::Edit::Paint(change) if Some(change.handle) == paint => {
                    if let Some(source) = &mut change.value { source.raster = color.clone(); }
                },
                layer_core::Edit::Coverage(change) if Some(change.handle) == coverage => {
                    if let Some(source) = &mut change.value { source.raster = mask.clone(); }
                },
                layer_core::Edit::Batch(edits) => for edit in edits { install(edit, paint, coverage, color, mask); }, _ => {}
            }
        }
        let mut output = plan.output;
        install(&mut output, plan.paint, plan.coverage, &RasterRevision::backed(color), &RasterRevision::backed(mask));
        Ok(output)
    }
}
