use crate::{analysis_compute::{Pipelines, Params, buffer, dims, groups, guide_extent}, PipelineDevice, submission::CommandEncoder};
use layer_core::color::RgbSpace;
use std::sync::Arc;

const ENTRIES: &[&str] = &["reduce_source", "normalize", "minimum", "rank_histogram", "rank_select", "air_reduce", "air_finish", "moments", "refine"];
const SOURCE: &str = concat!(include_str!("float_number.wgsl"), "\n", include_str!("guide_luminance.wgsl"), "\n", include_str!("../../../assets/filters/dehaze-math.wgsl"), "\n", include_str!("dehaze.wgsl"));
fn pipelines(device:&PipelineDevice)->&Arc<Pipelines> {
    device.dehaze_pipelines.get_or_init(||Arc::new(Pipelines::new(device,"Dehaze analysis",SOURCE,ENTRIES)))
}
pub(crate) struct Builder {
    device:PipelineDevice,
    pipelines:Arc<Pipelines>,
    document:[u32;2],
    extent:[u32;2],
    scratch:[wgpu::Buffer;3],
    guide:wgpu::Buffer,
    histogram:wgpu::Buffer,
    state:[wgpu::Buffer;2],
    air:wgpu::Buffer,
    transform:wgpu::Buffer,
}
impl Builder {
    pub(crate) fn allocation_bound(document:[u32;2])->Result<u64,String> {
        let extent=guide_extent(document)?;
        Ok(u64::from(extent[0])*u64::from(extent[1])*112+65536+4096)
    }
    pub(crate) async fn prepare_pipelines(device:&PipelineDevice)->Result<(),String> {pipelines(device).prepare().await}
    pub(crate) fn new(device:&PipelineDevice,document:[u32;2],space:RgbSpace)->Result<Self,String> {
        use wgpu::util::DeviceExt;
        let extent=guide_extent(document)?;let count=u64::from(extent[0])*u64::from(extent[1]);
        let largest=(count*32).max(count*16+32).max(65536);
        let limits=device.limits();
        if largest>u64::from(limits.max_storage_buffer_binding_size) || largest>limits.max_buffer_size {return Err("Dehaze guide exceeds GPU buffer limit".into());}
        let matrix:Vec<u8>=space.linear_transform(RgbSpace::Srgb).into_iter().flat_map(|row|row.into_iter().map(|v|v as f32).chain([0.])).flat_map(f32::to_ne_bytes).collect();
        let transform=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("Dehaze working primaries"),contents:&matrix,usage:wgpu::BufferUsages::STORAGE});
        Ok(Self {device:device.clone(),pipelines:pipelines(device).clone(),document,extent,
            scratch:std::array::from_fn(|_|buffer(device,count*32,"Dehaze scratch")),guide:buffer(device,count*16+32,"Dehaze guide"),
            histogram:buffer(device,65536,"Dehaze rank histogram"),state:std::array::from_fn(|_|buffer(device,32,"Dehaze rank state")),
            air:buffer(device,32,"Dehaze airlight"),transform})
    }
    fn encode(&self,encoder:&mut CommandEncoder,entry:usize,params:Params,inputs:[Option<&wgpu::Buffer>;3],output:&wgpu::Buffer,dispatch:[u32;2]) {
        self.pipelines.encode(&self.device,encoder,entry,params,inputs,output,None,dispatch);
    }
    pub(crate) fn reduce(&self,encoder:&mut CommandEncoder,texture:&wgpu::Texture,origin:[u32;2]) {
        let start=std::array::from_fn::<_,2,_>(|i|origin[i]*self.extent[i]/self.document[i]);
        let end=std::array::from_fn::<_,2,_>(|i|((origin[i]+[texture.width(),texture.height()][i])*self.extent[i]).div_ceil(self.document[i]));
        self.pipelines.encode(&self.device,encoder,0,Params {size:[self.extent[0],self.extent[1],start[0],start[1]],
            aux:[origin[0],origin[1],self.document[0],self.document[1]],..Default::default()},[None,None,Some(&self.transform)],&self.scratch[0],
            Some(&texture.create_view(&Default::default())),[(end[0]-start[0]).div_ceil(8),(end[1]-start[1]).div_ceil(8)]);
    }
    async fn submit(&self,encoder:CommandEncoder,queue:&wgpu::Queue)->Result<(),String> {
        encoder.submit(queue);crate::local_tone::wait_async(&self.device,queue).await
    }
    fn minimum(&self,encoder:&mut CommandEncoder,normalized:bool) {
        let mut p=dims(self.extent);p.aux[1]=u32::from(normalized);
        self.encode(encoder,2,p,[Some(&self.scratch[1]),None,Some(&self.air)],&self.scratch[0],groups(self.extent));
        let mut p=dims(self.extent);p.aux=[1,u32::from(normalized),0,0];
        self.encode(encoder,2,p,[Some(&self.scratch[1]),Some(&self.scratch[0]),Some(&self.air)],&self.guide,groups(self.extent));
    }
    pub(crate) async fn finish(self,queue:&wgpu::Queue,check:impl Fn()->Result<(),String>)->Result<wgpu::Buffer,String> {
        check()?;let mut encoder=CommandEncoder::new(&self.device,&Default::default());
        self.encode(&mut encoder,1,dims(self.extent),[Some(&self.scratch[0]),None,None],&self.scratch[1],groups(self.extent));
        self.minimum(&mut encoder,false);self.submit(encoder,queue).await?;
        for (step,(shift,mode)) in [(24,0),(16,0),(8,0),(0,0),(16,1),(8,1),(0,1)].into_iter().enumerate() {
            check()?;let mut encoder=CommandEncoder::new(&self.device,&Default::default());let mut p=dims(self.extent);p.aux=[shift,mode,0,0];
            self.encode(&mut encoder,3,p,[Some(&self.scratch[1]),Some(&self.guide),Some(&self.state[step%2])],&self.histogram,[64,1]);
            let mut p=dims(self.extent);p.aux=[shift,mode,0,0];
            self.encode(&mut encoder,4,p,[Some(&self.histogram),Some(&self.state[step%2]),None],&self.state[1-step%2],[1,1]);
            self.submit(encoder,queue).await?;
        }
        check()?;let mut encoder=CommandEncoder::new(&self.device,&Default::default());let mut count=self.extent[0]*self.extent[1];
        let mut p=dims(self.extent);p.aux=[0,0,count,0];
        self.encode(&mut encoder,5,p,[Some(&self.scratch[1]),Some(&self.guide),Some(&self.state[1])],&self.scratch[0],[count.div_ceil(256),1]);
        count=count.div_ceil(256);let mut chosen=0;let mut other=2;
        while count>1 {
            let mut p=dims(self.extent);p.aux=[1,0,count,0];
            self.encode(&mut encoder,5,p,[Some(&self.scratch[chosen]),None,None],&self.scratch[other],[count.div_ceil(256),1]);
            count=count.div_ceil(256);std::mem::swap(&mut chosen,&mut other);
        }
        self.encode(&mut encoder,6,dims(self.extent),[Some(&self.scratch[1]),Some(&self.scratch[chosen]),None],&self.air,[1,1]);
        encoder.submit(queue);
        let air=crate::local_tone::read_buffer_async(&self.device,queue,&self.air).await?;
        check()?;if air[24..28]!=[0;4] {return Err("Invalid HDR sample in Dehaze analysis".into());}
        let mut encoder=CommandEncoder::new(&self.device,&Default::default());self.minimum(&mut encoder,true);self.submit(encoder,queue).await?;
        for (entry,axis,input,output) in [(7,0,&self.guide,&self.scratch[2]),(7,1,&self.scratch[2],&self.scratch[0]),(8,0,&self.scratch[0],&self.scratch[2]),(8,1,&self.scratch[2],&self.guide)] {
            check()?;let mut encoder=CommandEncoder::new(&self.device,&Default::default());let mut p=dims(self.extent);p.aux=[axis,0,self.document[0],self.document[1]];
            self.encode(&mut encoder,entry,p,[Some(&self.scratch[1]),Some(input),Some(&self.air)],output,groups(self.extent));self.submit(encoder,queue).await?;
        }
        check()?;Ok(self.guide)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
#[test]
fn dehaze_constant_area_means_keep_exact_airlight_coordinate_ties(){
    let renderer=crate::WgpuRasterizer::new_native_headless(layer_core::color::DocumentColor{space:RgbSpace::Srgb,depth:layer_core::color::SampleDepth::F32}).unwrap();
    let builder=Builder::new(&renderer.device,[769,3],RgbSpace::Srgb).unwrap();let air=builder.air.clone();
    let texture=renderer.device.create_texture(&wgpu::TextureDescriptor{label:None,size:wgpu::Extent3d{width:769,height:3,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Float,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
    let bytes=(0..3).flat_map(|y|(0..769).flat_map(move|x|{let alpha=if y==1 && x%2==0{0.5}else{1.};[0.25*alpha,0.5*alpha,0.75*alpha,alpha].into_iter().flat_map(f32::to_le_bytes)})).collect::<Vec<_>>();
    renderer.queue.write_texture(texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(769*16),rows_per_image:None},texture.size());
    let mut encoder=CommandEncoder::new(&renderer.device,&Default::default());builder.reduce(&mut encoder,&texture,[0,0]);encoder.submit(&renderer.queue);
    pollster::block_on(builder.finish(&renderer.queue,||Ok(()))).unwrap();let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&renderer.device,&renderer.queue,&air)).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()),0);
    for (channel,expected) in [0.25f32,0.5,0.75].into_iter().enumerate(){assert_eq!(f32::from_le_bytes(bytes[channel*4..channel*4+4].try_into().unwrap()),expected);}
}
#[test]
fn dehaze_area_mean_preserves_small_bright_fraction_in_both_tile_orders(){
    let renderer=crate::WgpuRasterizer::new_native_headless(layer_core::color::DocumentColor{space:RgbSpace::Srgb,depth:layer_core::color::SampleDepth::F32}).unwrap();
    let tiny=f32::from_bits((2f32.powi(-32)).to_bits()-1);
    let expected=((1f64+768.*f64::from(tiny))/769.).log2();
    for reverse in [false,true]{
        let builder=Builder::new(&renderer.device,[769,1],RgbSpace::Srgb).unwrap();
        for (x,width) in if reverse {[(768,1),(0,768)]}else{[(0,768),(768,1)]}{
            let texture=renderer.device.create_texture(&wgpu::TextureDescriptor{label:None,size:wgpu::Extent3d{width,height:1,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Float,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
            let bytes=(0..width).flat_map(|i|{let v=if x+i==768{tiny}else{1.};[v,v,v,1.].into_iter().flat_map(f32::to_le_bytes)}).collect::<Vec<_>>();
            renderer.queue.write_texture(texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(width*16),rows_per_image:None},texture.size());
            let mut encoder=CommandEncoder::new(&renderer.device,&Default::default());builder.reduce(&mut encoder,&texture,[x,0]);encoder.submit(&renderer.queue);
        }
        let guide=pollster::block_on(builder.finish(&renderer.queue,||Ok(()))).unwrap();
        let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&renderer.device,&renderer.queue,&guide)).unwrap();
        let at=32+767*16;let actual=f32::from_le_bytes(bytes[at..at+4].try_into().unwrap());
        assert!((f64::from(actual)-expected).abs()<1e-5,"reverse={reverse} actual={actual} expected={expected}");
    }
}
#[test]
fn dehaze_deferred_recipe_does_not_retain_owner_cache(){
    for compiled in [false,true]{
        let renderer=crate::WgpuRasterizer::new_native_headless(layer_core::color::DocumentColor{space:layer_core::color::RgbSpace::Srgb,depth:layer_core::color::SampleDepth::F32}).unwrap();
        let owner=std::sync::Arc::downgrade(&renderer.device.dehaze_pipelines);
        let retained=pipelines(&renderer.device).clone();
        if compiled{pollster::block_on(retained.prepare()).unwrap();}
        drop(renderer);
        assert!(owner.upgrade().is_none(),"Deferred recipes retain the analysis owner cache; compiled={compiled}");
        drop(retained);
    }
}
}
