//! Demand-driven presentation composition with bounded region and scale grids.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;

pub(super) const APPROXIMATE_PAGES: usize = 4;
fn exact_strip(extent: [u32; 2]) -> [u32; 2] {
    let columns = extent[0].div_ceil(PAGE_SIZE).next_power_of_two().min((SOURCE_SLOTS as u32).isqrt().next_power_of_two());
    [PAGE_SIZE * columns, PAGE_SIZE * (SOURCE_SLOTS as u32 / columns).min(extent[1].div_ceil(PAGE_SIZE))]
}

mod sources;
mod graph;
mod signatures;
pub(crate) use signatures::{Capture,Declared,Input,Prepared as SignatureFrame,PreviewDeclaration,PreviewVersion,PreviewStage,PreviewPass,PreviewQuality,PreviewSeed,PreviewStamps};
mod effects;
mod refinement;
mod pages;
mod contact;
pub(crate) use pages::PresentedPage;
pub(crate) use sources::SOURCE_WORKING_BYTES;
mod navigator;
pub(crate) use navigator::Navigator;
pub(crate) use sources::Sources;
pub(super) type Pages=Arc<std::sync::Mutex<pages::Store>>;
pub(super) fn pages_within(display:&Arc<crate::residency::Tier>)->Pages {Arc::new(std::sync::Mutex::new(pages::Store::within(display)))}
pub(super) fn trim_pages_by(pages:&Pages,deficit:u64,class:Class) {let mut store=pages.lock().unwrap();let bytes=store.bytes();store.trim(bytes.saturating_sub(deficit),class);}
pub(super) fn release_pages_by(pages:&Pages,deficit:u64) {let mut store=pages.lock().unwrap();let bytes=store.bytes();store.release_undemanded(bytes.saturating_sub(deficit));}

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
    pub fn tile_region(tile: &brush_tiles::BrushTile) -> PixelRect {
        let [x, y] = tile.coordinate.map(|c| c * PAGE_SIZE);
        PixelRect::new(x + tile.local.min_x(), y + tile.local.min_y(), x + tile.local.max_x(), y + tile.local.max_y())
    }
    pub fn is_empty(&self) -> bool { self.regions.is_empty() }
    pub fn bounds(&self) -> PixelRect { self.regions.iter().fold(PixelRect::EMPTY, |r, n| r.union(*n)) }
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
    pub full: OutputPipelines,
    pub half: OutputPipelines,
}
#[derive(Clone)]
pub(crate) struct OutputPipelines {
    inputs: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
    pub reduce_phased: Deferred<wgpu::ComputePipeline>,
    pub reduce_pair: Deferred<wgpu::ComputePipeline>,
    pub compose: Deferred<wgpu::ComputePipeline>,
    pub project: Deferred<wgpu::ComputePipeline>,
}
fn shader_source(space:layer_core::color::RgbSpace,sampling:&str)->std::borrow::Cow<'static,str> {
    compose_wgsl(&[&working_color::source(space),sampling,include_str!("../blend_codes.wgsl"),
        include_str!("../blend_modes.wgsl"),include_str!("scale.wgsl"),include_str!("scale/compose.wgsl")])
}
impl Pipelines {
    pub fn new(device:&PipelineDevice)->Self {
        let records=crate::bindings::layout(device,"display composition records",&[
            crate::bindings::buffer(0,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Uniform,true,NonZeroU64::new(96)),
            crate::bindings::sampler(1,wgpu::ShaderStages::COMPUTE,wgpu::SamplerBindingType::Filtering),
        ]);
        let output=|format| {
            let inputs=crate::bindings::layout(device,"display composition inputs",&[
                crate::bindings::float_texture(0,wgpu::ShaderStages::COMPUTE,device),
                crate::bindings::float_texture(1,wgpu::ShaderStages::COMPUTE,device),
                crate::bindings::storage_texture(2,wgpu::ShaderStages::COMPUTE,format,wgpu::StorageTextureAccess::WriteOnly),
                crate::bindings::float_texture(3,wgpu::ShaderStages::COMPUTE,device),
                crate::bindings::float_texture(4,wgpu::ShaderStages::COMPUTE,device),
            ]);
            let layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label:Some("display resolution composition"),bind_group_layouts:&[Some(&records),Some(&inputs)],immediate_size:0,
            });
            let source=shader_source(device.working_space(),device.float_sampling_wgsl());
            let source=match format {wgpu::TextureFormat::Rgba16Float=>source.replace("texture_storage_2d<rgba32float","texture_storage_2d<rgba16float"),_=>source.into_owned()};
            let shader=Deferred::wgsl(device,"display resolution composition",source);
            let kernel=|entry|Deferred::compute(device,"display resolution composition",&layout,&shader,entry);
            let source=compose_wgsl(&[&working_color::source(device.working_space()),include_str!("scale/project.wgsl")]);
            let source=match format {wgpu::TextureFormat::Rgba16Float=>source.replace("texture_storage_2d<rgba32float","texture_storage_2d<rgba16float"),_=>source.into_owned()};
            let projection=Deferred::wgsl(device,"retained page projection",source);
            let project=Deferred::compute(device,"retained page projection",&layout,&projection,"project");
            OutputPipelines {inputs,reduce:kernel("reduce"),reduce_phased:kernel("reduce_phased"),reduce_pair:kernel("reduce_pair"),compose:kernel("compose"),project}
        };
        let full=output(wgpu::TextureFormat::Rgba32Float);
        let half=output(wgpu::TextureFormat::Rgba16Float);
        Self {records,full,half}
    }
    pub fn output(&self,format:wgpu::TextureFormat)->&OutputPipelines {
        match format {wgpu::TextureFormat::Rgba16Float=>&self.half,wgpu::TextureFormat::Rgba32Float=>&self.full,_=>unreachable!()}
    }
}

pub(super) struct Encoding<'a> {
    pub encoder: &'a mut crate::submission::CommandEncoder,
    pub commands: &'a mut RegionRecords,
    pub retained: u64,
    pub budget: &'a mut WorkBudget,
}
#[derive(Clone)]
struct SourceRequest {
    plan: display_mips::Plan,
    required: Damage,
    covered: PixelRect,
}

pub(crate) struct RegionRecords {
    records: wgpu::Buffer,
    record_binding: wgpu::BindGroup,
    stride: u32,
    cursor: u32,
    charge: crate::residency::Charge,
}
impl RegionRecords {
    #[cfg(test)]
    pub fn new(r: &WgpuRasterizer) -> Self {
        Self::with_capacity(r, 256, &Default::default())
    }
    pub(crate) fn charged(r: &WgpuRasterizer, tier: &Arc<crate::residency::Tier>) -> Self {
        Self::with_capacity(r, 256, tier)
    }
    fn with_capacity(r: &WgpuRasterizer, bytes: u64, tier: &Arc<crate::residency::Tier>) -> Self {
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
        Self { charge: tier.charge(records.size()), records, record_binding, stride, cursor: 0 }
    }
    pub(super) fn record_binding(r: &WgpuRasterizer, records: &wgpu::Buffer) -> wgpu::BindGroup {
        crate::bindings::group(&r.device, "display composition regions", &r.scene_pipelines.scale.records,
            [wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: records, offset: 0, size: NonZeroU64::new(96) }),
                wgpu::BindingResource::Sampler(&r.scene_pipelines.resample.sampler)])
    }
    pub fn begin(&mut self) { self.cursor = 0; }
    #[cfg(test)]
    pub fn storage_bytes(&self) -> u64 { self.records.size() }
    pub(super) fn binding(
        r: &WgpuRasterizer,
        source: &wgpu::TextureView,
        base: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        Self::inputs(r, [source, base, &r.empty_view, &r.empty_view], output)
    }
    pub(super) fn inputs(r: &WgpuRasterizer, sources: [&wgpu::TextureView; 4], output: &wgpu::TextureView) -> wgpu::BindGroup {
        crate::bindings::group(
            &r.device,
            "display composition images",
            &r.scene_pipelines.scale.output(output.texture().format()).inputs,
            [
                wgpu::BindingResource::TextureView(sources[0]),
                wgpu::BindingResource::TextureView(sources[1]),
                wgpu::BindingResource::TextureView(output),
                wgpu::BindingResource::TextureView(sources[2]),
                wgpu::BindingResource::TextureView(sources[3]),
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
        let offset = u64::from(self.cursor) * u64::from(self.stride);
        let needed = offset + bytes.len() as u64;
        if needed > self.records.size() {
            let size = needed.next_power_of_two().min(r.device.limits().max_buffer_size).min(u64::from(u32::MAX));
            if size < needed { return Err(GpuRasterError::ExtentUnsupported); }
            let mut grown = Self::with_capacity(r, size, self.charge.tier());
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
        values: [u32; 20], binding: &wgpu::BindGroup, label: &str, format:wgpu::TextureFormat,
    ) -> Result<(), GpuRasterError> {
        let values = Self::reduction_record(r, values);
        let offset = self.record(r, encoder, values)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label), timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.output(format).reduce);
        pass.set_bind_group(0, &self.record_binding, &[offset]);
        pass.set_bind_group(1, binding, &[]);
        pass.dispatch_workgroups(values[2].div_ceil(8), values[3].div_ceil(8), 1);
        Ok(())
    }

}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Evaluation { Display, Tiled(usize), Native }
impl Evaluation {pub fn fine_capacity(self)->Option<usize> {match self {Self::Tiled(capacity)=>Some(capacity),_=>None}}}
fn fine_source(evaluation:Evaluation,preview:Option<&layer_render::TransformPreview>,id:SourceTarget)->bool {
    evaluation.fine_capacity().is_some() && preview.is_some_and(|preview|preview.target==id)
}

#[derive(Clone, Copy)]
pub(crate) struct Request {
    pub plan: display_mips::Plan,
    pub evaluation: Evaluation,
}

enum Destination<'a> { View, Overview(&'a Cache, PixelRect), Navigator, Preview }

pub(crate) struct Cache {
    pub(super) submission_valid: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub evaluation: Evaluation,
    pub plan: display_mips::Plan,
    precision: crate::display_precision::DisplayPrecision,
    color: layer_core::color::DocumentColor,
    output: Vec<Image>,
    output_charge: crate::residency::Charge,
    used: Vec<scratch::Usage>,
    pixels: pages::Pixels,
    cells: std::sync::Mutex<pages::Cells>,
    above: pages::Pixels,
    above_revision: Option<(graph::Node,u64)>,
    pages: Pages,
    window: pages::WindowId,
    pub geometry: wgpu::Buffer,
    _geometry_charge: crate::residency::Charge,
    layer_count: usize,
    overview: Option<Box<Cache>>,
    preparing: bool,
    declaring: bool,
    exact_tile: Option<Image>,
    unchanged: bool,
    graph: graph::Graph,
    transform: Option<layer_render::TransformPreview>,
    retired: Option<Box<Cache>>,
    contact: bool,
    gesture: Option<contact::Contact>,
    stroked: Option<Arc<SignatureFrame>>,
    presented: pages::Accuracy,
    refused_at: Option<layer_render::ViewState>,
}

fn requires_native_effects(scene:SceneView<'_>,level:u32)->bool {
    scene.order().iter().any(|handle|scene.visible(*handle)
        && scene.effect(*handle).is_some_and(|effect|
            (effect.program.resolution==layer_core::EffectResolution::Native && !native_pointwise_alpha(effect.program))
                || (effect.program.image_boundary() && level==0)))
}

