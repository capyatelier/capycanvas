//! Bounded jobs per browser task. Futures own GPU handles, never a borrow of
//! the live renderer/session. Batch up to four required pipeline promises;
//! CPU recipes and effect transactions keep their individual task boundaries.
use super::*;
use std::{cell::RefCell, collections::VecDeque, future::Future, pin::Pin, rc::Rc};

type Completion = Pin<Box<dyn Future<Output = Result<(), String>>>>;
type Work = Box<dyn FnOnce() -> Completion>;
struct Job {
    priority: u8,
    work: Work,
    complete: Option<Box<dyn FnOnce()>>,
    batchable: bool,
    tracked: bool,
    overlaps: bool,
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    admission: admission::Admission,
    started: bool,
    busy: Option<(u8, bool)>,
    optional: Option<(u8, bool)>,
    waiting: Option<futures_channel::oneshot::Sender<()>>,
    error: Option<String>,
}
impl Queue {
    fn push(&mut self, job: Job) {
        if job.priority < OTHER && let Some(waiting) = self.waiting.take() { let _ = waiting.send(()); }
        self.jobs.push_back(job);
    }
    fn wait(&mut self) -> futures_channel::oneshot::Receiver<()> {
        let (sender, receiver) = futures_channel::oneshot::channel();
        self.waiting = Some(sender);
        receiver
    }
}
#[derive(Clone)]
pub(crate) struct Compiler {
    queue: Rc<RefCell<Queue>>,
    device: PipelineDevice,
}
impl Compiler {
    pub fn input(&self) { self.queue.borrow_mut().admission.input(); }
    pub fn idle(&self, idle: bool, speculative_idle: bool) {
        let mut queue = self.queue.borrow_mut();
        queue.admission.idle = idle;
        queue.admission.speculative_idle = speculative_idle;
    }
    pub fn is_idle(&self)->bool {self.queue.borrow().admission.idle}
    pub fn delay(&self) -> std::time::Duration {
        let queue = self.queue.borrow();
        queue.admission.delay(queue.jobs.iter().map(|job| job.priority))
    }
    pub fn new(device: &PipelineDevice) -> Result<Self, GpuRasterError> {
        Ok(Self {
            queue: Rc::default(),
            device: device.clone(),
        })
    }
    pub fn enqueue(&self, priority: u8, work: impl FnOnce() -> Result<(), String> + 'static) {
        self.enqueue_async(priority, move || Box::pin(std::future::ready(work())));
    }
    pub fn enqueue_async(&self, priority: u8, work: impl FnOnce() -> Completion + 'static) {
        self.queue.borrow_mut().push(Job {
            priority,
            work: Box::new(work),
            complete: None,
            batchable: false,
            tracked: true,
            overlaps: false,
        });
    }
    pub fn pipeline<T: 'static>(&self, pipeline: &Deferred<T>, priority: u8) {
        if let Some(promotion) = pipeline.promote(priority) {
            let work = pipeline.clone();
            let done = Rc::new(pipeline.validation());
            let validation = done.clone();
            self.queue.borrow_mut().push(Job {
                priority,
                work: Box::new(move || {
                    validation.begin();
                    work.compile_async()
                }),
                complete: Some(Box::new(move || { drop(done); drop(promotion); })),
                batchable: pipeline.async_pipeline(),
                tracked: true,
                overlaps: priority >= OTHER && pipeline.async_pipeline(),
            });
        }
    }
    /// Optional preparation that startup completion does not wait for; it is
    /// skipped once `current` reports a newer preparation.
    pub fn background<T: 'static>(&self, pipeline: &Deferred<T>, priority: u8, current: impl Fn() -> bool + Send + 'static) {
        if let Some(promotion) = pipeline.promote(priority) {
            let work = pipeline.clone();
            let done = Rc::new(pipeline.validation());
            let validation = done.clone();
            self.queue.borrow_mut().push(Job {
                priority,
                work: Box::new(move || {
                    if !current() { return Box::pin(std::future::ready(Ok(()))); }
                    validation.begin();
                    work.compile_async()
                }),
                complete: Some(Box::new(move || { drop(done); drop(promotion); })),
                batchable: false,
                tracked: false,
                overlaps: priority >= OTHER && pipeline.async_pipeline(),
            });
        }
    }
    pub fn require<'a, T: 'static>(&self, pipelines: impl IntoIterator<Item = &'a Deferred<T>>, priority: u8) -> bool {
        pipelines.into_iter().fold(true, |ready, p| { self.pipeline(p, priority); ready & p.ready() })
    }
    pub fn start(&self) {
        self.queue.borrow_mut().started = true;
    }
    pub fn pending(&self) -> usize {
        let queue = self.queue.borrow();
        queue.jobs.iter().filter(|job| job.tracked).count()
            + [queue.busy, queue.optional].iter().filter(|running| running.is_some_and(|(_, tracked)| tracked)).count()
    }
    pub fn has_work(&self, allow_optional: bool) -> bool {
        let queue = self.queue.borrow();
        allow_optional && queue.optional.is_some()
            || queue.jobs.iter().any(|job| (allow_optional || job.priority < OTHER) && queue.admission.allows(job.priority))
    }
    pub fn ready_through(&self, priority: u8) -> bool {
        let queue = self.queue.borrow();
        ![queue.busy, queue.optional].iter().any(|running| running.is_some_and(|(p, _)| p <= priority))
            && !queue.jobs.iter().any(|j| j.priority <= priority)
    }
    pub fn check(&self) -> Result<(), GpuRasterError> {
        self.queue
            .borrow()
            .error
            .as_ref()
            .map_or(Ok(()), |e| Err(GpuRasterError::Effect(e.clone())))
    }
    pub fn step(&self, allow_optional: bool) -> Pin<Box<dyn Future<Output = Result<(), GpuRasterError>>>> {
        let device = self.device.clone();
        let shared = self.queue.clone();
        Box::pin(async move {
            let jobs = {
                let mut queue = shared.borrow_mut();
                if queue.busy.is_some() || !queue.started {
                    return Ok(());
                }
                let Some(index) = queue
                    .jobs
                    .iter()
                    .enumerate()
                    .filter(|(_, job)| (allow_optional || job.priority < OTHER) && queue.admission.allows(job.priority)
                        && (job.priority < OTHER || queue.optional.is_none()))
                    .min_by_key(|(_, j)| j.priority)
                    .map(|(index, _)| index) else {
                        if queue.optional.is_none() { return Ok(()); }
                        let wait = queue.wait();
                        drop(queue);
                        let _ = wait.await;
                        return Ok(());
                    };
                let job = queue.jobs.remove(index).unwrap();
                if job.overlaps {
                    queue.optional = Some((job.priority, job.tracked));
                    let wait = queue.wait();
                    drop(queue);
                    wasm_bindgen_futures::spawn_local(async move { let _ = run(device, shared, vec![job]).await; });
                    let _ = wait.await;
                    return Ok(());
                }
                queue.busy = Some((job.priority, job.tracked));
                let mut jobs = vec![job];
                if jobs[0].batchable && jobs[0].priority < OTHER {
                    while jobs.len() < 4 {
                        let Some(index) = queue
                            .jobs
                            .iter()
                            .position(|job| job.batchable && job.priority == jobs[0].priority)
                        else {
                            break;
                        };
                        jobs.push(queue.jobs.remove(index).unwrap());
                    }
                }
                jobs
            };
            run(device, shared, jobs).await.map_err(GpuRasterError::Effect)
        })
    }
}

