use super::*;
use layer_core::raster::{RasterRevision, TileKey};

impl SnapshotGpu {
    pub async fn transform_pixels(&self, plan: layer_core::TransformPixelsPlan, control: CaptureControl) -> Result<layer_core::Edit, String> {
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
        snapshot.set_raw_plan(plan.clone());
        let placements = snapshot.raw_plan.as_ref().unwrap().sources.clone();
        snapshot.renderer.ensure_document_metadata(extent, snapshot.scene.view().with_scope(&snapshot.scope).with_offset(snapshot.offset)).map_err(|e| e.to_string())?;
        let mut color = RasterData { watercolor: snapshot.backing[&target].watercolor, ..Default::default() };
        let mut mask = RasterData::default();
        let mut empty = std::collections::BTreeMap::new();
        for &(plane,_) in &placements {
            if plane == RasterPlane::Mask && plan.coverage.and_then(|h| plan.scene.view().coverage(h)).map_or(0., |c| c.default_coverage) != 0. { continue; }
            let descriptor = plane.descriptor_for(snapshot.color(), plan.scene.view().color_mode(target));
            let tile = layer_core::raster::TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([PAGE_SIZE; 2]).unwrap()])?;
            let bytes = tile.read_compressed().await?;
            empty.insert(plane, tile.content_digest_bytes_on_worker(&bytes)?);
        }
        for coordinate in snapshot.raw_coordinates().map_err(|e| e.to_string())? {
            control.check().map_err(|e| e.to_string())?;
            let region = page_rect(coordinate).intersect(PixelRect::full(extent));
            let unit=scene::sources::InputUnit::tile(coordinate);
            let mut accepted:Option<scene::sources::AcceptedInputs>=None;
            let (tiles, capture) = snapshot.with_region_gpu(
                [region.min_x(), region.min_y(), region.width(), region.height()], 8 * 1024 * 1024,
                |r, packet, _, encoder| {
                    let mut scene = r.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(r))?;
                    let result=(|| {
                        if accepted.as_ref().is_some_and(|inputs|inputs.unit()!=Some(unit)) {scene.discard_unencoded_jobs();}
                        accepted=scene::sources::AcceptedInputs::take_for(&mut accepted,unit);
                        if accepted.is_none() {accepted=Some(scene.declare_raw_capture(r,packet,target,&placements,coordinate)?.for_unit(unit)?.accept_query(r,Some(&scene))?);}
                        let inputs=accepted.as_mut().ok_or(GpuRasterError::Invariant("accepted raw capture sources are missing"))?;
                        inputs.check_unit(unit)?;inputs.clone().track(encoder);
                        let result=scene.capture_raw_tile(r,packet,coordinate,&plan,&placements,encoder,inputs);
                        if result.is_ok() {accepted.take().unwrap().track(encoder);}
                        result
                    })();
                    r.scene = Some(scene);
                    result
                }).await.map_err(|e| e.to_string())?;
            #[cfg(not(target_arch = "wasm32"))]
            capture.finish()?;
            #[cfg(target_arch = "wasm32")]
            capture.finish_browser(self.encoder.as_ref().ok_or("Raster worker is unavailable")?).await?;
            for (plane, tile) in tiles {
                let backing = tile.try_backing().ok_or("Transformed pixels are not ready")??;
                let bytes = backing.read_compressed().await?;
                if empty.get(&plane) == Some(&backing.content_digest_bytes_on_worker(&bytes)?) { continue; }
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
