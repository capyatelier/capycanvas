use crate::{display_mips, GpuRasterError, PipelineDevice, WgpuRasterizer, PAGE_SIZE};
use layer_core::{authored::PortableId, color::{RgbSpace,source::SourceImage}};
use std::sync::{Arc,Weak,atomic::{AtomicBool,Ordering}};

#[derive(Clone)]
pub(crate) struct MovingRequest {
    pub id: PortableId,
    pub source: Arc<SourceImage>,
    pub level: u32,
    pub nearest: bool,
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
        minor.max(1.).log2().round().clamp(1.,f64::from(display_mips::MAX_LEVEL)) as u32
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
            if !changed && requests.iter().any(|request|entry.matches(request.id,&request.source) && (!request.nearest || entry.image.plan.level==0)) {retained.push(entry);}
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
            let fits=(0..=display_mips::MAX_LEVEL).find(|&level|extent.iter().all(|n|n.div_ceil(1<<level)<=r.device.limits().max_texture_dimension_2d)).ok_or(GpuRasterError::ExtentUnsupported)?;
            if request.nearest && fits>0 {continue;}
            let level=if request.nearest {0} else {request.level.clamp(1,display_mips::MAX_LEVEL).max(fits)};
            let plan=display_mips::Plan::at(extent,level);
            let last=display_mips::MAX_LEVEL.min(level+plan.size.into_iter().max().unwrap().next_power_of_two().ilog2());
            targets.push(Planned {request:request.clone(),plan,last});
        }
        loop {
            let bytes=self.retired.iter().map(|retired|retired.entry.storage_bytes()).sum::<u64>()+self.entries.iter().map(|entry|estimate(&r.device,entry.image.plan,entry.image.last_level()).max(entry.storage_bytes())).sum::<u64>()+targets.iter().map(|target|estimate(&r.device,target.plan,target.last)).sum::<u64>();
            if bytes<=budget {return Ok(MipPlan {targets,bytes,waiting:false});}
            let Some(target)=targets.iter_mut().filter(|target|!target.request.nearest && target.plan.level<display_mips::MAX_LEVEL).max_by_key(|target|estimate(&r.device,target.plan,target.last)) else {
                if let Some(index)=targets.iter().enumerate().filter(|(_,target)|target.request.nearest).max_by_key(|(_,target)|estimate(&r.device,target.plan,target.last)).map(|(index,_)|index) {
                    targets.swap_remove(index);
                    continue;
                }
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
    pub(crate) fn upcoming(&self,count:usize)->Vec<(PortableId,Arc<SourceImage>,[u32;2])> {
        let mut tiles=Vec::with_capacity(count);
        for entry in self.entries.iter().filter(|entry|!entry.ready && entry.valid.load(Ordering::Acquire)) {
            let Some(source)=entry.source.upgrade() else {continue;};
            let columns=entry.image.plan.extent[0].div_ceil(PAGE_SIZE);
            for cursor in entry.cursor..entry.tiles() {
                if tiles.len()==count {return tiles;}
                tiles.push((entry.id,source.clone(),[cursor%columns,cursor/columns]));
            }
        }
        tiles
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
pub(crate) struct RasterDecodeRequest {
    pub blob:Arc<layer_core::raster::TileBlob>,
    pub space:RgbSpace,
    pub destination:RgbSpace,
    pub scalar:bool,
}
pub(crate) struct PreparedRasterTile {
    pub blob:Arc<layer_core::raster::TileBlob>,
    pub space:RgbSpace,
    pub destination:RgbSpace,
    pub scalar:bool,
    pub pixels:Result<crate::scene::sources::PreparedSourcePixels,String>,
}
pub fn prepare_raster_tile(blob:&Arc<layer_core::raster::TileBlob>,space:RgbSpace,cache:&layer_core::raster::DecodedTileCache)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    use layer_core::color::source::SourceChannels;
    crate::scene::sources::validate_raster(blob,space).map_err(|error|error.to_string())?;
    let native=cache.decode(blob)?;
    if blob.descriptor.channels==4 {return Ok(crate::scene::sources::PreparedSourcePixels::NativeSamples(native));}
    let channels=if blob.descriptor.channels==1 {SourceChannels::Gray} else {SourceChannels::GrayAlpha};
    let depth=blob.descriptor.depth();
    let row=PAGE_SIZE as usize*4*depth.bytes();
    let mut expanded=vec![0;row*PAGE_SIZE as usize];
    for (input,output) in native.chunks_exact(PAGE_SIZE as usize*channels.count()*depth.bytes()).zip(expanded.chunks_exact_mut(row)) {
        crate::scene::sources::expand_source_row(input,output,channels,depth);
    }
    Ok(crate::scene::sources::PreparedSourcePixels::NativeSamples(Arc::new(expanded)))
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
    Raster(RasterDecodeRequest),
    Coordinates(CoordinateRequest),
}
pub(crate) enum PreparedImageWork {
    Image(PreparedMipTile),
    Raster(PreparedRasterTile),
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
pub(crate) fn retain_image_decode(prepared:&PreparedMipTile,space:RgbSpace,failures:&mut Vec<crate::moving_projection::FailedImageTile>)->bool {
    if let Err(error)=&prepared.pixels {
        let weak=Arc::downgrade(&prepared.source);
        failures.retain(|(id,source,coordinate,context,_)|source.strong_count()>0
            && !(*id==prepared.id && source.ptr_eq(&weak) && *coordinate==prepared.coordinate && *context==space));
        failures.push((prepared.id,weak,prepared.coordinate,space,error.clone()));
        false
    } else {true}
}
pub(crate) fn image_decode_failure<'a>(failures:&'a [crate::moving_projection::FailedImageTile],
    id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],space:RgbSpace)->Option<&'a str> {
    let weak=Arc::downgrade(source);
    failures.iter().find(|(owner,stored,tile,context,_)|*owner==id && stored.ptr_eq(&weak) && *tile==coordinate && *context==space)
        .map(|(_,_,_,_,error)|error.as_str())
}

