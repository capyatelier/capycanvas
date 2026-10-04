use crate::{deferred::Deferred, PipelineDevice, submission::CommandEncoder};
use layer_core::color::hdr::LOCAL_GUIDE_EDGE;

pub(crate) struct Pipelines {
    entries: &'static [&'static str],
    dummy: wgpu::Buffer,
    texture: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<Deferred<wgpu::ComputePipeline>>,
    #[cfg(target_arch = "wasm32")]
    ready: std::cell::OnceCell<
        futures_util::future::Shared<
            futures_util::future::LocalBoxFuture<'static, Result<(), String>>,
        >,
    >,
}
impl Pipelines {
    pub(crate) fn new(device: &PipelineDevice, label: &'static str, source: &'static str, names: &'static [&'static str]) -> Self {
        let mut entries = vec![crate::bindings::buffer(
            0,
            wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Uniform,
            false,
            None,
        )];
        for binding in 1..=4 {
            entries.push(crate::bindings::buffer(
                binding,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: binding != 4, },
                false,
                None,
            ));
        }
        entries.push(crate::bindings::texture(5, wgpu::ShaderStages::COMPUTE, false));
        let layout = crate::bindings::layout(device, label, &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = Deferred::wgsl(device, label, source);
        let pipelines = names
            .iter()
            .map(|entry| Deferred::compute(device, entry, &pipeline_layout, &module, entry))
            .collect();
        let texture=device.create_texture(&wgpu::TextureDescriptor {
            label:Some(label),size:wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,
            format:wgpu::TextureFormat::Rgba32Float,usage:wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
        }).create_view(&Default::default());
        Self {
            entries:names,texture,dummy:buffer(device,16,label),
            layout,
            pipelines,
            #[cfg(target_arch = "wasm32")]
            ready: Default::default(),
        }
    }

    pub(crate) async fn prepare(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            use futures_util::FutureExt;
            self.ready
                .get_or_init(|| {
                    let pipelines = self.pipelines.clone();
                    async move {
                        for pipeline in pipelines {
                            pipeline.compile_async().await?;
                        }
                        Ok(())
                    }
                    .boxed_local()
                    .shared()
                })
                .clone()
                .await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            for pipeline in &self.pipelines {
                pipeline.compile();
            }
            Ok(())
        }
    }
    pub(crate) fn encode(
        &self,
        device: &PipelineDevice,
        encoder: &mut CommandEncoder,
        entry: usize,
        params: Params,
        inputs: [Option<&wgpu::Buffer>; 3],
        output: &wgpu::Buffer,
        texture: Option<&wgpu::TextureView>,
        groups: [u32; 2],
    ) {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.entries[entry]),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        {
            let mut bytes = uniform
                .slice(..)
                .get_mapped_range_mut()
                .expect("new mapped uniform");
            let mut packed = [0u8; 64];
            packed[..16].copy_from_slice(params.size.map(u32::to_ne_bytes).as_flattened());
            packed[16..32].copy_from_slice(params.aux.map(u32::to_ne_bytes).as_flattened());
            packed[32..48].copy_from_slice(params.values.map(f32::to_ne_bytes).as_flattened());
            packed[48..].copy_from_slice(params.weights.map(f32::to_ne_bytes).as_flattened());
            bytes.copy_from_slice(&packed);
        }
        uniform.unmap();
        let group = crate::bindings::group(device, self.entries[entry], &self.layout, [
            uniform.as_entire_binding(),
            inputs[0].unwrap_or(&self.dummy).as_entire_binding(),
            inputs[1].unwrap_or(&self.dummy).as_entire_binding(),
            inputs[2].unwrap_or(&self.dummy).as_entire_binding(),
            output.as_entire_binding(),
            wgpu::BindingResource::TextureView(texture.unwrap_or(&self.texture)),
        ]);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(self.entries[entry]),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines[entry]);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }
}

pub(crate) fn buffer(device: &wgpu::Device, size: u64, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
#[derive(Default)]
pub(crate) struct Params {
    pub size: [u32; 4],
    pub aux: [u32; 4],
    pub values: [f32; 4],
    pub weights: [f32; 4],
}

pub(crate) fn guide_extent(document: [u32; 2]) -> Result<[u32; 2], String> {
    if document.contains(&0) || document.iter().any(|&n| n > 32768) {
        return Err("Invalid local tone-map dimensions".into());
    }
    let edge = document[0].max(document[1]).max(LOCAL_GUIDE_EDGE);
    Ok(document.map(|n| (n * LOCAL_GUIDE_EDGE).div_ceil(edge)))
}
pub(crate) fn dims(e: [u32; 2]) -> Params {
    Params {
        size: [e[0], e[1], 0, 0],
        ..Default::default()
    }
}
pub(crate) fn groups(e: [u32; 2]) -> [u32; 2] {
    e.map(|n| n.div_ceil(8))
}
