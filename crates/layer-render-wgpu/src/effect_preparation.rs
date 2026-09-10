//! Bounded, parameter-dependent WGSL work. No filter mathematics lives here.
use super::*;
use layer_core::{EffectLookup, EffectValue};

#[derive(Clone, PartialEq)]
pub(super) struct Key {
    pub definition: EffectLookup,
    pub inputs: Vec<[u32; 2]>,
    pub output: u32,
}
pub(super) struct State {
    pub key: Key,
    pub values: Vec<EffectValue>,
}
pub(super) struct Dispatch {
    pub pipeline: wgpu::ComputePipeline,
    pub binding: wgpu::BindGroup,
    pub groups: [u32; 3],
}
pub(super) struct Preparation {
    pub layout: wgpu::BindGroupLayout,
    pipelines: Vec<(Key, wgpu::ComputePipeline)>,
    pub pending: Vec<Dispatch>,
    pub executions: u64,
}
impl Preparation {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            layout: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("effect preparation storage"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                }],
            }),
            pipelines: Vec::new(),
            pending: Vec::new(),
            executions: 0,
        }
    }
    pub fn pipeline(
        &mut self,
        device: &wgpu::Device,
        key: &Key,
    ) -> Result<wgpu::ComputePipeline, GpuRasterError> {
        if let Some((_, pipeline)) = self.pipelines.iter().find(|(k, _)| k == key) {
            return Ok(pipeline.clone());
        }
        let mut source = String::from(
            "@group(0) @binding(0) var<storage,read_write> prep_data:array<vec4<f32>>;\nfn prep_parameter(index:u32,element:u32)->vec4<f32>{switch index {\n",
        );
        for (i, [offset, len]) in key.inputs.iter().enumerate() {
            source.push_str(&format!(
                "case {i}u:{{return prep_data[{offset}u+min(element,{}u)];}}\n",
                len - 1
            ));
        }
        source.push_str("default:{return vec4<f32>(0.);}}}\n");
        let lookup = &key.definition;
        source.push_str(&format!("fn prep_store(index:u32,value:vec4<f32>){{if index<{}u{{prep_data[{}u+index]=value;}}}}\n",lookup.values,key.output));
        for part in lookup
            .wgsl
            .sources()
            .map_err(|e| GpuRasterError::Effect(e.into()))?
        {
            source.push_str(part);
            source.push('\n');
        }
        let [x, y, z] = lookup.workgroup_size;
        source.push_str(&format!("\n@compute @workgroup_size({x},{y},{z}) fn prep_main(@builtin(local_invocation_id) local:vec3<u32>,@builtin(global_invocation_id) global:vec3<u32>){{{}(local,global);}}",lookup.entry));
        let module = naga::front::wgsl::parse_str(&source)
            .map_err(|e| GpuRasterError::Effect(e.emit_to_string(&source)))?;
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        // Libraries may declare private/workgroup scratch, never extra bound
        // resources, override constants, or entry points owned by the host.
        if module.entry_points.len() != 1
            || !module.overrides.is_empty()
            || module
                .global_variables
                .iter()
                .filter(|(_, v)| v.binding.is_some())
                .count()
                != 1
            || module.global_variables.iter().any(|(_, v)| {
                !matches!(
                    v.space,
                    naga::AddressSpace::Storage { .. }
                        | naga::AddressSpace::Private
                        | naga::AddressSpace::WorkGroup
                )
            })
        {
            return Err(GpuRasterError::Effect(
                "Invalid preparation shader interface".into(),
            ));
        }
        // Authored functions must use the bounded helpers, not reach through
        // the shared buffer into another effect's parameters or lookup output.
        if module.functions.iter().any(|(_,f)| {
            !matches!(f.name.as_deref(),Some("prep_parameter"|"prep_store"))
                && f.expressions.iter().any(|(_,e)| matches!(e,naga::Expression::GlobalVariable(g) if module.global_variables[*g].binding.is_some()))
        }) {
            return Err(GpuRasterError::Effect("Preparation must access storage through prep_parameter/prep_store".into()));
        }
        let mut sizes = naga::proc::Layouter::default();
        sizes
            .update(module.to_ctx())
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        let scratch: u64 = module
            .global_variables
            .iter()
            .filter(|(_, v)| v.space == naga::AddressSpace::WorkGroup)
            .map(|(_, v)| u64::from(sizes[v.ty].size))
            .sum();
        if scratch > u64::from(device.limits().max_compute_workgroup_storage_size)
            || x > device.limits().max_compute_workgroup_size_x
            || y > device.limits().max_compute_workgroup_size_y
            || z > device.limits().max_compute_workgroup_size_z
        {
            return Err(GpuRasterError::Effect(
                "Preparation exceeds device workgroup limits".into(),
            ));
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effect preparation"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect preparation"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effect preparation"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("prep_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        self.pipelines.push((key.clone(), pipeline.clone()));
        Ok(pipeline)
    }
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("changed effect lookup tables"),
            timestamp_writes: None,
        });
        for dispatch in self.pending.drain(..) {
            pass.set_pipeline(&dispatch.pipeline);
            pass.set_bind_group(0, &dispatch.binding, &[]);
            let [x, y, z] = dispatch.groups;
            pass.dispatch_workgroups(x, y, z);
            self.executions += 1;
        }
    }
}
