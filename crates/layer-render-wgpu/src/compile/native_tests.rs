use super::*;
use crate::startup::{ANTICIPATED, BACKGROUND, BRUSH, DOCUMENT, OTHER, VALIDATION};
use crate::{Compilation, WgpuRasterizer};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
struct Compiler {
    scope: Scope,
    batch: std::cell::RefCell<Option<Batch>>,
}
impl std::ops::Deref for Compiler {
    type Target = Scope;
    fn deref(&self) -> &Scope {
        &self.scope
    }
}
impl Compiler {
    fn new() -> Result<Self, GpuRasterError> {
        Self::with_workers(2)
    }
    fn with_workers(workers: usize) -> Result<Self, GpuRasterError> {
        let scope = native::Coordinator::with_workers(workers, None)?
            .scope()
            .unwrap();
        let batch = std::cell::RefCell::new(Some(scope.batch()));
        Ok(Self { scope, batch })
    }
    fn start(&self) {
        self.batch.borrow_mut().take();
    }
}
fn workers(parallelism: usize) -> usize {
    native::workers(parallelism)
}
#[test]
fn repeated_background_requests_promote_one_queue_entry() {
    let compiler = Compiler::with_workers(1).unwrap();
    let recipe = Deferred::pipeline(|_| Compilation::Ready(()));
    for _ in 0..100 {
        compiler.warm(&recipe, BACKGROUND, 0);
    }
    compiler.require([&recipe], BRUSH);
    let (count, priority) = {
        let coordinator = &compiler.scope.0.coordinator;
        let queue = coordinator.shared.queue.lock().unwrap();
        (
            queue.entries.len(),
            queue.entries.values().next().unwrap().class,
        )
    };
    assert_eq!(count, 1);
    assert_eq!(priority, BRUSH);
}

#[test]
fn worker_counts_follow_parallelism() {
    for (parallelism, expected) in [(2, 1), (4, 2), (6, 3), (8, 3), (12, 5), (16, 6), (32, 6)] {
        assert_eq!(workers(parallelism), expected, "parallelism {parallelism}");
    }
}

#[test]
fn device_teardown_joins_inflight_recipes_before_releasing_the_device() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let scope = renderer.device.compilation().unwrap();
    let coordinator = scope.0.coordinator.clone();
    let device = renderer.device.for_compilation();
    let pipeline = renderer.pipelines.stroke_edge.clone();
    let held = Arc::new(());
    let weak = Arc::downgrade(&held);
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let recipe = Deferred::pipeline(move |_| {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(60)).unwrap();
        pipeline.compile();
        drop((device, held));
        Compilation::Ready(())
    });
    scope.require([&recipe], DOCUMENT);
    started.recv_timeout(Duration::from_secs(60)).unwrap();
    drop(recipe);
    let (dropped, done) = mpsc::channel();
    let teardown = std::thread::spawn(move || {
        drop(renderer);
        drop(scope);
        drop(coordinator);
        dropped.send(weak.upgrade().is_none()).unwrap();
    });
    let early = done.recv_timeout(Duration::from_millis(250)).ok();
    release.send(()).unwrap();
    teardown.join().unwrap();
    assert_eq!(early, None, "device teardown detached a running recipe");
    assert!(done.recv_timeout(Duration::from_secs(60)).unwrap());
}

#[test]
fn final_shutdown_joins_pools_created_while_another_pool_is_finishing() {
    let blocked = || {
        let compiler = Compiler::with_workers(1).unwrap();
        let owner = compiler.scope.0.coordinator.clone();
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        compiler.enqueue(DOCUMENT, move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(60)).unwrap();
            Ok(())
        });
        compiler.start();
        started.recv_timeout(Duration::from_secs(60)).unwrap();
        drop(compiler);
        (owner, release)
    };
    let (first, release_first) = blocked();
    let (finished, done) = mpsc::channel();
    let shutdown = std::thread::spawn(move || {
        finish_shader_compiler_shutdown();
        finished.send(()).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !first.with_queue(|queue| queue.stopped) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let (second, release_second) = blocked();
    release_first.send(()).unwrap();
    let early = done.recv_timeout(Duration::from_millis(250)).is_ok();
    release_second.send(()).unwrap();
    shutdown.join().unwrap();
    assert!(!early, "shutdown missed a newly eligible device pool");
    assert!(second.with_queue(|queue| queue.stopped));
}

fn submit_unowned(coordinator: &native::Coordinator, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
    coordinator.with_queue(|queue| {
        let id = queue.next();
        queue.submit(Key::Job(id), Demand { owner: 0, generation: None, class: DOCUMENT }, false, || Box::new(work) as platform::Work);
    });
    coordinator.notify();
}

#[test]
fn exit_stop_returns_while_a_compile_job_is_blocked_mid_run() {
    let coordinator = native::Coordinator::with_workers(1, None).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel::<()>();
    submit_unowned(&coordinator, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(60)).unwrap();
        Ok(())
    });
    started.recv_timeout(Duration::from_secs(60)).unwrap();
    let (stopped, done) = mpsc::channel();
    let stopping = std::thread::spawn(move || {
        stop_shader_compilers();
        stopped.send(()).unwrap();
    });
    let returned = done.recv_timeout(Duration::from_secs(2)).is_ok();
    release.send(()).unwrap();
    stopping.join().unwrap();
    assert!(returned, "exit waited for a running compile");
    assert!(coordinator.with_queue(|queue| queue.stopped));
}

