use super::*;
use std::{
    collections::VecDeque,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
type Work = Box<dyn FnOnce() -> Result<(), String> + Send>;
struct Job {
    priority: u8,
    work: Work,
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    started: bool,
    stopped: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    pending: AtomicUsize,
    error: Mutex<Option<String>>,
}
pub(crate) struct Compiler(Arc<Shared>);
impl Compiler {
    pub fn new() -> Result<Self, GpuRasterError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            pending: AtomicUsize::new(0),
            error: Mutex::new(None),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("capy-shaders".into())
            .spawn(move || {
                loop {
                    let job = {
                        let mut queue = worker.queue.lock().unwrap();
                        while !queue.stopped && (!queue.started || queue.jobs.is_empty()) {
                            queue = worker.wake.wait(queue).unwrap();
                        }
                        if queue.stopped {
                            return;
                        }
                        let index = queue
                            .jobs
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, job)| job.priority)
                            .unwrap()
                            .0;
                        queue.jobs.remove(index).unwrap()
                    };
                    let _span = Span::new(job.priority);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.work))
                        .unwrap_or_else(|_| Err("Shader compilation failed".into()));
                    if let Err(error) = result {
                        *worker.error.lock().unwrap() = Some(error);
                    }
                    worker.pending.fetch_sub(1, Ordering::Release);
                }
            })
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        Ok(Self(shared))
    }
    pub fn enqueue(
        &self,
        priority: u8,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        self.0.pending.fetch_add(1, Ordering::Relaxed);
        self.0.queue.lock().unwrap().jobs.push_back(Job {
            priority,
            work: Box::new(work),
        });
        self.0.wake.notify_one();
    }
    pub fn pipeline<T: Send + Sync + 'static>(&self, pipeline: &Deferred<T>, priority: u8) {
        if pipeline.promote(priority) {
            let pipeline = pipeline.clone();
            self.enqueue(priority, move || {
                pipeline.compile();
                Ok(())
            });
        }
    }
    pub fn start(&self) {
        self.0.queue.lock().unwrap().started = true;
        self.0.wake.notify_one();
    }
    pub fn pending(&self) -> usize {
        self.0.pending.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<(), GpuRasterError> {
        self.0
            .error
            .lock()
            .unwrap()
            .as_ref()
            .map_or(Ok(()), |e| Err(GpuRasterError::Effect(e.clone())))
    }
}
impl Drop for Compiler {
    fn drop(&mut self) {
        let mut queue = self.0.queue.lock().unwrap();
        queue.stopped = true;
        queue.jobs.clear();
        self.0.wake.notify_one();
        // An in-flight driver compilation finishes using its own GPU handles.
        // Surface teardown never joins the compiler or waits for it.
    }
}
struct Span;
impl Span {
    fn new(priority: u8) -> Self {
        #[cfg(target_os = "android")]
        unsafe {
            ATrace_beginSection(
                match priority {
                    DOCUMENT => c"capy.compile.document",
                    BRUSH => c"capy.compile.brush",
                    _ => c"capy.compile.other",
                }
                .as_ptr(),
            );
        }
        #[cfg(not(target_os = "android"))]
        let _ = priority;
        Self
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            ATrace_endSection();
        }
    }
}
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ATrace_beginSection(name: *const std::ffi::c_char);
    fn ATrace_endSection();
}
