//! Losslessly backed native color is independent of its mutable GPU cache.
//! Pending captures and active edits stay resident; read-only consumers decode
//! cold color through the bounded source cache, never through a display mip.
use super::*;

impl WgpuRasterizer {
    #[cfg(test)]
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
            let contents = self.raster.as_ref().and_then(|raster| raster.targets.get(&layer.id)).map(|target| &target.contents);
            let held = |coordinate: [u32; 2]| required.iter().any(|(target, coordinates)| *target == layer.id && coordinates.binary_search(&coordinate).is_ok());
            layer.pages.iter().filter(|page| {
                page.stroke_start.is_none()
                    && !contents.is_some_and(|contents| contents.modified(page.coordinate))
                    && !held(page.coordinate)
                    && native.backing.get(&layer.id).and_then(|data| data.tiles.get(&TileKey { plane: RasterPlane::Color, coordinate: page.coordinate }))
                        .is_some_and(|tile| matches!(tile.try_backing(), Some(Ok(_))))
            }).map(|page| page.primary.storage_bytes() + page.secondary.as_ref().map_or(0, |surface|surface.storage_bytes())).sum::<u64>()
            + layer.coverage_pages.iter().filter(|page| page.owner.is_none_or(|owner| Some(owner) != self.live_stroke) && !held(page.coordinate))
                .map(|page| page.primary.storage_bytes() + page.secondary.storage_bytes()).sum::<u64>()
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
        if plane==RasterPlane::Mask && let Some(source)=self.layer_masks.definitions.get(&id) {
            let data=self.mask_data(id,source)?;
            let Some(tile)=data.tiles.get(&TileKey {plane,coordinate}) else {
                if self.mask_version(id,source,coordinate)?.matches(&contents::mask_default(source.domain,source.default_coverage,coordinate))!=Some(true) {return Err(GpuRasterError::NotReady);}
                return Ok(None);
            };
            if tile.content_version().matches(&self.mask_version(id,source,coordinate)?)!=Some(true) {return Err(GpuRasterError::NotReady);}
            return tile.try_backing().ok_or(GpuRasterError::NotReady)?.map(Some).map_err(GpuRasterError::Effect);
        }
        let selected=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&id));
        let data=selected.map(|target|target.contents.data()).or_else(||self.native_backing(id));
        let key=TileKey {plane,coordinate};
        let Some(tile)=data.and_then(|data|data.tiles.get(&key)) else {
            if let Some(selected)=selected && selected.contents.version(key).matches(&selected.contents.selected_version(key))!=Some(true) {return Err(GpuRasterError::NotReady);}
            return Ok(None);
        };
        if let Some(selected)=selected && tile.content_version().matches(&selected.contents.version(TileKey {plane,coordinate}))!=Some(true) {
            return Err(GpuRasterError::NotReady);
        }
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
            && !self.raster.as_ref().and_then(|raster| raster.targets.get(&id)).is_some_and(|target| target.contents.modified(coordinate))
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
            current.contents.retain(&keep);
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

    pub(super) fn select_restore_data(&mut self,target:SourceTarget,previous:&RasterData,data:&RasterData,required:&BTreeSet<[u32;2]>)->(RasterData,RasterData) {
        if self.native_edit.is_some() {
            let paint = self.paint_layers.iter().find(|l| l.id == target);
            let resident: BTreeSet<_> = paint
                .into_iter()
                .flat_map(|layer| layer.pages.iter().map(|page|page.coordinate)
                    .chain(layer.coverage_pages.iter().map(|page|page.coordinate))
                    .chain(layer.watercolor_wetness_pages.iter().map(|page|page.coordinate)))
                .chain(self.layer_masks.pages.keys().filter_map(|(id,coordinate)|(*id==target).then_some(*coordinate)))
                .chain(required.iter().copied()).collect();
            let select = |data: &RasterData| RasterData {
                watercolor: data.watercolor,
                tiles: resident.iter().flat_map(|coordinate| {
                    [RasterPlane::Color, RasterPlane::Mask, RasterPlane::WatercolorWetness].into_iter()
                        .filter_map(move |plane| {
                            let key = TileKey { plane, coordinate: *coordinate };
                            data.tiles.get(&key).filter(|tile| plane != RasterPlane::Color
                                || required.contains(coordinate)
                                || previous.tiles.get(&key).is_some_and(|old|old.same_capture(tile)))
                                .map(|tile| (key, tile.clone()))
                        })
                }).collect(),
            };
            return (select(previous),select(data));
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

    pub(crate) fn evict_raster(&mut self, bytes: u64, batches: &[DabBatch], tiles: &[Vec<BrushTile>], encoder: &mut submission::CommandEncoder) {
        let (pages, leases) = self.evict_color(bytes, batches, tiles);
        self.retire_native_color(pages, leases, encoder);
    }
    pub(crate) fn evict_color(&mut self, bytes: u64, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) -> (Vec<PageSurface>, Vec<Arc<crate::residency::Lease>>) {
        if bytes == 0 { return Default::default(); }
        let leases = self.offer_native_color(batches, tiles);
        self.raster_tiers.raster.heap().evict(bytes, crate::residency::Class::Fallback);
        self.raster_buffers.release_evicted();
        self.take_evicted_color(leases)
    }
    fn offer_native_color(&self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) -> std::collections::HashMap<(SourceTarget, [u32; 2], Offered), Arc<crate::residency::Lease>> {
        let mut leases = std::collections::HashMap::new();
        let Some(native) = &self.native_edit else { return leases; };
        if self.transform_preview.is_some() || self.transforms.as_ref().is_some_and(|t| t.has_preview()) { return leases; }
        let destinations = self.destination_pages(batches, tiles);
        let required = self.restore_preflight.as_ref().map_or(&[][..], |restore| restore.dependencies());
        let (heap, notice) = (self.raster_tiers.raster.heap(), Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let mut offer = |key, bytes, work| { let lease = heap.lease(&notice); lease.offer(bytes, work); leases.insert(key, lease); };
        for layer in &self.paint_layers {
            let data = native.backing.get(&layer.id);
            let contents = self.raster.as_ref().and_then(|r| r.targets.get(&layer.id)).map(|target| &target.contents);
            let held = |coordinate: [u32; 2]| destinations.contains(&(layer.id, coordinate))
                || required.iter().any(|(target, coordinates)| *target == layer.id && coordinates.binary_search(&coordinate).is_ok());
            for page in &layer.pages {
                if page.stroke_start.is_some() || held(page.coordinate) { continue; }
                if let Some(spare) = &page.secondary { offer((layer.id, page.coordinate, Offered::Spare), spare.storage_bytes(), 0); }
                let ready = data.and_then(|data| data.tiles.get(&TileKey { plane: RasterPlane::Color, coordinate: page.coordinate }))
                    .is_some_and(|tile| matches!(tile.try_backing(), Some(Ok(_))));
                if ready && !contents.is_some_and(|contents| contents.modified(page.coordinate)) {
                    let bytes = color_page_bytes(page);
                    offer((layer.id, page.coordinate, Offered::Color), bytes, crate::residency::rebuild_value(bytes, bytes));
                }
            }
            for page in layer.coverage_pages.iter().filter(|page| page.owner.is_none_or(|owner| Some(owner) != self.live_stroke) && !held(page.coordinate)) {
                offer((layer.id, page.coordinate, Offered::coverage(page)), page.primary.storage_bytes() + page.secondary.storage_bytes(), 0);
            }
        }
        leases
    }
    fn take_evicted_color(&mut self, mut leases: std::collections::HashMap<(SourceTarget, [u32; 2], Offered), Arc<crate::residency::Lease>>) -> (Vec<PageSurface>, Vec<Arc<crate::residency::Lease>>) {
        let (mut retired, mut evicted) = (Vec::new(), Vec::new());
        let mut take = |key| leases.remove(&key).filter(|lease: &Arc<crate::residency::Lease>| lease.evicted()).map(|lease| evicted.push(lease)).is_some();
        for layer in &mut self.paint_layers {
            let mut index = 0;
            while index < layer.pages.len() {
                let coordinate = layer.pages[index].coordinate;
                if take((layer.id, coordinate, Offered::Color)) {
                    for coverage in layer.coverage_pages.iter().filter(|p| p.coordinate == coordinate) { coverage.release_color_bindings(); }
                    let page = layer.pages.swap_remove(index);
                    retired.push(page.primary.retire());retired.extend(page.secondary.map(resident::Resident::retire));
                    continue;
                }
                let page = &mut layer.pages[index];
                if take((layer.id, coordinate, Offered::Spare)) && let Some(mut spare) = page.secondary.take() {
                    if page.active_secondary {
                        std::mem::swap(&mut page.primary, &mut spare);
                        page.primary_needs_clear = false;
                    }
                    page.active_secondary = false;
                    retired.push(spare.retire());
                    for coverage in layer.coverage_pages.iter().filter(|p| p.coordinate == coordinate) { coverage.release_color_bindings(); }
                }
                index += 1;
            }
            let mut index = 0;
            while index < layer.coverage_pages.len() {
                let page = &layer.coverage_pages[index];
                if !take((layer.id, page.coordinate, Offered::coverage(page))) { index += 1; continue; }
                for color in layer.pages.iter().filter(|color| color.coordinate == page.coordinate) {
                    color.primary.clear_bindings();
                    if let Some(spare) = &color.secondary { spare.clear_bindings(); }
                }
                let page = layer.coverage_pages.swap_remove(index);
                retired.push(page.primary.retire());retired.push(page.secondary.retire());
            }
        }
        (retired, evicted)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Offered { Color, Spare, Coverage(u32, wgpu::TextureFormat) }
impl Offered {
    fn coverage(page: &StrokeCoveragePage) -> Self { Self::Coverage(page.primary.size().height, page.primary.format()) }
}

fn color_page_bytes(page: &LayerPage) -> u64 {
    page.primary.storage_bytes()
        + page.stroke_start.as_ref().map_or(0,StrokeStart::storage_bytes)
        + page
            .secondary
            .as_ref()
            .map_or(0, |surface|surface.storage_bytes())
}
