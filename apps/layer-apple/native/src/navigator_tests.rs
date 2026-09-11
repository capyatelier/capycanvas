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

fn key(app: &App) -> CapyNavigatorKey {
    let mut value = CapyNavigatorKey::default();
    unsafe { capy_apple_navigator_key(app.0, &mut value) };
    value
}

#[test]
fn opening_a_document_replaces_pending_navigator_pixels_and_retains_diagnostics() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"stats","visible":true}}));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let old_key = key(&app);
        assert_eq!(poll(&app, 1_000_000_000, true), (1, std::ptr::null_mut()));
        let project = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_new(project.0, 600, 300) }, 0);
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr())
            },
            0
        );
        app.draw_frame();
        let next_key = key(&app);
        assert_ne!(next_key.epoch, old_key.epoch);
        let image = ready(&app, 2_000_000_000);
        unsafe {
            let mut info = std::mem::MaybeUninit::uninit();
            capy_preview_image_read(image, info.as_mut_ptr());
            let info = info.assume_init();
            assert_eq!(info.key, next_key);
            assert_eq!((info.width, info.height), (256, 128));
            assert!(
                std::slice::from_raw_parts(info.pixels, info.count)
                    .iter()
                    .all(|v| *v == 255)
            );
            capy_preview_image_free(image);
        }
        let stats = app.request(2, json!({"type":"renderer_stats"})).unwrap();
        assert!(!stats["samples"].as_array().unwrap().is_empty());
    }
}
fn poll(app: &App, now: u64, visible: bool) -> (i32, *mut CapyPreviewImage) {
    let mut image = std::ptr::null_mut();
    let status =
        unsafe { capy_apple_navigator_preview(app.0, now, u32::from(visible), &mut image) };
    assert!(status >= 0);
    (status, image)
}
fn wait_gpu(app: &App) {
    unsafe { &*app.0 }
        .host
        .session
        .engine()
        .backend()
        .0
        .as_ref()
        .unwrap()
        .device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
}
fn ready(app: &App, now: u64) -> *mut CapyPreviewImage {
    let (status, image) = poll(app, now, true);
    assert_eq!(status, 1);
    assert!(image.is_null());
    wait_gpu(app);
    let (status, image) = poll(app, now + 1, true);
    assert_eq!(status, 0);
    assert!(!image.is_null());
    image
}

#[test]
fn navigator_preview_is_bounded_owned_idle_and_keeps_the_throttled_final_stroke() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        assert_eq!(poll(&app, 0, false), (0, std::ptr::null_mut()));
        let first_key = key(&app);
        unsafe { capy_preview_image_free(ready(&app, 1_000_000_000)) };
        // Camera-only changes never invalidate document pixels or allocate a new image.
        for command in [
            "zoom_in",
            "rotate_right",
            "flip_horizontal",
            "flip_vertical",
            "zoom_out",
            "rotate_left",
        ] {
            app.invoke(command);
            app.draw_frame();
            assert_eq!(key(&app), first_key);
            assert_eq!(poll(&app, 1_010_000_000, true), (0, std::ptr::null_mut()));
        }
        app.stroke();
        app.draw_frame();
        let final_key = key(&app);
        assert_ne!(first_key.revision, final_key.revision);
        let pixels = app.pixels();
        // No more canvas frames will occur. The C ABI must keep the owner awake
        // through the shared 15Hz throttle until this final composition arrives.
        assert_eq!(poll(&app, 1_020_000_000, true), (1, std::ptr::null_mut()));
        let image = ready(&app, 1_066_666_667);
        assert_eq!(poll(&app, 1_100_000_000, true), (0, std::ptr::null_mut()));
        assert_eq!(app.pixels(), pixels);
        drop(app);
        let pointer = image as usize;
        std::thread::spawn(move || unsafe {
            let image = pointer as *mut CapyPreviewImage;
            let mut info = std::mem::MaybeUninit::uninit();
            capy_preview_image_read(image, info.as_mut_ptr());
            let info = info.assume_init();
            assert_eq!(info.key, final_key);
            assert_eq!(
                (info.width, info.height, info.stride, info.count),
                (256, 192, 1024, 196608)
            );
            let bytes = std::slice::from_raw_parts(info.pixels, info.count);
            assert!(bytes.chunks_exact(4).any(|p| p[0] < 240));
            assert!(bytes.chunks_exact(4).all(|p| p[3] == 255));
            capy_preview_image_free(image);
        })
        .join()
        .unwrap();
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
