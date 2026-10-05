use crate::{display_mips, GpuRasterError, PipelineDevice, WgpuRasterizer, PAGE_SIZE};
use layer_core::{authored::PortableId, color::{RgbSpace,source::SourceImage}};
use std::sync::{Arc,Weak,atomic::{AtomicBool,Ordering}};

#[derive(Clone)]
pub(crate) struct MovingRequest {
    pub id: PortableId,
    pub source: Arc<SourceImage>,
    pub level: u32,
}
#[derive(Clone)]
pub(crate) struct MovingSource {
    pub view: wgpu::TextureView,
    pub level: u32,
    pub extent: [u32;2],
}
struct Planned {
    request: MovingRequest,
    plan: display_mips::Plan,
    last: u32,
}
pub(crate) struct MipPlan {
    targets: Vec<Planned>,
    pub bytes: u64,
    pub waiting:bool,
}
struct Entry {
    id: PortableId,
    source: Weak<SourceImage>,
    image: display_mips::Image,
    cursor: u32,
    coarse: u32,
    updates: Option<display_mips::CompleteUpdates>,
    update_bytes: u64,
    valid: Arc<std::sync::atomic::AtomicBool>,
    ready: bool,
}
impl Entry {
    fn matches(&self,id:PortableId,source:&Arc<SourceImage>)->bool {self.id==id && self.source.ptr_eq(&Arc::downgrade(source))}
    fn tiles(&self)->u32 {self.image.plan.extent[0].div_ceil(PAGE_SIZE)*self.image.plan.extent[1].div_ceil(PAGE_SIZE)}
    fn storage_bytes(&self)->u64 {self.image.storage_bytes()+self.update_bytes}
}
struct Retired {entry:Entry,complete:Arc<AtomicBool>}
struct RetirementCompletion(Arc<AtomicBool>);
impl Drop for RetirementCompletion {
    fn drop(&mut self) {self.0.store(true,Ordering::Release);}
}
#[derive(Default)]
pub(crate) struct MovingImages {
    entries:Vec<Entry>,
    retired:Vec<Retired>,
    allocation_waiting:bool,
    context:Option<RgbSpace>,
    pipelines:Option<display_mips::Pipelines>,
    pub tile_builds:u64,
    pub completed_images:u64,
    pub evictions:u64,
}

