use super::*;
use std::collections::{BTreeMap, HashMap};
use layer_core::raster::{RasterData, RasterPlane};

fn missing_pages(required: &Damage, bounds: PixelRect, valid: &BTreeSet<[u32; 2]>, covered: PixelRect) -> Vec<[u32; 2]> {
    required.regions.iter().flat_map(|region| page_coordinates(region.intersect(bounds)))
        .filter(|coordinate| !valid.contains(coordinate) && page_rect(*coordinate).intersect(covered).is_empty())
        .collect::<BTreeSet<_>>().into_iter().collect()
}

struct PageInputs {
    source: wgpu::TextureView,
    base: wgpu::TextureView,
    level: u32,
    over: bool,
    empty: bool,
}

pub(super) struct Level {
    pub image: Image,
    pub valid: BTreeSet<[u32; 2]>,
    stale: BTreeMap<[u32; 2], PixelRect>,
    blend_space: layer_core::BlendSpace,
    watercolor: Option<WatercolorLayerStyle>,
}
pub(super) struct Source {
    pub extent: [u32; 2],
    /// How far right and down the levels hold the source's pixels.
    pub phase: [u32; 2],
    /// The document's pixels in the levels, whose texels at the source's
    /// edges also average the transparent pixels around it, unless the
    /// source is moving and its levels are resampled.
    window: Option<[u32; 2]>,
    pub updates: u64,
    pub blend_space: layer_core::BlendSpace,
    raster: u64,
    pub(super) watercolor: Option<WatercolorLayerStyle>,
    raw_material: bool,
    pub levels: BTreeMap<u32, Level>,
    source: Option<Arc<layer_core::color::source::SourceImage>>,
    base: Option<([u32; 2], layer_core::authored::PaintBasePolicy)>,
    backing: Option<Arc<RasterData>>,
    mask: Option<metadata::MaskMetadata>,
    preview: Damage,
    pub(super) damage: Damage,
}

