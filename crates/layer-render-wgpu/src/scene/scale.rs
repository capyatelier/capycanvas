//! Demand-driven presentation composition with bounded region and scale grids.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;

pub(crate) const CACHE_BYTES: u64 = 608 * 1024 * 1024;
fn exact_strip(extent: [u32; 2]) -> [u32; 2] {
    [PAGE_SIZE * extent[0].div_ceil(PAGE_SIZE).next_power_of_two().min(SOURCE_SLOTS as u32), PAGE_SIZE]
}
fn exact_strip_bytes(extent: [u32; 2]) -> u64 {exact_strip(extent).map(u64::from).into_iter().product::<u64>() * 16}

mod sources;
mod graph;
mod effects;
mod refinement;
mod hierarchy;
mod navigator;
pub(crate) use navigator::Navigator;
pub(crate) use sources::Sources;

/// Device recipes survive level changes; display cache retirement drops pixels only.
#[derive(Clone)]
pub(crate) struct Pipelines {
    records: wgpu::BindGroupLayout,
    inputs: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
    pub reduce_pair: Deferred<wgpu::ComputePipeline>,
    pub compose: Deferred<wgpu::ComputePipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let records_layout = crate::bindings::layout(
            device,
            "display composition records",
            &[crate::bindings::buffer(
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                true,
                NonZeroU64::new(80),
            )],
        );
        let inputs = crate::bindings::layout(
            device,
            "display composition inputs",
            &[
                crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, true),
                crate::bindings::texture(1, wgpu::ShaderStages::COMPUTE, true),
                crate::bindings::storage_texture(
                    2,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::TextureFormat::Rgba32Float,
                    wgpu::StorageTextureAccess::WriteOnly,
                ),
                crate::bindings::texture(3, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::texture(4, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::sampler(5, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
            ],
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display resolution composition"),
            bind_group_layouts: &[Some(&records_layout), Some(&inputs)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(
            device,
            "display resolution composition",
            compose_wgsl(&[&working_color::shader(device), include_str!("../blend_modes.wgsl"), include_str!("scale.wgsl"), include_str!("scale/compose.wgsl")]),
        );
        Self {
            records: records_layout,
            inputs,
            reduce: Deferred::compute(
                device,
                "reduce paint to display",
                &layout,
                &shader,
                "reduce",
            ),
            reduce_pair: Deferred::compute(device, "reduce adjacent display level", &layout, &shader, "reduce_pair"),
            compose: Deferred::compute(
                device,
                "compose display layers",
                &layout,
                &shader,
                "compose",
            ),
        }
    }
}

pub(super) struct Encoding<'a> {
    pub encoder: &'a mut crate::submission::CommandEncoder,
    pub commands: &'a mut Commands,
}
#[derive(Clone, Copy)]
struct SourceRequest {
    plan: display_mips::Plan,
    required: PixelRect,
    covered: PixelRect,
}

pub(crate) struct Commands {
    records: wgpu::Buffer,
    record_binding: wgpu::BindGroup,
    stride: u32,
    cursor: u32,
    jobs: Vec<DisplayJob>,
}
struct Composition {
    values: [u32; 20],
    binding: wgpu::BindGroup,
    leases: Vec<Arc<()>>,
}
enum DisplayJob {
    Compose(Composition),
    Resample { values: [u8; resample::UNIFORM_BYTES as usize], views: [wgpu::TextureView; 2], size: [u32; 2] },
    Mesh {values:[u8;resample::UNIFORM_BYTES as usize],views:[wgpu::TextureView;2],texels:[u32;4],draw:paint_transform::mesh::MeshDraw},
}
impl Commands {
    pub fn new(r: &WgpuRasterizer) -> Self {
        Self::with_capacity(r, 256)
    }
    fn with_capacity(r: &WgpuRasterizer, bytes: u64) -> Self {
        let stride = r
            .device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(256);
        let records = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display composition regions"),
            size: bytes.max(u64::from(stride)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let record_binding = Self::record_binding(r, &records);
        Self { records, record_binding, stride, cursor: 0, jobs: Vec::new() }
    }
    pub(super) fn record_binding(r: &WgpuRasterizer, records: &wgpu::Buffer) -> wgpu::BindGroup {
        crate::bindings::group(&r.device, "display composition regions", &r.scene_pipelines.scale.records,
            [wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: records, offset: 0, size: NonZeroU64::new(80) })])
    }
    pub fn begin(&mut self) { self.cursor = 0; self.jobs.clear(); }
    pub fn storage_bytes(&self) -> u64 { self.records.size() }
    pub(super) fn binding(
        r: &WgpuRasterizer,
        source: &wgpu::TextureView,
        base: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        Self::inputs(r, [source, base, &r.empty_view, &r.empty_view], output)
    }
    fn inputs(r: &WgpuRasterizer, sources: [&wgpu::TextureView; 4], output: &wgpu::TextureView) -> wgpu::BindGroup {
        crate::bindings::group(
            &r.device,
            "display composition images",
            &r.scene_pipelines.scale.inputs,
            [
                wgpu::BindingResource::TextureView(sources[0]),
                wgpu::BindingResource::TextureView(sources[1]),
                wgpu::BindingResource::TextureView(output),
                wgpu::BindingResource::TextureView(sources[2]),
                wgpu::BindingResource::TextureView(sources[3]),
                wgpu::BindingResource::Sampler(&r.scene_pipelines.resample.sampler),
            ],
        )
    }
    pub(super) fn record(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        values: [u32; 20],
    ) -> Result<u32, GpuRasterError> {
        let mut bytes = [0; 80];
        for (dst, value) in bytes.chunks_exact_mut(4).zip(values) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        self.write(r, encoder, &bytes)
    }
    pub(super) fn write(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, bytes: &[u8]) -> Result<u32, GpuRasterError> {
        self.flush(r, encoder)?;
        let offset = u64::from(self.cursor) * u64::from(self.stride);
        let needed = offset + bytes.len() as u64;
        if needed > self.records.size() {
            let size = needed.next_power_of_two().min(r.device.limits().max_buffer_size).min(u64::from(u32::MAX));
            if size < needed { return Err(GpuRasterError::ExtentUnsupported); }
            let mut grown = Self::with_capacity(r, size);
            encoder.copy_buffer_to_buffer(&self.records, 0, &grown.records, 0, offset);
            grown.cursor = self.cursor;
            *self = grown;
        }
        let offset = offset as u32;
        self.cursor += (bytes.len() as u32).div_ceil(self.stride);
        r.uploads.write_at(encoder, &self.records, u64::from(offset), bytes)?;
        Ok(offset)
    }
    pub(super) fn reduction_record(r: &WgpuRasterizer, mut values: [u32; 20]) -> [u32; 20] {
        if r.document_color().depth.is_float() && values[7] & 128 != 0 { values[7] |= 16; }
        values
    }
    fn reduce(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        values: [u32; 20], binding: &wgpu::BindGroup, label: &str,
    ) -> Result<(), GpuRasterError> {
        let values = Self::reduction_record(r, values);
        let offset = self.record(r, encoder, values)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label), timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.reduce);
        pass.set_bind_group(0, &self.record_binding, &[offset]);
        pass.set_bind_group(1, binding, &[]);
        pass.dispatch_workgroups(values[2].div_ceil(8), values[3].div_ceil(8), 1);
        Ok(())
    }
    fn push(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, job: DisplayJob) -> Result<(), GpuRasterError> {
        self.jobs.push(job);
        if self.jobs.len() >= 32 { self.flush(r, encoder)?; }
        Ok(())
    }
    fn flush(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if self.jobs.is_empty() { return Ok(()); }
        let jobs = std::mem::take(&mut self.jobs);
        let mut records = vec![0; jobs.len() * self.stride as usize];
        for (record, job) in records.chunks_exact_mut(self.stride as usize).zip(&jobs) {
            match job {
                DisplayJob::Compose(job) => for (dst, value) in record.chunks_exact_mut(4).zip(job.values) { dst.copy_from_slice(&value.to_le_bytes()); },
                DisplayJob::Resample { values, .. } | DisplayJob::Mesh {values,..} => record[..values.len()].copy_from_slice(values),
            }
        }
        let offset = self.write(r, encoder, &records)?;
        let resample = &r.scene_pipelines.resample;
        let bindings: Vec<_> = jobs.iter().enumerate().map(|(i, job)| match job {
            DisplayJob::Resample { views, .. } => Some(resample.binding(&r.device, &self.records,
                u64::from(offset + i as u32 * self.stride), [&views[0], &views[1], &views[0]])),
            DisplayJob::Mesh {views,..} => Some(resample.mesh_binding_at(&r.device,&self.records,u64::from(offset+i as u32*self.stride),&[views[0].clone(),views[0].clone()])),
            DisplayJob::Compose(_) => None,
        }).collect();
        let mut i=0;
        while i<jobs.len() {
            if let DisplayJob::Mesh {views,..}=&jobs[i] {
                let target=&views[1];
                let mut pass=encoder.color_pass("display mesh regions",target,wgpu::LoadOp::Load);
                while let Some(DisplayJob::Mesh {views,texels,draw,..})=jobs.get(i) {
                    if &views[1]!=target {break;}
                    resample.draw_mesh(&mut pass,bindings[i].as_ref().unwrap(),*texels,Some(draw),false);
                    i+=1;
                }
            } else {
                let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("display graph composition"),timestamp_writes:None});
                while i<jobs.len() {
                    let size=match &jobs[i] {
                        DisplayJob::Compose(job)=> {
                            pass.set_pipeline(&r.scene_pipelines.scale.compose);
                            pass.set_bind_group(0,&self.record_binding,&[offset+i as u32*self.stride]);
                            pass.set_bind_group(1,&job.binding,&[]);[job.values[2],job.values[3]]
                        }
                        DisplayJob::Resample {size,..}=> {
                            pass.set_pipeline(&resample.area);pass.set_bind_group(0,bindings[i].as_ref().unwrap(),&[]);*size
                        }
                        DisplayJob::Mesh {..}=>break,
                    };
                    pass.dispatch_workgroups(size[0].div_ceil(8),size[1].div_ceil(8),1);i+=1;
                }
            }
        }
        for mut job in jobs { if let DisplayJob::Compose(job) = &mut job { job.leases.clear(); } }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Evaluation { Display, Native }

