//! A layer whose placement moves is drawn straight into the display level
//! the view samples, from its own pixels reduced once, instead of
//! recomposing the document at full resolution every frame.
use super::*;
use paint_transform::resample::Reduced;

/// Frames a drag's placement stays still before what it drew is recomposed.
const STILL_FRAMES: u32 = 12;

/// A layer's own pixels reduced to one level, kept between drags while the
/// layer's pixels are unchanged.
pub(super) struct PlacementCopy {
    key: (LayerId, u32, u64, Option<usize>),
    extent: [u32; 2],
    reduced: Reduced,
    pending: Vec<[u32; 2]>,
    /// The display level as the first drag began, and the placement it
    /// showed the layer at. A lone layer over the paper is drawn from it
    /// until the copy is complete.
    display: Option<(Reduced, layer_core::Affine)>,
}
impl PlacementCopy {
    fn key(layer: &Layer, local: u32) -> (LayerId, u32, u64, Option<usize>) {
        let source = layer.source.as_ref().map(|s| Arc::as_ptr(s) as usize);
        (layer.id, local, layer.raster.identity(), source)
    }
    /// An empty copy of `layer`'s pixels reduced to `local`.
    pub fn new(r: &WgpuRasterizer, layer: &Layer, local: u32) -> Self {
        let extent = r.target_extent(layer.id);
        Self {
            key: Self::key(layer, local),
            extent,
            reduced: Reduced::new(r, 0, local, extent, Vec::new(), false),
            pending: page_coordinates(PixelRect::full(extent)).rev().collect(),
            display: None,
        }
    }
    /// Whether this holds `layer`'s current pixels reduced to `local`.
    pub fn matches(&self, layer: &Layer, local: u32) -> bool {
        self.key == Self::key(layer, local)
    }
    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }
    /// Reduce the layer's next tiles, at least one and at most `tiles`, until
    /// `budget` has elapsed.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder,
        tiles: usize,
        budget: std::time::Duration,
    ) -> Result<(), GpuRasterError> {
        let Some(layer) = packet.layers.iter().find(|l| l.id == self.key.0) else {
            return Ok(());
        };
        let started = web_time::Instant::now();
        let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
        let mut result = Ok(());
        for _ in 0..tiles {
            let Some(tile) = self.pending.pop() else {
                break;
            };
            result = scene.reduce_layer_tiles(r, packet, layer, &[tile], &mut self.reduced.image, encoder);
            if result.is_err() || started.elapsed() >= budget {
                break;
            }
        }
        r.scene = Some(scene);
        result
    }
    pub fn layer(&self) -> LayerId {
        self.key.0
    }
    pub fn storage_bytes(&self) -> u64 {
        self.reduced.storage_bytes() + self.display.as_ref().map_or(0, |(d, _)| d.storage_bytes())
    }
}

