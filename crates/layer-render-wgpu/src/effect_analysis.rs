use super::*;
use layer_core::{ArtworkQuery, ArtworkSource, EffectAnalysisKind};

pub(crate) struct Prepared {
    pub query: ArtworkQuery,
    pub kind: EffectAnalysisKind,
    pub resource: Arc<effects::resources::Resource>,
}
impl Prepared {
    pub fn layer(&self) -> OccurrenceHandle {
        let ArtworkSource::EffectInput(id) = self.query.source else { unreachable!() }; id
    }
    pub fn matches(&self, scene: &SceneSnapshot, _gpu: &WgpuRasterizer) -> bool {
        self.query.matches_snapshot(scene)
    }
}
impl WgpuRasterizer {
    pub(crate) fn analysis_bytes(&self) -> u64 {
        let mut resources = std::collections::HashSet::new();
        self.effect_analyses.iter().filter(|analysis| resources.insert(Arc::as_ptr(&analysis.resource)))
            .map(|analysis| analysis.resource.buffer.size()).sum()
    }
}

#[derive(Clone)]
pub(crate) struct BakeInput {
    pub scene: Arc<SceneSnapshot>,
    pub scope: SceneScope,
    pub offset: layer_core::Point,
    pub extent: [u32; 2],
    pub color: layer_core::color::DocumentColor,
    pub blend: layer_core::BlendSpace,
    pub time: f32,
}
impl BakeInput {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.scene, &other.scene) && self.scope == other.scope && self.offset == other.offset && self.extent == other.extent
            && self.color == other.color && self.blend == other.blend
            && (self.time == other.time || !self.scene.view().order().iter().any(|&h| self.scene.view().with_scope(&self.scope).visible(h) && self.scene.view().effect(h).is_some_and(|effect| effect.animated())))
    }
}
pub(crate) struct BakeTask {
    input: BakeInput,
    job: Option<Job>,
    result: Option<Result<Candidate, String>>,
}
impl BakeTask {
    pub(crate) fn set_wake(&self,wake:Arc<dyn Fn()+Send+Sync>) {
        if let Some(job)=&self.job {job.set_wake(wake);}
    }
}
enum Request { Artwork(ArtworkQuery), Bake(BakeInput) }
pub struct Candidate {
    pub(crate) entries: Vec<Arc<Prepared>>,
    pub(crate) masks: Option<Arc<layer_masks::SnapshotMasks>>,
}
pub type Job = snapshot::Job<Candidate>;
impl Job {
    pub fn start(gpu: snapshot::SnapshotGpu, query: ArtworkQuery) -> Result<Self, String> {
        Self::analyze(gpu, Request::Artwork(query))
    }
    pub(crate) fn frame(gpu: snapshot::SnapshotGpu, input: BakeInput) -> Result<Self, String> { Self::analyze(gpu, Request::Bake(input)) }
    fn analyze(gpu: snapshot::SnapshotGpu, request: Request) -> Result<Self, String> {
        let wake=gpu.preparation_wake.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let job=Self::spawn("capy-effect-analysis", move |control| pollster::block_on(run(gpu, request, control)))?;
        #[cfg(target_arch = "wasm32")]
        let job=Self::spawn_local(move |control| run(gpu, request, control));
        if let Some(wake)=wake {job.set_wake(wake);}
        Ok(job)
    }
}
async fn run(gpu: snapshot::SnapshotGpu, request: Request, control: snapshot::CaptureControl) -> Result<Candidate, String> {
    match request {
        Request::Artwork(query) => gpu.effect_analysis(query, control).await,
        Request::Bake(input) => gpu.bake_analysis(input, control).await,
    }
}
#[cfg(target_arch = "wasm32")]
pub type BackingWaiter = std::rc::Rc<dyn Fn(Arc<SceneSnapshot>, snapshot::CaptureControl)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>>;

