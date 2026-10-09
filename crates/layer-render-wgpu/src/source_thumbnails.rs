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
    offset: [u32; 2],
    extent: [u32; 2],
    pixels: wgpu::Buffer,
    contributions: wgpu::Buffer,
    tiles: BTreeMap<[u32; 2], Contribution>,
    valid: Arc<std::sync::atomic::AtomicBool>,
    remaining: VecDeque<[u32; 2]>,
}

struct Prepared {
    layer: SourceTarget,
    revision: u64,
    source: Weak<SourceImage>,
    offset: [u32; 2],
    extent: [u32; 2],
    pixels: wgpu::Buffer,
    remaining: VecDeque<[u32; 2]>,
    valid: Arc<std::sync::atomic::AtomicBool>,
}

pub(super) struct SourceThumbnails {
    layout: wgpu::BindGroupLayout,
    horizontal: wgpu::ComputePipeline,
    vertical: wgpu::ComputePipeline,
    display: wgpu::RenderPipeline,
    display_layout: wgpu::BindGroupLayout,
    display_parameters: wgpu::Buffer,
    parameters: wgpu::Buffer,
    rows: wgpu::Buffer,
    cache: VecDeque<Overview>,
    prepared: VecDeque<Prepared>,
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
        let layout = crate::bindings::layout(d, "photo overview integration", &entries);
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
        let display_layout = crate::bindings::layout(d, "photo overview display", &[
            crate::bindings::buffer(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                NonZeroU64::new(OVERVIEW_BYTES),
            ),
            crate::bindings::buffer(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Uniform,
                false,
                NonZeroU64::new(48),
            ),
        ]);
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photo overview display"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", crate::view_color::hdr_shader(d.working_space(), r.ui_preview_space), include_str!("source_thumbnail_display.wgsl")).into()),
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
        let display_parameters = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("photo thumbnail orientation and rendition"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            horizontal: pipeline("horizontal"),
            vertical: pipeline("vertical"),
            layout,
            display,
            display_layout,
            display_parameters,
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
            prepared: VecDeque::new(),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        ROW_BYTES
            + 48
            + 16
            + self.prepared.len() as u64 * OVERVIEW_BYTES
            + self
                .cache
                .iter()
                .map(|c| OVERVIEW_BYTES + c.contributions.size())
                .sum::<u64>()
    }
    /// Prepare at most `tile_limit` original or painted tiles. Submit or drop
    /// the encoder before preparing another batch.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: SourceTarget,
        encoder: &mut crate::submission::CommandEncoder,
        mut tile_limit: usize,
    ) -> Result<bool, GpuRasterError> {
        let base = r.tiled_sources[&layer].clone();
        // The layer's finite local backing, including original pixels beyond
        // the canvas. Placement changes only the display pass.
        let extent = r.target_extent(layer);
        let overview = self.overview(r, &base, extent, encoder, &mut tile_limit)?;
        let ready = overview.remaining.is_empty()
            && self.prepare_paint(r, layer, &overview, encoder, tile_limit)?;
        self.retain(overview);
        Ok(ready)
    }
    fn retain(&mut self, overview: Overview) {
        self.cache.push_back(overview);
        while self.cache.len() > OVERVIEWS {
            self.cache.pop_front();
        }
    }
    fn overview(
        &mut self,
        r: &mut WgpuRasterizer,
        base: &layer_core::authored::PaintBase,
        extent: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
        tile_limit: &mut usize,
    ) -> Result<Overview, GpuRasterError> {
        let weak = Arc::downgrade(base.image.storage());
        self.cache.retain(|c| {
            c.source.strong_count() > 0 && c.valid.load(std::sync::atomic::Ordering::Acquire)
        });
        let cached = self
            .cache
            .iter()
            .position(|c| c.source.ptr_eq(&weak) && c.offset == base.offset && c.extent == extent);
        let mut overview = if let Some(index) = cached {
            self.cache.remove(index).unwrap()
        } else {
            let pixels = overview_buffer(&r.device);
            let (tiles, bytes) = contribution_layout(base, extent)?;
            let contributions = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compact original tile thumbnail contributions"),
                size: bytes.max(16),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            Overview {
                source: weak,
                offset: base.offset,
                extent,
                pixels,
                contributions,
                remaining: tiles.keys().copied().collect(),
                tiles,
                valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            }
        };
        if !overview.remaining.is_empty() {
            let write = crate::submission::CacheWrite::new();
            while *tile_limit > 0 {
                let Some(coordinate) = overview.remaining.pop_front() else {
                    break;
                };
                *tile_limit -= 1;
                let tile = r
                    .paint_base_tile(base, coordinate, encoder)?
                    .unwrap();
                self.integrate(
                    r,
                    encoder,
                    overview.extent,
                    coordinate,
                    &tile.view,
                    false,
                    &overview.pixels,
                    &overview.contributions,
                    overview.tiles[&coordinate],
                )?;
            }
            write.track(encoder);
            overview.valid = write.validity();
        }
        Ok(overview)
    }
    fn prepare_paint(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: SourceTarget,
        overview: &Overview,
        encoder: &mut crate::submission::CommandEncoder,
        tile_limit: usize,
    ) -> Result<bool, GpuRasterError> {
        self.prepared.retain(|p| p.revision == r.artwork_revision && p.source.strong_count() > 0
            && p.valid.load(std::sync::atomic::Ordering::Acquire));
        let cached = self.prepared.iter().position(|p| p.layer == layer
            && p.source.ptr_eq(&overview.source) && p.offset == overview.offset && p.extent == overview.extent);
        let write = crate::submission::CacheWrite::new();
        let mut prepared = if let Some(index) = cached {
            self.prepared.remove(index).unwrap()
        } else {
            let pixels = overview_buffer(&r.device);
            encoder.copy_buffer_to_buffer(&overview.pixels, 0, &pixels, 0, OVERVIEW_BYTES);
            let mut coordinates: std::collections::BTreeSet<_> = r.paint_layers.iter()
                .find(|l| l.id == layer).into_iter().flat_map(|l| l.pages.iter().map(|p| p.coordinate)).collect();
            coordinates.extend(r.native_color_coordinates(layer));
            Prepared { layer, revision: r.artwork_revision, source: overview.source.clone(), offset: overview.offset, extent: overview.extent,
                pixels, remaining: coordinates.into_iter().filter(|c| !page_rect(*c)
                    .intersect(PixelRect::full(overview.extent)).is_empty()).collect(), valid: write.validity() }
        };
        let changed = cached.is_none() || (!prepared.remaining.is_empty() && tile_limit > 0);
        for _ in 0..tile_limit {
            let Some(coordinate) = prepared.remaining.pop_front() else { break };
            let tile = r.raw_layer_tile(layer, coordinate, encoder)?.unwrap();
            self.integrate(
                r,
                encoder,
                overview.extent,
                coordinate,
                &tile.view,
                true,
                &prepared.pixels,
                &overview.contributions,
                overview.tiles.get(&coordinate).copied().unwrap_or_default(),
            )?;
        }
        if changed { write.track(encoder); prepared.valid = write.validity(); }
        let ready = prepared.remaining.is_empty();
        self.prepared.push_back(prepared);
        while self.prepared.len() > OVERVIEWS { self.prepared.pop_front(); }
        Ok(ready)
    }
    pub fn render(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: SourceTarget,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        self.prepare(r, layer, encoder, usize::MAX)?;
        let pixels = self.prepared.back().unwrap().pixels.clone();
        self.display(r, &pixels, encoder)
    }
    fn display(
        &self,
        r: &mut WgpuRasterizer,
        pixels: &wgpu::Buffer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        let mapping = [1f32, 0., 0., 1.];
        r.uploads.write(
            encoder,
            &self.display_parameters,
            &mapping
                .into_iter().chain(r.ui_rendition_parameters())
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )?;
        let binding = crate::bindings::group(&r.device, "photo overview display", &self.display_layout, [
            pixels.as_entire_binding(), self.display_parameters.as_entire_binding(),
        ]);
        let result = create_page_surface(
            &r.device,
            &r.texture_layout,
            &r.sampler,
            [32, 32],
            r.device.working_format(),
            "photo layer thumbnail",
        );
        {
            let mut pass = encoder.color_pass(
                "photo thumbnail and checkerboard",
                &result.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(&self.display);
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(result)
    }
    #[expect(clippy::too_many_arguments, reason = "Thumbnail integration keeps tile input and separate reduction outputs explicit")]
    fn integrate(
        &self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
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
            extent[0],
            extent[1],
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
            .write(encoder, &self.parameters, &bytes)?;
        let binding = crate::bindings::group(&r.device, "ordered photo overview tile", &self.layout, [
            self.parameters.as_entire_binding(),
            wgpu::BindingResource::TextureView(pixels),
            contributions.as_entire_binding(),
            self.rows.as_entire_binding(),
            sums.as_entire_binding(),
        ]);
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

/// Fit the complete local rectangle after its linear transform. Translation
/// and uniform scale cancel under content framing, as with paint thumbnails.
/// Compute in f64 so large/small valid placements do not overflow intermediate
/// bounds. Only this disposable display overview is resampled.
fn contribution_layout(
    base: &layer_core::authored::PaintBase,
    extent: [u32; 2],
) -> Result<(BTreeMap<[u32; 2], Contribution>, u64), GpuRasterError> {
    let side = f64::from(extent[0].max(extent[1]));
    let scale = 32. / side;
    let origin = extent.map(|v| (f64::from(v) - side) * 0.5);
    let mut count = 0;
    let mut tiles = BTreeMap::new();
    for coordinate in page_coordinates(source_access::paint_base_bounds(base)) {
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
pub(crate) mod tests;