#[test]
fn exit_stop_starts_no_compile_job_after_it_returns() {
    let coordinator = native::Coordinator::with_workers(1, None).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel::<()>();
    submit_unowned(&coordinator, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(60)).unwrap();
        Ok(())
    });
    started.recv_timeout(Duration::from_secs(60)).unwrap();
    stop_shader_compilers();
    let ran = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        let ran = ran.clone();
        submit_unowned(&coordinator, move || {
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    }
    release.send(()).unwrap();
    drop(coordinator);
    assert_eq!(ran.load(Ordering::SeqCst), 0, "a compile job started after exit stopped compilation");
}

fn finish_jobs(compiler: &Compiler) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while compiler.with_queue(|q| {
        q.entries.values().any(|e| {
            (e.work.is_some() || e.running)
                && e.demands
                    .iter()
                    .any(|d| d.owner == compiler.scope.0.id && d.generation.is_none())
        })
    }) {
        assert!(
            std::time::Instant::now() < deadline,
            "shader jobs did not finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn canceled_compiler_releases_shared_recipe_admission() {
    let (sent, received) = mpsc::channel();
    let recipe = Deferred::new(move || {
        sent.send(()).unwrap();
        17
    });
    let canceled = Compiler::with_workers(1).unwrap();
    canceled.require([&recipe], DOCUMENT);
    drop(canceled);
    let next = Compiler::with_workers(1).unwrap();
    next.require([&recipe], DOCUMENT);
    next.start();
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(*next.blocking(|| recipe.compile()), 17);
    assert!(received.try_recv().is_err());
    drop(next);
    finish_shader_compiler_shutdown();
}

#[test]
fn compiler_preserves_static_and_owned_panic_diagnostics() {
    for owned in [false, true] {
        let compiler = Compiler::new().unwrap();
        compiler.enqueue(DOCUMENT, move || {
            if owned {
                std::panic::resume_unwind(Box::new(String::from("dry required owned diagnostic")));
            }
            std::panic::resume_unwind(Box::new("dry required static diagnostic"));
        });
        compiler.start();
        finish_jobs(&compiler);
        let error = compiler.check().unwrap_err().to_string();
        assert!(
            error.contains(if owned {
                "dry required owned diagnostic"
            } else {
                "dry required static diagnostic"
            }),
            "compiler lost its panic cause: {error}"
        );
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
    assert!(
        error.contains("first required shader cause")
            && !error.contains("later required shader cause"),
        "compiler replaced the original cause: {error}"
    );
    drop(compiler);
    finish_shader_compiler_shutdown();
}

#[test]
fn region_preparation_parks_during_contact_and_promotes_without_idle() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let flood = crate::flood::Flood::new(&renderer.device);
    let raw = crate::region_sources::RawRegions::new(&renderer.device);
    let compiler = Compiler::with_workers(1).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    compiler.enqueue(BRUSH, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(60)).unwrap();
        Ok(())
    });
    compiler.start();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    let refinement = layer_render::RegionRefinement::default();
    assert!(!flood.prepare(&compiler, refinement, false, true, OTHER));
    assert!(!raw.prepare(&compiler, OTHER));
    compiler.idle(false, false);
    assert!(!compiler.is_idle());
    let (sent, done) = mpsc::channel();
    compiler.enqueue(BRUSH, move || {
        sent.send(()).unwrap();
        Ok(())
    });
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(60)).unwrap();
    assert!(!flood.prepare(&compiler, refinement, false, true, OTHER));
    assert!(!raw.prepare(&compiler, OTHER));
    let _ =
        flood.prepare(&compiler, refinement, false, true, BRUSH) & raw.prepare(&compiler, BRUSH);
    let (sent, done) = mpsc::channel();
    compiler.enqueue(BRUSH, move || {
        sent.send(()).unwrap();
        Ok(())
    });
    done.recv_timeout(Duration::from_secs(60)).unwrap();
    assert!(
        flood.prepare(&compiler, refinement, false, true, BRUSH) & raw.prepare(&compiler, BRUSH)
    );
    assert!(!compiler.is_idle());
    compiler.idle(true, true);
    finish_jobs(&compiler);
    drop(compiler);
    finish_shader_compiler_shutdown();
}

