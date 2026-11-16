use super::*;
use crate::object_sampling::{ObjectAccumulator, SamplingPhase};
use objects::{COLLECTION_PART, CollectionJob, ObjectJob, ObjectKey};
use std::collections::{BTreeMap, BTreeSet, HashMap};
type Coordinates = Arc<std::sync::Mutex<crate::object_image_mips::CoordinateSlot>>;
type CoordinateKey = ([u64; 6], [u32; 2], u32, u32);
use std::sync::atomic::{AtomicBool, Ordering};
use crate::residency::{Charge, Lease, Tier};
use crate::schedule::{Budget, Closure, Group, Schedule};
use crate::frame_demand::Input;

pub(super) type DamageKey = (u32, Option<OccurrenceHandle>);
type Place = (OccurrenceHandle, u64, [i64; 2], u64);
type Space = (layer_core::BlendSpace, layer_core::color::RgbSpace);
pub(super) type ObjectWait = (Arc<Lease>, Option<crate::schedule::Waiter>);
pub(super) const INTERACTIVE_TAPS: u64 = 1 << 20;
pub(super) const IDLE_TAPS: u64 = 8 << 20;
const SEATS: usize = (IDLE_TAPS / PASS_TAPS) as usize;
static OWNERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
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
    group: Option<Group>,
    seated: u32,
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
        let bytes = self.gpu_bytes();
        self.lease.offer(bytes, crate::residency::rebuild_value(done * PASS_TAPS, bytes));
    }
    fn unfinished(&self) -> bool { !self.complete || !self.valid.load(Ordering::Acquire) }
    fn waits_for(&self) -> Input {
        let decoding = self.coordinates.as_ref().is_some_and(|slot| slot.lock().unwrap().result.is_none())
            || self.phase == 1 && self.tile < self.tiles.len() && sources::accepted_image(&self.inputs, &self.job.source, self.tiles[self.tile]).is_ok_and(|read| !read.ready());
        if decoding { Input::Decode } else { Input::Gpu }
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
        self.hold = None; self.group = None;
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
    objects: Arc<Tier>, schedule: Schedule, owner: u64,
    queues: Charge, damage_bytes: Charge, coordinate_slots: Charge, frame: u64, used: Vec<Arc<TaskPin>>,
    tasks: BTreeMap<u64, Task>, places: BTreeSet<Place>, spaces: HashMap<Space, usize>, next: u64,
    damage: BTreeMap<DamageKey, scale::Damage>, retiring: Vec<Task>, in_flight: Arc<AtomicBool>,
    coordinates: HashMap<CoordinateKey, crate::object_image_mips::CoordinateDestination>, retains: bool, uniform: Option<(wgpu::Buffer, Charge)>, requester: Class,
    evicted: Arc<AtomicBool>,
}
impl ObjectCache {
    pub fn new(tiers: &super::Tiers, keys: object_spatial::KeyLeases, retains: bool) -> Self {
        let [gpu, retired_gpu] = [(); 2].map(|()| Tier::within(&tiers.objects));
        let [ram, retired] = [(); 2].map(|()| Tier::within(&tiers.ram));
        let working = Tier::within(&gpu);
        Self { keys, pinned: Tier::within(&tiers.pinned), queues: ram.charge(0), damage_bytes: ram.charge(0), coordinate_slots: ram.charge(0), gpu, working, ram, retired, retired_gpu,
            objects: tiers.objects.clone(), schedule: tiers.schedule.clone(), owner: OWNERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            frame: 1, used: Vec::new(), tasks: BTreeMap::new(), places: BTreeSet::new(), spaces: HashMap::new(), next: 0,
            damage: BTreeMap::new(), retiring: Vec::new(), in_flight: Default::default(),
            coordinates: HashMap::new(), retains, uniform: None, requester: Class::Visible, evicted: Default::default() }
    }
    pub(super) fn set_requester(&mut self, class: Class) { self.requester = class; }
    fn started(&self, task: &Task) -> bool { Arc::ptr_eq(task.gpu.tier(), &self.working) }
    fn queue_bytes(&self) -> u64 {
        (self.tasks.len() * std::mem::size_of::<(u64, Task)>() + self.places.len() * std::mem::size_of::<Place>()
            + self.spaces.capacity() * std::mem::size_of::<(Space, usize)>() + self.retiring.capacity() * std::mem::size_of::<Task>()) as u64
    }
    fn sync_queues(&mut self) { self.queues.resize(self.queue_bytes()); }
    fn cells(request: &CollectionJob, bounds: DocRect) -> ([i64; 2], [i64; 2]) {
        let side = (f64::from(PAGE_SIZE) * request.window.side.max(1.)) as i64;
        (bounds.min.map(|v| v.div_euclid(side)), std::array::from_fn(|axis| (bounds.max[axis] - 1).max(bounds.min[axis]).div_euclid(side)))
    }
    fn index(&mut self, id: u64, request: &CollectionJob, add: bool) {
        let ([x0, y0], [x1, y1]) = Self::cells(request, request.bounds());
        for y in y0..=y1 { for x in x0..=x1 {
            let place = (request.owner, request.window.side.to_bits(), [x, y], id);
            if add { self.places.insert(place); } else { self.places.remove(&place); }
        }}
        let space = self.spaces.entry((request.blend, request.context)).or_default();
        if add { *space += 1; } else { *space -= 1; if *space == 0 { self.spaces.remove(&(request.blend, request.context)); } }
    }
    fn near(&self, request: &CollectionJob, bounds: DocRect) -> Vec<u64> {
        let ([x0, y0], [x1, y1]) = Self::cells(request, bounds);
        let side = request.window.side.to_bits();
        let mut ids: Vec<u64> = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| [x, y]))
            .flat_map(|cell| self.places.range((request.owner, side, cell, 0)..=(request.owner, side, cell, u64::MAX)).map(|place| place.3)).collect();
        ids.sort_unstable(); ids.dedup();
        #[cfg(test)] crate::test_support::PassCounts {object_lookups:1,object_lookup_visits:ids.len() as u64,..Default::default()}.record();
        ids
    }
    fn find(&self, request: &CollectionJob) -> Option<u64> {
        let origin = request.bounds();
        self.near(request, DocRect { max: origin.min.map(|v| v + 1), ..origin }).into_iter()
            .find(|id| self.tasks[id].collection.as_ref().is_some_and(|c| c.request.same(request)))
    }
    fn insert(&mut self, task: Task) -> u64 {
        let id = self.next; self.next += 1;
        if let Some(c) = &task.collection { let request = c.request.clone(); self.index(id, &request, true); }
        self.tasks.insert(id, task); self.sync_queues();
        id
    }
    fn take(&mut self, id: u64) -> Option<Task> {
        let task = self.tasks.remove(&id)?;
        if let Some(c) = &task.collection { let request = c.request.clone(); self.index(id, &request, false); }
        Some(task)
    }
    fn retire_where(&mut self, mut retired: impl FnMut(&Task) -> bool) {
        let ids: Vec<u64> = self.tasks.iter().filter(|(_, task)| retired(task)).map(|(id, _)| *id).collect();
        for id in ids { if let Some(task) = self.take(id) { self.retire(task); } }
        self.sync_queues();
    }
    fn prepare_inputs(&mut self,r:&mut WgpuRasterizer,task:&mut Task)->Result<(),GpuRasterError> {
        let Some(group)=&task.group else {return Ok(());};
        for index in task.tile..task.tiles.len() {
            let mut inputs=sources::Inputs::default();inputs.image(&task.job.source,task.tiles[index],r.device.working_space());
            if task.inputs.contains(&inputs) {continue;}
            let Some(seat)=self.schedule.budget(group,Budget::seat).flatten() else {break;};
            match inputs.accept(r,&mut (),|_,_,_|Some(seat),|_,_|Ok(()),|_,_,_|Ok(())) {
                Ok(Some(((),inputs)))=>{task.inputs.extend(inputs)?;task.seated+=1;},
                Ok(None)=>{self.schedule.budget(group,Budget::vacate);break;},
                Err(error)=>{self.schedule.budget(group,Budget::vacate);if error.is_preparation_pending() {break;} return Err(error);},
            }
        }
        task.inputs.prepare(r)?;Ok(())
    }
    fn vacate(&self,task:&mut Task) {
        if let Some(group)=&task.group {for _ in 0..task.seated {self.schedule.budget(group,Budget::vacate);}}
        task.seated=0;task.inputs=Default::default();
    }
    pub fn retire_evicted(&mut self) -> u64 {
        if !self.evicted.swap(false, Ordering::AcqRel) { return 0; }
        let mut bytes = 0;
        self.retire_where(|task| {
            let evicted = task.lease.evicted() && task.pin.strong_count() == 0;
            if evicted { bytes += task.gpu_bytes(); } else { task.lease.restore(); task.offer(); }
            evicted
        });
        bytes
    }
    #[cfg(test)]
    pub fn evict_to(&mut self, gpu: u64) -> u64 {
        let used = self.gpu.used().saturating_sub(self.retiring.iter().map(|task| task.gpu.bytes()).sum::<u64>());
        self.gpu.heap().evict(used.saturating_sub(gpu), Class::Fallback);
        self.retire_evicted()
    }
    pub fn clear(&mut self) { self.retire_where(|_| true); self.damage.clear(); self.damage_bytes.resize(0); self.coordinates.clear(); }
    fn retire(&mut self, mut task: Task) { task.coordinates = None; task.group = None; task.lease.hold(); task.hold = Some(task.pin(&self.pinned)); self.retiring.push(task); self.sync_queues(); }
    pub fn pending(&self) -> bool { self.tasks.values().any(Task::unfinished) }
    pub fn in_flight(&self) -> bool { self.in_flight.load(Ordering::Acquire) }
    #[cfg(test)]
    pub fn request_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.values().filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    #[cfg(test)]
    pub fn completed_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.values().filter(|task| task.complete).filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    #[cfg(test)]
    pub fn work_progress(&self) -> (usize, usize, u64) {
        (self.tasks.len(), self.tasks.values().filter(|task|task.complete).count(), self.tasks.values().map(|task|task.step + task.tile as u64 * 1000000 + task.phase as u64 * 1000000000 + task.collection.as_ref().map_or(0,|c|c.part as u64 * 1000000000000)).sum())
    }
    #[cfg(test)]
    pub fn bytes(&self) -> u64 { let [ram, gpu] = self.memory_bytes(); ram + gpu }
    #[cfg(test)]
    pub fn memory_bytes(&self) -> [u64; 2] { [self.ram.used() + self.retired.used(), self.gpu.used() + self.retired_gpu.used()] }
    #[cfg(test)]
    pub fn walked_bytes(&self) -> [u64; 3] {
        let tasks = || self.tasks.values().chain(self.retiring.iter());
        let gpu = self.uniform.as_ref().map_or(0, |(uniform, _)| uniform.size()) + self.retired_gpu.used() + tasks().map(Task::gpu_bytes).sum::<u64>();
        let ram = self.damage.values().map(|damage| std::mem::size_of::<(u32, scale::Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>()
            + self.retired.used() + self.coordinates.values().filter_map(std::sync::Weak::upgrade).map(|coordinates| { let state = coordinates.lock().unwrap(); std::mem::size_of::<Option<crate::object_image_mips::CoordinateResult>>() as u64 + state.result.as_ref().and_then(|result| result.as_ref().ok()).map_or(0, |bytes| bytes.capacity() as u64) }).sum::<u64>()
            + self.coordinates.capacity() as u64 * (std::mem::size_of::<CoordinateKey>() + std::mem::size_of::<crate::object_image_mips::CoordinateDestination>()) as u64
            + self.queue_bytes() + tasks().map(Task::ram_bytes).sum::<u64>();
        let pinned = self.retired_gpu.used() + self.retiring.iter().map(Task::gpu_bytes).sum::<u64>()
            + self.tasks.values().filter(|task| task.used == self.frame || task.pin.strong_count() > usize::from(task.hold.is_some()) || !self.retains).map(Task::gpu_bytes).sum::<u64>();
        [ram, gpu, pinned]
    }
    #[cfg(test)]
    pub fn pinned_gpu_bytes(&self) -> u64 { self.pinned.used() }
    pub fn needs_retirement(&self) -> bool { !self.retiring.is_empty() || (!self.retains && self.tasks.values().any(|task| task.used == self.frame && task.complete)) }
    pub fn flush_retired(&mut self, encoder: &crate::submission::CommandEncoder) {
        if self.retains { self.reset_used(); } else { let frame = self.frame; self.retire_where(|task| task.used == frame && task.complete); }
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
    pub fn cover(&mut self, request: &CollectionJob, mut inputs:Option<&mut sources::AcceptedInputs>) -> (Vec<(wgpu::TextureView, DocRect)>, bool, Vec<DocRect>, Vec<ObjectWait>) {
        let window = request.bounds();
        let mut missing = vec![window];
        let mut pieces = Vec::new();
        let mut complete = true;
        let mut waits = Vec::new();
        for id in self.near(request, window) {
            let Self { tasks, pinned, used, frame, schedule, .. } = self;
            let task = tasks.get_mut(&id).expect("an indexed object window is cached");
            let Some(bounds) = task.collection.as_ref().filter(|c| c.request.same_content(request)).map(|c| c.request.bounds()) else { continue; };
            if bounds.intersect(window).is_empty() { continue; }
            missing = missing.into_iter().flat_map(|rect| rect.subtract(bounds)).filter(|rect| !rect.is_empty()).collect();
            let ready = task.complete && task.valid.load(Ordering::Acquire);
            if let Some(inputs)=inputs.as_deref_mut() {
                if !inputs.declaring() {waits.push((task.lease.clone(),task.group.as_ref().filter(|_|!ready).and_then(|group|schedule.waiter(group))));}
                inputs.hold(task.pin(pinned));
            }
            if ready {
                used.extend(task.use_in(*frame, pinned)); task.offer();
                pieces.push((task.job.output.clone(), bounds));
            } else { complete = false; }
        }
        (pieces, complete, missing, waits)
    }
    pub(super) fn stamp(&mut self, request: &CollectionJob, class: Class) {
        let window = request.bounds();
        let epoch = self.gpu.heap().epoch();
        for id in self.near(request, window) {
            let task = &self.tasks[&id];
            if task.collection.as_ref().is_some_and(|c| c.request.same_content(request) && !c.request.bounds().intersect(window).is_empty()) {
                task.lease.stamp(class);
                if let Some(group)=&task.group {self.schedule.rewant(group,class,epoch);}
            }
        }
    }
    pub fn prepare_collection(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, request: &CollectionJob, inputs:Option<&mut sources::AcceptedInputs>) -> Result<bool, GpuRasterError> {
        if let Some(task) = self.find(request).and_then(|id| self.tasks.get_mut(&id)) {
            task.lease.stamp(self.requester);
            if let Some(group)=&task.group {self.schedule.rewant(group,self.requester,self.gpu.heap().epoch());}
            if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
            let ready = task.complete && task.valid.load(Ordering::Acquire);
            if self.retains && ready { self.used.extend(task.use_in(self.frame, &self.pinned)); }
            return Ok(ready);
        }
        self.resolve_collection(r, scene, request, inputs).map(|_| false)
    }
    pub fn resolve_collection(&mut self,r:&WgpuRasterizer,_scene:SceneView<'_>,request:&CollectionJob,inputs:Option<&mut sources::AcceptedInputs>)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        if self.spaces.keys().any(|space|*space!=(request.blend,request.context)) {
            self.retire_where(|task|task.collection.as_ref().is_some_and(|c|c.request.blend!=request.blend || c.request.context!=request.context));
        }
        if let Some(task)=self.find(request).and_then(|id|self.tasks.get_mut(&id)) {
            task.lease.stamp(self.requester);
            if let Some(group)=&task.group {self.schedule.rewant(group,self.requester,self.gpu.heap().epoch());}
            if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
            if task.complete && task.valid.load(Ordering::Acquire) {self.used.extend(task.use_in(self.frame,&self.pinned));task.offer();return Ok(Some(task.job.output.clone()));}
            return Ok(None);
        }
        self.add(r,request,self.requester,inputs)?;
        Ok(None)
    }
    fn add(&mut self,r:&WgpuRasterizer,request:&CollectionJob,class:Class,inputs:Option<&mut sources::AcceptedInputs>)->Result<(),GpuRasterError> {
        let group=self.schedule.want(self.owner,self.next,class,false,Self::closure(r,request)?,self.gpu.heap().epoch());
        let job=request.child(0,request.part(0),r.empty_view.clone())?;
        let key=Key {object:job.key.clone(),inverse:job.inverse.map(f64::to_bits),size:job.size};
        let lease=self.gpu.heap().lease(&self.evicted);lease.stamp(class);
        let collection=Collection {_metadata:self.keys.lease(&request.authored),request:request.clone(),part:0,spare:None,result:None,next:false};
        let mut task=Task {collection:Some(collection),key,job,texture:None,accumulation:None,tiles:Vec::new(),phase:0,tile:0,step:0,
            valid:Arc::new(AtomicBool::new(true)),bounds:request.bounds(),side:request.window.side,complete:false,used:0,coordinates:None,coordinates_first:0,inputs:Default::default(),
            gpu:self.gpu.charge(0),ram:self.ram.charge(0),group:Some(group),seated:0,lease,pin:std::sync::Weak::new(),hold:None};
        task.sync(&self.pinned,self.retains);
        if let Some(inputs)=inputs {inputs.hold(task.pin(&self.pinned));}
        self.insert(task);
        Ok(())
    }
    fn closure(r:&WgpuRasterizer,request:&CollectionJob)->Result<Closure,GpuRasterError> {
        let part=request.part(0);
        let (mut maximum,mut pages)=(0,0);
        for index in 0..request.keys.len() {
            let job=request.child(index,part,r.empty_view.clone())?;
            r.preflight_image_sampling(job.inverse,job.size,job.nearest)?;
            let (bytes,_,footprint)=working_set(&job)?;
            maximum=maximum.max(bytes);pages=pages.max(footprint);
        }
        let pixels=u64::from(part.size[0])*u64::from(part.size[1]);
        let result = if request.parts() > 1 { request.window.size.map(u64::from).into_iter().product::<u64>() * 16 } else { 0 };
        let (bytes,seat)=(maximum+pixels*16+result,sources::NORMALIZED_PAGE_BYTES);
        let room=r.memory.display_limit().saturating_sub(bytes)/seat;
        if room==0 {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        Ok(Closure {bytes,seats:pages.clamp(1,SEATS).min(room as usize) as u32,seat})
    }
    pub(super) fn admit(&mut self,r:&WgpuRasterizer,class:Class,requests:&[CollectionJob],mut inputs:Option<&mut sources::AcceptedInputs>)->Result<Option<crate::residency::Blocked>,GpuRasterError> {
        let epoch=self.gpu.heap().epoch();
        for request in requests {
            if let Some(task)=self.find(request).and_then(|id|self.tasks.get_mut(&id)) {
                task.lease.stamp(class);
                if let Some(group)=&task.group {self.schedule.rewant(group,class,epoch);}
                if let Some(inputs)=inputs.as_deref_mut() {inputs.hold(task.pin(&self.pinned));}
            } else {self.add(r,request,class,inputs.as_deref_mut())?;}
        }
        let stop=self.schedule.admit(epoch,r.arrival_token(),r.memory.display_grant(),|class,bytes|r.admit_tier(None,&self.objects,class,bytes));
        let waiting=requests.iter().any(|request|self.find(request).is_some_and(|id|self.tasks[&id].group.as_ref().is_some_and(|group|!self.schedule.admitted(group))));
        Ok(stop.filter(|_|waiting))
    }
    pub fn retain(&mut self, scene: SceneView<'_>, blend:layer_core::BlendSpace, context:layer_core::color::RgbSpace) {
        self.retire_where(|task| {
            let keep=if let Some(collection)=&task.collection {
                let owner=collection.request.owner;
                collection.request.blend==blend && collection.request.context==context && (!collection.request.display || scene.visible(owner))
                    && collection.request.authored.iter().map(|key|key.handle).eq(scene.object_handle(owner))
                    && collection.request.authored.iter().all(|key|key.same_authored(&ObjectKey::new(scene,owner,key.handle)))
            } else {scene.object_owner(task.key.object.handle).is_some_and(|owner|ObjectKey::new(scene,owner,task.key.object.handle).same_authored(&task.key.object) && (!task.job.live||scene.visible(owner)))};
            !keep
        });
        self.coordinates.retain(|_,token|token.strong_count()!=0);self.sync_coordinate_slots();
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
        let epoch = self.gpu.heap().epoch();
        for (&id, task) in &mut self.tasks {
            if task.unfinished() && task.group.is_none() && let Some(c) = &task.collection {
                task.group = Some(self.schedule.want(self.owner, id, task.lease.demand().unwrap_or(Class::Prefetch), false, Self::closure(r, &c.request)?, epoch));
            }
        }
        for tag in self.schedule.lapsed(self.owner) { if let Some(task) = self.take(tag) { self.retire(task); } }
        self.schedule.admit(epoch, r.arrival_token(), r.memory.display_grant(), |class, bytes| r.admit_tier(Some(scene), &self.objects, class, bytes));
        let runnable = self.schedule.runnable(self.owner);
        let since = r.arrival_token();
        if runnable.is_empty() { return Ok(0); }
        if !r.compilation.require(sampler.pipelines().into_iter().chain([&r.scene_pipelines.constant.2]), startup::DOCUMENT) { self.schedule.hold(self.owner, Input::Compile, since); return Ok(0); }
        if self.in_flight.load(Ordering::Acquire) { self.schedule.hold(self.owner, Input::Gpu, since); return Ok(0); }
        let mut used = 0;
        for tag in runnable {
            if used >= allowance { break; }
            let Some(mut task) = self.tasks.remove(&tag) else { continue; };
            let result = self.advance_task(r, scene, encoder, &sampler, &mut task, allowance - used);
            if task.complete {
                task.accumulation = None; task.tiles = Vec::new(); task.coordinates = None; self.vacate(&mut task); task.group = None;
                if let Some(c) = &mut task.collection { c.spare = None; }
                task.gpu = self.gpu.charge(task.gpu_bytes());
            } else if let Some(group) = &task.group && matches!(result, Ok(0)) { self.schedule.block(group, task.waits_for(), since); }
            task.sync(&self.pinned, self.retains);
            self.tasks.insert(tag, task);
            used += result?;
        }
        if self.in_flight.load(Ordering::Acquire) { self.schedule.hold(self.owner, Input::Gpu, since); }
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
            task.complete = false; task.used = 0; previous = task.accumulation.take(); task.coordinates = None; task.coordinates_first = 0; task.phase = 0; task.tile = 0;self.vacate(task); task.step = 0; task.valid = Arc::new(AtomicBool::new(true));
        }
        if let Some(c)=&mut task.collection && c.next {
            c.next=false;
            task.job=c.request.child(0,c.request.part(c.part),task.texture.as_ref().unwrap().create_view(&Default::default()))?;
            task.key.object=task.job.key.clone();task.key.inverse=task.job.inverse.map(f64::to_bits);
            previous=task.accumulation.take();task.tiles=Vec::new();task.phase=0;task.tile=0;self.vacate(task);task.step=0;task.coordinates_first=0;
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
            let Some(group)=task.group.as_ref().filter(|group|self.schedule.start(group)) else {return Ok(0);};
            let (_, bounds, pages) = working_set(&task.job)?;
            if task.texture.is_none() {
                let (texture,output)=create_color_target(&r.device,task.key.size,"private canonical object result");
                task.texture=Some(texture);task.job.output=output;
            }
            if let Some(c)=&mut task.collection {
                if c.spare.is_none() && c.request.blend==layer_core::BlendSpace::Perceptual {c.spare=Some(create_color_target(&r.device,task.key.size,"object color conversion").0);}
                if c.result.is_none() && c.request.parts()>1 {c.result=Some(create_color_target(&r.device,c.request.window.size,"private object window").0);}
            }
            if self.uniform.is_none() {
                let uniform=crate::object_sampling::ObjectSampler::uniform(&r.device);
                let charge=self.gpu.charge(uniform.size());
                self.uniform=Some((uniform,charge));
            }
            let (uniform,_)=self.uniform.as_ref().unwrap();
            let acc=sampler.create(&r.device,crate::object_sampling::SamplingRequest {inverse:task.job.inverse,size:task.job.size,nearest:task.job.nearest},&task.job.output,&r.empty_view,uniform)?;
            task.tiles.clear();task.tiles.reserve_exact(pages);task.tiles.extend(page_coordinates(bounds));task.accumulation=Some(acc);
            let bytes=task.gpu_bytes();
            task.gpu=self.schedule.budget(group,|budget|budget.spend(&self.working,bytes)).ok_or(GpuRasterError::Invariant("a started object window has no admitted budget"))?;
            task.sync(&self.pinned,self.retains);
            sampler.initialize(encoder,task.accumulation.as_ref().unwrap());steps+=PASS_TAPS;
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
        self.prepare_inputs(r,task)?;
        while steps < budget {
            let encoded = match task.phase {
                0 => sampler.encode_step(&r.device, encoder, &mut r.uploads, task.accumulation.as_ref().unwrap(), &SamplingPhase::Denominator { extent: task.job.source.extent }, task.step)?,
                1 if task.tile < task.tiles.len() => {
                    let coordinate = task.tiles[task.tile];
                    let input = {
                        let mut needed=sources::Inputs::default();needed.image(&task.job.source,coordinate,r.device.working_space());
                        if !task.inputs.contains(&needed) {self.prepare_inputs(r,task)?;}
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
                if let Some(group)=&task.group {self.schedule.budget(group,Budget::vacate);task.seated-=1;}
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
        if !recorded && steps > 0 { track_batch(encoder, &flight); }
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
