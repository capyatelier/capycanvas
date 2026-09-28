//! Derived display pixels. A reusable 256-pixel mip chain reduces completed
//! working tiles into a bounded coarse image; no editing consumer reads it.
use super::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

const MIP_COUNT: u32 = PAGE_SIZE.ilog2() + 1;
pub(super) const MAX_SIDE: u32 = 512;

/// Choose a texel footprint no wider than a surface pixel in any direction.
/// The singular value also handles rotated, reflected and nonuniform views.
pub(super) fn view_level(transform: [f32; 6], max: u32) -> Option<u32> {
    let [a, b, c, d, tx, ty] = transform.map(f64::from);
    if ![a, b, c, d, tx, ty].into_iter().all(f64::is_finite)
        || (a * d - b * c).abs() < 1e-12 { return None; }
    let scale = ((a + d).hypot(b - c) + (a - d).hypot(b + c)) * 0.5;
    let unit = 1. + 4. * f64::from(f32::EPSILON);
    Some((0..=max).rev().find(|level| scale * f64::from(1u32 << level) <= unit).unwrap_or(0))
}

pub(super) fn view_bounds(view: layer_render::ViewState, extent: [u32; 2], padding: u32) -> Result<PixelRect, GpuRasterError> {
    let [a, b, c, d, tx, ty] = view.document_to_surface.map(f64::from);
    let determinant = a * d - b * c;
    if ![a, b, c, d, tx, ty].into_iter().all(f64::is_finite)
        || determinant.abs() < 1e-12 || view.width_px == 0 || view.height_px == 0 {
        return Err(GpuRasterError::InvalidExtent);
    }
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for [x, y] in [[0., 0.], [f64::from(view.width_px), 0.],
        [0., f64::from(view.height_px)], [f64::from(view.width_px), f64::from(view.height_px)]] {
        let p = [(d * (x - tx) - c * (y - ty)) / determinant,
            (-b * (x - tx) + a * (y - ty)) / determinant];
        for i in 0..2 { low[i] = low[i].min(p[i]); high[i] = high[i].max(p[i]); }
    }
    if (0..2).any(|i| high[i] <= 0. || low[i] >= f64::from(extent[i])) { return Ok(PixelRect::EMPTY); }
    let low = [0, 1].map(|i| (low[i] - f64::from(padding)).clamp(0., f64::from(extent[i])).floor() as u32);
    let high = [0, 1].map(|i| (high[i] + f64::from(padding)).clamp(0., f64::from(extent[i])).ceil() as u32);
    Ok(PixelRect::new(low[0], low[1], high[0], high[1]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub extent: [u32; 2],
    pub bounds: PixelRect,
    pub size: [u32; 2],
    pub level: u32,
}
impl Plan {
    pub fn new(extent: [u32; 2]) -> Result<Self, GpuRasterError> {
        if extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let level = (0..MIP_COUNT)
            .find(|level| {
                extent
                    .into_iter()
                    .all(|v| v.div_ceil(1 << level) <= MAX_SIDE)
            })
            .ok_or(GpuRasterError::ExtentUnsupported)?;
        Ok(Self::at(extent, level))
    }
    pub fn at(extent: [u32; 2], level: u32) -> Self {
        Self::window(extent, level, PixelRect::full(extent))
    }
    pub fn window(extent: [u32; 2], level: u32, bounds: PixelRect) -> Self {
        let size = [bounds.width(), bounds.height()].map(|v| v.div_ceil(1 << level));
        Self { extent, bounds, size, level }
    }
    pub fn level_size(self, level: u32) -> [u32; 2] {
        [self.bounds.width(), self.bounds.height()].map(|v| v.div_ceil(1 << level))
    }
    pub fn level_bytes(self, level: u32) -> u64 {
        self.level_size(level).map(u64::from).into_iter().product::<u64>() * 16
    }
    pub fn pixel_bytes(self) -> u64 {
        self.pixel_bytes_through(self.level)
    }
    pub fn pixel_bytes_through(self, last: u32) -> u64 {
        let scratch = (0..=last)
            .map(|level| u64::from(PAGE_SIZE >> level).pow(2) * 16)
            .sum::<u64>();
        scratch + (self.level..=last).map(|level| self.level_bytes(level)).sum::<u64>()
    }
}

pub(super) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    fused_layout: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
    pub fused_reduce: Deferred<wgpu::ComputePipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = crate::bindings::layout(device, "display mip reduction", &[
            crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
            crate::bindings::buffer(
                1,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                true,
                std::num::NonZeroU64::new(16),
            ),
            crate::bindings::storage_texture(
                2,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::StorageTextureAccess::WriteOnly,
            ),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display mip reduction"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(&device, "display mip reduction", include_str!("display_mips.wgsl"));
        let fused_layout = crate::bindings::layout(device, "four display mip levels", &[
            crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
            native_tiles::buffer_entry(1, wgpu::BufferBindingType::Uniform, true, 16),
            mip_output_entry(2), mip_output_entry(3), mip_output_entry(4), mip_output_entry(5),
        ]);
        let fused_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("four display mip levels"), bind_group_layouts: &[Some(&fused_layout)], immediate_size: 0,
        });
        let fused_shader = Deferred::wgsl(&device, "four display mip levels", include_str!("display_mips_fused.wgsl"));
        let fused_reduce = Deferred::compute(device, "four display mip levels", &fused_pipeline_layout, &fused_shader, "reduce_four");
        let reduce = Deferred::compute(device, "display mip reduction", &pipeline_layout, &shader, "reduce");
        Self {
            layout,
            fused_layout,
            reduce,
            fused_reduce,
        }
    }
}

