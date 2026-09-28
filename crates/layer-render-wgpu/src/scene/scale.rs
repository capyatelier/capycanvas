//! Demand-driven presentation composition. Exact paint and exact query scenes
//! own document pixels; this cache owns reduced presentation levels only.
//! Unsupported stacks retain the exact compositor. There is no settling queue.
use super::*;
use layer_core::raster::{RasterData, RasterPlane, TileKey};
use std::collections::{BTreeSet, HashMap};
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
struct LayerImage {
    image: Image,
    valid: BTreeSet<[u32; 2]>,
    source: Option<Arc<layer_core::color::source::SourceImage>>,
    backing: Option<Arc<RasterData>>,
}

/// Device recipes survive level changes; display cache retirement drops pixels only.
#[derive(Clone)]
pub(crate) struct Pipelines {
    records: wgpu::BindGroupLayout,
    inputs: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
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
            include_str!("scale.wgsl"),
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

pub(crate) struct Cache {
    pub plan: display_mips::Plan,
    layers: HashMap<LayerId, LayerImage>,
    output: [Image; 2],
    next: Image,
    selected: usize,
    pub geometry: wgpu::Buffer,
    records: wgpu::Buffer,
    record_binding: wgpu::BindGroup,
    stride: u32,
    cursor: u32,
    layer_count: usize,
    // One adjacent level may survive unchanged navigation. Artwork changes
    // release it, including its references to exact native tile captures.
    spare: Option<Box<Cache>>,
    seed_from_spare: bool,
    ready: bool,
    reuse_output: bool,
    preview: Option<(LayerId, BTreeSet<[u32; 2]>)>,
}

/// Eligibility is a quality contract, not just a shader capability check.
/// Masks, non-normal blends, effects and transformed sources require their
/// exact dependencies until a scale-aware implementation exists for them.
pub(crate) fn level(r: &WgpuRasterizer, packet: FramePacket<'_>) -> Option<u32> {
    if r.native_edit.is_none() || r.transform_preview.is_some() {
        return None;
    }
    #[cfg(test)]
    if r.test.reference || r.test.exact_display {
        return None;
    }
    let level = display_mips::view_level(packet.view.document_to_surface, 4)?;
    if level == 0 {
        return None;
    }
    let count = packet
        .layers
        .iter()
        .filter(|l| l.visible && l.kind == LayerKind::Paint)
        .count() as u64;
    let records = record_bytes(r, packet.document_extent, packet.layers.len());
    let plan = display_mips::Plan::at(packet.document_extent, level);
    let bytes = plan.level_bytes(level) * (count + 2) + plan.level_bytes(level + 1) + records + 64;
    if bytes > live_display::CACHE_BYTES
        || records > r.device.limits().max_buffer_size.min(u64::from(u32::MAX))
    {
        return None;
    }
    let supported = packet
        .layers
        .iter()
        .all(|l| l.mask.as_ref().is_none_or(|m| !m.enabled || !m.show_area))
        && packet.layers.iter().all(|l| {
            !l.visible
                || !l.is_artwork()
                || (matches!(l.kind, LayerKind::Paint | LayerKind::Background)
                    && l.properties.parent.is_none()
                    && !l.properties.clipped
                    && l.properties.blend == layer_core::LayerBlend::Normal
                    && l.mask.is_none()
                    && l.effect.is_none()
                    && layer_core::target_transform(packet.layers, l.id)
                        == layer_core::Affine::IDENTITY
                    && r.paint_layers
                        .iter()
                        .find(|p| p.id == l.id)
                        .is_none_or(|p| p.watercolor.is_none()))
        })
        && packet
            .dab_batches
            .iter()
            .all(|b| b.style.execution != BrushExecution::Watercolor);
    supported.then_some(level)
}

#[cfg(test)]
mod tests;

fn record_bytes(r: &WgpuRasterizer, extent: [u32; 2], layers: usize) -> u64 {
    let pages = extent
        .map(|n| u64::from(n.div_ceil(PAGE_SIZE)))
        .into_iter()
        .product::<u64>();
    u64::from(
        r.device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(64),
    ) * ((pages + 1) * layers.max(1) as u64 + 1)
}

impl Cache {
    pub fn select(
        previous: Option<Self>,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        level: u32,
        unchanged: bool,
    ) -> (Self, bool) {
        let matches = |cache: &Self| {
            cache.plan.extent == packet.document_extent
                && cache.plan.level == level
                && cache.layer_count == packet.layers.len()
        };
        let Some(mut old) = previous else {
            return (
                Self::new(r, packet.document_extent, level, packet.layers.len()),
                true,
            );
        };
        let unchanged = unchanged && !packet.reset_layers && packet.restore_rasters.is_empty();
        if !unchanged {
            old.spare = None;
            old.seed_from_spare = false;
        }
        if matches(&old) {
            old.reuse_output = unchanged && old.ready;
            return (old, false);
        }
        let reusable = old.spare.take().filter(|cache| matches(cache));
        let mut next = reusable
            .map(|cache| *cache)
            .unwrap_or_else(|| Self::new(r, packet.document_extent, level, packet.layers.len()));
        next.reuse_output = unchanged && next.ready;
        let count = packet
            .layers
            .iter()
            .filter(|l| l.visible && l.kind == LayerKind::Paint)
            .count() as u64;
        let bound = next.plan.level_bytes(level) * (count + 2)
            + next.plan.level_bytes(level + 1)
            + next.records.size()
            + next.geometry.size();
        if unchanged
            && old.plan.extent == packet.document_extent
            && bound + old.storage_bytes() <= live_display::CACHE_BYTES
        {
            next.seed_from_spare = !next.ready && old.plan.level < level;
            next.spare = Some(Box::new(old));
        }
        (next, true)
    }

