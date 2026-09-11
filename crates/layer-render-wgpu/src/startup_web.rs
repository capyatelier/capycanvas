//! One bounded job per browser task. Futures own GPU handles, never a borrow of
//! the live renderer/session. The host yields a frame between compilation jobs.
use super::*;
use std::{cell::RefCell, collections::VecDeque, future::Future, pin::Pin, rc::Rc};

type Work = Box<dyn FnOnce() -> Result<(), String>>;
type MaskPixels = Rc<RefCell<Option<Result<builtin_masks::Pixels, GpuRasterError>>>>;
pub(super) struct Masks(Vec<(AssetId, Deferred<MaskPixels>)>);
impl Masks {
    pub fn new() -> Self {
        Self(
            builtin_masks()
                .into_iter()
                .map(|(id, generate)| {
                    (
                        AssetId::from(id),
                        Deferred::new(move || Rc::new(RefCell::new(Some(generate())))),
                    )
                })
                .collect(),
        )
    }
    pub fn style(&self, compiler: &Compiler, style: &layer_render::DabStyle, priority: u8) {
        let key = WgpuRasterizer::texture_set_key(style);
        for id in [
            key.primary,
            key.grain,
            key.dual,
            key.dual_grain,
            key.transport,
        ] {
            if let Some((_, pixels)) = self.0.iter().find(|(asset, _)| *asset == id) {
                compiler.pipeline(pixels, priority);
            }
        }
    }
    pub fn remaining(&self, compiler: &Compiler) {
        for (_, pixels) in &self.0 {
            compiler.pipeline(pixels, OTHER);
        }
    }
    pub fn take_ready(&mut self) -> Result<Vec<(AssetId, builtin_masks::Pixels)>, GpuRasterError> {
        let mut ready = Vec::new();
        for (id, pixels) in &self.0 {
            if pixels.ready() {
                if let Some(result) = pixels.compile().borrow_mut().take() {
                    ready.push((id.clone(), result?));
                }
            }
        }
        Ok(ready)
    }
}
struct Job {
    priority: u8,
    work: Work,
    complete: Option<Box<dyn FnOnce()>>,
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    started: bool,
    busy: Option<u8>,
    error: Option<String>,
}
pub(crate) struct Compiler {
    queue: Rc<RefCell<Queue>>,
    device: PipelineDevice,
}
impl Compiler {
    pub fn new(device: &PipelineDevice) -> Result<Self, GpuRasterError> {
        Ok(Self {
            queue: Rc::default(),
            device: device.clone(),
        })
    }
    pub fn enqueue(&self, priority: u8, work: impl FnOnce() -> Result<(), String> + 'static) {
        self.queue.borrow_mut().jobs.push_back(Job {
            priority,
            work: Box::new(work),
            complete: None,
        });
    }
    pub fn pipeline<T: 'static>(&self, pipeline: &Deferred<T>, priority: u8) {
        if pipeline.promote(priority) {
            let work = pipeline.clone();
            let done = pipeline.clone();
            self.queue.borrow_mut().jobs.push_back(Job {
                priority,
                work: Box::new(move || {
                    work.validating(true);
                    work.compile();
                    Ok(())
                }),
                complete: Some(Box::new(move || done.validating(false))),
            });
        }
    }
    pub fn start(&self) {
        self.queue.borrow_mut().started = true;
    }
    pub fn pending(&self) -> usize {
        let queue = self.queue.borrow();
        queue.jobs.len() + usize::from(queue.busy.is_some())
    }
    pub fn ready_through(&self, priority: u8) -> bool {
        let queue = self.queue.borrow();
        !queue.busy.is_some_and(|p| p <= priority)
            && !queue.jobs.iter().any(|j| j.priority <= priority)
    }
    pub fn check(&self) -> Result<(), GpuRasterError> {
        self.queue
            .borrow()
            .error
            .as_ref()
            .map_or(Ok(()), |e| Err(GpuRasterError::Effect(e.clone())))
    }
    pub fn step(&self) -> Pin<Box<dyn Future<Output = Result<(), GpuRasterError>>>> {
        let device = self.device.clone();
        let shared = self.queue.clone();
        Box::pin(async move {
            let job = {
                let mut queue = shared.borrow_mut();
                if queue.busy.is_some() || !queue.started || queue.jobs.is_empty() {
                    return Ok(());
                }
                let index = queue
                    .jobs
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, j)| j.priority)
                    .unwrap()
                    .0;
                let job = queue.jobs.remove(index).unwrap();
                queue.busy = Some(job.priority);
                job
            };
            let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
            let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let result = (job.work)();
            // Pop in this task before yielding so unrelated rendering never
            // lands inside the compiler's scopes. Await only owned futures.
            let validation = validation.pop();
            let memory = memory.pop();
            let internal = internal.pop();
            let a = validation.await;
            let b = memory.await;
            let c = internal.await;
            let result = result.and_then(|()| a.or(b).or(c).map_or(Ok(()), |e| Err(e.to_string())));
            if let Some(done) = job.complete {
                done();
            }
            let mut queue = shared.borrow_mut();
            queue.busy = None;
            if let Err(error) = &result {
                queue.error = Some(error.clone());
            }
            result.map_err(GpuRasterError::Effect)
        })
    }
}
