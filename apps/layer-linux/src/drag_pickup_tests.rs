//! Mutter-delivered contacts exercise pickup, native holds, and history.
use super::*;

#[test]
#[ignore = "isolated native-input.js --drag-pickup"]
fn native_drag_pickup_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.DragPickup");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let original = state(&w).workspace;
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu"))
        .unwrap();
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
        pump(35);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    for touch in [false, true] {
        for source in [
            "tile",
            "drawer-tile",
            "column",
            "tab",
            "toolbar-grip",
            "column-grip",
        ] {
            for held in [false, true] {
                if held && source.ends_with("grip") {
                    continue;
                }
                for cancel in [false, true] {
                    w.dispatch(UiAction::RestoreWorkspace {
                        workspace: original.clone(),
                    });
                    pump(220);
                    let group = original.layout.panel_group(Panel::Adjustments).unwrap();
                    if source == "drawer-tile" {
                        w.dispatch(UiAction::MovePanel {
                            panel: Panel::Toolbar,
                            target: DockTarget::Tab { group, index: None },
                            viewport: [w.surface.width() as f32, w.surface.height() as f32],
                        });
                    }
                    if source.starts_with("column") || source == "drawer-tile" {
                        w.dispatch(UiAction::Customize {
                            action: CustomizationAction::SetColumnCollapsed {
                                group,
                                collapsed: true,
                            },
                        });
                        pump(220);
                    }
                    if source == "drawer-tile" {
                        w.dispatch(UiAction::Customize {
                            action: CustomizationAction::ToggleColumnDrawer {
                                group,
                                panel: Panel::Toolbar,
                            },
                        });
                        pump(300);
                    }
                    let widget = match source {
                        "drawer-tile" => find_named(w.surface.upcast_ref(), "drawer-panel-Toolbar")
                            .unwrap()
                            .first_child()
                            .unwrap(),
                        "tile" => w.toolbar.first_child().unwrap(),
                        "column" => {
                            find_named(w.surface.upcast_ref(), "column-icon-Adjustments").unwrap()
                        }
                        "toolbar-grip" => find_css(w.toolbar.upcast_ref(), "panel-grip").unwrap(),
                        "column-grip" => {
                            let column =
                                find_css(w.surface.upcast_ref(), "collapsed-column").unwrap();
                            find_css(&column, "panel-grip").unwrap()
                        }
                        _ => w
                            .groups
                            .borrow()
                            .iter()
                            .find(|g| g.id == group)
                            .unwrap()
                            .tabs
                            .iter()
                            .find(|(p, _)| *p == Panel::Adjustments)
                            .unwrap()
                            .1
                            .clone()
                            .upcast(),
                    };
                    let bounds = widget.compute_bounds(&w.surface).unwrap();
                    let start = [
                        bounds.x() + bounds.width() / 2.,
                        bounds.y() + bounds.height() / 2.,
                    ];
                    let point = if source.ends_with("tile") {
                        let target = widget.next_sibling().unwrap().next_sibling().unwrap();
                        let b = target.compute_bounds(&w.surface).unwrap();
                        [b.x() + b.width() * 0.8, b.y() + b.height() * 0.8]
                    } else if source == "column-grip" {
                        [10., 470.]
                    } else {
                        [750., 470.]
                    };
                    let before = saved();
                    let press = if touch {
                        serde_json::json!([{"touch":"down","point":start}])
                    } else {
                        serde_json::json!([{"point":start},{"down":true}])
                    };
                    perform(press);
                    if held {
                        pump(800);
                        assert!(menu.is_visible(), "{touch} {source}: hold menu");
                        assert_eq!(saved(), before, "hold must not activate");
                    }
                    perform(if touch {
                        serde_json::json!([{"touch":"move","point":point}])
                    } else {
                        serde_json::json!([{"point":point}])
                    });
                    let expected = held || !source.ends_with("tile") && source != "column";
                    assert_eq!(
                        w.workspace_drag
                            .borrow()
                            .as_ref()
                            .is_some_and(|d| d.started),
                        expected,
                        "touch={touch} {source} held={held}: pickup"
                    );
                    assert!(!menu.is_visible(), "drag/scroll dismisses hold");
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
                    pump(200);
                    assert!(w.workspace_drag.borrow().is_none());
                    assert!(w.drop_hint.borrow().is_none());
                    if expected && !cancel {
                        let after = saved();
                        assert_ne!(after, before, "touch={touch} {source}: drop");
                        w.dispatch(UiAction::Invoke {
                            command: CommandId::UndoWorkspace,
                        });
                        pump(150);
                        assert_eq!(saved(), before, "single undo");
                        w.dispatch(UiAction::Invoke {
                            command: CommandId::RedoWorkspace,
                        });
                        pump(150);
                        assert_eq!(saved(), after, "single redo");
                    } else {
                        assert_eq!(saved(), before, "cancel/unheld tile cannot move");
                    }
                }
            }
        }
    }
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.destroy();
    pump(100);
}
