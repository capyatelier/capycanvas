//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
//! Linked paint and mask targets run the same transaction with separate origins.
use super::*;
use pixel_transform::{PixelTransform, TransformSource, TransformTarget};
mod snapshot;
use snapshot::TileSnapshot;

pub(super) struct PaintTransforms([ImageTransformState; 2]);
impl PaintTransforms {
    pub fn new(device: &PipelineDevice) -> Self {
        let primary = ImageTransformState::new(device);
        let companion = primary.fork();
        Self([primary, companion])
    }
    pub fn pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 3] {
        self.0[0].pipelines()
    }
    pub fn begin_frame(&mut self) {
        for t in &mut self.0 {
            t.begin_frame();
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        // Empty selection and ordered source-record buffers are shared by both targets.
        self.0
            .iter()
            .map(ImageTransformState::storage_bytes)
            .sum::<u64>()
            - 3 * 48
            - self.0[0].color.shared_source_bytes(&self.0[1].color)
            - self.0[0].scalar.shared_source_bytes(&self.0[1].scalar)
            - self.0[0]
                .visibility
                .shared_source_bytes(&self.0[1].visibility)
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
    pub fn source_captures(&self) -> u64 {
        self.0.iter().map(|t| t.source_captures).sum()
    }
    #[cfg(test)]
    pub fn spare_page_bytes(&self) -> u64 {
        self.0.iter().map(|t| t.spares.storage_bytes()).sum()
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: LayerId,
        operation: &layer_core::LayerOperation,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        self.0[0].apply(r, encoder, layer, operation, extent)
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
        self.discard_preview();
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
        extent: [u32; 2],
        layers: &[Layer],
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        let companion = next.companion(layers);
        let mut damage = self.0[0].update_preview(r, encoder, next, extent)?;
        if let Some(companion) = companion {
            damage.extend(self.0[1].update_preview(r, encoder, &companion, extent)?);
        } else {
            damage.extend(self.0[1].cancel_preview(r, encoder)?);
        }
        Ok(damage)
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
    spares: PreviewPages,
    #[cfg(test)]
    pub source_captures: u64,
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
            .map(|p| PAGE_BYTES * (1 + u64::from(p.secondary.is_some())))
            .sum::<u64>()
            + SCALAR_PAGE_BYTES
                * (self.material.len() + 2 * self.watercolor.len() + self.masks.len()) as u64
    }
}
impl ImageTransformState {
    pub fn new(device: &PipelineDevice) -> Self {
        Self::with_passes(
            PixelTransform::staged(device, false),
            PixelTransform::staged(device, true),
            PixelTransform::staged_visibility(device),
        )
    }
    pub fn fork(&self) -> Self {
        Self::with_passes(
            self.color.fork(),
            self.scalar.fork(),
            self.visibility.fork(),
        )
    }
    fn with_passes(
        color: PixelTransform,
        scalar: PixelTransform,
        visibility: PixelTransform,
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
            spares: PreviewPages::default(),
            #[cfg(test)]
            source_captures: 0,
        }
    }
    pub fn pipelines(&self) -> [&Deferred<wgpu::RenderPipeline>; 3] {
        [
            &self.color.pipeline,
            &self.scalar.pipeline,
            &self.visibility.pipeline,
        ]
    }
    pub fn begin_frame(&mut self) {
        self.color.begin_frame();
        self.scalar.begin_frame();
        self.visibility.begin_frame();
    }
    pub fn storage_bytes(&self) -> u64 {
        self.spares.storage_bytes()
            + self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scalar.storage_bytes()
            + self.visibility.storage_bytes()
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
        let layer_core::LayerOperationKind::Transform(transform) = operation.kind else {
            unreachable!()
        };
        if transform.affine == layer_core::Affine::IDENTITY {
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
        let result = self.render_source(r, encoder, layer, transform, &regions);
        self.release_snapshot();
        result
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
        {
            self.source_captures += 1;
        }
        let mut cut = layer_core::Rect {
            min: layer_core::Point {
                x: source_bounds.min_x() as f32,
                y: source_bounds.min_y() as f32,
            },
            max: layer_core::Point {
                x: source_bounds.max_x() as f32,
                y: source_bounds.max_y() as f32,
            },
        };
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
                self.sources[channel] = Some(TileSnapshot::new(
                    pages,
                    if channel == 0 { original.clone() } else { None },
                    bounds,
                ));
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
        transform: layer_core::ImageTransform,
        regions: &[PixelRect],
    ) -> Result<(), GpuRasterError> {
        if self.sources[0].is_none() || regions.iter().all(|b| b.is_empty()) {
            return Ok(());
        }
        let index = r.paint_layers.iter().position(|l| l.id == layer);
        let material = self.sources[1].is_some();
        let watercolor = self.sources[2].is_some();
        let support = self.channel_regions(transform, r.document_extent);
        let coordinates: std::collections::BTreeSet<_> = regions
            .iter()
            .copied()
            .filter(|b| !b.is_empty())
            .flat_map(page_coordinates)
            .collect();
        let mut plans: [Vec<snapshot::RegionJob>; 3] = Default::default();
        for (channel, snapshot) in self.sources.iter().enumerate() {
            if let Some(snapshot) = snapshot {
                plans[channel] = snapshot.jobs(
                    transform,
                    coordinates.iter().copied().filter(|c| {
                        channel == 0
                            || destination(r, layer, channel, false, *c).is_some()
                            || support[channel]
                                .iter()
                                .any(|b| !b.page_local(*c).is_empty())
                    }),
                    regions,
                )?;
            }
        }
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
                page.secondary_needs_clear = page.secondary.is_some();
                r.encode_clear(encoder, &page.primary.view, "initialize transformed paint");
                page.primary_needs_clear = false;
                r.paint_layers[index].pages.push(page);
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
                }
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
        for (channel, jobs) in plans.into_iter().enumerate() {
            if jobs.is_empty() {
                continue;
            }
            // Complete copies before sampling any original in this channel.
            for job in &jobs {
                self.capture_destination(r, encoder, layer, channel, job.coordinate);
            }
            let snapshot = self.sources[channel].as_ref().unwrap();
            let bounds = [
                snapshot.bounds.min_x() as i32,
                snapshot.bounds.min_y() as i32,
                snapshot.bounds.width() as i32,
                snapshot.bounds.height() as i32,
            ];
            let pass = if self.background.is_some() {
                &mut self.visibility
            } else if channel == 0 {
                &mut self.color
            } else {
                &mut self.scalar
            };
            let records: Vec<_> = jobs
                .iter()
                .map(|job| pixel_transform::TiledTransformRecord {
                    target: job.coordinate,
                    sources: &job.sources,
                })
                .collect();
            let offsets = pass
                .prepare_tiled(
                    &r.device,
                    &r.queue,
                    &mut r.uploads,
                    encoder,
                    bounds,
                    self.background.unwrap_or(0.),
                    transform,
                    &records,
                )
                .map_err(GpuRasterError::InvalidTransform)?;
            for (job_index, job) in jobs.into_iter().enumerate() {
                let Some((_, view)) =
                    destination(r, layer, channel, self.background.is_some(), job.coordinate)
                else {
                    continue;
                };
                let view = view.clone();
                let pass = if self.background.is_some() {
                    &mut self.visibility
                } else if channel == 0 {
                    &mut self.color
                } else {
                    &mut self.scalar
                };
                let snapshot = self.sources[channel].as_ref().unwrap();
                let source = snapshot.binding(
                    r,
                    pass,
                    &job,
                    self.has_selection
                        .then_some(self.selection.as_ref())
                        .flatten(),
                    encoder,
                )?;
                let mut target = TransformTarget {
                    view: &view,
                    extent: [PAGE_SIZE; 2],
                    origin: job.coordinate.map(|v| (v * PAGE_SIZE) as i32),
                    region: [
                        job.region.min_x(),
                        job.region.min_y(),
                        job.region.width(),
                        job.region.height(),
                    ],
                };
                if channel == 0 && initialize_original.remove(&job.coordinate) {
                    let region = target.region;
                    target.region = [0, 0, PAGE_SIZE, PAGE_SIZE];
                    pass.encode_prepared(encoder, &source, offsets, job_index, true, &target);
                    target.region = region;
                }
                pass.encode_prepared(encoder, &source, offsets, job_index, false, &target);
            }
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
            snapshot::SnapshotPage {
                texture: capture.texture.clone(),
                view: capture.view.clone(),
            },
        );
    }
    fn release_snapshot(&mut self) {
        self.sources = Default::default();
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
        if operation.kind != layer_core::LayerOperationKind::Transform(preview.transform)
            || operation.coverage.initial != preview.selection
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
        let Some(previous) = self.preview.take() else {
            return Ok(None);
        };
        let regions = self.preview_regions;
        // Removing the target already discarded its pixels. There is nothing
        // to restore, and cancellation must not turn deletion into a GPU error.
        if r.paint_layers.iter().any(|l| l.id == previous.layer)
            || r.layer_masks.definitions.contains_key(&previous.layer)
        {
            self.render_source(r, encoder, previous.layer, Default::default(), &regions)?;
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
        extent: [u32; 2],
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        if self.preview.as_ref() == Some(next) {
            return Ok(Vec::new());
        }
        let same_source = self.preview.as_ref().is_some_and(|p| {
            p.transaction == next.transaction
                && p.layer == next.layer
                && p.selection == next.selection
        });
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
        self.render_source(r, encoder, next.layer, next.transform, &affected)?;
        // Drop only pages created for a previous preview and no longer needed.
        // Original sparse pages remain untouched outside the preview footprint.
        self.retain_pages(r, next.layer, &regions, Some(next.transform));
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
    fn channel_regions(
        &self,
        transform: layer_core::ImageTransform,
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
        transform: Option<layer_core::ImageTransform>,
    ) {
        let support = transform
            .map(|t| self.channel_regions(t, r.document_extent))
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
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
                    .map(|((_, c), p)| {
                        (
                            *c,
                            snapshot::SnapshotPage {
                                texture: p.texture.clone(),
                                view: p.view.clone(),
                            },
                        )
                    })
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
                .map(|p| {
                    (
                        p.coordinate,
                        snapshot::SnapshotPage {
                            texture: p.active().texture.clone(),
                            view: p.active().view.clone(),
                        },
                    )
                })
                .collect(),
            l.material_pages
                .iter()
                .map(|p| {
                    (
                        p.coordinate,
                        snapshot::SnapshotPage {
                            texture: p.wetness.texture.clone(),
                            view: p.wetness.view.clone(),
                        },
                    )
                })
                .collect(),
            l.watercolor_wetness_pages
                .iter()
                .map(|p| {
                    (
                        p.coordinate,
                        snapshot::SnapshotPage {
                            texture: p.active().texture.clone(),
                            view: p.active().view.clone(),
                        },
                    )
                })
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
