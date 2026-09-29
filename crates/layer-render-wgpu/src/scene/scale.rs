//! Demand-driven presentation composition with bounded region and scale grids.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;

mod sources;
mod graph;
mod effects;
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
                crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::texture(1, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::storage_texture(
                    2,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::TextureFormat::Rgba32Float,
                    wgpu::StorageTextureAccess::WriteOnly,
                ),
                crate::bindings::texture(3, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::texture(4, wgpu::ShaderStages::COMPUTE, false),
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
    composition: Vec<Composition>,
}
struct Composition {
    values: [u32; 20],
    binding: wgpu::BindGroup,
    leases: Vec<Arc<()>>,
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
        let record_binding = crate::bindings::group(
            &r.device,
            "display composition regions",
            &r.scene_pipelines.scale.records,
            [wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &records,
                offset: 0,
                size: NonZeroU64::new(80),
            })],
        );
        Self { records, record_binding, stride, cursor: 0, composition: Vec::new() }
    }
    pub fn begin(&mut self) { self.cursor = 0; self.composition.clear(); }
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
    fn compose(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, job: Composition) -> Result<(), GpuRasterError> {
        self.composition.push(job);
        if self.composition.len() >= 32 { self.flush(r, encoder)?; }
        Ok(())
    }
    fn flush(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if self.composition.is_empty() { return Ok(()); }
        let jobs = std::mem::take(&mut self.composition);
        let mut records = vec![0; jobs.len() * self.stride as usize];
        for (record, job) in records.chunks_exact_mut(self.stride as usize).zip(&jobs) {
            for (dst, value) in record.chunks_exact_mut(4).zip(job.values) { dst.copy_from_slice(&value.to_le_bytes()); }
        }
        let offset = self.write(r, encoder, &records)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("display graph composition"), timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.compose);
        for (i, job) in jobs.iter().enumerate() {
            pass.set_bind_group(0, &self.record_binding, &[offset + i as u32 * self.stride]);
            pass.set_bind_group(1, &job.binding, &[]);
            pass.dispatch_workgroups(job.values[2].div_ceil(8), job.values[3].div_ceil(8), 1);
        }
        drop(pass);
        for mut job in jobs { job.leases.clear(); }
        Ok(())
    }
}

pub(crate) struct Cache {
    pub plan: display_mips::Plan,
    output: Vec<Image>,
    used: Vec<bool>,
    next: Option<Image>,
    selected: usize,
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
    unchanged: bool,
    graph: graph::Graph,
    transform: Option<layer_render::TransformPreview>,
    source_overlap: bool,
    streamed_sources: bool,
}

pub(crate) fn level(r: &WgpuRasterizer, packet: FramePacket<'_>) -> Option<u32> {
    if r.native_edit.is_none() {
        return None;
    }
    #[cfg(test)]
    if r.test.reference || r.test.exact_display {
        return None;
    }
    let level = display_mips::view_level(packet.view.document_to_surface, 4)?;
    let plan = request(packet, level)?;
    let records = records_for(r, plan, packet.layers);
    let bytes = allocation_for(r, plan, packet, None, bounded(packet.layers)).into_iter().sum::<u64>();
    if bytes > live_display::CACHE_BYTES
        || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
    {
        return None;
    }
    let fits = transform_plans(r, plan, packet.layers).all(|(source, _)| source.size.iter()
        .all(|size| *size <= r.device.limits().max_texture_dimension_2d)) && targets(r, packet).all(|(layer, id)| {
        source_plan(plan, layer_core::target_transform(packet.layers, id), layer.local_extent(plan.extent))
            .is_ok_and(|source| (plan.level == 0 && source.level == 0) || source.size.iter().all(|size| *size <= r.device.limits().max_texture_dimension_2d))
    });
    let supported = fits && packet.layers.iter().all(|l| {
            !l.visible
                || !l.is_artwork()
                || (matches!(l.kind, LayerKind::Paint | LayerKind::Background | LayerKind::Group | LayerKind::Effect)
                    && l.effect.as_ref().is_none_or(|effect| (!effect.program.image_boundary() || plan.level > 0)
                        && effect.program.resolution == layer_core::EffectResolution::Display)
                    && r.paint_layers
                        .iter()
                        .find(|p| p.id == l.id)
                        .is_none_or(|p| p.watercolor.is_none()))
        })
        && packet
            .dab_batches
            .iter()
            .all(|b| b.style.execution != BrushExecution::Watercolor);
    supported.then_some(plan.level)
}

