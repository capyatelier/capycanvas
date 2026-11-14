use super::*;
pub(crate) fn scope(workers: usize) -> Scope {
    native::Coordinator::with_workers(workers, None)
        .unwrap()
        .scope()
        .unwrap()
}

pub(crate) fn requested<T>(scope: &Scope, recipe: &Deferred<T>, class: Class) -> bool {
    scope.with_queue(|q| {
        q.entries
            .get(&Key::Recipe(recipe.identity()))
            .is_some_and(|e| e.class <= class)
    })
}

pub(crate) fn queued_recipes(scope: &Scope) -> usize {
    scope.with_queue(|queue| {
        queue.refresh();
        queue.entries.iter().filter(|(key, entry)| matches!(key, Key::Recipe(_))
            && entry.demands.iter().any(|demand| demand.owner == scope.0.id)).count()
    })
}

pub(crate) fn demands(scope: &Scope, class: Class) -> std::collections::HashSet<usize> {
    scope.with_queue(|queue| {
        queue.refresh();
        queue.entries.iter().filter_map(|(key, entry)| match key {
            Key::Recipe(id) if entry.demands.iter().any(|d| d.owner == scope.0.id && d.class <= class) => Some(*id),
            _ => None,
        }).collect()
    })
}

pub(crate) fn ordered_demands(scope: &Scope, class: Class) -> Vec<usize> {
    scope.with_queue(|queue| {
        queue.refresh();
        let mut entries: Vec<_> = queue.entries.iter().filter_map(|(key, entry)| match key {
            Key::Recipe(id) if entry.class == class && entry.demands.iter().any(|d| d.owner == scope.0.id && d.class == class) => Some((entry.sequence, *id)),
            _ => None,
        }).collect();
        entries.sort_unstable();
        entries.into_iter().map(|(_, id)| id).collect()
    })
}

#[test]
fn startup_declares_stroke_kernels_before_eraser_helpers_and_document_work() {
    let color = Default::default();
    let reference = crate::WgpuRasterizer::new_native_headless(color).unwrap();
    let mut renderer = crate::test_support::staged_renderer(&reference, color);
    let document = layer_core::Document::new(
        layer_core::PortableId::random(),
        64,
        64,
        layer_core::DocumentNames {
            paint: "Ink".into(),
            paper: "Paper".into(),
        },
    );
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
    let batch = renderer.compilation.batch();
    renderer.prepare_startup(&document, &brush, false).unwrap();
    let mut expected = Vec::new();
    for tool in [
        layer_core::StrokeTool::Brush,
        layer_core::StrokeTool::Eraser,
    ] {
        let mut style = layer_render::DabStyle::for_brush(&brush, tool);
        style.blend_space = document.composition().blend;
        style.paper = style
            .grain
            .and(document.drawing_target())
            .map(|target| document.paper_binding(target));
        let plan = crate::BrushPassPlan::for_device(&style, &renderer.device);
        for extra in crate::dry_material::Reach::of(&brush).required_variants(&style) {
            for coverage in [
                Some(plan.state.coverage),
                plan.state.coverage.then_some(false),
            ]
            .into_iter()
            .flatten()
            {
                let key = Key::Recipe(
                    renderer
                        .pipelines
                        .dry_material
                        .selected_kernel(&style, plan.material, coverage, extra)
                        .identity(),
                );
                if !expected.contains(&key) {
                    expected.push(key);
                }
            }
        }
    }
    let actual = renderer.compilation.with_queue(|queue| {
        let mut entries: Vec<_> = queue.entries.iter().collect();
        entries.sort_by_key(|(_, entry)| (entry.class, entry.sequence));
        entries.into_iter().map(|(key, _)| *key).collect::<Vec<_>>()
    });
    drop(batch);
    drop(renderer);
    drop(reference);
    finish_shader_compiler_shutdown();
    assert!(!expected.is_empty());
    assert_eq!(&actual[..expected.len()], expected.as_slice());
}

