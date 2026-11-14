use super::*;
use crate::object_sampling::{ObjectAccumulator, SamplingPhase};
use objects::{COLLECTION_PART, CollectionJob, ObjectJob, ObjectKey};
use std::collections::{BTreeMap, HashMap, VecDeque};
type Coordinates = Arc<std::sync::Mutex<crate::object_image_mips::CoordinateSlot>>;
type CoordinateKey = ([u64; 6], [u32; 2], u32, u32);
use std::sync::atomic::{AtomicBool, Ordering};
use crate::residency::{Charge, Lease, Reservation, Tier};

pub(super) type DamageKey = (u32, Option<OccurrenceHandle>);
pub(super) const INTERACTIVE_TAPS: u64 = 1 << 20;
pub(super) const IDLE_TAPS: u64 = 8 << 20;
const PREFETCH: usize = 18;
pub(super) const PASS_TAPS: u64 = 1 << 16;

#[derive(Clone, PartialEq, Eq)]
struct Key { object: ObjectKey, inverse: [u64; 6], size: [u32; 2] }
struct Collection {
    _metadata:Arc<object_spatial::KeyLease>, request:CollectionJob, part:u32, spare:Option<wgpu::Texture>, result:Option<wgpu::Texture>, next:bool,
}
struct TaskPin(std::sync::Mutex<Charge>);
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
    used: u64,
    coordinates: Option<Coordinates>,
    coordinates_first: u32,
    inputs:sources::AcceptedInputs,
    gpu: Charge,
    ram: Charge,
    reserved: Option<Reservation>,
    lease: Arc<Lease>,
    pin: std::sync::Weak<TaskPin>,
    hold: Option<Arc<TaskPin>>,
}
impl Task {
    fn gpu_bytes(&self) -> u64 { self.collection.as_ref().map_or(0,|c|c.spare.as_ref().map_or(0,texture_bytes)+c.result.as_ref().map_or(0,texture_bytes)) + self.accumulation.as_ref().map_or(0, ObjectAccumulator::owned_gpu_bytes) + self.texture.as_ref().map_or(0,texture_bytes) }
    fn ram_bytes(&self) -> u64 { self.collection.as_ref().map_or(0,|c|c.request.keys.len() as u64*std::mem::size_of::<ObjectKey>() as u64) + self.tiles.capacity() as u64 * 8 + self.inputs.cpu_bytes() + self.accumulation.as_ref().map_or(0, ObjectAccumulator::cpu_bytes) }
    fn pin(&mut self, tier: &Arc<Tier>) -> Arc<TaskPin> {
        self.pin.upgrade().unwrap_or_else(|| {
            let pin = Arc::new(TaskPin(std::sync::Mutex::new(tier.charge(self.gpu.bytes()))));
            self.pin = Arc::downgrade(&pin);
            let guard: Arc<dyn Send + Sync> = pin.clone();
            self.lease.guard(Arc::downgrade(&guard));
            pin
        })
    }
    fn offer(&self) {
        let pages = u64::from(self.key.size[0].div_ceil(PAGE_SIZE)) * u64::from(self.key.size[1].div_ceil(PAGE_SIZE));
        let done = if !self.valid.load(Ordering::Acquire) { 0 } else if self.complete { pages } else { self.tile as u64 };
        let bytes = self.gpu_bytes() + self.reserved.as_ref().map_or(0, Reservation::bytes);
        self.lease.offer(bytes, crate::residency::rebuild_value(done * PASS_TAPS, bytes));
    }
    fn unfinished(&self) -> bool { !self.complete || !self.valid.load(Ordering::Acquire) }
    fn runnable(&self) -> bool {
        if !self.valid.load(Ordering::Acquire) { return true; }
        if self.complete { return false; }
        let Some(accumulation) = &self.accumulation else { return true; };
        if self.collection.as_ref().is_some_and(|c| c.next) { return true; }
        if !accumulation.nearest_coordinates_ready() { return self.coordinates.as_ref().is_none_or(|slot| slot.lock().unwrap().result.is_some()); }
        self.phase != 1 || self.tile >= self.tiles.len() || !self.inputs.holds_image(&self.job.source, self.tiles[self.tile])
            || sources::accepted_image(&self.inputs, &self.job.source, self.tiles[self.tile]).map_or(true, |read| read.ready())
    }
    fn use_in(&mut self, frame: u64, tier: &Arc<Tier>) -> Option<Arc<TaskPin>> { (self.used != frame).then(|| { self.used = frame; self.pin(tier) }) }
    fn sync(&mut self, tier: &Arc<Tier>, retains: bool) {
        let gpu = self.gpu_bytes(); self.gpu.resize(gpu); self.ram.resize(self.ram_bytes());
        let holds = self.complete && !retains;
        if !holds { self.hold = None; } else if self.hold.is_none() { self.hold = Some(self.pin(tier)); }
        if let Some(pin) = self.pin.upgrade() { pin.0.lock().unwrap().resize(gpu); }
        self.offer();
    }
    fn release(&mut self) {
        self.hold = None; self.reserved = None;
        if let Some(pin) = self.pin.upgrade() { pin.0.lock().unwrap().resize(0); }
        self.gpu.resize(0); self.ram.resize(0);
    }
}
impl Drop for Task { fn drop(&mut self) { self.release(); } }
struct InFlight(Arc<AtomicBool>);
impl Drop for InFlight { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }
struct Retired { #[cfg(not(target_arch = "wasm32"))] _task: Task, _charges: [Charge; 3] }
pub(super) struct ObjectCache {
    keys: object_spatial::KeyLeases, gpu: Arc<Tier>, working: Arc<Tier>, pinned: Arc<Tier>, ram: Arc<Tier>, retired: Arc<Tier>, retired_gpu: Arc<Tier>,
    queues: Charge, damage_bytes: Charge, coordinate_slots: Charge, frame: u64, used: Vec<Arc<TaskPin>>,
    tasks: VecDeque<Task>, damage: BTreeMap<DamageKey, scale::Damage>, retiring: Vec<Task>, in_flight: Arc<AtomicBool>,
    coordinates: HashMap<CoordinateKey, crate::object_image_mips::CoordinateDestination>, retains: bool, uniform: Option<(wgpu::Buffer, Charge)>, requester: Class,
    evicted: Arc<AtomicBool>,
}
impl ObjectCache {
    pub fn new(tiers: &super::Tiers, keys: object_spatial::KeyLeases, retains: bool) -> Self {
        let [gpu, retired_gpu] = [(); 2].map(|()| Tier::within(&tiers.objects));
        let [ram, retired] = [(); 2].map(|()| Tier::within(&tiers.ram));
        let working = Tier::within(&gpu);
        Self { keys, pinned: Tier::within(&tiers.pinned), queues: ram.charge(0), damage_bytes: ram.charge(0), coordinate_slots: ram.charge(0), gpu, working, ram, retired, retired_gpu,
            frame: 1, used: Vec::new(), tasks: VecDeque::new(), damage: BTreeMap::new(), retiring: Vec::new(), in_flight: Default::default(),
            coordinates: HashMap::new(), retains, uniform: None, requester: Class::Visible, evicted: Default::default() }
    }
    pub(super) fn set_requester(&mut self, class: Class) { self.requester = class; }
    fn uniform_bytes(&self) -> u64 { self.uniform.as_ref().map_or(0, |(uniform, _)| uniform.size()) }
    fn started(&self, task: &Task) -> bool { Arc::ptr_eq(task.gpu.tier(), &self.working) }
    fn sync_queues(&mut self) { self.queues.resize((self.tasks.capacity() + self.retiring.capacity()) as u64 * std::mem::size_of::<Task>() as u64); }
    fn prepare_inputs(&mut self,r:&mut WgpuRasterizer,scene:&mut Scene,task:&mut Task)->Result<(),GpuRasterError> {
        let mut wanted=sources::Inputs::default();
        for &coordinate in task.tiles.iter().skip(task.tile).take(PREFETCH) {wanted.image(&task.job.source,coordinate,r.device.working_space());}
        task.inputs.retain(&wanted);
        let coordinates=task.tiles.iter().skip(task.tile).take(PREFETCH).copied().collect::<Vec<_>>();
        for coordinate in coordinates {
            let mut inputs=sources::Inputs::default();inputs.image(&task.job.source,coordinate,r.device.working_space());
            if task.inputs.contains(&inputs) {continue;}
            let mut context=(&mut *self,&mut *scene,&mut *task);
            let accepted=inputs.accept(r,&mut context,|(cache,scene,task),r,inputs| {
                task.sync(&cache.pinned,cache.retains);
                let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
                r.admit_display(Some(scene),Class::Visible,bytes).ok()
            },|_,_|Ok(()),|(cache,_,task),_,_| {task.sync(&cache.pinned,cache.retains);Ok(())});
            match accepted {
                Ok(Some(((),inputs)))=>task.inputs.extend(inputs)?,
                Ok(None)=>break,
                Err(error) if error.is_preparation_pending()=>break,
                Err(error)=>return Err(error),
            }
        }
        task.inputs.prepare(r)?;Ok(())
    }
    pub fn retire_evicted(&mut self) -> u64 {
        if !self.evicted.swap(false, Ordering::AcqRel) { return 0; }
        let mut bytes = 0;
        for task in std::mem::take(&mut self.tasks) {
            if task.lease.evicted() && task.pin.strong_count() == 0 { bytes += task.gpu_bytes(); self.retire(task); }
            else { task.lease.restore(); task.offer(); self.tasks.push_back(task); }
        }
        self.sync_queues();
        bytes
    }
    #[cfg(test)]
    pub fn evict_to(&mut self, gpu: u64) -> u64 {
        let used = self.gpu.used().saturating_sub(self.retiring.iter().map(|task| task.gpu.bytes()).sum::<u64>());
        self.gpu.heap().evict(used.saturating_sub(gpu), Class::Fallback);
        self.retire_evicted()
    }
    pub fn clear(&mut self) { while let Some(task) = self.tasks.pop_front() { self.retire(task); } self.damage.clear(); self.damage_bytes.resize(0); self.coordinates.clear(); }
    fn retire(&mut self, mut task: Task) { task.coordinates = None; task.reserved = None; task.lease.hold(); task.hold = Some(task.pin(&self.pinned)); self.retiring.push(task); self.sync_queues(); }
    pub fn pending(&self) -> bool { self.tasks.iter().any(Task::unfinished) }
    pub fn demanded(&self) -> bool { self.tasks.iter().any(|task| task.unfinished() && task.lease.recent()) }
    pub fn waiting(&self) -> bool { self.pending() && !self.in_flight.load(Ordering::Acquire) && !self.tasks.iter().any(Task::runnable) }
    pub fn required_sources(&self, tiles: &mut Vec<sources::RequiredSource>) -> Result<(), GpuRasterError> {
        let Some(task) = self.tasks.iter().find(|task| task.unfinished()) else { return Ok(()); };
        let restarted = !task.valid.load(Ordering::Acquire);
        let next = task.collection.as_ref().filter(|collection| restarted || collection.next)
            .map(|collection| collection.request.child(0, collection.request.part(if restarted { 0 } else { collection.part }), task.job.output.clone())).transpose()?;
        let job = next.as_ref().unwrap_or(&task.job);
        if task.tiles.is_empty() || restarted || next.is_some() {
            let (_, bounds, _) = working_set(job)?;
            tiles.extend(page_coordinates(bounds).take(PREFETCH).map(|coordinate| (job.source.clone(), coordinate)));
        } else {
            tiles.extend(task.tiles.iter().skip(task.tile).take(PREFETCH).map(|&coordinate| (task.job.source.clone(), coordinate)));
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn request_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.iter().filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    #[cfg(test)]
    pub fn completed_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.iter().filter(|task| task.complete).filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    #[cfg(test)]
    pub fn work_progress(&self) -> (usize, usize, u64) {
        (self.tasks.len(), self.tasks.iter().filter(|task|task.complete).count(), self.tasks.iter().map(|task|task.step + task.tile as u64 * 1000000 + task.phase as u64 * 1000000000 + task.collection.as_ref().map_or(0,|c|c.part as u64 * 1000000000000)).sum())
    }
    #[cfg(test)]
    pub fn bytes(&self) -> u64 { let [ram, gpu] = self.memory_bytes(); ram + gpu }
    #[cfg(test)]
    pub fn memory_bytes(&self) -> [u64; 2] { [self.ram.used() + self.retired.used(), self.gpu.used() + self.retired_gpu.used()] }
    #[cfg(test)]
    pub fn walked_bytes(&self) -> [u64; 3] {
        let tasks = || self.tasks.iter().chain(self.retiring.iter());
        let gpu = self.uniform_bytes() + self.retired_gpu.used() + tasks().map(|task| task.gpu_bytes() + task.reserved.as_ref().map_or(0, Reservation::bytes)).sum::<u64>();
        let ram = self.damage.values().map(|damage| std::mem::size_of::<(u32, scale::Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>()
            + self.retired.used() + self.coordinates.values().filter_map(std::sync::Weak::upgrade).map(|coordinates| { let state = coordinates.lock().unwrap(); std::mem::size_of::<Option<crate::object_image_mips::CoordinateResult>>() as u64 + state.result.as_ref().and_then(|result| result.as_ref().ok()).map_or(0, |bytes| bytes.capacity() as u64) }).sum::<u64>()
            + self.coordinates.capacity() as u64 * (std::mem::size_of::<CoordinateKey>() + std::mem::size_of::<crate::object_image_mips::CoordinateDestination>()) as u64
            + (self.tasks.capacity() + self.retiring.capacity()) as u64 * std::mem::size_of::<Task>() as u64 + tasks().map(Task::ram_bytes).sum::<u64>();
        let pinned = self.retired_gpu.used() + self.retiring.iter().map(Task::gpu_bytes).sum::<u64>()
            + self.tasks.iter().filter(|task| task.used == self.frame || task.pin.strong_count() > usize::from(task.hold.is_some()) || !self.retains).map(Task::gpu_bytes).sum::<u64>();
        [ram, gpu, pinned]
    }
    #[cfg(test)]
    pub fn pinned_gpu_bytes(&self) -> u64 { self.pinned.used() }
    pub fn needs_retirement(&self) -> bool { !self.retiring.is_empty() || (!self.retains && self.tasks.iter().any(|task| task.used == self.frame && task.complete)) }
    pub fn flush_retired(&mut self, encoder: &crate::submission::CommandEncoder) {
        if self.retains { self.reset_used(); } else {
            let mut retained = VecDeque::new();
            while let Some(task) = self.tasks.pop_front() { if task.used == self.frame && task.complete { self.retire(task); } else { retained.push_back(task); } }
            self.tasks = retained; self.sync_queues();
        }
        self.flush_retiring(encoder);
    }
    pub fn flush_retiring(&mut self,encoder:&crate::submission::CommandEncoder) {
        for mut task in self.retiring.drain(..) {
            let gpu = task.gpu_bytes();
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = task.ram_bytes() + std::mem::size_of::<Task>() as u64;
            #[cfg(target_arch = "wasm32")]
            let bytes = 0;
            task.release();
            let retired = Retired { #[cfg(not(target_arch = "wasm32"))] _task: task, _charges: [self.retired.charge(bytes), self.retired_gpu.charge(gpu), self.pinned.charge(gpu)] };
            encoder.retire(retired);
        }
    }
    pub fn reset_used(&mut self) { self.frame += 1; self.used.clear(); }
    pub fn take_damage(&mut self) -> BTreeMap<DamageKey, scale::Damage> { self.damage_bytes.resize(0); std::mem::take(&mut self.damage) }
    pub fn cover(&mut self, request: &CollectionJob, mut inputs:Option<&mut sources::AcceptedInputs>) -> (Vec<(wgpu::TextureView, DocRect)>, bool, Vec<DocRect>) {
        let window = request.bounds();
        let mut missing = vec![window];
        let mut pieces = Vec::new();
        let mut complete = true;
        for task in &mut self.tasks {
            let Some(bounds) = task.collection.as_ref().filter(|c| c.request.same_content(request)).map(|c| c.request.bounds()) else { continue; };
            if bounds.intersect(window).is_empty() { continue; }
            if let Some(inputs)=inputs.as_deref_mut() {inputs.hold(task.pin(&self.pinned));}
            missing = missing.into_iter().flat_map(|rect| rect.subtract(bounds)).filter(|rect| !rect.is_empty()).collect();
            if task.complete && task.valid.load(Ordering::Acquire) {
                self.used.extend(task.use_in(self.frame, &self.pinned)); task.offer();
                pieces.push((task.job.output.clone(), bounds));
            } else { complete = false; }
        }
        (pieces, complete, missing)
    }
    pub(super) fn stamp(&mut self, request: &CollectionJob, class: Class) {
        let window = request.bounds();
        for task in &mut self.tasks {
            if task.collection.as_ref().is_some_and(|c| c.request.same_content(request) && !c.request.bounds().intersect(window).is_empty()) { task.lease.stamp(class); }
        }
    }
    pub fn prepare_collection(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, request: &CollectionJob, inputs:Option<&mut sources::AcceptedInputs>) -> Result<bool, GpuRasterError> {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.collection.as_ref().is_some_and(|c| c.request.same(request))) {
            task.lease.stamp(self.requester);
            if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
            let ready = task.complete && task.valid.load(Ordering::Acquire);
            if self.retains && ready { self.used.extend(task.use_in(self.frame, &self.pinned)); }
            return Ok(ready);
        }
        self.resolve_collection(r, scene, request, inputs).map(|_| false)
    }
    pub fn resolve_collection(&mut self,r:&WgpuRasterizer,_scene:SceneView<'_>,request:&CollectionJob,inputs:Option<&mut sources::AcceptedInputs>)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        let mut retained=VecDeque::new();
        while let Some(task)=self.tasks.pop_front() {
            if task.collection.as_ref().is_some_and(|c|c.request.blend!=request.blend || c.request.context!=request.context) {self.retire(task);}
            else {retained.push_back(task);}
        }
        self.tasks=retained;self.sync_queues();
        if let Some(task)=self.tasks.iter_mut().find(|task|task.collection.as_ref().is_some_and(|c|c.request.same(request))) {
            task.lease.stamp(self.requester);
            if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
            if task.complete && task.valid.load(Ordering::Acquire) {self.used.extend(task.use_in(self.frame,&self.pinned));task.offer();return Ok(Some(task.job.output.clone()));}
            return Ok(None);
        }
        self.window_bytes(r,request)?;
        let task=self.task(r,request,None,self.requester)?;
        self.push(task,inputs);
        Ok(None)
    }
    fn task(&self,r:&WgpuRasterizer,request:&CollectionJob,reserved:Option<Reservation>,class:Class)->Result<Task,GpuRasterError> {
        let job=request.child(0,request.part(0),r.empty_view.clone())?;
        let key=Key {object:job.key.clone(),inverse:job.inverse.map(f64::to_bits),size:job.size};
        let lease=self.gpu.heap().lease(&self.evicted);lease.stamp(class);
        let collection=Collection {_metadata:self.keys.lease(&request.authored),request:request.clone(),part:0,spare:None,result:None,next:false};
        Ok(Task {collection:Some(collection),key,job,texture:None,accumulation:None,tiles:Vec::new(),phase:0,tile:0,step:0,
            valid:Arc::new(AtomicBool::new(true)),bounds:request.bounds(),side:request.window.side,complete:false,used:0,coordinates:None,coordinates_first:0,inputs:Default::default(),
            gpu:self.gpu.charge(0),ram:self.ram.charge(0),reserved,lease,pin:std::sync::Weak::new(),hold:None})
    }
    fn push(&mut self,mut task:Task,inputs:Option<&mut sources::AcceptedInputs>) {
        task.sync(&self.pinned,self.retains);
        if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
        self.tasks.push_back(task);self.sync_queues();
    }
    fn window_bytes(&self,r:&WgpuRasterizer,request:&CollectionJob)->Result<u64,GpuRasterError> {
        let part=request.part(0);
        let mut maximum=0;
        for index in 0..request.keys.len() {
            let job=request.child(index,part,r.empty_view.clone())?;
            r.preflight_image_sampling(job.inverse,job.size,job.nearest)?;
            maximum=maximum.max(working_set(&job)?.0);
        }
        let pixels=u64::from(part.size[0])*u64::from(part.size[1]);
        let result = if request.parts() > 1 { request.window.size.map(u64::from).into_iter().product::<u64>() * 16 } else { 0 };
        let bytes=maximum+pixels*16+result;
        if bytes > r.memory.display_limit() {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        Ok(bytes)
    }
    pub(super) fn admit(&mut self,r:&WgpuRasterizer,class:Class,requests:&[CollectionJob],mut inputs:Option<&mut sources::AcceptedInputs>)->Result<Option<crate::residency::Blocked>,GpuRasterError> {
        let mut missing=Vec::<(CollectionJob,u64)>::new();
        for request in requests {
            if let Some(task)=self.tasks.iter_mut().find(|task|task.collection.as_ref().is_some_and(|c|c.request.same(request))) {
                task.lease.stamp(class);
                if let Some(inputs)=inputs.as_deref_mut() {inputs.hold(task.pin(&self.pinned));}
            } else if !missing.iter().any(|(known,_)|known.same(request)) {missing.push((request.clone(),self.window_bytes(r,request)?));}
        }
        if missing.is_empty() {return Ok(None);}
        let bytes=missing.iter().map(|(_,bytes)|bytes).sum::<u64>();
        let admission=class.min(if self.working.used()==0 || !self.retains {Class::Visible} else {Class::Prefetch});
        let mut reservation=match r.admit_tier(None,&self.working,admission,bytes) {Ok(reservation)=>reservation,Err(blocked)=>return Ok(Some(blocked))};
        for (request,bytes) in missing {
            let reserved=reservation.split(bytes).ok_or(GpuRasterError::Invariant("an admitted window exceeds its unit reservation"))?;
            let task=self.task(r,&request,Some(reserved),class)?;
            self.push(task,inputs.as_deref_mut());
        }
        Ok(None)
    }
    pub fn retain(&mut self, scene: SceneView<'_>, blend:layer_core::BlendSpace, context:layer_core::color::RgbSpace) {
        let mut retained=VecDeque::new();
        while let Some(mut task)=self.tasks.pop_front() {
            let keep=if let Some(collection)=&mut task.collection {
                let owner=collection.request.owner;
                collection.request.blend==blend && collection.request.context==context && (!collection.request.display || scene.visible(owner))
                    && collection.request.authored.iter().map(|key|key.handle).eq(scene.object_handle(owner))
                    && collection.request.authored.iter().all(|key|key.same_authored(&ObjectKey::new(scene,owner,key.handle)))
            } else {scene.object_owner(task.key.object.handle).is_some_and(|owner|ObjectKey::new(scene,owner,task.key.object.handle).same_authored(&task.key.object) && (!task.job.live||scene.visible(owner)))};
            if keep {retained.push_back(task);} else {self.retire(task);}
        }
        self.tasks=retained;self.sync_queues();self.coordinates.retain(|_,token|token.strong_count()!=0);self.sync_coordinate_slots();
    }
    fn sync_coordinate_slots(&mut self) {
        self.coordinate_slots.resize(self.coordinates.capacity() as u64*(std::mem::size_of::<CoordinateKey>()+std::mem::size_of::<crate::object_image_mips::CoordinateDestination>()) as u64);
    }
    #[cfg(test)]
    pub fn advance(&mut self,r:&mut WgpuRasterizer,scene:&mut Scene,encoder:&mut crate::submission::CommandEncoder,budget:&mut WorkBudget)->Result<u64,GpuRasterError> {
        let used=self.advance_with_memory(r,scene,encoder,budget.allowance(Class::Prefetch))?;
        budget.spend(used);Ok(used)
    }
    pub fn advance_with_memory(&mut self, r: &mut WgpuRasterizer, scene:&mut Scene, encoder: &mut crate::submission::CommandEncoder, allowance: u64) -> Result<u64, GpuRasterError> {
        let sampler = r.scene_pipelines.objects.clone();
        if self.tasks.iter().any(Task::unfinished)
            && !r.compilation.require(sampler.pipelines().into_iter().chain([&r.scene_pipelines.constant.2]), startup::DOCUMENT) { return Ok(0); }
        if self.in_flight.load(Ordering::Acquire) { return Ok(0); }
        self.tasks.make_contiguous().sort_by_key(|task| { let demand = task.lease.demand(); (demand.is_none(), demand) });
        let mut used = 0;
        let mut index = 0;
        while used < allowance && index < self.tasks.len() {
            if self.tasks[index].complete && self.tasks[index].valid.load(Ordering::Acquire) { index += 1; continue; }
            let mut task = self.tasks.remove(index).unwrap();
            let result = self.advance_task(r, scene, encoder, &sampler, &mut task, allowance - used);
            let complete = task.complete;
            if complete {
                task.accumulation = None; task.tiles = Vec::new(); task.coordinates = None;task.inputs=Default::default(); if let Some(c) = &mut task.collection { c.spare = None; }
                task.gpu = self.gpu.charge(task.gpu_bytes());
            }
            task.sync(&self.pinned, self.retains);
            self.tasks.insert(index.min(self.tasks.len()), task);self.sync_queues();
            let taps = result?;
            used += taps;
            if complete || taps==0 {index+=1;}
        }
        crate::performance_trace::counter(c"Capy object sampling taps", used);
        Ok(used)
    }
    fn advance_task(&mut self, r: &mut WgpuRasterizer, scene:&mut Scene, encoder: &mut crate::submission::CommandEncoder, sampler: &crate::object_sampling::ObjectSampler, task: &mut Task, budget: u64) -> Result<u64, GpuRasterError> {
        let flight = self.in_flight.clone();
        let mut recorded = false;
        let mut steps = 0;
        let mut previous = None;
        if !task.valid.load(Ordering::Acquire) {
            if let Some(c)=&mut task.collection {c.part=0;c.next=false;task.job=c.request.child(0,c.request.part(0),task.job.output.clone())?;task.key.object=task.job.key.clone();task.key.inverse=task.job.inverse.map(f64::to_bits);}
            task.complete = false; task.used = 0; previous = task.accumulation.take(); task.coordinates = None; task.coordinates_first = 0; task.phase = 0; task.tile = 0;task.inputs=Default::default(); task.step = 0; task.valid = Arc::new(AtomicBool::new(true));
        }
        if let Some(c)=&mut task.collection && c.next {
            c.next=false;
            task.job=c.request.child(0,c.request.part(c.part),task.texture.as_ref().unwrap().create_view(&Default::default()))?;
            task.key.object=task.job.key.clone();task.key.inverse=task.job.inverse.map(f64::to_bits);
            previous=task.accumulation.take();task.tiles=Vec::new();task.phase=0;task.tile=0;task.inputs=Default::default();task.step=0;task.coordinates_first=0;
        }
        if task.accumulation.is_none() && self.started(task) {
            let (_, bounds, pages) = working_set(&task.job)?;
            let (uniform,_) = self.uniform.as_ref().ok_or(GpuRasterError::Invariant("a started object task has no sampling uniform"))?;
            let acc = sampler.reuse(&r.device,previous,crate::object_sampling::SamplingRequest {inverse:task.job.inverse,size:task.job.size,nearest:task.job.nearest},&task.job.output,&r.empty_view,uniform)?;
            task.tiles.clear();task.tiles.reserve_exact(pages);task.tiles.extend(page_coordinates(bounds));
            sampler.initialize(encoder,&acc);task.accumulation=Some(acc);steps+=PASS_TAPS;
            track_batch(encoder, &flight); recorded = true;
        }
        if task.accumulation.is_none() {
            let (working, bounds, pages) = working_set(&task.job)?;
            let output = task.texture.as_ref().map_or(0, texture_bytes);
            let collection_bytes=if let Some(c)=&task.collection {
                u64::from(task.key.size[0])*u64::from(task.key.size[1])*16*u64::from(c.spare.is_none() && c.request.blend==layer_core::BlendSpace::Perceptual)
                    + if c.result.is_none() && c.request.parts()>1 {c.request.window.size.map(u64::from).into_iter().product::<u64>()*16} else {0}
            } else {0};
            let extra = working.saturating_sub(output).saturating_sub(self.uniform_bytes())+collection_bytes;
            let working_bytes = task.gpu_bytes() + extra;
            if working_bytes > r.memory.display_limit() { return Err(GpuRasterError::SourceWorkingSetExceeded); }
            let running = self.tasks.iter().any(|other| self.started(other) && other.unfinished());
            let class = task.lease.demand().unwrap_or(Class::Prefetch).min(if running {Class::Prefetch} else {Class::Visible});
            let reserved = task.reserved.take();
            let mut inputs=sources::Inputs::default();
            if !objects::inline_sampling(r) && let Some(coordinate)=page_coordinates(bounds).next() {inputs.image(&task.job.source,coordinate,r.device.working_space());}
            let mut allocation=None;
            let mut context=(&mut *self,&mut *scene,&mut *task,&mut allocation);
            let accepted=inputs.accept(r,&mut context,|(cache,scene,task,allocation),r,inputs| {
                task.sync(&cache.pinned,cache.retains);
                let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
                **allocation=Some(match reserved {Some(reserved)=>reserved,None=>r.admit_tier(Some(scene),&cache.working,class,working_bytes).ok()?});
                let sources=r.admit_display(Some(scene),class,bytes).ok()?;
                Some(sources)
            },|(cache,_,task,_),r| {
                if task.texture.is_none() {
                    let (texture,output)=create_color_target(&r.device,task.key.size,"private canonical object result");
                    task.texture=Some(texture);task.job.output=output;
                }
                if let Some(c)=&mut task.collection {
                    if c.spare.is_none() && c.request.blend==layer_core::BlendSpace::Perceptual {c.spare=Some(create_color_target(&r.device,task.key.size,"object color conversion").0);}
                    if c.result.is_none() && c.request.parts()>1 {c.result=Some(create_color_target(&r.device,c.request.window.size,"private object window").0);}
                }
                if cache.uniform.is_none() {
                    let uniform=crate::object_sampling::ObjectSampler::uniform(&r.device);
                    let charge=cache.gpu.charge(uniform.size());
                    cache.uniform=Some((uniform,charge));
                }
                let (uniform,_)=cache.uniform.as_ref().unwrap();
                let acc=sampler.create(&r.device,crate::object_sampling::SamplingRequest {inverse:task.job.inverse,size:task.job.size,nearest:task.job.nearest},&task.job.output,&r.empty_view,uniform)?;
                task.tiles.clear();task.tiles.reserve_exact(pages);task.tiles.extend(page_coordinates(bounds));task.accumulation=Some(acc);Ok(())
            },|(cache,_,task,allocation),_,_| {
                if task.accumulation.is_some() && let Some(allocation)=allocation.take() {task.gpu=allocation.charge(task.gpu_bytes());}
                task.sync(&cache.pinned,cache.retains);Ok(())
            });
            if let Some(allocation)=allocation.take() {task.reserved=Some(allocation);}
            let inputs=match accepted {Ok(Some(((),inputs)))=>inputs,Ok(None)=>return Ok(0),
                Err(error) if error.is_preparation_pending()=>return Ok(0),Err(error)=>return Err(error)};
            task.inputs.extend(inputs)?;sampler.initialize(encoder,task.accumulation.as_ref().unwrap());steps+=PASS_TAPS;
            track_batch(encoder, &flight); recorded = true;
        }
        let guard = crate::submission::CacheWrite::shared(task.valid.clone()); guard.track(encoder);
        if task.job.nearest && !task.accumulation.as_ref().unwrap().nearest_coordinates_ready() {
            let length = task.key.size[0].checked_mul(task.key.size[1]).ok_or(GpuRasterError::SizeOverflow)?;
            let count = (length - task.coordinates_first).min(65536);
            let key = (task.key.inverse, task.key.size, task.coordinates_first, count);
            let token = self.coordinates.entry(key).or_default();
            let coordinates = token.upgrade().unwrap_or_else(|| {
                let coordinates = Arc::new(std::sync::Mutex::new(crate::object_image_mips::CoordinateSlot::new(&self.ram)));
                *token = Arc::downgrade(&coordinates); coordinates
            });
            self.sync_coordinate_slots();
            task.coordinates = Some(coordinates.clone());
            let prepared = coordinates.lock().unwrap().result.clone();
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
        self.prepare_inputs(r,scene,task)?;
        while steps < budget {
            let encoded = match task.phase {
                0 => sampler.encode_step(&r.device, encoder, &mut r.uploads, task.accumulation.as_ref().unwrap(), &SamplingPhase::Denominator { extent: task.job.source.extent }, task.step)?,
                1 if task.tile < task.tiles.len() => {
                    let coordinate = task.tiles[task.tile];
                    let input = {
                        let mut needed=sources::Inputs::default();needed.image(&task.job.source,coordinate,r.device.working_space());
                        if !task.inputs.contains(&needed) {self.prepare_inputs(r,scene,task)?;}
                        if !task.inputs.contains(&needed) {break;}
                        let read=sources::accepted_image(&task.inputs,&task.job.source,coordinate)?;
                        let before=encoder.pass_count();
                        task.inputs.clone().track(encoder);
                        match read.encode(r,encoder,Class::Visible) {
                            Ok(true)=>{},Ok(false)=>break,
                            Err(error) if error.is_preparation_pending()=>break,
                            Err(error)=>return Err(error),
                        }
                        steps=steps.saturating_add((encoder.pass_count()-before).saturating_mul(PASS_TAPS));
                        read.tile
                    };
                    let origin = coordinate.map(|v| v * PAGE_SIZE);
                    let extent = std::array::from_fn(|axis| (task.job.source.extent[axis] - origin[axis]).min(PAGE_SIZE));
                    sampler.encode_step(&r.device, encoder, &mut r.uploads, task.accumulation.as_ref().unwrap(), &SamplingPhase::Contribute { source: &input.view, origin: origin.map(|v| v as i32), extent }, task.step)?
                }
                1 => None,
                _ => sampler.encode_step(&r.device, encoder, &mut r.uploads, task.accumulation.as_ref().unwrap(), &SamplingPhase::Normalize, task.step)?,
            };
            if let Some(taps) = encoded { task.step += 1; steps += taps.max(PASS_TAPS); continue; }
            task.step = 0;
            if task.phase == 1 && task.tile < task.tiles.len() {
                task.inputs.finish_image(&task.job.source,task.tiles[task.tile]);task.tile += 1;
            }
            else if task.phase < 2 { task.phase += 1; }
            else {
                if let Some(c)=&mut task.collection {
                    if let Some(spare)=&c.spare {
                        let mut data=[0.;32];data[..4].copy_from_slice(&[0.,0.,task.key.size[0] as f32,task.key.size[1] as f32]);
                        data[4..8].copy_from_slice(&[task.key.size[0] as f32,task.key.size[1] as f32,0.,0.]);
                        data[8]=1.;data[9]=1.;data[31]=Convert::Encode.code();
                        scene.encode_batch(r,encoder,None,&[Job::Draw {target:spare.create_view(&Default::default()),sources:[task.job.output.clone(),r.empty_view.clone(),r.empty_view.clone(), r.empty_paper_view.clone()],data,over:false,clip:None,source_target:None,edges:None,pipeline:None}],&[])?;
                        steps+=u64::from(task.key.size[0])*u64::from(task.key.size[1]);
                        std::mem::swap(&mut task.texture,&mut c.spare);
                    }
                    if let Some(result)=&c.result {
                        let part=c.request.part(c.part);
                        let columns=c.request.window.size[0].div_ceil(COLLECTION_PART);
                        let offset=[c.part%columns*part.size[0],c.part/columns*part.size[1]];
                        encoder.copy_texture_to_texture(task.texture.as_ref().unwrap().as_image_copy(),
                            wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:offset[0],y:offset[1],z:0},..result.as_image_copy()},
                            wgpu::Extent3d {width:part.size[0].min(c.request.window.size[0]-offset[0]),height:part.size[1].min(c.request.window.size[1]-offset[1]),depth_or_array_layers:1});
                        c.part+=1;
                        if c.part<c.request.parts() {c.next=true;break;}
                        task.texture=c.result.take();
                    }
                    task.job.output=task.texture.as_ref().unwrap().create_view(&Default::default());
                }
                task.complete = true;
                let level = task.side.max(1.).log2().round() as u32;
                let entry = self.damage.entry((level, task.collection.as_ref().map(|c| c.request.owner)));
                let added = if matches!(entry, std::collections::btree_map::Entry::Vacant(_)) { std::mem::size_of::<(u32, scale::Damage)>() } else { 0 };
                let regions = &mut entry.or_insert(scale::Damage::EMPTY).regions;
                let before = regions.capacity(); regions.push(task.bounds.in_frame(r.document_extent));
                let grown = added + (regions.capacity() - before) * std::mem::size_of::<PixelRect>();
                self.damage_bytes.resize(self.damage_bytes.bytes() + grown as u64);
                break;
            }
        }
        Ok(steps)
    }
}

pub(super) fn working_set(job: &ObjectJob) -> Result<(u64, PixelRect, usize), GpuRasterError> {
    let bounds = crate::object_sampling::ObjectSampler::source_bounds(job.inverse, job.size, job.nearest, job.source.extent)?;
    let pages = u64::from(bounds.max_x().div_ceil(PAGE_SIZE) - bounds.min_x() / PAGE_SIZE)
        * u64::from(bounds.max_y().div_ceil(PAGE_SIZE) - bounds.min_y() / PAGE_SIZE);
    let pixels = u64::from(job.size[0]) * u64::from(job.size[1]);
    let bytes = crate::object_sampling::ObjectSampler::gpu_working_bytes(job.inverse, job.size, job.nearest)? + pixels * 16;
    Ok((bytes, bounds, usize::try_from(pages).map_err(|_| GpuRasterError::SizeOverflow)?))
}

fn track_batch(encoder: &crate::submission::CommandEncoder, flight: &Arc<AtomicBool>) {
    flight.store(true, Ordering::Release);
    let flight = InFlight(flight.clone()); encoder.hold(flight);
}

#[cfg(test)]
#[path="object_cache_q3_tests.rs"]
mod q3_tests;