#[test]
fn input_admission_resumes_without_polling_and_promoted_dependencies_run_once() {
    let compiler = Compiler::new().unwrap();
    compiler.idle(false, false);
    compiler.input();
    let (ran, events) = mpsc::channel();
    let promoted = {
        let ran = ran.clone();
        Deferred::new(move || {
            ran.send("required").unwrap();
            1
        })
    };
    compiler.require([&promoted], OTHER);
    let warmed = ran.clone();
    compiler.enqueue(BACKGROUND, move || {
        warmed.send("warm").unwrap();
        Ok(())
    });
    compiler.enqueue(OTHER, move || {
        ran.send("optional").unwrap();
        Ok(())
    });
    compiler.start();
    assert!(
        events.recv_timeout(Duration::from_millis(250)).is_err(),
        "a held gesture outlasts the quiet period"
    );
    compiler.require([&promoted], BRUSH);
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "required"
    );
    compiler.input();
    compiler.idle(true, false);
    assert!(events.recv_timeout(Duration::from_millis(100)).is_err());
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "optional"
    );
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
    let pipelines: Vec<_> = (0..8)
        .map(|_| {
            let (running, peak) = (running.clone(), peak.clone());
            Deferred::new(move || {
                peak.fetch_max(running.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                running.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for (index, pipeline) in pipelines.iter().enumerate() {
        compiler.require([pipeline], BRUSH);
        let (running, overlapped) = (running.clone(), overlapped.clone());
        compiler.enqueue(if index % 2 == 0 { BRUSH } else { VALIDATION }, move || {
            if running.fetch_add(1, Ordering::SeqCst) != 0 {
                overlapped.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(5));
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        });
    }
    compiler.start();
    finish_jobs(&compiler);
    assert_eq!(
        overlapped.load(Ordering::SeqCst),
        0,
        "a job that is not a pipeline compile never overlaps another job"
    );
    assert_eq!(
        peak.load(Ordering::SeqCst),
        1,
        "generic recipes remain ordered"
    );
    drop(compiler);
    finish_shader_compiler_shutdown();
}

#[test]
fn anticipated_pipelines_run_before_remaining_speculative_warming() {
    let compiler = Compiler::with_workers(1).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let first = Deferred::pipeline(move |_| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
        Compilation::Ready(())
    });
    compiler.warm(&first, BACKGROUND, 0);
    let (ran, events) = mpsc::channel();
    for label in ["background first", "background second"] {
        let ran = ran.clone();
        let pipeline = Deferred::pipeline(move |_| {
            ran.send(label).unwrap();
            Compilation::Ready(())
        });
        compiler.warm(&pipeline, BACKGROUND, 0);
    }
    compiler.start();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let anticipated = Deferred::pipeline(move |_| {
        ran.send("anticipated").unwrap();
        Compilation::Ready(())
    });
    compiler.warm(&anticipated, ANTICIPATED, 0);
    assert!(
        events.try_recv().is_err(),
        "optional pipelines stay serialized while warming is in flight"
    );
    release.send(()).unwrap();
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "anticipated"
    );
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "background first"
    );
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "background second"
    );
    drop(compiler);
    finish_shader_compiler_shutdown();
}

#[test]
fn required_completion_leaves_a_quiet_interval_before_speculation() {
    let compiler = Compiler::new().unwrap();
    let (ran, events) = mpsc::channel();
    for (priority, label) in [
        (BRUSH, "required"),
        (BACKGROUND, "first"),
        (BACKGROUND, "second"),
        (ANTICIPATED, "anticipated"),
    ] {
        let ran = ran.clone();
        compiler.enqueue(priority, move || {
            ran.send(label).unwrap();
            Ok(())
        });
    }
    compiler.start();
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "required"
    );
    assert!(events.recv_timeout(Duration::from_millis(200)).is_err());
    assert_eq!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        "anticipated"
    );
    assert_eq!(
        events.recv_timeout(Duration::from_millis(200)).unwrap(),
        "first"
    );
    assert_eq!(
        events.recv_timeout(Duration::from_millis(200)).unwrap(),
        "second"
    );
}

