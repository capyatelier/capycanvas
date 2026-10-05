use super::*;

pub(super) fn native_alpha_capture(scene: layer_core::SceneView<'_>, handles: &[OccurrenceHandle], level: u32) -> bool {
    handles.iter().any(|handle| native_pointwise_alpha(&scene.effect(*handle).unwrap().program))
        && (level > 0 || handles.iter().all(|handle| !scene.effect(*handle).unwrap().program.fusion_boundary()))
}

#[expect(clippy::too_many_arguments, reason = "Native capture keeps output ownership, initialized pages and exact regions explicit")]
pub(super) fn capture_native_effect(
    scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
    target: &Target, regions: &[DocRect], initialized: Option<&BTreeSet<[u32; 2]>>, encoder: &mut submission::CommandEncoder,
) -> Result<(), GpuRasterError> {
    let reserved = scene.used.clone();
    let rebased = target.plan.doc_bounds != DocRect::from(target.plan.bounds);
    let (packet, plan, regions, initialized) = if rebased {
        let origin = target.plan.doc_bounds.min;
        let size = target.plan.doc_bounds.size()?;
        let packet = FramePacket { scene: packet.scene.with_offset64(origin.map(|value| -(value as f64))), document_extent: size, ..packet };
        (packet, display_mips::Plan::at(size, target.plan.level), regions.iter().map(|region| target.plan.doc_bounds.local(*region)).collect::<Vec<_>>(), None)
    } else {
        (packet, target.plan, regions.iter().map(|region| region.in_frame(packet.document_extent)).collect::<Vec<_>>(), initialized)
    };
    let bounds = regions.iter().fold(PixelRect::EMPTY, |bounds, region| bounds.union(*region));
    let destination = Image { texture: target.view.texture().clone(), view: target.view.clone(), plan };
    let window = scene.cached_capture_window(packet.scene, bounds);
    let result = scene.prepare_region_in_frame(r, packet, window, PixelRect::EMPTY, false, encoder)
        .and_then(|()| scene.capture_prepared_regions_reuse(r, packet, &destination, &regions,
            scene::Output::EffectComposite(handle), false, Some(&reserved), initialized, encoder));
    scene.used.fill(false); scene.used[..reserved.len()].copy_from_slice(&reserved); scene.placement_display = true;
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
        let boundary = effect.program.image_boundary();
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
        let Target { view, slot, plan } = output.as_ref().filter(|target| target.plan == plan).cloned().unwrap_or_else(|| self.target());
        let input_support = input_node_support;
        let mut previous = input.view().unwrap().clone();
        let mut previous_slot = None;
        for (pass, &region) in regions[1..].iter().enumerate() {
            let last = pass + 2 == regions.len();
            let (target, temporary) = if last { (view.clone(), None) } else {
                let target = self.target();
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
                target: target.clone(), sources: [previous, input.view().unwrap().clone(), self.r.empty_view.clone()],
                data, prepared, masks: views.clone(),source_target:None,changed_cells:None,
            });
            self.encode_scene_jobs()?;
            self.release(previous_slot);
            previous_slot = temporary;
            previous = target;
        }
        for slot in input.slot().into_iter().chain(slots) { self.release(Some(slot)); }
        self.materialize(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false }, output)
    }

    fn native_effect(&mut self, handles: &[OccurrenceHandle], output: Option<Target>) -> Result<Value, GpuRasterError> {
        self.encode_scene_jobs()?;
        self.commands.flush(self.r, self.encoder)?;
        let target = output.unwrap_or_else(|| {
            let plan = self.working_plan();
            let slot = self.cache.allocate(self.r, plan);
            Target { view: self.cache.output[slot].view.clone(), slot: Some(Slot::Cache(slot)), plan }
        });
        let handle = *handles.last().unwrap();
        if !self.packet.scene.effect(handle).unwrap().program.fusion_boundary() {
            let side=if target.plan.level==0 {PAGE_SIZE} else {1<<target.plan.level};
            let bounds=self.region.aligned(side).intersect(target.plan.doc_bounds);
            let initialized=self.cache.initialized_pages(&target);
            capture_native_effect(self.scene,self.r,self.packet,handle,&target,&[bounds],initialized,self.encoder)?;
            return Ok(target.value());
        }
        let mut strip = self.cache.exact_tile.get_or_insert_with(|| Image::new(self.r,
            display_mips::Plan::at(exact_strip(self.packet.document_extent), 0), "exact composition working strip")).clone();
        let side = 1 << target.plan.level;
        let region = self.region.aligned(side).intersect(target.plan.doc_bounds);
        for y in (region.min[1]..region.max[1]).step_by(PAGE_SIZE as usize) {
            for x in (region.min[0]..region.max[0]).step_by(strip.texture.width() as usize) {
                let bounds = DocRect { min: [x, y], max: [(x + i64::from(strip.texture.width())).min(region.max[0]), (y + i64::from(PAGE_SIZE)).min(region.max[1])] };
                let size = bounds.size()?;
                strip.plan = display_mips::Plan::window(self.packet.document_extent, 0, PixelRect::full(size));
                strip.plan.doc_bounds = bounds;
                let target_strip = Target { view: strip.view.clone(), slot: None, plan: strip.plan };
                capture_native_effect(self.scene, self.r, self.packet, handle, &target_strip, &[bounds], None, self.encoder)?;
                let [dx, dy, width, height] = paint_transform::texel_rect(target.plan.doc_bounds.local(bounds), side);
                let mut values = [0; 20];
                values[..8].copy_from_slice(&[dx, dy, width, height, size[0], size[1], side, 0]);
                let binding = Commands::binding(self.r, &strip.view, &self.r.empty_view, &target.view);
                self.commands.reduce(self.r, self.encoder, values, &binding, "reduce native effect output")?;
            }
        }
        Ok(target.value())
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
