//! Render placed content from immutable source and local paint tiles. The
//! compositor owns disposable output; accepting a pose never publishes raster.
use super::*;
use paint_transform::snapshot::Splitter;
use pixel_transform::{BatchDraw, TiledTransformRecord, TransformTile};

#[derive(Clone)]
pub(super) struct PlacementJob {
    target: wgpu::TextureView,
    tile: [u32; 2],
    region: PixelRect,
    extent: [u32; 2],
    transform: layer_core::ImageTransform,
    taps: u32,
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
        let background = if mask.inverted {
            1. - mask.default_coverage
        } else {
            mask.default_coverage
        };
        let transform = layer_core::ImageTransform::affine(affine);
        self.placed_jobs(r, transform, extent, tile, background, |scene, c| {
            Ok(scene.mask_tile(r, mask, layer_core::Point::default(), c))
        })
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
        let affine = layer_core::target_transform(packet.layers, layer.id);
        if self.placement_display
            && scale::placement_level(packet.layers, layer.id) > 0
            && let Some((plan, view)) = self.scale_sources.sample(layer.id, scale::placement_level(packet.layers, layer.id))
        {
            let scale = (1 << plan.level) as f32;
            let size = plan.size;
            let view = view.clone();
            let transform = layer_core::ImageTransform::affine(
                layer_core::Affine([scale, 0., 0., scale, plan.bounds.min_x() as f32, plan.bounds.min_y() as f32]).then(affine),
            );
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                target: self.pool[out].view.clone(),
                tile,
                region: PixelRect::full([PAGE_SIZE; 2]),
                extent: size,
                transform,
                taps: pixel_transform::PREVIEW_TAPS,
                background: 0.,
                sources: vec![([0, 0], view)],
                source_size: size,
            })));
            return Ok(out);
        }
        let mut transform = layer_core::ImageTransform::affine(affine);
        if !self.placement_display && crate::paint_transform::magnification(affine) > 1. + 1e-4 {
            transform.interpolation = layer_core::Interpolation::Bicubic;
        }
        self.placed_jobs(r, transform, extent, tile, 0., |scene, c| scene.local_color_tile(r, packet, layer, c))
    }

    /// Place `tile` of a layer `extent` pixels large through `transform`, one
    /// job per piece whose source pages `source` draws into scratch. Exact
    /// capture averages as many taps as a minified pixel covers, up to
    /// EXACT_TAPS per axis; the live display keeps the preview's count.
    fn placed_jobs(
        &mut self,
        r: &WgpuRasterizer,
        transform: layer_core::ImageTransform,
        extent: [u32; 2],
        tile: [u32; 2],
        background: f32,
        mut source: impl FnMut(&mut Self, [u32; 2]) -> Result<usize, GpuRasterError>,
    ) -> Result<usize, GpuRasterError> {
        let bounds = PixelRect::full(extent);
        let exact = pixel_transform::exact_taps(&transform);
        let taps = if self.placement_display { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact };
        let mut pieces = Vec::new();
        Splitter::new(bounds, &transform, None, |c| !page_rect(c).intersect(bounds).is_empty())?
            .split(page_rect(tile), &mut pieces)?;
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        for piece in pieces {
            let mut sources = Vec::with_capacity(piece.sources.len());
            let mut scratch = Vec::with_capacity(piece.sources.len());
            for c in piece.sources {
                let page = source(self, c)?;
                sources.push((c, self.pool[page].view.clone()));
                scratch.push(page);
            }
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                target: self.pool[out].view.clone(),
                tile,
                region: piece.region.page_local(tile),
                extent,
                transform: transform.clone(),
                taps,
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

    /// A scratch tile of `layer`'s own pixels at its local `c`.
    pub(crate) fn local_color_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        layer: &Layer,
        c: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        let stored = r.paint_layers.iter().find(|l| l.id == layer.id);
        self.paint_page(r, packet, layer, stored, c, out, [0., 0., 256., 256.], Convert::None)?;
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
    let offset = pass
        .prepare_tiled(
            &r.device,
            &mut r.uploads,
            encoder,
            bounds,
            job.background,
            &job.transform,
            job.taps,
            &[TiledTransformRecord {
                target: job.tile,
                sources: &coordinates,
                source_size: job.source_size,
                texels: [0; 4],
                unmoved: false,
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
        .source_views(&r.device, &views, bounds, None, None, &r.empty_view)
        .map_err(GpuRasterError::InvalidTransform)?;
    let region = job.region;
    let scissor = [region.min_x(), region.min_y(), region.width(), region.height()];
    pass.encode_batch(encoder, &job.target, false, false, offset, &[BatchDraw { source: &source, job: 0, scissor }]);
    Ok(())
}
