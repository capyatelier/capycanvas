use super::*;

#[test]
#[ignore = "isolated Mutter input and SQLite; workspace-motion.sh gtk --workspace-transitions"]
fn native_workspace_transition_stability() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.WorkspaceTransitions");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(true);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    pump(300);
    let geometry = |w: &Workspace| {
        let b = w.area.compute_bounds(&w.window).unwrap();
        [b.x(), b.y(), b.width(), b.height()]
    };
    let original = geometry(&w);
    let frames = Rc::new(RefCell::new(Vec::new()));
    let clock = w.window.frame_clock().unwrap();
    let sampling = clock.connect_after_paint(glib::clone!(
        #[weak]
        w,
        #[strong]
        frames,
        move |_| frames.borrow_mut().push(geometry(&w))
    ));
    let disabled = Rc::new(Cell::new(0));
    w.surface.connect_sensitive_notify(glib::clone!(
        #[strong]
        disabled,
        move |surface| {
            if !surface.is_sensitive() {
                disabled.set(disabled.get() + 1);
            }
        }
    ));
    let notices = Rc::new(Cell::new(0));
    w.workspaces.root.connect_visible_notify(glib::clone!(
        #[strong]
        notices,
        move |root| {
            if root.is_visible() {
                notices.set(notices.get() + 1);
            }
        }
    ));
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    for _ in 0..2 {
        for (id, _) in DEFAULT_WORKSPACES {
            // The visible Sketch/Paint/Photo labels can change; widget identity
            // follows the stable stored IDs, as in workspace_switcher.rs.
            let name = id.rsplit(':').next().unwrap();
            let button = find_named(
                w.header.root.upcast_ref(),
                &format!("workspace-switch-{name}"),
            )
            .unwrap();
            click(&w, &dir, &mut step, &button);
            while w.workspaces.busy.get()
                || w.workspaces
                    .manager
                    .as_ref()
                    .unwrap()
                    .active_id()
                    .as_deref()
                    != Some(id)
            {
                pump(5);
                assert!(Instant::now() < deadline);
            }
            pump(200);
        }
    }
    // The same owner validation runs after a lease has expired or ownership
    // was lost while this window was inactive.
    w.workspaces.revalidate(&w);
    pump(300);
    // Hold the host's pause long enough to deliver real input. Checking the
    // button signal catches native activation even if the model rejects it.
    let painter = find_named(w.header.root.upcast_ref(), "workspace-switch-painter")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    let activations = Rc::new(Cell::new(0));
    painter.connect_clicked(glib::clone!(
        #[strong]
        activations,
        move |_| activations.set(activations.get() + 1)
    ));
    assert!(painter.grab_focus());
    let active = w.workspaces.manager.as_ref().unwrap().active_id();
    w.workspaces.busy.set(true);
    w.workspaces.update_status();
    click(&w, &dir, &mut step, painter.upcast_ref());
    let point = at(&w, painter.upcast_ref(), 0.5, 0.5);
    send(
        &dir,
        &mut step,
        serde_json::json!([
            {"touch":"down", "point":point}, {"touch":"up"},
            {"key":0xff0d, "down":true}, {"key":0xff0d, "down":false},
            {"key":0x20, "down":true}, {"key":0x20, "down":false},
        ]),
    );
    assert_eq!(
        activations.get(),
        0,
        "Paused editor must block mouse, touch and keyboard activation"
    );
    assert_eq!(w.workspaces.manager.as_ref().unwrap().active_id(), active);
    w.workspaces.busy.set(false);
    w.workspaces.update_status();
    click(&w, &dir, &mut step, painter.upcast_ref());
    assert_eq!(
        activations.get(),
        1,
        "Input resumes without replacing widgets or losing their signals"
    );
    assert_eq!(
        w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_id()
            .as_deref(),
        Some(DEFAULT_WORKSPACES[0].0)
    );
    pump(250);
    let busy_notices = notices.get();
    let switching_frames = frames.borrow().clone();
    capture_reference(&w, &dir.join("steady.png").to_string_lossy(), 1.0);
    // A real failure still needs visible recovery actions, without moving the
    // canvas or changing its viewport and GPU swapchain size.
    w.workspaces.show_error(layer_workspace::StoreError::new(
        layer_workspace::ErrorKind::FailedWrite,
        "Test storage failure",
    ));
    w.workspaces.update_status();
    pump(200);
    assert!(w.workspaces.root.is_visible());
    capture_reference(&w, &dir.join("notice.png").to_string_lossy(), 1.0);
    let error_geometry = geometry(&w);
    clock.disconnect(sampling);
    let moved = switching_frames
        .iter()
        .filter(|frame| **frame != original)
        .count();
    let report = serde_json::json!({
        "baseline":original, "frames":switching_frames, "resized_frames":moved,
        "disabled_editor_transitions":disabled.get(), "busy_notices":busy_notices,
        "notice_geometry":error_geometry,
    });
    std::fs::write(
        dir.join("transitions.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "Workspace transitions: {} frames, {moved} resized frames, {} editor disables, {busy_notices} busy notices; notice bounds {error_geometry:?}",
        switching_frames.len(),
        disabled.get()
    );
    std::fs::write(dir.join("finished"), "finished").unwrap();
    assert_eq!(
        disabled.get(),
        0,
        "Temporary input pauses must not restyle the entire editor"
    );
    assert_eq!(
        busy_notices, 0,
        "Ordinary switches must not flash a status row"
    );
    assert_eq!(
        moved, 0,
        "Workspace switches must keep the canvas geometry fixed"
    );
    assert_eq!(
        error_geometry, original,
        "Recovery notices must not resize the canvas"
    );
    w.window.close();
    pump(300);
}
use crate::workspace::manager::now_ms;
use layer_workspace::{DEFAULT_WORKSPACES, ManagerPage};

