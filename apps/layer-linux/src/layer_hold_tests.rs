//! Actual Mutter touch/mouse delivery for held row reordering and scrolling.
use super::*;

#[test]
#[ignore = "isolated native-input.js --layer-hold"]
fn native_layer_hold_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.LayerHold");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: layer_ui::WorkspaceState::default(),
    });
    for _ in 0..2 {
        w.dispatch(UiAction::Layer {
            action: layer_ui::LayerAction::New {
                group: false,
                clipped: false,
            },
        });
    }
    pump(300);
    let order = || state(&w).layers.iter().map(|l| l.id).collect::<Vec<_>>();
    let before = order();
    w.dispatch(UiAction::Layer {
        action: layer_ui::LayerAction::AddMask {
            id: before[0],
            replace: false,
        },
    });
    pump(300);
    let row = |id| find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}")).unwrap();
    let bounds = |node: &gtk::Widget| node.compute_bounds(&w.surface).unwrap();
    let dismiss = || {
        let popovers: Vec<_> = w
            .popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .collect();
        for p in popovers {
            p.popdown();
            p.set_autohide(true);
        }
        pump(120);
    };
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
        pump(120);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(400);
    for touch in [false, true] {
        for handle in [false, true] {
            let source = row(before[0]);
            let node = if handle {
                source.last_child().unwrap()
            } else {
                find_css(&source, "layer-name").unwrap()
            };
            let b = bounds(&node);
            let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
            let b = bounds(&row(before[1]));
            let target = [b.x() + b.width() / 2., b.y() + b.height() - 3.];
            let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
            let pickup = [start[0] - slop * 3., start[1]];
            let jitter = [start[0] - 2., start[1]];
            perform(if touch {
                serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":jitter},{"touch":"move","point":pickup}])
            } else {
                serde_json::json!([{"point":start},{"down":true},{"point":jitter},{"point":pickup}])
            });
            let controllers = if handle {
                node.observe_controllers()
            } else {
                source.observe_controllers()
            };
            let drag = (0..controllers.n_items())
                .find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>())
                .unwrap();
            assert_eq!(
                drag.drag().is_some(),
                handle || !touch,
                "pickup crosses slop without waiting for a hold"
            );
            perform(if touch {
                serde_json::json!([{"touch":"move","point":[target[0]+1.,target[1]]},{"touch":"up"}])
            } else {
                serde_json::json!([{"point":[target[0]+1.,target[1]]},{"down":false}])
            });
            if handle || !touch {
                assert_ne!(
                    order(),
                    before,
                    "touch={touch} handle={handle}: immediate row pickup"
                );
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Undo,
                });
                pump(150);
            }
            assert_eq!(
                order(),
                before,
                "unheld touch body must not reorder; direct drag is one undo"
            );
            dismiss();
        }
    }
    for region in [
        "padding", "name", "content", "mask", "eye", "check", "link", "grip",
    ] {
        for release_only in [true, false] {
            dismiss();
            let source = row(before[0]);
            let eye = source.first_child().unwrap();
            let check = eye.next_sibling().unwrap();
            let thumbnails = check.next_sibling().unwrap();
            let node = match region {
                "name" => find_css(&source, "layer-name").unwrap(),
                "content" => thumbnails.first_child().unwrap().next_sibling().unwrap(),
                "mask" => thumbnails.last_child().unwrap(),
                "eye" => eye,
                "check" => check,
                "link" => thumbnails
                    .first_child()
                    .unwrap()
                    .next_sibling()
                    .unwrap()
                    .next_sibling()
                    .unwrap(),
                "grip" => source.last_child().unwrap(),
                _ => source.clone(),
            };
            let b = bounds(&node);
            let start = [
                b.x()
                    + if region == "padding" {
                        1.
                    } else {
                        b.width() / 2.
                    },
                b.y() + b.height() / 2.,
            ];
            let visible = state(&w)
                .layers
                .iter()
                .find(|l| l.id == before[0])
                .unwrap()
                .visible;
            perform(serde_json::json!([{"touch":"down", "point":start}]));
            pump(800);
            let menu = w
                .popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .find(|p| p.is_visible())
                .expect("hold opens menu");
            assert_eq!(
                state(&w)
                    .layer_tools
                    .editing_layer
                    .as_ref()
                    .unwrap()
                    .mask_selected,
                region == "mask",
                "hold targets the content/mask menu"
            );
            if release_only {
                perform(serde_json::json!([{"touch":"up"}]));
                assert!(menu.is_visible(), "{region}: release keeps menu");
                assert_eq!(order(), before);
                assert_eq!(
                    state(&w)
                        .layers
                        .iter()
                        .find(|l| l.id == before[0])
                        .unwrap()
                        .visible,
                    visible,
                    "hold must not click row buttons"
                );
            } else {
                let b = bounds(&row(before[1]));
                let point = [b.x() + b.width() / 2., b.y() + b.height() - 3.];
                perform(serde_json::json!([{"touch":"move", "point":point}]));
                assert!(!menu.is_visible(), "{region}: drag closes menu");
                perform(
                    serde_json::json!([{"touch":"move", "point":[point[0]+1.,point[1]]}, {"touch":"up"}]),
                );
                assert_ne!(
                    order(),
                    before,
                    "{region}: held row drops with same contact"
                );
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Undo,
                });
                pump(150);
                assert_eq!(order(), before, "one undo restores order");
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Redo,
                });
                pump(150);
                assert_ne!(order(), before);
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Undo,
                });
                pump(150);
            }
        }
    }
    dismiss();
    let b = bounds(&find_css(&row(before[0]), "layer-name").unwrap());
    let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
    perform(serde_json::json!([{"touch":"down","point":start}]));
    pump(800);
    let b = bounds(&row(before[1]));
    let point = [b.x() + b.width() / 2., b.y() + b.height() - 3.];
    perform(serde_json::json!([{"touch":"move","point":point}]));
    perform(
        serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false},{"touch":"up"}]),
    );
    assert_eq!(
        order(),
        before,
        "Escape cancels held reorder without a history entry"
    );
    dismiss();
    let b = bounds(&find_css(&row(before[0]), "layer-name").unwrap());
    let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
    perform(serde_json::json!([{"point":start},{"down":true}]));
    pump(800);
    assert!(
        w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_visible()),
        "mouse hold opens menu"
    );
    perform(serde_json::json!([{"point":point},{"point":[point[0]+1.,point[1]]},{"down":false}]));
    assert_ne!(order(), before, "mouse continues held drag");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(150);
    assert_eq!(order(), before);
    for _ in 0..30 {
        w.dispatch(UiAction::Layer {
            action: layer_ui::LayerAction::New {
                group: false,
                clipped: false,
            },
        });
    }
    pump(300);
    w.layer_panel.list.vadjustment().set_value(0.);
    pump(200);
    let scrolling_order = order();
    let b = bounds(&find_css(&row(scrolling_order[3]), "layer-name").unwrap());
    let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
    perform(
        serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":[start[0],start[1]-40.]},{"touch":"move","point":[start[0],start[1]-85.]},{"touch":"up"}]),
    );
    pump(800);
    assert!(
        w.layer_panel.list.vadjustment().value() > 20.,
        "unheld touch scrolls normally"
    );
    assert_eq!(order(), scrolling_order);
    assert!(
        !w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_visible()),
        "scroll cancels the hold"
    );
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.destroy();
    pump(100);
}