fn fine_request_capacity(r:&WgpuRasterizer,packet:FramePacket<'_>,plan:display_mips::Plan)->Option<usize> {
    #[cfg(test)]
    if r.test.exact_display || r.test.reference {return None;}
    let preview=r.transform_preview.as_ref()?;
    if plan.level==0 || plan.bounds!=PixelRect::full(plan.extent) || packet.scene.target_extent(preview.target)!=plan.extent
        || !matches!(preview.target,SourceTarget::Paint(_)) || preview.companion(packet.scene).is_some()
        || r.layer_masks.definitions.contains_key(&preview.target)
        || !r.transforms.as_ref()?.fine_compatible(preview) {return None;}
    let owner=packet.scene.source_owner(preview.target)?;
    if !packet.scene.visible(owner) || packet.scene.effect(owner).is_some() {return None;}
    let extent=packet.scene.target_extent(preview.target);
    if display_mips::Plan::at(extent,1).size.into_iter().any(|n|n>r.device.limits().max_texture_dimension_2d) {return None;}
    if let Some(capacity)=r.transforms.as_ref()?.fine_capacity(preview) {return (capacity<=48).then_some(capacity);}
    let kept=paint_transform::keeps_pixels(preview.selection.as_ref(),PixelRect::full(extent));
    let available=r.memory.display_grant()
        .saturating_sub(r.composition_storage_bytes()+r.navigator.storage_bytes());
    (1..=page_coordinates(PixelRect::full(extent)).count().clamp(1,48)).rev()
        .find(|capacity|paint_transform::fine_bytes(*capacity,kept)<=available)
}


pub(crate) fn request(r: &WgpuRasterizer, packet: FramePacket<'_>) -> Result<Request, GpuRasterError> {
    display_mips::Plan::new(packet.document_extent)?;
    display_mips::view_bounds(packet.view, packet.document_extent, 0)?;
    #[cfg(test)]
    if r.test.reference {
        return Ok(Request { plan: display_mips::Plan::at(packet.document_extent, 0), evaluation: Evaluation::Native });
    }
    let level = display_mips::view_level(packet.view.document_to_surface, 4).ok_or(GpuRasterError::InvalidExtent)?;
    let coarsest = display_mips::Plan::new(packet.document_extent)?.level;
    let mut result = request_at(r, packet, level);
    for coarser in level + 1..=coarsest.max(level) {
        if !matches!(result, Err(GpuRasterError::ExtentUnsupported)) { break; }
        result = request_at(r, packet, coarser);
    }
    result
}

