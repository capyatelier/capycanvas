use crate::{display_mips, residency::{Charge, Class, Lease, Tier}, GpuRasterError, PipelineDevice, WgpuRasterizer, PAGE_SIZE, PixelRect};
use layer_core::{authored::PortableId, color::{RgbSpace,source::SourceImage}};
use std::{collections::VecDeque, sync::{Arc,Weak,atomic::{AtomicBool,Ordering}}};

#[derive(Clone)]
pub(crate) struct MovingRequest {
    pub id: PortableId,
    pub source: Arc<SourceImage>,
    pub level: u32,
    pub nearest: bool,
    pub bounds: PixelRect,
    pub format: wgpu::TextureFormat,
    pub rank: usize,
}
#[derive(Clone)]
pub(crate) struct MovingSource {
    pub view: wgpu::TextureView,
    pub level: u32,
    pub extent: [u32;2],
    pub origin: [u32;2],
    pub samples: Option<crate::scene::sources::Samples>,
}
struct Planned { request: MovingRequest, plan: display_mips::Plan, bytes: u64, root: bool }
pub(crate) struct MipPlan { targets: Vec<Planned>, pub waiting: bool }
struct Entry {
    id: PortableId,
    source: Weak<SourceImage>,
    image: display_mips::Window,
    pending: VecDeque<[u32;2]>,
    valid: Arc<AtomicBool>,
    root: bool,
    requested: bool,
    rank: usize,
    wanted:PixelRect,
    charges:[Charge;2],
    lease:Arc<Lease>,
}
pub(crate) struct BuildBudget {root:usize,view:usize}
impl BuildBudget {
    pub(crate) fn new(steps:usize)->Self {
        let root=(steps/4).max(1).min(steps);
        Self {root,view:steps-root}
    }
    fn take(&mut self,root:bool)->bool {
        let remaining=if root {&mut self.root}else {&mut self.view};
        if *remaining==0 {return false;}*remaining-=1;true
    }
}
impl Entry {
    fn matches(&self, id: PortableId, source: &Arc<SourceImage>) -> bool { self.id == id && self.source.ptr_eq(&Arc::downgrade(source)) }
    fn matches_plan(&self, target: &Planned) -> bool {
        self.matches(target.request.id, &target.request.source) && self.root == target.root && self.image.plan == target.plan
            && self.image.texture.format() == target.request.format
    }
    fn ready(&self) -> bool { self.pending.is_empty() && self.valid.load(Ordering::Acquire) }
    fn ready_for(&self,bounds:PixelRect)->bool {
        self.valid.load(Ordering::Acquire) && !self.pending.iter().any(|coordinate|!crate::page_rect(*coordinate).intersect(bounds).is_empty())
    }
    fn storage_bytes(&self) -> u64 { self.image.storage_bytes() }
    fn encoded(&self) -> bool {
        matches!(self.image.texture.format(),wgpu::TextureFormat::Rgba8Unorm|wgpu::TextureFormat::Rg32Uint)
    }
    fn plan(&self) {
        if self.requested {self.lease.stamp(Class::Visible);}
        self.offer();
    }
    fn offer(&self) {
        if !self.ready() {self.lease.hold();return;}
        let weight=if !self.requested {0} else if self.root {1} else {2};
        self.lease.offer(self.storage_bytes(),(weight<<24)+(self.rank as u64).min((1<<24)-1));
    }
}
#[cfg(target_arch="wasm32")]
struct RetirementCompletion(Arc<AtomicBool>);
#[cfg(target_arch="wasm32")]
impl Drop for RetirementCompletion { fn drop(&mut self) { self.0.store(true,Ordering::Release); } }
#[derive(Default)]
pub(crate) struct MovingImages {
    entries: Vec<Entry>,
    #[cfg(target_arch="wasm32")]
    retired: Vec<(Entry,Arc<AtomicBool>)>,
    live: Arc<Tier>,
    building: Arc<Tier>,
    retiring: Arc<Tier>,
    evicted: Arc<AtomicBool>,
    ram: Arc<Tier>,
    scratch: Arc<Tier>,
    work: Option<(display_mips::Image,crate::residency::Charge)>,
    allocation_waiting: bool,
    context: Option<RgbSpace>,
    pipelines: Option<display_mips::WindowPipelines>,
    planned: Option<(u64,Vec<MovingRequest>)>,
    pub tile_builds: u64,
    pub completed_images: u64,
    pub evictions: u64,
}
const LEVEL_ZERO_BYTES:u64=1024*1024*1024;
fn source_order(wanted:PixelRect,plan:display_mips::Plan)->impl Fn(&[u32;2])->(bool,u64) {
    let required=wanted.expand(1<<plan.level,plan.extent);
    let centre=[wanted.min_x()+wanted.width()/2,wanted.min_y()+wanted.height()/2];
    move|coordinate|(crate::page_rect(*coordinate).intersect(required).is_empty(),
        coordinate.iter().zip(centre).map(|(value,centre)|i64::from(value*PAGE_SIZE+PAGE_SIZE/2).abs_diff(i64::from(centre)).pow(2)).sum::<u64>())
}
fn working_bytes() -> u64 {
    display_mips::Plan::at([PAGE_SIZE;2],display_mips::MAX_LEVEL).pixel_bytes_through(display_mips::MAX_LEVEL)
        +u64::from(display_mips::MAX_LEVEL)*16
}
impl MovingRequest {
    fn allocation(&self, device: &PipelineDevice, level: u32, root: bool) -> Result<Option<Planned>,GpuRasterError> {
        self.allocation_in(device,level,root,self.bounds)
    }
    fn allocation_in(&self, device: &PipelineDevice, level: u32, root: bool, bounds:PixelRect) -> Result<Option<Planned>,GpuRasterError> {
        if self.bounds.is_empty() && !root { return Ok(None); }
        let extent=self.source.extent.map(|side|side.div_ceil(PAGE_SIZE)*PAGE_SIZE);
        let bounds=if root || self.nearest {PixelRect::full(extent)} else {crate::DocRect::from(bounds.expand(1<<level,self.source.extent)).aligned(PAGE_SIZE).in_frame(extent)};
        if bounds.is_empty() {return Ok(None);}
        if [bounds.width(),bounds.height()].iter().any(|side|side.div_ceil(1<<level)>device.limits().max_texture_dimension_2d) {
            return if self.nearest {Ok(None)}else{Err(GpuRasterError::ExtentUnsupported)};
        }
        let plan=display_mips::Plan::window(extent,level,bounds);
        Ok(Some(Planned {request:self.clone(),plan,bytes:display_mips::Window::byte_bound(device,plan,self.format),root}))
    }
    fn root(&self, device:&PipelineDevice)->Result<Option<Planned>,GpuRasterError> {
        let level=(0..=display_mips::MAX_LEVEL).find(|level|self.source.extent.into_iter().all(|side|side.div_ceil(1<<level)<=PAGE_SIZE));
        if self.nearest {return Ok(None);}
        level.map_or(Ok(None),|level|self.allocation(device,level,true))
    }
}
impl MovingImages {
    pub(crate) fn within(tiers:&crate::scene::Tiers)->Self {
        Self {live:Tier::within(&tiers.moving[0]),building:Tier::within(&tiers.moving[0]),retiring:Tier::within(&tiers.moving[0]),scratch:Tier::within(&tiers.moving[0]),ram:Tier::within(&tiers.moving[1]),..Default::default()}
    }
    pub(crate) fn source_format(source:&SourceImage)->wgpu::TextureFormat {
        crate::scene::sources::Samples::for_source(source,[0;2],RgbSpace::Srgb)
            .map_or(wgpu::TextureFormat::Rgba32Float,|samples|samples.format())
    }
    pub(crate) fn format(scene: layer_core::SceneView<'_>) -> wgpu::TextureFormat {
        crate::display_precision::DisplayPrecision::for_scene(scene).format()
    }

