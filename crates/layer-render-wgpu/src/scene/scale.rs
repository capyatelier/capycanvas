//! Demand-driven presentation composition. Exact paint and exact query scenes
//! own document pixels; this cache owns reduced presentation levels only.
//! Unsupported stacks retain the exact compositor. There is no settling queue.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;

struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Image {
    fn new(r: &WgpuRasterizer, size: [u32; 2]) -> Self {
        let (texture, view) = create_color_target(&r.device, size, "display composition level");
        Self { texture, view }
    }
}
mod sources;
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
    level: u32,
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
}

pub(crate) fn level(r: &WgpuRasterizer, packet: FramePacket<'_>) -> Option<u32> {
    if r.native_edit.is_none() || r.transform_preview.is_some() {
        return None;
    }
    #[cfg(test)]
    if r.test.reference || r.test.exact_display {
        return None;
    }
    let level = display_mips::view_level(packet.view.document_to_surface, 4)?;
    let plan = request(packet, level)?;
    let records = records_for(r, plan, packet.layers);
    let bytes = allocation(r, plan, packet).into_iter().sum::<u64>();
    if bytes > live_display::CACHE_BYTES
        || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
    {
        return None;
    }
    let fits = targets(r, packet).all(|(layer, id)| {
        let local = source_level(plan.level, layer_core::target_transform(packet.layers, id));
        plan.level == 0 || display_mips::Plan::at(layer.local_extent(plan.extent), local).size.iter()
            .all(|size| *size <= r.device.limits().max_texture_dimension_2d)
    });
    let supported = fits && packet.layers.iter().all(|l| {
            !l.visible
                || !l.is_artwork()
                || (matches!(l.kind, LayerKind::Paint | LayerKind::Background | LayerKind::Group)
                    && l.effect.is_none()
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

fn source_level(level: u32, placement: layer_core::Affine) -> u32 {
    paint_transform::local_level(level, placement).saturating_sub(u32::from(placement != layer_core::Affine::IDENTITY))
}

pub(super) fn placement_level(layers: &[Layer], id: LayerId) -> u32 {
    display_mips::view_level(layer_core::target_transform(layers, id).0, 8).unwrap_or(0)
}

fn local_regions(required: PixelRect, covered: PixelRect, placement: layer_core::Affine, extent: [u32; 2], level: u32) -> Result<(PixelRect, PixelRect), GpuRasterError> {
    if placement == layer_core::Affine::IDENTITY { return Ok((required.intersect(PixelRect::full(extent)), covered)); }
    let inverse = placement.inverse().ok_or(GpuRasterError::InvalidTransform("Transform must be finite and invertible"))?;
    Ok((pixel_rect(inverse.bounds(required.to_rect()), extent).expand(1 << level, extent), PixelRect::EMPTY))
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

fn allocation(r: &WgpuRasterizer, plan: display_mips::Plan, packet: FramePacket<'_>) -> [u64; 2] {
    let layers = packet.layers;
    let images = if plan.level == 0 { 1 } else { scratch_images(layers) };
    let (sources, root_mips) = if plan.level == 0 { (0, 0) } else { targets(r, packet).fold((0, 0), |(sum, largest), (layer, id)| {
        let level = source_level(plan.level, layer_core::target_transform(layers, id));
        let source = display_mips::Plan::at(layer.local_extent(plan.extent), level);
        let extra = source.level_bytes(level + 1) + source.level_bytes(source_coarse_level(source));
        (sum + source.level_bytes(level), largest.max(extra))
    }) };
    let records = records_for(r, plan, layers).next_power_of_two().max(
        r.scene.as_ref().and_then(|scene| scene.scale_commands.as_ref()).map_or(0, Commands::storage_bytes));
    let own = [sources, plan.level_bytes(plan.level) * images + root_mips + plan.level_bytes(plan.level + 1) + records + 64];
    if plan.bounds == PixelRect::full(plan.extent) { own }
    else {
        let overview = allocation(r, display_mips::Plan::new(plan.extent).unwrap(), packet);
        [own[0] + overview[0], own[1] + overview[1]]
    }
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
    3 * (1 + layers.iter().filter(|l| l.kind == LayerKind::Group).count() as u64)
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
        let bound = allocation(r, plan, packet).into_iter().sum::<u64>();
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
            output: (0..if level == 0 { 1 } else { 0 }).map(|_| Image::new(r, plan.size)).collect(),
            used: vec![false; if level == 0 { 1 } else { 0 }],
            next: None,
            selected: 0,
            placed: None,
            geometry,
            layer_count: layers,
            spare: None,
            ready: false,
            reuse_output: false,
            overview, shifted: None, valid: BTreeSet::new(), unchanged: false,
        }
    }
    pub fn preview_level(&self, packet: FramePacket<'_>, id: LayerId) -> u32 {
        if self.plan.level == 0 { return 0; }
        let placement = layer_core::target_transform(packet.layers, id);
        source_level(self.plan.level, placement).min(crate::preview_block(
            placement.then(layer_core::Affine(packet.view.document_to_surface)).0).ilog2())
    }
    pub fn source_levels(&self, r: &WgpuRasterizer, packet: FramePacket<'_>) -> BTreeMap<LayerId, BTreeSet<u32>> {
        let mut levels = BTreeSet::new();
        if self.plan.level > 0 { levels.insert(self.plan.level); }
        if let Some(overview) = &self.overview { levels.insert(overview.plan.level); }
        let mut requested: BTreeMap<_, BTreeSet<_>> = targets(r, packet).map(|(_, id)| {
            let placement = layer_core::target_transform(packet.layers, id);
            (id, levels.iter().map(|level| source_level(*level, placement)).collect())
        }).collect();
        if let Some(root) = &self.placed && let Some(levels) = requested.get_mut(&root.value.id) {
            levels.extend([root.value.plan.level + 1, root.coarse_level]);
        }
        requested
    }
    pub fn source_budget(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, commands: &Commands) -> u64 {
        let reserve = allocation(r, self.plan, packet)[1]
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
            + self.shifted.as_ref().map_or(0, |cache| cache.storage_bytes());
        let retained_records = commands.storage_bytes().saturating_sub(records_for(r, self.plan, packet.layers).next_power_of_two());
        live_display::CACHE_BYTES.saturating_sub((reserve + retained_records).max(self.storage_bytes() + commands.storage_bytes()))
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
        let [width, height] = root.value.plan.extent.map(|n| n as f32 / source_side);
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
    }
    fn allocate(&mut self, r: &WgpuRasterizer) -> usize {
        let slot = self.used.iter().position(|used| !used).unwrap_or_else(|| {
            self.output.push(Image::new(r, self.plan.size));
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
        let required = if scene.scale_sources.reset { self.plan.bounds } else { changed };
        for layer in &visible {
            let placement = layer_core::target_transform(packet.layers, layer.id);
            let level = source_level(self.plan.level, placement);
            let (needed, covered) = local_regions(required, covered, placement, layer.local_extent(packet.document_extent), level)?;
            let updated = scene.prepare_scale_color(commands, r, packet, encoder, layer, SourceRequest { level, required: needed, covered })?;
            changed = changed.union(pixel_rect(placement.bounds(updated.to_rect()), packet.document_extent));
        }
        for layer in packet.layers {
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && (m.show_area || images::visible(packet.layers, layer))) {
                let placement = layer_core::target_transform(packet.layers, mask.id);
                let level = source_level(self.plan.level, placement);
                let extent = layer.local_extent(packet.document_extent);
                let (needed, covered) = local_regions(required, covered, placement, extent, level)?;
                let updated = scene.prepare_scale_mask(commands, r, encoder, mask, SourceRequest { level, required: needed, covered })?;
                changed = changed.union(pixel_rect(placement.bounds(updated.to_rect()), packet.document_extent));
            }
        }
        let side = 1 << self.plan.level;
        let mut written = PixelRect::EMPTY;
        for region in changed.subtract(covered).into_iter().filter(|r| !r.is_empty()) {
            let [x, y, width, height] = paint_transform::texel_rect(region, side);
            self.used.fill(false);
            let mut compositor = Reduced { cache: self, commands, sources: &scene.scale_sources, r, packet, encoder, origin: [x, y], size: [width, height] };
            let output = stack::compose(&mut compositor, packet.layers, None, None)?;
            let output = compositor.inspect_masks(output)?;
            let output = match output {
                Value::Placed(value) if finer.is_none() => {
                    drop(compositor);
                    let coarse_level = source_coarse_level(value.plan);
                    for level in [value.plan.level + 1, coarse_level] {
                        scene.scale_sources.ensure_level(commands, r, encoder, value.id, level)?;
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
            let output = compositor.materialize(output)?;
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
        self.next.get_or_insert_with(|| Image::new(r, self.plan.level_size(self.plan.level + 1)));
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
        paint_transform::resample::Resample::values(paint_transform::resample::Request {
            moved: &self.transform, kept: &self.transform,
            clip: layer_core::Affine([scale, 0., 0., scale, 0., 0.]), extent: output.extent, texels,
            display: pixel_transform::DisplayLevel { side, opacity: self.opacity, extent: output.extent, backdrop: self.backdrop },
            keeps_pixels: false, mesh: false, source: self.plan, outside: self.outside,
        })
    }
}

enum Value {
    Color([f32; 4]),
    Image { view: wgpu::TextureView, slot: Option<usize> },
    Placed(Placed),
}
impl Value {
    fn slot(&self) -> Option<usize> { if let Self::Image { slot, .. } = self { *slot } else { None } }
    fn view(&self) -> Option<&wgpu::TextureView> { if let Self::Image { view, .. } = self { Some(view) } else { None } }
    fn color(&self) -> [f32; 4] { if let Self::Color(color) = self { *color } else { [0.; 4] } }
}
struct Reduced<'a> {
    cache: &'a mut Cache,
    commands: &'a mut Commands,
    sources: &'a Sources,
    r: &'a mut WgpuRasterizer,
    packet: FramePacket<'a>,
    encoder: &'a mut crate::submission::CommandEncoder,
    origin: [u32; 2],
    size: [u32; 2],
}
impl Reduced<'_> {
    fn source(&mut self, layer: &Layer, mask: bool) -> Result<Value, GpuRasterError> {
        let id = if mask { layer.mask.as_ref().unwrap().id } else { layer.id };
        if !self.sources.entries.contains_key(&id) { return Ok(Value::Color([0.; 4])); }
        let placement = layer_core::target_transform(self.packet.layers, id);
        let level = source_level(self.cache.plan.level, placement);
        let view = self.sources.image(id, level).image.view.clone();
        if placement == layer_core::Affine::IDENTITY && self.cache.plan.bounds.min_x() == 0 && self.cache.plan.bounds.min_y() == 0 {
            return Ok(Value::Image { view, slot: None });
        }
        let transform = paint_transform::resample_map(&layer_core::ImageTransform::default(), placement, level, self.cache.plan.level)?;
        let outside = layer.mask.as_ref().filter(|_| mask).map_or(0., |m| if m.inverted { 1. - m.default_coverage } else { m.default_coverage });
        Ok(Value::Placed(Placed { id, view, transform, plan: display_mips::Plan::at(layer.local_extent(self.packet.document_extent), level), outside, opacity: 1., backdrop: [0.; 4] }))
    }
    fn resample(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        let Value::Placed(placed) = value else { return Ok(value); };
        let slot = self.cache.allocate(self.r);
        let view = self.cache.output[slot].view.clone();
        let texels = [self.origin[0], self.origin[1], self.size[0], self.size[1]];
        let values = placed.record(self.cache.plan, texels)?;
        let offset = self.commands.write(self.r, self.encoder, &values)?;
        let resample = self.r.transforms.as_ref().unwrap().resample();
        let binding = resample.binding(&self.r.device, &self.commands.records, u64::from(offset), [&placed.view, &view, &placed.view, &self.r.empty_view]);
        resample.encode(self.encoder, &binding, texels, paint_transform::resample::Sampling::AffineArea);
        Ok(Value::Image { view, slot: Some(slot) })
    }
    fn inspect_masks(&mut self, mut output: Value) -> Result<Value, GpuRasterError> {
        for layer in self.packet.layers {
            if layer.mask.as_ref().is_some_and(|m| m.enabled && m.show_area) {
                let mask = self.source(layer, true)?;
                output = self.draw(mask, output, 1., layer_core::LayerBlend::Normal, 64)?;
            }
        }
        Ok(output)
    }

    fn materialize(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        let value = self.resample(value)?;
        if value.slot().is_some() { return Ok(value); }
        if value.view().is_some() {
            self.draw(value, Value::Color([0.; 4]), 1., layer_core::LayerBlend::Normal, 0)
        } else {
            self.draw(Value::Color([0.; 4]), value, 0., layer_core::LayerBlend::Normal, 0)
        }
    }
    fn draw(&mut self, front: Value, back: Value, opacity: f32, blend: layer_core::LayerBlend, flags: u32) -> Result<Value, GpuRasterError> {
        let (front, back) = match (front, back) {
            (Value::Placed(mut placed), Value::Color(color))
                if blend == layer_core::LayerBlend::Normal && flags == 0 && (placed.backdrop == [0.; 4] || opacity == 1.) => {
                placed.opacity *= opacity;
                let alpha = placed.backdrop[3];
                placed.backdrop = std::array::from_fn(|i| placed.backdrop[i] + color[i] * (1. - alpha));
                return Ok(Value::Placed(placed));
            }
            pair => pair,
        };
        let front = self.resample(front)?;
        let back = self.resample(back)?;
        let slot = self.cache.allocate(self.r);
        let mut values = [0; 16];
        values[..8].copy_from_slice(&[
            self.origin[0], self.origin[1], self.size[0], self.size[1], 0, 0,
            1 << self.cache.plan.level, if back.view().is_some() { 2 } else { 0 } | flags,
        ]);
        values[8..12].copy_from_slice(&back.color().map(f32::to_bits));
        values[12] = if front.view().is_some() { opacity } else { 0. }.to_bits();
        values[13] = (blend as u32 as f32).to_bits();
        let offset = self.commands.record(self.r, self.encoder, values)?;
        let view = self.cache.output[slot].view.clone();
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
        Ok(Value::Image { view, slot: Some(slot) })
    }
}
impl stack::Compositor for Reduced<'_> {
    type Image = Value;
    fn clear(&mut self, paper: bool) -> Value {
        let p = if paper { self.packet.view.background_rgba_linear } else { [0.; 4] };
        Value::Color([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
    }
    fn discard(&mut self, value: Value) {
        if let Some(slot) = value.slot() { self.cache.used[slot] = false; }
    }
    fn layer(&mut self, index: usize) -> Result<Value, GpuRasterError> {
        let layer = &self.packet.layers[index];
        let value = if layer.kind == LayerKind::Group {
            stack::compose(self, self.packet.layers, Some(layer.id), None)?
        } else {
            self.source(layer, false)?
        };
        if layer.mask.as_ref().is_some_and(|mask| mask.enabled) {
            let mask = self.source(layer, true)?;
            self.draw(value, mask, 1., layer_core::LayerBlend::Normal, 32)
        } else { Ok(value) }
    }
    fn blend(&mut self, front: Value, back: Value, index: usize, clipped: bool) -> Result<Value, GpuRasterError> {
        let layer = &self.packet.layers[index];
        self.draw(front, back, layer.opacity, layer.properties.blend, if clipped { 16 } else { 0 })
    }
    fn effect(&mut self, _: &[usize], _: Value) -> Result<Value, GpuRasterError> {
        Err(GpuRasterError::Effect("Effect was not admitted at the requested scale".into()))
    }
    fn has_content(&self, index: usize) -> bool {
        let layer = &self.packet.layers[index];
        layer.kind == LayerKind::Group || self.sources.entries.contains_key(&layer.id)
    }
}
