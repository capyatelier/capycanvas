use super::*;

#[test]
fn diagnostics_restored_before_gpu_attachment_collects_actual_drawing_samples() {
    for platform in [0, 1] {
        let app = App::new(platform);
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"stats","visible":true}}));
        let host = unsafe { &mut *app.0 };
        host.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        host.host.session.sync_renderer_telemetry();
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let stats = app.request(2, json!({"type":"renderer_stats"})).unwrap();
        assert_eq!(stats["rows"].as_array().unwrap().len(), 7);
        assert!(!stats["samples"].as_array().unwrap().is_empty());
        assert!(stats["samples"].as_array().unwrap().len() <= 120);
        assert_ne!(stats["rows"][0]["value"], "—");
        assert!(
            stats["rows"][2]["value"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert!(
            stats["rows"][3]["value"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert!(
            stats["rows"][1]["description"]
                .as_str()
                .unwrap()
                .contains("Excludes presentation")
        );
        app.invoke("zoom_in");
        app.draw_frame();
        let navigated = app.request(2, json!({"type":"renderer_stats"})).unwrap();
        assert_eq!(navigated["rows"][3]["value"], stats["rows"][3]["value"]);
        let samples = navigated["samples"].clone();
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"stats","visible":false}}));
        app.stroke();
        app.draw_frame();
        assert_eq!(
            app.request(2, json!({"type":"renderer_stats"})).unwrap()["samples"],
            samples
        );
    }
}

fn placements(app: &App, value: Value) -> i32 {
    let source = CString::new(value.to_string()).unwrap();
    unsafe { capy_apple_navigator_placements(app.0, source.as_ptr()) }
}
fn slot() -> Value {
    json!({"bounds":[10,20,264,200],"clip":[12,30,200,170],"order":3})
}
fn overview(app: &App) -> Vec<layer_render_wgpu::OverviewPlacement> {
    let app = unsafe { &*app.0 };
    app.metal.overview_placements(&app.host)
}

#[test]
fn live_navigator_uses_current_document_camera_and_display_scale_without_bitmaps() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"stats","visible":true}}));
        assert_eq!(unsafe { capy_apple_resize(app.0, 2400, 1800, 2.) }, 0);
        app.draw_frame();
        assert_eq!(placements(&app, json!([slot()])), 0);
        let initial = overview(&app)[0];
        assert_eq!(initial.bounds, [28., 48., 512., 384.]);
        assert_eq!(initial.clip, Some([24., 60., 400., 340.]));
        assert_eq!(initial.scale, 2.);
        let document_revision = unsafe { &*app.0 }.host.session.engine().document().revision;
        for command in ["zoom_in", "rotate_right", "flip_horizontal"] {
            app.invoke(command);
        }
        let moved = overview(&app)[0];
        assert_eq!(moved.bounds, initial.bounds);
        assert_ne!(moved.work_area, initial.work_area);
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().revision,
            document_revision
        );
        app.stroke();
        app.draw_frame();
        assert!(
            !unsafe { &*app.0 }
                .host
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .unwrap()
                .canvas_preview_pending()
        );
        let project = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_new(project.0, 600, 300) }, 0);
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr())
            },
            0
        );
        app.draw_frame();
        // No new layout or bitmap delivery is needed to adopt a different aspect ratio.
        let replaced = overview(&app)[0];
        assert_eq!(replaced.bounds, [28., 112., 512., 256.]);
        assert_eq!(replaced.clip, initial.clip);
        assert_eq!(unsafe { capy_apple_resize(app.0, 1200, 900, 1.) }, 0);
        let resized = overview(&app)[0];
        assert_eq!(resized.bounds, replaced.bounds.map(|v| v * 0.5));
        assert_eq!(resized.clip, initial.clip.map(|c| c.map(|v| v * 0.5)));
        assert_eq!(resized.scale, 1.);
        app.draw_frame();
        let stats = app.request(2, json!({"type":"renderer_stats"})).unwrap();
        assert!(!stats["samples"].as_array().unwrap().is_empty());
        assert_eq!(placements(&app, json!([])), 0);
        assert!(overview(&app).is_empty());
    }
}

