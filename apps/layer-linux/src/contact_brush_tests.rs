//! Native catalog activation, stylus input, presentation and history for all
//! shared contact presets. Runs on an isolated Wayland/Vulkan desktop.
use super::*;

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_contact_brushes() {
    let app = native_test_app("art.capycanvas.ContactBrushes");
    let w = fixture_workspace(&app);
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
        theme: Some(Theme::Light),
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
    for (row, preset) in layer_core::CONTACT_BRUSH_PRESETS
        .into_iter()
        .chain([layer_core::DefaultBrushPreset::Spray])
        .enumerate()
    {
        let id = preset as u32;
        let category = layer_ui::brush_catalog()
            .find(|b| b.id == id)
            .unwrap()
            .category;
        let group = layer_ui::ToolGroup::ALL
            .into_iter()
            .find(|group| group.label() == category)
            .unwrap();
        w.dispatch(UiAction::Invoke {
            command: group.tool().command(),
        });
        w.dispatch(UiAction::SelectToolGroup { group });
        pump(30);
        let button = w
            .tool_set
            .buttons
            .borrow()
            .iter()
            .find(|(item, _, _)| item.preview == Some(id))
            .expect("preset must be present in its native tool group")
            .1
            .clone();
        click(&button);
        pump(40);
        if state(&w).customization.drawer.is_some() {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::CloseExpanded,
            });
            pump(250);
        }
        assert_eq!(state(&w).brush.preset, id);
        assert!(button.has_css_class("selected-tool"));
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
        let camera = state(&w).camera;
        for i in 0..=64 {
            let t = i as f32 / 64.;
            let column = if matches!(
                preset,
                layer_core::DefaultBrushPreset::Eraser | layer_core::DefaultBrushPreset::Spray
            ) {
                0
            } else {
                row / 11
            };
            let x = 200. + column as f32 * 900. + 700. * t;
            let y = if preset == layer_core::DefaultBrushPreset::Spray {
                1450.
            } else {
                210. + (row % 11) as f32 * 110.
            } + (t * std::f32::consts::TAU).sin() * 16.;
            let now = glib::monotonic_time() as u64 * 1000;
            let phase = match i {
                0 => PenPhase::Down,
                64 => PenPhase::Up,
                _ => PenPhase::Move,
            };
            w.input.send(
                &w,
                PenEvent {
                    device_id: 94,
                    timestamp_ns: now,
                    pressure: (t * std::f32::consts::PI).sin().max(0.).powf(0.65),
                    tilt_radians: if row < 4 { [0., 0.8] } else { [0.; 2] },
                    ..pen_event(&camera, [x, y], phase, now)
                },
            );
            pump(5);
        }
        pump(180);
        capture_reference(&w, &format!("{output}/{id:02}-{preset:?}.png"), 1.);
        assert!(!w.status.is_visible(), "{preset:?}: {}", w.status.text());
        assert_eq!(
            ui_session(&w)
                .engine()
                .metrics()
                .committed_strokes,
            row as u64 + 1,
            "{preset:?}: GTK must commit exactly one stroke"
        );
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(150);
    assert!(ui_session(&w).engine().can_redo());
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(150);
    assert!(!ui_session(&w).engine().can_redo());
    capture_reference(&w, &format!("{output}/all-contact-brushes.png"), 1.);
    w.window.close();
    pump(50);
}
