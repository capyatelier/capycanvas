//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
use super::*;
use pixel_transform::{PixelTransform, TransformTarget};

pub(super) struct PaintTransforms {
    color: PixelTransform,
    scalar: Option<PixelTransform>,
    captures: [Option<wgpu::Texture>; 3],
}
impl PaintTransforms {
    pub fn new(r: &WgpuRasterizer) -> Self {
        Self {
            color: PixelTransform::new(&r.device),
            scalar: None,
            captures: Default::default(),
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
        let index = r
            .paint_layers
            .iter()
            .position(|l| l.id == layer)
            .ok_or(GpuRasterError::MissingPaintLayer(layer))?;
        let stored = &r.paint_layers[index];
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
        if let Some(s) = &operation.coverage.initial {
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
        let regions = transform
            .affected_regions(cut)
            .map(|b| pixel_rect(b, extent));
        if regions.iter().all(|b| b.is_empty()) {
            return Ok(());
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
        }
        let coordinates: std::collections::BTreeSet<_> = regions
            .into_iter()
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
            let source = pass
                .source(
                    &r.device,
                    self.captures[channel].as_ref().unwrap(),
                    [source_bounds.min_x as i32, source_bounds.min_y as i32],
                    operation
                        .coverage
                        .initial
                        .as_ref()
                        .and(r.selection_clip.buffer.as_ref()),
                )
                .map_err(GpuRasterError::InvalidTransform)?;
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
                        .into_iter()
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
            pass.encode(&r.device, &r.queue, encoder, &source, transform, &targets)
                .map_err(GpuRasterError::InvalidTransform)?;
        }
        // The next stroke establishes fresh stroke-scoped accumulation. Keep
        // persistent wetness and the layer-level watercolor edge style intact.
        for p in &mut r.paint_layers[index].coverage_pages {
            p.owner = None;
        }
        Ok(())
    }
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
