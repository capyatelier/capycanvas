//! Losslessly backed native color is independent of its mutable GPU cache.
//! Pending captures and active edits stay resident; read-only consumers decode
//! cold color through the bounded source cache, never through a display mip.
use super::*;

impl WgpuRasterizer {
    pub(crate) fn color_memory_bytes(&self) -> u64 {
        self.paint_layers.iter().flat_map(|layer| &layer.pages).map(color_page_bytes).sum()
    }
    pub(crate) fn pinned_paint_bytes(&self) -> u64 {
        self.paint_memory_bytes().saturating_sub(self.evictable_native_color_bytes())
    }
    fn evictable_native_color_bytes(&self) -> u64 {
        let Some(native) = &self.native_edit else { return 0; };
        if self.transform_preview.is_some() || self.transforms.as_ref().is_some_and(|transform| transform.has_preview()) { return 0; }
        let required = self.restore_preflight.as_ref().map_or(&[][..], |restore| restore.dependencies());
        self.paint_layers.iter().map(|layer| {
            let changed = self.raster.as_ref().and_then(|raster| raster.targets.get(&layer.id)).map(|target| &target.changed);
            layer.pages.iter().filter(|page| {
                page.stroke_start.is_none()
                    && !changed.is_some_and(|coordinates| coordinates.contains(&page.coordinate))
                    && !required.iter().any(|(target, coordinates)| *target == layer.id && coordinates.binary_search(&page.coordinate).is_ok())
                    && native.backing.get(&layer.id).and_then(|data| data.tiles.get(&TileKey { plane: RasterPlane::Color, coordinate: page.coordinate }))
                        .is_some_and(|tile| matches!(tile.try_backing(), Some(Ok(_))))
            }).map(|page| page.primary.storage_bytes() + page.secondary.as_ref().map_or(0, PageSurface::storage_bytes)).sum::<u64>()
        }).sum::<u64>()
    }
    pub(crate) fn native_backing(&self, id: SourceTarget) -> Option<&Arc<RasterData>> {
        self.native_edit.as_ref()?.backing.get(&id)
    }

    pub(crate) fn native_color_tile(
        &self,
        id: SourceTarget,
        coordinate: [u32; 2],
    ) -> Result<Option<Arc<TileBlob>>, GpuRasterError> {
        self.native_plane_tile(id, RasterPlane::Color, coordinate)
    }

    pub(crate) fn native_plane_tile(&self, id: SourceTarget, plane: RasterPlane, coordinate: [u32; 2])
        -> Result<Option<Arc<TileBlob>>, GpuRasterError> {
        let Some(tile) = self.native_backing(id).and_then(|data| {
            data.tiles.get(&TileKey { plane, coordinate })
        }) else {
            return Ok(None);
        };
        tile.try_backing()
            .ok_or_else(|| GpuRasterError::Effect("Native raster backing is not ready".into()))?
            .map(Some)
            .map_err(GpuRasterError::Effect)
    }

