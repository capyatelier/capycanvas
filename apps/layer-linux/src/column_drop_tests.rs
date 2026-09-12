//! Actual Mutter mouse/touch drops near collapsed-column separators.
use super::*;

#[test]
#[ignore = "isolated native-input.js --column-drops"]
fn native_collapsed_divider_drop_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.ColumnDrops");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
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
    for edge in [Edge::Left, Edge::Right] {
        let mut fixture = layer_ui::WorkspaceState::default();
        fixture.layout.bands[0].edge = edge;
        fixture.layout.bands[1].edge = if edge == Edge::Left {
            Edge::Right
        } else {
            Edge::Left
        };
        fixture
            .layout
            .set_column_collapsed(5, true, viewport)
            .unwrap();
        fixture
            .layout
            .set_column_collapsed(8, true, viewport)
            .unwrap();
        for touch in [false, true] {
            // +/-5px belongs to the divider; +/-8px joins a neighboring group.
            for (offset, merge, cancel) in [
                (-5., false, false),
                (0., false, false),
                (5., false, false),
                (-8., true, false),
                (8., true, false),
                (5., false, true),
            ] {
                println!(
                    "Checking {edge:?}, touch={touch}, offset={offset}, merge={merge}, cancel={cancel}"
                );
                w.dispatch(UiAction::RestoreWorkspace {
                    workspace: fixture.clone(),
                });
                pump(300);
                let before = saved();
                let r = w.resolved();
                let column = r.collapsed.iter().find(|c| c.id == 4).unwrap();
                let group = &column.groups[1];
                let divider_y = group.bounds.y - 6.;
                let destination = [column.bounds.x + 18., divider_y + offset];
                let merge_group = if offset < 0. { 5 } else { 6 };
                let strip = find_named(w.surface.upcast_ref(), "collapsed-column-4").unwrap();
                let separator = find_css(&strip, "collapsed-group")
                    .unwrap()
                    .next_sibling()
                    .unwrap();
                assert!(separator.has_css_class("column-divider"));
                let divider = separator.compute_bounds(&w.surface).unwrap();
                assert!(
                    (divider.y() + divider.height() / 2. - divider_y).abs() < 1.,
                    "native/shared divider alignment"
                );
                let source = find_named(w.surface.upcast_ref(), "column-icon-Layers")
                    .unwrap()
                    .compute_bounds(&w.surface)
                    .unwrap();
                let start = [
                    source.x() + source.width() / 2.,
                    source.y() + source.height() / 2.,
                ];
                perform(if touch {
                    serde_json::json!([{"touch":"down","point":start}])
                } else {
                    serde_json::json!([{"point":start},{"down":true}])
                });
                pump(800);
                assert!(
                    w.workspace_drag.borrow().as_ref().is_some_and(|d| d.held),
                    "held collapsed icon arms pickup"
                );
                perform(if touch {
                    serde_json::json!([{"touch":"move","point":[750.,470.]},{"touch":"move","point":destination}])
                } else {
                    serde_json::json!([{"point":[750.,470.]},{"point":destination}])
                });
                let hint = w.drop_hint.borrow().clone().unwrap_or_else(|| {
                    panic!(
                        "missing preview: point={destination:?}, drag={:?}, direct={:?}",
                        w.workspace_drag
                            .borrow()
                            .as_ref()
                            .map(|d| (d.started, d.held, d.point)),
                        w.drop_at(
                            destination[0],
                            destination[1],
                            layer_ui::DockItem::Panel {
                                panel: Panel::Layers
                            }
                        )
                    )
                });
                if merge {
                    assert!(
                        matches!(hint.target, DockTarget::Tab { group, .. } if group == merge_group)
                    );
                } else {
                    assert_eq!(
                        hint.target,
                        DockTarget::Split {
                            group: 6,
                            edge: Edge::Top
                        },
                        "{edge:?}, touch={touch}, offset={offset}"
                    );
                    assert!((hint.bounds.y + hint.bounds.height / 2. - divider_y).abs() < 1.);
                }
                if cancel {
                    perform(
                        serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
                    );
                    assert!(
                        w.workspace_drag.borrow().is_none(),
                        "Escape cancels before release"
                    );
                } else if edge == Edge::Left && !touch && !merge && offset == 0. {
                    capture_reference(&w, "/tmp/capy-column-drop-gtk.png", 1.0);
                }
                perform(if touch {
                    serde_json::json!([{"touch":"up"}])
                } else {
                    serde_json::json!([{"down":false}])
                });
                assert!(w.workspace_drag.borrow().is_none());
                assert!(w.drop_hint.borrow().is_none());
                if cancel {
                    assert_eq!(saved(), before);
                    continue;
                }
                let after = saved();
                let layout = state(&w).workspace.layout;
                assert!(layout.is_collapsed(4));
                assert!(layout.floating.is_empty());
                if merge {
                    assert_eq!(layout.panel_group(Panel::Layers).unwrap(), merge_group);
                } else {
                    let column = w
                        .resolved()
                        .collapsed
                        .into_iter()
                        .find(|c| c.id == 4)
                        .unwrap();
                    assert_eq!(column.groups.len(), 3);
                    assert_eq!(column.groups[1].icons[0].panel, Panel::Layers);
                }
                w.dispatch(UiAction::Invoke {
                    command: CommandId::UndoWorkspace,
                });
                pump(150);
                assert_eq!(saved(), before);
                w.dispatch(UiAction::Invoke {
                    command: CommandId::RedoWorkspace,
                });
                pump(150);
                assert_eq!(saved(), after);
            }
        }
    }
    for edge in [Edge::Top, Edge::Left] {
        let mut fixture = layer_ui::WorkspaceState::default();
        let old = fixture
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .to_vec();
        for tile in old {
            fixture.layout.remove_tool(Panel::Toolbar, tile.id).unwrap();
        }
        fixture
            .layout
            .insert_tools(
                Panel::Toolbar,
                None,
                &[
                    ToolbarControl::Command {
                        command: CommandId::Brush,
                    },
                    ToolbarControl::Command {
                        command: CommandId::Eraser,
                    },
                    ToolbarControl::Divider,
                    ToolbarControl::Command {
                        command: CommandId::Lasso,
                    },
                    ToolbarControl::Command {
                        command: CommandId::Hand,
                    },
                ],
            )
            .unwrap();
        fixture
            .layout
            .move_panel(
                viewport,
                Panel::Toolbar,
                DockTarget::Edge { edge, outer: true },
            )
            .unwrap();
        let ids = fixture
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>();
        for touch in [false, true] {
            for (offset, cancel) in [(-5., false), (5., false), (8., false), (5., true)] {
                println!(
                    "Checking toolbar {edge:?}, touch={touch}, offset={offset}, cancel={cancel}"
                );
                w.dispatch(UiAction::RestoreWorkspace {
                    workspace: fixture.clone(),
                });
                pump(300);
                let before = saved();
                let bounds = |id| {
                    find_named(w.surface.upcast_ref(), &format!("tile-{id}"))
                        .unwrap()
                        .compute_bounds(&w.surface)
                        .unwrap()
                };
                let b = bounds(ids[0]);
                let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
                let b = bounds(ids[2]);
                let center = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
                let mut point = center;
                point[if edge == Edge::Top { 0 } else { 1 }] += offset;
                perform(if touch {
                    serde_json::json!([{"touch":"down","point":start}])
                } else {
                    serde_json::json!([{"point":start},{"down":true}])
                });
                pump(800);
                perform(if touch {
                    serde_json::json!([{"touch":"move","point":point}])
                } else {
                    serde_json::json!([{"point":point}])
                });
                let hint = w
                    .drop_hint
                    .borrow()
                    .clone()
                    .expect("toolbar divider preview");
                if offset <= 6. {
                    assert_eq!(
                        hint.target,
                        DockTarget::TileGroup {
                            panel: Panel::Toolbar,
                            divider: ids[2]
                        }
                    );
                    assert!((hint.bounds.x + hint.bounds.width / 2. - center[0]).abs() < 1.);
                    assert!((hint.bounds.y + hint.bounds.height / 2. - center[1]).abs() < 1.);
                } else {
                    assert!(matches!(hint.target, DockTarget::Tile { .. }));
                }
                if cancel {
                    perform(
                        serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
                    );
                }
                perform(if touch {
                    serde_json::json!([{"touch":"up"}])
                } else {
                    serde_json::json!([{"down":false}])
                });
                assert!(w.workspace_drag.borrow().is_none());
                assert!(w.drop_hint.borrow().is_none());
                if cancel {
                    assert_eq!(saved(), before);
                    continue;
                }
                let after = saved();
                let layout = state(&w).workspace.layout;
                let tiles = layout.panel(Panel::Toolbar).unwrap().tiles();
                let expected = if offset <= 6. {
                    vec![
                        ids[1],
                        ids[2],
                        ids[0],
                        ids[4] + 1,
                        ids[3],
                        ids[4],
                    ]
                } else {
                    vec![ids[1], ids[2], ids[0], ids[3], ids[4]]
                };
                assert_eq!(tiles.iter().map(|t| t.id).collect::<Vec<_>>(), expected);
                assert_eq!(
                    tiles
                        .iter()
                        .filter(|t| t.control == ToolbarControl::Divider)
                        .count(),
                    if offset <= 6. { 2 } else { 1 }
                );
                w.dispatch(UiAction::Invoke {
                    command: CommandId::UndoWorkspace,
                });
                pump(150);
                assert_eq!(saved(), before);
                w.dispatch(UiAction::Invoke {
                    command: CommandId::RedoWorkspace,
                });
                pump(150);
                assert_eq!(saved(), after);
                if offset <= 6. {
                    // Move the sole tool out of the new group. Its two former
                    // boundaries must become one as part of this same edit.
                    let b = bounds(ids[0]);
                    let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
                    let b = bounds(ids[4]);
                    let point = if edge == Edge::Top {
                        [b.x() + b.width() - 3., b.y() + b.height() / 2.]
                    } else {
                        [b.x() + b.width() / 2., b.y() + b.height() - 3.]
                    };
                    perform(if touch {
                        serde_json::json!([{"touch":"down","point":start}])
                    } else {
                        serde_json::json!([{"point":start},{"down":true}])
                    });
                    pump(800);
                    perform(if touch {
                        serde_json::json!([{"touch":"move","point":point},{"touch":"up"}])
                    } else {
                        serde_json::json!([{"point":point},{"down":false}])
                    });
                    let collapsed = saved();
                    assert_eq!(
                        state(&w)
                            .workspace
                            .layout
                            .panel(Panel::Toolbar)
                            .unwrap()
                            .tiles()
                            .iter()
                            .map(|t| t.id)
                            .collect::<Vec<_>>(),
                        [ids[1], ids[2], ids[3], ids[4], ids[0]]
                    );
                    assert!(
                        find_named(w.surface.upcast_ref(), &format!("tile-{}", ids[4] + 1))
                            .is_none(),
                        "redundant divider widget is removed"
                    );
                    w.dispatch(UiAction::Invoke {
                        command: CommandId::UndoWorkspace,
                    });
                    pump(150);
                    assert_eq!(
                        saved(),
                        after,
                        "one undo restores the nonempty group and its divider IDs"
                    );
                    w.dispatch(UiAction::Invoke {
                        command: CommandId::RedoWorkspace,
                    });
                    pump(150);
                    assert_eq!(saved(), collapsed);
                }
            }
        }
    }
    println!(
        "PASS: native mouse/touch toolbar group drops and empty-group cleanup, centered previews on both axes, cancellation and one-step undo/redo"
    );
    println!(
        "PASS: native mouse/touch separator drops at +/-5px, adjacent tiles at +/-8px, aligned previews, cancellation, undo/redo on both sides"
    );
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.destroy();
    pump(100);
}
