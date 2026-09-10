//! Cached full-resolution boundaries for neighborhood and time-aware WGSL.
//! The tiled scene remains the only layer/clip/group compositor.
use super::*;

#[derive(Clone)]
struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Image {
    fn new(r: &WgpuRasterizer, extent: [u32; 2], label: &'static str) -> Self {
        let (texture, view) = create_color_target(&r.device, extent, label);
        Self { texture, view }
    }
    fn bytes(&self) -> u64 {
        self.texture.width() as u64 * self.texture.height() as u64 * 4
    }
}
struct CachedStage {
    id: LayerId,
    input: Image,
    input_owned: bool,
    output: Image,
    mask: Option<Image>,
    time: f32,
    valid: bool,
}
#[derive(Default)]
pub(super) struct ImageStages {
    extent: [u32; 2],
    stages: Vec<CachedStage>,
    scratch: Vec<Image>,
    metadata: Vec<Layer>,
    inputs: Vec<Vec<usize>>,
    preview_layer: Option<LayerId>,
    background: [f32; 4],
    pub input_updates: u64,
    pub pass_updates: u64,
    pub pass_pixels: u64,
}
impl ImageStages {
    pub fn output_texture(&self, id: LayerId) -> Option<&wgpu::Texture> {
        self.stages
            .iter()
            .find(|s| s.id == id && s.valid)
            .map(|s| &s.output.texture)
    }
    pub fn output(&self, id: LayerId) -> Option<wgpu::TextureView> {
        self.stages
            .iter()
            .find(|s| s.id == id && s.valid)
            .map(|s| s.output.view.clone())
    }
    pub fn storage_bytes(&self) -> u64 {
        self.stages
            .iter()
            .map(|s| {
                u64::from(s.input_owned) * s.input.bytes()
                    + s.output.bytes()
                    + s.mask.as_ref().map_or(0, Image::bytes)
            })
            .sum::<u64>()
            + self.scratch.iter().map(Image::bytes).sum::<u64>()
    }
}

// Stroke history does not participate in composition; batches invalidate paint.
// Mask stroke IDs are already shared. Do not clone stroke/operation vectors.
fn metadata(l: &Layer) -> Layer {
    l.composite_snapshot()
}
fn visible(layers: &[Layer], layer: &Layer) -> bool {
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

// Dependencies follow the same isolated group / clipping-stack boundaries as
// composition. Build on structural edits only, not on every dab or frame.
fn input_indices(layers: &[Layer], index: usize) -> Vec<usize> {
    let layer = &layers[index];
    let adjustment = layer
        .effect
        .as_ref()
        .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment);
    if layer.kind != LayerKind::Group && !adjustment {
        return Vec::new();
    }
    let parent = if layer.kind == LayerKind::Group {
        Some(layer.id)
    } else {
        layer.properties.parent
    };
    let end = if adjustment && layer.properties.clipped {
        layers
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, l)| l.properties.parent == parent && !l.properties.clipped)
            .map_or(layers.len(), |(i, _)| i)
    } else {
        layers.len()
    };
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

