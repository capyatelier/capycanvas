//! Cached document-coordinate windows for neighborhood and time-aware WGSL.
//! Tiled captures feed the same filter and composition operations at any origin.
use super::metadata::{Metadata, mask_metadata};
use super::*;
use wgpu::util::DeviceExt;

#[cfg(test)]
#[path = "scene/image_grid_tests.rs"]
mod grid_tests;

struct CachedStage {
    id: LayerId,
    input: Image,
    input_owned: bool,
    output: Image,
    mask: Option<Image>,
    mask_offset: layer_core::Point,
    time: f32,
    valid: bool,
    composition: Option<ImageComposition>,
}
struct ClipInput {
    base: usize,
    base_id: LayerId,
    dependencies: Vec<usize>,
    terminal: bool,
}
struct Backdrop {
    image: Image,
    valid: bool,
    updated: bool,
    damage: PixelRect,
}
/// A reusable source-over/blend operation. Its textures, bindings and uniforms
/// persist; animated frames change pixels, not the execution structure.
struct ImageComposition {
    output: Image,
    inputs: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    properties: [f32; 2],
    valid: bool,
}
impl ImageComposition {
    fn new(
        scene: &Scene,
        r: &WgpuRasterizer,
        bounds: PixelRect,
        front: &Image,
        back: &Image,
    ) -> Self {
        let output = Image::new(r, front.plan, "clipping composition cache");
        let inputs = crate::bindings::group(&r.device, "cached clipping composition inputs", &scene.layout, [
            wgpu::BindingResource::TextureView(&front.view),
            wgpu::BindingResource::TextureView(&back.view),
            wgpu::BindingResource::Sampler(&r.sampler),
        ]);
        let mut data = [0f32; 36];
        let [w, h] = [bounds.width(), bounds.height()].map(|v| v as f32);
        data[..6].copy_from_slice(&[0., 0., w, h, w, h]);
        data[8] = 4.;
        let uniform = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cached clipping composition parameters"),
                contents: &data
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let binding = uniform_binding(&r.device, &scene.uniforms, &uniform);
        Self {
            output,
            inputs,
            uniform,
            binding,
            properties: [f32::NAN; 2],
            valid: false,
        }
    }
    #[allow(clippy::too_many_arguments)] // Explicit composite operands.
    fn encode(
        &mut self,
        scene: &Scene,
        r: &WgpuRasterizer,
        base: &Layer,
        region: PixelRect,
        space: layer_core::BlendSpace,
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        let properties = [
            if base.visible { base.opacity } else { 0. },
            crate::blend_code(base.properties.blend, &r.device, space) as f32,
        ];
        if properties != self.properties {
            r.queue.write_buffer(
                &self.uniform,
                9 * 4,
                &properties
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>(),
            );
            self.properties = properties;
        }
        let attachments = [Some(attachment(&self.output.view, wgpu::LoadOp::Load))];
        let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
        pass.set_pipeline(&scene.pipeline[0]);
        pass.set_bind_group(0, &self.binding, &[0]);
        pass.set_bind_group(1, &self.inputs, &[]);
        let region = region.window_local(self.output.plan.bounds);
        pass.set_scissor_rect(
            region.min_x(),
            region.min_y(),
            region.width(),
            region.height(),
        );
        pass.draw(0..3, 0..1);
        self.valid = true;
    }
}
#[derive(Default)]
pub(super) struct ImageStages {
    extent: [u32; 2],
    pub(super) bounds: PixelRect,
    stages: Vec<CachedStage>,
    scratch: Vec<Image>,
    metadata: Vec<Metadata>,
    inputs: Vec<Vec<usize>>,
    clips: Vec<Option<ClipInput>>,
    backdrops: std::collections::HashMap<LayerId, Backdrop>,
    preview_layer: Option<LayerId>,
    background: [f32; 4],
    blend_space: layer_core::BlendSpace,
    pub input_updates: u64,
    pub pass_updates: u64,
    pub pass_pixels: u64,
}
impl ImageStages {
    /// Windowed filters rebuild their images for every dependency window. Once
    /// that window completes, retain only metadata used to classify future
    /// artwork damage; its temporary pixels have no reusable owner.
    pub(super) fn release_window_pixels(&mut self) {
        self.stages.clear();
        self.scratch.clear();
        self.backdrops.clear();
    }
    pub fn checkpoint(
        &self,
        index: usize,
        layer: &Layer,
    ) -> Option<(wgpu::TextureView, Option<(wgpu::TextureView, usize)>)> {
        let stage = self.stages.iter().find(|s| s.id == layer.id && s.valid)?;
        if !layer.properties.clipped {
            return Some((stage.output.view.clone(), None));
        }
        if let Some(c) = &stage.composition
            && c.valid
        {
            return Some((c.output.view.clone(), None));
        }
        let clip = self.clips.get(index)?.as_ref()?;
        let back = self.backdrops.get(&clip.base_id).filter(|b| b.valid)?;
        Some((
            back.image.view.clone(),
            Some((stage.output.view.clone(), clip.base)),
        ))
    }
    pub fn output(&self, id: LayerId) -> Option<wgpu::TextureView> {
        self.stages
            .iter()
            .find(|s| s.id == id && s.valid)
            .map(|s| s.output.view.clone())
    }
    fn views(&self) -> Vec<wgpu::TextureView> {
        self.stages.iter().flat_map(|stage| [
            stage.input_owned.then_some(&stage.input), Some(&stage.output), stage.mask.as_ref(),
            stage.composition.as_ref().map(|composition| &composition.output),
        ]).flatten().chain(&self.scratch).chain(self.backdrops.values().map(|backdrop| &backdrop.image))
            .map(|image| image.view.clone()).collect()
    }
    pub fn storage_bytes(&self) -> u64 {
        self.stages
            .iter()
            .map(|s| {
                u64::from(s.input_owned) * s.input.bytes()
                    + s.output.bytes()
                    + s.mask.as_ref().map_or(0, Image::bytes)
                    + s.composition
                        .as_ref()
                        .map_or(0, |c| c.output.bytes() + c.uniform.size())
            })
            .sum::<u64>()
            + self.scratch.iter().map(Image::bytes).sum::<u64>()
            + self
                .backdrops
                .values()
                .map(|b| b.image.bytes())
                .sum::<u64>()
    }
}

