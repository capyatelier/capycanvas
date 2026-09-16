//! Camera requests through the production GTK owner/worker/presentation path.
//! Synthetic software gestures do not measure physical device input latency.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
use std::sync::Arc;

fn photo(extent: [u32; 2]) -> layer_core::Project {
    let mut project = new_drawing(extent[0], extent[1]).unwrap();
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
    ] {
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
        _ => panic!("LAYER_NAVIGATION_PHOTO must be 24mp, 45mp or 60mp"),
    };
    let app = native_test_app("art.capycanvas.PhotoNavigation");
    let w = Workspace::with_project(&app, Some((photo(extent), None)));
    w.window.maximize();
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
    // Wake the real GLib event loop without polling sleeps that add artificial
    // presentation delay. Requests use an absolute 120 Hz schedule, not a wait
    // for the previous render, so slow frames cannot throttle the workload.
    let tick = glib::timeout_add_local(Duration::from_millis(1), || glib::ControlFlow::Continue);
    let start = Instant::now();
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
                    context.iteration(true);
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
                w.changed(change);
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
    pump(300);
    assert_eq!(
        w.gpu.borrow().as_ref().unwrap().session.engine().document(),
        &original
    );
    let stats = stats.lock().unwrap();
    assert!(stats.presented.iter().filter(|p| p[3] == 1).count() > 100);
    assert!(
        stats
            .camera_views
            .iter()
            .all(|v| v.2 == stats.camera_views[0].2),
        "navigation must not change the artwork preview revision"
    );
    let report = serde_json::json!({
        "extent": extent, "space": "ProPhoto", "depth": 16, "viewport": viewport,
        "gtk_renderer": w.window.renderer().unwrap().type_().name(),
        "requests": requests, "camera_views": stats.camera_views,
        "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
        "worker_thread_cpu": stats.thread_cpu, "worker_gpu": stats.gpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    drop(stats);
    std::fs::write(
        std::env::var("LAYER_PACING_REPORT").unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    w.window.destroy();
    pump(100);
}