fn mip_output_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::storage_texture(
        binding,
        wgpu::ShaderStages::COMPUTE,
        wgpu::TextureFormat::Rgba32Float,
        wgpu::StorageTextureAccess::WriteOnly,
    )
}

struct Record {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}

/// Update an already admitted complete pyramid in place. Immutable coordinates
/// avoid per-frame uniform allocations and remain valid across submissions.
/// Each dispatch writes one tile region; each level reads the completed finer
/// level. The same reduction kernel also serves the bounded scratch path.
pub(super) struct CompleteUpdates {
    records: wgpu::Buffer,
    bindings: Vec<wgpu::BindGroup>,
    fused_bindings: Vec<wgpu::BindGroup>,
    pipeline: Deferred<wgpu::ComputePipeline>,
    fused_pipeline: Deferred<wgpu::ComputePipeline>,
    columns: u32,
    stride: u32,
    pending: Vec<[u32; 2]>,
    /// The level pending tiles were written at; coarser levels are reduced.
    written: usize,
}
impl CompleteUpdates {
    pub(super) const BATCH: usize = 128;

    pub fn record_bytes(device: &wgpu::Device, plan: Plan) -> u64 {
        u64::from(plan.extent[0].div_ceil(PAGE_SIZE))
            * u64::from(plan.extent[1].div_ceil(PAGE_SIZE))
            * u64::from(plan.level)
            * u64::from(device.limits().min_uniform_buffer_offset_alignment.max(16))
    }

