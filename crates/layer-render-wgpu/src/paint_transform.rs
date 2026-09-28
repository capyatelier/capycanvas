//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
//! Linked paint and mask targets run the same transaction with separate origins.
use super::*;
use pixel_transform::PixelTransform;
pub(super) mod layers;
pub(super) mod mesh;
pub(super) mod resample;
pub(super) mod snapshot;
use snapshot::TileSnapshot;

pub(super) struct PaintTransforms([ImageTransformState; 2], layers::LayerComposite, resample::Resample);
impl PaintTransforms {
    pub(super) fn placement_pass(&self) -> PixelTransform {
        self.0[0].color.placement_pass()
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let primary = ImageTransformState::new(device);
        let companion = primary.fork();
        Self([primary, companion], layers::LayerComposite::new(device), resample::Resample::new(device))
    }
    pub fn composite(&self) -> &layers::LayerComposite {
        &self.1
    }
    pub fn resample(&self) -> &resample::Resample {
        &self.2
    }
    pub fn pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 3] {
        self.0[0].pipelines()
    }
    /// What warp meshes draw with, compiled when a mesh is first shown.
    pub fn mesh_pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 4] {
        self.0[0].mesh_pipelines()
    }
    /// Drag previews draw into the display with these once they are ready;
    /// they never delay input.
    pub fn display_pipelines(&self) -> [&Deferred<wgpu::ComputePipeline>; 3] {
        [self.0[0].color.display.as_ref().expect("color transform"), &self.1.pipeline, &self.2.pipeline]
    }
    pub fn begin_frame(&mut self) {
        for t in &mut self.0 {
            t.begin_frame();
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.0.iter().map(ImageTransformState::storage_bytes).sum::<u64>() - 3 * 48
    }
    pub fn has_preview(&self) -> bool {
        self.0.iter().any(ImageTransformState::has_preview)
    }
    pub fn discard_preview(&mut self) {
        for t in &mut self.0 {
            t.discard_preview();
        }
    }
    #[cfg(test)]
    pub fn reduced_level(&self) -> Option<u32> {
        self.0[0].reduced.as_ref().filter(|r| r.pending.is_empty()).map(|r| r.level)
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        operation: &layer_core::LayerOperation,
    ) -> Result<(), GpuRasterError> {
        self.0[0].apply(r, encoder, layer, operation, r.target_extent(layer))
    }
    pub fn consume_commit(&mut self, packet: FramePacket<'_>) -> Vec<(LayerId, u32)> {
        let Some(matches) = self
            .0
            .iter()
            .filter(|t| t.has_preview())
            .map(|t| t.matching_commit(packet))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        if !matches.is_empty() {
            self.discard_preview();
        }
        matches
    }
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        let mut damage = Vec::with_capacity(2);
        for t in &mut self.0 {
            damage.extend(t.cancel_preview(r, encoder)?);
        }
        Ok(damage)
    }
    pub fn update_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layers: &[Layer],
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        let companion = next.companion(layers);
        let layer = layers.iter().find(|l| l.id == next.layer);
        let mut damage = self.0[0].update_preview(r, encoder, next, layer, r.target_extent(next.layer))?;
        if let Some(companion) = companion {
            damage.extend(self.0[1].update_preview(r, encoder, &companion, None, r.target_extent(companion.layer))?);
        } else {
            damage.extend(self.0[1].cancel_preview(r, encoder)?);
        }
        Ok(damage)
    }
    /// Reduce, within this frame's preparation, more pages of the moving
    /// layer to display `level`, which drag frames then resample instead of
    /// evaluating every layer pixel, and once complete, with `spares`, the
    /// spare pages a still preview settles into. Returns whether pages
    /// remain to reduce.
    pub fn prepare_reduced(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layers: &[Layer],
        level: u32,
        spares: bool,
    ) -> Result<bool, GpuRasterError> {
        let local = local_level(level, layer_core::target_transform(layers, next.layer));
        let layer = layers.iter().find(|l| l.id == next.layer);
        let extent = r.target_extent(next.layer);
        self.0[0].prepare_reduced(r, encoder, next, layer, extent, local, spares)
    }
    /// Capture `layer` and reduce it to display `level` while no transform
    /// is open, as the transaction of a Move drag of its `selection`, in
    /// layer pixels, would. That transaction adopts them, so its first drag
    /// frames draw at once. Returns whether pages remain to reduce.
    pub fn prepare_standby(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layers: &[Layer],
        layer: LayerId,
        selection: &layer_core::Selection,
        level: u32,
    ) -> Result<bool, GpuRasterError> {
        let Some(stored) = layers.iter().find(|l| l.id == layer) else {
            return Ok(false);
        };
        let local = local_level(level, layer_core::target_transform(layers, layer));
        let extent = r.target_extent(layer);
        self.0[0].prepare_standby(r, encoder, stored, selection, extent, local)
    }
    /// Drop what was captured ahead of a Move drag, before paint or a
    /// restore rewrites the pages it captured.
    pub fn release_standby(&mut self) {
        for t in &mut self.0 {
            if t.preview.is_none() && t.standby.is_some() {
                t.release_snapshot();
            }
        }
    }
    /// Whether `next`'s layer, captured by this transaction, still has to be
    /// reduced before its drag frames.
    pub fn reduces(&self, next: &layer_render::TransformPreview) -> bool {
        self.0[0].reduces(next)
    }
    /// Draw, within this frame's preparation, more full-resolution pages of
    /// a still preview last drawn straight into the display. Returns the
    /// layer regions to recompose once every page is drawn.
    pub fn settle(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layers: &[Layer],
    ) -> Result<Option<Vec<(LayerId, PixelRect)>>, GpuRasterError> {
        let settled = self.0[0].settle(r, encoder, next, r.target_extent(next.layer))?;
        Ok(settled.map(|damage| {
            damage
                .into_iter()
                .map(|(id, local)| (id, brush_tiles::document_damage(layers, id, local, r.document_extent)))
                .collect()
        }))
    }
    /// Draw a preview of a layer without a mask straight into a reduced
    /// display `level`, resampled from its reduced copy, a warp through its
    /// mesh rasterized at the level's texels, or exactly from the captured
    /// originals while a still unplaced layer is reduced. The layer's pages
    /// keep the last full preview. Returns the document region drawn, or None
    /// when the transaction has not captured this layer or reduced a placed
    /// one or a warped one.
    pub fn render_display(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layers: &[Layer],
        level: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel,
    ) -> Result<Option<PixelRect>, GpuRasterError> {
        let placement = layer_core::target_transform(layers, next.layer);
        let extent = r.target_extent(next.layer);
        self.0[0].render_display(r, encoder, next, extent, placement, level, display, &self.2)
    }
}

struct ImageTransformState {
    color: PixelTransform,
    scalar: PixelTransform,
    visibility: PixelTransform,
    background: Option<f32>,
    sources: [Option<TileSnapshot>; 3],
    captures: [std::collections::BTreeMap<[u32; 2], snapshot::SnapshotPage>; 3],
    selection: Option<wgpu::Buffer>,
    has_selection: bool,
    cut: layer_core::Rect,
    original_pages: [Vec<[u32; 2]>; 3],
    source_bounds: [PixelRect; 3],
    preview: Option<layer_render::TransformPreview>,
    preview_regions: [PixelRect; 2],
    /// The preview last drawn into a display level instead of pages, and
    /// its regions.
    displayed: Option<(layer_render::TransformPreview, [PixelRect; 2])>,
    /// The still preview whose pages are being drawn a few at a time, the
    /// regions it redraws, and the pages still to draw.
    settling: Option<(layer_render::TransformPreview, [PixelRect; 4], Vec<[u32; 2]>)>,
    reduced: Option<resample::Reduced>,
    spares: PreviewPages,
    atlases: [Option<Atlas>; 2],
    /// What the sources and reduced copy were captured for while no
    /// transform was open.
    standby: Option<Standby>,
    /// Mesh positions for each window of pages a warp draws.
    positions: mesh::Positions,
    /// Mesh positions for the display texels a moving warp resamples.
    display_positions: mesh::Positions,
    /// The warp mesh last tessellated, the display tolerance it was
    /// tessellated within, if any, and its geometry.
    mesh: Option<(Arc<layer_core::MeshMap>, Option<f32>, Arc<mesh::MeshGeometry>)>,
}