#[derive(Clone, Copy)]
pub(crate) struct Request {
    pub plan: display_mips::Plan,
    pub evaluation: Evaluation,
}

enum Destination<'a> { View, Overview(&'a Cache, PixelRect), Navigator }

pub(crate) struct Cache {
    pub(super) submission_valid: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub evaluation: Evaluation,
    pub plan: display_mips::Plan,
    output: Vec<Image>,
    used: Vec<bool>,
    pixels: hierarchy::Pixels,
    placed: Option<Presentation>,
    pub geometry: wgpu::Buffer,
    layer_count: usize,
    // One adjacent level may survive unchanged navigation. Artwork changes
    // release it, including its references to exact native tile captures.
    spare: Option<Box<Cache>>,
    ready: bool,
    reuse_output: bool,
    overview: Option<Box<Cache>>,
    shifted: Option<Box<Cache>>,
    valid: BTreeSet<[u32; 2]>,
    refined: BTreeSet<[u32; 2]>,
    exact_tile: Option<Image>,
    unchanged: bool,
    graph: graph::Graph,
    transform: Option<layer_render::TransformPreview>,
    source_overlap: bool,
    streamed_sources: bool,
    hierarchy: Option<hierarchy::Hierarchy>,
    residency_checked: bool,
}

pub(crate) fn request(r: &WgpuRasterizer, packet: FramePacket<'_>) -> Result<Request, GpuRasterError> {
    display_mips::Plan::new(packet.document_extent)?;
    display_mips::view_bounds(packet.view, packet.document_extent, 0)?;
    #[cfg(test)]
    if r.test.reference {
        return Ok(Request { plan: display_mips::Plan::at(packet.document_extent, 0), evaluation: Evaluation::Native });
    }
    let level = display_mips::view_level(packet.view.document_to_surface, 4).ok_or(GpuRasterError::InvalidExtent)?;
    let plan = view_plan(packet, level, Evaluation::Display).ok_or(GpuRasterError::InvalidExtent)?;
    let input = input_plan(plan, packet.layers);
    let records = records_for(r, plan, packet.layers);
    let fits = transform_plans(r, plan, packet.layers).all(|(source, _)| source.size.iter()
        .all(|size| *size <= r.device.limits().max_texture_dimension_2d)) && targets(r, packet).all(|(layer, id)| {
        source_plan(r.scene.as_ref(), input, &layer_core::target_geometry(packet.layers, id), target_extent(layer,id,plan.extent), r.moving_layer == Some(id))
            .is_ok_and(|source| (plan.level == 0 && source.level == 0) || source.size.iter().all(|size| *size <= r.device.limits().max_texture_dimension_2d))
    });
    let native = !fits || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
        || allocation_for(r, plan, packet, None, bounded(packet.layers), None).into_iter().sum::<u64>() > CACHE_BYTES
        || packet.layers.iter().any(|l| images::visible(packet.layers, l)
            && l.effect.as_ref().is_some_and(|e| e.program.resolution == layer_core::EffectResolution::Native
                || (e.program.image_boundary() && level == 0)));
    #[cfg(test)]
    let native = native || r.test.exact_display;
    let evaluation = if native { Evaluation::Native } else { Evaluation::Display };
    let plan = view_plan(packet, level, evaluation).ok_or(GpuRasterError::InvalidExtent)?;
    let output_bytes = |p: display_mips::Plan| p.level_bytes(p.level) + p.level_bytes(p.level + 1) + 32;
    let material_pages = if targets(r, packet).any(|(_, id)| mapped_material(r, packet, id)) { Scene::MATERIAL_CACHE_PAGES as u64 } else { 0 };
    let native_bytes = Scene::geometry_bytes(packet.layers,r.scene.as_ref()) + output_bytes(plan) + exact_strip_bytes(packet.document_extent) + u64::from(PAGE_SIZE).pow(2) * 16 * material_pages
        + if plan.bounds == PixelRect::full(plan.extent) { 0 } else { output_bytes(overview_plan(plan)) };
    if plan.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) || native_bytes > CACHE_BYTES {
        return Err(GpuRasterError::ExtentUnsupported);
    }
    Ok(Request { plan, evaluation })
}

pub(crate) fn source_level(level: u32, placement: &layer_core::ImageTransform, extent: [u32; 2]) -> u32 {
    let bounds = PixelRect::full(extent).to_rect();
    ((level as f32 - placement.magnification(bounds).log2()).ceil().clamp(0., 4.) as u32)
        .saturating_sub(u32::from(!placement.is_identity()))
}

pub(super) fn placement_level(layers: &[Layer], id: LayerId) -> u32 {
    let extent = layers.iter().find(|l| l.id == id || l.mask.as_ref().is_some_and(|m| m.id == id)).map_or([1;2], |l| target_extent(l,id,[1;2]));
    let rate = layer_core::target_geometry(layers, id).magnification(PixelRect::full(extent).to_rect());
    (-rate.log2()).floor().clamp(0.,8.) as u32
}

fn page_regions(pages: impl IntoIterator<Item = [u32; 2]>, bounds: PixelRect) -> Vec<PixelRect> {
    let mut columns: Vec<PixelRect> = Vec::new();
    for page in pages.into_iter().collect::<BTreeSet<_>>() {
        let region = page_rect(page).intersect(bounds);
        if region.is_empty() { continue; }
        if let Some(last) = columns.last_mut().filter(|r| r.min_x() == region.min_x() && r.max_y() == region.min_y()) {
            *last = last.union(region);
        } else { columns.push(region); }
    }
    let mut regions: Vec<PixelRect> = Vec::new();
    for column in columns {
        if let Some(last) = regions.last_mut().filter(|r| r.min_y() == column.min_y() && r.max_y() == column.max_y() && r.max_x() == column.min_x()) {
            *last = last.union(column);
        } else { regions.push(column); }
    }
    regions
}

fn local_regions(scene: Option<&Scene>, required: PixelRect, covered: PixelRect, placement: &layer_core::ImageTransform, extent: [u32; 2], level: u32) -> Result<(PixelRect, PixelRect), GpuRasterError> {
    if required.is_empty() { return Ok((PixelRect::EMPTY, covered)); }
    if placement.is_identity() { return Ok((required.intersect(PixelRect::full(extent)), covered)); }
    Ok((paint_transform::snapshot::source_region(placement, required.to_rect(), extent, scene.and_then(|scene|scene.mesh_geometry(placement)))?.expand(1 << level, extent), PixelRect::EMPTY))
}

fn source_plan(scene: Option<&Scene>, output: display_mips::Plan, placement: &layer_core::ImageTransform, extent: [u32; 2], moving: bool) -> Result<display_mips::Plan, GpuRasterError> {
    let level = source_level(output.level, placement, extent).saturating_sub(u32::from(moving && placement.is_identity()));
    let (bounds, _) = local_regions(scene, output.bounds, PixelRect::EMPTY, placement, extent, level)?;
    let coarse = source_coarse_level(display_mips::Plan::at(extent, level));
    let bounds = if bounds.is_empty() { PixelRect::EMPTY } else {
        paint_transform::aligned(bounds.expand(2 << coarse, extent), PAGE_SIZE.max(1 << coarse), extent)
    };
    Ok(display_mips::Plan::window(extent, level, bounds))
}

fn view_plan(packet: FramePacket<'_>, level: u32, evaluation: Evaluation) -> Option<display_mips::Plan> {
    let extent = packet.document_extent;
    if evaluation == Evaluation::Display && level > 0 && effect_radius(packet.layers, level).is_none() {
        return Some(display_mips::Plan::at(extent, level));
    }
    let visible = display_mips::view_bounds(packet.view, extent, 2 << level).ok()?;
    if visible.is_empty() { return display_mips::Plan::new(extent).ok(); }
    let bounds = paint_transform::aligned(visible.expand(PAGE_SIZE, extent), PAGE_SIZE.max(1 << level), extent);
    Some(display_mips::Plan::window(extent, level, bounds))
}

pub(super) fn bounded(layers: &[Layer]) -> bool {
    !layers.iter().any(|l| images::visible(layers, l)
        && l.effect.as_ref().is_some_and(|e| e.program.image_boundary()))
}

fn effect_radius(layers: &[Layer], level: u32) -> Option<u32> {
    layers.iter().filter(|l| images::visible(layers, l)).filter_map(|l| l.effect.as_ref())
        .try_fold(0u32, |radius, effect| radius.checked_add(crate::effects::damage_radius(effect, level)?))
}