    pub(crate) fn demand(requests: &[MovingRequest], viewport: [u32;2]) -> Result<u64,GpuRasterError> {
        if requests.iter().all(|request|request.nearest && request.bounds.is_empty()) { return Ok(0); }
        let pixels=viewport.map(u64::from).into_iter().fold(1u64,u64::saturating_mul);
        let bpp=if requests.iter().any(|request|!request.nearest && request.format==wgpu::TextureFormat::Rgba32Float) {16} else {8};
        let reduced=if requests.iter().any(|request|!request.nearest) {pixels.saturating_mul(bpp).saturating_mul(8)}else{0};
        let mut seen=std::collections::BTreeSet::new();
        let level_zero=requests.iter().filter(|request|request.nearest && !request.bounds.is_empty() && seen.insert(request.id))
            .map(|request|request.source.extent.map(|side|u64::from(side.div_ceil(PAGE_SIZE)*PAGE_SIZE)).into_iter().fold(u64::from(request.format.block_copy_size(None).unwrap()),u64::saturating_mul))
            .fold(0u64,u64::saturating_add);
        let bytes=working_bytes().saturating_add(reduced).saturating_add(level_zero);
        Ok(if level_zero>0 {bytes.min(LEVEL_ZERO_BYTES)}else{bytes})
    }
    pub(crate) fn pipelines(&mut self, device: &PipelineDevice) -> Vec<&crate::Deferred<wgpu::ComputePipeline>> {
        let pipelines=self.pipelines.get_or_insert_with(||display_mips::WindowPipelines::new(device));
        vec![&pipelines.reduce.reduce,&pipelines.half]
    }
    pub(crate) fn level(inverse:[f64;6])->u32 {
        let [a,b,c,d,_,_]=inverse;
        let major=((a+d).hypot(b-c)+(a-d).hypot(b+c))*0.5;
        let minor=(a*d-b*c).abs()/major;
        if !minor.is_finite() {return 1;}
        minor.max(1.).log2().round().clamp(1.,f64::from(display_mips::MAX_LEVEL)) as u32
    }
    pub(crate) fn work_bytes(&self)->u64 {self.scratch.used()}
    pub(crate) fn storage_bytes(&self)->u64 {self.work_bytes()+self.live.used()+self.building.used()+self.retiring.used()}
    pub(crate) fn metadata_bytes(&self)->u64 {
        self.entries.capacity() as u64*std::mem::size_of::<Entry>() as u64
            +self.work.as_ref().map_or(0,|(work,_)|work.cpu_bytes())
            +self.planned.as_ref().map_or(0,|(_,requests)|requests.capacity() as u64*std::mem::size_of::<MovingRequest>() as u64)
    }
    pub(crate) fn pinned_bytes(&self)->u64 {self.work_bytes()+self.retiring.used()+self.building.used()}
    fn settle(&self,entry:&mut Entry) {
        let tier=if entry.ready() {&self.live} else {&self.building};
        if !Arc::ptr_eq(entry.charges[0].tier(),tier) {entry.charges[0]=tier.charge(entry.storage_bytes());}
        entry.offer();
    }
    fn idle(&self)->bool {self.entries.is_empty() && self.retiring.used()==0}
    fn retire(&mut self,mut entry:Entry,encoder:&mut crate::submission::CommandEncoder) {
        self.evictions+=1;
        entry.lease.hold();
        entry.charges[0]=self.retiring.charge(entry.charges[0].bytes());
        #[cfg(not(target_arch="wasm32"))]
        encoder.retire(entry);
        #[cfg(target_arch="wasm32")]
        {
            let complete=Arc::new(AtomicBool::new(false));let callback=RetirementCompletion(complete.clone());
            encoder.hold(callback);self.retired.push((entry,complete));
        }
    }
    fn drop_retired(&mut self) {
        #[cfg(target_arch="wasm32")]
        self.retired.retain(|(_,complete)|!complete.load(Ordering::Acquire));
    }
    pub(crate) fn retire_evicted(&mut self,encoder:&mut crate::submission::CommandEncoder)->u64 {
        self.drop_retired();
        let mut bytes=0;
        if self.evicted.swap(false,Ordering::AcqRel) {
            for entry in std::mem::take(&mut self.entries) {
                if entry.lease.evicted() {bytes+=entry.storage_bytes();self.retire(entry,encoder);} else {self.entries.push(entry);}
            }
            if bytes!=0 {self.planned=None;}
        }
        if self.idle() { self.work=None; }
        bytes
    }
    pub(crate) fn pending(&self)->bool {self.allocation_waiting || self.retiring.used()!=0 || self.entries.iter().any(|entry|!entry.ready())}
    fn window(&self,device:&PipelineDevice,request:&MovingRequest,level:u32,limit:u64)->Result<Planned,GpuRasterError> {
        let tight=request.allocation(device,level,false)?.unwrap();
        if request.nearest {return Ok(tight);}
        let entries=||self.entries.iter().filter(|entry|!entry.root && entry.matches(request.id,&request.source)
            && entry.image.texture.format()==request.format);
        if let Some(entry)=entries().filter(|entry|entry.image.plan.level==level && entry.storage_bytes()<=limit
            && entry.image.plan.bounds.intersect(tight.plan.bounds)==tight.plan.bounds)
            .min_by_key(|entry|(!entry.requested,entry.storage_bytes())) {
            return Ok(Planned {plan:entry.image.plan,bytes:entry.storage_bytes(),..tight});
        }
        if entries().next().is_none() {return Ok(tight);}
        let mut margin=[request.bounds.width()/2,request.bounds.height()/2];
        while margin!=[0;2] {
            let bounds=PixelRect::new(request.bounds.min_x().saturating_sub(margin[0]),request.bounds.min_y().saturating_sub(margin[1]),
                request.bounds.max_x().saturating_add(margin[0]).min(request.source.extent[0]),
                request.bounds.max_y().saturating_add(margin[1]).min(request.source.extent[1]));
            match request.allocation_in(device,level,false,bounds) {
                Ok(Some(target)) if target.bytes<=limit=>return Ok(target),
                Ok(_) | Err(GpuRasterError::ExtentUnsupported)=>{},
                Err(error)=>return Err(error),
            }
            margin=margin.map(|side|side/2);
        }
        Ok(tight)
    }
    pub(crate) fn plan(&mut self,r:&WgpuRasterizer,requests:&[MovingRequest],budget:u64,encoder:&mut crate::submission::CommandEncoder)->Result<MipPlan,GpuRasterError> {
        self.drop_retired();
        for entry in &mut self.entries {
            if !entry.root && let Some(request)=requests.iter().find(|request|entry.matches(request.id,&request.source)) && entry.wanted!=request.bounds {
                entry.wanted=request.bounds;
                entry.pending.make_contiguous().sort_by_key(source_order(request.bounds,entry.image.plan));
            }
        }
        self.allocation_waiting=false;
        let changed=self.context.replace(r.device.working_space())!=Some(r.device.working_space());
        let root_budget=budget.saturating_sub(working_bytes());
        if !changed && self.planned.as_ref().is_some_and(|(limit,planned)|*limit==budget && planned.len()==requests.len()
            && planned.iter().zip(requests).all(|(a,b)|a.id==b.id && Arc::ptr_eq(&a.source,&b.source)
                && a.level==b.level && a.nearest==b.nearest && a.bounds.is_empty()==b.bounds.is_empty() && a.format==b.format && a.rank==b.rank
                && (b.bounds.is_empty() || self.entries.iter().any(|entry|entry.requested && entry.matches(b.id,&b.source)
                    && entry.image.texture.format()==b.format && (!b.nearest || entry.image.plan.level==0)
                    && (!entry.root || !self.entries.iter().any(|window|window.requested && !window.root && window.matches(b.id,&b.source)))
                    && entry.image.plan.bounds.intersect(b.bounds.expand(1<<entry.image.plan.level,b.source.extent))
                        ==b.bounds.expand(1<<entry.image.plan.level,b.source.extent))))) {
            self.entries.iter().for_each(Entry::plan);
            return Ok(MipPlan {targets:Vec::new(),waiting:false});
        }
        self.planned=None;
        let mut targets:Vec<Planned>=Vec::new();let mut roots=std::collections::BTreeSet::new();let mut root_bytes=0;
        for request in requests {
            let level=if request.nearest {0} else {request.level.clamp(1,display_mips::MAX_LEVEL)};
            if let Some(target)=request.allocation(&r.device,level,false)? {
                if let Some(known)=targets.iter_mut().find(|known|known.request.id==target.request.id && known.plan==target.plan && !known.root) {
                    known.request.rank=known.request.rank.max(request.rank);
                } else {targets.push(target);}
            }
            if !request.nearest && roots.insert(request.id) && let Some(mut root)=request.root(&r.device)? {
                root.request.rank=requests.iter().filter(|other|other.id==request.id && Arc::ptr_eq(&other.source,&request.source)).map(|other|other.rank).max().unwrap();
                root_bytes+=root.bytes;targets.push(root);
            }
        }
        while root_bytes>root_budget {
            let Some(index)=targets.iter().enumerate().filter(|(_,target)|target.root)
                .min_by_key(|(_,target)|(target.request.rank,std::cmp::Reverse(target.bytes))).map(|(index,_)|index) else {break;};
            let target=&targets[index];let previous=target.bytes;
            if target.plan.level==display_mips::MAX_LEVEL {targets.swap_remove(index);root_bytes-=previous;continue;}
            let next=target.request.allocation(&r.device,target.plan.level+1,true)?.unwrap();
            root_bytes=root_bytes-previous+next.bytes;targets[index]=next;
        }
        let available=budget.saturating_sub(working_bytes()).saturating_sub(root_bytes);
        let mut level_zero=targets.iter().filter(|target|target.request.nearest).map(|target|target.bytes).sum::<u64>();
        while level_zero>available {
            let index=targets.iter().enumerate().filter(|(_,target)|target.request.nearest)
                .min_by_key(|(_,target)|(target.request.rank,std::cmp::Reverse(target.bytes))).unwrap().0;
            level_zero-=targets.swap_remove(index).bytes;
        }
        let target_budget=available.saturating_sub(level_zero)/2;
        for target in targets.iter_mut().filter(|target|!target.root && !target.request.nearest) {
            *target=self.window(&r.device,&target.request,target.plan.level,target_budget)?;
        }
        let mut target_bytes=targets.iter().filter(|target|!target.root && !target.request.nearest).map(|target|target.bytes).sum::<u64>();
        while target_bytes>target_budget {
            let Some(index)=targets.iter().enumerate().filter(|(_,target)|!target.root && !target.request.nearest)
                .min_by_key(|(_,target)|(target.request.rank,std::cmp::Reverse(target.bytes))).map(|(index,_)|index) else {break;};
            let target=&targets[index];let previous=target.bytes;
            if target.request.nearest || target.plan.level==display_mips::MAX_LEVEL {targets.swap_remove(index);target_bytes-=previous;continue;}
            let next=self.window(&r.device,&target.request,target.plan.level+1,target_budget)?;
            target_bytes=target_bytes-previous+next.bytes;targets[index]=next;
        }
        targets.sort_by_key(|target|(target.request.id,Arc::as_ptr(&target.request.source) as usize,target.root,target.plan.level,
            target.plan.bounds.min_x(),target.plan.bounds.min_y(),target.plan.bounds.max_x(),target.plan.bounds.max_y()));
        targets.dedup_by(|a,b| {
            let same=a.request.id==b.request.id && Arc::ptr_eq(&a.request.source,&b.request.source)
                && a.root==b.root && a.plan==b.plan && a.request.format==b.request.format;
            if same {b.request.rank=b.request.rank.max(a.request.rank);}
            same
        });
        let entries=std::mem::take(&mut self.entries);
        let ready:Vec<_>=targets.iter().map(|target|entries.iter().any(|entry|entry.ready() && entry.matches_plan(target))).collect();
        for mut entry in entries {
            entry.requested=targets.iter().any(|target|entry.matches_plan(target));
            entry.rank=requests.iter().filter(|request|entry.matches(request.id,&request.source)).map(|request|request.rank).max().unwrap_or(0);
            let requested=targets.iter().enumerate().any(|(index,target)|entry.matches_plan(target)
                || (!ready[index] && !entry.root && !target.root && entry.matches(target.request.id,&target.request.source)
                    && entry.image.texture.format()==target.request.format));
            if !changed && requested {entry.plan();self.entries.push(entry);} else {self.retire(entry,encoder);}
        }
        targets.retain(|target|!self.entries.iter().any(|entry|entry.matches_plan(target)));
        if targets.is_empty() && self.idle() {self.work=None;}
        let bytes=self.storage_bytes()+targets.iter().map(|target|target.bytes).sum::<u64>()
            +if self.work.is_none() && !targets.is_empty() {working_bytes()} else {0};
        if bytes>budget {
            let entries=std::mem::take(&mut self.entries);
            for entry in entries {
                if entry.requested {self.entries.push(entry);} else {self.retire(entry,encoder);}
            }
            self.allocation_waiting=self.retiring.used()!=0;
            return Ok(MipPlan {targets:Vec::new(),waiting:self.allocation_waiting});
        }
        if targets.is_empty() {self.planned=Some((budget,requests.to_vec()));}
        Ok(MipPlan {targets,waiting:false})
    }
    pub(crate) fn allocate(&mut self,r:&WgpuRasterizer,plan:MipPlan,encoder:&mut crate::submission::CommandEncoder)->bool {
        if plan.targets.is_empty() {return false;}
        self.pipelines(&r.device);
        let Ok(windows)=plan.targets.iter().map(|target|r.admit_tier(None,&self.building,Class::Visible,target.bytes)).collect::<Result<Vec<_>,_>>()
            else {self.allocation_waiting=true;return false;};
        let (work,charge)=match self.work.take() {
            Some(work)=>work,
            None=>{
                let Ok(reserved)=r.admit_tier(None,&self.scratch,Class::Visible,working_bytes()) else {self.allocation_waiting=true;return false;};
                let work=display_mips::Image::with_device(&r.device,display_mips::Plan::at([PAGE_SIZE;2],display_mips::MAX_LEVEL),display_mips::MAX_LEVEL);
                let charge=reserved.charge(work.storage_bytes());
                (work,charge)
            },
        };
        let mut replacements=Vec::new();let mut copied:Vec<(PortableId,Weak<SourceImage>)>=Vec::new();
        for (target,reserved) in plan.targets.into_iter().zip(windows) {
            let mut image=display_mips::Window::new(&r.device,target.plan,target.request.format,&work,self.pipelines.as_ref().unwrap());
            if image.texture.format()==wgpu::TextureFormat::Rgba8Unorm
                && crate::scene::sources::Samples::for_source(&target.request.source,[0;2],r.device.working_space()).is_some_and(|samples|samples.srgb_view()) {
                image.view=image.texture.create_view(&wgpu::TextureViewDescriptor {format:Some(wgpu::TextureFormat::Rgba8UnormSrgb),usage:Some(wgpu::TextureUsages::TEXTURE_BINDING),..Default::default()});
            }
            let mut pending:Vec<_>=crate::page_coordinates(target.plan.bounds).collect();
            if target.root || target.plan.bounds!=PixelRect::full(target.plan.extent) {
                pending.sort_by_key(source_order(target.request.bounds.intersect(target.plan.bounds),target.plan));
            }
            let valid=Arc::new(AtomicBool::new(true));
            for previous in self.entries.iter().filter(|entry|entry.valid.load(Ordering::Acquire) && entry.root==target.root
                && entry.matches(target.request.id,&target.request.source) && entry.image.plan.level==target.plan.level
                && entry.image.texture.format()==target.request.format) {
                let overlap=previous.image.plan.bounds.intersect(target.plan.bounds);
                for coordinate in crate::page_coordinates(overlap).filter(|coordinate|!previous.pending.contains(coordinate)) {
                    if !pending.contains(&coordinate) {continue;}
                    let page=crate::page_rect(coordinate).intersect(overlap);let scale=1<<target.plan.level;
                    encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {
                        x:(page.min_x()-previous.image.plan.bounds.min_x())/scale,
                        y:(page.min_y()-previous.image.plan.bounds.min_y())/scale,z:0,
                    },..previous.image.texture.as_image_copy()},wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {
                        x:(page.min_x()-target.plan.bounds.min_x())/scale,
                        y:(page.min_y()-target.plan.bounds.min_y())/scale,z:0,
                    },..image.texture.as_image_copy()},wgpu::Extent3d {
                        width:page.width().div_ceil(scale),height:page.height().div_ceil(scale),depth_or_array_layers:1,
                    });
                    pending.retain(|&pending|pending!=coordinate);
                    crate::submission::CacheWrite::shared(valid.clone()).track(encoder);
                }
            }
            let source=Arc::downgrade(&target.request.source);
            if pending.is_empty() && !copied.iter().any(|(id,known)|*id==target.request.id && known.ptr_eq(&source)) {copied.push((target.request.id,source.clone()));}
            let building=reserved.charge(image.storage_bytes());
            let window=if pending.is_empty() {self.live.charge(building.bytes())} else {building};
            let charges=[window,self.ram.charge(pending.capacity() as u64*std::mem::size_of::<[u32;2]>() as u64)];
            replacements.push(Entry {id:target.request.id,source,image,pending:pending.into(),valid,root:target.root,requested:true,rank:target.request.rank,wanted:target.request.bounds,charges,lease:self.live.heap().lease(&self.evicted)});
        }
        let entries=std::mem::take(&mut self.entries);
        for entry in entries {
            if !entry.ready() && !entry.requested {self.retire(entry,encoder);} else {self.entries.push(entry);}
        }
        replacements.iter().for_each(Entry::plan);
        self.entries.extend(replacements);
        self.work=Some((work,charge));
        let changed=!copied.is_empty();
        for (id,source) in copied {self.record_completed_image(id,&source);}
        changed
    }
    fn record_completed_image(&mut self,id:PortableId,source:&Weak<SourceImage>) {
        if self.entries.iter().filter(|entry|entry.requested && entry.id==id && entry.source.ptr_eq(source)).all(|entry|entry.pending.is_empty()) {self.completed_images+=1;}
    }
    pub(crate) fn lookup(&self,id:PortableId,source:&Arc<SourceImage>,requested:u32,bounds:PixelRect,format:wgpu::TextureFormat,context:RgbSpace)->Option<MovingSource> {
        if self.context!=Some(context) {return None;}
        let candidates=||self.entries.iter().rev().filter(|entry|entry.matches(id,source)
            && entry.image.texture.format()==format
            && entry.ready_for(bounds.expand(1<<entry.image.plan.level,source.extent))
            && bounds.expand(1<<entry.image.plan.level,source.extent).intersect(entry.image.plan.bounds)==bounds.expand(1<<entry.image.plan.level,source.extent));
        let entry=candidates().filter(|entry|entry.image.plan.level<=requested && entry.image.plan.level+1>=requested)
            .max_by_key(|entry|entry.image.plan.level)
            .or_else(||candidates().filter(|entry|!entry.root && entry.image.plan.level>requested).min_by_key(|entry|entry.image.plan.level))
            .or_else(||candidates().filter(|entry|entry.root).max_by_key(|entry|entry.image.plan.level))?;
        let samples=entry.encoded().then(||crate::scene::sources::Samples::for_source(source,[0;2],context)).flatten();
        let view=entry.image.view.clone();
        let origin=[entry.image.plan.bounds.min_x(),entry.image.plan.bounds.min_y()];
        let scale=1<<entry.image.plan.level;
        let extent=std::array::from_fn(|axis|entry.image.plan.size[axis].min(source.extent[axis].saturating_sub(origin[axis]).div_ceil(scale)));
        Some(MovingSource {view,level:entry.image.plan.level,extent,origin,samples})
    }
    fn order(&self)->impl Iterator<Item=&Entry> {
        let mut entries=self.entries.iter().filter(|entry|entry.valid.load(Ordering::Acquire) && !entry.pending.is_empty() && entry.source.strong_count()!=0).collect::<Vec<_>>();
        entries.sort_by_key(|entry|(!entry.root,std::cmp::Reverse(entry.rank)));
        entries.into_iter()
    }
    fn give_unused(&self,budget:&mut BuildBudget) {
        let mut root=false;let mut view=false;
        for entry in self.order() {if entry.root {root=true;}else {view=true;}}
        if !root {budget.view+=budget.root;budget.root=0;}
        if !view {budget.root+=budget.view;budget.view=0;}
    }
    pub(crate) fn next_build(&mut self,budget:&mut BuildBudget,ready:impl Fn(PortableId,&Arc<SourceImage>,[u32;2],Option<wgpu::TextureFormat>)->bool)->Option<(PortableId,Arc<SourceImage>,[u32;2])> {
        for entry in &mut self.entries {
            if !entry.valid.load(Ordering::Acquire) {
                entry.pending=crate::page_coordinates(entry.image.plan.bounds).collect();entry.valid=Arc::new(AtomicBool::new(true));
                entry.charges[0]=self.building.charge(entry.storage_bytes());entry.offer();
            }
        }
        self.give_unused(budget);
        self.order().find_map(|entry| {
            let source=entry.source.upgrade()?;let coordinate=*entry.pending.front()?;
            if ready(entry.id,&source,coordinate,entry.encoded().then(||entry.image.texture.format())) && budget.take(entry.root) {
                Some((entry.id,source,coordinate))
            } else {None}
        })
    }
    pub(crate) fn accept_build(&self,budget:&mut BuildBudget,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.give_unused(budget);
        self.order().any(|entry|entry.matches(id,source) && entry.pending.front()==Some(&coordinate) && budget.take(entry.root))
    }
    pub(crate) fn upcoming(&self,count:usize)->Vec<(PortableId,Arc<SourceImage>,[u32;2])> {
        let mut budget=BuildBudget::new(count);self.give_unused(&mut budget);
        let mut upcoming=Vec::new();
        for entry in self.order() {
            let Some(source)=entry.source.upgrade() else {continue;};
            let available=if entry.root {budget.root}else {budget.view};
            for &coordinate in entry.pending.iter().take(available) {
                budget.take(entry.root);upcoming.push((entry.id,source.clone(),coordinate));
            }
            if upcoming.len()==count {break;}
        }
        upcoming
    }
    pub(crate) fn accepts_tile(&self,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.entries.iter().any(|entry|entry.matches(id,source) && entry.valid.load(Ordering::Acquire) && entry.pending.front()==Some(&coordinate))
    }
    pub(crate) fn needs_samples(&self,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],texture:wgpu::TextureFormat)->bool {
        self.entries.iter().any(|entry|entry.matches(id,source) && entry.pending.contains(&coordinate)
            && entry.encoded() && entry.image.texture.format()!=texture)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_tile(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],texture:impl Into<crate::source_access::TileTexture>,pixels:Option<&crate::scene::sources::PreparedSourcePixels>)->Result<bool,GpuRasterError> {
        let texture=&texture.into();
        if !self.accepts_tile(id,source,coordinate) {return Err(GpuRasterError::InvalidImage);}
        let windows=||self.entries.iter().filter(|entry|entry.matches(id,source) && entry.pending.contains(&coordinate));
        let level=windows().filter(|entry|!entry.encoded())
            .map(|entry|entry.image.plan.level).max();
        for entry in windows().filter(|entry|entry.encoded()) {
            let origin=coordinate.map(|value|value*PAGE_SIZE);
            let destination=[origin[0]-entry.image.plan.bounds.min_x(),origin[1]-entry.image.plan.bounds.min_y()];
            let extent=wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1};
            if entry.image.texture.format()==texture.format() {
                encoder.copy_texture_to_texture(texture.as_image_copy(),wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:destination[0],y:destination[1],z:0},..entry.image.texture.as_image_copy()},extent);
            } else {
                let Some(crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes))=pixels else {return Err(GpuRasterError::NotReady);};
                let row=PAGE_SIZE*entry.image.texture.format().block_copy_size(None).unwrap();
                if bytes.len()!=row as usize*PAGE_SIZE as usize {return Err(GpuRasterError::InvalidImage);}
                let reservation=r.reserve_upload(Class::Visible,bytes.len() as u64).ok_or(GpuRasterError::NotReady)?;
                r.uploads.write_texture_region(encoder,&entry.image.texture,wgpu::Origin3d {x:destination[0],y:destination[1],z:0},extent,row,|mapped|mapped.copy_from_slice(bytes))?;
                r.charge_upload(encoder,reservation,bytes.len() as u64);
            }
        }
        if let Some(level)=level {
            self.pipelines.as_ref().unwrap().prepare_tile(r,encoder,&mut self.work.as_mut().unwrap().0,texture,source,coordinate,level)?;
            display_mips::Window::copy_tiles(self.pipelines.as_ref().unwrap(),encoder,&self.work.as_ref().unwrap().0,coordinate,
                windows().filter(|entry|!entry.encoded()).map(|entry|&entry.image))?;
        }
        let mut changed=false;
        let mut entries=std::mem::take(&mut self.entries);
        for entry in entries.iter_mut().filter(|entry|entry.matches(id,source) && entry.pending.contains(&coordinate)) {
            crate::submission::CacheWrite::shared(entry.valid.clone()).track(encoder);
            let position=entry.pending.iter().position(|pending|*pending==coordinate).unwrap();
            entry.pending.remove(position);
            if entry.pending.is_empty() {changed=true;self.planned=None;self.settle(entry);}
        }
        self.entries=entries;
        if changed {self.record_completed_image(id,&Arc::downgrade(source));}
        self.tile_builds+=1;
        Ok(changed)
    }
}