#[test]
#[ignore = "GTK reference with real pointer input; --workspace-manager-visual"]
fn native_workspace_manager_visual() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output =
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or(dir.to_string_lossy().into_owned());
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.WorkspaceManagerVisual");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    fn record(widget: &gtk::Widget, root: &gtk::Widget) -> serde_json::Value {
        let b = widget.compute_bounds(root).unwrap();
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_visible() && c.is_child_visible() {
                children.push(record(&c, root));
            }
        }
        serde_json::json!({"type":widget.type_().name(), "name":widget.widget_name().to_string(),
            "css":widget.css_classes().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "bounds":[b.x(),b.y(),b.width(),b.height()], "state":format!("{:?}",widget.state_flags()),
            "font":widget.pango_context().font_description().map(|f|f.to_string()),
            "text":widget.downcast_ref::<gtk::Label>().map(|l|l.text().to_string()), "children":children})
    }
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.workspaces.ui.show(&w, ManagerPage::Workspaces);
        pump(500);
        let painter = row(&w, DEFAULT_WORKSPACES[0].0);
        let current = row(&w, DEFAULT_WORKSPACES[1].0);
        let grip = find_named(
            &painter,
            &format!("workspace-reorder-handle-{}", DEFAULT_WORKSPACES[0].0),
        )
        .unwrap();
        let more = menu_button(&painter).unwrap();
        let add = find_named(w.window.upcast_ref(), "workspace-manager-new").unwrap();
        let cancel = find_button(w.window.upcast_ref(), "Cancel").unwrap();
        for (state, point) in [
            ("normal", [10., 10.]),
            ("row", at(&w, &painter, 0.4, 0.5)),
            ("grip", at(&w, &grip, 0.5, 0.5)),
            ("options", at(&w, more.upcast_ref(), 0.5, 0.5)),
            ("current", at(&w, &current, 0.4, 0.5)),
            ("new", at(&w, &add, 0.5, 0.5)),
            ("cancel", at(&w, cancel.upcast_ref(), 0.5, 0.5)),
        ] {
            send(&dir, &mut step, serde_json::json!([{"point":point}]));
            capture_reference(&w, &format!("{output}/gtk-{name}-{state}.png"), 1.0);
            std::fs::write(
                format!("{output}/gtk-{name}-{state}.json"),
                serde_json::to_vec_pretty(&record(
                    w.workspaces.ui.dialog.upcast_ref(),
                    w.window.upcast_ref(),
                ))
                .unwrap(),
            )
            .unwrap();
        }
        w.workspaces.ui.close();
        pump(250);
    }
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.close();
    pump(300);
}