impl Scene {
    // A write-only tile suffix can draw straight into the image cache. Adjacent
    // tiles then share one render pass, without a temporary tile or GPU copy.
    // Read/modify/write suffixes keep the existing tiled compositor unchanged.
    fn capture_tile(
        &mut self,
        r: &WgpuRasterizer,
        output: usize,
        destination: &Image,
        tile: [u32; 2],
        extent: [u32; 2],
    ) {
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
            let region = page_rect(tile).intersect(PixelRect::full(extent));
            let mut fill = [0.; 24];
            fill[..6].copy_from_slice(&[
                region.min_x as f32,
                region.min_y as f32,
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
                sources: [r.empty_view.clone(), r.empty_view.clone()],
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
                data[0] += region.min_x as f32;
                data[1] += region.min_y as f32;
                data[4] = extent[0] as f32;
                data[5] = extent[1] as f32;
                *clip = Some(region);
            }
            self.free(output);
        } else {
            self.copy_tile(output, &destination.texture, tile, extent);
        }
    }
    pub(super) fn image_tile(
        &mut self,
        r: &WgpuRasterizer,
        view: wgpu::TextureView,
        extent: [u32; 2],
        tile: [u32; 2],
    ) -> usize {
        let out = self.reserve(r);
        self.draw(
            r,
            out,
            view,
            None,
            [
                -((tile[0] * PAGE_SIZE) as f32),
                -((tile[1] * PAGE_SIZE) as f32),
                extent[0] as f32,
                extent[1] as f32,
            ],
            [1., 1., 0., 0.],
            false,
        );
        out
    }
    pub(super) fn copy_tile(
        &mut self,
        output: usize,
        destination: &wgpu::Texture,
        tile: [u32; 2],
        extent: [u32; 2],
    ) {
        let origin = [tile[0] * PAGE_SIZE, tile[1] * PAGE_SIZE];
        self.jobs.push(Job::Copy {
            source: self.pool[output].texture.clone(),
            destination: destination.clone(),
            origin,
            width: PAGE_SIZE.min(extent[0] - origin[0]),
            height: PAGE_SIZE.min(extent[1] - origin[1]),
        });
        self.free(output);
    }
    pub(super) fn update_images(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        let extent = packet.document_extent;
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
        if self.images.extent != extent {
            self.images = ImageStages {
                extent,
                ..Default::default()
            };
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
                    .is_none_or(|old| *old != metadata(l))
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
                        || a.properties.parent != b.properties.parent
                        || (a.kind == LayerKind::Group
                            && (a.properties.offset != b.properties.offset
                                || a.visible != b.visible))
                });
        let reset = packet.reset_layers
            || structure
            || self.images.background != packet.view.background_rgba_linear;
        if structure
            || self.images.inputs.len() != packet.layers.len()
            || self
                .images
                .metadata
                .iter()
                .zip(packet.layers)
                .any(|(a, b)| {
                    a.properties.clipped != b.properties.clipped
                        || a.kind != b.kind
                        || a.effect.as_ref().map(|e| e.program.kind)
                            != b.effect.as_ref().map(|e| e.program.kind)
                })
        {
            self.images.inputs = (0..packet.layers.len())
                .map(|i| input_indices(packet.layers, i))
                .collect();
        }
        let painting = !packet.dab_batches.is_empty()
            || !packet.dabs.is_empty()
            || (!packet.composite_all && !dirty.is_empty());
        let unidentified_paint = painting
            && packet.dab_batches.is_empty()
            && self.images.preview_layer.is_none()
            && r.preview_layer_id.is_none();
        let mut changes: Vec<PixelRect> = packet
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if reset || changed[i] {
                    PixelRect::full(extent)
                } else if painting
                    && (unidentified_paint
                        || self.images.preview_layer == Some(l.id)
                        || r.preview_layer_id == Some(l.id)
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
            let alias = packet.layers[index + 1..]
                .iter()
                .find(|l| l.properties.parent == layer.properties.parent)
                .filter(|l| {
                    l.visible
                        && l.properties.clipped == layer.properties.clipped
                        && effect.program.kind == layer_core::EffectKind::Adjustment
                        && l.effect
                            .as_ref()
                            .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                })
                .and_then(|l| self.images.stages.iter().find(|s| s.id == l.id && s.valid))
                .map(|s| s.output.clone());
            let mut cached =
                if let Some(i) = self.images.stages.iter().position(|s| s.id == layer.id) {
                    self.images.stages.swap_remove(i)
                } else {
                    CachedStage {
                        id: layer.id,
                        input: alias
                            .clone()
                            .unwrap_or_else(|| Image::new(r, extent, "effect source cache")),
                        input_owned: alias.is_none(),
                        output: Image::new(r, extent, "effect result cache"),
                        mask: None,
                        time: f32::NAN,
                        valid: false,
                    }
                };
            if let Some(input) = alias {
                cached.input = input;
                cached.input_owned = false;
            } else if !cached.input_owned {
                cached.input = Image::new(r, extent, "effect source cache");
                cached.input_owned = true;
                cached.valid = false;
            }
            let time = effect.time_seconds(packet.time_seconds);
            let input_scope_changed = self.images.metadata.get(index).is_none_or(|old| {
                old.properties.clipped != layer.properties.clipped
                    || old.effect.as_ref().map(|e| e.program.kind) != Some(effect.program.kind)
            });
            let input_dirty = if !cached.valid || reset || input_scope_changed {
                PixelRect::full(extent)
            } else {
                source_damage
            };
            let output_dirty = if !cached.valid || changed[index] || cached.time != time || reset {
                PixelRect::full(extent)
            } else {
                changes[index].union(effect.damage_radius().map_or_else(
                    || {
                        if input_dirty.is_empty() {
                            PixelRect::EMPTY
                        } else {
                            PixelRect::full(extent)
                        }
                    },
                    |radius| input_dirty.expand(radius, extent),
                ))
            };
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
                        let input = self.group(r, packet, layer.properties.parent, tile)?;
                        self.capture_tile(r, input, &cached.input, tile, extent);
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
                    let mask_changed = !cached.valid
                        || cached.mask.is_none()
                        || reset
                        || self
                            .images
                            .metadata
                            .get(index)
                            .is_none_or(|old| old.mask != layer.mask)
                        || packet.dab_batches.iter().any(|b| b.layer_id == mask.id);
                    let image = cached
                        .mask
                        .get_or_insert_with(|| Image::new(r, extent, "effect mask cache"));
                    if mask_changed {
                        for tile in page_coordinates(PixelRect::full(extent)) {
                            let m = self.mask_tile(
                                r,
                                mask,
                                world_offset(packet.layers, layer.id, true),
                                tile,
                            );
                            self.copy_tile(m, &image.texture, tile, extent);
                        }
                        self.encode_jobs(r, encoder)?;
                    }
                } else {
                    cached.mask = None;
                }
                self.jobs.clear();
                let count = effect.program.passes.len().max(1);
                while self.images.scratch.len() < count.saturating_sub(1).min(2) {
                    self.images
                        .scratch
                        .push(Image::new(r, extent, "reusable effect intermediate"));
                }
                let mut previous = cached.input.view.clone();
                for pass in 0..count {
                    let last = pass + 1 == count;
                    let target = if last {
                        cached.output.view.clone()
                    } else {
                        self.images.scratch[pass % 2].view.clone()
                    };
                    // Intermediate scratch is shared, so populate the halo that
                    // later passes will read even outside the final dirty area.
                    let region = effect
                        .program
                        .passes
                        .iter()
                        .skip(pass + 1)
                        .try_fold(output_dirty, |rect, p| {
                            Some(rect.expand(p.sampling.radius(effect)?, extent))
                        })
                        .unwrap_or(PixelRect::full(extent));
                    let mut data = [0.; 24];
                    data[..6].copy_from_slice(&[
                        region.min_x as f32,
                        region.min_y as f32,
                        region.width() as f32,
                        region.height() as f32,
                        extent[0] as f32,
                        extent[1] as f32,
                    ]);
                    data[14..16].copy_from_slice(&[extent[0] as f32, extent[1] as f32]);
                    data[9] = f32::from(layer.properties.clipped);
                    let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
                    if last && let Some(mask) = &cached.mask {
                        masks[0] = mask.view.clone();
                        data[11] = 1.;
                    }
                    self.jobs.push(Job::Effect {
                        target: target.clone(),
                        sources: [previous, cached.input.view.clone()],
                        data,
                        prepared: self.effects.prepare(
                            r,
                            &[layer],
                            effects::Execution::Image(pass),
                            packet.time_seconds,
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
            self.images.stages.push(cached);
            changes[index] = output_dirty;
        }
        if self.images.stages.is_empty() {
            self.images.scratch.clear();
        }
        self.images.metadata = packet.layers.iter().map(metadata).collect();
        self.images.background = packet.view.background_rgba_linear;
        self.images.preview_layer = r.preview_layer_id;
        Ok(damage)
    }
}
