//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
//! Linked paint and mask targets run the same transaction with separate origins.
use super::*;
use pixel_transform::{PixelTransform, TransformSource, TransformTarget};

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
        // The three empty selection bindings are shared by both targets.
        self.0
            .iter()
            .map(ImageTransformState::storage_bytes)
            .sum::<u64>()
            - 3 * 48
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
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
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
        encoder: &mut wgpu::CommandEncoder,
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
        encoder: &mut wgpu::CommandEncoder,
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
    captures: [Option<wgpu::Texture>; 3],
    sources: [Option<TransformSource>; 3],
    selection: Option<wgpu::Buffer>,
    cut: layer_core::Rect,
    original_pages: [Vec<[u32; 2]>; 3],
    source_bounds: [PixelRect; 3],
    preview: Option<layer_render::TransformPreview>,
    preview_regions: [PixelRect; 2],
    #[cfg(test)]
    pub source_captures: u64,
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
            captures: Default::default(),
            sources: Default::default(),
            selection: None,
            cut: layer_core::Rect::EMPTY,
            original_pages: Default::default(),
            source_bounds: [PixelRect::EMPTY; 3],
            preview: None,
            preview_regions: [PixelRect::EMPTY; 2],
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
        self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scalar.storage_bytes()
            + self.visibility.storage_bytes()
            + self
                .captures
                .iter()
                .flatten()
                .map(|t| {
                    u64::from(t.width())
                        * u64::from(t.height())
                        * if t.format() == COLOR_FORMAT { 4 } else { 1 }
                })
                .sum::<u64>()
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
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
        self.render_source(r, encoder, layer, transform, &regions)
    }
    fn capture_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        layer: LayerId,
        selection: Option<&layer_core::Selection>,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        self.sources = Default::default();
        self.cut = layer_core::Rect::EMPTY;
        let (background, pages) = source_pages(r, layer)?;
        self.background = background;
        self.original_pages = std::array::from_fn(|i| pages[i].iter().map(|(c, _)| *c).collect());
        self.source_bounds = std::array::from_fn(|i| {
            pages[i]
                .iter()
                .fold(PixelRect::EMPTY, |b, (c, _)| b.union(page_rect(*c)))
                .intersect(PixelRect::full(extent))
        });
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
                x: source_bounds.min_x as f32,
                y: source_bounds.min_y as f32,
            },
            max: layer_core::Point {
                x: source_bounds.max_x as f32,
                y: source_bounds.max_y as f32,
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
        let material = !pages[1].is_empty();
        let watercolor = !pages[2].is_empty();
        // Capture every channel before overwriting any destination. Reuse the
        // textures across operations; encoder ordering keeps consecutive edits
        // independent even when they share the capture resources.
        for (channel, enabled) in [true, material, watercolor].into_iter().enumerate() {
            if !enabled {
                self.captures[channel] = None;
                continue;
            }
            // Each scalar field may occupy much less area than the pigment.
            // Capturing their union would allocate and transform dry canvas too.
            let source_bounds = if self.source_bounds[channel].is_empty() {
                source_bounds
            } else {
                self.source_bounds[channel]
            };
            let size = [source_bounds.width(), source_bounds.height()];
            if size
                .iter()
                .any(|v| *v > r.device.limits().max_texture_dimension_2d)
            {
                return Err(GpuRasterError::SizeOverflow);
            }
            let format = if channel == 0 && background.is_none() {
                COLOR_FORMAT
            } else {
                wgpu::TextureFormat::R8Unorm
            };
            let capture =
                self.captures[channel].get_or_insert_with(|| capture(&r.device, size, format));
            if [capture.width(), capture.height()] != size || capture.format() != format {
                *capture = self::capture(&r.device, size, format);
            }
            r.encode_clear_value(
                encoder,
                &capture.create_view(&Default::default()),
                "clear transform source",
                background.unwrap_or(0.),
            );
            let mut copy = |coordinate: [u32; 2], source: &wgpu::Texture| {
                let region = page_rect(coordinate).intersect(source_bounds);
                if region.is_empty() {
                    return;
                }
                let local = region.page_local(coordinate);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        origin: wgpu::Origin3d {
                            x: local.min_x,
                            y: local.min_y,
                            z: 0,
                        },
                        ..source.as_image_copy()
                    },
                    wgpu::TexelCopyTextureInfo {
                        origin: wgpu::Origin3d {
                            x: region.min_x - source_bounds.min_x,
                            y: region.min_y - source_bounds.min_y,
                            z: 0,
                        },
                        ..capture.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width: region.width(),
                        height: region.height(),
                        depth_or_array_layers: 1,
                    },
                );
            };
            for (coordinate, texture) in &pages[channel] {
                copy(*coordinate, texture);
            }
            let pass = if background.is_some() {
                &mut self.visibility
            } else if channel == 0 {
                &mut self.color
            } else {
                &mut self.scalar
            };
            self.sources[channel] = Some(
                pass.source(
                    &r.device,
                    capture,
                    [source_bounds.min_x as i32, source_bounds.min_y as i32],
                    selection.and(self.selection.as_ref()),
                )
                .map_err(GpuRasterError::InvalidTransform)?,
            );
            self.sources[channel].as_mut().unwrap().background = background.unwrap_or(0.);
        }
        Ok(())
    }
    fn render_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
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
        for c in coordinates {
            if let Some(background) = self.background {
                if !r.layer_masks.pages.contains_key(&(layer, c)) {
                    let page = layer_masks::MaskPage::new(&r.device);
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
                let mut page = r.create_page(c, "transformed paint page");
                r.encode_clear(encoder, &page.primary.view, "initialize transformed paint");
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
                let wetness = r.create_scalar_page_surface("transformed material wetness");
                r.encode_clear(encoder, &wetness.view, "initialize transformed material");
                r.paint_layers[index]
                    .material_pages
                    .push(CanvasMaterialPage {
                        coordinate: c,
                        wetness,
                        needs_clear: false,
                    });
            }
            if watercolor
                && support[2].iter().any(|b| !b.page_local(c).is_empty())
                && r.paint_layers[index]
                    .watercolor_wetness_pages
                    .iter()
                    .all(|p| p.coordinate != c)
            {
                let primary = r.create_scalar_page_surface("transformed watercolor wetness A");
                let secondary = r.create_scalar_page_surface("transformed watercolor wetness B");
                r.encode_clear(
                    encoder,
                    &primary.view,
                    "initialize transformed watercolor A",
                );
                r.encode_clear(
                    encoder,
                    &secondary.view,
                    "initialize transformed watercolor B",
                );
                r.paint_layers[index]
                    .watercolor_wetness_pages
                    .push(WatercolorWetnessPage {
                        coordinate: c,
                        primary,
                        secondary,
                        active_secondary: false,
                        primary_needs_clear: false,
                        secondary_needs_clear: false,
                    });
            }
        }
        let stored = index.map(|i| &r.paint_layers[i]);
        for (channel, enabled) in [true, material, watercolor].into_iter().enumerate() {
            if !enabled {
                continue;
            }
            let pass = if self.background.is_some() {
                &mut self.visibility
            } else if channel == 0 {
                &mut self.color
            } else {
                &mut self.scalar
            };
            let source = self.sources[channel].as_ref().unwrap();
            let pages: Vec<_> = if self.background.is_some() {
                r.layer_masks
                    .pages
                    .iter()
                    .filter(|((id, _), _)| *id == layer)
                    .map(|((_, c), p)| (*c, &p.view))
                    .collect()
            } else {
                match channel {
                    0 => stored
                        .unwrap()
                        .pages
                        .iter()
                        .map(|p| (p.coordinate, &p.active().view))
                        .collect(),
                    1 => stored
                        .unwrap()
                        .material_pages
                        .iter()
                        .map(|p| (p.coordinate, &p.wetness.view))
                        .collect(),
                    _ => stored
                        .unwrap()
                        .watercolor_wetness_pages
                        .iter()
                        .map(|p| (p.coordinate, &p.active().view))
                        .collect(),
                }
            };
            let targets: Vec<_> = pages
                .into_iter()
                .filter_map(|(c, view)| {
                    let rect = regions
                        .iter()
                        .copied()
                        .map(|b| b.page_local(c))
                        .fold(PixelRect::EMPTY, PixelRect::union);
                    (!rect.is_empty()).then(|| TransformTarget {
                        view,
                        extent: [PAGE_SIZE; 2],
                        origin: c.map(|v| (v * PAGE_SIZE) as i32),
                        region: [rect.min_x, rect.min_y, rect.width(), rect.height()],
                    })
                })
                .collect();
            pass.encode(&r.device, &r.queue, encoder, source, transform, &targets)
                .map_err(GpuRasterError::InvalidTransform)?;
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

    pub fn discard_preview(&mut self) {
        self.preview = None;
        self.preview_regions = [PixelRect::EMPTY; 2];
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
            .find_map(|l| l.target_history(preview.layer))?
            .1
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
        encoder: &mut wgpu::CommandEncoder,
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
        Ok(Some((previous.layer, regions[0].union(regions[1]))))
    }
    pub fn update_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
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
                    x: (b.min_x as f32).max(self.cut.min.x),
                    y: (b.min_y as f32).max(self.cut.min.y),
                },
                max: layer_core::Point {
                    x: (b.max_x as f32).min(self.cut.max.x),
                    y: (b.max_y as f32).min(self.cut.max.y),
                },
            };
            transform
                .affected_regions(cut)
                .map(|b| pixel_rect(b, extent))
        })
    }
    fn retain_pages(
        &self,
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
            r.layer_masks
                .pages
                .retain(|(mask, c), _| *mask != id || keep(*c, &self.original_pages[0], regions));
        } else if let Some(layer) = r.paint_layers.iter_mut().find(|l| l.id == id) {
            layer
                .pages
                .retain(|p| keep(p.coordinate, &self.original_pages[0], regions));
            layer
                .material_pages
                .retain(|p| keep(p.coordinate, &self.original_pages[1], &support[1]));
            layer
                .watercolor_wetness_pages
                .retain(|p| keep(p.coordinate, &self.original_pages[2], &support[2]));
        }
    }
}
type TexturePages = [Vec<([u32; 2], wgpu::Texture)>; 3];
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
                    .map(|((_, c), p)| (*c, p.texture.clone()))
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
                .map(|p| (p.coordinate, p.active().texture.clone()))
                .collect(),
            l.material_pages
                .iter()
                .map(|p| (p.coordinate, p.wetness.texture.clone()))
                .collect(),
            l.watercolor_wetness_pages
                .iter()
                .map(|p| (p.coordinate, p.active().texture.clone()))
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
fn capture(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("immutable transform capture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
