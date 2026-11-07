use crate::{display_mips, GpuRasterError, PipelineDevice, WgpuRasterizer, PAGE_SIZE, PixelRect};
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
pub(crate) struct MipPlan { targets: Vec<Planned>, pub bytes: u64, pub waiting: bool }
struct Entry {
    id: PortableId,
    source: Weak<SourceImage>,
    image: display_mips::Window,
    pending: VecDeque<[u32;2]>,
    valid: Arc<AtomicBool>,
    root: bool,
    requested: bool,
    rank: usize,
}
impl Entry {
    fn matches(&self, id: PortableId, source: &Arc<SourceImage>) -> bool { self.id == id && self.source.ptr_eq(&Arc::downgrade(source)) }
    fn matches_plan(&self, target: &Planned) -> bool {
        self.matches(target.request.id, &target.request.source) && self.root == target.root && self.image.plan == target.plan
            && self.image.texture.format() == target.request.format
    }
    fn ready(&self) -> bool { self.pending.is_empty() && self.valid.load(Ordering::Acquire) }
    fn storage_bytes(&self) -> u64 { self.image.storage_bytes() }
    fn encoded(&self) -> bool {
        matches!(self.image.texture.format(),wgpu::TextureFormat::Rgba8Unorm|wgpu::TextureFormat::Rg32Uint)
    }
    fn requested(&self, requests: &[MovingRequest]) -> bool { requests.iter().any(|request| self.matches(request.id, &request.source)) }
}
struct Retired { entry: Entry, complete: Arc<AtomicBool> }
struct RetirementCompletion(Arc<AtomicBool>);
impl Drop for RetirementCompletion { fn drop(&mut self) { self.0.store(true,Ordering::Release); } }
#[derive(Default)]
pub(crate) struct MovingImages {
    entries: Vec<Entry>,
    retired: Vec<Retired>,
    work: Option<display_mips::Image>,
    allocation_waiting: bool,
    context: Option<RgbSpace>,
    pipelines: Option<display_mips::WindowPipelines>,
    planned: Option<(u64,Vec<MovingRequest>)>,
    pub tile_builds: u64,
    pub completed_images: u64,
    pub evictions: u64,
}
const LEVEL_ZERO_BYTES:u64=1024*1024*1024;
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
    pub(crate) fn source_format(source:&SourceImage)->wgpu::TextureFormat {
        crate::scene::sources::Samples::for_source(source,[0;2],RgbSpace::Srgb)
            .map_or(wgpu::TextureFormat::Rgba32Float,|samples|samples.format())
    }
    pub(crate) fn format(scene: layer_core::SceneView<'_>) -> wgpu::TextureFormat {
        let full=scene.composition().color.depth.is_float() || scene.order().iter().any(|&owner| {
            scene.effect(owner).is_some()
                || scene.object_layer(owner).is_some_and(|object|object.image.interpretation.depth.is_float())
                || scene.paint_source(owner).is_some_and(|paint|paint.base.as_ref().is_some_and(|base|base.image.interpretation.depth.is_float()))
        });
        if full {wgpu::TextureFormat::Rgba32Float} else {wgpu::TextureFormat::Rgba16Float}
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
    pub(crate) fn storage_bytes(&self)->u64 {
        self.work.as_ref().map_or(0,display_mips::Image::storage_bytes)
            +self.entries.iter().map(Entry::storage_bytes).sum::<u64>()
            +self.retired.iter().map(|retired|retired.entry.storage_bytes()).sum::<u64>()
    }
    pub(crate) fn cpu_bytes(&self)->u64 {
        self.entries.capacity() as u64*std::mem::size_of::<Entry>() as u64
            +self.retired.capacity() as u64*std::mem::size_of::<Retired>() as u64
            +self.entries.iter().chain(self.retired.iter().map(|retired|&retired.entry))
                .map(|entry|entry.pending.capacity() as u64*std::mem::size_of::<[u32;2]>() as u64).sum::<u64>()
            +self.work.as_ref().map_or(0,display_mips::Image::cpu_bytes)
            +self.planned.as_ref().map_or(0,|(_,requests)|requests.capacity() as u64*std::mem::size_of::<MovingRequest>() as u64)
    }
    pub(crate) fn pinned_bytes(&self)->u64 {
        self.work.as_ref().map_or(0,display_mips::Image::storage_bytes)
            +self.retired.iter().filter(|retired|!retired.complete.load(Ordering::Acquire)).map(|retired|retired.entry.storage_bytes()).sum::<u64>()
            +self.entries.iter().filter(|entry|!entry.ready()).map(Entry::storage_bytes).sum::<u64>()
    }
    fn retire(&mut self,entry:Entry,encoder:&mut crate::submission::CommandEncoder) {
        self.evictions+=1;
        let complete=Arc::new(AtomicBool::new(false));
        let callback=RetirementCompletion(complete.clone());
        encoder.on_submitted_work_done(move||drop(callback));
        self.retired.push(Retired {entry,complete});
    }
    pub(crate) fn trim_unused(&mut self,limit:u64,requests:&[MovingRequest],encoder:&mut crate::submission::CommandEncoder)->u64 {
        self.retired.retain(|retired|!retired.complete.load(Ordering::Acquire));
        let mut bytes=0;
        while self.storage_bytes().saturating_sub(self.retired.iter().map(|retired|retired.entry.storage_bytes()).sum::<u64>())>limit {
            let Some(index)=self.entries.iter().enumerate().filter(|(_,entry)|entry.ready())
                .max_by_key(|(_,entry)|(!entry.requested(requests),entry.root,std::cmp::Reverse(entry.rank),entry.storage_bytes())).map(|(index,_)|index) else {break;};
            let entry=self.entries.swap_remove(index);bytes+=entry.storage_bytes();self.retire(entry,encoder);self.planned=None;
        }
        if self.entries.is_empty() && self.retired.is_empty() { self.work=None; }
        bytes
    }
    pub(crate) fn pending(&self)->bool {self.allocation_waiting || !self.retired.is_empty() || self.entries.iter().any(|entry|!entry.ready())}
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
        self.retired.retain(|retired|!retired.complete.load(Ordering::Acquire));
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
            return Ok(MipPlan {targets:Vec::new(),bytes:self.storage_bytes(),waiting:false});
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
            if !changed && requested {self.entries.push(entry);} else {self.retire(entry,encoder);}
        }
        targets.retain(|target|!self.entries.iter().any(|entry|entry.matches_plan(target)));
        if targets.is_empty() && self.entries.is_empty() && self.retired.is_empty() {self.work=None;}
        let bytes=self.storage_bytes()+targets.iter().map(|target|target.bytes).sum::<u64>()
            +if self.work.is_none() && !targets.is_empty() {working_bytes()} else {0};
        if bytes>budget {
            let entries=std::mem::take(&mut self.entries);
            for entry in entries {
                if entry.requested {self.entries.push(entry);} else {self.retire(entry,encoder);}
            }
            self.allocation_waiting=!self.retired.is_empty();
            return Ok(MipPlan {targets:Vec::new(),bytes:self.storage_bytes(),waiting:self.allocation_waiting});
        }
        if targets.is_empty() {self.planned=Some((budget,requests.to_vec()));}
        Ok(MipPlan {targets,bytes,waiting:false})
    }
    pub(crate) fn allocate(&mut self,r:&WgpuRasterizer,plan:MipPlan,encoder:&mut crate::submission::CommandEncoder)->bool {
        if plan.targets.is_empty() {return false;}
        self.pipelines(&r.device);
        let work=self.work.get_or_insert_with(||display_mips::Image::with_device(&r.device,
            display_mips::Plan::at([PAGE_SIZE;2],display_mips::MAX_LEVEL),display_mips::MAX_LEVEL));
        let mut replacements=Vec::new();let mut copied:Vec<(PortableId,Weak<SourceImage>)>=Vec::new();
        for target in plan.targets {
            let mut image=display_mips::Window::new(&r.device,target.plan,target.request.format,work,self.pipelines.as_ref().unwrap());
            if image.texture.format()==wgpu::TextureFormat::Rgba8Unorm
                && crate::scene::sources::Samples::for_source(&target.request.source,[0;2],r.device.working_space()).is_some_and(|samples|samples.srgb_view()) {
                image.view=image.texture.create_view(&wgpu::TextureViewDescriptor {format:Some(wgpu::TextureFormat::Rgba8UnormSrgb),usage:Some(wgpu::TextureUsages::TEXTURE_BINDING),..Default::default()});
            }
            let mut pending:Vec<_>=crate::page_coordinates(target.plan.bounds).collect();
            let centre=[target.plan.bounds.min_x()+target.plan.bounds.width()/2,target.plan.bounds.min_y()+target.plan.bounds.height()/2];
            if target.plan.bounds!=PixelRect::full(target.plan.extent) {
                pending.sort_by_key(|coordinate|coordinate.into_iter().zip(centre).map(|(value,centre)|i64::from(value*PAGE_SIZE+PAGE_SIZE/2).abs_diff(i64::from(centre)).pow(2)).sum::<u64>());
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
            replacements.push(Entry {id:target.request.id,source,image,pending:pending.into(),valid,root:target.root,requested:true,rank:target.request.rank});
        }
        let entries=std::mem::take(&mut self.entries);
        for entry in entries {
            if !entry.ready() && !entry.requested {self.retire(entry,encoder);} else {self.entries.push(entry);}
        }
        self.entries.extend(replacements);
        let changed=!copied.is_empty();
        for (id,source) in copied {self.record_completed_image(id,&source);}
        changed
    }
    fn record_completed_image(&mut self,id:PortableId,source:&Weak<SourceImage>) {
        if self.entries.iter().filter(|entry|entry.requested && entry.id==id && entry.source.ptr_eq(source)).all(|entry|entry.pending.is_empty()) {self.completed_images+=1;}
    }
    pub(crate) fn lookup(&self,id:PortableId,source:&Arc<SourceImage>,requested:u32,bounds:PixelRect,format:wgpu::TextureFormat,context:RgbSpace)->Option<MovingSource> {
        if self.context!=Some(context) {return None;}
        let candidates=||self.entries.iter().rev().filter(|entry|entry.matches(id,source) && entry.ready()
            && entry.image.texture.format()==format
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
    pub(crate) fn next_build(&mut self)->Option<(PortableId,Arc<SourceImage>,[u32;2])> {
        for entry in &mut self.entries {
            if !entry.valid.load(Ordering::Acquire) {
                entry.pending=crate::page_coordinates(entry.image.plan.bounds).collect();entry.valid=Arc::new(AtomicBool::new(true));
            }
        }
        let entry=self.entries.iter().filter(|entry|!entry.pending.is_empty())
            .min_by_key(|entry|(entry.root,std::cmp::Reverse(entry.rank)))?;
        Some((entry.id,entry.source.upgrade()?,*entry.pending.front()?))
    }
    pub(crate) fn upcoming(&self,count:usize)->Vec<(PortableId,Arc<SourceImage>,[u32;2])> {
        let mut entries:Vec<_>=self.entries.iter().filter(|entry|entry.valid.load(Ordering::Acquire)).collect();
        entries.sort_by_key(|entry|(entry.root,std::cmp::Reverse(entry.rank)));
        entries.into_iter().flat_map(|entry| {
            let source=entry.source.upgrade();
            entry.pending.iter().filter_map(move|&coordinate|Some((entry.id,source.clone()?,coordinate)))
        }).take(count).collect()
    }
    pub(crate) fn accepts_tile(&self,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.entries.iter().any(|entry|entry.matches(id,source) && entry.valid.load(Ordering::Acquire) && entry.pending.front()==Some(&coordinate))
    }
    pub(crate) fn needs_samples(&self,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],texture:&wgpu::Texture)->bool {
        self.entries.iter().any(|entry|entry.matches(id,source) && entry.pending.contains(&coordinate)
            && entry.encoded() && entry.image.texture.format()!=texture.format())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_tile(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,id:PortableId,source:&Arc<SourceImage>,coordinate:[u32;2],texture:&wgpu::Texture,pixels:Option<&crate::scene::sources::PreparedSourcePixels>)->Result<bool,GpuRasterError> {
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
                let Some(crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes))=pixels else {return Err(GpuRasterError::DeferredObjectWork);};
                let row=PAGE_SIZE*entry.image.texture.format().block_copy_size(None).unwrap();
                if bytes.len()!=row as usize*PAGE_SIZE as usize {return Err(GpuRasterError::InvalidImage);}
                r.uploads.write_texture_region(encoder,&entry.image.texture,destination,extent,row,|mapped|mapped.copy_from_slice(bytes))?;
                r.source_tiles.borrow().charge_upload(encoder,bytes.len() as u64);
            }
        }
        if let Some(level)=level {
            self.pipelines.as_ref().unwrap().prepare_tile(r,encoder,self.work.as_mut().unwrap(),texture,source,coordinate,level)?;
            display_mips::Window::copy_tiles(self.pipelines.as_ref().unwrap(),encoder,self.work.as_ref().unwrap(),coordinate,
                windows().filter(|entry|!entry.encoded()).map(|entry|&entry.image))?;
        }
        let mut changed=false;
        for entry in self.entries.iter_mut().filter(|entry|entry.matches(id,source) && entry.pending.contains(&coordinate)) {
            crate::submission::CacheWrite::shared(entry.valid.clone()).track(encoder);
            let position=entry.pending.iter().position(|pending|*pending==coordinate).unwrap();
            entry.pending.remove(position);
            if entry.pending.is_empty() {changed=true;self.planned=None;}
        }
        if changed {self.record_completed_image(id,&Arc::downgrade(source));}
        self.tile_builds+=1;
        Ok(changed)
    }
}

