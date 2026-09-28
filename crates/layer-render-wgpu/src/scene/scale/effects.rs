use super::*;

impl Reduced<'_> {
    pub(super) fn effect(
        &mut self, input: Value, chain: &[(LayerId, u64, u32)], masks: &[Option<graph::Node>],
        output: Option<(wgpu::TextureView, Option<usize>)>,
    ) -> Result<Value, GpuRasterError> {
        let input = self.effect_input(input)?;
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
        let (view, slot) = output.unwrap_or_else(|| {
            let slot = self.cache.allocate(self.r);
            (self.cache.output[slot].view.clone(), Some(slot))
        });
        let layers: Vec<_> = chain.iter().map(|(id, _, _)| {
            self.packet.layers.iter().find(|l| l.id == *id).unwrap()
        }).collect();
        let mut data = crate::effects::image_grid(self.cache.plan, self.cache.plan, self.cache.plan);
        data[..4].copy_from_slice(&[self.origin[0] as f32, self.origin[1] as f32, self.size[0] as f32, self.size[1] as f32]);
        data[6] = present as f32;
        data[16..24].fill(0.);
        let prepared = self.scene.effects.prepare(self.r, &layers, crate::effects::Execution::Fused, self.packet.time_seconds)?;
        self.scene.jobs.push(Job::Effect {
            target: view.clone(), sources: [input.view().unwrap().clone(), self.r.empty_view.clone(), self.r.empty_view.clone()],
            data, prepared, masks: views,
        });
        self.scene.encode_jobs(self.r, self.encoder)?;
        for slot in input.slot().into_iter().chain(slots) { self.cache.used[slot] = false; }
        Ok(Value::Image { view, slot, opacity: 1. })
    }

    fn effect_input(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        let value = self.resample(value)?;
        if value.view().is_some() && value.opacity() == 1. { Ok(value) }
        else { self.materialize(value, None) }
    }
}
