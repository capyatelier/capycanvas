//! Render placed content from immutable source and local paint tiles. The
//! compositor owns disposable output; accepting a pose never publishes raster.
use super::*;
use pixel_transform::{TiledTransformRecord, TransformTarget, TransformTile};
mod mips;
pub(super) use mips::Mip;

#[derive(Clone)]
pub(super) struct PlacementJob {
    target: wgpu::TextureView,
    tile: [u32; 2],
    region: PixelRect,
    extent: [u32; 2],
    transform: layer_core::ImageTransform,
    background: f32,
    sources: Vec<([u32; 2], wgpu::TextureView)>,
    source_size: [u32; 2],
}

impl Scene {
    pub(super) fn mask_at(
        &mut self,
        r: &WgpuRasterizer,
        mask: &layer_core::LayerMask,
        affine: layer_core::Affine,
        extent: [u32; 2],
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        if affine.0[..4] == [1., 0., 0., 1.] {
            return Ok(self.mask_tile(
                r,
                mask,
                layer_core::Point {
                    x: affine.0[4],
                    y: affine.0[5],
                },
                tile,
            ));
        }
        let bounds = PixelRect::full(extent);
        let transform = layer_core::ImageTransform {
            affine,
            ..Default::default()
        };
        let jobs = paint_transform::snapshot::region_jobs(
            bounds,
            transform,
            std::iter::once(tile),
            &[page_rect(tile)],
            |c| !page_rect(c).intersect(bounds).is_empty(),
        )?;
        let background = if mask.inverted {
            1. - mask.default_coverage
        } else {
            mask.default_coverage
        };
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        for job in jobs {
            let mut sources = Vec::new();
            let mut scratch = Vec::new();
            for c in job.sources {
                let page = self.mask_tile(r, mask, layer_core::Point::default(), c);
                sources.push((c, self.pool[page].view.clone()));
                scratch.push(page);
            }
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                target: self.pool[out].view.clone(),
                tile,
                region: job.region,
                extent,
                transform,
                background,
                sources,
                source_size: [PAGE_SIZE; 2],
            })));
            for page in scratch {
                self.free(page);
            }
        }
        Ok(out)
    }

    pub(super) fn placed_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let layer = &packet.layers[index];
        let extent = layer.local_extent(r.document_extent);
        let bounds = PixelRect::new(0, 0, extent[0], extent[1]);
        let transform = layer_core::ImageTransform {
            affine: layer_core::target_transform(packet.layers, layer.id),
            ..Default::default()
        };
        if self.placement_display
            && let Some(mip) = self.placement_mips.get(&layer.id).filter(|m| m.usable)
        {
            let (level, view, size) = mip.image.sample(mip.sample_level);
            let scale = (1 << level) as f32;
            let view = view.clone();
            let transform = layer_core::ImageTransform {
                affine: layer_core::Affine([scale, 0., 0., scale, 0., 0.]).then(transform.affine),
                ..Default::default()
            };
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                target: self.pool[out].view.clone(),
                tile,
                region: PixelRect::full([PAGE_SIZE; 2]),
                extent: size,
                transform,
                background: 0.,
                sources: vec![([0, 0], view)],
                source_size: size,
            })));
            return Ok(out);
        }
        let jobs = paint_transform::snapshot::region_jobs(
            bounds,
            transform,
            std::iter::once(tile),
            &[page_rect(tile)],
            |c| !page_rect(c).intersect(bounds).is_empty(),
        )?;
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        for job in jobs {
            let mut sources = Vec::with_capacity(job.sources.len());
            let mut scratch = Vec::new();
            for c in job.sources {
                let page = self.local_color_tile(r, packet, layer, c)?;
                sources.push((c, self.pool[page].view.clone()));
                scratch.push(page);
            }
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                target: self.pool[out].view.clone(),
                tile,
                region: job.region,
                extent,
                transform,
                background: 0.,
                sources,
                source_size: [PAGE_SIZE; 2],
            })));
            // Jobs execute in order. Reuse scratch only after its consuming draw.
            for page in scratch {
                self.free(page);
            }
        }
        Ok(out)
    }

    fn local_color_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        layer: &Layer,
        c: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        let stored = r.paint_layers.iter().find(|l| l.id == layer.id);
        let preview = r.preview_layer_id == Some(layer.id)
            && !r.preview_damage.intersect(page_rect(c)).is_empty();
        let watercolor_preview = preview
            && packet.dab_batches.iter().any(|b| {
                b.layer_id == layer.id
                    && b.kind == DabBatchKind::Preview
                    && b.style.execution == BrushExecution::Watercolor
            });
        let wet = stored.is_some_and(|s| {
            (s.watercolor.is_some() || watercolor_preview)
                && s.watercolor_wetness_pages
                    .iter()
                    .chain(
                        r.preview_watercolor_wetness_pages
                            .iter()
                            .filter(|_| preview),
                    )
                    .any(|p| {
                        p.coordinate[0].abs_diff(c[0]) <= 1 && p.coordinate[1].abs_diff(c[1]) <= 1
                    })
        });
        if wet {
            let binding = self.watercolor_binding(r, layer, stored.unwrap(), c, preview)?;
            self.jobs.push(Job::Watercolor {
                layer: layer.id,
                target: self.pool[out].view.clone(),
                binding,
                record: *r
                    .layer_style_records
                    .get(&layer.id)
                    .ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
                coordinate: c,
            });
        } else {
            let persistent = stored.and_then(|s| s.pages.iter().find(|p| p.coordinate == c));
            let predicted = if preview {
                r.preview_pages.iter().find(|p| p.coordinate == c)
            } else {
                None
            };
            let view = if let Some(p) = predicted.filter(|_| r.preview_requires_base).or(persistent)
            {
                Some(p.active().view.clone())
            } else {
                self.source_tile(r, layer, c)?
            };
            if let Some(view) = view {
                self.draw(
                    r,
                    out,
                    view,
                    None,
                    [0., 0., 256., 256.],
                    [1., 1., 0., 0.],
                    true,
                );
            }
            if let Some(p) = predicted.filter(|_| !r.preview_requires_base) {
                self.draw(
                    r,
                    out,
                    p.active().view.clone(),
                    None,
                    [0., 0., 256., 256.],
                    [1., 1., 0., 0.],
                    true,
                );
            }
        }
        Ok(out)
    }
}

