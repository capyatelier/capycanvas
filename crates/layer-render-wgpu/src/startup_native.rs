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
    admission: admission::Admission,
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
/// Thread-safe input admission only; this handle never accesses a live canvas.
#[derive(Clone)]
pub struct Activity(Arc<Shared>);
impl Activity {
    pub fn input(&self) { self.0.queue.lock().unwrap().admission.input(); }
    pub fn idle(&self, idle: bool) {
        let mut queue = self.0.queue.lock().unwrap();
        if queue.admission.idle != idle {
            queue.admission.idle = idle;
            self.0.wake.notify_one();
        }
    }
}
impl Compiler {
    pub fn activity(&self) -> Activity { Activity(self.0.clone()) }
    pub fn input(&self) { self.activity().input(); }
    pub fn idle(&self, idle: bool) { self.activity().idle(idle); }
    pub fn delay(&self) -> std::time::Duration { self.0.queue.lock().unwrap().admission.delay() }
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
                        loop {
                            if queue.stopped { return; }
                            if queue.started && let Some(index) = queue.jobs.iter().enumerate()
                                .filter(|(_, job)| queue.admission.allows(job.priority))
                                .min_by_key(|(_, job)| job.priority).map(|(index, _)| index) {
                                break queue.jobs.remove(index).unwrap();
                            }
                            let delay = queue.admission.delay();
                            queue = if queue.started && queue.admission.idle && !delay.is_zero() {
                                worker.wake.wait_timeout(queue, delay).unwrap().0
                            } else { worker.wake.wait(queue).unwrap() };
                        }
                    };
                    let _span = crate::performance_trace::Span::new(match job.priority {
                        DOCUMENT => c"capy.compile.document",
                        BRUSH => c"capy.compile.brush",
                        _ => c"capy.compile.other",
                    });
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
    pub fn require<'a, T: Send + Sync + 'static>(&self, pipelines: impl IntoIterator<Item = &'a Deferred<T>>, priority: u8) -> bool {
        pipelines.into_iter().fold(true, |ready, p| { self.pipeline(p, priority); ready & p.ready() })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn input_admission_resumes_without_polling_and_promoted_dependencies_run_once() {
        let compiler = Compiler::new().unwrap();
        compiler.idle(false);
        compiler.input();
        let (ran, events) = mpsc::channel();
        let promoted = {
            let ran = ran.clone();
            Deferred::new(move || { ran.send("required").unwrap(); 1 })
        };
        compiler.pipeline(&promoted, OTHER);
        compiler.enqueue(OTHER, move || { ran.send("optional").unwrap(); Ok(()) });
        compiler.start();
        assert!(events.recv_timeout(Duration::from_millis(250)).is_err(), "a held gesture outlasts the quiet period");
        compiler.pipeline(&promoted, BRUSH);
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "required");
        compiler.input();
        compiler.idle(true);
        assert!(events.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "optional");
        drop(compiler);
        finish_shader_compiler_shutdown();
        assert!(events.try_recv().is_err());
    }

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
