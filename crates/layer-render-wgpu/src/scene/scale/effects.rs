use super::*;

pub(super) fn native_alpha_capture(scene: layer_core::SceneView<'_>, handles: &[OccurrenceHandle], level: u32) -> bool {
    handles.iter().any(|handle| native_pointwise_alpha(scene.effect(*handle).unwrap().program))
        && (level > 0 || handles.iter().all(|handle| !scene.effect(*handle).unwrap().program.fusion_boundary()))
}

fn capture_frame<'a>(packet: FramePacket<'a>, target: &Target, regions: &[DocRect]) -> Result<(FramePacket<'a>, display_mips::Plan, Vec<PixelRect>, [i64; 2]), GpuRasterError> {
    if target.plan.doc_bounds == DocRect::from(target.plan.bounds) {
        return Ok((packet, target.plan, regions.iter().map(|region| region.in_frame(packet.document_extent)).collect(), [0; 2]));
    }
    let origin = target.plan.doc_bounds.min;
    let size = target.plan.doc_bounds.size()?;
    let packet = FramePacket { scene: packet.scene.with_offset64(origin.map(|value| -(value as f64))), document_extent: size, ..packet };
    Ok((packet, display_mips::Plan::at(size, target.plan.level), regions.iter().map(|region| target.plan.doc_bounds.local(*region)).collect(), origin))
}

#[expect(clippy::too_many_arguments, reason = "Native capture keeps output ownership, initialized pages and exact regions explicit")]
#[cfg(test)]
pub(super) fn capture_native_effect(
    scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
    target: &Target, regions: &[DocRect], initialized: Option<&BTreeSet<[u32; 2]>>, encoder: &mut submission::CommandEncoder,
) -> Result<(), GpuRasterError> {
    let reserved = scene.used.clone();
    let (packet, plan, regions, _) = capture_frame(packet, target, regions)?;
    let initialized = initialized.filter(|_| target.plan.doc_bounds == DocRect::from(target.plan.bounds));
    let bounds = regions.iter().fold(PixelRect::EMPTY, |bounds, region| bounds.union(*region));
    let destination = Image { texture: target.view.texture().clone(), view: target.view.clone(), plan };
    let window = scene.cached_capture_window(packet.scene, bounds);
    let output=scene::Output::EffectComposite(handle);
    let result = scene.prepare_region_in_frame(r, packet, (bounds, window), PixelRect::EMPTY, false, encoder)
        .and_then(|()| scene.declare_capture(r,packet,&destination,&regions,output,Some(&reserved),initialized)?.accept_query(r,Some(scene)))
        .and_then(|mut accepted| {accepted.clone().track(encoder);
            scene.capture_prepared_regions_reuse(r, packet, &destination, &regions, output, false, Some(&reserved), initialized, encoder, &mut accepted)});
    scene.used.iter_mut().for_each(scratch::Usage::reset); scene.used[..reserved.len()].clone_from_slice(&reserved); scene.placement_display = true;
    result
}

