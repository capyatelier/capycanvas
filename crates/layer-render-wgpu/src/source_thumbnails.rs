//! Bounded original-photo overviews. An edit integrates only paint overrides
//! minus their originals; it never scans the unchanged photograph again.
use super::*;
use layer_core::color::source::SourceImage;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
};

const OVERVIEW_BYTES: u64 = 32 * 32 * 16;
const ROW_BYTES: u64 = 32 * PAGE_SIZE as u64 * 16;
const OVERVIEWS: usize = 8;
// At most (tile columns + 32) * (tile rows + 32) small footprints.
// The supported 32768px extent fits below 512 KiB per original.
const CONTRIBUTION_LIMIT: u64 = 512 * 1024;
#[derive(Clone, Copy, Default)]
struct Contribution {
    bounds: [u32; 4],
    offset: u32,
}

struct Overview {
    source: Weak<SourceImage>,
    extent: [u32; 2],
    pixels: wgpu::Buffer,
    contributions: wgpu::Buffer,
    tiles: BTreeMap<[u32; 2], Contribution>,
    valid: Arc<std::sync::atomic::AtomicBool>,
}

pub(super) struct SourceThumbnails {
    layout: wgpu::BindGroupLayout,
    horizontal: wgpu::ComputePipeline,
    vertical: wgpu::ComputePipeline,
    display: wgpu::RenderPipeline,
    display_binding: wgpu::BindGroup,
    parameters: wgpu::Buffer,
    rows: wgpu::Buffer,
    working: wgpu::Buffer,
    cache: VecDeque<Overview>,
    #[cfg(test)]
    pub builds: usize,
}
impl SourceThumbnails {
    pub fn new(r: &WgpuRasterizer) -> Self {
        let d = &r.device;
        let mut entries = Vec::new();
        for binding in 0..5 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: if binding == 1 {
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                } else {
                    wgpu::BindingType::Buffer {
                        ty: if binding == 0 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage { read_only: false }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(match binding {
                            0 => 48,
                            2 => 16,
                            3 => ROW_BYTES,
                            _ => OVERVIEW_BYTES,
                        }),
                    }
                },
                count: None,
            });
        }
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photo overview integration"),
            entries: &entries,
        });
        let pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("photo overview integration"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photo overview integration"),
            source: wgpu::ShaderSource::Wgsl(include_str!("source_thumbnails.wgsl").into()),
        });
        let pipeline = |entry| {
            d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let display_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photo overview display"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(OVERVIEW_BYTES),
                },
                count: None,
            }],
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photo overview display"),
            source: wgpu::ShaderSource::Wgsl(include_str!("source_thumbnail_display.wgsl").into()),
        });
        let display = fullscreen_pipeline(
            d,
            &d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("photo overview display"),
                bind_group_layouts: &[Some(&display_layout)],
                immediate_size: 0,
            }),
            &shader,
            "fragment_main",
            None,
            d.working_format(),
            "photo overview display",
        );
        let working = overview_buffer(d);
        let display_binding = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photo overview display"),
            layout: &display_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: working.as_entire_binding(),
            }],
        });
        Self {
            horizontal: pipeline("horizontal"),
            vertical: pipeline("vertical"),
            layout,
            display,
            display_binding,
            working,
            parameters: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ordered photo overview parameters"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            rows: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bounded photo overview row scratch"),
                size: ROW_BYTES,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            cache: VecDeque::new(),
            #[cfg(test)]
            builds: 0,
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        ROW_BYTES
            + 48
            + OVERVIEW_BYTES
            + self
                .cache
                .iter()
                .map(|c| OVERVIEW_BYTES + c.contributions.size())
                .sum::<u64>()
    }
    pub fn render(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: LayerId,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        let source = r.tiled_sources[&layer].clone();
        let weak = Arc::downgrade(&source);
        self.cache.retain(|c| {
            c.source.strong_count() > 0 && c.valid.load(std::sync::atomic::Ordering::Acquire)
        });
        let cached = self
            .cache
            .iter()
            .position(|c| c.source.ptr_eq(&weak) && c.extent == r.document_extent);
        let overview = if let Some(index) = cached {
            self.cache.remove(index).unwrap()
        } else {
            let write = crate::submission::CacheWrite::new();
            let pixels = overview_buffer(&r.device);
            let (tiles, bytes) = contribution_layout(&source, r.document_extent)?;
            let contributions = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compact original tile thumbnail contributions"),
                size: bytes.max(16),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            for coordinate in source.tiles.keys().copied() {
                if page_rect(coordinate)
                    .intersect(PixelRect::full(r.document_extent))
                    .is_empty()
                {
                    continue;
                }
                let tile = r
                    .original_source_tile(&source, coordinate, encoder)?
                    .unwrap();
                self.integrate(
                    r,
                    encoder,
                    coordinate,
                    &tile.view,
                    false,
                    &pixels,
                    &contributions,
                    tiles[&coordinate],
                )?;
            }
            #[cfg(test)]
            {
                self.builds += 1;
            }
            write.track(encoder);
            Overview {
                source: weak,
                extent: r.document_extent,
                pixels,
                contributions,
                tiles,
                valid: write.validity(),
            }
        };
        encoder.copy_buffer_to_buffer(&overview.pixels, 0, &self.working, 0, OVERVIEW_BYTES);
        // Bindings live only through their ordered dispatch. Keeping paint
        // handles in this cache would retain retired document generations.
        let mut coordinates: std::collections::BTreeSet<_> = r
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .into_iter()
            .flat_map(|l| l.pages.iter().map(|p| p.coordinate))
            .collect();
        coordinates.extend(r.native_color_coordinates(layer));
        for coordinate in coordinates {
            if page_rect(coordinate)
                .intersect(PixelRect::full(r.document_extent))
                .is_empty()
            {
                continue;
            }
            let tile = r.raw_layer_tile(layer, coordinate, encoder)?.unwrap();
            self.integrate(
                r,
                encoder,
                coordinate,
                &tile.view,
                true,
                &self.working,
                &overview.contributions,
                overview.tiles.get(&coordinate).copied().unwrap_or_default(),
            )?;
        }
        self.cache.push_back(overview);
        while self.cache.len() > OVERVIEWS {
            self.cache.pop_front();
        }
        let result = create_page_surface(
            &r.device,
            &r.texture_layout,
            &r.sampler,
            [32, 32],
            r.device.working_format(),
            "photo layer thumbnail",
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("photo thumbnail and checkerboard"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &result.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.display);
            pass.set_bind_group(0, &self.display_binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(result)
    }
    fn integrate(
        &self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        coordinate: [u32; 2],
        pixels: &wgpu::TextureView,
        difference: bool,
        sums: &wgpu::Buffer,
        contributions: &wgpu::Buffer,
        contribution: Contribution,
    ) -> Result<(), GpuRasterError> {
        let record = [
            coordinate[0] * PAGE_SIZE,
            coordinate[1] * PAGE_SIZE,
            r.document_extent[0],
            r.document_extent[1],
            contribution.bounds[0],
            contribution.bounds[1],
            contribution.bounds[2],
            contribution.bounds[3],
            u32::from(difference),
            contribution.offset,
            0,
            0,
        ];
        let mut bytes = [0; 48];
        for (dst, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(record) {
            *dst = value.to_le_bytes();
        }
        r.uploads
            .write(encoder, &r.queue, &self.parameters, &bytes)?;
        let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ordered photo overview tile"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.parameters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(pixels),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: contributions.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.rows.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: sums.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bounded photo overview integration"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &binding, &[]);
        pass.set_pipeline(&self.horizontal);
        pass.dispatch_workgroups(4, PAGE_SIZE / 8, 1);
        pass.set_pipeline(&self.vertical);
        pass.dispatch_workgroups(4, 4, 1);
        Ok(())
    }
}
fn contribution_layout(
    source: &SourceImage,
    extent: [u32; 2],
) -> Result<(BTreeMap<[u32; 2], Contribution>, u64), GpuRasterError> {
    let side = f64::from(extent[0].max(extent[1]));
    let scale = 32. / side;
    let origin = extent.map(|v| (f64::from(v) - side) * 0.5);
    let mut count = 0;
    let mut tiles = BTreeMap::new();
    for coordinate in source.tiles.keys().copied() {
        let region = page_rect(coordinate).intersect(PixelRect::full(extent));
        if region.is_empty() {
            continue;
        }
        let low = [region.min_x(), region.min_y()];
        let high = [region.max_x(), region.max_y()];
        let bounds = std::array::from_fn::<_, 2, _>(|i| {
            let min = ((f64::from(low[i]) - origin[i]) * scale)
                .floor()
                .clamp(0., 32.) as u32;
            let max = ((f64::from(high[i]) - origin[i]) * scale)
                .ceil()
                .clamp(0., 32.) as u32;
            [min, max - min]
        });
        let [x, y] = bounds;
        tiles.insert(
            coordinate,
            Contribution {
                bounds: [x[0], y[0], x[1], y[1]],
                offset: count,
            },
        );
        count += x[1] * y[1];
    }
    let bytes = u64::from(count) * 16;
    if bytes > CONTRIBUTION_LIMIT {
        return Err(GpuRasterError::SizeOverflow);
    }
    Ok((tiles, bytes))
}

fn overview_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bounded linear photo overview"),
        size: OVERVIEW_BYTES,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
#[path = "tests/source_thumbnails.rs"]
mod tests;