/// A layer's pixels, and the selection of them a Move drag may move, that a
/// capture and reduction made before the drag's transaction began.
#[derive(PartialEq)]
struct Standby {
    layer: LayerId,
    raster: u64,
    source: Option<usize>,
    selection: layer_core::Selection,
}
impl Standby {
    fn of(layer: &Layer, selection: &layer_core::Selection) -> Self {
        Self {
            layer: layer.id,
            raster: layer.raster.identity(),
            source: layer.source.as_ref().map(|s| Arc::as_ptr(s) as usize),
            selection: selection.clone(),
        }
    }
    /// The still preview of the transaction it is captured for.
    fn preview(&self) -> layer_render::TransformPreview {
        layer_render::TransformPreview {
            transaction: STANDBY,
            moving: false,
            layer: self.layer,
            selection: Some(self.selection.clone()),
            transform: Default::default(),
        }
    }
}
/// The transaction a standby capture is made for, which no transform uses.
const STANDBY: u64 = u64::MAX;

/// Time a still frame spends allocating the spare pages a preview settles
/// into.
const SPARE_PAGE_TIME: std::time::Duration = std::time::Duration::from_millis(2);

/// Transform records one frame of a released preview draws, reserved before
/// the drag so settling it allocates none.
const SETTLE_RECORDS: u64 = 4096;


/// Distinct source-cache tiles one batch may bind. Staying below the cache's
/// smallest capacity keeps every tile of a batch resident until it is drawn.
const BATCH_ORIGINAL_TILES: usize = 48;

/// Pages per window of a mesh transform, matching its positions texture.
const MESH_WINDOW: [u32; 2] = [mesh::WINDOW_PAGES; 2];

/// A destination page and the page-local part of it a transform draws.
struct Target {
    texture: wgpu::Texture,
    region: PixelRect,
    initialize: bool,
}

/// Destination pages drawn together into the atlas, which holds the pages
/// from `origin`, then copied into place. Jobs may span several pages.
struct Window {
    origin: [u32; 2],
    /// Absolute destination regions, and whether each draws unmoved pixels.
    jobs: Vec<(snapshot::Footprint, bool)>,
    pages: Vec<([u32; 2], Target)>,
}

/// The texels of `side` pixels each that cover `region`: the first, and how
/// many across and down.
pub(crate) fn texel_rect(region: PixelRect, side: u32) -> [u32; 4] {
    let low = [region.min_x() / side, region.min_y() / side];
    [low[0], low[1], region.max_x().div_ceil(side) - low[0], region.max_y().div_ceil(side) - low[1]]
}

/// `region` grown to whole texels of `side` pixels, within `extent`.
pub(crate) fn aligned(region: PixelRect, side: u32, extent: [u32; 2]) -> PixelRect {
    let near = |v: u32| v / side * side;
    let far = |v: u32, limit: u32| v.div_ceil(side).saturating_mul(side).min(limit);
    PixelRect::new(near(region.min_x()), near(region.min_y()), far(region.max_x(), extent[0]), far(region.max_y(), extent[1]))
}

/// The level of a layer's own pixels whose texels are at least as fine as
/// those of display `level` along the axis `placement` magnifies most.
pub(crate) fn local_level(level: u32, placement: layer_core::Affine) -> u32 {
    (level as f32 - magnification(placement).log2()).floor().clamp(0., 4.) as u32
}

/// How far `placement` stretches layer pixels along the axis it magnifies
/// most.
pub(crate) fn magnification(placement: layer_core::Affine) -> f32 {
    let [a, b, c, d, _, _] = placement.0;
    let sum = a * a + b * b + c * c + d * d;
    let det = (a * d - b * c).abs();
    ((sum + (sum * sum - 4. * det * det).max(0.).sqrt()) * 0.5).sqrt()
}

/// `transform` followed by `placement`, from texels of the layer reduced to
/// `local` to texels of display `level`, sampled bilinearly. A warp resamples
/// from mesh positions instead and maps through the placement alone.
pub(crate) fn resample_map(
    transform: &layer_core::ImageTransform,
    placement: layer_core::Affine,
    local: u32,
    level: u32,
) -> Result<layer_core::ImageTransform, GpuRasterError> {
    let moved = transform.map.projective().unwrap_or(layer_core::Projective::IDENTITY);
    let scale = |s: f32| layer_core::Projective([s, 0., 0., 0., s, 0., 0., 0., 1.]);
    let map = [moved, layer_core::Projective::from_affine(placement), scale(1. / (1u32 << level) as f32)]
        .into_iter()
        .try_fold(scale((1u32 << local) as f32), layer_core::Projective::then)
        .ok_or(GpuRasterError::InvalidTransform("Transform must be finite and invertible"))?;
    Ok(layer_core::ImageTransform { map: map.into(), interpolation: layer_core::Interpolation::Linear, ..Default::default() })
}

/// Taps per axis a preview's pages average over a minified pixel: the exact
/// count once it stops moving, since Apply may keep those pages.
fn preview_taps(preview: &layer_render::TransformPreview) -> u32 {
    let exact = pixel_transform::exact_taps(&preview.drawn());
    if preview.moving { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact }
}

/// Whether `selection` fully covers every pixel of `bounds`, so a transform
/// moves all of them and leaves none in place.
fn selects_all(selection: &layer_core::Selection, bounds: PixelRect) -> bool {
    let layer_core::SelectionShape::Contours(contours) = &selection.shape else {
        return false;
    };
    let [a, b, c, d, x, y] = selection.affine.0;
    match &contours[..] {
        [] => selection.inverted,
        [contour] if !selection.inverted && b == 0. && c == 0. && contour.len() == 4 => {
            let corners: Vec<_> = contour.iter().map(|p| [a * p.x + x, d * p.y + y]).collect();
            let rectangle = (0..4).all(|i| {
                let [p, q] = [corners[i], corners[(i + 1) % 4]];
                (p[0] == q[0]) != (p[1] == q[1])
            });
            let low = corners.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
            let high = corners.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
            rectangle
                && low[0] <= bounds.min_x() as f32
                && low[1] <= bounds.min_y() as f32
                && high[0] >= bounds.max_x() as f32
                && high[1] >= bounds.max_y() as f32
        }
        _ => false,
    }
}

fn on_page(local: PixelRect, coordinate: [u32; 2]) -> PixelRect {
    let [x, y] = coordinate.map(|v| v * PAGE_SIZE);
    PixelRect::new(x + local.min_x(), y + local.min_y(), x + local.max_x(), y + local.max_y())
}

/// Consecutive jobs that together bind at most BATCH_ORIGINAL_TILES tiles
/// the transaction has not captured.
fn source_batches(
    jobs: &[(snapshot::Footprint, bool)],
    captured: impl Fn([u32; 2]) -> bool,
) -> Vec<std::ops::Range<usize>> {
    let mut batches = Vec::new();
    let mut originals = std::collections::HashSet::new();
    let mut start = 0;
    for (index, (job, _)) in jobs.iter().enumerate() {
        let needed: Vec<_> = job.sources.iter().filter(|c| !captured(**c)).collect();
        let fresh = needed.iter().filter(|c| !originals.contains(**c)).count();
        if index > start && originals.len() + fresh > BATCH_ORIGINAL_TILES {
            batches.push(start..index);
            start = index;
            originals.clear();
        }
        originals.extend(needed.into_iter().copied());
    }
    batches.push(start..jobs.len());
    batches
}

/// Scratch holding a window of destination pages. One pass per 256x256 page
/// would dominate the frame for large layers.
struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Atlas {
    fn pages(format: wgpu::TextureFormat) -> [u32; 2] {
        match format.block_copy_size(None).unwrap_or(16) {
            16.. => [4, 4],
            8.. => [8, 4],
            _ => [8, 8],
        }
    }
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let [columns, rows] = Self::pages(format);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("batched transform pages"),
            size: wgpu::Extent3d {
                width: columns * PAGE_SIZE,
                height: rows * PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }
    fn storage_bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }
}

