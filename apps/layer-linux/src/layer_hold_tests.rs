//! Actual Mutter touch/mouse delivery for held row reordering and scrolling.
use super::*;

#[test]
#[ignore = "isolated native-input.js --layer-hold"]
fn native_layer_hold_input() {
    let app = native_test_app("art.capycanvas.LayerHold");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::default()),
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
    let row = |id| widgets(w.layer_panel.root.upcast_ref()).find(|node| node.is_mapped() && node.widget_name() == format!("art-layer-{id}")).unwrap();
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
    let mut input = RemoteInput::new().settle_ms(120);
    input.ready();
    pump(400);
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
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
            input.perform(if touch {
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
            let drop = [target[0] + 1., target[1]];
            input.perform(serde_json::json!([
                contact(device, "move", drop),
                contact(device, "up", drop)
            ]));
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
            let mask_before = state(&w).layer_tools.editing_layer.as_ref().unwrap().mask_selected;
            input.perform(serde_json::json!([{"touch":"down", "point":start}]));
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
                region == "mask" || mask_before,
                "hold preserves editing target unless the mask menu is requested"
            );
            if release_only {
                input.perform(serde_json::json!([{"touch":"up"}]));
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
                input.perform(serde_json::json!([{"touch":"move", "point":point}]));
                assert!(!menu.is_visible(), "{region}: drag closes menu");
                input.perform(
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
    input.perform(serde_json::json!([{"touch":"down","point":start}]));
    pump(800);
    let b = bounds(&row(before[1]));
    let point = [b.x() + b.width() / 2., b.y() + b.height() - 3.];
    input.perform(serde_json::json!([{"touch":"move","point":point}]));
    input.perform(
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
    input.perform(serde_json::json!([{"point":start},{"down":true}]));
    pump(800);
    assert!(
        !w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_visible()),
        "mouse hold must not open a menu"
    );
    let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
    input.perform(serde_json::json!([{"point":[start[0]-2.,start[1]]},{"point":[start[0]-slop*3.,start[1]]},{"point":point},{"point":[point[0]+1.,point[1]]},{"down":false}]));
    assert_ne!(order(), before, "mouse continues held drag");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(150);
    assert_eq!(order(), before);
    let layer = |action| { w.dispatch(UiAction::Layer { action }); pump(150); };
    let checked = || state(&w).layers.iter().filter(|r| r.selected).map(|r| r.id).collect::<Vec<_>>();
    let check = |id| row(id).first_child().unwrap().next_sibling().unwrap();
    let click = |input: &mut RemoteInput, node: &gtk::Widget| input.click(screen_point(node, &w.surface, [0.5,0.5]));
    let capture = |input: &mut RemoteInput, theme: Theme, name: &str| {
        if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
            input.perform(serde_json::json!([{"capture":format!("{name}-{}",format!("{theme:?}").to_lowercase())}]));
        }
    };
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) }); pump(180);
        layer(layer_ui::LayerAction::Select { id: before[0], mask: true });
        click(&mut input,&check(before[1]));
        click(&mut input,&find_css(&row(before[0]),"layer-name").unwrap());
        assert!(state(&w).layer_tools.editing_layer.unwrap().mask_selected,"active row preserves mask editing");
        assert_eq!(checked(),before[..2],"active row preserves checked companions");
        let link = find_css(&row(before[0]),"layer-link").unwrap().downcast::<gtk::Button>().unwrap();
        click(&mut input,link.upcast_ref());
        assert_eq!(crate::icons::name(&link.child().unwrap().downcast::<gtk::Image>().unwrap()).as_deref(),Some("layer-unlink-symbolic"));
        capture(&mut input,theme,"mask-unlinked");
        click(&mut input,link.upcast_ref());
        assert_eq!(crate::icons::name(&link.child().unwrap().downcast::<gtk::Image>().unwrap()).as_deref(),Some("layer-link-symbolic"));
        capture(&mut input,theme,"mask-linked");
        click(&mut input,&find_css(&row(before[0]),"layer-thumbnail").unwrap());
        input.perform(serde_json::json!([{"key":65505,"down":true}]));
        click(&mut input,&find_css(&row(before[2]),"layer-name").unwrap());
        input.perform(serde_json::json!([{"key":65505,"down":false}]));
        assert_eq!(checked(),before[..3],"Shift-click selects a visible range");
        layer(layer_ui::LayerAction::Select { id: before[0], mask: false });
        input.perform(serde_json::json!([{"key":65505,"down":true}]));
        click(&mut input,&check(before[2]));
        input.perform(serde_json::json!([{"key":65505,"down":false}]));
        assert_eq!(checked(),before[..3],"Shift checkbox selects the same range");
        let group = find_named(w.layer_panel.footer.upcast_ref(),"layer-folder-symbolic").unwrap();
        click(&mut input,&group);
        let group_id = state(&w).layer_tools.editing_layer.unwrap().id;
        assert!(state(&w).layers.iter().filter(|r| before[..3].contains(&r.id)).all(|r|r.depth==1));
        assert!(!state(&w).layers.iter().find(|r|r.id==group_id).unwrap().content_selected);
        click(&mut input,&find_named(w.layer_panel.footer.upcast_ref(),"delete-selected-layers").unwrap());
        assert_eq!(order(),before,"deleting an expanded folder preserves unchecked children");
        for _ in 0..2 { w.dispatch(UiAction::Invoke { command: CommandId::Undo }); pump(150); }
        assert_eq!(order(),before);
        layer(layer_ui::LayerAction::Select { id: before[0], mask: false });
        w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }); pump(180);
        w.dispatch(UiAction::SelectPanelTab { group: state(&w).workspace.layout.panel_group(Panel::Layers).unwrap(), panel: Panel::Layers }); pump(180);
        let effect = state(&w).layer_tools.editing_layer.unwrap().id;
        let thumbnail = find_css(&row(effect),"layer-thumbnail").unwrap();
        assert!(!descendants::<gtk::DrawingArea>(&thumbnail).first().unwrap().is_visible(),"filter glyph has no editable-pixel corners");
        capture(&mut input,theme,"filter-selected");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo }); pump(180);
        layer(layer_ui::LayerAction::Select { id: before[0], mask: false });
        click(&mut input,&check(before[1]));
        let source = row(before[0]);
        let name = find_css(&source,"layer-name").unwrap();
        assert!(name.is_mapped(),"checked row name is visible");
        let start = screen_point(&name,&w.surface,[0.5,0.5]);
        let target = screen_point(&row(before[2]),&w.surface,[0.7,0.9]);
        let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
        input.perform(serde_json::json!([{"point":start},{"down":true},{"point":[start[0]-2.,start[1]]},{"point":[start[0]-slop*3.,start[1]]}]));
        let controllers = source.observe_controllers();
        let drag = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>()).unwrap();
        assert!(drag.drag().is_some(),"checked row starts native pickup");
        input.perform(serde_json::json!([{"point":target},{"point":[target[0]+1.,target[1]]},{"down":false}]));
        assert_eq!(order(),[vec![before[2],before[0],before[1]],before[3..].to_vec()].concat(),"drag moves checked rows as one ordered block");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo }); pump(180);
        assert_eq!(order(),before);
        layer(layer_ui::LayerAction::Select { id: before[0], mask: false });
    }
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
    input.perform(
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
    input.finish();
    w.window.destroy();
    pump(100);
}