#[test]
#[ignore = "requires isolated workspace storage and a native GTK display"]
fn native_starting_layout_preview() {
    assert!(std::env::var_os("CAPY_WORKSPACE_DIR").is_some());
    let app = native_test_app("art.capycanvas.StartingLayoutPreview");
    let w = Workspace::new(&app);
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = w.workspaces.manager.as_ref().unwrap();
    let baseline = manager
        .current()
        .unwrap()
        .starting_layout(layer_ui::Platform::Gtk)
        .unwrap();
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Layers,
        target: DockTarget::Float {
            position: [480., 220.],
        },
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
    });
    w.dispatch(UiAction::SetBrushSize { value: 73. });
    pump(400);
    let capture = || {
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .capture_workspace()
            .unwrap()
    };
    let before = capture();
    assert_ne!(before.history.layout(), &baseline);
    let id = manager.active_id().unwrap();
    for confirm in [false, true] {
        let done = Rc::new(Cell::new(false));
        glib::spawn_future_local(glib::clone!(
            #[strong]
            w,
            #[strong]
            id,
            #[strong]
            done,
            async move {
                w.workspaces
                    .perform(&w, layer_workspace::ManagerAction::Reset(id))
                    .await
                    .unwrap();
                done.set(true);
            }
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        let restore = loop {
            pump(20);
            if let Some(button) = find_button(w.window.upcast_ref(), "Restore") {
                break button;
            }
            assert!(Instant::now() < deadline, "starting layout dialog opens");
        };
        pump(200);
        let dialog = restore
            .ancestor(adw::AlertDialog::static_type())
            .unwrap()
            .downcast::<adw::AlertDialog>()
            .unwrap();
        assert!(dialog.body().contains("Window → Undo Workspace"));
        assert_eq!(durable_layout(&state(&w).workspace.layout), baseline);
        assert_eq!(
            capture(),
            before,
            "opening the preview does not change history or tools"
        );
        if confirm {
            restore.emit_clicked();
        } else {
            find_button(dialog.upcast_ref(), "Cancel")
                .unwrap()
                .emit_clicked();
        }
        while !done.get() {
            pump(20);
            assert!(Instant::now() < deadline);
        }
        pump(200);
        if !confirm {
            assert_eq!(capture(), before);
            assert_eq!(
                &durable_layout(&state(&w).workspace.layout),
                before.history.layout()
            );
        }
    }
    assert_eq!(capture().history.undo.len(), before.history.undo.len() + 1);
    assert_eq!(capture().working, before.working);
    assert_eq!(durable_layout(&state(&w).workspace.layout), baseline);
    w.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    });
    assert_eq!(
        &durable_layout(&state(&w).workspace.layout),
        before.history.layout()
    );
    w.window.close();
    pump(300);
}

#[test]
#[ignore = "requires isolated workspace storage and a native GTK display"]
fn native_active_workspace_delete() {
    check_active_workspace_delete(false);
}

#[test]
#[ignore = "requires isolated workspace storage and a native GTK display"]
fn native_active_workspace_delete_with_occupied_default() {
    check_active_workspace_delete(true);
}

