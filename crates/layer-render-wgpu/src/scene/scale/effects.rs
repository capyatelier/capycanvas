use super::*;

impl Evaluator<'_> {
    pub(super) fn effect(
        &mut self, input: &graph::Node, chain: &[(OccurrenceHandle, u64, u32)], masks: &[Option<graph::Node>],
        output: Option<Target>,
    ) -> Result<Value, GpuRasterError> {
        let handles: Vec<_> = chain.iter().map(|(handle, _, _)| *handle).collect();
        let effect = self.packet.scene.effect(handles[0]).unwrap();
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
                data, prepared, masks: views.clone(),
            });
            self.encode_scene_jobs()?;
            self.release(previous_slot);
            previous_slot = temporary;
            previous = target;
        }
        for slot in input.slot().into_iter().chain(slots) { self.release(Some(slot)); }
        self.materialize(Value::Image { view, slot, opacity: 1., plan, preview: None, encode: false }, output)
    }

    fn with_region<T>(&mut self, region: DocRect, evaluate: impl FnOnce(&mut Self) -> Result<T, GpuRasterError>) -> Result<T, GpuRasterError> {
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
