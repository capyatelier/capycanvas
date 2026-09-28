use super::*;
use std::collections::{BTreeMap, HashMap};
use layer_core::raster::{RasterData, RasterPlane};

pub(super) struct Level {
    pub image: Image,
    pub valid: BTreeSet<[u32; 2]>,
}
pub(super) struct Source {
    pub extent: [u32; 2],
    pub updates: u64,
    raster: u64,
    watercolor: Option<WatercolorLayerStyle>,
    pub levels: BTreeMap<u32, Level>,
    source: Option<Arc<layer_core::color::source::SourceImage>>,
    backing: Option<Arc<RasterData>>,
    mask: Option<layer_core::LayerMask>,
    preview: BTreeSet<[u32; 2]>,
    pub(super) damage: PixelRect,
}

#[derive(Default)]
pub(crate) struct Sources {
    pub(super) entries: HashMap<LayerId, Source>,
    pub(super) reset: bool,
}
impl Sources {
    #[cfg(test)]
    pub fn cache_info(&self, id: LayerId) -> Option<(wgpu::Texture, u64, u32)> {
        let source = self.entries.get(&id)?;
        let (&level, image) = source.levels.first_key_value()?;
        Some((image.image.texture.clone(), source.updates, level))
    }
    pub fn storage_bytes(&self) -> u64 {
        self.entries.values().flat_map(|s| s.levels.values()).map(|l| texture_bytes(&l.image.texture)).sum()
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>) {
        let mut wanted = BTreeSet::new();
        self.reset = false;
        for layer in packet.layers {
            let visible = images::visible(packet.layers, layer);
            if visible && layer.kind == LayerKind::Paint && layer.is_artwork() && stack::has_content(r, layer) {
                wanted.insert(layer.id);
                self.update(r, packet, layer, false);
                let source = self.entries.get_mut(&layer.id).unwrap();
                source.raster = layer.raster.identity();
                let watercolor = r.paint_layers.iter().find(|l| l.id == layer.id).and_then(|l| l.watercolor);
                if source.watercolor != watercolor {
                    for level in source.levels.values_mut() { level.valid.clear(); }
                    source.watercolor = watercolor;
                    self.reset = true;
                    source.damage = PixelRect::full(source.extent);
                }
            }
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (visible || m.show_area)) {
                wanted.insert(mask.id);
                self.update(r, packet, layer, true);
            }
        }
        self.entries.retain(|id, _| wanted.contains(id));
    }
    fn update(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, layer: &Layer, is_mask: bool) {
        let extent = layer.local_extent(packet.document_extent);
        let (id, plane, image, mask) = if is_mask {
            (layer.mask.as_ref().unwrap().id, RasterPlane::Mask, None, metadata::mask_metadata(&layer.mask))
        } else { (layer.id, RasterPlane::Color, layer.source.clone(), None) };
        self.reset |= !self.entries.contains_key(&id);
        let source = self.entries.entry(id).or_insert_with(|| Source {
            extent, updates: 0, raster: 0, watercolor: None, levels: BTreeMap::new(), source: None, backing: None, mask: None, preview: BTreeSet::new(), damage: PixelRect::EMPTY,
        });
        let resized = source.extent != extent;
        if resized { source.extent = extent; source.levels.clear(); self.reset = true; }
        let same_image = match (&source.source, &image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false,
        };
        let reset = resized || packet.reset_layers || !same_image || source.mask != mask;
        if reset {
            for level in source.levels.values_mut() { level.valid.clear(); }
            self.reset = true;
        }
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
        for batch in packet.dab_batches.iter().filter(|b| b.layer_id == id) {
            let coordinates = page_coordinates(batch_pixel_rect(batch, extent));
            damage.extend(coordinates.clone());
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
        source.damage = if reset { PixelRect::full(extent) } else { damage.iter().fold(PixelRect::EMPTY, |r, c| r.union(page_rect(*c).intersect(PixelRect::full(extent)))) };
        for level in source.levels.values_mut() { level.valid.retain(|c| !damage.contains(c)); }
    }
    pub fn sample(&self, id: LayerId, requested: u32) -> Option<(u32, &wgpu::TextureView, [u32; 2])> {
        let source = self.entries.get(&id)?;
        let (&level, image) = source.levels.range(..=requested).next_back()?;
        let size = [image.image.texture.width(), image.image.texture.height()];
        Some((level, &image.image.view, size))
    }
    pub fn complete_texture(&self, layer: &Layer, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        let source = self.entries.get(&layer.id)?;
        let current = source.extent == extent && source.raster == layer.raster.identity() && source.preview.is_empty()
            && match (&source.source, &layer.source) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        let image = source.levels.get(&level)?;
        (current && page_coordinates(PixelRect::full(extent)).all(|c| image.valid.contains(&c))).then_some(&image.image.texture)
    }
    pub fn placement_levels(&mut self, requested: &BTreeMap<LayerId, u32>, budget: u64, max_side: u32) -> BTreeMap<LayerId, u32> {
        let cost = |id: LayerId, level: u32| display_mips::Plan::at(self.entries[&id].extent, level).level_bytes(level);
        let mut selected = BTreeMap::new();
        let mut remaining = budget;
        for (&id, &level) in requested {
            if level == 0 { continue; }
            let plan = display_mips::Plan::at(self.entries[&id].extent, level);
            let bytes = cost(id, level);
            if bytes <= remaining && plan.size.iter().all(|n| *n <= max_side) {
                selected.insert(id, level);
                remaining -= bytes;
            }
        }
        for (&id, &requested) in requested {
            let mut level = selected.get(&id).copied().or_else(|| {
                (requested == 0).then(|| self.entries[&id].levels.keys().next().copied()).flatten()
                    .filter(|level| cost(id, *level) <= remaining)
            });
            if requested == 0 && let Some(l) = level { remaining -= cost(id, l); }
            while let Some(l) = level.filter(|l| requested > 0 && *l > 1) {
                let plan = display_mips::Plan::at(self.entries[&id].extent, l - 1);
                let additional = cost(id, l - 1) - cost(id, l);
                if additional > remaining || plan.size.iter().any(|n| *n > max_side) { break; }
                remaining -= additional;
                level = Some(l - 1);
            }
            if let Some(l) = level { selected.insert(id, l); }
        }
        for (&id, source) in &mut self.entries {
            if requested.contains_key(&id) {
                let selected = selected.get(&id).copied();
                source.levels.retain(|level, image| {
                    if Some(*level) == selected { return true; }
                    let bytes = texture_bytes(&image.image.texture);
                    if selected.is_some_and(|l| *level > l) && bytes <= remaining {
                        remaining -= bytes;
                        true
                    } else { false }
                });
            }
        }
        selected
    }
    pub(super) fn image(&self, id: LayerId, level: u32) -> &Level { &self.entries[&id].levels[&level] }
    pub(super) fn ensure_level(&mut self, commands: &mut Commands, r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, id: LayerId, level: u32,
    ) -> Result<(), GpuRasterError> {
        let source = self.entries.get_mut(&id).unwrap();
        let plan = display_mips::Plan::at(source.extent, level);
        let mut target = source.levels.remove(&level).unwrap_or_else(|| Level { image: Image::new(r, plan.size), valid: BTreeSet::new() });
        if let Some((&finer, previous)) = source.levels.range(..level).next_back() {
            let mut columns: Vec<PixelRect> = Vec::new();
            for tile in previous.valid.difference(&target.valid) {
                let region = page_rect(*tile).intersect(PixelRect::full(source.extent));
                if let Some(last) = columns.last_mut().filter(|r| r.min_x() == region.min_x() && r.max_y() == region.min_y()) {
                    *last = last.union(region);
                } else { columns.push(region); }
            }
            let mut regions: Vec<PixelRect> = Vec::new();
            for column in columns {
                if let Some(last) = regions.last_mut().filter(|r| r.min_y() == column.min_y() && r.max_y() == column.max_y() && r.max_x() == column.min_x()) {
                    *last = last.union(column);
                } else { regions.push(column); }
            }
            for changed in regions {
                let [x, y, width, height] = paint_transform::texel_rect(changed, 1 << level);
                let binding = Commands::binding(r, &previous.image.view, &r.empty_view, &target.image.view);
                let mut values = [0; 16];
                values[..8].copy_from_slice(&[x, y, width, height, source.extent[0], source.extent[1],
                    1 << (level - finer), (finer << 8) | 8]);
                let offset = commands.record(r, encoder, values)?;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("derive source level"), timestamp_writes: None,
                });
                pass.set_pipeline(&r.scene_pipelines.scale.reduce);
                pass.set_bind_group(0, &commands.record_binding, &[offset]);
                pass.set_bind_group(1, &binding, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            target.valid.extend(&previous.valid);
        }
        source.levels.insert(level, target);
        Ok(())
    }
    pub fn retain_levels(&mut self, requested: &BTreeMap<LayerId, BTreeSet<u32>>, budget: u64) {
        let mut reserve = 0;
        let empty = BTreeSet::new();
        for (id, source) in &mut self.entries {
            let levels = requested.get(id).unwrap_or(&empty);
            let finer = levels.first().and_then(|requested| source.levels.range(..requested).next_back()).or_else(|| levels.is_empty().then(|| source.levels.first_key_value()).flatten()).map(|(&level, _)| level);
            source.levels.retain(|level, _| levels.contains(level) || Some(*level) == finer);
            reserve += levels.iter().filter(|level| !source.levels.contains_key(level))
                .map(|level| display_mips::Plan::at(source.extent, *level).level_bytes(*level)).sum::<u64>();
        }
        let mut optional: Vec<_> = self.entries.iter().flat_map(|(id, source)| source.levels.iter()
            .filter(|(level, _)| !requested.get(id).is_some_and(|levels| levels.contains(level))).map(|(level, image)| (texture_bytes(&image.image.texture), *id, *level))).collect();
        optional.sort_unstable_by_key(|(bytes, _, _)| std::cmp::Reverse(*bytes));
        let mut bytes = self.storage_bytes() + reserve;
        for (size, id, level) in optional {
            if bytes <= budget { break; }
            self.entries.get_mut(&id).unwrap().levels.remove(&level);
            bytes -= size;
        }
    }
}

