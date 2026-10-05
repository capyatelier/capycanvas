//! Bounded, parameter-dependent WGSL work. No filter mathematics lives here.
use super::*;
use layer_core::{EffectLookup, EffectValue};

#[derive(Clone, PartialEq)]
pub(super) struct Key {
    pub definition: EffectLookup,
    pub inputs: Vec<[u32; 2]>,
    pub output: u32,
    pub geometry: u32,
}
pub(super) struct State {
    pub key: Key,
    pub values: Vec<EffectValue>,
}
pub(super) struct Dispatch {
    pub pipeline: Deferred<wgpu::ComputePipeline>,
    pub binding: wgpu::BindGroup,
    pub groups: [u32; 3],
}
pub(super) enum Work {
    Dispatch(Dispatch),
    Copy { source: wgpu::Buffer, destination: wgpu::Buffer, offset: u64, size: u64 },
}
pub(super) struct Preparation {
    pub layout: wgpu::BindGroupLayout,
    pub(super) pipelines: Vec<(Key, Deferred<wgpu::ComputePipeline>)>,
    pub pending: Vec<Work>,
    pub executions: u64,
}
impl Preparation {
    pub fn retain_programs(&mut self, programs: &[Arc<EffectProgram>]) {
        self.pipelines
            .retain(|(key, _)| programs.iter().any(|p| p.lookups.contains(&key.definition)));
    }
    pub fn fork(&self) -> Self {
        Self {
            layout: self.layout.clone(),
            pipelines: self.pipelines.clone(),
            pending: Vec::new(),
            executions: 0,
        }
    }
    pub fn merge(&mut self, other: Self) {
        for (key, pipeline) in other.pipelines {
            if !self.pipelines.iter().any(|(k, _)| *k == key) {
                self.pipelines.push((key, pipeline));
            }
        }
    }
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            layout: crate::bindings::layout(device, "effect preparation storage", &[crate::bindings::buffer(
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: false },
                false,
                NonZeroU64::new(16),
            )]),
            pipelines: Vec::new(),
            pending: Vec::new(),
            executions: 0,
        }
    }
    pub fn pipeline(
        &mut self,
        device: &PipelineDevice,
        key: &Key,
    ) -> Result<Deferred<wgpu::ComputePipeline>, GpuRasterError> {
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
        source.push_str(&format!("fn prep_texel_size()->f32{{return prep_data[{}u].z;}}\n", key.geometry));
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
        let module = super::effects::parse_validated(&source)?;
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
            !matches!(f.name.as_deref(),Some("prep_parameter"|"prep_store"|"prep_texel_size"))
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
            || u64::from(x) * u64::from(y) * u64::from(z) > u64::from(device.limits().max_compute_invocations_per_workgroup)
        {
            return Err(GpuRasterError::Effect(
                "Preparation exceeds device workgroup limits".into(),
            ));
        }
        let module = Deferred::wgsl(device, "effect preparation", source);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect preparation"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let pipeline = Deferred::compute(device, "effect preparation", &layout, &module, "prep_main");
        self.pipelines.push((key.clone(), pipeline.clone()));
        Ok(pipeline)
    }
    pub fn encode(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        let mut pending = self.pending.drain(..).peekable();
        while let Some(work) = pending.next() {
            match work {
                Work::Copy { source, destination, offset, size } => encoder.copy_buffer_to_buffer(&source, offset, &destination, offset, size),
                Work::Dispatch(mut dispatch) => {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("changed effect lookup tables"), timestamp_writes: None,
                    });
                    loop {
                        pass.set_pipeline(&dispatch.pipeline);
                        pass.set_bind_group(0, &dispatch.binding, &[]);
                        let [x, y, z] = dispatch.groups;
                        pass.dispatch_workgroups(x, y, z);
                        self.executions += 1;
                        if !matches!(pending.peek(), Some(Work::Dispatch(_))) { break; }
                        let Some(Work::Dispatch(next)) = pending.next() else { unreachable!() };
                        dispatch = next;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_refuses_aggregate_workgroup_overflow_before_dispatch() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut definition=crate::tests::fixture("gaussian_blur").program().lookups[0].clone();
        let limits=r.device.limits();let x=256.min(limits.max_compute_workgroup_size_x);
        let y=limits.max_compute_invocations_per_workgroup/x+1;
        assert!(y<=limits.max_compute_workgroup_size_y);
        definition.workgroup_size=[x,y,1];
        let key=Key{definition,inputs:vec![[0,1]],output:2,geometry:1};
        let mut preparation=Preparation::new(&r.device);
        let error=preparation.pipeline(&r.device,&key).err().unwrap();
        assert!(error.to_string().contains("Preparation exceeds device workgroup limits"),"{error}");
        assert!(preparation.pipelines.is_empty());
    }
}
