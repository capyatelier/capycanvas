//! A layer whose placement moves is drawn straight into the display level
//! the view samples, from its own pixels reduced once, instead of
//! recomposing the document at full resolution every frame.
use super::*;
use paint_transform::resample::Reduced;

/// Layer tiles reduced per frame before a placement drag draws.
const REDUCE_TILES: usize = 48;

/// A layer's own pixels reduced to one level, kept between drags while the
/// layer's pixels are unchanged.
pub(super) struct PlacementCopy {
    key: (LayerId, u32, u64, Option<usize>),
    extent: [u32; 2],
    reduced: Reduced,
    pending: Vec<[u32; 2]>,
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
        self.reduced.storage_bytes()
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
}
impl PlacementDrag {
    /// Begin a drag of `layer`, whose display shows it at `shown`, reusing
    /// `copy` when it still holds the layer's pixels at the level needed.
    pub fn new(
        r: &WgpuRasterizer,
        layer: &Layer,
        level: u32,
        shown: layer_core::Affine,
        copy: Option<PlacementCopy>,
    ) -> Self {
        let local = paint_transform::local_level(level, shown);
        let copy = copy.filter(|c| c.matches(layer, local)).unwrap_or_else(|| PlacementCopy::new(r, layer, local));
        Self {
            layer: layer.id,
            level,
            copy,
            shown,
            touched: PixelRect::EMPTY,
        }
    }
    pub fn ready(&self) -> bool {
        self.copy.ready()
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
    /// Reduce the next tiles of the layer's own pixels.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.copy.prepare(r, packet, encoder, REDUCE_TILES, std::time::Duration::MAX)
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
        let transform = paint_transform::resample_map(
            &layer_core::ImageTransform::default(),
            placement,
            self.copy.key.1,
            self.level,
        );
        let clip = layer_core::Affine([side as f32, 0., 0., side as f32, 0., 0.])
            .then(placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?);
        let at_level = pixel_transform::DisplayLevel {
            side: 1,
            extent: display.extent.map(|n| n.div_ceil(side)),
            ..display
        };
        self.copy
            .reduced
            .draw(r, pass, encoder, target, &transform, &transform, clip, extent, texels, at_level)?;
        self.shown = placement;
        self.touched = self.touched.union(region);
        Ok(region)
    }
}
