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
    mask_offset: Option<[i64; 2]>,
    time: f32,
    valid: bool,
    dependencies: Vec<OccurrenceHandle>,
    input_gather: Option<InputGather>,
}
struct InputGather {
    revision: u64,
    time: Option<u32>,
    blend_space: layer_core::BlendSpace,
    moving: Option<OccurrenceHandle>,
    preview: Option<SourceTarget>,
    pages: std::collections::BTreeSet<[u32; 2]>,
}
#[derive(Default)]
pub(super) struct ImageStages {
    extent: [u32; 2],
    pub(super) doc_bounds: DocRect,
    stages: Vec<CachedStage>,
    scratch: Vec<Image>,
    metadata: std::collections::HashMap<OccurrenceHandle, Metadata>,
    preview_layer: Option<SourceTarget>,
    object_moving: Option<OccurrenceHandle>,
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
pub(super) fn capture_window(scene: SceneView<'_>, region: PixelRect, _extent: [u32; 2]) -> DocRect {
    capture_window_cached(scene,region,None)
}
pub(super) fn capture_window_cached(scene:SceneView<'_>,region:PixelRect,objects:Option<&object_spatial::SpatialIndex>)->DocRect {
    if matches!(scene.scope(), Some(layer_core::SceneScope::Raw(_) | layer_core::SceneScope::RawObjects(_))) { return region.into(); }
    dependency_window(scene, region.into(), 0,objects)
}

fn content_support(scene: SceneView<'_>, handle: OccurrenceHandle, level:u32, objects:&mut object_spatial::SpatialIndex) -> DocRect {
    if !scene.visible(handle) { return DocRect::default(); }
    scene.source_target(handle).map_or_else(|| objects.content(scene,handle).map_or_else(DocRect::default,|content|content.bounds_at(level)), |target|
        DocRect::from(PixelRect::full(scene.target_extent(target))).translated(scene.target_offset(target)))
}

fn support_map(scene: SceneView<'_>, level: u32, objects:&mut object_spatial::SpatialIndex) -> std::collections::HashMap<OccurrenceHandle, DocRect> {
    let mut supports = std::collections::HashMap::<OccurrenceHandle, DocRect>::new();
    for &handle in scene.order().iter().rev() {
        let dependencies = layer_core::composite_input_layers(scene, handle);
        let input = dependencies.iter().fold(content_support(scene, handle,level,objects), |support, dependency| support.union(supports.get(dependency).copied().unwrap_or_default()));
        let output = if !scene.visible(handle) { DocRect::default() } else if let Some(effect) = scene.effect(handle) {
            if effect.program.kind == layer_core::EffectKind::Generator {
                let offset = scene.evaluation_offset64();
                DocRect { min: offset.map(|n| n.floor() as i64), max: std::array::from_fn(|axis| (offset[axis] + f64::from(scene.composition().size[axis])).ceil() as i64) }
            } else { effects::pass_input_support(effect, effect.program.passes.len(), input, level).unwrap_or(input) }
        } else { input };
        supports.insert(handle, output);
    }
    supports
}

pub(super) fn dependency_window(scene: SceneView<'_>, output: DocRect, level: u32, objects:Option<&object_spatial::SpatialIndex>) -> DocRect {
    if stack::support(scene, level) == Some(0) { return output; }
    let mut objects=objects.cloned().unwrap_or_default();
    let supports = support_map(scene, level,&mut objects);
    let mut required = std::collections::HashMap::<OccurrenceHandle, DocRect>::new();
    let mut window = output;
    for &handle in scene.order() {
        if !scene.visible(handle) { continue; }
        let region = required.get(&handle).copied().unwrap_or(output).union(output);
        let dependencies = layer_core::composite_input_layers(scene, handle);
        let input = dependencies.iter().fold(content_support(scene, handle,level,&mut objects), |support, dependency| support.union(supports.get(dependency).copied().unwrap_or_default()));
        let region = if let Some(effect) = scene.effect(handle).filter(|effect| effect.program.image_boundary()) {
            let regions = effects::document_pass_regions(effect, region, input, level);
            for &region in &regions { window = window.union(region); }
            regions[0]
        } else { region };
        for dependency in dependencies {
            required.entry(dependency).and_modify(|required| *required = required.union(region)).or_insert(region);
        }
    }
    window
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
                clip: Some(region),source_target:None,
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
        bounds: DocRect,
        tile: [u32; 2],
    ) -> usize {
        let out = self.reserve(r);
        self.draw(
            r,
            out,
            view,
            None,
            [
                (bounds.min[0] - self.image_evaluation_origin[0]) as f32 - (tile[0] * PAGE_SIZE) as f32,
                (bounds.min[1] - self.image_evaluation_origin[1]) as f32 - (tile[1] * PAGE_SIZE) as f32,
                (bounds.max[0] - bounds.min[0]) as f32,
                (bounds.max[1] - bounds.min[1]) as f32,
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
                Some(Job::Draw { target, sources, data, over: false, clip, .. })
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
        if !packet.scene.order().iter().any(|handle| packet.scene.visible(*handle) && packet.scene.effect(*handle).is_some_and(|effect| effect.program.image_boundary())) {
            self.retire_images(|scene| scene.images = ImageStages::default());
            return Ok(dirty);
        }
        let doc_bounds = self.image_window.unwrap_or_else(|| capture_window_cached(packet.scene, PixelRect::full(packet.document_extent), Some(&self.object_spatial)));
        let size = doc_bounds.size()?;
        if size.into_iter().any(|side| side > r.device.limits().max_texture_dimension_2d) { return Err(GpuRasterError::ExtentUnsupported); }
        let rebased = FramePacket { scene: packet.scene.with_offset64(doc_bounds.min.map(|value| -(value as f64))), document_extent: size, ..packet };
        let local_dirty = doc_bounds.local(dirty);
        let previous = std::mem::replace(&mut self.image_evaluation_origin, doc_bounds.min);
        let damage = self.image_damage.take();
        self.image_damage = damage.as_ref().map(|damage| damage.map(|region| doc_bounds.local(region)));
        let result = self.retire_images(|scene| scene.update_image_stages(r, rebased, local_dirty, encoder));
        self.image_evaluation_origin = previous;
        self.image_damage = damage;
        result.map(|local| {
            let document = DocRect { min: [doc_bounds.min[0] + i64::from(local.min_x()), doc_bounds.min[1] + i64::from(local.min_y())], max: [doc_bounds.min[0] + i64::from(local.max_x()), doc_bounds.min[1] + i64::from(local.max_y())] };
            document.in_frame(packet.document_extent)
        })
    }
    fn update_image_stages(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        let extent = packet.document_extent;
        let bounds = PixelRect::full(extent);
        let doc_bounds = self.image_window.unwrap_or(DocRect { min: self.image_evaluation_origin, max: [self.image_evaluation_origin[0] + i64::from(extent[0]), self.image_evaluation_origin[1] + i64::from(extent[1])] });
        let grid = display_mips::Plan::window(extent, 0, bounds);
        if self.images.extent != extent || self.images.doc_bounds != doc_bounds {
            self.images = ImageStages {
                extent,
                doc_bounds,
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
        let metadata: std::collections::HashMap<_,_> = order.iter().map(|&h|(h,Metadata::new_cached(scene,h,&mut self.object_spatial))).collect();
        let object_moving = self.object_live.then_some(r.moving_layer).flatten().filter(|owner| scene.object_layer(*owner).is_some());
        let quality_changed = |owner| self.images.object_moving != object_moving && (self.images.object_moving == Some(owner) || object_moving == Some(owner));
        let changed: std::collections::HashSet<_> = order.iter().copied().filter(|h|quality_changed(*h)||self.images.metadata.get(h).is_none_or(|old|old != &metadata[h])).collect();
        let content_changed: std::collections::HashSet<_> = order.iter().copied().filter(|h|quality_changed(*h)||self.images.metadata.get(h).is_none_or(|old|!old.same_content(&metadata[h]))).collect();
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
                    self.scale_sources.document_damage(scene, target).map_or_else(|| sparse.regions.clone(), |damage| damage.map(|region| region.in_frame(extent)).collect())
                }));
                sources.intersect(bounds)
            }else{scale::Damage::default()};
            (h,damage)
        }).collect();
        let supports = support_map(scene, 0,&mut self.object_spatial);
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
                        mask_offset: None,
                        time: f32::NAN,
                        valid: false,
                        dependencies: Vec::new(),
                        input_gather: None,
                    }
                };
            if let Some(input) = alias {
                cached.input = input;
                cached.input_owned = false;
            } else if !cached.input_owned {
                cached.input = Image::new(r, grid, "effect source cache");
                cached.input_owned = true;
                cached.valid = false;
                cached.input_gather = None;
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
                } else if !dependencies.iter().any(|dependency| scene.object_layer(*dependency).is_some()) {
                    for tile in input_dirty.pages() {
                        let pixels = self.group(r, packet, layer_core::composite_input_scope(scene, handle), tile)?;
                        self.capture_tile(r, pixels, &cached.input, tile, input);
                    }
                } else {
                    let time = order.iter().any(|h| scene.visible(*h) && scene.effect(*h).is_some_and(|effect| effect.animated())).then_some(packet.time_seconds.to_bits());
                    if cached.input_gather.as_ref().is_none_or(|gather| gather.revision != scene.revision() || gather.time != time
                        || gather.blend_space != packet.blend_space || gather.moving != r.moving_layer || gather.preview != r.preview_layer_id)
                        || !packet.dabs.is_empty() || !packet.dab_batches.is_empty() || !packet.restore_rasters.is_empty() || !r.transform_damage.is_empty() {
                        cached.input_gather = Some(InputGather { revision: scene.revision(), time, blend_space: packet.blend_space,
                            moving: r.moving_layer, preview: r.preview_layer_id, pages: input_dirty.pages().into_iter().collect() });
                    }
                    cached.valid = false;
                    while let Some(tile) = cached.input_gather.as_ref().unwrap().pages.first().copied() {
                        let result = (|| {
                            let pixels = self.group(r, packet, layer_core::composite_input_scope(scene, handle), tile)?;
                            self.capture_tile(r, pixels, &cached.input, tile, input);
                            self.encode_jobs(r, encoder)
                        })();
                        if let Err(error) = result {
                            self.stop_before = None;
                            self.object_results.reset_used(); self.exact_object_results.reset_used();
                            if matches!(error, GpuRasterError::DeferredObjectWork) {
                                for stage in &mut self.images.stages {
                                    if scene.position(stage.id).unwrap() < scene.position(handle).unwrap() { stage.valid = false; }
                                }
                                cached.dependencies = dependencies;
                                self.images.stages.push(cached);
                                self.images.metadata = metadata;
                                self.images.blend_space = packet.blend_space;
                                self.images.preview_layer = r.preview_layer_id;
                                self.images.object_moving = object_moving;
                            }
                            return Err(error);
                        }
                        cached.input_gather.as_mut().unwrap().pages.remove(&tile);
                        self.object_results.flush_retired(encoder); self.exact_object_results.flush_retired(encoder);
                        self.clear_material_pages(); self.used.fill(false);
                    }
                    cached.input_gather = None;
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
                        || cached.mask_offset != Some(mask_offset)
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
                    cached.mask_offset = Some(mask_offset);
                } else {
                    cached.mask = None;
                }
                self.jobs.clear();
                let count = effect.program.passes.len().max(1);
                let input_support = layer_core::composite_input_layers(scene, handle).iter().fold(DocRect::default(), |support, dependency| support.union(supports.get(dependency).copied().unwrap_or_default()));
                let regions: Vec<Vec<_>> = output_dirty.regions.iter().map(|&region| effects::document_pass_regions(effect, region.into(), input_support, 0)
                    .into_iter().map(|region| grid.doc_bounds.local(region)).collect()).collect();
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
                    let dependencies = layer_core::composite_input_layers(scene, handle);
                    let mut front = grid;
                    front.support = dependencies.iter().fold(DocRect::default(), |support, dependency| support.union(supports.get(dependency).copied().unwrap_or_default()));
                    front.support = effects::pass_input_support(effect, pass, front.support, 0).unwrap_or(front.support);
                    let mut original = cached.input.plan;
                    original.support = dependencies.iter().fold(DocRect::default(), |support, dependency| support.union(supports.get(dependency).copied().unwrap_or_default()));
                    let mut data = effects::image_grid(grid, front, original);
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
                        masks,source_target:None,changed_cells:None,
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
        self.images.object_moving = object_moving;
        Ok(damage.bounds())
    }
}
