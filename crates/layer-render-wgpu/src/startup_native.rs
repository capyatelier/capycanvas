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
    tracked: bool,
    independent: bool,
    work: Work,
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    admission: admission::Admission,
    running: usize,
    required_running: usize,
    ordered_running: bool,
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

pub(crate) struct Compiler(Arc<Shared>, Vec<std::thread::JoinHandle<()>>);
/// Thread-safe input admission only; this handle never accesses a live canvas.
#[derive(Clone)]
pub struct Activity(Arc<Shared>);
impl Activity {
    pub fn input(&self) { self.0.queue.lock().unwrap().admission.input(); }
    pub fn idle(&self, idle: bool, speculative_idle: bool) {
        let mut queue = self.0.queue.lock().unwrap();
        if queue.admission.idle != idle || queue.admission.speculative_idle != speculative_idle {
            queue.admission.idle = idle;
            queue.admission.speculative_idle = speculative_idle;
            self.0.wake.notify_all();
        }
    }
}
impl Compiler {
    pub fn activity(&self) -> Activity { Activity(self.0.clone()) }
    pub fn input(&self) { self.activity().input(); }
    pub fn idle(&self, idle: bool, speculative_idle: bool) { self.activity().idle(idle, speculative_idle); }
    pub fn is_idle(&self)->bool {self.0.queue.lock().unwrap().admission.idle}
    pub fn delay(&self) -> std::time::Duration {
        let queue = self.0.queue.lock().unwrap();
        queue.admission.delay(queue.jobs.iter().map(|job| job.priority))
    }
    pub fn new() -> Result<Self, GpuRasterError> {
        let workers = std::thread::available_parallelism().map_or(1, |n| (n.get() / 8).clamp(1, 4));
        Self::with_workers(workers)
    }
    fn with_workers(workers: usize) -> Result<Self, GpuRasterError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            pending: AtomicUsize::new(0),
            error: Mutex::new(None),
        });
        let mut compiler = Self(shared, Vec::with_capacity(workers));
        for index in 0..workers {
            let worker = compiler.0.clone();
            let thread = std::thread::Builder::new()
                .name(format!("capy-shaders-{index}"))
                .spawn(move || compile(&worker))
                .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
            compiler.1.push(thread);
        }
        Ok(compiler)
    }
    pub fn enqueue(
        &self,
        priority: u8,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        self.submit(Job { priority, tracked: true, independent: false, work: Box::new(work) });
    }
    pub fn enqueue_untracked(&self, priority: u8, independent: bool, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        self.submit(Job { priority, tracked: false, independent: independent && priority < OTHER, work: Box::new(work) });
    }
    fn submit(&self, job: Job) {
        self.0.pending.fetch_add(usize::from(job.tracked), Ordering::Relaxed);
        self.0.queue.lock().unwrap().jobs.push_back(job);
        self.0.wake.notify_one();
    }
    /// Optional preparation that startup completion does not wait for; it is
    /// skipped once `current` reports a newer preparation.
    pub fn background<T: Send + Sync + 'static>(&self, pipeline: &Deferred<T>, priority: u8, current: impl Fn() -> bool + Send + 'static) {
        let pipeline = pipeline.clone();
        self.enqueue_untracked(priority, pipeline.async_pipeline(), move || {
            if current() { pipeline.compile(); }
            Ok(())
        });
    }
    pub fn pipeline<T: Send + Sync + 'static>(&self, pipeline: &Deferred<T>, priority: u8) {
        if let Some(promotion) = pipeline.promote(priority) {
            let pipeline = pipeline.clone();
            self.submit(Job { priority, tracked: true, independent: pipeline.async_pipeline() && priority < OTHER, work: Box::new(move || {
                pipeline.compile();
                drop(promotion);
                Ok(())
            }) });
        }
    }
    pub fn require<'a, T: Send + Sync + 'static>(&self, pipelines: impl IntoIterator<Item = &'a Deferred<T>>, priority: u8) -> bool {
        pipelines.into_iter().fold(true, |ready, p| { self.pipeline(p, priority); ready & p.ready() })
    }
    pub fn start(&self) {
        self.0.queue.lock().unwrap().started = true;
        self.0.wake.notify_all();
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
fn compile(worker: &Shared) {
    loop {
        let job = {
            let mut queue = worker.queue.lock().unwrap();
            loop {
                if queue.stopped { return; }
                if queue.started && let Some(index) = queue.jobs.iter().enumerate()
                    .filter(|(_, job)| queue.admission.allows(job.priority) && (job.priority < OTHER || queue.required_running == 0))
                    .min_by_key(|(_, job)| job.priority).map(|(index, _)| index)
                    && !queue.ordered_running && (queue.jobs[index].independent || queue.running == 0) {
                    let job = queue.jobs.remove(index).unwrap();
                    queue.running += 1;
                    queue.required_running += usize::from(job.priority < OTHER);
                    queue.ordered_running = !job.independent;
                    break job;
                }
                let delay = queue.admission.delay(queue.jobs.iter().map(|job| job.priority));
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
            .unwrap_or_else(|payload| Err(crate::panic_message(&*payload)));
        if let Err(error) = result {
            worker.error.lock().unwrap().get_or_insert(error);
        }
        let mut queue = worker.queue.lock().unwrap();
        queue.admission.finished(job.priority);
        queue.running -= 1;
        queue.required_running -= usize::from(job.priority < OTHER);
        queue.ordered_running &= job.independent;
        drop(queue);
        worker.wake.notify_all();
        worker.pending.fetch_sub(usize::from(job.tracked), Ordering::Release);
    }
}
impl Drop for Compiler {
    fn drop(&mut self) {
        let mut queue = self.0.queue.lock().unwrap();
        queue.stopped = true;
        queue.jobs.clear();
        self.0.wake.notify_all();
        drop(queue);
        // An in-flight driver compilation finishes using its own GPU handles.
        // Surface teardown never joins the compiler or waits for it.
        let mut retired = RETIRED.lock().unwrap();
        retired.retain(|worker| !worker.is_finished());
        retired.append(&mut self.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    fn finish_jobs(compiler: &Compiler) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while compiler.pending() != 0 {
            assert!(std::time::Instant::now() < deadline, "shader jobs did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn canceled_compiler_releases_shared_recipe_admission() {
        let (sent, received) = mpsc::channel();
        let recipe = Deferred::new(move || { sent.send(()).unwrap(); 17 });
        let canceled = Compiler::with_workers(1).unwrap();
        canceled.pipeline(&recipe, DOCUMENT);
        drop(canceled);
        let next = Compiler::with_workers(1).unwrap();
        next.pipeline(&recipe, DOCUMENT);
        next.start();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*recipe.compile(), 17);
        assert!(received.try_recv().is_err());
        drop(next);
        finish_shader_compiler_shutdown();
    }

    #[test]
    fn compiler_preserves_static_and_owned_panic_diagnostics() {
        for owned in [false, true] {
            let compiler = Compiler::new().unwrap();
            compiler.enqueue(DOCUMENT, move || {
                if owned { std::panic::resume_unwind(Box::new(String::from("dry required owned diagnostic"))); }
                std::panic::resume_unwind(Box::new("dry required static diagnostic"));
            });
            compiler.start();
            finish_jobs(&compiler);
            let error = compiler.check().unwrap_err().to_string();
            assert!(error.contains(if owned { "dry required owned diagnostic" } else { "dry required static diagnostic" }),
                "compiler lost its panic cause: {error}");
            drop(compiler);
            finish_shader_compiler_shutdown();
        }
    }

    #[test]
    fn compiler_keeps_the_first_failure_when_later_jobs_also_fail() {
        let compiler = Compiler::new().unwrap();
        for message in ["first required shader cause", "later required shader cause"] {
            compiler.enqueue(DOCUMENT, move || Err(message.into()));
        }
        compiler.start();
        finish_jobs(&compiler);
        let error = compiler.check().unwrap_err().to_string();
        assert!(error.contains("first required shader cause") && !error.contains("later required shader cause"),
            "compiler replaced the original cause: {error}");
        drop(compiler);
        finish_shader_compiler_shutdown();
    }

    #[test]
    fn region_preparation_parks_during_contact_and_promotes_without_idle() {
        let renderer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let flood=crate::flood::Flood::new(&renderer.device);
        let raw=crate::region_sources::RawRegions::new(&renderer.device);
        let compiler=Compiler::with_workers(1).unwrap();
        let (entered,started)=mpsc::channel();let (release,resume)=mpsc::channel();
        compiler.enqueue(BRUSH,move||{entered.send(()).unwrap();resume.recv_timeout(Duration::from_secs(60)).unwrap();Ok(())});
        compiler.start();started.recv_timeout(Duration::from_secs(10)).unwrap();
        let refinement=layer_render::RegionRefinement::default();
        assert!(!flood.prepare(&compiler,refinement,false,true,OTHER));
        assert!(!raw.prepare(&compiler,OTHER));
        compiler.idle(false,false);
        assert!(!compiler.is_idle());
        let (sent,done)=mpsc::channel();compiler.enqueue(BRUSH,move||{sent.send(()).unwrap();Ok(())});
        release.send(()).unwrap();done.recv_timeout(Duration::from_secs(60)).unwrap();
        assert!(!flood.prepare(&compiler,refinement,false,true,OTHER));
        assert!(!raw.prepare(&compiler,OTHER));
        let _=flood.prepare(&compiler,refinement,false,true,BRUSH) & raw.prepare(&compiler,BRUSH);
        let (sent,done)=mpsc::channel();compiler.enqueue(BRUSH,move||{sent.send(()).unwrap();Ok(())});
        done.recv_timeout(Duration::from_secs(60)).unwrap();
        assert!(flood.prepare(&compiler,refinement,false,true,BRUSH) & raw.prepare(&compiler,BRUSH));
        assert!(!compiler.is_idle());
        compiler.idle(true,true);finish_jobs(&compiler);
        drop(compiler);finish_shader_compiler_shutdown();
    }

    #[test]
    fn input_admission_resumes_without_polling_and_promoted_dependencies_run_once() {
        let compiler = Compiler::new().unwrap();
        compiler.idle(false, false);
        compiler.input();
        let (ran, events) = mpsc::channel();
        let promoted = {
            let ran = ran.clone();
            Deferred::new(move || { ran.send("required").unwrap(); 1 })
        };
        compiler.pipeline(&promoted, OTHER);
        let warmed = ran.clone();
        compiler.enqueue(BACKGROUND, move || { warmed.send("warm").unwrap(); Ok(()) });
        compiler.enqueue(OTHER, move || { ran.send("optional").unwrap(); Ok(()) });
        compiler.start();
        assert!(events.recv_timeout(Duration::from_millis(250)).is_err(), "a held gesture outlasts the quiet period");
        compiler.pipeline(&promoted, BRUSH);
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "required");
        compiler.input();
        compiler.idle(true, false);
        assert!(events.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "optional");
        assert!(events.recv_timeout(Duration::from_secs(1)).is_err());
        compiler.idle(true, true);
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "warm");
        drop(compiler);
        finish_shader_compiler_shutdown();
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn ordered_jobs_and_generic_recipes_never_overlap() {
        let compiler = Compiler::with_workers(2).unwrap();
        let running = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let pipelines: Vec<_> = (0..8).map(|_| {
            let (running, peak) = (running.clone(), peak.clone());
            Deferred::new(move || {
                peak.fetch_max(running.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                running.fetch_sub(1, Ordering::SeqCst);
            })
        }).collect();
        for (index, pipeline) in pipelines.iter().enumerate() {
            compiler.pipeline(pipeline, BRUSH);
            let (running, overlapped) = (running.clone(), overlapped.clone());
            compiler.enqueue(if index % 2 == 0 { BRUSH } else { VALIDATION }, move || {
                if running.fetch_add(1, Ordering::SeqCst) != 0 { overlapped.fetch_add(1, Ordering::SeqCst); }
                std::thread::sleep(Duration::from_millis(5));
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            });
        }
        compiler.start();
        finish_jobs(&compiler);
        assert_eq!(overlapped.load(Ordering::SeqCst), 0, "a job that is not a pipeline compile never overlaps another job");
        assert_eq!(peak.load(Ordering::SeqCst), 1, "generic recipes remain ordered");
        drop(compiler);
        finish_shader_compiler_shutdown();
    }

    #[test]
    fn required_completion_leaves_a_quiet_interval_before_speculation() {
        let compiler = Compiler::new().unwrap();
        let (ran, events) = mpsc::channel();
        for (priority, label) in [(BRUSH, "required"), (BACKGROUND, "first"), (BACKGROUND, "second")] {
            let ran = ran.clone();
            compiler.enqueue(priority, move || { ran.send(label).unwrap(); Ok(()) });
        }
        compiler.start();
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "required");
        assert!(events.recv_timeout(Duration::from_millis(200)).is_err());
        assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), "first");
        assert_eq!(events.recv_timeout(Duration::from_millis(200)).unwrap(), "second");
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

    #[test]
    fn required_pipeline_jobs_overlap_before_optional_work_and_cache_finalization() {
        let compiler = Compiler::with_workers(2).unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (finished, finished_rx) = mpsc::channel();
        let (optional, optional_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let mut pipelines = Vec::new();
        for index in 0..2 {
            let entered = entered.clone();
            let finished = finished.clone();
            let (release, wait_release) = mpsc::channel();
            releases.push(release);
            let pipeline = Deferred::pipeline(move |_| {
                entered.send(index).unwrap();
                wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
                finished.send(index).unwrap();
                Compilation::Ready(index)
            });
            compiler.pipeline(&pipeline, BRUSH);
            pipelines.push(pipeline);
        }
        let optional_event = optional.clone();
        let optional_pipeline = Deferred::pipeline(move |_| {
            optional_event.send("optional").unwrap();
            Compilation::Ready(())
        });
        compiler.pipeline(&optional_pipeline, OTHER);
        compiler.enqueue(BACKGROUND, move || {optional.send("cache").unwrap(); Ok(())});
        compiler.start();
        let first = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = entered_rx.recv_timeout(Duration::from_secs(2)).ok();
        let _ = releases[first].send(());
        assert_eq!(finished_rx.recv_timeout(Duration::from_secs(2)).unwrap(), first);
        let optional_before_last_completion = optional_rx.recv_timeout(Duration::from_millis(1200)).ok();
        let _ = releases[1-first].send(());
        finish_jobs(&compiler);
        drop(compiler);
        finish_shader_compiler_shutdown();
        assert_eq!(second, Some(1-first), "both required recipes enter before either is released");
        assert_eq!(optional_before_last_completion, None, "optional and cache work wait beyond both quiet intervals for the last required recipe");
        assert_eq!(finished_rx.try_recv().unwrap(), 1-first);
        assert!(pipelines.iter().all(Deferred::ready));
        assert!(optional_pipeline.ready());
        assert_eq!(optional_rx.try_recv().unwrap(), "optional");
        assert_eq!(optional_rx.try_recv().unwrap(), "cache");
    }

    #[test]
    fn generic_data_and_optional_pipeline_jobs_stay_serial() {
        for kind in 0..4 {
            let compiler = Compiler::with_workers(2).unwrap();
            let (entered, events) = mpsc::channel();
            let mut releases = Vec::new();
            let mut pipelines = Vec::new();
            for index in 0..2 {
                let entered = entered.clone();
                let (release, wait_release) = mpsc::channel();
                releases.push(release);
                let work = move || {
                    entered.send(index).unwrap();
                    wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
                    index
                };
                if kind == 0 {
                    compiler.enqueue(DOCUMENT, move || {work(); Ok(())});
                } else {
                    let pipeline = if kind == 1 {Deferred::new(work)}
                        else {Deferred::pipeline(move |_| Compilation::Ready(work()))};
                    compiler.pipeline(&pipeline, match kind {1 => BRUSH, 2 => OTHER, _ => BACKGROUND});
                    pipelines.push(pipeline);
                }
            }
            compiler.start();
            let first = events.recv_timeout(Duration::from_secs(2)).unwrap();
            let second_before_release = events.recv_timeout(Duration::from_millis(250)).ok();
            for release in releases {let _ = release.send(());}
            finish_jobs(&compiler);
            drop(compiler);
            finish_shader_compiler_shutdown();
            assert_eq!(first, 0, "generic/data/optional FIFO: kind={kind}");
            assert_eq!(second_before_release, None, "only required pipeline recipes overlap: kind={kind}");
            assert_eq!(events.try_recv().unwrap(), 1);
            assert!(pipelines.iter().all(Deferred::ready));
        }
    }

    #[test]
    fn surface_teardown_retires_both_required_workers_and_cancels_queued_work() {
        let compiler = Compiler::with_workers(2).unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (finished, finished_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let mut pipelines = Vec::new();
        for index in 0..2 {
            let entered = entered.clone();
            let finished = finished.clone();
            let (release, wait_release) = mpsc::channel();
            releases.push(release);
            let pipeline = Deferred::pipeline(move |_| {
                entered.send(index).unwrap();
                wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
                finished.send(index).unwrap();
                Compilation::Ready(index)
            });
            compiler.pipeline(&pipeline, BRUSH);
            pipelines.push(pipeline);
        }
        let canceled = Arc::new(AtomicUsize::new(0));
        let flag = canceled.clone();
        let queued = Deferred::pipeline(move |_| {
            flag.fetch_add(1, Ordering::Relaxed);
            Compilation::Ready(())
        });
        compiler.pipeline(&queued, OTHER);
        compiler.start();
        let first = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = entered_rx.recv_timeout(Duration::from_secs(2)).ok();
        let (dropped, dropped_rx) = mpsc::channel();
        let teardown = std::thread::spawn(move || {drop(compiler); dropped.send(()).unwrap();});
        let returned_before_release = dropped_rx.recv_timeout(Duration::from_secs(2)).is_ok();
        let finished_before_release = finished_rx.try_recv().is_ok();
        for release in releases {let _ = release.send(());}
        teardown.join().unwrap();
        finish_shader_compiler_shutdown();
        assert_eq!(second, Some(1-first), "both workers were in flight during teardown");
        assert!(returned_before_release && !finished_before_release, "surface teardown never joins blocked recipes");
        let mut completed = vec![finished_rx.try_recv().unwrap(), finished_rx.try_recv().unwrap()];
        completed.sort_unstable();
        assert_eq!(completed, [0,1], "final shutdown joins both in-flight workers");
        assert!(pipelines.iter().all(Deferred::ready));
        assert_eq!(canceled.load(Ordering::Relaxed), 0);
        assert!(!queued.ready());
    }
}
