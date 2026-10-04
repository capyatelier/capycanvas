//! Cached document-coordinate windows for neighborhood and time-aware WGSL.
//! Tiled captures feed the same filter and composition operations at any origin.
use super::metadata::{Metadata, mask_metadata};
use super::*;
use crate::effects::Gpu;
use layer_core::{SceneView,SourceTarget};

#[cfg(test)]
#[path = "scene/image_grid_tests.rs"]
mod grid_tests;

struct CachedStage {
    id: OccurrenceHandle,
    input: Image,
    input_owned: bool,
    output: Image,
    mask: Option<Image>,
    mask_offset: layer_core::Point,
    time: f32,
    valid: bool,
    dependencies: Vec<OccurrenceHandle>,
}
#[derive(Default)]
pub(super) struct ImageStages {
    extent: [u32; 2],
    pub(super) bounds: PixelRect,
    stages: Vec<CachedStage>,
    scratch: Vec<Image>,
    metadata: std::collections::HashMap<OccurrenceHandle, Metadata>,
    preview_layer: Option<SourceTarget>,
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

    }
    pub fn output(&self, id: OccurrenceHandle) -> Option<wgpu::TextureView> {
        self.stages
            .iter()
            .find(|s| s.id == id && s.valid)
            .map(|s| s.output.view.clone())
    }
    fn views(&self) -> Vec<wgpu::TextureView> {
        self.stages.iter().flat_map(|stage| [
            stage.input_owned.then_some(&stage.input), Some(&stage.output), stage.mask.as_ref(),
        ]).flatten().chain(&self.scratch)
            .map(|image| image.view.clone()).collect()
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

pub(super) fn visible(scene:SceneView<'_>,handle:OccurrenceHandle)->bool{scene.visible(handle)}
pub(super) fn capture_window(scene:SceneView<'_>,region:PixelRect,extent:[u32;2])->PixelRect{
    if matches!(scene.scope(),Some(layer_core::SceneScope::Raw(_))) {return region;}
    stack::support(scene,0).map_or_else(||PixelRect::full(extent),|radius|region.expand(radius,extent))
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
            let direct = match self.jobs.last_mut() {
                Some(Job::Draw { target, sources, data, over: false, clip })
                    if clip.is_none_or(|clip| clip == PixelRect::full([PAGE_SIZE; 2])) => Some((target, sources, data, Some(clip))),
                Some(Job::Effect { target, sources, data, prepared, .. }) if prepared.pointwise => Some((target, sources, data, None)),
                _ => None,
            };
            if destination.texture.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING)
                && let Some((target, sources, data, clip)) = direct
                && target == view && !sources.contains(view) && !sources.contains(&destination.view)
                && data[..4] == [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32]
            {
                *target = destination.view.clone();
                data[0] = (tile[0] * PAGE_SIZE) as f32 - bounds.min_x() as f32;
                data[1] = (tile[1] * PAGE_SIZE) as f32 - bounds.min_y() as f32;
                data[4] = extent[0] as f32;
                data[5] = extent[1] as f32;
                if let Some(clip) = clip { *clip = Some(region.window_local(bounds)); }
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
                input_updates: self.images.input_updates,
                pass_updates: self.images.pass_updates,
                pass_pixels: self.images.pass_pixels,
                ..Default::default()
            };
        }
        let dirty = dirty.intersect(bounds);
        let scene=packet.scene;let order=scene.order();
        if self.images.stages.is_empty() && !order.iter().any(|h|visible(scene,*h)&&scene.effect(*h).is_some_and(|e|e.program.image_boundary())){return Ok(dirty);}
        self.images.stages.retain(|s|visible(scene,s.id)&&scene.effect(s.id).is_some_and(|e|e.program.image_boundary()));
        let metadata: std::collections::HashMap<_,_> = order.iter().map(|&h|(h,Metadata::new(scene,h))).collect();
        let changed: std::collections::HashSet<_> = order.iter().copied().filter(|h|self.images.metadata.get(h).is_none_or(|old|old != &metadata[h])).collect();
        let content_changed: std::collections::HashSet<_> = order.iter().copied().filter(|h|self.images.metadata.get(h).is_none_or(|old|!old.same_content(&metadata[h]))).collect();
        let reset = packet.reset_layers || self.images.blend_space != packet.blend_space;
        let painting = !packet.dab_batches.is_empty()
            || !packet.dabs.is_empty()
            || !r.transform_damage.is_empty()
            || (!packet.composite_all && !dirty.is_empty());
        let unidentified_paint = painting
            && packet.dab_batches.is_empty()
            && r.transform_damage.is_empty()
            && self.images.preview_layer.is_none()
            && r.preview_layer_id.is_none();
        let sparse = self.image_damage.clone().unwrap_or_else(||dirty.into()).intersect(bounds);
        let mut changes: std::collections::HashMap<_,scale::Damage> = order.iter().copied().map(|h|{
            let target=scene.source_target(h);let mask=scene.mask(h).map(|(m,_)|SourceTarget::Coverage(m.source));
            let changed_target=|t:SourceTarget|Some(t)==target||Some(t)==mask;
            let damage = if reset||changed.contains(&h){bounds.into()}else if painting&&(unidentified_paint
                ||self.images.preview_layer.is_some_and(changed_target)||r.preview_layer_id.is_some_and(changed_target)
                ||r.transform_damage.iter().any(|(t,_)|changed_target(*t))||packet.dab_batches.iter().any(|b|changed_target(b.target))) {
                let sources = scale::Damage::from_regions(target.into_iter().chain(mask).flat_map(|target| {
                    let geometry = scene.target_geometry(target);
                    self.scale_sources.damage(target).map_or_else(||sparse.clone(),|source|source.map(|region|pixel_rect(geometry.forward_bounds(region.to_rect()),extent))).regions
                }));
                sources.intersect(bounds)
            }else{scale::Damage::default()};
            (h,damage)
        }).collect();
        let mut content_changes = changes.clone();
        for &h in order {
            if changed.contains(&h) && !content_changed.contains(&h) { content_changes.insert(h,scale::Damage::default()); }
        }
        let mut damage = sparse;
        for &handle in order.iter().rev() {
            let dependencies = layer_core::composite_input_layers(scene,handle);
            let owner = scene.effect_owner(handle);
            let source_damage = scale::Damage::from_regions(dependencies.iter().flat_map(|h|{
                if owner == Some(*h) { content_changes[h].regions.iter().copied() } else { changes[h].regions.iter().copied() }
            }));
            let Some(effect)=scene.effect(handle).filter(|e|e.program.image_boundary()&&visible(scene,handle))else{changes.get_mut(&handle).unwrap().extend(&source_damage);content_changes.get_mut(&handle).unwrap().extend(&source_damage);continue;};
            let input=Convert::filter_input(packet,effect.program.space);
            let previous = if let Some(owner) = scene.effect_owner(handle) {
                scene.attached_effects(owner).iter().copied().take_while(|h|*h!=handle).filter(|h|scene.visible(*h)).last()
            } else {
                order[scene.position(handle).unwrap()+1..].iter().copied().find(|h|scene.includes(*h)&&scene.evaluation_parent(*h)==scene.evaluation_parent(handle)&&scene.effect_owner(*h).is_none())
            };
            let alias = previous.filter(|h| input==Convert::None&&scene.visible(*h)
                &&effect.program.kind==layer_core::EffectKind::Adjustment&&scene.effect(*h).is_some_and(|e|e.program.kind==layer_core::EffectKind::Adjustment))
                .and_then(|h|self.images.stages.iter().find(|s|s.id==h&&s.valid).map(|s|s.output.clone()));
            let mut cached =
                if let Some(i) = self.images.stages.iter().position(|s| s.id == handle) {
                    self.images.stages.swap_remove(i)
                } else {
                    CachedStage {
                        id: handle,
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
                        dependencies: Vec::new(),
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
            let time=r.effect_time(scene,handle,packet.time_seconds);
            let input_scope_changed = cached.dependencies != dependencies
                || self.images.metadata.get(&handle).is_none_or(|old|old.effect_contract.map(|v|(v.0,v.1)) != Some((effect.program.kind,effect.program.space)));
            let input_dirty = if !cached.valid || reset || input_scope_changed { bounds.into() } else { source_damage.intersect(bounds) };
            let output_dirty = if !cached.valid || changed.contains(&handle) || cached.time != time || reset { bounds.into() }
                else {
                    let mut result = changes[&handle].clone();
                    let filtered = effect.damage_radius().map_or_else(||if input_dirty.is_empty(){scale::Damage::default()}else{bounds.into()},|radius|input_dirty.expand(radius,extent));
                    result.extend(&filtered);
                    result.intersect(bounds)
                };
            if cached.input_owned && !input_dirty.is_empty() {
                self.jobs.clear();
                self.used.fill(false);
                self.stop_before = Some(handle);
                if effect.program.kind == layer_core::EffectKind::Generator {
                    self.jobs.push(Job::Clear(
                        cached.input.view.clone(),
                        wgpu::Color::TRANSPARENT,
                    ));
                } else {
                    for tile in input_dirty.pages() {
                        let pixels = self.group(r, packet, layer_core::composite_input_scope(scene, handle), tile)?;
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
                if let Some((mask,coverage)) = scene.mask(handle).filter(|(m,_)| m.enabled) {
                    let mask_offset = world_offset(scene, handle, true);
                    let mask_reset = !cached.valid
                        || cached.mask.is_none()
                        || cached.mask_offset != mask_offset
                        || reset
                        || self
                            .images
                            .metadata
                            .get(&handle)
                            .is_none_or(|old| old.mask != mask_metadata(scene,handle));
                    let mask_dirty = if mask_reset {
                        scale::Damage::from(bounds)
                    } else if unidentified_paint
                        || self.images.preview_layer == Some(SourceTarget::Coverage(mask.source))
                        || r.preview_layer_id == Some(SourceTarget::Coverage(mask.source))
                        || r.transform_damage.iter().any(|(id, _)| *id == SourceTarget::Coverage(mask.source))
                        || packet.dab_batches.iter().any(|b| b.target == SourceTarget::Coverage(mask.source))
                    {
                        self.image_damage.clone().unwrap_or_else(||dirty.into()).intersect(bounds)
                    } else {
                        scale::Damage::default()
                    };
                    let image = cached
                        .mask
                        .get_or_insert_with(|| Image::new(r, grid, "effect mask cache"));
                    if !mask_dirty.is_empty() {
                        for tile in mask_dirty.pages() {
                            let m = self.mask_tile(r, mask,coverage, mask_offset, tile);
                            self.copy_window_tile(m, image, tile);
                        }
                        self.encode_jobs(r, encoder)?;
                    }
                    cached.mask_offset = mask_offset;
                } else {
                    cached.mask = None;
                }
                self.jobs.clear();
                let count = effect.program.passes.len().max(1);
                let regions: Vec<_> = output_dirty.regions.iter().map(|&region|effects::pass_regions(effect,region,grid)).collect();
                while self.images.scratch.len() < count.saturating_sub(1).min(2) {
                    self.images
                        .scratch
                        .push(Image::new(r, grid, "reusable effect intermediate"));
                }
                for regions in regions {
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
                            scene,
                            &[handle],
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
                }
                self.encode_jobs(r, encoder)?;
                cached.time = time;
                cached.valid = true;
                damage.extend(&output_dirty);
            }
            changes.insert(handle,output_dirty);
            cached.dependencies = dependencies;
            self.images.stages.push(cached);
        }
        if self.images.stages.is_empty() {
            self.images.scratch.clear();
        }
        self.images.metadata = metadata;
        self.images.blend_space = packet.blend_space;
        self.images.preview_layer = r.preview_layer_id;
        Ok(damage.bounds())
    }
}