/// The display request at `level`, or ExtentUnsupported when its native
/// window would exceed the display budget or a texture.
fn request_at(r: &WgpuRasterizer, packet: FramePacket<'_>, level: u32) -> Result<Request, GpuRasterError> {
    let plan = view_plan(packet, level).ok_or(GpuRasterError::InvalidExtent)?;
    let input = input_plan(plan, packet.scene,r.scene.as_ref().map(|s|&s.object_spatial));
    let fits = transform_plans(r, plan, packet.scene).all(|(source, _)| source.size.iter()
        .all(|size| *size <= r.device.limits().max_texture_dimension_2d)) && targets(r, packet).all(|(_, id)| {
        let source = source_plan(input, source_frame(r.scene.as_ref().map(|s| &s.scale_sources), packet.scene, id), packet.scene.target_extent(id), r.moving_layer == packet.scene.source_owner(id),
            r.scene.as_ref().map_or(0, |scene| preview_input_level(&scene.scale_pages, id)));
        (plan.level == 0 && source.level == 0) || source.size.iter().all(|size| *size <= r.device.limits().max_texture_dimension_2d)
    });
    let native_effects=requires_native_effects(packet.scene,level);
    let native = input.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) || !fits || native_effects;
    #[cfg(test)]
    let native = native || r.test.exact_display;
    let evaluation = if native && !native_effects {fine_request_capacity(r,packet,plan).map_or(Evaluation::Native,Evaluation::Tiled)}
        else if native_effects {Evaluation::Native}
        else {Evaluation::Display};
    let plan = view_plan(packet, level).ok_or(GpuRasterError::InvalidExtent)?;
    if plan.size.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) {
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

pub(super) fn source_level(level: u32, frame: SourceFrame, moving: bool, preview: u32) -> u32 {
    level.min(4).saturating_sub(u32::from(!frame.aligned())).saturating_sub(u32::from(moving && frame.aligned())).max(preview)
}
fn preview_input_level(pages: &std::sync::Mutex<pages::Store>, id: SourceTarget) -> u32 {
    pages.lock().unwrap().expected().map_or(0, |frame| frame.preview_level(id))
}

fn native_pointwise_alpha(program: &layer_core::EffectProgram) -> bool {
    program.resolution == layer_core::EffectResolution::Native
        && program.kind == layer_core::EffectKind::Adjustment && program.alpha == layer_core::EffectAlpha::Filter
        && !program.image_boundary() && program.auxiliary.is_none()
}

#[cfg(test)]
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

fn source_plan(output: display_mips::Plan, frame: SourceFrame, extent: [u32; 2], moving: bool, preview: u32) -> display_mips::Plan {
    let level = source_level(output.level, frame, moving, preview);
    let bounds = frame.local(output.doc_bounds, extent);
    let extent = frame.extent(extent);
    let bounds = if frame.aligned() && !moving { bounds } else { bounds.expand(1 << level, extent) };
    let coarse = source_coarse_level(display_mips::Plan::at(extent, level));
    let bounds = if bounds.is_empty() { PixelRect::EMPTY } else {
        paint_transform::aligned(bounds.expand(2 << coarse, extent), PAGE_SIZE.max(1 << coarse), extent)
    };
    display_mips::Plan::window(extent, level, bounds)
}

fn view_plan(packet: FramePacket<'_>, level: u32) -> Option<display_mips::Plan> {
    let extent = packet.document_extent;
    let visible = display_mips::view_bounds(packet.view, extent, 2 << level).ok()?;
    if visible.is_empty() { return display_mips::Plan::new(extent).ok(); }
    let bounds = paint_transform::aligned(visible.expand(PAGE_SIZE<<level, extent), PAGE_SIZE<<level, extent);
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

fn unit_plan(input: display_mips::Plan, scene: SceneView<'_>, region: PixelRect, tiled: bool, objects: Option<&object_spatial::SpatialIndex>) -> display_mips::Plan {
    if tiled || bounded(scene) { return input; }
    let cells = DocRect::from(region).aligned(PAGE_SIZE << input.level);
    let window = images::dependency_window(scene, cells, input.level, objects).aligned(1 << input.level);
    let margin: [i64; 2] = std::array::from_fn(|axis| (cells.min[axis] - window.min[axis]).max(window.max[axis] - cells.max[axis]));
    let bounds = DocRect { min: std::array::from_fn(|axis| cells.min[axis] - margin[axis]), max: std::array::from_fn(|axis| cells.max[axis] + margin[axis]) };
    let Ok(size) = bounds.size() else { return input; };
    let mut result = display_mips::Plan::window(input.extent, input.level, PixelRect::full(size));
    result.doc_bounds = bounds;
    result
}

fn overview_plan(plan: display_mips::Plan) -> display_mips::Plan {
    display_mips::Plan::at(plan.extent, display_mips::Plan::new(plan.extent).unwrap().level.max(plan.level + 1))
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

fn use_tiles(_r:&WgpuRasterizer,_plan:display_mips::Plan,packet:FramePacket<'_>,_scene:Option<&Scene>)->bool {
    bounded(packet.scene)
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

fn work_view(packet:FramePacket<'_>,level:u32)->layer_render::ViewState {
    let mut view=packet.view;
    if let Some(batch)=packet.dab_batches.iter().rev().find(|batch|batch.dab_count!=0) {
        let offset=layer_core::offsets::point(packet.scene.target_offset(batch.target));
        let center=[(batch.damage.min.x+batch.damage.max.x)*0.5+offset.x,(batch.damage.min.y+batch.damage.max.y)*0.5+offset.y];
        let [a,b,c,d,_,_]=view.document_to_surface;
        view.document_to_surface[4]=view.width_px as f32*0.5-a*center[0]-c*center[1];
        view.document_to_surface[5]=view.height_px as f32*0.5-b*center[0]-d*center[1];
    }
    for value in &mut view.document_to_surface[..4] {*value*=(1<<level) as f32;}
    view
}

fn nearest_pages(bounds:PixelRect,view:layer_render::ViewState,limit:usize,held:impl IntoIterator<Item=[u32;2]>,needed:impl Fn(&[u32;2])->bool)->Vec<[u32;2]> {
    if limit==0 || bounds.is_empty() {return Vec::new();}
    let [a,b,c,d,tx,ty]=view.document_to_surface.map(f64::from);
    let det=a*d-b*c;
    let surface=[f64::from(view.width_px)*0.5-tx,f64::from(view.height_px)*0.5-ty];
    let center=[(d*surface[0]-c*surface[1])/det,(a*surface[1]-b*surface[0])/det];
    let held=held.into_iter().collect::<BTreeSet<_>>();
    let distance=|coordinate:[u32;2]| {
        let rect=page_rect(coordinate);
        let dx=f64::from(rect.min_x()+rect.max_x())*0.5-center[0];
        let dy=f64::from(rect.min_y()+rect.max_y())*0.5-center[1];
        ((dx*dx+dy*dy).to_bits(),!held.contains(&coordinate),coordinate)
    };
    let mut result=Vec::new();
    let first=bounds.min_x()/PAGE_SIZE;let end=bounds.max_x().div_ceil(PAGE_SIZE);
    let column=((center[0]/f64::from(PAGE_SIZE)).ceil() as u32).saturating_sub(1).clamp(first,end-1);
    let mut nearest=std::collections::BinaryHeap::new();
    for row in bounds.min_y()/PAGE_SIZE..bounds.max_y().div_ceil(PAGE_SIZE) {nearest.push(std::cmp::Reverse(distance([column,row])));}
    while let Some(std::cmp::Reverse((_,_,coordinate)))=nearest.pop() {
        if needed(&coordinate) {result.push(coordinate);if result.len()==limit {break;}}
        let [x,y]=coordinate;
        if x<=column && x>first {nearest.push(std::cmp::Reverse(distance([x-1,y])));}
        if x>=column && x+1<end {nearest.push(std::cmp::Reverse(distance([x+1,y])));}
    }
    result
}

fn source_coarse_level(plan: display_mips::Plan) -> u32 {
    display_mips::Plan::new(plan.extent).map_or(8, |p| p.level).max(plan.level + 2)
}

fn coarse_level(plan: display_mips::Plan) -> u32 {
    plan.level + u32::from(plan.size.iter().any(|size| *size > display_mips::MAX_SIDE))
}


impl Cache {
    pub fn select(
        previous: Option<Self>,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        request: Request,
        unchanged: bool,
    ) -> (Self, bool) {
        let Request {plan,evaluation}=request;
        let unchanged=unchanged && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty();
        let required=display_mips::view_bounds(packet.view,plan.extent,2<<plan.level).expect("admitted display view");
        let Some(mut old)=previous else {
            let cache=Self::new(r,request,packet.scene.order().len());
            return (cache,true);
        };
        if old.plan!=plan && old.overview.as_ref().is_some_and(|overview|overview.plan==plan) {
            return Self::select(old.overview.take().map(|overview|*overview),r,packet,request,unchanged);
        }
        let unchanged=unchanged && old.transform==r.transform_preview;
        if old.plan.extent==plan.extent && old.plan.level==plan.level && old.plan.bounds.intersect(required)==required
            && old.layer_count==packet.scene.order().len() {
            old.evaluation=evaluation;old.transform=r.transform_preview.clone();old.unchanged=unchanged;
            if let Some(overview)=&mut old.overview {
                overview.unchanged=unchanged;
            }
            return (old,false);
        }
        let mut next=Self::new(r,request,packet.scene.order().len());
        next.pages=old.pages.clone();next.window=old.window;next.graph=old.graph.without_pixels();next.unchanged=unchanged;
        let mut old=Some(Box::new(old));
        if let Some(overview)=&mut next.overview {
            overview.pages=next.pages.clone();overview.window=next.window;overview.unchanged=unchanged;
            let previous=if old.as_ref().is_some_and(|old|old.plan==overview.plan) {old.take()}
                else {old.as_mut().and_then(|old|old.overview.take()).filter(|cache|cache.plan==overview.plan)};
            if let Some(mut previous)=previous {
                previous.exit_contact();
                previous.pages=next.pages.clone();previous.window=next.window;previous.evaluation=Evaluation::Display;previous.unchanged=unchanged;
                next.overview=Some(previous);
            }
        }
        if let Some(old)=&old {old.exit_contact();}
        next.retired=old.filter(|old|old.has_direct_cells());
        (next,true)
    }

    pub fn new(r: &WgpuRasterizer, request: Request, layers: usize) -> Self {
        let Request { plan, evaluation } = request;
        let pages=r.scene.as_ref().map(|scene|scene.scale_pages.clone()).unwrap_or_default();
        let overview_plan=overview_plan(plan);
        let mut overview = (plan.bounds != PixelRect::full(plan.extent))
            .then(|| Box::new(Self::new(r, Request { plan: overview_plan, evaluation:Evaluation::Display }, layers)));
        let window=pages::WindowId::new();
        if let Some(overview)=&mut overview {overview.pages=pages.clone();overview.window=window;}
        let geometry = Self::geometry(r, plan, overview.as_ref().map_or_else(|| coarse_level(plan), |c| coarse_level(c.plan)));
        Self {
            submission_valid: None,
            evaluation, plan, precision:crate::display_precision::DisplayPrecision::Full,color:r.document_color(),
            output: Vec::new(), output_charge:r.scene_tiers.gpu.charge(0),
            used: Vec::new(),
            pixels: Default::default(), cells:Default::default(), above:Default::default(),above_revision:None,pages,window,
            _geometry_charge:r.scene_tiers.windows.charge(geometry.size()),
            geometry,
            layer_count: layers,
            overview, preparing: false, declaring: false, exact_tile: None,
            unchanged: false, graph: Default::default(), transform: r.transform_preview.clone(), retired: None, contact: false, gesture: None, stroked: None, presented: pages::Accuracy::Display, refused_at: None,
        }
    }
    fn geometry(r: &WgpuRasterizer, plan: display_mips::Plan, coarse: u32) -> wgpu::Buffer {
        let level = plan.level;
        let pages=pages::coordinates(plan.bounds,plan.level).count();
        let mut geometry = vec![0u32;8+pages.div_ceil(32).next_multiple_of(4).max(4)];
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
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            })
    }
    pub fn native_preview_input(&self,packet:FramePacket<'_>,id:SourceTarget)->bool {
        self.graph.root.as_ref().is_some_and(|root|root.source_domains(packet.scene)[1].contains(&id))
    }
    pub fn preview_level(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, id: SourceTarget, contribution:bool) -> u32 {
        if self.evaluation == Evaluation::Native || self.plan.level == 0 { return 0; }
        if !contribution && self.native_preview_input(packet,id) { return 0; }
        let frame = source_frame(r.scene.as_ref().map(|scene| &scene.scale_sources), packet.scene, id);
        if contribution && frame.phase != [0; 2] { return 0; }
        source_level(self.plan.level, frame, false, 0).min(crate::preview_block(packet.view.document_to_surface).ilog2())
    }
    fn fine_output_budget(&self,r:&WgpuRasterizer,scene:&Scene,retained:u64)->u64 {
        r.memory.display_grant().saturating_sub(self.frame_memory(r,retained)[0]
            +scene.working_bytes())
    }

    pub fn prepare_graph(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, scene: &Scene) -> Result<(), GpuRasterError> {
        if !Arc::ptr_eq(&self.pages,&scene.scale_pages) {
            self.pages=scene.scale_pages.clone();
            if let Some(overview)=&mut self.overview {overview.pages=self.pages.clone();}
        }
        let sources=&scene.scale_sources;
        let held=self.contact_holds(r,packet);
        if let Some(overview)=&mut self.overview {overview.prepare_root(r,packet,sources,held)?;}
        self.prepare_root(r,packet,sources,held)?;
        Ok(())
    }

    pub(super) fn retain_shared_work(&self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,navigator:&Navigator) {
        let mut windows=Vec::new();
        self.append_wanted_work(r,packet,scene,&mut windows);
        for cache in std::iter::once(self).chain(self.overview.as_deref()).filter(|_|!refinement::exact_at_view_level(packet.scene)) {
            for level in 0..=cache.plan.level {windows.push(cache.refinement_interest(self.evaluation,level,cache.plan.bounds));}
        }
        navigator.append_wanted_work(r,packet,scene,&mut windows);
        self.pages.lock().unwrap().retain_work(&windows);
    }

    fn refinement_interest(&self,evaluation:Evaluation,level:u32,region:PixelRect)->pages::Interest {
        let span=PAGE_SIZE<<level;
        let key=pages::Key {level,coordinate:[region.min_x()/span,region.min_y()/span],format:wgpu::TextureFormat::Rgba32Float,
            ..self.page_key(self.graph.root.clone().unwrap(),region)};
        Self::exact_interest(key,evaluation,region)
    }
    fn exact_interest(key:pages::Key,evaluation:Evaluation,region:PixelRect)->pages::Interest {pages::Interest::new(key,evaluation,region,0)}
    fn parent_interest(&self,region:PixelRect)->pages::Interest {
        pages::Interest::new(self.page_key(self.graph.root.clone().unwrap(),region),self.evaluation,region,self.required_input_level())
    }

    fn append_wanted_work(&self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,windows:&mut Vec<pages::Interest>) {
        let sources=&scene.scale_sources;
        windows.extend(std::iter::once(self).chain(self.overview.as_deref())
            .map(|cache|cache.parent_interest(cache.plan.bounds)));
        for cache in std::iter::once(self).chain(self.overview.as_deref()) {
            let reduced=cache.graph.root.as_ref().unwrap().source_domains(packet.scene)[0].clone();
            let input=input_plan(cache.plan,packet.scene,Some(cache.graph.objects()));
            for (_,id) in targets(r,packet).filter(|(_,id)|reduced.contains(id) && sources.entries.contains_key(id)
                && !fine_source(cache.evaluation,r.transform_preview.as_ref(),*id) && !r.transforms.as_ref().is_some_and(|t|t.display_source(*id))) {
                let plan=source_plan(input,source_frame(Some(sources),packet.scene,id),packet.scene.target_extent(id),r.moving_layer==packet.scene.source_owner(id),preview_input_level(&scene.scale_pages,id));
                if !plan.bounds.is_empty() {windows.push(sources.entries[&id].interest(r,id,plan,plan.bounds,cache.evaluation));}
            }
        }
    }

    fn prepare_root(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, held: bool) -> Result<(), GpuRasterError> {
        self.color=r.document_color();
        self.precision=r.display_precision(packet.scene);
        let plan = if self.evaluation == Evaluation::Native { display_mips::Plan::window(self.plan.extent, 0, self.plan.bounds) }
            else { input_plan(self.plan, packet.scene,Some(self.graph.objects())) };
        self.graph.prepare(r, packet, sources, plan, held)?;
        Ok(())
    }

    pub(super) fn held(&self,r:&WgpuRasterizer)->bool {r.vector_motion_split.is_some() && self.pixels.committed_root().is_some()}
    pub(crate) fn presentable(&self) -> Option<&Self> {
        if self.pixels.committed_root().is_some() {Some(self)}
        else {self.overview.as_deref().and_then(Self::presentable)}
    }
    pub fn presentation(&self)->Option<PresentedPage<'_>> {
        let root=self.pixels.committed_root()?;
        let next=self.pixels.committed_next().unwrap_or(root);
        Some(PresentedPage {views:[root,self.coarse_projection()?,next],geometry:&self.geometry})
    }
    #[cfg(test)]
    pub fn view(&self) -> &wgpu::TextureView {&self.pixels.root().expect("composition target").view}
    #[cfg(test)]
    pub fn texture(&self) -> &wgpu::Texture {&self.pixels.root().expect("composition target").texture}
    fn coarse_projection(&self)->Option<&pages::CommittedImage> {
        if let Some(image)=self.overview.as_deref().and_then(|overview|overview.coarse_projection()) {return Some(image);}
        self.pixels.committed_next().filter(|_|coarse_level(self.plan)>self.plan.level).or_else(||self.pixels.committed_root())
    }
    fn write_projection_scales(&self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let next=self.pixels.committed_next().map_or(self.plan.level,|image|image.image().plan.level);
        let levels=[1<<self.plan.level,1<<self.coarse_projection().map_or(self.plan.level,|image|image.image().plan.level),1<<next];
        let bytes=levels.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        r.uploads.write_at(encoder,&self.geometry,0,&bytes)
    }
    pub(crate) fn pinned_page_bytes(&self) -> u64 { self.pages.lock().unwrap().pinned_bytes() }
    pub(crate) fn charged_scratch_bytes(&self)->u64 {self.output_charge.bytes()+self.overview.as_ref().map_or(0,|cache|cache.charged_scratch_bytes())}
    fn window_memory(&self)->u64 { self.window_bytes().saturating_sub(self.charged_scratch_bytes()) }
    fn owed_window_bytes(&self)->u64 {
        if self.pixels.root().is_none() || self.pixels.next().is_some() {return 0;}
        let plan=display_mips::Plan::window(self.plan.extent,self.plan.level+1,self.plan.bounds);
        plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(self.precision.format().block_copy_size(None).unwrap())
    }
    fn claim_page(&self,scene:&Scene,r:&WgpuRasterizer,key:pages::Key,class:Class)->Result<Result<pages::Held,crate::residency::Blocked>,GpuRasterError> {
        if let Some(image)=self.pages.lock().unwrap().allocated(&key,class) {return Ok(Ok(image));}
        let tier=self.pages.lock().unwrap().tier();
        let mut reservation=match r.admit_tier(Some(scene),&tier,class,key.bytes()) {Ok(reservation)=>reservation,Err(blocked)=>return Ok(Err(blocked))};
        let image=self.pages.lock().unwrap().claim_reserved(r,key.clone(),self.window,class,&mut reservation)?;
        if key.source.is_none() && self.graph.content.as_ref()!=Some(&key.expression) {self.pages.lock().unwrap().set_optional(&key);}
        Ok(Ok(image))
    }
    fn reserve_window(&self,r:&WgpuRasterizer,scene:&Scene,bytes:u64)->Option<crate::residency::Reservation> {
        r.admit_tier(Some(scene),&r.scene_tiers.windows,Class::Visible,bytes).ok()
    }
    pub(crate) fn frame_memory(&self,r:&WgpuRasterizer,retained:u64)->[u64;2] {
        let other=r.scale_display.as_ref().map_or(0,Cache::window_memory);
        let pages=self.pages.lock().unwrap();
        [self.window_memory()+retained+other+pages.bytes(),r.scene_tiers.windows.used()+pages.pinned_bytes()]
    }
    /// The layers above moving paths over `region`, which the presenter draws
    /// over the moving image and the composite without those paths; no view
    /// when nothing lies above them.
    pub(crate) fn motion_above(&self, region: PixelRect) -> Option<(Option<&wgpu::TextureView>, display_mips::Plan)> {
        let [above, _] = self.graph.motion.as_ref()?;
        if graph::Expression::is_clear(above) { return Some((None, self.plan)); }
        if !self.branch_ready(above,region,Class::Visible) {return None;}
        let image=self.above.committed_root()?.image();
        Some((Some(&image.view),image.plan))
    }
    fn branch_ready(&self,node:&graph::Node,region:PixelRect,class:Class)->bool {
        let region=region.intersect(self.plan.bounds);let required_input_level=self.required_input_level();
        !region.is_empty() && pages::coordinates(region,self.plan.level).all(|coordinate| {
            let key=self.page_key(node.clone(),pages::bounds(coordinate,self.plan.level,self.plan.extent));
            let area=region.intersect(key.plan().bounds);
            let mut store=self.pages.lock().unwrap();
            store.get(&key,area,pages::Accuracy::Display,required_input_level,class).or_else(||store.get(&key,area,pages::Accuracy::Exact,0,class)).is_some()
        })
    }
    fn prepared_regions(&mut self, scene: &Scene, packet: FramePacket<'_>, destination: &Destination<'_>, budget: &WorkBudget) -> Vec<PixelRect> {
        if matches!(destination, Destination::View|Destination::Overview(..)) { self.preparing = false; }
        let (Destination::View|Destination::Overview(..), Some(halves), Some((_, _, bounds))) = (destination, &self.graph.prepared, &scene.scale_sources.prepared) else { return Vec::new() };
        let region = bounds.turning_reach().expand(2).in_frame(packet.document_extent).intersect(self.plan.bounds);
        let side=1<<self.plan.level;
        let bounds=PixelRect::new(region.min_x()/side,region.min_y()/side,region.max_x().div_ceil(side),region.max_y().div_ceil(side));
        let view=work_view(packet,self.plan.level);
        let missing=|coordinate:&[u32;2]|halves.iter().any(|half|self.graph.caches(half)
            && !self.branch_ready(half,pages::bounds(*coordinate,self.plan.level,self.plan.extent),Class::Prefetch));
        let limit=((budget.remaining()/object_cache::PASS_TAPS).max(1) as usize).min(APPROXIMATE_PAGES);
        let pages=nearest_pages(bounds,view,limit+1,std::iter::empty(),missing);
        self.preparing=pages.len()>limit;
        pages.into_iter().take(limit).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent)).collect()
    }

    pub fn storage_bytes(&self) -> u64 { self.window_bytes()+self.pages.lock().unwrap().bytes() }
    fn window_bytes(&self) -> u64 {
        #[cfg(test)]
        crate::test_support::PassCounts {display_walks:1,..Default::default()}.record();
        self.output
            .iter()
            .map(|i| texture_bytes(&i.texture))
            .sum::<u64>()
            + self.geometry.size()
            + self.pixels.storage_bytes() + self.above.storage_bytes()
            + self.overview.as_ref().map_or(0, |cache| cache.window_bytes())
            + self.graph.storage_bytes()
            + self.exact_tile.as_ref().map_or(0, |image| texture_bytes(&image.texture))
    }
    pub(crate) fn scratch_images(&self) -> usize { self.graph.root.as_ref().map_or(0, |root| root.cost().1 as usize + 1) }
    pub(crate) fn trim_scratch(&mut self) -> Vec<Image> {
        let keep = self.scratch_images();
        let mut retired = Vec::new();
        if self.graph.motion.as_ref().is_none_or(|[above,_]|graph::Expression::is_clear(above)) {
            retired.extend(self.above.take());self.above_revision=None;
        }
        while self.output.len() > keep && self.used.last().is_some_and(|usage| !usage.occupied()) {
            let image=self.output.pop().unwrap();
            self.output_charge.resize(self.output_charge.bytes()-image.bytes());
            retired.push(image);self.used.pop();
        }
        for cache in self.overview.iter_mut() {
            retired.extend(cache.trim_scratch());
        }
        retired
    }
    fn allocate(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan) -> Result<scratch::Lease, GpuRasterError> {
        self.allocate_output(r,plan,None)
    }
    fn allocate_reserved(&mut self,r:&WgpuRasterizer,plan:display_mips::Plan,reservation:&mut crate::residency::Reservation)->Result<scratch::Lease,GpuRasterError> {
        self.allocate_output(r,plan,Some(reservation))
    }
    fn allocate_output(&mut self,r:&WgpuRasterizer,plan:display_mips::Plan,reservation:Option<&mut crate::residency::Reservation>)->Result<scratch::Lease,GpuRasterError> {
        let slot = self.used.iter().enumerate().position(|(index, used)| !used.occupied() && self.output[index].plan.size == plan.size).unwrap_or(self.output.len());
        if slot == self.output.len() {
            let bytes=plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(r.device.working_format().block_copy_size(None).unwrap());
            let mut charge=match reservation {Some(reservation)=>reservation.split(bytes).ok_or(GpuRasterError::Invariant("display scratch exceeds its accepted reservation"))?.charge(bytes),
                None if self.declaring=>self.output_charge.tier().charge(bytes),
                None=>r.admit_tier(None,self.output_charge.tier(),Class::Visible,bytes).map_err(|_|GpuRasterError::NotReady)?.charge(bytes)};
            let image=if self.declaring { Image { texture: r.declared_scratch[0].texture.clone(), view: r.declared_scratch[0].view.clone(), plan } }
                else { Image::new(r, plan, "display composition level") };
            charge.resize(image.bytes());
            self.output_charge.merge(charge);
            self.output.push(image);
            self.used.push(Default::default());
        }
        self.output[slot].plan = plan;
        self.used[slot].reserve();
        Ok(self.used[slot].lease(slot))
    }
    pub(super) fn render_preview(&mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<bool, GpuRasterError> {
        if self.overview.is_some() {return Ok(false);}
        self.render_coarse_preview(scene,r,packet,dirty,encoding,tiles)
    }
    fn render_coarse_preview(&mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<bool, GpuRasterError> {
        if self.coverage_complete() { return Ok(false); }
        let result = self.render_graph(scene, r, packet, dirty, encoding, Destination::Preview, tiles);
        match result {
            Err(GpuRasterError::NotReady) if !scene.pending_pipelines.ready()=>Err(GpuRasterError::NotReady),
            Ok(_) | Err(GpuRasterError::NotReady)=>Ok(self.coverage_complete()),
            Err(error)=>Err(error),
        }
    }
    pub(super) fn render(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let mut overview=self.overview.take();
        let visible=display_mips::view_bounds(packet.view,self.plan.extent,0).unwrap_or(PixelRect::EMPTY);
        let first=overview.is_some() && self.holes(visible);
        let retained=encoding.retained;
        r.telemetry.phase_begin(9,&r.device,&r.queue,encoding.encoder);
        let result=self.render_caches(overview.as_deref_mut(),first,scene,r,packet,dirty,encoding,tiles);
        encoding.retained=retained;
        r.telemetry.phase_end(9,encoding.encoder);
        self.overview=overview;
        result
    }
    #[expect(clippy::too_many_arguments, reason = "The view and its overview share one frame's input, damage and encoding")]
    fn render_caches(
        &mut self, mut overview: Option<&mut Cache>, first: bool, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let retained=encoding.retained;
        let with=|memory:u64|retained+memory;
        let rendered=|scene:&mut Scene,result|match result {Err(GpuRasterError::NotReady)=>{scene.clear_unencoded_jobs();Ok(PixelRect::EMPTY)},result=>result};
        let mut changed=PixelRect::EMPTY;
        if first && let Some(overview)=overview.as_deref_mut() {
            encoding.retained=with(self.window_memory());
            let result=overview.render_graph(scene,r,packet,dirty,encoding,Destination::Overview(self,PixelRect::EMPTY),tiles);
            changed=rendered(scene,result)?;
        }
        encoding.retained=with(overview.as_deref().map_or(0,Cache::window_memory));
        let result=self.render_graph(scene,r,packet,dirty,encoding,Destination::View,tiles);
        let view=rendered(scene,result)?;
        changed=changed.union(view);
        if !first && let Some(overview)=overview.filter(|overview|!overview.held(r)) {
            encoding.retained=with(self.window_memory());
            let result=overview.render_graph(scene,r,packet,dirty,encoding,Destination::Overview(self,view),tiles);
            changed=changed.union(rendered(scene,result)?);
        }
        Ok(changed)
    }

    #[expect(clippy::too_many_arguments, reason = "Graph rendering keeps frame input, damage, encoding, and destination domains explicit")]
    fn render_graph(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        _dirty: PixelRect, encoding: &mut Encoding<'_>, destination: Destination<'_>, _tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let Encoding { encoder, commands, retained, budget } = encoding;
        scene.placement_display=true;
        let finer=match destination {Destination::Overview(finer,changed)=>Some((finer,changed)),_=>None};
        if self.refused_at.replace(packet.view)!=Some(packet.view) {self.forget_refusals();}
        self.derive_cells(finer.map(|(finer,_)|finer));
        if let Some(mut previous)=self.retired.take() {previous.write_back(r,scene,encoder)?;}
        let stroking=!packet.dab_batches.is_empty();
        let view=matches!(destination,Destination::View);
        if self.gesture.is_some() && !self.contact_continues(scene,r,packet,encoder) {
            self.end_held_frame(r,packet)?;self.write_back(r,scene,encoder)?;self.commit_window(r,encoder)?;
        }
        let live=self.gesture.is_some();
        if std::mem::replace(&mut self.contact,stroking) && !stroking && !live {self.write_back(r,scene,encoder)?;}
        if self.pixels.root().is_some_and(|image|image.texture.format()!=self.precision.format()) {self.end_held_frame(r,packet)?;}
        if !self.ensure_window(r,scene,encoder) {return Err(GpuRasterError::NotReady);}
        let live=self.gesture.is_some();
        let visible=if matches!(destination,Destination::View) {display_mips::view_bounds(packet.view,self.plan.extent,0).unwrap_or(PixelRect::EMPTY)}else {PixelRect::EMPTY};
        let deferred=match destination {Destination::View=>Class::Prefetch,Destination::Navigator=>Class::Navigator,Destination::Overview(..)|Destination::Preview=>Class::Fallback};
        let served=if matches!(destination,Destination::View) {visible}else {self.plan.bounds};
        let presented=refinement::presented_accuracy(r,packet.scene,matches!(destination,Destination::Preview));
        if std::mem::replace(&mut self.presented,presented)==pages::Accuracy::Display && presented==pages::Accuracy::Exact {self.owe_exact();}
        let selected=|region:PixelRect|if region.intersect(visible).is_empty() {deferred}else {Class::Visible};
        let selection=if live {pages::Selection {copies:Vec::new(),composes:self.contact_cells(),transition:false,waiting:false}}
            else {self.select_cells(served,presented,&selected)?};
        let transition=selection.transition;
        let direct=if view {self.stroke_only(packet)}else {stroking};
        let target=if transition && !direct {Root::Store}else {Root::Window};
        let pages::Selection {mut copies,mut composes,..}=selection;
        let blocking=|scene:&Scene,r:&WgpuRasterizer|if scene.compiling() {Some(crate::frame_demand::Input::Compile)}
            else {r.image_decode_waiting().then_some(crate::frame_demand::Input::Decode)};
        let unready=|scene:&Scene,r:&WgpuRasterizer|blocking(scene,r).unwrap_or(crate::frame_demand::Input::Residency);
        let mut blocks=Vec::new();let mut refusals=Vec::new();
        let class=|region:PixelRect|if region.intersect(visible).is_empty() {deferred}else if transition {Class::Authored}else {Class::Visible};
        let side=1<<self.plan.level;
        let output_bounds=self.output_plan().bounds;
        let mut written=PixelRect::EMPTY;
        let mut composed=Vec::new();
        if live {
            let (done,blocked)=self.compose_contact(scene,r,packet,encoder,std::mem::take(&mut composes),*retained)?;
            #[cfg(test)] crate::test_support::PassCounts {destination_units:(done.len()+blocked.len()) as u64,..Default::default()}.record();
            for region in &done {
                let [x, y, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
                self.pixels.wrote(PixelRect::new(x, y, x + width, y + height));
                written = written.union(PixelRect::new(x, y, x + width, y + height));
            }
            if !written.is_empty() || self.pixels.committed_root().is_none() {self.commit_window(r,encoder)?;}
            for region in blocked {self.hide(region);self.block(r,region,Some(unready(scene,r)));}
            composed=done;
        } else if !composes.is_empty() || !copies.is_empty() || self.graph.prepared.is_some() {
            let root = self.graph.root.clone().expect("prepared composition graph");
            let input = input_plan(self.plan, packet.scene,Some(self.graph.objects()));
            let tiled = use_tiles(r, self.plan, packet, Some(scene)) && !(self.plan.level > 0 && root.fused_transform(r));
            let view=work_view(packet,0);
            composes.sort_by_key(|region| {
                let [a,b,c,d,x,y]=view.document_to_surface.map(f64::from);
                let center=[f64::from(region.min_x()+region.max_x())*0.5,f64::from(region.min_y()+region.max_y())*0.5];
                let offset=[a*center[0]+c*center[1]+x-f64::from(view.width_px)*0.5,b*center[0]+d*center[1]+y-f64::from(view.height_px)*0.5];
                (offset[0]*offset[0]+offset[1]*offset[1]) as u64
            });
            let prepared = self.prepared_regions(scene, packet, &destination, budget);
            let halves:Vec<_>=self.graph.prepared.clone().into_iter().flatten().filter(|half|self.graph.caches(half)).collect();
            let preview=matches!(destination,Destination::Preview);
            let mut accepted=Vec::new();
            for region in composes {
                if !budget.fits(class(region),!accepted.is_empty(),object_cache::PASS_TAPS) {break;}
                let requests=self.region_sources(scene,r,packet,region,tiled);
                let mut progressed=false;
                for (id,request) in requests.iter().cloned() {progressed|=!self.prepare_source(scene,r,packet,encoder,id,request,class(region),budget)?.is_empty();}
                if !requests.iter().all(|(id,request)|self.source_ready(scene,r,*id,request)) {
                    #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
                    blocks.push((region,if progressed {blocking(scene,r)}else {Some(unready(scene,r))}));continue;
                }
                let handle=match self.accept_parent(scene,r,packet,region,&requests,*retained,tiled,std::slice::from_ref(&root),target,preview,class(region),if budget.required(class(region)) {class(region)} else {Class::Prefetch},encoder)? {
                    Ok(handle)=>handle,
                    Err(Some(refusal))=>{refusals.push((region,refusal));continue;},
                    Err(None)=>{blocks.push((region,Some(unready(scene,r))));continue;},
                };
                r.image_decode_queue(r.device.working_space())?.prioritize(packet.view,region);
                budget.spend(object_cache::PASS_TAPS);
                let sources=self.pages.lock().unwrap().take_inputs(&handle)?;
                if let Some(refusal)=sources.objects_refusal() {
                    refusals.push((region,refusal.clone()));
                    if self.displays(region) {
                        if class(region).demanded() {self.pages.lock().unwrap().hold_inputs(&handle,sources)?;}else {self.pages.lock().unwrap().finish_inputs(&handle);}
                        continue;
                    }
                }
                let evaluated=if sources.objects_fall_back() {Class::Fallback}else {class(region)};
                #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
                accepted.push((region,handle,sources,evaluated));
            }
            let mut completed = Vec::<ReadyUnit>::new();
            let native = root.native_capture(packet.scene, self.plan.level);
            let mut accepted=accepted.into_iter();
            for (region,handle,mut sources,evaluated) in accepted.by_ref() {
                if native {
                    let jobs = std::mem::take(&mut scene.jobs);
                    let source_jobs = std::mem::take(&mut scene.source_jobs);
                    let output = DocRect::from(region).aligned(PAGE_SIZE.max(side)).intersect(input.doc_bounds).in_frame(packet.document_extent);
                    let window = scene.cached_capture_window(packet.scene, output);
                    let result=scene.prepare_region_in_frame(r,packet,(output,window),PixelRect::EMPTY,false,encoder);
                    scene.discard_unencoded_jobs();
                    scene.jobs = jobs;scene.source_jobs = source_jobs;
                    scene.placement_display = true;
                    if let Err(error) = result {
                        if matches!(error,GpuRasterError::NotReady) {
                            blocks.push((region,Some(unready(scene,r))));
                            if class(region).demanded() {self.pages.lock().unwrap().hold_inputs(&handle,sources)?;}else {self.pages.lock().unwrap().finish_inputs(&handle);}
                            continue;
                        }
                        if matches!(error,GpuRasterError::SourceWorkingSetExceeded) {self.pages.lock().unwrap().finish_inputs(&handle);continue;}
                        scene.discard_unencoded_jobs();return Err(error);
                    }
                }
                let start = scene.jobs.len();
                let mut unit = Unit::default();
                let input = unit_plan(input, packet.scene, region, tiled, Some(self.graph.objects()));
                let result = (|| -> Result<(), GpuRasterError> {
                    #[cfg(test)]
                    let _record=crate::scene::sources::record_reads(&sources);
                    let mut compositor = Evaluator { cache: &mut *self, scene: &mut *scene, packet, r: &mut *r, encoder,
                        region: region.into(), input, tiled, unit: &mut unit, accepted:&mut sources,
                        previous_motion: completed.iter().rev().find_map(|ready| ready.unit.motion.as_ref()),retained:*retained,
                        approximate: false, preview, class: evaluated };
                    compositor.evaluate_root(&root, target)?;
                    if !compositor.unit.ready(compositor.scene, compositor.r, compositor.encoder, start, class(region))? {
                        return Err(GpuRasterError::NotReady);
                    }
                    Ok(())
                })();
                unit.sources.extend(sources)?;unit.sources.check_unit(handle.input_unit())?;
                match result {
                    Ok(()) => {
                        let work=scene.planned_work(start);
                        if !budget.fits(class(region),!completed.is_empty(),work) {
                            self.pages.lock().unwrap().finish_inputs(&handle);
                            scene.discard_jobs_from(start);
                            break;
                        }
                        budget.spend(work);unit.work=work;
                        completed.push(ReadyUnit {unit,handle,publish:true});
                    },
                    Err(GpuRasterError::NotReady) => {
                        blocks.push((region,Some(unready(scene,r))));
                        if class(region).demanded() {self.pages.lock().unwrap().hold_inputs(&handle,std::mem::take(&mut unit.sources))?;}
                        else {self.pages.lock().unwrap().finish_inputs(&handle);}
                        scene.discard_jobs_from(start);
                    }
                    Err(GpuRasterError::SourceWorkingSetExceeded) => {
                        self.pages.lock().unwrap().finish_inputs(&handle);scene.discard_jobs_from(start);
                    }
                    Err(error) => {scene.discard_unencoded_jobs();return Err(error);}
                }
            }
            for (region,handle,sources,_) in accepted {
                if class(region).demanded() {self.pages.lock().unwrap().hold_inputs(&handle,sources)?;}else {self.pages.lock().unwrap().finish_inputs(&handle);}
            }
            if self.graph.prepared.is_some() {
                for region in prepared {
                    if !budget.fits(Class::Prefetch,!completed.is_empty(),object_cache::PASS_TAPS) {self.preparing=true;break;}
                    let requests=self.region_sources(scene,r,packet,region,tiled);
                    for (id,request) in requests.iter().cloned() {self.prepare_source(scene,r,packet,encoder,id,request,Class::Prefetch,budget)?;}
                    if !requests.iter().all(|(id,request)|self.source_ready(scene,r,*id,request)) {self.preparing=true;continue;}
                    let Ok(handle)=self.accept_parent(scene,r,packet,region,&requests,*retained,tiled,&halves,Root::Store,preview,Class::Prefetch,Class::Prefetch,encoder)? else {self.preparing=true;continue;};
                    #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
                    budget.spend(object_cache::PASS_TAPS);
                    let start = scene.jobs.len();
                    let mut unit = Unit::default();
                    let mut sources=self.pages.lock().unwrap().take_inputs(&handle)?;
                    let input = unit_plan(input, packet.scene, region, tiled, Some(self.graph.objects()));
                    let result = (|| {
                        #[cfg(test)]
                        let _record=crate::scene::sources::record_reads(&sources);
                        let mut compositor = Evaluator { cache: self, scene, packet, r, encoder, region: region.into(), input, tiled,
                            unit: &mut unit, accepted:&mut sources,
                            previous_motion: completed.iter().rev().find_map(|ready| ready.unit.motion.as_ref()),retained:*retained,
                            approximate: false, preview, class: Class::Prefetch };
                        for half in &halves { compositor.evaluate_root(half,Root::Store)?; }
                        compositor.unit.ready(compositor.scene, compositor.r, compositor.encoder, start, Class::Prefetch)
                    })();
                    unit.sources.extend(sources)?;unit.sources.check_unit(handle.input_unit())?;
                    match result {
                        Ok(true) => {
                            let work=scene.planned_work(start);
                            if !budget.fits(Class::Prefetch,!completed.is_empty(),work) {scene.discard_jobs_from(start);self.preparing=true;break;}
                            budget.spend(work);unit.work=work;
                            completed.push(ReadyUnit {unit,handle,publish:false});
                        },
                        Ok(false)|Err(GpuRasterError::SourceWorkingSetExceeded) => {self.pages.lock().unwrap().finish_inputs(&handle);scene.discard_jobs_from(start);self.preparing=true;},
                        Err(GpuRasterError::NotReady) if !completed.is_empty() => {
                            self.pages.lock().unwrap().finish_inputs(&handle);scene.discard_jobs_from(start);self.preparing=true;
                        }
                        Err(error) => {scene.discard_unencoded_jobs();return Err(error);}
                    }
                }
            }
            self.encode_units(scene, r, encoder, completed.iter_mut().map(|ready| std::mem::take(&mut ready.unit)))?;
            for ReadyUnit {handle,publish,..} in completed {
                self.pages.lock().unwrap().finish_inputs(&handle);
                if publish {composed.push(handle.region());}
            }
            let held=target==Root::Store && {
                let selection=self.select_cells(served,presented,&selected)?;
                let complete=selection.complete(served);
                if !complete {blocks.extend(selection.copies.iter().map(|(_,copy,_)|(copy.region,Some(unready(scene,r)))));}
                copies=if complete {selection.copies}else {Vec::new()};
                !complete
            };
            let copied=self.copy_cells(r,encoder,commands,copies)?;
            for region in composed.iter().copied().filter(|_|target==Root::Window).chain((!copied.is_empty()).then_some(copied)) {
                let [x, y, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
                self.pixels.wrote(PixelRect::new(x, y, x + width, y + height));
                written = written.union(PixelRect::new(x, y, x + width, y + height));
            }
            if !held && (!written.is_empty() || transition || self.pixels.committed_root().is_none()) {self.commit_window(r,encoder)?;}
            if presented==pages::Accuracy::Exact {blocks.extend(composed.iter().map(|region|(*region,blocking(scene,r))));}
            for (region,input) in blocks {self.block(r,region,input);}
            for (region,refusal) in refusals {self.refuse(region,&refusal);}
        }
        if view && live && self.ends_contact(packet) {self.write_back(r,scene,encoder)?;}
        else if view && !live && stroking {self.begin_contact(scene,r,packet,encoder,*retained)?;}
        self.project_above(scene,r,packet,encoder,commands)?;
        for region in &composed {
            let [_, _, width, height] = paint_transform::texel_rect(region.window_local(output_bounds), side);
            r.metrics.composited_pixels += u64::from(width) * u64::from(height);
            r.metrics.frame_composited_pages.extend(page_coordinates(*region).map(|coordinate| (self.plan.level, coordinate)));
        }
        if scene.object_results.needs_retirement() { scene.object_results.flush_retired(encoder); }
        let bounds = self.plan.bounds;
        let document = |written: PixelRect| if written.is_empty() { written } else { PixelRect::new(
            output_bounds.min_x() + written.min_x() * side, output_bounds.min_y() + written.min_y() * side,
            output_bounds.min_x() + written.max_x() * side, output_bounds.min_y() + written.max_y() * side,
        ).intersect(bounds) };
        if matches!(destination, Destination::View) {
            crate::performance_trace::counter(c"Capy main mip box pixels", written.area());
            crate::performance_trace::counter(c"Capy main mip regions", composed.len() as u64);
        }
        if !matches!(destination, Destination::Navigator) {
            r.telemetry.phase_begin(11, &r.device, &r.queue, encoder);
            if let Some((finer,changed))=finer {
                let [x,y,width,height]=paint_transform::texel_rect(self.derive(scene,r,encoder,commands,finer,changed)?.window_local(output_bounds),side);
                written=written.union(PixelRect::new(x,y,x+width,y+height));
            }
            self.reduce_output(scene,r,encoder,commands)?;
            r.telemetry.phase_end(11, encoder);
        }
        Ok(document(written))
    }
    fn encode_units(&mut self, scene: &mut Scene, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder,
        units: impl IntoIterator<Item = Unit>,
    ) -> Result<(), GpuRasterError> {
        if let Err(error) = scene.encode_jobs_inner(r, encoder, None) {
            scene.discard_unencoded_jobs();return Err(error);
        }
        for mut unit in units {
            std::mem::take(&mut unit.sources).track(encoder);
            unit.commit(self, scene, encoder);
        }
        Ok(())
    }

    fn copy_level(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords)->Result<PixelRect,GpuRasterError> {
        if !self.ensure_window(r,scene,encoder) {return Err(GpuRasterError::NotReady);}
        let selection=self.select_cells(self.plan.bounds,pages::Accuracy::Display,&|_|Class::Visible)?;
        if selection.transition && !selection.complete(self.plan.bounds) {return Ok(PixelRect::EMPTY);}
        let copied=self.copy_cells(r,encoder,commands,selection.copies)?;
        if !copied.is_empty() {self.commit_window(r,encoder)?;}
        self.reduce_output(scene,r,encoder,commands)?;
        Ok(copied)
    }

    fn derive(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords,
        finer:&Cache,changed:PixelRect,
    )->Result<PixelRect,GpuRasterError> {
        let region=self.cells.lock().unwrap().derivations(self.plan,changed);
        let Some(input)=finer.pixels.committed_next().or(finer.pixels.committed_root()).map(pages::CommittedImage::image) else {return Ok(PixelRect::EMPTY);};
        let region=region.intersect(input.plan.bounds);
        let Some(written)=self.reduce_from(scene,r,encoder,commands,input,region)? else {return Ok(PixelRect::EMPTY);};
        self.cells.lock().unwrap().reduced(self.plan,region);
        self.pixels.derived(written,encoder.submission());
        Ok(region)
    }
    fn reduce_from(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords,
        input:&Image,region:PixelRect,
    )->Result<Option<PixelRect>,GpuRasterError> {
        if region.is_empty() {return Ok(None);}
        let format=self.precision.format();
        if self.pixels.root().is_none_or(|image|image.texture.format()!=format) {
            let bytes=self.plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(format.block_copy_size(None).unwrap());
            let Some(reservation)=self.reserve_window(r,scene,bytes) else {return Ok(None);};
            let charge=reservation.charge(bytes);
            let retired=self.pixels.take();
            let bytes=retired.iter().map(Image::bytes).sum();
            encoder.retire(crate::DisplayRetirement {_charge:r.retired_display_bytes.charge(bytes),
                #[cfg(not(target_arch="wasm32"))] _scratch:(Vec::new(),retired)});
            self.pixels.ensure_root(r,self.plan,format,"composition window",charge);
        }
        let output=self.pixels.root().unwrap();
        let [x,y,width,height]=paint_transform::texel_rect(region.window_local(output.plan.bounds),1<<self.plan.level);
        let level=input.plan.level;
        let mut values=[0;20];
        values[..8].copy_from_slice(&[x,y,width,height,input.plan.bounds.width(),input.plan.bounds.height(),1<<(self.plan.level-level),(level<<8)|8]);
        values[14]=((input.plan.bounds.min_x()>>level) as f32).to_bits();
        values[15]=((input.plan.bounds.min_y()>>level) as f32).to_bits();
        let binding=RegionRecords::binding(r,&input.view,&r.empty_view,&output.view);
        commands.reduce(r,encoder,values,&binding,"reduce a finer window",format)?;
        Ok(Some(PixelRect::new(x,y,x+width,y+height)))
    }
    fn reduce_output(&mut self,scene:&Scene, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords) -> Result<(), GpuRasterError> {
        let region=self.pixels.unreduced();
        if region.is_empty() || self.pixels.committed_root().is_none() { return Ok(()); }
        if !self.coverage_complete() {return self.write_projection_scales(r,encoder);}
        if self.pixels.next().is_none() {
            let bytes=self.owed_window_bytes();
            let Some(reservation)=self.reserve_window(r,scene,bytes) else {return self.write_projection_scales(r,encoder);};
            self.pixels.ensure(r,self.plan,self.precision.format(),reservation.charge(bytes));
        }
        let levels = [self.pixels.root().unwrap().clone(), self.pixels.next().unwrap().clone()];
        let mut changed = region;
        let mut jobs = Vec::with_capacity(levels.len() - 1);
        for pair in levels.windows(2) {
            let [input, output] = pair else { unreachable!() };
            let next_origin = [changed.min_x() / 2, changed.min_y() / 2];
            let next_end = [changed.max_x().div_ceil(2), changed.max_y().div_ceil(2)];
            let next_size = [0, 1].map(|i| next_end[i] - next_origin[i]);
            let binding = RegionRecords::binding(r, &input.view, &r.empty_view, &output.view);
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
        pass.set_pipeline(&r.scene_pipelines.scale.output(levels[1].texture.format()).reduce_pair);
        for (binding, offset, size) in &jobs {
            pass.set_bind_group(0, &commands.record_binding, &[*offset]);
            pass.set_bind_group(1, binding, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
        drop(pass);
        self.pixels.commit_next(encoder.submission());
        self.write_projection_scales(r,encoder)?;
        Ok(())
    }
}

#[derive(Clone)]
struct Placed {
    view: wgpu::TextureView,
    slot: Option<Slot>,
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
enum Slot { Root, Cache(scratch::Lease), Scene(scratch::Lease), Scenes([scratch::Lease; 2]), Decoded(super::sources::Lease) }

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
    fn slot(&self) -> Option<Slot> { match self {Self::Image {slot,..}=>slot.clone(),Self::Placed(placed)=>placed.slot.clone(),_=>None} }
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
struct ReadyUnit {
    unit: Unit,
    handle: pages::Accepted,
    publish: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {Window, Store}

#[derive(Default)]
struct Unit {
    window: Vec<pages::Write>,
    waiting: bool,
    updates: Vec<(SourceTarget, usize)>,
    motion: Option<super::vectors::MotionImage>,
    sources: crate::scene::sources::AcceptedInputs,
    pages: Vec<pages::Write>,
    reads: Vec<pages::Held>,
    work: u64,
}
impl Unit {
    fn ready(&mut self, scene: &mut Scene, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, start: usize, class: Class) -> Result<bool, GpuRasterError> {
        let ready=scene.prepare_jobs(r,encoder,start,class)?;
        if self.waiting || !ready {return Ok(false);}
        Ok(true)
    }
    fn commit(self, cache: &mut Cache, scene: &mut Scene, encoder:&mut submission::CommandEncoder) {
        for image in self.reads {image.track(encoder);}
        for write in self.pages {cache.pages.lock().unwrap().commit(write,self.work,encoder);}
        for write in self.window {cache.show(write,encoder.submission());}
        for (id, pages) in self.updates { scene.scale_sources.entries.get_mut(&id).unwrap().updates += pages as u64; }
        if let Some(motion) = self.motion { scene.vectors.replace_motion(motion); }
    }
}
struct Evaluator<'a> {
    cache: &'a mut Cache,
    scene: &'a mut Scene,
    packet: FramePacket<'a>,
    r: &'a mut WgpuRasterizer,
    encoder: &'a mut crate::submission::CommandEncoder,
    region: DocRect,
    input: display_mips::Plan,
    tiled: bool,
    unit: &'a mut Unit,
    accepted:&'a mut crate::scene::sources::AcceptedInputs,
    previous_motion: Option<&'a super::vectors::MotionImage>,
    retained:u64,
    approximate: bool,
    preview: bool,
    class: Class,
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
    fn target(&mut self) -> Result<Target, GpuRasterError> {
        let plan = self.working_plan();
        if self.tiled {
            let slot = self.scene.reserve(self.r)?;
            Ok(Target { view: self.scene.pool[slot].view.clone(), slot: Some(Slot::Scene(self.scene.used[slot].lease(slot))), plan })
        } else {
            let slot = self.cache.allocate(self.r, plan)?;
            Ok(Target { view: self.cache.output[slot.index].view.clone(), slot: Some(Slot::Cache(slot)), plan })
        }
    }
    fn release(&mut self, slot: Option<Slot>) {
        match slot {
            Some(Slot::Cache(lease) | Slot::Scene(lease)) => drop(lease),
            Some(Slot::Scenes(leases)) => drop(leases),
            slot => drop(slot),
        }
    }
    fn texels(&self, plan: display_mips::Plan) -> [u32; 4] {
        paint_transform::texel_rect(plan.doc_bounds.local(self.region), 1 << plan.level)
    }
    fn source(&mut self, id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let value = self.source_pixels(id, frame, extent, outside)?;
        if self.cache.plan.level == 0 || !mapped_material(self.r, self.packet, id)
            || self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) { return Ok(value); }
        let placement=self.accepted.placement(id)?;
        let origin = placement.offset;
        let (bounds, radius) = self.scene.material_coverage(self.r, id, origin, self.packet.dab_batches);
        if bounds.is_empty() { return Ok(value); }
        let material = bounds.expand(radius + 2 * (1 << self.cache.plan.level)).aligned(PAGE_SIZE);
        let region = self.region.aligned(PAGE_SIZE).intersect(material).intersect(self.input.doc_bounds);
        if region.is_empty() { return Ok(value); }
        let value = self.materialize(value, None)?;
        let handle = self.packet.scene.source_owner(id).unwrap();
        let size = region.size()?;
        let local = PixelRect::full(size);
        let pages: Vec<_> = page_coordinates(local).collect();
        let mut plan = display_mips::Plan::window(self.packet.document_extent, self.cache.plan.level, local);
        plan.doc_bounds = region;
        let slot = self.cache.allocate(self.r, plan)?;
        let view = self.cache.output[slot.index].view.clone();
        let packet = FramePacket { scene: self.packet.scene.with_offset64(region.min.map(|n| -(n as f64))), document_extent: size, ..self.packet };
        let placement=placement.material_view(region.min);
        let local_plan = display_mips::Plan::window(size, self.cache.plan.level, local);
        self.scene.reduce_color_pages(self.r, packet,
            handle, local_plan, &view, &pages, Some(&placement), [0; 2], &BTreeMap::new(), self.accepted)?;
        self.unit.updates.push((id, pages.len()));
        let material = Target { view, slot: Some(Slot::Cache(slot)), plan }.value();
        self.draw(material, value, layer_core::LayerBlend::Normal, 128, None)
    }
    fn vector(&mut self, owner: OccurrenceHandle, moving: Option<&super::vectors::Moving>, output: Option<Target>, append: bool) -> Result<Value, GpuRasterError> {
        let mut target = self.target()?;
        let side = 1u32 << target.plan.level;
        let bounds = self.region.intersect(target.plan.doc_bounds).aligned(side);
        let size = bounds.size().map_err(|_| GpuRasterError::ExtentUnsupported)?;
        target.plan.doc_bounds = bounds; target.plan.size = size.map(|v| v.div_ceil(side)); target.plan.bounds = PixelRect::full(size);
        let plan = target.plan;
        let palette = Scene::vector_palette(self.r, self.packet);
        let origin = |input: display_mips::Plan| paint_transform::texel_rect(input.doc_bounds.local(bounds), side);
        let base = output.as_ref().filter(|output| append && output.plan.level == plan.level).map(|output| (output.view.clone(), output.plan));
        let (scene, extent) = (self.packet.scene, self.packet.document_extent);
        if !self.scene.vectors.window_ready(scene,owner,bounds.min,plan.level,plan.size,extent,palette) {
            self.unit.waiting=true;
            return self.materialize(Value::Color([0.;4]),output);
        }
        let excluded = moving.map(|moving| &*moving.0);
        let unmoved=excluded.and_then(|_| {
            let node=Arc::new(graph::Expression::Vector {owner,offset:scene.occurrence_offset64(owner).map(f64::to_bits),moving:None});
            let key=self.cache.page_key(node,bounds.in_frame(extent));
            if key.level!=plan.level {return None;}
            let image=self.cache.pages.lock().unwrap().get(&key,bounds.in_frame(extent),pages::Accuracy::Display,self.cache.required_input_level(),self.class)?;
            let source=(image.view.clone(),image.plan);self.unit.reads.push(image);
            Some(source)
        });
        let appended = base.as_ref().and_then(|_| self.scene.vectors.append_batch(scene, owner, excluded, bounds.min, plan.level, plan.size, extent, palette));
        let (batch, base, kept) = match appended {
            Some(batch) => (batch, base.clone(), base),
            None => match base.as_ref().and_then(|_| self.scene.vectors.repaint_batch(scene, owner, excluded, None, bounds.min, plan.level, plan.size, extent, palette)) {
                Some(batch) => (batch, None, base),
                None => match unmoved.as_ref().and_then(|_| self.scene.vectors.repaint_batch(scene, owner, excluded, excluded, bounds.min, plan.level, plan.size, extent, palette)) {
                    Some(batch) => (batch, None, unmoved),
                    None => (self.scene.vectors.batch(scene, owner, excluded, bounds.min, plan.level, plan.size, extent, palette), None, None),
                },
            },
        };
        match kept {
            Some((source, input)) if source.texture().format() == target.view.texture().format() => {
                let [x, y, ..] = origin(input);
                self.scene.jobs.push(Job::Copy { source: source.texture().clone(), source_origin: [x, y],
                    destination: target.view.texture().clone(), origin: [0;2], width: plan.size[0], height: plan.size[1] });
            },
            Some((view, plan)) => { self.materialize(Value::Image { view, slot: None, opacity: 1., plan, preview: None, encode: false }, Some(target.clone()))?; },
            None => self.scene.jobs.push(Job::Clear(target.view.clone(), wgpu::Color::TRANSPARENT)),
        }
        if batch.tile_count() > 0 {
            let base = base.map(|(view, input)| { let [x, y, ..] = origin(input); (view, [x as i32, y as i32]) });
            self.scene.jobs.push(super::Job::Vector(Box::new(super::vectors::VectorJob { batch, output: target.view.clone(), base })));
        }
        self.materialize(Value::Image { view: target.view, slot: target.slot, opacity: 1., plan, preview: None, encode: false }, output)
    }
    fn vector_motion(&mut self, owner: OccurrenceHandle, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let Some((motion, moving)) = self.scene.scale_sources.vector_motion.clone().filter(|(m, _)| m.owner == owner) else { return self.materialize(Value::Color([0.; 4]), output) };
        let working = self.working_plan();
        let level = working.level;
        let side = 1u32 << level;
        let Some(inverse) = super::vectors::level_transform(&motion, level).inverse() else { return self.materialize(Value::Color([0.; 4]), output) };
        let moving = moving.aligned(side);
        let window = self.region.intersect(working.doc_bounds).aligned(side);
        let needed = super::moved_bounds(inverse, window).expand(side).intersect(moving).in_frame(self.packet.document_extent);
        if needed.is_empty() { return self.materialize(Value::Color([0.; 4]), output); }
        let needed = DocRect::from(needed).aligned(side);
        let paths = self.r.vector_motion_paths_drawn.clone();
        let key = (owner, super::vectors::Moving(motion.items.clone()), level, self.packet.scene.occurrence_offset64(owner).map(|v| v as i64), super::vectors::Replaced(paths.clone()));
        let source = self.unit.motion.iter().chain(self.previous_motion).chain(self.scene.vectors.motion.iter())
            .find(|image| image.key == key && image.window.intersect(needed) == needed)
            .map(|image| (image.image.view.clone(), image.window.min));
        let (source, origin) = if let Some(source) = source { source } else {
            let budget = DocRect { min: needed.min.map(|v| v - 512 * i64::from(side)), max: needed.max.map(|v| v + 512 * i64::from(side)) };
            let area = moving.intersect(budget).in_frame(self.packet.document_extent);
            let plan = display_mips::Plan::window(self.packet.document_extent, level, area);
            let image = self.scene.vectors.motion_image(self.r, plan);
            let palette = Scene::vector_palette(self.r, self.packet);
            let batch = match &paths {
                Some(paths) => self.scene.vectors.replacement_batch(paths, self.packet.scene.occurrence_offset64(owner), plan.doc_bounds.min, level, plan.size, self.packet.document_extent, palette),
                None => self.scene.vectors.motion_batch(self.packet.scene, owner, &motion.items, plan.doc_bounds.min, level, plan.size, self.packet.document_extent, palette),
            };
            self.scene.jobs.push(Job::Clear(image.view.clone(), wgpu::Color::TRANSPARENT));
            if batch.tile_count() > 0 {
                self.scene.jobs.push(super::Job::Vector(Box::new(super::vectors::VectorJob { batch, output: image.view.clone(), base: None })));
            }
            let source = (image.view.clone(), plan.doc_bounds.min);
            self.unit.motion = Some(super::vectors::MotionImage { key, window: plan.doc_bounds, image, revision: 0 });
            source
        };
        let mut target = self.target()?;
        let size = window.size().map_err(|_| GpuRasterError::ExtentUnsupported)?;
        target.plan.doc_bounds = window; target.plan.size = size.map(|v| v.div_ceil(side)); target.plan.bounds = PixelRect::full(size);
        self.scene.jobs.push(Job::Clear(target.view.clone(), wgpu::Color::TRANSPARENT));
        self.scene.jobs.push(Job::VectorResample { source, origin,
            target: target.view.clone(), target_origin: window.min, side, size: target.plan.size, inverse });
        let plan = target.plan;
        self.materialize(Value::Image { view: target.view, slot: target.slot, opacity: 1., plan, preview: None, encode: false }, output)
    }
    fn vector_preview(&mut self, owner: OccurrenceHandle, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let Some((_, path, bounds)) = self.scene.scale_sources.vector_preview.clone().filter(|(o, ..)| *o == owner) else { return self.materialize(Value::Color([0.; 4]), output) };
        if bounds.intersect(self.region).is_empty() { return self.materialize(Value::Color([0.; 4]), output); }
        let mut target = self.target()?;
        let side = 1u32 << target.plan.level;
        let window = self.region.intersect(target.plan.doc_bounds).aligned(side);
        let size = window.size().map_err(|_| GpuRasterError::ExtentUnsupported)?;
        target.plan.doc_bounds = window; target.plan.size = size.map(|v| v.div_ceil(side)); target.plan.bounds = PixelRect::full(size);
        let plan = target.plan;
        let offset = self.packet.scene.occurrence_offset64(owner);
        let batch = self.scene.vectors.preview_batch(&path, offset, window.min, plan.level, plan.size, self.packet.document_extent, Scene::vector_palette(self.r, self.packet));
        self.scene.jobs.push(Job::Clear(target.view.clone(), wgpu::Color::TRANSPARENT));
        if batch.tile_count() > 0 {
            self.scene.jobs.push(super::Job::Vector(Box::new(super::vectors::VectorJob { batch, output: target.view.clone(), base: None })));
        }
        self.materialize(Value::Image { view: target.view, slot: target.slot, opacity: 1., plan, preview: None, encode: false }, output)
    }
    fn objects(&mut self,content:&object_spatial::Content,moving:bool,output:Option<Target>)->Result<Value,GpuRasterError> {
        let mut target=self.target()?;let side=1u32<<target.plan.level;
        let bounds=self.region.intersect(target.plan.doc_bounds).aligned(side);
        let size=bounds.size().map_err(|_|GpuRasterError::ExtentUnsupported)?;
        target.plan.doc_bounds=bounds;target.plan.size=size.map(|v|v.div_ceil(side));target.plan.bounds=PixelRect::full(size);
        let plan=target.plan;
        let window=objects::ObjectWindow {origin:bounds.min.map(|v|v as f64),side:f64::from(side),size:plan.size};
        let mut request=self.scene.collection_job(self.r,self.packet,content.owner(),content.clone(),window);
        request.live=true;request.display=true;
        if request.keys.is_empty() {self.release(target.slot);return self.materialize(Value::Color([0.;4]),output);}
        let moving = moving || self.r.moving_layer.is_some() || self.preview;
        let fallback = self.accepted.objects_fall_back();
        let preview = if moving || fallback {self.scene.collection_preview(self.r,&request)?} else {None};
        if preview.is_none() && !fallback && let Some(pieces)=self.scene.canonical_cover(self.r,self.packet,content,&request,self.accepted,self.class)? {
            let window=request.bounds();
            self.scene.jobs.push(Job::Clear(target.view.clone(), wgpu::Color::TRANSPARENT));
            for (view,piece) in pieces {
                let shared=piece.intersect(window);
                let texels=|rect:DocRect,origin:[i64;2]|std::array::from_fn::<u32,2,_>(|i|((rect.min[i]-origin[i])/i64::from(side)) as u32);
                let [width,height]=shared.size()?.map(|v|v/side);
                let [x,y]=texels(shared,piece.min);
                let [dx,dy]=texels(shared,window.min);
                self.scene.jobs.push(Job::Copy { source: view.texture().clone(), source_origin: [x,y],
                    destination: target.view.texture().clone(), origin: [dx,dy], width, height });
            }
            return self.materialize(Value::Image {view:target.view,slot:target.slot,opacity:1.,plan,preview:None,encode:false},output);
        }
        let preview=match preview {None if !moving && !fallback=>self.scene.collection_preview(self.r,&request)?,preview=>preview};
        let Some((sources,objects))=preview else {self.release(target.slot);return Err(GpuRasterError::NotReady);};
        self.approximate = true;
        let scratch=(crate::object_sampling::CollectionPreview::passes(&objects)>1).then(||self.target()).transpose()?;
        self.scene.jobs.push(Job::Collection(Box::new(crate::object_sampling::CollectionPreview {sources,objects,size:plan.size,output:target.view.clone(),
            scratch:scratch.as_ref().map(|scratch|scratch.view.clone()),encode:self.packet.blend_space==layer_core::BlendSpace::Perceptual})));
        if let Some(scratch)=scratch {self.release(scratch.slot);}
        self.materialize(Value::Image {view:target.view,slot:target.slot,opacity:1.,plan,preview:None,encode:false},output)
    }
    fn source_pixels(&mut self, id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let encode = self.packet.blend_space == layer_core::BlendSpace::Perceptual
            && matches!(id, SourceTarget::Paint(_));
        let placement=self.accepted.placement(id)?;
        debug_assert_eq!(frame,placement.frame,"accepted source frame differs from its expression");
        if frame!=placement.frame {return Err(GpuRasterError::Invariant("accepted source frame differs from its expression"));}
        let frame=placement.frame;
        let origin = placement.offset;
        if self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) {
            return Ok(Value::Transform(TransformSource { id, placement: layer_core::Affine::translation(layer_core::offsets::point(origin)), opacity: 1., backdrop: [0.; 4], encode }));
        }
        let moving = self.r.moving_layer == self.packet.scene.source_owner(id);
        let level=source_level(self.input.level,frame,moving,preview_input_level(&self.cache.pages,id));
        let plan=display_mips::Plan::at(frame.extent(extent),level);
        if self.tiled && self.cache.plan.level == 0 && plan.level == 0 {
            let tile = [self.region.in_frame(self.packet.document_extent).min_x() / PAGE_SIZE, self.region.in_frame(self.packet.document_extent).min_y() / PAGE_SIZE];
            let slot = if let SourceTarget::Paint(_) = id {
                let handle = self.packet.scene.source_owner(id).unwrap();
                if origin == [0; 2] && self.r.watercolor_style(id, self.packet.dab_batches).is_none() {
                    let inputs = self.scene.color_inputs(self.r,self.packet.scene,id,tile,self.accepted)?;
                    let (base, preview) = match inputs {
                        [Some(base), preview] => (base, preview.map(|preview| preview.view)),
                        [None, Some(preview)] => (preview, None),
                        [None, None] => return Ok(Value::Color([0.; 4])),
                    };
                    return Ok(Value::Image { view: base.view, slot: base.lease.map(Slot::Decoded), opacity: 1., plan: self.working_plan(), preview, encode });
                }
                if self.r.watercolor_style(id, self.packet.dab_batches).is_none() {
                    let (color, flow) = self.scene.placed_color(self.r, self.packet.scene, id, &placement, tile, self.accepted)?;
                    let color = self.scene.own(color);
                    let flow = flow.map(|flow| self.scene.own(flow));
                    let view = self.scene.pool[color.index].view.clone();
                    let preview = flow.as_ref().map(|flow| self.scene.pool[flow.index].view.clone());
                    let slot = match flow {Some(flow) => Slot::Scenes([color, flow]),None => Slot::Scene(color)};
                    return Ok(Value::Image { view, slot: Some(slot), opacity: 1., plan: self.working_plan(), preview, encode });
                }
                self.scene.paint_tile(self.r, self.packet, handle, &placement, tile, self.accepted)?
            } else {
                let handle = self.packet.scene.source_owner(id).unwrap();
                let (mask, source) = self.packet.scene.mask(handle).unwrap();
                self.scene.mask_at(self.r, mask, source, origin, tile, self.accepted)?
            };
            let slot = self.scene.own(slot);
            return Ok(Target { view: self.scene.pool[slot.index].view.clone(), slot: Some(Slot::Scene(slot)), plan: self.working_plan() }.value().with_encoding(encode));
        }
        let encode = encode && self.scene.scale_sources.entries[&id].blend_space == layer_core::BlendSpace::Linear;
        let needed=frame.local(self.region,extent);
        let needed=if frame.aligned() && !moving {needed}else {needed.expand(1<<plan.level,plan.extent)};
        let source=&self.scene.scale_sources.entries[&id];
        let needed=needed.intersect(PixelRect::full(source.valid_extent(plan.level)));
        if needed.is_empty() {return Ok(Value::Color([outside;4]));}
        let mut inputs=Vec::new();
        for coordinate in pages::coordinates(needed,plan.level) {
            let key=source.key(self.r,id,plan.level,coordinate);
            let region=key.plan().bounds.intersect(needed);
            let image=match &self.cache.gesture {
                Some(contact)=>contact.pages.get(&key).cloned(),
                None=>self.cache.pages.lock().unwrap().get(&key,region,pages::Accuracy::Display,plan.level,self.class),
            }.ok_or(GpuRasterError::NotReady)?;
            inputs.push((key,image,region));
        }
        let (view,plan,slot)=if inputs.len()==1 {
            let (_,image,_)=inputs.pop().unwrap();
            self.unit.reads.push(image.clone());
            (image.view.clone(),image.plan,Some(Slot::Root))
        } else {
            let side=1<<plan.level;
            let bounds=paint_transform::aligned(needed,side,plan.extent);
            let plan=display_mips::Plan::window(plan.extent,plan.level,bounds);
            let slot=self.cache.allocate(self.r,plan)?;
            let view=self.cache.output[slot.index].view.clone();
            for (_,input,region) in inputs {
                let [sx,sy,width,height]=paint_transform::texel_rect(region.window_local(input.plan.bounds),side);
                let [x,y,_,_]=paint_transform::texel_rect(region.window_local(plan.bounds),side);
                self.scene.jobs.push(Job::Copy {source:input.texture.clone(),source_origin:[sx,sy],destination:view.texture().clone(),origin:[x,y],width,height});
                self.unit.reads.push(input);
            }
            (view,plan,Some(Slot::Cache(slot)))
        };
        if frame.aligned() && !moving && plan.level == self.cache.plan.level
            && (outside == 0. || self.region.intersect(frame.document(plan.bounds)) == self.region) {
            return Ok(Value::Image {view,slot,opacity:1.,plan:frame.placed(plan),preview:None,encode});
        }
        let from_texels = layer_core::Projective::from_affine(layer_core::Affine([(1u32 << plan.level) as f32,0.,0.,(1u32 << plan.level) as f32,
            plan.bounds.min_x() as f32,plan.bounds.min_y() as f32]));
        let side = (1u32 << self.cache.plan.level) as f32;
        let mut transform = layer_core::ImageTransform::default();
        transform.placement = transform.placement.post(layer_core::Projective([1./side,0.,0.,0.,1./side,0.,0.,0.,1.]))
            .ok_or(GpuRasterError::InvalidTransform("Invalid source placement"))?;
        transform.source_from_owner = Some(from_texels.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid source plan"))?);
        Ok(Value::Placed(Placed { view,slot,transform, shift: frame.shift, plan, outside, opacity: 1., backdrop: [0.; 4], encode }))
    }
    fn transform(&mut self, source: TransformSource, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let Target { view, slot, plan } = match output { Some(target) => target, None => self.target()? };
        let side = 1 << self.cache.plan.level;
        let region = self.region;
        let display = pixel_transform::DisplayLevel { side, extent: self.cache.plan.extent, opacity: source.opacity, backdrop: source.backdrop, encode: source.encode };
        if self.accepted.declaring() { return Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false }); }
        let fine_budget=self.cache.fine_output_budget(self.r,self.scene,self.retained);
        let mut transforms = self.r.transforms.take().unwrap();
        let result = transforms.prepare_region(self.r, self.encoder, source.id, source.placement, &view, display, plan, region,fine_budget);
        self.r.transforms = Some(transforms);
        self.scene.jobs.push(Job::Transform(Box::new(result?)));
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
    fn resample(&mut self, value: Value, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, output); }
        let Value::Placed(placed) = value else { return Ok(value); };
        let Target { view, slot, plan } = match output { Some(target) => target, None => self.target()? };
        let texels = self.texels(plan);
        let values = placed.record(plan, texels)?;
        if texels[2] != 0 && texels[3] != 0 { self.scene.jobs.push(Job::Resample {
            values, views: [placed.view, view.clone()], size: [texels[2], texels[3]],
        }); }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
    fn materialize(&mut self, value: Value, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if matches!(value,Value::Transform(_) | Value::Placed(_)) {
            if output.as_ref().is_some_and(|target|target.view.texture().format()==wgpu::TextureFormat::Rgba16Float) {
                let value=self.resample(value,None)?;
                return self.materialize(value,output);
            }
            return self.resample(value,output);
        }
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
        let Target { view, slot, plan } = match output { Some(target) => target, None => self.target()? };
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
            let side = 1 << target.level;
            values[offset] = ((target.doc_bounds.min[0] - source.doc_bounds.min[0]) / i64::from(side)) as u32;
            values[offset + 1] = ((target.doc_bounds.min[1] - source.doc_bounds.min[1]) / i64::from(side)) as u32;
        }
        let sources = [front.view(), back.view(), front.preview(), back.preview()].map(|source| source.unwrap_or(&self.r.empty_view).clone());
        let leases = [front.slot(), back.slot()].into_iter().filter_map(|slot| match slot {
            Some(Slot::Decoded(lease)) => Some(lease), _ => None,
        }).collect();
        if width != 0 && height != 0 { self.scene.jobs.push(Job::Compose { values, output: view.clone(), sources, _leases: leases, format:view.texture().format() }); }
        for slot in [front.slot(), back.slot()] { self.release(slot); }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
}

#[cfg(test)]
mod signed_region_tests;

#[cfg(test)]
pub(crate) use pages::query_tests::{assert_regional_object_overview,regional_object_overview_footprint};

#[cfg(test)]
pub(crate) use pages::query_tests::assert_display_grant_decrease_reconciles_retention;

