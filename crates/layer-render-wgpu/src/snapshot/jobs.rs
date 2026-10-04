use super::*;

#[cfg(not(target_arch = "wasm32"))]
impl SnapshotGpu {
    async fn run_snapshot(&self, request: layer_render::SnapshotRequest, control: CaptureControl)
        -> Result<layer_render::SnapshotResult, String> {
        match request {
            layer_render::SnapshotRequest::LevelsStatistics(query)=>self.levels_statistics(query,control).await.map(layer_render::SnapshotResult::LevelsStatistics),
            layer_render::SnapshotRequest::ArtworkStatistics(request) => self.artwork_statistics(request, control).await.map(layer_render::SnapshotResult::ArtworkStatistics),
            layer_render::SnapshotRequest::ArtworkSample(request) => self.artwork_sample(request, control).await.map(layer_render::SnapshotResult::ArtworkSample),
            layer_render::SnapshotRequest::Bounds(request) => self.content_bounds(request, control).await.map(layer_render::SnapshotResult::Bounds),
            layer_render::SnapshotRequest::TransformPixels(plan) => self.transform_pixels(plan, control).await
                .map(layer_render::SnapshotResult::TransformPixels),
        }
    }
}

pub struct SnapshotJob {
    control: CaptureControl,
    receiver: mpsc::Receiver<Result<layer_render::SnapshotResult, String>>,
}
impl Drop for SnapshotJob {
    fn drop(&mut self) { self.control.cancel(); }
}
#[cfg(target_arch = "wasm32")]
pub type BrowserSnapshot = std::rc::Rc<dyn Fn(layer_render::SnapshotRequest, CaptureControl)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<layer_render::SnapshotResult, String>>>>>;

impl SnapshotJob {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(gpu: SnapshotGpu, request: layer_render::SnapshotRequest) -> Result<Self, String> {
        let control = CaptureControl::default();
        let capture = control.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("capy-snapshot".into()).stack_size(8 * 1024 * 1024).spawn(move || {
            let result = pollster::block_on(gpu.run_snapshot(request, capture));
            let _ = sender.send(result);
        }).map_err(|e| e.to_string())?;
        Ok(Self { control, receiver })
    }
    #[cfg(target_arch = "wasm32")]
    pub fn start(worker: BrowserSnapshot, request: layer_render::SnapshotRequest) -> Result<Self, String> {
        let control = CaptureControl::default();
        let capture = control.clone();
        let (sender, receiver) = mpsc::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = sender.send(worker(request, capture).await);
        });
        Ok(Self { control, receiver })
    }
    pub fn take(&mut self) -> Option<Result<layer_render::SnapshotResult, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The operation stopped".into())),
        }
    }
}