#[test]
fn navigator_layout_is_bounded_atomic_ordered_and_does_not_keep_idle_frames_awake() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let upper = json!({"bounds":[30,40,264,200],"clip":[30,40,264,200],"order":9});
        assert_eq!(placements(&app, json!([upper, slot()])), 0);
        let before = overview(&app);
        assert_eq!(before.len(), 2);
        assert!(before[0].bounds[0] < before[1].bounds[0]);
        unsafe { &mut *app.0 }.host.dirty = false;
        assert_eq!(placements(&app, json!([slot(), upper])), 0);
        assert!(
            !unsafe { &*app.0 }.host.dirty,
            "Identical sorted layout stays idle"
        );
        for invalid in [
            json!([{"bounds":[0,0,0,0],"clip":[0,0,1,1],"order":0}]),
            json!(vec![slot(); 33]),
            json!([{"bounds":[null,0,1,1],"clip":[0,0,1,1],"order":0}]),
        ] {
            assert_eq!(placements(&app, invalid), -1);
            assert_eq!(
                overview(&app),
                before,
                "Rejected geometry keeps the last valid layout"
            );
        }
        assert_eq!(placements(&app, json!([])), 0);
        assert!(
            unsafe { &*app.0 }.host.dirty,
            "Hiding the last preview clears its surface pixels"
        );
        assert!(overview(&app).is_empty());
    }
}

#[test]
fn navigator_geometry_and_gestures_preserve_document_pixels_and_history() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let pixels = app.pixels();
        let revision = unsafe { &*app.0 }.host.session.engine().document().revision;
        for _ in 0..6 {
            app.invoke("zoom_in");
        }
        app.invoke("rotate_right");
        app.invoke("flip_horizontal");
        let viewport = [264., 200.];
        let state = app.state();
        let source =
            CString::new(json!([state["camera"], [2048, 1536], viewport]).to_string()).unwrap();
        let output = unsafe { capy_apple_navigator_geometry(source.as_ptr()) };
        let geometry: Value =
            serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(output) };
        assert_eq!(
            geometry,
            app.request(2, json!({"type":"navigator","viewport":viewport}))
                .unwrap()
        );
        assert_eq!(
            geometry["image"],
            json!({"x":4.0,"y":4.0,"width":256.0,"height":192.0})
        );
        let camera = || unsafe { &*app.0 }.host.session.state().camera.clone();
        let send = |phase, position| {
            app.action(
                json!({"type":"navigator","phase":phase,"position":position,"viewport":viewport}),
            )
        };
        let middle = std::array::from_fn::<_, 2, _>(|i| {
            geometry["work_area"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p[i].as_f64().unwrap())
                .sum::<f64>()
                / 4.
        });
        let before = camera();
        send("down", middle);
        for (a, b) in before.translation.into_iter().zip(camera().translation) {
            assert!((a - b).abs() < 0.001);
        }
        send("move", [middle[0] + 16., middle[1] + 8.]);
        assert_ne!(camera().translation, before.translation);
        send("cancel", [0., 0.]);
        let stopped = camera();
        send("move", middle);
        assert_eq!(camera(), stopped);
        send("down", [9., 9.]);
        send("up", [9., 9.]);
        let current = camera();
        let [x, y] = current.work_area_center();
        let center = current.input_transform().map(layer_core::Point { x, y });
        assert!((center.x - 40.).abs() < 0.01 && (center.y - 40.).abs() < 0.01);
        app.draw_frame();
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().revision,
            revision
        );
        assert_eq!(app.pixels(), pixels);
        app.invoke("undo");
        app.draw_frame();
        assert_ne!(app.pixels(), pixels);
        app.invoke("redo");
        app.draw_frame();
        assert_eq!(app.pixels(), pixels);
    }
}
