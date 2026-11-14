//! Bounded, parameter-dependent WGSL work. No filter mathematics lives here.
use super::*;
use layer_core::{EffectLookup, EffectValue};
use std::sync::Weak;

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
    pub parameters: Weak<wgpu::Buffer>,
}
pub(super) enum Work {
    Dispatch(Dispatch),
    Copy { source: Arc<wgpu::Buffer>, destination: Weak<wgpu::Buffer>, offset: u64, size: u64 },
    Upload { buffer: Weak<wgpu::Buffer>, offset: u64, bytes: Vec<u8> },
}
impl Work {
    fn destination(&self) -> &Weak<wgpu::Buffer> {
        match self {
            Self::Dispatch(dispatch) => &dispatch.parameters,
            Self::Copy {destination,..} => destination,
            Self::Upload {buffer,..} => buffer,
        }
    }
    pub(super) fn needed(&self) -> bool { self.destination().strong_count() != 0 }
}
pub(super) struct Recipes {
    layout: wgpu::BindGroupLayout,
    pipelines: std::sync::Mutex<Vec<(Key, Deferred<wgpu::ComputePipeline>)>>,
}
pub(super) struct Preparation {
    recipes: Arc<Recipes>,
    pub layout: wgpu::BindGroupLayout,
    pub(super) pipelines: Vec<(Key, Deferred<wgpu::ComputePipeline>)>,
    pub pending: Vec<Work>,
    pub executions: u64,
}
impl Recipes {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            layout: crate::bindings::layout(device, "effect preparation storage", &[crate::bindings::buffer(
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: false },
                false,
                NonZeroU64::new(16),
            )]),
            pipelines: Default::default(),
        }
    }
}
impl Preparation {
    pub fn discard_unreferenced(&mut self) {
        self.pending.reverse();
        self.pending.retain(Work::needed);
        self.pending.reverse();
    }
    pub fn pending_for(&self, buffer: &Arc<wgpu::Buffer>) -> bool {
        let buffer = Arc::downgrade(buffer);
        self.pending.iter().any(|work| work.destination().ptr_eq(&buffer))
    }
    fn selected<'a>(&self, parameters: impl IntoIterator<Item = &'a Arc<wgpu::Buffer>>) -> Vec<bool> {
        if self.pending.is_empty() { return Vec::new(); }
        let mut required: std::collections::HashSet<_> = parameters.into_iter().map(Arc::as_ptr).collect();
        let mut selected = vec![false; self.pending.len()];
        for (index, work) in self.pending.iter().enumerate().rev() {
            if required.contains(&work.destination().as_ptr()) {
                selected[index] = true;
                if let Work::Copy { source, .. } = work { required.insert(Arc::as_ptr(source)); }
            }
        }
        selected
    }
    pub fn ready<'a>(&self, parameters: impl IntoIterator<Item = &'a Arc<wgpu::Buffer>>) -> bool {
        self.pending.iter().zip(self.selected(parameters)).all(|(work, selected)| !selected || match work {
            Work::Dispatch(dispatch) => dispatch.pipeline.ready(),
            Work::Copy { .. } | Work::Upload { .. } => true,
        })
    }
    pub fn retain_programs(&mut self, programs: &[Arc<EffectProgram>]) {
        self.pipelines
            .retain(|(key, _)| programs.iter().any(|p| p.lookups.contains(&key.definition)));
    }
    pub fn fork(&self) -> Self {
        Self {
            recipes: self.recipes.clone(),
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
    #[cfg(test)]
    pub fn new(device: &wgpu::Device) -> Self { Self::from_recipes(Arc::new(Recipes::new(device))) }
    pub fn from_recipes(recipes: Arc<Recipes>) -> Self {
        Self { layout: recipes.layout.clone(), recipes, pipelines: Vec::new(), pending: Vec::new(), executions: 0 }
    }
    pub fn pipeline(
        &mut self,
        device: &PipelineDevice,
        key: &Key,
    ) -> Result<Deferred<wgpu::ComputePipeline>, GpuRasterError> {
        if let Some((_, pipeline)) = self.pipelines.iter().find(|(k, _)| k == key) {
            return Ok(pipeline.clone());
        }
        if let Some((_, pipeline)) = self.recipes.pipelines.lock().unwrap().iter().find(|(k, _)| k == key).cloned() {
            self.pipelines.push((key.clone(), pipeline.clone()));
            return Ok(pipeline);
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
        let validated = crate::shader_limits::ValidatedModule::parse(&source)?;
        let module = validated.module();
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
        let scratch = validated.workgroup_storage("prep_main")?;
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
        let pipeline = {
            let mut shared = self.recipes.pipelines.lock().unwrap();
            if let Some((_, pipeline)) = shared.iter().find(|(k, _)| k == key) { pipeline.clone() }
            else { shared.push((key.clone(), pipeline.clone())); pipeline }
        };
        self.pipelines.push((key.clone(), pipeline.clone()));
        Ok(pipeline)
    }
    pub fn encode<'a>(&mut self, uploads: &mut crate::Uploads, encoder: &mut crate::submission::CommandEncoder,
        parameters: impl IntoIterator<Item = &'a Arc<wgpu::Buffer>>,
    ) -> Result<(), GpuRasterError> {
        self.discard_unreferenced();
        let mut selected = self.selected(parameters).into_iter();
        let mut pending = self.pending.extract_if(.., |_| selected.next().unwrap()).collect::<Vec<_>>().into_iter().peekable();
        while let Some(work) = pending.next() {
            match work {
                Work::Upload { buffer, offset, bytes } => if let Some(buffer) = buffer.upgrade() { uploads.write_at(encoder, &buffer, offset, &bytes)?; },
                Work::Copy { source, destination, offset, size } => if let Some(destination) = destination.upgrade() {
                    encoder.copy_buffer_to_buffer(&source, offset, &destination, offset, size);
                },
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_preparation_encodes_copy_dependencies_without_unrelated_parameters() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let buffer=||Arc::new(r.device.create_buffer(&wgpu::BufferDescriptor {
            label:Some("selected effect parameters"),size:16,
            usage:wgpu::BufferUsages::COPY_SRC|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false,
        }));
        let source=buffer();let intermediate=buffer();let output=buffer();let other=buffer();
        let mut preparation=Preparation::new(&r.device);
        preparation.pending.push(Work::Upload {buffer:Arc::downgrade(&source),offset:0,bytes:vec![42;16]});
        preparation.pending.push(Work::Copy {source:source.clone(),destination:Arc::downgrade(&intermediate),offset:0,size:16});
        preparation.pending.push(Work::Copy {source:intermediate.clone(),destination:Arc::downgrade(&output),offset:0,size:16});
        preparation.pending.push(Work::Upload {buffer:Arc::downgrade(&other),offset:0,bytes:vec![99;16]});
        assert_eq!(preparation.selected([&output]),[true,true,true,false]);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let mut uploads=crate::Uploads::new(&r.device,1024);
        preparation.encode(&mut uploads,&mut encoder,[&output]).unwrap();
        uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        assert_eq!(preparation.pending.len(),1);
        let read=|buffer:&wgpu::Buffer|pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,buffer)).unwrap();
        assert_eq!(read(&output),vec![42;16]);
        assert_eq!(read(&other),vec![0;16]);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        preparation.encode(&mut uploads,&mut encoder,[&other]).unwrap();
        uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        assert!(preparation.pending.is_empty());
        assert_eq!(read(&other),vec![99;16]);
    }
    #[test]
    fn cancelled_preparation_releases_copy_dependencies_but_keeps_owned_parameters() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let buffer=||Arc::new(r.device.create_buffer(&wgpu::BufferDescriptor {
            label:Some("effect preparation lifetime"),size:16,
            usage:wgpu::BufferUsages::COPY_SRC|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false,
        }));
        let source=buffer();let intermediate=buffer();let output=buffer();
        let source_weak=Arc::downgrade(&source);
        let intermediate_weak=Arc::downgrade(&intermediate);
        let output_weak=Arc::downgrade(&output);
        let mut preparation=Preparation::new(&r.device);
        preparation.pending.push(Work::Upload {buffer:source_weak.clone(),offset:0,bytes:vec![0;16]});
        preparation.pending.push(Work::Copy {source:source.clone(),destination:intermediate_weak.clone(),offset:0,size:16});
        preparation.pending.push(Work::Copy {source:intermediate.clone(),destination:output_weak.clone(),offset:0,size:16});
        let reader=source.clone();
        drop((source,intermediate));
        preparation.discard_unreferenced();
        assert_eq!(preparation.pending.len(),3);
        assert!(preparation.pending_for(&output));
        drop(output);
        preparation.discard_unreferenced();
        assert_eq!(preparation.pending.len(),1);
        assert!(intermediate_weak.upgrade().is_none());
        assert!(output_weak.upgrade().is_none());
        assert!(preparation.pending_for(&reader));
        drop(reader);
        preparation.discard_unreferenced();
        assert!(preparation.pending.is_empty());
        assert!(source_weak.upgrade().is_none());
    }
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

    #[test]
    fn preparation_counts_padded_reachable_storage_before_pipeline_creation() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let limit=r.device.limits().max_compute_workgroup_storage_size;
        let mut definition=crate::tests::fixture("gaussian_blur").program().lookups[0].clone();
        definition.entry="lookup".into();definition.workgroup_size=[1;3];
        for (reserved,accepted) in [(32,true),(16,false)] {
            let unused=if accepted {format!("var<workgroup> unused:array<u32,{}>;",limit/4)}else{String::new()};
            definition.wgsl=format!("var<workgroup> head:array<u32,{}>;var<workgroup> one:u32;var<workgroup> two:u32;{unused}fn lookup(local:vec3<u32>,global:vec3<u32>){{head[0]=1u;one=1u;two=1u;prep_store(0u,vec4(f32(head[0]+one+two)));}}",(limit-reserved)/4).into();
            let key=Key{definition:definition.clone(),inputs:vec![[0,1]],output:2,geometry:1};
            let mut preparation=Preparation::new(&r.device);
            let result=preparation.pipeline(&r.device,&key);
            assert_eq!(result.is_ok(),accepted,"reserved {reserved}: {:?}",result.err());
            assert_eq!(preparation.pipelines.len(),usize::from(accepted));
        }
    }
}