#[derive(Clone)]
enum DecodeKey {
    Image(Weak<SourceImage>,[u32;2]),
    Raster(Weak<layer_core::raster::TileBlob>,RgbSpace,RgbSpace,bool),
}
impl DecodeKey {
    fn matches(&self,other:&Self)->bool {
        match (self,other) {
            (Self::Image(a,p),Self::Image(b,q))=>a.ptr_eq(b) && p==q,
            (Self::Raster(a,space,destination,scalar),Self::Raster(b,other,target,coverage))=>a.ptr_eq(b) && space==other && destination==target && scalar==coverage,
            _=>false,
        }
    }
}
pub(crate) struct MipDecodeQueue {
    #[cfg(not(target_arch="wasm32"))]
    pool:Arc<DecodePool>,
    #[cfg(not(target_arch="wasm32"))]
    context:Arc<DecodeContext>,
    #[cfg(not(target_arch="wasm32"))]
    waiting:Option<u64>,
    #[cfg(not(target_arch="wasm32"))]
    results:std::sync::mpsc::Receiver<PreparedImageWork>,
    #[cfg(target_arch="wasm32")]
    external:Option<MipDecodeRequest>,
    #[cfg(target_arch="wasm32")]
    completed:Option<PreparedImageWork>,
    ready:Arc<std::sync::atomic::AtomicUsize>,
    outstanding:Vec<Option<DecodeKey>>,
    capacity:usize,
}
#[cfg(not(target_arch="wasm32"))]
struct DecodeContext {
    cache:Arc<layer_core::raster::DecodedTileCache>,destination:RgbSpace,
    send:std::sync::mpsc::SyncSender<PreparedImageWork>,ready:Arc<std::sync::atomic::AtomicUsize>,
}
#[cfg(not(target_arch="wasm32"))]
struct DecodeJob {request:MipDecodeRequest,context:Weak<DecodeContext>}
#[cfg(not(target_arch="wasm32"))]
struct DecodePool {requests:std::sync::mpsc::SyncSender<DecodeJob>,capacity:usize,epoch:Arc<std::sync::atomic::AtomicU64>}
#[cfg(not(target_arch="wasm32"))]
type Decoders=std::collections::VecDeque<(Weak<SourceImage>,RgbSpace,layer_color::WorkingDecoder)>;
#[cfg(not(target_arch="wasm32"))]
impl DecodeJob {
    fn run(self,decoders:&mut Decoders) {
        let Some(context)=self.context.upgrade() else {return;};
        let (cache,destination)=(context.cache.clone(),context.destination);drop(context);
        let result=prepare_request(self.request,destination,&cache,decoders);
        if let Some(context)=self.context.upgrade() && context.send.send(result).is_ok() {
            context.ready.fetch_add(1,Ordering::AcqRel);
        }
    }
}
#[cfg(not(target_arch="wasm32"))]
impl DecodePool {
    fn shared()->Result<Arc<Self>,GpuRasterError> {
        static POOL:std::sync::OnceLock<std::io::Result<Arc<DecodePool>>>=std::sync::OnceLock::new();
        POOL.get_or_init(||Self::start(std::thread::available_parallelism().map_or(1,|count|count.get().saturating_sub(2).clamp(1,3))))
            .as_ref().cloned().map_err(|_|GpuRasterError::Color("Image preparation worker unavailable".into()))
    }
    fn start(workers:usize)->std::io::Result<Arc<Self>> {
        let capacity=workers*6;
        let (requests,receive)=std::sync::mpsc::sync_channel::<DecodeJob>(capacity);
        let epoch=Arc::new(std::sync::atomic::AtomicU64::new(0));
        let receive=Arc::new(std::sync::Mutex::new(receive));
        for worker in 0..workers {
            let (receive,epoch)=(receive.clone(),epoch.clone());
            std::thread::Builder::new().name(format!("capy-image-prefilter-{worker}")).spawn(move || {
                let mut decoders=Decoders::new();
                loop {
                    let Ok(job)=receive.lock().unwrap().recv() else {break;};
                    epoch.fetch_add(1,Ordering::Release);
                    job.run(&mut decoders);
                }
            })?;
        }
        Ok(Arc::new(Self {requests,capacity,epoch}))
    }
}
#[cfg(not(target_arch="wasm32"))]
fn contained<T,E:From<String>>(work:impl FnOnce()->Result<T,E>)->Result<T,E> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|payload| Err(crate::panic_message(&*payload).into()))
}
#[cfg(not(target_arch="wasm32"))]
fn prepare_request(request:MipDecodeRequest,destination:RgbSpace,cache:&layer_core::raster::DecodedTileCache,decoders:&mut Decoders)->PreparedImageWork {
    match request {
        MipDecodeRequest::Raster(request) => {
            let pixels=contained(||prepare_raster_tile(&request.blob,request.space,cache));
            PreparedImageWork::Raster(PreparedRasterTile {blob:request.blob,space:request.space,destination:request.destination,scalar:request.scalar,pixels})
        },
        MipDecodeRequest::Coordinates(request) => {
            let pixels=contained(|| if request.destination.strong_count()==0 {Err("Coordinate request cancelled".into())}
                else {crate::object_sampling::prepare_nearest_coordinates(request.inverse,request.size,request.first,request.count)});
            PreparedImageWork::Coordinates {destination:request.destination,pixels}
        },
        MipDecodeRequest::Image(request) => {
            let pixels=contained(|| {
                if matches!(request.source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) {
                    return prepare_image_tile(&request.source,request.coordinate,destination,cache,None);
                }
                let weak=Arc::downgrade(&request.source);
                let index=if let Some(index)=decoders.iter().position(|(source,space,_)|source.ptr_eq(&weak) && *space==destination) {index} else {
                    let decoder=layer_color::WorkingDecoder::new(&request.source.interpretation,destination,Default::default())?;
                    if decoders.len()==4 {decoders.pop_front();}
                    decoders.push_back((weak,destination,decoder));decoders.len()-1
                };
                let decoder=decoders.remove(index).unwrap();
                let decoded=prepare_image_tile(&request.source,request.coordinate,destination,cache,Some(&decoder.2));
                decoders.push_back(decoder);
                decoded
            });
            PreparedImageWork::Image(PreparedMipTile {id:request.id,source:request.source,coordinate:request.coordinate,pixels})
        },
    }
}
impl MipDecodeQueue {
    pub(crate) fn accepts_held(&self,held:usize,receipt:bool)->bool {self.outstanding.len().saturating_add(held)<self.capacity+usize::from(receipt)}
    pub(crate) fn new(device:&PipelineDevice)->Result<Self,GpuRasterError> {
        #[cfg(not(target_arch="wasm32"))]
        {Self::native(device.source_samples.clone(),device.working_space())}
        #[cfg(target_arch="wasm32")]
        {let _=device;Ok(Self {external:None,completed:None,ready:Default::default(),outstanding:Vec::new(),capacity:1})}
    }
    #[cfg(not(target_arch="wasm32"))]
    fn native(cache:Arc<layer_core::raster::DecodedTileCache>,destination:RgbSpace)->Result<Self,GpuRasterError> {
        let pool=DecodePool::shared()?;
        let capacity=pool.capacity;
        let ready=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (send,results)=std::sync::mpsc::sync_channel(capacity);
        let context=Arc::new(DecodeContext {cache,destination,send,ready:ready.clone()});
        Ok(Self {pool,context,waiting:None,results,ready,outstanding:Vec::new(),capacity})
    }
    pub(crate) fn request(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2])->bool {
        let weak=Arc::downgrade(&source);
        self.enqueue(MipDecodeRequest::Image(ImageDecodeRequest {id,source,coordinate}),Some(DecodeKey::Image(weak,coordinate)))
    }
    pub(crate) fn request_raster(&mut self,blob:Arc<layer_core::raster::TileBlob>,space:RgbSpace,destination:RgbSpace,scalar:bool)->bool {
        let key=DecodeKey::Raster(Arc::downgrade(&blob),space,destination,scalar);
        self.enqueue(MipDecodeRequest::Raster(RasterDecodeRequest {blob,space,destination,scalar}),Some(key))
    }
    pub(crate) fn request_coordinates(&mut self,inverse:[f64;6],size:[u32;2],first:u32,count:u32,destination:CoordinateDestination)->bool {
        self.enqueue(MipDecodeRequest::Coordinates(CoordinateRequest {inverse,size,first,count,destination}),None)
    }
    fn enqueue(&mut self,request:MipDecodeRequest,key:Option<DecodeKey>)->bool {
        if key.as_ref().is_some_and(|key|self.outstanding.iter().flatten().any(|stored|stored.matches(key))) {return true;}
        if self.outstanding.len()>=self.capacity {return false;}
        #[cfg(not(target_arch="wasm32"))]
        {
            let epoch=self.pool.epoch.load(Ordering::Acquire);
            if self.pool.requests.try_send(DecodeJob {request,context:Arc::downgrade(&self.context)}).is_err() {self.waiting=Some(epoch);return false;}
            self.waiting=None;
        }
        #[cfg(target_arch="wasm32")]
        {self.external=Some(request);}
        self.outstanding.push(key);true
    }
    pub(crate) fn poll(&mut self)->Option<PreparedImageWork> {
        if !self.ready() {return None;}
        #[cfg(not(target_arch="wasm32"))]
        let result=self.results.try_recv().ok()?;
        #[cfg(target_arch="wasm32")]
        let result=self.completed.take()?;
        self.ready.fetch_sub(1,Ordering::AcqRel);
        let key=match &result {
            PreparedImageWork::Image(tile)=>Some(DecodeKey::Image(Arc::downgrade(&tile.source),tile.coordinate)),
            PreparedImageWork::Raster(tile)=>Some(DecodeKey::Raster(Arc::downgrade(&tile.blob),tile.space,tile.destination,tile.scalar)),
            PreparedImageWork::Coordinates {..}=>None,
        };
        let index=self.outstanding.iter().position(|stored|match (stored,&key) {
            (Some(stored),Some(key))=>stored.matches(key),
            (None,None)=>true,
            _=>false,
        });
        if let Some(index)=index {self.outstanding.swap_remove(index);}
        Some(result)
    }
    pub(crate) fn pending(&self)->bool {
        #[cfg(not(target_arch="wasm32"))]
        if self.waiting.is_some_and(|epoch|epoch==self.pool.epoch.load(Ordering::Acquire)) {return true;}
        !self.outstanding.is_empty()
    }
    pub(crate) fn ready(&self)->bool {self.ready.load(Ordering::Acquire)>0}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn take_external(&mut self)->Option<MipDecodeRequest> {self.external.take()}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn complete_external(&mut self,result:PreparedImageWork) {self.completed=Some(result);self.ready.store(1,Ordering::Release);}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{page_texture,upload_page};
    #[cfg(not(target_arch="wasm32"))]
    fn queued(requests:std::sync::mpsc::SyncSender<DecodeJob>,results:std::sync::mpsc::Receiver<PreparedImageWork>,
        ready:Arc<std::sync::atomic::AtomicUsize>,outstanding:Vec<Option<DecodeKey>>,capacity:usize)->MipDecodeQueue {
        let (send,_)=std::sync::mpsc::sync_channel(capacity);
        let context=Arc::new(DecodeContext {cache:Arc::new(layer_core::raster::DecodedTileCache::new(0)),destination:RgbSpace::Srgb,send,ready:ready.clone()});
        MipDecodeQueue {pool:Arc::new(DecodePool {requests,capacity,epoch:Default::default()}),context,waiting:None,results,ready,outstanding,capacity}
    }
    #[test]
    fn obsolete_image_decode_failure_is_visible_only_to_its_requested_source_and_context() {
        let a=layer_core::color::source::rgba8_source([1;2],|_,_|[1,2,3,255]);
        let b=layer_core::color::source::rgba8_source([1;2],|_,_|[9,8,7,255]);
        let (aid,bid)=(PortableId::random(),PortableId::random());
        let weak=Arc::downgrade(&a);let mut failures=Vec::new();
        let stale=PreparedMipTile {id:aid,source:a.clone(),coordinate:[0;2],pixels:Err("retired A decode".into())};
        assert!(!retain_image_decode(&stale,RgbSpace::Srgb,&mut failures));
        let current=PreparedMipTile {id:bid,source:b.clone(),coordinate:[0;2],pixels:Ok(crate::scene::sources::PreparedSourcePixels::NativeSamples(Arc::new(vec![9,8,7,255])))};
        assert!(retain_image_decode(&current,RgbSpace::Srgb,&mut failures));
        assert!(image_decode_failure(&failures,bid,&b,[0;2],RgbSpace::Srgb).is_none());
        assert!(image_decode_failure(&failures,aid,&b,[0;2],RgbSpace::Srgb).is_none());
        assert!(image_decode_failure(&failures,aid,&a,[0;2],RgbSpace::DisplayP3).is_none());
        assert_eq!(image_decode_failure(&failures,aid,&a,[0;2],RgbSpace::Srgb),Some("retired A decode"));
        let crate::scene::sources::PreparedSourcePixels::NativeSamples(pixels)=current.pixels.unwrap() else {panic!("Successful B retains native pixels");};
        assert_eq!(pixels.as_slice(),[9,8,7,255]);
        drop(stale);drop(a);assert_eq!(weak.strong_count(),0);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn out_of_order_source_receipts_consume_held_pixels_once_without_redispatch() {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let space=r.device.working_space();
        let a=layer_core::color::source::rgba8_source([1;2],|_,_|[255,0,0,255]);
        let b=layer_core::color::source::rgba8_source([1;2],|_,_|[0,0,255,255]);
        let aid=PortableId::random();let bid=PortableId::random();
        let pending=|r:&WgpuRasterizer,source:&Arc<SourceImage>|Arc::new(r.source_tiles.borrow_mut().plan(r,source,[0;2]).unwrap().1.unwrap());
        let (ra,rb)=(pending(&r,&a),pending(&r,&b));
        let result=|id,source:&Arc<SourceImage>|PreparedImageWork::Image(PreparedMipTile {id,source:source.clone(),coordinate:[0;2],
            pixels:prepare_image_tile(source,[0;2],space,&r.device.source_samples,None)});
        let (send,results)=std::sync::mpsc::sync_channel(2);assert!(send.send(result(bid,&b)).is_ok());assert!(send.send(result(aid,&a)).is_ok());
        let (requests,receive)=std::sync::mpsc::sync_channel(2);
        r.moving_decode=Some((space,queued(requests,results,Arc::new(std::sync::atomic::AtomicUsize::new(2)),
            vec![Some(DecodeKey::Image(Arc::downgrade(&a),[0;2])),Some(DecodeKey::Image(Arc::downgrade(&b),[0;2]))],2)));
        r.source_receipt=Some(Arc::downgrade(&ra));
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decode(&mut encoder).unwrap();assert_eq!(r.source_results.len(),1);assert!(ra.prepared().is_none());
        assert!(r.request_image_decode(bid,b.clone(),[0;2]).unwrap());assert!(receive.try_recv().is_err());
        r.drain_image_decode(&mut encoder).unwrap();assert!(ra.prepared().is_some());assert_eq!(r.source_results.len(),1);
        r.source_receipt=Some(Arc::downgrade(&rb));assert!(r.held_result_ready());assert!(!r.image_decode_waiting());
        assert!(r.request_image_decode(bid,b.clone(),[0;2]).unwrap());assert!(receive.try_recv().is_err());
        r.drain_image_decode(&mut encoder).unwrap();assert!(rb.prepared().is_some());assert!(r.source_results.is_empty());
        assert!(r.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.pending()));
        r.drain_image_decode(&mut encoder).unwrap();assert!(r.source_results.is_empty());assert!(receive.try_recv().is_err());
        let c=layer_core::color::source::rgba8_source([1;2],|_,_|[0,255,0,255]);
        assert!(r.request_image_decode(bid,c.clone(),[0;2]).unwrap());
        assert!(matches!(receive.try_recv(),Ok(DecodeJob {request:MipDecodeRequest::Image(request),..}) if Arc::ptr_eq(&request.source,&c)));
        assert!(!encoder.has_work(),"raw receipts must not publish into shared GPU cache slots");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn preparation_channel_delivery_precedes_readiness_publication() {
        let (requests,_)=std::sync::mpsc::sync_channel(1);
        let (sender,results)=std::sync::mpsc::sync_channel(1);
        let ready=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut queue=queued(requests,results,ready.clone(),vec![None],1);
        let target=Arc::new(std::sync::Mutex::new(None));
        let destination=Arc::downgrade(&target);
        assert!(sender.send(PreparedImageWork::Coordinates {destination:destination.clone(),pixels:Ok(Arc::new(vec![7]))}).is_ok());
        assert!(queue.poll().is_none());
        assert!(!queue.ready());assert!(queue.pending());
        ready.fetch_add(1,Ordering::AcqRel);
        let PreparedImageWork::Coordinates {destination:completed,pixels}=queue.poll().unwrap() else {panic!("Prepared coordinates retain their identity");};
        assert!(completed.ptr_eq(&destination));assert_eq!(pixels.unwrap().as_slice(),[7]);
        assert!(!queue.ready());assert!(!queue.pending());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn preparation_workers_bound_outstanding_tiles_deduplicate_requests_and_drop_cancelled_geometry() {
        fn drain(queue:&mut MipDecodeQueue,count:usize)->Vec<PreparedImageWork> {
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
            let mut results=Vec::new();
            while results.len()<count {
                if let Some(result)=queue.poll() {results.push(result);continue;}
                assert!(std::time::Instant::now()<deadline,"prepared work must complete");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            results
        }
        let cache=Arc::new(layer_core::raster::DecodedTileCache::new(64*1024*1024));
        let mut queue=MipDecodeQueue::native(cache.clone(),RgbSpace::Srgb).unwrap();
        let source=layer_core::color::source::rgba8_source([2048;2],|x,y|[(x%256) as u8,(y%256) as u8,0,255]);
        let id=PortableId::random();
        let tiles:Vec<[u32;2]>=(0..64).map(|index|[index%8,index/8]).collect();
        let mut accepted=Vec::new();
        for tile in &tiles {
            if !queue.request(id,source.clone(),*tile) {break;}
            assert!(queue.request(id,source.clone(),*tile),"an outstanding tile is not requested twice");
            accepted.push(*tile);
        }
        assert!(!accepted.is_empty() && accepted.len()<tiles.len(),"outstanding preparation is bounded: {}",accepted.len());
        assert_eq!(queue.outstanding.len(),accepted.len());
        let mut prepared:Vec<_>=drain(&mut queue,accepted.len()).into_iter().map(|result|{
            let PreparedImageWork::Image(tile)=result else {panic!("Image request must return image samples");};
            assert_eq!(tile.id,id);assert!(Arc::ptr_eq(&tile.source,&source));
            let crate::scene::sources::PreparedSourcePixels::NativeSamples(samples)=tile.pixels.unwrap() else {panic!("Builtin samples retain native precision");};
            assert_eq!(&samples[..4],&[0,0,0,255]);
            tile.coordinate
        }).collect();
        prepared.sort();accepted.sort();assert_eq!(prepared,accepted);
        assert!(!queue.pending());assert_eq!(cache.stats().misses,accepted.len() as u64);
        use layer_core::color::{AlphaAssociation,PixelDescriptor,SampleType,TransferEncoding};
        let hdr=PixelDescriptor {channels:4,bits_per_channel:32,sample:SampleType::Float,encoding:TransferEncoding::Linear,alpha:AlphaAssociation::Straight};
        for (descriptor,input,expected) in [
            (PixelDescriptor {bits_per_channel:16,..PixelDescriptor::COVERAGE8},vec![0x34,0x12],vec![0x34,0x12,0x34,0x12,0x34,0x12,255,255]),
            (PixelDescriptor {channels:2,..hdr},[-2f32,0.5].into_iter().flat_map(f32::to_le_bytes).collect(),[-2f32,-2.,-2.,0.5].into_iter().flat_map(f32::to_le_bytes).collect()),
            (hdr,[-1f32,2.,0.25,0.5].into_iter().flat_map(f32::to_le_bytes).collect(),[-1f32,2.,0.25,0.5].into_iter().flat_map(f32::to_le_bytes).collect()),
        ] {
            let blob=Arc::new(layer_core::raster::TileBlob::encode(descriptor,&input.repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap());
            assert!(queue.request_raster(blob.clone(),RgbSpace::Srgb,RgbSpace::DisplayP3,false));
            assert!(queue.request_raster(blob.clone(),RgbSpace::Srgb,RgbSpace::DisplayP3,false));
            assert_eq!(queue.outstanding.len(),1);
            let PreparedImageWork::Raster(tile)=drain(&mut queue,1).pop().unwrap() else {panic!("Raster preparation returns native samples");};
            assert!(Arc::ptr_eq(&tile.blob,&blob));assert_eq!(tile.space,RgbSpace::Srgb);assert_eq!(tile.destination,RgbSpace::DisplayP3);
            let crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes)=tile.pixels.unwrap() else {panic!("Raster codes retain their descriptor and alpha");};
            assert_eq!(bytes.as_slice(),expected.repeat((PAGE_SIZE*PAGE_SIZE) as usize));
            assert!(!queue.pending());
        }
        let destination=Arc::new(std::sync::Mutex::new(None));
        let weak=Arc::downgrade(&destination);
        assert!(queue.request_coordinates([1.,0.,0.,1.,3.,-2.],[256;2],17,4,weak.clone()));
        let PreparedImageWork::Coordinates {destination:completed,pixels}=drain(&mut queue,1).pop().unwrap() else {panic!("Geometry request must return geometry");};
        assert!(completed.ptr_eq(&weak));
        assert_eq!(pixels.unwrap().as_slice(),[20_i32,-2,21,-2,22,-2,23,-2].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>());
        assert!(queue.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],0,65536,weak));
        drop(destination);
        let PreparedImageWork::Coordinates {destination:completed,..}=drain(&mut queue,1).pop().unwrap() else {panic!("Cancelled geometry retains its typed result");};
        assert!(completed.upgrade().is_none(),"queued work must not keep a deleted object pose alive");
        assert!(!queue.pending());assert!(!queue.ready());
    }
    fn moving_request()->MovingRequest {
        MovingRequest {id:PortableId::random(),source:layer_core::color::source::rgba8_source([256;2],|x,y|{let value=if (x+y)%2==0 {0} else {255};[value,value,value,255]}),level:1,nearest:false}
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
        let values:Vec<_>=bytes[..32].as_chunks::<4>().0.iter().map(|bytes|f32::from_ne_bytes(*bytes)).collect();
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
        for pixel in bytes.as_chunks::<16>().0.iter() {
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
        assert!(images.lookup(unadmitted.id,&unadmitted.source,1).is_none() && images.entries.is_empty());
        assert!(!images.pending());
        encoder.submit(&r.queue);
    }
}

#[cfg(all(test,not(target_arch="wasm32")))]
#[path="object_image_pool_tests.rs"]
mod pool_tests;
