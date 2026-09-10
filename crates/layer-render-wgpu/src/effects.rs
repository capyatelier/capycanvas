//! Validated WGSL execution for built-ins and programmable effects. Compatible
//! pointwise chains are fused; declared image passes share the same ABI helpers.
use super::*;
use layer_core::{EffectInstance, EffectKind, EffectProgram};
use std::{collections::HashMap, sync::Arc};

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
    prepared: PreparedEffect,
    offsets: Vec<u32>,
}
pub(super) struct Effects {
    layout: wgpu::BindGroupLayout,
    pub masks: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(Vec<Arc<EffectProgram>>, Execution, wgpu::RenderPipeline)>,
    instances: HashMap<(Vec<LayerId>, Execution), Instance>,
    pub compilations: u64,
}
impl Effects {
    pub fn storage_bytes(&self) -> u64 {
        self.instances.values().map(|i| i.buffer.size()).sum()
    }
    pub fn new(
        r: &WgpuRasterizer,
        uniforms: &wgpu::BindGroupLayout,
        sources: &wgpu::BindGroupLayout,
    ) -> Self {
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
            compilations: 0,
        }
    }
    pub fn retain(&mut self, layers: &[Layer]) {
        self.instances
            .retain(|(ids, _), _| ids.iter().all(|id| layers.iter().any(|l| l.id == *id)));
    }
    pub fn prepare(
        &mut self,
        r: &WgpuRasterizer,
        layers: &[&Layer],
        stage: Execution,
        time: f32,
    ) -> Result<PreparedEffect, GpuRasterError> {
        let ids = (layers.iter().map(|l| l.id).collect::<Vec<_>>(), stage);
        let effects: Vec<_> = layers
            .iter()
            .map(|l| l.effect.as_ref().unwrap().clone())
            .collect();
        let properties: Vec<_> = layers
            .iter()
            .map(|l| {
                [
                    l.opacity,
                    l.properties.blend as u32 as f32,
                    l.mask.as_ref().filter(|m| m.enabled).map_or(1., |m| {
                        if m.inverted {
                            1. - m.default_coverage
                        } else {
                            m.default_coverage
                        }
                    }),
                    l.effect.as_ref().unwrap().time_seconds(time),
                ]
            })
            .collect();
        if let Some(old) = self.instances.get_mut(&ids)
            && old
                .effects
                .iter()
                .zip(&effects)
                .all(|(a, b)| Arc::ptr_eq(a, b))
            && old
                .properties
                .iter()
                .zip(&properties)
                .all(|(a, b)| a[..3] == b[..3])
        {
            // Animation updates only one scalar per instance, never its LUTs.
            for (i, (a, b)) in old.properties.iter().zip(&properties).enumerate() {
                if a[3] != b[3] {
                    r.queue.write_buffer(
                        &old.buffer,
                        old.offsets[i] as u64 * 16 + 12,
                        &b[3].to_le_bytes(),
                    );
                }
            }
            old.properties = properties;
            return Ok(old.prepared.clone());
        }
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
        if let Some(old) = self.instances.get_mut(&ids)
            && old.effects.iter().map(|e| &e.program).eq(programs.iter())
            && old.buffer.size() == bytes.len() as u64
        {
            r.queue.write_buffer(&old.buffer, 0, &bytes);
            old.effects = effects;
            old.properties = properties;
            return Ok(old.prepared.clone());
        }
        let pipeline = if let Some((_, _, pipeline)) = self
            .pipelines
            .iter()
            .find(|(p, s, _)| *p == programs && *s == stage)
        {
            pipeline.clone()
        } else {
            let source = shader_source(&programs, &offsets, stage)?;
            let module = naga::front::wgsl::parse_str(&source)
                .map_err(|e| GpuRasterError::Effect(e.emit_to_string(&source)))?;
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
            // The wrapper owns all ABI resources and entries.
            if module.global_variables.len() != 5 + MASK_SLOTS || module.entry_points.len() != 3 {
                return Err(GpuRasterError::Effect(
                    "Effects cannot add bindings or entry points".into(),
                ));
            }
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
        let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect parameter values"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        r.queue.write_buffer(&buffer, 0, &bytes);
        let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect parameter binding"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let prepared = PreparedEffect { pipeline, binding };
        self.instances.insert(
            ids,
            Instance {
                effects,
                properties,
                buffer,
                prepared: prepared.clone(),
                offsets,
            },
        );
        Ok(prepared)
    }
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
        if !included.contains(&program.wgsl.as_ref()) {
            source.push_str(&program.wgsl);
            source.push('\n');
            included.push(&program.wgsl);
        }
    }
    if stage == Execution::Preview {
        source.push_str("@fragment fn effect_fragment(v:Vertex)->@location(0) vec4<f32> {let position=v.position.xy+settings.color.xy;let c=fx_sample(position);switch u32(settings.extent.z) {\n");
        for (i, (p, offset)) in programs.iter().zip(offsets).enumerate() {
            source.push_str(&format!("case {i}u: {{\n"));
            for (j, pass) in p.passes.iter().enumerate() {
                source.push_str(&format!(
                    "if u32(settings.extent.w)=={j}u {{return {}(c,position,{}u);}}\n",
                    pass.entry,
                    offset + 1
                ));
            }
            source.push_str(&format!(
                "return {}(c,position,{}u);}}\n",
                p.entry,
                offset + 1
            ));
        }
        source.push_str("default: {return c;} }}");
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
    #[test]
    fn builtin_shaders_and_fused_chain_validate() {
        let programs: Vec<_> = layer_core::BuiltinEffect::ALL
            .into_iter()
            .map(|b| b.program())
            .collect();
        for p in &programs {
            validate(std::slice::from_ref(p));
        }
        validate(&programs);
    }
    fn validate(p: &[Arc<EffectProgram>]) {
        let source = shader_source(p, &vec![0; p.len()], Execution::Fused).unwrap();
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
