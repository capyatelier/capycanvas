use super::*;

impl Reduced<'_> {
    pub(super) fn effect(
        &mut self, input: &graph::Node, chain: &[(LayerId, u64, u32)], masks: &[Option<graph::Node>],
        output: Option<(wgpu::TextureView, Option<usize>)>,
    ) -> Result<Value, GpuRasterError> {
        let layers: Vec<_> = chain.iter().map(|(id, _, _)| {
            self.packet.layers.iter().find(|l| l.id == *id).unwrap()
        }).collect();
        let effect = layers[0].effect.as_ref().unwrap();
        let boundary = effect.program.image_boundary();
        let plan = self.cache.plan;
        let region = PixelRect::new(self.origin[0] << plan.level, self.origin[1] << plan.level,
            (self.origin[0] + self.size[0]) << plan.level, (self.origin[1] + self.size[1]) << plan.level).intersect(plan.bounds);
        let regions = if boundary { crate::effects::pass_regions(effect, region, plan) } else { vec![region; 2] };
        let input = self.with_region(regions[0], |compositor| {
            let value = compositor.evaluate(input)?;
            compositor.effect_input(value)
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
        let (view, slot) = output.unwrap_or_else(|| {
            let slot = self.cache.allocate(self.r);
            (self.cache.output[slot].view.clone(), Some(slot))
        });
        let mut previous = input.view().unwrap().clone();
        let mut previous_slot = None;
        for (pass, &region) in regions[1..].iter().enumerate() {
            let last = pass + 2 == regions.len();
            let (target, temporary) = if last { (view.clone(), None) } else {
                let slot = self.cache.allocate(self.r);
                (self.cache.output[slot].view.clone(), Some(slot))
            };
            let [x, y, width, height] = paint_transform::texel_rect(region, 1 << plan.level);
            let mut data = crate::effects::image_grid(plan, plan, plan);
            data[..4].copy_from_slice(&[x as f32, y as f32, width as f32, height as f32]);
            if boundary {
                data[9] = f32::from(layers[0].properties.clipped);
                data[11] = f32::from(last && present != 0);
            } else {
                data[6] = present as f32;
                data[16..24].fill(0.);
            }
            let stage = if boundary { crate::effects::Execution::Image(pass) } else { crate::effects::Execution::Fused };
            let prepared = self.scene.effects.prepare(self.r, &layers, stage, self.packet.time_seconds, plan.level)?;
            self.scene.jobs.push(Job::Effect {
                target: target.clone(), sources: [previous, input.view().unwrap().clone(), self.r.empty_view.clone()],
                data, prepared, masks: views.clone(),
            });
            self.scene.encode_jobs(self.r, self.encoder)?;
            if let Some(slot) = previous_slot { self.cache.used[slot] = false; }
            previous_slot = temporary;
            previous = target;
        }
        for slot in input.slot().into_iter().chain(slots) { self.cache.used[slot] = false; }
        Ok(Value::Image { view, slot, opacity: 1. })
    }

    fn with_region<T>(&mut self, region: PixelRect, evaluate: impl FnOnce(&mut Self) -> Result<T, GpuRasterError>) -> Result<T, GpuRasterError> {
        let (origin, size) = (self.origin, self.size);
        let [x, y, width, height] = paint_transform::texel_rect(region, 1 << self.cache.plan.level);
        self.origin = [x, y];
        self.size = [width, height];
        let value = evaluate(self);
        self.origin = origin;
        self.size = size;
        value
    }

    fn effect_input(&mut self, value: Value) -> Result<Value, GpuRasterError> {
        let value = self.resample(value)?;
        if value.view().is_some() && value.opacity() == 1. { Ok(value) }
        else { self.materialize(value, None) }
    }
}
