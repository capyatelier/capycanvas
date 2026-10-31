//! Native catalog activation, host pen events, presentation and history for all
//! shared contact presets. Runs on an isolated Wayland/Vulkan desktop.
use super::*;
use layer_render::CanvasRenderer;

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_contact_brushes() {
    let app = native_test_app("art.capycanvas.ContactBrushes");
    let project = new_drawing(2048, 1536, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::default() });
    w.window.present();
    pump(1600);
    until(
        || {
            ui_session(&w)
                .engine()
                .backend()
                .startup
                .brush_ready
        },
        "brush startup timed out",
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|v| v == "dark") { Theme::Dark } else { Theme::Light }),
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.006, 0.006, 0.006, 1.],
    });
    let output = std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../artifacts/contact-brushes/gtk"
        )
        .into()
    });
    std::fs::create_dir_all(&output).unwrap();
    let mut committed = 0;
    for (row, preset) in layer_core::CONTACT_BRUSH_PRESETS
        .into_iter()
        .chain([layer_core::DefaultBrushPreset::Spray])
        .enumerate()
    {
        let id = preset as u32;
        w.dispatch(UiAction::SelectBrush { id });
        pump(40);
        if state(&w).customization.drawer.is_some() {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::CloseExpanded,
            });
            pump(250);
        }
        assert_eq!(state(&w).brush.preset, id);
        assert_eq!(
            ui_session(&w)
                .engine()
                .configured_brush()
                .contact
                .is_some(),
            preset != layer_core::DefaultBrushPreset::Spray
        );
        w.dispatch(UiAction::SetBrushSize { value: 54. });
        let prepare_started = Instant::now();
        let deadline = prepare_started + Duration::from_secs(20);
        while !w.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            engine.backend().paint_ready(engine.document(), engine.brush(), false)
        }) {
            assert!(Instant::now() < deadline, "{preset:?}: brush preparation timed out");
            pump(20);
        }
        eprintln!("brush_ready preset={preset:?} elapsed_ms={:.3}", prepare_started.elapsed().as_secs_f64() * 1000.);
        pump(100);
        let dry = ui_session(&w).engine().configured_brush().contact
            .as_ref().is_some_and(|contact| contact.dry.is_some());
        let mut poses = vec![("0", [0., 0.])];
        if dry { poses.extend([("40", [0., 40.]), ("60", [0., 60.])]); }
        if matches!(preset, layer_core::DefaultBrushPreset::Pencil | layer_core::DefaultBrushPreset::Charcoal) {
            poses.push(("60-diagonal", [50.76848, 50.76848]));
        }
        for (tilt, angles) in poses {
            let camera = state(&w).camera;
            let column = if tilt == "60-diagonal" { 1 } else if matches!(preset, layer_core::DefaultBrushPreset::Eraser | layer_core::DefaultBrushPreset::Spray) { 0 } else { row / 13 };
            let base_y = if preset == layer_core::DefaultBrushPreset::Spray { 1450. } else { 150. + (row % 13) as f32 * 100. };
            let region = if tilt == "60-diagonal" { [390 + column as u32 * 900, (base_y - 120.) as u32, 320, 240] }
                else { [180 + column as u32 * 900, (base_y - 65.) as u32, 740, 130] };
            let before = contact_pixels(&w, region);
            if tilt == "60-diagonal" {
                w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
                    id: PreferenceId::Cursor, value: PreferenceValue::Choice(7),
                } });
                let now = glib::monotonic_time() as u64 * 1000;
                w.cursor_input(Some(PenEvent { pressure: 0., tilt_radians: angles.map(f32::to_radians),
                    ..pen_event(&camera, [550. + column as f32 * 900., base_y], PenPhase::Hover, now) }));
                pump(100);
                let cursor = ui_session_mut(&w).canvas_cursor().unwrap();
                let [mut xx, mut xy, mut yy] = [0.; 3];
                for segment in cursor.segments.iter().filter(|s| s.marker <= 1.) {
                    let [x, y] = [segment.from[0] - cursor.center[0], segment.from[1] - cursor.center[1]];
                    xx += x * x; xy += x * y; yy += y * y;
                }
                assert!(xy / (xx + yy) > 0.2 && (xx - yy).abs() / (xx + yy) < 0.05,
                    "{preset:?}: the 60-degree diagonal cursor follows the physical pen: {xx}/{xy}/{yy}");
                capture_reference(&w, &format!("{output}/{id:02}-{preset:?}-tilt{tilt}-cursor.png"), 1.);
                w.cursor_input(None);
            }
            for i in 0..=64 {
                let t = i as f32 / 64.;
                let x = if tilt == "60-diagonal" { 550. + column as f32 * 900. } else { 200. + column as f32 * 900. + 700. * t };
                let y = if tilt == "60-diagonal" { base_y } else { base_y + (t * std::f32::consts::TAU).sin() * 16. };
                let now = glib::monotonic_time() as u64 * 1000;
                let phase = match i { 0 => PenPhase::Down, 64 => PenPhase::Up, _ => PenPhase::Move };
                w.input.send(&w, PenEvent {
                    device_id: 94, timestamp_ns: now,
                    pressure: 0.15 + 0.85 * (t * std::f32::consts::PI).sin().max(0.).powf(0.65),
                    tilt_radians: angles.map(f32::to_radians),
                    ..pen_event(&camera, [x, y], phase, now)
                });
                pump(5);
            }
            pump(180);
            committed += 1;
            let painted = contact_pixels(&w, region);
            assert_ne!(before, painted, "{preset:?} tilt{tilt}: visible stroke");
            if tilt == "60-diagonal" {
                let [mut xx, mut xy, mut yy] = [0.; 3];
                for (index, (before, after)) in before.iter().zip(&painted).enumerate() {
                    let weight = before.iter().zip(after).take(3).map(|(a, b)| (a - b).abs()).sum::<f32>();
                    let x = (index as u32 % region[2]) as f32 - 160.;
                    let y = (index as u32 / region[2]) as f32 - 120.;
                    xx += weight * x * x; xy += weight * x * y; yy += weight * y * y;
                }
                assert!(xy / (xx + yy) > 0.1 && (xx - yy).abs() / (xx + yy) < 0.2,
                    "{preset:?}: diagonal pigment follows the cursor: {xx}/{xy}/{yy}");
            }
            assert!(!w.status.is_visible(), "{preset:?}: {}", w.status.text());
            assert_eq!(ui_session(&w).engine().metrics().committed_strokes, committed,
                "{preset:?}: GTK must commit exactly one stroke");
            capture_reference(&w, &format!("{output}/{id:02}-{preset:?}-tilt{tilt}.png"), 1.);
            w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            pump(150);
            assert!(ui_session(&w).engine().can_redo());
            assert_eq!(contact_pixels(&w, region), before, "{preset:?} tilt{tilt}: exact Undo pixels");
            w.dispatch(UiAction::Invoke { command: CommandId::Redo });
            pump(150);
            assert!(!ui_session(&w).engine().can_redo());
            assert_eq!(contact_pixels(&w, region), painted, "{preset:?} tilt{tilt}: exact Redo pixels");
            eprintln!("contact preset={preset:?} tilt={tilt}: visible stroke, exact Undo/Redo passed");
        }
    }
    capture_reference(&w, &format!("{output}/all-contact-brushes.png"), 1.);
    w.window.close();
    pump(50);
}

