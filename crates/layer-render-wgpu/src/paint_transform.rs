//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
use super::*;
use pixel_transform::{PixelTransform, TransformSource, TransformTarget};

pub(super) struct PaintTransforms {
    color: PixelTransform,
    scalar: Option<PixelTransform>,
    captures: [Option<wgpu::Texture>; 3],
    sources: [Option<TransformSource>; 3],
    selection: Option<wgpu::Buffer>,
    cut: layer_core::Rect,
    original_pages: [Vec<[u32; 2]>; 3],
    preview: Option<layer_render::TransformPreview>,
    preview_regions: [PixelRect; 2],
    #[cfg(test)]
    pub source_captures: u64,
}
impl PaintTransforms {
    pub fn new(r: &WgpuRasterizer) -> Self {
        Self {
            color: PixelTransform::new(&r.device),
            scalar: None,
            captures: Default::default(),
            sources: Default::default(),
            selection: None,
            cut: layer_core::Rect::EMPTY,
            original_pages: Default::default(),
            preview: None,
            preview_regions: [PixelRect::EMPTY; 2],
            #[cfg(test)]
            source_captures: 0,
        }
    }
    pub fn begin_frame(&mut self) {
        self.color.begin_frame();
        if let Some(s) = &mut self.scalar {
            s.begin_frame();
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self
                .scalar
                .as_ref()
                .map_or(0, PixelTransform::storage_bytes)
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
        let index = r
            .paint_layers
            .iter()
            .position(|l| l.id == layer)
            .ok_or(GpuRasterError::MissingPaintLayer(layer))?;
        let stored = &r.paint_layers[index];
        self.original_pages = [
            stored.pages.iter().map(|p| p.coordinate).collect(),
            stored.material_pages.iter().map(|p| p.coordinate).collect(),
            stored
                .watercolor_wetness_pages
                .iter()
                .map(|p| p.coordinate)
                .collect(),
        ];
        let source_bounds = stored
            .pages
            .iter()
            .map(|p| p.coordinate)
            .chain(stored.material_pages.iter().map(|p| p.coordinate))
            .chain(stored.watercolor_wetness_pages.iter().map(|p| p.coordinate))
            .fold(PixelRect::EMPTY, |b, c| b.union(page_rect(c)))
            .intersect(PixelRect::full(extent));
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
        let material = !stored.material_pages.is_empty();
        let watercolor = !stored.watercolor_wetness_pages.is_empty();
        let size = [source_bounds.width(), source_bounds.height()];
        if size
            .iter()
            .any(|v| *v > r.device.limits().max_texture_dimension_2d)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        // Capture every channel before overwriting any destination. Reuse the
        // textures across operations; encoder ordering keeps consecutive edits
        // independent even when they share the capture resources.
        for (channel, enabled) in [true, material, watercolor].into_iter().enumerate() {
            if !enabled {
                continue;
            }
            let format = if channel == 0 {
                COLOR_FORMAT
            } else {
                wgpu::TextureFormat::R8Unorm
            };
            let capture =
                self.captures[channel].get_or_insert_with(|| capture(&r.device, size, format));
            if [capture.width(), capture.height()] != size {
                *capture = self::capture(&r.device, size, format);
            }
            r.encode_clear(
                encoder,
                &capture.create_view(&Default::default()),
                "clear transform source",
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
            match channel {
                0 => {
                    for p in &stored.pages {
                        copy(p.coordinate, &p.active().texture);
                    }
                }
                1 => {
                    for p in &stored.material_pages {
                        copy(p.coordinate, &p.wetness.texture);
                    }
                }
                _ => {
                    for p in &stored.watercolor_wetness_pages {
                        copy(p.coordinate, &p.active().texture);
                    }
                }
            }
            let pass = if channel == 0 {
                &mut self.color
            } else {
                self.scalar
                    .get_or_insert_with(|| PixelTransform::scalar(&r.device))
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
        let index = r
            .paint_layers
            .iter()
            .position(|l| l.id == layer)
            .ok_or(GpuRasterError::MissingPaintLayer(layer))?;
        let material = self.sources[1].is_some();
        let watercolor = self.sources[2].is_some();
        let coordinates: std::collections::BTreeSet<_> = regions
            .iter()
            .copied()
            .filter(|b| !b.is_empty())
            .flat_map(page_coordinates)
            .collect();
        for c in coordinates {
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
        let stored = &r.paint_layers[index];
        for (channel, enabled) in [true, material, watercolor].into_iter().enumerate() {
            if !enabled {
                continue;
            }
            let pass = if channel == 0 {
                &mut self.color
            } else {
                self.scalar
                    .get_or_insert_with(|| PixelTransform::scalar(&r.device))
            };
            let source = self.sources[channel].as_ref().unwrap();
            let pages: Vec<_> = match channel {
                0 => stored
                    .pages
                    .iter()
                    .map(|p| (p.coordinate, &p.active().view))
                    .collect(),
                1 => stored
                    .material_pages
                    .iter()
                    .map(|p| (p.coordinate, &p.wetness.view))
                    .collect(),
                _ => stored
                    .watercolor_wetness_pages
                    .iter()
                    .map(|p| (p.coordinate, &p.active().view))
                    .collect(),
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
        for p in &mut r.paint_layers[index].coverage_pages {
            p.owner = None;
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
    pub fn consume_commit(&mut self, packet: FramePacket<'_>) -> Option<(LayerId, u32)> {
        let preview = self.preview.as_ref()?;
        let [batch] = packet.dab_batches else {
            return None;
        };
        let DabBatchKind::LayerOperation(index) = batch.kind else {
            return None;
        };
        if batch.layer_id != preview.layer || !packet.dabs.is_empty() {
            return None;
        }
        let operation = packet
            .layers
            .iter()
            .find(|l| l.id == preview.layer)?
            .operations
            .get(index as usize)?;
        if operation.kind != layer_core::LayerOperationKind::Transform(preview.transform)
            || operation.coverage.initial != preview.selection
        {
            return None;
        }
        let result = (preview.layer, index);
        self.discard_preview();
        Some(result)
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
        if r.paint_layers.iter().any(|l| l.id == previous.layer) {
            self.render_source(r, encoder, previous.layer, Default::default(), &regions)?;
        }
        if let Some(layer) = r.paint_layers.iter_mut().find(|l| l.id == previous.layer) {
            layer
                .pages
                .retain(|p| self.original_pages[0].contains(&p.coordinate));
            layer
                .material_pages
                .retain(|p| self.original_pages[1].contains(&p.coordinate));
            layer
                .watercolor_wetness_pages
                .retain(|p| self.original_pages[2].contains(&p.coordinate));
        }
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
        let keep = |c: [u32; 2], original: &Vec<_>| {
            original.contains(&c) || regions.iter().any(|b| !b.page_local(c).is_empty())
        };
        if let Some(layer) = r.paint_layers.iter_mut().find(|l| l.id == next.layer) {
            layer
                .pages
                .retain(|p| keep(p.coordinate, &self.original_pages[0]));
            layer
                .material_pages
                .retain(|p| keep(p.coordinate, &self.original_pages[1]));
            layer
                .watercolor_wetness_pages
                .retain(|p| keep(p.coordinate, &self.original_pages[2]));
        }
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
