//! Native content measurement and release sizing, with real mouse/touch input.
use super::*;

#[test]
#[ignore = "isolated native-input.js --workspace-drop-sizes"]
fn native_workspace_drop_sizes() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let captures = std::env::var("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dir.join("captures"));
    std::fs::create_dir_all(&captures).unwrap();
    let app = native_test_app("art.capycanvas.DropSizes");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        let file = dir.join(format!("step-{step}.json"));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native input timed out");
            pump(5);
        }
        step += 1;
        pump(100);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    let mut reports = Vec::new();
    for count in [2, 60] {
        while state(&w).layers.len() < count {
            w.dispatch(UiAction::Layer {
                action: layer_ui::LayerAction::New {
                    group: false,
                    clipped: false,
                },
            });
        }
        assert_eq!(state(&w).layers.len(), count);
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            for touch in [false, true] {
                let cases = if count == 2 {
                    vec![
                        ("color-wide", Panel::Color, 360., 700., 20.),
                        ("color-squished", Panel::Color, 200., 80., 600.),
                        ("color-footer", Panel::Color, 280., 700., 500.),
                        ("two-layers", Panel::Layers, 400., 700., 20.),
                        ("two-squished", Panel::Layers, 400., 80., 600.),
                    ]
                } else {
                    vec![
                        ("long-squished", Panel::Layers, 400., 80., 600.),
                        ("long-useful", Panel::Layers, 400., 320., 600.),
                        ("long-room", Panel::Layers, 400., 700., 290.),
                        ("long-bottom", Panel::Layers, 400., 700., 20.),
                        ("existing", Panel::Layers, 400., 600., 700.),
                        ("existing-bottom", Panel::Layers, 400., 600., 20.),
                        ("filters", Panel::Adjustments, 400., 700., 600.),
                    ]
                };
                for (name, panel, width, source_height, room) in cases {
                    println!("Checking {name}, {theme:?}, touch={touch}, layers={count}");
                    let footer = name == "color-footer";
                    let existing = name.starts_with("existing");
                    let mut fixture = layer_ui::WorkspaceState::default();
                    fixture.layout.bands.retain(|b| b.id != 7);
                    let band = fixture.layout.bands.iter_mut().find(|b| b.id == 3).unwrap();
                    band.extent = width;
                    let layer_ui::DockNode::Split {
                        first, fraction, ..
                    } = &mut band.root
                    else {
                        unreachable!()
                    };
                    *fraction =
                        source_height / (viewport[1] - HEADER_HEIGHT - WORKSPACE_SPACING * 2.);
                    **first = layer_ui::DockNode::Tabs {
                        id: 5,
                        panels: vec![panel],
                        active: panel,
                        tab_style: Default::default(),
                    };
                    fixture
                        .layout
                        .panels
                        .iter_mut()
                        .find(|p| p.id == panel)
                        .unwrap()
                        .hide_tab = footer;
                    if existing {
                        fixture
                            .layout
                            .move_panel(
                                viewport,
                                panel,
                                DockTarget::Float {
                                    position: [500., 200.],
                                },
                            )
                            .unwrap();
                        let floating = fixture.layout.floating.last_mut().unwrap();
                        floating.width = width;
                        floating.height = Some(source_height);
                    }
                    w.dispatch(UiAction::RestoreWorkspace {
                        workspace: Box::new(fixture),
                    });
                    pump(250);
                    let source = w
                        .resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.active == panel)
                        .unwrap();
                    let view = w
                        .groups
                        .borrow()
                        .iter()
                        .find(|g| g.id == source.id)
                        .unwrap()
                        .root
                        .clone();
                    let handle: gtk::Widget = if footer {
                        find_css(view.upcast_ref(), "panel-grip").unwrap()
                    } else {
                        w.groups
                            .borrow()
                            .iter()
                            .flat_map(|g| &g.tabs)
                            .find(|(p, _)| *p == panel)
                            .unwrap()
                            .1
                            .clone()
                            .upcast()
                    };
                    let b = handle.compute_bounds(&w.surface).unwrap();
                    let start = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
                    let event = |phase: &str, p: [f32; 2]| {
                        if touch {
                            serde_json::json!({"touch": phase, "point": p})
                        } else {
                            match phase {
                                "down" => serde_json::json!({"point": p, "down": true}),
                                "up" => serde_json::json!({"down": false}),
                                _ => serde_json::json!({"point": p}),
                            }
                        }
                    };
                    perform(serde_json::json!([event("down", start)]));
                    let center = [850., 450.];
                    perform(serde_json::json!([event("move", center)]));
                    let moving = || {
                        w.gpu
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .session
                            .workspace_update()
                            .drag
                            .unwrap()
                            .group
                            .unwrap()
                    };
                    let preview = moving();
                    assert!(
                        (preview.bounds.height - source.bounds.height).abs() < 1.,
                        "preview retains source height"
                    );
                    let offset = center[1] - preview.bounds.y;
                    let bottom =
                        viewport[1] - state(&w).workspace.layout.bottom_inset - WORKSPACE_SPACING;
                    let end = [
                        850.,
                        if footer {
                            HEADER_HEIGHT + room - preview.bounds.height + offset
                        } else {
                            bottom - room + offset
                        },
                    ];
                    perform(serde_json::json!([event("move", end)]));
                    assert_eq!(moving().bounds.height, preview.bounds.height);
                    assert!(w.drop_hint.borrow().is_none());
                    perform(serde_json::json!([event("up", end)]));
                    pump(150);
                    let placed = w
                        .resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.active == panel)
                        .unwrap();
                    assert!(placed.floating);
                    assert!(placed.bounds.y >= HEADER_HEIGHT);
                    assert!(placed.bounds.y + placed.bounds.height <= bottom + 1.);
                    let root = w
                        .groups
                        .borrow()
                        .iter()
                        .find(|g| g.id == placed.id)
                        .unwrap()
                        .root
                        .clone();
                    let native = root.compute_bounds(&w.surface).unwrap();
                    assert!((native.height() - placed.bounds.height).abs() < 1.);
                    let measurement = state(&w)
                        .workspace
                        .layout
                        .measurements
                        .into_iter()
                        .find(|m| m.panel == panel)
                        .unwrap();
                    if panel == Panel::Color {
                        let chrome = if footer {
                            placed.footer_grip.unwrap().height
                        } else {
                            TAB_BAR_HEIGHT
                        };
                        assert!(
                            (placed.bounds.height - placed.bounds.width - chrome).abs() < 1.,
                            "square plus chrome: {:?}",
                            placed.bounds
                        );
                        let wheel = w.color_panel.root.first_child().unwrap();
                        assert!((wheel.width() - wheel.height()).abs() <= 1);
                        if footer {
                            assert!(
                                (placed.bounds.y + placed.bounds.height - HEADER_HEIGHT - room)
                                    .abs()
                                    < 1.
                            );
                        }
                    } else if panel == Panel::Layers {
                        let scroll = measurement.scroll.unwrap();
                        let actual_list = w.layer_panel.list.height() as f32;
                        if count == 2 {
                            assert!(
                                (actual_list - 2. * scroll.unit_height).abs() < 2.,
                                "exactly two rows, no blank space: {actual_list}, {scroll:?}"
                            );
                            let a = w.layer_panel.list.vadjustment();
                            assert!(a.upper() - a.page_size() <= 1., "both rows fully visible");
                        } else {
                            assert!(
                                actual_list >= 4. * scroll.unit_height - 1.,
                                "four complete rows remain"
                            );
                            assert!(
                                w.layer_panel.list.vadjustment().upper() > actual_list as f64 * 4.
                            );
                        }
                    } else {
                        assert!(
                            measurement.content_height > 400.,
                            "large filter catalog measured through nested scroller"
                        );
                        assert!(placed.bounds.height <= 450.);
                    }
                    capture_reference(
                        &w,
                        captures
                            .join(format!("{theme:?}-{touch}-{name}.png"))
                            .to_str()
                            .unwrap(),
                        1.,
                    );
                    reports.push(serde_json::json!({"case": name, "theme": format!("{theme:?}"), "touch": touch,
                        "source": source.bounds, "placed": placed.bounds, "measurement": measurement}));
                }
            }
        }
    }
    std::fs::write(
        captures.join("drop-sizes.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.close();
    pump(100);
}
