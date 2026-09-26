//! Mutter-delivered contacts exercise pickup, native holds, and history.
use super::*;

#[test]
#[ignore = "isolated native-input.js --drag-pickup"]
fn native_drag_pickup_input() {
    let app = native_test_app("art.capycanvas.DragPickup");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let original = state(&w).workspace;
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let native = w.window.surface().unwrap();
    let pointer = native.display().default_seat().unwrap().pointer().unwrap();
    let cursor = || native.device_cursor(&pointer).and_then(|cursor| cursor.name());
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu"))
        .unwrap();
    let mut input = RemoteInput::new().settle_ms(35);
    input.ready();
    pump(500);
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
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
                        workspace: Box::new(original.clone()),
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
                        enable_individual_column_panels(&w, group);
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
                    let tile = source.ends_with("tile") || source == "column";
                    if !touch {
                        input.perform(serde_json::json!([{"point":start}]));
                        assert_eq!(cursor().as_deref(), Some(if tile { "default" } else { "grab" }), "{source}: hover cursor");
                    }
                    if !touch && !held && !cancel && source != "column-grip" {
                        input.perform(serde_json::json!([
                            {"point":start},{"down":true,"button":273},{"down":false,"button":273}
                        ]));
                        assert!(menu.is_visible(), "{source}: right-click opens menu");
                        input.perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
                    }
                    let before = saved();
                    let press = if touch {
                        serde_json::json!([{"touch":"down","point":start}])
                    } else {
                        serde_json::json!([{"point":start},{"down":true}])
                    };
                    input.perform(press.clone());
                    if !touch && tile {
                        assert_eq!(cursor().as_deref(), Some("default"), "press alone keeps the pointer");
                    }
                    if held {
                        pump(800);
                        assert_eq!(menu.is_visible(), touch, "{touch} {source}: only touch holds open menus");
                        assert_eq!(saved(), before, "hold must not activate");
                        if source.ends_with("tile") || source == "column" {
                            assert!(w.workspace_drag.borrow().as_ref().is_some_and(|d| d.held), "{touch} {source}: hold arms pickup");
                            if !touch {
                                assert_eq!(cursor().as_deref(), Some("grab"), "{source}: held cursor");
                                input.perform(serde_json::json!([{"point":[start[0]+1.,start[1]]}]));
                                assert_eq!(cursor().as_deref(), Some("grab"), "small held movement keeps the open hand");
                            }
                        }
                        if !cancel && (source.ends_with("tile") || source == "column") {
                            input.perform(serde_json::json!([contact(device, "up", start)]));
                            assert_eq!(menu.is_visible(), touch, "hold release menu lifetime");
                            assert_eq!(saved(), before, "held release must not activate");
                            if !touch {
                                assert_eq!(cursor().as_deref(), Some("default"), "held release restores pointer");
                            }
                            w.dismiss_context();
                            pump(150);
                            input.perform(press);
                            pump(800);
                        }
                    }
                    input.perform(serde_json::json!([contact(device, "move", point)]));
                    let expected = held || !source.ends_with("tile") && source != "column";
                    assert_eq!(
                        w.workspace_drag
                            .borrow()
                            .as_ref()
                            .is_some_and(|d| d.started),
                        expected,
                        "touch={touch} {source} held={held}: pickup"
                    );
                    if !touch && expected {
                        assert_eq!(cursor().as_deref(), Some("grabbing"), "{source}: dragging cursor");
                    }
                    assert!(!menu.is_visible(), "drag/scroll dismisses hold");
                    if cancel {
                        input.perform(
                            serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
                        );
                    }
                    input.perform(serde_json::json!([contact(device, "up", point)]));
                    pump(200);
                    assert!(w.workspace_drag.borrow().is_none());
                    assert!(w.drop_hint.borrow().is_none());
                    if !touch {
                        assert_ne!(cursor().as_deref(), Some("grabbing"), "release/cancel restores cursor");
                        if tile {
                            assert_eq!(widget.cursor().and_then(|cursor| cursor.name()).as_deref(), Some("default"), "source restores its idle cursor");
                        }
                    }
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
    // An armed cursor also retires without ever becoming a drag.
    for reason in ["escape", "blur", "removed"] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original.clone()),
        });
        pump(250);
        let widget = w.toolbar.first_child().unwrap();
        let bounds = widget.compute_bounds(&w.surface).unwrap();
        let start = [bounds.x() + bounds.width() / 2., bounds.y() + bounds.height() / 2.];
        input.perform(serde_json::json!([{"point":start},{"down":true}]));
        pump(800);
        assert_eq!(cursor().as_deref(), Some("grab"));
        match reason {
            "escape" => input.perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}])),
            "blur" => { w.interact(UiInput::Blur); }
            _ => {
                let tile = original.layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::RemoveTool { panel: Panel::Toolbar, tile },
                });
                pump(150);
                input.perform(serde_json::json!([{"point":[start[0]+20.,start[1]]}]));
            }
        }
        assert!(w.workspace_drag.borrow().is_none(), "{reason}: retires hold");
        // GTK may clear the device override after the old widget disappears;
        // an unset window cursor is also the regular pointer.
        assert!(matches!(cursor().as_deref(), None | Some("default")), "{reason}: restores pointer");
        input.perform(serde_json::json!([{"down":false}]));
        assert!(w.workspace_drag.borrow().is_none());
    }
    input.finish();
    w.window.destroy();
    pump(100);
}
