//! Demand-driven presentation composition with bounded region and scale grids.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;

pub(crate) const CACHE_BYTES: u64 = 608 * 1024 * 1024;
fn exact_strip(extent: [u32; 2]) -> [u32; 2] {
    let columns = extent[0].div_ceil(PAGE_SIZE).next_power_of_two().min(SOURCE_SLOTS as u32);
    [PAGE_SIZE * columns, PAGE_SIZE * (SOURCE_SLOTS as u32 / columns).min(extent[1].div_ceil(PAGE_SIZE))]
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

#[derive(Clone, Default)]
pub(crate) struct Damage {
    pub regions: Vec<PixelRect>,
}
impl From<PixelRect> for Damage {
    fn from(region: PixelRect) -> Self { Self::from_regions([region]) }
}
impl Damage {
    pub const EMPTY: Self = Self { regions: Vec::new() };
    pub fn from_regions(regions: impl IntoIterator<Item = PixelRect>) -> Self {
        let mut damage = Self::EMPTY;
        for region in regions { damage.push(region); }
        damage
    }
    pub fn from_tiles(bounds: PixelRect, tiles: Option<&BTreeSet<[u32; 2]>>) -> Self {
        tiles.map_or_else(|| bounds.into(), |tiles| Self::from_regions(tiles.iter().map(|c| page_rect(*c).intersect(bounds))))
    }
    pub fn tile_region(tile: &brush_tiles::BrushTile) -> PixelRect {
        let [x, y] = tile.coordinate.map(|c| c * PAGE_SIZE);
        PixelRect::new(x + tile.local.min_x(), y + tile.local.min_y(), x + tile.local.max_x(), y + tile.local.max_y())
    }
    pub fn is_empty(&self) -> bool { self.regions.is_empty() }
    pub fn bounds(&self) -> PixelRect { self.regions.iter().fold(PixelRect::EMPTY, |r, n| r.union(*n)) }
    pub fn area(&self) -> u64 { self.regions.iter().map(|r| r.area()).sum() }
    pub fn intersects(&self, region: PixelRect) -> bool { self.regions.iter().any(|r| !r.intersect(region).is_empty()) }
    pub fn union(mut self, other: Self) -> Self { self.extend(&other); self }
    pub fn extend(&mut self, other: &Self) { for &region in &other.regions { self.push(region); } }
    fn push(&mut self, mut region: PixelRect) {
        if region.is_empty() { return; }
        let mut index = 0;
        while index < self.regions.len() {
            let previous = self.regions[index];
            let combined = previous.union(region);
            if combined.area() == previous.area() + region.area() - previous.intersect(region).area() {
                region = combined;
                self.regions.swap_remove(index);
                index = 0;
            } else { index += 1; }
        }
        self.regions.push(region);
    }
    pub fn intersect(&self, bounds: PixelRect) -> Self { self.map(|r| r.intersect(bounds)) }
    pub fn expand(&self, radius: u32, extent: [u32; 2]) -> Self { self.map(|r| r.expand(radius, extent)) }
    pub fn map(&self, f: impl FnMut(PixelRect) -> PixelRect) -> Self { Self::from_regions(self.regions.iter().copied().map(f)) }
    pub fn pages(&self) -> BTreeSet<[u32; 2]> { self.regions.iter().flat_map(|r| page_coordinates(*r)).collect() }
    fn update_regions(&self, plan: display_mips::Plan, initialized: &BTreeSet<[u32; 2]>) -> Vec<PixelRect> {
        let mut work = Self::from_regions(page_regions(
            page_coordinates(plan.bounds).filter(|c| !initialized.contains(c)), plan.bounds));
        for region in &self.regions {
            let region = paint_transform::aligned(*region, 1 << plan.level, plan.extent).intersect(plan.bounds);
            for coordinate in page_coordinates(region).filter(|c| initialized.contains(c)) {
                work.push(region.intersect(page_rect(coordinate)));
            }
        }
        work.regions
    }
    fn dependency(&self, radius: Option<u32>, plan: display_mips::Plan) -> Self {
        if self.is_empty() { Self::EMPTY }
        else if let Some(radius) = radius { self.expand(radius, plan.extent).intersect(plan.bounds) }
        else { plan.bounds.into() }
    }
}

/// Device recipes survive level changes; display cache retirement drops pixels only.
#[derive(Clone)]
pub(crate) struct Pipelines {
    records: wgpu::BindGroupLayout,
    inputs: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
    pub reduce_phased: Deferred<wgpu::ComputePipeline>,
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
                crate::bindings::texture(3, wgpu::ShaderStages::COMPUTE, true),
                crate::bindings::texture(4, wgpu::ShaderStages::COMPUTE, true),
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
            reduce_phased: Deferred::compute(device, "reduce offset paint to display", &layout, &shader, "reduce_phased"),
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
#[derive(Clone)]
struct SourceRequest {
    plan: display_mips::Plan,
    required: Damage,
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
struct RootComposition {
    target: wgpu::TextureView,
    composition: Composition,
}
#[expect(clippy::large_enum_variant, reason = "Display commands retain uniform records inline without per-job boxing")]
enum DisplayJob {
    Compose(Composition),
    Resample { values: [u8; resample::UNIFORM_BYTES as usize], views: [wgpu::TextureView; 2], size: [u32; 2] },
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
        for (dst, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
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
        let empty = match &job {
            DisplayJob::Compose(job) => job.values[2] == 0 || job.values[3] == 0,
            DisplayJob::Resample { size, .. } => size.contains(&0),
        };
        if empty { return Ok(()); }
        self.jobs.push(job);
        if self.jobs.len() >= 32 { self.flush(r, encoder)?; }
        Ok(())
    }
    fn flush_root(&mut self, scene: &mut Scene, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, jobs: &mut Vec<RootComposition>) -> Result<(), GpuRasterError> {
        if jobs.is_empty() { return Ok(()); }
        self.flush(r, encoder)?;
        if !scene.jobs.is_empty() { scene.encode_jobs(r, encoder)?; }
        self.jobs.extend(jobs.drain(..).map(|job|DisplayJob::Compose(job.composition)));
        self.flush(r, encoder)
    }
    fn flush(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if self.jobs.is_empty() { return Ok(()); }
        let jobs = std::mem::take(&mut self.jobs);
        let mut records = vec![0; jobs.len() * self.stride as usize];
        for (record, job) in records.chunks_exact_mut(self.stride as usize).zip(&jobs) {
            match job {
                DisplayJob::Compose(job) => for (dst, value) in record.as_chunks_mut::<4>().0.iter_mut().zip(job.values) { dst.copy_from_slice(&value.to_le_bytes()); },
                DisplayJob::Resample { values, .. } => record[..values.len()].copy_from_slice(values),
            }
        }
        let offset = self.write(r, encoder, &records)?;
        let resample = &r.scene_pipelines.resample;
        let bindings: Vec<_> = jobs.iter().enumerate().map(|(i, job)| match job {
            DisplayJob::Resample { views, .. } => Some(resample.binding(&r.device, &self.records,
                u64::from(offset + i as u32 * self.stride), [&views[0], &views[1], &views[0]])),
            DisplayJob::Compose(_) => None,
        }).collect();
        let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("display graph composition"),timestamp_writes:None});
        for (i,job) in jobs.iter().enumerate() {
            let size=match job {
                DisplayJob::Compose(job)=> {
                    pass.set_pipeline(&r.scene_pipelines.scale.compose);
                    pass.set_bind_group(0,&self.record_binding,&[offset+i as u32*self.stride]);
                    pass.set_bind_group(1,&job.binding,&[]);[job.values[2],job.values[3]]
                }
                DisplayJob::Resample {size,..}=> {
                    pass.set_pipeline(&resample.area);pass.set_bind_group(0,bindings[i].as_ref().unwrap(),&[]);*size
                }
            };
            pass.dispatch_workgroups(size[0].div_ceil(8),size[1].div_ceil(8),1);
        }
        drop(pass);
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

pub(crate) struct DisplayBackup {
    pub views: [wgpu::TextureView; 3],
    pub geometry: wgpu::Buffer,
    pub placement: [f32; 20],
    pub resample: [u8; scene::resample::UNIFORM_BYTES as usize],
    pub bytes: u64,
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
    let input = input_plan(plan, packet.scene,r.scene.as_ref().map(|s|&s.object_spatial));
    let records = records_for(r, plan, packet.scene);
    let fits = transform_plans(r, plan, packet.scene).all(|(source, _)| source.size.iter()
        .all(|size| *size <= r.device.limits().max_texture_dimension_2d)) && targets(r, packet).all(|(_, id)| {
        let source = source_plan(input, source_frame(r.scene.as_ref().map(|s| &s.scale_sources), packet.scene, id), packet.scene.target_extent(id), r.moving_layer == packet.scene.source_owner(id));
        (plan.level == 0 && source.level == 0) || source.size.iter().all(|size| *size <= r.device.limits().max_texture_dimension_2d)
    });
    let native = input.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) || !fits || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
        || allocation_for(r, plan, packet, None, bounded(packet.scene), None).into_iter().sum::<u64>() > CACHE_BYTES
        || packet.scene.order().iter().any(|handle| packet.scene.visible(*handle)
            && packet.scene.effect(*handle).is_some_and(|e| (e.program.resolution == layer_core::EffectResolution::Native && !native_pointwise_alpha(e.program))
                || (e.program.image_boundary() && level == 0)));
    #[cfg(test)]
    let native = native || r.test.exact_display;
    let evaluation = if native { Evaluation::Native } else { Evaluation::Display };
    let plan = view_plan(packet, level, evaluation).ok_or(GpuRasterError::InvalidExtent)?;
    let output_bytes = |p: display_mips::Plan| p.level_bytes(p.level) + p.level_bytes(p.level + 1) + 32;
    let material_pages = if targets(r, packet).any(|(_, id)| mapped_material(r, packet, id)) { Scene::MATERIAL_CACHE_PAGES as u64 } else { 0 };
    let native_bytes = Scene::geometry_bytes(r.scene.as_ref()) + output_bytes(plan) + exact_strip_bytes(packet.document_extent) + u64::from(PAGE_SIZE).pow(2) * 16 * material_pages
        + if plan.bounds == PixelRect::full(plan.extent) { 0 } else { output_bytes(overview_plan(plan)) };
    if plan.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) || native_bytes > CACHE_BYTES {
        return Err(GpuRasterError::ExtentUnsupported);
    }
    Ok(Request { plan, evaluation })
}

/// Where a source's reduced levels lie in the document: its pixels moved
/// right and down by `phase`, then by `shift`. The levels share the
/// document's texels while `shift` is a whole number of pages; otherwise the
/// source is moving and its levels are resampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct SourceFrame {
    pub phase: [u32; 2],
    pub shift: [i64; 2],
}
impl SourceFrame {
    pub(super) fn at(origin: [i64; 2], phase: [u32; 2]) -> Self {
        Self { phase, shift: [origin[0] - i64::from(phase[0]), origin[1] - i64::from(phase[1])] }
    }
    pub(super) fn origin(self) -> [i64; 2] { [self.shift[0] + i64::from(self.phase[0]), self.shift[1] + i64::from(self.phase[1])] }
    pub(super) fn aligned(self) -> bool { self.shift.iter().all(|v| v.rem_euclid(i64::from(PAGE_SIZE)) == 0) }
    pub(super) fn extent(self, extent: [u32; 2]) -> [u32; 2] { [extent[0] + self.phase[0], extent[1] + self.phase[1]] }
    /// The pixels of the levels, `extent` pixels of source large, that `region` covers.
    pub(super) fn local(self, region: DocRect, extent: [u32; 2]) -> PixelRect {
        region.translated(self.shift.map(|v| -v)).in_frame(self.extent(extent))
    }
    pub(super) fn document(self, region: PixelRect) -> DocRect { DocRect::from(region).translated(self.shift) }
    /// `plan` of the levels, placed in the document.
    pub(super) fn placed(self, mut plan: display_mips::Plan) -> display_mips::Plan {
        plan.doc_bounds = self.document(plan.bounds);
        plan
    }
}
pub(super) fn source_frame(sources: Option<&Sources>, scene: SceneView<'_>, id: SourceTarget) -> SourceFrame {
    let origin = scene.target_offset(id);
    SourceFrame::at(origin, sources.and_then(|sources| sources.entries.get(&id)).map_or_else(|| crate::scene::page_phase(origin), |source| source.phase))
}