pub(super) fn encode(
    pass: &mut pixel_transform::PixelTransform,
    r: &mut WgpuRasterizer,
    encoder: &mut crate::submission::CommandEncoder,
    job: &PlacementJob,
) -> Result<(), GpuRasterError> {
    let coordinates: Vec<_> = job.sources.iter().map(|(c, _)| *c).collect();
    let bounds = [0, 0, job.extent[0] as i32, job.extent[1] as i32];
    let offsets = pass
        .prepare_tiled(
            &r.device,
            &r.queue,
            &mut r.uploads,
            encoder,
            bounds,
            job.background,
            job.transform,
            &[TiledTransformRecord {
                target: job.tile,
                sources: &coordinates,
                source_size: job.source_size,
            }],
        )
        .map_err(GpuRasterError::InvalidTransform)?;
    let views: Vec<_> = job
        .sources
        .iter()
        .map(|(c, view)| TransformTile {
            view,
            origin: c.map(|v| (v * PAGE_SIZE) as i32),
            extent: job.source_size,
        })
        .collect();
    let source = pass
        .source_views(&r.device, &views, bounds, None, &r.empty_view)
        .map_err(GpuRasterError::InvalidTransform)?;
    pass.encode_prepared(
        encoder,
        &source,
        offsets,
        0,
        false,
        &TransformTarget {
            view: &job.target,
            extent: [PAGE_SIZE; 2],
            origin: job.tile.map(|v| (v * PAGE_SIZE) as i32),
            region: [
                job.region.min_x(),
                job.region.min_y(),
                job.region.width(),
                job.region.height(),
            ],
        },
    );
    Ok(())
}