fn input_plan(plan: display_mips::Plan, layers: &[Layer]) -> display_mips::Plan {
    let bounds = crate::effects::dependency(plan.bounds, effect_radius(layers, plan.level), display_mips::Plan::at(plan.extent, plan.level));
    display_mips::Plan::window(plan.extent, plan.level, paint_transform::aligned(bounds, PAGE_SIZE.max(1 << plan.level), plan.extent))
}

fn overview_plan(plan: display_mips::Plan) -> display_mips::Plan {
    let level = display_mips::Plan::new(plan.extent).unwrap().level.max(plan.level + 1);
    display_mips::Plan::at(plan.extent, level)
}

fn records_for(r: &WgpuRasterizer, plan: display_mips::Plan, layers: &[Layer]) -> u64 {
    record_bytes(r, source_extent(plan.extent, layers), layers.len())
}
pub(super) fn source_extent(extent: [u32; 2], layers: &[Layer]) -> [u32; 2] {
    layers.iter().fold(extent, |size, layer| {
        let local = layer.local_extent(extent);
        [size[0].max(local[0]), size[1].max(local[1])]
    })
}
fn target_extent(layer: &Layer, id: LayerId, canvas: [u32;2]) -> [u32;2] {
    let extent = layer.local_extent(canvas);
    layer.mask.as_ref().filter(|mask| mask.id == id).map_or(extent, |mask| mask.local_extent(extent))
}
fn targets<'r, 'p: 'r>(r: &'r WgpuRasterizer, packet: FramePacket<'p>) -> impl Iterator<Item = (&'p Layer, LayerId)> + 'r {
    packet.layers.iter().flat_map(move |layer| {
        let visible = images::visible(packet.layers, layer);
        let content = stack::has_content(r, layer) || !layer.raster.is_empty() || !layer.pending_operations.is_empty()
            || packet.dab_batches.iter().any(|b| b.layer_id == layer.id)
            || packet.restore_rasters.iter().any(|(id, data)| *id == layer.id && !data.is_empty());
        let paint = (visible && layer.is_artwork() && layer.kind == LayerKind::Paint && content).then_some(layer.id);
        let mask = layer.mask.as_ref().filter(|m| m.enabled && (m.show_area || visible)).map(|m| m.id);
        paint.into_iter().chain(mask).map(move |id| (layer, id))
    })
}

fn allocation(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>) -> [u64; 2] {
    allocation_for(r, plan, packet, sources, false, None)
}

fn allocation_for(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>) -> [u64; 2] {
    allocation_with_tiles(r, plan, packet, sources, streamed, scene, use_tiles(r, plan, packet, sources, streamed, scene))
}

fn use_tiles(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>) -> bool {
    bounded(packet.layers) && (plan.level == 0
        || !packet.layers.iter().any(|l| images::visible(packet.layers, l) && l.effect.is_some())
        || allocation_with_tiles(r, plan, packet, sources, streamed, scene, false).into_iter().sum::<u64>() > CACHE_BYTES)
}

fn allocation_with_tiles(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>, tiled: bool) -> [u64; 2] {
    let scene=scene.or(r.scene.as_ref());
    let layers = packet.layers;
    let images = if tiled { scratch_images(layers) }
        else { graph::scratch_images(packet, plan.level, r.device.working_space()).unwrap_or_else(|_| scratch_images(layers)) };
    let material = targets(r, packet).any(|(_, id)| mapped_material(r, packet, id));
    let images = images + u64::from(plan.level > 0 && material);
    let input = input_plan(plan, layers);
    let (source_bytes, root_mips) = targets(r, packet).fold((0, 0), |(sum, largest), (layer, id)| {
        let placement = layer_core::target_geometry(layers, id);
        if streamed && placement.is_identity() { return (sum, largest); }
        let Ok(source) = source_plan(scene, input, &placement, target_extent(layer,id,plan.extent), r.moving_layer == Some(id)) else { return (sum, largest); };
        if plan.level == 0 && source.level == 0 { return (sum, largest); }
        let source = sources.map_or(source, |s| s.resident_plan(id, source));
        let extra = if plan.level == 0 { 0 } else { source.level_bytes(source.level + 1) + source.level_bytes(source_coarse_level(source)) };
        (sum + source.level_bytes(source.level), largest.max(extra))
    });
    let records = records_for(r, plan, layers).next_power_of_two().max(
        r.scene.as_ref().and_then(|scene| scene.scale_commands.as_ref()).map_or(0, Commands::storage_bytes));
    let transform = transform_plans(r, plan, layers).map(|(source, kept)| {
            let records = page_coordinates(PixelRect::full(source.extent)).count() as u64 * 512;
            (source.pixel_bytes_through(display_mips::MAX_LEVEL) + records * u64::from(display_mips::MAX_LEVEL))
                * if kept { 2 } else { 1 }
        }).sum::<u64>();
    let output = plan.level_bytes(plan.level);
    let working = if tiled {
        u64::from(PAGE_SIZE).pow(2) * 16 * (images + pixel_transform::TRANSFORM_SLOTS as u64)
    } else { input.level_bytes(input.level) * (images - 1) };
    let own = [source_bytes, Scene::geometry_bytes(layers,scene) + output + working + root_mips + plan.level_bytes(plan.level + 1) + records + 32 + transform
        + exact_strip_bytes(plan.extent) + u64::from(PAGE_SIZE).pow(2) * 16 * u64::from(material) * Scene::MATERIAL_CACHE_PAGES as u64];
    if plan.bounds == PixelRect::full(plan.extent) { own }
    else {
        let overview = allocation_for(r, overview_plan(plan), packet, sources, false, scene);
        [own[0] + overview[0], own[1] + overview[1]]
    }
}

fn transform_plans<'a>(r: &'a WgpuRasterizer, plan: display_mips::Plan, layers: &'a [Layer])
    -> impl Iterator<Item = (display_mips::Plan, bool)> + 'a {
    let active = r.transform_preview.iter().filter(move |_| plan.level > 0)
        .flat_map(|p| std::iter::once(p.clone()).chain(p.companion(layers)))
        .filter_map(move |preview| {
            let layer = layers.iter().find(|l| l.id == preview.layer)?;
            let extent = layer.local_extent(plan.extent);
            let level = paint_transform::input_level(plan.level, &preview, &layer_core::target_geometry(layers, layer.id), extent);
            let (level, kept) = r.transforms.as_ref().map_or_else(
                || (level, paint_transform::keeps_pixels(preview.selection.as_ref(), PixelRect::full(extent))),
                |transforms| transforms.input_requirements(&preview, level, extent));
            Some((display_mips::Plan::at(extent, level), kept))
        });
    let standby = r.moving_pixels.as_ref().filter(|_| r.transform_preview.is_none() && plan.level > 0)
        .and_then(|(id, selection)| {
            let layer = layers.iter().find(|l| l.id == *id && l.kind == LayerKind::Paint)?;
            let extent = layer.local_extent(plan.extent);
            let level = paint_transform::sampling::selection_level(plan.level, &layer_core::target_geometry(layers, *id), Some(selection), extent);
            let (level, kept) = r.transforms.as_ref().unwrap().standby_requirements(layer, selection, level, extent);
            Some((display_mips::Plan::at(extent, level), kept))
        });
    active.chain(standby)
}

#[cfg(test)]
pub(crate) mod tests;

fn source_coarse_level(plan: display_mips::Plan) -> u32 {
    display_mips::Plan::new(plan.extent).map_or(8, |p| p.level).max(plan.level + 2)
}

fn coarse_level(plan: display_mips::Plan) -> u32 {
    plan.level + u32::from(plan.size.iter().any(|size| *size > display_mips::MAX_SIDE))
}

fn scratch_images(layers: &[Layer]) -> u64 {
    3 + u64::from((4 * layers.len().max(1)).next_power_of_two().ilog2())
        + 2 * u64::from(layers.iter().any(|l| l.effect.as_ref().is_some_and(|e| e.program.image_boundary())))
        + layers.iter().filter(|l| l.effect.is_some() && l.mask.as_ref().is_some_and(|m| m.enabled)).count().min(crate::effects::MASK_SLOTS) as u64
}

pub(super) fn record_bytes(r: &WgpuRasterizer, extent: [u32; 2], layers: usize) -> u64 {
    let pages = extent
        .map(|n| u64::from(n.div_ceil(PAGE_SIZE)))
        .into_iter()
        .product::<u64>();
    u64::from(
        r.device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(256),
    ) * ((2 * pages + 12) * layers.max(1) as u64 + 2)
}