fn contact_pixels(w: &Workspace, region: [u32; 4]) -> Vec<[f32; 4]> {
    until(|| {
        let gpu = w.gpu.borrow();
        let engine = gpu.as_ref().unwrap().session.engine();
        !engine.has_pending_document_edits() && engine.document().scene().targets()
            .filter_map(|target| engine.document().target_raster(target))
            .all(|raster| raster.try_data().is_some_and(|data| data.is_ok_and(|data| data.host_backed())))
    }, "contact stroke capture timed out");
    let gpu = w.snapshot_gpu().unwrap();
    let project = w.gpu.borrow().as_ref().unwrap().session.capture_artwork().unwrap();
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        let mut renderer = gpu.capture(project, Default::default()).unwrap();
        renderer.read_region(region).unwrap()
    })).unwrap()
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_dry_brush_preparation() {
    let app = native_test_app("art.capycanvas.DryBrushPreparation");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1600);
    until(|| ui_session(&w).engine().backend().startup.complete, "startup timed out");
    pump(1500);
    let measure = |label: &str, change: &dyn Fn()| {
        let context = glib::MainContext::default();
        let started = Instant::now();
        change();
        let dispatch = started.elapsed();
        let mut worst = Duration::ZERO;
        while !w.gpu.borrow().as_ref().is_some_and(|g| { let e = g.session.engine(); e.backend().paint_ready(e.document(), e.brush(), false) }) {
            assert!(started.elapsed() < Duration::from_secs(600), "{label} preparation timed out");
            while context.pending() {
                let t = Instant::now();
                context.iteration(false);
                worst = worst.max(t.elapsed());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        eprintln!("PROBE_GTK {label} ready_ms={:.1} dispatch_ms={:.1} worst_main_iteration_ms={:.1}",
            started.elapsed().as_secs_f64() * 1000., dispatch.as_secs_f64() * 1000., worst.as_secs_f64() * 1000.);
    };
    use layer_core::DefaultBrushPreset as D;
    for (label, preset) in [("Pencil", D::Pencil), ("Pointy Pencil", D::PointyPencil), ("Chalk", D::Chalk), ("Pencil again", D::Pencil)] {
        let id = preset as u32;
        let group = layer_ui::brush_catalog().find(|b| b.id == id).unwrap().group;
        w.dispatch(UiAction::Invoke { command: group.tool().command() });
        w.dispatch(UiAction::SelectToolGroup { group });
        pump(30);
        let button = w.tool_set.buttons.borrow().iter().find(|(item, _, _)| item.preview == Some(id)).unwrap().1.clone();
        measure(label, &|| click(&button));
        pump(200);
    }
    measure("Pencil 36 px", &|| w.dispatch(UiAction::SetBrushSize { value: 36. }));
    pump(200);
    let camera = state(&w).camera;
    for i in 0..=48 {
        let t = i as f32 / 48.;
        let now = glib::monotonic_time() as u64 * 1000;
        let phase = match i { 0 => PenPhase::Down, 48 => PenPhase::Up, _ => PenPhase::Move };
        w.input.send(&w, PenEvent { device_id: 97, timestamp_ns: now, pressure: 0.2 + 0.8 * (t * std::f32::consts::PI).sin(),
            ..pen_event(&camera, [300. + 700. * t, 400. + (t * std::f32::consts::TAU).sin() * 16.], phase, now) });
        pump(6);
    }
    pump(400);
    assert_eq!(ui_session(&w).engine().metrics().committed_strokes, 1, "the stroke after preparation is committed");
    w.window.close();
    pump(50);
}