    pub fn new(r: &WgpuRasterizer, extent: [u32; 2], level: u32, layers: usize) -> Self {
        let plan = display_mips::Plan::at(extent, level);
        let stride = r
            .device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(64);
        let records = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display composition regions"),
            size: record_bytes(r, extent, layers),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
        let mut geometry = [0u32; 16];
        geometry[0] = 1;
        geometry[1] = 1 << level;
        geometry[2] = 1 << level;
        geometry[6..8].copy_from_slice(&plan.size);
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
            layers: HashMap::new(),
            output: [Image::new(r, plan.size), Image::new(r, plan.size)],
            next: Image::new(r, plan.level_size(level + 1)),
            selected: 0,
            geometry,
            records,
            record_binding,
            stride,
            cursor: 0,
            layer_count: layers,
            spare: None,
            seed_from_spare: false,
            ready: false,
            reuse_output: false,
            preview: None,
        }
    }
    pub fn view(&self) -> &wgpu::TextureView {
        &self.output[self.selected].view
    }
    pub fn next_view(&self) -> &wgpu::TextureView {
        &self.next.view
    }
    pub fn storage_bytes(&self) -> u64 {
        self.output
            .iter()
            .map(|i| texture_bytes(&i.texture))
            .sum::<u64>()
            + self
                .layers
                .values()
                .map(|l| texture_bytes(&l.image.texture))
                .sum::<u64>()
            + self.records.size()
            + self.geometry.size()
            + texture_bytes(&self.next.texture)
            + self.spare.as_ref().map_or(0, |cache| cache.storage_bytes())
    }
    fn binding(
        &self,
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
    fn record(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        values: [u32; 16],
    ) -> Result<u32, GpuRasterError> {
        let offset = self.cursor * self.stride;
        self.cursor += 1;
        let mut bytes = [0; 64];
        for (dst, value) in bytes.chunks_exact_mut(4).zip(values) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        r.uploads
            .write_at(encoder, &self.records, u64::from(offset), &bytes)?;
        Ok(offset)
    }
    pub fn render(
        &mut self,
        scene: &mut Scene,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if self.reuse_output {
            return Ok(());
        }
        self.cursor = 0;
        if self.seed_from_spare {
            // Coarsening unchanged content needs only already-reduced pixels.
            // Keep the finer image for the return trip across this boundary.
            let finer = self.spare.take().unwrap();
            for (id, layer) in &finer.layers {
                let image = Image::new(r, self.plan.size);
                let binding = self.binding(r, &layer.image.view, &r.empty_view, &image.view);
                let mut values = [0; 16];
                values[..8].copy_from_slice(&[
                    0,
                    0,
                    self.plan.size[0],
                    self.plan.size[1],
                    self.plan.extent[0],
                    self.plan.extent[1],
                    1 << (self.plan.level - finer.plan.level),
                    finer.plan.level << 8,
                ]);
                let offset = self.record(r, encoder, values)?;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("derive coarser display layer"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&r.scene_pipelines.scale.reduce);
                pass.set_bind_group(0, &self.record_binding, &[offset]);
                pass.set_bind_group(1, &binding, &[]);
                pass.dispatch_workgroups(
                    self.plan.size[0].div_ceil(8),
                    self.plan.size[1].div_ceil(8),
                    1,
                );
                drop(pass);
                self.layers.insert(
                    *id,
                    LayerImage {
                        image,
                        valid: layer.valid.clone(),
                        source: layer.source.clone(),
                        backing: layer.backing.clone(),
                    },
                );
            }
            self.spare = Some(finer);
            self.seed_from_spare = false;
        }
        let visible: Vec<_> = packet
            .layers
            .iter()
            .rev()
            .filter(|l| l.visible && l.kind == LayerKind::Paint && l.is_artwork())
            .collect();
        self.layers
            .retain(|id, _| visible.iter().any(|l| l.id == *id));
        if let Some((id, tiles)) = self.preview.take()
            && let Some(layer) = self.layers.get_mut(&id)
        {
            layer.valid.retain(|c| !tiles.contains(c));
        }
        let mut changed = dirty;
        for layer in &visible {
            let cached = self.layers.entry(layer.id).or_insert_with(|| LayerImage {
                image: Image::new(r, self.plan.size),
                valid: BTreeSet::new(),
                source: layer.source.clone(),
                backing: None,
            });
            let same_source = match (&cached.source, &layer.source) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if packet.reset_layers || !same_source {
                cached.valid.clear();
            }
            cached.source = layer.source.clone();
            let backing = r.native_backing(layer.id).cloned();
            cached.valid.retain(|&coordinate| {
                let key = TileKey {
                    plane: RasterPlane::Color,
                    coordinate,
                };
                match (
                    cached.backing.as_ref().and_then(|b| b.tiles.get(&key)),
                    backing.as_ref().and_then(|b| b.tiles.get(&key)),
                ) {
                    (Some(a), Some(b)) => a.same_capture(b),
                    (None, None) => true,
                    _ => false,
                }
            });
            cached.backing = backing;
            for batch in packet.dab_batches.iter().filter(|b| b.layer_id == layer.id) {
                for c in page_coordinates(batch_pixel_rect(batch, packet.document_extent)) {
                    cached.valid.remove(&c);
                }
            }
            let output = cached.image.view.clone();
            let missing: Vec<_> = page_coordinates(PixelRect::full(packet.document_extent))
                .filter(|c| !cached.valid.contains(c))
                .collect();
            for chunk in missing.chunks(32) {
                let mut jobs = Vec::with_capacity(chunk.len());
                for &tile in chunk {
                    let persistent = r
                        .paint_layers
                        .iter()
                        .find(|l| l.id == layer.id)
                        .and_then(|l| l.pages.iter().find(|p| p.coordinate == tile))
                        .map(|p| p.active().view.clone());
                    let predicted = (r.preview_layer_id == Some(layer.id))
                        .then(|| r.preview_page(tile))
                        .flatten()
                        .map(|p| p.active().view.clone());
                    let reduced_preview = predicted.is_some() && r.preview_level > 0;
                    let base = if predicted.is_some() && r.preview_requires_base {
                        None
                    } else if persistent.is_some() {
                        persistent
                    } else {
                        scene.source_tile(r, layer, tile)?
                    };
                    let over = predicted.is_some() && !r.preview_requires_base;
                    let empty = predicted.is_none() && base.is_none();
                    let source = predicted
                        .or_else(|| base.clone())
                        .unwrap_or_else(|| r.empty_view.clone());
                    let base = base.unwrap_or_else(|| r.empty_view.clone());
                    let valid = page_rect(tile).intersect(PixelRect::full(packet.document_extent));
                    changed = changed.union(valid);
                    let size =
                        [valid.width(), valid.height()].map(|n| n.div_ceil(1 << self.plan.level));
                    let origin = tile.map(|n| (n * PAGE_SIZE) >> self.plan.level);
                    let input_level = if reduced_preview { r.preview_level } else { 0 };
                    let mut values = [0; 16];
                    values[..8].copy_from_slice(&[
                        origin[0],
                        origin[1],
                        size[0],
                        size[1],
                        valid.width(),
                        valid.height(),
                        1 << (self.plan.level - input_level),
                        u32::from(over) | if empty { 4 } else { 0 } | (input_level << 8),
                    ]);
                    let offset = self.record(r, encoder, values)?;
                    jobs.push((self.binding(r, &source, &base, &output), offset, size));
                }
                // Decode batches stay bounded by source-cache ownership. All
                // reductions consume their sources before the next reuse.
                scene.encode_jobs(r, encoder)?;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("reduce changed paint pages"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&r.scene_pipelines.scale.reduce);
                for (binding, offset, size) in &jobs {
                    pass.set_bind_group(0, &self.record_binding, &[*offset]);
                    pass.set_bind_group(1, binding, &[]);
                    pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
                }
                drop(pass);
                self.layers.get_mut(&layer.id).unwrap().valid.extend(chunk);
            }
        }
        if let Some(id) = r.preview_layer_id {
            self.preview = Some((
                id,
                r.preview_contact_tiles
                    .clone()
                    .unwrap_or_else(|| page_coordinates(r.preview_damage).collect()),
            ));
        }
        let side = 1 << self.plan.level;
        let origin = [changed.min_x() / side, changed.min_y() / side];
        let size = [
            changed.max_x().div_ceil(side) - origin[0],
            changed.max_y().div_ceil(side) - origin[1],
        ];
        if size.contains(&0) {
            return Ok(());
        }
        let paper = packet.view.background_rgba_linear;
        for index in 0..visible.len().max(1) {
            let layer = visible.get(index);
            let source = layer.map_or(&r.empty_view, |l| &self.layers[&l.id].image.view);
            let target = index % 2;
            let binding = self.binding(
                r,
                source,
                &self.output[1 - target].view,
                &self.output[target].view,
            );
            let mut values = [0; 16];
            values[..8].copy_from_slice(&[
                origin[0],
                origin[1],
                size[0],
                size[1],
                0,
                0,
                side,
                if index > 0 { 2 } else { 0 },
            ]);
            for i in 0..4 {
                values[8 + i] = (paper[i] * if i < 3 { paper[3] } else { 1. }).to_bits();
            }
            values[12] = layer.map_or(0., |l| l.opacity).to_bits();
            let offset = self.record(r, encoder, values)?;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("compose display region"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&r.scene_pipelines.scale.compose);
            pass.set_bind_group(0, &self.record_binding, &[offset]);
            pass.set_bind_group(1, &binding, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
            self.selected = target;
        }
        r.metrics.composited_pixels += u64::from(size[0]) * u64::from(size[1]);
        // Presentation interpolates between adjacent output levels. This
        // small derived image replaces sixteen samples per screen pixel.
        let next_origin = origin.map(|n| n / 2);
        let next_size = [0, 1].map(|i| (origin[i] + size[i]).div_ceil(2) - next_origin[i]);
        let binding = self.binding(r, self.view(), &r.empty_view, &self.next.view);
        let mut values = [0; 16];
        values[..8].copy_from_slice(&[
            next_origin[0],
            next_origin[1],
            next_size[0],
            next_size[1],
            self.plan.extent[0],
            self.plan.extent[1],
            2,
            (self.plan.level << 8) | 8,
        ]);
        let offset = self.record(r, encoder, values)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce composed display for trilinear presentation"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.reduce);
        pass.set_bind_group(0, &self.record_binding, &[offset]);
        pass.set_bind_group(1, &binding, &[]);
        pass.dispatch_workgroups(next_size[0].div_ceil(8), next_size[1].div_ceil(8), 1);
        drop(pass);
        self.ready = true;
        Ok(())
    }
}