impl Source {
    pub(super) fn levels_extent(&self) -> [u32; 2] { [self.extent[0] + self.phase[0], self.extent[1] + self.phase[1]] }
    /// The pixels of the levels that texels of `level` average: those in the
    /// document, and around paint, the transparent rest of its edge texels.
    fn valid_extent(&self, level: u32) -> [u32; 2] {
        let side = 1 << level;
        let extent = self.levels_extent();
        let Some(window) = self.window else { return extent; };
        std::array::from_fn(|i| if self.mask.is_some() { extent[i] } else { extent[i].div_ceil(side) * side }.min(window[i]))
    }
    fn material(&self) -> Option<WatercolorLayerStyle> { if self.raw_material { None } else { self.watercolor } }
    pub(super) fn accepts(&self, level: &Level) -> bool { level.blend_space == self.blend_space && level.watercolor == self.material() }
    fn new_level(&self, r: &WgpuRasterizer, plan: display_mips::Plan) -> Level {
        Level { image: Image::new(r, plan, "composition source level"), valid: BTreeSet::new(), stale: BTreeMap::new(),
            blend_space: self.blend_space, watercolor: self.material() }
    }
    pub(super) fn derive_pages(
        &self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, plan: display_mips::Plan,
        output: &wgpu::TextureView, missing: &mut BTreeSet<[u32; 2]>,
    ) -> Result<(), GpuRasterError> {
        for (&finer, previous) in self.levels.range(..=plan.level).rev() {
            if previous.image.view == *output || !self.accepts(previous) { continue; }
            let valid = self.valid_extent(plan.level);
            let completed: Vec<_> = previous.valid.intersection(missing).copied().filter(|tile| {
                let region = page_rect(*tile).intersect(PixelRect::full(self.levels_extent()));
                region.intersect(previous.image.plan.bounds) == region && region.intersect(plan.bounds) == region
            }).collect();
            for changed in page_regions(completed.iter().copied(), plan.bounds.intersect(PixelRect::full(valid))) {
                let [x, y, width, height] = paint_transform::texel_rect(changed.window_local(plan.bounds), 1 << plan.level);
                let binding = Commands::binding(r, &previous.image.view, &r.empty_view, output);
                let bounds = previous.image.plan.bounds;
                let mut values = [0; 20];
                values[..8].copy_from_slice(&[x, y, width, height, valid[0].min(bounds.max_x()).saturating_sub(bounds.min_x()),
                    valid[1].min(bounds.max_y()).saturating_sub(bounds.min_y()), 1 << (plan.level - finer), (finer << 8) | 8]);
                values[14] = ((previous.image.plan.bounds.min_x() as f32 - plan.bounds.min_x() as f32) / (1 << finer) as f32).to_bits();
                values[15] = ((previous.image.plan.bounds.min_y() as f32 - plan.bounds.min_y() as f32) / (1 << finer) as f32).to_bits();
                commands.reduce(r, encoder, values, &binding, "derive source level")?;
            }
            for tile in completed { missing.remove(&tile); }
            if missing.is_empty() { break; }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Sources {
    pub(super) entries: HashMap<SourceTarget, Source>,
    pub(super) reset: bool,
    pub(super) object_moving: Option<OccurrenceHandle>,
    pub(in crate::scene) object_damage: BTreeMap<Option<OccurrenceHandle>, Damage>,
    pub(in crate::scene) object_refinements: BTreeMap<super::super::object_cache::DamageKey, Damage>,
}
impl Sources {
    pub(crate) fn published(&mut self, id: SourceTarget, data: Arc<RasterData>) {
        if let Some(source) = self.entries.get_mut(&id) { source.backing = Some(data); }
    }
    /// The document pixels a source's latest changes cover.
    pub fn document_damage(&self, scene: SceneView<'_>, id: SourceTarget) -> Option<impl Iterator<Item = DocRect> + '_> {
        let source = self.entries.get(&id)?;
        let frame = SourceFrame::at(scene.target_offset(id), source.phase);
        Some(source.damage.regions.iter().map(move |region| frame.document(*region)))
    }
    #[cfg(test)]
    pub fn cache_info(&self, id: SourceTarget) -> Option<(wgpu::Texture, u64, u32)> {
        self.cache_info_at(id, *self.entries.get(&id)?.levels.first_key_value()?.0)
    }
    #[cfg(test)]
    pub fn cache_info_at(&self, id: SourceTarget, level: u32) -> Option<(wgpu::Texture, u64, u32)> {
        let source = self.entries.get(&id)?;
        let image = source.levels.get(&level)?;
        Some((image.image.texture.clone(), source.updates, level))
    }
    pub fn storage_bytes(&self) -> u64 {
        self.entries.values().flat_map(|s| s.levels.values()).map(|l| texture_bytes(&l.image.texture)).sum::<u64>()
            + self.object_refinements.values().chain(self.object_damage.values()).map(|damage| std::mem::size_of::<(super::super::object_cache::DamageKey, Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>()
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, batch_tiles: &[Vec<brush_tiles::BrushTile>]) {
        let mut wanted = BTreeSet::new();
        self.reset = false;
        self.object_moving = r.moving_layer;
        for &handle in packet.scene.order() {
            let occurrence = packet.scene.occurrence(handle).unwrap();
            let visible = packet.scene.visible(handle);
            if visible && occurrence.kind() == LayerKind::Paint && occurrence.is_artwork() && stack::has_content(r, packet.scene, handle) {
                let id = packet.scene.source_target(handle).unwrap();
                if wanted.insert(id) { self.update(r, packet, handle, false, batch_tiles); }
                let source = self.entries.get_mut(&id).unwrap();
                source.raster = packet.scene.paint_source(handle).unwrap().raster.identity();
                let watercolor = r.watercolor_style(id, packet.dab_batches);
                if source.watercolor != watercolor {
                    source.watercolor = watercolor; self.reset = true; source.damage = PixelRect::full(source.levels_extent()).into();
                }
            }
            if let Some((mask, _)) = packet.scene.mask(handle).filter(|(mask, _)| mask.enabled && (visible || packet.inspect_mask == Some(handle)))
                && wanted.insert(SourceTarget::Coverage(mask.source)) { self.update(r, packet, handle, true, batch_tiles); }
        }
        self.entries.retain(|id, _| wanted.contains(id));
    }
    fn update(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle, is_mask: bool, batch_tiles: &[Vec<brush_tiles::BrushTile>]) {
        let scene = packet.scene;
        let (id, plane, image, mask) = if is_mask {
            (SourceTarget::Coverage(scene.mask(handle).unwrap().0.source), RasterPlane::Mask, None, metadata::mask_metadata(scene, handle))
        } else { (scene.source_target(handle).unwrap(), RasterPlane::Color, scene.paint_source(handle).unwrap().base.as_ref().map(|base| base.image.storage().clone()), None) };
        let base = scene.paint_source(handle).filter(|_| !is_mask).and_then(|paint| paint.base.as_ref()).map(|base| (base.offset, base.policy));
        let extent = scene.target_extent(id);
        let raw_material = !is_mask && mapped_material(r, packet, id);
        let blend_space = if is_mask || r.moving_layer == Some(handle) || raw_material {
            layer_core::BlendSpace::Linear
        } else { packet.blend_space };
        let origin = scene.target_offset(id);
        let settled = if raw_material { [0; 2] } else { crate::scene::page_phase(origin) };
        let window = |phase: [u32; 2]| Some(std::array::from_fn(|i| (i64::from(packet.document_extent[i]) - origin[i] + i64::from(phase[i])).clamp(0, i64::from(u32::MAX)) as u32));
        let moving = r.moving_layer.is_some_and(|layer| std::iter::successors(Some(handle), |h| scene.parent(*h)).any(|h| h == layer));
        self.reset |= !self.entries.contains_key(&id);
        let source = self.entries.entry(id).or_insert_with(|| Source {
            extent, phase: settled, window: window(settled), updates: 0, blend_space, raster: 0, watercolor: None, raw_material, levels: BTreeMap::new(), source: None, base: None, backing: None, mask: None, preview: Damage::EMPTY, damage: Damage::EMPTY,
        });
        let (phase, window) = if raw_material { ([0; 2], None) } else if moving { (source.phase, None) } else { (settled, window(settled)) };
        let resized = source.extent != extent || source.phase != phase;
        if resized { source.extent = extent; source.phase = phase; source.window = window; source.levels.clear(); self.reset = true; }
        let mut regrown = Vec::new();
        if source.window != window {
            let before: Vec<_> = source.levels.keys().map(|&level| (level, source.valid_extent(level))).collect();
            source.window = window;
            for (level, old) in before {
                let new = source.valid_extent(level);
                let kept = PixelRect::full([old[0].min(new[0]), old[1].min(new[1])]);
                let reach = PixelRect::full([old[0].max(new[0]), old[1].max(new[1])]);
                let target = source.levels.get_mut(&level).unwrap();
                let stays = |c: &[u32; 2]| page_rect(*c).intersect(reach) == page_rect(*c).intersect(kept);
                target.stale.retain(|c, _| stays(c));
                target.valid.retain(|c| {
                    if !stays(c) { regrown.push(page_rect(*c)); }
                    stays(c)
                });
            }
        }
        let same_image = match (&source.source, &image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false,
        };
        let reset = resized || packet.reset_layers || !same_image || source.mask != mask || source.base != base;
        let changed = reset || source.blend_space != blend_space || source.raw_material != raw_material;
        source.raw_material = raw_material;
        source.blend_space = blend_space;
        if reset {
            for level in source.levels.values_mut() { level.valid.clear(); level.stale.clear(); }
        }
        self.reset |= changed;
        source.source = image;
        source.base = base;
        source.mask = mask;
        let backing = r.native_backing(id).cloned();
        let same_backing = match (&source.backing, &backing) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false,
        };
        let mut damage = std::mem::take(&mut source.preview);
        let touched: BTreeSet<_> = packet.dab_batches.iter().zip(batch_tiles).filter(|(b, _)| b.target == id
            && !packet.restore_rasters.iter().any(|(target, _)| *target == id))
            .flat_map(|(_, tiles)| tiles.iter().map(|t| t.coordinate)).collect();
        if !same_backing {
            for key in source.backing.iter().chain(backing.iter()).flat_map(|b| b.tiles.keys()).filter(|k| k.plane == plane) {
                let before = source.backing.as_ref().and_then(|b| b.tiles.get(key));
                let after = backing.as_ref().and_then(|b| b.tiles.get(key));
                if !touched.contains(&key.coordinate) && !matches!((before, after), (Some(a), Some(b)) if a.same_capture(b)) {
                    damage.regions.push(page_rect(key.coordinate));
                }
            }
        }
        source.backing = backing;
        for (batch, tiles) in packet.dab_batches.iter().zip(batch_tiles).filter(|(b, _)| b.target == id) {
            let regions = tiles.iter().map(Damage::tile_region);
            damage.regions.extend(regions.clone());
            damage.regions.extend(r.stroke_finish_pages(batch).map(page_rect));
            if batch.kind == DabBatchKind::Preview { source.preview.regions.extend(regions); }
        }
        if r.preview_layer_id == Some(id) {
            if source.preview.is_empty() {
                source.preview = Damage::from_tiles(r.preview_damage, r.preview_contact_tiles.as_ref());
            }
        }
        for &(target, region) in &r.transform_damage {
            if target == id { damage.regions.push(region); }
        }
        let radius = source.watercolor.map_or(0, |w| w.radius());
        if radius > 0 { damage = damage.expand(radius, extent); }
        let levels = PixelRect::full(source.levels_extent());
        source.damage = if changed { levels.into() } else {
            damage.intersect(PixelRect::full(extent)).map(|region| region.translated(phase)).union(Damage::from_regions(regrown)).intersect(levels)
        };
        for level in source.levels.values_mut() {
            let damaged = |c: [u32; 2]| source.damage.regions.iter().fold(PixelRect::EMPTY, |rect, region| rect.union(region.intersect(page_rect(c))));
            for (c, rect) in level.stale.iter_mut() { *rect = rect.union(damaged(*c)); }
            let stale = &mut level.stale;
            level.valid.retain(|c| {
                let rect = damaged(*c);
                if !rect.is_empty() { stale.insert(*c, rect); }
                rect.is_empty()
            });
        }
    }
    pub fn complete_texture(&self, scene: SceneView<'_>, target: SourceTarget, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        let SourceTarget::Paint(paint) = target else { return None; };
        let paint = scene.paint(paint)?;
        let source = self.entries.get(&target)?;
        let current = source.blend_space == layer_core::BlendSpace::Linear && source.extent == extent && source.raster == paint.raster.identity() && source.preview.is_empty()
            && source.base == paint.base.as_ref().map(|base| (base.offset, base.policy))
            && match (&source.source, paint.base.as_ref().map(|base| base.image.storage())) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        let image = source.levels.get(&level)?;
        let levels = PixelRect::full(source.levels_extent());
        let whole = source.valid_extent(level).into_iter().zip(source.levels_extent()).all(|(valid, extent)| valid >= extent);
        (current && whole && source.phase == crate::scene::page_phase(scene.target_offset(target)) && source.accepts(image)
            && image.watercolor.is_none() && image.image.plan.bounds == levels
            && page_coordinates(levels).all(|c| image.valid.contains(&c))).then_some(&image.image.texture)
    }
    pub(super) fn image(&self, id: SourceTarget, level: u32) -> &Level { &self.entries[&id].levels[&level] }
    pub(super) fn resident_plan(&self, id: SourceTarget, requested: display_mips::Plan) -> display_mips::Plan {
        self.entries.get(&id).and_then(|s| s.levels.get(&requested.level)).map(|l| l.image.plan)
            .filter(|p| p.extent == requested.extent && !requested.bounds.is_empty())
            .map(|p| display_mips::Plan::window(p.extent, p.level, p.bounds.union(requested.bounds)))
            .filter(|p| p.level_bytes(p.level) <= requested.level_bytes(requested.level).saturating_mul(2))
            .unwrap_or(requested)
    }
    pub(super) fn ensure_level(&mut self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, id: SourceTarget, plan: display_mips::Plan,
    ) -> Result<(), GpuRasterError> {
        let source = self.entries.get_mut(&id).unwrap();
        let level = plan.level;
        let mut target = source.levels.remove(&level).unwrap_or_else(|| source.new_level(r, plan));
        if !source.accepts(&target) {
            target.valid.clear();
            target.stale.clear();
            target.blend_space = source.blend_space;
            target.watercolor = source.material();
        }
        if target.image.plan != plan {
            let previous = target;
            target = source.new_level(r, plan);
            let overlap = previous.image.plan.bounds.intersect(plan.bounds);
            if !overlap.is_empty() {
                let [sx, sy, width, height] = paint_transform::texel_rect(overlap.window_local(previous.image.plan.bounds), 1 << level);
                let [dx, dy, _, _] = paint_transform::texel_rect(overlap.window_local(plan.bounds), 1 << level);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: sx, y: sy, z: 0 }, ..previous.image.texture.as_image_copy() },
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: dx, y: dy, z: 0 }, ..target.image.texture.as_image_copy() },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
                target.valid.extend(previous.valid.into_iter().filter(|c| {
                    let region = page_rect(*c).intersect(PixelRect::full(plan.extent));
                    region.intersect(overlap) == region
                }));
            }
        }
        let mut missing: BTreeSet<_> = page_coordinates(plan.bounds).filter(|c| !target.valid.contains(c)).collect();
        source.derive_pages(commands, r, encoder, plan, &target.image.view, &mut missing)?;
        target.valid.extend(page_coordinates(plan.bounds).filter(|c| !missing.contains(c)));
        let valid = &target.valid;
        target.stale.retain(|c, _| !valid.contains(c));
        source.levels.insert(level, target);
        Ok(())
    }
    pub fn retain_levels(&mut self, requested: &BTreeMap<SourceTarget, BTreeMap<u32, display_mips::Plan>>, budget: u64) -> u64 {
        let mut reserve = 0;
        let empty = BTreeMap::new();
        for (id, source) in &mut self.entries {
            let levels = requested.get(id).unwrap_or(&empty);
            let finer = levels.first_key_value().and_then(|(requested, _)| source.levels.range(..requested).next_back()).or_else(|| levels.is_empty().then(|| source.levels.first_key_value()).flatten()).map(|(&level, _)| level);
            source.levels.retain(|level, _| levels.contains_key(level) || Some(*level) == finer);
            reserve += levels.iter().map(|(level, plan)| plan.level_bytes(*level)
                .saturating_sub(source.levels.get(level).map_or(0, |l| l.image.bytes()))).sum::<u64>();
        }
        let mut optional: Vec<_> = self.entries.iter().flat_map(|(id, source)| source.levels.iter()
            .filter(|(level, _)| !requested.get(id).is_some_and(|levels| levels.contains_key(level))).map(|(level, image)| (texture_bytes(&image.image.texture), *id, *level))).collect();
        optional.sort_unstable_by_key(|(bytes, _, _)| std::cmp::Reverse(*bytes));
        let mut bytes = self.storage_bytes() + reserve;
        for (size, id, level) in optional {
            if bytes <= budget { break; }
            self.entries.get_mut(&id).unwrap().levels.remove(&level);
            bytes -= size;
        }
        bytes
    }
}

impl Scene {
    pub(in crate::scene) fn prepare_display_sources(
        &mut self, cache: &Cache, commands: &mut Commands, r: &mut WgpuRasterizer,
        packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let requested = cache.source_levels(r, packet, self);
        let budget = cache.source_budget(r, packet, commands, Some(self));
        let reserved = self.scale_sources.retain_levels(&requested, budget);
        let mut remaining = budget.saturating_sub(reserved);
        for (handle, id) in targets(r, packet).collect::<Vec<_>>() {
            if cache.streamed_sources && source_frame(Some(&self.scale_sources), packet.scene, id).aligned() { continue; }
            let Some((_, required)) = requested.get(&id).and_then(|levels| levels.first_key_value()) else { continue; };
            let source = &self.scale_sources.entries[&id];
            let existing = source.levels.range(..required.level).rev().find(|(_, level)| source.accepts(level));
            let cold = source.levels.range(..=required.level).next().is_none()
                && !packet.dab_batches.iter().any(|batch| batch.target == id);
            let Some(level) = existing.map(|(&level, _)| level)
                .or_else(|| (cold && required.level > 1).then(|| required.level - 1)) else { continue; };
            if r.preview_layer_id == Some(id) && r.preview_level > level { continue; }
            let plan = self.scale_sources.resident_plan(id,
                display_mips::Plan::window(required.extent, level, required.bounds));
            let previous = existing.map_or(0, |(_, image)| image.image.bytes());
            let bytes = plan.level_bytes(level);
            if bytes > previous + remaining || plan.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) { continue; }
            remaining = remaining + previous - bytes;
            let request = SourceRequest { plan, required: plan.bounds.into(), covered: PixelRect::EMPTY };
            if matches!(id, SourceTarget::Paint(_)) {
                self.prepare_scale_color(commands, r, packet, encoder, handle, request)?;
            } else {
                let (mask, source) = packet.scene.mask(handle).unwrap();
                self.prepare_scale_mask(commands, r, encoder, mask, source, request)?;
            }
        }
        Ok(())
    }

