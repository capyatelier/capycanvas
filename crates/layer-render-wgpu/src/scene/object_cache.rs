use super::*;
use crate::object_sampling::{ObjectAccumulator, SamplingPhase};
use objects::{COLLECTION_PART, CollectionJob, ObjectJob, ObjectKey};
use std::collections::{BTreeMap, HashMap, VecDeque};
type Coordinates = Arc<std::sync::Mutex<Option<Result<Arc<Vec<u8>>, String>>>>;
type CoordinateKey = ([u64; 6], [u32; 2], u32, u32);
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const BYTES: u64 = 64 * 1024 * 1024;
pub(super) type DamageKey = (u32, Option<OccurrenceHandle>);
pub(super) const INTERACTIVE_TAPS: u64 = 1 << 20;
const IDLE_TAPS: u64 = 8 << 20;
const PREFETCH: usize = 18;
pub(super) const PASS_TAPS: u64 = 1 << 16;
const REQUESTS: usize = 256;

#[derive(Clone, PartialEq, Eq)]
struct Key { object: ObjectKey, inverse: [u64; 6], size: [u32; 2] }
struct MetadataCharge { bytes:u64,total:Arc<AtomicU64> }
impl Drop for MetadataCharge {fn drop(&mut self) {self.total.fetch_sub(self.bytes,Ordering::AcqRel);}}
fn metadata_id(keys:&Arc<[ObjectKey]>)->usize {Arc::as_ptr(keys) as *const ObjectKey as usize}
struct Collection {
    _metadata:Arc<MetadataCharge>, request:CollectionJob, cursor:usize, part:u32, prefix:Option<wgpu::Texture>, spare:Option<wgpu::Texture>, result:Option<wgpu::Texture>, initialized:bool, next:bool,
    objects:layer_core::authored::Store<layer_core::authored::ImageObject>,
    layers:layer_core::authored::Store<layer_core::authored::ObjectLayer>, offset:[f64;2],
}
struct Task {
    collection:Option<Collection>,
    key: Key,
    job: ObjectJob,
    texture: Option<wgpu::Texture>,
    accumulation: Option<ObjectAccumulator>,
    tiles: Vec<[u32; 2]>,
    phase: usize,
    tile: usize,
    step: u64,
    valid: Arc<AtomicBool>,
    bounds: DocRect,
    side: f64,
    complete: bool,
    used: bool,
    last_used: u64,
    coordinates: Option<Coordinates>,
    coordinates_first: u32,
}
impl Task {
    fn bytes(&self) -> u64 { self.collection.as_ref().map_or(0,|c|c.request.keys.len() as u64*std::mem::size_of::<ObjectKey>() as u64 + c.prefix.as_ref().map_or(0,texture_bytes)+c.spare.as_ref().map_or(0,texture_bytes)+c.result.as_ref().map_or(0,texture_bytes)) + self.tiles.capacity() as u64 * 8 + self.accumulation.as_ref().map_or(0, ObjectAccumulator::storage_bytes) + self.texture.as_ref().map_or(0,texture_bytes) }
    fn result_bytes(&self) -> u64 {
        if self.collection.is_none() { return 0; }
        if self.complete { self.texture.as_ref().map_or(0,texture_bytes) } else { self.collection.as_ref().and_then(|c|c.result.as_ref()).map_or(0,texture_bytes) }
    }
}
struct InFlight(Arc<AtomicBool>);
impl Drop for InFlight { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }
struct Retired { #[cfg(not(target_arch = "wasm32"))] _task: Option<Task>, bytes: u64, charge: Arc<AtomicU64> }
impl Drop for Retired { fn drop(&mut self) { self.charge.fetch_sub(self.bytes, Ordering::AcqRel); } }
#[derive(Default)]
pub(super) struct ObjectCache { metadata:Arc<AtomicU64>, metadata_leases:HashMap<usize,std::sync::Weak<MetadataCharge>>, tasks: VecDeque<Task>, damage: BTreeMap<DamageKey, scale::Damage>, retired: Arc<AtomicU64>, retiring: Vec<Task>, in_flight: Arc<AtomicBool>, coordinates: HashMap<CoordinateKey, crate::object_image_mips::CoordinateDestination>, retains: bool, uses: u64, uniform: Option<wgpu::Buffer> }
impl ObjectCache {
    pub fn live() -> Self { Self { retains: true, ..Default::default() } }
    fn evict(&mut self, bytes: u64, tasks: usize) -> bool {
        let mut freed = 0;
        while freed < bytes || self.tasks.len() > tasks {
            let Some(index) = self.tasks.iter().enumerate().filter(|(_, task)| self.retains && task.complete && !task.used)
                .min_by_key(|(_, task)| task.last_used).map(|(index, _)| index) else { return false; };
            let task = self.tasks.remove(index).unwrap();
            freed += task.bytes();
            self.retire(task);
        }
        true
    }
    pub fn clear(&mut self) { while let Some(task) = self.tasks.pop_front() { self.retire(task); } self.damage.clear(); self.coordinates.clear(); }
    fn retire(&mut self, mut task: Task) { task.coordinates = None; self.retiring.push(task); }
    pub fn pending(&self) -> bool { self.tasks.iter().any(|task| !task.complete || !task.valid.load(Ordering::Acquire)) }
    #[cfg(test)]
    pub fn work_progress(&self) -> (usize, usize, u64) {
        (self.tasks.len(), self.tasks.iter().filter(|task|task.complete).count(), self.tasks.iter().map(|task|task.step + task.tile as u64 * 1000000 + task.phase as u64 * 1000000000 + task.collection.as_ref().map_or(0,|c|c.cursor as u64 * 1000000000000)).sum())
    }
    fn exempt(&self, task: &Task) -> bool { !task.complete || task.used || !self.retains }
    fn charged(&self) -> u64 { self.bytes() - self.tasks.iter().filter(|task| self.exempt(task)).map(Task::result_bytes).sum::<u64>() }
    pub fn bytes(&self) -> u64 { self.metadata.load(Ordering::Acquire) + self.metadata_leases.capacity() as u64*std::mem::size_of::<(usize,std::sync::Weak<MetadataCharge>)>() as u64 + self.damage.values().map(|damage| std::mem::size_of::<(u32, scale::Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>() + self.retired.load(Ordering::Acquire) + self.coordinates.values().filter_map(std::sync::Weak::upgrade).map(|coordinates| { let state = coordinates.lock().unwrap(); std::mem::size_of_val(&*state) as u64 + state.as_ref().and_then(|result| result.as_ref().ok()).map_or(0, |bytes| bytes.capacity() as u64) }).sum::<u64>() + self.coordinates.capacity() as u64 * (std::mem::size_of::<CoordinateKey>() + std::mem::size_of::<crate::object_image_mips::CoordinateDestination>() ) as u64 + (self.tasks.capacity() + self.retiring.capacity()) as u64 * std::mem::size_of::<Task>() as u64 + self.tasks.iter().chain(self.retiring.iter()).map(Task::bytes).sum::<u64>() }
    pub fn metadata_allocations(&self)->impl Iterator<Item=(usize,u64)>+'_ {self.metadata_leases.iter().filter_map(|(id,lease)|lease.upgrade().map(|lease|(*id,lease.bytes)))}
    pub fn needs_retirement(&self) -> bool { !self.retiring.is_empty() || (!self.retains && self.tasks.iter().any(|task| task.used && task.complete)) }
    pub fn flush_retired(&mut self, encoder: &crate::submission::CommandEncoder) {
        if self.retains { for task in &mut self.tasks { task.used = false; } } else {
            let mut retained = VecDeque::new();
            while let Some(task) = self.tasks.pop_front() { if task.used && task.complete { self.retire(task); } else { retained.push_back(task); } }
            self.tasks = retained;
        }
        for task in self.retiring.drain(..) {
            let bytes = task.bytes() + std::mem::size_of::<Task>() as u64;
            self.retired.fetch_add(bytes, Ordering::AcqRel);
            let retired = Retired { #[cfg(not(target_arch = "wasm32"))] _task: Some(task), bytes, charge: self.retired.clone() };
            encoder.on_submitted_work_done(move || drop(retired));
        }
    }
    pub fn reset_used(&mut self) { for task in &mut self.tasks { task.used = false; } }
    pub fn take_damage(&mut self) -> BTreeMap<DamageKey, scale::Damage> { std::mem::take(&mut self.damage) }
    pub fn cover(&mut self, request: &CollectionJob) -> (Vec<(wgpu::TextureView, DocRect)>, bool, Vec<DocRect>) {
        let window = request.bounds();
        let mut missing = vec![window];
        let mut pieces = Vec::new();
        let mut complete = true;
        for task in &mut self.tasks {
            let Some(bounds) = task.collection.as_ref().filter(|c| c.request.same_content(request)).map(|c| c.request.bounds()) else { continue; };
            if bounds.intersect(window).is_empty() { continue; }
            missing = missing.into_iter().flat_map(|rect| rect.subtract(bounds)).filter(|rect| !rect.is_empty()).collect();
            if task.complete && task.valid.load(Ordering::Acquire) {
                task.used = true; task.last_used = self.uses; self.uses += 1;
                pieces.push((task.job.output.clone(), bounds));
            } else { complete = false; }
        }
        (pieces, complete, missing)
    }
    pub fn prepare_collection(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, request: &CollectionJob) -> Result<bool, GpuRasterError> {
        if let Some(task) = self.tasks.iter().find(|task| task.collection.as_ref().is_some_and(|c| c.request.same(request))) {
            return Ok(task.complete && task.valid.load(Ordering::Acquire));
        }
        self.resolve_collection(r, scene, request).map(|_| false)
    }
    pub fn resolve_collection(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,request:&CollectionJob)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        let mut retained=VecDeque::new();
        while let Some(task)=self.tasks.pop_front() {
            if task.collection.as_ref().is_some_and(|c|c.request.blend!=request.blend || c.request.context!=request.context) {self.retire(task);}
            else {retained.push_back(task);}
        }
        self.tasks=retained;
        if let Some(task)=self.tasks.iter_mut().find(|task|task.collection.as_ref().is_some_and(|c|c.request.same(request))) {
            if task.complete && task.valid.load(Ordering::Acquire) {task.used=true;task.last_used=self.uses;self.uses+=1;return Ok(Some(task.job.output.clone()));}
            return Ok(None);
        }
        let part=request.part(0);
        let mut maximum=0;
        for index in 0..request.keys.len() {
            let job=request.child(index,part,r.empty_view.clone())?;
            r.preflight_image_sampling(job.inverse,job.size,job.nearest)?;
            maximum=maximum.max(working_set(&job)?.0);
        }
        let pixels=u64::from(part.size[0])*u64::from(part.size[1]);
        if maximum+pixels*32+(request.keys.len()+request.authored.len()) as u64*std::mem::size_of::<ObjectKey>() as u64 > BYTES {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        let metadata=request.authored.len() as u64*std::mem::size_of::<ObjectKey>() as u64;
        let new_metadata=if self.metadata_leases.get(&metadata_id(&request.authored)).and_then(std::sync::Weak::upgrade).is_some() {0} else {metadata};
        let needed=new_metadata+request.keys.len() as u64*std::mem::size_of::<ObjectKey>() as u64+std::mem::size_of::<Task>() as u64;
        if (self.charged()+needed>BYTES || self.tasks.len()>=REQUESTS) && !self.evict((self.charged()+needed).saturating_sub(BYTES),REQUESTS-1) {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        let job=request.child(0,part,r.empty_view.clone())?;
        let key=Key {object:job.key.clone(),inverse:job.inverse.map(f64::to_bits),size:job.size};
        let bounds=request.bounds();
        let offset=std::array::from_fn(|i|scene.occurrence_offset64(request.owner)[i]-scene.evaluation_offset64()[i]);
        self.metadata_leases.retain(|_,lease|lease.strong_count()!=0);
        let id=metadata_id(&request.authored);
        let lease=self.metadata_leases.entry(id).or_default();
        let metadata=lease.upgrade().unwrap_or_else(|| {
            let bytes=request.authored.len() as u64*std::mem::size_of::<ObjectKey>() as u64;
            self.metadata.fetch_add(bytes,Ordering::AcqRel);
            let charge=Arc::new(MetadataCharge {bytes,total:self.metadata.clone()});*lease=Arc::downgrade(&charge);charge
        });
        let collection=Collection {_metadata:metadata,request:request.clone(),cursor:0,part:0,prefix:None,spare:None,result:None,initialized:false,next:false,
            objects:scene.artwork().objects.clone(),layers:scene.artwork().object_layers.clone(),offset};
        self.tasks.push_back(Task {collection:Some(collection),key,job,texture:None,accumulation:None,tiles:Vec::new(),phase:0,tile:0,step:0,
            valid:Arc::new(AtomicBool::new(true)),bounds,side:request.window.side,complete:false,used:false,last_used:self.uses,coordinates:None,coordinates_first:0});
        Ok(None)
    }
    pub fn retain(&mut self, scene: SceneView<'_>, blend:layer_core::BlendSpace, context:layer_core::color::RgbSpace) {
        let mut retained=VecDeque::new();
        while let Some(mut task)=self.tasks.pop_front() {
            let keep=if let Some(collection)=&mut task.collection {
                let owner=collection.request.owner;
                let offset=std::array::from_fn(|i|scene.occurrence_offset64(owner)[i]-scene.evaluation_offset64()[i]);
                if collection.request.blend!=blend || collection.request.context!=context || (collection.request.display && !scene.visible(owner)) {false}
                else if collection.objects.same_root(&scene.artwork().objects) && collection.layers.same_root(&scene.artwork().object_layers) && collection.offset==offset {scene.object_layer(owner).is_some()}
                else {
                    let same=collection.request.authored.iter().map(|key|key.handle).eq(scene.object_layer(owner).into_iter().flat_map(|layer|layer.children.iter().copied()).filter(|handle|scene.object(*handle).is_some_and(|o|o.visible)))
                        && collection.request.authored.iter().all(|key|key.same_authored(&ObjectKey::new(scene,owner,key.handle)));
                    if same {collection.objects=scene.artwork().objects.clone();collection.layers=scene.artwork().object_layers.clone();collection.offset=offset;}
                    same
                }
            } else {scene.object_owner(task.key.object.handle).is_some_and(|owner|ObjectKey::new(scene,owner,task.key.object.handle).same_authored(&task.key.object) && (!task.job.live||scene.visible(owner)))};
            if keep {retained.push_back(task);} else {self.retire(task);}
        }
        self.tasks=retained;self.coordinates.retain(|_,token|token.strong_count()!=0);
    }
    pub fn retain_view(&mut self, bounds: DocRect, level: u32) {
        let minimum = f64::from(1u32 << level);
        let mut retained = VecDeque::new();
        while let Some(task) = self.tasks.pop_front() { if task.side >= minimum && task.side <= minimum * 64. && !task.bounds.intersect(bounds).is_empty() { retained.push_back(task); } else { self.retire(task); } }
        self.tasks = retained;
    }
    pub fn advance(&mut self, r: &mut WgpuRasterizer, scene:&mut Scene, encoder: &mut crate::submission::CommandEncoder, interactive: bool) -> Result<u64, GpuRasterError> {
        let sampler = r.scene_pipelines.objects.clone();
        if self.in_flight.load(Ordering::Acquire) { return Ok(0); }
        for task in &mut self.tasks { if task.complete && task.valid.load(Ordering::Acquire) { task.accumulation = None; task.tiles = Vec::new(); task.coordinates=None; if let Some(c)=&mut task.collection {c.prefix=None;c.spare=None;} } }
        let budget = if interactive { INTERACTIVE_TAPS } else { IDLE_TAPS };
        let mut used = 0;
        let mut index = 0;
        while used < budget && index < self.tasks.len() {
            if self.tasks[index].complete && self.tasks[index].valid.load(Ordering::Acquire) { index += 1; continue; }
            let mut task = self.tasks.remove(index).unwrap();
            let result = self.advance_task(r, scene, encoder, &sampler, &mut task, budget - used);
            let complete = task.complete;
            if complete { task.accumulation = None; task.tiles = Vec::new(); task.coordinates = None; if let Some(c) = &mut task.collection { c.prefix = None; c.spare = None; } }
            self.tasks.insert(index.min(self.tasks.len()), task);
            let taps = result?;
            used += taps;
            if complete { index += 1; } else if taps == 0 { break; }
        }
        crate::performance_trace::counter(c"Capy object sampling taps", used);
        Ok(used)
    }
    fn advance_task(&mut self, r: &mut WgpuRasterizer, scene:&mut Scene, encoder: &mut crate::submission::CommandEncoder, sampler: &crate::object_sampling::ObjectSampler, task: &mut Task, budget: u64) -> Result<u64, GpuRasterError> {
        let mut allocated = self.charged() + task.bytes() - if self.exempt(task) { task.result_bytes() } else { 0 };
        let flight = self.in_flight.clone();
        let mut recorded = false;
        let mut steps = 0;
        if !task.valid.load(Ordering::Acquire) {
            let before = task.bytes();
            if let Some(c)=&mut task.collection {c.cursor=0;c.part=0;c.initialized=false;c.next=false;task.job=c.request.child(0,c.request.part(0),task.job.output.clone())?;task.key.object=task.job.key.clone();task.key.inverse=task.job.inverse.map(f64::to_bits);}
            task.complete = false; task.used = false; task.accumulation = None; task.coordinates = None; task.coordinates_first = 0; task.phase = 0; task.tile = 0; task.step = 0; task.valid = Arc::new(AtomicBool::new(true));
            allocated -= before - task.bytes();
        }
        if let Some(c)=&mut task.collection && c.next {
            c.next=false;
            task.job=c.request.child(c.cursor,c.request.part(c.part),task.texture.as_ref().unwrap().create_view(&Default::default()))?;
            task.key.object=task.job.key.clone();task.key.inverse=task.job.inverse.map(f64::to_bits);
            let before=task.bytes();task.accumulation=None;task.tiles=Vec::new();task.phase=0;task.tile=0;task.step=0;task.coordinates_first=0;
            allocated-=before-task.bytes();
        }
        if task.accumulation.is_none() {
            let (working, bounds, pages) = working_set(&task.job)?;
            let output = task.texture.as_ref().map_or(0, texture_bytes);
            let collection_bytes=if let Some(c)=&task.collection {u64::from(task.key.size[0])*u64::from(task.key.size[1])*16*(u64::from(c.prefix.is_none())+u64::from(c.spare.is_none()))} else {0};
            let extra = working.saturating_sub(output + task.tiles.capacity() as u64 * 8)+collection_bytes;
            if allocated + extra > BYTES {
                if self.evict(allocated + extra - BYTES, usize::MAX) {}
                else if self.retired.load(Ordering::Acquire) == 0 && self.retiring.is_empty() { return Err(GpuRasterError::SourceWorkingSetExceeded); }
                else { return Ok(0); }
            }
            if task.texture.is_none() {
                let (texture, output) = create_color_target(&r.device, task.key.size, "private canonical object result");
                task.texture = Some(texture); task.job.output = output;
            }
            if let Some(c)=&mut task.collection {
                if c.prefix.is_none() {c.prefix=Some(create_color_target(&r.device,task.key.size,"private object collection prefix").0);}
                if c.spare.is_none() {c.spare=Some(create_color_target(&r.device,task.key.size,"private object collection conversion").0);}
                if c.result.is_none() && c.request.parts()>1 {c.result=Some(create_color_target(&r.device,c.request.window.size,"private object collection window").0);}
                if !c.initialized {
                    scene.jobs.push(Job::Clear(c.prefix.as_ref().unwrap().create_view(&Default::default()),wgpu::Color::TRANSPARENT));
                    scene.encode_jobs(r,encoder)?;c.initialized=true;
                }
            }
            let uniform = self.uniform.get_or_insert_with(|| crate::object_sampling::ObjectSampler::uniform(&r.device));
            let acc = sampler.create(&r.device,crate::object_sampling::SamplingRequest {inverse:task.job.inverse,size:task.job.size,nearest:task.job.nearest},&task.job.output,&r.empty_view,uniform)?;
            task.tiles.clear(); task.tiles.reserve_exact(pages); task.tiles.extend(page_coordinates(bounds));
            sampler.initialize(encoder, &acc); task.accumulation = Some(acc);
            track_batch(encoder, &flight); recorded = true;
        }
        let guard = crate::submission::CacheWrite::shared(task.valid.clone()); guard.track(encoder);
        if task.job.nearest && !task.accumulation.as_ref().unwrap().nearest_coordinates_ready() {
            let length = task.key.size[0].checked_mul(task.key.size[1]).ok_or(GpuRasterError::SizeOverflow)?;
            let count = (length - task.coordinates_first).min(65536);
            let key = (task.key.inverse, task.key.size, task.coordinates_first, count);
            let token = self.coordinates.entry(key).or_default();
            let coordinates = token.upgrade().unwrap_or_else(|| {
                let coordinates = Arc::new(std::sync::Mutex::new(None));
                *token = Arc::downgrade(&coordinates); coordinates
            });
            task.coordinates = Some(coordinates.clone());
            let prepared = coordinates.lock().unwrap().clone();
            let Some(prepared) = prepared else {
                r.request_nearest_coordinates(task.job.inverse, task.job.size, task.coordinates_first, count, Arc::downgrade(&coordinates))?;
                return Ok(steps);
            };
            let prepared = prepared.map_err(GpuRasterError::Color)?;
            sampler.set_nearest_coordinates(encoder, &mut r.uploads, task.accumulation.as_ref().unwrap(), task.coordinates_first, &prepared)?;
            if !recorded { track_batch(encoder, &flight); recorded = true; }
            task.coordinates_first += count;
            steps += u64::from(count);
            if !task.accumulation.as_ref().unwrap().nearest_coordinates_ready() { return Ok(steps); }
        }
        if !recorded { track_batch(encoder, &flight); }
        let external = {
            #[cfg(target_arch = "wasm32")] { r.browser_image_decoder.is_some() }
            #[cfg(not(target_arch = "wasm32"))] { false }
        };
        if task.phase == 1 && (task.job.live || external) {
            for &coordinate in task.tiles.iter().skip(task.tile).take(PREFETCH) {
                if r.source_tiles.borrow().prepared_view(&task.job.source, coordinate).is_none()
                    && !r.request_image_decode(task.key.object.image, task.job.source.clone(), coordinate)? { break; }
            }
        }
        while steps < budget {
            let acc = task.accumulation.as_ref().unwrap();
            let encoded = match task.phase {
                0 => sampler.encode_step(&r.device, encoder, &mut r.uploads, acc, &SamplingPhase::Denominator { extent: task.job.source.extent }, task.step)?,
                1 if task.tile < task.tiles.len() => {
                    let coordinate = task.tiles[task.tile];
                    let input = if task.job.live || external {
                        let Some(input) = objects::decode_live(r, task.key.object.image, &task.job.source, coordinate)? else { break; };
                        input
                    } else { objects::decode(r, encoder, &task.job.source, coordinate)? };
                    let origin = coordinate.map(|v| v * PAGE_SIZE);
                    let extent = std::array::from_fn(|axis| (task.job.source.extent[axis] - origin[axis]).min(PAGE_SIZE));
                    sampler.encode_step(&r.device, encoder, &mut r.uploads, acc, &SamplingPhase::Contribute { source: &input.view, origin: origin.map(|v| v as i32), extent }, task.step)?
                }
                1 => None,
                _ => sampler.encode_step(&r.device, encoder, &mut r.uploads, acc, &SamplingPhase::Normalize, task.step)?,
            };
            if let Some(taps) = encoded { task.step += 1; steps += taps.max(PASS_TAPS); continue; }
            task.step = 0;
            if task.phase == 1 && task.tile < task.tiles.len() { task.tile += 1; }
            else if task.phase < 2 { task.phase += 1; }
            else {
                if let Some(c)=&mut task.collection {
                    let child=task.job.output.clone();
                    let prefix=c.prefix.as_ref().unwrap().create_view(&Default::default());
                    let spare=c.spare.as_ref().unwrap().create_view(&Default::default());
                    let mut data=[0.;32];data[..4].copy_from_slice(&[0.,0.,task.key.size[0] as f32,task.key.size[1] as f32]);
                    data[4..8].copy_from_slice(&[task.key.size[0] as f32,task.key.size[1] as f32,0.,0.]);
                    data[8]=1.;data[9]=1.;data[31]=if c.request.blend==layer_core::BlendSpace::Perceptual {Convert::Encode.code()} else {0.};
                    scene.jobs.push(Job::Draw {target:spare.clone(),sources:[child.clone(),r.empty_view.clone(),r.empty_view.clone()],data,over:false,clip:None,source_target:None});
                    data[8]=4.;data[10]=crate::blend_code(layer_core::LayerBlend::Normal,&r.device,c.request.blend) as f32;data[31]=0.;
                    scene.jobs.push(Job::Draw {target:child,sources:[spare,prefix,r.empty_view.clone()],data,over:false,clip:None,source_target:None});
                    scene.encode_jobs(r,encoder)?;
                    steps+=2*u64::from(task.key.size[0])*u64::from(task.key.size[1]);
                    std::mem::swap(&mut task.texture,&mut c.prefix);
                    c.cursor+=1;
                    if c.cursor<c.request.keys.len() {
                        c.next=true;break;
                    }
                    if let Some(result)=&c.result {
                        let part=c.request.part(c.part);
                        let columns=c.request.window.size[0].div_ceil(COLLECTION_PART);
                        let offset=[c.part%columns*part.size[0],c.part/columns*part.size[1]];
                        scene.jobs.push(Job::Copy {source:c.prefix.clone().unwrap(),source_origin:[0;2],destination:result.clone(),origin:offset,
                            width:part.size[0].min(c.request.window.size[0]-offset[0]),height:part.size[1].min(c.request.window.size[1]-offset[1])});
                        scene.encode_jobs(r,encoder)?;
                        c.part+=1;
                        if c.part<c.request.parts() {c.cursor=0;c.initialized=false;c.next=true;break;}
                        task.texture=c.result.take();
                    } else { std::mem::swap(&mut task.texture,&mut c.prefix); }
                    task.job.output=task.texture.as_ref().unwrap().create_view(&Default::default());
                }
                task.complete = true;
                let level = task.side.max(1.).log2().round() as u32;
                self.damage.entry((level, task.collection.as_ref().map(|c| c.request.owner))).or_insert(scale::Damage::EMPTY).regions.push(task.bounds.in_frame(r.document_extent));
                break;
            }
        }
        Ok(steps)
    }
}

fn working_set(job: &ObjectJob) -> Result<(u64, PixelRect, usize), GpuRasterError> {
    let bounds = crate::object_sampling::ObjectSampler::source_bounds(job.inverse, job.size, job.nearest, job.source.extent)?;
    let pages = u64::from(bounds.max_x().div_ceil(PAGE_SIZE) - bounds.min_x() / PAGE_SIZE)
        * u64::from(bounds.max_y().div_ceil(PAGE_SIZE) - bounds.min_y() / PAGE_SIZE);
    let pixels = u64::from(job.size[0]) * u64::from(job.size[1]);
    let bytes = crate::object_sampling::ObjectSampler::working_bytes(job.inverse, job.size, job.nearest)?
        + pixels * 16 + if job.nearest { pixels.min(65536) * 8 } else { 0 } + pages * 8;
    Ok((bytes, bounds, usize::try_from(pages).map_err(|_| GpuRasterError::SourceWorkingSetExceeded)?))
}

fn track_batch(encoder: &crate::submission::CommandEncoder, flight: &Arc<AtomicBool>) {
    flight.store(true, Ordering::Release);
    let flight = InFlight(flight.clone()); encoder.on_submitted_work_done(move || drop(flight));
}
