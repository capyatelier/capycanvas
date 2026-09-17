//! Losslessly backed native color is independent of its mutable GPU cache.
//! Pending captures and active edits stay resident; read-only consumers decode
//! cold color through the bounded source cache, never through a display mip.
use super::*;

impl WgpuRasterizer {
    pub(crate) fn native_backing(&self, id: LayerId) -> Option<&Arc<RasterData>> {
        return self.native_edit.as_ref()?.backing.get(&id);
    }

    pub(crate) fn native_color_tile(
        &self,
        id: LayerId,
        coordinate: [u32; 2],
    ) -> Result<Option<Arc<TileBlob>>, GpuRasterError> {
        let Some(tile) = self.native_backing(id).and_then(|data| {
            data.tiles.get(&TileKey {
                plane: RasterPlane::Color,
                coordinate,
            })
        }) else {
            return Ok(None);
        };
        tile.try_backing()
            .ok_or_else(|| GpuRasterError::Effect("Native color backing is not ready".into()))?
            .map(Some)
            .map_err(GpuRasterError::Effect)
    }

    pub(crate) fn native_color_coordinates(
        &self,
        id: LayerId,
    ) -> impl Iterator<Item = [u32; 2]> + '_ {
        self.native_backing(id)
            .into_iter()
            .flat_map(|data| data.tiles.keys())
            .filter(|key| key.plane == RasterPlane::Color)
            .map(|key| key.coordinate)
    }

    pub(crate) fn retain_native_backing(&mut self, layers: &[Layer], reset: bool) {
        if let Some(native) = &mut self.native_edit {
            if reset {
                native.backing.clear();
            }
            native.backing.retain(|id, _| {
                layers
                    .iter()
                    .any(|l| l.id == *id || l.masks().any(|m| m.id == *id))
            });
        }
    }

    /// Live reconciliation may keep cold color in its original integer backing.
    /// Standalone snapshot callers still restore their explicitly selected pages.
    pub(crate) fn restore_live_raster(
        &mut self,
        target: LayerId,
        previous: &RasterData,
        data: &Arc<RasterData>,
    ) -> Result<(), GpuRasterError> {
        if let Some(native) = &self.native_edit {
            let paint = self.paint_layers.iter().find(|l| l.id == target);
            data.validate_index(self.target_extent(target), paint.is_none(), self.document_color())
                .map_err(GpuRasterError::Effect)?;
            let resident: BTreeSet<_> = paint
                .into_iter()
                .flat_map(|l| &l.pages)
                .map(|p| p.coordinate)
                .collect();
            let other_bytes: u64 = self
                .paint_layers
                .iter()
                .filter(|l| l.id != target)
                .flat_map(|l| &l.pages)
                .map(color_page_bytes)
                .sum();
            let colors = data
                .tiles
                .keys()
                .filter(|key| key.plane == RasterPlane::Color)
                .count() as u64;
            // Reserve both mutable color surfaces, including a future blend
            // companion, before deciding whether an eager restore fits.
            let eager = other_bytes.saturating_add(
                colors.saturating_mul(2 * PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16),
            ) <= native.color_cache_bytes;
            let selected = RasterData {
                watercolor: data.watercolor,
                tiles: data
                    .tiles
                    .iter()
                    .filter(|(key, _)| {
                        key.plane != RasterPlane::Color
                            || eager
                            || resident.contains(&key.coordinate)
                    })
                    .map(|(key, tile)| (*key, tile.clone()))
                    .collect(),
            };
            let mut before = previous.clone();
            before.tiles.retain(|key, _| {
                key.plane != RasterPlane::Color || resident.contains(&key.coordinate)
            });
            self.restore_raster(target, &before, &selected)?;
            self.native_edit
                .as_mut()
                .unwrap()
                .backing
                .insert(target, data.clone());
            return Ok(());
        }
        self.restore_raster(target, previous, data)
    }

    /// Retire disposable blend scratch, then evict completed canonical color.
    /// This cache target is not a hard active-stroke/transform allocation limit;
    /// those workloads need separate scheduling. No backing wait or readback
    /// happens here.
    pub(crate) fn trim_native_color_cache(&mut self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) {
        if let Some(native) = &self.native_edit {
            if self.transform_preview.is_some()
                || self.transforms.as_ref().is_some_and(|t| t.has_preview())
            {
                return;
            }
            let mut bytes: u64 = self
                .paint_layers
                .iter()
                .flat_map(|l| &l.pages)
                .map(color_page_bytes)
                .sum();
            if bytes <= native.color_cache_bytes {
                return;
            }
            let destinations = self.destination_pages(batches, tiles);
            // An inactive blend surface is cheaper to recreate than a canonical
            // page is to decompress. Retire scratch first, even on changed pages:
            // their active surface stays pinned until capture completes. Keep
            // this frame's destinations to avoid immediate drop/reallocation.
            for layer in &mut self.paint_layers {
                for page in &mut layer.pages {
                    if bytes <= native.color_cache_bytes {
                        return;
                    }
                    if !destinations.contains(&(layer.id, page.coordinate)) {
                        bytes -= page.discard_inactive();
                    }
                }
            }
            for layer in &mut self.paint_layers {
                let Some(data) = native.backing.get(&layer.id) else {
                    continue;
                };
                let changed = self
                    .raster
                    .as_ref()
                    .and_then(|r| r.targets.get(&layer.id))
                    .map(|t| &t.changed);
                layer.pages.retain(|page| {
                    if bytes <= native.color_cache_bytes
                        || changed.is_some_and(|c| c.contains(&page.coordinate))
                    {
                        return true;
                    }
                    let ready = data
                        .tiles
                        .get(&TileKey {
                            plane: RasterPlane::Color,
                            coordinate: page.coordinate,
                        })
                        .is_some_and(|tile| matches!(tile.try_backing(), Some(Ok(_))));
                    if ready {
                        bytes -= color_page_bytes(page);
                    }
                    !ready
                });
            }
        }
    }
}

fn color_page_bytes(page: &LayerPage) -> u64 {
    page.primary.storage_bytes()
        + page
            .secondary
            .as_ref()
            .map_or(0, PageSurface::storage_bytes)
}
