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
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .startup
        .brush_ready
    {
        assert!(Instant::now() < deadline, "brush startup timed out");
        pump(20);
    }
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
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .configured_brush()
                .contact
                .is_some(),
            preset != layer_core::DefaultBrushPreset::Spray
        );
        w.dispatch(UiAction::SetBrushSize { value: 54. });
        let deadline = Instant::now() + Duration::from_secs(20);
        while !w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .backend()
            .startup
            .brush_ready
        {
            assert!(
                Instant::now() < deadline,
                "{preset:?}: brush startup timed out"
            );
            pump(20);
        }
        pump(100);
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
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
            w.input.send(
                &w,
                PenEvent {
                    device_id: 94,
                    sequence: now,
                    timestamp_ns: now,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * x + m[2] * y + m[4],
                        y: m[1] * x + m[3] * y + m[5],
                    },
                    pressure: (t * std::f32::consts::PI).sin().max(0.).powf(0.65),
                    tilt_radians: if row < 4 { [0., 0.8] } else { [0.; 2] },
                    twist_radians: 0.,
                    distance: 0.,
                    phase: if i == 0 {
                        PenPhase::Down
                    } else if i == 64 {
                        PenPhase::Up
                    } else {
                        PenPhase::Move
                    },
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                },
            );
            pump(5);
        }
        pump(180);
        capture_reference(&w, &format!("{output}/{id:02}-{preset:?}.png"), 1.);
        assert!(!w.status.is_visible(), "{preset:?}: {}", w.status.text());
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
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
    assert!(w.gpu.borrow().as_ref().unwrap().session.engine().can_redo());
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(150);
    assert!(!w.gpu.borrow().as_ref().unwrap().session.engine().can_redo());
    capture_reference(&w, &format!("{output}/all-contact-brushes.png"), 1.);
    w.window.close();
    pump(50);
}
