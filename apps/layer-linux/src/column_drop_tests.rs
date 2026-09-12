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
            // Offsets +/-15 are over neighboring tiles, outside the old 12px gap.
            for (offset, merge, cancel) in [
                (-15., false, false),
                (0., false, false),
                (15., false, false),
                (0., true, false),
                (15., false, true),
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
                let destination = [
                    column.bounds.x + 18.,
                    if merge {
                        group.bounds.y + 18.
                    } else {
                        divider_y + offset
                    },
                ];
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
                    assert!(matches!(hint.target, DockTarget::Tab { group: 6, .. }));
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
                    assert_eq!(layout.panel_group(Panel::Layers).unwrap(), 6);
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
    println!(
        "PASS: native mouse/touch separator drops at +/-15px, aligned previews, tile merging, cancellation, undo/redo on both sides"
    );
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.destroy();
    pump(100);
}