pub(crate) fn source_level(level: u32, placement: layer_core::Affine) -> u32 {
    paint_transform::local_level(level, placement).saturating_sub(u32::from(placement != layer_core::Affine::IDENTITY))
}

pub(super) fn placement_level(layers: &[Layer], id: LayerId) -> u32 {
    display_mips::view_level(layer_core::target_transform(layers, id).0, 8).unwrap_or(0)
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

fn local_regions(required: PixelRect, covered: PixelRect, placement: layer_core::Affine, extent: [u32; 2], level: u32) -> Result<(PixelRect, PixelRect), GpuRasterError> {
    if required.is_empty() { return Ok((PixelRect::EMPTY, covered)); }
    if placement == layer_core::Affine::IDENTITY { return Ok((required.intersect(PixelRect::full(extent)), covered)); }
    let inverse = placement.inverse().ok_or(GpuRasterError::InvalidTransform("Transform must be finite and invertible"))?;
    Ok((pixel_rect(inverse.bounds(required.to_rect()), extent).expand(1 << level, extent), PixelRect::EMPTY))
}

fn source_plan(output: display_mips::Plan, placement: layer_core::Affine, extent: [u32; 2]) -> Result<display_mips::Plan, GpuRasterError> {
    let level = source_level(output.level, placement);
    let (bounds, _) = local_regions(output.bounds, PixelRect::EMPTY, placement, extent, level)?;
    let coarse = source_coarse_level(display_mips::Plan::at(extent, level));
    let bounds = if bounds.is_empty() { PixelRect::EMPTY } else {
        paint_transform::aligned(bounds.expand(2 << coarse, extent), PAGE_SIZE.max(1 << coarse), extent)
    };
    Ok(display_mips::Plan::window(extent, level, bounds))
}

fn request(packet: FramePacket<'_>, level: u32) -> Option<display_mips::Plan> {
    let extent = packet.document_extent;
    if level > 0 && !bounded(packet.layers) {
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
    allocation_for(r, plan, packet, sources, false)
}

fn allocation_for(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>, sources: Option<&Sources>, streamed: bool) -> [u64; 2] {
    let layers = packet.layers;
    let images = scratch_images(layers);
    let (source_bytes, root_mips) = targets(r, packet).fold((0, 0), |(sum, largest), (layer, id)| {
        let placement = layer_core::target_transform(layers, id);
        if streamed && placement == layer_core::Affine::IDENTITY { return (sum, largest); }
        let Ok(source) = source_plan(plan, placement, layer.local_extent(plan.extent)) else { return (sum, largest); };
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
    let working = if bounded(layers) {
        u64::from(PAGE_SIZE).pow(2) * 16 * (images + pixel_transform::TRANSFORM_SLOTS as u64)
    } else { output * (images - 1) };
    let own = [source_bytes, output + working + root_mips + plan.level_bytes(plan.level + 1) + records + 64 + transform];
    if plan.bounds == PixelRect::full(plan.extent) { own }
    else {
        let overview = allocation(r, overview_plan(plan), packet, sources);
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
            let level = paint_transform::input_level(plan.level, &preview, layer_core::target_transform(layers, layer.id), extent);
            let (level, kept) = r.transforms.as_ref().map_or_else(
                || (level, paint_transform::keeps_pixels(preview.selection.as_ref(), PixelRect::full(extent))),
                |transforms| transforms.input_requirements(&preview, level, extent));
            Some((display_mips::Plan::at(extent, level), kept))
        });
    let standby = r.moving_pixels.as_ref().filter(|_| r.transform_preview.is_none() && plan.level > 0)
        .and_then(|(id, selection)| {
            let layer = layers.iter().find(|l| l.id == *id && l.kind == LayerKind::Paint)?;
            let extent = layer.local_extent(plan.extent);
            let level = paint_transform::sampling::selection_level(plan.level, layer_core::target_transform(layers, *id), Some(selection), extent);
            let (level, kept) = r.transforms.as_ref().unwrap().standby_requirements(layer, selection, level, extent);
            Some((display_mips::Plan::at(extent, level), kept))
        });
    active.chain(standby)
}

#[cfg(test)]
mod tests;

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
        level: u32,
        unchanged: bool,
    ) -> (Self, bool) {
        let plan = request(packet, level).expect("admitted display request");
        let required = display_mips::view_bounds(packet.view, plan.extent, 2 << plan.level)
            .expect("admitted display view");
        let matches = |cache: &Self| {
            cache.plan.extent == plan.extent && cache.plan.level == plan.level
                && cache.plan.bounds.intersect(required) == required
                && cache.layer_count == packet.layers.len()
                && allocation_for(r, cache.plan, packet, None, bounded(packet.layers))
                    .into_iter().sum::<u64>() <= live_display::CACHE_BYTES
        };
        let unchanged = unchanged && !packet.reset_layers && packet.restore_rasters.is_empty();
        let Some(mut old) = previous else {
            let mut cache = Self::new(r, plan, packet.layers.len());
            cache.admit_source_overlap(r, packet);
            return (cache, true);
        };
        let unchanged = unchanged && old.transform == r.transform_preview;
        old.transform = r.transform_preview.clone();
        if !unchanged {
            old.spare = None;
        }
        if matches(&old) {
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
            .unwrap_or_else(|| Self::new(r, plan, packet.layers.len()));
        next.unchanged = unchanged;
        next.reuse_output = unchanged && next.ready;
        if next.overview.as_ref().map(|c| c.plan) == old.overview.as_ref().map(|c| c.plan)
            && let Some(mut overview) = old.overview.take() {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.ready;
                next.overview = Some(overview);
        }
        next.admit_source_overlap(r, packet);
        let bound = allocation_for(r, next.plan, packet,
            r.scene.as_ref().filter(|_| next.source_overlap).map(|s| &s.scale_sources), next.streamed_sources).into_iter().sum::<u64>();
        if unchanged && old.placed.is_none() && old.plan.extent == plan.extent && bound + old.storage_bytes() <= live_display::CACHE_BYTES {
            if old.plan.level == plan.level && old.ready {
                next.shifted = Some(Box::new(old));
            } else {
                next.spare = Some(Box::new(old));
            }
        }
        (next, true)
    }

    pub fn new(r: &WgpuRasterizer, plan: display_mips::Plan, layers: usize) -> Self {
        let level = plan.level;
        let mut geometry = [0u32; 16];
        geometry[0] = 1;
        let overview = (plan.bounds != PixelRect::full(plan.extent))
            .then(|| Box::new(Self::new(r, overview_plan(plan), layers)));
        geometry[1] = 1 << overview.as_ref().map_or_else(|| coarse_level(plan), |c| coarse_level(c.plan));
        geometry[2] = 1 << level;
        geometry[4..8].copy_from_slice(&[
            plan.bounds.min_x() >> level, plan.bounds.min_y() >> level,
            plan.bounds.max_x().div_ceil(1 << level), plan.bounds.max_y().div_ceil(1 << level),
        ]);
        geometry[8..10].copy_from_slice(&plan.size);
        geometry[11] = 1 << (level + 1);
        let geometry = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("display composition geometry"),
                contents: &geometry
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        Self {
            plan,
            output: (0..if level == 0 { 1 } else { 0 }).map(|_| Image::new(r, plan, "display composition level")).collect(),
            used: vec![false; if level == 0 { 1 } else { 0 }],
            next: None,
            selected: 0,
            placed: None,
            geometry,
            layer_count: layers,
            spare: None,
            ready: false, source_overlap: false, streamed_sources: false,
            reuse_output: false,
            overview, shifted: None, valid: BTreeSet::new(), unchanged: false, graph: Default::default(), transform: r.transform_preview.clone(),
        }
    }
    fn admit_source_overlap(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>) {
        self.streamed_sources = bounded(packet.layers) && allocation(r, self.plan, packet, None)
            .into_iter().sum::<u64>() > live_display::CACHE_BYTES;
        self.source_overlap = allocation(r, self.plan, packet, r.scene.as_ref().map(|s| &s.scale_sources))
            .into_iter().sum::<u64>() <= live_display::CACHE_BYTES;
        if let Some(overview) = &mut self.overview { overview.source_overlap = self.source_overlap; }
    }
    fn source_plan(&self, sources: &Sources, id: LayerId, requested: display_mips::Plan) -> display_mips::Plan {
        if self.source_overlap { sources.resident_plan(id, requested) } else { requested }
    }
    pub fn preview_level(&self, packet: FramePacket<'_>, id: LayerId) -> u32 {
        if self.plan.level == 0 { return 0; }
        let placement = layer_core::target_transform(packet.layers, id);
        source_level(self.plan.level, placement).min(crate::preview_block(
            placement.then(layer_core::Affine(packet.view.document_to_surface)).0).ilog2())
    }
    pub fn source_levels(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources) -> BTreeMap<LayerId, BTreeMap<u32, display_mips::Plan>> {
        let plans = std::iter::once((self.plan, self.streamed_sources)).chain(self.overview.as_ref().map(|c| (c.plan, c.streamed_sources)));
        let mut requested: BTreeMap<_, BTreeMap<_, _>> = targets(r, packet)
            .filter(|(_, id)| sources.entries.contains_key(id) && !r.transforms.as_ref().is_some_and(|t| t.display_source(*id))).map(|(layer, id)| {
            let placement = layer_core::target_transform(packet.layers, id);
            (id, plans.clone().filter(|(_, streamed)| !streamed || placement != layer_core::Affine::IDENTITY)
                .filter_map(|(p, _)| source_plan(p, placement, layer.local_extent(p.extent)).ok().filter(|s| p.level > 0 || s.level > 0))
                .filter(|p| !p.bounds.is_empty()).map(|p| (p.level, self.source_plan(sources, id, p))).collect())
        }).collect();
        if let Some(root) = &self.placed && let Some(levels) = requested.get_mut(&root.value.id) {
            if let Some(plan) = levels.get(&root.value.plan.level).copied() {
                for level in [plan.level + 1, source_coarse_level(plan)] {
                    levels.insert(level, display_mips::Plan::window(plan.extent, level, plan.bounds));
                }
            }
        }
        requested
    }
    pub fn source_budget(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, commands: &Commands, sources: Option<&Sources>) -> u64 {
        let reserve = allocation_for(r, self.plan, packet, sources.filter(|_| self.source_overlap), self.streamed_sources)[1]
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes());
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let branches = self.graph.reserved_bytes(self.plan)
            + self.overview.as_ref().map_or(0, |c| c.graph.reserved_bytes(c.plan));
        live_display::CACHE_BYTES.saturating_sub((reserve + retained_records + branches).max(self.storage_bytes() + commands.storage_bytes()))
    }

    pub fn prepare_graph(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, commands: &Commands) -> Result<(), GpuRasterError> {
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let budget = live_display::CACHE_BYTES.saturating_sub(allocation_for(r, self.plan, packet, self.source_overlap.then_some(sources), self.streamed_sources).into_iter().sum::<u64>()
            + retained_records + self.spare.as_ref().map_or(0, |c| c.storage_bytes()) + self.shifted.as_ref().map_or(0, |c| c.storage_bytes()));
        self.graph.prepare(r, packet, sources, self.plan, budget)?;
        if let Some(overview) = &mut self.overview {
            overview.graph.prepare(r, packet, sources, overview.plan, budget.saturating_sub(self.graph.reserved_bytes(self.plan)))?;
        }
        Ok(())
    }

    pub fn view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| &self.output[self.selected].view, |root| &root.value.view)
    }
    pub fn texture(&self) -> &wgpu::Texture { &self.output[self.selected].texture }
    pub fn coarse_view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| self.overview.as_ref().map_or_else(|| if coarse_level(self.plan) > self.plan.level { self.next_view() } else { self.view() }, |c| c.coarse_view()), |root| &root.coarse)
    }
    pub fn next_view(&self) -> &wgpu::TextureView {
        self.placed.as_ref().map_or_else(|| &self.next.as_ref().expect("composed output mip").view, |root| &root.next)
    }
    pub fn placement_values(&self) -> [f32; 20] {
        let Some(root) = &self.placed else { return [0.; 20]; };
        let [x, y, _] = pixel_transform::inverse_rows(&root.value.transform).expect("validated placement");
        let side = (1 << self.plan.level) as f32;
        let source_side = (1 << root.value.plan.level) as f32;
        let [width, height] = [root.value.plan.bounds.width(), root.value.plan.bounds.height()].map(|n| n as f32 / source_side);
        let [r, g, b, a] = root.value.backdrop;
        [x[0] / side, x[1] / side, x[2], 0., y[0] / side, y[1] / side, y[2], 0.,
            width, height, (1 << (root.coarse_level - root.value.plan.level)) as f32, 2.,
            root.value.opacity, 1., root.value.outside, f32::from(root.value.encode), r, g, b, a]
    }
    pub fn storage_bytes(&self) -> u64 {
        self.output
            .iter()
            .map(|i| texture_bytes(&i.texture))
            .sum::<u64>()
            + self.geometry.size()
            + self.next.as_ref().map_or(0, |image| texture_bytes(&image.texture))
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.overview.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.graph.storage_bytes()
    }
    fn allocate(&mut self, r: &WgpuRasterizer) -> usize {
        let slot = self.used.iter().position(|used| !used).unwrap_or_else(|| {
            self.output.push(Image::new(r, self.plan, "display composition level"));
            self.used.push(false);
            self.used.len() - 1
        });
        self.used[slot] = true;
        slot
    }
    pub(super) fn render(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<(), GpuRasterError> {
        let changed = self.render_graph(scene, r, packet, dirty, encoding, None, tiles)?;
        if let Some(mut overview) = self.overview.take() {
            let result = overview.render_graph(scene, r, packet, dirty, encoding, Some((self, changed)), tiles);
            self.overview = Some(overview);
            result?;
        }
        Ok(())
    }

    fn render_graph(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, finer: Option<(&Cache, PixelRect)>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let Encoding { encoder, commands } = encoding;
        if self.reuse_output { return Ok(PixelRect::EMPTY); }
        let dirty = if self.unchanged { PixelRect::EMPTY } else { dirty };
        let covered = finer.map_or(PixelRect::EMPTY, |(cache, _)| cache.plan.bounds);
        let visible: Vec<_> = packet
            .layers
            .iter()
            .rev()
            .filter(|l| images::visible(packet.layers, l) && l.kind == LayerKind::Paint && l.is_artwork() && stack::has_content(r, l))
            .collect();
        let copied = self.copy_shifted(r, encoder);
        let root = self.graph.root.clone().expect("prepared composition graph");
        let invalid = dirty.union(root.damage(&scene.scale_sources, self.plan));
        self.valid.retain(|c| page_rect(*c).intersect(invalid).is_empty() || tiles.is_some_and(|tiles| !tiles.contains(c)));
        let regions = page_regions(page_coordinates(self.plan.bounds).filter(|c| !self.valid.contains(c)), self.plan.bounds);
        let changed = regions.iter().fold(PixelRect::EMPTY, |a, b| a.union(*b));
        let required = if scene.scale_sources.reset { self.plan.bounds } else { root.required(changed, self.plan).union(changed) };
        let source_covered = if packet.layers.iter().any(|l| l.effect.as_ref().is_some_and(|e| !e.program.passes.is_empty())) { PixelRect::EMPTY } else { covered };
        for layer in &visible {
            if r.transforms.as_ref().is_some_and(|t| t.display_source(layer.id)) { continue; }
            let placement = layer_core::target_transform(packet.layers, layer.id);
            if self.streamed_sources && placement == layer_core::Affine::IDENTITY { continue; }
            let extent = layer.local_extent(packet.document_extent);
            let requested = source_plan(self.plan, placement, extent)?;
            if self.plan.level == 0 && requested.level == 0 { continue; }
            let plan = self.source_plan(&scene.scale_sources, layer.id, requested);
            let (needed, covered) = local_regions(required, source_covered, placement, extent, plan.level)?;
            let needed = if placement == layer_core::Affine::IDENTITY { needed } else { plan.bounds };
            scene.prepare_scale_color(commands, r, packet, encoder, layer, SourceRequest { plan, required: needed, covered })?;
        }
        for layer in packet.layers {
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (m.show_area || images::visible(packet.layers, layer))) {
                let placement = layer_core::target_transform(packet.layers, mask.id);
                if self.streamed_sources && placement == layer_core::Affine::IDENTITY { continue; }
                let extent = layer.local_extent(packet.document_extent);
                let requested = source_plan(self.plan, placement, extent)?;
                if self.plan.level == 0 && requested.level == 0 { continue; }
                let plan = self.source_plan(&scene.scale_sources, mask.id, requested);
                let (needed, covered) = local_regions(required, source_covered, placement, extent, plan.level)?;
                let needed = if placement == layer_core::Affine::IDENTITY { needed } else { plan.bounds };
                scene.prepare_scale_mask(commands, r, encoder, mask, SourceRequest { plan, required: needed, covered })?;
            }
        }
        let side = 1 << self.plan.level;
        let [x, y, width, height] = paint_transform::texel_rect(copied.window_local(self.plan.bounds), side);
        let mut written = PixelRect::new(x, y, x + width, y + height);
        let regions: Vec<_> = regions.into_iter().flat_map(|r| r.subtract(covered)).filter(|r| !r.is_empty()).collect();
        let tiled = bounded(packet.layers);
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
            let [x, y, width, height] = paint_transform::texel_rect(region.window_local(self.plan.bounds), side);
            self.used.fill(false);
            let mut compositor = Evaluator { cache: self, commands, scene, packet, r, encoder, region, tiled };
            let root = compositor.cache.graph.root.clone().expect("prepared composition graph");
            let output = compositor.evaluate_root(&root)?;
            let output = match output {
                Value::Placed(value) if finer.is_none() => {
                    drop(compositor);
                    let coarse_level = source_coarse_level(value.plan);
                    for level in [value.plan.level + 1, coarse_level] {
                        scene.scale_sources.ensure_level(commands, r, encoder, value.id,
                            display_mips::Plan::window(value.plan.extent, level, value.plan.bounds))?;
                    }
                    let next = scene.scale_sources.image(value.id, value.plan.level + 1).image.view.clone();
                    let coarse = scene.scale_sources.image(value.id, coarse_level).image.view.clone();
                    self.placed = Some(Presentation { value, next, coarse, coarse_level });
                    self.output.clear();
                    self.used.clear();
                    self.next = None;
                    self.ready = true;
                    return Ok(self.plan.bounds);
                }
                output => output,
            };
            compositor.cache.placed = None;
            let output = compositor.materialize(output, None)?;
            let Some(Slot::Cache(selected)) = output.slot() else { unreachable!() };
            compositor.cache.selected = selected;
            compositor.cache.valid.extend(page_coordinates(region));
            written = written.union(PixelRect::new(x, y, x + width, y + height));
            r.metrics.composited_pixels += u64::from(width) * u64::from(height);
        }
        if let Some((finer, changed)) = finer {
            let region = if self.ready { changed } else { covered };
            if !region.is_empty() {
                if self.output.is_empty() { self.selected = self.allocate(r); }
                let [x, y, width, height] = paint_transform::texel_rect(region.window_local(self.plan.bounds), side);
                let input_level = finer.plan.level + 1;
                let mut values = [0; 20];
                values[..8].copy_from_slice(&[x, y, width, height, finer.plan.bounds.width(), finer.plan.bounds.height(),
                    1 << (self.plan.level - input_level), (input_level << 8) | 8]);
                values[14] = ((finer.plan.bounds.min_x() >> input_level) as f32).to_bits();
                values[15] = ((finer.plan.bounds.min_y() >> input_level) as f32).to_bits();
                let offset = commands.record(r, encoder, values)?;
                let binding = Commands::binding(r, finer.next_view(), &r.empty_view, self.view());
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("derive overview from completed detail"), timestamp_writes: None,
                });
                pass.set_pipeline(&r.scene_pipelines.scale.reduce);
                pass.set_bind_group(0, &commands.record_binding, &[offset]);
                pass.set_bind_group(1, &binding, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
                drop(pass);
                written = written.union(PixelRect::new(x, y, x + width, y + height));
                self.valid.extend(page_coordinates(region));
            }
        }
        self.reduce_output(r, encoder, written, commands)?;
        Ok(if written.is_empty() { written } else { PixelRect::new(
            self.plan.bounds.min_x() + written.min_x() * side, self.plan.bounds.min_y() + written.min_y() * side,
            self.plan.bounds.min_x() + written.max_x() * side, self.plan.bounds.min_y() + written.max_y() * side,
        ).intersect(self.plan.bounds) })
    }

    fn copy_shifted(&mut self, r: &WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> PixelRect {
        let bounds = self.plan.bounds;
        let mut changed = PixelRect::EMPTY;
        if let Some(old) = self.shifted.take() {
            let overlap = bounds.intersect(old.plan.bounds);
            if !overlap.is_empty() {
                if self.output.is_empty() { self.allocate(r); }
                let side = 1 << self.plan.level;
                let [sx, sy, width, height] = paint_transform::texel_rect(overlap.window_local(old.plan.bounds), side);
                let [x, y, _, _] = paint_transform::texel_rect(overlap.window_local(bounds), side);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: sx, y: sy, z: 0 },
                        ..old.texture().as_image_copy() },
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x, y, z: 0 },
                        ..self.output[0].texture.as_image_copy() },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );
                self.valid.extend(old.valid.iter().copied().filter(|c| !page_rect(*c).intersect(overlap).is_empty()));
                changed = overlap;
            }
        }
        changed
    }

    fn reduce_output(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, region: PixelRect, commands: &mut Commands) -> Result<(), GpuRasterError> {
        if region.is_empty() { return Ok(()); }
        let origin = [region.min_x(), region.min_y()];
        let size = [region.width(), region.height()];
        let next_origin = origin.map(|n| n / 2);
        let next_size = [0, 1].map(|i| (origin[i] + size[i]).div_ceil(2) - next_origin[i]);
        self.next.get_or_insert_with(|| Image::new(r,
            display_mips::Plan::window(self.plan.extent, self.plan.level + 1, self.plan.bounds), "adjacent composition level"));
        let binding = Commands::binding(r, self.view(), &r.empty_view, self.next_view());
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[
            next_origin[0],
            next_origin[1],
            next_size[0],
            next_size[1],
            self.plan.bounds.width(),
            self.plan.bounds.height(),
            2,
            (self.plan.level << 8) | 8,
        ]);
        let offset = commands.record(r, encoder, values)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce composed display for trilinear presentation"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.reduce_pair);
        pass.set_bind_group(0, &commands.record_binding, &[offset]);
        pass.set_bind_group(1, &binding, &[]);
        pass.dispatch_workgroups(next_size[0].div_ceil(8), next_size[1].div_ceil(8), 1);
        drop(pass);
        self.ready = true;
        Ok(())
    }
}