impl Cache {
    pub fn select(
        previous: Option<Self>,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        request: Request,
        unchanged: bool,
    ) -> (Self, bool) {
        let Request { plan, evaluation } = request;
        let required = display_mips::view_bounds(packet.view, plan.extent, 2 << plan.level)
            .expect("admitted display view");
        let matches = |cache: &Self| {
            cache.evaluation == evaluation && cache.plan.extent == plan.extent && cache.plan.level == plan.level
                && cache.plan.bounds.intersect(required) == required
                && cache.layer_count == packet.layers.len()
                && (evaluation == Evaluation::Native || allocation_for(r, cache.plan, packet, None, bounded(packet.layers), None)
                    .into_iter().sum::<u64>() <= CACHE_BYTES)
        };
        let unchanged = unchanged && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty();
        let Some(mut old) = previous.filter(|c| c.submission_valid.as_ref()
            .is_none_or(|v| v.load(std::sync::atomic::Ordering::Acquire))) else {
            let mut cache = Self::new(r, request, packet.layers.len());
            cache.admit_source_overlap(r, packet);
            return (cache, true);
        };
        let unchanged = unchanged && old.transform == r.transform_preview;
        old.transform = r.transform_preview.clone();
        let hierarchy = old.hierarchy.take().filter(|c| old.plan.extent == plan.extent
            && (unchanged || !old.residency_checked || c.fits(r)));
        if hierarchy.is_none() && matches!(old.pixels, hierarchy::Pixels::Resident { .. }) {
            let mut next = Self::new(r, request, packet.layers.len());
            next.graph = old.graph.without_pixels();
            next.admit_source_overlap(r, packet);
            return (next, true);
        }
        if !unchanged {
            old.spare = None;
            old.residency_checked = false;
        }
        if matches(&old) {
            old.hierarchy = hierarchy;
            old.admit_source_overlap(r, packet);
            old.unchanged = unchanged;
            old.reuse_output = unchanged && old.ready;
            if let Some(overview) = &mut old.overview {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.ready;
            }
            return (old, false);
        }
        let reusable = old.spare.take().filter(|cache| matches(cache));
        let mut next = reusable.map(|cache| *cache)
            .unwrap_or_else(|| Self::new(r, request, packet.layers.len()));
        next.unchanged = unchanged;
        next.reuse_output = unchanged && next.ready;
        if old.evaluation == evaluation && next.overview.as_ref().map(|c| c.plan) == old.overview.as_ref().map(|c| c.plan)
            && let Some(mut overview) = old.overview.take() {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.ready;
                next.overview = Some(overview);
        }
        next.admit_source_overlap(r, packet);
        if let Some(hierarchy) = hierarchy {
            next.graph = old.graph.without_pixels();
            next.share_levels(r, &hierarchy, true);
            next.hierarchy = Some(hierarchy);
            let rebuilt = !next.reuse_output;
            return (next, rebuilt);
        }
        let bound = allocation_for(r, next.plan, packet,
            r.scene.as_ref().filter(|_| next.source_overlap).map(|s| &s.scale_sources), next.streamed_sources, None).into_iter().sum::<u64>();
        if unchanged && old.evaluation == evaluation && old.placed.is_none() && old.plan.extent == plan.extent && bound + old.storage_bytes() <= CACHE_BYTES {
            if old.plan.level == plan.level && old.ready {
                next.shifted = Some(Box::new(old));
            } else {
                next.spare = Some(Box::new(old));
            }
        }
        (next, true)
    }

