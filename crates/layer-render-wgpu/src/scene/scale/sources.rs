use super::*;
use std::collections::{BTreeMap, HashMap};
use layer_core::raster::{RasterData, RasterPlane};

pub(super) struct Level {
    pub image: Image,
    pub valid: BTreeSet<[u32; 2]>,
    blend_space: layer_core::BlendSpace,
    watercolor: Option<WatercolorLayerStyle>,
}
pub(super) struct Source {
    pub extent: [u32; 2],
    pub updates: u64,
    pub blend_space: layer_core::BlendSpace,
    raster: u64,
    pub(super) watercolor: Option<WatercolorLayerStyle>,
    raw_material: bool,
    pub levels: BTreeMap<u32, Level>,
    source: Option<Arc<layer_core::color::source::SourceImage>>,
    backing: Option<Arc<RasterData>>,
    mask: Option<layer_core::LayerMask>,
    preview: BTreeSet<[u32; 2]>,
    pub(super) damage: PixelRect,
}

impl Source {
    fn material(&self) -> Option<WatercolorLayerStyle> { if self.raw_material { None } else { self.watercolor } }
    pub(super) fn accepts(&self, level: &Level) -> bool { level.blend_space == self.blend_space && level.watercolor == self.material() }
    fn new_level(&self, r: &WgpuRasterizer, plan: display_mips::Plan) -> Level {
        Level { image: Image::new(r, plan, "composition source level"), valid: BTreeSet::new(),
            blend_space: self.blend_space, watercolor: self.material() }
    }
    pub(super) fn derive_pages(
        &self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, plan: display_mips::Plan,
        output: &wgpu::TextureView, missing: &mut BTreeSet<[u32; 2]>,
    ) -> Result<(), GpuRasterError> {
        for (&finer, previous) in self.levels.range(..=plan.level).rev() {
            if previous.image.view == *output || !self.accepts(previous) { continue; }
            let completed: Vec<_> = previous.valid.intersection(missing).copied().filter(|tile| {
                let region = page_rect(*tile).intersect(PixelRect::full(self.extent));
                region.intersect(previous.image.plan.bounds) == region && region.intersect(plan.bounds) == region
            }).collect();
            for changed in page_regions(completed.iter().copied(), plan.bounds) {
                let [x, y, width, height] = paint_transform::texel_rect(changed.window_local(plan.bounds), 1 << plan.level);
                let binding = Commands::binding(r, &previous.image.view, &r.empty_view, output);
                let mut values = [0; 20];
                values[..8].copy_from_slice(&[x, y, width, height, previous.image.plan.bounds.width(), previous.image.plan.bounds.height(),
                    1 << (plan.level - finer), (finer << 8) | 8]);
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
    pub(super) entries: HashMap<LayerId, Source>,
    pub(super) reset: bool,
}
impl Sources {
    #[cfg(test)]
    pub fn cache_info(&self, id: LayerId) -> Option<(wgpu::Texture, u64, u32)> {
        self.cache_info_at(id, *self.entries.get(&id)?.levels.first_key_value()?.0)
    }
    #[cfg(test)]
    pub fn cache_info_at(&self, id: LayerId, level: u32) -> Option<(wgpu::Texture, u64, u32)> {
        let source = self.entries.get(&id)?;
        let image = source.levels.get(&level)?;
        Some((image.image.texture.clone(), source.updates, level))
    }
    pub fn storage_bytes(&self) -> u64 {
        self.entries.values().flat_map(|s| s.levels.values()).map(|l| texture_bytes(&l.image.texture)).sum()
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, batch_tiles: &[Vec<brush_tiles::BrushTile>]) {
        let mut wanted = BTreeSet::new();
        self.reset = false;
        for layer in packet.layers {
            let visible = images::visible(packet.layers, layer);
            if visible && layer.kind == LayerKind::Paint && layer.is_artwork() && stack::has_content(r, layer) {
                wanted.insert(layer.id);
                self.update(r, packet, layer, false, batch_tiles);
                let source = self.entries.get_mut(&layer.id).unwrap();
                source.raster = layer.raster.identity();
                let watercolor = r.paint_layers.iter().find(|l| l.id == layer.id).and_then(|l| l.watercolor);
                if source.watercolor != watercolor {
                    source.watercolor = watercolor;
                    self.reset = true;
                    source.damage = PixelRect::full(source.extent);
                }
            }
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (visible || m.show_area)) {
                wanted.insert(mask.id);
                self.update(r, packet, layer, true, batch_tiles);
            }
        }
        self.entries.retain(|id, _| wanted.contains(id));
    }
    fn update(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, layer: &Layer, is_mask: bool, batch_tiles: &[Vec<brush_tiles::BrushTile>]) {
        let extent = layer.local_extent(packet.document_extent);
        let (id, plane, image, mask) = if is_mask {
            (layer.mask.as_ref().unwrap().id, RasterPlane::Mask, None, metadata::mask_metadata(&layer.mask))
        } else { (layer.id, RasterPlane::Color, layer.source.clone(), None) };
        let blend_space = if is_mask || r.moving_layer == Some(id) || layer_core::target_transform(packet.layers, id) != layer_core::Affine::IDENTITY {
            layer_core::BlendSpace::Linear
        } else { packet.blend_space };
        let raw_material = !is_mask && mapped_material(r, packet, id);
        self.reset |= !self.entries.contains_key(&id);
        let source = self.entries.entry(id).or_insert_with(|| Source {
            extent, updates: 0, blend_space, raster: 0, watercolor: None, raw_material, levels: BTreeMap::new(), source: None, backing: None, mask: None, preview: BTreeSet::new(), damage: PixelRect::EMPTY,
        });
        let resized = source.extent != extent;
        if resized { source.extent = extent; source.levels.clear(); self.reset = true; }
        let same_image = match (&source.source, &image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false,
        };
        let reset = resized || packet.reset_layers || !same_image || source.mask != mask;
        let changed = reset || source.blend_space != blend_space || source.raw_material != raw_material;
        source.raw_material = raw_material;
        source.blend_space = blend_space;
        if reset {
            for level in source.levels.values_mut() { level.valid.clear(); }
        }
        self.reset |= changed;
        source.source = image;
        source.mask = mask;
        let backing = r.native_backing(id).cloned();
        let same_backing = match (&source.backing, &backing) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false,
        };
        let mut damage = std::mem::take(&mut source.preview);
        if !same_backing {
            for key in source.backing.iter().chain(backing.iter()).flat_map(|b| b.tiles.keys()).filter(|k| k.plane == plane) {
                let before = source.backing.as_ref().and_then(|b| b.tiles.get(key));
                let after = backing.as_ref().and_then(|b| b.tiles.get(key));
                if !matches!((before, after), (Some(a), Some(b)) if a.same_capture(b)) { damage.insert(key.coordinate); }
            }
        }
        source.backing = backing;
        for (batch, tiles) in packet.dab_batches.iter().zip(batch_tiles).filter(|(b, _)| b.layer_id == id) {
            let coordinates = tiles.iter().map(|t| t.coordinate);
            damage.extend(coordinates.clone());
            damage.extend(r.stroke_finish_pages(batch));
            if batch.kind == DabBatchKind::Preview { source.preview.extend(coordinates); }
        }
        if r.preview_layer_id == Some(id) {
            source.preview.extend(r.preview_contact_tiles.clone().unwrap_or_else(|| page_coordinates(r.preview_damage).collect()));
        }
        for &(target, region) in &r.transform_damage {
            if target == id { damage.extend(page_coordinates(region)); }
        }
        let radius = source.watercolor.map_or(0, |w| w.radius());
        if radius > 0 { damage = damage.into_iter().flat_map(|c| page_coordinates(page_rect(c).expand(radius, extent))).collect(); }
        source.damage = if changed { PixelRect::full(extent) } else { damage.iter().fold(PixelRect::EMPTY, |r, c| r.union(page_rect(*c).intersect(PixelRect::full(extent)))) };
        for level in source.levels.values_mut() { level.valid.retain(|c| !damage.contains(c)); }
    }
    pub fn sample(&self, id: LayerId, requested: u32) -> Option<(display_mips::Plan, &wgpu::TextureView)> {
        let source = self.entries.get(&id)?;
        if source.blend_space != layer_core::BlendSpace::Linear { return None; }
        let (_, image) = source.levels.range(..=requested).rev().find(|(_, level)| source.accepts(level))?;
        Some((image.image.plan, &image.image.view))
    }
    pub fn complete_texture(&self, layer: &Layer, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        let source = self.entries.get(&layer.id)?;
        let current = source.blend_space == layer_core::BlendSpace::Linear && source.extent == extent && source.raster == layer.raster.identity() && source.preview.is_empty()
            && match (&source.source, &layer.source) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        let image = source.levels.get(&level)?;
        (current && source.accepts(image) && image.watercolor.is_none() && image.image.plan.bounds == PixelRect::full(extent)
            && page_coordinates(PixelRect::full(extent)).all(|c| image.valid.contains(&c))).then_some(&image.image.texture)
    }
    pub(super) fn image(&self, id: LayerId, level: u32) -> &Level { &self.entries[&id].levels[&level] }
    pub(super) fn resident_plan(&self, id: LayerId, requested: display_mips::Plan) -> display_mips::Plan {
        self.entries.get(&id).and_then(|s| s.levels.get(&requested.level)).map(|l| l.image.plan)
            .filter(|p| p.extent == requested.extent && !requested.bounds.is_empty())
            .map(|p| display_mips::Plan::window(p.extent, p.level, p.bounds.union(requested.bounds)))
            .filter(|p| p.level_bytes(p.level) <= requested.level_bytes(requested.level).saturating_mul(2))
            .unwrap_or(requested)
    }
    pub(super) fn ensure_level(&mut self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, id: LayerId, plan: display_mips::Plan,
    ) -> Result<(), GpuRasterError> {
        let source = self.entries.get_mut(&id).unwrap();
        let level = plan.level;
        let mut target = source.levels.remove(&level).unwrap_or_else(|| source.new_level(r, plan));
        if !source.accepts(&target) {
            target.valid.clear();
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
        source.levels.insert(level, target);
        Ok(())
    }
    pub fn retain_levels(&mut self, requested: &BTreeMap<LayerId, BTreeMap<u32, display_mips::Plan>>, budget: u64) -> u64 {
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
        let requested = cache.source_levels(r, packet, &self.scale_sources);
        let budget = cache.source_budget(r, packet, commands, Some(&self.scale_sources));
        let reserved = self.scale_sources.retain_levels(&requested, budget);
        let mut remaining = budget.saturating_sub(reserved);
        for (layer, id) in targets(r, packet).collect::<Vec<_>>() {
            if cache.streamed_sources && layer_core::target_transform(packet.layers, id) == layer_core::Affine::IDENTITY { continue; }
            let Some((_, required)) = requested.get(&id).and_then(|levels| levels.first_key_value()) else { continue; };
            let source = &self.scale_sources.entries[&id];
            let existing = source.levels.range(..required.level).rev().find(|(_, level)| source.accepts(level));
            let cold = source.levels.range(..=required.level).next().is_none()
                && !packet.dab_batches.iter().any(|batch| batch.layer_id == id);
            let Some(level) = existing.map(|(&level, _)| level)
                .or_else(|| (cold && required.level > 1).then(|| required.level - 1)) else { continue; };
            if r.preview_layer_id == Some(id) && r.preview_level > level { continue; }
            let plan = self.scale_sources.resident_plan(id,
                display_mips::Plan::window(required.extent, level, required.bounds));
            let previous = existing.map_or(0, |(_, image)| image.image.bytes());
            let bytes = plan.level_bytes(level);
            if bytes > previous + remaining || plan.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) { continue; }
            remaining = remaining + previous - bytes;
            let request = SourceRequest { plan, required: plan.bounds, covered: PixelRect::EMPTY };
            if id == layer.id {
                self.prepare_scale_color(commands, r, packet, encoder, layer, request)?;
            } else {
                self.prepare_scale_mask(commands, r, encoder, layer.mask.as_ref().unwrap(), request)?;
            }
        }
        Ok(())
    }

    pub(super) fn prepare_scale_color(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, layer: &Layer, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { plan, required, covered } = request;
        if plan.bounds.is_empty() { return Ok(PixelRect::EMPTY); }
        let level = plan.level;
        self.scale_sources.ensure_level(commands, r, encoder, layer.id, plan)?;
        let cached = self.scale_sources.image(layer.id, level);
        let output = cached.image.view.clone();
        let missing: Vec<_> = page_coordinates(required.intersect(plan.bounds))
            .filter(|c| !cached.valid.contains(c) && page_rect(*c).intersect(covered).is_empty())
            .collect();
        let changed = self.reduce_color_pages(commands, r, packet, encoder, layer, plan, &output, &missing, None)?;
        self.scale_sources.entries.get_mut(&layer.id).unwrap().levels.get_mut(&level).unwrap().valid.extend(missing);
        Ok(changed)
    }
    pub(super) fn reduce_color_pages(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, layer: &Layer, plan: display_mips::Plan,
        output: &wgpu::TextureView, missing: &[[u32; 2]], placement: Option<layer_core::Affine>,
    ) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let extent = if placement.is_some() { packet.document_extent } else { layer.local_extent(packet.document_extent) };
        let blend_space = if placement.is_some() { packet.blend_space } else { self.scale_sources.entries[&layer.id].blend_space };
        let mut changed = PixelRect::EMPTY;
        for chunk in missing.chunks(32) {
            for &tile in chunk {
                let entry = &self.scale_sources.entries[&layer.id];
                let mut scratch = None;
                let (source, base, reduced_preview, over, empty) = if placement.is_some() || (entry.watercolor.is_some() && !entry.raw_material) {
                    let (tile, pigment) = match placement {
                        Some(placement) => self.placed_material_inputs(r, packet, layer, placement, tile)?,
                        None => (self.local_color_tile(r, packet, layer, tile)?, r.empty_view.clone()),
                    };
                    scratch = Some(tile);
                    (self.pool[tile].view.clone(), pigment, false, false, false)
                } else {
                    let persistent = r
                        .paint_layers
                        .iter()
                        .find(|l| l.id == layer.id)
                        .and_then(|l| l.pages.iter().find(|p| p.coordinate == tile))
                        .map(|p| p.active().view.clone());
                    let predicted = (r.preview_layer_id == Some(layer.id))
                        .then(|| r.preview_page(tile))
                        .flatten()
                        .map(|p| p.active().view.clone());
                    let reduced_preview = predicted.is_some() && r.preview_level > 0;
                    let base = if predicted.is_some() && r.preview_requires_base {
                        None
                    } else if persistent.is_some() {
                        persistent
                    } else {
                        self.source_tile(r, layer, tile)?
                    };
                    let over = predicted.is_some() && !r.preview_requires_base;
                    let empty = predicted.is_none() && base.is_none();
                    let source = predicted
                        .or_else(|| base.clone())
                        .unwrap_or_else(|| r.empty_view.clone());
                    let base = base.unwrap_or_else(|| r.empty_view.clone());
                    (source, base, reduced_preview, over, empty)
                };
                let valid = page_rect(tile).intersect(PixelRect::full(extent));
                changed = changed.union(valid);
                let size =
                    [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
                let origin = [(tile[0] * PAGE_SIZE - plan.bounds.min_x()) >> level, (tile[1] * PAGE_SIZE - plan.bounds.min_y()) >> level];
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
                self.jobs.push(Job::Reduce { binding: Commands::binding(r, &source, &base, output), values, size });
                if let Some(slot) = scratch { self.free(slot); }
            }
            commands.flush(r, encoder)?;
            self.encode_jobs(r, encoder)?;
            self.scale_sources.entries.get_mut(&layer.id).unwrap().updates += chunk.len() as u64;
        }
        Ok(changed)
    }
    pub(super) fn prepare_scale_mask(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        mask: &layer_core::LayerMask, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { plan, required, covered } = request;
        if plan.bounds.is_empty() { return Ok(PixelRect::EMPTY); }
        let level = plan.level;
        self.scale_sources.ensure_level(commands, r, encoder, mask.id, plan)?;
        let cached = self.scale_sources.image(mask.id, level);
        let output = cached.image.view.clone();
        let missing: Vec<_> = page_coordinates(required.intersect(plan.bounds)).filter(|c| !cached.valid.contains(c) && page_rect(*c).intersect(covered).is_empty()).collect();
        let changed = self.reduce_mask_pages(commands, r, encoder, mask, plan, &output, &missing)?;
        self.scale_sources.entries.get_mut(&mask.id).unwrap().levels.get_mut(&level).unwrap().valid.extend(missing);
        Ok(changed)
    }
    pub(super) fn reduce_mask_pages(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, mask: &layer_core::LayerMask,
        plan: display_mips::Plan, output: &wgpu::TextureView, missing: &[[u32; 2]],
    ) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let extent = self.scale_sources.entries[&mask.id].extent;
        let mut changed = PixelRect::EMPTY;
        for &tile in missing {
            let source = r.layer_masks.pages.get(&(mask.id, tile)).map(|p| p.view.clone());
            let valid = page_rect(tile).intersect(PixelRect::full(extent));
            changed = changed.union(valid);
            let size = [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
            let origin = [(tile[0] * PAGE_SIZE - plan.bounds.min_x()) >> level, (tile[1] * PAGE_SIZE - plan.bounds.min_y()) >> level];
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[
                origin[0], origin[1], size[0], size[1], valid.width(), valid.height(), 1 << level,
                if source.is_none() { 4 } else { 32 | if mask.inverted { 64 } else { 0 } },
            ]);
            let default = if mask.inverted { 1. - mask.default_coverage } else { mask.default_coverage };
            values[8..12].fill(default.to_bits());
            let binding = Commands::binding(r, source.as_ref().unwrap_or(&r.empty_view), &r.empty_view, &output);
            commands.reduce(r, encoder, values, &binding, "reduce changed mask pages")?;
        }
        Ok(changed)
    }
}

impl Scene {
    pub fn reduced_layer(&self, _r: &WgpuRasterizer, layer: &Layer, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        self.scale_sources.complete_texture(layer, extent, level)
    }

}
