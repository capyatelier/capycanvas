//! Validated WGSL execution for built-ins and programmable effects. Compatible
//! pointwise chains are fused; declared image passes share the same ABI helpers.
use super::*;
use layer_core::{EffectInstance, EffectKind, EffectProgram, EffectView};
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
    data[12..16].copy_from_slice(&[output.bounds.min_x() as f32, output.bounds.min_y() as f32,
        output.extent[0] as f32, output.extent[1] as f32]);
    for (offset, plan) in [(16, front), (20, original)] {
        data[offset..offset + 4].copy_from_slice(&[plan.bounds.min_x() as f32, plan.bounds.min_y() as f32,
            plan.bounds.width() as f32, plan.bounds.height() as f32]);
    }
    data[24..27].copy_from_slice(&[front, original, output].map(|p| (1 << p.level) as f32));
    data[28..30].copy_from_slice(&[output.bounds.width() as f32, output.bounds.height() as f32]);
    data
}

pub(super) fn pass_radius(pass: &layer_core::EffectPass, effect: EffectView<'_>, level: u32) -> Option<u32> {
    pass.sampling.radius(effect)?.checked_add((1 << level) - 1)
}

pub(super) fn damage_radius(effect: EffectView<'_>, level: u32) -> Option<u32> {
    effect.program.passes.iter().try_fold(0u32, |radius, pass| radius.checked_add(pass_radius(pass, effect, level)?))
}

pub(super) fn dependency(region: PixelRect, radius: Option<u32>, plan: display_mips::Plan) -> PixelRect {
    if region.is_empty() { return region; }
    radius.map_or(plan.bounds, |radius| region.expand(radius, plan.extent).intersect(plan.bounds))
}