    pub(super) fn prepare_scale_color(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, handle: OccurrenceHandle, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { plan, required, covered } = request;
        let id = packet.scene.source_target(handle).unwrap();
        if plan.bounds.is_empty() { return Ok(PixelRect::EMPTY); }
        let level = plan.level;
        self.scale_sources.ensure_level(commands, r, encoder, id, plan)?;
        let cached = self.scale_sources.image(id, level);
        let output = cached.image.view.clone();
        let missing = missing_pages(&required, plan.bounds, &cached.valid, covered);
        let stale = cached.stale.clone();
        let changed = self.reduce_color_pages(commands, r, packet, encoder, handle, plan, &output, &missing, None, [0; 2], &stale)?;
        let target = self.scale_sources.entries.get_mut(&id).unwrap().levels.get_mut(&level).unwrap();
        for page in &missing { target.stale.remove(page); }
        target.valid.extend(missing);
        Ok(changed)
    }
    /// Reduce `missing` pages of a layer's levels, or of `placement` of it,
    /// into `output`, whose `plan` lies `shift` pixels before the pages.
    #[expect(clippy::too_many_arguments, reason = "Color reduction keeps source placement, missing pages, and GPU output bindings explicit")]
    pub(super) fn reduce_color_pages(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, handle: OccurrenceHandle, plan: display_mips::Plan,
        output: &wgpu::TextureView, missing: &[[u32; 2]], placement: Option<[i64; 2]>, shift: [i64; 2],
        stale: &BTreeMap<[u32; 2], PixelRect>,
    ) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let id = packet.scene.source_target(handle).unwrap();
        let phase = if placement.is_some() { [0; 2] } else { self.scale_sources.entries[&id].phase };
        let extent = if placement.is_some() { packet.document_extent } else { self.scale_sources.entries[&id].valid_extent(level) };
        let blend_space = if placement.is_some() { packet.blend_space } else { self.scale_sources.entries[&id].blend_space };
        let phased = phased_packet(packet, id, phase);
        let mut changed = PixelRect::EMPTY;
        for chunk in missing.chunks(32) {
            for &tile in chunk {
                if placement.is_none() && phase != [0; 2] && r.watercolor_style(id, packet.dab_batches).is_none()
                    && let Some((region, reads)) = self.reduce_shifted_page(r, packet.scene, id, plan, output, tile, phase, shift, extent, blend_space, stale.get(&tile).copied())? {
                    changed = changed.union(region);
                    r.metrics.display_reduction_reads += reads;
                    continue;
                }
                let entry = &self.scale_sources.entries[&id];
                let mut scratch = [None; 2];
                let (source, base, reduced_preview, over, empty) = if placement.is_some() || phase != [0; 2] || (entry.watercolor.is_some() && !entry.raw_material)
                    || (r.preview_contribution && r.preview_layer_id==Some(id)) {
                    let (color, flow, pigment) = match &placement {
                        Some(offset) => { let (page, pigment) = self.placed_material_inputs(r, packet, handle, *offset, tile)?; (page, None, pigment) }
                        None if phase != [0; 2] && r.watercolor_style(id, packet.dab_batches).is_some() =>
                            (self.paint_tile(r, phased, handle, tile)?, None, r.empty_view.clone()),
                        None if phase != [0; 2] => { let (color, flow) = self.placed_color(r, packet.scene, id, phase.map(i64::from), tile)?; (color, flow, r.empty_view.clone()) }
                        None => (self.local_color_tile(r, packet, handle, tile)?, None, r.empty_view.clone()),
                    };
                    scratch = [Some(color), flow];
                    match flow {
                        Some(flow) => (self.pool[flow].view.clone(), self.pool[color].view.clone(), false, true, false),
                        None => (self.pool[color].view.clone(), pigment, false, false, false),
                    }
                } else {
                    let inputs = self.page_inputs(r, packet.scene, id, tile)?;
                    (inputs.source, inputs.base, inputs.level > 0, inputs.over, inputs.empty)
                };
                let valid = page_rect(tile).intersect(PixelRect::full(extent));
                changed = changed.union(valid);
                let size =
                    [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
                let origin = output_origin(tile, shift, plan);
                let input_level = if reduced_preview { r.preview_level } else { 0 };
                let mut values = [0; 20];
                values[..8].copy_from_slice(&[
                    origin[0],
                    origin[1],
                    size[0],
                    size[1],
                    valid.width(),
                    valid.height(),
                    1 << (level - input_level),
                    u32::from(over) | if empty { 4 } else { 0 } | (input_level << 8)
                        | if placement.is_some() { 128 } else { 0 }
                        | if blend_space == layer_core::BlendSpace::Perceptual { 2 } else { 0 },
                ]);
                let reads = if values[7] & 4 != 0 { 0 } else { values[6].pow(2) * (1 + (values[7] & 1) + (values[7] >> 7 & 1)) };
                r.metrics.display_reduction_reads += u64::from(size[0] * size[1] * reads);
                self.jobs.push(Job::Reduce { binding: Commands::binding(r, &source, &base, output), values, size, kernel: Reduction::Pages });
                for slot in scratch.into_iter().flatten() { self.free(slot); }
            }
            commands.flush(r, encoder)?;
            self.encode_jobs(r, encoder)?;
            self.scale_sources.entries.get_mut(&id).unwrap().updates += chunk.len() as u64;
        }
        Ok(changed)
    }
    fn page_inputs(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, id: SourceTarget, c: [u32; 2]) -> Result<PageInputs, GpuRasterError> {
        let persistent = r.paint_layers.iter().find(|l| l.id == id)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == c)).map(|p| p.active().view.clone());
        let predicted = (r.preview_layer_id == Some(id)).then(|| r.preview_page(c)).flatten().map(|p| p.active().view.clone());
        let level = if predicted.is_some() { r.preview_level } else { 0 };
        let base = if predicted.is_some() && r.preview_requires_base { None }
            else if persistent.is_some() { persistent } else { self.source_tile(r, scene, id, c)? };
        let over = predicted.is_some() && !r.preview_requires_base;
        let empty = predicted.is_none() && base.is_none();
        let source = predicted.or_else(|| base.clone()).unwrap_or_else(|| r.empty_view.clone());
        Ok(PageInputs { source, base: base.unwrap_or_else(|| r.empty_view.clone()), level, over, empty })
    }
    /// Reduce level page `tile` of a layer whose levels lie `phase` pixels
    /// after its own pages, or of the page only the texels over `stale`.
    /// `None` while a stroke previews a flow that needs its base.
    #[expect(clippy::too_many_arguments, reason = "A shifted reduction keeps its plan, output, phase, frame and stale part explicit")]
    fn reduce_shifted_page(
        &mut self, r: &WgpuRasterizer, scene: SceneView<'_>, id: SourceTarget, plan: display_mips::Plan, output: &wgpu::TextureView,
        tile: [u32; 2], phase: [u32; 2], shift: [i64; 2], extent: [u32; 2], blend_space: layer_core::BlendSpace, stale: Option<PixelRect>,
    ) -> Result<Option<(PixelRect, u64)>, GpuRasterError> {
        let side = 1u32 << plan.level;
        let valid = page_rect(tile).intersect(PixelRect::full(extent));
        let size = [valid.width(), valid.height()];
        let count = size.map(|n| n.div_ceil(side));
        let part = stale.map_or(valid, |stale| stale.intersect(valid));
        if part.is_empty() { return Ok(Some((PixelRect::EMPTY, 0))); }
        let low: [u32; 2] = std::array::from_fn(|axis| ([part.min_x(), part.min_y()][axis] - tile[axis] * PAGE_SIZE) / side);
        let high: [u32; 2] = std::array::from_fn(|axis| ([part.max_x(), part.max_y()][axis] - tile[axis] * PAGE_SIZE).div_ceil(side));
        let pages = scene.target_extent(id).map(|n| n.div_ceil(PAGE_SIZE));
        let mut inputs: [Option<PageInputs>; 4] = Default::default();
        for (index, input) in inputs.iter_mut().enumerate() {
            let at = [(index % 2) as u32, (index / 2) as u32];
            let [Some(x), Some(y)] = std::array::from_fn(|i| (tile[i] + at[i]).checked_sub(1).filter(|c| *c < pages[i])) else { continue; };
            *input = Some(self.page_inputs(r, scene, id, [x, y])?);
        }
        if inputs.iter().flatten().any(|input| input.over) { return Ok(None); }
        let reduced = inputs.iter().flatten().filter(|input| !input.empty).map(|input| input.level).max().unwrap_or(0);
        let units = |axis: usize| {
            let mut halves = [0u64; 2];
            for texel in low[axis]..high[axis] {
                let length = side.min(size[axis] - texel * side);
                let cell = if length == side { 1 << reduced } else { 1 };
                let snapped = (PAGE_SIZE - phase[axis] + texel * side + cell / 2) & !(cell - 1);
                for unit in (snapped..snapped + length).step_by(cell as usize) { halves[(unit / PAGE_SIZE) as usize] += 1; }
            }
            halves
        };
        let [x, y] = [units(0), units(1)];
        let reads = (0..4).filter(|&page| inputs[page].as_ref().is_some_and(|input| !input.empty)).map(|page| x[page % 2] * y[page / 2]).sum();
        let origin = output_origin(tile, shift, plan);
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[origin[0], origin[1], count[0], count[1], size[0], size[1], side,
            if blend_space == layer_core::BlendSpace::Perceptual { 2 } else { 0 }]);
        for (word, input) in values[8..12].iter_mut().zip(&inputs) { *word = input.as_ref().filter(|input| !input.empty).map_or(0, |input| 16 | input.level); }
        values[12..16].copy_from_slice(&[phase[0], phase[1], low[0] | low[1] << 16, high[0] | high[1] << 16]);
        let views: [&wgpu::TextureView; 4] = std::array::from_fn(|i| inputs[i].as_ref().map_or(&r.empty_view, |input| &input.source));
        self.jobs.push(Job::Reduce { binding: Commands::inputs(r, views, output), values, size: [high[0] - low[0], high[1] - low[1]], kernel: Reduction::Phased });
        Ok(Some((part, reads)))
    }
    pub(super) fn prepare_scale_mask(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        mask: &MaskUse, coverage: &CoverageSource, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { plan, required, covered } = request;
        if plan.bounds.is_empty() { return Ok(PixelRect::EMPTY); }
        let level = plan.level;
        self.scale_sources.ensure_level(commands, r, encoder, SourceTarget::Coverage(mask.source), plan)?;
        let cached = self.scale_sources.image(SourceTarget::Coverage(mask.source), level);
        let output = cached.image.view.clone();
        let missing = missing_pages(&required, plan.bounds, &cached.valid, covered);
        let changed = self.reduce_mask_pages(commands, r, encoder, mask, coverage, plan, &output, &missing, [0; 2])?;
        self.scale_sources.entries.get_mut(&SourceTarget::Coverage(mask.source)).unwrap().levels.get_mut(&level).unwrap().valid.extend(missing);
        Ok(changed)
    }
    /// Reduce `missing` pages of a mask's levels into `output`, whose `plan`
    /// lies `shift` pixels before the pages.
    #[expect(clippy::too_many_arguments, reason = "Mask reduction keeps missing pages and GPU output bindings explicit")]
    pub(super) fn reduce_mask_pages(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, mask: &MaskUse, coverage: &CoverageSource,
        plan: display_mips::Plan, output: &wgpu::TextureView, missing: &[[u32; 2]], shift: [i64; 2],
    ) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let id = SourceTarget::Coverage(mask.source);
        let entry = &self.scale_sources.entries[&id];
        let (extent, phase) = (entry.valid_extent(level), entry.phase);
        let default = if mask.inverted { 1. - coverage.default_coverage } else { coverage.default_coverage };
        let mut changed = PixelRect::EMPTY;
        for &tile in missing {
            let scratch = (phase != [0; 2]).then(|| self.mask_at(r, mask, coverage, phase.map(i64::from), tile));
            let source = match scratch {
                Some(slot) => Some(self.pool[slot].view.clone()),
                None => r.layer_masks.pages.get(&(id, tile)).map(|p| p.view.clone()),
            };
            let valid = page_rect(tile).intersect(PixelRect::full(extent));
            changed = changed.union(valid);
            let size = [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
            let origin = output_origin(tile, shift, plan);
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[
                origin[0], origin[1], size[0], size[1], valid.width(), valid.height(), 1 << level,
                match (&source, scratch) { (None, _) => 4, (Some(_), Some(_)) => 0, (Some(_), None) => 32 | if mask.inverted { 64 } else { 0 } },
            ]);
            values[8..12].fill(default.to_bits());
            if let Some(slot) = scratch {
                self.encode_jobs(r, encoder)?;
                let binding = Commands::binding(r, &self.pool[slot].view, &r.empty_view, output);
                commands.reduce(r, encoder, values, &binding, "reduce changed mask pages")?;
                self.free(slot);
                continue;
            }
            let binding = Commands::binding(r, source.as_ref().unwrap_or(&r.empty_view), &r.empty_view, output);
            commands.reduce(r, encoder, values, &binding, "reduce changed mask pages")?;
        }
        Ok(changed)
    }
}

/// The texel of `plan` where page `tile` of levels lying `shift` pixels
/// after the plan begins.
fn output_origin(tile: [u32; 2], shift: [i64; 2], plan: display_mips::Plan) -> [u32; 2] {
    std::array::from_fn(|i| ((i64::from(tile[i] * PAGE_SIZE) + shift[i] - i64::from([plan.bounds.min_x(), plan.bounds.min_y()][i])) >> plan.level) as u32)
}

/// `packet` with its frame moved so `id`'s pixel (0, 0) lies `phase` pixels
/// right and down of the origin.
fn phased_packet(packet: FramePacket<'_>, id: SourceTarget, phase: [u32; 2]) -> FramePacket<'_> {
    let origin = packet.scene.target_offset(id);
    FramePacket { scene: packet.scene.with_offset64(std::array::from_fn(|i| (i64::from(phase[i]) - origin[i]) as f64)), ..packet }
}

impl Scene {
    pub fn reduced_layer(&self, _r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        self.scale_sources.complete_texture(scene, target, extent, level)
    }

}
