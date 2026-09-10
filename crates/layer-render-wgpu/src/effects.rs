//! Validated WGSL execution for built-ins and programmable effects. Compatible
//! pointwise chains are fused; declared image passes share the same ABI helpers.
use super::*;
use layer_core::{EffectInstance, EffectKind, EffectProgram};
use std::{collections::HashMap, sync::Arc};
#[path = "effect_preparation.rs"]
mod preparation;

// WebGPU guarantees 16 sampled textures per stage. Two are scene inputs;
// the rest let ordinary aligned masks participate in the same fused shader.
pub(super) const MASK_SLOTS: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Execution {
    Fused,
    Image(usize),
    Preview,
}

#[derive(Clone)]
pub(super) struct PreparedEffect {
    pub pipeline: wgpu::RenderPipeline,
    pub binding: wgpu::BindGroup,
}
struct Instance {
    effects: Vec<Arc<EffectInstance>>,
    properties: Vec<[f32; 4]>,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    compute_binding: wgpu::BindGroup,
    pipelines: HashMap<Execution, wgpu::RenderPipeline>,
    lookups: Vec<preparation::State>,
    offsets: Vec<u32>,
}
pub(super) struct Effects {
    layout: wgpu::BindGroupLayout,
    pub masks: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(Vec<Arc<EffectProgram>>, Execution, wgpu::RenderPipeline)>,
    // Parameters and GPU tables are shared by every pass of the same chain.
    instances: HashMap<Vec<LayerId>, Instance>,
    // Reuse the lookup key; ordinary painting/animation does not repack inputs.
    ids: Vec<LayerId>,
    preparation: preparation::Preparation,
    pub compilations: u64,
}
impl Effects {
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
            ids: Vec::new(),
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
    pub fn encode_preparation(&mut self, encoder: &mut wgpu::CommandEncoder) {
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
        let layout = r
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("effect parameters"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                }],
            });
        let masks = r
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("effect mask inputs"),
                entries: &(0..MASK_SLOTS)
                    .map(|i| wgpu::BindGroupLayoutEntry {
                        binding: i as u32,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            });
        let pipeline_layout = r
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WGSL effects ABI 2"),
                bind_group_layouts: &[Some(uniforms), Some(sources), Some(&layout), Some(&masks)],
                immediate_size: 0,
            });
        Self {
            layout,
            masks,
            pipeline_layout,
            pipelines: Vec::new(),
            instances: HashMap::new(),
            ids: Vec::new(),
            preparation: preparation::Preparation::new(&r.device),
            compilations: 0,
        }
    }
    pub fn retain(&mut self, layers: &[Layer]) {
        self.instances
            .retain(|ids, _| ids.iter().all(|id| layers.iter().any(|l| l.id == *id)));
    }
    pub fn prepare(
        &mut self,
        r: &WgpuRasterizer,
        layers: &[&Layer],
        stage: Execution,
        time: f32,
    ) -> Result<PreparedEffect, GpuRasterError> {
        self.ids.clear();
        self.ids.extend(layers.iter().map(|l| l.id));
        if let Some(old) = self.instances.get_mut(self.ids.as_slice())
            && old
                .effects
                .iter()
                .zip(layers)
                .all(|(effect, layer)| Arc::ptr_eq(effect, layer.effect.as_ref().unwrap()))
            && old
                .properties
                .iter()
                .zip(layers)
                .all(|(a, layer)| a[..3] == effect_properties(layer, time)[..3])
            && let Some(pipeline) = old.pipelines.get(&stage)
        {
            // Animation updates only one scalar per instance, never its LUTs.
            for (i, (properties, layer)) in old.properties.iter_mut().zip(layers).enumerate() {
                let seconds = layer.effect.as_ref().unwrap().time_seconds(time);
                if properties[3] != seconds {
                    r.queue.write_buffer(
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
            });
        }
        let ids = self.ids.clone();
        let effects: Vec<_> = layers
            .iter()
            .map(|l| l.effect.as_ref().unwrap().clone())
            .collect();
        let properties: Vec<_> = layers.iter().map(|l| effect_properties(l, time)).collect();
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for (effect, properties) in effects.iter().zip(&properties) {
            effect
                .validate()
                .map_err(|e| GpuRasterError::Effect(e.into()))?;
            offsets.push(data.len() as u32);
            data.push(*properties);
            data.extend(effect.gpu_parameters());
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
            let source = shader_source(&programs, &offsets, stage)?;
            validate_source(&source)?;
            let module = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("checked pointwise effect"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = fullscreen_pipeline(
                &r.device,
                &self.pipeline_layout,
                &module,
                "effect_fragment",
                None,
                COLOR_FORMAT,
                "pointwise effect chain",
            );
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
                    layer_core::EFFECT_LUT_SAMPLES as u32
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
                        self.preparation.pipeline(&r.device, &key)?,
                        definition.workgroups,
                    ));
                }
                lookups.push(preparation::State { key, values });
            }
        }
        let buffer = if reusable {
            self.instances[&ids].buffer.clone()
        } else {
            r.device.create_buffer(&wgpu::BufferDescriptor {
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
            r.queue.write_buffer(
                &buffer,
                u64::from(base) * 16,
                &bytes[base as usize * 16..end * 16],
            );
        }
        let binding = if reusable {
            self.instances[&ids].binding.clone()
        } else {
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect parameter binding"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let compute_binding = if reusable {
            self.instances[&ids].compute_binding.clone()
        } else {
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect preparation binding"),
                layout: &self.preparation.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
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
        };
        self.instances.insert(
            ids,
            Instance {
                effects,
                properties,
                buffer,
                binding,
                compute_binding,
                pipelines,
                lookups,
                offsets,
            },
        );
        Ok(prepared)
    }
}