#[test]
fn surface_teardown_cancels_queue_and_final_shutdown_joins_inflight_work() {
    let compiler = Compiler::new().unwrap();
    let device_owner = compiler.scope.0.coordinator.clone();
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
    drop(compiler);
    assert!(wait_finished.try_recv().is_err());
    release.send(()).unwrap();
    finish_shader_compiler_shutdown();
    drop(device_owner);
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
        compiler.require([&pipeline], BRUSH);
        pipelines.push(pipeline);
    }
    let optional_event = optional.clone();
    let optional_pipeline = Deferred::pipeline(move |_| {
        optional_event.send("optional").unwrap();
        Compilation::Ready(())
    });
    compiler.require([&optional_pipeline], OTHER);
    compiler.enqueue(BACKGROUND, move || {
        optional.send("cache").unwrap();
        Ok(())
    });
    compiler.start();
    let first = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = entered_rx.recv_timeout(Duration::from_secs(2)).ok();
    let _ = releases[first].send(());
    assert_eq!(
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        first
    );
    let optional_before_last_completion =
        optional_rx.recv_timeout(Duration::from_millis(1200)).ok();
    let _ = releases[1 - first].send(());
    finish_jobs(&compiler);
    drop(compiler);
    finish_shader_compiler_shutdown();
    assert_eq!(
        second,
        Some(1 - first),
        "both required recipes enter before either is released"
    );
    assert_eq!(
        optional_before_last_completion, None,
        "optional and cache work wait beyond both quiet intervals for the last required recipe"
    );
    assert_eq!(finished_rx.try_recv().unwrap(), 1 - first);
    assert!(pipelines.iter().all(Deferred::ready));
    assert!(optional_pipeline.ready());
    assert_eq!(optional_rx.try_recv().unwrap(), "optional");
    assert_eq!(optional_rx.try_recv().unwrap(), "cache");
}

#[test]
fn required_preparation_never_waits_for_an_inflight_optional_compile() {
    let compiler = Compiler::with_workers(1).unwrap();
    let (entered, entered_rx) = mpsc::channel();
    let (release, wait_release) = mpsc::channel::<()>();
    let optional = Deferred::pipeline(move |_| {
        entered.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
        Compilation::Ready(())
    });
    compiler.warm(&optional, BACKGROUND, 0);
    compiler.start();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let required = Deferred::pipeline(|_| Compilation::Ready(()));
    compiler.require([&required], BRUSH);
    let (document, document_rx) = mpsc::channel();
    compiler.enqueue(DOCUMENT, move || {
        document.send(()).unwrap();
        Ok(())
    });
    finish_jobs(&compiler);
    assert!(
        required.ready() && document_rx.try_recv().is_ok(),
        "required preparation finishes while the optional compile is in flight"
    );
    assert!(!optional.ready());
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !optional.ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "the optional compile finishes after release"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(compiler);
    finish_shader_compiler_shutdown();
}

#[test]
fn generic_data_and_optional_pipeline_jobs_stay_serial() {
    for kind in 0..5 {
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
                compiler.enqueue(DOCUMENT, move || {
                    work();
                    Ok(())
                });
            } else {
                let pipeline = if kind == 1 {
                    Deferred::new(work)
                } else {
                    Deferred::pipeline(move |_| Compilation::Ready(work()))
                };
                compiler.require(
                    [&pipeline],
                    match kind {
                        1 => BRUSH,
                        2 => OTHER,
                        3 => ANTICIPATED,
                        _ => BACKGROUND,
                    },
                );
                pipelines.push(pipeline);
            }
        }
        compiler.start();
        let first = events.recv_timeout(Duration::from_secs(2)).unwrap();
        let second_before_release = events.recv_timeout(Duration::from_millis(250)).ok();
        for release in releases {
            let _ = release.send(());
        }
        finish_jobs(&compiler);
        drop(compiler);
        finish_shader_compiler_shutdown();
        assert_eq!(first, 0, "generic/data/optional FIFO: kind={kind}");
        assert_eq!(
            second_before_release, None,
            "only required pipeline recipes overlap: kind={kind}"
        );
        assert_eq!(events.try_recv().unwrap(), 1);
        assert!(pipelines.iter().all(Deferred::ready));
    }
}