fn padded_extent(extent:[u32;2])->[u32;2] {extent.map(|v|v.div_ceil(PAGE_SIZE)*PAGE_SIZE)}
fn update_bytes(device:&PipelineDevice,plan:display_mips::Plan,last:u32)->u64 {
    u64::from(plan.extent[0].div_ceil(PAGE_SIZE))*u64::from(plan.extent[1].div_ceil(PAGE_SIZE))*u64::from(last-plan.level)*u64::from(device.limits().min_uniform_buffer_offset_alignment.max(16))
}
fn estimate(device:&PipelineDevice,plan:display_mips::Plan,last:u32)->u64 {
    plan.pixel_bytes_through(last)+u64::from(plan.level)*16+update_bytes(device,plan,last)
}
impl MovingImages {
    pub(crate) fn pipelines(&mut self,device:&PipelineDevice)->[&crate::Deferred<wgpu::ComputePipeline>;2] {
        let pipelines=self.pipelines.get_or_insert_with(||display_mips::Pipelines::new(device));
        [&pipelines.reduce,&pipelines.fused_reduce]
    }
    pub(crate) fn level(inverse:[f64;6])->u32 {
        let [a,b,c,d,_,_]=inverse;
        let major=((a+d).hypot(b-c)+(a-d).hypot(b+c))*0.5;
        let minor=(a*d-b*c).abs()/major;
        if !minor.is_finite() {return 1;}
        minor.max(1.).log2().floor().clamp(1.,f64::from(display_mips::MAX_LEVEL)) as u32
    }
    pub(crate) fn storage_bytes(&self)->u64 {self.entries.iter().map(Entry::storage_bytes).sum::<u64>()+self.retired.iter().map(|retired|retired.entry.storage_bytes()).sum::<u64>()}
    pub(crate) fn pending(&self)->bool {self.allocation_waiting || !self.retired.is_empty() || self.entries.iter().any(|entry|!entry.ready || !entry.valid.load(Ordering::Acquire))}
    pub(crate) fn plan(&mut self,r:&WgpuRasterizer,requests:&[MovingRequest],budget:u64,encoder:&mut crate::submission::CommandEncoder)->Result<MipPlan,GpuRasterError> {
        self.retired.retain(|retired|!retired.complete.load(Ordering::Acquire));
        self.allocation_waiting=false;
        let context=r.device.working_space();
        let changed=self.context!=Some(context);
        let mut retained=Vec::new();
        for entry in self.entries.drain(..) {
            if !changed && requests.iter().any(|request|entry.matches(request.id,&request.source)) {retained.push(entry);}
            else {
                self.evictions+=1;
                let complete=Arc::new(AtomicBool::new(false));
                let callback=RetirementCompletion(complete.clone());
                encoder.on_submitted_work_done(move || drop(callback));
                self.retired.push(Retired {entry,complete});
            }
        }
        self.entries=retained;
        if changed {self.context=Some(context);}
        let mut targets=Vec::<Planned>::new();
        for request in requests {
            if self.entries.iter().any(|entry|entry.matches(request.id,&request.source))
                || targets.iter().any(|target|target.request.id==request.id && Arc::ptr_eq(&target.request.source,&request.source)) {continue;}
            let extent=padded_extent(request.source.extent);
            let level=request.level.clamp(1,display_mips::MAX_LEVEL).max((0..=display_mips::MAX_LEVEL).find(|&level|extent.iter().all(|n|n.div_ceil(1<<level)<=r.device.limits().max_texture_dimension_2d)).ok_or(GpuRasterError::ExtentUnsupported)?);
            let plan=display_mips::Plan::at(extent,level);
            let last=display_mips::MAX_LEVEL.min(level+plan.size.into_iter().max().unwrap().next_power_of_two().ilog2());
            targets.push(Planned {request:request.clone(),plan,last});
        }
        loop {
            let bytes=self.retired.iter().map(|retired|retired.entry.storage_bytes()).sum::<u64>()+self.entries.iter().map(|entry|estimate(&r.device,entry.image.plan,entry.image.last_level()).max(entry.storage_bytes())).sum::<u64>()+targets.iter().map(|target|estimate(&r.device,target.plan,target.last)).sum::<u64>();
            if bytes<=budget {return Ok(MipPlan {targets,bytes,waiting:false});}
            let Some(target)=targets.iter_mut().filter(|target|target.plan.level<display_mips::MAX_LEVEL).max_by_key(|target|estimate(&r.device,target.plan,target.last)) else {
                if !self.retired.is_empty() {
                    self.allocation_waiting=true;
                    return Ok(MipPlan {targets:Vec::new(),bytes:self.storage_bytes(),waiting:true});
                }
                if let Some(index)=self.entries.iter().enumerate().filter(|(_,entry)|entry.image.plan.level<display_mips::MAX_LEVEL).max_by_key(|(_,entry)|entry.storage_bytes()).map(|(index,_)|index) {
                    let entry=self.entries.swap_remove(index);
                    self.evictions+=1;
                    let complete=Arc::new(AtomicBool::new(false));
                    let callback=RetirementCompletion(complete.clone());
                    encoder.on_submitted_work_done(move || drop(callback));
                    self.retired.push(Retired {entry,complete});
                    self.allocation_waiting=true;
                    return Ok(MipPlan {targets:Vec::new(),bytes:self.storage_bytes(),waiting:true});
                }
                return Ok(MipPlan {targets:Vec::new(),bytes:self.storage_bytes(),waiting:false});
            };
            target.plan=display_mips::Plan::at(target.plan.extent,target.plan.level+1);
            target.last=display_mips::MAX_LEVEL.min(target.plan.level+target.plan.size.into_iter().max().unwrap().next_power_of_two().ilog2());
        }
    }
    pub(crate) fn allocate(&mut self,r:&WgpuRasterizer,plan:MipPlan) {
        if !plan.targets.is_empty() {self.pipelines(&r.device);}
        for target in plan.targets {
            self.entries.push(Entry {id:target.request.id,source:Arc::downgrade(&target.request.source),image:display_mips::Image::with_mips(r,target.plan,target.last),cursor:0,coarse:0,updates:None,update_bytes:0,valid:Arc::new(std::sync::atomic::AtomicBool::new(true)),ready:false});
        }
    }
    pub(crate) fn lookup(&self,id:PortableId,source:&Arc<SourceImage>,requested:u32)->Option<MovingSource> {
        let entry=self.entries.iter().find(|entry|entry.matches(id,source) && entry.ready && entry.valid.load(Ordering::Acquire))?;
        let level=requested.clamp(entry.image.plan.level,entry.image.last_level());
        let view=entry.image.texture.create_view(&wgpu::TextureViewDescriptor {base_mip_level:level-entry.image.plan.level,mip_level_count:Some(1),..Default::default()});
        Some(MovingSource {view,level,extent:source.extent.map(|v|v.div_ceil(1<<level))})
    }
    pub(crate) fn admitted(&self,id:PortableId,source:&Arc<SourceImage>)->bool {self.entries.iter().any(|entry|entry.matches(id,source))}
    pub(crate) fn next_build(&mut self)->Option<(PortableId,Arc<SourceImage>,[u32;2])> {
        for entry in &mut self.entries {
            if !entry.valid.load(Ordering::Acquire) {entry.cursor=0;entry.coarse=0;entry.ready=false;entry.valid=Arc::new(std::sync::atomic::AtomicBool::new(true));}
            if entry.ready || entry.cursor==entry.tiles() {continue;}
            let source=entry.source.upgrade()?;
            let columns=entry.image.plan.extent[0].div_ceil(PAGE_SIZE);
            return Some((entry.id,source,[entry.cursor%columns,entry.cursor/columns]));
        }
        None
    }
    pub(crate) fn accepts_tile(&self,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.entries.iter().any(|entry|entry.matches(id,source) && !entry.ready && entry.valid.load(Ordering::Acquire)
            && {let columns=entry.image.plan.extent[0].div_ceil(PAGE_SIZE);coordinate==[entry.cursor%columns,entry.cursor/columns]})
    }
    pub(crate) fn write_tile(&mut self,r:&WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],texture:&wgpu::Texture)->Result<(),GpuRasterError> {
        let entry=self.entries.iter_mut().find(|entry|entry.matches(id,source)).ok_or(GpuRasterError::InvalidImage)?;
        let columns=entry.image.plan.extent[0].div_ceil(PAGE_SIZE);
        if coordinate!=[entry.cursor%columns,entry.cursor/columns] {return Err(GpuRasterError::InvalidImage);}
        entry.image.write_tile(&r.device,self.pipelines.as_ref().unwrap(),encoder,texture,[0;2],coordinate)?;
        crate::submission::CacheWrite::shared(entry.valid.clone()).track(encoder);
        entry.cursor+=1;
        self.tile_builds+=1;
        if entry.cursor==entry.tiles() && entry.image.last_level()==entry.image.plan.level {entry.ready=true;self.completed_images+=1;}
        Ok(())
    }
    pub(crate) fn advance_coarse(&mut self,r:&WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder)->bool {
        let Some(entry)=self.entries.iter_mut().find(|entry|!entry.ready && entry.valid.load(Ordering::Acquire) && entry.cursor==entry.tiles()) else {return false;};
        if entry.updates.is_none() {
            let views:Vec<_>=(0..entry.image.texture.mip_level_count()).map(|level|entry.image.texture.create_view(&wgpu::TextureViewDescriptor {base_mip_level:level,mip_level_count:Some(1),..Default::default()})).collect();
            let plan=display_mips::Plan::at(entry.image.plan.extent,entry.image.last_level());
            entry.update_bytes=update_bytes(&r.device,entry.image.plan,entry.image.last_level());
            entry.updates=Some(display_mips::CompleteUpdates::from_level(&r.device,self.pipelines.as_ref().unwrap(),plan,entry.image.plan.level,&views.iter().collect::<Vec<_>>()));
        }
        let columns=entry.image.plan.extent[0].div_ceil(PAGE_SIZE);
        let updates=entry.updates.as_mut().unwrap();
        updates.tile(encoder,[entry.coarse%columns,entry.coarse/columns]);
        updates.flush(encoder);
        crate::submission::CacheWrite::shared(entry.valid.clone()).track(encoder);
        entry.coarse+=1;
        if entry.coarse==entry.tiles() {entry.ready=true;self.completed_images+=1;}
        true
    }
}