struct Presentation {
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
enum Slot { Cache(usize), Scene(usize), Decoded(Arc<()>) }

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
    tiled: bool,
}
impl Evaluator<'_> {
    fn working_plan(&self) -> display_mips::Plan {
        if self.tiled {
            let span = PAGE_SIZE << self.cache.plan.level;
            let [x, y] = [self.region.min_x() / span * span, self.region.min_y() / span * span];
            display_mips::Plan::window(self.cache.plan.extent, self.cache.plan.level, PixelRect::new(x, y, x + span, y + span))
        } else { self.cache.plan }
    }
    fn target(&mut self) -> Target {
        let plan = self.working_plan();
        if self.tiled {
            let slot = self.scene.reserve(self.r);
            Target { view: self.scene.pool[slot].view.clone(), slot: Some(Slot::Scene(slot)), plan }
        } else {
            let slot = self.cache.allocate(self.r);
            Target { view: self.cache.output[slot].view.clone(), slot: Some(Slot::Cache(slot)), plan }
        }
    }
    fn release(&mut self, slot: Option<Slot>) {
        match slot {
            Some(Slot::Cache(slot)) => self.cache.used[slot] = false,
            Some(Slot::Scene(slot)) => self.scene.free(slot),
            Some(Slot::Decoded(lease)) => drop(lease),
            None => {}
        }
    }
    fn texels(&self, plan: display_mips::Plan) -> [u32; 4] {
        paint_transform::texel_rect(self.region.window_local(plan.bounds), 1 << plan.level)
    }
    fn source(&mut self, id: LayerId, placement: layer_core::Affine, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        let encode = self.packet.blend_space == layer_core::BlendSpace::Perceptual
            && self.packet.layers.iter().any(|layer| layer.id == id);
        if self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) {
            return Ok(Value::Transform(TransformSource { id, placement, opacity: 1., backdrop: [0.; 4], encode }));
        }
        let plan = source_plan(self.cache.plan, placement, extent)?;
        if self.cache.plan.level == 0 && plan.level == 0 {
            let tile = [self.region.min_x() / PAGE_SIZE, self.region.min_y() / PAGE_SIZE];
            let slot = if let Some(index) = self.packet.layers.iter().position(|l| l.id == id) {
                let layer = &self.packet.layers[index];
                let stored = self.r.paint_layers.iter().find(|l| l.id == id);
                if placement == layer_core::Affine::IDENTITY && stored.is_none_or(|s| s.watercolor.is_none()) {
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
        if self.cache.streamed_sources && placement == layer_core::Affine::IDENTITY {
            let cached = self.scene.scale_sources.entries[&id].levels.get(&plan.level).filter(|level|
                self.region.intersect(level.image.plan.bounds) == self.region
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
                self.scene.reduce_color_pages(self.commands, self.r, self.packet, self.encoder, layer, target.plan, &target.view, &pages)?;
            } else {
                let mask = self.packet.layers.iter().find_map(|l| l.mask.as_ref().filter(|m| m.id == id)).unwrap();
                self.scene.reduce_mask_pages(self.commands, self.r, self.encoder, mask, target.plan, &target.view, &pages)?;
            }
            return Ok(target.value().with_encoding(encode));
        }
        let image = &self.scene.scale_sources.image(id, plan.level).image;
        let plan = image.plan;
        let view = image.view.clone();
        if placement == layer_core::Affine::IDENTITY && plan.level == self.cache.plan.level
            && (outside == 0. || self.region.intersect(plan.bounds) == self.region) {
            return Ok(Value::Image { view, slot: None, opacity: 1., plan, preview: None, encode });
        }
        let placement = layer_core::Affine::translation(layer_core::Point { x: plan.bounds.min_x() as f32, y: plan.bounds.min_y() as f32 }).then(placement);
        let transform = paint_transform::resample_map(&layer_core::ImageTransform::default(), placement, plan.level, self.cache.plan.level)?;
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
    fn resample(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, None); }
        let Value::Placed(placed) = value else { return Ok(value); };
        let Target { view, slot, plan } = self.target();
        let texels = self.texels(plan);
        let values = placed.record(plan, texels)?;
        let offset = self.commands.write(self.r, self.encoder, &values)?;
        let resample = &self.r.scene_pipelines.resample;
        let binding = resample.binding(&self.r.device, &self.commands.records, u64::from(offset), [&placed.view, &view, &placed.view]);
        resample.encode(self.encoder, &binding, texels, scene::resample::Sampling::AffineArea);
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
    fn materialize(&mut self, value: Value, output: Option<Target>) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, output); }
        let value = self.resample(value)?;
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
                if output.is_none() && blend == layer_core::LayerBlend::Normal && flags == 0 => {
                let alpha = placed.backdrop[3];
                placed.backdrop = std::array::from_fn(|i| placed.backdrop[i] + color[i] * (1. - alpha));
                return Ok(Value::Placed(placed));
            }
            pair => pair,
        };
        let front = self.resample(front)?;
        let front = if matches!(front, Value::Color(c) if c != [0.; 4]) { self.materialize(front, None)? } else { front };
        let back = self.resample(back)?;
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
        self.commands.compose(self.r, self.encoder, Composition { values, binding, leases })?;
        for slot in [front.slot(), back.slot()] { self.release(slot); }
        Ok(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false })
    }
}