pub(super) fn pass_regions(effect: EffectView<'_>, output: PixelRect, plan: display_mips::Plan) -> Vec<PixelRect> {
    let mut regions = vec![output; effect.program.passes.len().max(1) + 1];
    for (i, pass) in effect.program.passes.iter().enumerate().rev() {
        regions[i] = dependency(regions[i + 1], pass_radius(pass, effect, plan.level), plan);
    }
    regions
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
        sources: &wgpu::BindGroupLayout,
    ) -> Self {
        if let Some(cache) = &r.validated_effects {
            return cache.fork();
        }
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
                label: Some("WGSL effects ABI 4"),
                bind_group_layouts: &[Some(uniforms), Some(sources), Some(&layout), Some(&masks)],
                immediate_size: 0,
            });
        Self {
            layout,
            masks,
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
        if let Some(old) = self.instances.get_mut(&self.ids)
            && analysis.as_ref().is_none_or(|resource| Arc::ptr_eq(resource, &old.resource))
            && old
                .effects
                .iter()
                .zip(layers)
                .all(|(effect, layer)| scene.effect(*layer).is_some_and(|current| effect.program.as_ref() == current.program && effect.values == current.values))
            && old
                .properties
                .iter()
                .zip(layers)
                .all(|(a, layer)| a[..3] == effect_properties(r.device(), scene, *layer, time, space)[..3])
            && let Some(pipeline) = old.pipelines.get(&stage)
        {
            // Animation updates only one scalar per instance, never its LUTs.
            for (i, (properties, layer)) in old.properties.iter_mut().zip(layers).enumerate() {
                let seconds = r.effect_time(scene, *layer, time);
                if properties[3] != seconds {
                    r.queue().write_buffer(
                        &old.buffer,
                        old.offsets[i] as u64 * 16 + 12,
                        &seconds.to_le_bytes(),
                    );
                    properties[3] = seconds;
                }
            }
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
                let definition = scene.artwork().definitions.get(application.definition).unwrap();
                CachedEffect {program: definition.program.clone(), values: application.values.clone()}
            })
            .collect();
        let properties: Vec<_> = layers.iter().map(|l| effect_properties(r.device(), scene, *l, r.effect_time(scene, *l, time), space)).collect();
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for (effect, properties) in effects.iter().zip(&properties) {
            offsets.push(data.len() as u32);
            data.push(*properties);
            data.extend(effect.gpu_parameters(r.device().working_space()).map_err(GpuRasterError::Effect)?);
            data[*offsets.last().unwrap() as usize + 1][2] = (1 << level) as f32;
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
            .is_some_and(|old| old.buffer.size() == bytes.len() as u64 && old.offsets == offsets);
        let mut lookups = Vec::new();
        let mut dispatches = Vec::new();
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
                if !reusable || old.is_none_or(|old| old.key != key || old.values != values) {
                    dispatches.push((
                        self.preparation.pipeline(r.device(), &key)?,
                        definition.workgroups,
                    ));
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
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        // Parameter edits never overwrite GPU-owned tables. Neither render-code
        // changes nor edits unrelated to preparation regenerate those tables.
        for (i, (&base, effect)) in offsets.iter().zip(&effects).enumerate() {
            if reusable
                && self.instances[&ids].effects[i].program == effect.program
                && self.instances[&ids].effects[i].values == effect.values
                && self.instances[&ids].properties[i] == properties[i]
            {
                continue;
            }
            let directory = base as usize + 1 + data[base as usize + 1][0] as usize;
            let end = directory + effect.program.lookups.len();
            r.queue().write_buffer(
                &buffer,
                u64::from(base) * 16,
                &bytes[base as usize * 16..end * 16],
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
        for (pipeline, groups) in dispatches {
            self.preparation.pending.push(preparation::Dispatch {
                pipeline,
                binding: compute_binding.clone(),
                groups,
            });
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
    if module.global_variables.len() != 6 + MASK_SLOTS
        || module.entry_points.len() != 3
        || !module.overrides.is_empty()
    {
        return Err(GpuRasterError::Effect(
            "Effects cannot add bindings, overrides or entry points".into(),
        ));
    }
    Ok(())
}

/// Linking all library modules checks conflicting declarations without
/// generating a giant executable chain or allocating image intermediates.
pub(super) fn validate_namespace(programs: &[Arc<EffectProgram>]) -> Result<(), GpuRasterError> {
    let Some(first) = programs.first() else {
        return Ok(());
    };
    let mut linked = (**first).clone();
    let mut parts = Vec::new();
    for program in programs {
        EffectInstance::new(program.clone())
            .validate()
            .map_err(|e| GpuRasterError::Effect(e.into()))?;
        for source in program
            .wgsl
            .sources()
            .map_err(|e| GpuRasterError::Effect(e.into()))?
        {
            if !parts.contains(source) {
                parts.push(source.clone());
            }
        }
    }
    if parts.iter().map(|s| s.len()).sum::<usize>() > 16 * 1024 * 1024 {
        return Err(GpuRasterError::Effect(
            "Filter namespace exceeds source limits".into(),
        ));
    }
    linked.wgsl = layer_core::EffectShader::Linked {
        sources: parts.into(),
    };
    validate_source(&shader_source(
        &[Arc::new(linked)],
        &[0],
        Execution::Preview,
        Default::default(),
        false,
        Default::default(),
    )?)
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
    source.push_str(include_str!("effects_color.wgsl"));
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
@group(2) @binding(1) var<storage,read> effect_auxiliary:array<vec4<f32>>;
fn fx_auxiliary(index:u32)->vec4<f32> {return effect_auxiliary[index];}
fn fx_parameter(base:u32,index:u32)->vec4<f32> { return effect_data[base+1u+index]; }
fn fx_lookup(base:u32,table:u32,index:u32)->vec4<f32> {
    let directory=base+u32(effect_data[base].x);let entry=effect_data[directory+table];
    return effect_data[base+u32(entry.x)+min(index,u32(entry.y)-1u)];
}
fn fx_time(base:u32)->f32 { return effect_data[base-1u].w; }
fn fx_extent()->vec2<f32> { return settings.color.zw; }
fn fx_position(local:vec2<f32>)->vec2<f32> {
    let side=settings.operation_linear.z;
    if side<=1. {return settings.color.xy+local;}
    let low=floor(local)*side;
    return settings.color.xy+(low+min(low+side,settings.operation_offset.xy))*.5;
}
fn fx_grid_sample(image:texture_2d<f32>,point:vec2<f32>,grid:vec4<f32>,step:f32)->vec4<f32> {
    if grid.z<=0. {return working_sample_float(image,point);}
    if step<=1. {return working_sample_float(image,clamp(point-grid.xy,vec2(.5),grid.zw-.5));}
    let q=(point-grid.xy)/step;let extent=grid.zw/step;
    let last=ceil(extent)-1.;let previous=last-.5;
    let adjusted=select(q,previous+(q-previous)/((extent-last+1.)*.5),q>previous);
    return textureSampleLevel(image,sampling,clamp(adjusted,vec2(.5),last+.5)/vec2<f32>(textureDimensions(image)),0.);
}
fn fx_sample(p:vec2<f32>)->vec4<f32> {
    let point=p-settings.operation_offset.zw;
    return fx_grid_sample(front,point,settings.source_over,settings.operation_linear.x);
}
fn fx_original(p:vec2<f32>)->vec4<f32> {
    let point=p-settings.operation_offset.zw;
    return fx_grid_sample(back,point,settings.backdrop,settings.operation_linear.y);
}
@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {
    return effect_result(v);
}

"#,
    );
    source.push_str(include_str!("effect_tables.wgsl"));
    source.push_str(include_str!("tetrahedron.wgsl"));
    for (index, chosen) in layer_core::color::RgbSpace::ALL.into_iter().enumerate() {
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
        source.push_str("fn effect_result(v:Vertex)->vec4<f32> {let position=fx_position(v.position.xy);let c=fx_sample(position);\n");
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
        let adjusted = format!("{entry}(fx_sample(position),position,1u)");
        let adjusted = if last { encoded(adjusted) } else { adjusted };
        source.push_str(&format!("fn effect_result(v:Vertex)->vec4<f32> {{ let position=fx_position(v.position.xy); let adjusted={adjusted};\n"));
        if last && p.kind == EffectKind::Adjustment {
            source.push_str(&format!("let c={};let controls=effect_data[0];var coverage=controls.z;if settings.options.w>.5 {{coverage=textureLoad(effect_mask_0,vec2<i32>(v.position.xy),0).a;}}", encoded("fx_original(position)".into())));
            if p.alpha == layer_core::EffectAlpha::Filter {
                source.push_str("if settings.options.y<.5 {if controls.y==0. {return mix(c,adjusted,controls.x*coverage);}let rgb=blend(fx_unassociate(adjusted),fx_unassociate(c),u32(controls.y));return mix(c,vec4<f32>(rgb*adjusted.a,adjusted.a),controls.x*coverage);}");
            }
            source.push_str("return fx_adjustment(c,adjusted,u32(controls.y),controls.x*coverage);}");
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
            "{{ let adjusted={}; let controls=effect_data[{}u];\n",
            encoded(format!("{} ({},position,{}u)", p.entry, linear("c"), offset + 1)),
            offset
        ));
        if p.kind == EffectKind::Adjustment {
            source.push_str("var coverage=controls.z; if settings.options.w>.5 {coverage=mask;}\n");
            if i < MASK_SLOTS {
                let bit = 1u32 << i;
                source.push_str(&format!("else if (u32(settings.extent.z)&{bit}u)!=0u {{let m=textureLoad(effect_mask_{i},vec2<i32>(local),0).r;coverage=select(m,1.-m,(u32(settings.extent.w)&{bit}u)!=0u);}}\n"));
            }
            source.push_str("c=fx_adjustment(c,adjusted,u32(controls.y),controls.x*coverage); }\n");
        } else {
            source.push_str("c=adjusted; }\n");
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
    fn builtin_shaders_and_fused_chain_validate() {
        let programs: Vec<_> = fixtures().iter().map(|b| b.program()).collect();
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
    fn runtime_namespace_rejects_conflicting_declarations() {
        let programs: Vec<_> = fixtures().iter().map(|f| f.program()).collect();
        validate_namespace(&programs).unwrap();
        let mut changed = (*programs[0]).clone();
        changed.id = "conflicting_filter".into();
        changed.wgsl = format!(
            "{}\n// different module with the same declarations",
            changed.wgsl.sources().unwrap().join("\n")
        )
        .into();
        assert!(validate_namespace(&[programs[0].clone(), Arc::new(changed)]).is_err());
    }
    fn validate(p: &[Arc<EffectProgram>], execution: Execution) {
        for blend in layer_core::BlendSpace::ALL {
            let source = shader_source(p, &vec![0; p.len()], execution, Default::default(), false, blend).unwrap();
            validate_source(&source).unwrap();
        }
    }
}
