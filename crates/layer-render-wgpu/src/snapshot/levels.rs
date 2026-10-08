use super::*;
use layer_core::{ArtworkQuery,ArtworkSource,levels::LevelsStatistics};
use wgpu::util::DeviceExt;

pub(crate) struct LevelsPipeline {
    layout:wgpu::BindGroupLayout,
    measure:wgpu::ComputePipeline,
    count:wgpu::ComputePipeline,
    fold:wgpu::ComputePipeline,
}
impl LevelsPipeline {
    fn new(device:&PipelineDevice)->Arc<Self> {
        device.levels_pipeline.get_or_init(|| {
            let layout=crate::bindings::layout(device,"Levels statistics",&[
                crate::bindings::texture(0,wgpu::ShaderStages::COMPUTE,false),
                crate::bindings::buffer(1,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Uniform,false,None),
                crate::bindings::buffer(2,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:false},false,None),
                crate::bindings::buffer(3,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:false},false,None),
            ]);
            let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("Levels statistics"),source:wgpu::ShaderSource::Wgsl(include_str!("levels.wgsl").into())});
            let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:Some("Levels statistics"),bind_group_layouts:&[Some(&layout)],immediate_size:0});
            let pipeline=|entry|device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {label:Some(entry),layout:Some(&pipeline_layout),module:&shader,entry_point:Some(entry),compilation_options:Default::default(),cache:None});
            Arc::new(Self {layout,measure:pipeline("measure"),count:pipeline("count"),fold:pipeline("fold")})
        }).clone()
    }
}
impl SnapshotGpu {
    pub async fn levels_statistics(&self,query:ArtworkQuery,control:CaptureControl)->Result<LevelsStatistics,String> {
        if !matches!(query.source,ArtworkSource::EffectInput(id)|ArtworkSource::EffectChannels(id) if query.snapshot.view().effect(id).is_some_and(|e|e.program.id.as_ref()=="levels")) {
            return Err("Auto requires a Levels adjustment".into());
        }
        let (mut snapshot,output)=self.artwork_capture(&query,control.clone()).await?;
        let pipeline=LevelsPipeline::new(&self.device);
        let initial:Vec<u8>=[u32::MAX,u32::MAX,u32::MAX,0,0,0,0,0].into_iter().chain(std::iter::repeat_n(0,12288)).flat_map(u32::to_le_bytes).collect();
        let summary=self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("Levels exact summary"),contents:&initial,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC});
        let shards=self.device.create_buffer(&wgpu::BufferDescriptor {label:Some("Levels quantile shards"),size:16*12288*4,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false});
        let mut result=LevelsStatistics {minimum:[0.;3],maximum:[0.;3],bins:std::array::from_fn(|_|vec![0;4096]),pixels:0};
        for phase in 0..2 {
            let ranges:[f32;12]=std::array::from_fn(|i| {
                let c=i/4;let scale=result.minimum[c].abs().max(result.maximum[c].abs()).max(1.);
                match i%4 {0=>(result.minimum[c]/scale) as f32,1=>(result.maximum[c]/scale) as f32,2=>scale as f32,_=>0.}
            });
            let mut folded=None;
            snapshot.capture_query_gpu(output,false,if phase==0 {4} else {1},summary.size()+shards.size(),None,|r,view,region,encoder| {
                let words=[region.width(),region.height(),query.snapshot.view().composition().color.space as u32,0].into_iter().chain(ranges.map(f32::to_bits)).flat_map(u32::to_le_bytes).collect::<Vec<_>>();
                let area=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("Levels region"),contents:&words,usage:wgpu::BufferUsages::UNIFORM});
                let bind=crate::bindings::group(&r.device,"Levels region",&pipeline.layout,[wgpu::BindingResource::TextureView(view),area.as_entire_binding(),summary.as_entire_binding(),shards.as_entire_binding()]);
                let mut pass=encoder.begin_compute_pass(&Default::default());pass.set_bind_group(0,&bind,&[]);
                pass.set_pipeline(if phase==0 {&pipeline.measure} else {&pipeline.count});pass.dispatch_workgroups(if phase==0 {64} else {16},if phase==0 {1} else {3},1);
                drop(pass);folded=Some(bind);Ok(())
            }).await.map_err(|e|e.to_string())?;
            if phase==1 && let Some(bind)=folded {
                let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
                let mut pass=encoder.begin_compute_pass(&Default::default());pass.set_bind_group(0,&bind,&[]);
                pass.set_pipeline(&pipeline.fold);pass.dispatch_workgroups(192,1,1);drop(pass);encoder.submit(&self.queue);
            }
            let bytes=crate::local_tone::read_buffer_async(&self.device,&self.queue,&summary).await?;
            control.check().map_err(|e|e.to_string())?;
            let words:Vec<_>=bytes.as_chunks::<4>().0.iter().map(|b|u32::from_le_bytes(*b)).collect();
            if words[7]!=0 {return Err("The artwork contains colors Auto cannot represent".into());}
            if words[6]==0 {return Err("There are no usable pixels for Auto".into());}
            if phase==0 {
                let value=|bits:u32|f64::from(f32::from_bits(if bits>>31!=0 {bits^0x80000000} else {!bits}));
                result.minimum=std::array::from_fn(|i|value(words[i]));result.maximum=std::array::from_fn(|i|value(words[i+3]));result.pixels=u64::from(words[6]);
            } else {for c in 0..3 {for (out,n) in result.bins[c].iter_mut().zip(&words[8+c*4096..8+(c+1)*4096]) {*out=u64::from(*n);}}}
        }
        Ok(result)
    }
}
