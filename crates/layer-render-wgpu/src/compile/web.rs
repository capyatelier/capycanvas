use super::*;
use crate::PipelineDevice;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, sync::Arc};

type Completion = Pin<Box<dyn Future<Output = Result<(), String>>>>;
pub(super) struct Work {
    begin: Box<dyn FnOnce() -> Completion>,
    complete: Option<Box<dyn FnOnce(&Result<(), String>)>>,
}
pub(crate) struct Coordinator {
    queue: RefCell<Queue<Work>>,
    device: PipelineDevice,
    waiting: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
}
impl Coordinator {
    pub fn new(device: &PipelineDevice) -> Result<Arc<Self>, GpuRasterError> {
        let coordinator = Arc::new(Self {
            queue: RefCell::new(Queue::new(4)),
            device: device.for_recipe(),
            waiting: RefCell::default(),
        });
        Ok(coordinator)
    }
    pub(super) fn with_queue<T>(&self, work: impl FnOnce(&mut Queue<Work>) -> T) -> T {
        work(&mut self.queue.borrow_mut())
    }
    pub(super) fn notify(&self) {
        for waiting in std::mem::take(&mut *self.waiting.borrow_mut()) {
            let _ = waiting.send(());
        }
    }
    pub(super) fn notify_one(&self) {
        self.notify();
    }
    pub(super) fn drain(&self) {
        self.notify();
    }
    fn wait(&self) -> futures_channel::oneshot::Receiver<()> {
        let (sender, receiver) = futures_channel::oneshot::channel();
        self.waiting.borrow_mut().push(sender);
        receiver
    }
    pub(super) fn step(
        self: &Arc<Self>,
        allow_optional: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), GpuRasterError>>>> {
        let shared = self.clone();
        Box::pin(async move {
            let jobs = {
                let mut queue = shared.queue.borrow_mut();
                if queue.required != 0 {
                    let receiver = shared.wait();
                    drop(queue);
                    let _ = receiver.await;
                    return Ok(());
                }
                let Some(first) = queue.take(Instant::now(), allow_optional) else {
                    if queue.optional == 0 {
                        return Ok(());
                    }
                    let receiver = shared.wait();
                    drop(queue);
                    let _ = receiver.await;
                    return Ok(());
                };
                let mut jobs = vec![first];
                if jobs[0].class.required() && jobs[0].independent {
                    while jobs.len() < 4 {
                        let Some(key) = queue.selected(Instant::now(), false) else {
                            break;
                        };
                        let entry = &queue.entries[&key];
                        if !entry.independent || entry.class != jobs[0].class {
                            break;
                        }
                        let Some(job) = queue.take(Instant::now(), false) else {
                            break;
                        };
                        jobs.push(job);
                    }
                }
                jobs
            };
            if !jobs[0].class.required() && jobs[0].independent {
                let receiver = shared.wait();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = run(shared, jobs).await;
                });
                let _ = receiver.await;
                Ok(())
            } else {
                run(shared, jobs).await.map_err(GpuRasterError::Effect)
            }
        })
    }
}
async fn run(shared: Arc<Coordinator>, jobs: Vec<Running<Work>>) -> Result<(), String> {
    let internal = shared.device.push_error_scope(wgpu::ErrorFilter::Internal);
    let memory = shared
        .device
        .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = shared
        .device
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let work: Vec<_> = jobs
        .into_iter()
        .map(|job| {
            (
                (job.key, job.class, job.independent),
                execute(job.work.begin),
                job.work.complete,
            )
        })
        .collect();
    let (validation, memory, internal) = (validation.pop(), memory.pop(), internal.pop());
    let mut results = Vec::new();
    for (key, future, complete) in work {
        results.push((key, future.await, complete));
    }
    let (a, b, c) = (validation.await, memory.await, internal.await);
    let error = a.or(b).or(c).map(|e| e.to_string());
    let mut result = Ok(());
    for ((key, class, independent), compiled, complete) in results {
        let compiled = compiled.and_then(|()| error.clone().map_or(Ok(()), Err));
        if let Some(complete) = complete {
            complete(&compiled);
        }
        result = result.and(compiled.clone());
        let wakes = shared
            .queue
            .borrow_mut()
            .finish(key, class, independent, compiled);
        wake_owners(wakes,|owner,error|shared.with_queue(|q|{q.fail(owner,error);}));
    }
    shared.notify();
    result
}

pub(super) fn pipeline<T: Resource>(recipe: Deferred<T>) -> Work {
    let done = Rc::new(recipe.validation());
    let validation = done.clone();
    Work {
        begin: Box::new(move || {
            validation.begin();
            recipe.compile_async()
        }),
        complete: Some(Box::new(move |result| {
            done.finish(result);
            drop(done);
        })),
    }
}
pub(super) fn task(work: impl Task) -> Work {
    Work {
        begin: Box::new(move || Box::pin(std::future::ready(work()))),
        complete: None,
    }
}
pub(super) fn asynchronous(work: impl FnOnce() -> Completion + 'static) -> Work {
    Work {
        begin: Box::new(work),
        complete: None,
    }
}
