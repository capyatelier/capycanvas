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
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    admission: admission::Admission,
    started: bool,
    busy: Option<u8>,
    error: Option<String>,
}
pub(crate) struct Compiler {
    queue: Rc<RefCell<Queue>>,
    device: PipelineDevice,
}
impl Compiler {
    pub fn input(&self) { self.queue.borrow_mut().admission.input(); }
    pub fn idle(&self, idle: bool) { self.queue.borrow_mut().admission.idle = idle; }
    pub fn delay(&self) -> std::time::Duration { self.queue.borrow().admission.delay() }
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
        self.queue.borrow_mut().jobs.push_back(Job {
            priority,
            work: Box::new(work),
            complete: None,
            batchable: false,
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
                    work.compile_async()
                }),
                complete: Some(Box::new(move || done.validating(false))),
                batchable: pipeline.async_pipeline(),
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
    pub fn has_work(&self, allow_optional: bool) -> bool {
        let queue = self.queue.borrow();
        queue.jobs.iter().any(|job| (allow_optional || job.priority < OTHER) && queue.admission.allows(job.priority))
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
    pub fn step(&self, allow_optional: bool) -> Pin<Box<dyn Future<Output = Result<(), GpuRasterError>>>> {
        let device = self.device.clone();
        let shared = self.queue.clone();
        Box::pin(async move {
            let jobs = {
                let mut queue = shared.borrow_mut();
                if queue.busy.is_some() || !queue.started || queue.jobs.is_empty() {
                    return Ok(());
                }
                let Some(index) = queue
                    .jobs
                    .iter()
                    .enumerate()
                    .filter(|(_, job)| (allow_optional || job.priority < OTHER) && queue.admission.allows(job.priority))
                    .min_by_key(|(_, j)| j.priority)
                    .map(|(index, _)| index) else { return Ok(()); };
                let job = queue.jobs.remove(index).unwrap();
                queue.busy = Some(job.priority);
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
            queue.busy = None;
            if let Err(error) = &result {
                queue.error = Some(error.clone());
            }
            result.map_err(GpuRasterError::Effect)
        })
    }
}
