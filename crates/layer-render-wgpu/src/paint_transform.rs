//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
//! Linked paint and mask targets run the same transaction with separate origins.
use super::*;
use pixel_transform::PixelTransform;
pub(super) mod mesh;
pub(crate) use mesh::MeshBuffers;
use crate::scene::resample;
pub(super) mod snapshot;
pub(crate) mod sampling;
pub(crate) use sampling::input_level;
use snapshot::TileSnapshot;

pub(super) struct PaintTransforms([ImageTransformState; 2]);
impl PaintTransforms {
    pub(super) fn placement_pass(&self, scalar: bool) -> PixelTransform {
        if scalar { self.0[0].scalar.placement_pass() } else { self.0[0].color.placement_pass() }
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let primary = ImageTransformState::new(device);
        let companion = primary.fork();
        Self([primary, companion])
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
    pub fn display_pipelines(&self) -> [&Deferred<wgpu::ComputePipeline>; 1] {
        [self.0[0].color.display.as_ref().expect("color transform")]
    }
    pub fn placement_pipelines(&self) -> [&Deferred<wgpu::ComputePipeline>; 2] {
        [&self.0[0].color.placement_pipeline, &self.0[0].scalar.placement_pipeline]
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
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        operation: &layer_core::RasterOperation,
    ) -> Result<(), GpuRasterError> {
        self.0[0].apply(r, encoder, layer, operation, r.target_extent(layer))
    }
    pub fn consume_commit(&mut self, packet: FramePacket<'_>) -> Vec<(SourceTarget, u32)> {
        let Some(matches) = self
            .0
            .iter()
            .filter(|t| t.has_preview())
            .map(|t| t.matching_commit(packet))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        if !matches.is_empty() { self.discard_preview(); }
        matches
    }
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        let mut damage = Vec::with_capacity(2);
        for t in &mut self.0 {
            damage.extend(t.cancel_preview(r, encoder)?);
        }
        Ok(damage)
    }
    pub fn update_preview(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview, scene: SceneView<'_>, level: Option<u32>,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        let mut damage = Vec::new();
        for (state, next) in self.0.iter_mut().zip([Some(next.clone()), next.companion(scene)]) {
            if let Some(next) = next {
                let extent = r.target_extent(next.target);
                let native = level.is_none() || r.layer_masks.definitions.contains_key(&next.target);
                damage.extend(state.update_preview(r, encoder, &next, Some((scene, next.target)), extent, native)?);
                if let Some(level) = level.filter(|_| !native) {
                    let local = input_level(level, &next, &scene.target_geometry(next.target), extent);
                    state.prepare_reduced(r, encoder, next.selection.as_ref(), Some((scene, next.target)), extent, local)?;
                }
            } else { damage.extend(state.cancel_preview(r, encoder)?); }
        }
        Ok(damage)
    }
    pub fn prepare_standby(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection, level: u32,
    ) -> Result<(), GpuRasterError> {
        if !matches!(id, SourceTarget::Paint(_)) || scene.raster(id).is_none() { return Ok(()); }
        let state = &mut self.0[0];
        if state.preview.is_some() { return Ok(()); }
        let extent = r.target_extent(id);
        let standby = Standby::of(scene, id, selection);
        if state.standby.as_ref() != Some(&standby) {
            state.capture_source(r, encoder, id, Some(selection), extent)?;
            state.standby = Some(standby);
        }
        let local = sampling::selection_level(level, &scene.target_geometry(id), Some(selection), extent);
        state.prepare_reduced(r, encoder, Some(selection), Some((scene, id)), extent, local)
    }
    pub fn release_standby(&mut self) {
        for state in &mut self.0 {
            if state.preview.is_none() && state.standby.is_some() { state.release_snapshot(); }
        }
    }
    pub fn standby_requirements(&self, scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection, requested: u32, extent: [u32; 2]) -> (u32, bool) {
        let key = Standby::of(scene, id, selection);
        let state = self.0.iter().find(|state| state.standby.as_ref() == Some(&key));
        let level = state.and_then(|state| state.reduced.as_ref()).map_or(requested, |input| input.level.min(requested));
        let bounds = state.and_then(|state| state.sources[0].as_ref()).map_or(PixelRect::full(extent), |s| s.bounds);
        (level, keeps_pixels(Some(selection), bounds))
    }
    pub fn display_source(&self, id: SourceTarget) -> bool {
        self.0.iter().any(|t| t.preview.as_ref().is_some_and(|p| p.target == id) && !t.native_preview)
    }
    pub fn direct_source(&self, id: SourceTarget) -> bool {
        self.0.iter().any(|t| t.preview.as_ref().is_some_and(|p| p.target == id
            && !p.transform.keep_source && p.transform.placement.mesh.is_none())
            && !t.native_preview && t.reduced.as_ref().is_some_and(|input| input.kept.is_none()))
    }
    pub fn input_requirements(&self, preview: &layer_render::TransformPreview, requested: u32, extent: [u32; 2]) -> (u32, bool) {
        let state = self.0.iter().find(|t| t.displayable(preview));
        let level = state.and_then(|t| t.reduced.as_ref()).map_or(requested, |input| input.level.min(requested));
        let kept = state.map_or_else(|| keeps_pixels(preview.selection.as_ref(), PixelRect::full(extent)), |t| t.keeps_pixels(preview));
        (level, kept)
    }
    pub fn materialize_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        scene: SceneView<'_>, region: PixelRect,
    ) -> Result<(), GpuRasterError> {
        for state in self.0.iter_mut().filter(|t| !t.native_preview && t.preview.is_some()) {
            let preview = state.preview.clone().unwrap();
            let extent = r.target_extent(preview.target);
            let transform = scene.target_geometry(preview.target);
            let mesh = self::mesh_geometry_for(&transform);
            let bounds = PixelRect::full(extent);
            let splitter = snapshot::Splitter::new(bounds, &transform, mesh, |c| !page_rect(c).intersect(bounds).is_empty())?;
            let mut jobs = Vec::new();
            splitter.split(aligned(region, PAGE_SIZE, r.document_extent), &mut jobs)?;
            let pages: std::collections::BTreeSet<_> = jobs.into_iter().flat_map(|job| job.sources).collect();
            let retired: Vec<_> = state.queried.difference(&pages).copied().map(page_rect).collect();
            state.render_source(r, encoder, preview.target, &Default::default(), pixel_transform::PREVIEW_TAPS, &retired)?;
            let missing: Vec<_> = pages.difference(&state.queried).map(|c| page_rect(*c).intersect(bounds)).collect();
            state.render_source(r, encoder, preview.target, &preview.drawn(), preview_taps(&preview), &missing)?;
            let retained: Vec<_> = pages.iter().copied().map(page_rect).collect();
            state.retain_pages(r, preview.target, &retained, Some(&preview.transform));
            state.queried = pages;
        }
        Ok(())
    }
    pub fn presentation(&self, id: SourceTarget, placement: layer_core::Affine, extent: [u32; 2], opacity: f32, backdrop: [f32; 4], encode: bool)
        -> Result<resample::Mapped, GpuRasterError> {
        let state = self.0.iter().find(|t| t.preview.as_ref().is_some_and(|p| p.target == id)).unwrap();
        let preview = state.preview.as_ref().unwrap();
        let inputs = state.reduced.as_ref().unwrap();
        let display = pixel_transform::DisplayLevel { side: 1, extent, opacity, backdrop, encode };
        let values = state.display_record(preview, extent, placement, display, display_mips::Plan::at(extent, 0), [0; 4])?;
        Ok(resample::Mapped { view: inputs.sampling[0].clone(), values })
    }
    #[expect(clippy::too_many_arguments, reason = "Paint transforms keep source placement and output sampling regions explicit")]
    pub fn render_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        id: SourceTarget, placement: layer_core::Affine, output: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel, target: display_mips::Plan, region: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let state = self.0.iter_mut().find(|t| t.preview.as_ref().is_some_and(|p| p.target == id)).unwrap();
        let preview = state.preview.clone().unwrap();
        let resample = r.scene_pipelines.resample.clone();
        state.render_region(r, encoder, &preview, r.target_extent(id), placement, output, display, target, region, &resample)
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
    native_preview: bool,
    queried: std::collections::BTreeSet<[u32; 2]>,
    reduced: Option<snapshot::DisplayInputs>,
    standby: Option<Standby>,
    spares: PreviewPages,
    atlases: [Option<Atlas>; 2],
    /// Mesh positions for each window of pages a warp draws.
    positions: mesh::Positions,
    display_mesh: MeshBuffers,
    /// The warp mesh last tessellated, the display tolerance it was
    /// tessellated within, if any, and its geometry.
    mesh: Option<(layer_core::LayerPlacement, Option<f32>, Arc<mesh::MeshGeometry>)>,
}