fn check_active_workspace_delete(occupied_default: bool) {
    assert!(std::env::var_os("CAPY_WORKSPACE_DIR").is_some());
    let app = native_test_app("art.capycanvas.WorkspaceDelete");
    let w = Workspace::new(&app);
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = w.workspaces.manager.as_ref().unwrap();
    let original = manager.current().unwrap().capture().unwrap();
    let create = |name: &str| {
        glib::MainContext::default().block_on(async {
            let incoming = manager
                .create_workspace(name, None, false, now_ms())
                .await
                .unwrap();
            w.workspaces.adopt(&w, Ok(incoming)).await;
        });
        pump(100);
        manager.active_id().unwrap()
    };
    let deleted = create("Delete Me");
    let other = layer_workspace::WorkspaceManager::new(manager.store.clone(), Platform::Gtk);
    if occupied_default {
        let incoming = glib::MainContext::default()
            .block_on(other.prepare_switch(DEFAULT_WORKSPACES[1].0, now_ms()))
            .unwrap();
        other.activate(incoming);
    }
    let replacement = DEFAULT_WORKSPACES[if occupied_default { 0 } else { 1 }].0;
    let expected = glib::MainContext::default()
        .block_on(manager.load(replacement))
        .unwrap()
        .entity
        .capture()
        .unwrap();
    for confirm in [false, true] {
        w.workspaces.ui.show(&w, ManagerPage::Workspaces);
        pump(400);
        let more = menu_button(&row(&w, &deleted)).unwrap();
        more.popup();
        pump(100);
        find_button(more.popover().unwrap().upcast_ref(), "Delete…")
            .unwrap()
            .emit_clicked();
        let deadline = Instant::now() + Duration::from_secs(5);
        let button = loop {
            pump(20);
            assert!(
                find_named(w.window.upcast_ref(), "workspace-item-choice").is_none(),
                "deleting the active workspace should not require a replacement picker"
            );
            if let Some(button) = find_button(w.window.upcast_ref(), "Delete") {
                break button;
            }
            assert!(
                Instant::now() < deadline,
                "delete confirmation did not open"
            );
        };
        let dialog = button.ancestor(adw::AlertDialog::static_type()).unwrap();
        find_button(&dialog, if confirm { "Delete" } else { "Cancel" })
            .unwrap()
            .emit_clicked();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            pump(20);
            let is_deleted = glib::MainContext::default()
                .block_on(manager.load(&deleted))
                .unwrap()
                .entity
                .metadata
                .deleted_at_ms
                .is_some();
            if (!confirm && !dialog.is_mapped())
                || (confirm && is_deleted && manager.active_id().as_deref() == Some(replacement))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "delete did not finish: {:?}",
                manager.error()
            );
        }
        if !confirm {
            assert_eq!(manager.active_id().as_ref(), Some(&deleted));
            assert!(manager.switcher_ids().contains(&deleted));
            assert!(
                glib::MainContext::default()
                    .block_on(manager.load(&deleted))
                    .unwrap()
                    .entity
                    .metadata
                    .deleted_at_ms
                    .is_none()
            );
            find_button(w.window.upcast_ref(), "Cancel")
                .unwrap()
                .emit_clicked();
            pump(300);
        }
    }
    pump(300);
    assert!(find_named(w.window.upcast_ref(), &format!("workspace-row-{deleted}")).is_none());
    assert!(
        find_named(
            w.header.root.upcast_ref(),
            &format!("workspace-switch-{deleted}")
        )
        .is_none()
    );
    assert!(!manager.switcher_ids().contains(&deleted));
    assert_eq!(manager.current().unwrap().capture().unwrap(), expected);
    if occupied_default {
        assert_eq!(other.current().unwrap().capture().unwrap(), original);
        assert!(other.lease_valid(now_ms()));
    }
    w.workspaces.ui.close();
    pump(300);
    w.window.close();
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.window.is_visible() {
        pump(20);
        assert!(Instant::now() < deadline, "acknowledged close");
    }
    drop(w);
    pump(30);
    let reopened = Workspace::new(&app);
    reopened.window.present();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !reopened.workspaces.ready.get() || reopened.workspaces.busy.get() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = reopened.workspaces.manager.as_ref().unwrap();
    assert_eq!(manager.active_id().as_deref(), Some(replacement));
    assert!(
        !manager
            .rows(ManagerPage::Workspaces, "", now_ms())
            .iter()
            .any(|row| row.id == deleted)
    );
    assert!(!manager.switcher_ids().contains(&deleted));
    reopened.window.close();
    pump(200);
    glib::MainContext::default()
        .block_on(other.close())
        .unwrap();
}

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
fn switcher_buttons(w: &Workspace) -> Vec<gtk::ToggleButton> {
    fn collect(widget: &gtk::Widget, buttons: &mut Vec<gtk::ToggleButton>) {
        if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>() {
            buttons.push(button.clone());
        }
        let mut child = widget.first_child();
        while let Some(node) = child {
            collect(&node, buttons);
            child = node.next_sibling();
        }
    }
    let mut buttons = Vec::new();
    collect(w.workspaces.switcher.upcast_ref(), &mut buttons);
    buttons
}
fn switcher_names(w: &Workspace) -> Vec<String> {
    switcher_buttons(w)
        .iter()
        .map(|button| button.widget_name().to_string())
        .collect()
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
    assert!(find_button(popup.upcast_ref(), "Rename…").is_none());
    assert!(find_button(popup.upcast_ref(), "Delete…").is_none());
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
            popup.is_visible(),
            touch,
            "only touch holds open the row menu"
        );
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
        if touch {
            assert!(!popup.is_autohide(), "hold retains the contact");
        }
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
            popup.is_visible(),
            touch,
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
    assert!(w.workspaces.switcher.is_visible());
    assert_eq!(switcher_names(&w), ["workspace-switch-illustrator"]);
    assert!(switcher_buttons(&w)[0].is_active());
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
    assert_eq!(
        switcher_names(&w),
        [
            "workspace-switch-illustrator".to_string(),
            format!("workspace-switch-{custom}"),
            "workspace-switch-painter".into()
        ]
    );
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
    let custom_button = find_named(
        w.header.root.upcast_ref(),
        &format!("workspace-switch-{custom}"),
    )
    .unwrap();
    click(&w, &dir, &mut step, &custom_button);
    let deadline = Instant::now() + Duration::from_secs(10);
    while manager.active_id().as_ref() != Some(&custom) {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(
        switcher_names(&w),
        [
            "workspace-switch-painter".to_string(),
            format!("workspace-switch-{custom}")
        ],
        "switching away removes the temporary Illustrator entry"
    );
    assert_eq!(manager.switcher_ids(), [p.clone(), custom.clone()]);
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
            reopened.header.root.upcast_ref(),
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
