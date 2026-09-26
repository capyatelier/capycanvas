//! Compact color controls with real input on the private Mutter display.
use super::*;

pub(super) fn hue_guide(w: &Workspace) -> gtk::gdk::Texture {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    let wheel = find_named(w.color_panel.root.upcast_ref(), "color-wheel")
        .unwrap()
        .downcast::<crate::tool_panels::ColorWheel>()
        .unwrap();
    let cache = wheel.imp().ring.borrow();
    let (side, shape, space, view, texture) = cache.as_ref().expect("visible wheel caches its hue guide");
    assert_eq!(*shape, state(w).colors.shape);
    assert_eq!(*space, state(w).colors.rgb_space());
    assert_eq!(*view, wheel.imp().view.get());
    assert_eq!(
        *side,
        (wheel.drawing_bounds().0 * wheel.scale_factor() as f32).ceil() as u32
    );
    assert_eq!(texture.width(), *side as i32);
    texture.clone()
}

fn assert_hue_guide_colors(w: &Workspace, texture: &gtk::gdk::Texture) {
    // Compare the texture against the former native gradient at the same DPI.
    // Only the ring is displayed; the gradient's center is not part of the UI.
    let colors = state(w).colors;
    let side = texture.width() as usize;
    let geometry = layer_ui::ColorWheelGeometry::new(side as f32).unwrap();
    let bounds = gtk::graphene::Rect::new(0., 0., side as f32, side as f32);
    let stops: Vec<_> = colors
        .wheel_hue_stops()
        .iter()
        .map(|stop| {
            let [r, g, b] = stop.color;
            gtk::gsk::ColorStop::new(stop.offset, gdk::RGBA::new(r, g, b, 1.))
        })
        .collect();
    let snapshot = gtk::Snapshot::new();
    snapshot.append_conic_gradient(
        &bounds,
        &gtk::graphene::Point::new(geometry.center[0], geometry.center[1]),
        colors.wheel_hue_start_degrees() + 90.,
        &stops,
    );
    let reference = w
        .window
        .renderer()
        .unwrap()
        .render_texture(&snapshot.to_node().unwrap(), Some(&bounds));
    let mut expected = vec![0; side * side * 4];
    let mut actual = vec![0; expected.len()];
    reference.download(&mut expected, side * 4);
    texture.download(&mut actual, side * 4);
    let mut maximum = 0;
    let mut worst = (0, 0, 0, 0, 0);
    let mut samples = 0;
    for y in 0..side {
        for x in 0..side {
            let radius = (x as f32 + 0.5 - geometry.center[0])
                .hypot(y as f32 + 0.5 - geometry.center[1]);
            if radius < geometry.inner + 2. || radius > geometry.outer - 2. {
                continue;
            }
            let offset = (y * side + x) * 4;
            for c in 0..4 {
                let difference = actual[offset + c].abs_diff(expected[offset + c]);
                if difference > maximum {
                    maximum = difference;
                    worst = (x, y, c, actual[offset + c], expected[offset + c]);
                }
            }
            samples += 1;
        }
    }
    assert!(samples > 100);
    assert!(
        maximum <= 2,
        "{:?} hue guide differs from native gradient by {maximum}/255 at {worst:?}, side {side}",
        colors.shape
    );
}

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
    let mut previous_guide = None;
    for theme in [Theme::Dark, Theme::Light] {
        for width in [144., 160., 200., 280., 360.] {
            for float in &mut fixture.layout.floating {
                if let DockNode::Tabs { panels, .. } = &float.root {
                    if panels.contains(&Panel::Color) {
                        float.width = width;
                        float.height = Some(width + 36.);
                    }
                }
            }
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(fixture.clone()),
            });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            w.dispatch(UiAction::SetColor {
                rgba: [0.2, 0.72, 0.58, 1.],
            });
            for shape in [
                layer_ui::ColorShape::Circle,
                layer_ui::ColorShape::Square,
                layer_ui::ColorShape::Triangle,
            ] {
                w.dispatch(UiAction::Color {
                    action: layer_ui::ColorAction::Shape { shape },
                });
                pump(150);
                let guide = hue_guide(&w);
                if width == 280. {
                    assert_hue_guide_colors(&w, &guide);
                }
                if let Some(previous) = previous_guide.replace(guide.clone()) {
                    assert_ne!(guide, previous, "shape/size changes replace the hue guide");
                }
                let root = &w.color_panel.root;
                let wheel = find_named(root.upcast_ref(), "color-wheel").unwrap();
                let wb = wheel.compute_bounds(root).unwrap();
                let panel = w.panel_widget(Panel::Color);
                let visible = root.compute_bounds(&panel).unwrap();
                assert!(
                    visible.x() >= 0. && visible.x() + visible.width() <= panel.width() as f32,
                    "Color controls fit {width}px panel width"
                );
                assert!(
                    visible.y() + wb.y() + wb.height() <= panel.height() as f32,
                    "Entire square remains visible"
                );
                assert!(
                    wheel.height() >= wheel.width(),
                    "wheel and footer {} x {}",
                    wheel.width(),
                    wheel.height()
                );
                for name in [
                    "color-Foreground",
                    "color-Background",
                    "color-Transparent",
                    "color-Black",
                    "color-White",
                    "color-shape-0",
                    "color-shape-1",
                    "color-swap",
                ] {
                    let button = find_named(root.upcast_ref(), name).unwrap();
                    let b = button.compute_bounds(root).unwrap();
                    assert!(
                        b.width() >= 20. && b.height() >= 20.,
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
                    if name.starts_with("color-shape-") || name == "color-swap" {
                        let image = button
                            .downcast_ref::<gtk::Button>()
                            .unwrap()
                            .child()
                            .and_downcast::<gtk::Image>()
                            .unwrap();
                        assert!(image.paintable().unwrap().is::<gtk::Svg>(),
                            "{name} uses the shared vector renderer");
                    }
                }
                let fg = find_named(root.upcast_ref(), "color-Foreground")
                    .unwrap()
                    .compute_bounds(root)
                    .unwrap();
                let bg = find_named(root.upcast_ref(), "color-Background")
                    .unwrap()
                    .compute_bounds(root)
                    .unwrap();
                let transparent = find_named(root.upcast_ref(), "color-Transparent")
                    .unwrap()
                    .compute_bounds(root)
                    .unwrap();
                assert!(fg.width() > bg.width() && fg.x() < bg.x() && fg.y() < bg.y());
                assert!(
                    fg.x() + fg.width() > bg.x() && fg.y() + fg.height() > bg.y(),
                    "intentional paint overlap"
                );
                assert_eq!(bg.width(), transparent.width());
                let wheel = wheel.downcast::<crate::tool_panels::ColorWheel>().unwrap();
                let (size, origin) = wheel.drawing_bounds();
                let g = layer_ui::ColorWheelGeometry::new(size).unwrap();
                for hue in (0..360).step_by(5) {
                    let [x, y] = g.hue_marker(hue as f32);
                    let hit = wheel
                        .pick(
                            (x + origin[0]) as f64,
                            (y + origin[1]) as f64,
                            gtk::PickFlags::DEFAULT,
                        )
                        .unwrap();
                    assert_eq!(
                        hit,
                        wheel.clone().upcast::<gtk::Widget>(),
                        "{width}px hue {hue} unobscured"
                    );
                }
                assert_eq!(state(&w).colors.readout, layer_ui::ColorReadout::Shape);
                for model in [layer_ui::ColorReadout::Shape, layer_ui::ColorReadout::Rgb] {
                    while state(&w).colors.readout != model {
                        w.dispatch(UiAction::Color {
                            action: layer_ui::ColorAction::ToggleReadout,
                        });
                    }
                    pump(50);
                    assert_eq!(hue_guide(&w), guide, "readout refresh retains the hue guide");
                    let name = format!("{theme:?}-{width}-{shape:?}-{model:?}");
                    let b = root.compute_bounds(&w.window).unwrap();
                    reports.push(serde_json::json!({"name":name,"panel":[b.x(),b.y(),b.width(),wb.y()+wb.height()],"wheel":[wb.x()+origin[0],wb.y()+origin[1],size,size],"shape":shape,"readout":model}));
                    capture_reference(&w, output.join(format!("{name}.png")).to_str().unwrap(), 1.);
                    if width == 280. || width == 144. {
                        capture_reference(
                            &w,
                            output.join(format!("{name}-2x.png")).to_str().unwrap(),
                            2.,
                        );
                    }
                }
            }
        }
    }
    std::fs::write(
        output.join("geometry.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    for float in &mut fixture.layout.floating {
        if let DockNode::Tabs { panels, .. } = &float.root {
            if panels.contains(&Panel::Color) {
                float.width = 100.;
                float.height = Some(180.);
            }
        }
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(fixture),
    });
    pump(150);
    let root = &w.color_panel.root;
    assert_eq!(
        root.width(),
        128,
        "Resizing below four tiles preserves the 144px panel minimum"
    );
    let locate = |name: &str, x: f32, y: f32| {
        let widget = find_named(root.upcast_ref(), name).unwrap();
        if let Some(popup) = widget.native().and_downcast::<gtk::Popover>() {
            assert!(popup.is_visible(), "{name} menu is open");
            let b = widget.compute_bounds(&popup).unwrap();
            let surface = popup
                .surface()
                .unwrap()
                .downcast::<gtk::gdk::Popup>()
                .unwrap();
            let (dx, dy) = popup.surface_transform();
            [
                surface.position_x() as f32 - dx as f32 + b.x() + b.width() * x,
                surface.position_y() as f32 - dy as f32 + b.y() + b.height() * y,
            ]
        } else {
            let b = widget.compute_bounds(&w.window).unwrap();
            [b.x() + b.width() * x, b.y() + b.height() * y]
        }
    };
    let on_ring = |hue: f32| {
        let wheel = find_named(root.upcast_ref(), "color-wheel")
            .unwrap()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap();
        let (size, origin) = wheel.drawing_bounds();
        let b = wheel.compute_bounds(&w.window).unwrap();
        let p = layer_ui::ColorWheelGeometry::new(size)
            .unwrap()
            .hue_marker(hue);
        [b.x() + origin[0] + p[0], b.y() + origin[1] + p[1]]
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
        let guide = hue_guide(&w);
        gesture(on_ring(150.), on_ring(240.));
        assert_eq!(hue_guide(&w), guide, "hue picking retains the hue guide");
        assert_ne!(
            state(&w).colors.foreground,
            before,
            "touch={touch}: hue drag"
        );
        for shape in [
            layer_ui::ColorShape::Circle,
            layer_ui::ColorShape::Square,
            layer_ui::ColorShape::Triangle,
        ] {
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Shape { shape },
            });
            w.dispatch(UiAction::SetColor {
                rgba: [0.2, 0.72, 0.58, 1.],
            });
            pump(40);
            let before = state(&w).colors.foreground;
            let guide = hue_guide(&w);
            gesture(
                locate("color-wheel", 0.5, 0.5),
                locate("color-wheel", 0.58, 0.42),
            );
            assert_ne!(
                state(&w).colors.foreground,
                before,
                "touch={touch}: {shape:?} drag"
            );
            assert_eq!(hue_guide(&w), guide, "field picking retains the hue guide");
        }
        for (name, slot) in [
            ("color-Background", layer_ui::ColorSlot::Background),
            ("color-Transparent", layer_ui::ColorSlot::Transparent),
        ] {
            let p = locate(name, 0.5, 0.5);
            gesture(p, p);
            assert_eq!(state(&w).colors.slot, slot, "touch={touch}: {name}");
        }
        // Picking while transparent resumes the remembered background paint.
        gesture(
            locate("color-wheel", 0.5, 0.5),
            locate("color-wheel", 0.55, 0.45),
        );
        assert_eq!(state(&w).colors.slot, layer_ui::ColorSlot::Background);
        let remembered = state(&w).colors;
        let p = locate("color-Transparent", 0.5, 0.5); gesture(p, p);
        for (name, rgba) in [("color-Black", [0.,0.,0.,1.]), ("color-White", [1.;4])] {
            let p = locate(name, 0.5, 0.5); gesture(p, p);
            let colors = state(&w).colors;
            assert_eq!(colors.slot, layer_ui::ColorSlot::Temporary);
            assert_eq!(colors.definition().rgba, rgba);
            assert_eq!((colors.foreground, colors.background), (remembered.foreground, remembered.background));
        }
        let p = locate("color-Background", 0.5, 0.5); gesture(p, p);
        for expected in [
            layer_ui::ColorShape::Circle,
            layer_ui::ColorShape::Square,
            layer_ui::ColorShape::Triangle,
            layer_ui::ColorShape::Circle,
        ] {
            if state(&w).colors.readout != layer_ui::ColorReadout::Rgb {
                w.dispatch(UiAction::Color { action: layer_ui::ColorAction::ToggleReadout });
            }
            let index = state(&w)
                .colors
                .other_shapes()
                .iter()
                .position(|shape| *shape == expected)
                .unwrap();
            let p = locate(&format!("color-shape-{index}"), 0.5, 0.5);
            gesture(p, p);
            assert_eq!(state(&w).colors.shape, expected);
            assert_eq!(state(&w).colors.readout, layer_ui::ColorReadout::Shape);
            assert_eq!(state(&w).colors.readout_label(), match expected {
                layer_ui::ColorShape::Circle => "OKLCH",
                layer_ui::ColorShape::Square => "HSB",
                layer_ui::ColorShape::Triangle => "HLS",
            });
        }
        let before = state(&w).colors;
        let p = locate("color-swap", 0.5, 0.5);
        gesture(p, p);
        assert_eq!(state(&w).colors.foreground, before.background);
        assert_eq!(state(&w).colors.background, before.foreground);
        // Black has many valid field positions. Retain the actual drag position.
        let wheel = find_named(root.upcast_ref(), "color-wheel")
            .unwrap()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap();
        let (size, origin) = wheel.drawing_bounds();
        let bounds = wheel.compute_bounds(&w.window).unwrap();
        let g = layer_ui::ColorWheelGeometry::new(size).unwrap();
        for shape in [layer_ui::ColorShape::Circle, layer_ui::ColorShape::Square] {
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Shape { shape },
            });
            for s in [0.2, 0.8] {
                let point = if shape == layer_ui::ColorShape::Circle {
                    g.disc_marker([s, 0.])
                } else {
                    [g.square[0] + s * g.square[2], g.square[1] + g.square[2]]
                };
                let p = [
                    bounds.x() + origin[0] + point[0],
                    bounds.y() + origin[1] + point[1],
                ];
                gesture(locate("color-wheel", 0.5, 0.5), p);
                let values = state(&w).colors.wheel_components();
                assert!(
                    (values[1] - s * 100.).abs() < 2. && values[2] < 2.,
                    "touch={touch}: {shape:?} {values:?}"
                );
                gesture(on_ring(90.), on_ring(270.));
                assert!((state(&w).colors.wheel_components()[1] - values[1]).abs() < 0.001);
            }
        }
        let before = state(&w).colors.rgba();
        for _ in 0..2 {
            let expected = state(&w).colors.readout.next();
            let p = locate("color-readout", 0.12, 0.07);
            gesture(p, p);
            assert_eq!(
                state(&w).colors.readout,
                expected,
                "touch={touch}: readout cycle"
            );
            assert_eq!(state(&w).colors.rgba(), before);
        }
        drop(gesture);
        let p = locate("color-Foreground", 0.5, 0.5);
        if touch {
            perform(serde_json::json!([{ "touch":"down", "point":p }]));
            pump(900);
            let menu = find_named(root.upcast_ref(), "color-swap-menu")
                .unwrap()
                .native()
                .and_downcast::<gtk::Popover>()
                .unwrap();
            assert!(
                menu.is_visible(),
                "touch hold opens swap menu before release"
            );
            perform(serde_json::json!([{ "touch":"up" }]));
        } else {
            perform(
                serde_json::json!([{ "point":p, "button":273, "down":true }, { "button":273, "down":false }]),
            );
        }
        pump(200);
        let before = state(&w).colors;
        capture_reference(
            &w,
            output
                .join(format!("swap-menu-{touch}.png"))
                .to_str()
                .unwrap(),
            1.,
        );
        let p = locate("color-swap-menu", 0.5, 0.5);
        if touch {
            perform(serde_json::json!([{ "touch":"down", "point":p }]));
            assert_eq!(
                state(&w).colors,
                before,
                "Menu press must not pick through to the wheel"
            );
            perform(serde_json::json!([{ "touch":"up" }]));
        } else {
            perform(serde_json::json!([{ "point":p, "down":true }, { "down":false }]));
        }
        assert_eq!(
            state(&w).colors.foreground,
            before.background,
            "touch={touch}: swap foreground"
        );
        assert_eq!(state(&w).colors.background, before.foreground);
    }
    // A mouse hold remains an ordinary swatch click; it never opens a menu.
    let p = locate("color-Foreground", 0.5, 0.5);
    perform(serde_json::json!([{ "point":p, "down":true }]));
    pump(900);
    let swap = find_named(root.upcast_ref(), "color-swap-menu").unwrap();
    let menu = swap.native().and_downcast::<gtk::Popover>().unwrap();
    assert!(!menu.is_visible());
    perform(serde_json::json!([{ "down":false }]));
    // Native keyboard activation uses the same focused, accessible buttons.
    let readout = find_named(root.upcast_ref(), "color-readout").unwrap();
    assert!(readout.grab_focus());
    let before = state(&w).colors.readout;
    perform(serde_json::json!([{ "key":32, "down":true }, { "key":32, "down":false }]));
    assert_eq!(state(&w).colors.readout, before.next());
    let before = state(&w).colors.readout;
    perform(serde_json::json!([{ "key":65293, "down":true }, { "key":65293, "down":false }]));
    assert_eq!(state(&w).colors.readout, before.next());
    capture_reference(&w, output.join("keyboard-focus.png").to_str().unwrap(), 2.);
    // Three-digit RGB readouts are the widest values at the minimum size.
    w.dispatch(UiAction::SetColor { rgba: [1.; 4] });
    while state(&w).colors.readout != layer_ui::ColorReadout::Rgb {
        w.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::ToggleReadout,
        });
    }
    pump(80);
    capture_reference(&w, output.join("minimum-rgb-255.png").to_str().unwrap(), 2.);
    std::fs::write(
        output.join("geometry.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}