pub(crate) struct ImageDecodeRequest {
    pub id:PortableId,
    pub source:Arc<SourceImage>,
    pub coordinate:[u32;2],
}
pub(crate) type CoordinateResult = Result<Arc<Vec<u8>>,String>;
pub(crate) type CoordinateDestination = Weak<std::sync::Mutex<Option<CoordinateResult>>>;
pub(crate) struct CoordinateRequest {
    pub inverse:[f64;6],
    pub size:[u32;2],
    pub first:u32,
    pub count:u32,
    pub destination:CoordinateDestination,
}
pub(crate) enum MipDecodeRequest {
    Image(ImageDecodeRequest),
    Coordinates(CoordinateRequest),
}
pub(crate) enum PreparedImageWork {
    Image(PreparedMipTile),
    Coordinates {destination:CoordinateDestination,pixels:CoordinateResult},
}
pub fn prepare_image_tile(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace,cache:&layer_core::raster::DecodedTileCache,decoder:Option<&layer_color::WorkingDecoder>)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    use crate::scene::sources::PreparedSourcePixels;
    if matches!(source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) && source.interpretation.channels!=layer_core::color::source::SourceChannels::Cmyk {
        let tile=source.tiles.get(&coordinate).ok_or("Missing immutable image tile")?;
        let native=cache.decode(tile)?;
        if source.interpretation.channels==layer_core::color::source::SourceChannels::Rgba {return Ok(PreparedSourcePixels::NativeSamples(native));}
        let row=PAGE_SIZE as usize*4*source.interpretation.depth.bytes();
        let mut expanded=vec![0;row*PAGE_SIZE as usize];
        for (input,output) in native.chunks_exact(PAGE_SIZE as usize*source.interpretation.pixel_bytes()).zip(expanded.chunks_exact_mut(row)) {
            crate::scene::sources::expand_source_row(input,output,source.interpretation.channels,source.interpretation.depth);
        }
        return Ok(PreparedSourcePixels::NativeSamples(Arc::new(expanded)));
    }
    let owned;
    let decoder=if let Some(decoder)=decoder {decoder} else {
        owned=layer_color::WorkingDecoder::new(&source.interpretation,destination,Default::default())?;
        &owned
    };
    let mut pixels=vec![[0.;4];(PAGE_SIZE*PAGE_SIZE) as usize];
    decoder.decode_tile_cached(source,coordinate,&mut pixels,cache)?;
    let bytes=pixels.into_iter().flat_map(|[r,g,b,a]|[r*a,g*a,b*a,a]).flat_map(f32::to_ne_bytes).collect();
    Ok(PreparedSourcePixels::PremultipliedWorkingPixels(Arc::new(bytes)))
}
pub(crate) struct PreparedMipTile {
    pub id:PortableId,
    pub source:Arc<SourceImage>,
    pub coordinate:[u32;2],
    pub pixels:Result<crate::scene::sources::PreparedSourcePixels,String>,
}
pub(crate) struct MipDecodeQueue {
    #[cfg(not(target_arch="wasm32"))]
    requests:std::sync::mpsc::SyncSender<MipDecodeRequest>,
    #[cfg(not(target_arch="wasm32"))]
    results:std::sync::mpsc::Receiver<PreparedImageWork>,
    #[cfg(target_arch="wasm32")]
    external:Option<MipDecodeRequest>,
    #[cfg(target_arch="wasm32")]
    completed:Option<PreparedImageWork>,
    ready:Arc<AtomicBool>,
    pending:bool,
}
impl MipDecodeQueue {
    pub(crate) fn new(device:&PipelineDevice)->Self {
        #[cfg(not(target_arch="wasm32"))]
        {Self::native(device.source_samples.clone(),device.working_space())}
        #[cfg(target_arch="wasm32")]
        {let _=device;Self {external:None,completed:None,ready:Arc::new(AtomicBool::new(false)),pending:false}}
    }
    #[cfg(not(target_arch="wasm32"))]
    fn native(cache:Arc<layer_core::raster::DecodedTileCache>,destination:RgbSpace)->Self {
        let ready=Arc::new(AtomicBool::new(false));
            let (requests,receive)=std::sync::mpsc::sync_channel::<MipDecodeRequest>(1);
            let (send,results)=std::sync::mpsc::sync_channel(1);
            let completion=ready.clone();
            std::thread::Builder::new().name("capy-image-prefilter".into()).spawn(move || {
                let mut decoders=std::collections::VecDeque::<(Weak<SourceImage>,layer_color::WorkingDecoder)>::new();
                while let Ok(request)=receive.recv() {
                    let result=match request {
                    MipDecodeRequest::Coordinates(request) => {
                        let pixels=if request.destination.strong_count()==0 {Err("Coordinate request cancelled".into())}
                            else {crate::object_sampling::prepare_nearest_coordinates(request.inverse,request.size,request.first,request.count)};
                        PreparedImageWork::Coordinates {destination:request.destination,pixels}
                    },
                    MipDecodeRequest::Image(request) => {
                    let pixels=(|| {
                        if matches!(request.source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) {
                            return prepare_image_tile(&request.source,request.coordinate,destination,&cache,None);
                        }
                        let weak=Arc::downgrade(&request.source);
                        let index=if let Some(index)=decoders.iter().position(|(source,_)|source.ptr_eq(&weak)) {index} else {
                            let decoder=layer_color::WorkingDecoder::new(&request.source.interpretation,destination,Default::default())?;
                            if decoders.len()==4 {decoders.pop_front();}
                            decoders.push_back((weak,decoder));decoders.len()-1
                        };
                        let decoder=decoders.remove(index).unwrap();
                        let decoded=prepare_image_tile(&request.source,request.coordinate,destination,&cache,Some(&decoder.1));
                        decoders.push_back(decoder);
                        decoded
                    })();
                    PreparedImageWork::Image(PreparedMipTile {id:request.id,source:request.source,coordinate:request.coordinate,pixels})
                    },
                    };
                    completion.store(true,Ordering::Release);
                    if send.send(result).is_err() {break;}
                }
            }).expect("Immutable image preparation worker");
            Self {requests,results,ready,pending:false}
    }
    pub(crate) fn request(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.enqueue(MipDecodeRequest::Image(ImageDecodeRequest {id,source,coordinate}))
    }
    pub(crate) fn request_coordinates(&mut self,inverse:[f64;6],size:[u32;2],first:u32,count:u32,destination:CoordinateDestination)->bool {
        self.enqueue(MipDecodeRequest::Coordinates(CoordinateRequest {inverse,size,first,count,destination}))
    }
    fn enqueue(&mut self,request:MipDecodeRequest)->bool {
        if self.pending {return false;}
        self.ready.store(false,Ordering::Release);
        #[cfg(not(target_arch="wasm32"))]
        if self.requests.try_send(request).is_err() {return false;}
        #[cfg(target_arch="wasm32")]
        {self.external=Some(request);}
        self.pending=true;true
    }
    pub(crate) fn poll(&mut self)->Option<PreparedImageWork> {
        #[cfg(not(target_arch="wasm32"))]
        let result=self.results.try_recv().ok()?;
        #[cfg(target_arch="wasm32")]
        let result=self.completed.take()?;
        self.pending=false;
        self.ready.store(false,Ordering::Release);
        Some(result)
    }
    pub(crate) fn pending(&self)->bool {self.pending}
    pub(crate) fn ready(&self)->bool {self.ready.load(Ordering::Acquire)}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn take_external(&mut self)->Option<MipDecodeRequest> {self.external.take()}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn complete_external(&mut self,result:PreparedImageWork) {self.completed=Some(result);self.ready.store(true,Ordering::Release);}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{page_texture,upload_page};
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn preparation_queue_serializes_geometry_and_image_work_without_retaining_cancelled_geometry() {
        fn result(queue:&mut MipDecodeQueue)->PreparedImageWork {
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
            loop {
                if let Some(result)=queue.poll() {return result;}
                assert!(std::time::Instant::now()<deadline,"prepared work must complete");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let cache=Arc::new(layer_core::raster::DecodedTileCache::new(2*1024*1024));
        let mut queue=MipDecodeQueue::native(cache.clone(),RgbSpace::Srgb);
        let source=layer_core::color::source::rgba8_source([8;2],|_,_|[255,128,0,255]);
        let destination=Arc::new(std::sync::Mutex::new(None));
        let weak=Arc::downgrade(&destination);
        assert!(queue.request_coordinates([1.,0.,0.,1.,3.,-2.],[256;2],17,4,weak.clone()));
        assert!(!queue.request(PortableId::random(),source.clone(),[0;2]));
        let PreparedImageWork::Coordinates {destination:completed,pixels}=result(&mut queue) else {panic!("Geometry request must return geometry");};
        assert!(completed.ptr_eq(&weak));
        assert_eq!(pixels.unwrap().as_slice(),[20_i32,-2,21,-2,22,-2,23,-2].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>());
        assert_eq!(cache.stats().misses,0,"geometry must not read image samples");
        let id=PortableId::random();assert!(queue.request(id,source.clone(),[0;2]));
        let PreparedImageWork::Image(prepared)=result(&mut queue) else {panic!("Image request must return image samples");};
        assert_eq!(prepared.id,id);assert!(Arc::ptr_eq(&prepared.source,&source));assert_eq!(prepared.coordinate,[0;2]);
        let crate::scene::sources::PreparedSourcePixels::NativeSamples(samples)=prepared.pixels.unwrap() else {panic!("Builtin samples retain native precision");};
        assert_eq!(&samples[..4],&[255,128,0,255]);assert_eq!(cache.stats().misses,1);
        assert!(queue.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],0,65536,weak));
        drop(destination);
        let PreparedImageWork::Coordinates {destination:completed,..}=result(&mut queue) else {panic!("Cancelled geometry retains its typed result");};
        assert!(completed.upgrade().is_none(),"queued work must not keep a deleted object pose alive");
        assert!(!queue.pending());
    }
    fn moving_request()->MovingRequest {
        MovingRequest {id:PortableId::random(),source:layer_core::color::source::rgba8_source([256;2],|x,y|{let value=if (x+y)%2==0 {0} else {255};[value,value,value,255]}),level:1}
    }
    #[test]
    fn shared_worker_preparation_preserves_native_codes_and_premultiplies_embedded_profile_pixels() {
        use layer_core::color::{ColorProfile,RgbSpace};
        use crate::scene::sources::PreparedSourcePixels;
        let source=layer_core::color::source::rgba8_source([256;2],|x,_|[255,128,0,if x%2==0 {0} else {128}]);
        let cache=layer_core::raster::DecodedTileCache::new(2*1024*1024);
        let native=prepare_image_tile(&source,[0;2],RgbSpace::Srgb,&cache,None).unwrap();
        let PreparedSourcePixels::NativeSamples(native)=native else {panic!("Builtin source must retain native codes");};
        assert_eq!(&native[..8],&[255,128,0,0,255,128,0,128]);
        let mut embedded=(*source).clone();
        embedded.interpretation.profile=ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap().into());
        let pixels=prepare_image_tile(&embedded,[0;2],RgbSpace::Srgb,&cache,None).unwrap();
        let PreparedSourcePixels::PremultipliedWorkingPixels(bytes)=pixels else {panic!("ICC source must be prepared in working space");};
        assert_eq!(bytes.len(),(PAGE_SIZE*PAGE_SIZE*16) as usize);
        let values:Vec<_>=bytes[..32].chunks_exact(4).map(|bytes|f32::from_ne_bytes(bytes.try_into().unwrap())).collect();
        assert_eq!(&values[..4],&[0.;4]);
        assert!((values[4]-128./255.).abs()<1e-5);
        assert!((values[5]-0.2158605*128./255.).abs()<2e-4);
        assert!(values[6].abs()<1e-5);
        assert_eq!(values[7],128./255.);
    }
    #[test]
    fn native_worker_expansion_handles_integer_gray_and_float_rgb_rows() {
        use layer_core::color::{SampleDepth,source::{SourceBuilder,SourceChannels,SourceInterpretation}};
        use crate::scene::sources::PreparedSourcePixels;
        let cache=layer_core::raster::DecodedTileCache::new(2*1024*1024);
        for (channels,depth,input,expected) in [
            (SourceChannels::Gray,SampleDepth::U8,vec![74],vec![74,74,74,255]),
            (SourceChannels::Gray,SampleDepth::U16,vec![0x34,0x12],vec![0x34,0x12,0x34,0x12,0x34,0x12,255,255]),
            (SourceChannels::Rgb,SampleDepth::F32,[-2f32,0.5,4.].into_iter().flat_map(f32::to_le_bytes).collect(),[-2f32,0.5,4.,1.].into_iter().flat_map(f32::to_le_bytes).collect()),
        ] {
            let mut builder=SourceBuilder::new([1;2],SourceInterpretation {channels,depth,profile:Default::default(),profile_assumed:false},1024*1024).unwrap();
            builder.push_row(&input).unwrap();
            let source=builder.finish().unwrap();
            let pixels=prepare_image_tile(&source,[0;2],RgbSpace::Srgb,&cache,None).unwrap();
            let PreparedSourcePixels::NativeSamples(bytes)=pixels else {panic!("Builtin source must retain native precision");};
            assert_eq!(bytes.len(),PAGE_SIZE as usize*PAGE_SIZE as usize*4*depth.bytes());
            assert_eq!(&bytes[..expected.len()],expected);
        }
    }
    #[test]
    fn moving_image_mips_share_immutable_sources_across_order_and_density_changes() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut images=MovingImages::default();
        let request=moving_request();
        let second=moving_request();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&[request.clone(),second.clone(),request.clone()],u64::MAX,&mut encoder).unwrap();
        assert_eq!(plan.targets.len(),2);
        let reserved=plan.bytes;
        images.allocate(&r,plan);
        let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..256).flat_map(|y|(0..256).flat_map(move|x|{let value=((x+y)%2) as f32;[value,value,value,1.]})).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&texture,&bytes);
        for request in [&request,&second] {images.write_tile(&r,&mut encoder,request.id,&request.source,[0;2],&texture).unwrap();}
        while images.advance_coarse(&r,&mut encoder) {}
        assert!(!images.pending());
        assert!(images.storage_bytes()<=reserved);
        encoder.submit(&r.queue);
        let bytes=crate::layer_tests::page_bytes(&r,&images.entries[0].image.texture);
        for pixel in bytes.chunks_exact(16) {
            let values:[f32;4]=std::array::from_fn(|channel|f32::from_le_bytes(pixel[channel*4..channel*4+4].try_into().unwrap()));
            assert_eq!(values,[0.5,0.5,0.5,1.]);
        }
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let coarse=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let entry=&images.entries[0];
        encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {texture:&entry.image.texture,mip_level:entry.image.last_level()-entry.image.plan.level,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},coarse.as_image_copy(),wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1});
        encoder.submit(&r.queue);
        let bytes=crate::layer_tests::page_bytes(&r,&coarse);
        let values:[f32;4]=std::array::from_fn(|channel|f32::from_le_bytes(bytes[channel*4..channel*4+4].try_into().unwrap()));
        assert_eq!(values,[0.5,0.5,0.5,1.]);
        let builds=images.tile_builds;
        for level in [1,4,2,7,1] {
            let mut request=request.clone();request.level=level;
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let plan=images.plan(&r,&[second.clone(),request.clone(),request.clone()],reserved,&mut encoder).unwrap();
            assert!(plan.targets.is_empty());
            assert!(!plan.waiting);
            images.allocate(&r,plan);
            assert!(images.lookup(request.id,&request.source,level).is_some());
            assert_eq!(images.tile_builds,builds);
            encoder.submit(&r.queue);
        }
        let changed=MovingRequest {source:Arc::new((*request.source).clone()),..request};
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&[changed.clone(),second],u64::MAX,&mut encoder).unwrap();
        assert_eq!(plan.targets.len(),1);
        assert!(images.lookup(changed.id,&changed.source,1).is_none());
        assert!(!images.retired.is_empty());
        encoder.submit(&r.queue);
    }
    #[test]
    fn adding_an_image_rebudgets_retained_mips_without_rejecting_artwork() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut images=MovingImages::default();
        let first=moving_request();let second=moving_request();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let initial=images.plan(&r,std::slice::from_ref(&first),u64::MAX,&mut encoder).unwrap();
        let minimum=display_mips::Plan::at([256;2],display_mips::MAX_LEVEL);
        let budget=initial.bytes+estimate(&r.device,minimum,display_mips::MAX_LEVEL)-1;
        images.allocate(&r,initial);
        let plan=images.plan(&r,&[first.clone(),second.clone()],budget,&mut encoder).unwrap();
        assert!(plan.waiting);
        assert_eq!(images.entries.len(),0);
        let retired=images.storage_bytes();
        assert!(retired>0);
        let submission=encoder.submit(&r.queue);
        r.device.poll(wgpu::PollType::Wait {submission_index:Some(submission),timeout:Some(std::time::Duration::from_secs(10))}).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&[first,second],budget,&mut encoder).unwrap();
        assert!(!plan.waiting);
        assert_eq!(plan.targets.len(),2);
        assert!(plan.bytes<=budget);
        assert_eq!(images.storage_bytes(),0);
        images.allocate(&r,plan);
        assert_eq!(images.entries.len(),2);
        assert!(images.entries.iter().all(|entry|entry.image.plan.level>1));
        encoder.submit(&r.queue);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        images.plan(&r,&[],budget,&mut encoder).unwrap();
        assert!(images.storage_bytes()>0);
        assert!(images.pending());
        drop(encoder);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&[],budget,&mut encoder).unwrap();
        assert_eq!(plan.bytes,0);
        assert_eq!(images.storage_bytes(),0);
        assert!(!images.pending());
        let unadmitted=moving_request();
        let plan=images.plan(&r,std::slice::from_ref(&unadmitted),0,&mut encoder).unwrap();
        assert!(plan.targets.is_empty());
        assert!(!plan.waiting);
        images.allocate(&r,plan);
        assert!(!images.admitted(unadmitted.id,&unadmitted.source));
        assert!(!images.pending());
        encoder.submit(&r.queue);
    }
}