impl Scene {
    pub(super) fn prepare_scale_color(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, layer: &Layer, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { level, required, covered } = request;
        let extent = layer.local_extent(packet.document_extent);
        let mut changed = PixelRect::EMPTY;
        self.scale_sources.ensure_level(commands, r, encoder, layer.id, level)?;
        let cached = self.scale_sources.image(layer.id, level);
        let output = cached.image.view.clone();
        let missing: Vec<_> = page_coordinates(PixelRect::full(extent))
            .filter(|c| !cached.valid.contains(c) && !page_rect(*c).intersect(required).is_empty() && page_rect(*c).intersect(covered).is_empty())
            .collect();
        for chunk in missing.chunks(32) {
            let mut jobs = Vec::with_capacity(chunk.len());
            let mut scratch = Vec::new();
            for &tile in chunk {
                let (source, base, reduced_preview, over, empty) = if self.scale_sources.entries[&layer.id].watercolor.is_some() {
                    let tile = self.local_color_tile(r, packet, layer, tile)?;
                    scratch.push(tile);
                    (self.pool[tile].view.clone(), r.empty_view.clone(), false, false, false)
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
                let origin = tile.map(|n| (n * PAGE_SIZE) >> level);
                let input_level = if reduced_preview { r.preview_level } else { 0 };
                let mut values = [0; 16];
                values[..8].copy_from_slice(&[
                    origin[0],
                    origin[1],
                    size[0],
                    size[1],
                    valid.width(),
                    valid.height(),
                    1 << (level - input_level),
                    u32::from(over) | if empty { 4 } else { 0 } | (input_level << 8),
                ]);
                let offset = commands.record(r, encoder, values)?;
                jobs.push((Commands::binding(r, &source, &base, &output), offset, size));
            }
            self.encode_jobs(r, encoder)?;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("reduce changed paint pages"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&r.scene_pipelines.scale.reduce);
            for (binding, offset, size) in &jobs {
                pass.set_bind_group(0, &commands.record_binding, &[*offset]);
                pass.set_bind_group(1, binding, &[]);
                pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
            }
            drop(pass);
            for slot in scratch { self.free(slot); }
            self.scale_sources.entries.get_mut(&layer.id).unwrap().updates += chunk.len() as u64;
            self.scale_sources.entries.get_mut(&layer.id).unwrap().levels.get_mut(&level).unwrap().valid.extend(chunk);
        }
        Ok(changed)
    }
    pub(super) fn prepare_scale_mask(
        &mut self, commands: &mut Commands, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        mask: &layer_core::LayerMask, request: SourceRequest,
    ) -> Result<PixelRect, GpuRasterError> {
        let SourceRequest { level, required, covered } = request;
        let extent = self.scale_sources.entries[&mask.id].extent;
        let mut changed = PixelRect::EMPTY;
        self.scale_sources.ensure_level(commands, r, encoder, mask.id, level)?;
        let cached = self.scale_sources.image(mask.id, level);
        let output = cached.image.view.clone();
        let missing: Vec<_> = page_coordinates(PixelRect::full(extent)).filter(|c| !cached.valid.contains(c) && !page_rect(*c).intersect(required).is_empty() && page_rect(*c).intersect(covered).is_empty()).collect();
        for tile in missing {
            let source = r.layer_masks.pages.get(&(mask.id, tile)).map(|p| p.view.clone());
            let valid = page_rect(tile).intersect(PixelRect::full(extent));
            changed = changed.union(valid);
            let size = [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
            let origin = tile.map(|n| (n * PAGE_SIZE) >> level);
            let mut values = [0; 16];
            values[..8].copy_from_slice(&[
                origin[0], origin[1], size[0], size[1], valid.width(), valid.height(), 1 << level,
                if source.is_none() { 4 } else { 32 | if mask.inverted { 64 } else { 0 } },
            ]);
            let default = if mask.inverted { 1. - mask.default_coverage } else { mask.default_coverage };
            values[8..12].fill(default.to_bits());
            let offset = commands.record(r, encoder, values)?;
            let binding = Commands::binding(r, source.as_ref().unwrap_or(&r.empty_view), &r.empty_view, &output);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("reduce changed mask pages"), timestamp_writes: None });
            pass.set_pipeline(&r.scene_pipelines.scale.reduce);
            pass.set_bind_group(0, &commands.record_binding, &[offset]);
            pass.set_bind_group(1, &binding, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
            drop(pass);
            self.scale_sources.entries.get_mut(&mask.id).unwrap().levels.get_mut(&level).unwrap().valid.insert(tile);
        }
        Ok(changed)
    }
}

impl Scene {
    pub fn reduced_layer(&self, _r: &WgpuRasterizer, layer: &Layer, extent: [u32; 2], level: u32) -> Option<&wgpu::Texture> {
        self.scale_sources.complete_texture(layer, extent, level)
    }
    #[cfg(test)]
    pub fn forget_placement_mips(&mut self) { self.scale_sources = Default::default(); }
    pub(in crate::scene) fn prepare_placed_sources(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder, commands: &mut Commands,
    ) -> Result<(), GpuRasterError> {
        let budget = if let Some(native) = &r.native_edit {
            native.image_pixel_budget(r, packet.layers, packet.document_extent)?
                .min(native.display_complete_bytes.saturating_sub(PixelRect::full(packet.document_extent).area() * 16))
                .saturating_sub(Self::capture_image_bound(packet.layers, PixelRect::full(packet.document_extent)))
        } else { 0 };
        let requested: BTreeMap<_, _> = packet.layers.iter()
            .filter(|layer| layer.source.is_some() && images::visible(packet.layers, layer))
            .map(|layer| (layer.id, placement_level(packet.layers, layer.id))).collect();
        let selected = self.scale_sources.placement_levels(&requested, budget, r.device.limits().max_texture_dimension_2d);
        for (id, level) in selected {
            if requested[&id] == 0 { continue; }
            let layer = packet.layers.iter().find(|l| l.id == id).unwrap();
            self.prepare_scale_color(commands, r, packet, encoder, layer, SourceRequest { level, required: PixelRect::full(layer.local_extent(packet.document_extent)), covered: PixelRect::EMPTY })?;
            for next in level + 1..=8 {
                let source = &self.scale_sources.entries[&id];
                let bytes = display_mips::Plan::at(source.extent, next).level_bytes(next);
                if source.levels.contains_key(&next) || self.scale_sources.storage_bytes() + bytes <= budget {
                    self.scale_sources.ensure_level(commands, r, encoder, id, next)?;
                }
            }
        }
        self.placement_display = true;
        Ok(())
    }
}
