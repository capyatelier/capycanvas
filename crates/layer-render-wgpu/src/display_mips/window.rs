use super::*;

pub(crate) struct WindowPipelines {
    pub reduce: Pipelines,
    layout: wgpu::BindGroupLayout,
    pub half: Deferred<wgpu::ComputePipeline>,
}
impl WindowPipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = crate::bindings::layout(device, "display mip window", &[
            crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
            native_tiles::buffer_entry(1, wgpu::BufferBindingType::Uniform, true, 16),
            crate::bindings::storage_texture(2, wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba16Float, wgpu::StorageTextureAccess::WriteOnly),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display mip window"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        let shader = Deferred::wgsl(device, "display mip window", [
            include_str!("half.wgsl"), include_str!("window.wgsl"),
        ].concat());
        Self { reduce: Pipelines::new(device), half: Deferred::compute(device,
            "display mip window", &pipeline_layout, &shader, "copy"), layout }
    }
    pub fn prepare_tile(&self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        work:&mut Image,source:&crate::source_access::TileTexture,image:&layer_core::color::source::SourceImage,coordinate:[u32;2],level:u32,
    )->Result<(),GpuRasterError> {
        if source.format()==wgpu::TextureFormat::Rgba32Float {
            work.copy_to_scratch(encoder,source,PixelRect::full([PAGE_SIZE;2]),[0;2])?;
        } else {
            crate::scene::Scene::display_source(r,encoder,image,coordinate,source,&work.views[0])?;
        }
        Image::reduce_to_level(&r.device,&self.reduce,encoder,&work.views,&mut work.records,[PAGE_SIZE;2],level);
        Ok(())
    }
}

pub(crate) struct Window {
    pub plan: Plan,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    records: Option<Record>,
    stride: u32,
}
impl Window {
    pub fn byte_bound(device: &PipelineDevice, plan: Plan, format: wgpu::TextureFormat) -> u64 {
        let pixels = plan.size.map(u64::from).into_iter().product::<u64>()
            * u64::from(format.block_copy_size(None).unwrap());
        pixels + if format == wgpu::TextureFormat::Rgba16Float {
            u64::from(plan.bounds.width().div_ceil(PAGE_SIZE)) * u64::from(plan.bounds.height().div_ceil(PAGE_SIZE)) * u64::from(device.limits().min_uniform_buffer_offset_alignment.max(16))
        } else { 0 }
    }
    pub fn new(device: &PipelineDevice, plan: Plan, format: wgpu::TextureFormat,
        work: &Image, pipelines: &WindowPipelines,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image mip window"), size: wgpu::Extent3d {
                width: plan.size[0], height: plan.size[1], depth_or_array_layers: 1,
            }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: if format==wgpu::TextureFormat::Rgba8Unorm {&[wgpu::TextureFormat::Rgba8UnormSrgb]}else{&[]},
        });
        let view = texture.create_view(&Default::default());
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let records = (format == wgpu::TextureFormat::Rgba16Float).then(|| {
            let mut bytes = vec![0; page_coordinates(plan.bounds).count() * stride as usize];
            for (index, coordinate) in page_coordinates(plan.bounds).enumerate() {
                let origin = coordinate.map(|v| v * PAGE_SIZE);
                let valid = std::array::from_fn::<_, 2, _>(|axis| (plan.extent[axis] - origin[axis]).min(PAGE_SIZE));
                let scale = 1 << plan.level;
                let values = [valid[0].div_ceil(scale), valid[1].div_ceil(scale),
                    (origin[0] - plan.bounds.min_x()) / scale, (origin[1] - plan.bounds.min_y()) / scale];
                for (channel, value) in values.into_iter().enumerate() {
                    let offset = index * stride as usize + channel * 4;
                    bytes[offset..offset+4].copy_from_slice(&value.to_le_bytes());
                }
            }
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("image mip window coordinates"), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM,
            });
            let binding = crate::bindings::group(device, "image mip window", &pipelines.layout, [
                wgpu::BindingResource::TextureView(&work.views[plan.level as usize]),
                wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uniform, offset: 0, size: NonZeroU64::new(16) }),
                wgpu::BindingResource::TextureView(&view),
            ]);
            Record { uniform, binding }
        });
        Self { plan, texture, view, records, stride }
    }
    pub fn storage_bytes(&self) -> u64 {
        texture_bytes(&self.texture) + self.records.as_ref().map_or(0, |record| record.uniform.size())
    }
    pub fn copy_tiles<'a>(pipelines: &WindowPipelines,
        encoder: &mut crate::submission::CommandEncoder, work: &Image, coordinate: [u32; 2],
        windows: impl Iterator<Item=&'a Self> + Clone,
    ) -> Result<(), GpuRasterError> {
        let origin = coordinate.map(|v| v * PAGE_SIZE);
        for window in windows.clone() {
            if window.plan.bounds.intersect(page_rect(coordinate)).is_empty() { return Err(GpuRasterError::InvalidExtent); }
            if window.records.is_some() { continue; }
            let valid:[u32;2] = std::array::from_fn(|axis| (window.plan.extent[axis] - origin[axis]).min(PAGE_SIZE));
            let scale = 1 << window.plan.level;
            encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo { mip_level: window.plan.level, ..work.scratch.as_image_copy() },
                wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d {
                    x: (origin[0]-window.plan.bounds.min_x())/scale, y: (origin[1]-window.plan.bounds.min_y())/scale, z: 0,
                }, ..window.texture.as_image_copy() }, wgpu::Extent3d {
                    width: valid[0].div_ceil(scale), height: valid[1].div_ceil(scale), depth_or_array_layers: 1,
                });
        }
        let mut half=windows.filter(|window|window.records.is_some()).peekable();
        if half.peek().is_some() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("copy display mip windows"), timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.half);
            for window in half {
                let valid:[u32;2] = std::array::from_fn(|axis| (window.plan.extent[axis] - origin[axis]).min(PAGE_SIZE));
                let scale = 1 << window.plan.level;
                let columns = window.plan.bounds.width().div_ceil(PAGE_SIZE);
                let offset = ((coordinate[1] - window.plan.bounds.min_y()/PAGE_SIZE)*columns
                    + coordinate[0] - window.plan.bounds.min_x()/PAGE_SIZE)*window.stride;
                pass.set_bind_group(0, &window.records.as_ref().unwrap().binding, &[offset]);
                pass.dispatch_workgroups(valid[0].div_ceil(scale).div_ceil(8), valid[1].div_ceil(scale).div_ceil(8), 1);
            }
        }
        Ok(())
    }
}
