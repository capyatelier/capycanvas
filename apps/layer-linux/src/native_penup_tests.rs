//! Hardware comparison of native commit presentation and concurrent backing.
use super::*;

#[test]
#[ignore = "private 120 Hz Wayland display and release hardware GPU benchmark"]
fn native_penup_and_following_strokes() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
    let app = native_test_app("art.capycanvas.NativePenupPacing");
    let mut project = new_drawing(4096, 4096).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    for _ in 0..31 {
        let id = project.document.allocate_layer_id();
        let position = project.document.layers.len() - 1;
        project.document.layers.insert(position, layer_core::Layer::paint(id, "pacing layer"));
    }
    project.validate(Default::default()).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::PaletteKnife as u32,
    });
    w.dispatch(UiAction::SetBrushSize { value: 720. });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(5);
        if w.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            engine.backend().startup.complete
                && !engine.has_pending_document_edits()
                && engine
                    .backend()
                    .paint_ready(engine.document(), engine.configured_brush(), false)
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "native pen-up fixture startup");
    }
    let stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    let camera = state(&w).camera;
    let context = glib::MainContext::default();
    let due = Rc::new(Cell::new(false));
    let tick = glib::timeout_add_local(
        Duration::from_millis(2),
        glib::clone!(
            #[strong]
            due,
            move || {
                due.set(true);
                glib::ControlFlow::Continue
            }
        ),
    );
    let mut sequence = 0;
    let mut pending = Vec::<(usize, layer_core::raster::RasterRevision)>::new();
    let mut backed = Vec::<[u64; 2]>::new();
    let mut ups = Vec::new();
    let observe = |pending: &mut Vec<(usize, layer_core::raster::RasterRevision)>,
                   backed: &mut Vec<[u64; 2]>| {
        pending.retain(|(stroke, root)| {
            if root.host_backed() {
                backed.push([*stroke as u64, glib::monotonic_time() as u64 * 1000]);
                false
            } else {
                true
            }
        });
    };
    let count: usize = std::env::var("LAYER_PENUP_STROKES").map_or(12, |v| v.parse().unwrap());
    assert!((4..=100).contains(&count));
    let mut expected = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .metrics()
        .committed_strokes;
    // The first contact warms the actual brush path, then undo completes before
    // statistics reset. Subsequent contacts overlap the previous backing work.
    for stroke in 0..=count {
        let start = Instant::now();
        let mut first = true;
        let mut last = None;
        while start.elapsed() < Duration::from_millis(800) {
            let t = (start.elapsed().as_secs_f32() / 0.8).min(1.);
            let m = camera.document_to_surface();
            let x = 400. + 3250. * t;
            let y = 2048. + 1100. * (t * std::f32::consts::TAU + stroke as f32 * 0.3).sin();
            let event = PenEvent {
                device_id: 1,
                sequence,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: m[0] * x + m[2] * y + m[4],
                    y: m[1] * x + m[3] * y + m[5],
                },
                pressure: 0.8,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase: if first {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            sequence += 1;
            w.cursor_input(Some(event));
            w.input.send(&w, event);
            first = false;
            last = Some(event);
            while !due.replace(false) {
                context.iteration(true);
            }
            observe(&mut pending, &mut backed);
        }
        let up = PenEvent {
            sequence,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            phase: PenPhase::Up,
            ..last.unwrap()
        };
        sequence += 1;
        w.input.send(&w, up);
        expected += 1;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            pump(1);
            let gpu = w.gpu.borrow();
            let engine = gpu.as_ref().unwrap().session.engine();
            if engine.metrics().committed_strokes == expected {
                let root = engine
                    .document()
                    .layers
                    .iter()
                    .find(|l| l.id == engine.document().active_layer)
                    .unwrap()
                    .raster
                    .clone();
                pending.push((stroke, root));
                break;
            }
            assert!(Instant::now() < deadline, "stroke admission");
        }
        if stroke == 0 {
            while !pending.is_empty() {
                pump(2);
                observe(&mut pending, &mut backed);
                assert!(Instant::now() < deadline, "warmup backing");
            }
            w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            loop {
                pump(2);
                if !w
                    .gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .has_pending_document_edits()
                {
                    break;
                }
                assert!(Instant::now() < deadline, "warmup undo");
            }
            pump(150);
            *stats.lock().unwrap() = Default::default();
            backed.clear();
        } else {
            ups.push([stroke as u64, up.timestamp_ns]);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pending.is_empty() {
        pump(2);
        observe(&mut pending, &mut backed);
        assert!(Instant::now() < deadline, "backing drain");
    }
    tick.remove();
    pump(200);
    let stats = stats.lock().unwrap();
    assert_eq!(
        stats.raster_commits.len(),
        count,
        "each pen-up must render once"
    );
    assert_eq!(backed.len(), count);
    assert!(stats.presented.iter().filter(|p| p[3] == 1).count() > 100);
    let report = serde_json::json!({
        "document": {"extent": [4096,4096], "space": "ProPhoto", "depth": 16, "paint_layers": 32},
        "brush": "PaletteKnife", "brush_size": 720, "contact_ms": 800, "contacts": count,
        "viewport": camera.viewport, "gtk_renderer": w.window.renderer().unwrap().type_().name(),
        "path": "app-owned Wayland Vulkan subsurface", "backing_observation": "first observed host-backed at 2ms event-loop sampling",
        "penups": ups, "host_backed": backed, "raster_commits": stats.raster_commits,
        "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
        "worker_thread_cpu": stats.thread_cpu, "worker_gpu": stats.gpu,
        "canvas_presentation": stats.presented,
    });
    drop(stats);
    let path = std::env::var("LAYER_PACING_REPORT").unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    w.window.destroy();
    pump(100);
}
