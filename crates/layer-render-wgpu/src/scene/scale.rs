//! Demand-driven presentation composition. Exact paint and exact query scenes
//! own document pixels; this cache owns reduced presentation levels only.
//! Unsupported stacks retain the exact compositor. There is no settling queue.
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
                NonZeroU64::new(64),
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
                size: NonZeroU64::new(64),
            })],
        );
        Self { records, record_binding, stride, cursor: 0 }
    }
    pub fn begin(&mut self) { self.cursor = 0; }
    pub fn storage_bytes(&self) -> u64 { self.records.size() }
    pub(super) fn binding(
        r: &WgpuRasterizer,
        source: &wgpu::TextureView,
        base: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        crate::bindings::group(
            &r.device,
            "display composition images",
            &r.scene_pipelines.scale.inputs,
            [
                wgpu::BindingResource::TextureView(source),
                wgpu::BindingResource::TextureView(base),
                wgpu::BindingResource::TextureView(output),
            ],
        )
    }
    pub(super) fn record(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        values: [u32; 16],
    ) -> Result<u32, GpuRasterError> {
        let mut bytes = [0; 64];
        for (dst, value) in bytes.chunks_exact_mut(4).zip(values) {
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
            let mut grown = Self::with_capacity(r, size);
            encoder.copy_buffer_to_buffer(&self.records, 0, &grown.records, 0, offset);
            grown.cursor = self.cursor;
            *self = grown;
        }
        let offset = offset as u32;
        self.cursor += 1;
        r.uploads.write_at(encoder, &self.records, u64::from(offset), bytes)?;
        Ok(offset)
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
    let bytes = allocation(r, plan, packet, r.scene.as_ref().map(|s| &s.scale_sources)).into_iter().sum::<u64>();
    if bytes > live_display::CACHE_BYTES
        || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
    {
        return None;
    }
    let fits = transform_plans(r, plan, packet.layers).all(|(source, _)| source.size.iter()
        .all(|size| *size <= r.device.limits().max_texture_dimension_2d)) && targets(r, packet).all(|(layer, id)| {
        plan.level == 0 || source_plan(plan, layer_core::target_transform(packet.layers, id), layer.local_extent(plan.extent))
            .is_ok_and(|source| source.size.iter().all(|size| *size <= r.device.limits().max_texture_dimension_2d))
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
    if level > 0 { return Some(display_mips::Plan::at(extent, level)); }
    let visible = display_mips::view_bounds(packet.view, extent, 2).ok()?;
    if visible.is_empty() { return display_mips::Plan::new(extent).ok(); }
    let bounds = paint_transform::aligned(visible.expand(PAGE_SIZE, extent), PAGE_SIZE, extent);
    Some(display_mips::Plan::window(extent, level, bounds))
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
fn targets<'a>(r: &'a WgpuRasterizer, packet: FramePacket<'a>) -> impl Iterator<Item = (&'a Layer, LayerId)> {
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
    let layers = packet.layers;
    let images = if plan.level == 0 { 1 } else { scratch_images(layers) };
    let (source_bytes, root_mips) = if plan.level == 0 { (0, 0) } else { targets(r, packet).fold((0, 0), |(sum, largest), (layer, id)| {
        let Ok(source) = source_plan(plan, layer_core::target_transform(layers, id), layer.local_extent(plan.extent)) else { return (sum, largest); };
        let source = sources.map_or(source, |s| s.resident_plan(id, source));
        let extra = source.level_bytes(source.level + 1) + source.level_bytes(source_coarse_level(source));
        (sum + source.level_bytes(source.level), largest.max(extra))
    }) };
    let records = records_for(r, plan, layers).next_power_of_two().max(
        r.scene.as_ref().and_then(|scene| scene.scale_commands.as_ref()).map_or(0, Commands::storage_bytes));
    let transform = transform_plans(r, plan, layers).map(|(source, kept)| {
            let records = page_coordinates(PixelRect::full(source.extent)).count() as u64 * 512;
            (source.pixel_bytes_through(display_mips::MAX_LEVEL) + records * u64::from(display_mips::MAX_LEVEL))
                * if kept { 2 } else { 1 }
        }).sum::<u64>();
    let own = [source_bytes, plan.level_bytes(plan.level) * images + root_mips + plan.level_bytes(plan.level + 1) + records + 64 + transform];
    if plan.bounds == PixelRect::full(plan.extent) { own }
    else {
        let overview = allocation(r, display_mips::Plan::new(plan.extent).unwrap(), packet, sources);
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
        };
        let unchanged = unchanged && !packet.reset_layers && packet.restore_rasters.is_empty();
        let Some(mut old) = previous else {
            return (Self::new(r, plan, packet.layers.len()), true);
        };
        let unchanged = unchanged && old.transform == r.transform_preview;
        old.transform = r.transform_preview.clone();
        if !unchanged {
            old.spare = None;
        }
        if matches(&old) {
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
        if next.overview.is_some() && old.plan.extent == plan.extent
            && let Some(mut overview) = old.overview.take() {
                overview.unchanged = unchanged;
                overview.reuse_output = unchanged && overview.ready;
                next.overview = Some(overview);
        }
        let bound = allocation(r, plan, packet, r.scene.as_ref().map(|s| &s.scale_sources)).into_iter().sum::<u64>();
        if unchanged && old.placed.is_none() && old.plan.extent == plan.extent && bound + old.storage_bytes() <= live_display::CACHE_BYTES {
            if old.plan.level == plan.level && plan.level == 0 && old.ready {
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
            .then(|| Box::new(Self::new(r, display_mips::Plan::new(plan.extent).unwrap(), layers)));
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
            ready: false,
            reuse_output: false,
            overview, shifted: None, valid: BTreeSet::new(), unchanged: false, graph: Default::default(), transform: r.transform_preview.clone(),
        }
    }
    pub fn preview_level(&self, packet: FramePacket<'_>, id: LayerId) -> u32 {
        if self.plan.level == 0 { return 0; }
        let placement = layer_core::target_transform(packet.layers, id);
        source_level(self.plan.level, placement).min(crate::preview_block(
            placement.then(layer_core::Affine(packet.view.document_to_surface)).0).ilog2())
    }
    pub fn source_levels(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources) -> BTreeMap<LayerId, BTreeMap<u32, display_mips::Plan>> {
        let plans = std::iter::once(self.plan).chain(self.overview.as_ref().map(|c| c.plan)).filter(|p| p.level > 0);
        let mut requested: BTreeMap<_, BTreeMap<_, _>> = targets(r, packet).filter(|(_, id)| !r.transforms.as_ref().is_some_and(|t| t.display_source(*id))).map(|(layer, id)| {
            let placement = layer_core::target_transform(packet.layers, id);
            (id, plans.clone().filter_map(|p| source_plan(p, placement, layer.local_extent(p.extent)).ok())
                .filter(|p| !p.bounds.is_empty()).map(|p| (p.level, sources.resident_plan(id, p))).collect())
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
        let reserve = allocation(r, self.plan, packet, sources)[1]
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes());
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let branches = self.graph.reserved_bytes(self.plan)
            + self.overview.as_ref().map_or(0, |c| c.graph.reserved_bytes(c.plan));
        live_display::CACHE_BYTES.saturating_sub((reserve + retained_records + branches).max(self.storage_bytes() + commands.storage_bytes()))
    }

    pub fn prepare_graph(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, commands: &Commands) -> Result<(), GpuRasterError> {
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        let budget = live_display::CACHE_BYTES.saturating_sub(allocation(r, self.plan, packet, Some(sources)).into_iter().sum::<u64>()
            + retained_records + self.spare.as_ref().map_or(0, |c| c.storage_bytes()) + self.shifted.as_ref().map_or(0, |c| c.storage_bytes()));
        let cache = if let Some(overview) = &mut self.overview { overview.as_mut() } else { self };
        if cache.plan.level > 0 { cache.graph.prepare(packet, sources, cache.plan, budget)?; }
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
            root.value.opacity, 1., root.value.outside, 0., r, g, b, a]
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
        dirty: PixelRect, encoding: &mut Encoding<'_>,
        tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<(), GpuRasterError> {
        if self.plan.level != 0 { return self.render_reduced(scene, r, packet, dirty, encoding, None); }
        let dirty = if self.unchanged { PixelRect::EMPTY } else { dirty };
        let mut changed = PixelRect::EMPTY;
        if !self.reuse_output {
            changed = self.render_exact(scene, r, packet, dirty, encoding.encoder, tiles)?;
            self.reduce_output(r, encoding.encoder, changed, encoding.commands)?;
        }
        if let Some(mut overview) = self.overview.take() {
            let changed = if changed.is_empty() { changed } else { PixelRect::new(
                self.plan.bounds.min_x() + changed.min_x(), self.plan.bounds.min_y() + changed.min_y(),
                self.plan.bounds.min_x() + changed.max_x(), self.plan.bounds.min_y() + changed.max_y(),
            ) };
            let result = overview.render_reduced(scene, r, packet, dirty, encoding, Some((self, changed)));
            self.overview = Some(overview);
            result?;
        }
        Ok(())
    }

    fn render_reduced(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, finer: Option<(&Cache, PixelRect)>,
    ) -> Result<(), GpuRasterError> {
        let Encoding { encoder, commands } = encoding;
        if self.reuse_output { return Ok(()); }
        let dirty = if self.unchanged { PixelRect::EMPTY } else { dirty };
        let covered = finer.map_or(PixelRect::EMPTY, |(cache, _)| cache.plan.bounds);
        let visible: Vec<_> = packet
            .layers
            .iter()
            .rev()
            .filter(|l| images::visible(packet.layers, l) && l.kind == LayerKind::Paint && l.is_artwork() && stack::has_content(r, l))
            .collect();
        let mut changed = if self.ready && self.placed.is_none() { dirty } else { self.plan.bounds };
        let root = self.graph.root.clone().expect("prepared composition graph");
        changed = changed.union(root.damage(&scene.scale_sources, self.plan));
        let required = if scene.scale_sources.reset { self.plan.bounds } else { root.required(changed, self.plan).union(changed) };
        let source_covered = if packet.layers.iter().any(|l| l.effect.as_ref().is_some_and(|e| !e.program.passes.is_empty())) { PixelRect::EMPTY } else { covered };
        for layer in &visible {
            if r.transforms.as_ref().is_some_and(|t| t.display_source(layer.id)) { continue; }
            let placement = layer_core::target_transform(packet.layers, layer.id);
            let extent = layer.local_extent(packet.document_extent);
            let plan = scene.scale_sources.resident_plan(layer.id, source_plan(self.plan, placement, extent)?);
            let (needed, covered) = local_regions(required, source_covered, placement, extent, plan.level)?;
            let needed = if placement == layer_core::Affine::IDENTITY { needed } else { plan.bounds };
            let updated = scene.prepare_scale_color(commands, r, packet, encoder, layer, SourceRequest { plan, required: needed, covered })?;
            if !updated.is_empty() { changed = changed.union(pixel_rect(placement.bounds(updated.to_rect()), packet.document_extent)); }
        }
        for layer in packet.layers {
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (m.show_area || images::visible(packet.layers, layer))) {
                let placement = layer_core::target_transform(packet.layers, mask.id);
                let extent = layer.local_extent(packet.document_extent);
                let plan = scene.scale_sources.resident_plan(mask.id, source_plan(self.plan, placement, extent)?);
                let (needed, covered) = local_regions(required, source_covered, placement, extent, plan.level)?;
                let needed = if placement == layer_core::Affine::IDENTITY { needed } else { plan.bounds };
                let updated = scene.prepare_scale_mask(commands, r, encoder, mask, SourceRequest { plan, required: needed, covered })?;
                if !updated.is_empty() { changed = changed.union(pixel_rect(placement.bounds(updated.to_rect()), packet.document_extent)); }
            }
        }
        changed = paint_transform::aligned(changed, PAGE_SIZE, self.plan.extent);
        let side = 1 << self.plan.level;
        let mut written = PixelRect::EMPTY;
        for region in changed.subtract(covered).into_iter().filter(|r| !r.is_empty()) {
            let [x, y, width, height] = paint_transform::texel_rect(region, side);
            self.used.fill(false);
            let mut compositor = Reduced { cache: self, commands, scene, packet, r, encoder, origin: [x, y], size: [width, height] };
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
                    return Ok(());
                }
                output => output,
            };
            compositor.cache.placed = None;
            let output = compositor.materialize(output, None)?;
            compositor.cache.selected = output.slot().unwrap();
            written = written.union(PixelRect::new(x, y, x + width, y + height));
            r.metrics.composited_pixels += u64::from(width) * u64::from(height);
        }
        if let Some((finer, changed)) = finer {
            let region = if self.ready { changed } else { covered };
            if !region.is_empty() {
                if self.output.is_empty() { self.selected = self.allocate(r); }
                let [x, y, width, height] = paint_transform::texel_rect(region, side);
                let input_level = finer.plan.level + 1;
                let mut values = [0; 16];
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
            }
        }
        self.reduce_output(r, encoder, written, commands)
    }

    fn render_exact(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoder: &mut crate::submission::CommandEncoder, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let bounds = self.plan.bounds;
        let mut changed = PixelRect::EMPTY;
        if let Some(old) = self.shifted.take() {
            let overlap = bounds.intersect(old.plan.bounds);
            if !overlap.is_empty() {
                let source = overlap.window_local(old.plan.bounds);
                let target = overlap.window_local(bounds);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: source.min_x(), y: source.min_y(), z: 0 },
                        ..old.texture().as_image_copy() },
                    wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: target.min_x(), y: target.min_y(), z: 0 },
                        ..self.output[0].texture.as_image_copy() },
                    wgpu::Extent3d { width: overlap.width(), height: overlap.height(), depth_or_array_layers: 1 },
                );
                self.valid.extend(old.valid.iter().copied().filter(|c| !page_rect(*c).intersect(overlap).is_empty()));
                changed = overlap;
            }
        }
        self.valid.retain(|c| page_rect(*c).intersect(dirty).is_empty() || tiles.is_some_and(|tiles| !tiles.contains(c)));
        let missing: Vec<_> = page_coordinates(bounds).filter(|c| !self.valid.contains(c)).collect();
        scene.placement_display = true;
        scene.jobs.clear();
        scene.used.fill(false);
        for chunk in missing.chunks(32) {
            for &coordinate in chunk {
                let first = scene.jobs.len();
                let output = scene.display_tile(r, packet, coordinate)?;
                let local = [coordinate[0] - bounds.min_x() / PAGE_SIZE, coordinate[1] - bounds.min_y() / PAGE_SIZE];
                if scene.compose_tile_direct(r, first, output, local, self.plan.size, &self.output[0].view, false) {
                    scene.free(output);
                } else {
                    scene.copy_window_tile(output, &self.output[0].texture, coordinate, bounds);
                }
                changed = changed.union(page_rect(coordinate).intersect(bounds));
                if scene.jobs.len() >= DISPLAY_JOBS_PER_SUBMISSION {
                    scene.group_display_decodes();
                    scene.encode_jobs(r, encoder)?;
                }
            }
            scene.group_display_decodes();
            scene.encode_jobs(r, encoder)?;
            self.valid.extend(chunk);
        }
        self.selected = 0;
        r.metrics.composited_pixels += missing.iter().map(|c| page_rect(*c).intersect(bounds).area()).sum::<u64>();
        Ok(changed.window_local(bounds))
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
        let mut values = [0; 16];
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
}