/// Pages which left the live preview footprint. Reuse within the transaction,
/// not as document content; apply/cancel releases them. Capacity follows peak
/// live footprints, not the accumulated area visited by the moving selection.
#[derive(Default)]
struct PreviewPages {
    paint: Vec<LayerPage>,
    material: Vec<CanvasMaterialPage>,
    watercolor: Vec<WatercolorWetnessPage>,
    masks: Vec<layer_masks::MaskPage>,
}
impl PreviewPages {
    fn clear(&mut self) {
        self.paint.clear();
        self.material.clear();
        self.watercolor.clear();
        self.masks.clear();
    }
    fn storage_bytes(&self) -> u64 {
        self.paint
            .iter()
            .map(|p| p.primary.storage_bytes() + p.secondary.as_ref().map_or(0, PageSurface::storage_bytes))
            .sum::<u64>()
            + self.material.iter().map(|p| p.wetness.storage_bytes()).sum::<u64>()
            + self.watercolor.iter().map(|p| p.primary.storage_bytes() + p.secondary.storage_bytes()).sum::<u64>()
            + self.masks.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>()
    }
}
impl ImageTransformState {
    pub fn new(device: &PipelineDevice) -> Self {
        let [color, scalar, visibility] = PixelTransform::passes(device);
        Self::with_passes(color, scalar, visibility, mesh::Positions::new(device))
    }
    pub fn fork(&self) -> Self {
        Self::with_passes(
            self.color.fork(),
            self.scalar.fork(),
            self.visibility.fork(),
            self.positions.fork(),
        )
    }
    fn with_passes(
        color: PixelTransform,
        scalar: PixelTransform,
        visibility: PixelTransform,
        positions: mesh::Positions,
    ) -> Self {
        Self {
            color,
            scalar,
            visibility,
            background: None,
            sources: Default::default(),
            captures: Default::default(),
            selection: None,
            has_selection: false,
            cut: layer_core::Rect::EMPTY,
            original_pages: Default::default(),
            source_bounds: [PixelRect::EMPTY; 3],
            preview: None,
            preview_regions: [PixelRect::EMPTY; 2],
            displayed: None,
            settling: None,
            reduced: None,
            standby: None,
            spares: PreviewPages::default(),
            atlases: Default::default(),
            display_positions: positions.fork(),
            positions,
            mesh: None,
        }
    }
    pub fn pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 3] {
        [
            &self.color.pipeline,
            &self.scalar.pipeline,
            &self.visibility.pipeline,
        ]
    }
    pub fn mesh_pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 4] {
        [
            &self.color.mesh_pipeline,
            &self.scalar.mesh_pipeline,
            &self.visibility.mesh_pipeline,
            &self.positions.pipeline,
        ]
    }
    pub fn begin_frame(&mut self) {
        self.color.begin_frame();
        self.scalar.begin_frame();
        self.visibility.begin_frame();
    }
    pub fn storage_bytes(&self) -> u64 {
        self.spares.storage_bytes()
            + self.atlases.iter().flatten().map(Atlas::storage_bytes).sum::<u64>()
            + self.positions.storage_bytes()
            + self.display_positions.storage_bytes()
            + self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scalar.storage_bytes()
            + self.visibility.storage_bytes()
            + self.reduced.as_ref().map_or(0, resample::Reduced::storage_bytes)
            + self
                .captures
                .iter()
                .flat_map(|pages| pages.values())
                .map(|p| {
                    u64::from(p.texture.width())
                        * u64::from(p.texture.height())
                        * u64::from(p.texture.format().block_copy_size(None).unwrap())
                })
                .sum::<u64>()
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        operation: &layer_core::LayerOperation,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        let layer_core::LayerOperationKind::Transform(transform) = &operation.kind else {
            unreachable!()
        };
        if transform.is_identity() {
            return Ok(());
        }
        self.capture_source(
            r,
            encoder,
            layer,
            operation.coverage.initial.as_ref(),
            extent,
        )?;
        let regions = transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        let taps = pixel_transform::exact_taps(transform);
        let result = self.render_source(r, encoder, layer, transform, taps, &regions);
        if result.is_ok() && self.moves_everything(operation.coverage.initial.as_ref()) {
            let placed = regions[1];
            r.drop_vacated_pages(layer, |c| !placed.page_local(c).is_empty());
        }
        self.release_snapshot();
        result
    }
    /// Whether the captured transform moves every pixel its target holds,
    /// leaving only the pages its forward bounds reach. A photo's original
    /// stays under its pages, so emptied pages must keep covering it.
    fn moves_everything(&self, selection: Option<&layer_core::Selection>) -> bool {
        let source = self.source_bounds.into_iter().fold(PixelRect::EMPTY, PixelRect::union);
        self.sources[0].as_ref().is_some_and(|s| s.original.is_none())
            && !source.is_empty()
            && selection.is_none_or(|s| selects_all(s, source))
    }
    fn capture_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        selection: Option<&layer_core::Selection>,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        self.spares.clear();
        self.release_snapshot();
        self.has_selection = selection.is_some();
        self.cut = layer_core::Rect::EMPTY;
        let (background, pages) = source_pages(r, layer)?;
        let backing = background.is_none().then(|| r.native_backing(layer).cloned()
            .map(|data| (data, r.document_color().space))).flatten();
        let original = background
            .is_none()
            .then(|| r.tiled_sources.get(&layer).cloned())
            .flatten();
        self.background = background;
        self.original_pages = std::array::from_fn(|i| pages[i].iter().map(|(c, _)| *c).collect());
        self.source_bounds = std::array::from_fn(|i| {
            pages[i]
                .iter()
                .fold(PixelRect::EMPTY, |b, (c, _)| b.union(page_rect(*c)))
                .intersect(PixelRect::full(extent))
        });
        if let Some((data, _)) = &backing {
            for key in data.tiles.keys().filter(|key| key.plane == layer_core::raster::RasterPlane::Color) {
                if !self.original_pages[0].contains(&key.coordinate) { self.original_pages[0].push(key.coordinate); }
                self.source_bounds[0] = self.source_bounds[0].union(page_rect(key.coordinate).intersect(PixelRect::full(extent)));
            }
        }
        if let Some(original) = &original {
            self.source_bounds[0] = self.source_bounds[0]
                .union(PixelRect::full(original.extent).intersect(PixelRect::full(extent)));
        }
        let source_bounds = self
            .source_bounds
            .into_iter()
            .fold(PixelRect::EMPTY, PixelRect::union);
        if source_bounds.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        r.test.source_captures.update(|n| n + 1);
        let mut cut = source_bounds.to_rect();
        if let Some(s) = selection {
            if !s.inverted {
                let b = s.bounds();
                cut.min.x = cut.min.x.max(b.min.x);
                cut.min.y = cut.min.y.max(b.min.y);
                cut.max.x = cut.max.x.min(b.max.x);
                cut.max.y = cut.max.y.min(b.max.y);
            }
            r.selection_clip.prepare(
                &r.device,
                encoder,
                extent,
                &std::sync::Arc::new(s.clone()),
            )?;
        }
        self.cut = cut;
        if selection.is_some() {
            let source = r.selection_clip.buffer.as_ref().unwrap();
            let target = self
                .selection
                .get_or_insert_with(|| selection_capture(&r.device, source.size()));
            if target.size() < source.size() {
                *target = selection_capture(&r.device, source.size());
            }
            encoder.copy_buffer_to_buffer(source, 0, target, 0, source.size());
        }
        for (channel, pages) in pages.into_iter().enumerate() {
            if channel == 0 || !pages.is_empty() {
                let bounds = if self.source_bounds[channel].is_empty() {
                    source_bounds
                } else {
                    self.source_bounds[channel]
                };
                self.sources[channel] = Some(TileSnapshot {
                    pages: pages.into_iter().collect(),
                    original: if channel == 0 { original.clone() } else { None },
                    backing: if channel == 0 { backing.clone() } else { None },
                    bounds,
                });
            }
        }
        for (channel, pages) in self.captures.iter_mut().enumerate() {
            pages.retain(|c, p| {
                self.sources[channel].as_ref().is_some_and(|s| {
                    s.pages
                        .get(c)
                        .is_some_and(|source| source.texture.format() == p.texture.format())
                })
            });
        }
        self.retain_pool_bindings();
        Ok(())
    }
    fn render_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        transform: &layer_core::ImageTransform,
        taps: u32,
        regions: &[PixelRect],
    ) -> Result<(), GpuRasterError> {
        if self.sources[0].is_none() || regions.iter().all(|b| b.is_empty()) {
            return Ok(());
        }
        let index = r.paint_layers.iter().position(|l| l.id == layer);
        let mesh = match &transform.map {
            layer_core::TransformMap::Mesh(map) => {
                let geometry = self.mesh_geometry(map, None);
                self.positions.upload(r, encoder, &geometry)?;
                Some(geometry)
            }
            _ => None,
        };
        let material = self.sources[1].is_some();
        let watercolor = self.sources[2].is_some();
        let support = self.channel_regions(transform, r.target_extent(layer));
        let coordinates: std::collections::BTreeSet<_> = regions
            .iter()
            .copied()
            .filter(|b| !b.is_empty())
            .flat_map(page_coordinates)
            .collect();
        let channel_pages: [Vec<[u32; 2]>; 3] = std::array::from_fn(|channel| {
            if self.sources[channel].is_none() {
                return Vec::new();
            }
            coordinates
                .iter()
                .copied()
                .filter(|c| {
                    channel == 0
                        || destination(r, layer, channel, false, *c).is_some()
                        || support[channel]
                            .iter()
                            .any(|b| !b.page_local(*c).is_empty())
                })
                .collect()
        });
        let mut initialize_original = std::collections::BTreeSet::new();
        for c in coordinates {
            if let Some(background) = self.background {
                if !r.layer_masks.pages.contains_key(&(layer, c)) {
                    let page = self
                        .spares
                        .masks
                        .pop()
                        .unwrap_or_else(|| layer_masks::MaskPage::new(&r.device));
                    r.encode_clear_value(
                        encoder,
                        &page.view,
                        "transformed visibility page",
                        background,
                    );
                    r.layer_masks.pages.insert((layer, c), page);
                }
                continue;
            }
            let index = index.ok_or(GpuRasterError::MissingPaintLayer(layer))?;
            if r.paint_layers[index]
                .pages
                .iter()
                .all(|p| p.coordinate != c)
            {
                let mut page = self
                    .spares
                    .paint
                    .pop()
                    .unwrap_or_else(|| r.create_page(c, "transformed paint page"));
                page.coordinate = c;
                page.active_secondary = false;
                if self.sources[0]
                    .as_ref()
                    .unwrap()
                    .original
                    .as_ref()
                    .is_some_and(|s| {
                        c[0] * PAGE_SIZE < s.extent[0] && c[1] * PAGE_SIZE < s.extent[1]
                    })
                {
                    initialize_original.insert(c);
                } else {
                    r.encode_clear(encoder, &page.primary.view, "initialize transformed paint");
                }
                page.primary_needs_clear = false;
                r.paint_layers[index].pages.push(page);
            }
            if material
                && support[1].iter().any(|b| !b.page_local(c).is_empty())
                && r.paint_layers[index]
                    .material_pages
                    .iter()
                    .all(|p| p.coordinate != c)
            {
                let mut page = self
                    .spares
                    .material
                    .pop()
                    .unwrap_or_else(|| CanvasMaterialPage {
                        coordinate: c,
                        wetness: r.create_scalar_page_surface("transformed material wetness"),
                        needs_clear: false,
                    });
                page.coordinate = c;
                page.needs_clear = false;
                r.encode_clear(
                    encoder,
                    &page.wetness.view,
                    "initialize transformed material",
                );
                r.paint_layers[index].material_pages.push(page);
            }
            if watercolor
                && support[2].iter().any(|b| !b.page_local(c).is_empty())
                && r.paint_layers[index]
                    .watercolor_wetness_pages
                    .iter()
                    .all(|p| p.coordinate != c)
            {
                let mut page =
                    self.spares
                        .watercolor
                        .pop()
                        .unwrap_or_else(|| WatercolorWetnessPage {
                            coordinate: c,
                            primary: r
                                .create_scalar_page_surface("transformed watercolor wetness A"),
                            secondary: r
                                .create_scalar_page_surface("transformed watercolor wetness B"),
                            active_secondary: false,
                            primary_needs_clear: false,
                            secondary_needs_clear: false,
                        });
                page.coordinate = c;
                page.active_secondary = false;
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
                r.encode_clear(
                    encoder,
                    &page.primary.view,
                    "initialize transformed watercolor A",
                );
                r.encode_clear(
                    encoder,
                    &page.secondary.view,
                    "initialize transformed watercolor B",
                );
                r.paint_layers[index].watercolor_wetness_pages.push(page);
            }
        }
        for (channel, pages) in channel_pages.into_iter().enumerate() {
            if pages.is_empty() {
                continue;
            }
            // Complete copies before sampling any original in this channel.
            for c in &pages {
                self.capture_destination(r, encoder, layer, channel, *c);
            }
            let mask = self.background.is_some();
            let targets = pages
                .into_iter()
                .filter_map(|c| {
                    let region = regions
                        .iter()
                        .map(|b| b.intersect(page_rect(c)))
                        .fold(PixelRect::EMPTY, PixelRect::union);
                    let (texture, _) = destination(r, layer, channel, mask, c)?;
                    let target = Target {
                        texture: texture.clone(),
                        region: region.page_local(c),
                        initialize: channel == 0 && initialize_original.remove(&c),
                    };
                    (!region.is_empty()).then_some((c, target))
                })
                .collect();
            let windows = self.plan_windows(r, channel, transform, mesh.as_ref(), targets)?;
            if channel == 0 && r.device.working_format().block_copy_size(None) == Some(4) {
                self.capture_originals(r, encoder, &windows)?;
            }
            self.draw_windows(r, encoder, channel, transform, taps, &windows, self.has_selection)?;
        }
        // The next stroke establishes fresh stroke-scoped accumulation. Keep
        // persistent wetness and the layer-level watercolor edge style intact.
        if let Some(index) = index {
            for p in &mut r.paint_layers[index].coverage_pages {
                p.owner = None;
            }
        }
        Ok(())
    }

    /// Group target pages into atlas windows and split each window's 2x2
    /// page blocks into jobs that fit the source bindings. Pages marked for
    /// initialization first receive their whole unmoved page.
    fn plan_windows(
        &self,
        r: &WgpuRasterizer,
        channel: usize,
        transform: &layer_core::ImageTransform,
        mesh: Option<&Arc<mesh::MeshGeometry>>,
        targets: Vec<([u32; 2], Target)>,
    ) -> Result<Vec<Window>, GpuRasterError> {
        let [columns, rows] = if mesh.is_some() {
            MESH_WINDOW
        } else {
            Atlas::pages(self.atlas_format(r, channel))
        };
        let mut windows = std::collections::BTreeMap::new();
        for (c, target) in targets {
            let key = [c[0] / columns, c[1] / rows];
            windows
                .entry(key)
                .or_insert_with(|| Window {
                    origin: [key[0] * columns, key[1] * rows],
                    jobs: Vec::new(),
                    pages: Vec::new(),
                })
                .pages
                .push((c, target));
        }
        let splitter = self.sources[channel]
            .as_ref()
            .unwrap()
            .splitter(transform, mesh.cloned())?;
        let mut found = Vec::new();
        for window in windows.values_mut() {
            let mut blocks = std::collections::BTreeMap::new();
            for (c, target) in &window.pages {
                let region = on_page(target.region, *c);
                blocks
                    .entry([c[0] / 2, c[1] / 2])
                    .and_modify(|b: &mut PixelRect| *b = b.union(region))
                    .or_insert(region);
            }
            window.jobs.extend(window.pages.iter().filter(|(_, t)| t.initialize).map(|(c, _)| {
                let job = snapshot::Footprint {
                    region: page_rect(*c),
                    sources: vec![*c],
                };
                (job, true)
            }));
            for start in blocks.into_values() {
                splitter.split(start, &mut found)?;
            }
            window.jobs.extend(found.drain(..).map(|job| (job, false)));
        }
        Ok(windows.into_values().collect())
    }

    fn atlas_format(&self, r: &WgpuRasterizer, channel: usize) -> wgpu::TextureFormat {
        if self.background.is_some() || channel != 0 {
            r.device.scalar_format()
        } else {
            r.device.working_format()
        }
    }

    /// Draw each window's jobs into the atlas, in as few passes as the source
    /// cache allows, then copy each page's region to its target.
    #[allow(clippy::too_many_arguments)]
    fn draw_windows(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        channel: usize,
        transform: &layer_core::ImageTransform,
        taps: u32,
        windows: &[Window],
        selected: bool,
    ) -> Result<(), GpuRasterError> {
        let format = self.atlas_format(r, channel);
        let scalar = format != r.device.working_format() || channel != 0;
        let atlas = self.atlases[usize::from(scalar)]
            .get_or_insert_with(|| Atlas::new(&r.device, format));
        let atlas = (atlas.texture.clone(), atlas.view.clone());
        let snapshot = self.sources[channel].as_ref().unwrap();
        let bounds = snapshot.source_bounds();
        let records: Vec<_> = windows
            .iter()
            .flat_map(|window| {
                window.jobs.iter().map(|(job, unmoved)| pixel_transform::TiledTransformRecord {
                    source_size: [PAGE_SIZE; 2],
                    target: window.origin,
                    sources: &job.sources,
                    texels: [0; 4],
                    unmoved: *unmoved,
                })
            })
            .collect();
        let meshed = matches!(transform.map, layer_core::TransformMap::Mesh(_));
        let positions = meshed
            .then(|| self.positions.view(&r.device, MESH_WINDOW.map(|n| n * PAGE_SIZE)));
        let pass = if self.background.is_some() {
            &mut self.visibility
        } else if channel == 0 {
            &mut self.color
        } else {
            &mut self.scalar
        };
        let offset = pass
            .prepare_tiled(
                &r.device,
                &mut r.uploads,
                encoder,
                bounds,
                self.background.unwrap_or(0.),
                transform,
                taps,
                &records,
                None,
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        let selection = selected.then_some(self.selection.as_ref()).flatten();
        let mut first = 0;
        for window in windows {
            let origin = window.origin.map(|v| v * PAGE_SIZE);
            if meshed {
                self.positions.draw(r, encoder, window.origin)?;
            }
            let snapshot = self.sources[channel].as_ref().unwrap();
            let batches = source_batches(&window.jobs, |c| snapshot.pages.contains_key(&c));
            for (n, batch) in batches.into_iter().enumerate() {
                let mut sources = Vec::with_capacity(batch.len());
                for (job, _) in &window.jobs[batch.clone()] {
                    let snapshot = self.sources[channel].as_ref().unwrap();
                    sources.push(snapshot.binding(
                        r,
                        pass,
                        &job.sources,
                        selection,
                        positions.as_ref(),
                        encoder,
                    )?);
                }
                let draws: Vec<_> = batch
                    .zip(&sources)
                    .map(|(index, source)| {
                        let (job, _) = &window.jobs[index];
                        pixel_transform::BatchDraw {
                            source,
                            job: first + index,
                            scissor: [
                                job.region.min_x() - origin[0],
                                job.region.min_y() - origin[1],
                                job.region.width(),
                                job.region.height(),
                            ],
                        }
                    })
                    .collect();
                pass.encode_batch(encoder, &atlas.1, n == 0, meshed, offset, &draws);
            }
            first += window.jobs.len();
            for (c, target) in &window.pages {
                let region = if target.initialize {
                    PixelRect::full([PAGE_SIZE; 2])
                } else {
                    target.region
                };
                let slot = [0, 1].map(|axis| (c[axis] - window.origin[axis]) * PAGE_SIZE);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &atlas.0,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: slot[0] + region.min_x(),
                            y: slot[1] + region.min_y(),
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &target.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: region.min_x(),
                            y: region.min_y(),
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: region.width(),
                        height: region.height(),
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        Ok(())
    }

    /// Decode each original photo or backing tile the jobs sample into a page
    /// owned by the transaction, once. Later frames bind those pages instead
    /// of cycling a large photo through the bounded source cache. An 8-bit
    /// working page costs a quarter of a cached Float32 tile.
    fn capture_originals(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        windows: &[Window],
    ) -> Result<(), GpuRasterError> {
        let snapshot = self.sources[0].as_ref().unwrap();
        let missing: std::collections::BTreeSet<_> = windows
            .iter()
            .flat_map(|window| window.jobs.iter())
            .flat_map(|(job, _)| job.sources.iter().copied())
            .filter(|c| !snapshot.pages.contains_key(c))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let format = r.device.working_format();
        let targets: Vec<_> = missing
            .into_iter()
            .map(|coordinate| {
                let capture = self.captures[0]
                    .entry(coordinate)
                    .or_insert_with(|| snapshot_page(&r.device, format));
                if capture.texture.format() != format {
                    *capture = snapshot_page(&r.device, format);
                }
                let target = Target {
                    texture: capture.texture.clone(),
                    region: PixelRect::full([PAGE_SIZE; 2]),
                    initialize: false,
                };
                (coordinate, target)
            })
            .collect();
        let identity = layer_core::ImageTransform::default();
        let copies = self.plan_windows(r, 0, &identity, None, targets)?;
        self.draw_windows(r, encoder, 0, &identity, pixel_transform::PREVIEW_TAPS, &copies, false)?;
        let snapshot = self.sources[0].as_mut().unwrap();
        for (coordinate, _) in copies.iter().flat_map(|window| &window.pages) {
            let capture = &self.captures[0][coordinate];
            snapshot.pages.insert(
                *coordinate,
                snapshot::SnapshotPage {
                    texture: capture.texture.clone(),
                    view: capture.view.clone(),
                },
            );
        }
        Ok(())
    }

    fn capture_destination(
        &mut self,
        r: &WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        channel: usize,
        coordinate: [u32; 2],
    ) {
        let Some((texture, _)) =
            destination(r, layer, channel, self.background.is_some(), coordinate)
        else {
            return;
        };
        let snapshot = self.sources[channel].as_mut().unwrap();
        if !snapshot.aliases(coordinate, texture) {
            return;
        }
        let capture = self.captures[channel]
            .entry(coordinate)
            .or_insert_with(|| snapshot_page(&r.device, texture.format()));
        if capture.texture.format() != texture.format() {
            *capture = snapshot_page(&r.device, texture.format());
        }
        encoder.copy_texture_to_texture(
            texture.as_image_copy(),
            capture.texture.as_image_copy(),
            texture.size(),
        );
        snapshot.pages.insert(
            coordinate,
            snapshot::SnapshotPage::of(&capture.texture, &capture.view),
        );
    }
    fn mesh_geometry(&mut self, map: &Arc<layer_core::MeshMap>, display: Option<f32>) -> Arc<mesh::MeshGeometry> {
        if let Some((cached, within, geometry)) = &self.mesh
            && *within == display
            && (Arc::ptr_eq(cached, map) || cached == map)
        {
            return geometry.clone();
        }
        let geometry = Arc::new(mesh::MeshGeometry::new(map, display));
        self.mesh = Some((map.clone(), display, geometry.clone()));
        geometry
    }
    fn release_snapshot(&mut self) {
        self.sources = Default::default();
        self.atlases = Default::default();
        self.reduced = None;
        self.standby = None;
        // Active copies follow the overwritten paint footprint. After the
        // transaction, retain only a small reusable pool, never every area
        // visited by unrelated transforms. Originals themselves stay tiled.
        for pages in &mut self.captures {
            while pages.len() > 64 {
                pages.pop_last();
            }
        }
        self.retain_pool_bindings();
    }
    fn retain_pool_bindings(&mut self) {
        let color: Vec<_> = self.captures[0].values().map(|p| &p.view).collect();
        let scalar: Vec<_> = self.captures[1..]
            .iter()
            .flat_map(|p| p.values().map(|p| &p.view))
            .collect();
        self.color
            .retain_source_bindings(&color, self.selection.as_ref());
        self.visibility
            .retain_source_bindings(&color, self.selection.as_ref());
        self.scalar
            .retain_source_bindings(&scalar, self.selection.as_ref());
    }

    pub fn discard_preview(&mut self) {
        self.preview = None;
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.displayed = None;
        self.settling = None;
        self.spares.clear();
        self.release_snapshot();
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    /// An identical committed operation can retain the already-rendered result.
    /// Any intervening paint or parameter change takes the normal replay path.
    pub fn matching_commit(&self, packet: FramePacket<'_>) -> Option<(LayerId, u32)> {
        let preview = self.preview.as_ref()?;
        if packet
            .dab_batches
            .iter()
            .any(|b| !matches!(b.kind, DabBatchKind::LayerOperation(_)))
        {
            return None;
        }
        let batch = packet
            .dab_batches
            .iter()
            .find(|b| b.layer_id == preview.layer)?;
        let DabBatchKind::LayerOperation(index) = batch.kind else {
            return None;
        };
        if batch.layer_id != preview.layer || !packet.dabs.is_empty() {
            return None;
        }
        let operation = packet
            .layers
            .iter()
            .find_map(|l| l.target_operations(preview.layer))?
            .get(index as usize)?;
        if !matches!(&operation.kind, layer_core::LayerOperationKind::Transform(t) if *t == *preview.drawn())
            || operation.coverage.initial != preview.selection
            || preview_taps(preview) != pixel_transform::exact_taps(&preview.drawn())
        {
            return None;
        }
        Some((preview.layer, index))
    }
    /// Restore before persistent edits; the same identity shader copies exact
    /// pigment/wetness, including fractional selection edges.
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<(LayerId, PixelRect)>, GpuRasterError> {
        self.displayed = None;
        self.settling = None;
        let Some(previous) = self.preview.take() else {
            return Ok(None);
        };
        let regions = self.preview_regions;
        // Removing the target already discarded its pixels. There is nothing
        // to restore, and cancellation must not turn deletion into a GPU error.
        if r.paint_layers.iter().any(|l| l.id == previous.layer)
            || r.layer_masks.definitions.contains_key(&previous.layer)
        {
            self.render_source(r, encoder, previous.layer, &Default::default(), pixel_transform::PREVIEW_TAPS, &regions)?;
        }
        self.retain_pages(r, previous.layer, &[], None);
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.spares.clear();
        self.release_snapshot();
        Ok(Some((previous.layer, regions[0].union(regions[1]))))
    }
    pub fn update_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layer: Option<&Layer>,
        extent: [u32; 2],
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        self.displayed = None;
        self.settling = None;
        if self.preview.as_ref() == Some(next) {
            return Ok(Vec::new());
        }
        let same_source = self.preview.as_ref().is_some_and(|p| {
            p.transaction == next.transaction
                && p.layer == next.layer
                && p.selection == next.selection
        }) || self.adopt_standby(next, layer);
        let mut damage = Vec::with_capacity(2);
        if !same_source {
            damage.extend(self.cancel_preview(r, encoder)?);
            self.capture_source(r, encoder, next.layer, next.selection.as_ref(), extent)?;
        }
        let regions = next
            .transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        let affected = [
            self.preview_regions[0],
            self.preview_regions[1],
            regions[0],
            regions[1],
        ];
        self.render_source(r, encoder, next.layer, &next.drawn(), preview_taps(next), &affected)?;
        // Drop only pages created for a previous preview and no longer needed.
        // Original sparse pages remain untouched outside the preview footprint.
        self.retain_pages(r, next.layer, &regions, Some(&next.transform));
        damage.push((
            next.layer,
            affected
                .into_iter()
                .fold(PixelRect::EMPTY, PixelRect::union),
        ));
        self.preview = Some(next.clone());
        self.preview_regions = regions;
        Ok(damage)
    }
    /// Whether this transaction's captured originals can draw `next` into a
    /// display level.
    fn displayable(&self, next: &layer_render::TransformPreview) -> bool {
        self.preview.as_ref().is_some_and(|p| {
            p.transaction == next.transaction
                && p.layer == next.layer
                && p.selection == next.selection
        }) && self.reducible()
    }
    /// Whether the captured originals are plain paint a display level can
    /// draw.
    fn reducible(&self) -> bool {
        self.background.is_none() && self.sources[0].is_some() && self.sources[1].is_none() && self.sources[2].is_none()
    }
    /// Capture `layer` for a Move drag of its `selection`, unless that is
    /// already captured, and reduce more of it to `level`. Returns whether
    /// pages remain to reduce.
    fn prepare_standby(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: &Layer,
        selection: &layer_core::Selection,
        extent: [u32; 2],
        level: u32,
    ) -> Result<bool, GpuRasterError> {
        if self.preview.is_some() {
            return Ok(false);
        }
        let standby = Standby::of(layer, selection);
        let next = standby.preview();
        if self.standby.as_ref() != Some(&standby) {
            self.capture_source(r, encoder, layer.id, next.selection.as_ref(), extent)?;
            self.standby = Some(standby);
        }
        if !self.reducible() {
            return Ok(false);
        }
        self.reduce(r, encoder, &next, Some(layer), extent, level)
    }
    /// Take over what was captured before `next`'s transaction began, when
    /// that was `layer`'s pixels and `next`'s selection.
    fn adopt_standby(&mut self, next: &layer_render::TransformPreview, layer: Option<&Layer>) -> bool {
        let adopted = self.preview.is_none()
            && layer.zip(next.selection.as_ref()).is_some_and(|(layer, selection)| {
                self.standby.as_ref() == Some(&Standby::of(layer, selection))
            });
        if adopted {
            self.standby = None;
            if let Some(reduced) = &mut self.reduced {
                reduced.transaction = next.transaction;
            }
        }
        adopted
    }
    fn render_display(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        extent: [u32; 2],
        placement: layer_core::Affine,
        level: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel,
        resample: &resample::Resample,
    ) -> Result<Option<PixelRect>, GpuRasterError> {
        if !self.displayable(next) {
            return Ok(None);
        }
        let display_level = display.side.trailing_zeros();
        let local = local_level(display_level, placement);
        let reduced = self.reduced.as_ref().is_some_and(|reduced| {
            reduced.transaction == next.transaction && reduced.level == local && reduced.pending.is_empty()
        });
        let placed = placement != layer_core::Affine::IDENTITY;
        let mesh = match &next.transform.map {
            layer_core::TransformMap::Mesh(map) => Some(map.clone()),
            _ => None,
        };
        if (placed || mesh.is_some()) && !reduced {
            return Ok(None);
        }
        if self.displayed.as_ref().is_some_and(|(shown, _)| shown == next) {
            return Ok(Some(PixelRect::EMPTY));
        }
        self.settling = None;
        let regions = next
            .transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        let previous = self.displayed.as_ref().map_or(self.preview_regions, |(_, shown)| *shown);
        let side = display.side;
        let drawn = [previous[0], previous[1], regions[0], regions[1]]
            .into_iter()
            .fold(PixelRect::EMPTY, PixelRect::union);
        let drawn = aligned(drawn, side, extent);
        self.displayed = Some((next.clone(), regions));
        let shown = if placed {
            aligned(pixel_rect(placement.bounds(drawn.to_rect()), display.extent), side, display.extent)
        } else {
            drawn
        };
        if shown.is_empty() {
            return Ok(Some(shown));
        }
        if reduced {
            let texels = texel_rect(shown, side);
            let at_level = pixel_transform::DisplayLevel {
                side: 1,
                extent: display.extent.map(|n| n.div_ceil(side)),
                ..display
            };
            let transform = resample_map(&next.transform, placement, local, display_level)?;
            let kept = layer_core::ImageTransform {
                keep_source: next.transform.keep_source,
                ..resample_map(&layer_core::ImageTransform::default(), placement, local, display_level)?
            };
            let clip = layer_core::Affine([side as f32, 0., 0., side as f32, 0., 0.])
                .then(placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?);
            let positions = match &mesh {
                Some(map) => {
                    let tolerance = 0.5 * side as f32 / magnification(placement).max(1e-6);
                    let geometry = self.mesh_geometry(map, Some(tolerance));
                    let texel = 1. / side as f32;
                    let to_texels = placement.then(layer_core::Affine([texel, 0., 0., texel, 0., 0.]));
                    let origin = [texels[0] as f32 - 1., texels[1] as f32 - 1.];
                    let view = self.display_positions.view(&r.device, [texels[2], texels[3]]);
                    self.display_positions.upload(r, encoder, &geometry)?;
                    self.display_positions
                        .draw_display(r, encoder, origin, to_texels, 1. / (1u32 << local) as f32)?;
                    Some(view)
                }
                None => None,
            };
            self.reduced.as_mut().unwrap().draw(
                r,
                resample,
                encoder,
                level,
                &transform,
                &kept,
                clip,
                extent,
                texels,
                at_level,
                positions.as_ref(),
            )?;
        } else {
            self.draw_exact(r, encoder, &next.drawn(), drawn, level, display, pixel_transform::Part::Whole)?;
        }
        Ok(Some(shown))
    }
    /// Draw `part` of `drawn`, whose corners lie on texel corners, by
    /// evaluating every layer pixel of every texel from the captured originals.
    #[allow(clippy::too_many_arguments)]
    fn draw_exact(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        transform: &layer_core::ImageTransform,
        drawn: PixelRect,
        level: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel,
        part: pixel_transform::Part,
    ) -> Result<(), GpuRasterError> {
        let side = display.side;
        let snapshot = self.sources[0].as_ref().unwrap();
        let splitter = snapshot.splitter(transform, None)?.aligned(side);
        let mut blocks = std::collections::BTreeMap::new();
        for c in page_coordinates(drawn) {
            blocks
                .entry([c[0] / 2, c[1] / 2])
                .and_modify(|b: &mut PixelRect| *b = b.union(page_rect(c).intersect(drawn)))
                .or_insert(page_rect(c).intersect(drawn));
        }
        let mut jobs = Vec::new();
        for block in blocks.into_values() {
            splitter.split(block, &mut jobs)?;
        }
        drop(splitter);
        let bounds = [
            snapshot.bounds.min_x() as i32,
            snapshot.bounds.min_y() as i32,
            snapshot.bounds.width() as i32,
            snapshot.bounds.height() as i32,
        ];
        let texels = |region| texel_rect(region, side);
        let records: Vec<_> = jobs
            .iter()
            .map(|job| pixel_transform::TiledTransformRecord {
                source_size: [PAGE_SIZE; 2],
                target: [0; 2],
                sources: &job.sources,
                texels: texels(job.region),
                unmoved: false,
            })
            .collect();
        let offset = self
            .color
            .prepare_tiled(
                &r.device,
                &mut r.uploads,
                encoder,
                bounds,
                0.,
                transform,
                pixel_transform::PREVIEW_TAPS,
                &records,
                Some((display, part)),
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        let selection = self.has_selection.then_some(self.selection.as_ref()).flatten();
        let captured = |c| snapshot.pages.contains_key(&c);
        let pieces: Vec<_> = jobs.into_iter().map(|job| (job, false)).collect();
        for batch in source_batches(&pieces, captured) {
            let mut sources = Vec::with_capacity(batch.len());
            for (job, _) in &pieces[batch.clone()] {
                let snapshot = self.sources[0].as_ref().unwrap();
                sources.push(snapshot.binding(r, &mut self.color, &job.sources, selection, None, encoder)?);
            }
            let draws: Vec<_> = batch
                .zip(&sources)
                .map(|(index, source)| pixel_transform::BatchDraw {
                    source,
                    job: index,
                    scissor: texels(pieces[index].0.region),
                })
                .collect();
            self.color.encode_display(&r.device, encoder, level, side, offset, &draws);
        }
        Ok(())
    }
    /// Whether `next`'s selection keeps some of the layer's pixels in place.
    fn keeps_pixels(&self, next: &layer_render::TransformPreview) -> bool {
        self.has_selection
            && !next
                .selection
                .as_ref()
                .is_some_and(|s| selects_all(s, self.sources[0].as_ref().unwrap().bounds))
    }
    /// Whether `next`'s layer still has to be reduced before drag frames.
    fn reduces(&self, next: &layer_render::TransformPreview) -> bool {
        self.displayable(next)
            && self
                .reduced
                .as_ref()
                .is_none_or(|r| r.transaction != next.transaction || !r.pending.is_empty())
    }
    /// Reduce, within this frame's preparation, more pages of the layer to
    /// display `level` for drag frames to resample, their originals decoded
    /// as they are reached. A page is reduced whole as the area mean of its
    /// pixels, except where a selection edge splits it; those pages are drawn
    /// exactly, the pixels the selection moves apart from those it keeps. A
    /// whole placed photo is copied instead from its placement preview when
    /// that already holds it reduced. Once complete, a still frame with
    /// `spares` reserves the pages a preview settles into. Returns whether
    /// pages remain to reduce.
    #[allow(clippy::too_many_arguments)]
    fn prepare_reduced(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layer: Option<&Layer>,
        extent: [u32; 2],
        level: u32,
        spares: bool,
    ) -> Result<bool, GpuRasterError> {
        if !self.displayable(next) {
            return Ok(false);
        }
        if self.reduce(r, encoder, next, layer, extent, level)? {
            return Ok(true);
        }
        self.color.reserve(&r.device, SETTLE_RECORDS);
        let format = self.atlas_format(r, 0);
        self.atlases[0].get_or_insert_with(|| Atlas::new(&r.device, format));
        if spares && !next.moving && !self.displayed.as_ref().is_some_and(|(shown, _)| shown.moving) {
            let pages = match &self.settling {
                Some((_, _, remaining)) => remaining.clone(),
                None => page_coordinates(self.sources[0].as_ref().unwrap().bounds).collect(),
            };
            self.reserve_spare_pages(r, next.layer, pages);
        }
        Ok(false)
    }
    /// Reduce more pages of the captured layer to `level` for `next`'s
    /// transaction. Returns whether pages remain to reduce.
    fn reduce(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layer: Option<&Layer>,
        extent: [u32; 2],
        level: u32,
    ) -> Result<bool, GpuRasterError> {
        if self
            .reduced
            .as_ref()
            .is_none_or(|reduced| reduced.transaction != next.transaction || reduced.level != level)
        {
            let kept = self.keeps_pixels(next);
            let placed = (!kept)
                .then(|| r.scene.as_ref()?.reduced_layer(r, layer?, extent, level).cloned())
                .flatten();
            let reduced = if let Some(texture) = placed {
                let reduced = resample::Reduced::new(r, next.transaction, level, extent, Vec::new(), false);
                encoder.copy_texture_to_texture(
                    texture.as_image_copy(),
                    reduced.image.texture.as_image_copy(),
                    texture.size(),
                );
                reduced
            } else {
                let snapshot = self.sources[0].as_ref().unwrap();
                let pending = page_coordinates(snapshot.bounds)
                    .filter(|c| snapshot.contains(*c))
                    .map(|c| page_rect(c).intersect(PixelRect::full(extent)))
                    .rev()
                    .collect();
                resample::Reduced::new(r, next.transaction, level, extent, pending, kept)
            };
            self.reduced = Some(reduced);
        }
        if !self.reduced.as_ref().unwrap().pending.is_empty() {
            let pipelines = r.display_pipelines.take().unwrap_or_else(|| display_mips::Pipelines::new(&r.device));
            let reduced = r.prepare(encoder, Work::Reduce, |r, encoder| {
                let Some(page) = self.reduced.as_mut().unwrap().pending.pop() else {
                    return Ok(false);
                };
                #[cfg(test)]
                r.test.reduced_pages.update(|n| n + 1);
                self.reduce_page(r, encoder, &pipelines, next, extent, level, page).map(|()| true)
            });
            r.display_pipelines = Some(pipelines);
            reduced?;
        }
        Ok(!self.reduced.as_ref().unwrap().pending.is_empty())
    }
    /// Reduce one page of the layer into the image of the pixels that move,
    /// or of those kept when the selection leaves the page out, or draw both
    /// exactly where a selection edge splits it.
    #[allow(clippy::too_many_arguments)]
    fn reduce_page(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        pipelines: &display_mips::Pipelines,
        next: &layer_render::TransformPreview,
        extent: [u32; 2],
        level: u32,
        page: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let snapshot = self.sources[0].as_ref().unwrap();
        let reduced = self.reduced.as_mut().unwrap();
        let moves = reduced.kept.is_none()
            || next.selection.as_ref().is_some_and(|s| selects_all(s, page.intersect(snapshot.bounds)));
        let kept = !moves && page.intersect(pixel_rect(self.cut, extent)).is_empty();
        let image = match &mut reduced.kept {
            Some(image) if kept => Some(image),
            _ => moves.then_some(&mut reduced.image),
        };
        if let Some(image) = image {
            let c = [page.min_x() / PAGE_SIZE, page.min_y() / PAGE_SIZE];
            if let Some(tile) = snapshot.original_page(r, c, encoder)? {
                image.write_tile(&r.device, pipelines, encoder, &tile.texture, [0; 2], c)?;
            }
            return Ok(());
        }
        let parts = [
            (pixel_transform::Part::Selected, reduced.image.view.clone()),
            (pixel_transform::Part::Kept, reduced.kept.as_ref().unwrap().view.clone()),
        ];
        let display = pixel_transform::DisplayLevel {
            side: 1 << level,
            opacity: 1.,
            extent,
            backdrop: [0.; 4],
        };
        let identity = layer_core::ImageTransform::default();
        #[cfg(test)]
        r.test.reduced_exactly.update(|n| n + 1);
        parts.iter().try_for_each(|(part, view)| self.draw_exact(r, encoder, &identity, page, view, display, *part))
    }
    /// Allocate, a few per still frame, the spare paint pages the preview
    /// settles into at `pages`, which settling waits for: those a layer
    /// lacks, until it settles, and then those its settling still needs.
    fn reserve_spare_pages(&mut self, r: &WgpuRasterizer, layer: LayerId, pages: Vec<[u32; 2]>) {
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer) else {
            return;
        };
        let present: std::collections::HashSet<_> = stored.pages.iter().map(|p| p.coordinate).collect();
        let needed: std::collections::HashSet<_> = pages.into_iter().filter(|c| !present.contains(c)).collect();
        let started = web_time::Instant::now();
        while self.spares.paint.len() < needed.len() && started.elapsed() < SPARE_PAGE_TIME {
            self.spares.paint.push(r.create_page([0; 2], "transformed paint page"));
        }
    }
    /// Whether drawing `layer`'s page at `coordinate` needs a new paint page.
    fn lacks_page(&self, r: &WgpuRasterizer, layer: LayerId, coordinate: [u32; 2]) -> bool {
        self.background.is_none()
            && r.paint_layers
                .iter()
                .find(|l| l.id == layer)
                .is_some_and(|l| l.pages.iter().all(|p| p.coordinate != coordinate))
    }
    fn settle(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        extent: [u32; 2],
    ) -> Result<Option<Vec<(LayerId, PixelRect)>>, GpuRasterError> {
        if self.settling.is_none() && self.preview.as_ref() == Some(next) {
            return Ok(Some(Vec::new()));
        }
        if self.settling.as_ref().is_none_or(|(settling, _, _)| settling != next) {
            let regions = next
                .transform
                .affected_regions(self.cut)
                .map(|b| pixel_rect(b, extent));
            let affected = [
                self.preview_regions[0],
                self.preview_regions[1],
                regions[0],
                regions[1],
            ];
            let remaining: std::collections::BTreeSet<_> =
                affected.iter().flat_map(|b| page_coordinates(*b)).collect();
            self.preview_regions = [
                self.preview_regions[0].union(regions[0]),
                self.preview_regions[1].union(regions[1]),
            ];
            self.settling = Some((next.clone(), affected, remaining.into_iter().rev().collect()));
        }
        let affected = self.settling.as_ref().unwrap().1;
        let drawn = next.drawn();
        r.prepare(encoder, Work::Settle, |r, encoder| {
            let Some(&c) = self.settling.as_ref().unwrap().2.last() else {
                return Ok(false);
            };
            if self.spares.paint.is_empty() && self.lacks_page(r, next.layer, c) {
                return Ok(false);
            }
            self.settling.as_mut().unwrap().2.pop();
            let regions: Vec<_> = affected.map(|b| b.intersect(page_rect(c))).into_iter().filter(|b| !b.is_empty()).collect();
            self.render_source(r, encoder, next.layer, &drawn, preview_taps(next), &regions)?;
            Ok(true)
        })?;
        if !self.settling.as_ref().unwrap().2.is_empty() {
            return Ok(None);
        }
        let regions = next
            .transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        self.retain_pages(r, next.layer, &regions, Some(&next.transform));
        self.settling = None;
        self.preview = Some(next.clone());
        self.preview_regions = regions;
        Ok(Some(vec![(next.layer, affected.into_iter().fold(PixelRect::EMPTY, PixelRect::union))]))
    }
    fn channel_regions(
        &self,
        transform: &layer_core::ImageTransform,
        extent: [u32; 2],
    ) -> [[PixelRect; 2]; 3] {
        self.source_bounds.map(|b| {
            let cut = layer_core::Rect {
                min: layer_core::Point {
                    x: (b.min_x() as f32).max(self.cut.min.x),
                    y: (b.min_y() as f32).max(self.cut.min.y),
                },
                max: layer_core::Point {
                    x: (b.max_x() as f32).min(self.cut.max.x),
                    y: (b.max_y() as f32).min(self.cut.max.y),
                },
            };
            transform
                .affected_regions(cut)
                .map(|b| pixel_rect(b, extent))
        })
    }
    fn retain_pages(
        &mut self,
        r: &mut WgpuRasterizer,
        id: LayerId,
        regions: &[PixelRect],
        transform: Option<&layer_core::ImageTransform>,
    ) {
        let support = transform
            .map(|t| self.channel_regions(t, r.target_extent(id)))
            .unwrap_or([[PixelRect::EMPTY; 2]; 3]);
        let keep = |c: [u32; 2], original: &Vec<_>, regions: &[PixelRect]| {
            original.contains(&c) || regions.iter().any(|b| !b.page_local(c).is_empty())
        };
        if self.background.is_some() {
            self.spares.masks.extend(
                r.layer_masks
                    .pages
                    .extract_if(.., |(mask, c), _| {
                        *mask == id && !keep(*c, &self.original_pages[0], regions)
                    })
                    .map(|(_, page)| page),
            );
        } else if let Some(layer) = r.paint_layers.iter_mut().find(|l| l.id == id) {
            self.spares.paint.extend(layer.pages.extract_if(.., |p| {
                !keep(p.coordinate, &self.original_pages[0], regions)
            }));
            self.spares
                .material
                .extend(layer.material_pages.extract_if(.., |p| {
                    !keep(p.coordinate, &self.original_pages[1], &support[1])
                }));
            self.spares
                .watercolor
                .extend(layer.watercolor_wetness_pages.extract_if(.., |p| {
                    !keep(p.coordinate, &self.original_pages[2], &support[2])
                }));
        }
    }
}
fn snapshot_page(device: &wgpu::Device, format: wgpu::TextureFormat) -> snapshot::SnapshotPage {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("reusable transform copy before overwrite"),
        size: wgpu::Extent3d {
            width: PAGE_SIZE,
            height: PAGE_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    snapshot::SnapshotPage { texture, view }
}

fn destination(
    r: &WgpuRasterizer,
    id: LayerId,
    channel: usize,
    mask: bool,
    coordinate: [u32; 2],
) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
    if mask {
        return r
            .layer_masks
            .pages
            .get(&(id, coordinate))
            .map(|p| (&p.texture, &p.view));
    }
    let layer = r.paint_layers.iter().find(|l| l.id == id)?;
    let surface = match channel {
        0 => layer
            .pages
            .iter()
            .find(|p| p.coordinate == coordinate)?
            .active(),
        1 => {
            &layer
                .material_pages
                .iter()
                .find(|p| p.coordinate == coordinate)?
                .wetness
        }
        _ => layer
            .watercolor_wetness_pages
            .iter()
            .find(|p| p.coordinate == coordinate)?
            .active(),
    };
    Some((&surface.texture, &surface.view))
}

type TexturePages = [Vec<([u32; 2], snapshot::SnapshotPage)>; 3];
fn source_pages(
    r: &WgpuRasterizer,
    id: LayerId,
) -> Result<(Option<f32>, TexturePages), GpuRasterError> {
    if let Some(mask) = r.layer_masks.definitions.get(&id) {
        return Ok((
            Some(mask.default_coverage),
            [
                r.layer_masks
                    .pages
                    .iter()
                    .filter(|((target, _), _)| *target == id)
                    .map(|((_, c), p)| (*c, snapshot::SnapshotPage::of(&p.texture, &p.view)))
                    .collect(),
                Vec::new(),
                Vec::new(),
            ],
        ));
    }
    let l = r
        .paint_layers
        .iter()
        .find(|l| l.id == id)
        .ok_or(GpuRasterError::MissingPaintLayer(id))?;
    Ok((
        None,
        [
            l.pages
                .iter()
                .map(|p| (p.coordinate, snapshot::SnapshotPage::of(&p.active().texture, &p.active().view)))
                .collect(),
            l.material_pages
                .iter()
                .map(|p| (p.coordinate, snapshot::SnapshotPage::of(&p.wetness.texture, &p.wetness.view)))
                .collect(),
            l.watercolor_wetness_pages
                .iter()
                .map(|p| (p.coordinate, snapshot::SnapshotPage::of(&p.active().texture, &p.active().view)))
                .collect(),
        ],
    ))
}
fn selection_capture(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("immutable transform selection"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