pub(crate) struct ImageDecodeRequest {
    pub use_:crate::scene::sources::SourceUse,
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
    Paper {blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2]},
    Coordinates(CoordinateRequest),
}
pub(crate) enum PreparedImageWork {
    Image(PreparedMipTile),
    Raster(PreparedRasterTile),
    Paper {blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2],pixels:Result<Arc<Vec<u8>>,String>},
    Coordinates {destination:CoordinateDestination,pixels:CoordinateResult},
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
    wake: std::sync::OnceLock<Arc<dyn Fn() + Send + Sync>>,
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
            if let Some(wake)=context.wake.get() {wake();}
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
        MipDecodeRequest::Paper {blob,coordinate}=> {
            let pixels=contained(||cache.decode_on_worker(&blob));
            PreparedImageWork::Paper {blob,coordinate,pixels}
        },
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
            PreparedImageWork::Image(PreparedMipTile {use_:request.use_,id:request.id,source:request.source,coordinate:request.coordinate,pixels})
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
        let context=Arc::new(DecodeContext {cache,destination,send,ready:ready.clone(),wake:Default::default()});
        Ok(Self {pool,context,waiting:None,results,ready,outstanding:Vec::new(),capacity})
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {let _=self.context.wake.set(wake);}
    #[cfg(test)]
    pub(crate) fn request(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2])->bool {
        self.request_for(id,source,coordinate,crate::scene::sources::SourceUse::Display)
    }
    pub(crate) fn request_for(&mut self,id:PortableId,source:Arc<SourceImage>,coordinate:[u32;2],use_:crate::scene::sources::SourceUse)->bool {
        let weak=Arc::downgrade(&source);
        self.enqueue(MipDecodeRequest::Image(ImageDecodeRequest {use_,id,source,coordinate}),Some(DecodeKey::Image(weak,coordinate)))
    }
    pub(crate) fn request_raster(&mut self,blob:Arc<layer_core::raster::TileBlob>,space:RgbSpace,destination:RgbSpace,scalar:bool)->bool {
        let key=DecodeKey::Raster(Arc::downgrade(&blob),space,destination,scalar);
        self.enqueue(MipDecodeRequest::Raster(RasterDecodeRequest {blob,space,destination,scalar}),Some(key))
    }
    pub(crate) fn request_paper(&mut self,blob:Arc<layer_core::raster::TileBlob>,coordinate:[u32;2])->bool {
        let key=DecodeKey::Paper(Arc::downgrade(&blob),coordinate);
        self.enqueue(MipDecodeRequest::Paper {blob,coordinate},Some(key))
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
            PreparedImageWork::Paper {blob,coordinate,..}=>Some(DecodeKey::Paper(Arc::downgrade(blob),*coordinate)),
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
    pub(crate) fn in_flight(&self)->bool {!self.outstanding.is_empty()}
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
        let context=Arc::new(DecodeContext {cache:Arc::new(layer_core::raster::DecodedTileCache::new(0)),destination:RgbSpace::Srgb,send,ready:ready.clone(),wake:Default::default()});
        MipDecodeQueue {pool:Arc::new(DecodePool {requests,capacity,epoch:Default::default()}),context,waiting:None,results,ready,outstanding,capacity}
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
        for coordinate in [[0,0],[1,0]] {
            assert!(matches!(receive.try_recv(),Ok(DecodeJob {request:MipDecodeRequest::Paper {coordinate:at,..},..}) if at==coordinate));
        }
        assert!(receive.try_recv().is_err());
        send.send(PreparedImageWork::Paper {blob:blob.clone(),coordinate:[1,0],pixels:Ok(pixels.clone())}).unwrap();ready.fetch_add(1,Ordering::AcqRel);
        assert!(matches!(queue.poll(),Some(PreparedImageWork::Paper {coordinate:[1,0],..})));
        assert!(queue.request_paper(blob.clone(),[0,0]));assert!(receive.try_recv().is_err());
        assert!(queue.request_paper(blob.clone(),[2,0]));
        assert!(matches!(receive.try_recv(),Ok(DecodeJob {request:MipDecodeRequest::Paper {coordinate:[2,0],..},..})));
        for coordinate in [[2,0],[0,0]] {
            send.send(PreparedImageWork::Paper {blob:blob.clone(),coordinate,pixels:Ok(pixels.clone())}).unwrap();ready.fetch_add(1,Ordering::AcqRel);
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
        let target=Arc::new(Mutex::new(None));let destination=Arc::downgrade(&target);
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
        let target=Arc::new(Mutex::new(None));let ready=queue.ready.clone();
        let job=DecodeJob {context:Arc::downgrade(&queue.context),request:MipDecodeRequest::Coordinates(CoordinateRequest {
            inverse:[1.,0.,0.,1.,0.,0.],size:[256;2],first:0,count:1,destination:Arc::downgrade(&target)})};
        drop(queue);assert!(callback_lifetime.upgrade().is_none());
        job.run(&mut Decoders::new());
        assert_eq!(calls.load(Ordering::Relaxed),0);assert_eq!(ready.load(Ordering::Acquire),0);
        assert!(target.lock().unwrap().is_none());
    }
    #[test]
    fn obsolete_image_decode_failure_is_visible_only_to_its_requested_source_and_context() {
        let a=layer_core::color::source::rgba8_source([1;2],|_,_|[1,2,3,255]);
        let b=layer_core::color::source::rgba8_source([1;2],|_,_|[9,8,7,255]);
        let (aid,bid)=(PortableId::random(),PortableId::random());
        let weak=Arc::downgrade(&a);let mut failures=Vec::new();
        let stale=PreparedMipTile {use_:crate::scene::sources::SourceUse::Display,id:aid,source:a.clone(),coordinate:[0;2],pixels:Err("retired A decode".into())};
        assert!(!retain_image_decode(&stale,RgbSpace::Srgb,&mut failures));
        let current=PreparedMipTile {use_:crate::scene::sources::SourceUse::Display,id:bid,source:b.clone(),coordinate:[0;2],pixels:Ok(crate::scene::sources::PreparedSourcePixels::NativeSamples(Arc::new(vec![9,8,7,255])))};
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
        let result=|id,source:&Arc<SourceImage>|PreparedImageWork::Image(PreparedMipTile {use_:crate::scene::sources::SourceUse::Display,id,source:source.clone(),coordinate:[0;2],
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