#[derive(PartialEq)]
struct Standby {
    layer: SourceTarget,
    raster: u64,
    source: Option<usize>,
    selection: layer_core::Selection,
}
impl Standby {
    fn of(scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection) -> Self {
        Self {
            layer: id,
            raster: scene.raster(id).map_or(0, |r| r.identity()),
            source: scene.original(id).map(|s| Arc::as_ptr(s) as usize),
            selection: selection.clone(),
        }
    }
}

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

/// `transform` followed by `placement`, from texels of the layer reduced to
/// `local` to texels of display `level`, sampled bilinearly. A warp resamples
/// from mesh positions instead and maps through the placement alone.
pub(crate) fn resample_map(
    transform: &layer_core::ImageTransform,
    placement: layer_core::Affine,
    local: u32,
    level: u32,
) -> Result<layer_core::ImageTransform, GpuRasterError> {
    if transform.placement.mesh.is_some() {
        let scale = 1. / (1u32 << local) as f32;
        let mut result = transform.clone();
        result.source_from_owner = Some(transform.source_from_owner.unwrap_or(layer_core::Projective::IDENTITY)
            .then(layer_core::Projective([scale,0.,0.,0.,scale,0.,0.,0.,1.])).ok_or(GpuRasterError::InvalidTransform("Invalid source placement"))?);
        return Ok(result);
    }
    let moved = transform.projective().unwrap_or(layer_core::Projective::IDENTITY);
    let scale = |s: f32| layer_core::Projective([s, 0., 0., 0., s, 0., 0., 0., 1.]);
    let map = [moved, layer_core::Projective::from_affine(placement), scale(1. / (1u32 << level) as f32)]
        .into_iter()
        .try_fold(scale((1u32 << local) as f32), layer_core::Projective::then)
        .ok_or(GpuRasterError::InvalidTransform("Transform must be finite and invertible"))?;
    Ok(layer_core::ImageTransform { placement: layer_core::LayerPlacement { interpolation: layer_core::Interpolation::Linear, ..layer_core::LayerPlacement::from_projective(map) }, ..Default::default() })
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

pub(crate) fn keeps_pixels(selection: Option<&layer_core::Selection>, bounds: PixelRect) -> bool {
    selection.is_some_and(|selection| !selects_all(selection, bounds))
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
            native_preview: false,
            queried: Default::default(),
            reduced: None,
            standby: None,
            spares: PreviewPages::default(),
            atlases: Default::default(),
            display_mesh: MeshBuffers::default(),
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
            + self.display_mesh.storage_bytes()
            + self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scalar.storage_bytes()
            + self.visibility.storage_bytes()
            + self.reduced.as_ref().map_or(0, snapshot::DisplayInputs::storage_bytes)
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
        layer: SourceTarget,
        operation: &layer_core::RasterOperation,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        let layer_core::RasterOperationKind::Transform(transform) = &operation.kind else {
            unreachable!()
        };
        if transform.is_identity() {
            return Ok(());
        }
        self.capture_source(
            r,
            encoder,
            layer,
            operation.coverage.source.initial.as_ref(),
            extent,
        )?;
        let regions = transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        let taps = pixel_transform::exact_taps(transform);
        let result = self.render_source(r, encoder, layer, transform, taps, &regions);
        if result.is_ok() && self.moves_everything(operation.coverage.source.initial.as_ref()) {
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
        layer: SourceTarget,
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
        let planes = [layer_core::raster::RasterPlane::Color, layer_core::raster::RasterPlane::Wetness,
            layer_core::raster::RasterPlane::WatercolorWetness];
        if let Some((data, _)) = &backing {
            for (i, plane) in planes.iter().enumerate() {
                for key in data.tiles.keys().filter(|key| key.plane == *plane) {
                    if !self.original_pages[i].contains(&key.coordinate) { self.original_pages[i].push(key.coordinate); }
                    self.source_bounds[i] = self.source_bounds[i].union(page_rect(key.coordinate).intersect(PixelRect::full(extent)));
                }
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
            if channel == 0 || !self.source_bounds[channel].is_empty() {
                let bounds = if self.source_bounds[channel].is_empty() {
                    source_bounds
                } else {
                    self.source_bounds[channel]
                };
                self.sources[channel] = Some(TileSnapshot {
                    pages: pages.into_iter().collect(),
                    original: if channel == 0 { original.clone() } else { None },
                    backing: backing.as_ref().map(|(data, space)| (data.clone(), *space, planes[channel])),
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
        layer: SourceTarget,
        transform: &layer_core::ImageTransform,
        taps: u32,
        regions: &[PixelRect],
    ) -> Result<(), GpuRasterError> {
        if self.sources[0].is_none() || regions.iter().all(|b| b.is_empty()) {
            return Ok(());
        }
        let index = r.paint_layers.iter().position(|l| l.id == layer);
        let mesh = match &transform.placement.mesh {
            Some(_) => {
                let geometry = self.mesh_geometry(&transform.placement, None);
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
                    clear: false,
                })
            })
            .collect();
        let meshed = transform.placement.mesh.is_some();
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
        layer: SourceTarget,
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
    fn mesh_geometry(&mut self, map: &layer_core::LayerPlacement, display: Option<f32>) -> Arc<mesh::MeshGeometry> {
        if let Some((cached, within, geometry)) = &self.mesh
            && *within == display
            && cached == map
        {
            return geometry.clone();
        }
        let geometry = Arc::new(mesh::MeshGeometry::new(map.mesh.as_ref().unwrap(), map.outer, display));
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
        self.native_preview = false;
        self.queried.clear();
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.spares.clear();
        self.release_snapshot();
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    /// An identical committed operation can retain the already-rendered result.
    /// Any intervening paint or parameter change takes the normal replay path.
    pub fn matching_commit(&self, packet: FramePacket<'_>) -> Option<(SourceTarget, u32)> {
        if !self.native_preview { return None; }
        let preview = self.preview.as_ref()?;
        if packet
            .dab_batches
            .iter()
            .any(|b| !matches!(b.kind, DabBatchKind::RasterOperation(_)))
        {
            return None;
        }
        let batch = packet
            .dab_batches
            .iter()
            .find(|b| b.target == preview.target)?;
        let DabBatchKind::RasterOperation(index) = batch.kind else {
            return None;
        };
        if batch.target != preview.target || !packet.dabs.is_empty() {
            return None;
        }
        let operation = packet.scene.operations(preview.target)?.get(index as usize)?;
        if !matches!(&operation.kind, layer_core::RasterOperationKind::Transform(t) if t == preview.drawn().as_ref())
            || operation.coverage.source.initial != preview.selection
            || preview_taps(preview) != pixel_transform::exact_taps(&preview.drawn())
        {
            return None;
        }
        Some((preview.target, index))
    }
    /// Restore before persistent edits; the same identity shader copies exact
    /// pigment/wetness, including fractional selection edges.
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<(SourceTarget, PixelRect)>, GpuRasterError> {
        let Some(previous) = self.preview.take() else {
            return Ok(None);
        };
        let regions = self.preview_regions;
        // Removing the target already discarded its pixels. There is nothing
        // to restore, and cancellation must not turn deletion into a GPU error.
        if (self.native_preview || !self.queried.is_empty()) && (r.paint_layers.iter().any(|l| l.id == previous.target)
            || r.layer_masks.definitions.contains_key(&previous.target))
        {
            let restored: Vec<_> = if self.native_preview { regions.to_vec() } else { self.queried.iter().copied().map(page_rect).collect() };
            self.render_source(r, encoder, previous.target, &Default::default(), pixel_transform::PREVIEW_TAPS, &restored)?;
        }
        self.retain_pages(r, previous.target, &[], None);
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.native_preview = false;
        self.queried.clear();
        self.spares.clear();
        self.release_snapshot();
        Ok(Some((previous.target, regions[0].union(regions[1]))))
    }
    pub fn update_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layer: Option<(SceneView<'_>, SourceTarget)>, extent: [u32; 2], native: bool,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        if self.preview.as_ref() == Some(next) && self.native_preview == native {
            return Ok(Vec::new());
        }
        let same_source = self.preview.as_ref().is_some_and(|p| {
            p.transaction == next.transaction
                && p.target == next.target
                && p.selection == next.selection
        }) || (self.preview.is_none() && layer.zip(next.selection.as_ref())
            .is_some_and(|((scene, id), selection)| self.standby.as_ref() == Some(&Standby::of(scene, id, selection))));
        self.standby = None;
        let mut damage = Vec::with_capacity(2);
        if !same_source {
            damage.extend(self.cancel_preview(r, encoder)?);
            self.capture_source(r, encoder, next.target, next.selection.as_ref(), extent)?;
        }
        let regions = next
            .transform
            .affected_regions(self.cut)
            .map(|b| pixel_rect(b, extent));
        let cut = if !same_source || self.native_preview != native || self.preview_regions[0] != regions[0] {
            self.preview_regions[0].union(regions[0])
        } else { PixelRect::EMPTY };
        let affected = [cut, self.preview_regions[1], regions[1]];
        if native {
            self.render_source(r, encoder, next.target, &next.drawn(), preview_taps(next), &affected)?;
            self.retain_pages(r, next.target, &regions, Some(&next.transform));
        } else if self.native_preview || !self.queried.is_empty() {
            let restored: Vec<_> = if self.native_preview { self.preview_regions.to_vec() } else { self.queried.iter().copied().map(page_rect).collect() };
            self.render_source(r, encoder, next.target, &Default::default(), pixel_transform::PREVIEW_TAPS, &restored)?;
            self.retain_pages(r, next.target, &[], None);
        }
        self.native_preview = native;
        self.queried.clear();
        damage.push((
            next.target,
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
                && p.target == next.target
                && p.selection == next.selection
        }) && self.reducible()
    }
    fn reducible(&self) -> bool { self.background.is_none() && self.sources[0].is_some() }
    #[expect(clippy::too_many_arguments, reason = "Transform previews keep source level, preview geometry, and output sampling regions explicit")]
    fn render_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview, extent: [u32; 2], placement: layer_core::Affine,
        level: &wgpu::TextureView, display: pixel_transform::DisplayLevel, target: display_mips::Plan, region: PixelRect,
        resample: &resample::Resample,
    ) -> Result<(), GpuRasterError> {
        let side = display.side;
        let mesh = next.transform.placement.mesh.clone();
        let texels = texel_rect(region.window_local(target.bounds), side);
        let values = self.display_record(next, extent, placement, display, target, texels)?;
        if mesh.is_some() {
            let tolerance = 0.5 * side as f32 / placement.magnification().max(1e-6);
            let geometry = self.mesh_geometry(&next.transform.placement, Some(tolerance));
            self.display_mesh.upload(r, encoder, &geometry)?;
        }
        self.reduced.as_mut().unwrap().draw(r, resample, encoder, level, &values, texels, mesh.as_ref().map(|_| &self.display_mesh))
    }
    fn display_record(&self, next: &layer_render::TransformPreview, extent: [u32; 2], placement: layer_core::Affine,
        display: pixel_transform::DisplayLevel, target: display_mips::Plan, texels: [u32; 4],
    ) -> Result<[u8; resample::UNIFORM_BYTES as usize], GpuRasterError> {
        let inputs = self.reduced.as_ref().unwrap();
        let display_level = display.side.trailing_zeros();
        let moved = resample_map(&next.transform, placement, inputs.level, display_level)?;
        let kept = resample_map(&layer_core::ImageTransform::default(), placement, inputs.level, display_level)?;
        let side = display.side as f32;
        let clip = layer_core::Affine([side, 0., 0., side, 0., 0.])
            .then(placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?);
        resample::Resample::values(resample::Request { moved: &moved, kept: &kept, clip, extent, texels, display, target,
            source: inputs.image.plan, max_lod: inputs.image.last_level()-inputs.image.plan.level, outside: 0.,
            keep_source: next.transform.keep_source, identity: next.transform.is_identity() })
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
                clear: false,
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
        keeps_pixels(next.selection.as_ref(), self.sources[0].as_ref().unwrap().bounds)
    }
    fn prepare_reduced(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        selection: Option<&layer_core::Selection>, layer: Option<(SceneView<'_>, SourceTarget)>, extent: [u32; 2], level: u32,
    ) -> Result<(), GpuRasterError> {
        if !self.reducible() || self.reduced.as_ref().is_some_and(|input| input.level <= level) {
            return Ok(());
        }
        let kept = keeps_pixels(selection, self.sources[0].as_ref().unwrap().bounds);
        let cached = (!kept).then(|| { let (scene, target) = layer?; r.scene.as_ref()?.reduced_layer(r, scene, target, extent, level) }.cloned()).flatten();
        let input = snapshot::DisplayInputs::new(r, level, extent, kept);
        if let Some(texture) = &cached {
            encoder.copy_texture_to_texture(texture.as_image_copy(), input.image.texture.as_image_copy(), texture.size());
        }
        self.reduced = Some(input);
        let snapshot = self.sources[0].as_ref().unwrap();
        let pages: Vec<_> = page_coordinates(snapshot.bounds).filter(|c| snapshot.contains(*c))
            .map(|c| page_rect(c).intersect(PixelRect::full(extent))).collect();
        let pipelines = r.display_pipelines.take().unwrap_or_else(|| display_mips::Pipelines::new(&r.device));
        let result = pages.into_iter().filter(|_| cached.is_none()).try_for_each(|page| {
            #[cfg(test)]
            r.test.reduced_pages.update(|n| n + 1);
            self.reduce_page(r, encoder, &pipelines, selection, extent, level, page)
        });
        if result.is_ok() {
            let input = self.reduced.as_ref().unwrap();
            for image in std::iter::once(&input.image).chain(input.kept.iter()) {
                image.generate_mips(&r.device, &pipelines, encoder);
            }
        }
        r.display_pipelines = Some(pipelines);
        result
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
        selection: Option<&layer_core::Selection>,
        extent: [u32; 2],
        level: u32,
        page: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let snapshot = self.sources[0].as_ref().unwrap();
        let reduced = self.reduced.as_mut().unwrap();
        let moves = reduced.kept.is_none()
            || selection.is_some_and(|s| selects_all(s, page.intersect(snapshot.bounds)));
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
            encode: false,
        };
        let identity = layer_core::ImageTransform::default();
        parts.iter().try_for_each(|(part, view)| self.draw_exact(r, encoder, &identity, page, view, display, *part))
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
        id: SourceTarget,
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
    id: SourceTarget,
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
    id: SourceTarget,
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

fn mesh_geometry_for(transform: &layer_core::ImageTransform) -> Option<std::sync::Arc<mesh::MeshGeometry>> {
    transform.placement.mesh.as_ref().map(|mesh| std::sync::Arc::new(mesh::MeshGeometry::new(mesh, transform.placement.outer, None)))
}
