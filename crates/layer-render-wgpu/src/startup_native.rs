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
static RETIRED: Mutex<Vec<std::thread::JoinHandle<()>>> = Mutex::new(Vec::new());

/// Wait for canceled native shader workers after closing all renderers at process
/// shutdown. Ordinary surface teardown stays asynchronous; final shutdown must
/// not unload the graphics driver while an in-flight compilation is using it.
pub fn finish_shader_compiler_shutdown() {
    let workers = std::mem::take(&mut *RETIRED.lock().unwrap());
    for worker in workers {
        let _ = worker.join();
    }
}

pub(crate) struct Compiler(Arc<Shared>, Option<std::thread::JoinHandle<()>>);
impl Compiler {
    pub fn new() -> Result<Self, GpuRasterError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            pending: AtomicUsize::new(0),
            error: Mutex::new(None),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
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
        Ok(Self(shared, Some(thread)))
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
        drop(queue);
        // An in-flight driver compilation finishes using its own GPU handles.
        // Surface teardown never joins the compiler or waits for it.
        let mut retired = RETIRED.lock().unwrap();
        retired.retain(|worker| !worker.is_finished());
        retired.push(self.1.take().unwrap());
    }
}
struct Span {
    #[cfg(target_os = "windows")]
    start: Option<std::time::Instant>,
    #[cfg(target_os = "windows")]
    priority: u8,
}
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
        Self {
            #[cfg(target_os = "windows")]
            start: std::env::var_os("CAPY_TRACE_SHADER_JOBS").map(|_| {
                eprintln!(
                    "shader_job begin priority={priority} utc_ms={}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
                std::time::Instant::now()
            }),
            #[cfg(target_os = "windows")]
            priority,
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(start) = self.start {
            eprintln!(
                "shader_job end priority={} elapsed_ms={:.3} utc_ms={}",
                self.priority,
                start.elapsed().as_secs_f64() * 1000.,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn surface_teardown_cancels_queue_and_final_shutdown_joins_inflight_work() {
        let compiler = Compiler::new().unwrap();
        let (entered, wait_entered) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        compiler.enqueue(DOCUMENT, move || {
            entered.send(()).unwrap();
            wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
            finished.send(()).unwrap();
            Ok(())
        });
        let canceled = Arc::new(AtomicUsize::new(0));
        let flag = canceled.clone();
        compiler.enqueue(OTHER, move || {
            flag.fetch_add(1, Ordering::Relaxed);
            Ok(())
        });
        compiler.start();
        wait_entered.recv_timeout(Duration::from_secs(10)).unwrap();
        // Drop must return while the worker remains blocked.
        drop(compiler);
        assert!(wait_finished.try_recv().is_err());
        release.send(()).unwrap();
        finish_shader_compiler_shutdown();
        wait_finished.try_recv().unwrap();
        assert_eq!(canceled.load(Ordering::Relaxed), 0);
    }
}