pub(super) fn source_level(level: u32, frame: SourceFrame) -> u32 {
    level.min(4).saturating_sub(u32::from(!frame.aligned()))
}

fn native_pointwise_alpha(program: &layer_core::EffectProgram) -> bool {
    program.resolution == layer_core::EffectResolution::Native
        && program.kind == layer_core::EffectKind::Adjustment && program.alpha == layer_core::EffectAlpha::Filter
        && !program.image_boundary() && program.auxiliary.is_none()
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

/// The pixels of the levels that `required` document pixels need, and the
/// part of them a finer cache still covers.
fn local_damage(required: &Damage, covered: PixelRect, frame: SourceFrame, extent: [u32; 2], level: u32, moving: bool) -> (Damage, PixelRect) {
    let local = |region: PixelRect| frame.local(region.into(), extent);
    if frame.aligned() && !moving { return (required.map(local), local(covered)); }
    (required.map(|region| local(region).expand(1 << level, frame.extent(extent))), PixelRect::EMPTY)
}

fn source_plan(output: display_mips::Plan, frame: SourceFrame, extent: [u32; 2], moving: bool) -> display_mips::Plan {
    let level = source_level(output.level, frame).saturating_sub(u32::from(moving && frame.aligned()));
    let bounds = frame.local(output.doc_bounds, extent);
    let extent = frame.extent(extent);
    let bounds = if frame.aligned() && !moving { bounds } else { bounds.expand(1 << level, extent) };
    let coarse = source_coarse_level(display_mips::Plan::at(extent, level));
    let bounds = if bounds.is_empty() { PixelRect::EMPTY } else {
        paint_transform::aligned(bounds.expand(2 << coarse, extent), PAGE_SIZE.max(1 << coarse), extent)
    };
    display_mips::Plan::window(extent, level, bounds)
}

fn view_plan(packet: FramePacket<'_>, level: u32, evaluation: Evaluation) -> Option<display_mips::Plan> {
    let extent = packet.document_extent;
    if evaluation == Evaluation::Display && level > 0 && effect_radius(packet.scene, level).is_none() {
        return Some(display_mips::Plan::at(extent, level));
    }
    let visible = display_mips::view_bounds(packet.view, extent, 2 << level).ok()?;
    if visible.is_empty() { return display_mips::Plan::new(extent).ok(); }
    let bounds = paint_transform::aligned(visible.expand(PAGE_SIZE, extent), PAGE_SIZE.max(1 << level), extent);
    Some(display_mips::Plan::window(extent, level, bounds))
}

pub(super) fn bounded(scene: SceneView<'_>) -> bool {
    !scene.order().iter().any(|handle| scene.visible(*handle) && scene.effect(*handle).is_some_and(|effect| effect.program.image_boundary()))
}
fn effect_radius(scene: SceneView<'_>, level: u32) -> Option<u32> {
    stack::support(scene, level)
}
fn input_plan(plan: display_mips::Plan, scene: SceneView<'_>, objects:Option<&object_spatial::SpatialIndex>) -> display_mips::Plan {
    if effect_radius(scene, plan.level) == Some(0) { return plan; }
    let bounds = images::dependency_window(scene, plan.doc_bounds, plan.level,objects).aligned(PAGE_SIZE.max(1 << plan.level));
    let size = bounds.size().unwrap_or([1 << 24; 2]);
    let mut result = display_mips::Plan::window(plan.extent, plan.level, PixelRect::full(size));
    result.doc_bounds = bounds;
    result
}

fn overview_plan(plan: display_mips::Plan) -> display_mips::Plan {
    let level = display_mips::Plan::new(plan.extent).unwrap().level.max(plan.level + 1);
    display_mips::Plan::at(plan.extent, level)
}

fn records_for(r: &WgpuRasterizer, plan: display_mips::Plan, scene: SceneView<'_>) -> u64 {
    record_bytes(r, source_extent(plan.extent, scene), scene.order().len())
}
pub(super) fn source_extent(extent: [u32; 2], scene: SceneView<'_>) -> [u32; 2] {
    scene.order().iter().fold(extent, |size, handle| {
        let local = scene.local_extent(*handle);
        [size[0].max(local[0]), size[1].max(local[1])]
    })
}
fn targets<'r, 'p: 'r>(r: &'r WgpuRasterizer, packet: FramePacket<'p>) -> impl Iterator<Item = (OccurrenceHandle, SourceTarget)> + 'r {
    packet.scene.order().iter().copied().flat_map(move |handle| {
        let scene = packet.scene;
        let occurrence = scene.occurrence(handle).unwrap();
        let visible = scene.visible(handle);
        let target = scene.source_target(handle);
        let paint = target.filter(|_| visible && occurrence.is_artwork() && occurrence.kind() == LayerKind::Paint
            && (stack::has_content(r, scene, handle) || scene.paint_source(handle).is_some_and(|paint| !paint.raster.is_empty() || !paint.operations.is_empty())
                || packet.dab_batches.iter().any(|batch| Some(batch.target) == target)
                || target.is_some_and(|target| r.native_color_coordinates(target).next().is_some())));
        let mask = scene.mask(handle).filter(|(mask, _)| mask.enabled && (packet.inspect_mask == Some(handle) || visible))
            .map(|(mask, _)| SourceTarget::Coverage(mask.source));
        paint.into_iter().chain(mask).map(move |target| (handle, target))
    })
}

fn allocation(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>) -> [u64; 2] {
    allocation_for(r, plan, packet, sources, false, None)
}

fn allocation_for(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>) -> [u64; 2] {
    let bounded = bounded(packet.scene);
    if bounded && (plan.level == 0 || !packet.scene.order().iter().any(|handle| packet.scene.visible(*handle) && packet.scene.effect(*handle).is_some())) {
        return allocation_with_tiles(r, plan, packet, sources, streamed, scene, true);
    }
    let untiled = allocation_with_tiles(r, plan, packet, sources, streamed, scene, false);
    if bounded && untiled.into_iter().sum::<u64>() > CACHE_BYTES {
        allocation_with_tiles(r, plan, packet, sources, streamed, scene, true)
    } else { untiled }
}

fn use_tiles(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>) -> bool {
    bounded(packet.scene) && (plan.level == 0
        || !packet.scene.order().iter().any(|handle| packet.scene.visible(*handle) && packet.scene.effect(*handle).is_some())
        || allocation_with_tiles(r, plan, packet, sources, streamed, scene, false).into_iter().sum::<u64>() > CACHE_BYTES)
}

fn allocation_with_tiles(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool, scene: Option<&Scene>, tiled: bool) -> [u64; 2] {
    let scene=scene.or(r.scene.as_ref());
    let layers = packet.scene;
    let images = if tiled { scratch_images(layers) }
        else { graph::scratch_images(packet, plan.level, r.device.working_space(),scene.map(|s|&s.object_spatial)).unwrap_or_else(|_| scratch_images(layers)) };
    let material = targets(r, packet).any(|(_, id)| mapped_material(r, packet, id));
    let images = images + u64::from(plan.level > 0 && material);
    let input = input_plan(plan, layers,scene.map(|s|&s.object_spatial));
    let (source_bytes, root_mips) = targets(r, packet).fold((0, 0), |(sum, largest), (_, id)| {
        let frame = source_frame(sources.or(scene.map(|s| &s.scale_sources)), layers, id);
        if streamed && frame.aligned() { return (sum, largest); }
        let source = source_plan(input, frame, packet.scene.target_extent(id), r.moving_layer == packet.scene.source_owner(id));
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
    let own = [source_bytes+r.changed_cells.as_ref().map_or(0,changed_cells::ChangedCells::storage_bytes), Scene::geometry_bytes(scene) + output + working + root_mips + plan.level_bytes(plan.level + 1) + records + 32 + transform
        + exact_strip_bytes(plan.extent) + u64::from(PAGE_SIZE).pow(2) * 16 * u64::from(material) * Scene::MATERIAL_CACHE_PAGES as u64];
    if plan.bounds == PixelRect::full(plan.extent) { own }
    else {
        let overview = allocation_for(r, overview_plan(plan), packet, sources, false, scene);
        [own[0] + overview[0], own[1] + overview[1]]
    }
}

fn transform_plans<'a>(r: &'a WgpuRasterizer, plan: display_mips::Plan, scene: SceneView<'a>)
    -> impl Iterator<Item = (display_mips::Plan, bool)> + 'a {
    let active = r.transform_preview.iter().filter(move |_| plan.level > 0)
        .flat_map(move |preview| std::iter::once(preview.clone()).chain(preview.companion(scene)))
        .map(move |preview| {
            let extent = scene.target_extent(preview.target);
            let level = paint_transform::input_level(plan.level, &preview, extent);
            let (level, kept) = r.transforms.as_ref().map_or_else(
                || (level, paint_transform::keeps_pixels(preview.selection.as_ref(), PixelRect::full(extent))),
                |transforms| transforms.input_requirements(&preview, level, extent));
            (display_mips::Plan::at(extent, level), kept)
        });
    let standby = r.moving_pixels.as_ref().filter(|_| r.transform_preview.is_none() && plan.level > 0)
        .map(move |(target, selection)| {
            let extent = scene.target_extent(*target);
            let level = paint_transform::sampling::selection_level(plan.level, Some(selection), extent);
            let (level, kept) = r.transforms.as_ref().unwrap().standby_requirements(scene, *target, selection, level, extent);
            (display_mips::Plan::at(extent, level), kept)
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

fn scratch_images(scene: SceneView<'_>) -> u64 {
    3 + u64::from((4 * scene.order().len().max(1)).next_power_of_two().ilog2())
        + 2 * u64::from(scene.order().iter().any(|handle| scene.effect(*handle).is_some_and(|effect| effect.program.image_boundary())))
        + scene.order().iter().filter(|handle| scene.effect(**handle).is_some() && scene.mask(**handle).is_some_and(|(mask, _)| mask.enabled)).count().min(crate::effects::MASK_SLOTS) as u64
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
                && cache.layer_count == packet.scene.order().len()
                && (evaluation == Evaluation::Native || allocation_for(r, cache.plan, packet, None, bounded(packet.scene), None)
                    .into_iter().sum::<u64>() <= CACHE_BYTES)
        };
        let unchanged = unchanged && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty();
        let Some(mut old) = previous.filter(|c| c.submission_valid.as_ref()
            .is_none_or(|v| v.load(std::sync::atomic::Ordering::Acquire))) else {
            let mut cache = Self::new(r, request, packet.scene.order().len());
            cache.admit_source_overlap(r, packet);
            return (cache, true);
        };
        let unchanged = unchanged && old.transform == r.transform_preview;
        old.transform = r.transform_preview.clone();
        let hierarchy = old.hierarchy.take().filter(|c| old.plan.extent == plan.extent
            && (unchanged || !old.residency_checked || c.fits(r)));
        if hierarchy.is_none() && matches!(old.pixels, hierarchy::Pixels::Resident { .. }) {
            let mut next = Self::new(r, request, packet.scene.order().len());
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
            old.reuse_output = unchanged && old.output_complete();
            if let Some(overview) = &mut old.overview {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.output_complete();
            }
            return (old, false);
        }
        let reusable = old.spare.take().filter(|cache| matches(cache));
        let mut next = reusable.map(|cache| *cache)
            .unwrap_or_else(|| Self::new(r, request, packet.scene.order().len()));
        next.unchanged = unchanged;
        next.reuse_output = unchanged && next.output_complete();
        if old.evaluation == evaluation && next.overview.as_ref().map(|c| c.plan) == old.overview.as_ref().map(|c| c.plan)
            && let Some(mut overview) = old.overview.take() {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.output_complete();
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
        self.streamed_sources = bounded(packet.scene) && allocation(r, self.plan, packet, None)
            .into_iter().sum::<u64>() > CACHE_BYTES;
        self.source_overlap = allocation(r, self.plan, packet, r.scene.as_ref().map(|s| &s.scale_sources))
            .into_iter().sum::<u64>() <= CACHE_BYTES;
        if let Some(overview) = &mut self.overview { overview.source_overlap = self.source_overlap; }
    }
    fn source_plan(&self, sources: &Sources, id: SourceTarget, requested: display_mips::Plan) -> display_mips::Plan {
        if self.source_overlap { sources.resident_plan(id, requested) } else { requested }
    }
    pub fn native_preview_input(&self,packet:FramePacket<'_>,id:SourceTarget)->bool {
        self.graph.root.as_ref().is_some_and(|root|root.source_domains(packet.scene)[1].contains(&id))
    }
    pub fn preview_level(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, id: SourceTarget, contribution:bool) -> u32 {
        if self.evaluation == Evaluation::Native || self.plan.level == 0 { return 0; }
        if !contribution && self.native_preview_input(packet,id) { return 0; }
        let frame = source_frame(r.scene.as_ref().map(|scene| &scene.scale_sources), packet.scene, id);
        if contribution && frame.phase != [0; 2] { return 0; }
        source_level(self.plan.level, frame).min(crate::preview_block(packet.view.document_to_surface).ilog2())
    }
    pub fn source_levels(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, scene: &Scene) -> BTreeMap<SourceTarget, BTreeMap<u32, display_mips::Plan>> {
        let sources=&scene.scale_sources;
        let reduced = self.graph.root.as_ref().map(|root| root.source_domains(packet.scene)[0].clone());
        let plans = std::iter::once((input_plan(self.plan, packet.scene,Some(self.graph.objects())), self.streamed_sources))
            .chain(self.overview.as_ref().map(|c| (input_plan(c.plan, packet.scene,Some(c.graph.objects())), c.streamed_sources)));
        let mut requested: BTreeMap<_, BTreeMap<_, _>> = targets(r, packet)
            .filter(|(_, id)| sources.entries.contains_key(id) && reduced.as_ref().is_none_or(|ids| ids.contains(id))
                && !r.transforms.as_ref().is_some_and(|t| t.display_source(*id))).map(|(_, id)| {
            let frame = source_frame(Some(sources), packet.scene, id);
            (id, plans.clone().filter(|(_, streamed)| !streamed || !frame.aligned())
                .map(|(p, _)| (p, source_plan(p, frame, packet.scene.target_extent(id), r.moving_layer == packet.scene.source_owner(id))))
                .filter_map(|(p, s)| (p.level > 0 || s.level > 0).then_some(s))
                .filter(|p| !p.bounds.is_empty()).map(|p| (p.level, self.source_plan(sources, id, p))).collect())
        }).collect();
        if let Some(Presentation::Placed(root)) = &self.placed && let Some(levels) = requested.get_mut(&root.value.id)
            && let Some(plan) = levels.get(&root.value.plan.level).copied() {
                for level in [plan.level + 1, source_coarse_level(plan)] {
                    levels.insert(level, display_mips::Plan::window(plan.extent, level, plan.bounds));
                }
            }
        requested
    }
    pub(crate) fn auxiliary_budget(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->u64 {
        let required=allocation_for(r,self.plan,packet,None,self.streamed_sources,r.scene.as_ref()).into_iter().sum::<u64>();
        CACHE_BYTES.saturating_sub(required+self.graph.reserved_bytes(input_plan(self.plan,packet.scene,Some(self.graph.objects()))))
            .min(CACHE_BYTES.saturating_sub(self.working_bytes()))
    }
    pub fn source_budget(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, commands: &Commands, scene: Option<&Scene>) -> u64 {
        let reserve = allocation_for(r, self.plan, packet, scene.map(|s|&s.scale_sources).filter(|_| self.source_overlap), self.streamed_sources, scene)[1]
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes());
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.scene).next_power_of_two());
        let branches = self.graph.reserved_bytes(input_plan(self.plan, packet.scene,Some(self.graph.objects())))
            + self.overview.as_ref().map_or(0, |c| c.graph.reserved_bytes(input_plan(c.plan, packet.scene,Some(c.graph.objects()))));
        CACHE_BYTES.saturating_sub((reserve + retained_records + branches).max(self.working_bytes() + commands.storage_bytes()))
    }

    pub fn prepare_graph(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, scene: &Scene, commands: &Commands, tiles: Option<&BTreeSet<[u32; 2]>>) -> Result<(), GpuRasterError> {
        let sources=&scene.scale_sources;
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.scene).next_power_of_two());
        let budget = CACHE_BYTES.saturating_sub(allocation_for(r, self.plan, packet, self.source_overlap.then_some(sources), self.streamed_sources, Some(scene)).into_iter().sum::<u64>()
            + retained_records + self.spare.as_ref().map_or(0, |c| c.storage_bytes()) + self.shifted.as_ref().map_or(0, |c| c.storage_bytes()));
        let budget = if self.evaluation == Evaluation::Native { 0 } else { budget };
        self.prepare_root(r, packet, sources, budget, tiles)?;
        if let Some(overview) = &mut self.overview {
            overview.prepare_root(r, packet, sources, budget.saturating_sub(self.graph.reserved_bytes(input_plan(self.plan, packet.scene,Some(self.graph.objects())))), tiles)?;
        }
        Ok(())
    }

    fn prepare_root(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, budget: u64, tiles: Option<&BTreeSet<[u32; 2]>>) -> Result<(), GpuRasterError> {
        let previous = self.graph.root.clone();
        let plan = if self.evaluation == Evaluation::Native { display_mips::Plan::window(self.plan.extent, 0, self.plan.bounds) }
            else { input_plan(self.plan, packet.scene,Some(self.graph.objects())) };
        self.graph.prepare(r, packet, sources, plan, budget)?;
        if self.placed.is_some() && !self.graph.root.as_ref().unwrap().deferred(r, packet.scene, packet.dab_batches) {
            self.valid.clear(); self.reuse_output = false;
        }
        if previous != self.graph.root {
            if let Some(damage) = r.artwork_frame.as_ref().and_then(|old| Scene::object_edit_damage(old.scene.view().with_scope(&old.scope), packet.scene, self.plan.extent)) {
                if let Some(hierarchy) = &mut self.hierarchy { hierarchy.invalidate(damage.clone()); }
                self.valid.retain(|c| !damage.intersects(page_rect(*c)));
                self.refined.retain(|c| !damage.intersects(page_rect(*c)));
                self.reuse_output = false;
                return Ok(());
            }
            let tiles = tiles.filter(|_| r.artwork_frame.as_ref().is_some_and(|old|
                old.blend_space == packet.blend_space
                    && (old.time == packet.time_seconds || !packet.scene.order().iter().any(|handle|
                        packet.scene.visible(*handle) && packet.scene.effect(*handle).is_some_and(|effect| effect.animated())))
                    && old.scene.view().with_scope(&old.scope).order() == packet.scene.order()
                    && packet.scene.order().iter().all(|handle| metadata::Metadata::new(old.scene.view().with_scope(&old.scope), *handle) == metadata::Metadata::new(packet.scene, *handle))));
            if !self.unchanged && let Some(hierarchy) = &mut self.hierarchy {
                hierarchy.invalidate(Damage::from_tiles(PixelRect::full(self.plan.extent), tiles));
            }
            if self.evaluation == Evaluation::Native || tiles.is_none() {
                self.valid.retain(|c| tiles.is_some_and(|tiles| !tiles.contains(c)));
            }
            self.refined.retain(|c| tiles.is_some_and(|tiles| !tiles.contains(c)));
            self.reuse_output = false;
        }
        Ok(())
    }

    pub(super) fn refresh_objects(&mut self) {
        self.reuse_output = false;
        if let Some(overview) = &mut self.overview { overview.reuse_output = false; }
    }
    fn output_complete(&self) -> bool {
        self.ready && (self.placed.is_some() || page_coordinates(self.plan.bounds).all(|coordinate| self.valid.contains(&coordinate)))
    }

    pub fn view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| &self.pixels.root().expect("composed output").view, |root| root.view())
    }
    pub fn backup(&self,r:&WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder)->Option<DisplayBackup> {
        if self.placed.is_none() && self.pixels.root().is_none() {return None;}
        let levels=match &self.placed {Some(Presentation::Placed(p))=>[0,1,p.coarse_level-p.value.plan.level],_=>[0;3]};
        let sources=[self.view(),self.next_view(),self.coarse_view()];
        let mut bytes=0;
        let views=std::array::from_fn(|index| {
            let source=sources[index].texture();let level=levels[index].min(source.mip_level_count()-1);
            let size=wgpu::Extent3d {width:(source.width()>>level).max(1),height:(source.height()>>level).max(1),depth_or_array_layers:1};
            let texture=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("retained accepted display"),size,
                mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:source.format(),
                usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
            encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {mip_level:level,..source.as_image_copy()},texture.as_image_copy(),size);
            bytes+=texture_bytes(&texture);
            texture.create_view(&Default::default())
        });
        Some(DisplayBackup {views,geometry:self.geometry.clone(),placement:self.placement_values(),resample:self.resample_values(),bytes})
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
        let shifted = |row: [f32; 3]| (f64::from(row[2]) - (f64::from(row[0]) * root.value.shift[0] as f64 + f64::from(row[1]) * root.value.shift[1] as f64) / f64::from(side)) as f32;
        let source_side = (1 << root.value.plan.level) as f32;
        let [width, height] = [root.value.plan.bounds.width(), root.value.plan.bounds.height()].map(|n| n as f32 / source_side);
        let [r, g, b, a] = root.value.backdrop;
        [x[0] / side, x[1] / side, shifted(x), 0., y[0] / side, y[1] / side, shifted(y), 0.,
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

    #[expect(clippy::too_many_arguments, reason = "Graph rendering keeps frame input, damage, encoding, and destination domains explicit")]
    fn render_graph(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, destination: Destination<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let Encoding { encoder, commands } = encoding;
        if self.reuse_output { return Ok(PixelRect::EMPTY); }
        let dirty = if self.unchanged { PixelRect::EMPTY } else { dirty };
        let objects = packet.scene.order().iter().any(|&owner| packet.scene.object_layer(owner).is_some());
        let finer = match destination { Destination::Overview(cache, changed) if !objects => Some((cache, changed)), _ => None };
        let covered = finer.map_or(PixelRect::EMPTY, |(cache, _)| cache.plan.bounds);
        let visible: Vec<_> = packet.scene.order().iter().copied().rev()
            .filter(|handle| packet.scene.visible(*handle) && packet.scene.occurrence(*handle).unwrap().kind() == LayerKind::Paint
                && stack::has_content(r, packet.scene, *handle)).collect();
        let copied = self.copy_shifted(r, encoder);
        let root = self.graph.root.clone().expect("prepared composition graph");
        let reduced = root.source_domains(packet.scene)[0].clone();
        let input = input_plan(self.plan, packet.scene,Some(self.graph.objects()));
        let invalid = tiles.map_or_else(|| Damage::from(dirty), |_| Damage::from_regions(r.document_damage.iter().copied()
            .chain(r.transform_damage.iter().map(|&(target, region)|
                brush_tiles::document_damage(packet.scene, target, region, packet.document_extent)))))
            .union(root.damage(&scene.scale_sources, self.plan));
        if objects { self.valid.retain(|c| !invalid.intersects(page_rect(*c))); }
        self.refined.retain(|c| !invalid.intersects(page_rect(*c)));
        let regions = invalid.update_regions(self.plan, &self.valid);
        let changed = regions.iter().fold(PixelRect::EMPTY, |a, b| a.union(*b));
        let required = if scene.scale_sources.reset { input.bounds.into() } else {
            let changed = Damage::from_regions(regions.iter().copied());
            root.required(changed.clone(), input).union(changed)
        };
        let mut source_plans=std::collections::HashMap::new();
        let source_covered = if packet.scene.order().iter().any(|handle| packet.scene.effect(*handle).is_some_and(|effect| !effect.program.passes.is_empty())) { PixelRect::EMPTY } else { covered };
        for &handle in &visible {
            let id = packet.scene.source_target(handle).unwrap();
            if !reduced.contains(&id) { continue; }
            if r.transforms.as_ref().is_some_and(|t| t.display_source(id)) { continue; }
            let frame = source_frame(Some(&scene.scale_sources), packet.scene, id);
            if self.streamed_sources && frame.aligned() { continue; }
            let extent = packet.scene.local_extent(handle);
            let requested = source_plan(input, frame, extent, r.moving_layer == Some(handle));
            source_plans.insert(id, (frame, extent, requested));
            if self.plan.level == 0 && requested.level == 0 { continue; }
            let plan = self.source_plan(&scene.scale_sources, id, requested);
            let (needed, covered) = if input.doc_bounds != DocRect::from(input.bounds) {
                (plan.bounds.into(), PixelRect::EMPTY)
            } else { local_damage(&required, source_covered, frame, extent, plan.level, r.moving_layer == Some(handle)) };
            scene.prepare_scale_color(commands, r, packet, encoder, handle, SourceRequest { plan, required: needed, covered })?;
        }
        for &handle in packet.scene.order() {
            if let Some((mask, source)) = packet.scene.mask(handle).filter(|(mask, _)| mask.enabled && (packet.inspect_mask == Some(handle) || packet.scene.visible(handle))) {
                let id = SourceTarget::Coverage(mask.source);
                if !reduced.contains(&id) { continue; }
                let frame = source_frame(Some(&scene.scale_sources), packet.scene, id);
                if self.streamed_sources && frame.aligned() { continue; }
                let extent = source.domain;
                let requested = source_plan(input, frame, extent, false);
                source_plans.insert(id, (frame, extent, requested));
                if self.plan.level == 0 && requested.level == 0 { continue; }
                let plan = self.source_plan(&scene.scale_sources, id, requested);
                let (needed, covered) = if input.doc_bounds != DocRect::from(input.bounds) {
                (plan.bounds.into(), PixelRect::EMPTY)
            } else { local_damage(&required, source_covered, frame, extent, plan.level, false) };
                scene.prepare_scale_mask(commands, r, encoder, mask, source, SourceRequest { plan, required: needed, covered })?;
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
        self.prepare_native_branches(scene, r, packet, &regions, input, &mut Encoding { encoder, commands })?;
        let mut root_compositions = Vec::new();
        for region in regions {
            let [x, y, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
            self.used.fill(false);
            let mut compositor = Evaluator { cache: self, commands, scene, packet, r, encoder, region: region.into(), input, tiled, source_plans: &mut source_plans, root_compositions: &mut root_compositions, defer_root: root.required(region.into(), input).bounds() == region };
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
            if compositor.scene.object_results.needs_retirement() {
                compositor.commands.flush(compositor.r, compositor.encoder)?;
                compositor.scene.object_results.flush_retired(compositor.encoder);
            }
            compositor.cache.valid.extend(page_coordinates(region));
            written = written.union(PixelRect::new(x, y, x + width, y + height));
            written_pixels += u64::from(width) * u64::from(height);
            written_regions += 1;
            r.metrics.composited_pixels += u64::from(width) * u64::from(height);
            r.metrics.frame_composited_pages.extend(page_coordinates(region).map(|coordinate| (self.plan.level, coordinate)));
        }
        commands.flush_root(scene, r, encoder, &mut root_compositions)?;
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
        if self.plan.level == 0 && self.transform.is_none() {
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

#[expect(clippy::large_enum_variant, reason = "Per-frame presentation retains GPU views and geometry inline without boxing")]
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
    id: SourceTarget,
    view: wgpu::TextureView,
    transform: layer_core::ImageTransform,
    shift: [i64; 2],
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
        let mut target = output;
        target.doc_bounds = output.doc_bounds.translated(self.shift.map(|v| -v));
        scene::resample::Resample::values(scene::resample::Request {
            moved: &self.transform, kept: &self.transform,
            clip: layer_core::Affine([scale, 0., 0., scale, -(target.doc_bounds.min[0] as f32), -(target.doc_bounds.min[1] as f32)]), extent: [output.bounds.width(), output.bounds.height()], texels,
            display: pixel_transform::DisplayLevel { side, opacity: self.opacity, extent: output.extent, backdrop: self.backdrop, encode: self.encode },
            target, source: self.plan, max_lod: 0, outside: self.outside, keep_source: false, identity: false,
        })
    }
}

struct TransformSource { id: SourceTarget, placement: layer_core::Affine, opacity: f32, backdrop: [f32; 4], encode: bool }

#[derive(Clone)]
enum Slot { Root, Cache(usize), Scene(usize), Scenes([usize; 2]), Decoded(Arc<()>) }

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
    region: DocRect,
    input: display_mips::Plan,
    tiled: bool,
    source_plans: &'a mut std::collections::HashMap<SourceTarget,(SourceFrame,[u32;2],display_mips::Plan)>,
    root_compositions: &'a mut Vec<RootComposition>,
    defer_root: bool,
}
fn mapped_material(r: &WgpuRasterizer, packet: FramePacket<'_>, id: SourceTarget) -> bool {
    r.watercolor_style(id, packet.dab_batches).is_some() && r.moving_layer == packet.scene.source_owner(id)
}

impl Evaluator<'_> {
    fn working_plan(&self) -> display_mips::Plan {
        if self.tiled && self.region.min.iter().all(|n| *n >= 0) {
            let span = PAGE_SIZE << self.cache.plan.level;
            let [x, y] = [self.region.min[0] as u32 / span * span, self.region.min[1] as u32 / span * span];
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
            Some(Slot::Scenes(slots)) => for slot in slots { self.scene.free(slot); },
            Some(Slot::Decoded(lease)) => drop(lease),
            Some(Slot::Root) | None => {}
        }
    }
    fn texels(&self, plan: display_mips::Plan) -> [u32; 4] {
        paint_transform::texel_rect(plan.doc_bounds.local(self.region), 1 << plan.level)
    }
    fn source(&mut self, id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let value = self.source_pixels(id, frame, extent, outside)?;
        if self.cache.plan.level == 0 || !mapped_material(self.r, self.packet, id)
            || self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) { return Ok(value); }
        let origin = frame.origin();
        let (bounds, radius) = self.scene.material_coverage(self.r, id, origin, self.packet.dab_batches);
        if bounds.is_empty() { return Ok(value); }
        let material = bounds.expand(radius + 2 * (1 << self.cache.plan.level)).aligned(PAGE_SIZE);
        let region = self.region.aligned(PAGE_SIZE).intersect(material).intersect(self.input.doc_bounds);
        if region.is_empty() { return Ok(value); }
        let value = self.materialize(value, None)?;
        self.commands.flush(self.r, self.encoder)?;
        let handle = self.packet.scene.source_owner(id).unwrap();
        let size = region.size()?;
        let local = PixelRect::full(size);
        let pages: Vec<_> = page_coordinates(local).collect();
        let mut plan = display_mips::Plan::window(self.packet.document_extent, self.cache.plan.level, local);
        plan.doc_bounds = region;
        let slot = self.cache.allocate(self.r, plan);
        let view = self.cache.output[slot].view.clone();
        let packet = FramePacket { scene: self.packet.scene.with_offset64(region.min.map(|n| -(n as f64))), document_extent: size, ..self.packet };
        let placement = [origin[0] - region.min[0], origin[1] - region.min[1]];
        let local_plan = display_mips::Plan::window(size, self.cache.plan.level, local);
        self.scene.reduce_color_pages(self.commands, self.r, packet, self.encoder,
            handle, local_plan, &view, &pages, Some(placement), [0; 2], &BTreeMap::new())?;
        let material = Target { view, slot: Some(Slot::Cache(slot)), plan }.value();
        self.draw(material, value, layer_core::LayerBlend::Normal, 128 | 16384, None)
    }
    fn objects(&mut self,content:&object_spatial::Content,moving:bool,output:Option<Target>)->Result<Value,GpuRasterError> {
        self.commands.flush(self.r,self.encoder)?;
        let mut target=self.target();let side=1u32<<target.plan.level;
        let bounds=self.region.intersect(target.plan.doc_bounds).aligned(side);
        let size=bounds.size().map_err(|_|GpuRasterError::ExtentUnsupported)?;
        target.plan.doc_bounds=bounds;target.plan.size=size.map(|v|v.div_ceil(side));target.plan.bounds=PixelRect::full(size);
        let plan=target.plan;
        let window=objects::ObjectWindow {origin:bounds.min.map(|v|v as f64),side:f64::from(side),size:plan.size};
        let mut request=self.scene.collection_job(self.r,self.packet,content.owner(),content.clone(),window);
        request.live=true;request.display=true;
        if request.keys.is_empty() {self.release(target.slot);return self.materialize(Value::Color([0.;4]),output);}
        let moving = moving || self.r.moving_layer.is_some();
        let preview = if moving {Scene::collection_preview(self.r,&request)?} else {None};
        if (!moving || preview.is_none()) && let Some(pieces)=self.scene.canonical_cover(self.r,self.packet,content,&request)? {
            let window=request.bounds();
            self.r.encode_clear(self.encoder,&target.view,"canonical object pieces");
            for (view,piece) in pieces {
                let shared=piece.intersect(window);
                let texels=|rect:DocRect,origin:[i64;2]|std::array::from_fn::<u32,2,_>(|i|((rect.min[i]-origin[i])/i64::from(side)) as u32);
                let [width,height]=shared.size()?.map(|v|v/side);
                let [x,y]=texels(shared,piece.min);
                let [dx,dy]=texels(shared,window.min);
                self.encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {texture:view.texture(),mip_level:0,origin:wgpu::Origin3d {x,y,z:0},aspect:wgpu::TextureAspect::All},
                    wgpu::TexelCopyTextureInfo {texture:target.view.texture(),mip_level:0,origin:wgpu::Origin3d {x:dx,y:dy,z:0},aspect:wgpu::TextureAspect::All},
                    wgpu::Extent3d {width,height,depth_or_array_layers:1});
            }
            return self.materialize(Value::Image {view:target.view,slot:target.slot,opacity:1.,plan,preview:None,encode:false},output);
        }
        let Some((sources,objects))=(if moving {preview} else {Scene::collection_preview(self.r,&request)?}) else {self.release(target.slot);return Err(GpuRasterError::DeferredObjectWork);};
        let scratch=(crate::object_sampling::CollectionPreview::passes(&objects)>1).then(||self.target());
        self.scene.jobs.push(Job::Collection(Box::new(crate::object_sampling::CollectionPreview {sources,objects,size:plan.size,output:target.view.clone(),
            scratch:scratch.as_ref().map(|scratch|scratch.view.clone()),encode:self.packet.blend_space==layer_core::BlendSpace::Perceptual})));
        self.encode_scene_jobs()?;
        if let Some(scratch)=scratch {self.release(scratch.slot);}
        self.materialize(Value::Image {view:target.view,slot:target.slot,opacity:1.,plan,preview:None,encode:false},output)
    }
    fn source_pixels(&mut self, id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let encode = self.packet.blend_space == layer_core::BlendSpace::Perceptual
            && matches!(id, SourceTarget::Paint(_));
        let origin = frame.origin();
        if self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) {
            return Ok(Value::Transform(TransformSource { id, placement: layer_core::Affine::translation(layer_core::offsets::point(origin)), opacity: 1., backdrop: [0.; 4], encode }));
        }
        let moving = self.r.moving_layer == self.packet.scene.source_owner(id);
        let plan = if let Some((cached, domain, plan)) = self.source_plans.get(&id) && *cached == frame && *domain == extent { *plan } else {
            let plan = source_plan(self.input, frame, extent, moving);
            self.source_plans.insert(id, (frame, extent, plan));
            plan
        };
        if self.cache.plan.level == 0 && plan.level == 0 {
            let tile = [self.region.in_frame(self.packet.document_extent).min_x() / PAGE_SIZE, self.region.in_frame(self.packet.document_extent).min_y() / PAGE_SIZE];
            let slot = if let SourceTarget::Paint(_) = id {
                let handle = self.packet.scene.source_owner(id).unwrap();
                let stored = self.r.paint_layers.iter().find(|stored| stored.id == id);
                if origin == [0; 2] && self.r.watercolor_style(id, self.packet.dab_batches).is_none() {
                    let preview = self.r.preview_layer_id == Some(id);
                    let inputs = match self.scene.color_inputs(self.r, self.packet.scene, id, stored, tile, preview) {
                        Err(GpuRasterError::SourceWorkingSetExceeded) => {
                            self.commands.flush(self.r, self.encoder)?;
                            let stored = self.r.paint_layers.iter().find(|stored| stored.id == id);
                            self.scene.color_inputs(self.r, self.packet.scene, id, stored, tile, preview)?
                        }
                        result => result?,
                    };
                    self.encode_scene_jobs()?;
                    let (base, preview) = match inputs {
                        [Some(base), preview] => (base, preview.map(|preview| preview.view)),
                        [None, Some(preview)] => (preview, None),
                        [None, None] => return Ok(Value::Color([0.; 4])),
                    };
                    return Ok(Value::Image { view: base.view, slot: base.lease.map(Slot::Decoded), opacity: 1., plan: self.working_plan(), preview, encode });
                }
                self.commands.flush(self.r, self.encoder)?;
                if self.r.watercolor_style(id, self.packet.dab_batches).is_none() {
                    let (color, flow) = self.scene.placed_color(self.r, self.packet.scene, id, origin, tile)?;
                    self.encode_scene_jobs()?;
                    let view = self.scene.pool[color].view.clone();
                    let preview = flow.map(|flow| self.scene.pool[flow].view.clone());
                    let slot = flow.map_or(Slot::Scene(color), |flow| Slot::Scenes([color, flow]));
                    return Ok(Value::Image { view, slot: Some(slot), opacity: 1., plan: self.working_plan(), preview, encode });
                }
                self.scene.paint_tile(self.r, self.packet, handle, tile)?
            } else {
                let handle = self.packet.scene.source_owner(id).unwrap();
                let (mask, source) = self.packet.scene.mask(handle).unwrap();
                self.scene.mask_at(self.r, mask, source, origin, tile)
            };
            self.encode_scene_jobs()?;
            return Ok(Target { view: self.scene.pool[slot].view.clone(), slot: Some(Slot::Scene(slot)), plan: self.working_plan() }.value().with_encoding(encode));
        }
        if plan.bounds.is_empty() { return Ok(Value::Color([outside; 4])); }
        let encode = encode && self.scene.scale_sources.entries[&id].blend_space == layer_core::BlendSpace::Linear;
        if self.cache.streamed_sources && frame.aligned() {
            let source = &self.scene.scale_sources.entries[&id];
            let local = frame.local(self.region, extent);
            let cached = source.levels.get(&plan.level).filter(|level|
                source.accepts(level) && self.region.intersect(frame.document(level.image.plan.bounds)) == self.region
                    && page_coordinates(local).all(|c| level.valid.contains(&c)));
            if let Some(level) = cached {
                return Ok(Value::Image { view: level.image.view.clone(), slot: None, opacity: 1., plan: frame.placed(level.image.plan), preview: None, encode });
            }
            self.commands.flush(self.r, self.encoder)?;
            let target = self.target();
            let mut missing = page_coordinates(local).collect();
            if frame.shift == [0; 2] {
                self.scene.scale_sources.entries[&id].derive_pages(self.commands, self.r, self.encoder, target.plan, &target.view, &mut missing)?;
            }
            let pages: Vec<_> = missing.into_iter().collect();
            let handle = self.packet.scene.source_owner(id).unwrap();
            if matches!(id, SourceTarget::Paint(_)) {
                self.scene.reduce_color_pages(self.commands, self.r, self.packet, self.encoder, handle, target.plan, &target.view, &pages, None, frame.shift, &BTreeMap::new())?;
            } else {
                let (mask, source) = self.packet.scene.mask(handle).unwrap();
                self.scene.reduce_mask_pages(self.commands, self.r, self.encoder, mask, source, target.plan, &target.view, &pages, frame.shift)?;
            }
            return Ok(target.value().with_encoding(encode));
        }
        let image = &self.scene.scale_sources.image(id, plan.level).image;
        let plan = image.plan;
        let view = image.view.clone();
        if frame.aligned() && !moving && plan.level == self.cache.plan.level
            && (outside == 0. || self.region.intersect(frame.document(plan.bounds)) == self.region) {
            return Ok(Value::Image { view, slot: None, opacity: 1., plan: frame.placed(plan), preview: None, encode });
        }
        let from_texels = layer_core::Projective::from_affine(layer_core::Affine([(1u32 << plan.level) as f32,0.,0.,(1u32 << plan.level) as f32,
            plan.bounds.min_x() as f32,plan.bounds.min_y() as f32]));
        let side = (1u32 << self.cache.plan.level) as f32;
        let mut transform = layer_core::ImageTransform::default();
        transform.placement = transform.placement.post(layer_core::Projective([1./side,0.,0.,0.,1./side,0.,0.,0.,1.]))
            .ok_or(GpuRasterError::InvalidTransform("Invalid source placement"))?;
        transform.source_from_owner = Some(from_texels.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid source plan"))?);
        Ok(Value::Placed(Placed { id, view, transform, shift: frame.shift, plan, outside, opacity: 1., backdrop: [0.; 4], encode }))
    }
    fn encode_scene_jobs(&mut self) -> Result<(), GpuRasterError> {
        if self.scene.jobs.is_empty() { return Ok(()); }
        self.commands.flush(self.r, self.encoder)?;
        self.scene.encode_jobs(self.r, self.encoder)
    }
    fn transform(&mut self, source: TransformSource, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if output.as_ref().is_some_and(|target|matches!(target.slot,Some(Slot::Root))) { self.commands.flush_root(self.scene,self.r,self.encoder,self.root_compositions)?; }
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
        if output.as_ref().is_some_and(|target|matches!(target.slot,Some(Slot::Root))) { self.commands.flush_root(self.scene,self.r,self.encoder,self.root_compositions)?; }
        let Target { view, slot, plan } = output.unwrap_or_else(|| self.target());
        let texels = self.texels(plan);
        let values = placed.record(plan, texels)?;
        self.commands.push(self.r, self.encoder, DisplayJob::Resample {
            values, views: [placed.view, view.clone()], size: [texels[2], texels[3]],
        })?;
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
        let deferred = if matches!(slot, Some(Slot::Root)) {
            let retained = |input: &Value| matches!(input,
                Value::Image { view: source, slot: None, opacity: 1., plan: source_plan, preview: None, encode: false }
                if *source_plan == plan && plan.doc_bounds == DocRect::from(PixelRect::full(plan.extent)) && source.texture() != view.texture()
                    && source.texture().width() == plan.size[0] && source.texture().height() == plan.size[1]
                    && (self.cache.initialized_pages(&Target {view:source.clone(),slot:None,plan}).is_some()
                        || self.scene.scale_sources.entries.values().any(|s|s.levels.values().any(|l|l.image.view==*source))));
            let clip = PixelRect::new(x, y, x + width, y + height);
            let deferred = self.defer_root && blend == layer_core::LayerBlend::Normal && flags == 0
                && retained(&front) && retained(&back) && clip != PixelRect::full(plan.size);
            if !deferred || self.root_compositions.len() == 32 || self.root_compositions.iter().any(|job| {
                let [x,y,width,height]=job.composition.values[..4].try_into().unwrap();
                job.target != view || !PixelRect::new(x,y,x+width,y+height).intersect(clip).is_empty()
            }) { self.commands.flush_root(self.scene, self.r, self.encoder, self.root_compositions)?; }
            deferred
        } else { false };
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
            let side = 1 << target.level;
            values[offset] = ((target.doc_bounds.min[0] - source.doc_bounds.min[0]) / i64::from(side)) as u32;
            values[offset + 1] = ((target.doc_bounds.min[1] - source.doc_bounds.min[1]) / i64::from(side)) as u32;
        }
        let binding = Commands::inputs(self.r, [front.view(), back.view(), front.preview(), back.preview()]
            .map(|view| view.unwrap_or(&self.r.empty_view)), &view);
        let leases = [front.slot(), back.slot()].into_iter().filter_map(|slot| match slot {
            Some(Slot::Decoded(lease)) => Some(lease), _ => None,
        }).collect();
        let composition = Composition { values, binding, leases };
        if deferred { self.root_compositions.push(RootComposition {target:view.clone(),composition}); }
        else { self.commands.push(self.r, self.encoder, DisplayJob::Compose(composition))?; }
        for slot in [front.slot(), back.slot()] { self.release(slot); }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
}

#[cfg(test)]
mod signed_region_tests;
