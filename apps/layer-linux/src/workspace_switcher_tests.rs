use super::*;
use layer_workspace::{DEFAULT_WORKSPACES, ManagerPage};

fn send(dir: &std::path::Path, step: &mut usize, events: serde_json::Value) {
    std::fs::write(
        dir.join(format!("step-{step}.json")),
        serde_json::to_vec(&events).unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !dir.join(format!("done-{step}")).exists() {
        pump(10);
        assert!(Instant::now() < deadline, "native step {step} timed out");
    }
    *step += 1;
    pump(200);
}
fn at(w: &Workspace, widget: &gtk::Widget, x: f32, y: f32) -> [f32; 2] {
    if let Some(popup) = widget.native().and_downcast::<gtk::Popover>() {
        let b = widget.compute_bounds(&popup).unwrap();
        let surface = popup.surface().unwrap().downcast::<gdk::Popup>().unwrap();
        let (dx, dy) = popup.surface_transform();
        [
            surface.position_x() as f32 - dx as f32 + b.x() + b.width() * x,
            surface.position_y() as f32 - dy as f32 + b.y() + b.height() * y,
        ]
    } else {
        let b = widget.compute_bounds(&w.window).unwrap();
        [b.x() + b.width() * x, b.y() + b.height() * y]
    }
}
fn row(w: &Workspace, id: &str) -> gtk::Widget {
    find_named(w.window.upcast_ref(), &format!("workspace-row-{id}")).unwrap()
}
fn click(w: &Workspace, dir: &std::path::Path, step: &mut usize, widget: &gtk::Widget) {
    send(
        dir,
        step,
        serde_json::json!([{ "point": at(w, widget, 0.5, 0.5) }, { "down": true }, { "down": false }]),
    );
}
fn menu_button(widget: &gtk::Widget) -> Option<gtk::MenuButton> {
    if let Some(menu) = widget.downcast_ref::<gtk::MenuButton>() {
        return Some(menu.clone());
    }
    let mut child = widget.first_child();
    while let Some(w) = child {
        if let Some(menu) = menu_button(&w) {
            return Some(menu);
        }
        child = w.next_sibling();
    }
    None
}
fn drag(
    w: &Workspace,
    dir: &std::path::Path,
    step: &mut usize,
    id: &str,
    target: &str,
    after: bool,
    touch: bool,
    handle: bool,
    hold: bool,
) {
    let source = if handle {
        find_named(
            w.window.upcast_ref(),
            &format!("workspace-reorder-handle-{id}"),
        )
        .unwrap()
    } else {
        row(w, id)
    };
    let start = at(w, &source, if handle { 0.5 } else { 0.35 }, 0.5);
    let end = at(w, &row(w, target), 0.35, if after { 0.85 } else { 0.15 });
    let mut events = vec![];
    if touch {
        events.push(serde_json::json!({"touch":"down", "point":start}));
    } else {
        events.extend([
            serde_json::json!({"point":start}),
            serde_json::json!({"down":true}),
        ]);
    }
    if hold {
        events.extend((0..10).map(|_| serde_json::json!({})));
    }
    for t in [0.2, 0.5, 0.8, 1.] {
        let point = [
            start[0] + (end[0] - start[0]) * t,
            start[1] + (end[1] - start[1]) * t,
        ];
        events.push(if touch {
            serde_json::json!({"touch":"move", "point":point})
        } else {
            serde_json::json!({"point":point})
        });
    }
    events.push(if touch {
        serde_json::json!({"touch":"up"})
    } else {
        serde_json::json!({"down":false})
    });
    send(dir, step, serde_json::Value::Array(events));
}

#[test]
#[ignore = "isolated Mutter mouse/touch driver and SQLite; bench/workspace-switcher.sh"]
fn native_workspace_switcher_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.WorkspaceSwitcherInput");
    let settings = gtk::Settings::default().unwrap();
    settings.set_gtk_enable_animations(false);
    settings.set_gtk_long_press_time(500);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    pump(300);
    let manager = w.workspaces.manager.as_ref().unwrap();
    let [p, i, f] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let original = manager.current().unwrap().capture().unwrap();
    let document = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .clone();
    w.workspaces.ui.show(&w, ManagerPage::Workspaces);
    pump(500);
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    click(&w, &dir, &mut step, &row(&w, &f));
    assert_eq!(manager.active_id().as_deref(), Some(i.as_str()));
    let preview = durable_layout(&state(&w).workspace.layout);
    assert_eq!(
        preview,
        layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk)
    );
    for id in [&p, &i, &f] {
        let handle = find_named(
            w.window.upcast_ref(),
            &format!("workspace-reorder-handle-{id}"),
        )
        .unwrap();
        assert!(handle.width() <= 16, "grips should stay narrow");
    }
    let popup = menu_button(&row(&w, &p)).unwrap().popover().unwrap();
    send(
        &dir,
        &mut step,
        serde_json::json!([
            {"point":at(&w, &row(&w, &p), 0.35, 0.5)},
            {"button":273,"down":true}, {"button":273,"down":false}
        ]),
    );
    assert!(popup.is_visible(), "right-click opens the row menu");
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    send(
        &dir,
        &mut step,
        serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
    );
    assert!(!popup.is_visible());
    // Only touch holds open menus; mouse release remains with the native row.
    for touch in [false, true] {
        let point = at(&w, &row(&w, &p), 0.35, 0.5);
        let mut events = if touch {
            vec![serde_json::json!({"touch":"down","point":point})]
        } else {
            vec![
                serde_json::json!({"point":point}),
                serde_json::json!({"down":true}),
            ]
        };
        events.extend((0..10).map(|_| serde_json::json!({})));
        send(&dir, &mut step, serde_json::Value::Array(events));
        assert_eq!(
            popup.is_visible(), touch,
            "only touch holds open the row menu"
        );
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
        if touch { assert!(!popup.is_autohide(), "hold retains the contact"); }
        send(
            &dir,
            &mut step,
            serde_json::json!([if touch {
                serde_json::json!({"touch":"up"})
            } else {
                serde_json::json!({"down":false})
            }]),
        );
        assert_eq!(
            popup.is_visible(), touch,
            "only touch release retains a menu"
        );
        if !touch {
            // Restore the preview after the native mouse release before testing
            // that touch hold/release preserves it.
            click(&w, &dir, &mut step, &row(&w, &f));
        }
        if touch {
            assert!(popup.is_autohide(), "released menu is dismissible");
            send(
                &dir,
                &mut step,
                serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
            );
        }
        assert!(!popup.is_visible());
        assert!(
            durable_layout(&state(&w).workspace.layout) == preview,
            "hold release must not change the preview"
        );
    }
    // Dragging any row must preserve the selected Photographer preview.
    drag(&w, &dir, &mut step, &p, &f, true, false, false, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "mouse body drag"
    );
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    assert_eq!(manager.current().unwrap().capture().unwrap(), original);
    drag(&w, &dir, &mut step, &p, &i, false, false, true, false);
    assert_eq!(
        manager.switcher_ids(),
        [p.clone(), i.clone(), f.clone()],
        "mouse handle drag"
    );
    drag(&w, &dir, &mut step, &p, &f, true, true, true, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "touch handle without hold"
    );
    drag(&w, &dir, &mut step, &p, &i, false, true, false, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "ordinary touch swipe must not reorder"
    );
    let held_popup = menu_button(&row(&w, &p)).unwrap().popover().unwrap();
    drag(&w, &dir, &mut step, &p, &i, false, true, false, true);
    assert!(
        !held_popup.is_visible(),
        "same-contact drag closes the hold menu"
    );
    assert_eq!(
        manager.switcher_ids(),
        [p.clone(), i.clone(), f.clone()],
        "touch body hold drag"
    );
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    assert_eq!(manager.current().unwrap().capture().unwrap(), original);
    let menu = menu_button(&row(&w, &p)).unwrap();
    click(&w, &dir, &mut step, menu.upcast_ref());
    let popup = menu.popover().unwrap();
    capture_popover(&popup, dir.join("switcher-options.png").to_str().unwrap());
    click(
        &w,
        &dir,
        &mut step,
        find_button(popup.upcast_ref(), "Move Down")
            .unwrap()
            .upcast_ref(),
    );
    assert_eq!(manager.switcher_ids(), [i.clone(), p.clone(), f.clone()]);
    let menu = menu_button(&row(&w, &p)).unwrap();
    click(&w, &dir, &mut step, menu.upcast_ref());
    let pin = find_named(
        menu.popover().unwrap().upcast_ref(),
        &format!("workspace-pin-{p}"),
    )
    .unwrap();
    click(&w, &dir, &mut step, &pin);
    assert_eq!(manager.switcher_ids(), [i.clone(), f.clone()]);
    assert!(
        find_named(
            w.window.upcast_ref(),
            &format!("workspace-reorder-handle-{p}")
        )
        .is_some()
    );
    drag(&w, &dir, &mut step, &p, &f, true, true, true, false);
    assert_eq!(manager.workspace_ids(), [i.clone(), f.clone(), p.clone()]);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone()],
        "reordering a hidden row never pins it"
    );
    drag(&w, &dir, &mut step, &p, &i, false, false, true, false);
    assert_eq!(manager.workspace_ids(), [p.clone(), i.clone(), f.clone()]);
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    capture_reference(&w, dir.join("switcher-manager.png").to_str().unwrap(), 1.);
    click(
        &w,
        &dir,
        &mut step,
        find_button(w.window.upcast_ref(), "Cancel")
            .unwrap()
            .upcast_ref(),
    );
    assert_eq!(
        durable_layout(&state(&w).workspace.layout),
        *original.history.layout()
    );
    assert_eq!(
        w.gpu.borrow().as_ref().unwrap().session.engine().document(),
        &document
    );
    // Populate a longer library without switching the live editor.
    let mut custom = String::new();
    let mut creates = Vec::new();
    for index in 0..9 {
        let entity = layer_workspace::Entity::workspace(
            &if index == 0 {
                "A Sketching".into()
            } else {
                format!("Study {index}")
            },
            original.clone(),
            original.history.layout().clone(),
            None,
            20_000,
        );
        if index == 0 {
            custom = entity.id.clone();
        }
        creates.push(layer_workspace::Mutation::Create {
            entity,
            claim: false,
            name_policy: layer_workspace::NamePolicy::Exact,
        });
    }
    glib::MainContext::default().block_on(async {
        manager
            .store
            .request(layer_workspace::StoreRequest::Commit {
                batch: layer_workspace::CommitBatch::prepare(manager.owner.clone(), creates)
                    .unwrap(),
            })
            .await
            .unwrap();
        manager.refresh().await.unwrap();
    });
    w.workspaces.ui.show(&w, ManagerPage::Workspaces);
    pump(400);
    click(&w, &dir, &mut step, &row(&w, &f));
    let toggle_pin = |id: &str, step: &mut usize| {
        let menu = menu_button(&row(&w, id)).unwrap();
        click(&w, &dir, step, menu.upcast_ref());
        let pin = find_named(
            menu.popover().unwrap().upcast_ref(),
            &format!("workspace-pin-{id}"),
        )
        .unwrap();
        click(&w, &dir, step, &pin);
    };
    toggle_pin(&custom, &mut step);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), custom.clone()]
    );
    for id in manager.switcher_ids() {
        toggle_pin(&id, &mut step);
    }
    assert!(manager.switcher_ids().is_empty());
    assert!(!w.workspaces.switcher.is_visible());
    // All hidden rows still have grips and can move before being shown again.
    drag(&w, &dir, &mut step, &custom, &p, false, false, true, false);
    assert_eq!(manager.workspace_ids()[0], custom);
    assert!(manager.switcher_ids().is_empty());
    let hidden_menu = menu_button(&row(&w, &custom)).unwrap();
    send(
        &dir,
        &mut step,
        serde_json::json!([
            {"point":at(&w, &row(&w, &custom), 0.35, 0.5)},
            {"button":273,"down":true}, {"button":273,"down":false}
        ]),
    );
    assert!(
        hidden_menu.popover().unwrap().is_visible(),
        "hidden rows have context menus too"
    );
    send(
        &dir,
        &mut step,
        serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
    );
    toggle_pin(&custom, &mut step);
    toggle_pin(&p, &mut step);
    assert_eq!(manager.switcher_ids(), [custom.clone(), p.clone()]);
    assert!(w.workspaces.switcher.is_visible());
    // Row-menu ordering is available through keyboard activation, too.
    let menu = menu_button(&row(&w, &p)).unwrap();
    click(&w, &dir, &mut step, menu.upcast_ref());
    let up = find_button(menu.popover().unwrap().upcast_ref(), "Move Up").unwrap();
    up.grab_focus();
    pump(100);
    assert!(up.has_focus() && up.is_sensitive());
    send(
        &dir,
        &mut step,
        serde_json::json!([{"key":32,"down":true},{"key":32,"down":false}]),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while manager.switcher_ids() != [p.clone(), custom.clone()] && Instant::now() < deadline {
        pump(20);
    }
    assert_eq!(manager.switcher_ids(), [p.clone(), custom.clone()]);
    let list = find_named(w.window.upcast_ref(), "workspace-manager-items").unwrap();
    let scroll = list
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
        .unwrap();
    scroll.vadjustment().set_value(0.);
    pump(100);
    drag(&w, &dir, &mut step, &custom, &p, false, true, false, false);
    assert!(
        scroll.vadjustment().value() > 10.,
        "touch body swipe should scroll a long list"
    );
    assert_eq!(manager.switcher_ids(), [p.clone(), custom.clone()]);
    assert!(
        durable_layout(&state(&w).workspace.layout) == preview,
        "scrolling must preserve the preview"
    );
    scroll.set_kinetic_scrolling(false);
    scroll.vadjustment().set_value(0.);
    pump(200);
    let start = at(&w, &row(&w, &p), 0.35, 0.5);
    let end = at(&w, &row(&w, &custom), 0.35, 0.8);
    send(
        &dir,
        &mut step,
        serde_json::json!([
            { "point":start }, { "down":true },
            { "point":[start[0], start[1]+18.] }, { "point":end }, { "point":end }
        ]),
    );
    assert!(row(&w, &p).has_css_class("workspace-row-dragging"));
    send(
        &dir,
        &mut step,
        serde_json::json!([
            {"key":65307,"down":true}, {"key":65307,"down":false}, {"down":false}
        ]),
    );
    assert_eq!(
        manager.switcher_ids(),
        [p.clone(), custom.clone()],
        "Escape cancels a reorder"
    );
    assert!(
        w.workspaces.ui.dialog.is_visible(),
        "Escape during drag must keep the manager open"
    );
    assert!(
        durable_layout(&state(&w).workspace.layout) == preview,
        "cancelled drag must preserve the preview"
    );
    assert_eq!(manager.current().unwrap().capture().unwrap(), original);
    capture_reference(&w, dir.join("switcher-custom.png").to_str().unwrap(), 1.);
    click(
        &w,
        &dir,
        &mut step,
        find_button(w.window.upcast_ref(), "Cancel")
            .unwrap()
            .upcast_ref(),
    );
    let saved_order = manager.workspace_ids();
    w.window.close();
    pump(400);
    assert!(!w.window.is_visible());
    let reopened = Workspace::new(&app);
    reopened.window.present();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !reopened.workspaces.ready.get() || reopened.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(
        reopened.workspaces.manager.as_ref().unwrap().switcher_ids(),
        [p, custom.clone()]
    );
    // This fixture may import its unbound legacy settings on reopening. Any
    // new entry follows the saved rows without disturbing their relative order.
    assert!(
        reopened
            .workspaces
            .manager
            .as_ref()
            .unwrap()
            .workspace_ids()
            .starts_with(&saved_order)
    );
    assert!(
        find_named(
            reopened.header.upcast_ref(),
            &format!("workspace-switch-{custom}")
        )
        .is_some()
    );
    reopened.window.close();
    pump(300);
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.close();
    pump(500);
    assert!(!w.window.is_visible());
}