    pub fn new(r: &WgpuRasterizer, request: Request, layers: usize) -> Self {
        let Request { plan, evaluation } = request;
        let overview = (plan.bounds != PixelRect::full(plan.extent))
            .then(|| Box::new(Self::new(r, Request { plan: overview_plan(plan), evaluation }, layers)));
        let geometry = Self::geometry(r, plan, overview.as_ref().map_or_else(|| coarse_level(plan), |c| coarse_level(c.plan)));
        Self {
            submission_valid: None,
            evaluation, plan,
            output: Vec::new(),
            used: Vec::new(),
            pixels: Default::default(),
            placed: None,
            geometry,
            layer_count: layers,
            spare: None,
            ready: false, source_overlap: false, streamed_sources: false,
            hierarchy: None, residency_checked: false,
            reuse_output: false,
            overview, shifted: None, valid: BTreeSet::new(), refined: BTreeSet::new(), exact_tile: None,
            unchanged: false, graph: Default::default(), transform: r.transform_preview.clone(),
        }
    }
    fn geometry(r: &WgpuRasterizer, plan: display_mips::Plan, coarse: u32) -> wgpu::Buffer {
        let level = plan.level;
        let mut geometry = [0u32; 8];
        geometry[0] = 1 << level;
        geometry[1] = 1 << coarse;
        geometry[2] = 1 << (level + 1);
        geometry[4..8].copy_from_slice(&[
            plan.bounds.min_x() >> level, plan.bounds.min_y() >> level,
            plan.bounds.max_x().div_ceil(1 << level), plan.bounds.max_y().div_ceil(1 << level),
        ]);
        r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("display composition geometry"),
                contents: &geometry
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::STORAGE,
            })
    }
    fn admit_source_overlap(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>) {
        if self.evaluation == Evaluation::Native { return; }
        self.streamed_sources = bounded(packet.layers) && allocation(r, self.plan, packet, None)
            .into_iter().sum::<u64>() > CACHE_BYTES;
        self.source_overlap = allocation(r, self.plan, packet, r.scene.as_ref().map(|s| &s.scale_sources))
            .into_iter().sum::<u64>() <= CACHE_BYTES;
        if let Some(overview) = &mut self.overview { overview.source_overlap = self.source_overlap; }
    }
    fn source_plan(&self, sources: &Sources, id: LayerId, requested: display_mips::Plan) -> display_mips::Plan {
        if self.source_overlap { sources.resident_plan(id, requested) } else { requested }
    }
    pub fn preview_level(&self, packet: FramePacket<'_>, id: LayerId) -> u32 {
        if self.evaluation == Evaluation::Native || self.plan.level == 0 { return 0; }
        let placement = layer_core::target_geometry(packet.layers, id);
        let extent = packet.layers.iter().find(|layer| layer.id == id || layer.mask.as_ref().is_some_and(|m| m.id == id))
            .map_or(packet.document_extent, |layer| layer.mask.as_ref().filter(|m| m.id == id)
                .map_or(layer.local_extent(packet.document_extent), |m| m.local_extent(layer.local_extent(packet.document_extent))));
        source_level(self.plan.level, &placement, extent).min(placement.as_affine().map_or(0, |affine|
            crate::preview_block(affine.then(layer_core::Affine(packet.view.document_to_surface)).0).ilog2()))
    }
    pub fn source_levels(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, scene: &Scene) -> BTreeMap<LayerId, BTreeMap<u32, display_mips::Plan>> {
        let sources=&scene.scale_sources;
        let plans = std::iter::once((input_plan(self.plan, packet.layers), self.streamed_sources))
            .chain(self.overview.as_ref().map(|c| (input_plan(c.plan, packet.layers), c.streamed_sources)));
        let mut requested: BTreeMap<_, BTreeMap<_, _>> = targets(r, packet)
            .filter(|(_, id)| sources.entries.contains_key(id) && !r.transforms.as_ref().is_some_and(|t| t.display_source(*id))).map(|(layer, id)| {
            let placement = layer_core::target_geometry(packet.layers, id);
            (id, plans.clone().filter(|(_, streamed)| !streamed || !placement.is_identity())
                .filter_map(|(p, _)| source_plan(Some(scene), p, &placement, target_extent(layer,id,p.extent), r.moving_layer == Some(id)).ok().filter(|s| p.level > 0 || s.level > 0))
                .filter(|p| !p.bounds.is_empty()).map(|p| (p.level, self.source_plan(sources, id, p))).collect())
        }).collect();
        if let Some(Presentation::Placed(root)) = &self.placed && let Some(levels) = requested.get_mut(&root.value.id) {
            if let Some(plan) = levels.get(&root.value.plan.level).copied() {
                for level in [plan.level + 1, source_coarse_level(plan)] {
                    levels.insert(level, display_mips::Plan::window(plan.extent, level, plan.bounds));
                }
            }
        }
        requested
    }
    pub fn source_budget(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, commands: &Commands, scene: Option<&Scene>) -> u64 {
        let reserve = allocation_for(r, self.plan, packet, scene.map(|s|&s.scale_sources).filter(|_| self.source_overlap), self.streamed_sources, scene)[1]
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes());
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let branches = self.graph.reserved_bytes(input_plan(self.plan, packet.layers))
            + self.overview.as_ref().map_or(0, |c| c.graph.reserved_bytes(input_plan(c.plan, packet.layers)));
        CACHE_BYTES.saturating_sub((reserve + retained_records + branches).max(self.working_bytes() + commands.storage_bytes()))
    }

    pub fn prepare_graph(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, scene: &Scene, commands: &Commands, tiles: Option<&BTreeSet<[u32; 2]>>) -> Result<(), GpuRasterError> {
        let sources=&scene.scale_sources;
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let budget = CACHE_BYTES.saturating_sub(allocation_for(r, self.plan, packet, self.source_overlap.then_some(sources), self.streamed_sources, Some(scene)).into_iter().sum::<u64>()
            + retained_records + self.spare.as_ref().map_or(0, |c| c.storage_bytes()) + self.shifted.as_ref().map_or(0, |c| c.storage_bytes()));
        let budget = if self.evaluation == Evaluation::Native { 0 } else { budget };
        self.prepare_root(r, packet, sources, budget, tiles)?;
        if let Some(overview) = &mut self.overview {
            overview.prepare_root(r, packet, sources, budget.saturating_sub(self.graph.reserved_bytes(input_plan(self.plan, packet.layers))), tiles)?;
        }
        Ok(())
    }

    fn prepare_root(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, budget: u64, tiles: Option<&BTreeSet<[u32; 2]>>) -> Result<(), GpuRasterError> {
        let previous = self.graph.root.clone();
        let plan = if self.evaluation == Evaluation::Native { display_mips::Plan::window(self.plan.extent, 0, self.plan.bounds) }
            else { input_plan(self.plan, packet.layers) };
        self.graph.prepare(r, packet, sources, plan, budget)?;
        if self.placed.is_some() && !self.graph.root.as_ref().unwrap().deferred(r, packet.dab_batches) {
            self.valid.clear(); self.reuse_output = false;
        }
        if previous != self.graph.root {
            let tiles = tiles.filter(|_| r.artwork_frame.as_ref().is_some_and(|old|
                old.blend_space == packet.blend_space
                    && old.view.background_rgba_linear == packet.view.background_rgba_linear
                    && old.layers.len() == packet.layers.len()
                    && old.layers.iter().zip(packet.layers).all(|(a, b)| metadata::Metadata::new(a) == metadata::Metadata::new(b))));
            if !self.unchanged && let Some(hierarchy) = &mut self.hierarchy {
                hierarchy.invalidate(PixelRect::full(self.plan.extent), tiles);
            }
            self.valid.retain(|c| tiles.is_some_and(|tiles| !tiles.contains(c)));
            self.refined.retain(|c| tiles.is_some_and(|tiles| !tiles.contains(c)));
            self.reuse_output = false;
        }
        Ok(())
    }

    pub fn view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| &self.pixels.root().expect("composed output").view, |root| root.view())
    }
    pub fn texture(&self) -> &wgpu::Texture {
        &self.pixels.root().expect("composed output").texture
    }
    pub fn coarse_view(&self) -> &wgpu::TextureView {
        if self.placed.is_none() && let hierarchy::Pixels::Resident { levels, .. } = &self.pixels {
            return &levels.last().unwrap().view;
        }
        self.placed.as_ref().map_or_else(|| self.overview.as_ref().map_or_else(|| if coarse_level(self.plan) > self.plan.level { self.next_view() } else { self.view() }, |c| c.coarse_view()), |root| root.coarse())
    }
    pub fn next_view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| &self.pixels.next().expect("composed output mip").view, |root| root.next())
    }
    pub fn placement_values(&self) -> [f32; 20] {
        let mut values = [0.; 20];
        let Some(presentation) = &self.placed else { return values; };
        let Presentation::Placed(root) = presentation else { values[13] = 2.; return values; };
        let [x, y, _] = pixel_transform::inverse_rows(&root.value.transform).expect("validated placement");
        let side = (1 << self.plan.level) as f32;
        let source_side = (1 << root.value.plan.level) as f32;
        let [width, height] = [root.value.plan.bounds.width(), root.value.plan.bounds.height()].map(|n| n as f32 / source_side);
        let [r, g, b, a] = root.value.backdrop;
        [x[0] / side, x[1] / side, x[2], 0., y[0] / side, y[1] / side, y[2], 0.,
            width, height, (1 << (root.coarse_level - root.value.plan.level)) as f32, 2.,
            root.value.opacity, 1., root.value.outside, f32::from(root.value.encode), r, g, b, a]
    }
    pub fn resample_values(&self) -> [u8; scene::resample::UNIFORM_BYTES as usize] {
        match &self.placed { Some(Presentation::Mapped(mapped)) => mapped.values, _ => [0; scene::resample::UNIFORM_BYTES as usize] }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.output
            .iter()
            .map(|i| texture_bytes(&i.texture))
            .sum::<u64>()
            + self.geometry.size()
            + self.pixels.storage_bytes()
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.overview.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.graph.storage_bytes()
            + self.exact_tile.as_ref().map_or(0, |image| texture_bytes(&image.texture))
            + self.resident_bytes()
    }
    fn allocate(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan) -> usize {
        let slot = self.used.iter().position(|used| !used).unwrap_or_else(|| {
            self.output.push(Image::new(r, plan, "display composition level"));
            self.used.push(false);
            self.used.len() - 1
        });
        if self.output[slot].plan.size != plan.size { self.output[slot] = Image::new(r, plan, "display composition level"); }
        self.output[slot].plan = plan;
        self.used[slot] = true;
        slot
    }
    pub(super) fn render(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        r.telemetry.phase_begin(9, &r.device, &r.queue, encoding.encoder);
        let records = encoding.commands.cursor;
        let mut changed = self.render_graph(scene, r, packet, dirty, encoding, Destination::View, tiles)?;
        r.telemetry.phase_end(9, encoding.encoder);
        crate::performance_trace::counter(c"Capy main composition records", u64::from(encoding.commands.cursor - records));
        if let Some(mut overview) = self.overview.take() {
            r.telemetry.phase_begin(10, &r.device, &r.queue, encoding.encoder);
            let records = encoding.commands.cursor;
            let result = overview.render_graph(scene, r, packet, dirty, encoding, Destination::Overview(self, changed), tiles);
            r.telemetry.phase_end(10, encoding.encoder);
            crate::performance_trace::counter(c"Capy overview composition records", u64::from(encoding.commands.cursor - records));
            self.overview = Some(overview);
            changed = changed.union(result?);
        }
        Ok(changed)
    }

    fn render_graph(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, destination: Destination<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let Encoding { encoder, commands } = encoding;
        if self.reuse_output { return Ok(PixelRect::EMPTY); }
        let dirty = if self.unchanged { PixelRect::EMPTY } else { dirty };
        let finer = match destination { Destination::Overview(cache, changed) => Some((cache, changed)), _ => None };
        let covered = finer.map_or(PixelRect::EMPTY, |(cache, _)| cache.plan.bounds);
        let visible: Vec<_> = packet
            .layers
            .iter()
            .rev()
            .filter(|l| images::visible(packet.layers, l) && l.kind == LayerKind::Paint && l.is_artwork() && stack::has_content(r, l))
            .collect();
        let copied = self.copy_shifted(r, encoder);
        let root = self.graph.root.clone().expect("prepared composition graph");
        let input = input_plan(self.plan, packet.layers);
        let invalid = dirty.union(root.damage(&scene.scale_sources, self.plan));
        self.valid.retain(|c| page_rect(*c).intersect(invalid).is_empty() || tiles.is_some_and(|tiles| !tiles.contains(c)));
        self.refined.retain(|c| page_rect(*c).intersect(invalid).is_empty() || tiles.is_some_and(|tiles| !tiles.contains(c)));
        let regions = page_regions(page_coordinates(self.plan.bounds).filter(|c| !self.valid.contains(c)), self.plan.bounds);
        let changed = regions.iter().fold(PixelRect::EMPTY, |a, b| a.union(*b));
        let required = if scene.scale_sources.reset { input.bounds } else { root.required(changed, input).union(changed) };
        let mut source_plans=std::collections::HashMap::new();
        let source_covered = if packet.layers.iter().any(|l| l.effect.as_ref().is_some_and(|e| !e.program.passes.is_empty())) { PixelRect::EMPTY } else { covered };
        for layer in &visible {
            if r.transforms.as_ref().is_some_and(|t| t.display_source(layer.id)) { continue; }
            let placement = layer_core::target_geometry(packet.layers, layer.id);
            if self.streamed_sources && placement.is_identity() { continue; }
            let extent = layer.local_extent(packet.document_extent);
            let requested = source_plan(Some(scene), input, &placement, extent, r.moving_layer == Some(layer.id))?;
            source_plans.insert(layer.id,(placement.clone(),extent,requested));
            if self.plan.level == 0 && requested.level == 0 { continue; }
            let plan = self.source_plan(&scene.scale_sources, layer.id, requested);
            let (needed, covered) = local_regions(Some(scene), required, source_covered, &placement, extent, plan.level)?;
            let needed = if placement.is_identity() { needed } else { plan.bounds };
            scene.prepare_scale_color(commands, r, packet, encoder, layer, SourceRequest { plan, required: needed, covered })?;
        }
        for layer in packet.layers {
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (m.show_area || images::visible(packet.layers, layer))) {
                let placement = layer_core::target_geometry(packet.layers, mask.id);
                if self.streamed_sources && placement.is_identity() { continue; }
                let extent = mask.local_extent(layer.local_extent(packet.document_extent));
                let requested = source_plan(Some(scene), input, &placement, extent, false)?;
                source_plans.insert(mask.id,(placement.clone(),extent,requested));
                if self.plan.level == 0 && requested.level == 0 { continue; }
                let plan = self.source_plan(&scene.scale_sources, mask.id, requested);
                let (needed, covered) = local_regions(Some(scene), required, source_covered, &placement, extent, plan.level)?;
                let needed = if placement.is_identity() { needed } else { plan.bounds };
                scene.prepare_scale_mask(commands, r, encoder, mask, SourceRequest { plan, required: needed, covered })?;
            }
        }
        let side = 1 << self.plan.level;
        let output_bounds = self.output_plan().bounds;
        let [x, y, width, height] = paint_transform::texel_rect(copied.window_local(output_bounds), side);
        let mut written = PixelRect::new(x, y, x + width, y + height);
        let mut written_pixels = written.area();
        let mut written_regions = u64::from(!written.is_empty());
        let regions: Vec<_> = regions.into_iter().flat_map(|r| r.subtract(covered)).filter(|r| !r.is_empty()).collect();
        let tiled = use_tiles(r, self.plan, packet, self.source_overlap.then_some(&scene.scale_sources), self.streamed_sources, Some(scene))
            && !(self.plan.level > 0 && root.fused_transform(r));
        let regions = if tiled {
            regions.into_iter().flat_map(|region| {
                let [x, y, width, height] = paint_transform::texel_rect(region, side);
                page_coordinates(PixelRect::new(x, y, x + width, y + height)).map(move |c| {
                    let p = page_rect(c);
                    PixelRect::new(p.min_x() * side, p.min_y() * side, p.max_x() * side, p.max_y() * side).intersect(region)
                })
            }).collect()
        } else { regions };
        for region in regions {
            let [x, y, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
            self.used.fill(false);
            let mut compositor = Evaluator { cache: self, commands, scene, packet, r, encoder, region, input, tiled, source_plans: &mut source_plans };
            let root = compositor.cache.graph.root.clone().expect("prepared composition graph");
            let output = compositor.evaluate_root(&root, matches!(destination, Destination::View))?;
            let presentation = match &output {
                Value::Placed(value) if finer.is_none() => {
                    let coarse_level = source_coarse_level(value.plan);
                    for level in [value.plan.level + 1, coarse_level] {
                        compositor.scene.scale_sources.ensure_level(compositor.commands, compositor.r, compositor.encoder, value.id,
                            display_mips::Plan::window(value.plan.extent, level, value.plan.bounds))?;
                    }
                    let next = compositor.scene.scale_sources.image(value.id, value.plan.level + 1).image.view.clone();
                    let coarse = compositor.scene.scale_sources.image(value.id, coarse_level).image.view.clone();
                    Some(Presentation::Placed(Placement { value: value.clone(), next, coarse, coarse_level }))
                }
                Value::Transform(source) if finer.is_none() => Some(Presentation::Mapped(compositor.r.transforms.as_ref().unwrap()
                    .presentation(source.id, source.placement, compositor.r.target_extent(source.id), source.opacity, source.backdrop, source.encode)?)),
                _ => None,
            };
            compositor.cache.placed = presentation;
            if compositor.cache.placed.is_some() {
                drop(compositor);
                self.output.clear(); self.used.clear(); self.refined.clear();
                let changed = if matches!(self.placed, Some(Presentation::Mapped(_))) {
                    self.valid.extend(page_coordinates(self.plan.bounds));
                    changed
                } else { self.valid.clear(); self.plan.bounds };
                if matches!(self.pixels, hierarchy::Pixels::Window { .. }) { self.pixels = Default::default(); }
                self.ready = true;
                return Ok(changed);
            }
            let output = compositor.materialize(output, None)?;
            assert!(matches!(output.slot(), Some(Slot::Root)));
            compositor.cache.valid.extend(page_coordinates(region));
            written = written.union(PixelRect::new(x, y, x + width, y + height));
            written_pixels += u64::from(width) * u64::from(height);
            written_regions += 1;
            r.metrics.composited_pixels += u64::from(width) * u64::from(height);
        }
        if let Some((finer, changed)) = finer {
            let region = if self.ready { changed } else { covered };
            if !region.is_empty() {
                if self.pixels.shares_levels(&finer.pixels) {
                    self.ready = true;
                } else {
                    self.pixels.ensure(r, self.plan);
                    let [x, y, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
                    let input_level = finer.plan.level + 1;
                    let mut values = [0; 20];
                    let finer_plan = finer.output_plan();
                    values[..8].copy_from_slice(&[x, y, width, height, finer_plan.bounds.width(), finer_plan.bounds.height(),
                        1 << (self.plan.level - input_level), (input_level << 8) | 8]);
                    values[14] = ((finer_plan.bounds.min_x() >> input_level) as f32).to_bits();
                    values[15] = ((finer_plan.bounds.min_y() >> input_level) as f32).to_bits();
                    let binding = Commands::binding(r, finer.next_view(), &r.empty_view, self.view());
                    commands.reduce(r, encoder, values, &binding, "derive overview from completed detail")?;
                    written = written.union(PixelRect::new(x, y, x + width, y + height));
                    written_pixels += u64::from(width) * u64::from(height);
                    written_regions += 1;
                }
                self.valid.extend(page_coordinates(region));
                self.refined.extend(finer.refined.iter().copied().filter(|c| page_rect(*c).intersect(region) == page_rect(*c).intersect(self.plan.bounds)));
            }
        }
        if self.plan.level == 0 && self.transform.is_none()
            && targets(r, packet).all(|(_, id)| layer_core::target_geometry(packet.layers, id).is_identity())
        {
            self.refined.clone_from(&self.valid);
        }
        if matches!(destination, Destination::View) {
            crate::performance_trace::counter(c"Capy main mip box pixels", written.area());
            crate::performance_trace::counter(c"Capy main mip changed pixels", written_pixels);
            crate::performance_trace::counter(c"Capy main mip regions", written_regions);
        }
        if !matches!(destination, Destination::Navigator) {
            let phase = if finer.is_some() { 12 } else { 11 };
            if crate::performance_trace::enabled() { commands.flush(r, encoder)?; }
            r.telemetry.phase_begin(phase, &r.device, &r.queue, encoder);
            self.reduce_output(r, encoder, written, commands)?;
            r.telemetry.phase_end(phase, encoder);
        }
        Ok(if written.is_empty() { written } else { PixelRect::new(
            output_bounds.min_x() + written.min_x() * side, output_bounds.min_y() + written.min_y() * side,
            output_bounds.min_x() + written.max_x() * side, output_bounds.min_y() + written.max_y() * side,
        ).intersect(self.plan.bounds) })
    }

    fn copy_shifted(&mut self, r: &WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> PixelRect {
        let bounds = self.plan.bounds;
        let mut changed = PixelRect::EMPTY;
        if let Some(old) = self.shifted.take() {
            let overlap = bounds.intersect(old.plan.bounds);
            if !overlap.is_empty() {
                self.pixels.ensure(r, self.plan);
                let side = 1 << self.plan.level;
                let [sx, sy, width, height] = paint_transform::texel_rect(overlap.window_local(old.output_plan().bounds), side);
                let [x, y, _, _] = paint_transform::texel_rect(overlap.window_local(self.output_plan().bounds), side);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: sx, y: sy, z: 0 },
                        ..old.texture().as_image_copy() },
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x, y, z: 0 },
                        ..self.texture().as_image_copy() },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );
                self.valid.extend(old.valid.iter().copied().filter(|c| !page_rect(*c).intersect(overlap).is_empty()));
                self.refined.extend(old.refined.iter().copied().filter(|c| !page_rect(*c).intersect(overlap).is_empty()));
                changed = overlap;
            }
        }
        changed
    }

    fn reduce_output(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, region: PixelRect, commands: &mut Commands) -> Result<(), GpuRasterError> {
        if region.is_empty() { return Ok(()); }
        self.pixels.ensure(r, self.plan);
        let levels: Vec<_> = match &self.pixels {
            hierarchy::Pixels::Window { root, next } => vec![root.as_ref().unwrap().clone(), next.as_ref().unwrap().clone()],
            hierarchy::Pixels::Resident { levels, level } => levels[*level as usize..].to_vec(),
        };
        let mut changed = region;
        let mut jobs = Vec::with_capacity(levels.len() - 1);
        for pair in levels.windows(2) {
            let [input, output] = pair else { unreachable!() };
            let next_origin = [changed.min_x() / 2, changed.min_y() / 2];
            let next_end = [changed.max_x().div_ceil(2), changed.max_y().div_ceil(2)];
            let next_size = [0, 1].map(|i| next_end[i] - next_origin[i]);
            let binding = Commands::binding(r, &input.view, &r.empty_view, &output.view);
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[
                next_origin[0], next_origin[1], next_size[0], next_size[1],
                input.plan.bounds.width(), input.plan.bounds.height(), 2, (input.plan.level << 8) | 8,
            ]);
            let offset = commands.record(r, encoder, values)?;
            jobs.push((binding, offset, next_size));
            changed = PixelRect::new(next_origin[0], next_origin[1], next_end[0], next_end[1]);
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce composed display levels"), timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.reduce_pair);
        for (binding, offset, size) in &jobs {
            pass.set_bind_group(0, &commands.record_binding, &[*offset]);
            pass.set_bind_group(1, binding, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
        self.ready = true;
        Ok(())
    }
}

enum Presentation {
    Placed(Placement),
    Mapped(scene::resample::Mapped),
}
impl Presentation {
    fn view(&self) -> &wgpu::TextureView { match self { Self::Placed(p) => &p.value.view, Self::Mapped(p) => &p.view } }
    fn next(&self) -> &wgpu::TextureView { match self { Self::Placed(p) => &p.next, Self::Mapped(p) => &p.view } }
    fn coarse(&self) -> &wgpu::TextureView { match self { Self::Placed(p) => &p.coarse, Self::Mapped(p) => &p.view } }
}
struct Placement {
    value: Placed,
    next: wgpu::TextureView,
    coarse: wgpu::TextureView,
    coarse_level: u32,
}

#[derive(Clone)]
struct Placed {
    id: LayerId,
    view: wgpu::TextureView,
    transform: layer_core::ImageTransform,
    plan: display_mips::Plan,
    outside: f32,
    opacity: f32,
    backdrop: [f32; 4],
    encode: bool,
}

impl Placed {
    fn record(&self, output: display_mips::Plan, texels: [u32; 4]) -> Result<[u8; scene::resample::UNIFORM_BYTES as usize], GpuRasterError> {
        let side = 1 << output.level;
        let scale = side as f32;
        scene::resample::Resample::values(scene::resample::Request {
            moved: &self.transform, kept: &self.transform,
            clip: layer_core::Affine([scale, 0., 0., scale, 0., 0.]), extent: output.extent, texels,
            display: pixel_transform::DisplayLevel { side, opacity: self.opacity, extent: output.extent, backdrop: self.backdrop, encode: self.encode },
            target: output, source: self.plan, max_lod: 0, outside: self.outside, keep_source: false, identity: false,
        })
    }
}

struct TransformSource { id: LayerId, placement: layer_core::Affine, opacity: f32, backdrop: [f32; 4], encode: bool }

#[derive(Clone)]
enum Slot { Root, Cache(usize), Scene(usize), Decoded(Arc<()>) }

#[derive(Clone)]
struct Target {
    view: wgpu::TextureView,
    slot: Option<Slot>,
    plan: display_mips::Plan,
}
impl Target {
    fn value(self) -> Value { Value::Image { view: self.view, slot: self.slot, opacity: 1., plan: self.plan, preview: None, encode: false } }
}

enum Value {
    Color([f32; 4]),
    Image { view: wgpu::TextureView, slot: Option<Slot>, opacity: f32, plan: display_mips::Plan, preview: Option<wgpu::TextureView>, encode: bool },
    Placed(Placed),
    Transform(TransformSource),
}
impl Value {
    fn needs_encode(&self) -> bool { matches!(self, Self::Image { encode: true, .. }) }
    fn with_encoding(mut self, value: bool) -> Self {
        if let Self::Image { encode, .. } = &mut self { *encode = value; }
        self
    }
    fn slot(&self) -> Option<Slot> { if let Self::Image { slot, .. } = self { slot.clone() } else { None } }
    fn view(&self) -> Option<&wgpu::TextureView> { if let Self::Image { view, .. } = self { Some(view) } else { None } }
    fn preview(&self) -> Option<&wgpu::TextureView> { if let Self::Image { preview, .. } = self { preview.as_ref() } else { None } }
    fn color(&self) -> [f32; 4] { if let Self::Color(color) = self { *color } else { [0.; 4] } }
    fn opacity(&self) -> f32 { if let Self::Image { opacity, .. } = self { *opacity } else { 1. } }
    fn with_opacity(mut self, amount: f32) -> Self {
        match &mut self {
            Self::Color(c) => *c = c.map(|v| v * amount),
            Self::Image { opacity, .. } => *opacity *= amount,
            Self::Placed(p) => { p.opacity *= amount; p.backdrop = p.backdrop.map(|v| v * amount); }
            Self::Transform(p) => { p.opacity *= amount; p.backdrop = p.backdrop.map(|v| v * amount); }
        }
        self
    }
}
struct Evaluator<'a> {
    cache: &'a mut Cache,
    commands: &'a mut Commands,
    scene: &'a mut Scene,
    packet: FramePacket<'a>,
    r: &'a mut WgpuRasterizer,
    encoder: &'a mut crate::submission::CommandEncoder,
    region: PixelRect,
    input: display_mips::Plan,
    tiled: bool,
    source_plans: &'a mut std::collections::HashMap<LayerId,(layer_core::ImageTransform,[u32;2],display_mips::Plan)>,
}
fn mapped_material(r: &WgpuRasterizer, packet: FramePacket<'_>, id: LayerId) -> bool {
    r.watercolor_style(id, packet.dab_batches).is_some()
        && (r.moving_layer == Some(id) || !layer_core::target_geometry(packet.layers, id).is_identity())
}