async fn run(device: PipelineDevice, shared: Rc<RefCell<Queue>>, jobs: Vec<Job>) -> Result<(), String> {
    let overlapped = jobs[0].overlaps;
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let work: Vec<_> = jobs
        .into_iter()
        .map(|job| ((job.work)(), job.complete))
        .collect();
    // Pop in this task before yielding so unrelated rendering never
    // lands inside the compiler's scopes. Await only owned futures.
    let validation = validation.pop();
    let memory = memory.pop();
    let internal = internal.pop();
    let mut result = Ok(());
    let mut completed = Vec::new();
    for (future, complete) in work {
        // All promises have started. Drain every result even on failure,
        // keeping handles private until this batch's scopes resolve.
        result = result.and(future.await);
        completed.push(complete);
    }
    let a = validation.await;
    let b = memory.await;
    let c = internal.await;
    let result = result.and_then(|()| a.or(b).or(c).map_or(Ok(()), |e| Err(e.to_string())));
    for done in completed.into_iter().flatten() {
        done();
    }
    let mut queue = shared.borrow_mut();
    let (priority, _) = if overlapped { queue.optional.take() } else { queue.busy.take() }.unwrap();
    queue.admission.finished(priority);
    if let Err(error) = &result {
        queue.error.get_or_insert_with(|| error.clone());
    }
    if overlapped && let Some(waiting) = queue.waiting.take() { let _ = waiting.send(()); }
    result
}
