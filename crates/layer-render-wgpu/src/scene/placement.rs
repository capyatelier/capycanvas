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
        let transform = layer_core::ImageTransform::affine(affine);
        let jobs = paint_transform::snapshot::region_jobs(
            bounds,
            &transform,
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
                transform: transform.clone(),
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
        let affine = layer_core::target_transform(packet.layers, layer.id);
        let transform = layer_core::ImageTransform::affine(affine);
        if self.placement_display
            && let Some(mip) = self.placement_mips.get(&layer.id).filter(|m| m.usable)
        {
            let (level, view, size) = mip.image.sample(mip.sample_level);
            let scale = (1 << level) as f32;
            let view = view.clone();
            let transform = layer_core::ImageTransform::affine(
                layer_core::Affine([scale, 0., 0., scale, 0., 0.]).then(affine),
            );
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
            &transform,
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
                transform: transform.clone(),
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
        self.paint_page(r, packet, layer, stored, c, out, [0., 0., 256., 256.])?;
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
            &mut r.uploads,
            encoder,
            bounds,
            job.background,
            &job.transform,
            &[TiledTransformRecord {
                target: job.tile,
                sources: &coordinates,
                source_size: job.source_size,
                texels: [0; 4],
            }],
            None,
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
