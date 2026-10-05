//! Validated WGSL execution for built-ins and programmable effects. Compatible
//! pointwise chains are fused; declared image passes share the same ABI helpers.
use super::*;
use layer_core::{EffectKind, EffectProgram, EffectView};
use std::{collections::HashMap, sync::Arc};
#[path = "effect_preparation.rs"]
mod preparation;
#[path = "effect_resources.rs"]
pub(crate) mod resources;

/// Only immutable GPU handles are shared with the compiler thread.
pub(super) trait Gpu {
    fn device(&self) -> &PipelineDevice;
    fn queue(&self) -> &wgpu::Queue;
    fn analysis_resource(&self, _occurrence: OccurrenceHandle) -> Option<Arc<resources::Resource>> { None }
    fn effect_time(&self, scene: SceneView<'_>, occurrence: OccurrenceHandle, elapsed: f32) -> f32 {
        captured_phase(scene,occurrence).unwrap_or_else(|| scene.effect(occurrence).unwrap().time_seconds(elapsed))
    }
}
fn captured_phase(scene: SceneView<'_>, occurrence: OccurrenceHandle) -> Option<f32> {
    let target=scene.effect_handle(occurrence)?;
    scene.evaluation_context()?.phases.iter().find(|(h,_)|*h==target).map(|(_,phase)|*phase)
}
pub(super) type Clocks = HashMap<OccurrenceHandle, (Arc<str>, layer_core::EffectClock)>;
impl Gpu for WgpuRasterizer {
    fn analysis_resource(&self, occurrence: OccurrenceHandle) -> Option<Arc<resources::Resource>> {
        self.effect_analyses.iter().find(|analysis| analysis.layer() == occurrence).map(|analysis| analysis.resource.clone())
    }
    fn effect_time(&self, scene: SceneView<'_>, occurrence: OccurrenceHandle, elapsed: f32) -> f32 {
        if let Some(phase)=captured_phase(scene,occurrence) { return phase; }
        let effect = scene.effect(occurrence).unwrap();
        self.effect_clocks.get(&occurrence).filter(|(id, _)| id.as_ref() == effect.program.id.as_ref())
            .map_or_else(|| effect.time_seconds(elapsed), |(_, clock)| clock.clone().advance(effect, elapsed))
    }
    fn device(&self) -> &PipelineDevice { &self.device }
    fn queue(&self) -> &wgpu::Queue { &self.queue }
}
pub(super) struct Context {
    pub device: PipelineDevice,
    pub queue: wgpu::Queue,
}
impl Gpu for Context {
    fn device(&self) -> &PipelineDevice {
        &self.device
    }
    fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

// WebGPU guarantees 16 sampled textures per stage. Two are scene inputs;
// the rest let ordinary aligned masks participate in the same fused shader.
pub(super) const MASK_SLOTS: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Execution {
    Fused,
    Image(usize),
    Preview,
}
/// A compiled variant: the execution, and the blend space of the composite
/// the effect blends onto. Linear variants convert nothing.
type Stage = (Execution, layer_core::BlendSpace);

pub(super) fn image_grid(output: display_mips::Plan, front: display_mips::Plan, original: display_mips::Plan) -> [f32; 32] {
    let mut data = [0.; 32];
    let [w, h] = output.size.map(|n| n as f32);
    data[..6].copy_from_slice(&[0., 0., w, h, w, h]);
    data[12..16].copy_from_slice(&[output.doc_bounds.min[0] as f32, output.doc_bounds.min[1] as f32,
        output.extent[0] as f32, output.extent[1] as f32]);
    for (offset, plan) in [(16, front), (20, original)] {
        data[offset..offset + 4].copy_from_slice(&[plan.doc_bounds.min[0] as f32, plan.doc_bounds.min[1] as f32,
            plan.bounds.width() as f32, plan.bounds.height() as f32]);
    }
    data[24..27].copy_from_slice(&[front, original, output].map(|p| (1 << p.level) as f32));
    data[28..30].copy_from_slice(&[output.bounds.width() as f32, output.bounds.height() as f32]);
    data[14..16].copy_from_slice(&front.support.min.map(|v| v as f32));
    data[30..32].copy_from_slice(&front.support.max.map(|v| v as f32));
    data[8..11].copy_from_slice(&[original.support.min[0] as f32, original.support.min[1] as f32, original.support.max[0] as f32]);
    data[27] = original.support.max[1] as f32;
    data
}

pub(super) fn pass_radius(pass: &layer_core::EffectPass, effect: EffectView<'_>, level: u32) -> Option<u32> {
    effect.spatial_radius(pass.sampling.radius(effect)?)?.checked_add((1 << level) - 1)
}

fn pass_expansion(effect: EffectView<'_>, pass: &layer_core::EffectPass, level: u32) -> Option<[u32; 2]> {
    let builtin = layer_core::bundled_effect_catalog().get(&effect.program.id)
        .is_some_and(|builtin| builtin.program().wgsl == effect.program.wgsl && builtin.program().passes == effect.program.passes
            && builtin.program().lookups == effect.program.lookups);
    let direction = if builtin && matches!(pass.entry.as_ref(), "capy_blur_h" | "capy_bloom_h") { Some(0) }
        else if builtin && effect.program.passes.first().is_some_and(|first| matches!(first.entry.as_ref(), "capy_blur_h" | "capy_bloom_h")) { Some(2) }
        else { None };
    let Some(column) = direction else { return Some([pass_radius(pass, effect, level)?; 2]); };
    let linear = effect.spatial.map_or([1., 0., 0., 1., 0., 0.], |spatial| spatial.mapping.0);
    let radius = f64::from(pass.sampling.radius(effect)?);
    let values = std::array::from_fn::<_, 2, _>(|axis| (linear[column + axis].abs() * radius).ceil() + f64::from((1 << level) - 1));
    if values.iter().any(|value| !value.is_finite() || *value > f64::from(u32::MAX)) { return None; }
    Some(values.map(|value| value as u32))
}

fn expanded(region: DocRect, radius: [u32; 2]) -> DocRect {
    if region.is_empty() { return region; }
    DocRect { min: std::array::from_fn(|axis| region.min[axis].saturating_sub(i64::from(radius[axis]))),
        max: std::array::from_fn(|axis| region.max[axis].saturating_add(i64::from(radius[axis]))) }
}

pub(super) fn pass_input_support(effect: EffectView<'_>, pass: usize, input: DocRect, _level: u32) -> Option<DocRect> {
    effect.program.passes.iter().take(pass).try_fold(input, |support, descriptor| Some(expanded(support, pass_expansion(effect, descriptor, 0)?)))
}

pub(super) fn document_pass_regions(effect: EffectView<'_>, output: DocRect, input: DocRect, level: u32) -> Vec<DocRect> {
    let mut regions = vec![output; effect.program.passes.len().max(1) + 1];
    for (index, pass) in effect.program.passes.iter().enumerate().rev() {
        let support = pass_input_support(effect, index, input, level).unwrap_or(input.union(output));
        regions[index] = pass_expansion(effect, pass, level).map_or(support, |radius| expanded(regions[index + 1], radius).clamped(support));
    }
    regions
}

pub(super) fn damage_radius(effect: EffectView<'_>, level: u32) -> Option<u32> {
    effect.program.passes.iter().try_fold(0u32, |radius, pass| radius.checked_add(pass_radius(pass, effect, level)?))
}

#[derive(Clone)]
pub(super) struct PreparedEffect {
    pub pipeline: Deferred<wgpu::RenderPipeline>,
    pub binding: wgpu::BindGroup,
    pub pointwise: bool,
    pub _resource: Arc<resources::Resource>,
}
struct CachedEffect {
    program: Arc<EffectProgram>,
    values: Vec<layer_core::EffectValue>,
    geometry: [[f32; 4]; 3],
}
impl CachedEffect {
    fn view(&self) -> EffectView<'_> { EffectView::new(&self.program, &self.values) }
    fn lut3d(&self) -> Option<&Arc<layer_core::Lut3d>> { self.view().lut3d() }
    fn gpu_parameters(&self, space: layer_core::color::RgbSpace) -> Result<Vec<[f32;4]>, String> {
        self.view().gpu_parameters(space)
    }
}
struct Instance {
    effects: Vec<CachedEffect>,
    properties: Vec<[f32; 4]>,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    compute_binding: wgpu::BindGroup,
    resource: Arc<resources::Resource>,
    pipelines: HashMap<Stage, Deferred<wgpu::RenderPipeline>>,
    lookups: Vec<preparation::State>,
    offsets: Vec<u32>,
}
pub(super) struct Effects {
    layout: wgpu::BindGroupLayout,
    pub masks: wgpu::BindGroupLayout,
    pub sources: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(
        Vec<Arc<EffectProgram>>,
        Stage,
        Deferred<wgpu::RenderPipeline>,
    )>,
    // Parameters and GPU tables are shared by every pass of the same chain.
    instances: HashMap<(Vec<OccurrenceHandle>, u32), Instance>,
    // Reuse the lookup key; ordinary painting/animation does not repack inputs.
    ids: (Vec<OccurrenceHandle>, u32),
    preparation: preparation::Preparation,
    pub compilations: u64,
}
impl Effects {
    pub(super) fn chain_ready(&self, scene: SceneView<'_>, layers: &[OccurrenceHandle], execution: Execution, space: layer_core::BlendSpace) -> bool {
        let programs: Vec<_> = layers.iter().filter_map(|&h| scene.effect(h).map(|e| e.program)).collect();
        self.pipelines.iter().any(|(chain, stage, pipeline)| *stage == (execution, space)
            && chain.iter().map(Arc::as_ref).eq(programs.iter().copied()) && pipeline.ready())
            && self.preparation.pipelines.iter().all(|(_, p)| p.ready())
    }
    /// Queue missing variants without starting compilation on the render path.
    /// The same cache serves document rendering and visible filter previews.
    pub(super) fn enqueue(&self, compiler: &startup::Compiler, priority: u8) -> bool {
        compiler.require(self.pipelines.iter().map(|(_, _, p)| p), priority)
            & compiler.require(self.preparation.pipelines.iter().map(|(_, p)| p), priority)
    }
    pub(super) fn enqueue_active(&self, compiler: &startup::Compiler, priority: u8) -> bool {
        compiler.require(self.instances.values().flat_map(|instance| instance.pipelines.values()), priority)
            & compiler.require(self.preparation.pending.iter().filter_map(|work| match work {
                preparation::Work::Dispatch(dispatch) => Some(&dispatch.pipeline),
                preparation::Work::Copy { .. } => None,
            }), priority)
    }
    pub(super) fn discard_instances(&mut self) {
        self.instances.clear();
        self.preparation.pending.clear();
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compile(&self) {
        for (_, _, pipeline) in &self.pipelines {
            pipeline.compile();
        }
        for (_, pipeline) in &self.preparation.pipelines {
            pipeline.compile();
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn compile_async(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>> {
        // Start every pipeline in the bounded program job before its error scopes
        // are popped. Futures own handles; no live effects/session borrow escapes.
        let futures: Vec<_> = self
            .pipelines
            .iter()
            .map(|(_, _, p)| p.compile_async())
            .chain(
                self.preparation
                    .pipelines
                    .iter()
                    .map(|(_, p)| p.compile_async()),
            )
            .collect();
        Box::pin(async move {
            let mut error = None;
            for future in futures {
                if let Err(next) = future.await {
                    error.get_or_insert(next);
                }
            }
            error.map_or(Ok(()), Err)
        })
    }
    /// Cold catalog publication only. Keep live instances until the shared
    /// document owner publishes; discard superseded compilation versions.
    pub fn retain_compilations(&mut self, programs: &[Arc<EffectProgram>]) {
        let mut keep = programs.to_vec();
        for instance in self.instances.values() {
            for effect in &instance.effects {
                if !keep.contains(&effect.program) {
                    keep.push(effect.program.clone());
                }
            }
        }
        self.pipelines
            .retain(|(chain, _, _)| chain.iter().all(|p| keep.contains(p)));
        self.preparation.retain_programs(&keep);
    }
    pub fn fork(&self) -> Self {
        Self {
            layout: self.layout.clone(),
            masks: self.masks.clone(),
            sources: self.sources.clone(),
            pipeline_layout: self.pipeline_layout.clone(),
            pipelines: self.pipelines.clone(),
            instances: HashMap::new(),
            ids: (Vec::new(), 0),
            preparation: self.preparation.fork(),
            compilations: 0,
        }
    }
    pub fn merge_validated(&mut self, other: Self) {
        for (programs, stage, pipeline) in other.pipelines {
            if !self
                .pipelines
                .iter()
                .any(|(p, s, _)| *p == programs && *s == stage)
            {
                self.pipelines.push((programs, stage, pipeline));
            }
        }
        self.preparation.merge(other.preparation);
    }
    pub fn encode_preparation(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        self.preparation.encode(encoder);
    }
    #[cfg(test)]
    pub fn preparation_count(&self) -> u64 {
        self.preparation.executions
    }
    pub fn storage_bytes(&self) -> u64 {
        self.instances.values().map(|i| i.buffer.size()).sum()
    }
    pub fn new(
        r: &WgpuRasterizer,
        uniforms: &wgpu::BindGroupLayout,
    ) -> Self {
        if let Some(cache) = &r.validated_effects {
            return cache.fork();
        }
        let sources = crate::bindings::layout(&r.device, "effect sources", &[
            crate::bindings::texture(0,wgpu::ShaderStages::FRAGMENT,true),
            crate::bindings::texture(1,wgpu::ShaderStages::FRAGMENT,true),
            crate::bindings::sampler(2,wgpu::ShaderStages::FRAGMENT,wgpu::SamplerBindingType::Filtering),
        ]);
        let layout = crate::bindings::layout(&r.device, "effect parameters", &[crate::bindings::buffer(
            0,
            wgpu::ShaderStages::FRAGMENT,
            wgpu::BufferBindingType::Storage { read_only: true },
            false,
            NonZeroU64::new(16),
        ), crate::bindings::buffer(1, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage {read_only: true}, false, NonZeroU64::new(16))]);
        let masks = crate::bindings::layout(
            &r.device,
            "effect mask inputs",
            &(0..MASK_SLOTS)
                .map(|i| crate::bindings::texture(i as u32, wgpu::ShaderStages::FRAGMENT, true))
                .collect::<Vec<_>>(),
        );
        let pipeline_layout = r
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WGSL effects ABI 5"),
                bind_group_layouts: &[Some(uniforms), Some(&sources), Some(&layout), Some(&masks)],
                immediate_size: 0,
            });
        Self {
            layout,
            masks,
            sources,
            pipeline_layout,
            pipelines: Vec::new(),
            instances: HashMap::new(),
            ids: (Vec::new(), 0),
            preparation: preparation::Preparation::new(&r.device),
            compilations: 0,
        }
    }
    pub fn retain(&mut self, scene: SceneView<'_>) {
        self.instances.retain(|(ids, _), instance| ids.iter().zip(&instance.effects).all(|(id, old)|
            scene.position(*id).is_some() && scene.includes(*id) && scene.effect(*id).is_some_and(|effect|
                old.program.as_ref() == effect.program && old.lut3d().map(|resource|resource.digest()) == effect.lut3d().map(|resource|resource.digest()))));
    }
    #[expect(clippy::too_many_arguments, reason = "Effect evaluation keeps scene, occurrence, phase, inputs and output space explicit")]
    pub fn prepare(
        &mut self,
        r: &impl Gpu,
        scene: SceneView<'_>,
        layers: &[OccurrenceHandle],
        stage: Execution,
        time: f32,
        level: u32,
        space: layer_core::BlendSpace,
    ) -> Result<PreparedEffect, GpuRasterError> {
        let execution = stage;
        let stage = (execution, space);
        self.ids.0.clear();
        self.ids.0.extend_from_slice(layers);
        self.ids.1 = level;
        let analysis = if let Some(layer) = layers.first().filter(|&&h| scene.effect(h).unwrap().program.analysis().is_some()) {
            Some(match r.analysis_resource(*layer) {
                Some(resource) => resource,
                None => r.device().effect_resources.lock().unwrap().get(r.device(), r.queue(), None)?,
            })
        } else { None };
        if let Some(old) = self.instances.get(&self.ids)
            && analysis.as_ref().is_none_or(|resource| Arc::ptr_eq(resource, &old.resource))
            && old
                .effects
                .iter()
                .zip(layers)
                .all(|(effect, layer)| scene.effect(*layer).is_some_and(|current| effect.program.as_ref() == current.program && effect.values == current.values && effect_geometry(scene, *layer).is_ok_and(|geometry| effect.geometry == geometry)))
            && old
                .properties
                .iter()
                .zip(layers)
                .all(|(a, layer)| *a == effect_properties(r.device(), scene, *layer, r.effect_time(scene, *layer, time), space))
            && let Some(pipeline) = old.pipelines.get(&stage)
        {
            return Ok(PreparedEffect {
                pipeline: pipeline.clone(),
                binding: old.binding.clone(),
                pointwise: execution == Execution::Fused,
                _resource: old.resource.clone(),
            });
        }
        let ids = self.ids.clone();
        let effects: Vec<_> = layers
            .iter()
            .map(|&h| {
                let occurrence = scene.occurrence(h).unwrap();
                let OccurrenceContent::Effect(handle) = occurrence.content else { unreachable!() };
                let application = scene.artwork().effects.get(handle).unwrap();
                Ok(CachedEffect {program: application.program.clone(), values: application.values.clone(), geometry: effect_geometry(scene, h)?})
            })
            .collect::<Result<_, GpuRasterError>>()?;
        let properties: Vec<_> = layers.iter().map(|l| effect_properties(r.device(), scene, *l, r.effect_time(scene, *l, time), space)).collect();
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for (effect, properties) in effects.iter().zip(&properties) {
            data.extend(effect.geometry);
            offsets.push(data.len() as u32);
            data.push(*properties);
            data.extend(effect.gpu_parameters(r.device().working_space()).map_err(GpuRasterError::Effect)?);
            data[*offsets.last().unwrap() as usize + 1][2] = (1 << level) as f32;
            data[*offsets.last().unwrap() as usize + 1][3] = crate::gradient::quantum(r.device().depth());
        }
        let programs: Vec<_> = effects.iter().map(|e| e.program.clone()).collect();
        let bytes: Vec<_> = data
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let pipeline = if let Some((_, _, pipeline)) = self
            .pipelines
            .iter()
            .find(|(p, s, _)| *p == programs && *s == stage)
        {
            pipeline.clone()
        } else {
            let source = shader_source(
                &programs,
                &offsets,
                execution,
                r.device().working_space(),
                r.device().hdr(),
                space,
            )?;
            validate_source(&source)?;
            let module = r
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("checked pointwise effect"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
            let (device, layout) = (r.device().clone(), self.pipeline_layout.clone());
            let pipeline = Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(
                    mode,
                    &device,
                    &layout,
                    &module,
                    "effect_fragment",
                    None,
                    device.working_format(),
                    "pointwise effect chain",
                )
            });
            self.pipelines.push((programs, stage, pipeline.clone()));
            self.compilations += 1;
            pipeline
        };
        let reusable = self
            .instances
            .get(&ids)
            .is_some_and(|old| old.buffer.size() == bytes.len() as u64 && old.offsets == offsets
                && old.properties == properties && old.effects.iter().zip(&effects)
                    .all(|(old, effect)| old.program == effect.program && old.values == effect.values && old.geometry == effect.geometry));
        let mut lookups = Vec::new();
        let mut dispatches = Vec::new();
        let mut copies = Vec::new();
        for (effect, &base) in effects.iter().zip(&offsets) {
            let mut parameters = Vec::new();
            let mut position = base + 2;
            for value in &effect.values {
                let size = if matches!(
                    value,
                    layer_core::EffectValue::Curve(_) | layer_core::EffectValue::Gradient(_)
                ) {
                    layer_core::EFFECT_TABLE_VECTORS as u32
                } else {
                    1
                };
                parameters.push([position, size]);
                position += size;
            }
            let directory = base as usize + 1 + data[base as usize + 1][0] as usize;
            for (i, definition) in effect.program.lookups.iter().enumerate() {
                let indices: Vec<_> = definition
                    .dependencies
                    .iter()
                    .map(|key| {
                        effect
                            .program
                            .parameters
                            .iter()
                            .position(|p| p.key == *key)
                            .unwrap()
                    })
                    .collect();
                let key = preparation::Key {
                    definition: definition.clone(),
                    geometry: base + 1,
                    inputs: indices.iter().map(|i| parameters[*i]).collect(),
                    output: base + 1 + data[directory + i][0] as u32,
                };
                let values: Vec<_> = indices.iter().map(|i| effect.values[*i].clone()).collect();
                let old = self
                    .instances
                    .get(&ids)
                    .and_then(|old| old.lookups.get(lookups.len()));
                if old.is_none_or(|old| old.key != key || old.values != values) {
                    dispatches.push((
                        self.preparation.pipeline(r.device(), &key)?,
                        definition.workgroups,
                    ));
                } else if !reusable {
                    copies.push((self.instances[&ids].buffer.clone(), u64::from(key.output) * 16, u64::from(definition.values) * 16));
                }
                lookups.push(preparation::State { key, values });
            }
        }
        let buffer = if reusable {
            self.instances[&ids].buffer.clone()
        } else {
            r.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("effect parameter values"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        for (i, (&base, effect)) in offsets.iter().zip(&effects).enumerate() {
            if reusable
                && self.instances[&ids].effects[i].program == effect.program
                && self.instances[&ids].effects[i].values == effect.values
                && self.instances[&ids].effects[i].geometry == effect.geometry
                && self.instances[&ids].properties[i] == properties[i]
            {
                continue;
            }
            let directory = base as usize + 1 + data[base as usize + 1][0] as usize;
            let end = directory + effect.program.lookups.len();
            r.queue().write_buffer(
                &buffer,
                u64::from(base - 3) * 16,
                &bytes[(base as usize - 3) * 16..end * 16],
            );
        }
        let resource = match analysis {
            Some(resource) => resource,
            None => r.device().effect_resources.lock().unwrap().get(r.device(), r.queue(), effects.first().and_then(|e| e.lut3d()).map(Arc::as_ref))?,
        };
        let binding = if reusable && Arc::ptr_eq(&resource, &self.instances[&ids].resource) {
            self.instances[&ids].binding.clone()
        } else {
            crate::bindings::group(r.device(), "effect parameter binding", &self.layout, [
                buffer.as_entire_binding(), resource.buffer.as_entire_binding(),
            ])
        };
        let compute_binding = if reusable {
            self.instances[&ids].compute_binding.clone()
        } else {
            crate::bindings::group(r.device(), "effect preparation binding", &self.preparation.layout, [
                buffer.as_entire_binding(),
            ])
        };
        for (source, offset, size) in copies {
            self.preparation.pending.push(preparation::Work::Copy { source, destination: buffer.clone(), offset, size });
        }
        for (pipeline, groups) in dispatches {
            self.preparation.pending.push(preparation::Work::Dispatch(preparation::Dispatch {
                pipeline,
                binding: compute_binding.clone(),
                groups,
            }));
        }
        let mut pipelines = HashMap::new();
        if let Some(old) = self.instances.get(&ids)
            && old
                .effects
                .iter()
                .map(|e| &e.program)
                .eq(effects.iter().map(|e| &e.program))
        {
            pipelines = old.pipelines.clone();
        }
        pipelines.insert(stage, pipeline.clone());
        let prepared = PreparedEffect {
            pipeline,
            binding: binding.clone(),
            pointwise: execution == Execution::Fused,
            _resource: resource.clone(),
        };
        self.instances.insert(
            ids,
            Instance {
                effects,
                properties,
                buffer,
                binding,
                compute_binding,
                resource,
                pipelines,
                lookups,
                offsets,
            },
        );
        Ok(prepared)
    }
}

fn effect_geometry(scene: SceneView<'_>, handle: OccurrenceHandle) -> Result<[[f32; 4]; 3], GpuRasterError> {
    let offset = scene.evaluation_offset64();
    let (mapping, extent) = scene.effect_application(handle).unwrap().spatial.as_ref()
        .map_or(([1., 0., 0., 1., 0., 0.], scene.composition().size.map(f64::from)),
            |reference| (reference.mapping.0, reference.extent));
    let [a, b, c, d, x, y] = mapping;
    let geometry = [[a as f32, b as f32, c as f32, d as f32],
        [(x + offset[0]) as f32, (y + offset[1]) as f32, extent[0] as f32, extent[1] as f32],
        [offset[0] as f32, offset[1] as f32, scene.composition().size[0] as f32, scene.composition().size[1] as f32]];
    let [a,b,c,d]=geometry[0];let determinant=a*d-b*c;
    if !geometry.iter().flatten().all(|value| value.is_finite()) || !determinant.is_finite() || determinant==0.
        || ![d/determinant,-b/determinant,-c/determinant,a/determinant].into_iter().all(f32::is_finite) {
        return Err(GpuRasterError::InvalidTransform("Effect coordinates exceed the GPU numerical range"));
    }
    Ok(geometry)
}

fn effect_properties(device: &PipelineDevice, scene: SceneView<'_>, handle: OccurrenceHandle, time: f32, space: layer_core::BlendSpace) -> [f32; 4] {
    let occurrence = scene.occurrence(handle).unwrap();
    [occurrence.opacity, crate::blend_code(occurrence.blend, device, space) as f32,
        scene.mask(handle).filter(|(use_, _)| use_.enabled).map_or(1., |(use_, coverage)| {
            if use_.inverted { 1. - coverage.default_coverage } else { coverage.default_coverage }
        }), time]
}

pub(super) fn parse_validated(source: &str) -> Result<naga::Module, GpuRasterError> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| GpuRasterError::Effect(e.emit_to_string(source)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
    Ok(module)
}
fn validate_source(source: &str) -> Result<(), GpuRasterError> {
    let module = parse_validated(source)?;
    if module.global_variables.len() != 9 + MASK_SLOTS
        || module.entry_points.len() != 3
        || !module.overrides.is_empty()
    {
        return Err(GpuRasterError::Effect(
            "Effects cannot add bindings, overrides or entry points".into(),
        ));
    }
    Ok(())
}

fn shader_source(
    programs: &[Arc<EffectProgram>],
    offsets: &[u32],
    stage: Execution,
    space: layer_core::color::RgbSpace,
    hdr: bool,
    blend: layer_core::BlendSpace,
) -> Result<String, GpuRasterError> {
    if programs.len() > 1 && programs.iter().any(|program| program.auxiliary.is_some()) {
        return Err(GpuRasterError::Effect("An auxiliary resource requires its own effect stage".into()));
    }
    // A Perceptual composite holds encoded values. Filters that read linear
    // values convert their input and output; a filter that follows the
    // document's Blending reads the composite as it is.
    let perceptual = blend == layer_core::BlendSpace::Perceptual;
    let input_encoded = stage != Execution::Fused && programs[0].space.encoded(blend);
    let converts = perceptual && !input_encoded;
    let encoded = |expression: String| if converts { format!("working_encode({expression})") } else { expression };
    let linear = |expression: &str| if converts { format!("working_decode({expression})") } else { expression.into() };
    let mut source = working_color::source(space);
    source.push_str(&crate::view_color::hdr_shader(space, layer_core::color::RgbSpace::Srgb));
    source.push_str(include_str!("blend_modes.wgsl"));
    source.push_str(include_str!("scene.wgsl"));
    let space_id = layer_core::color::RgbSpace::ALL
        .iter()
        .position(|s| *s == space)
        .unwrap();
    let y = space.to_xyz()[1];
    source.push_str(&format!("\nconst FX_EXTENDED:bool=true;\nconst FX_HDR:bool={hdr};\nconst FX_ENCODED:bool={input_encoded};\nconst FX_SPACE:u32={space_id}u;\nconst FX_LUMA:vec3<f32>=vec3<f32>({:.12},{:.12},{:.12});\n", y[0], y[1], y[2]));
    source.push_str(&format!("const FX_CLAMP_INPUT:bool={};\n", !programs[0].passes.is_empty()));
    source.push_str(include_str!("effects_color.wgsl"));
    source.push_str(crate::gradient::SOURCE);
    source.push_str(include_str!("float_number.wgsl"));
    source.push_str(include_str!("guide_luminance.wgsl"));
    for i in 0..MASK_SLOTS {
        source.push_str(&format!(
            "@group(3) @binding({i}) var effect_mask_{i}:texture_2d<f32>;\n"
        ));
    }
    source.push_str(
        r#"
@group(2) @binding(0) var<storage,read> effect_data:array<vec4<f32>>;
@group(2) @binding(1) var<storage,read> effect_auxiliary:array<vec4<u32>>;
fn fx_auxiliary_words(index:u32)->vec4<u32>{return effect_auxiliary[index];}
fn fx_auxiliary(index:u32)->vec4<f32> {return bitcast<vec4<f32>>(effect_auxiliary[index]);}
fn fx_parameter(base:u32,index:u32)->vec4<f32> { return effect_data[base+1u+index]; }
fn gradient_record(base:u32,index:u32)->vec4<f32>{
    return effect_data[base+index];
}
fn fx_gradient(base:u32,value:f32,position:vec2<f32>)->vec4<f32>{
    let sample=gradient_sample(base+1u,value);
    return gradient_dither(sample.color,position,effect_data[base].w,sample.dither);
}
fn fx_lookup(base:u32,table:u32,index:u32)->vec4<f32> {
    let directory=base+u32(effect_data[base].x);let entry=effect_data[directory+table];
    return effect_data[base+u32(entry.x)+min(index,u32(entry.y)-1u)];
}
fn fx_time(base:u32)->f32 { return effect_data[base-1u].w; }
var<private> fx_reference_linear:vec4<f32>;
var<private> fx_reference_frame:vec4<f32>;
var<private> fx_composition_frame:vec4<f32>;
fn fx_begin(base:u32) {
    fx_reference_linear=effect_data[base-4u];
    fx_reference_frame=effect_data[base-3u];
    fx_composition_frame=effect_data[base-2u];
}
fn fx_to_document(point:vec2<f32>)->vec2<f32> {
    let m=fx_reference_linear;
    return vec2(m.x*point.x+m.z*point.y,m.y*point.x+m.w*point.y)+fx_reference_frame.xy;
}
fn fx_from_document(point:vec2<f32>)->vec2<f32> {
    let m=fx_reference_linear;let q=point-fx_reference_frame.xy;let determinant=m.x*m.w-m.y*m.z;
    return vec2(m.w*q.x-m.z*q.y,-m.y*q.x+m.x*q.y)/determinant;
}
fn fx_extent()->vec2<f32> { return fx_reference_frame.zw; }
fn fx_in_frame(point:vec2<f32>)->bool {
    return all(point>=fx_composition_frame.xy)&&all(point<fx_composition_frame.xy+fx_composition_frame.zw);
}
fn fx_position(local:vec2<f32>)->vec2<f32> {
    let side=settings.operation_linear.z;
    if side<=0. {return settings.color.xy+local;}
    let low=floor(local)*side;
    return settings.color.xy+(low+min(low+side,settings.operation_offset.xy))*.5;
}
fn fx_generator_position(local:vec2<f32>)->vec2<f32> {
    let side=settings.operation_linear.z;
    if side<=0. {return fx_position(local);}
    let low=floor(local)*side;
    let a=max(settings.color.xy+low,fx_composition_frame.xy);
    let b=min(settings.color.xy+min(low+side,settings.operation_offset.xy),fx_composition_frame.xy+fx_composition_frame.zw);
    if any(b<=a) {return fx_position(local);}
    return (a+b)*.5;
}
fn fx_load(image:texture_2d<f32>,point:vec2<i32>)->vec4<f32> {
    if any(point<vec2<i32>(0))||any(point>=vec2<i32>(textureDimensions(image))) {return vec4(0.);}
    return textureLoad(image,point,0);
}
fn fx_window_gather(image:texture_2d<f32>,origin:vec2<i32>,f:vec2<f32>)->vec4<f32> {
    let a=fx_load(image,origin);
    if f.x==0. {
        if f.y==0. {return a;}
        return working_sample_mix(a,fx_load(image,origin+vec2<i32>(0,1)),f.y);
    }
    let b=fx_load(image,origin+vec2<i32>(1,0));
    if f.y==0. {return working_sample_mix(a,b,f.x);}
    let c=fx_load(image,origin+vec2<i32>(0,1));let d=fx_load(image,origin+vec2<i32>(1,1));
    return working_sample_mix(working_sample_mix(a,b,f.x),working_sample_mix(c,d,f.x),f.y);
}
fn fx_window_sample(image:texture_2d<f32>,point:vec2<f32>)->vec4<f32> {
    let p=point-.5;
    return fx_window_gather(image,vec2<i32>(floor(p)),fract(p));
}
fn fx_grid_unit_sample(image:texture_2d<f32>,point:vec2<f32>,grid:vec2<f32>)->vec4<f32> {
    let p=point-.5;let phase=fract(p)-fract(grid);let carry=floor(phase);
    let origin=vec2<i32>(floor(p)-floor(grid))+vec2<i32>(carry);
    return fx_window_gather(image,origin,fract(phase));
}
fn fx_finite_support_bounds(bounds:vec4<f32>,origin:vec2<f32>,step:f32)->vec4<f32> {
    let first=origin+floor((bounds.xy-origin)/step)*step;
    let last=origin+(ceil((bounds.zw-origin)/step)-1.)*step;
    return vec4((max(bounds.xy,first)+min(bounds.zw,first+step))*.5,(max(bounds.xy,last)+min(bounds.zw,last+step))*.5);
}
fn fx_support_bounds(step:f32)->vec4<f32> {
    return fx_finite_support_bounds(vec4(settings.color.zw,settings.operation_offset.zw),settings.source_over.xy,max(step,.00001));
}
fn fx_grid_coordinate(point:f32,low:f32,high:f32,origin:f32,step:f32)->f32 {
    let first=floor((low-origin)/step);let last=ceil((high-origin)/step)-1.;
    let a=(max(low,origin+first*step)+min(high,origin+(first+1.)*step))*.5;
    let b=(max(low,origin+last*step)+min(high,origin+(last+1.)*step))*.5;
    if first==last {return first+.5;}
    if last==first+1. {return first+.5+(point-a)/(b-a);}
    let q=(point-origin)/step;
    let next=origin+(first+1.5)*step;let previous=origin+(last-.5)*step;
    if point<next {return first+.5+(point-a)/(next-a);}
    if point>previous {return last-.5+(point-previous)/(b-previous);}
    return q;
}
fn fx_grid_sample(image:texture_2d<f32>,point:vec2<f32>,grid:vec4<f32>,step:f32,bounds:vec4<f32>)->vec4<f32> {
    if grid.z<=0. {return fx_window_sample(image,point);}
    if any(bounds.zw<=bounds.xy) {return vec4(0.);}
    if !FX_CLAMP_INPUT&&(any(point<bounds.xy)||any(point>=bounds.zw)) {return vec4(0.);}
    let support=fx_finite_support_bounds(bounds,grid.xy,step);
    if any(support.zw<support.xy) {return vec4(0.);}
    let sample=select(point,clamp(point,support.xy,support.zw),FX_CLAMP_INPUT);
    if step==1. {return fx_grid_unit_sample(image,sample,grid.xy);}
    return fx_window_sample(image,vec2(fx_grid_coordinate(sample.x,bounds.x,bounds.z,grid.x,step),
        fx_grid_coordinate(sample.y,bounds.y,bounds.w,grid.y,step)));
}
fn fx_reference_sample(image:texture_2d<f32>,p:vec2<f32>,grid:vec4<f32>,step:f32,bounds:vec4<f32>)->vec4<f32> {
    if grid.z<=0.||step!=1. {return fx_grid_sample(image,fx_to_document(p),grid,step,bounds);}
    let m=fx_reference_linear;let point=vec2(m.x*p.x+m.z*p.y,m.y*p.x+m.w*p.y);
    let offset=fx_reference_frame.xy;
    return fx_grid_sample(image,point,vec4(grid.xy-offset,grid.zw),step,bounds-vec4(offset,offset));
}
fn fx_sample(p:vec2<f32>)->vec4<f32> {
    return fx_reference_sample(front,p,settings.source_over,settings.operation_linear.x,vec4(settings.color.zw,settings.operation_offset.zw));
}
fn fx_sample_bounds()->vec4<f32> {
    let support=fx_support_bounds(settings.operation_linear.x);
    let a=fx_from_document(support.xy);let b=fx_from_document(support.zw);
    let c=fx_from_document(vec2(support.x,support.w));let d=fx_from_document(vec2(support.z,support.y));
    return vec4(min(min(a,b),min(c,d)),max(max(a,b),max(c,d)));
}
fn fx_original(p:vec2<f32>)->vec4<f32> {
    return fx_reference_sample(back,p,settings.backdrop,settings.operation_linear.y,fx_original_support());
}
@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {
    return effect_result(v);
}

"#,
    );
    source.push_str(include_str!("effect_tables.wgsl"));
    source.push_str(if stage == Execution::Fused {
        "fn fx_original_support()->vec4<f32>{return vec4(settings.color.zw,settings.operation_offset.zw);}\n"
    } else {
        "fn fx_original_support()->vec4<f32>{return vec4(settings.options.xyz,settings.operation_linear.w);}\n"
    });
    source.push_str(include_str!("tetrahedron.wgsl"));
    for chosen in layer_core::color::RgbSpace::ALL {
        let index = chosen.shader_code();
        source.push_str(&crate::view_color::transform(&format!("cube_to_{index}"), space, chosen));
        source.push_str(&crate::view_color::transform(&format!("cube_from_{index}"), chosen, space));
        source.push_str(&format!("const CUBE_LIMIT_{index}:f32={:.12e};\n", chosen.encode(f64::from(f32::MAX) * (1.-2048.*f64::from(f32::EPSILON)))));
    }
    source.push_str(include_str!("effect_cube.wgsl"));
    let mut included = Vec::<&str>::new();
    for program in programs {
        for part in program
            .wgsl
            .sources()
            .map_err(|e| GpuRasterError::Effect(e.into()))?
        {
            if !included.contains(&part.as_ref()) {
                source.push_str(part);
                source.push('\n');
                included.push(part);
            }
        }
    }
    if stage == Execution::Preview {
        let p = &programs[0];
        let base = offsets[0] + 1;
        let shown = |expression: String| if input_encoded { format!("working_decode({expression})") } else { expression };
        let position=if p.kind==EffectKind::Generator {"fx_generator_position"} else {"fx_position"};
        source.push_str(&format!("fn effect_result(v:Vertex)->vec4<f32> {{fx_begin({base}u);let position=fx_from_document({position}(v.position.xy));let c=fx_sample(position);\n"));
        if p.kind == EffectKind::Generator {source.push_str("if !fx_in_frame(fx_to_document(position)) {return vec4(0.); }\n");}
        for (j, pass) in p.passes.iter().enumerate() {
            let result = format!("{}(c,position,{base}u)", pass.entry);
            let result = if j + 1 == p.passes.len() { shown(result) } else { result };
            source.push_str(&format!("if u32(settings.extent.w)=={j}u {{return {result};}}\n"));
        }
        source.push_str(&format!("return {};}}", shown(format!("{}(c,position,{base}u)", p.entry))));
        return Ok(source);
    }
    if let Execution::Image(stage) = stage {
        let p = &programs[0];
        let entry = p.passes.get(stage).map_or(&p.entry, |p| &p.entry);
        let last = stage + 1 >= p.passes.len();
        let base = offsets[0] + 1;
        let adjusted = format!("{entry}(fx_sample(position),position,{base}u)");
        let adjusted = if last { encoded(adjusted) } else { adjusted };
        let position=if p.kind==EffectKind::Generator {"fx_generator_position"} else {"fx_position"};
        source.push_str(&format!("fn effect_result(v:Vertex)->vec4<f32> {{fx_begin({base}u); let position=fx_from_document({position}(v.position.xy)); let adjusted={adjusted};\n"));
        if p.kind == EffectKind::Generator {source.push_str("if !fx_in_frame(fx_to_document(position)) {return vec4(0.); }\n");}
        if last && p.kind == EffectKind::Adjustment {
            source.push_str(&format!("let c={};let controls=effect_data[{}u];var coverage=controls.z;if settings.options.w>.5 {{coverage=textureLoad(effect_mask_0,vec2<i32>(v.position.xy),0).a;}}", encoded("fx_original(position)".into()), offsets[0]));
            if p.alpha == layer_core::EffectAlpha::Filter {
                source.push_str("return fx_filter(c,adjusted,u32(controls.y),controls.x*coverage);}");
            } else {
                source.push_str("return fx_adjustment(c,adjusted,u32(controls.y),controls.x*coverage);}");
            }
        } else {
            source.push_str("return adjusted;}");
        }
        return Ok(source);
    }
    source.push_str(
        r#"
fn effect_result(v:Vertex)->vec4<f32> {
    let local=select(v.position.xy-settings.rect.xy,v.position.xy,settings.operation_linear.z>0.);
    var c=textureLoad(front,vec2<i32>(local),0);
    if settings.source_over.w>.5 {
"#,
    );
    if perceptual {
        source.push_str("        c=working_encode(c);\n");
    }
    source.push_str(
        r#"        let coverage=select(settings.source_over.y,mix(textureLoad(back,vec2<i32>(local),0).r,1.-textureLoad(back,vec2<i32>(local),0).r,settings.source_over.z-2.),settings.source_over.z>=2.);
        c*=settings.source_over.x*coverage;
        if settings.source_over.w<1.5 {c+=settings.backdrop*(1.-c.a);}
    }
    let mask=select(1.,textureLoad(back,vec2<i32>(local),0).a,settings.options.w>.5);
    let position=fx_position(local);
"#,
    );
    for (i, (p, offset)) in programs.iter().zip(offsets).enumerate() {
        if p.kind == EffectKind::Generator && programs.len() != 1 {
            return Err(GpuRasterError::Effect(
                "Generators cannot be fused as adjustments".into(),
            ));
        }
        source.push_str(&format!(
            "{{fx_begin({}u); {}let adjusted={}; let controls=effect_data[{}u];\n",
            offset + 1,
            if p.kind==EffectKind::Generator {"let position=fx_generator_position(local); "} else {""},
            encoded(format!("{} ({},fx_from_document(position),{}u)", p.entry, linear("c"), offset + 1)),
            offset
        ));
        if p.kind == EffectKind::Adjustment {
            source.push_str("var coverage=controls.z; if settings.options.w>.5 {coverage=mask;}\n");
            if i < MASK_SLOTS {
                let bit = 1u32 << i;
                source.push_str(&format!("else if (u32(settings.extent.z)&{bit}u)!=0u {{let m=textureLoad(effect_mask_{i},vec2<i32>(local),0).r;coverage=select(m,1.-m,(u32(settings.extent.w)&{bit}u)!=0u);}}\n"));
            }
            source.push_str(if p.alpha == layer_core::EffectAlpha::Filter { "c=fx_filter(c,adjusted,u32(controls.y),controls.x*coverage); }\n" } else { "c=fx_adjustment(c,adjusted,u32(controls.y),controls.x*coverage); }\n" });
        } else {
            source.push_str("c=select(vec4(0.),adjusted,fx_in_frame(position)); }\n");
        }
    }
    source.push_str("if settings.options.x>.5 {c=blend_composite(select(c*settings.options.y,c,settings.options.y==1.),settings.backdrop,u32(settings.options.z));} return c; }\n");
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures;
    #[test]
    fn builtin_shaders_and_saved_fixture_compile_with_the_current_catalog() {
        use layer_core::package::{codec::{open,OpenOutcome},ImmutableBacking,transport::ChunkedBytes};
        let bytes=include_bytes!("../../layer-core/src/package/codec/fixtures/authored-filters.capy");
        let backing=ImmutableBacking::new(Arc::new(ChunkedBytes::new(vec![Arc::from(bytes.as_slice())]).unwrap())).unwrap();
        let OpenOutcome::Candidate {artwork,..}=open(backing,Default::default(),&std::sync::atomic::AtomicBool::new(false)).unwrap() else {panic!("saved fixture must be editable")};
        let mut programs:std::collections::BTreeMap<_,_>=fixtures().iter().map(|f|(f.id().to_string(),f.program())).collect();
        for (_,_,application) in artwork.effects.iter() {programs.insert(application.program.id.to_string(),application.program.clone());}
        let programs:Vec<_>=programs.into_values().collect();
        for p in &programs {
            validate(std::slice::from_ref(p), Execution::Preview);
            if p.image_boundary() {
                for stage in 0..p.passes.len().max(1) {
                    validate(std::slice::from_ref(p), Execution::Image(stage));
                }
            } else {
                validate(std::slice::from_ref(p), Execution::Fused);
            }
        }
        validate(
            &programs
                .into_iter()
                .filter(|p| !p.fusion_boundary() && p.kind == EffectKind::Adjustment)
                .collect::<Vec<_>>(),
            Execution::Fused,
        );
    }
    #[test]
    fn intermediate_support_tracks_preceding_separable_pass_in_authored_axes() {
        use layer_core::authored::*;
        let mut effect=layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
        effect.set("sigma",layer_core::EffectValue::Number(2.)).unwrap();
        let application=EffectApplication::new(effect.program,effect.values,[32;2]);
        let input=DocRect {min:[-5,7],max:[1029,517]};
        for (linear,axis) in [([1.,0.,0.,1.,0.,0.],0),([0.,1.,-1.,0.,0.,0.],1)] {
            let mut spatial=application.spatial.unwrap();spatial.mapping=Affine64(linear);
            let view=EffectView::new(&application.program,&application.values).with_spatial(Some(&spatial));
            let radius=view.program.passes[0].sampling.radius(view).unwrap();
            let mut expected=input;
            expected.min[axis]-=i64::from(radius);expected.max[axis]+=i64::from(radius);
            for level in [0,1,3] {
                assert_eq!(pass_input_support(view,0,input,level),Some(input));
                assert_eq!(pass_input_support(view,1,input,level),Some(expected));
            }
        }
    }
    #[test]
    fn finite_authored_coordinates_outside_gpu_range_are_refused_before_encoding() {
        use layer_core::authored::*;
        let mut artwork=Artwork::new([32,24]).unwrap();
        let program=layer_core::bundled_effect_catalog().get("vignette").unwrap().program();
        let application=EffectApplication::new(program.clone(),layer_core::EffectInstance::new(program).values,[32,24]);
        let effect=artwork.effects.insert(PortableId::random(),application).unwrap();
        let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),"Vignette")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        for mapping in [[1.,0.,0.,1.,1e300,0.],[1e50,0.,0.,1e50,0.,0.],[1e-50,0.,0.,1e-50,0.,0.]] {
            artwork.effects.get_mut(effect).unwrap().spatial.as_mut().unwrap().mapping=Affine64(mapping);
            let document=layer_core::Document::from_artwork(artwork.clone()).unwrap();
            assert!(matches!(effect_geometry(document.scene(),occurrence),Err(GpuRasterError::InvalidTransform(_))));
        }
    }
    #[test]
    fn custom_filters_with_shared_function_names_compile_independently() {
        let builtin=fixtures()[0].program();
        let mut custom=(*builtin).clone();
        custom.id="custom_filter".into();
        custom.wgsl=format!("{}\n",custom.wgsl.sources().unwrap().join("\n")).into();
        assert!(custom.fusion_boundary());
        validate(&[builtin],Execution::Fused);
        validate(&[Arc::new(custom)],Execution::Fused);
    }
    fn validate(p: &[Arc<EffectProgram>], execution: Execution) {
        for blend in layer_core::BlendSpace::ALL {
            let source = shader_source(p, &vec![3; p.len()], execution, Default::default(), false, blend).unwrap();
            validate_source(&source).unwrap();
        }
    }
}
