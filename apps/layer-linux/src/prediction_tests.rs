use super::*;
use layer_ui::{PreferenceAction, PreferenceId, SettingsPage, UiAction};

#[test]
#[ignore = "native GTK settings; requires isolated settings/workspaces and Wayland/Vulkan"]
fn native_prediction_settings() {
    let app = native_test_app("art.capycanvas.PredictionSettingsTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    let w = windows.borrow()[0].clone();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !w
        .gpu
        .borrow()
        .as_ref()
        .is_some_and(|g| g.session.engine().backend().startup.complete)
    {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Input,
    });
    pump(200);
    let choice = find_named(w.window.upcast_ref(), "setting-prediction-algorithm")
        .unwrap().downcast::<adw::ComboRow>().unwrap();
    assert_eq!(choice.selected(), 0);
    choice.set_selected(1);
    pump(100);
    assert_eq!(state(&w).settings.prediction_algorithm, layer_engine::PredictionAlgorithm::Previous);
    let choice = find_named(w.window.upcast_ref(), "setting-prediction-algorithm")
        .unwrap().downcast::<adw::ComboRow>().unwrap();
    choice.set_selected(0);
    pump(100);
    assert_eq!(state(&w).settings.prediction_algorithm, layer_engine::PredictionAlgorithm::Optimized);
    assert!(find_named(w.window.upcast_ref(), "setting-prediction-horizon").is_some());
    w.window.destroy();
    pump(100);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
#[ignore = "4K GTK/Vulkan integration; use an isolated 3840x2160 Wayland compositor"]
fn native_fullscreen_prediction() {
    let app = native_test_app("art.capycanvas.FullscreenPredictionTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    let w = windows.borrow()[0].clone();
    w.window.fullscreen();
    pump(400);
    let surface = w.window.surface().unwrap();
    assert!(surface.width() * surface.scale_factor() >= 3840);
    assert!(surface.height() * surface.scale_factor() >= 2160);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::PredictionHorizon,
            value: layer_ui::PreferenceValue::Number(32.),
        },
    });
    // The first fit-to-window camera is not valid until startup has painted.
    // Converting a physical brush size before then can exceed the size limit.
    let ready_deadline = Instant::now() + Duration::from_secs(30);
    while !w.gpu.borrow().as_ref().is_some_and(|g| {
        g.session.engine().backend().startup.complete
            && g.session.state().camera.viewport[0] >= 3000
            && g.session.state().camera.viewport[1] >= 1800
    }) {
        assert!(
            Instant::now() < ready_deadline,
            "full-screen startup did not finish"
        );
        pump(5);
    }
    let camera = state(&w).camera;
    let m = camera.document_to_surface();
    w.dispatch(UiAction::SetBrushSize {
        value: 128. / m[0].hypot(m[1]),
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w.gpu.borrow().as_ref().is_some_and(|g| {
        let e = g.session.engine();
        e.backend().paint_ready(e.document(), e.brush(), false)
    }) {
        assert!(
            Instant::now() < deadline,
            "brush readiness: {}",
            w.status.text()
        );
        pump(5);
    }
    let camera = state(&w).camera;
    let m = camera.document_to_surface();
    let diameter_px = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .brush()
        .diameter
        * m[0].hypot(m[1]);
    assert!(
        (diameter_px - 128.).abs() < 0.01,
        "physical brush diameter: {diameter_px}"
    );
    assert!(
        camera.viewport[0] >= 3000 && camera.viewport[1] >= 1800,
        "physical canvas dimensions: {:?}",
        camera.viewport
    );
    let before = w.gpu.borrow().as_ref().unwrap().session.engine().metrics();
    let start = Instant::now();
    let due = Rc::new(Cell::new(false));
    let timer = glib::timeout_add_local(
        Duration::from_millis(5),
        glib::clone!(
            #[strong]
            due,
            move || {
                due.set(true);
                glib::ControlFlow::Continue
            }
        ),
    );
    let context = glib::MainContext::default();
    let mut last = None;
    let mut samples = 0;
    while start.elapsed() < Duration::from_millis(800) {
        let t = start.elapsed().as_secs_f32();
        let event = PenEvent {
            device_id: 95,
            sequence: samples,
            timestamp_ns: (glib::monotonic_time() as u64 / 1000) * 1_000_000,
            view_revision: camera.revision,
            surface_position: Point {
                x: camera.viewport[0] as f32 * 0.5 + 850. * (8. * t).cos(),
                y: camera.viewport[1] as f32 * 0.5 + 850. * (8. * t).sin(),
            },
            pressure: 0.6 + 0.2 * (60. * t).sin(),
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.,
            phase: if samples == 0 {
                PenPhase::Down
            } else {
                PenPhase::Move
            },
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        w.input.send(&w, event);
        last = Some(event);
        samples += 1;
        while !due.replace(false) {
            context.iteration(true);
        }
    }
    timer.remove();
    let after = w.gpu.borrow().as_ref().unwrap().session.engine().metrics();
    assert!(after.engine_prediction_frames > before.engine_prediction_frames + 10);
    assert!(after.last_tip_gap_surface_px < 0.1);
    eprintln!(
        "native Smooth Motion viewport={:?} samples={samples} predicted_frames={} gap={}px",
        camera.viewport,
        after.engine_prediction_frames - before.engine_prediction_frames,
        after.last_tip_gap_surface_px
    );
    let last = last.unwrap();
    w.input.send(
        &w,
        PenEvent {
            phase: PenPhase::Up,
            sequence: samples,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            ..last
        },
    );
    pump(150);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .metrics()
            .committed_strokes,
        before.committed_strokes + 1
    );
    assert!(!w.status.is_visible(), "{}", w.status.text());

    w.window.destroy();
    pump(100);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}