impl Evaluator<'_> {
    pub(super) fn effect(
        &mut self, input: &graph::Node, chain: &[(OccurrenceHandle, u64, u32)], masks: &[Option<graph::Node>],
        output: Option<Target>,
    ) -> Result<Value, GpuRasterError> {
        let handles: Vec<_> = chain.iter().map(|(handle, _, _)| *handle).collect();
        let effect = self.packet.scene.effect(handles[0]).unwrap();
        if native_alpha_capture(self.packet.scene, &handles, self.cache.plan.level) {
            return self.native_effect(&handles, output);
        }
        let boundary = !effect.program.passes.is_empty();
        let plan = self.working_plan();
        let region = self.region;
        let input_node_support = input.support(self.packet.scene, self.packet.document_extent,self.input.level);
        let regions = if boundary { crate::effects::document_pass_regions(effect, region, input_node_support, plan.level).into_iter().map(|region| region.intersect(plan.doc_bounds)).collect() } else { vec![region; 2] };
        let input = self.with_region(regions[0], |compositor| {
            let value = compositor.evaluate(input)?;
            let value = compositor.effect_input(value)?;
            if boundary && Convert::filter_input(compositor.packet, effect.program.space) == Convert::Decode {
                compositor.draw(value, Value::Color([0.; 4]), layer_core::LayerBlend::Normal, 4096, None)
            } else { Ok(value) }
        })?;
        let mut views = Box::new(std::array::from_fn(|_| self.r.empty_view.clone()));
        let mut slots = Vec::new();
        let mut present = 0u32;
        for (i, mask) in masks.iter().enumerate() {
            let Some(mask) = mask else { continue; };
            let value = self.evaluate(mask)?;
            let value = self.effect_input(value)?;
            views[i] = value.view().unwrap().clone();
            present |= 1 << i;
            slots.extend(value.slot());
        }
        let Target { view, slot, plan } = match output.as_ref().filter(|target| target.plan == plan).cloned() { Some(target) => target, None => self.target()? };
        let input_support = input_node_support;
        let mut previous = input.view().unwrap().clone();
        let mut previous_slot = None;
        for (pass, &region) in regions[1..].iter().enumerate() {
            let last = pass + 2 == regions.len();
            let (target, temporary) = if last { (view.clone(), None) } else {
                let target = self.target()?;
                (target.view, target.slot)
            };
            let [x, y, width, height] = paint_transform::texel_rect(plan.doc_bounds.local(region), 1 << plan.level);
            let mut front = plan;
            front.support = crate::effects::pass_input_support(effect, pass, input_support, plan.level).unwrap_or(input_support);
            let mut original = plan;
            original.support = input_support;
            let mut data = crate::effects::image_grid(plan, front, original);
            data[..4].copy_from_slice(&[x as f32, y as f32, width as f32, height as f32]);
            if boundary {
                data[11] = f32::from(last && present != 0);
            } else {
                data[6] = present as f32;
                data[8..11].fill(0.);
                data[16..24].fill(0.);
            }
            let stage = if boundary { crate::effects::Execution::Image(pass) } else { crate::effects::Execution::Fused };
            let prepared = self.scene.effects.prepare(self.r, self.packet.scene, &handles, stage, self.packet.time_seconds, plan.level, self.packet.blend_space)?;
            self.scene.jobs.push(Job::Effect {
                target: target.clone(), sources: [previous, input.view().unwrap().clone(), self.r.empty_view.clone(), self.r.empty_paper_view.clone()],
                data, prepared, masks: views.clone(),source_target:None,changed_cells:None,
            });
            self.release(previous_slot);
            previous_slot = temporary;
            previous = target;
        }
        for slot in input.slot().into_iter().chain(slots) { self.release(Some(slot)); }
        self.materialize(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false }, output)
    }

    fn native_effect(&mut self, handles: &[OccurrenceHandle], output: Option<Target>) -> Result<Value, GpuRasterError> {
        let target = match output { Some(target) => target, None => self.target()? };
        let handle = *handles.last().unwrap();
        if !self.packet.scene.effect(handle).unwrap().program.fusion_boundary() {
            let side = if target.plan.level == 0 { PAGE_SIZE } else { 1 << target.plan.level };
            let bounds = self.region.aligned(side).intersect(target.plan.doc_bounds);
            self.capture_native_jobs(handle, &target, bounds)?;
        } else {
            if self.cache.exact_tile.is_none() {
                let strip = Image::new(self.r, display_mips::Plan::at(exact_strip(self.packet.document_extent), 0), "exact composition working strip");
                self.cache.output_charge.resize(self.cache.output_charge.bytes() + strip.bytes());
                self.cache.exact_tile = Some(strip);
            }
            let mut strip = self.cache.exact_tile.clone().unwrap();
            let side = 1 << target.plan.level;
            let region = self.region.aligned(side).intersect(target.plan.doc_bounds);
            for y in (region.min[1]..region.max[1]).step_by(strip.texture.height() as usize) {
                for x in (region.min[0]..region.max[0]).step_by(strip.texture.width() as usize) {
                    let bounds = DocRect { min: [x, y], max: [(x + i64::from(strip.texture.width())).min(region.max[0]), (y + i64::from(strip.texture.height())).min(region.max[1])] };
                    let size = bounds.size()?;
                    strip.plan = display_mips::Plan::window(self.packet.document_extent, 0, PixelRect::full(size));
                    strip.plan.doc_bounds = bounds;
                    self.capture_native_jobs(handle, &Target { view: strip.view.clone(), slot: None, plan: strip.plan }, bounds)?;
                    let [dx, dy, width, height] = paint_transform::texel_rect(target.plan.doc_bounds.local(bounds), side);
                    let mut values = [0; 20];
                    values[..8].copy_from_slice(&[dx, dy, width, height, size[0], size[1], side, 0]);
                    self.scene.jobs.push(Job::Reduce { output: target.view.clone(), values, size: [width, height], kernel: super::super::Reduction::Pages,format:target.view.texture().format(),
                        sources: [strip.view.clone(), self.r.empty_view.clone(), self.r.empty_view.clone(), self.r.empty_view.clone()] });
                }
            }
        }
        Ok(target.value())
    }

    fn capture_native_jobs(&mut self, handle: OccurrenceHandle, target: &Target, region: DocRect) -> Result<(), GpuRasterError> {
        let (packet, plan, regions, origin) = capture_frame(self.packet, target, &[region])?;
        let initialized = self.cache.initialized_pages(target).filter(|_| target.plan.doc_bounds == DocRect::from(target.plan.bounds));
        let destination = Image { texture: target.view.texture().clone(), view: target.view.clone(), plan };
        let previous = self.scene.images.doc_bounds;
        self.scene.images.doc_bounds = previous.translated(origin.map(|v| -v));
        let display = std::mem::replace(&mut self.scene.placement_display, false);
        let result = (|| {
            for tile in regions.iter().flat_map(|region| page_coordinates(*region)) {
                let covered = regions.iter().fold(PixelRect::EMPTY, |bounds, region| bounds.union(page_rect(tile).intersect(*region))).intersect(plan.bounds);
                if covered.is_empty() { continue; }
                match self.scene.capture_prepared_tile_jobs(self.r, packet, &destination, tile, covered, scene::Output::EffectComposite(handle), initialized.as_ref(), self.accepted) {
                    Err(GpuRasterError::NotReady) => self.unit.waiting = true,
                    result => result?,
                }
            }
            Ok(())
        })();
        self.scene.images.doc_bounds = previous;
        self.scene.placement_display = display;
        result
    }

    pub(super) fn with_region<T>(&mut self, region: DocRect, evaluate: impl FnOnce(&mut Self) -> Result<T, GpuRasterError>) -> Result<T, GpuRasterError> {
        let previous = std::mem::replace(&mut self.region, region);
        let value = evaluate(self);
        self.region = previous;
        value
    }

    fn effect_input(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        let value = self.resample(value, None)?;
        if matches!(&value, Value::Image { opacity: 1., plan, preview: None, encode: false, .. } if *plan == self.working_plan()) { Ok(value) }
        else { self.draw(value, Value::Color([0.; 4]), layer_core::LayerBlend::Normal, 0, None) }
    }
}
