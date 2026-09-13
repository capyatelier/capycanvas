//! Compact color controls with real input on the private Mutter display.
use super::*;

#[test]
#[ignore = "isolated Mutter input driver: --color-panel"]
fn native_color_panel_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.ColorPanel");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1400);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        assert!(Instant::now() < deadline, "workspace startup");
        pump(20);
    }
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut fixture = layer_ui::WorkspaceState::default();
    fixture
        .layout
        .set_panel_visible(Panel::Color, true)
        .unwrap();
    fixture
        .layout
        .move_panel(
            viewport,
            Panel::Color,
            DockTarget::Float {
                position: [480., 120.],
            },
        )
        .unwrap();
    let mut reports = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        for width in [200., 280., 360.] {
            for float in &mut fixture.layout.floating {
                if let DockNode::Tabs { panels, .. } = &float.root {
                    if panels.contains(&Panel::Color) {
                        float.width = width;
                        float.height = Some(width + 70.);
                    }
                }
            }
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: fixture.clone(),
            });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            w.dispatch(UiAction::SetColor {
                rgba: [0.2, 0.72, 0.58, 1.],
            });
            for space in [layer_ui::ColorSpace::Hsv, layer_ui::ColorSpace::Hls] {
                w.dispatch(UiAction::Color {
                    action: layer_ui::ColorAction::Space { space },
                });
                pump(150);
                let root = &w.color_panel.root;
                let wheel = find_named(root.upcast_ref(), "color-wheel").unwrap();
                let field = find_named(root.upcast_ref(), "color-component-2").unwrap();
                let wb = wheel.compute_bounds(root).unwrap();
                let fb = field.compute_bounds(root).unwrap();
                let panel = w.panel_widget(Panel::Color);
                let visible = root.compute_bounds(&panel).unwrap();
                assert!(
                    visible.x() >= 0. && visible.x() + visible.width() <= panel.width() as f32,
                    "Color controls fit panel width"
                );
                assert!(
                    visible.y() + fb.y() + fb.height() <= panel.height() as f32,
                    "Values remain visible without scrolling"
                );
                assert!(
                    fb.y() + fb.height() <= wb.height() + 36.,
                    "one compact numeric row"
                );
                assert!(
                    (wheel.width() - wheel.height()).abs() <= 1,
                    "square wheel {} x {}",
                    wheel.width(),
                    wheel.height()
                );
                for name in [
                    "color-Foreground",
                    "color-Background",
                    "color-Transparent",
                    "color-space",
                    "color-swap",
                ] {
                    let button = find_named(root.upcast_ref(), name).unwrap();
                    let b = button.compute_bounds(root).unwrap();
                    assert!(
                        b.width() >= 24. && b.height() >= 24.,
                        "{name} usable target"
                    );
                    assert!(
                        b.x() >= 0. && b.x() + b.width() <= root.width() as f32 + 1.,
                        "{name} fits"
                    );
                    let hit = root
                        .pick(
                            b.x() as f64 + b.width() as f64 / 2.,
                            b.y() as f64 + b.height() as f64 / 2.,
                            gtk::PickFlags::DEFAULT,
                        )
                        .unwrap();
                    assert!(
                        hit == button || hit.is_ancestor(&button),
                        "{name} unobscured"
                    );
                }
                let name = format!("{theme:?}-{width}-{space:?}");
                let b = root.compute_bounds(&w.window).unwrap();
                reports.push(serde_json::json!({"name":name,"panel":[b.x(),b.y(),b.width(),fb.y()+fb.height()],"wheel":[wb.x(),wb.y(),wb.width(),wb.height()]}));
                capture_reference(&w, output.join(format!("{name}.png")).to_str().unwrap(), 1.);
                if width == 280. {
                    capture_reference(
                        &w,
                        output.join(format!("{name}-2x.png")).to_str().unwrap(),
                        2.,
                    );
                }
            }
        }
    }
    let root = &w.color_panel.root;
    let locate = |name: &str, x: f32, y: f32| {
        let widget = find_named(root.upcast_ref(), name).unwrap();
        let b = widget.compute_bounds(&w.window).unwrap();
        [b.x() + b.width() * x, b.y() + b.height() * y]
    };
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json.tmp")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        std::fs::rename(
            dir.join(format!("step-{step}.json.tmp")),
            dir.join(format!("step-{step}.json")),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native color input");
            pump(2);
        }
        step += 1;
        pump(100);
    };
    for touch in [false, true] {
        let mut gesture = |from: [f32; 2], to: [f32; 2]| {
            perform(if touch {
                serde_json::json!([
                    {"touch":"down","point":from}, {"touch":"move","point":to}, {"touch":"up"}
                ])
            } else {
                serde_json::json!([
                    {"point":from,"down":true}, {"point":to}, {"down":false}
                ])
            });
        };
        w.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::Select {
                slot: layer_ui::ColorSlot::Foreground,
            },
        });
        w.dispatch(UiAction::SetColor {
            rgba: [0.2, 0.72, 0.58, 1.],
        });
        pump(100);
        let before = state(&w).colors.foreground;
        gesture(
            locate("color-wheel", 0.94, 0.5),
            locate("color-wheel", 0.5, 0.94),
        );
        assert_ne!(
            state(&w).colors.foreground,
            before,
            "touch={touch}: hue drag"
        );
        for space in [layer_ui::ColorSpace::Hsv, layer_ui::ColorSpace::Hls] {
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Space { space },
            });
            let before = state(&w).colors.foreground;
            gesture(
                locate("color-wheel", 0.5, 0.5),
                locate("color-wheel", 0.58, 0.42),
            );
            assert_ne!(
                state(&w).colors.foreground,
                before,
                "touch={touch}: {space:?} drag"
            );
        }
        for (name, slot) in [
            ("color-Background", layer_ui::ColorSlot::Background),
            ("color-Transparent", layer_ui::ColorSlot::Transparent),
        ] {
            let p = locate(name, 0.5, 0.5);
            gesture(p, p);
            assert_eq!(state(&w).colors.slot, slot, "touch={touch}: {name}");
        }
        let p = locate("color-space", 0.5, 0.5);
        gesture(p, p);
        assert_eq!(state(&w).colors.space, layer_ui::ColorSpace::Hsv);
        let before = state(&w).colors;
        let p = locate("color-swap", 0.5, 0.5);
        gesture(p, p);
        assert_eq!(state(&w).colors.foreground, before.background);
        assert_eq!(state(&w).colors.background, before.foreground);
    }
    let hue = find_named(root.upcast_ref(), "color-component-0")
        .unwrap()
        .downcast::<crate::number_control::NumberControl>()
        .unwrap();
    edit_number(&hue, "180/2");
    assert!((state(&w).colors.components()[0] - 90.).abs() < 0.01);
    assert_eq!(state(&w).colors.slot, layer_ui::ColorSlot::Background);
    std::fs::write(
        output.join("geometry.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}