pub(super) fn visible(layers: &[Layer], layer: &Layer) -> bool {
    if !layer.visible {
        return false;
    }
    let mut parent = layer.properties.parent;
    while let Some(id) = parent {
        let Some(l) = layers.iter().find(|l| l.id == id) else {
            return false;
        };
        if !l.visible {
            return false;
        }
        parent = l.properties.parent;
    }
    true
}

/// A conservative dependency closure across isolated groups and clipping
/// stacks. Summing support may include unrelated branches but cannot omit a
/// preceding filter's halo. Document samplers require the entire input.
pub(super) fn capture_window(layers: &[Layer], region: PixelRect, extent: [u32; 2]) -> PixelRect {
    layers
        .iter()
        .filter(|l| visible(layers, l))
        .filter_map(|l| l.effect.as_ref())
        .try_fold(region, |bounds, e| {
            Some(bounds.expand(e.damage_radius()?, extent))
        })
        .unwrap_or(PixelRect::full(extent))
}

// Dependencies follow the same isolated group / clipping-stack boundaries as
// composition, through Pass Through groups. Build on structural edits only,
// not on every dab or frame.
fn input_indices(layers: &[Layer], index: usize) -> Vec<usize> {
    let layer = &layers[index];
    let adjustment = layer
        .effect
        .as_ref()
        .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment);
    if layer.kind == LayerKind::Group {
        return below_indices(layers, index, Some(layer.id), layers.len());
    }
    if !adjustment {
        return Vec::new();
    }
    if !layer.properties.clipped {
        return layer_core::backdrop_layers(layers, index);
    }
    let parent = layer.properties.parent;
    let end = layers
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, l)| l.properties.parent == parent && !l.properties.clipped)
        .map_or(layers.len(), |(i, _)| i);
    below_indices(layers, index, parent, end)
}
/// The group a layer's input is composed in: its clipping stack's group, or
/// the nearest group around it that does not pass through.
pub(super) fn input_scope(layers: &[Layer], layer: &Layer) -> Option<LayerId> {
    if layer.properties.clipped {
        layer.properties.parent
    } else {
        layer_core::isolated_scope(layers, layer.properties.parent)
    }
}
fn below_indices(
    layers: &[Layer],
    index: usize,
    parent: Option<LayerId>,
    end: usize,
) -> Vec<usize> {
    (index + 1..layers.len())
        .filter(|&i| {
            let mut root = i;
            while layers[root].properties.parent != parent {
                let Some(id) = layers[root].properties.parent else {
                    return false;
                };
                let Some(next) = layers.iter().position(|l| l.id == id) else {
                    return false;
                };
                root = next;
            }
            root > index && root <= end
        })
        .collect()
}

