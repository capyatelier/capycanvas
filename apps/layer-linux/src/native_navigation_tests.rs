//! Camera requests through the production GTK owner/worker/presentation path.
//! Synthetic software gestures do not measure physical device input latency.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
use std::sync::Arc;

fn photo(extent: [u32; 2]) -> layer_core::Project {
    let mut project = new_drawing(1, 1).unwrap();
    // Imported photographs may exceed the New Drawing dialog's size ceiling.
    project.document.width = extent[0];
    project.document.height = extent[1];
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let mut source = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    let mut random = 0x1357abcdu32;
    let mut row = Vec::with_capacity(extent[0] as usize * 8);
    for y in 0..extent[1] {
        row.clear();
        for x in 0..extent[0] {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let noise = (random % 1024) as u16;
            for code in [
                (u64::from(x) * 55000 / u64::from(extent[0])) as u16 + noise,
                (u64::from(y) * 55000 / u64::from(extent[1])) as u16 + noise,
                ((u64::from(x) + u64::from(y)) * 13 % 60000) as u16 + noise,
                65535,
            ] {
                row.extend_from_slice(&code.to_le_bytes());
            }
        }
        source.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
    for _ in 0..31 {
        let id = project.document.allocate_layer_id();
        project
            .document
            .layers
            .insert(1, layer_core::Layer::paint(id, "empty"));
    }
    for (name, key, value) in [
        ("exposure", "exposure", 0.25),
        ("white_balance", "temperature", 4.),
        ("levels", "gamma", 1.08),
        ("hue_saturation", "saturation", 5.),
        ("color_balance", "midtones_red", 2.),
    ].into_iter().chain(
        (std::env::var("LAYER_NAVIGATION_PHYSICAL").as_deref() == Ok("1"))
            .then_some(("gaussian_blur", "sigma", 4.)),
    ) {
        let id = project.document.allocate_layer_id();
        let mut layer = layer_core::Layer::paint(id, name);
        layer.kind = layer_core::LayerKind::Effect;
        let mut effect = layer_core::EffectInstance::new(
            layer_core::bundled_effect_catalog()
                .get(name)
                .unwrap()
                .program(),
        );
        effect
            .set(key, layer_core::EffectValue::Number(value))
            .unwrap();
        layer.effect = Some(Arc::new(effect));
        project.document.layers.insert(0, layer);
    }
    project.validate(Default::default()).unwrap();
    project
}

#[test]
#[ignore = "private 120 Hz Wayland display; release hardware navigation qualification"]
fn native_large_photo_navigation() {
    let extent = match std::env::var("LAYER_NAVIGATION_PHOTO")
        .as_deref()
        .unwrap_or("60mp")
    {
        "24mp" => [6000, 4000],
        "45mp" => [8192, 5504],
        "60mp" => [8192, 7324],
        "61mp" => [9504, 6336],
        _ => panic!("LAYER_NAVIGATION_PHOTO must be 24mp, 45mp, 60mp or 61mp"),
    };
    let app = native_test_app("art.capycanvas.PhotoNavigation");
    let w = Workspace::with_project(&app, Some((photo(extent), None)));
    if std::env::var("LAYER_NAVIGATION_MAXIMIZE").as_deref() == Ok("0") {
        w.window.set_default_size(1200, 900);
    } else {
        w.window.maximize();
    }
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        pump(5);
        let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
            assert!(!g.session.rendering_suspended());
            g.session.engine().backend().startup.complete
                && !g.session.engine().has_pending_document_edits()
                && w.frame_timer.borrow().is_none()
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "photo startup must settle");
    }
    let proof = super::proof::benchmark_proof(&w);
    let settle_ms = std::env::var("LAYER_NAVIGATION_SETTLE_MS")
        .ok().map(|v| v.parse::<u64>().unwrap()).unwrap_or(0);
    pump(settle_ms);
    let concurrent = (std::env::var("LAYER_NAVIGATION_CONCURRENT").as_deref() == Ok("1")).then(|| {
        let gpu = w.snapshot_gpu().unwrap();
        let snapshot = {
            let g = w.gpu.borrow();
            let session = &g.as_ref().unwrap().session;
            DocumentExport { project: session.capture_project_recovery().unwrap(),
                background: session.engine().view().background_rgba_linear,
                time: session.engine().animation_time() }
        };
        let prefix = std::path::PathBuf::from(std::env::var("LAYER_PACING_REPORT").unwrap());
        std::thread::spawn(move || {
            let start = Instant::now();
            let project = prefix.with_extension("capy");
            layer_core::atomic_write(&project, |file| snapshot.project.write(file)).unwrap();
            let saved_ms = start.elapsed().as_secs_f64() * 1000.;
            let reopened = layer_core::Project::read(std::fs::File::open(&project).unwrap(), Default::default()).unwrap();
            assert_eq!(reopened.document.proof, snapshot.project.document.proof);
            let recipe = ExportRecipe::further_editing(snapshot.project.document.color);
            let delivery = prefix.with_extension("tif");
            crate::files::export::write_snapshot(gpu, snapshot, recipe, &delivery, &Default::default()).unwrap();
            serde_json::json!({"save_ms": saved_ms, "save_export_ms": start.elapsed().as_secs_f64()*1000.,
                "master_bytes": std::fs::metadata(project).unwrap().len(),
                "delivery_bytes": std::fs::metadata(delivery).unwrap().len()})
        })
    });
    let original = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .clone();
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
    *stats.lock().unwrap() = Default::default();
    let viewport = state(&w).camera.viewport;
    let fit = (viewport[0] as f32 / extent[0] as f32).min(viewport[1] as f32 / extent[1] as f32);
    let context = glib::MainContext::default();
    let gtk_frames = Rc::new(RefCell::new(Vec::new()));
    let gtk_phases = Rc::new(RefCell::new(Vec::new()));
    let gtk_frame_start = Rc::new(Cell::new(0_u64));
    let frame_clock = w.window.frame_clock().unwrap();
    let before_paint = frame_clock.connect_before_paint(glib::clone!(
        #[strong] gtk_frame_start,
        move |_| gtk_frame_start.set(glib::monotonic_time() as u64 * 1000)
    ));
    let after_paint = frame_clock.connect_after_paint(glib::clone!(
        #[strong] gtk_frames,
        #[strong] gtk_frame_start,
        move |_| gtk_frames.borrow_mut().push([
            gtk_frame_start.get(), glib::monotonic_time() as u64 * 1000,
        ])
    ));
    let layout = frame_clock.connect_layout(glib::clone!(
        #[strong] gtk_phases,
        move |_| gtk_phases.borrow_mut().push(("layout", glib::monotonic_time() as u64 * 1000))
    ));
    let paint = frame_clock.connect_paint(glib::clone!(
        #[strong] gtk_phases,
        move |_| gtk_phases.borrow_mut().push(("paint", glib::monotonic_time() as u64 * 1000))
    ));
    let mut slow_iterations = Vec::new();
    // Wake the real GLib event loop without polling sleeps that add artificial
    // presentation delay. Requests use an absolute 120 Hz schedule, not a wait
    // for the previous render, so slow frames cannot throttle the workload.
    let tick = glib::timeout_add_local(Duration::from_millis(1), || glib::ControlFlow::Continue);
    let input_phase_ns = std::env::var("LAYER_NAVIGATION_PHASE_NS")
        .ok().map(|v| v.parse::<u64>().unwrap());
    let start = if let Some(phase) = input_phase_ns {
        let gpu = w.gpu.borrow();
        let clock = &gpu.as_ref().unwrap().session.engine().backend().clock;
        let now = glib::monotonic_time() as u64 * 1000;
        assert!(phase < clock.period());
        let due = clock.presentation(now) + 2 * clock.period() + phase;
        Instant::now() + Duration::from_nanos(due.saturating_sub(now))
    } else { Instant::now() };
    let mut requests = Vec::new();
    for (phase, fixed_scale) in [
        ("fit-pan-rotate", fit),
        ("half-pan-rotate", 0.5),
        ("native-pan-rotate", 1.),
        ("double-pan-rotate", 2.),
        ("zoom-pan-rotate", 0.),
    ] {
        for repeat in 0..2 {
            for step in 0..96 {
                let due = start + Duration::from_secs_f64(requests.len() as f64 / 120.);
                while Instant::now() < due {
                    let before = glib::monotonic_time() as u64 * 1000;
                    context.iteration(true);
                    let after = glib::monotonic_time() as u64 * 1000;
                    if after - before > 2_000_000 {
                        slow_iterations.push([before, after]);
                    }
                }
                let angle = step as f32 / 95. * std::f32::consts::TAU;
                let scale = if fixed_scale == 0. {
                    fit * (2. / fit).powf((angle.sin() + 1.) * 0.5)
                } else {
                    fixed_scale
                };
                let center = [0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin()];
                let camera = state(&w).camera;
                let m = camera.document_to_surface();
                let [x, y] = std::array::from_fn(|i| center[i] * extent[i] as f32);
                let from = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
                let to = viewport.map(|v| v as f32 * 0.5);
                let requested_ns = glib::monotonic_time() as u64 * 1000;
                let change = w.gpu.borrow_mut().as_mut().unwrap().session.gesture(
                    from,
                    to,
                    scale / camera.zoom,
                    angle - camera.rotation,
                );
                assert!(change.is_ok(), "navigation failed: {change:?}");
                w.changed(change);
                assert!(!w.gpu.borrow().as_ref().unwrap().session.rendering_suspended(),
                    "GPU worker stopped during {phase}, repeat {repeat}, step {step}");
                requests.push(serde_json::json!({
                    "phase": phase, "repeat": repeat, "step": step,
                    "requested_ns": requested_ns,
                    "lateness_ms": due.elapsed().as_secs_f64() * 1000.,
                    "matrix": state(&w).camera.document_to_surface(),
                }));
            }
        }
    }
    tick.remove();
    frame_clock.disconnect(before_paint);
    frame_clock.disconnect(after_paint);
    frame_clock.disconnect(layout);
    frame_clock.disconnect(paint);
    pump(300);
    assert!(w.frame_timer.borrow().is_none(), "idle navigation must stop requesting frames");
    assert!(!w.gpu.borrow().as_ref().unwrap().session.rendering_suspended());
    assert_eq!(
        w.gpu.borrow().as_ref().unwrap().session.engine().document(),
        &original
    );
    let stats = stats.lock().unwrap();
    let unchanged_work = stats.camera_work.first().is_some_and(|work|
        stats.camera_work.iter().all(|frame| frame[1] == work[1] && frame[4] == 0));
    assert!(stats.presented.iter().filter(|p| p[3] == 1).count() > 100);
    assert!(
        stats
            .camera_views
            .iter()
            .all(|v| v.2 == stats.camera_views[0].2),
        "navigation must not change the artwork preview revision"
    );
    let mut report = serde_json::json!({
        "proof": proof,
        "extent": extent, "space": "ProPhoto", "depth": 16, "viewport": viewport,
        "gtk_renderer": w.window.renderer().unwrap().type_().name(),
        "requests": requests, "camera_views": stats.camera_views,
        "camera_work": stats.camera_work,
        "monitor_scale": w.area.scale_factor(),
        "physical_filter": std::env::var("LAYER_NAVIGATION_PHYSICAL").as_deref() == Ok("1"),
        "input_phase_ns": input_phase_ns,
        "gtk_frames_ns": *gtk_frames.borrow(),
        "gtk_phases_ns": *gtk_phases.borrow(),
        "settle_ms": settle_ms,
        "slow_event_loop_iterations_ns": slow_iterations,
        "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
        "worker_thread_cpu": stats.thread_cpu, "worker_gpu": stats.gpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    drop(stats);
    if let Some(worker) = concurrent { report["concurrent"] = worker.join().unwrap(); }
    std::fs::write(
        std::env::var("LAYER_PACING_REPORT").unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    // Preserve the evidence for a failed zero-work assertion as well as passes.
    if std::env::var("LAYER_NAVIGATION_COMPLETE").as_deref() == Ok("1") {
        assert!(unchanged_work,
            "complete display navigation must not recompose or decode source tiles");
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display; real GTK thumbnail publication and history"]
fn native_photo_thumbnail_finishes_after_idle_and_restores_on_undo() {
    let app = native_test_app("art.capycanvas.PhotoThumbnailIdle");
    let mut project = photo([2049, 1537]);
    project.document.layers.retain(|layer| layer.source.is_some());
    let original = project.document.clone();
    let target = original.layers[0].id.0;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    fn picture(root: &gtk::Widget) -> Option<gtk::Picture> {
        if let Ok(picture) = root.clone().downcast() { return Some(picture); }
        let mut child = root.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(picture) = picture(&widget) { return Some(picture); }
        }
        None
    }
    let pixels = || -> Option<Vec<u8>> {
        let button = find_css(w.layer_panel.root.upcast_ref(), "layer-thumbnail")?;
        let texture: gdk::Texture = picture(&button)?.paintable()?.downcast().ok()?;
        let mut bytes = vec![0; (texture.width() * texture.height() * 4) as usize];
        texture.download(&mut bytes, texture.width() as usize * 4);
        Some(bytes)
    };
    let wait = |predicate: &dyn Fn(&[u8]) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            pump(5);
            assert!(!w.gpu.borrow().as_ref().is_some_and(|g| g.session.rendering_suspended()));
            if let Some(bytes) = pixels().filter(|bytes| predicate(bytes)) { break bytes; }
            assert!(Instant::now() < deadline, "idle thumbnail must finish and publish");
        }
    };
    let before = wait(&|_| true);
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Clear { id: target } });
    let cleared = wait(&|bytes| bytes != before);
    assert_ne!(before, cleared);
    click(&command(&w, CommandId::Undo));
    assert_eq!(wait(&|bytes| bytes == before), before);
    let mut restored = w.gpu.borrow().as_ref().unwrap().session.engine().document().clone();
    // Undo publishes a new document revision while restoring exact artwork.
    assert!(restored.revision > original.revision);
    restored.revision = original.revision;
    assert_eq!(restored, original);
    w.window.destroy();
    pump(100);
}