    pub fn new(device: &PipelineDevice, pipelines: &Pipelines, plan: Plan,
        views: &[&wgpu::TextureView]) -> Self {
        assert_eq!(views.len(), plan.level as usize + 1);
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let columns = plan.extent[0].div_ceil(PAGE_SIZE);
        let mut bytes = vec![0; Self::record_bytes(device, plan) as usize];
        for y in 0..plan.extent[1].div_ceil(PAGE_SIZE) {
            for x in 0..columns {
                for level in 1..=plan.level {
                    let offset = ((y * columns + x) * plan.level + level - 1) * stride;
                    for (i, value) in [plan.extent[0], plan.extent[1], 1 << (level - 1), x | (y << 16)]
                        .into_iter().enumerate() {
                        bytes[offset as usize + i * 4..offset as usize + i * 4 + 4]
                            .copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }
        let records = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained display tile coordinates"), contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bindings = views.windows(2).map(|pair| crate::bindings::group(device, "retained display reduction", &pipelines.layout, [
            wgpu::BindingResource::TextureView(pair[0]),
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &records, offset: 0, size: NonZeroU64::new(16), }),
            wgpu::BindingResource::TextureView(pair[1]),
        ])).collect();
        let fused_bindings = (0..plan.level as usize / 4).map(|chunk| {
            let first = chunk * 4;
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(views[first]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &records, offset: 0, size: NonZeroU64::new(16),
                }) },
            ];
            entries.extend((0..4).map(|i| wgpu::BindGroupEntry {
                binding: i as u32 + 2, resource: wgpu::BindingResource::TextureView(views[first + i + 1]),
            }));
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("four retained display mip levels"), layout: &pipelines.fused_layout, entries: &entries,
            })
        }).collect();
        Self { records, bindings, fused_bindings, pipeline: pipelines.reduce.clone(),
            fused_pipeline: pipelines.fused_reduce.clone(), columns, stride,
            pending: Vec::with_capacity(Self::BATCH), written: 0 }
    }

    pub fn storage_bytes(&self) -> u64 { self.records.size() }

    pub fn discard_pending(&mut self) { self.pending.clear(); }

    pub fn tile(&mut self, encoder: &mut crate::submission::CommandEncoder, coordinate: [u32; 2]) {
        self.pending.push(coordinate);
        if self.pending.len() == Self::BATCH { self.flush(encoder); }
    }

    /// Reduce the tiles, by level-0 coordinate, whose pixels were written
    /// directly at `level` into every coarser level.
    pub fn tiles_written_at(&mut self, encoder: &mut crate::submission::CommandEncoder,
        level: usize, coordinates: &[[u32; 2]]) {
        self.flush(encoder);
        for chunk in coordinates.chunks(Self::BATCH) {
            self.pending.extend_from_slice(chunk);
            self.written = level;
            self.flush(encoder);
        }
    }

    pub fn flush(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        let written = std::mem::take(&mut self.written);
        if self.pending.is_empty() { return; }
        let _trace = crate::performance_trace::Span::new(c"capy.mip_encode");
        // Same pixels and per-level dependency order, fewer driver commands.
        self.pending.sort_unstable_by_key(|&[x, y]| (y, x));
        self.pending.dedup();
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce retained display tiles"), timestamp_writes: None,
        });
        let mut level = written;
        while level < self.bindings.len() {
            let fused = level % 4 == 0 && level / 4 < self.fused_bindings.len();
            let binding = if fused { &self.fused_bindings[level / 4] } else { &self.bindings[level] };
            pass.set_pipeline(if fused { &self.fused_pipeline } else { &self.pipeline });
            let side = PAGE_SIZE >> (level + 1);
            let mut first = 0;
            while first < self.pending.len() {
                let [x, y] = self.pending[first];
                let mut end = first + 1;
                while end < self.pending.len()
                    && self.pending[end] == [x + (end - first) as u32, y] {
                    end += 1;
                }
                let offset = ((y * self.columns + x) * self.bindings.len() as u32 + level as u32) * self.stride;
                pass.set_bind_group(0, binding, &[offset]);
                pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), (end - first) as u32);
                first = end;
            }
            level += if fused { 4 } else { 1 };
        }
        self.pending.clear();
    }
}

