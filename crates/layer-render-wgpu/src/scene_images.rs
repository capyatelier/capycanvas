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
    background: [f32; 4],
    pub input_updates: u64,
    pub pass_updates: u64,
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
    Layer {
        id: l.id,
        name: "".into(),
        kind: l.kind,
        visible: l.visible,
        opacity: l.opacity,
        strokes: Vec::new(),
        asset: l.asset.clone(),
        source_revision: None,
        properties: l.properties.clone(),
        mask: l.mask.clone(),
        operations: Vec::new(),
        effect: l.effect.clone(),
    }
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

impl Scene {
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
    fn copy_tile(
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
        let painting = !packet.dab_batches.is_empty()
            || !packet.dabs.is_empty()
            || (!packet.composite_all && !dirty.is_empty());
        let mut damage = dirty;
        let mut upstream = reset;
        for index in (0..packet.layers.len()).rev() {
            let layer = &packet.layers[index];
            let Some(effect) = layer
                .effect
                .as_ref()
                .filter(|e| e.program.image_boundary() && visible(packet.layers, layer))
            else {
                upstream |= changed[index];
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
            let whole_output = !cached.valid || changed[index] || cached.time != time;
            let input_scope_changed = self.images.metadata.get(index).is_none_or(|old| {
                old.properties.clipped != layer.properties.clipped
                    || old.effect.as_ref().map(|e| e.program.kind) != Some(effect.program.kind)
            });
            let input_dirty = if !cached.valid || upstream || input_scope_changed {
                PixelRect::full(extent)
            } else if painting {
                damage
            } else {
                PixelRect::EMPTY
            };
            let output_dirty = if !cached.valid || changed[index] || cached.time != time || upstream
            {
                PixelRect::full(extent)
            } else {
                effect.program.damage_radius().map_or_else(
                    || {
                        if input_dirty.is_empty() {
                            PixelRect::EMPTY
                        } else {
                            PixelRect::full(extent)
                        }
                    },
                    |radius| input_dirty.expand(radius, extent),
                )
            };
            if cached.input_owned && !input_dirty.is_empty() {
                self.jobs.clear();
                self.used.fill(false);
                self.stop_before = Some(index);
                if effect.program.kind == layer_core::EffectKind::Generator {
                    self.jobs.push(Job::Clear(
                        cached.input.view.clone(),
                        wgpu::Color::TRANSPARENT,
                    ));
                } else {
                    for tile in page_coordinates(input_dirty) {
                        let input = self.group(r, packet, layer.properties.parent, tile)?;
                        self.copy_tile(input, &cached.input.texture, tile, extent);
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
                        .try_fold(output_dirty, |rect, p| match p.sampling {
                            layer_core::EffectSampling::Neighborhood { radius } => {
                                Some(rect.expand(radius, extent))
                            }
                            layer_core::EffectSampling::Document => None,
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
                            Some(pass),
                            packet.time_seconds,
                        )?,
                        masks,
                    });
                    previous = target;
                    self.images.pass_updates += 1;
                }
                self.encode_jobs(r, encoder)?;
                cached.time = time;
                cached.valid = true;
                damage = damage.union(output_dirty);
            }
            self.images.stages.push(cached);
            upstream |= whole_output;
        }
        if self.images.stages.is_empty() {
            self.images.scratch.clear();
        }
        self.images.metadata = packet.layers.iter().map(metadata).collect();
        self.images.background = packet.view.background_rgba_linear;
        Ok(damage)
    }
}
