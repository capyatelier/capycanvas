use super::*;
use layer_core::{MemoryOwner, MemoryOwnerKind as Kind, MemoryReport};

const MIB: u64 = 1024 * 1024;
const SOURCE_WORKING_BYTES: u64 = SOURCE_SLOTS as u64 * scene::sources::NORMALIZED_PAGE_BYTES;
const SCRATCH_PAGES: u64 = 4;
const WORKING_BYTES: u64 = (SOURCE_SLOTS as u64 + SCRATCH_PAGES) * scene::sources::NORMALIZED_PAGE_BYTES;
pub(crate) const CACHE_BYTES: u64 = 608 * MIB;
pub(crate) const OBJECT_CACHE_BYTES: u64 = 128 * MIB;
const DECODED_SAMPLE_BYTES: u64 = 512 * MIB;
const IN_FLIGHT_DECODE_BYTES: u64 = 18 * scene::sources::NORMALIZED_PAGE_BYTES;
pub fn new_drawing_working_bytes(viewport:[u32;2],unified:bool)->u64 {
    let extent=viewport.map(|side|side.max(1).saturating_add(2*PAGE_SIZE));
    let display=display_mips::Plan::new(extent).map(|plan|plan.pixel_bytes_through(display_mips::MAX_LEVEL)).unwrap_or(u64::MAX);
    let ram=SOURCE_WORKING_BYTES+layer_core::package::MAX_RANGE_BYTES as u64;
    ram.saturating_add(if unified {(3*WORKING_BYTES).saturating_add(display)}else {0})
}
pub(crate) const RASTER_BUFFER_BYTES: u64 = 64 * MIB;

#[cfg(test)]
#[path = "memory_tests.rs"]
pub(crate) mod memory_tests;

struct Owner {
    handle: MemoryOwner,
    published: std::cell::Cell<MemoryReport>,
}
impl Owner {
    fn new(factory: &RendererFactory, kind: Kind, report: MemoryReport) -> Self {
        let handle = factory.memory_owner(kind);
        handle.report(report);
        Self { handle, published: std::cell::Cell::new(report) }
    }
    fn publish(&self, report: MemoryReport) -> bool {
        if self.published.get() == report || !self.handle.publish(report) { return false; }
        self.published.set(report);
        true
    }
}

#[derive(Clone)]
pub(crate) struct RasterTiers {pub(crate) raster:Arc<crate::residency::Tier>,pub(crate) paint:Arc<crate::residency::Tier>,pub(crate) admitted:Arc<crate::residency::Tier>}
impl RasterTiers {
    pub(crate) fn new(ledger:MemoryOwner)->Self {
        let raster=crate::residency::Tier::limited_by(ledger);
        Self {paint:crate::residency::Tier::within(&raster),admitted:crate::residency::Tier::within(&raster),raster}
    }
}

pub(super) struct DisplayUsage {ram:u64,pinned:[u64;5]}

struct DemandKey {
    owner: u64,
    revision: u64,
    extent: [u32; 2],
    scope: layer_core::SceneScope,
    visible: DocRect,
}

#[derive(Clone, Copy)]
struct Demand {floor:u64,mips:u64,sources:[u64;2],inputs:[u64;2],resources:u64}
impl Demand {
    fn unit(self)->u64 {unit_bytes(self.inputs)}
    fn pinned(self,[cells,objects,moving,sources,resources]:[u64;5])->[u64;5] {
        [cells.saturating_add(self.floor),objects,moving,sources.saturating_add(self.unit()),resources.max(WORKING_BYTES)]
    }
    fn wanted(self,preparation:u64)->[u64;5] {
        [CACHE_BYTES.saturating_add(self.floor).max(preparation),OBJECT_CACHE_BYTES,self.mips,self.sources[0].max(self.sources[1]),self.resources.max(WORKING_BYTES)]
    }
}

#[derive(Default)]
struct Lane {refused:Option<crate::residency::Blocked>,wanted:u64,asked:bool,lent:u64,base:u64,lendable:u64,exhausted:bool,refusals:u64}

pub(super) struct Memory {
    display: Owner,
    samples: Owner,
    raster: Owner,
    staging: Owner,
    demand: std::cell::Cell<Demand>,
    demand_key: std::cell::Cell<Option<DemandKey>>,
    applied_limits: std::cell::Cell<Option<[u64; 4]>>,
    limits_dirty: std::cell::Cell<bool>,
    preparation: std::cell::Cell<u64>,
    target: std::cell::Cell<u64>,
    refused: std::cell::Cell<u64>,
    lane: std::cell::RefCell<Lane>,
    trimmed: std::cell::Cell<Option<[u64; 2]>>,
    retired: std::cell::Cell<Option<[u64; 2]>>,
    wake: Arc<std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
    resources: Arc<std::sync::atomic::AtomicU64>,
    overdraft: std::cell::RefCell<Vec<layer_core::MemoryReservation>>,
    fruitless: std::cell::RefCell<Vec<(Arc<crate::residency::Tier>,crate::residency::Class,u64,crate::residency::Blocked)>>,
    #[cfg(not(target_arch = "wasm32"))]
    watch: std::cell::RefCell<Option<layer_core::MemoryWatch>>,
    unified: bool,
    pub idle: std::sync::atomic::AtomicBool,
}
pub(super) fn retained_limit(grant: u64, occupied: u64) -> u64 { grant.saturating_sub(occupied) }
fn source_demand(viewport: [u32;2]) -> u64 {
    viewport.map(|side| u64::from(side.saturating_add(2*PAGE_SIZE))).into_iter()
        .fold(16u64,u64::saturating_mul).max(SOURCE_WORKING_BYTES)
}
fn total(parts: [u64; 5]) -> u64 { parts.into_iter().fold(0, u64::saturating_add) }
fn presented_cells(visible: PixelRect, finest: u32) -> u64 {
    if visible.is_empty() { return 0; }
    let mut cells = 0;
    for level in finest..=display_mips::MAX_LEVEL {
        let side = PAGE_SIZE << level;
        let span = |low: u32, high: u32| u64::from(high.div_ceil(side) - low / side);
        let level_cells = span(visible.min_x(), visible.max_x()) * span(visible.min_y(), visible.max_y());
        cells += level_cells;
        if level_cells == 1 { break; }
    }
    cells
}
fn unit_bytes([stack, capture]: [u64; 2]) -> u64 {
    stack.saturating_add(capture).saturating_add(SCRATCH_PAGES * scene::sources::NORMALIZED_PAGE_BYTES).max(WORKING_BYTES)
}
fn stack_input_bytes(scene: SceneView<'_>) -> u64 {
    scene.order().iter().filter(|&&owner| scene.includes(owner) && scene.visible(owner))
        .map(|&owner| u64::from(scene.paint_source(owner).is_some()) + u64::from(scene.mask(owner).is_some_and(|(mask, _)| mask.enabled)))
        .sum::<u64>().saturating_mul(scene::sources::NORMALIZED_PAGE_BYTES)
}
fn display_floor(packet: FramePacket<'_>, presented: u64, unit: u64) -> u64 {
    let format = crate::display_precision::DisplayPrecision::for_scene(packet.scene).format();
    let page = u64::from(PAGE_SIZE).pow(2) * u64::from(format.block_copy_size(None).unwrap());
    let visible = display_mips::view_bounds(packet.view, packet.document_extent, 0).unwrap_or(PixelRect::EMPTY);
    let cells = display_mips::view_level(packet.view.document_to_surface, display_mips::MAX_LEVEL).map_or(0, |level| presented_cells(visible, level));
    cells.saturating_mul(2 * page).saturating_sub(presented).saturating_add(unit).saturating_add(IN_FLIGHT_DECODE_BYTES)
}