#[test]
fn a_panicking_wake_fails_its_owner_and_keeps_the_worker_running() {
    let compiler = scope(1);
    let healthy = compiler.0.coordinator.scope().unwrap();
    compiler.set_wake(Arc::new(|| panic!("host wake failure")));
    let recipe = Deferred::new(|| 17);
    compiler.require([&recipe], Class::Required);
    healthy.require([&recipe], Class::Required);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let error = loop {
        if let Err(error) = compiler.check() {
            break error;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert!(error.to_string().contains("host wake failure"));
    assert!(healthy.check().is_ok());
    let (completed, done) = std::sync::mpsc::channel();
    healthy.enqueue(Class::Required, move || {
        completed.send(()).unwrap();
        Ok(())
    });
    done.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(recipe.ready());
}

#[test]
fn completed_and_cancelled_recipes_release_their_resources() {
    use crate::Compilation;
    for cancel in [false, true] {
        let coordinator = native::Coordinator::with_workers(1, None).unwrap();
        let scope = coordinator.scope().unwrap();
        scope.idle(!cancel, !cancel);
        let resource = Arc::new(());
        let released = Arc::downgrade(&resource);
        let recipe = Deferred::pipeline(move |_| Compilation::Ready(resource));
        scope.warm(&recipe, Class::Background, 0);
        drop(recipe);
        if cancel {
            drop(scope);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while released.upgrade().is_some() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            released.upgrade().is_none(),
            "coordinator retained recipe resources: cancel={cancel}"
        );
    }
}

#[test]
fn a_paused_required_declaration_blocks_optional_work_from_another_owner() {
    let mut queue = Queue::<()>::new(1);
    let required = queue.owner();
    let optional = queue.owner();
    queue.owners.get_mut(&required).unwrap().paused = 1;
    queue.submit(
        Key::Recipe(1),
        Demand {
            owner: required,
            generation: None,
            class: Class::Required,
        },
        true,
        || (),
    );
    queue.submit(
        Key::Recipe(2),
        Demand {
            owner: optional,
            generation: None,
            class: Class::Background,
        },
        true,
        || (),
    );
    assert!(queue.take(Instant::now(), true).is_none());
}

#[test]
fn final_shutdown_joins_a_pool_retained_by_the_device_cache() {
    use std::sync::mpsc;
    let coordinator = native::Coordinator::with_workers(1, None).unwrap();
    let scope = coordinator.scope().unwrap();
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    scope.enqueue(Class::Required, move || {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
        finished.send(()).unwrap();
        Ok(())
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(scope);
    let (joined, join_done) = mpsc::channel();
    let joining = std::thread::spawn(move || {
        finish_shader_compiler_shutdown();
        joined.send(()).unwrap();
    });
    let returned_while_compiling = join_done.recv_timeout(Duration::from_millis(100)).is_ok();
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    joining.join().unwrap();
    drop(coordinator);
    finish_shader_compiler_shutdown();
    assert!(!returned_while_compiling);
}

#[test]
fn failed_shared_recipes_publish_one_failure_and_never_retry() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let scope = scope(1);
    let second = scope.0.coordinator.scope().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let recipe = Deferred::pipeline(move |_| -> crate::Compilation<()> {
        count.fetch_add(1, Ordering::Relaxed);
        panic!("shared compile failure");
    });
    let batch = scope.batch();
    scope.require([&recipe], Class::Required);
    second.require([&recipe], Class::Required);
    drop(batch);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while scope.pending() + second.pending() != 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(scope.check().is_err());
    assert!(second.check().is_err());
    let later = scope.0.coordinator.scope().unwrap();
    assert!(!later.require([&recipe], Class::Required));
    assert!(later.check().is_err());
    assert_eq!(later.pending(), 0);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn permitted_factories_resolve_nested_recipes_and_restore_the_permit() {
    let scope = scope(1);
    let nested = Deferred::new(|| 17);
    let recipe = Deferred::new(move || *nested + 1);
    pollster::block_on(scope.prepare_all([&recipe])).unwrap();
    assert_eq!(*recipe, 18);
    assert!(!EXECUTING.get());
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scope.blocking(|| panic!("factory"))
    }));
    assert!(failure.is_err());
    assert!(!EXECUTING.get());
}

#[test]
fn factory_renderers_and_snapshot_devices_share_one_compilation_slot() {
    let renderer = crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let factory = renderer.renderer_factory();
    let second = factory.rasterizer(Default::default()).unwrap();
    let device = factory.pipeline_device(Default::default());
    let snapshot = device.compilation().unwrap();
    assert!(Arc::ptr_eq(
        &renderer.compilation.0.coordinator,
        &second.compilation.0.coordinator
    ));
    assert!(Arc::ptr_eq(
        &renderer.compilation.0.coordinator,
        &snapshot.0.coordinator
    ));
}