fn effect_properties(layer: &Layer, time: f32) -> [f32; 4] {
    [
        layer.opacity,
        layer.properties.blend as u32 as f32,
        layer.mask.as_ref().filter(|m| m.enabled).map_or(1., |m| {
            if m.inverted {
                1. - m.default_coverage
            } else {
                m.default_coverage
            }
        }),
        layer.effect.as_ref().unwrap().time_seconds(time),
    ]
}

fn validate_source(source: &str) -> Result<(), GpuRasterError> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| GpuRasterError::Effect(e.emit_to_string(source)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
    if module.global_variables.len() != 5 + MASK_SLOTS
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
    )?)
}
fn shader_source(
    programs: &[Arc<EffectProgram>],
    offsets: &[u32],
    stage: Execution,
) -> Result<String, GpuRasterError> {
    let mut source = include_str!("scene.wgsl").to_string();
    for i in 0..MASK_SLOTS {
        source.push_str(&format!(
            "@group(3) @binding({i}) var effect_mask_{i}:texture_2d<f32>;\n"
        ));
    }
    source.push_str(
        r#"
@group(2) @binding(0) var<storage,read> effect_data:array<vec4<f32>>;
fn fx_parameter(base:u32,index:u32)->vec4<f32> { return effect_data[base+1u+index]; }
fn fx_lookup(base:u32,table:u32,index:u32)->vec4<f32> {
    let directory=base+u32(effect_data[base].x);let entry=effect_data[directory+table];
    return effect_data[base+u32(entry.x)+min(index,u32(entry.y)-1u)];
}
fn fx_time(base:u32)->f32 { return effect_data[base-1u].w; }
fn fx_extent()->vec2<f32> { return settings.color.zw; }
fn fx_sample(p:vec2<f32>)->vec4<f32> {
    let point=clamp(p,vec2<f32>(.5),fx_extent()-.5);
    if settings.source_over.z>0. {return textureSampleLevel(front,sampling,(point-settings.source_over.xy)/settings.source_over.zw,0.);}
    return textureSampleLevel(front,sampling,point/fx_extent(),0.);
}
fn fx_original(p:vec2<f32>)->vec4<f32> {
    return textureSampleLevel(back,sampling,clamp(p,vec2<f32>(.5),fx_extent()-.5)/fx_extent(),0.);
}
fn fx_lut(base:u32,offset:u32,value:f32)->vec4<f32> {
    let x=clamp(value,0.,1.)*255.; let i=u32(x);
    return mix(effect_data[base+1u+offset+i],effect_data[base+1u+offset+min(i+1u,255u)],fract(x));
}

"#,
    );
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
        source.push_str("@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {let position=v.position.xy+settings.color.xy;let c=fx_sample(position);\n");
        for (j, pass) in p.passes.iter().enumerate() {
            source.push_str(&format!(
                "if u32(settings.extent.w)=={j}u {{return {}(c,position,{base}u);}}\n",
                pass.entry
            ));
        }
        source.push_str(&format!("return {}(c,position,{base}u);}}", p.entry));
        return Ok(source);
    }
    if let Execution::Image(stage) = stage {
        let p = &programs[0];
        let entry = p.passes.get(stage).map_or(&p.entry, |p| &p.entry);
        let last = stage + 1 >= p.passes.len();
        source.push_str(&format!("@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {{ let position=v.position.xy+settings.color.xy; let adjusted={entry}(fx_sample(position),position,1u);\n"));
        if last && p.kind == EffectKind::Adjustment {
            source.push_str("let c=fx_original(position);let controls=effect_data[0];var coverage=controls.z;if settings.options.w>.5 {coverage=textureLoad(effect_mask_0,vec2<i32>(position),0).r;}let rgb=clamp(blend(adjusted.rgb/max(adjusted.a,.000001),c.rgb/max(c.a,.000001),u32(controls.y)),vec3<f32>(0.),vec3<f32>(1.));");
            if p.alpha == layer_core::EffectAlpha::Filter {
                source.push_str("if settings.options.y<.5 {return mix(c,vec4<f32>(rgb*adjusted.a,adjusted.a),controls.x*coverage);}");
            }
            source.push_str("return vec4<f32>(mix(c.rgb,rgb*c.a,controls.x*coverage),c.a);}");
        } else {
            source.push_str("return adjusted;}");
        }
        return Ok(source);
    }
    source.push_str(
        r#"
@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {
    let local=v.position.xy-settings.rect.xy;
    var c=textureLoad(front,vec2<i32>(local),0);
    if settings.source_over.w>.5 {
        let coverage=select(settings.source_over.y,mix(textureLoad(back,vec2<i32>(local),0).r,1.-textureLoad(back,vec2<i32>(local),0).r,settings.source_over.z-2.),settings.source_over.z>=2.);
        c*=settings.source_over.x*coverage;
        if settings.source_over.w<1.5 {c+=settings.backdrop*(1.-c.a);}
    }
    let mask=select(1.,textureLoad(back,vec2<i32>(local),0).r,settings.options.w>.5);
    let position=settings.color.xy+local;
"#,
    );
    for (i, (p, offset)) in programs.iter().zip(offsets).enumerate() {
        if p.kind == EffectKind::Generator && programs.len() != 1 {
            return Err(GpuRasterError::Effect(
                "Generators cannot be fused as adjustments".into(),
            ));
        }
        source.push_str(&format!(
            "{{ let adjusted={} (c,position,{}u); let controls=effect_data[{}u];\n",
            p.entry,
            offset + 1,
            offset
        ));
        if p.kind == EffectKind::Adjustment {
            source.push_str("var coverage=controls.z; if settings.options.w>.5 {coverage=mask;}\n");
            if i < MASK_SLOTS {
                let bit = 1u32 << i;
                source.push_str(&format!("else if (u32(settings.extent.z)&{bit}u)!=0u {{let m=textureLoad(effect_mask_{i},vec2<i32>(local),0).r;coverage=select(m,1.-m,(u32(settings.extent.w)&{bit}u)!=0u);}}\n"));
            }
            source.push_str("let rgb=blend(adjusted.rgb/max(adjusted.a,.000001),c.rgb/max(c.a,.000001),u32(controls.y)); c=vec4<f32>(mix(c.rgb,clamp(rgb,vec3<f32>(0.),vec3<f32>(1.))*c.a,controls.x*coverage),c.a); }\n");
        } else {
            source.push_str("c=adjusted; }\n");
        }
    }
    source.push_str("if settings.options.x>.5 {let src=c*settings.options.y;let dst=settings.backdrop;let rgb=blend(src.rgb/max(src.a,.000001),dst.rgb/max(dst.a,.000001),u32(settings.options.z));c=vec4<f32>((1.-src.a)*dst.rgb+(1.-dst.a)*src.rgb+src.a*dst.a*rgb,src.a+dst.a*(1.-src.a));} return c; }\n");
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
                .filter(|p| !p.image_boundary())
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
        let source = shader_source(p, &vec![0; p.len()], execution).unwrap();
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(module.global_variables.len(), 5 + MASK_SLOTS);
        assert_eq!(module.entry_points.len(), 3);
    }
}