impl Memory {
    pub fn new(factory: &RendererFactory, samples: Option<MemoryOwner>) -> Self {
        let working = WORKING_BYTES;
        let demand = Demand {floor:working,mips:0,sources:[SOURCE_WORKING_BYTES,scene::scale::SOURCE_WORKING_BYTES+working],inputs:[0;2],resources:working};
        let display = Owner::new(factory, Kind::Scene, MemoryReport { pinned_gpu_bytes: total(demand.pinned([0;5])), demand_gpu_bytes: total(demand.pinned([0;5])), optional_gpu_bytes: u64::MAX, ..Default::default() });
        let resources = Arc::new(std::sync::atomic::AtomicU64::new(display.handle.grant().gpu_bytes));
        let wake: Arc<std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>> = Default::default();
        let forwarded = wake.clone();
        display.handle.set_wake(Arc::new(move || if let Some(wake) = forwarded.lock().unwrap().clone() { wake(); }));
        let memory = Self {
            display,
            samples: samples.map_or_else(|| Owner::new(factory, Kind::DecodedSamples, MemoryReport { pinned_ram_bytes: SOURCE_WORKING_BYTES, demand_ram_bytes: DECODED_SAMPLE_BYTES, ..Default::default() }),
                |handle| { let report = handle.reported(); Owner { handle, published: std::cell::Cell::new(report) } }),
            raster: Owner::new(factory, Kind::RasterPool, MemoryReport { pinned_gpu_bytes: working, pinned_ram_bytes: working, demand_gpu_bytes: working, demand_ram_bytes: working, ..Default::default() }),
            staging: Owner::new(factory, Kind::Staging, MemoryReport { pinned_gpu_bytes: working, demand_gpu_bytes: working, ..Default::default() }),
            demand: std::cell::Cell::new(demand), demand_key: std::cell::Cell::new(None), applied_limits: std::cell::Cell::new(None), limits_dirty: std::cell::Cell::new(true), preparation: std::cell::Cell::new(0), target: std::cell::Cell::new(total(demand.wanted(0))), refused: Default::default(), lane: Default::default(), trimmed: Default::default(), retired: Default::default(), wake, resources, overdraft: Default::default(), fruitless: Default::default(), #[cfg(not(target_arch = "wasm32"))] watch: Default::default(),
            unified: factory.unified_memory(), idle: std::sync::atomic::AtomicBool::new(true),
        };
        memory.applied_limits.set(Some(memory.grants()));
        memory
    }
    #[cfg(test)]
    pub fn display_owner(&self) -> MemoryOwner { self.display.handle.clone() }
    pub(crate) fn display_limit(&self) -> u64 { self.display.handle.gpu_working_limit() }
    pub(crate) fn display_grant(&self) -> u64 { self.retention_grant().min(self.target.get()) }
    pub(crate) fn retention_grant(&self) -> u64 { self.display.handle.grant().gpu_bytes }
    pub(crate) fn retention_limit(&self) -> u64 { if self.unified { self.display_grant() } else { self.retention_grant() } }
    pub(crate) fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) { *self.wake.lock().unwrap() = Some(wake); }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn watch(&self, budget: &layer_core::MemoryBudget, edge: Arc<dyn Fn() + Send + Sync>) { *self.watch.borrow_mut() = Some(budget.watch(edge)); }
    pub(crate) fn raster_grant(&self) -> u64 { self.raster.handle.grant().gpu_bytes }
    pub(crate) fn staging_grant(&self) -> u64 { self.staging.handle.grant().gpu_bytes }
    pub(crate) fn sample_grant(&self) -> u64 { self.samples.handle.grant().ram_bytes }
    pub(crate) fn mip_budget(&self) -> u64 { self.demand.get().mips.min(self.display_grant()) }
    fn grants(&self) -> [u64; 4] { [self.retention_grant(), self.raster_grant(), self.sample_grant(), self.staging_grant()] }
    fn owner(&self,kind:Kind)->&Owner {
        match kind {Kind::Scene=>&self.display,Kind::RasterPool=>&self.raster,Kind::Staging=>&self.staging,_=>unreachable!("renderer memory owner")}
    }
    pub(super) fn sync_report(&self,kind:Kind,report:MemoryReport) {
        #[cfg(test)] crate::test_support::PassCounts {report_syncs:1,..Default::default()}.record();
        let owner=self.owner(kind);let before=owner.published.replace(report);
        if before.pinned_gpu_bytes!=report.pinned_gpu_bytes || before.demand_gpu_bytes!=report.demand_gpu_bytes || before.optional_gpu_bytes!=report.optional_gpu_bytes {self.invalidate_limits();}
        owner.handle.publish(report);owner.handle.report(report);
        if kind==Kind::Scene {self.overdraft.borrow_mut().clear();}
    }
    fn overdrawn(&self)->bool {!self.overdraft.borrow().is_empty()}
    fn stop(&self)->Option<crate::residency::Blocked> {self.lane.borrow().refused.clone()}
    fn lent(&self,raster:u64)->u64 {
        let lane=self.lane.borrow();
        if lane.refused.is_some() {lane.lent}else {lane.lent.min(raster.saturating_add(lane.wanted).saturating_sub(lane.base))}
    }
    fn lend(&self,raster:u64,wanted:u64,more:u64) {
        let lent=self.lent(raster);let mut lane=self.lane.borrow_mut();
        lane.lent=lent.saturating_add(more).min(lane.lendable.max(lent));lane.wanted=wanted;
        lane.base=raster.saturating_add(wanted).saturating_sub(lane.lent);
    }
    fn draw(&self,raster:u64,limit:u64) {
        let lent=self.lent(raster);let drawn=raster.saturating_sub(limit.saturating_sub(lent)).min(lent);
        let mut lane=self.lane.borrow_mut();(lane.lent,lane.wanted,lane.base)=(drawn,0,raster.saturating_sub(drawn));
    }
    pub(crate) fn stroke_memory_refusals(&self)->u64 {self.lane.borrow().refusals}
    fn carry_overdraft(&self,used:u64) {
        if used>self.retention_grant() && self.lane.borrow().refused.is_none() {self.overdraft.borrow_mut().extend(self.display.handle.admit_working_through(used,None));}
    }
    pub fn sample_owner(&self) -> MemoryOwner { self.samples.handle.clone() }
    pub(crate) fn raster_owner(&self) -> MemoryOwner { self.raster.handle.clone() }
    pub fn resource_limit(&self) -> Arc<std::sync::atomic::AtomicU64> { self.resources.clone() }
    pub fn retire(&self, device:&crate::PipelineDevice, queue: &wgpu::Queue) {
        for owner in [&self.display, &self.raster, &self.staging] {
            let mut report = owner.published.get();
            report.pinned_gpu_bytes = report.gpu_bytes;
            owner.publish(report);
        }
        let owners = [self.display.handle.clone(), self.samples.handle.clone(), self.raster.handle.clone(), self.staging.handle.clone()];
        device.on_queue_done(queue,move || drop(owners));
    }
    fn demand_region(&self, scene: SceneView<'_>, extent: [u32; 2], visible: PixelRect) {
        let rounded = DocRect::from(visible).aligned(PAGE_SIZE);
        if let Some(key) = self.demand_key.take() {
            if key.owner == scene.owner() && key.revision == scene.revision() && key.extent == extent
                && key.scope == *scene.scope().unwrap_or(&layer_core::SceneScope::All) && key.visible == rounded {
                self.demand_key.set(Some(key));
                return;
            }
        }
        self.demand_key.set(Some(DemandKey {owner: scene.owner(), revision: scene.revision(), extent,
            scope: scene.scope().cloned().unwrap_or_default(), visible: rounded}));
        let visible = rounded.in_frame(extent);
        self.limits_dirty.set(true);
        let window = PixelRect::new(visible.min_x(), visible.min_y(),
            visible.min_x().saturating_add(PAGE_SIZE).min(extent[0]), visible.min_y().saturating_add(PAGE_SIZE).min(extent[1]));
        let mut resources = 0u64;
        let mut luts = std::collections::HashSet::new();
        for &handle in scene.order() {
            let Some(effect) = scene.effect(handle) else { continue; };
            if let Some(resource) = effect.lut3d() && luts.insert(resource.digest()) {
                resources = resources.saturating_add((96 + u64::from(resource.size()).pow(3) * 16).next_multiple_of(16));
            }
            let analysis = match effect.program.analysis() {
                Some(layer_core::EffectAnalysisKind::LocalIllumination) => crate::local_tone::Builder::allocation_bound(extent),
                Some(layer_core::EffectAnalysisKind::Dehaze) => crate::dehaze::Builder::allocation_bound(extent),
                None => continue,
            };
            resources = resources.saturating_add(analysis.unwrap_or(0));
        }
        self.demand.set(Demand {floor:scene::Scene::capture_image_bound(scene, window).max(WORKING_BYTES),inputs:[stack_input_bytes(scene),0],resources,..self.demand.get()});
    }
    pub fn mip_demand(&self, bytes: u64, viewport: [u32;2]) -> bool {
        let demand = self.demand.get();
        let sources = [source_demand(viewport),demand.sources[1]];
        if demand.mips == bytes && demand.sources == sources { return false; }
        self.demand.set(Demand {mips:bytes,sources,..demand});
        self.limits_dirty.set(true);
        true
    }
    pub fn working_limit(&self) -> u64 { self.demand.get().floor }
    pub(crate) fn unit(&self) -> u64 { self.demand.get().unit() }
    pub fn invalidate_limits(&self) { self.limits_dirty.set(true); }
    fn limits_changed(&self) -> bool { self.limits_dirty.get() || self.applied_limits.get() != Some(self.grants()) }
    pub fn apply_limits(&self, limits: [u64; 4]) -> bool {
        let changed = self.applied_limits.replace(Some(limits)) != Some(limits);
        self.limits_dirty.replace(false) || changed
    }
}