pub(super) struct PlacementDrag {
    pub layer: LayerId,
    pub level: u32,
    copy: PlacementCopy,
    /// The placement the display shows.
    shown: layer_core::Affine,
    /// The document region drawn since the drag began.
    touched: PixelRect,
    /// Frames since the placement last moved.
    still: u32,
}
impl PlacementDrag {
    /// Begin a drag of `layer`, whose display shows it at `shown`, reusing
    /// `copy` when it still holds the layer's pixels at the level needed.
    /// Without a complete copy, a lone layer is drawn from `display`, the
    /// display level showing it over the paper.
    pub fn new(
        r: &WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: &Layer,
        level: u32,
        shown: layer_core::Affine,
        copy: Option<PlacementCopy>,
        display: Option<&wgpu::Texture>,
    ) -> Self {
        let local = paint_transform::local_level(level, shown);
        let mut copy = copy.filter(|c| c.matches(layer, local)).unwrap_or_else(|| PlacementCopy::new(r, layer, local));
        if let Some(texture) = display
            && copy.display.is_none()
            && !copy.ready()
            && shown.inverse().is_some()
        {
            let captured = Reduced::new(r, 0, level, r.document_extent, Vec::new(), false);
            encoder.copy_texture_to_texture(texture.as_image_copy(), captured.image.texture.as_image_copy(), texture.size());
            copy.display = Some((captured, shown));
        }
        Self {
            layer: layer.id,
            level,
            copy,
            shown,
            touched: PixelRect::EMPTY,
            still: 0,
        }
    }
    /// Count a frame in which the placement did not move. Returns whether the
    /// drag continues.
    pub fn hold(&mut self) -> bool {
        self.still += 1;
        self.still <= STILL_FRAMES
    }
    /// Whether a frame can draw the layer, from its copy or the display.
    pub fn drawable(&self) -> bool {
        self.copy.ready() || self.copy.display.is_some()
    }
    pub fn storage_bytes(&self) -> u64 {
        self.copy.storage_bytes()
    }
    /// Add document tiles the drag must recompose once it ends.
    pub fn touch(&mut self, region: PixelRect) {
        self.touched = self.touched.union(region);
    }
    /// The layer's reduced copy and the document region drawn.
    pub fn finish(self) -> (PlacementCopy, PixelRect) {
        (self.copy, self.touched)
    }
    /// Reduce the next tiles of the layer's own pixels, at most `tiles`,
    /// within a frame's budget.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        tiles: usize,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.copy.prepare(r, packet, encoder, tiles, PREPARE_MOVING)
    }
    /// Draw the layer at `placement` into display `target`, clearing where
    /// the display showed it before. Returns the document region drawn.
    pub fn draw(
        &mut self,
        r: &mut WgpuRasterizer,
        pass: &paint_transform::resample::Resample,
        encoder: &mut crate::submission::CommandEncoder,
        target: &wgpu::TextureView,
        placement: layer_core::Affine,
        display: pixel_transform::DisplayLevel,
    ) -> Result<PixelRect, GpuRasterError> {
        self.still = 0;
        let side = display.side;
        let extent = self.copy.extent;
        let bounds = PixelRect::full(extent).to_rect();
        let region = [self.shown, placement]
            .into_iter()
            .map(|p| pixel_rect(p.bounds(bounds), display.extent))
            .fold(PixelRect::EMPTY, PixelRect::union);
        let region = PixelRect::new(
            region.min_x() / side * side,
            region.min_y() / side * side,
            region.max_x().div_ceil(side).saturating_mul(side).min(display.extent[0]),
            region.max_y().div_ceil(side).saturating_mul(side).min(display.extent[1]),
        );
        if region.is_empty() {
            return Ok(region);
        }
        let low = [region.min_x() / side, region.min_y() / side];
        let texels = [
            low[0],
            low[1],
            region.max_x().div_ceil(side) - low[0],
            region.max_y().div_ceil(side) - low[1],
        ];
        let clip = layer_core::Affine([side as f32, 0., 0., side as f32, 0., 0.])
            .then(placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?);
        let at_level = pixel_transform::DisplayLevel {
            side: 1,
            extent: display.extent.map(|n| n.div_ceil(side)),
            ..display
        };
        let ready = self.copy.ready();
        match &mut self.copy.display {
            Some((captured, original)) if !ready => {
                let back = layer_core::ImageTransform::affine(original.inverse().unwrap());
                let transform = paint_transform::resample_map(&back, placement, self.level, self.level);
                let over_paper = pixel_transform::DisplayLevel { opacity: 1., ..at_level };
                let captured_at = clip.then(*original);
                let document = r.document_extent;
                captured.draw(r, pass, encoder, target, &transform, &transform, captured_at, document, texels, over_paper, None)?;
            }
            _ => {
                let transform = paint_transform::resample_map(
                    &layer_core::ImageTransform::default(),
                    placement,
                    self.copy.key.1,
                    self.level,
                );
                self.copy
                    .reduced
                    .draw(r, pass, encoder, target, &transform, &transform, clip, extent, texels, at_level, None)?;
            }
        }
        self.shown = placement;
        self.touched = self.touched.union(region);
        Ok(region)
    }
}