impl Evaluator<'_> {
    fn working_plan(&self) -> display_mips::Plan {
        if self.tiled {
            let span = PAGE_SIZE << self.cache.plan.level;
            let [x, y] = [self.region.min_x() / span * span, self.region.min_y() / span * span];
            display_mips::Plan::window(self.cache.plan.extent, self.cache.plan.level, PixelRect::new(x, y, x + span, y + span))
        } else { self.input }
    }
    fn target(&mut self) -> Target {
        let plan = self.working_plan();
        if self.tiled {
            let slot = self.scene.reserve(self.r);
            Target { view: self.scene.pool[slot].view.clone(), slot: Some(Slot::Scene(slot)), plan }
        } else {
            let slot = self.cache.allocate(self.r, plan);
            Target { view: self.cache.output[slot].view.clone(), slot: Some(Slot::Cache(slot)), plan }
        }
    }
    fn release(&mut self, slot: Option<Slot>) {
        match slot {
            Some(Slot::Cache(slot)) => self.cache.used[slot] = false,
            Some(Slot::Scene(slot)) => self.scene.free(slot),
            Some(Slot::Decoded(lease)) => drop(lease),
            Some(Slot::Root) | None => {}
        }
    }
    fn texels(&self, plan: display_mips::Plan) -> [u32; 4] {
        paint_transform::texel_rect(self.region.window_local(plan.bounds), 1 << plan.level)
    }
    fn source(&mut self, id: LayerId, placement: layer_core::ImageTransform, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let value = self.source_pixels(id, placement.clone(), extent, outside)?;
        if self.cache.plan.level == 0 || !mapped_material(self.r, self.packet, id)
            || self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) { return Ok(value); }
        let (bounds, radius) = self.scene.material_coverage(self.r, id, &placement, self.packet.dab_batches);
        if bounds.is_empty() { return Ok(value); }
        let bounds = bounds.outset((radius + 2 * (1 << self.cache.plan.level)) as f32);
        let region = self.region.intersect(paint_transform::aligned(pixel_rect(bounds, self.packet.document_extent), PAGE_SIZE, self.packet.document_extent));
        if region.is_empty() { return Ok(value); }
        let value = self.materialize(value, None)?;
        self.commands.flush(self.r, self.encoder)?;
        let layer = self.packet.layers.iter().find(|layer| layer.id == id).unwrap();
        let pages: Vec<_> = page_coordinates(region).collect();
        let plan = display_mips::Plan::window(self.packet.document_extent, self.cache.plan.level, region);
        let slot = self.cache.allocate(self.r, plan);
        let view = self.cache.output[slot].view.clone();
        self.scene.reduce_color_pages(self.commands, self.r, self.packet, self.encoder,
            layer, plan, &view, &pages, Some(placement))?;
        let material = Target { view, slot: Some(Slot::Cache(slot)), plan }.value();
        self.draw(material, value, layer_core::LayerBlend::Normal, 128 | 16384, None)
    }
    fn source_pixels(&mut self, id: LayerId, placement: layer_core::ImageTransform, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let encode = self.packet.blend_space == layer_core::BlendSpace::Perceptual
            && self.packet.layers.iter().any(|layer| layer.id == id);
        if self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) {
            return Ok(Value::Transform(TransformSource { id, placement: placement.as_affine().ok_or(GpuRasterError::InvalidTransform("Apply the transform before editing selected pixels"))?, opacity: 1., backdrop: [0.; 4], encode }));
        }
        let plan=if let Some((geometry,domain,plan))=self.source_plans.get(&id) && geometry==&placement && *domain==extent {*plan} else {
            let plan=source_plan(Some(self.scene),self.input,&placement,extent,self.r.moving_layer==Some(id))?;
            self.source_plans.insert(id,(placement.clone(),extent,plan));plan
        };
        if self.cache.plan.level == 0 && plan.level == 0 {
            let tile = [self.region.min_x() / PAGE_SIZE, self.region.min_y() / PAGE_SIZE];
            let slot = if let Some(index) = self.packet.layers.iter().position(|l| l.id == id) {
                let layer = &self.packet.layers[index];
                let stored = self.r.paint_layers.iter().find(|l| l.id == id);
                if placement.is_identity() && self.r.watercolor_style(id, self.packet.dab_batches).is_none() {
                    let preview = self.r.preview_layer_id == Some(id);
                    let inputs = match self.scene.color_inputs(self.r, layer, stored, tile, preview) {
                        Err(GpuRasterError::SourceWorkingSetExceeded) => {
                            self.commands.flush(self.r, self.encoder)?;
                            let stored = self.r.paint_layers.iter().find(|l| l.id == id);
                            self.scene.color_inputs(self.r, layer, stored, tile, preview)?
                        }
                        result => result?,
                    };
                    self.encode_scene_jobs()?;
                    let (base, preview) = match inputs {
                        [Some(base), preview] => (base, preview.map(|p| p.view)),
                        [None, Some(preview)] => (preview, None),
                        [None, None] => return Ok(Value::Color([0.; 4])),
                    };
                    return Ok(Value::Image { view: base.view, slot: base.lease.map(Slot::Decoded),
                        opacity: 1., plan: self.working_plan(), preview, encode });
                }
                self.commands.flush(self.r, self.encoder)?;
                self.scene.paint_tile(self.r, self.packet, index, tile, plan.level)?
            } else {
                let mask = self.packet.layers.iter().find_map(|l| l.mask.as_ref().filter(|m| m.id == id)).unwrap();
                self.scene.mask_at(self.r, mask, placement, extent, tile)?
            };
            self.encode_scene_jobs()?;
            return Ok(Target { view: self.scene.pool[slot].view.clone(), slot: Some(Slot::Scene(slot)), plan: self.working_plan() }.value().with_encoding(encode));
        }
        if plan.bounds.is_empty() { return Ok(Value::Color([outside; 4])); }
        let encode = encode && self.scene.scale_sources.entries[&id].blend_space == layer_core::BlendSpace::Linear;
        if self.cache.streamed_sources && placement.is_identity() {
            let source = &self.scene.scale_sources.entries[&id];
            let cached = source.levels.get(&plan.level).filter(|level|
                source.accepts(level) && self.region.intersect(level.image.plan.bounds) == self.region
                    && page_coordinates(self.region).all(|c| level.valid.contains(&c)));
            if let Some(level) = cached {
                return Ok(Value::Image { view: level.image.view.clone(), slot: None, opacity: 1., plan: level.image.plan, preview: None, encode });
            }
            self.commands.flush(self.r, self.encoder)?;
            let target = self.target();
            let mut missing = page_coordinates(self.region.intersect(PixelRect::full(extent))).collect();
            self.scene.scale_sources.entries[&id].derive_pages(self.commands, self.r, self.encoder, target.plan, &target.view, &mut missing)?;
            let pages: Vec<_> = missing.into_iter().collect();
            if let Some(layer) = self.packet.layers.iter().find(|l| l.id == id) {
                self.scene.reduce_color_pages(self.commands, self.r, self.packet, self.encoder, layer, target.plan, &target.view, &pages, None)?;
            } else {
                let mask = self.packet.layers.iter().find_map(|l| l.mask.as_ref().filter(|m| m.id == id)).unwrap();
                self.scene.reduce_mask_pages(self.commands, self.r, self.encoder, mask, target.plan, &target.view, &pages)?;
            }
            return Ok(target.value().with_encoding(encode));
        }
        let image = &self.scene.scale_sources.image(id, plan.level).image;
        let plan = image.plan;
        let view = image.view.clone();
        if placement.is_identity() && self.r.moving_layer != Some(id) && plan.level == self.cache.plan.level
            && (outside == 0. || self.region.intersect(plan.bounds) == self.region) {
            return Ok(Value::Image { view, slot: None, opacity: 1., plan, preview: None, encode });
        }
        let from_texels = layer_core::Projective::from_affine(layer_core::Affine([(1u32 << plan.level) as f32,0.,0.,(1u32 << plan.level) as f32,
            plan.bounds.min_x() as f32,plan.bounds.min_y() as f32]));
        let mut transform = placement.clone();
        if transform.placement.mesh.is_none() {
            let side = (1u32 << self.cache.plan.level) as f32;
            transform.placement = transform.placement.post(layer_core::Projective([1./side,0.,0.,0.,1./side,0.,0.,0.,1.]))
                .ok_or(GpuRasterError::InvalidTransform("Invalid source placement"))?;
        }
        transform.source_from_owner = Some(placement.source_from_owner.unwrap_or(layer_core::Projective::IDENTITY)
            .then(from_texels.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid source plan"))?).ok_or(GpuRasterError::InvalidTransform("Invalid source plan"))?);
        Ok(Value::Placed(Placed { id, view, transform, plan, outside, opacity: 1., backdrop: [0.; 4], encode }))
    }
    fn encode_scene_jobs(&mut self) -> Result<(), GpuRasterError> {
        if self.scene.jobs.is_empty() { return Ok(()); }
        self.commands.flush(self.r, self.encoder)?;
        self.scene.encode_jobs(self.r, self.encoder)
    }
    fn transform(&mut self, source: TransformSource, output: Option<Target>) -> Result<Value, GpuRasterError> {
        self.commands.flush(self.r, self.encoder)?;
        let Target { view, slot, plan } = output.unwrap_or_else(|| self.target());
        let side = 1 << self.cache.plan.level;
        let region = self.region;
        let display = pixel_transform::DisplayLevel { side, extent: self.cache.plan.extent, opacity: source.opacity, backdrop: source.backdrop, encode: source.encode };
        let mut transforms = self.r.transforms.take().unwrap();
        let result = transforms.render_region(self.r, self.encoder, source.id, source.placement, &view, display, plan, region);
        self.r.transforms = Some(transforms);
        result?;
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
    fn resample(&mut self, value: Value, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, output); }
        let Value::Placed(placed) = value else { return Ok(value); };
        let Target { view, slot, plan } = output.unwrap_or_else(|| self.target());
        let texels = self.texels(plan);
        let values = placed.record(plan, texels)?;
        if placed.transform.placement.mesh.is_none() {
            self.commands.push(self.r, self.encoder, DisplayJob::Resample {
                values, views: [placed.view, view.clone()], size: [texels[2], texels[3]],
            })?;
        } else {
            let geometry=self.scene.mesh_geometry(&placed.transform).unwrap();
            if !self.scene.display_mesh.matches(&geometry) {
                self.commands.flush(self.r,self.encoder)?;
                self.scene.display_mesh.upload(self.r,self.encoder,&geometry)?;
            }
            let side=(1u32<<plan.level) as f32;
            let region=layer_core::Rect {min:layer_core::Point {x:plan.bounds.min_x() as f32+texels[0] as f32*side,y:plan.bounds.min_y() as f32+texels[1] as f32*side},
                max:layer_core::Point {x:plan.bounds.min_x() as f32+(texels[0]+texels[2]) as f32*side,y:plan.bounds.min_y() as f32+(texels[1]+texels[3]) as f32*side}};
            let triangles=self.scene.display_mesh.range_for_region(region);
            let draw=self.scene.display_mesh.drawing(triangles);
            self.commands.push(self.r,self.encoder,DisplayJob::Mesh {values,views:[placed.view,view.clone()],texels,draw})?;
        }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
    fn materialize(&mut self, value: Value, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if matches!(value, Value::Transform(_) | Value::Placed(_)) { return self.resample(value, output); }
        if !value.needs_encode() && value.preview().is_none() && value.opacity() == 1. && output.as_ref().map_or_else(|| value.slot().is_some(), |target| value.view() == Some(&target.view)) { return Ok(value); }
        let (front, back) = if matches!(value, Value::Color(_)) { (Value::Color([0.; 4]), value) } else { (value, Value::Color([0.; 4])) };
        self.draw(front, back, layer_core::LayerBlend::Normal, 0, output)
    }
    fn draw(&mut self, front: Value, back: Value, blend: layer_core::LayerBlend, flags: u32, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let (front, back) = match (front, back) {
            (Value::Transform(mut source), Value::Color(color)) if blend == layer_core::LayerBlend::Normal && flags == 0 => {
                let alpha = source.backdrop[3];
                source.backdrop = std::array::from_fn(|i| source.backdrop[i] + color[i] * (1. - alpha));
                return if output.is_some() { self.transform(source, output) } else { Ok(Value::Transform(source)) };
            }
            (Value::Placed(mut placed), Value::Color(color))
                if blend == layer_core::LayerBlend::Normal && flags == 0 => {
                let alpha = placed.backdrop[3];
                placed.backdrop = std::array::from_fn(|i| placed.backdrop[i] + color[i] * (1. - alpha));
                return if output.is_some() { self.resample(Value::Placed(placed), output) } else { Ok(Value::Placed(placed)) };
            }
            pair => pair,
        };
        let front = self.resample(front, None)?;
        let front = if matches!(front, Value::Color(c) if c != [0.; 4]) { self.materialize(front, None)? } else { front };
        let back = self.resample(back, None)?;
        let Target { view, slot, plan } = output.unwrap_or_else(|| self.target());
        let [x, y, width, height] = self.texels(plan);
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[
            x, y, width, height, 0, 0,
            1 << self.cache.plan.level, if back.view().is_some() { 2 } else { 0 } | flags,
        ]);
        values[8..12].copy_from_slice(&back.color().map(f32::to_bits));
        values[12] = if front.view().is_some() { front.opacity() } else { 0. }.to_bits();
        values[13] = (blend_code(blend, &self.r.device, self.packet.blend_space) as f32).to_bits();
        values[14] = back.opacity().to_bits();
        values[7] |= if front.preview().is_some() { 256 } else { 0 } | if back.preview().is_some() { 512 } else { 0 };
        values[7] |= if front.needs_encode() { 1024 } else { 0 } | if back.needs_encode() { 2048 } else { 0 }
            | if self.packet.blend_space == layer_core::BlendSpace::Perceptual { 8192 } else { 0 };
        for (offset, input) in [(16, &front), (18, &back)] {
            let Value::Image { plan: source, .. } = input else { continue; };
            let target = plan;
            let side = (1 << target.level) as i32;
            values[offset] = ((target.bounds.min_x() as i32 - source.bounds.min_x() as i32) / side) as u32;
            values[offset + 1] = ((target.bounds.min_y() as i32 - source.bounds.min_y() as i32) / side) as u32;
        }
        let binding = Commands::inputs(self.r, [front.view(), back.view(), front.preview(), back.preview()]
            .map(|view| view.unwrap_or(&self.r.empty_view)), &view);
        let leases = [front.slot(), back.slot()].into_iter().filter_map(|slot| match slot {
            Some(Slot::Decoded(lease)) => Some(lease), _ => None,
        }).collect();
        self.commands.push(self.r, self.encoder, DisplayJob::Compose(Composition { values, binding, leases }))?;
        for slot in [front.slot(), back.slot()] { self.release(slot); }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
}