#[test]
fn surface_teardown_cancels_queued_work_without_joining_required_workers() {
    let compiler = Compiler::with_workers(2).unwrap();
    let device_owner = compiler.scope.0.coordinator.clone();
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
        compiler.require([&pipeline], BRUSH);
        pipelines.push(pipeline);
    }
    let canceled = Arc::new(AtomicUsize::new(0));
    let flag = canceled.clone();
    let queued = Deferred::pipeline(move |_| {
        flag.fetch_add(1, Ordering::Relaxed);
        Compilation::Ready(())
    });
    compiler.require([&queued], OTHER);
    compiler.start();
    let first = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = entered_rx.recv_timeout(Duration::from_secs(2)).ok();
    let (dropped, dropped_rx) = mpsc::channel();
    let teardown = std::thread::spawn(move || {
        drop(compiler);
        dropped.send(()).unwrap();
    });
    let returned_before_release = dropped_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    let finished_before_release = finished_rx.try_recv().is_ok();
    for release in releases {
        let _ = release.send(());
    }
    teardown.join().unwrap();
    finish_shader_compiler_shutdown();
    drop(device_owner);
    assert_eq!(
        second,
        Some(1 - first),
        "both workers were in flight during teardown"
    );
    assert!(
        returned_before_release && !finished_before_release,
        "surface teardown never joins blocked recipes"
    );
    let mut completed = vec![
        finished_rx.try_recv().unwrap(),
        finished_rx.try_recv().unwrap(),
    ];
    completed.sort_unstable();
    assert_eq!(
        completed,
        [0, 1],
        "final shutdown joins both in-flight workers"
    );
    assert!(pipelines.iter().all(Deferred::ready));
    assert_eq!(canceled.load(Ordering::Relaxed), 0);
    assert!(!queued.ready());
}

#[test]
fn final_shutdown_waits_for_canceled_snapshot_workers_to_release_their_gpu() {
    let gpu = Arc::new(());
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel::<()>();
    let job = crate::snapshot::SnapshotJob::spawn("capy-snapshot", {
        let gpu = gpu.clone();
        move |_| {
            let _gpu = gpu;
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(10)).unwrap();
            Err("canceled".into())
        }
    })
    .unwrap();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(job);
    let (returned, shutdown) = mpsc::channel();
    let finishing = std::thread::spawn(move || {
        finish_shader_compiler_shutdown();
        returned.send(()).unwrap();
    });
    let early = shutdown.recv_timeout(Duration::from_millis(250)).is_ok();
    release.send(()).unwrap();
    finishing.join().unwrap();
    assert!(
        !early,
        "final shutdown returned while a canceled snapshot worker was still inside the GPU"
    );
    assert_eq!(
        Arc::strong_count(&gpu),
        1,
        "the canceled snapshot worker released its GPU before final shutdown returned"
    );
}

#[test]
fn shader_workers_keep_the_device_service_root_until_their_join() {
    let poll = crate::device_poll::Binding::default();
    let recipe_poll = poll.recipe();
    let released_poll = recipe_poll.clone();
    let coordinator = native::Coordinator::with_binding(1, &poll).unwrap();
    let scope = coordinator.scope().unwrap();
    drop(poll);
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let recipe = Deferred::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(recipe_poll.owned());
    });
    scope.require([&recipe], DOCUMENT);
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(recipe);
    let (finished, done) = mpsc::channel();
    let teardown = std::thread::spawn(move || {
        drop(scope);
        drop(coordinator);
        finished.send(()).unwrap();
    });
    assert!(matches!(done.recv_timeout(Duration::from_millis(25)), Err(mpsc::RecvTimeoutError::Timeout)));
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(10)).unwrap();
    teardown.join().unwrap();
    assert!(released_poll.is_retired());
}

