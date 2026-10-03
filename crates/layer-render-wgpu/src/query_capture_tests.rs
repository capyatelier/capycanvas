use super::*;
use crate::{artwork_sample_tests::gpu, artwork_statistics_tests::generated};
use layer_core::{ArtworkQuery, ArtworkSource, color::{DocumentColor, SampleDepth}};
use wgpu::util::DeviceExt;

const PIXELS: [[f32;4];3] = [[0.125,0.25,0.5,1.],[0.25,0.125,0.0625,0.5],[0.;4]];

struct Oracle {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    counts: wgpu::Buffer,
}
impl Oracle {
    fn new(device: &PipelineDevice) -> Self {
        let layout=crate::bindings::layout(device,"query capture oracle",&[
            crate::bindings::texture(0,wgpu::ShaderStages::COMPUTE,false),
            crate::bindings::buffer(1,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Uniform,false,None),
            crate::bindings::buffer(2,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:false},false,None),
        ]);
        let source="struct Area {origin:vec2<u32>,size:vec2<u32>}
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var<uniform> area:Area;
@group(0) @binding(2) var<storage,read_write> counts:array<atomic<u32>>;
@compute @workgroup_size(64)
fn check(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=area.size.x*area.size.y {return;}
    let local=vec2(id.x%area.size.x,id.x/area.size.x);
    let global=area.origin+local;
    let index=(global.x+3u*global.y)%3u;
    let values=array(vec4(0.125,0.25,0.5,1.),vec4(0.25,0.125,0.0625,0.5),vec4(0.));
    let actual=textureLoad(image,vec2<i32>(local),0);
    atomicAdd(&counts[index],1u);
    if any(abs(actual-values[index])>vec4(0.000001)) {atomicAdd(&counts[3],1u);}
}";
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("query capture oracle"),source:wgpu::ShaderSource::Wgsl(source.into())});
        let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:Some("query capture oracle"),bind_group_layouts:&[Some(&layout)],immediate_size:0});
        let pipeline=device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {label:Some("query capture oracle"),layout:Some(&pipeline_layout),module:&shader,entry_point:Some("check"),compilation_options:Default::default(),cache:None});
        let counts=device.create_buffer(&wgpu::BufferDescriptor {label:Some("query capture oracle counts"),size:16,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false});
        Self {pipeline,layout,counts}
    }
    fn encode(&self,r:&mut WgpuRasterizer,view:&wgpu::TextureView,region:PixelRect,encoder:&mut submission::CommandEncoder) {
        let words=[region.min_x(),region.min_y(),region.width(),region.height()];
        let area=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("query capture oracle region"),contents:&words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),usage:wgpu::BufferUsages::UNIFORM});
        let binding=crate::bindings::group(&r.device,"query capture oracle",&self.layout,[wgpu::BindingResource::TextureView(view),area.as_entire_binding(),self.counts.as_entire_binding()]);
        let mut pass=encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);pass.set_bind_group(0,&binding,&[]);pass.dispatch_workgroups((region.area() as u32).div_ceil(64),1,1);
    }
    fn read(&self,gpu:&SnapshotGpu)->[u32;4] {
        let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&gpu.device,&gpu.queue,&self.counts)).unwrap();
        std::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()))
    }
}
fn query(extent:[u32;2],control:CaptureControl)->(SnapshotGpu,SnapshotRenderer,scene::Output) {
    let doc=generated(extent,DocumentColor {depth:SampleDepth::F32,..Default::default()},&PIXELS);
    let gpu=gpu();let (capture,output)=pollster::block_on(gpu.artwork_capture(&ArtworkQuery::new(&doc,ArtworkSource::Visible),control)).unwrap();
    (gpu,capture,output)
}
fn required(result:Result<(&mut WgpuRasterizer,FramePacket<'_>,PixelRect,submission::CommandEncoder),GpuRasterError>)->u64 {
    match result {Err(GpuRasterError::CaptureBudget {required,..})=>required,Err(error)=>panic!("unexpected admission failure: {error}"),Ok(_)=>panic!("zero budget admitted capture")}
}

#[test]
fn bounded_query_budget_splitting_preserves_each_pixel_once_across_windows_and_tile_edges() {
    for batch in [1,4] {
    let extent=[1033,517];let (gpu,mut capture,output)=query(extent,CaptureControl::default());
    let oracle=Oracle::new(&gpu.device);let reserved=oracle.counts.size()+16;
    capture.planned_pixel_bytes=0;
    let small=required(capture.prepare_region_gpu([0,0,256,256],reserved));
    let large=required(capture.prepare_region_gpu([0,0,1024,517],reserved));
    assert!(large>small);capture.planned_pixel_bytes=small+(large-small)/2;
    let mut visited=vec![0u8;(extent[0]*extent[1]) as usize];
    let mut expected=[0u32;3];
    pollster::block_on(capture.capture_query_gpu(output,false,batch,reserved,None,|r,view,region,encoder| {
        assert!(region.width()<=256 && region.height()<=256);
        for y in region.min_y()..region.max_y() {for x in region.min_x()..region.max_x() {
            let index=(y*extent[0]+x) as usize;visited[index]+=1;expected[((x+3*y)%3) as usize]+=1;
        }}
        oracle.encode(r,view,region,encoder);Ok(())
    })).unwrap();
    assert!(visited.into_iter().all(|n|n==1));
    assert_eq!(oracle.read(&gpu),[expected[0],expected[1],expected[2],0]);
    }
}

#[test]
fn bounded_query_cancellation_after_completed_tile_refuses_partial_capture() {
    for extent in [[512,256],[1024,512]] {
    let control=CaptureControl::default();let (gpu,mut capture,output)=query(extent,control.clone());
    let oracle=Oracle::new(&gpu.device);let mut callbacks=0;
    let result=pollster::block_on(capture.capture_query_gpu(output,false,1,oracle.counts.size()+16,None,|r,view,region,encoder| {
        callbacks+=1;
        if callbacks==2 {
            let counts=oracle.read(&gpu);assert_eq!(counts[..3].iter().sum::<u32>(),256*256);
            control.cancel();return Ok(());
        }
        oracle.encode(r,view,region,encoder);Ok(())
    }));
    assert!(matches!(result,Err(GpuRasterError::Color(ref error)) if error.contains("cancel")),"{result:?}");
    assert_eq!(callbacks,2);
    let counts=oracle.read(&gpu);assert_eq!(counts[..3].iter().sum::<u32>(),256*256);assert_eq!(counts[3],0);
    assert!(control.is_cancelled());
    }
}