fn clip_input(layers: &[Layer], index: usize) -> Option<ClipInput> {
    let layer = &layers[index];
    if !visible(layers, layer)
        || !layer.properties.clipped
        || !layer.effect.as_ref().is_some_and(|e| {
            e.program.image_boundary() && e.program.kind == layer_core::EffectKind::Adjustment
        })
    {
        return None;
    }
    let parent = layer.properties.parent;
    let base = (index + 1..layers.len())
        .find(|&i| layers[i].properties.parent == parent && !layers[i].properties.clipped)?;
    let terminal = !layers[..index]
        .iter()
        .rev()
        .filter(|l| l.properties.parent == parent)
        .take_while(|l| l.properties.clipped)
        .any(|l| l.visible);
    Some(ClipInput {
        base,
        base_id: layers[base].id,
        dependencies: layer_core::backdrop_layers(layers, base),
        terminal,
    })
}

impl Scene {
    fn update_clipping_composition(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        cached: &mut CachedStage,
        changes: &[PixelRect],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        let output_dirty = changes[index];
        let Some(plan) = &self.images.clips[index] else {
            cached.composition = None;
            return Ok(output_dirty);
        };
        let base_index = plan.base;
        let base = &packet.layers[base_index];
        let terminal = plan.terminal;
        let dirty = plan
            .dependencies
            .iter()
            .fold(PixelRect::EMPTY, |r, &i| r.union(changes[i]));
        let bounds = self.images.bounds;
        let mut backdrop = self
            .images
            .backdrops
            .remove(&base.id)
            .unwrap_or_else(|| Backdrop {
                image: Image::new(r, display_mips::Plan::window(self.images.extent, 0, bounds), "clipping backdrop cache"),
                valid: false,
                updated: false,
                damage: PixelRect::EMPTY,
            });
        if !backdrop.updated {
            backdrop.damage = if !backdrop.valid {
                bounds
            } else {
                dirty.intersect(bounds)
            };
            if !backdrop.damage.is_empty() {
                self.jobs.clear();
                self.used.fill(false);
                self.stop_before = Some((base_index, false));
                for tile in page_coordinates(backdrop.damage) {
                    let pixels = self.group(r, packet, input_scope(packet.layers, base), tile)?;
                    self.capture_tile(r, pixels, &backdrop.image, tile, Convert::None);
                }
                self.stop_before = None;
                self.encode_jobs(r, encoder)?;
                backdrop.valid = true;
            }
            backdrop.updated = true;
        }
        let damage = if terminal {
            if cached.composition.is_none() {
                cached.composition = Some(ImageComposition::new(
                    self,
                    r,
                    bounds,
                    &cached.output,
                    &backdrop.image,
                ));
            }
            let composition = cached.composition.as_mut().unwrap();
            let damage = if !composition.valid {
                bounds
            } else {
                output_dirty.union(backdrop.damage).intersect(bounds)
            };
            if !damage.is_empty() {
                composition.encode(self, r, base, damage, packet.blend_space, encoder);
            }
            damage
        } else {
            cached.composition = None;
            // Its raw output is consumed by further clips. Backdrop changes
            // do not invalidate the isolated input of those filters.
            output_dirty
        };
        self.images.backdrops.insert(base.id, backdrop);
        Ok(damage)
    }
    // A write-only tile suffix can draw straight into the image cache. Adjacent
    // tiles then share one render pass, without a temporary tile or GPU copy.
    // Read/modify/write suffixes keep the existing tiled compositor unchanged.
    fn capture_tile(
        &mut self,
        r: &WgpuRasterizer,
        output: usize,
        destination: &Image,
        tile: [u32; 2],
        convert: Convert,
    ) {
        let output = self.converted(r, output, convert);
        let bounds = destination.plan.bounds;
        let extent = [bounds.width(), bounds.height()];
        let view = &self.pool[output].view;
        let start = self
            .jobs
            .iter()
            .rposition(|j| matches!(j, Job::Clear(v, _) if v == view));
        if let Some(start) = start
            && self.jobs[start + 1..]
                .iter()
                .all(|j| matches!(j, Job::Draw{target,..} if target==view))
        {
            let Job::Clear(_, color) = self.jobs[start] else {
                unreachable!()
            };
            let region = page_rect(tile).window_local(bounds);
            let mut fill = [0.; 32];
            fill[..6].copy_from_slice(&[
                region.min_x() as f32,
                region.min_y() as f32,
                region.width() as f32,
                region.height() as f32,
                extent[0] as f32,
                extent[1] as f32,
            ]);
            fill[12..16].copy_from_slice(&[
                color.r as f32,
                color.g as f32,
                color.b as f32,
                color.a as f32,
            ]);
            self.jobs[start] = Job::Draw {
                target: destination.view.clone(),
                sources: [r.empty_view.clone(), r.empty_view.clone(), r.empty_view.clone()],
                data: fill,
                over: false,
                clip: Some(region),
            };
            for job in &mut self.jobs[start + 1..] {
                let Job::Draw {
                    target, data, clip, ..
                } = job
                else {
                    unreachable!()
                };
                *target = destination.view.clone();
                data[0] += (tile[0] * PAGE_SIZE) as f32 - bounds.min_x() as f32;
                data[1] += (tile[1] * PAGE_SIZE) as f32 - bounds.min_y() as f32;
                data[4] = extent[0] as f32;
                data[5] = extent[1] as f32;
                *clip = Some(region);
            }
            self.free(output);
        } else {
            self.copy_window_tile(output, destination, tile);
        }
    }
    pub(super) fn image_tile(
        &mut self,
        r: &WgpuRasterizer,
        view: wgpu::TextureView,
        bounds: PixelRect,
        tile: [u32; 2],
    ) -> usize {
        let out = self.reserve(r);
        self.draw(
            r,
            out,
            view,
            None,
            [
                bounds.min_x() as f32 - (tile[0] * PAGE_SIZE) as f32,
                bounds.min_y() as f32 - (tile[1] * PAGE_SIZE) as f32,
                bounds.width() as f32,
                bounds.height() as f32,
            ],
            [1., 1., 0., 0.],
            false,
            Convert::None,
        );
        out
    }
    pub(super) fn copy_window_tile(
        &mut self,
        output: usize,
        destination: &Image,
        tile: [u32; 2],
    ) {
        let bounds = destination.plan.bounds;
        let region = page_rect(tile).intersect(bounds);
        if !region.is_empty() {
            let extent = [destination.texture.width(), destination.texture.height()];
            let view = &self.pool[output].view;
            if (extent == [bounds.width(), bounds.height()] || region == page_rect(tile))
                && destination.texture.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING)
                && let Some(Job::Draw { target, sources, data, over: false, clip }) = self.jobs.last_mut()
                && target == view && !sources.contains(view)
                && data[..4] == [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32]
                && clip.is_none_or(|clip| clip == PixelRect::full([PAGE_SIZE; 2]))
            {
                *target = destination.view.clone();
                data[0] = (tile[0] * PAGE_SIZE) as f32 - bounds.min_x() as f32;
                data[1] = (tile[1] * PAGE_SIZE) as f32 - bounds.min_y() as f32;
                data[4] = extent[0] as f32;
                data[5] = extent[1] as f32;
                *clip = Some(region.window_local(bounds));
                let n = self.jobs.len();
                if n >= 2 && matches!(&self.jobs[n - 2], Job::Clear(target, _) if target == view) {
                    self.jobs.remove(n - 2);
                }
                self.free(output);
                return;
            }
            self.jobs.push(Job::Copy {
                source: self.pool[output].texture.clone(),
                source_origin: [
                    region.min_x() - tile[0] * PAGE_SIZE,
                    region.min_y() - tile[1] * PAGE_SIZE,
                ],
                destination: destination.texture.clone(),
                origin: [
                    region.min_x() - bounds.min_x(),
                    region.min_y() - bounds.min_y(),
                ],
                width: region.width(),
                height: region.height(),
            });
        }
        self.free(output);
    }
    pub(super) fn retire_images<T>(&mut self, change: impl FnOnce(&mut Self) -> T) -> T {
        let held = self.images.views();
        let result = change(self);
        if !held.is_empty() {
            let kept = self.images.views();
            let retired: Vec<_> = held.into_iter().filter(|view| !kept.contains(view)).collect();
            self.forget_bindings(&retired);
        }
        result
    }
    pub(super) fn update_images(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        self.retire_images(|scene| scene.update_image_stages(r, packet, dirty, encoder))
    }
    fn update_image_stages(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        let extent = packet.document_extent;
        let bounds = self.image_window.unwrap_or(PixelRect::full(extent));
        let grid = display_mips::Plan::window(extent, 0, bounds);
        if self.images.extent != extent || self.images.bounds != bounds {
            self.images = ImageStages {
                extent,
                bounds,
                ..Default::default()
            };
        }
        let dirty = dirty.intersect(bounds);
        if self.images.stages.is_empty()
            && !packet.layers.iter().any(|l| {
                l.visible
                    && l.effect
                        .as_ref()
                        .is_some_and(|e| e.program.image_boundary())
            })
        {
            return Ok(dirty);
        }
        self.images.stages.retain(|s| {
            packet.layers.iter().any(|l| {
                l.id == s.id
                    && visible(packet.layers, l)
                    && l.effect
                        .as_ref()
                        .is_some_and(|e| e.program.image_boundary())
            })
        });
        let changed: Vec<bool> = packet
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                self.images
                    .metadata
                    .get(i)
                    .is_none_or(|old| *old != Metadata::new(l))
            })
            .collect();
        let structure = self.images.metadata.len() != packet.layers.len()
            || self
                .images
                .metadata
                .iter()
                .zip(packet.layers)
                .any(|(a, b)| {
                    a.id != b.id
                        || a.kind != b.kind
                        || a.properties.clipped != b.properties.clipped
                        || a.effect.as_ref().map(|e| e.program.kind)
                            != b.effect.as_ref().map(|e| e.program.kind)
                        || a.properties.parent != b.properties.parent
                        || a.passes_through() != b.passes_through()
                        || (a.kind == LayerKind::Group
                            && (a.properties.offset != b.properties.offset
                                || a.visible != b.visible))
                });
        let reset = packet.reset_layers
            || structure
            || self.images.background != packet.view.background_rgba_linear
            || self.images.blend_space != packet.blend_space;
        if structure
            || self.images.inputs.len() != packet.layers.len()
            || self
                .images
                .metadata
                .iter()
                .zip(packet.layers)
                .any(|(a, b)| {
                    a.properties.clipped != b.properties.clipped
                        || a.visible != b.visible
                        || a.kind != b.kind
                        || a.effect.as_ref().map(|e| e.program.image_boundary())
                            != b.effect.as_ref().map(|e| e.program.image_boundary())
                        || a.effect.as_ref().map(|e| e.program.kind)
                            != b.effect.as_ref().map(|e| e.program.kind)
                })
        {
            self.images.inputs = (0..packet.layers.len())
                .map(|i| input_indices(packet.layers, i))
                .collect();
            self.images.clips = (0..packet.layers.len())
                .map(|i| clip_input(packet.layers, i))
                .collect();
            self.images
                .backdrops
                .retain(|id, _| self.images.clips.iter().flatten().any(|c| c.base_id == *id));
            // Rebuilt dependencies can point at a different base/backdrop.
            for stage in &mut self.images.stages {
                stage.composition = None;
            }
        }
        for backdrop in self.images.backdrops.values_mut() {
            backdrop.updated = false;
            if reset {
                backdrop.valid = false;
            }
        }
        if reset {
            for stage in &mut self.images.stages {
                if let Some(c) = &mut stage.composition {
                    c.valid = false;
                }
            }
        }
        let painting = !packet.dab_batches.is_empty()
            || !packet.dabs.is_empty()
            || !r.transform_damage.is_empty()
            || (!packet.composite_all && !dirty.is_empty());
        let unidentified_paint = painting
            && packet.dab_batches.is_empty()
            && r.transform_damage.is_empty()
            && self.images.preview_layer.is_none()
            && r.preview_layer_id.is_none();
        let mut changes: Vec<PixelRect> = packet
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if reset || changed[i] {
                    bounds
                } else if painting
                    && (unidentified_paint
                        || self.images.preview_layer == Some(l.id)
                        || r.preview_layer_id == Some(l.id)
                        || r.transform_damage.iter().any(|(id, _)| {
                            *id == l.id || l.mask.as_ref().is_some_and(|m| m.id == *id)
                        })
                        || packet.dab_batches.iter().any(|b| {
                            b.layer_id == l.id
                                || l.mask.as_ref().is_some_and(|m| m.id == b.layer_id)
                        }))
                {
                    dirty
                } else {
                    PixelRect::EMPTY
                }
            })
            .collect();
        let mut damage = dirty;
        for index in (0..packet.layers.len()).rev() {
            let layer = &packet.layers[index];
            let source_damage = self.images.inputs[index]
                .iter()
                .fold(PixelRect::EMPTY, |rect, &i| rect.union(changes[i]));
            let Some(effect) = layer
                .effect
                .as_ref()
                .filter(|e| e.program.image_boundary() && visible(packet.layers, layer))
            else {
                changes[index] = changes[index].union(source_damage);
                continue;
            };
            // Adjacent compatible boundaries consume the same GPU image. No
            // allocation, intermediate composition, or image copy is needed.
            let input = Convert::filter_input(packet, effect.program.space);
            let alias = packet.layers[index + 1..]
                .iter()
                .find(|l| l.properties.parent == layer.properties.parent)
                .filter(|l| {
                    input == Convert::None
                        && l.visible
                        && (!layer.properties.clipped || l.properties.clipped)
                        && effect.program.kind == layer_core::EffectKind::Adjustment
                        && l.effect
                            .as_ref()
                            .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                })
                .and_then(|l| {
                    let stage = self
                        .images
                        .stages
                        .iter()
                        .find(|s| s.id == l.id && s.valid)?;
                    if !layer.properties.clipped && l.properties.clipped {
                        stage
                            .composition
                            .as_ref()
                            .filter(|c| c.valid)
                            .map(|c| c.output.clone())
                    } else {
                        Some(stage.output.clone())
                    }
                });
            let mut cached =
                if let Some(i) = self.images.stages.iter().position(|s| s.id == layer.id) {
                    self.images.stages.swap_remove(i)
                } else {
                    CachedStage {
                        id: layer.id,
                        input: alias
                            .clone()
                            .unwrap_or_else(|| Image::new(r, grid, "effect source cache")),
                        input_owned: alias.is_none(),
                        output: Image::new(r, grid, "effect result cache"),
                        mask: None,
                        mask_offset: layer_core::Point {
                            x: f32::NAN,
                            y: f32::NAN,
                        },
                        time: f32::NAN,
                        valid: false,
                        composition: None,
                    }
                };
            if let Some(input) = alias {
                cached.input = input;
                cached.input_owned = false;
            } else if !cached.input_owned {
                cached.input = Image::new(r, grid, "effect source cache");
                cached.input_owned = true;
                cached.valid = false;
            }
            let time = effect.time_seconds(packet.time_seconds);
            let input_scope_changed = self.images.metadata.get(index).is_none_or(|old| {
                old.properties.clipped != layer.properties.clipped
                    || old.effect.as_ref().map(|e| (e.program.kind, e.program.space))
                        != Some((effect.program.kind, effect.program.space))
            });
            let input_dirty = if !cached.valid || reset || input_scope_changed {
                bounds
            } else {
                source_damage.intersect(bounds)
            };
            let output_dirty = if !cached.valid || changed[index] || cached.time != time || reset {
                bounds
            } else {
                changes[index].union(effect.damage_radius().map_or_else(
                    || {
                        if input_dirty.is_empty() {
                            PixelRect::EMPTY
                        } else {
                            bounds
                        }
                    },
                    |radius| input_dirty.expand(radius, extent),
                ))
            }
            .intersect(bounds);
            if cached.input_owned && !input_dirty.is_empty() {
                self.jobs.clear();
                self.used.fill(false);
                self.stop_before = Some((index, layer.properties.clipped));
                if effect.program.kind == layer_core::EffectKind::Generator {
                    self.jobs.push(Job::Clear(
                        cached.input.view.clone(),
                        wgpu::Color::TRANSPARENT,
                    ));
                } else {
                    for tile in page_coordinates(input_dirty) {
                        let pixels = self.group(r, packet, input_scope(packet.layers, layer), tile)?;
                        self.capture_tile(r, pixels, &cached.input, tile, input);
                    }
                }
                self.stop_before = None;
                self.encode_jobs(r, encoder)?;
                self.images.input_updates += 1;
            }
            if !output_dirty.is_empty() {
                self.jobs.clear();
                self.used.fill(false);
                if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
                    let mask_offset = world_offset(packet.layers, layer.id, true);
                    let mask_reset = !cached.valid
                        || cached.mask.is_none()
                        || cached.mask_offset != mask_offset
                        || reset
                        || self
                            .images
                            .metadata
                            .get(index)
                            .is_none_or(|old| old.mask != mask_metadata(&layer.mask));
                    let mask_dirty = if mask_reset {
                        bounds
                    } else if unidentified_paint
                        || self.images.preview_layer == Some(mask.id)
                        || r.preview_layer_id == Some(mask.id)
                        || r.transform_damage.iter().any(|(id, _)| *id == mask.id)
                        || packet.dab_batches.iter().any(|b| b.layer_id == mask.id)
                    {
                        dirty
                    } else {
                        PixelRect::EMPTY
                    };
                    let image = cached
                        .mask
                        .get_or_insert_with(|| Image::new(r, grid, "effect mask cache"));
                    if !mask_dirty.is_empty() {
                        for tile in page_coordinates(mask_dirty) {
                            let m = self.mask_tile(r, mask, mask_offset, tile);
                            self.copy_window_tile(m, &image, tile);
                        }
                        self.encode_jobs(r, encoder)?;
                    }
                    cached.mask_offset = mask_offset;
                } else {
                    cached.mask = None;
                }
                self.jobs.clear();
                let count = effect.program.passes.len().max(1);
                let regions = effects::pass_regions(effect, output_dirty, grid);
                while self.images.scratch.len() < count.saturating_sub(1).min(2) {
                    self.images
                        .scratch
                        .push(Image::new(r, grid, "reusable effect intermediate"));
                }
                let mut previous = cached.input.view.clone();
                for pass in 0..count {
                    let last = pass + 1 == count;
                    let target = if last {
                        cached.output.view.clone()
                    } else {
                        self.images.scratch[pass % 2].view.clone()
                    };
                    let region = regions[pass + 1];
                    let local = region.window_local(bounds);
                    let mut data = effects::image_grid(grid, grid, cached.input.plan);
                    data[..4].copy_from_slice(&[
                        local.min_x() as f32,
                        local.min_y() as f32,
                        local.width() as f32,
                        local.height() as f32,
                    ]);
                    data[9] = f32::from(layer.properties.clipped);
                    let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
                    if last && let Some(mask) = &cached.mask {
                        masks[0] = mask.view.clone();
                        data[11] = 1.;
                    }
                    self.jobs.push(Job::Effect {
                        target: target.clone(),
                        sources: [previous, cached.input.view.clone(), r.empty_view.clone()],
                        data,
                        prepared: self.effects.prepare(
                            r,
                            &[layer],
                            effects::Execution::Image(pass),
                            packet.time_seconds,
                            grid.level,
                            packet.blend_space,
                        )?,
                        masks,
                    });
                    previous = target;
                    self.images.pass_updates += 1;
                    self.images.pass_pixels += region.area();
                }
                self.encode_jobs(r, encoder)?;
                cached.time = time;
                cached.valid = true;
                damage = damage.union(output_dirty);
            }
            changes[index] = output_dirty;
            let composed_dirty =
                self.update_clipping_composition(r, packet, index, &mut cached, &changes, encoder)?;
            damage = damage.union(composed_dirty);
            self.images.stages.push(cached);
            changes[index] = composed_dirty;
        }
        if self.images.stages.is_empty() {
            self.images.scratch.clear();
            self.images.backdrops.clear();
        }
        self.images.metadata = packet.layers.iter().map(Metadata::new).collect();
        self.images.background = packet.view.background_rgba_linear;
        self.images.blend_space = packet.blend_space;
        self.images.preview_layer = r.preview_layer_id;
        Ok(damage)
    }
}
