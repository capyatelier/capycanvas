//! Real mouse/touch delivery through the menu bar and panel contents.
use super::*;

#[test]
#[ignore = "isolated native-input.js --native-test=native_layout_drop_input"]
fn native_layout_drop_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.LayoutDrops");
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native input timed out");
            pump(5);
        }
        step += 1;
        pump(100);
    };
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    for (theme, edge) in [(Theme::Dark, Edge::Left), (Theme::Light, Edge::Right)] {
        for touch in [false, true] {
            let event = |phase: &str, point: [f32; 2]| {
                if touch {
                    serde_json::json!({"touch":phase,"point":point})
                } else {
                    match phase {
                        "down" => serde_json::json!({"point":point,"down":true}),
                        "up" => serde_json::json!({"down":false}),
                        _ => serde_json::json!({"point":point}),
                    }
                }
            };
            for target in ["menubar", "stack-menubar", "body", "tabs-top", "tabs-lower"] {
                for source in ["panel", "group", "toolbar", "column"] {
                    if source == "column" && (target == "body" || target.starts_with("tabs")) {
                        continue;
                    }
                    let target_group = if target == "tabs-lower" { 6 } else { 5 };
                    eprintln!("Layout drop: {theme:?}, touch={touch}, {source} -> {target}");
                    let mut fixture = layer_ui::WorkspaceState::default();
                    fixture.layout.bands[0].edge = edge;
                    fixture.layout.bands[1].edge = if edge == Edge::Left {
                        Edge::Right
                    } else {
                        Edge::Left
                    };
                    if target == "stack-menubar" {
                        fixture
                            .layout
                            .set_column_collapsed(5, true, viewport)
                            .unwrap();
                    }
                    if source == "column" {
                        fixture
                            .layout
                            .set_column_collapsed(8, true, viewport)
                            .unwrap();
                    }
                    if source == "group" {
                        fixture.layout.select_tab(8, Panel::Properties).unwrap();
                        fixture
                            .layout
                            .move_item(
                                viewport,
                                DockItem::Group { group: 8 },
                                DockTarget::Float {
                                    position: [700., 300.],
                                },
                            )
                            .unwrap();
                    }
                    w.dispatch(UiAction::RestoreWorkspace {
                        workspace: Box::new(fixture),
                    });
                    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                    pump(300);
                    let before = layer_ui::durable_layout(&state(&w).workspace.layout);
                    let start = match source {
                        "panel" => center(
                            w.tab_hits()
                                .into_iter()
                                .find(|h| h.group == 8 && h.index == 0)
                                .unwrap()
                                .bounds,
                        ),
                        "column" => center(
                            w.resolved()
                                .collapsed
                                .iter()
                                .find(|c| c.id == 8)
                                .unwrap()
                                .grip,
                        ),
                        "toolbar" => {
                            let grip = find_css(w.toolbar.upcast_ref(), "panel-grip").unwrap();
                            let b = grip.compute_bounds(&w.surface).unwrap();
                            [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
                        }
                        _ => {
                            let b = w
                                .resolved()
                                .groups
                                .iter()
                                .find(|g| g.id == 8)
                                .unwrap()
                                .bounds;
                            [b.x + b.width - 10., b.y + layer_ui::TAB_BAR_HEIGHT * 0.5]
                        }
                    };
                    let r = w.resolved();
                    let bounds = if target == "stack-menubar" {
                        r.collapsed.iter().find(|c| c.id == 4).unwrap().bounds
                    } else {
                        r.groups
                            .iter()
                            .find(|g| g.id == target_group)
                            .unwrap()
                            .bounds
                    };
                    let tab = w.tab_hits().into_iter().find(|t| t.group == target_group);
                    let destination = [
                        if target.starts_with("tabs") {
                            let tab = tab.as_ref().unwrap().bounds;
                            if target == "tabs-top" {
                                tab.x + 4.
                            } else {
                                tab.x + tab.width - 4.
                            }
                        } else {
                            bounds.x + bounds.width * 0.5
                        },
                        match target {
                            "body" => bounds.y + bounds.height * 0.5,
                            "tabs-top" | "tabs-lower" => bounds.y + layer_ui::TAB_BAR_HEIGHT + 3.,
                            _ => layer_ui::HEADER_HEIGHT * 0.5,
                        },
                    ];
                    let expected = match target {
                        "stack-menubar" => DockTarget::StackColumn {
                            column: 4,
                            before: true,
                        },
                        "menubar" => DockTarget::Split {
                            group: 5,
                            edge: Edge::Top,
                        },
                        _ => DockTarget::Tab {
                            group: target_group,
                            index: Some(usize::from(target == "tabs-lower")),
                        },
                    };
                    perform(serde_json::json!([
                        event("down", start),
                        event("move", [viewport[0] * 0.5, viewport[1] * 0.5]),
                        event("move", destination)
                    ]));
                    let hint = w
                        .drop_hint
                        .borrow()
                        .clone()
                        .expect("visible layout drop preview");
                    assert_eq!(hint.target, expected);
                    if target == "body" {
                        assert_eq!(
                            hint.bounds,
                            Bounds {
                                y: bounds.y + layer_ui::TAB_BAR_HEIGHT,
                                height: bounds.height - layer_ui::TAB_BAR_HEIGHT,
                                ..bounds
                            }
                        );
                    } else if target.starts_with("tabs") {
                        assert_eq!(hint.bounds.width, 3.);
                        assert_eq!(hint.bounds.y, bounds.y);
                        assert_eq!(hint.bounds.height, layer_ui::TAB_BAR_HEIGHT);
                    } else {
                        assert!(hint.bounds.height <= 3.);
                    }
                    if source == "group" && !touch {
                        capture_reference(
                            &w,
                            output
                                .join(format!("{target}-{theme:?}.png"))
                                .to_str()
                                .unwrap(),
                            1.,
                        );
                    }
                    if source == "panel" {
                        perform(
                            serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
                        );
                        perform(serde_json::json!([event("up", destination)]));
                        assert_eq!(
                            layer_ui::durable_layout(&state(&w).workspace.layout),
                            before
                        );
                        perform(serde_json::json!([
                            event("down", start),
                            event("move", destination)
                        ]));
                    }
                    perform(serde_json::json!([event("up", destination)]));
                    assert!(w.workspace_drag.borrow().is_none() && w.drop_hint.borrow().is_none());
                    let layout = state(&w).workspace.layout;
                    layout.validate().unwrap();
                    let moved = match source {
                        "toolbar" => vec![Panel::Toolbar],
                        "group" | "column" => {
                            vec![Panel::Layers, Panel::Adjustments, Panel::Properties]
                        }
                        _ => vec![Panel::Layers],
                    };
                    if target == "body" || target.starts_with("tabs") {
                        let expected = if target == "tabs-lower" {
                            [vec![Panel::Sizes], moved.clone()].concat()
                        } else {
                            [moved.clone(), vec![Panel::Brushes]].concat()
                        };
                        assert_eq!(layout.group_panels(target_group).unwrap(), expected);
                        let selected = if source == "group" {
                            Panel::Properties
                        } else {
                            moved[0]
                        };
                        assert_eq!(
                            w.resolved()
                                .groups
                                .iter()
                                .find(|g| g.id == target_group)
                                .unwrap()
                                .active,
                            selected
                        );
                    } else if target == "stack-menubar" {
                        let stack = layout.column_stack(4);
                        assert_eq!(stack.members.len(), 2);
                        assert_eq!(stack.members[1], 4);
                        assert!(!stack.drawers);
                    } else {
                        let r = w.resolved();
                        let incoming = r
                            .groups
                            .iter()
                            .find(|g| g.panels.contains(&moved[0]))
                            .unwrap();
                        let previous = r.groups.iter().find(|g| g.id == 5).unwrap();
                        assert!(incoming.bounds.y < previous.bounds.y);
                        assert!(layout.collapsed.is_empty());
                    }
                    let after = layer_ui::durable_layout(&layout);
                    w.dispatch(UiAction::Invoke {
                        command: CommandId::UndoWorkspace,
                    });
                    pump(150);
                    assert_eq!(
                        layer_ui::durable_layout(&state(&w).workspace.layout),
                        before
                    );
                    w.dispatch(UiAction::Invoke {
                        command: CommandId::RedoWorkspace,
                    });
                    pump(150);
                    assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), after);
                }
            }
        }
    }
}