pub(super) struct Image {
    pub plan: Plan,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    scratch: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    reduced: Vec<(wgpu::Texture, wgpu::TextureView)>,
    // An image has only four combinations of full/partial tile dimensions.
    // Immutable records preserve command order when the scratch tile is reused.
    records: BTreeMap<[u32; 2], Vec<Record>>,
}
impl Image {
    pub fn new(r: &WgpuRasterizer, plan: Plan) -> Self {
        Self::with_mips(r, plan, plan.level)
    }
    /// Retain optional coarser levels from the same tile reduction. Consumers
    /// select an existing level; source pixels and editing precision are intact.
    pub fn with_mips(r: &WgpuRasterizer, plan: Plan, last: u32) -> Self {
        assert!(last >= plan.level && last < MIP_COUNT);
        let (texture, view) = create_target(
            &r.device,
            plan.size,
            wgpu::TextureFormat::Rgba32Float,
            "coarse display image",
        );
        let scratch = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reusable display mip tile"),
            size: wgpu::Extent3d {
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: last + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views = (0..=last)
            .map(|level| {
                scratch.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("single display mip"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let reduced = (plan.level + 1..=last).map(|level| create_target(
            &r.device, plan.level_size(level),
            wgpu::TextureFormat::Rgba32Float, "retained image mip",
        )).collect();
        Self {
            plan,
            texture,
            view,
            scratch,
            views,
            reduced,
            records: BTreeMap::new(),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.plan.pixel_bytes_through(self.last_level())
            + self
                .records
                .values()
                .flatten()
                .map(|r| r.uniform.size())
                .sum::<u64>()
    }
    pub fn last_level(&self) -> u32 { self.views.len() as u32 - 1 }
    pub fn copy_mip(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        level: u32,
        coordinate: [u32; 2],
        destination: &wgpu::Texture,
        origin: [u32; 2],
    ) {
        assert!(level <= self.last_level());
        let valid: [u32; 2] = std::array::from_fn(|i| {
            (self.plan.extent[i] - coordinate[i] * PAGE_SIZE)
                .min(PAGE_SIZE)
                .div_ceil(1 << level)
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: level,
                ..self.scratch.as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                ..destination.as_image_copy()
            },
            wgpu::Extent3d {
                width: valid[0],
                height: valid[1],
                depth_or_array_layers: 1,
            },
        );
    }
    pub fn tile_target(&self) -> (&wgpu::Texture, &wgpu::TextureView) {
        (&self.scratch, &self.views[0])
    }

    /// Consume one completed full-resolution composition tile. `source_origin`
    /// supports both a temporary tile and the current full composite during its
    /// replacement. Every output pixel averages all valid document pixels in its
    /// power-of-two footprint, including partial right/bottom edge footprints.
    pub fn write_tile(
        &mut self,
        device: &PipelineDevice,
        pipelines: &Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        source: &wgpu::Texture,
        source_origin: [u32; 2],
        coordinate: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        if coordinate
            .into_iter()
            .zip(self.plan.extent)
            .any(|(c, n)| c >= n.div_ceil(PAGE_SIZE))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let origin = coordinate.map(|v| v * PAGE_SIZE);
        let valid = std::array::from_fn(|i| (self.plan.extent[i] - origin[i]).min(PAGE_SIZE));
        if source.format() != wgpu::TextureFormat::Rgba32Float
            || (source == &self.scratch && source_origin != [0; 2])
            || source_origin
                .into_iter()
                .zip(valid)
                .zip([source.width(), source.height()])
                .any(|((start, n), size)| start.checked_add(n).is_none_or(|end| end > size))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if source != &self.scratch {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: source_origin[0],
                        y: source_origin[1],
                        z: 0,
                    },
                    ..source.as_image_copy()
                },
                self.scratch.as_image_copy(),
                wgpu::Extent3d {
                    width: valid[0],
                    height: valid[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        let last = self.last_level();
        let records = self.records.entry(valid).or_insert_with(|| {
            (1..=last)
                .map(|level| {
                    let data = [valid[0], valid[1], 1 << (level - 1), 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>();
                    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("display mip footprint"),
                        contents: &data,
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let binding = crate::bindings::group(device, "display mip source", &pipelines.layout, [
                        wgpu::BindingResource::TextureView( &self.views[level as usize - 1], ),
                        uniform.as_entire_binding(),
                        wgpu::BindingResource::TextureView(&self.views[level as usize]),
                    ]);
                    Record { uniform, binding }
                })
                .collect()
        });
        if !records.is_empty() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("reduce display tile"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.reduce);
            for (index, record) in records.iter().enumerate() {
                pass.set_bind_group(0, &record.binding, &[0]);
                let side = PAGE_SIZE >> (index + 1);
                pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), 1);
            }
        }
        let scale = 1 << self.plan.level;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: self.plan.level,
                ..self.scratch.as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: origin[0] / scale,
                    y: origin[1] / scale,
                    z: 0,
                },
                ..self.texture.as_image_copy()
            },
            wgpu::Extent3d {
                width: valid[0].div_ceil(scale),
                height: valid[1].div_ceil(scale),
                depth_or_array_layers: 1,
            },
        );
        for (index, (texture, _)) in self.reduced.iter().enumerate() {
            let level = self.plan.level + index as u32 + 1;
            self.copy_mip(encoder, level, coordinate, texture, origin.map(|v| v >> level));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
