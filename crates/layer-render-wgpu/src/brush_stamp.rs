use super::*;
use layer_render::BrushStampRequest;
use wgpu::util::DeviceExt;

pub(super) struct BrushStamps {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(u32, Deferred<wgpu::ComputePipeline>)>,
    pending: Option<wgpu::Buffer>,
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
}

fn shader_source(flags:u32) -> Cow<'static,str> {
    compose_wgsl(&[include_str!("brush_types.wgsl"),
        &include_str!("brush_textures.wgsl").replace("@group(3)","@group(1)"),
        &dry_material::coverage_source(flags),include_str!("brush_stamp.wgsl")])
}

impl BrushStamps {
    fn new(r: &WgpuRasterizer) -> Self {
        let entries=[0,1,2,3].map(|binding| bindings::buffer(binding,wgpu::ShaderStages::COMPUTE,
            match binding {1=>wgpu::BufferBindingType::Storage {read_only:true},3=>wgpu::BufferBindingType::Storage {read_only:false},
                _=>wgpu::BufferBindingType::Uniform},false,None));
        let layout=bindings::layout(&r.device,"brush stamp",&entries);
        let pipeline_layout=r.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("brush stamp"),bind_group_layouts:&[Some(&layout),Some(&r.advanced_texture_layout)],immediate_size:0,
        });
        let (tx,rx)=mpsc::channel();
        Self {layout,pipeline_layout,pipelines:Vec::new(),pending:None,tx,rx}
    }

    fn request(&mut self,r:&mut WgpuRasterizer,id:u64,request:&BrushStampRequest)->Result<bool,GpuRasterError> {
        if self.pending.is_some() {return Ok(false);}
        if request.dabs.len()>64 || request.bounds.is_empty()
            || [request.bounds.min.x,request.bounds.min.y,request.bounds.max.x,request.bounds.max.y]
                .iter().any(|v|!v.is_finite())
            || request.shape_strength.is_some_and(|v|!(0.0..=1.0).contains(&v)) {return Err(GpuRasterError::InvalidImage);}
        let completion=request.style.contact.is_some_and(|c|c.dry.is_some()) && request.dabs.iter().any(|d|d.is_completion());
        let fine=(request.style.contact.is_some_and(|c|c.dry.is_some_and(|d|d.corner==0.)) || completion)
            && request.dabs.iter().any(|d|(0..2).any(|i|0.5*(d.radii[i]+d.previous[i])<8.));
        let flags=dry_material::contact_flags(request.style.contact)
            | if request.style.rendering.accumulation==BrushAccumulation::Uniform {128} else {0}
            | if fine {16384} else {0}
            | if completion {dry_material::COMPLETION} else {0};
        let pipeline=if let Some(index)=self.pipelines.iter().position(|(key,_)|*key==flags) {
            self.pipelines.remove(index).1
        } else {
            let (device,layout)=(r.device.for_recipe(),self.pipeline_layout.clone());
            let shader_device=device.clone();
            let shader=Deferred::new(move||shader_device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label:Some("brush stamp"),source:wgpu::ShaderSource::Wgsl(dry_material::specialize_contact(shader_source(flags),flags)),
            }));
            Deferred::pipeline(move |mode|mode.compute(&device,&wgpu::ComputePipelineDescriptor {
                label:Some("brush stamp"),layout:Some(&layout),module:&shader,entry_point:Some("stamp"),
                compilation_options:Default::default(),cache:None,
            }))
        };
        if self.pipelines.len()==8 {self.pipelines.remove(0);}
        self.pipelines.push((flags,pipeline.clone()));
        if !r.background_pipeline_ready(&pipeline) || !r.paper_ready(&request.style, startup::OTHER)? {return Ok(false);}
        let key=r.ensure_texture_set(&request.style)?;
        let mut dabs:Vec<DabGpu>=request.dabs.iter().copied().map(DabGpu::from).collect();
        dry_material::prepare_geometry(&request.style,&mut dabs);
        let mut contacts=dab_bytes(&dabs).to_vec();
        contacts.resize(64*mem::size_of::<DabGpu>(),0);
        let style=StyleGpu::for_brush([192;2],&request.style,0,request.dabs.len() as u32,&r.device,r.paper_style(&request.style));
        let params=[request.bounds.min.x.to_bits(),request.bounds.min.y.to_bits(),
            request.bounds.max.x.to_bits(),request.bounds.max.y.to_bits(),request.dabs.len() as u32,
            u32::from(request.shape_strength.is_some()),request.shape_strength.unwrap_or(1.).to_bits(),0];
        let params:Vec<u8>=params.iter().flat_map(|v|v.to_ne_bytes()).collect();
        let inputs=[(style_bytes(&style),wgpu::BufferUsages::UNIFORM),(contacts.as_slice(),wgpu::BufferUsages::STORAGE),
            (params.as_slice(),wgpu::BufferUsages::UNIFORM)].map(|(contents,usage)|
            r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label:Some("brush stamp input"),contents,usage,
            }));
        let size=192*192;
        let output=r.device.create_buffer(&wgpu::BufferDescriptor {
            label:Some("brush stamp output"),size,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false,
        });
        let mut entries:Vec<_>=inputs.iter().enumerate().map(|(i,buffer)|wgpu::BindGroupEntry {
            binding:i as u32,resource:buffer.as_entire_binding(),
        }).collect();
        entries.push(wgpu::BindGroupEntry {binding:3,resource:output.as_entire_binding()});
        let binding=r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("brush stamp"),layout:&self.layout,entries:&entries});
        let readback=r.device.create_buffer(&wgpu::BufferDescriptor {
            label:Some("brush stamp readback"),size,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false,
        });
        let mut encoder=submission::CommandEncoder::new(&r.device,&wgpu::CommandEncoderDescriptor {label:Some("brush stamp")});
        {
            let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("brush stamp"),timestamp_writes:None});
            pass.set_pipeline(&pipeline);pass.set_bind_group(0,&binding,&[]);
            pass.set_bind_group(1,&r.texture_sets.iter().find(|set|set.key==key).unwrap().bind_group,&[]);
            pass.dispatch_workgroups(1,192,1);
        }
        encoder.copy_buffer_to_buffer(&output,0,&readback,0,size);
        r.last_submission=encoder.submit(&r.queue);
        let tx=self.tx.clone();
        raster::map_then(&r.device,None,&readback,size,move|data|Ok(ReadbackImage {request_id:id,width:192,height:192,stride:192,bytes:data.to_vec()}),
            move|result|{let _=tx.send(result);});
        self.pending=Some(readback);
        Ok(true)
    }
}

impl WgpuRasterizer {
    pub(super) fn start_brush_stamp(&mut self,id:u64,request:&BrushStampRequest)->Result<bool,GpuRasterError> {
        let mut stamps=self.brush_stamps.take().unwrap_or_else(||BrushStamps::new(self));
        let result=stamps.request(self,id,request);self.brush_stamps=Some(stamps);result
    }
    pub(super) fn brush_stamp_pending(&self)->bool {self.brush_stamps.as_ref().is_some_and(|stamps|stamps.pending.is_some())}
    pub(super) fn poll_brush_stamp(&mut self)->Option<Result<ReadbackImage,GpuRasterError>> {
        let stamps=self.brush_stamps.as_mut()?;
        let result=stamps.rx.try_recv().ok()?;stamps.pending=None;Some(result)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamp_uses_valid_shared_coverage() {
        let module=naga::front::wgsl::parse_str(&super::shader_source(u32::MAX)).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
    }
}