#[test]
fn lost_device_retirement_closes_its_workers_without_waiting_for_consumers() {
    struct Root(mpsc::Sender<std::thread::ThreadId>);
    impl Drop for Root {
        fn drop(&mut self) { self.0.send(std::thread::current().id()).unwrap(); }
    }
    let lost = crate::device_poll::Binding::default();
    let consumer = lost.owned();
    let old = native::Coordinator::with_binding(1, &lost).unwrap();
    let old_scope = old.scope().unwrap();
    let healthy = crate::device_poll::Binding::default();
    let sibling = native::Coordinator::with_binding(1, &healthy).unwrap();
    let sibling_scope = sibling.scope().unwrap();
    let (released, release) = mpsc::channel();
    native::retire_device_services(lost.owned(), Root(released)).unwrap();
    let closing_thread = release.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_ne!(closing_thread, std::thread::current().id());
    assert!(old.scope().is_none());
    assert!(native::Coordinator::with_binding(1, &consumer).is_err());
    assert!(sibling.scope().is_some());
    assert!(native::Coordinator::with_binding(1, &healthy).is_ok());
    drop((old_scope, sibling_scope, consumer));
    finish_shader_compiler_shutdown();
}

#[test]
fn process_finalization_joins_a_pending_device_retirement() {
    let poll = crate::device_poll::Binding::default();
    let coordinator = native::Coordinator::with_binding(1, &poll).unwrap();
    let scope = coordinator.scope().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let recipe = Deferred::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    scope.require([&recipe], DOCUMENT);
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    native::retire_device_services(poll, ()).unwrap();
    let (finished, done) = mpsc::channel();
    let finalizer = std::thread::spawn(move || {
        finish_shader_compiler_shutdown();
        finished.send(()).unwrap();
    });
    assert!(matches!(done.recv_timeout(Duration::from_millis(25)), Err(mpsc::RecvTimeoutError::Timeout)));
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(10)).unwrap();
    finalizer.join().unwrap();
    assert!(coordinator.scope().is_none());
    drop((scope, recipe));
}

#[test]
fn concurrent_device_finalizers_both_join_the_shader_workers() {
    let coordinator = native::Coordinator::with_workers(1, None).unwrap();
    let scope = coordinator.scope().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let recipe = Deferred::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    scope.require([&recipe], DOCUMENT);
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    coordinator.stop(true);
    let (joining, joins) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let mut finalizers = Vec::new();
    for _ in 0..2 {
        let coordinator = coordinator.clone();
        let joining = joining.clone();
        let finished = finished.clone();
        finalizers.push(std::thread::spawn(move || {
            joining.send(()).unwrap();
            coordinator.join();
            finished.send(()).unwrap();
        }));
    }
    joins.recv_timeout(Duration::from_secs(10)).unwrap();
    joins.recv_timeout(Duration::from_secs(10)).unwrap();
    let early = done.recv_timeout(Duration::from_millis(50));
    release.send(()).unwrap();
    for finalizer in finalizers { finalizer.join().unwrap(); }
    assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)), "a device finalizer returned before the shader worker exited");
}

#[test]
fn retired_scope_rejects_late_work_and_wakes_existing_owners() {
    struct Root(mpsc::Sender<()>);
    impl Drop for Root { fn drop(&mut self) { self.0.send(()).unwrap(); } }
    let binding=crate::device_poll::Binding::default();
    let coordinator=native::Coordinator::with_binding(1,&binding).unwrap();
    let scope=coordinator.scope().unwrap();
    let ready=Deferred::new(||());
    pollster::block_on(scope.prepare_all([&ready])).unwrap();
    let _batch=scope.batch();
    scope.enqueue(DOCUMENT,||Ok(()));
    let (wake,woken)=mpsc::channel();
    let weak=Arc::downgrade(&scope.0);
    scope.set_wake(Arc::new(move|| {
        let failed=weak.upgrade().is_some_and(|lease|Scope(lease).check().is_err());
        let _=wake.send(failed);
    }));
    while woken.try_recv().is_ok() {}
    let (released,release)=mpsc::channel();
    native::retire_device_services(binding.owned(),Root(released)).unwrap();
    release.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(scope.check().is_err(),"retirement must durably fail retained owners");
    assert!(woken.recv_timeout(Duration::from_secs(10)).unwrap(),"terminal wake must observe failure outside the queue lock");
    scope.enqueue(DOCUMENT,||panic!("retired work must not run"));
    let recipe=Deferred::pipeline(|_|Compilation::Ready(()));
    scope.require([&recipe],DOCUMENT);
    assert!(coordinator.with_queue(|queue|queue.entries.is_empty()),"retired scopes must not enqueue work without workers");
    assert!(scope.check().is_err());
    assert!(pollster::block_on(scope.prepare_all([&ready])).is_err());
    assert!(pollster::block_on(scope.prepare_all([&recipe])).is_err());
}
