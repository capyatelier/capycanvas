//! Disposable reduced layer pixels for live composition. Exact artwork capture
//! explicitly bypasses this cache. Source samples and paint backing stay intact.
use super::*;
use std::{collections::BTreeSet, sync::Weak};

pub(in crate::scene) struct Mip {
    source: Weak<layer_core::color::source::SourceImage>,
    raster: u64,
    backing: Weak<layer_core::raster::RasterData>,
    space: layer_core::color::RgbSpace,
    preview: PixelRect,
    watercolor: Option<WatercolorLayerStyle>,
    pub image: display_mips::Image,
    pub usable: bool,
    pub sample_level: u32,
    pub updates: u64,
}

impl Scene {
    pub(in crate::scene) fn prepare_placement_mips(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.placement_display = true;
        let mut remaining = if let Some(native) = &r.native_edit {
            native.image_pixel_budget(r, packet.layers, packet.document_extent)?
                .min(native.display_complete_bytes.saturating_sub(PixelRect::full(packet.document_extent).area() * 16))
                .saturating_sub(
                Self::capture_image_bound(packet.layers, PixelRect::full(packet.document_extent)))
        } else { 0 };
        // Placement previews use only the shared allowance left after display
        // and exact filter dependencies. A windowed graph has no such surplus.
        let mut wanted = Vec::new();
        let mut retained = Vec::new();
        for layer in packet.layers {
            let Some(source) = &layer.source else {
                continue;
            };
            if remaining == 0 || !images::visible(packet.layers, layer) {
                continue;
            }
            let extent = layer.local_extent(packet.document_extent);
            let existing = self.placement_mips.get(&layer.id).filter(|m| {
                m.source.ptr_eq(&Arc::downgrade(source))
                    && m.space == r.document_color().space
                    && m.image.plan.extent == extent
            });
            let [a, b, c, d, _, _] = layer.properties.placement.0;
            let sum = a * a + b * b + c * c + d * d;
            let det = (a * d - b * c).abs();
            let high = ((sum + (sum * sum - 4. * det * det).max(0.).sqrt()) * 0.5).sqrt();
            // Keep enough samples along the most magnified affine axis, also
            // for nonuniform scale and shear. A narrower axis cannot discard
            // detail still visible along the wider one.
            let level = high.recip().log2().floor().max(0.) as u32;
            let level = level.min(8);
            if level == 0 {
                // A 100% view uses exact local tiles. Retain its reduced cache
                // only after reserving the previews other visible layers need.
                if let Some(mip) = existing {
                    retained.push((layer, mip.image.plan, 0));
                }
                continue;
            }
            let plan = display_mips::Plan {
                extent,
                size: extent.map(|n| n.div_ceil(1 << level)),
                level,
            };
            let bytes = plan.pixel_bytes() + 4096;
            if bytes > remaining
                || plan
                    .size
                    .iter()
                    .any(|n| *n > r.device.limits().max_texture_dimension_2d)
            {
                continue;
            }
            remaining -= bytes;
            wanted.push((layer, plan, level));
        }
        for (layer, plan, sample_level) in retained {
            let bytes = plan.pixel_bytes() + 4096;
            if bytes <= remaining {
                remaining -= bytes;
                wanted.push((layer, plan, sample_level));
            }
        }
        // Reserve every visible layer's required preview first, then spend only
        // the remaining admitted allowance on finer previews. Preparing spare
        // resolution during the cold load avoids rereading an entire source on
        // the first small scale-up from an exact 1/2, 1/4, ... fit boundary.
        // This never expands the shared display-memory allowance or creates a
        // full-resolution copy. Exact capture still bypasses these previews.
        for (_, plan, sample_level) in &mut wanted {
            while *sample_level > 0 && plan.level > 1 {
                let level = plan.level - 1;
                let finer = display_mips::Plan {
                    extent: plan.extent,
                    size: plan.extent.map(|n| n.div_ceil(1 << level)),
                    level,
                };
                let additional = finer.pixel_bytes() - plan.pixel_bytes();
                if additional > remaining
                    || finer.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d)
                {
                    break;
                }
                remaining -= additional;
                *plan = finer;
            }
        }
        // Spare detail prevents cold source replay when scaling up. Retaining
        // its smaller levels also lets small poses sample at their actual LOD,
        // avoiding the bandwidth of the finest preview on every moving frame.
        // Both representations stay inside the same admitted display allowance.
        let wanted: Vec<_> = wanted.into_iter().map(|(layer, plan, sample_level)| {
            let mut last = plan.level;
            while last < 8 {
                let additional = plan.pixel_bytes_through(last + 1) - plan.pixel_bytes_through(last);
                if additional > remaining { break; }
                remaining -= additional;
                last += 1;
            }
            (layer, plan, sample_level, last)
        }).collect();
        self.placement_mips.retain(|id, mip| {
            wanted.iter().any(|(layer, plan, _, last)| {
                layer.id == *id
                    && mip.image.plan == *plan
                    && mip.image.last_level() == *last
                    && mip.space == r.document_color().space
                    && mip
                        .source
                        .ptr_eq(&Arc::downgrade(layer.source.as_ref().unwrap()))
            })
        });
        for (layer, plan, sample_level, last) in wanted {
            let usable = sample_level > 0;
            let old = self.placement_mips.remove(&layer.id);
            let cold = old.is_none();
            let pipelines = r
                .display_pipelines
                .take()
                .unwrap_or_else(|| display_mips::Pipelines::new(&r.device));
            let mut mip = old.unwrap_or_else(|| Mip {
                source: Arc::downgrade(layer.source.as_ref().unwrap()),
                raster: layer.raster.identity(),
                backing: Weak::new(),
                space: r.document_color().space,
                preview: PixelRect::EMPTY,
                watercolor: None,
                image: display_mips::Image::with_mips(r, &pipelines, plan, last),
                usable,
                sample_level,
                updates: 0,
            });
            let result = (|| {
                let mut changed = BTreeSet::new();
                let mut damage = mip.preview;
                let preview = if r.preview_layer_id == Some(layer.id) {
                    r.preview_damage
                } else {
                    PixelRect::EMPTY
                };
                damage = damage.union(preview);
                for batch in packet.dab_batches.iter().filter(|b| b.layer_id == layer.id) {
                    damage = damage.union(batch_pixel_rect(batch, plan.extent));
                }
                for &(id, region) in &r.transform_damage {
                    if id == layer.id {
                        damage = damage.union(region);
                    }
                }
                let current = layer
                    .raster
                    .try_data()
                    .transpose()
                    .map_err(GpuRasterError::Effect)?;
                if mip.raster != layer.raster.identity() {
                    if let (Some(before), Some(after)) = (mip.backing.upgrade(), current.as_ref()) {
                        for (key, tile) in before.tiles.iter().chain(after.tiles.iter()) {
                            if before.tiles.get(key).is_none_or(|a| !a.same_capture(tile))
                                || after.tiles.get(key).is_none_or(|b| !b.same_capture(tile))
                            {
                                changed.insert(key.coordinate);
                            }
                        }
                    } else if damage.is_empty() {
                        // A cold revision cannot prove a small difference.
                        damage = PixelRect::full(plan.extent);
                    }
                }
                let stored = r.paint_layers.iter().find(|l| l.id == layer.id);
                let watercolor = stored.and_then(|l| l.watercolor);
                let radius = watercolor
                    .map_or(0, |w| w.radius())
                    .max(mip.watercolor.map_or(0, |w| w.radius()));
                if watercolor != mip.watercolor {
                    for c in stored
                        .into_iter()
                        .flat_map(|l| l.pages.iter().map(|p| p.coordinate))
                        .chain(r.native_color_coordinates(layer.id))
                    {
                        damage = damage.union(page_rect(c));
                    }
                }
                if cold || packet.reset_layers {
                    damage = PixelRect::full(plan.extent);
                }
                changed.extend(page_coordinates(damage.expand(radius, plan.extent)));
                if radius > 0 {
                    let halo: Vec<_> = changed
                        .iter()
                        .flat_map(|c| page_coordinates(page_rect(*c).expand(radius, plan.extent)))
                        .collect();
                    changed.extend(halo);
                }
                self.jobs.clear();
                self.used.fill(false);
                #[cfg(target_os = "android")]
                let mut recorded_tiles = 0;
                for coordinate in changed {
                    if page_rect(coordinate)
                        .intersect(PixelRect::full(plan.extent))
                        .is_empty()
                    {
                        continue;
                    }
                    let page = self.local_color_tile(r, packet, layer, coordinate)?;
                    self.encode_jobs(r, encoder)?;
                    mip.image.write_tile(
                        &r.device,
                        &pipelines,
                        encoder,
                        &self.pool[page].texture,
                        [0, 0],
                        coordinate,
                    )?;
                    self.free(page);
                    mip.updates += 1;
                    // A cold photo preview can replay thousands of source and
                    // paint passes even when all source pixels are cached.
                    // Bound Adreno command storage at the existing upload/queue
                    // boundary, before recording the next tile batch. The
                    // finished preview is still published only after success.
                    #[cfg(target_os = "android")]
                    {
                        // Count reduction/copy levels as well as source tiles;
                        // a retained pyramid must not multiply the driver's
                        // peak command storage during cold preparation.
                        recorded_tiles += last + 1;
                        if recorded_tiles >= 64 {
                            Self::submit_chunk(r, encoder, "bounded placed photo preview")?;
                            recorded_tiles = 0;
                        }
                    }
                }
                mip.preview = preview;
                mip.watercolor = watercolor;
                mip.raster = layer.raster.identity();
                if let Some(current) = current {
                    mip.backing = Arc::downgrade(&current);
                }
                mip.usable = usable;
                mip.sample_level = sample_level;
                Ok::<_, GpuRasterError>(())
            })();
            r.display_pipelines = Some(pipelines);
            if result.is_ok() {
                self.placement_mips.insert(layer.id, mip);
            }
            result?;
        }
        Ok(())
    }
}
