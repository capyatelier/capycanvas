use super::*;
use layer_render::CanvasRenderer;

#[test]
#[ignore = "private 3840x2160 scale-2 Wayland display and hardware GPU"]
fn native_cold_first_photo_tab_zoom() {
    let fixture = |extent| {
        glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
            let mut project = photo(extent);
            retain_photos(&mut project);
            assert_eq!(project.scene().children(None).len(), 1);
            project
        })).unwrap()
    };
    let now = || glib::monotonic_time() as u64 * 1000;
    let output = std::path::PathBuf::from(std::env::var("LAYER_PACING_REPORT").unwrap());
    let app = native_test_app("art.capycanvas.ColdFirstPhotoZoom");
    let w = Workspace::with_project(&app, Some((fixture([9504, 6336]), None)));
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    new_photo::invoke(&w, CommandId::FitCanvas);
    new_photo::ready(&w);
    let original = ui_session(&w).engine().document().clone();
    let camera = state(&w).camera;
    let viewport = camera.viewport;
    let fit = camera.zoom;
    assert_eq!(viewport, [3840, 2160]);
    assert_eq!(w.area.scale_factor(), 2);
    assert!(fit > 0. && fit < 1.41);
    let first = w.documents.selected();
    glib::MainContext::default().block_on(w.documents.open(&w, (fixture([6000, 4000]), None))).unwrap();
    new_photo::ready(&w);
    until(|| w.documents.parked_memory().1, "parked 61 MP renderer retires");
    let second = w.documents.selected();
    assert_ne!(first, second);

    let mut targets = Vec::new();
    let mut zoom = fit;
    for (upward, end) in [(true, 16.), (false, fit), (true, 16.), (false, fit)] {
        loop {
            zoom = if upward { (zoom * 1.03).min(end) } else { (zoom / 1.03).max(end) };
            targets.push(zoom);
            if zoom == end { break; }
        }
    }
    let center = viewport.map(|v| v as f32 * 0.5);
    let context = glib::MainContext::default();
    let tick = glib::timeout_add_local(Duration::from_millis(1), || glib::ControlFlow::Continue);
    let mut requests = Vec::with_capacity(targets.len());
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(60);
    input.ready();
    let mut captured_report = None;
    let _frames = super::super::ui_motion::record_pixels(&w, &mut input, "cold-first-photo-tab-zoom",
        [[w.surface.width() as f32 * 0.5, w.surface.height() as f32 * 0.5]], || {
        let switch_ns = now();
        w.documents.select(&w, first, false);
        until(|| w.documents.selected() == first && !w.documents.changing.get(), "61 MP tab is adopted");
        let adopted_ns = now();
        let stats = {
            let mut gpu = w.gpu.borrow_mut();
            let gpu = gpu.as_mut().unwrap();
            gpu.session.renderer_mut().set_telemetry_enabled(true);
            gpu.session.begin_view_gesture();
            gpu.session.engine().backend().stats.clone()
        };
        let motion_begin_ns = now();
        let start = Instant::now();
        for (step, target) in targets.iter().copied().enumerate() {
            let due = start + Duration::from_secs_f64(step as f64 / 120.);
            while Instant::now() < due { context.iteration(true); }
            let requested_ns = now();
            let current = state(&w).camera.zoom;
            let change = w.gpu.borrow_mut().as_mut().unwrap().session.multi_touch_gesture(center, center, target / current, 0.);
            assert!(change.is_ok(), "navigation failed: {change:?}");
            w.navigation_changed(change);
            assert!(!ui_session(&w).rendering_suspended(), "GPU worker stopped during cold zoom step {step}");
            requests.push(serde_json::json!({
                "phase": "cold-first-zoom", "repeat": 0, "step": step,
                "requested_ns": requested_ns, "target_zoom": target,
                "zoom": state(&w).camera.zoom,
                "matrix": state(&w).camera.document_to_surface(),
                "lateness_ms": due.elapsed().as_secs_f64() * 1000.,
            }));
        }
        let motion_end_ns = now();
        tick.remove();
        pump(300);
        let idle_after_300_ms = w.frame_timer.borrow().is_none();
        let deadline = Instant::now() + Duration::from_secs(30);
        while w.frame_timer.borrow().is_some() && Instant::now() < deadline { pump(5); }
        let settled_after_input_ms = (now() - motion_end_ns) as f64 / 1e6;
        let idle = w.frame_timer.borrow().is_none();
        let suspended = ui_session(&w).rendering_suspended();
        let document_unchanged = ui_session(&w).engine().document() == &original;
        let final_zoom = state(&w).camera.zoom;
        let presented;
        let report = {
            let stats = stats.lock().unwrap();
            presented = stats.presented.iter().filter(|p| p[3] == 1 && motion_begin_ns <= p[1] && p[1] <= motion_end_ns).count();
            serde_json::json!({
                "extent": [9504, 6336], "viewport": viewport, "monitor_scale": w.area.scale_factor(),
                "space": "ProPhoto", "depth": original.composition().color.depth.bits(),
                "gtk_renderer": w.window.renderer().unwrap().type_().name(),
                "first": first, "second": second, "fit": fit,
                "switch_ns": switch_ns, "adopted_ns": adopted_ns,
                "motion_begin_ns": motion_begin_ns, "motion_end_ns": motion_end_ns,
                "requests": requests, "camera_views": stats.camera_views,
                "camera_publications": stats.camera_publications, "camera_work": stats.camera_work,
                "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
                "worker_thread_cpu": stats.thread_cpu, "worker_gpu": stats.gpu,
                "canvas_presentation": stats.presented, "source_transfers": stats.source_transfers,
                "idle_after_300_ms": idle_after_300_ms, "settled_after_input_ms": settled_after_input_ms,
                "idle_navigation": idle, "rendering_suspended": suspended,
                "document_unchanged": document_unchanged,
                "presented_during_motion": presented, "final_zoom": final_zoom,
            })
        };
        captured_report = Some((report, idle, suspended, document_unchanged, presented, final_zoom));
    });
    let (report, idle, suspended, document_unchanged, presented, final_zoom) = captured_report.unwrap();
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
    })).unwrap().unwrap();
    input.finish();
    w.window.destroy();
    pump(100);
    assert!(idle, "idle navigation must stop requesting frames");
    assert!(!suspended);
    assert!(document_unchanged, "navigation must not change the document");
    assert!(presented > 100, "cold navigation presented only {presented} frames");
    assert!((final_zoom - fit).abs() < 1e-5, "far gesture returns to Fit");
}