    pub(crate) fn native_color_coordinates(
        &self,
        id: SourceTarget,
    ) -> impl Iterator<Item = [u32; 2]> + '_ {
        self.native_backing(id)
            .into_iter()
            .flat_map(|data| data.tiles.keys())
            .filter(|key| key.plane == RasterPlane::Color)
            .map(|key| key.coordinate)
    }

    pub(crate) fn base_copy(&self, scene: SceneView<'_>, id: SourceTarget, coordinate: [u32; 2]) -> bool {
        scene.paint_base(id).is_some_and(|base| source_access::paint_base_contains(base, coordinate))
            && !self.native_backing(id).is_some_and(|data| data.tiles.contains_key(&TileKey { plane: RasterPlane::Color, coordinate }))
            && !self.raster.as_ref().and_then(|raster| raster.targets.get(&id)).is_some_and(|target| target.changed.contains(&coordinate))
    }

    /// Forget `target`'s pages, in every plane, where `keep` is false. A
    /// transform that moved every pixel of the target left them empty, so
    /// neither the GPU nor the next publication holds them.
    pub(crate) fn drop_vacated_pages(&mut self, target: SourceTarget, keep: impl Fn([u32; 2]) -> bool) {
        if let Some(layer) = self.paint_layers.iter_mut().find(|l| l.id == target) {
            layer.pages.retain(|p| keep(p.coordinate));
            layer.watercolor_wetness_pages.retain(|p| keep(p.coordinate));
        } else {
            self.layer_masks.pages.retain(|(id, c), _| *id != target || keep(*c));
        }
        let prune = |data: &mut Arc<RasterData>| {
            if data.tiles.keys().any(|key| !keep(key.coordinate)) {
                Arc::make_mut(data).tiles.retain(|key, _| keep(key.coordinate));
            }
        };
        if let Some(current) = self.raster.as_mut().and_then(|r| r.targets.get_mut(&target)) {
            prune(&mut current.data);
        }
        if let Some(backing) = self.native_edit.as_mut().and_then(|n| n.backing.get_mut(&target)) {
            prune(backing);
        }
    }

    pub(crate) fn retain_native_backing(&mut self, retained: &std::collections::BTreeMap<SourceTarget, SceneView<'_>>, reset: bool) {
        if let Some(native) = &mut self.native_edit {
            if reset {
                native.backing.clear();
            }
            native.backing.retain(|id, _| retained.contains_key(id));
        }
    }

    /// Live reconciliation may keep cold color in its original integer backing.
    /// Snapshots have no color cache, so all of their color stays there.
    pub(crate) fn restore_live_raster(
        &mut self,
        target: SourceTarget,
        previous: &RasterData,
        data: &Arc<RasterData>,
    ) -> Result<(), GpuRasterError> {
        if !self.snapshot_worker { return self.publish_prepared_restore(target,data); }
        let (before,selected)=self.select_restore_data(target,previous,data,&BTreeSet::new());
        self.restore_raster(target,&before,&selected)?;
        if let Some(native)=&mut self.native_edit {native.backing.insert(target,data.clone());}
        Ok(())
    }

    pub(super) fn select_restore_data(&self,target:SourceTarget,previous:&RasterData,data:&RasterData,required:&BTreeSet<[u32;2]>)->(RasterData,RasterData) {
        if let Some(native) = &self.native_edit {
            let paint = self.paint_layers.iter().find(|l| l.id == target);
            let resident: BTreeSet<_> = paint
                .into_iter()
                .flat_map(|l| &l.pages)
                .map(|p| p.coordinate)
                .chain(required.iter().copied()).collect();
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
            return (before,selected);
        }
        (previous.clone(),data.clone())
    }

    pub(crate) fn publish_cold_raster(&mut self, target: SourceTarget, data: &Arc<RasterData>) -> Result<(), GpuRasterError> {
        let mask = !self.paint_layers.iter().any(|l| l.id == target);
        data.validate_storage_index(self.target_extent(target), mask, self.document_color()).map_err(GpuRasterError::Effect)?;
        self.restore_raster(target, &RasterData::default(), &RasterData { watercolor: data.watercolor, tiles: Default::default() })?;
        if let Some(native) = &mut self.native_edit { native.backing.insert(target, data.clone()); }
        Ok(())
    }

    /// Retire disposable blend scratch, then evict completed canonical color.
    /// This cache target is not a hard active-stroke/transform allocation limit;
    /// those workloads need separate scheduling. No backing wait or readback
    /// happens here.
    pub(crate) fn trim_native_color_cache(&mut self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) -> Vec<PageSurface> {
        let mut retired = Vec::new();
        let Some(native) = &self.native_edit else { return retired; };
        if self.transform_preview.is_some() || self.transforms.as_ref().is_some_and(|t| t.has_preview()) { return retired; }
        let mut bytes = self.color_memory_bytes();
        let limit = native.color_cache_bytes.saturating_add(native.color_cache_pinned_bytes);
        if bytes <= limit { return retired; }
        let destinations = self.destination_pages(batches, tiles);
        for layer in &mut self.paint_layers {
            for page in &mut layer.pages {
                if bytes <= limit || retired.len() == SOURCE_SLOTS { return retired; }
                if page.stroke_start.is_some() || destinations.contains(&(layer.id, page.coordinate)) { continue; }
                let Some(mut spare) = page.secondary.take() else { continue; };
                if page.active_secondary {
                    std::mem::swap(&mut page.primary, &mut spare);
                    page.primary_needs_clear = false;
                }
                page.active_secondary = false;
                bytes -= spare.storage_bytes();retired.push(spare);
                for coverage in layer.coverage_pages.iter().filter(|p| p.coordinate == page.coordinate) { coverage.release_color_bindings(); }
            }
        }
        let required = self.restore_preflight.as_ref().map_or(&[][..], |restore| restore.dependencies());
        for layer in &mut self.paint_layers {
            let Some(data) = native.backing.get(&layer.id) else { continue; };
            let changed = self.raster.as_ref().and_then(|r| r.targets.get(&layer.id)).map(|target| &target.changed);
            let mut index = 0;
            while index < layer.pages.len() {
                if bytes <= limit || retired.len() == SOURCE_SLOTS { return retired; }
                let page = &layer.pages[index];
                let keep = changed.is_some_and(|coordinates| coordinates.contains(&page.coordinate))
                    || page.stroke_start.is_some() || destinations.contains(&(layer.id, page.coordinate))
                    || required.iter().any(|(target, coordinates)| *target == layer.id && coordinates.binary_search(&page.coordinate).is_ok());
                let ready = data.tiles.get(&TileKey { plane: RasterPlane::Color, coordinate: page.coordinate })
                    .is_some_and(|tile| matches!(tile.try_backing(), Some(Ok(_))));
                let count = 1 + usize::from(page.secondary.is_some());
                if keep || !ready || retired.len() + count > SOURCE_SLOTS { index += 1;continue; }
                for coverage in layer.coverage_pages.iter().filter(|p| p.coordinate == page.coordinate) { coverage.release_color_bindings(); }
                let page = layer.pages.swap_remove(index);
                bytes -= color_page_bytes(&page);
                retired.push(page.primary);retired.extend(page.secondary);
            }
        }
        retired
    }

}

fn color_page_bytes(page: &LayerPage) -> u64 {
    page.primary.storage_bytes()
        + page.stroke_start.as_ref().map_or(0, |(_, start)| start.storage_bytes())
        + page
            .secondary
            .as_ref()
            .map_or(0, PageSurface::storage_bytes)
}
