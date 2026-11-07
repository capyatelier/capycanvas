use super::*;
use layer_core::{MemoryOwner, MemoryOwnerKind as Kind, MemoryReport};

const MIB: u64 = 1024 * 1024;
const PARTS: usize = 5;
const SOURCE_WORKING_BYTES: u64 = SOURCE_SLOTS as u64 * scene::sources::NORMALIZED_PAGE_BYTES;
const WORKING_BYTES: u64 = (SOURCE_SLOTS as u64 + 4) * PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16;
pub(crate) const CACHE_BYTES: u64 = 608 * MIB;
pub(crate) const OBJECT_CACHE_BYTES: u64 = 128 * MIB;
const DECODED_SAMPLE_BYTES: u64 = 512 * MIB;
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
    fn publish(&self, report: MemoryReport, force: bool) -> bool {
        let before = self.published.get();
        let changed = [before.ram_bytes.abs_diff(report.ram_bytes), before.gpu_bytes.abs_diff(report.gpu_bytes),
            before.pinned_ram_bytes.abs_diff(report.pinned_ram_bytes), before.pinned_gpu_bytes.abs_diff(report.pinned_gpu_bytes),
            before.demand_ram_bytes.abs_diff(report.demand_ram_bytes), before.demand_gpu_bytes.abs_diff(report.demand_gpu_bytes)]
            .into_iter().any(|bytes| bytes >= MIB);
        if (force || changed) && self.handle.publish(report) { self.published.set(report); true } else { false }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DisplayDemand {
    pub level: u32,
    pub transforms: [Option<(display_mips::Plan, bool)>; 2],
    pub moving: Option<layer_core::authored::OccurrenceHandle>,
    pub material: bool,
    pub bounds: PixelRect,
}

struct DemandKey {
    owner: u64,
    revision: u64,
    extent: [u32; 2],
    scope: layer_core::SceneScope,
    visible: DocRect,
    display: DisplayDemand,
}

pub(super) struct Memory {
    display: Owner,
    samples: Owner,
    raster: Owner,
    staging: Owner,
    demand: std::cell::Cell<[u64; PARTS]>,
    scene_retention: std::cell::Cell<u64>,
    demand_key: std::cell::Cell<Option<DemandKey>>,
    active_owner: std::cell::Cell<Option<u64>>,
    applied_limits: std::cell::Cell<Option<[u64; 7]>>,
    limits_dirty: std::cell::Cell<bool>,
    pinned: std::cell::Cell<[u64; PARTS]>,
    color_pinned: std::cell::Cell<u64>,
    sample_demand: std::cell::Cell<u64>,
    source_inputs: std::cell::Cell<[u64;2]>,
    preparation: std::cell::Cell<u64>,
    floors: std::cell::Cell<[u64; PARTS]>,
    resources: Arc<std::sync::atomic::AtomicU64>,
    published_at: std::cell::Cell<web_time::Instant>,
    pub idle: std::sync::atomic::AtomicBool,
}
pub(super) fn retained_limit(grant: u64, occupied: u64) -> u64 { grant.saturating_sub(occupied) }
fn source_demand(viewport: [u32;2]) -> u64 {
    viewport.map(|side| u64::from(side.saturating_add(2*PAGE_SIZE))).into_iter()
        .fold(16u64,u64::saturating_mul).max(SOURCE_WORKING_BYTES)
}
fn total(parts: [u64; PARTS]) -> u64 { parts.into_iter().fold(0, u64::saturating_add) }
fn partition(grant: u64, demand: [u64; PARTS], pinned: [u64; PARTS], index: usize) -> u64 {
    let remaining = grant.saturating_sub(total(pinned));
    let demand: [_; PARTS] = std::array::from_fn(|part| (part, demand[part].saturating_sub(pinned[part])));
    let extra = layer_core::memory_budget::distribute(remaining, &demand).into_iter().find(|(part, _)| *part == index).map_or(0, |(_, bytes)| bytes);
    pinned[index].saturating_add(extra)
}

impl Memory {
    pub fn new(factory: &RendererFactory, samples: Option<MemoryOwner>) -> Self {
        let working = WORKING_BYTES;
        let demand = [CACHE_BYTES, OBJECT_CACHE_BYTES, 0, scene::scale::SOURCE_WORKING_BYTES+working, working];
        let floors = [working, 0, 0, working, working];
        let display = Owner::new(factory, Kind::Scene, MemoryReport { pinned_gpu_bytes: total(floors), demand_gpu_bytes: total(demand), ..Default::default() });
        let resources = Arc::new(std::sync::atomic::AtomicU64::new(partition(display.handle.grant().gpu_bytes, demand, floors, 4)));
        let memory = Self {
            display,
            samples: samples.map_or_else(|| Owner::new(factory, Kind::DecodedSamples, MemoryReport { pinned_ram_bytes: SOURCE_WORKING_BYTES, demand_ram_bytes: DECODED_SAMPLE_BYTES, ..Default::default() }),
                |handle| { let report = handle.reported(); Owner { handle, published: std::cell::Cell::new(report) } }),
            raster: Owner::new(factory, Kind::RasterPool, MemoryReport { pinned_gpu_bytes: working, pinned_ram_bytes: working, demand_gpu_bytes: working, demand_ram_bytes: working, ..Default::default() }),
            staging: Owner::new(factory, Kind::Staging, MemoryReport { pinned_gpu_bytes: working, demand_gpu_bytes: working, ..Default::default() }),
            demand: std::cell::Cell::new(demand), scene_retention: std::cell::Cell::new(CACHE_BYTES), demand_key: std::cell::Cell::new(None), active_owner: std::cell::Cell::new(None), applied_limits: std::cell::Cell::new(None), limits_dirty: std::cell::Cell::new(true), pinned: std::cell::Cell::new([0; PARTS]), sample_demand: std::cell::Cell::new(DECODED_SAMPLE_BYTES), source_inputs:std::cell::Cell::new([SOURCE_WORKING_BYTES,scene::scale::SOURCE_WORKING_BYTES+working]), preparation: std::cell::Cell::new(0), floors: std::cell::Cell::new(floors), resources, published_at: std::cell::Cell::new(web_time::Instant::now()),
            color_pinned: std::cell::Cell::new(0),
            idle: std::sync::atomic::AtomicBool::new(true),
        };
        memory.applied_limits.set(Some([memory.limit(Kind::Scene, false), memory.limit(Kind::RasterPool, false),
            memory.limit(Kind::Other, false), memory.limit(Kind::DecodedSamples, true), memory.limit(Kind::DecodedSamples, false),
            memory.limit(Kind::Staging, false), memory.limit(Kind::Mips, false)]));
        memory
    }
    pub fn display_owner(&self) -> MemoryOwner { self.display.handle.clone() }
    pub fn sample_owner(&self) -> MemoryOwner { self.samples.handle.clone() }
    pub fn resource_limit(&self) -> Arc<std::sync::atomic::AtomicU64> { self.resources.clone() }
    pub fn retire(&self, queue: &wgpu::Queue) {
        for owner in [&self.display, &self.raster, &self.staging] {
            let mut report = owner.published.get();
            report.pinned_gpu_bytes = report.gpu_bytes;
            owner.publish(report, true);
        }
        let owners = [self.display.handle.clone(), self.samples.handle.clone(), self.raster.handle.clone(), self.staging.handle.clone()];
        queue.on_submitted_work_done(move || drop(owners));
    }
    fn demand(&self, packet: FramePacket<'_>, display: DisplayDemand) -> Option<bool> {
        self.demand_region(packet.scene, packet.document_extent, display.bounds, display)
    }
    fn demand_region(&self, scene: SceneView<'_>, extent: [u32; 2], visible: PixelRect, display: DisplayDemand) -> Option<bool> {
        let rounded = DocRect::from(visible).aligned(PAGE_SIZE);
        if let Some(key) = self.demand_key.take() {
            if key.owner == scene.owner() && key.revision == scene.revision() && key.extent == extent
                && key.scope == *scene.scope().unwrap_or(&layer_core::SceneScope::All) && key.visible == rounded && key.display == display {
                self.demand_key.set(Some(key));
                return None;
            }
        }
        self.demand_key.set(Some(DemandKey {owner: scene.owner(), revision: scene.revision(), extent,
            scope: scene.scope().cloned().unwrap_or_default(), visible: rounded, display}));
        let activated = self.active_owner.replace(Some(scene.owner())) != Some(scene.owner());
        let visible = rounded.in_frame(extent);
        self.limits_dirty.set(true);
        let working = WORKING_BYTES;
        let pyramid = display_mips::Plan::at(extent, 0).pixel_bytes_through(display_mips::MAX_LEVEL);
        let window = PixelRect::new(visible.min_x(), visible.min_y(),
            visible.min_x().saturating_add(PAGE_SIZE).min(extent[0]), visible.min_y().saturating_add(PAGE_SIZE).min(extent[1]));
        let floor = scene::Scene::capture_image_bound(scene, window).max(working);
        let floors = [floor, 0, 0, working, working];
        self.floors.set(floors);
        let mut resources = 0u64;
        let mut native_sources=false;
        let mut luts = std::collections::HashSet::new();
        for &handle in scene.order() {
            native_sources |= scene.paint_source(handle).is_some_and(|paint|paint.base.is_some() || !paint.raster.is_empty() || !paint.operations.is_empty())
                || scene.mask(handle).is_some_and(|(_,mask)|!mask.raster.is_empty() || !mask.operations.is_empty());
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
        let mips = self.demand.get()[2];
        let hierarchy = if scene::scale::hierarchy_eligible(scene) { pyramid } else { 0 };
        let retention = CACHE_BYTES.saturating_add(hierarchy);
        self.scene_retention.set(retention);
        let windows = scene::Scene::capture_image_bound(scene, PixelRect::full(extent));
        let transform_sources=display.transforms.into_iter().flatten().map(|(plan,_)|DocRect::from(plan.bounds).aligned(PAGE_SIZE).area().saturating_mul(16).saturating_add(working))
            .max().unwrap_or(0).min(1024*MIB);
        let mut sources=self.source_inputs.get();sources[1]=transform_sources.max(if native_sources {scene::scale::SOURCE_WORKING_BYTES+working}else{0});
        self.source_inputs.set(sources);
        self.demand.set([retention.saturating_add(windows), OBJECT_CACHE_BYTES, mips, sources[0].max(sources[1]), resources.max(working)]);
        self.sample_demand.set(DECODED_SAMPLE_BYTES);
        Some(activated)
    }
    fn display_demand(&self) -> [u64; PARTS] {
        let mut demand = self.demand.get();
        demand[0] = demand[0].max(self.preparation.get());
        demand
    }
    pub(super) fn part_pins(&self) -> [u64; PARTS] {
        let actual = self.pinned.get();
        let floors = self.floors.get();
        std::array::from_fn(|part| if part == 0 || part == 3 { actual[part].saturating_add(floors[part]) } else { actual[part].max(floors[part]) })
    }
    pub fn mip_demand(&self, bytes: u64, viewport: [u32;2]) -> bool {
        let mut demand = self.demand.get();
        let mut inputs=self.source_inputs.get();inputs[0]=source_demand(viewport);self.source_inputs.set(inputs);
        let sources=inputs[0].max(inputs[1]);
        if demand[2] == bytes && demand[3] == sources { return false; }
        demand[2] = bytes;
        demand[3] = sources;
        self.demand.set(demand);
        self.limits_dirty.set(true);
        true
    }
    pub fn working_limit(&self) -> u64 { self.floors.get()[0] }
    pub fn invalidate_limits(&self) { self.limits_dirty.set(true); }
    pub fn preparation_limits(&self) -> [u64; 2] { self.applied_limits.get().map_or([0; 2], |limits| [limits[0], limits[1]]) }
    pub fn scene_limit(&self) -> u64 { self.applied_limits.get().unwrap()[0] }
    pub fn color_limit(&self, grant: u64, unused: u64) -> u64 {
        grant.saturating_sub(self.raster.published.get().pinned_gpu_bytes).saturating_sub(unused)
    }
    pub fn color_pinned(&self) -> u64 { self.color_pinned.get() }
    pub fn apply_limits(&self, limits: [u64; 7]) -> bool {
        let changed = self.applied_limits.replace(Some(limits)) != Some(limits);
        self.limits_dirty.replace(false) || changed
    }
    pub fn limit(&self, kind: Kind, ram: bool) -> u64 {
        let grant = match kind {
            Kind::RasterPool => self.raster.handle.grant(),
            Kind::Staging => self.staging.handle.grant(),
            Kind::DecodedSamples if ram => self.samples.handle.grant(),
            _ => self.display.handle.grant(),
        };
        if ram { return grant.ram_bytes; }
        let index = match kind { Kind::Scene => 0, Kind::Objects => 1, Kind::Mips => 2, Kind::DecodedSamples => 3,
            Kind::RasterPool | Kind::Staging => return grant.gpu_bytes, Kind::Other => 4, _ => return 0 };
        partition(grant.gpu_bytes, self.display_demand(), self.part_pins(), index)
    }
}

impl Drop for WgpuRasterizer {
    fn drop(&mut self) {
        self.publish_memory(true);
        self.memory.retire(&self.queue);
    }
}

impl WgpuRasterizer {
    pub(crate) fn memory_limit(&self, kind: Kind) -> u64 { self.memory.limit(kind, false) }
    pub(crate) fn ram_limit(&self, kind: Kind) -> u64 { self.memory.limit(kind, true) }
    pub(super) fn memory_demand(&self, packet: FramePacket<'_>) {
        let display = scene::scale::memory_key(self, packet);
        if let Some(activated) = self.memory.demand(packet, display) {
            self.publish_memory(activated);
            self.factory.memory_budget().flush_reports();
        }
    }
    pub(crate) fn begin_frame_memory(&mut self, packet: FramePacket<'_>) {
        self.memory_demand(packet);
        self.refresh_memory_limits();
    }
    pub(super) fn prepare_capture_memory(&mut self, scene: SceneView<'_>, extent: [u32; 2]) {
        self.memory.demand_region(scene, extent, PixelRect::full(extent), DisplayDemand::default());
        let mut sources=self.memory.source_inputs.get();sources[1]=scene::scale::SOURCE_WORKING_BYTES+WORKING_BYTES;self.memory.source_inputs.set(sources);
        let mut demand=self.memory.demand.get();demand[3]=sources[0].max(sources[1]);self.memory.demand.set(demand);
        self.publish_memory(true);
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
    }
    pub(super) fn prepare_capture_sources(&mut self,bytes:u64) {
        let mut sources=self.memory.source_inputs.get();
        let required=bytes.saturating_add(WORKING_BYTES);
        if required<=sources[1] {return;}
        sources[1]=required;self.memory.source_inputs.set(sources);
        let mut demand=self.memory.demand.get();demand[3]=sources[0].max(sources[1]);self.memory.demand.set(demand);
        self.publish_memory(true);self.factory.memory_budget().flush_reports();self.refresh_memory_limits();
    }
    pub(crate) fn reserve_effect_analysis(&mut self, bytes: u64) -> Result<crate::effect_analysis::Lease, String> {
        let required = (*self.device.analysis_memory.lock().unwrap()).saturating_add(bytes);
        if required <= self.device.resource_limit.load(std::sync::atomic::Ordering::Acquire) {
            return crate::effect_analysis::Lease::reserve(&self.device, bytes);
        }
        let mut demand = self.memory.demand.get();
        let resources = demand[4];
        demand[4] = resources.max(required);
        self.memory.demand.set(demand);
        self.publish_memory(true);
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
        let result = crate::effect_analysis::Lease::reserve(&self.device, bytes);
        demand[4] = resources;
        self.memory.demand.set(demand);
        self.publish_memory(true);
        self.factory.memory_budget().flush_reports();
        self.refresh_memory_limits();
        result
    }
    pub(crate) fn preparation_memory(&self, bytes: u64) {
        let before = self.memory.preparation.replace(if bytes == 0 { 0 } else { bytes.saturating_add(self.memory.working_limit()) });
        if before.abs_diff(self.memory.preparation.get()) >= MIB { self.publish_memory(false); }
    }
    pub(super) fn paint_memory_bytes(&self) -> u64 {
        self.metrics.paint_storage_bytes.saturating_add(self.metrics.paint_state_storage_bytes)
            .saturating_add(self.metrics.destination_storage_bytes).saturating_add(self.metrics.preview_storage_bytes)
    }
    pub(super) fn publish_motion_memory(&self) {
        if self.memory.published_at.get().elapsed() >= std::time::Duration::from_millis(250) { self.publish_memory(false); }
    }
    pub(super) fn publish_memory(&self, force: bool) {
        self.memory.published_at.set(web_time::Instant::now());
        let [mut ram, mut gpu] = self.scene.as_ref().map_or([0; 2], |scene| scene.memory_bytes());
        let source = self.source_tiles.borrow();
        ram = ram.saturating_add(source.cpu_bytes()).saturating_add(self.moving_images.cpu_bytes());
        let retired = self.retired_display_bytes.load(std::sync::atomic::Ordering::Acquire);
        let resources = *self.device.analysis_memory.lock().unwrap();
        let transforms = self.transforms.as_ref().map_or(0, paint_transform::PaintTransforms::storage_bytes);
        let pinned_transforms = if self.transform_preview.is_some() || self.transforms.as_ref().is_some_and(|transforms| transforms.has_preview()) { transforms } else { 0 };
        gpu = gpu.saturating_add(source.gpu_bytes()).saturating_add(self.moving_images.storage_bytes())
            .saturating_add(self.scale_display.as_ref().map_or(0, |cache| cache.storage_bytes()))
            .saturating_add(self.navigator.storage_bytes()).saturating_add(self.display_backup.as_ref().map_or(0, |backup| backup.bytes))
            .saturating_add(retired).saturating_add(resources).saturating_add(transforms);
        let pinned = [
            self.scale_display.as_ref().map_or(0, |cache| cache.presentation_bytes())
                .saturating_add(self.navigator.storage_bytes()).saturating_add(self.display_backup.as_ref().map_or(0, |backup| backup.bytes)).saturating_add(retired).saturating_add(pinned_transforms),
            self.scene.as_ref().map_or(0, |scene| scene.pinned_bytes()),
            self.moving_images.pinned_bytes(), source.pinned_bytes().saturating_add(source.non_slot_bytes()), resources,
        ];
        self.memory.pinned.set(pinned);
        let pinned = self.memory.part_pins();
        let display_changed = self.memory.display.publish(MemoryReport { ram_bytes: ram, gpu_bytes: gpu, pinned_gpu_bytes: total(pinned),
            demand_ram_bytes: ram, demand_gpu_bytes: total(std::array::from_fn(|part| self.memory.display_demand()[part].max(pinned[part]))), ..Default::default() }, force);
        let [ram, gpu] = self.raster_buffers.memory_bytes();
        let paint = self.paint_memory_bytes();
        let paint_pinned = self.pinned_paint_bytes();
        let color_pinned = self.color_memory_bytes().saturating_sub(paint.saturating_sub(paint_pinned));
        let raster_changed = self.memory.raster.publish(MemoryReport { ram_bytes: ram, gpu_bytes: gpu.saturating_add(paint),
            pinned_ram_bytes: self.raster_buffers.working.load(std::sync::atomic::Ordering::Acquire).max(WORKING_BYTES),
            pinned_gpu_bytes: self.raster_buffers.transfer.load(std::sync::atomic::Ordering::Acquire).saturating_add(paint_pinned).max(WORKING_BYTES),
            demand_ram_bytes: ram.max(WORKING_BYTES), demand_gpu_bytes: gpu.saturating_add(paint).max(self.memory.scene_retention.get()), ..Default::default() },
            force || self.memory.color_pinned.get().abs_diff(color_pinned) >= MIB);
        if raster_changed { self.memory.color_pinned.set(color_pinned); }
        let [uploads, uploads_pinned] = self.uploads.memory_bytes();
        let [restore, restore_pinned] = self.restore_uploads.memory_bytes();
        let staging_changed = self.memory.staging.publish(MemoryReport { gpu_bytes: uploads.saturating_add(restore),
            pinned_gpu_bytes: uploads_pinned.saturating_add(restore_pinned).max(WORKING_BYTES), demand_gpu_bytes: uploads.saturating_add(restore).max(WORKING_BYTES), ..Default::default() }, force);
        #[cfg(not(target_arch = "wasm32"))]
        if display_changed || raster_changed || staging_changed { self.factory.request_memory_update(); }
        #[cfg(target_arch = "wasm32")]
        let _ = (display_changed, raster_changed, staging_changed);
    }
    fn publish_samples(&self, force: bool) {
        let samples = self.device.source_samples.stats();
        if self.memory.samples.publish(MemoryReport { ram_bytes: samples.physical_bytes as u64,
            pinned_ram_bytes: (samples.pinned_bytes as u64).max(SOURCE_WORKING_BYTES),
            demand_ram_bytes: self.memory.sample_demand.get(), ..Default::default() }, force) {
            self.factory.request_memory_update();
        }
    }
    pub fn after_present(&mut self) {
        if !self.memory.idle.load(std::sync::atomic::Ordering::Acquire)
            || self.startup.as_ref().is_some_and(|startup| !startup.compiler.is_idle()) { return; }
        if !self.restore_preparation_pending() && !self.bake_preflight.as_ref().is_some_and(|state|state.pending()) { self.memory.preparation.set(0); }
        self.publish_memory(false);
        self.publish_samples(false);
        #[cfg(target_arch = "wasm32")]
        self.factory.request_memory_update();
        self.device.source_samples.set_limit(self.ram_limit(Kind::DecodedSamples).min(usize::MAX as u64) as usize);
        let decoded_trimmed = self.device.source_samples.trim_unused(SOURCE_SLOTS);
        self.raster_buffers.set_retained_limit(retained_limit(self.memory_limit(Kind::RasterPool), self.paint_memory_bytes()));
        let pool_trimmed = self.raster_buffers.trim_unused();
        let staging_limit = self.memory_limit(Kind::Staging);
        let uploads = self.uploads.memory_bytes()[0];
        let restore = self.restore_uploads.memory_bytes()[0];
        let retained = uploads.saturating_add(restore);
        let uploads_limit = if retained == 0 { 0 } else { ((u128::from(staging_limit) * u128::from(uploads)) / u128::from(retained)) as u64 };
        let staging_trimmed = self.uploads.trim_unused(uploads_limit) + self.restore_uploads.trim_unused(staging_limit.saturating_sub(uploads_limit));
        self.refresh_memory_limits();
        let mut encoder = submission::CommandEncoder::new(&self.device, &wgpu::CommandEncoderDescriptor { label: Some("memory trim") });
        let [ram, gpu] = [self.ram_limit(Kind::Objects), self.memory_limit(Kind::Objects)];
        let pages = self.trim_native_color_cache(&[], &[]);
        self.refresh_storage_metrics();
        let paint_retired = pages.iter().map(PageSurface::storage_bytes).sum::<u64>();
        if paint_retired != 0 {
            self.retired_display_bytes.fetch_add(paint_retired, std::sync::atomic::Ordering::AcqRel);
            let charge = DisplayRetirement { bytes: paint_retired, charge: self.retired_display_bytes.clone(),
                #[cfg(not(target_arch = "wasm32"))] _backup: None,
                #[cfg(not(target_arch = "wasm32"))] _scratch: (pages, Vec::new()) };
            encoder.on_submitted_work_done(move || drop(charge));
        }
        let retired = paint_retired + self.moving_images.trim_unused(self.memory_limit(Kind::Mips),
            self.moving_projection.as_ref().map_or(&[], |projection| &projection.requests), &mut encoder)
            + self.scene.as_mut().map_or(0, |scene| scene.trim_memory(ram, gpu, &mut encoder))
            + self.source_tiles.borrow_mut().trim_unused(&encoder);
        if retired != 0 { encoder.submit(&self.queue); }
        self.publish_memory(decoded_trimmed != 0 || pool_trimmed != 0 || staging_trimmed != 0 || retired != 0);
        if decoded_trimmed != 0 { self.publish_samples(true); }
        #[cfg(target_arch = "wasm32")]
        self.factory.request_memory_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_partitions_share_one_grant_without_multiplying_it() {
        for grant in [0, 1, 100, u64::MAX] {
            let demand = [100, 200, 300, 400, 500];
            let used: u64 = (0..PARTS).map(|index| partition(grant, demand, [0; PARTS], index)).sum();
            assert!(used <= grant.min(total(demand)));
        }
    }
    #[test]
    fn pressure_keeps_pinned_bytes_in_the_cache_that_owns_them() {
        let demand = [20 * MIB; PARTS];
        let pins = [16 * MIB, 0, 0, 0, 0];
        assert_eq!(std::array::from_fn::<_, PARTS, _>(|part| partition(16 * MIB, demand, pins, part)), pins);
        let limits: [u64; PARTS] = std::array::from_fn(|part| partition(32 * MIB, demand, pins, part));
        assert!(limits[0] >= pins[0]);
        assert!(total(limits) <= 32 * MIB);
        assert_eq!(partition(0, demand, pins, 0), pins[0]);
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