impl WgpuRasterizer {
    pub fn apply_effect_analysis(&mut self, candidate: Candidate) {
        for entry in candidate.entries {
            self.effect_analyses.retain(|old| old.layer() != entry.layer());
            self.effect_analyses.push(entry);
        }
        self.analysis_changed();
    }
    pub(crate) fn analysis_changed(&mut self) {
        self.analysis_dirty = true;
        if let Some(scene) = &mut self.scene { scene.analysis_changed(); }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn set_analysis_backing_waiter(&mut self, waiter: BackingWaiter) { self.analysis_backing_waiter = Some(waiter); }
}

impl WgpuRasterizer {
    pub(crate) fn bake_analyses_ready(&mut self, packet: FramePacket<'_>) -> bool {
        let mut inputs = Vec::new();
        for target in source_access::placed_targets(packet.scene) {
            for operation in packet.scene.operations(target).into_iter().flatten() {
                let (scene, scope, offset) = match &operation.kind {
                    layer_core::RasterOperationKind::Bake {scene, scope, offset}
                    | layer_core::RasterOperationKind::FrequencyDetail {scene, scope, offset, ..} => (scene, scope, *offset), _ => continue,
                };
                if !scene.view().order().iter().any(|&h| scene.view().with_scope(scope).visible(h) && scene.view().effect(h).is_some_and(|effect| effect.program.analysis().is_some()))
                    && !bake_needs_masks(scene, scope) { continue; }
                let input = BakeInput {scene: scene.clone(), scope: scope.clone(), offset, extent: packet.scene.target_extent(target),
                    color: self.document_color, blend: scene.view().composition().blend, time: scene.context.elapsed};
                if !inputs.iter().any(|old: &BakeInput| old.matches(&input)) { inputs.push(input); }
            }
        }
        self.bake_analyses.retain(|task| inputs.iter().any(|input| task.input.matches(input)));
        for input in inputs {
            if let Some(task) = self.bake_analyses.iter_mut().find(|task| task.input.matches(&input)) {
                if let Some(job) = &mut task.job {
                    if let Some(result) = job.take() { task.result = Some(result); task.job = None; }
                    else { return false; }
                }
            } else {
                let result = Job::frame(self.snapshot_gpu(), input.clone());
                let (job, result) = match result { Ok(job) => (Some(job), None), Err(error) => (None, Some(Err(error))) };
                let pending = job.is_some();
                self.bake_analyses.push(BakeTask {input, job, result});
                if pending { return false; }
            }
        }
        true
    }
    pub(crate) fn bake_mask_pages(&self, scene: &Arc<SceneSnapshot>, scope: &SceneScope, offset: layer_core::Point, extent: [u32;2])
        -> Result<Option<Arc<layer_masks::SnapshotMasks>>, GpuRasterError> {
        if !bake_needs_masks(scene, scope) { return Ok(None); }
        let task = self.bake_analyses.iter().find(|task| Arc::ptr_eq(&task.input.scene, scene) && task.input.scope == *scope
            && task.input.offset == offset && task.input.extent == extent)
            .ok_or_else(|| GpuRasterError::Effect("Merge mask backing is not ready".into()))?;
        match &task.result {
            Some(Ok(candidate)) => Ok(candidate.masks.clone()),
            Some(Err(error)) => Err(GpuRasterError::Effect(error.clone())),
            None => Err(GpuRasterError::Effect("Merge mask backing is not ready".into())),
        }
    }
    pub(crate) fn bake_analysis_entries(&self, scene: &Arc<SceneSnapshot>, scope: &SceneScope, offset: layer_core::Point, extent: [u32;2])
        -> Result<Option<Vec<Arc<Prepared>>>, GpuRasterError> {
        if !scene.view().order().iter().any(|&h| scene.view().with_scope(scope).visible(h) && scene.view().effect(h).is_some_and(|effect| effect.program.analysis().is_some())) { return Ok(None); }
        let task = self.bake_analyses.iter().find(|task| Arc::ptr_eq(&task.input.scene, scene) && task.input.scope == *scope
            && task.input.offset == offset && task.input.extent == extent)
            .ok_or_else(|| GpuRasterError::Effect("Merge analysis is not ready".into()))?;
        match &task.result {
            Some(Ok(candidate)) => Ok(Some(candidate.entries.clone())),
            Some(Err(error)) => Err(GpuRasterError::Effect(error.clone())),
            None => Err(GpuRasterError::Effect("Merge analysis is not ready".into())),
        }
    }
}

fn bake_needs_masks(scene: &Arc<SceneSnapshot>, scope: &SceneScope) -> bool {
    snapshot::capture_targets(scene.view().with_scope(scope), scope).into_iter().any(|target| match target {
        SourceTarget::Coverage(handle) => scene.view().coverage(handle).is_some_and(|coverage| !coverage.raster.is_empty()),
        _ => false,
    })
}

#[derive(Debug)]
pub(crate) struct Lease(crate::residency::Charge);
impl Lease {
    pub fn reserve(device: &PipelineDevice, bytes: u64) -> Result<Self, String> {
        device.analysis_memory.charge_within(device.resource_limit.load(std::sync::atomic::Ordering::Acquire), bytes).map(Self)
            .ok_or_else(|| "Effect analysis exceeds available GPU memory".into())
    }
    pub fn split(&mut self, bytes: u64) -> Arc<Self> {
        Arc::new(Self(self.0.split(bytes).expect("guide exceeds reserved analysis storage")))
    }
}

#[cfg(test)]
#[path = "effect_analysis_lease_tests.rs"]
mod lease_tests;
