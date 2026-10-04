use super::*;
use effects::Gpu;
use layer_core::{ArtworkQuery, ArtworkSource, EffectAnalysisKind};

pub(crate) struct Prepared {
    pub query: ArtworkQuery,
    pub kind: EffectAnalysisKind,
    pub resource: Arc<effects::resources::Resource>,
}
impl Prepared {
    pub fn layer(&self) -> LayerId {
        let ArtworkSource::EffectInput(id) = self.query.source else { unreachable!() }; id
    }
    pub fn matches(&self, document: &layer_core::Document, time: f32, gpu: &WgpuRasterizer) -> bool {
        self.query.matches_source(document) && self.query.effect_times.iter().all(|(id, phase)|
            document.layer(*id).is_some_and(|layer| gpu.effect_time(layer, time) == *phase))
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
    pub members: Arc<[Layer]>,
    pub offset: layer_core::Point,
    pub extent: [u32; 2],
    pub color: layer_core::color::DocumentColor,
    pub blend: layer_core::BlendSpace,
    pub time: f32,
}
impl BakeInput {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.members, &other.members) && self.offset == other.offset && self.extent == other.extent
            && self.color == other.color && self.blend == other.blend
            && (self.time == other.time || !self.members.iter().any(|layer| layer.effect.as_ref().is_some_and(|effect| effect.animated())))
    }
}
pub(crate) struct BakeTask {
    input: BakeInput,
    job: Option<Job>,
    result: Option<Result<Candidate, String>>,
}
enum Request { Layer(ArtworkQuery), Bake(BakeInput) }
pub struct Candidate { pub(crate) entries: Vec<Arc<Prepared>> }
pub struct Job {
    control: snapshot::CaptureControl,
    receiver: std::sync::mpsc::Receiver<Result<Candidate, String>>,
}
impl Drop for Job { fn drop(&mut self) { self.control.cancel(); } }
impl Job {
    pub fn start(gpu: snapshot::SnapshotGpu, query: ArtworkQuery) -> Result<Self, String> {
        Self::spawn(gpu, Request::Layer(query))
    }
    pub(crate) fn frame(gpu: snapshot::SnapshotGpu, input: BakeInput) -> Result<Self, String> { Self::spawn(gpu, Request::Bake(input)) }
    fn spawn(gpu: snapshot::SnapshotGpu, request: Request) -> Result<Self, String> {
        let control = snapshot::CaptureControl::default();
        let capture = control.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::Builder::new().name("capy-effect-analysis".into()).stack_size(8 * 1024 * 1024).spawn(move || {
            let _ = sender.send(pollster::block_on(Self::run(gpu, request, capture)));
        }).map_err(|e| e.to_string())?;
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move { let _ = sender.send(Self::run(gpu, request, capture).await); });
        Ok(Self { control, receiver })
    }
    async fn run(gpu: snapshot::SnapshotGpu, request: Request, control: snapshot::CaptureControl) -> Result<Candidate, String> {
        match request {
            Request::Layer(query) => gpu.effect_analysis(query, control).await,
            Request::Bake(input) => gpu.bake_analysis(input, control).await,
        }
    }
    pub fn take(&mut self) -> Option<Result<Candidate, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("Effect analysis stopped".into())),
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub type BackingWaiter = std::rc::Rc<dyn Fn(Arc<layer_core::Document>, snapshot::CaptureControl)
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
        self.scale_display = None;
    }
    #[cfg(target_arch = "wasm32")]
    pub fn set_analysis_backing_waiter(&mut self, waiter: BackingWaiter) { self.analysis_backing_waiter = Some(waiter); }
}

impl WgpuRasterizer {
    pub(crate) fn bake_analyses_ready(&mut self, packet: FramePacket<'_>) -> bool {
        let mut inputs = Vec::new();
        for layer in packet.layers {
            for operation in &layer.pending_operations {
                let (members, offset) = match &operation.kind {
                    layer_core::LayerOperationKind::Bake {members, offset}
                    | layer_core::LayerOperationKind::FrequencyDetail {members, offset, ..} => (members, *offset),
                    _ => continue,
                };
                if !members.iter().any(|layer| layer.effect.as_ref().is_some_and(|effect| effect.program.analysis().is_some())) { continue; }
                let input = BakeInput {members: members.clone(), offset, extent: layer.local_extent(packet.document_extent),
                    color: self.document_color, blend: packet.blend_space, time: packet.time_seconds};
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
                let result = Job::spawn(self.snapshot_gpu(), Request::Bake(input.clone()));
                let (job, result) = match result { Ok(job) => (Some(job), None), Err(error) => (None, Some(Err(error))) };
                let pending = job.is_some();
                self.bake_analyses.push(BakeTask {input, job, result});
                if pending { return false; }
            }
        }
        true
    }
    pub(crate) fn bake_analysis_entries(&self, members: &[Layer], offset: layer_core::Point, extent: [u32;2])
        -> Result<Option<Vec<Arc<Prepared>>>, GpuRasterError> {
        if !members.iter().any(|layer| layer.effect.as_ref().is_some_and(|effect| effect.program.analysis().is_some())) { return Ok(None); }
        let task = self.bake_analyses.iter().find(|task| std::ptr::eq(task.input.members.as_ref(), members)
            && task.input.offset == offset && task.input.extent == extent)
            .ok_or_else(|| GpuRasterError::Effect("Merge analysis is not ready".into()))?;
        match &task.result {
            Some(Ok(candidate)) => Ok(Some(candidate.entries.clone())),
            Some(Err(error)) => Err(GpuRasterError::Effect(error.clone())),
            None => Err(GpuRasterError::Effect("Merge analysis is not ready".into())),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Lease { counter: Arc<std::sync::Mutex<u64>>, bytes: u64 }
impl Lease {
    pub fn reserve(device: &PipelineDevice, bytes: u64) -> Result<Self, String> {
        let mut retained = device.analysis_memory.lock().unwrap();
        #[cfg(target_arch = "wasm32")]
        let budget = 512 * 1024 * 1024;
        #[cfg(not(target_arch = "wasm32"))]
        let budget = crate::display_memory::resource_budget(device, *retained);
        if retained.saturating_add(bytes) > budget { return Err("Effect analysis exceeds available GPU memory".into()); }
        *retained += bytes;
        Ok(Self {counter: device.analysis_memory.clone(), bytes})
    }
    pub fn split(&mut self, bytes: u64) -> Arc<Self> {
        self.bytes = self.bytes.checked_sub(bytes).expect("guide exceeds reserved analysis storage");
        Arc::new(Self {counter: self.counter.clone(), bytes})
    }
}
impl Drop for Lease { fn drop(&mut self) { *self.counter.lock().unwrap() -= self.bytes; } }

#[cfg(test)]
#[path = "effect_analysis_lease_tests.rs"]
mod lease_tests;