impl Placed {
    fn record(&self, output: display_mips::Plan, texels: [u32; 4]) -> Result<[u8; 224], GpuRasterError> {
        let side = 1 << output.level;
        let scale = side as f32;
        scene::resample::Resample::values(scene::resample::Request {
            moved: &self.transform, kept: &self.transform,
            clip: layer_core::Affine([scale, 0., 0., scale, 0., 0.]), extent: output.extent, texels,
            display: pixel_transform::DisplayLevel { side, opacity: self.opacity, extent: output.extent, backdrop: self.backdrop },
            source: self.plan, max_lod: 0, outside: self.outside, keep_source: false, identity: false,
        })
    }
}

struct TransformSource { id: LayerId, placement: layer_core::Affine, opacity: f32, backdrop: [f32; 4] }

enum Value {
    Color([f32; 4]),
    Image { view: wgpu::TextureView, slot: Option<usize>, opacity: f32 },
    Placed(Placed),
    Transform(TransformSource),
}
impl Value {
    fn slot(&self) -> Option<usize> { if let Self::Image { slot, .. } = self { *slot } else { None } }
    fn view(&self) -> Option<&wgpu::TextureView> { if let Self::Image { view, .. } = self { Some(view) } else { None } }
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
struct Reduced<'a> {
    cache: &'a mut Cache,
    commands: &'a mut Commands,
    scene: &'a mut Scene,
    packet: FramePacket<'a>,
    r: &'a mut WgpuRasterizer,
    encoder: &'a mut crate::submission::CommandEncoder,
    origin: [u32; 2],
    size: [u32; 2],
}
impl Reduced<'_> {
    fn source(&mut self, id: LayerId, placement: layer_core::Affine, extent: [u32; 2], outside: f32) -> Result<Value, GpuRasterError> {
        if self.r.transforms.as_ref().is_some_and(|t| t.display_source(id)) {
            return Ok(Value::Transform(TransformSource { id, placement, opacity: 1., backdrop: [0.; 4] }));
        }
        let plan = source_plan(self.cache.plan, placement, extent)?;
        if plan.bounds.is_empty() { return Ok(Value::Color([outside; 4])); }
        let image = &self.scene.scale_sources.image(id, plan.level).image;
        let plan = image.plan;
        let view = image.view.clone();
        if placement == layer_core::Affine::IDENTITY && plan.bounds.min_x() == 0 && plan.bounds.min_y() == 0 {
            return Ok(Value::Image { view, slot: None, opacity: 1. });
        }
        let placement = layer_core::Affine::translation(layer_core::Point { x: plan.bounds.min_x() as f32, y: plan.bounds.min_y() as f32 }).then(placement);
        let transform = paint_transform::resample_map(&layer_core::ImageTransform::default(), placement, plan.level, self.cache.plan.level)?;
        Ok(Value::Placed(Placed { id, view, transform, plan, outside, opacity: 1., backdrop: [0.; 4] }))
    }
    fn transform(&mut self, source: TransformSource, output: Option<(wgpu::TextureView, Option<usize>)>) -> Result<Value, GpuRasterError> {
        let (view, slot) = output.unwrap_or_else(|| {
            let slot = self.cache.allocate(self.r);
            (self.cache.output[slot].view.clone(), Some(slot))
        });
        let side = 1 << self.cache.plan.level;
        let region = PixelRect::new(self.origin[0] * side, self.origin[1] * side,
            (self.origin[0] + self.size[0]) * side, (self.origin[1] + self.size[1]) * side).intersect(self.cache.plan.bounds);
        let display = pixel_transform::DisplayLevel { side, extent: self.cache.plan.extent, opacity: source.opacity, backdrop: source.backdrop };
        let mut transforms = self.r.transforms.take().unwrap();
        let result = transforms.render_region(self.r, self.encoder, source.id, source.placement, &view, display, region);
        self.r.transforms = Some(transforms);
        result?;
        Ok(Value::Image { view, slot, opacity: 1. })
    }
    fn resample(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, None); }
        let Value::Placed(placed) = value else { return Ok(value); };
        let slot = self.cache.allocate(self.r);
        let view = self.cache.output[slot].view.clone();
        let texels = [self.origin[0], self.origin[1], self.size[0], self.size[1]];
        let values = placed.record(self.cache.plan, texels)?;
        let offset = self.commands.write(self.r, self.encoder, &values)?;
        let resample = &self.r.scene_pipelines.resample;
        let binding = resample.binding(&self.r.device, &self.commands.records, u64::from(offset), [&placed.view, &view, &placed.view]);
        resample.encode(self.encoder, &binding, texels, scene::resample::Sampling::AffineArea);
        Ok(Value::Image { view, slot: Some(slot), opacity: 1. })
    }
    fn materialize(&mut self, value: Value, output: Option<(wgpu::TextureView, Option<usize>)>) -> Result<Value, GpuRasterError> {
        if let Value::Transform(source) = value { return self.transform(source, output); }
        let value = self.resample(value)?;
        if value.slot().is_some() && value.opacity() == 1. && output.as_ref().is_none_or(|(_, slot)| *slot == value.slot()) { return Ok(value); }
        let (front, back) = if matches!(value, Value::Color(_)) { (Value::Color([0.; 4]), value) } else { (value, Value::Color([0.; 4])) };
        self.draw(front, back, layer_core::LayerBlend::Normal, 0, output)
    }
    fn draw(&mut self, front: Value, back: Value, blend: layer_core::LayerBlend, flags: u32, output: Option<(wgpu::TextureView, Option<usize>)>) -> Result<Value, GpuRasterError> {
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
        let (view, slot) = output.unwrap_or_else(|| {
            let slot = self.cache.allocate(self.r);
            (self.cache.output[slot].view.clone(), Some(slot))
        });
        let mut values = [0; 16];
        values[..8].copy_from_slice(&[
            self.origin[0], self.origin[1], self.size[0], self.size[1], 0, 0,
            1 << self.cache.plan.level, if back.view().is_some() { 2 } else { 0 } | flags,
        ]);
        values[8..12].copy_from_slice(&back.color().map(f32::to_bits));
        values[12] = if front.view().is_some() { front.opacity() } else { 0. }.to_bits();
        values[13] = (blend_code(blend, &self.r.device) as f32).to_bits();
        values[14] = back.opacity().to_bits();
        let offset = self.commands.record(self.r, self.encoder, values)?;
        let binding = Commands::binding(self.r, front.view().unwrap_or(&self.r.empty_view), back.view().unwrap_or(&self.r.empty_view), &view);
        let mut pass = self.encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("compose display region"), timestamp_writes: None,
        });
        pass.set_pipeline(&self.r.scene_pipelines.scale.compose);
        pass.set_bind_group(0, &self.commands.record_binding, &[offset]);
        pass.set_bind_group(1, &binding, &[]);
        pass.dispatch_workgroups(self.size[0].div_ceil(8), self.size[1].div_ceil(8), 1);
        drop(pass);
        for slot in [front.slot(), back.slot()].into_iter().flatten() { self.cache.used[slot] = false; }
        Ok(Value::Image { view, slot, opacity: 1. })
    }
}