pub(crate) struct ImageDecodeRequest {
    pub class:Class,
    pub use_:crate::scene::sources::SourceUse,
    pub id:PortableId,
    pub source:Arc<SourceImage>,
    pub coordinate:[u32;2],
}
pub(crate) struct RasterDecodeRequest {
    pub class:Class,
    pub blob:Arc<layer_core::raster::TileBlob>,
    pub space:RgbSpace,
    pub destination:RgbSpace,
    pub scalar:bool,
}
pub(crate) struct PreparedRasterTile {
    pub class:Class,
    pub blob:Arc<layer_core::raster::TileBlob>,
    pub space:RgbSpace,
    pub destination:RgbSpace,
    pub scalar:bool,
    pub pixels:Result<crate::scene::sources::PreparedSourcePixels,String>,
}
pub fn prepare_raster_tile(blob:&Arc<layer_core::raster::TileBlob>,space:RgbSpace,cache:&layer_core::raster::DecodedTileCache)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    prepare_raster_tile_with(blob,space,|| {
        #[cfg(not(target_arch="wasm32"))]
        {cache.decode_on_worker(blob)}
        #[cfg(target_arch="wasm32")]
        {cache.decode_bytes_on_worker(blob,&blob.memory_compressed()?.ok_or("Raster worker input is not prepared")?)}
    })
}
pub fn prepare_raster_tile_from_bytes(blob:&Arc<layer_core::raster::TileBlob>,space:RgbSpace,cache:&layer_core::raster::DecodedTileCache,bytes:&[u8])->Result<crate::scene::sources::PreparedSourcePixels,String> {
    prepare_raster_tile_with(blob,space,||cache.decode_bytes_on_worker(blob,bytes))
}
fn prepare_raster_tile_with(blob:&Arc<layer_core::raster::TileBlob>,space:RgbSpace,decode:impl FnOnce()->Result<Arc<Vec<u8>>,String>)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    use layer_core::color::source::SourceChannels;
    crate::scene::sources::validate_raster(blob,space).map_err(|error|error.to_string())?;
    let native=decode()?;
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
pub(crate) struct CoordinateSlot {pub(crate) result:Option<CoordinateResult>,charge:crate::residency::Charge}
impl CoordinateSlot {
    pub(crate) fn new(tier:&Arc<crate::residency::Tier>)->Self {Self {result:None,charge:tier.charge(std::mem::size_of::<Option<CoordinateResult>>() as u64)}}
    pub(crate) fn write(&mut self,result:CoordinateResult) {
        self.charge.resize((std::mem::size_of::<Option<CoordinateResult>>()+result.as_ref().map_or(0,|bytes|bytes.capacity())) as u64);
        self.result=Some(result);
    }
}
pub(crate) type CoordinateDestination = Weak<std::sync::Mutex<CoordinateSlot>>;
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
    Paper {blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2]},
    Coordinates(CoordinateRequest),
}
pub(crate) enum PreparedImageWork {
    Image(PreparedMipTile),
    Raster(PreparedRasterTile),
    Paper {blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2],pixels:Result<Arc<Vec<u8>>,String>},
    Coordinates {destination:CoordinateDestination,pixels:CoordinateResult},
}
impl PreparedImageWork {
    pub(crate) fn class(&self)->Class {
        match self {Self::Image(tile)=>tile.class,Self::Raster(tile)=>tile.class,Self::Paper {..}|Self::Coordinates {..}=>Class::Visible}
    }
    pub(crate) fn raise(&mut self,class:Class) {
        match self {Self::Image(tile)=>tile.class=tile.class.min(class),Self::Raster(tile)=>tile.class=tile.class.min(class),Self::Paper {..}|Self::Coordinates {..}=>{}}
    }
}
pub fn prepare_image_tile(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace,cache:&layer_core::raster::DecodedTileCache,decoder:Option<&layer_color::WorkingDecoder>)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    prepare_image_tile_with(source,coordinate,destination,decoder,|tile| {
        #[cfg(not(target_arch="wasm32"))]
        {cache.decode_on_worker(tile)}
        #[cfg(target_arch="wasm32")]
        {cache.decode_bytes_on_worker(tile,&tile.memory_compressed()?.ok_or("Image worker input is not prepared")?)}
    })
}
pub fn prepare_image_tile_from_bytes(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace,cache:&layer_core::raster::DecodedTileCache,decoder:Option<&layer_color::WorkingDecoder>,bytes:&[u8])->Result<crate::scene::sources::PreparedSourcePixels,String> {
    prepare_image_tile_with(source,coordinate,destination,decoder,|tile|cache.decode_bytes_on_worker(tile,bytes))
}
fn prepare_image_tile_with(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace,decoder:Option<&layer_color::WorkingDecoder>,decode:impl FnOnce(&Arc<layer_core::raster::TileBlob>)->Result<Arc<Vec<u8>>,String>)->Result<crate::scene::sources::PreparedSourcePixels,String> {
    use crate::scene::sources::PreparedSourcePixels;
    if matches!(source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) && source.interpretation.channels!=layer_core::color::source::SourceChannels::Cmyk {
        let tile=source.tiles.get(&coordinate).ok_or("Missing immutable image tile")?;
        let native=decode(tile)?;
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
    decoder.decode_tile_with(source,coordinate,&mut pixels,decode)?;
    let bytes=pixels.into_iter().flat_map(|[r,g,b,a]|[r*a,g*a,b*a,a]).flat_map(f32::to_ne_bytes).collect();
    Ok(PreparedSourcePixels::PremultipliedWorkingPixels(Arc::new(bytes)))
}
#[derive(Clone)]
pub(crate) struct PreparedMipTile {
    pub class:Class,
    pub use_:crate::scene::sources::SourceUse,
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
    Paper(Weak<layer_core::raster::TileBlob>,[u32;2]),
}
impl DecodeKey {
    fn matches(&self,other:&Self)->bool {
        match (self,other) {
            (Self::Image(a,p),Self::Image(b,q))=>a.ptr_eq(b) && p==q,
            (Self::Raster(a,space,destination,scalar),Self::Raster(b,other,target,coverage))=>a.ptr_eq(b) && space==other && destination==target && scalar==coverage,
            (Self::Paper(a,p),Self::Paper(b,q))=>a.ptr_eq(b) && p==q,
            _=>false,
        }
    }
}
struct Outstanding {
    sequence:u64,key:Option<DecodeKey>,region:PixelRect,class:Class,
    #[cfg(not(target_arch="wasm32"))]
    input:Arc<DecodeInput>,
}
#[cfg(not(target_arch="wasm32"))]
struct DecodeInput {samples:Option<Arc<Vec<u8>>>,read:Result<Option<layer_core::tile_io::ReadWait>,String>}
pub(crate) struct MipDecodeQueue {
    #[cfg(not(target_arch="wasm32"))]
    pool:Arc<DecodePool>,
    #[cfg(not(target_arch="wasm32"))]
    context:Arc<DecodeContext>,
    #[cfg(not(target_arch="wasm32"))]
    waiting:Option<u64>,
    #[cfg(not(target_arch="wasm32"))]
    results:std::sync::mpsc::Receiver<(u64,PreparedImageWork)>,
    #[cfg(target_arch="wasm32")]
    external:Option<(u64,MipDecodeRequest)>,
    #[cfg(target_arch="wasm32")]
    completed:Option<(u64,PreparedImageWork)>,
    ready:Arc<std::sync::atomic::AtomicUsize>,
    outstanding:Vec<Outstanding>,
    next_sequence:u64,
    capacity:usize,
    priority:Option<(layer_render::ViewState,PixelRect)>,
}
#[cfg(not(target_arch="wasm32"))]
struct DecodeContext {
    cache:Arc<layer_core::raster::DecodedTileCache>,destination:RgbSpace,
    send:std::sync::mpsc::SyncSender<(u64,PreparedImageWork)>,ready:Arc<std::sync::atomic::AtomicUsize>,
    wake: std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    waiter: std::sync::Mutex<Option<std::task::Waker>>,
}
#[cfg(not(target_arch="wasm32"))]
struct DecodeJob {sequence:u64,request:MipDecodeRequest,context:Weak<DecodeContext>,input:Weak<DecodeInput>}
#[cfg(not(target_arch="wasm32"))]
struct DecodePool {requests:std::sync::mpsc::SyncSender<DecodeJob>,capacity:usize,epoch:Arc<std::sync::atomic::AtomicU64>,
    capacity_waiters:Arc<std::sync::Mutex<Vec<Weak<DecodeContext>>>>,resumed:std::sync::Mutex<VecDeque<DecodeJob>>}
#[cfg(not(target_arch="wasm32"))]
struct ParkedDecode {job:std::sync::Mutex<Option<DecodeJob>>,pool:Weak<DecodePool>}
#[cfg(not(target_arch="wasm32"))]
impl std::task::Wake for ParkedDecode {
    fn wake(self:Arc<Self>) {
        let job=self.job.lock().unwrap().take();
        if let Some(job)=job && let Some(pool)=self.pool.upgrade() {pool.resume(job);}
    }
}
#[cfg(not(target_arch="wasm32"))]
impl DecodeContext {
    fn notify(&self) {
        let wake=self.wake.lock().unwrap().clone();
        let waiter=self.waiter.lock().unwrap().take();
        if let Some(wake)=wake {wake();}
        if let Some(waiter)=waiter {waiter.wake();}
    }
}
#[cfg(not(target_arch="wasm32"))]
type Decoders=std::collections::VecDeque<(Weak<SourceImage>,RgbSpace,layer_color::WorkingDecoder)>;
#[cfg(not(target_arch="wasm32"))]
impl DecodeJob {
    fn run(self,decoders:&mut Decoders,pool:&Weak<DecodePool>) {
        let Some(context)=self.context.upgrade() else {return;};
        let (cache,destination)=(context.cache.clone(),context.destination);drop(context);
        let Some(input)=self.input.upgrade() else {return;};
        let (job,bytes)=match &input.read {
            Ok(Some(read))=>{
                let parked=Arc::new(ParkedDecode {job:std::sync::Mutex::new(Some(self)),pool:pool.clone()});
                let std::task::Poll::Ready(bytes)=read.poll_bytes(&std::task::Waker::from(parked.clone())) else {return;};
                let Some(job)=parked.job.lock().unwrap().take() else {return;};
                (job,bytes.map(Some))
            },
            Ok(None)=>(self,Ok(None)),
            Err(error)=>(self,Err(error.clone())),
        };
        let result=prepare_request(job.request,destination,&cache,decoders,input.samples.clone(),bytes);
        if let Some(context)=job.context.upgrade() && context.send.send((job.sequence,result)).is_ok() {
            context.ready.fetch_add(1,Ordering::AcqRel);
            context.notify();
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
    fn subscribe(&self,context:&Arc<DecodeContext>,epoch:u64) {
        let changed={
            let mut waiters=self.capacity_waiters.lock().unwrap();
            if self.epoch.load(Ordering::Acquire)!=epoch {true}else {
                waiters.retain(|waiting|waiting.strong_count()!=0);
                let waiting=Arc::downgrade(context);
                if !waiters.iter().any(|stored|stored.ptr_eq(&waiting)) {waiters.push(waiting);}
                false
            }
        };
        if changed {context.notify();}
    }
    fn start(workers:usize)->std::io::Result<Arc<Self>> {
        let capacity=workers*6;
        let (requests,receive)=std::sync::mpsc::sync_channel::<DecodeJob>(capacity);
        let epoch=Arc::new(std::sync::atomic::AtomicU64::new(0));
        let receive=Arc::new(std::sync::Mutex::new(receive));
        let capacity_waiters=Arc::new(std::sync::Mutex::new(Vec::<Weak<DecodeContext>>::new()));
        let pool=Arc::new(Self {requests,capacity,epoch:epoch.clone(),capacity_waiters:capacity_waiters.clone(),resumed:Default::default()});
        for worker in 0..workers {
            let (receive,epoch,waiters,pool)=(receive.clone(),epoch.clone(),capacity_waiters.clone(),Arc::downgrade(&pool));
            std::thread::Builder::new().name(format!("capy-image-prefilter-{worker}")).spawn(move || {
                let mut decoders=Decoders::new();
                loop {
                    let resumed=pool.upgrade().and_then(|pool|pool.resumed.lock().unwrap().pop_front());
                    let job=if let Some(job)=resumed {job} else {
                        let Ok(job)=receive.lock().unwrap().recv() else {break;};
                        let subscribers={
                            let mut waiters=waiters.lock().unwrap();
                            epoch.fetch_add(1,Ordering::Release);
                            std::mem::take(&mut *waiters)
                        };
                        for context in subscribers.into_iter().filter_map(|context|context.upgrade()) {context.notify();}
                        job
                    };
                    job.run(&mut decoders,&pool);
                }
            })?;
        }
        Ok(pool)
    }
    fn resume(&self,job:DecodeJob) {
        if let Err(std::sync::mpsc::TrySendError::Full(job))=self.requests.try_send(job) {self.resumed.lock().unwrap().push_back(job);}
    }
}
#[cfg(not(target_arch="wasm32"))]
fn contained<T,E:From<String>>(work:impl FnOnce()->Result<T,E>)->Result<T,E> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|payload| Err(crate::panic_message(&*payload).into()))
}
#[cfg(not(target_arch="wasm32"))]
fn prepare_request(request:MipDecodeRequest,destination:RgbSpace,cache:&layer_core::raster::DecodedTileCache,decoders:&mut Decoders,samples:Option<Arc<Vec<u8>>>,input:Result<Option<layer_core::package::ByteRange>,String>)->PreparedImageWork {
    let decode=|blob:&Arc<layer_core::raster::TileBlob>| {
        if let Some(samples)=&samples {return Ok(samples.clone());}
        match input.as_ref().map_err(Clone::clone)? {Some(bytes)=>cache.decode_bytes_on_worker(blob,bytes),None=>cache.decode_on_worker(blob)}
    };
    match request {
        MipDecodeRequest::Paper {blob,coordinate}=> {
            let pixels=contained(||decode(&blob));
            PreparedImageWork::Paper {blob,coordinate,pixels}
        },
        MipDecodeRequest::Raster(request) => {
            let pixels=contained(||prepare_raster_tile_with(&request.blob,request.space,||decode(&request.blob)));
            PreparedImageWork::Raster(PreparedRasterTile {class:request.class,blob:request.blob,space:request.space,destination:request.destination,scalar:request.scalar,pixels})
        },
        MipDecodeRequest::Coordinates(request) => {
            let pixels=contained(|| if request.destination.strong_count()==0 {Err("Coordinate request cancelled".into())}
                else {crate::object_sampling::prepare_nearest_coordinates(request.inverse,request.size,request.first,request.count)});
            PreparedImageWork::Coordinates {destination:request.destination,pixels}
        },
        MipDecodeRequest::Image(request) => {
            let pixels=contained(|| {
                if matches!(request.source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) {
                    return prepare_image_tile_with(&request.source,request.coordinate,destination,None,decode);
                }
                let weak=Arc::downgrade(&request.source);
                let index=if let Some(index)=decoders.iter().position(|(source,space,_)|source.ptr_eq(&weak) && *space==destination) {index} else {
                    let decoder=layer_color::WorkingDecoder::new(&request.source.interpretation,destination,Default::default())?;
                    if decoders.len()==4 {decoders.pop_front();}
                    decoders.push_back((weak,destination,decoder));decoders.len()-1
                };
                let decoder=decoders.remove(index).unwrap();
                let decoded=prepare_image_tile_with(&request.source,request.coordinate,destination,Some(&decoder.2),decode);
                decoders.push_back(decoder);
                decoded
            });
            PreparedImageWork::Image(PreparedMipTile {class:request.class,use_:request.use_,id:request.id,source:request.source,coordinate:request.coordinate,pixels})
        },
    }
}
impl MipDecodeQueue {
    pub(crate) fn prioritize(&mut self,view:layer_render::ViewState,region:PixelRect) {
        #[cfg(not(target_arch="wasm32"))]
        if self.priority.is_none_or(|(previous,_)|previous!=view) {
            for request in &self.outstanding {
                if !request.region.is_empty() && let Ok(Some(read))=&request.input.read {read.prioritize(Self::distance(view,request.region));}
            }
        }
        self.priority=Some((view,region));
    }
    fn distance(view:layer_render::ViewState,region:PixelRect)->u64 {
        let [a,b,c,d,tx,ty]=view.document_to_surface.map(f64::from);
        let x=f64::from(region.min_x()+region.max_x())*0.5;let y=f64::from(region.min_y()+region.max_y())*0.5;
        let dx=a*x+c*y+tx-f64::from(view.width_px)*0.5;let dy=b*x+d*y+ty-f64::from(view.height_px)*0.5;
        (dx*dx+dy*dy).min(u64::MAX as f64) as u64
    }
    #[cfg(test)]
    pub(crate) fn requested_image_tiles(&self)->Vec<(usize,[u32;2])> {
        let mut tiles:Vec<_>=self.outstanding.iter().filter_map(|request|request.key.as_ref()).filter_map(|key|match key {
            DecodeKey::Image(source,coordinate)=>Some((source.as_ptr() as usize,*coordinate)),_=>None,
        }).collect();
        tiles.sort();tiles
    }
    pub(crate) fn accepts_held(&self,held:usize,receipt:bool)->bool {self.outstanding.len().saturating_add(held)<self.capacity+usize::from(receipt)}
    pub(crate) fn new(device:&PipelineDevice)->Result<Self,GpuRasterError> {
        #[cfg(not(target_arch="wasm32"))]
        {Self::native(device.source_samples.clone(),device.working_space())}
        #[cfg(target_arch="wasm32")]
        {let _=device;Ok(Self {external:None,completed:None,ready:Default::default(),outstanding:Vec::new(),next_sequence:0,capacity:1,priority:None})}
    }
    #[cfg(not(target_arch="wasm32"))]
    fn native(cache:Arc<layer_core::raster::DecodedTileCache>,destination:RgbSpace)->Result<Self,GpuRasterError> {
        let pool=DecodePool::shared()?;
        let capacity=pool.capacity;
        let ready=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (send,results)=std::sync::mpsc::sync_channel(capacity);
        let context=Arc::new(DecodeContext {cache,destination,send,ready:ready.clone(),wake:Default::default(),waiter:Default::default()});
        Ok(Self {pool,context,waiting:None,results,ready,outstanding:Vec::new(),next_sequence:0,capacity,priority:None})
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.context.wake.lock().unwrap()=Some(wake.clone());
        if self.ready() || self.capacity_changed() {wake();}
    }
    #[cfg(not(target_arch="wasm32"))]
    fn capacity_changed(&self)->bool {self.waiting.is_some_and(|epoch|epoch!=self.pool.epoch.load(Ordering::Acquire))}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn register_waker(&self,waker:&std::task::Waker) {
        let previous=self.context.waiter.lock().unwrap().replace(waker.clone());
        drop(previous);
        if self.ready() || self.capacity_changed() {
            let waiter=self.context.waiter.lock().unwrap().take();
            if let Some(waiter)=waiter {waiter.wake();}
        }
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn clear_waker(&self) {let waiter=self.context.waiter.lock().unwrap().take();drop(waiter);}
    #[cfg(test)]
    pub(crate) fn request(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.request_for(id,source,coordinate,crate::scene::sources::SourceUse::Native,Class::Visible)
    }
    pub(crate) fn request_for(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2],use_:crate::scene::sources::SourceUse,class:Class)->bool {
        let weak=Arc::downgrade(&source);
        self.enqueue(MipDecodeRequest::Image(ImageDecodeRequest {class,use_,id,source,coordinate}),Some(DecodeKey::Image(weak,coordinate)),class)
    }
    pub(crate) fn request_raster(&mut self,blob:Arc<layer_core::raster::TileBlob>,space:RgbSpace,destination:RgbSpace,scalar:bool,class:Class)->bool {
        let key=DecodeKey::Raster(Arc::downgrade(&blob),space,destination,scalar);
        self.enqueue(MipDecodeRequest::Raster(RasterDecodeRequest {class,blob,space,destination,scalar}),Some(key),class)
    }
    pub(crate) fn request_paper(&mut self,blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2])->bool {
        let key=DecodeKey::Paper(Arc::downgrade(&blob),coordinate);
        self.enqueue(MipDecodeRequest::Paper {blob,coordinate},Some(key),Class::Visible)
    }
    pub(crate) fn request_coordinates(&mut self,inverse:[f64;6],size:[u32;2],first:u32,count:u32,destination:CoordinateDestination)->bool {
        self.enqueue(MipDecodeRequest::Coordinates(CoordinateRequest {inverse,size,first,count,destination}),None,Class::Visible)
    }
    fn enqueue(&mut self,request:MipDecodeRequest,key:Option<DecodeKey>,class:Class)->bool {
        if let Some(stored)=key.as_ref().and_then(|key|self.outstanding.iter_mut().find(|stored|stored.key.as_ref().is_some_and(|stored|stored.matches(key)))) {
            stored.class=stored.class.min(class);
            if let Some((view,region))=self.priority && (stored.region.is_empty() || Self::distance(view,region)<Self::distance(view,stored.region)) {
                stored.region=region;
                #[cfg(not(target_arch="wasm32"))]
                if let Ok(Some(read))=&stored.input.read {read.prioritize(Self::distance(view,region));}
            }
            return true;
        }
        if self.outstanding.len()>=self.capacity {return false;}
        let sequence=self.next_sequence;
        #[cfg(not(target_arch="wasm32"))]
        {
            let blob=match &request {MipDecodeRequest::Image(request)=>request.source.tiles.get(&request.coordinate),MipDecodeRequest::Raster(request)=>Some(&request.blob),MipDecodeRequest::Paper {blob,..}=>Some(blob),MipDecodeRequest::Coordinates(_)=>None};
            let samples=blob.map(|blob|self.context.cache.cached(blob)).transpose().map(Option::flatten);
            let read=samples.as_ref().map_err(Clone::clone).and_then(|samples|if samples.is_some() {Ok(None)}else{blob.map(|blob|blob.read_interest()).transpose().map(Option::flatten)});
            let input=Arc::new(DecodeInput {samples:samples.ok().flatten(),read});
            if let Some((view,region))=self.priority && let Ok(Some(read))=&input.read {read.prioritize(Self::distance(view,region));}
            let epoch=self.pool.epoch.load(Ordering::Acquire);
            if self.pool.requests.try_send(DecodeJob {sequence,request,context:Arc::downgrade(&self.context),input:Arc::downgrade(&input)}).is_err() {self.waiting=Some(epoch);self.pool.subscribe(&self.context,epoch);return false;}
            if let Ok(Some(read))=&input.read {read.start();}
            self.waiting=None;self.outstanding.push(Outstanding {sequence,key,region:self.priority.map_or(PixelRect::EMPTY,|(_,region)|region),class,input});
        }
        #[cfg(target_arch="wasm32")]
        {self.external=Some((sequence,request));self.outstanding.push(Outstanding {sequence,key,region:self.priority.map_or(PixelRect::EMPTY,|(_,region)|region),class});}
        self.next_sequence+=1;
        true
    }
    pub(crate) fn poll(&mut self)->Option<PreparedImageWork> {
        if !self.ready() {return None;}
        #[cfg(not(target_arch="wasm32"))]
        let (sequence,mut result)=self.results.try_recv().ok()?;
        #[cfg(target_arch="wasm32")]
        let (sequence,mut result)=self.completed.take()?;
        self.ready.fetch_sub(1,Ordering::AcqRel);
        if let Some(index)=self.outstanding.iter().position(|stored|stored.sequence==sequence) {result.raise(self.outstanding.swap_remove(index).class);}
        Some(result)
    }
    pub(crate) fn pending(&self)->bool {
        #[cfg(not(target_arch="wasm32"))]
        if self.waiting.is_some_and(|epoch|epoch==self.pool.epoch.load(Ordering::Acquire)) {return true;}
        !self.outstanding.is_empty()
    }
    pub(crate) fn ready(&self)->bool {self.ready.load(Ordering::Acquire)>0}
    #[cfg(test)]
    pub(crate) fn outstanding_classes(&self)->Vec<Class> {self.outstanding.iter().map(|outstanding|outstanding.class).collect()}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn take_external(&mut self)->Option<(u64,MipDecodeRequest)> {self.external.take()}
    #[cfg(target_arch="wasm32")]
    pub(crate) fn complete_external(&mut self,sequence:u64,result:PreparedImageWork) {self.completed=Some((sequence,result));self.ready.store(1,Ordering::Release);}
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::{page_texture,upload_page};
    #[cfg(not(target_arch="wasm32"))]
    fn queued(requests:std::sync::mpsc::SyncSender<DecodeJob>,results:std::sync::mpsc::Receiver<(u64,PreparedImageWork)>,
        ready:Arc<std::sync::atomic::AtomicUsize>,outstanding:Vec<Option<DecodeKey>>,capacity:usize)->MipDecodeQueue {
        let (send,_)=std::sync::mpsc::sync_channel(capacity);
        let context=Arc::new(DecodeContext {cache:Arc::new(layer_core::raster::DecodedTileCache::new(0)),destination:RgbSpace::Srgb,send,ready:ready.clone(),wake:Default::default(),waiter:Default::default()});
        MipDecodeQueue {pool:Arc::new(DecodePool {requests,capacity,epoch:Default::default(),capacity_waiters:Default::default(),resumed:Default::default()}),context,waiting:None,results,ready,next_sequence:outstanding.len() as u64,
            outstanding:outstanding.into_iter().zip(0..).map(|(key,sequence)|Outstanding {sequence,key,region:PixelRect::EMPTY,class:Class::Visible,input:Arc::new(DecodeInput {samples:None,read:Ok(None)})}).collect(),capacity,priority:None}
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn snapshot_decode_registration_rechecks_capacity_and_releases_its_task_waker() {
        struct Count(std::sync::atomic::AtomicUsize);
        impl std::task::Wake for Count {fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}}
        let (requests,_)=std::sync::mpsc::sync_channel(1);let (_,results)=std::sync::mpsc::sync_channel(1);
        let mut queue=queued(requests,results,Default::default(),Vec::new(),1);
        queue.waiting=Some(0);queue.pool.epoch.store(1,Ordering::Release);
        let count=Arc::new(Count(Default::default()));let waker=std::task::Waker::from(count.clone());
        queue.register_waker(&waker);assert_eq!(count.0.load(Ordering::Relaxed),1);
        assert!(queue.context.waiter.lock().unwrap().is_none());
        queue.waiting=None;queue.register_waker(&waker);assert_eq!(Arc::strong_count(&count),3);
        queue.clear_waker();assert_eq!(Arc::strong_count(&count),2);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn snapshot_decode_late_results_are_observed_after_registration() {
        use std::sync::{Mutex,mpsc,atomic::AtomicUsize};
        for paper in [false,true] {
            let (requests,_)=mpsc::sync_channel(1);let (send,results)=mpsc::sync_channel(1);
            let ready=Arc::new(AtomicUsize::new(1));
            let mut queue=queued(requests,results,ready,Vec::new(),1);
            let target=Arc::new(Mutex::new(CoordinateSlot::new(&Default::default())));
            let result=if paper {
                let pixels=Arc::new(vec![73;(PAGE_SIZE*PAGE_SIZE) as usize]);
                let blob=Arc::new(layer_core::raster::TileBlob::encode(layer_core::color::PixelDescriptor::COVERAGE8,&pixels).unwrap());
                PreparedImageWork::Paper {blob,coordinate:[0;2],pixels:Ok(pixels)}
            }else {PreparedImageWork::Coordinates {destination:Arc::downgrade(&target),pixels:Ok(Arc::new(vec![7]))}};
            send.send((0,result)).unwrap();let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
            queue.set_wake(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);}));
            assert_eq!(calls.load(Ordering::Relaxed),1,"late registration must observe the durable result (paper={paper})");
            assert!(queue.poll().is_some());assert!(!queue.ready());
        }
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_refused_decode_with_no_own_jobs_wakes_when_another_owner_frees_capacity() {
        use std::sync::{Mutex,mpsc,atomic::AtomicUsize};
        use std::time::Duration;
        let cache=Arc::new(layer_core::raster::DecodedTileCache::new(0));
        let pool=DecodePool::start(1).unwrap();
        let owner=|| {
            let mut queue=MipDecodeQueue::native(cache.clone(),RgbSpace::Srgb).unwrap();
            queue.capacity=pool.capacity;queue.pool=pool.clone();queue
        };
        let mut blockers=owner();
        let workers=blockers.capacity/6;
        let (entered,seen)=mpsc::channel();let (release,held)=mpsc::channel();let held=Mutex::new(held);
        blockers.set_wake(Arc::new(move || {let _=entered.send(());let _=held.lock().unwrap().recv();}));
        let target=Arc::new(Mutex::new(CoordinateSlot::new(&Default::default())));
        for n in 0..workers {assert!(blockers.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],n as u32,1,Arc::downgrade(&target)));}
        for _ in 0..workers {seen.recv_timeout(Duration::from_secs(5)).unwrap();}
        let mut other=owner();
        for n in 0..other.capacity {assert!(other.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],n as u32,1,Arc::downgrade(&target)));}
        let mut refused=owner();
        let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();let (notified,woken)=mpsc::channel();
        refused.set_wake(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);let _=notified.send(());}));
        assert!(!refused.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],0,1,Arc::downgrade(&target)));
        assert!(refused.outstanding.is_empty());assert!(refused.waiting.is_some());
        for _ in 0..workers {release.send(()).unwrap();}
        let result=woken.recv_timeout(Duration::from_secs(5));
        assert!(result.is_ok(),"request-channel capacity must wake a queue with no result of its own");
        assert_eq!(calls.load(Ordering::Relaxed),1);assert!(!refused.pending());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_resident_decode_completes_while_every_worker_read_is_held() {
        use std::time::Duration;
        let cache=Arc::new(layer_core::raster::DecodedTileCache::new(0));
        let pool=DecodePool::start(1).unwrap();
        let owner=|| {
            let mut queue=MipDecodeQueue::native(cache.clone(),RgbSpace::Srgb).unwrap();
            queue.capacity=pool.capacity;queue.pool=pool.clone();queue
        };
        let workers=pool.capacity/6;
        let stored=layer_core::color::source::rgba8_source([workers as u32*PAGE_SIZE,PAGE_SIZE],|x,y|[x as u8,y as u8,41,255]);
        let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([stored.as_ref()]),false);
        let mut held=owner();
        let (landed,arrived)=std::sync::mpsc::channel();
        held.set_wake(Arc::new(move || {let _=landed.send(());}));
        for x in 0..workers as u32 {assert!(held.request(PortableId::random(),stored.clone(),[x,0]));}
        store.gate.entered();
        let resident=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|x,y|[y as u8,x as u8,97,255]);
        let mut visible=owner();
        let (notified,woken)=std::sync::mpsc::channel();
        visible.set_wake(Arc::new(move || {let _=notified.send(());}));
        assert!(visible.request(PortableId::random(),resident,[0,0]));
        assert!(woken.recv_timeout(Duration::from_secs(5)).is_ok(),"a decode whose input is resident must not wait behind another owner's held read");
        assert!(matches!(visible.poll(),Some(PreparedImageWork::Image(tile)) if tile.pixels.is_ok()));
        assert_eq!(held.ready.load(Ordering::Acquire),0);
        store.gate.release();
        let deadline=std::time::Instant::now()+Duration::from_secs(10);
        let mut decoded=0;
        while decoded<workers {
            assert!(arrived.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).is_ok(),"a released read must resume its decode");
            while let Some(work)=held.poll() {assert!(matches!(work,PreparedImageWork::Image(tile) if tile.pixels.is_ok()));decoded+=1;}
        }
        assert!(!held.pending());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn accepted_decode_jobs_fill_the_read_window_before_a_decode_worker_runs() {
        let source=layer_core::color::source::rgba8_source([8*PAGE_SIZE,PAGE_SIZE],|x,y|[x as u8,y as u8,73,255]);
        let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
        let (requests,_receive)=std::sync::mpsc::sync_channel(8);let (_send,results)=std::sync::mpsc::sync_channel(8);
        let mut queue=queued(requests,results,Default::default(),Vec::new(),8);
        for x in 0..8 {assert!(queue.request(PortableId::random(),source.clone(),[x,0]));}
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while store.gate.reads.load(Ordering::Acquire)<8 && std::time::Instant::now()<deadline {std::thread::yield_now();}
        assert_eq!(store.gate.reads.load(Ordering::Acquire),8,"queued accepted jobs must initiate reads without occupying decode workers");
        assert_eq!(queue.ready.load(Ordering::Acquire),0);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn resident_samples_do_not_reread_the_compressed_store() {
        let source=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[29,71,83,255]);
        let (requests,receive)=std::sync::mpsc::sync_channel(1);let (_send,results)=std::sync::mpsc::sync_channel(1);
        let mut queue=queued(requests,results,Default::default(),Vec::new(),1);
        queue.context.cache.set_limit((PAGE_SIZE*PAGE_SIZE*4) as usize);
        let samples=queue.context.cache.decode_on_worker(&source.tiles[&[0,0]]).unwrap();
        let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
        assert!(queue.request(PortableId::random(),source,[0,0]));let job=receive.try_recv().unwrap();
        let input=job.input.upgrade().unwrap();assert!(input.read.as_ref().unwrap().is_none());assert!(Arc::ptr_eq(input.samples.as_ref().unwrap(),&samples));
        assert_eq!(store.gate.reads.load(Ordering::Acquire),0);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn paper_tiles_pipeline_to_capacity_and_complete_out_of_order() {
        use std::sync::atomic::AtomicUsize;
        let (requests,receive)=std::sync::mpsc::sync_channel(2);
        let (send,results)=std::sync::mpsc::sync_channel(2);let ready=Arc::new(AtomicUsize::new(0));
        let mut queue=queued(requests,results,ready.clone(),Vec::new(),2);
        let pixels=Arc::new(vec![73;(PAGE_SIZE*PAGE_SIZE) as usize]);
        let blob=Arc::new(layer_core::raster::TileBlob::encode(layer_core::color::PixelDescriptor::COVERAGE8,&pixels).unwrap());
        assert!(queue.request_paper(blob.clone(),[0,0]));assert!(queue.request_paper(blob.clone(),[1,0]));
        assert!(queue.request_paper(blob.clone(),[0,0]));assert!(!queue.request_paper(blob.clone(),[2,0]));
        for (sequence,coordinate) in [[0,0],[1,0]].into_iter().enumerate() {
            assert!(matches!(receive.try_recv(),Ok(DecodeJob {sequence:job,request:MipDecodeRequest::Paper {coordinate:at,..},..}) if at==coordinate && job==sequence as u64));
        }
        assert!(receive.try_recv().is_err());
        send.send((1,PreparedImageWork::Paper {blob:blob.clone(),coordinate:[1,0],pixels:Ok(pixels.clone())})).unwrap();ready.fetch_add(1,Ordering::AcqRel);
        assert!(matches!(queue.poll(),Some(PreparedImageWork::Paper {coordinate:[1,0],..})));
        assert!(queue.request_paper(blob.clone(),[0,0]));assert!(receive.try_recv().is_err());
        assert!(queue.request_paper(blob.clone(),[2,0]));
        assert!(matches!(receive.try_recv(),Ok(DecodeJob {sequence:2,request:MipDecodeRequest::Paper {coordinate:[2,0],..},..})));
        for (sequence,coordinate) in [(2,[2,0]),(0,[0,0])] {
            send.send((sequence,PreparedImageWork::Paper {blob:blob.clone(),coordinate,pixels:Ok(pixels.clone())})).unwrap();ready.fetch_add(1,Ordering::AcqRel);
            assert!(matches!(queue.poll(),Some(PreparedImageWork::Paper {coordinate:at,..}) if at==coordinate));
        }
        assert!(!queue.pending());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn completed_preparation_publishes_pixels_and_readiness_before_one_wake() {
        use std::sync::{Mutex,mpsc,atomic::AtomicUsize};
        use std::time::Duration;
        let cache=Arc::new(layer_core::raster::DecodedTileCache::new(0));
        let mut queue=MipDecodeQueue::native(cache,RgbSpace::Srgb).unwrap();
        let context=Arc::downgrade(&queue.context);
        let calls=Arc::new(AtomicUsize::new(0));let notified=calls.clone();
        let (wake,awakened)=mpsc::channel();let (release,held)=mpsc::channel();let held=Mutex::new(held);
        let (finished,completed)=mpsc::channel();
        queue.set_wake(Arc::new(move|| {
            let ready=context.upgrade().unwrap().ready.load(Ordering::Acquire);
            notified.fetch_add(1,Ordering::Relaxed);
            if wake.send(ready).is_ok() {let _=held.lock().unwrap().recv();}
            let _=finished.send(());
        }));
        let target=Arc::new(Mutex::new(CoordinateSlot::new(&Default::default())));let destination=Arc::downgrade(&target);
        assert!(queue.request_coordinates([1.,0.,0.,1.,3.,-2.],[256;2],17,1,destination.clone()));
        let ready=awakened.recv_timeout(Duration::from_secs(5));
        let result=queue.poll();let pending=queue.pending();let still_ready=queue.ready();
        let _=release.send(());
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(ready.unwrap(),1);
        let PreparedImageWork::Coordinates {destination:received,pixels}=result.unwrap() else {panic!("Coordinate preparation must be queued before notification");};
        assert!(received.ptr_eq(&destination));
        assert_eq!(pixels.unwrap().as_slice(),[20_i32,-2].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>());
        assert!(!pending);assert!(!still_ready);assert!(queue.poll().is_none());
        assert_eq!(calls.load(Ordering::Relaxed),1);assert!(awakened.try_recv().is_err());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn dropped_preparation_queue_releases_its_wake_and_abandoned_jobs_never_notify() {
        use std::sync::{Mutex,atomic::AtomicUsize};
        let queue=MipDecodeQueue::native(Arc::new(layer_core::raster::DecodedTileCache::new(0)),RgbSpace::Srgb).unwrap();
        let calls=Arc::new(AtomicUsize::new(0));let notified=calls.clone();
        let callback_owner=Arc::new(());let callback_lifetime=Arc::downgrade(&callback_owner);
        queue.set_wake(Arc::new(move|| {let _=&callback_owner;notified.fetch_add(1,Ordering::Relaxed);}));
        let target=Arc::new(Mutex::new(CoordinateSlot::new(&Default::default())));let ready=queue.ready.clone();
        let input=Arc::new(DecodeInput {samples:None,read:Ok(None)});
        let job=DecodeJob {sequence:0,context:Arc::downgrade(&queue.context),input:Arc::downgrade(&input),request:MipDecodeRequest::Coordinates(CoordinateRequest {
            inverse:[1.,0.,0.,1.,0.,0.],size:[256;2],first:0,count:1,destination:Arc::downgrade(&target)})};
        drop(queue);assert!(callback_lifetime.upgrade().is_none());
        job.run(&mut Decoders::new(),&Weak::new());
        assert_eq!(calls.load(Ordering::Relaxed),0);assert_eq!(ready.load(Ordering::Acquire),0);
        assert!(target.lock().unwrap().result.is_none());
    }
    #[test]
    fn obsolete_image_decode_failure_is_visible_only_to_its_requested_source_and_context() {
        let a=layer_core::color::source::rgba8_source([1;2],|_,_|[1,2,3,255]);
        let b=layer_core::color::source::rgba8_source([1;2],|_,_|[9,8,7,255]);
        let (aid,bid)=(PortableId::random(),PortableId::random());
        let weak=Arc::downgrade(&a);let mut failures=Vec::new();
        let stale=PreparedMipTile {class:crate::residency::Class::Visible,use_:crate::scene::sources::SourceUse::Native,id:aid,source:a.clone(),coordinate:[0;2],pixels:Err("retired A decode".into())};
        assert!(!retain_image_decode(&stale,RgbSpace::Srgb,&mut failures));
        let current=PreparedMipTile {class:crate::residency::Class::Visible,use_:crate::scene::sources::SourceUse::Native,id:bid,source:b.clone(),coordinate:[0;2],pixels:Ok(crate::scene::sources::PreparedSourcePixels::NativeSamples(Arc::new(vec![9,8,7,255])))};
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
        let pending=|r:&WgpuRasterizer,source:&Arc<SourceImage>|Arc::new(r.source_tiles.borrow_mut().fixture_image_plan(r,source,[0;2],crate::scene::sources::SourceUse::Native).unwrap().1.unwrap());
        let (ra,rb)=(pending(&r,&a),pending(&r,&b));
        let result=|id,source:&Arc<SourceImage>|PreparedImageWork::Image(PreparedMipTile {class:crate::residency::Class::Visible,use_:crate::scene::sources::SourceUse::Native,id,source:source.clone(),coordinate:[0;2],
            pixels:prepare_image_tile(source,[0;2],space,&r.device.source_samples,None)});
        let (send,results)=std::sync::mpsc::sync_channel(2);assert!(send.send((1,result(bid,&b))).is_ok());assert!(send.send((0,result(aid,&a))).is_ok());
        let (requests,receive)=std::sync::mpsc::sync_channel(2);
        r.moving_decode=Some((space,queued(requests,results,Arc::new(std::sync::atomic::AtomicUsize::new(2)),
            vec![Some(DecodeKey::Image(Arc::downgrade(&a),[0;2])),Some(DecodeKey::Image(Arc::downgrade(&b),[0;2]))],2)));
        r.source_receipt=Some(Arc::downgrade(&ra));
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decode(&mut encoder).unwrap();assert_eq!(r.source_results.len(),1);assert!(ra.prepared().is_none());
        assert!(r.request_image_decode(bid,b.clone(),[0;2],crate::residency::Class::Visible).unwrap());assert!(receive.try_recv().is_err());
        r.drain_image_decode(&mut encoder).unwrap();assert!(ra.prepared().is_some());assert_eq!(r.source_results.len(),1);
        r.source_receipt=Some(Arc::downgrade(&rb));assert!(r.held_result_ready());assert!(!r.image_decode_waiting());
        assert!(r.request_image_decode(bid,b.clone(),[0;2],crate::residency::Class::Visible).unwrap());assert!(receive.try_recv().is_err());
        r.drain_image_decode(&mut encoder).unwrap();assert!(rb.prepared().is_some());assert!(r.source_results.is_empty());
        assert!(r.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.pending()));
        r.drain_image_decode(&mut encoder).unwrap();assert!(r.source_results.is_empty());assert!(receive.try_recv().is_err());
        let c=layer_core::color::source::rgba8_source([1;2],|_,_|[0,255,0,255]);
        assert!(r.request_image_decode(bid,c.clone(),[0;2],crate::residency::Class::Visible).unwrap());
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
        let target=Arc::new(std::sync::Mutex::new(CoordinateSlot::new(&Default::default())));
        let destination=Arc::downgrade(&target);
        assert!(sender.send((0,PreparedImageWork::Coordinates {destination:destination.clone(),pixels:Ok(Arc::new(vec![7]))})).is_ok());
        assert!(queue.poll().is_none());
        assert!(!queue.ready());assert!(queue.pending());
        ready.fetch_add(1,Ordering::AcqRel);
        let PreparedImageWork::Coordinates {destination:completed,pixels}=queue.poll().unwrap() else {panic!("Prepared coordinates retain their identity");};
        assert!(completed.ptr_eq(&destination));assert_eq!(pixels.unwrap().as_slice(),[7]);
        assert!(!queue.ready());assert!(!queue.pending());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn out_of_order_keyless_results_complete_the_requests_that_produced_them() {
        let mut queue=MipDecodeQueue::native(Arc::new(layer_core::raster::DecodedTileCache::new(0)),RgbSpace::Srgb).unwrap();
        let (requests,receive)=std::sync::mpsc::sync_channel(3);
        queue.pool=Arc::new(DecodePool {requests,capacity:3,epoch:Default::default(),capacity_waiters:Default::default(),resumed:Default::default()});
        let targets:Vec<_>=(0..3).map(|_|Arc::new(std::sync::Mutex::new(CoordinateSlot::new(&Default::default())))).collect();
        for (first,target) in targets.iter().enumerate() {assert!(queue.request_coordinates([1.,0.,0.,1.,0.,0.],[256;2],first as u32,1,Arc::downgrade(target)));}
        let jobs:Vec<_>=receive.try_iter().collect();assert_eq!(jobs.len(),3);
        for (first,job) in jobs.into_iter().enumerate().rev() {
            job.run(&mut Decoders::new(),&Weak::new());
            let Some(PreparedImageWork::Coordinates {destination,pixels})=queue.poll() else {panic!("request {first} must deliver its own result");};
            assert!(destination.ptr_eq(&Arc::downgrade(&targets[first])));
            assert_eq!(pixels.unwrap().as_slice(),[first as i32,0].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>());
        }
        assert!(!queue.pending());assert!(!queue.ready());
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
        assert!(!queue.pending());assert_eq!(cache.stats().misses,2*accepted.len() as u64,"each tile misses once when queued and once when its worker decodes it");
        use layer_core::color::{AlphaAssociation,PixelDescriptor,SampleType,TransferEncoding};
        let hdr=PixelDescriptor {channels:4,bits_per_channel:32,sample:SampleType::Float,encoding:TransferEncoding::Linear,alpha:AlphaAssociation::Straight};
        for (descriptor,input,expected) in [
            (PixelDescriptor {bits_per_channel:16,..PixelDescriptor::COVERAGE8},vec![0x34,0x12],vec![0x34,0x12,0x34,0x12,0x34,0x12,255,255]),
            (PixelDescriptor {channels:2,..hdr},[-2f32,0.5].into_iter().flat_map(f32::to_le_bytes).collect(),[-2f32,-2.,-2.,0.5].into_iter().flat_map(f32::to_le_bytes).collect()),
            (hdr,[-1f32,2.,0.25,0.5].into_iter().flat_map(f32::to_le_bytes).collect(),[-1f32,2.,0.25,0.5].into_iter().flat_map(f32::to_le_bytes).collect()),
        ] {
            let blob=Arc::new(layer_core::raster::TileBlob::encode(descriptor,&input.repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap());
            assert!(queue.request_raster(blob.clone(),RgbSpace::Srgb,RgbSpace::DisplayP3,false,crate::residency::Class::Visible));
            assert!(queue.request_raster(blob.clone(),RgbSpace::Srgb,RgbSpace::DisplayP3,false,crate::residency::Class::Visible));
            assert_eq!(queue.outstanding.len(),1);
            let PreparedImageWork::Raster(tile)=drain(&mut queue,1).pop().unwrap() else {panic!("Raster preparation returns native samples");};
            assert!(Arc::ptr_eq(&tile.blob,&blob));assert_eq!(tile.space,RgbSpace::Srgb);assert_eq!(tile.destination,RgbSpace::DisplayP3);
            let crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes)=tile.pixels.unwrap() else {panic!("Raster codes retain their descriptor and alpha");};
            assert_eq!(bytes.as_slice(),expected.repeat((PAGE_SIZE*PAGE_SIZE) as usize));
            assert!(!queue.pending());
        }
        let destination=Arc::new(std::sync::Mutex::new(CoordinateSlot::new(&Default::default())));
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
        MovingRequest {id:PortableId::random(),source:layer_core::color::source::rgba8_source([256;2],|x,y|{let value=if (x+y)%2==0 {0} else {255};[value,value,value,255]}),level:1,nearest:false,bounds:PixelRect::full([256;2]),format:wgpu::TextureFormat::Rgba16Float,rank:0}
    }
    #[test]
    fn photo_mip_demand_has_one_view_bound_for_one_twenty_eight_and_one_hundred_layers() {
        let extent=[4096,3072];
        let bound=working_bytes()+2*4*2560u64*1600*8;
        for count in [1,28,100] {
            let requests:Vec<_>=(0..count).map(|index| {
                let tile=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[index,73,149,255]);
                let blob=tile.tiles.values().next().unwrap().clone();
                let tiles=(0..extent[1]/PAGE_SIZE).flat_map(|y|(0..extent[0]/PAGE_SIZE).map(move|x|[x,y]))
                    .map(|coordinate|(coordinate,blob.clone())).collect();
                MovingRequest {source:Arc::new(SourceImage {extent,tiles,..(*tile).clone()}),bounds:PixelRect::full(extent),..moving_request()}
            }).collect();
            let demand=MovingImages::demand(&requests,[2560,1600]).unwrap();
            assert!(demand<=bound,"{count} photos demand {demand} bytes for one view bounded by {bound}");
        }
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
    include!("object_image_window_tests.rs");
}

#[cfg(all(test,not(target_arch="wasm32")))]
#[path="object_image_pool_tests.rs"]
mod pool_tests;
