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
        let mut remaining = r.native_edit.as_ref().map_or(0, |n| {
            n.display_complete_bytes
                .saturating_sub(
                    u64::from(packet.document_extent[0])
                        * u64::from(packet.document_extent[1])
                        * 16,
                )
        });
        // Use the admitted display allowance, which already leaves headroom
        // for editing and other GPU users. A separate fixed cap can evict the
        // preview precisely when a small scale-up needs more detail: a 61 MP
        // photo's half-size Float32 preview needs about 231 MiB. Falling back
        // then makes every transform/paint frame fetch hundreds of source tiles.
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
            let level = (det / high).recip().log2().floor().max(0.) as u32;
            let level = level.min(8);
            if level == 0 {
                // A 100% view uses exact local tiles. Retain its reduced cache
                // only after reserving the previews other visible layers need.
                if let Some(mip) = existing {
                    retained.push((layer, mip.image.plan, false));
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
            wanted.push((layer, plan, level > 0));
        }
        for (layer, plan, usable) in retained {
            let bytes = plan.pixel_bytes() + 4096;
            if bytes <= remaining {
                remaining -= bytes;
                wanted.push((layer, plan, usable));
            }
        }
        // Reserve every visible layer's required preview first, then spend only
        // the remaining admitted allowance on finer previews. Preparing spare
        // resolution during the cold load avoids rereading an entire source on
        // the first small scale-up from an exact 1/2, 1/4, ... fit boundary.
        // This never expands the shared display-memory allowance or creates a
        // full-resolution copy. Exact capture still bypasses these previews.
        for (_, plan, usable) in &mut wanted {
            while *usable && plan.level > 1 {
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
        self.placement_mips.retain(|id, mip| {
            wanted.iter().any(|(layer, plan, _)| {
                layer.id == *id
                    && mip.image.plan == *plan
                    && mip.space == r.document_color().space
                    && mip
                        .source
                        .ptr_eq(&Arc::downgrade(layer.source.as_ref().unwrap()))
            })
        });
        for (layer, plan, usable) in wanted {
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
                image: display_mips::Image::new(r, &pipelines, plan),
                usable,
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
                }
                mip.preview = preview;
                mip.watercolor = watercolor;
                mip.raster = layer.raster.identity();
                if let Some(current) = current {
                    mip.backing = Arc::downgrade(&current);
                }
                mip.usable = usable;
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