impl Drop for WgpuRasterizer {
    fn drop(&mut self) {
        self.publish_memory();
        self.memory.retire(&self.device,&self.queue);
    }
}

impl WgpuRasterizer {
    pub(super) fn memory_demand(&self, packet: FramePacket<'_>) {
        let visible=display_mips::view_bounds(packet.view,packet.document_extent,PAGE_SIZE).unwrap_or(PixelRect::EMPTY);
        self.memory.demand_region(packet.scene,packet.document_extent,visible);
    }
    fn sync_raster_report(&mut self) {
        self.memory.sync_report(Kind::RasterPool,self.raster_memory_report());
    }
    pub(crate) fn admit_raster_working(&mut self,bytes:u64)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        self.admit_group(bytes,0).map(|(_,held)|held)
    }
    pub(crate) fn admit_group(&mut self,raster:u64,display:u64)->Result<(crate::residency::Reservation,crate::residency::Reservation),crate::residency::Blocked> {
        self.memory.lane.borrow_mut().asked=true;self.memory.lend(self.raster_tiers.raster.used(),raster,0);
        let drain=crate::residency::Drain::new(self.draining());
        let admitted=self.try_group(raster,display).or_else(|_|{self.yield_to_lane(raster,display);self.try_group(raster,display)});
        match admitted {
            Ok(held)=>{
                self.memory.draw(self.raster_tiers.raster.used(),self.raster_tiers.raster.limit());
                self.end_lane_wait();self.memory.invalidate_limits();self.publish_memory();self.refresh_memory_limits();Ok(held)
            },
            Err(blocked)=>Err(self.hold_lane(blocked.draining(drain))),
        }
    }
    fn draining(&self)->Vec<Arc<crate::residency::Tier>> {
        vec![self.retired_display_bytes.clone(),self.source_tiles.borrow().completing_tier(),self.device.transient_display.clone(),self.raster_buffers.transfer.clone()]
    }
    fn try_group(&mut self,raster:u64,display:u64)->Result<(crate::residency::Reservation,crate::residency::Reservation),crate::residency::Blocked> {
        #[cfg(test)] crate::test_support::PassCounts {raster_admissions:1,..Default::default()}.record();
        let class=crate::residency::Class::Authored;
        self.sync_raster_report();self.factory.memory_budget().flush_reports();
        let tiers=self.raster_tiers.clone();
        let held=tiers.admitted.admit(class,raster).or_else(|blocked| {
            let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
            self.evict_raster(blocked.deficit(),&[],&[],&mut encoder);
            if encoder.has_work() {self.last_submission=encoder.submit(&self.queue);}
            tiers.admitted.admit(class,raster)
        }).map_err(|blocked|blocked.with(self.scene_tiers.display.wanting(class,display)))?;
        let display_tier=&self.scene_tiers.display;
        let source=if display==0 {display_tier.admit(class,0)} else {self.admit_unstopped(None,display_tier,class,display,true)}
            .map_err(|blocked|blocked.with(tiers.admitted.wanting(class,raster)))?;
        Ok((source,held))
    }
    fn yield_to_lane(&mut self,raster:u64,display:u64) {
        let deficit=|r:&Self|r.raster_tiers.raster.used().saturating_add(raster).saturating_sub(r.raster_tiers.raster.limit());
        let shown=&self.scene_tiers.display;
        self.trim_display_by(self.scene.as_deref(),deficit(self).max(shown.used().saturating_add(display).saturating_sub(shown.limit())));
        self.factory.memory_budget().flush_reports();
        self.memory.lend(self.raster_tiers.raster.used(),raster,if deficit(self)==0 {0}else {u64::MAX});
        self.publish_memory();self.factory.memory_budget().flush_reports();
        self.scene_tiers.display.update_limit(self.memory.display_limit());
    }
    fn hold_lane(&mut self,blocked:crate::residency::Blocked)->crate::residency::Blocked {
        self.memory.lane.borrow_mut().refused=Some(blocked.clone());self.memory.invalidate_limits();
        self.wait_for_release(blocked.clone());
        let (display,raster)=(&self.scene_tiers.display,&self.raster_tiers.raster);
        let ours=[&self.memory.display,&self.memory.raster,&self.memory.staging].map(|owner|{let report=owner.published.get();report.gpu_bytes.saturating_sub(report.pinned_gpu_bytes)});
        let draining=crate::residency::Drain::new(self.draining()).bytes();
        let optional=self.factory.memory_budget().snapshot().devices.iter().find(|device|device.device==self.factory.memory_device())
            .map_or(0,|device|device.physical_bytes.saturating_sub(device.pinned_bytes)).saturating_sub(ours.into_iter().sum());
        let mut lane=self.memory.lane.borrow_mut();
        let short=raster.used().saturating_add(lane.wanted).saturating_sub(raster.limit());
        let lent_out=lane.lent>=lane.lendable || short==0;
        lane.exhausted=self.live_stroke.is_some() && draining==0 && !self.raster_backing_in_flight() && optional<short.max(blocked.deficit()) && lent_out && display.yielding()==0 && display.heap().pending()==0 && !blocked.fits();
        blocked
    }
    pub(crate) fn lane_exhausted(&self)->bool {self.memory.lane.borrow().exhausted}
    pub(crate) fn take_lane_failure(&mut self)->Option<GpuRasterError> {
        if !self.lane_exhausted() {return None;}
        self.memory.lane.borrow_mut().refusals+=1;
        self.retire_restore();self.end_lane_wait();
        Some(GpuRasterError::StrokeMemory)
    }
    pub(crate) fn lane_polls(&self)->bool {
        #[cfg(not(target_arch="wasm32"))] {self.preparation_wake.is_none() && self.lane_waits()}
        #[cfg(target_arch="wasm32")] {self.lane_waits()}
    }
    pub(crate) fn lane_waits(&self)->bool {
        self.scene_tiers.display.heap().pending()==0 && self.memory.stop().is_some_and(|lane|self.refuses(&lane))
    }
    fn end_lane_wait(&self) {
        let ended={let mut lane=self.memory.lane.borrow_mut();(lane.wanted,lane.exhausted)=(0,false);lane.refused.take().is_some()};
        if !ended {return;}
        self.memory.invalidate_limits();self.publish_memory();
        if let Some(wake)=&self.preparation_wake {wake();}
    }
    pub(crate) fn admit_raster_window(&mut self,least:u64,most:u64,reserve:u64)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        let (wanted,raster)=(most.saturating_add(reserve),&self.raster_tiers.raster);
        let room=raster.limit().saturating_sub(raster.used());
        let mut reservation=self.admit_raster_working(if wanted<=room {wanted}else {room.max(least.saturating_add(reserve))})?;
        drop(reservation.split(reserve));
        Ok(reservation)
    }
    pub(crate) fn commit_raster_working(&mut self,reservation:crate::residency::Reservation) {
        drop(reservation);self.sync_raster_report();
        self.memory.invalidate_limits();self.refresh_memory_limits();
    }
    pub(crate) fn retire_native_color(&self,mut pages:Vec<PageSurface>,leases:Vec<Arc<crate::residency::Lease>>,encoder:&mut submission::CommandEncoder) {
        let retiring=pages.iter().map(PageSurface::storage_bytes).sum::<u64>();
        if retiring==0 {return;}
        let retirement=DisplayRetirement {_charge:self.raster_buffers.transfer.charge(retiring),
            #[cfg(not(target_arch="wasm32"))] _scratch:(Vec::new(),Vec::new())};
        pages.iter_mut().for_each(PageSurface::discharge);
        encoder.retain_resources();
        #[cfg(not(target_arch="wasm32"))]
        encoder.retire((pages,retirement,leases));
        #[cfg(target_arch="wasm32")]
        {drop(pages);encoder.retire((retirement,leases));}
    }
    pub(crate) fn refresh_memory_limits(&mut self) {
        if !self.memory_pass_due() {return;}
        #[cfg(test)] crate::test_support::PassCounts {grant_encoders:1,..Default::default()}.record();
        let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
        self.apply_frame_grants(&mut encoder);
        if encoder.has_work() {self.last_submission=encoder.submit(&self.queue);}
    }
    pub(crate) fn apply_frame_grants(&mut self,encoder:&mut submission::CommandEncoder) {
        let mut scene=self.scene.take();let mut cache=self.scale_display.take();
        self.apply_memory_grants(scene.as_deref_mut(),cache.as_mut(),encoder);
        self.scene=scene;self.scale_display=cache;
    }
    pub(crate) fn apply_memory_grants(&mut self,mut scene:Option<&mut scene::Scene>,cache:Option<&mut scene::scale::Cache>,encoder:&mut submission::CommandEncoder) {
        self.retire_victims(scene.as_deref_mut(),encoder);
        if !self.memory.apply_limits(self.memory.grants()) && !self.memory.overdrawn() && !self.retention_above_grant() {return;}
        let composition=self.composition_memory(scene.as_deref());
        self.memory.sync_report(Kind::Scene,self.display_memory_report(scene.as_deref(),composition));
        let raster=self.raster_memory_report();self.memory.sync_report(Kind::RasterPool,raster);
        self.memory.sync_report(Kind::Staging,self.staging_memory_report());
        self.factory.memory_budget().flush_reports();
        let grants=self.memory.grants();let [display,_,ram,uploads]=grants;
        debug_assert!(self.memory.raster_grant()>=raster.pinned_gpu_bytes);
        self.memory.apply_limits(grants);
        self.scene_tiers.display.update_limit(self.memory.display_limit());
        self.scene_tiers.display.set_grant(display);
        self.device.resource_limit.store(self.memory.display_grant(),std::sync::atomic::Ordering::Release);
        self.device.source_samples.set_limit(ram.min(usize::MAX as u64) as usize);
        if self.device.source_samples.trim_unused(SOURCE_SLOTS)!=0 {self.publish_samples();}
        self.source_tiles.get_mut().admit_limits(uploads);
        let upload_bytes=self.uploads.memory_bytes()[0];let restore_bytes=self.restore_uploads.memory_bytes()[0];
        let retained_uploads=upload_bytes.saturating_add(restore_bytes);
        let upload_limit=if retained_uploads==0 {0}else {((u128::from(uploads)*u128::from(upload_bytes))/u128::from(retained_uploads)) as u64};
        self.uploads.trim_unused(upload_limit);self.restore_uploads.trim_unused(uploads.saturating_sub(upload_limit));
        self.memory.sync_report(Kind::Staging,self.staging_memory_report());
        self.evict_raster(self.raster_excess(),&[],&[],encoder);
        self.trim_display_to_grant(scene.as_deref());
        self.retire_victims(scene.as_deref_mut(),encoder);
        if self.scene_tiers.display.used()>self.memory.retention_grant() {self.retire_scratch(scene.as_deref_mut(),cache,encoder);}
        self.source_tiles.get_mut().flush_retired(encoder);
        self.memory.sync_report(Kind::Scene,self.display_memory_report(scene.as_deref(),composition));
        self.memory.sync_report(Kind::RasterPool,self.raster_memory_report());
        if self.warm.is_none() {self.memory.carry_overdraft(self.scene_tiers.display.used());}
    }
    fn memory_pass_due(&self)->bool {
        let display=&self.scene_tiers.display;
        display.heap().pending()!=0 || self.memory.retired.get()!=Some([display.heap().epoch(),display.floor()])
            || self.memory.limits_changed() || self.memory.overdrawn() || self.retention_above_grant()
    }
    fn retire_victims(&mut self,scene:Option<&mut scene::Scene>,encoder:&mut submission::CommandEncoder) {
        let floor=self.scene_tiers.display.floor();
        self.memory.retired.set(Some([self.scene_tiers.display.heap().epoch(),floor]));
        let sources=self.source_tiles.get_mut();
        if self.warm.is_some() {sources.release_ready();} else {sources.release_unused(floor);}
        let decoded=sources.retire_evicted();sources.flush_retired(encoder);
        if decoded+self.moving_images.retire_evicted(encoder)+scene.map_or(0,|scene|scene.retire_victims(encoder))!=0 {encoder.retain_resources();}
    }
    pub(crate) fn begin_frame_memory(&mut self, packet: FramePacket<'_>) {
        if let Some(lane)=self.memory.stop() {lane.rearm();}
        self.memory.lane.borrow_mut().exhausted=false;
        self.memory.fruitless.borrow_mut().clear();
        if self.restore_preparation()==Some(crate::frame_demand::Outstanding::Blocked(crate::frame_demand::Input::Residency))
            && let Some(scene)=&self.scene {scene.advance_demand_epoch();}
        self.frame_room(packet);
        self.scene_tiers.display.begin_frame();
    }
    pub(crate) fn frame_room(&mut self, packet: FramePacket<'_>) {
        self.set_display_room(packet);
        self.fit_grants();
    }
    pub(crate) fn fit_grants(&mut self) {
        self.trim_display_to_grant(self.scene.as_deref());
        self.refresh_memory_limits();
        self.trim_raster_to_grant();
    }
    fn trim_raster_to_grant(&mut self) {
        if self.raster_tiers.raster.used().saturating_sub(self.raster_buffers.transfer.used())<=self.memory.raster_grant() {return;}
        let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
        self.evict_raster(self.raster_excess(),&[],&[],&mut encoder);
        self.sync_raster_report();
        if encoder.has_work() {self.last_submission=encoder.submit(&self.queue);}
    }
    fn set_display_room(&mut self, packet: FramePacket<'_>) {
        #[cfg(test)] crate::test_support::PassCounts {display_rooms:1,..Default::default()}.record();
        self.memory_demand(packet);
        self.memory.refused.set(self.scale_display.as_ref().map_or(0,scene::scale::Cache::refused_bytes));
        let presented=self.composition_memory(self.scene.as_deref())[1];
        self.scene_tiers.display.set_limit(self.memory.display_limit(),display_floor(packet,presented,self.memory.unit()));
        self.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.scene_tiers.display.set_grant(self.memory.retention_limit());
    }
    pub(crate) fn end_frame_memory(&mut self) {
        if !std::mem::take(&mut self.memory.lane.borrow_mut().asked) {self.end_lane_wait();}
        self.scene_tiers.display.heap().begin_frame();self.scene_tiers.display.end_frame();
    }
    fn trim_display_to_grant(&self,scene:Option<&scene::Scene>) {
        let display=&self.scene_tiers.display;let grant=self.memory.retention_limit();
        self.trim_display_by(scene,display.used().saturating_sub(grant.saturating_add(self.scene_tiers.retiring.used())));
        self.memory.trimmed.set(Some([grant,display.used()]));
    }
    fn trim_display_by(&self,scene:Option<&scene::Scene>,excess:u64) {
        let display=&self.scene_tiers.display;let before=display.used();
        display.heap().reclaim(excess);
        let excess=excess.saturating_sub(display.heap().pending());
        if excess!=0 && let Some(scene)=scene {scene.release_pages_by(excess);}
        if display.used()<before {self.memory.display.publish(self.display_memory_report(scene,self.composition_memory(scene)));}
    }
    pub(crate) fn retention_above_grant(&self)->bool {
        let display=&self.scene_tiers.display;let grant=self.memory.retention_limit();
        display.used()>grant.saturating_add(self.scene_tiers.retiring.used()) && self.memory.trimmed.get()!=Some([grant,display.used()])
    }
    pub(crate) fn admit_display(&self,scene:Option<&scene::Scene>,class:crate::residency::Class,bytes:u64)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        self.admit_tier(scene,&self.scene_tiers.display,class,bytes)
    }
    pub(crate) fn admit_tier(&self,scene:Option<&scene::Scene>,tier:&Arc<crate::residency::Tier>,class:crate::residency::Class,bytes:u64)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        let scan=!self.searched_in_vain(tier,class,bytes);
        let admitted=self.admit_within(scene,tier,class,bytes,scan);
        let display=&self.scene_tiers.display;
        if admitted.is_err() && scan && class.floor_reserved() {display.heap().release_stale(display.above_optional());}
        if scan && self.memory.stop().is_none() && let Err(refusal)=&admitted {
            let mut fruitless=self.memory.fruitless.borrow_mut();
            fruitless.retain(|(searched,refused,least,_)|!(Arc::ptr_eq(searched,tier) && *refused>=class && *least>=bytes));
            fruitless.push((tier.clone(),class,bytes,refusal.clone()));
        }
        admitted
    }
    fn searched_in_vain(&self,tier:&Arc<crate::residency::Tier>,class:crate::residency::Class,bytes:u64)->bool {
        let mut fruitless=self.memory.fruitless.borrow_mut();
        fruitless.retain(|(_,_,_,refusal)|!refusal.fits());
        fruitless.iter().any(|(searched,refused,least,_)|Arc::ptr_eq(searched,tier) && *refused<=class && bytes>=*least)
    }
    fn admit_within(&self,scene:Option<&scene::Scene>,tier:&Arc<crate::residency::Tier>,class:crate::residency::Class,bytes:u64,scan:bool)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        if let Some(lane)=self.memory.stop() {return Err(lane);}
        self.admit_unstopped(scene,tier,class,bytes,scan)
    }
    fn admit_unstopped(&self,scene:Option<&scene::Scene>,tier:&Arc<crate::residency::Tier>,class:crate::residency::Class,bytes:u64,scan:bool)->Result<crate::residency::Reservation,crate::residency::Blocked> {
        #[cfg(test)] crate::test_support::PassCounts {room_scans:u64::from(scan),..Default::default()}.record();
        let display=&self.scene_tiers.display;
        display.set_grant(self.memory.retention_limit());
        let scene=scene.or(self.scene.as_deref());
        let over=|| display.used().saturating_add(bytes).saturating_sub(display.grant());
        let cover=if class.floor_reserved() && over()!=0 {
            let ledger=&self.memory.display.handle;
            if scan {
                display.heap().evict(over(),class);
                if let Some(scene)=scene {scene.trim_pages_by(over().saturating_sub(display.heap().pending()),class);}
            }
            let total=display.used().saturating_add(bytes);
            (over()!=0).then(||ledger.admit_working_through(total,None).ok_or_else(||tier.refused_by(ledger.clone(),class,bytes))).transpose()
                .inspect_err(|blocked|self.wait_for_release(blocked.clone()))?
        } else {None};
        let reservation=if scan {tier.admit(class,bytes).or_else(|blocked| {
            if let Some(scene)=scene {scene.trim_pages_by(blocked.deficit(),class);}
            tier.admit(class,bytes)
        })} else {tier.admit_unevicted(class,bytes)}.inspect_err(|blocked|self.wait_for_release(blocked.clone()))?;
        self.memory.overdraft.borrow_mut().extend(cover);
        Ok(reservation)
    }
    #[cfg_attr(target_arch="wasm32",expect(unused_variables,reason="Wasm admissions have no release wake to wait on"))]
    pub(crate) fn wait_for_release(&self,blocked:crate::residency::Blocked) {
        #[cfg(not(target_arch="wasm32"))]
        if let Some(wake)=self.preparation_wake.clone() {blocked.wake_on_release(&self.device.retirement_handle(),wake);}
    }
    pub(crate) fn refuses(&self,refusal:&crate::residency::Blocked)->bool {
        #[cfg(not(target_arch="wasm32"))]
        if let Some(wake)=&self.preparation_wake {return self.device.retirement_handle().refuses(refusal,wake);}
        !refusal.fits()
    }
    pub(super) fn prepare_capture_memory(&mut self, scene: SceneView<'_>, extent: [u32; 2]) {
        self.memory.demand_region(scene, extent, PixelRect::full(extent));
        let demand=self.memory.demand.get();
        self.memory.demand.set(Demand {sources:[demand.sources[0],scene::scale::SOURCE_WORKING_BYTES+WORKING_BYTES],..demand});
        self.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
    }
    pub(super) fn prepare_capture_sources(&mut self,bytes:u64) {
        let demand=self.memory.demand.get();
        let required=bytes.saturating_add(WORKING_BYTES);
        if required<=demand.sources[1] && bytes<=demand.inputs[1] {return;}
        self.memory.demand.set(Demand {sources:[demand.sources[0],demand.sources[1].max(required)],inputs:[demand.inputs[0],demand.inputs[1].max(bytes)],..demand});
        self.publish_memory();self.factory.memory_budget().flush_reports();self.refresh_memory_limits();
    }
    pub(crate) fn reserve_effect_analysis(&mut self, bytes: u64) -> Result<crate::effect_analysis::Lease, String> {
        let required = self.device.analysis_memory.used().saturating_add(bytes);
        if required <= self.device.resource_limit.load(std::sync::atomic::Ordering::Acquire) {
            return crate::effect_analysis::Lease::reserve(&self.device, bytes);
        }
        let demand = self.memory.demand.get();
        self.memory.demand.set(Demand {resources:demand.resources.max(required),..demand});
        self.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
        let result = crate::effect_analysis::Lease::reserve(&self.device, bytes);
        self.memory.demand.set(Demand {resources:demand.resources,..self.memory.demand.get()});
        self.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
        result
    }
    pub(crate) fn refresh_preparation_memory(&mut self) {
        if self.memory.preparation.get() == 0 { return; }
        self.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
    }
    pub(crate) fn preparation_memory(&self, bytes: u64) {
        let before = self.memory.preparation.replace(if bytes == 0 { 0 } else { bytes.saturating_add(self.memory.working_limit()) });
        if before != self.memory.preparation.get() { self.publish_memory(); }
    }
    pub(super) fn paint_memory_bytes(&self) -> u64 {
        self.raster_tiers.paint.used()
    }
    pub fn display_bytes(&self)->u64 {self.scene_tiers.display.used()}
    pub(crate) fn resident_bytes(&self)->u64 {
        self.scene_tiers.display.used().saturating_add(self.device.transient_display.used()).saturating_add(self.non_display_bytes())
    }
    pub(crate) fn non_display_bytes(&self)->u64 {
        [&self.raster_tiers.raster,&self.scene_tiers.ram,&self.scene_tiers.decodes,&self.raster_buffers.working]
            .into_iter().map(|tier|tier.used()).sum::<u64>().saturating_add(self.uncharged_bytes())
    }
    fn uncharged_bytes(&self)->u64 {
        let masks=&self.layer_masks;
        self.portable_blend.byte_len()+self.material_records.storage_bytes()+self.transport_records.storage_bytes()
            +self.dry_display_inputs.len() as u64*dry_material::DisplayInputs::storage_bytes()
            +self.thumbnails.storage_bytes()+self.retouch.as_ref().map_or(0,|retouch|retouch.storage_bytes())
            +self.native_edit.as_ref().map_or(0,|native|native.storage_bytes())
            +self.filter_previews.as_ref().map_or(0,|previews|previews.storage_bytes())
            +masks.pages.values().chain(masks.previews.values().map(|preview|&preview.page)).chain(masks.command_pages.values())
                .chain(masks.snapshots.values().flat_map(|snapshot|snapshot.pages.values())).map(layer_masks::MaskPage::storage_bytes).sum::<u64>()
    }
    pub(super) fn composition_storage_bytes(&self)->u64 {
        self.scale_display.as_ref().map_or_else(||self.scene.as_ref().map_or(0,|scene|scene.retained_scale_bytes()[0]),|cache|cache.storage_bytes())
    }
    pub(super) fn display_memory_report(&self, scene: Option<&scene::Scene>, [_, presented]: [u64; 2]) -> MemoryReport {
        self.display_report(scene,&self.display_usage(),presented)
    }
    pub(super) fn display_usage(&self)->DisplayUsage {
        let scene=self.scene_tiers.usage();
        let source = self.source_tiles.borrow();
        let ram = scene.ram.saturating_add(source.cpu_bytes()).saturating_add(self.scene_tiers.moving[1].used()).saturating_add(self.scene_tiers.decodes.used()).saturating_add(self.moving_images.metadata_bytes());
        let resources = self.device.analysis_memory.used();
        let pinned_transforms = if self.transform_preview.is_some() || self.transforms.as_ref().is_some_and(|transforms| transforms.has_preview()) {
            self.transforms.as_ref().map_or(0, paint_transform::PaintTransforms::charged_bytes) } else { 0 };
        let cells=self.changed_cells.as_ref().map_or(0,changed_cells::ChangedCells::storage_bytes);
        DisplayUsage {ram,pinned:[
            cells.saturating_add(pinned_transforms).saturating_add(scene.working),
            scene.pinned,
            self.moving_images.pinned_bytes(), source.pinned_bytes().saturating_add(source.non_slot_bytes()), resources,
        ]}
    }
    pub(super) fn display_report(&self,scene:Option<&scene::Scene>,usage:&DisplayUsage,presented:u64)->MemoryReport {
        let ram=usage.ram.saturating_add(scene.map_or(0,scene::Scene::store_metadata_bytes));
        let gpu=self.scene_tiers.display.used();
        let demand=self.memory.demand.get();
        let mut held=usage.pinned;held[0]=held[0].saturating_add(presented).saturating_add(self.retired_display_bytes.used());
        let pinned=demand.pinned(held);let wanted=demand.wanted(self.memory.preparation.get());
        let floor=self.scene_tiers.display.floor();let refused=self.memory.refused.get();
        let required=total(pinned).saturating_add(wanted[4].saturating_sub(pinned[4])).saturating_add(floor)
            .max(if refused==0 {0}else {gpu.saturating_add(refused).saturating_add(floor)}).max(self.memory.preparation.get());
        self.memory.target.set(total(std::array::from_fn(|part| wanted[part].max(pinned[part]))).max(required));
        let lendable=total(pinned).saturating_sub(total(held)).saturating_sub(demand.unit());
        self.memory.lane.borrow_mut().lendable=lendable;
        let lent=self.memory.lent(self.raster_tiers.raster.used()).min(lendable);
        self.warm_report(MemoryReport { ram_bytes: ram, gpu_bytes: gpu, pinned_gpu_bytes: total(pinned)-lent,
            demand_ram_bytes: ram, demand_gpu_bytes: required, optional_gpu_bytes: u64::MAX, ..Default::default() }, gpu)
    }
    fn raster_pins(&self)->u64 {
        let held=self.staged_restore_mask_bytes().saturating_add(self.raster_tiers.admitted.used());
        self.raster_buffers.transfer.used().saturating_add(self.pinned_paint_bytes()).saturating_add(held)
    }
    pub(crate) fn raster_excess(&self)->u64 {
        let floor=WORKING_BYTES.saturating_sub(self.raster_pins());
        self.raster_tiers.raster.used().saturating_add(self.staged_restore_mask_bytes()).saturating_add(floor).saturating_sub(self.memory.raster_grant())
    }
    pub(super) fn raster_memory_report(&self)->MemoryReport {
        let [ram, _] = self.raster_buffers.memory_bytes();
        let gpu = self.raster_tiers.raster.used().saturating_add(self.staged_restore_mask_bytes());
        let raster_pins = self.raster_pins();
        let raster_floor = raster_pins.max(WORKING_BYTES);
        self.warm_report(MemoryReport {ram_bytes:ram,gpu_bytes:gpu,
            pinned_ram_bytes:self.raster_buffers.working.used().max(WORKING_BYTES),
            pinned_gpu_bytes:raster_floor,demand_ram_bytes:ram.max(WORKING_BYTES),
            demand_gpu_bytes:gpu.saturating_add(self.projected_restore_bytes().max(self.memory.lane.borrow().wanted))
                .saturating_add(raster_floor.saturating_sub(raster_pins)).max(WORKING_BYTES),..Default::default()},0)
    }
    fn staging_memory_report(&self)->MemoryReport {
        let [uploads,pinned]=self.uploads.memory_bytes();let [restore,restore_pinned]=self.restore_uploads.memory_bytes();
        self.warm_report(MemoryReport {gpu_bytes:uploads.saturating_add(restore),pinned_gpu_bytes:pinned.saturating_add(restore_pinned).max(WORKING_BYTES),
            demand_gpu_bytes:uploads.saturating_add(restore).max(WORKING_BYTES),..Default::default()},0)
    }
    pub(crate) fn current_display_memory_report(&self)->MemoryReport {
        self.display_memory_report(self.scene.as_deref(),self.composition_memory(self.scene.as_deref()))
    }
    pub(crate) fn composition_memory(&self,scene:Option<&scene::Scene>)->[u64;2] {
        [0,self.scene_tiers.windows.used().saturating_add(self.scale_display.as_ref().map_or_else(||scene.map_or(0,|scene|scene.retained_scale_bytes()[1]),scene::scale::Cache::pinned_page_bytes))]
    }
    pub(super) fn publish_memory(&self) {
        self.memory.display.publish(self.current_display_memory_report());
        self.memory.raster.publish(self.raster_memory_report());
        self.memory.staging.publish(self.staging_memory_report());
    }
    pub(crate) fn publish_samples(&self) {
        let samples = self.device.source_samples.stats();
        self.memory.samples.publish(self.warm_report(MemoryReport { ram_bytes: samples.physical_bytes as u64,
            pinned_ram_bytes: (samples.pinned_bytes as u64).max(SOURCE_WORKING_BYTES),
            demand_ram_bytes: DECODED_SAMPLE_BYTES, ..Default::default() }, 0));
    }
    pub fn after_present(&mut self) {
        if !self.memory.idle.load(std::sync::atomic::Ordering::Acquire)
            || self.startup.as_ref().is_some_and(|startup| !startup.compiler.is_idle()) { return; }
        if self.restore_preparation().is_none() && !self.bake_preflight.as_ref().is_some_and(|state|state.pending()) { self.memory.preparation.set(0); }
        self.publish_memory();
        self.publish_samples();
        self.memory.invalidate_limits();self.refresh_memory_limits();
        self.publish_memory();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_presented_floor_follows_the_view_and_not_the_document() {
        assert_eq!(presented_cells(PixelRect::new(0, 0, 512, 512), 0), 4 + 1);
        assert_eq!(presented_cells(PixelRect::EMPTY, 0), 0);
        let view = PixelRect::new(1000, 1000, 3000, 2500);
        let cells = [16_384, 65_536].map(|side| presented_cells(view.intersect(PixelRect::full([side; 2])), 0));
        assert_eq!(cells[0], cells[1]);
        assert!(presented_cells(view, 2) < cells[0]);
    }
    #[test]
    fn raster_retention_subtracts_the_other_physical_cache() {
        for grant in [0, 1, 1024, u64::MAX] {
            for (pool, paint) in [(0, 0), (128, 256), (512, 512), (2048, 4096), (u64::MAX, u64::MAX)] {
                let kept_pool = pool.min(retained_limit(grant, paint));
                let kept_paint = paint.min(retained_limit(grant, kept_pool));
                assert!(kept_pool.saturating_add(kept_paint) <= grant);
                assert_eq!(retained_limit(grant, grant), 0);
            }
        }
    }
}
