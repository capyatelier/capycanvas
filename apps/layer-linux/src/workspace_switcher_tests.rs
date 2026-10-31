use super::*;

#[test]
#[ignore = "isolated Mutter input and SQLite; workspace-motion.sh gtk --workspace-transitions"]
fn native_workspace_transition_stability() {
    let mut input = switcher_input();
    let app = native_test_app("art.capycanvas.WorkspaceTransitions");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(true);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(300);
    let geometry = |w: &Workspace| {
        let b = w.area.compute_bounds(&w.window).unwrap();
        [b.x(), b.y(), b.width(), b.height()]
    };
    w.dispatch(UiAction::SetBrushSize { value: 73. });
    w.dispatch(UiAction::SetColor { rgba: [0.2, 0.3, 0.4, 1.] });
    w.dispatch(UiAction::Color { action: layer_ui::ColorAction::Library {
        action: layer_ui::ColorLibraryAction::CreatePalette { name: "Shared colors".into() },
    } });
    let editing = ui_session(&w).editing_state();
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
    input.ready();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (id, _) in DEFAULT_WORKSPACES {
            // The visible Sketch/Paint/Photo labels can change; widget identity
            // follows the stable stored IDs, as in workspace_switcher.rs.
            let name = id.rsplit(':').next().unwrap();
            let button = find_named(
                w.header.root.upcast_ref(),
                &format!("workspace-switch-{name}"),
            )
            .unwrap();
            click(&w, &mut input, &button);
            until(
                || {
                    !w.workspaces.busy()
                        && w.workspaces.manager().unwrap().active_id().as_deref() == Some(id)
                },
                "workspace switch",
            );
            pump(200);
            assert_eq!(ui_session(&w).editing_state(), editing);
        }
    }
    // The same owner validation runs after a lease has expired or ownership
    // was lost while this window was inactive.
    w.workspaces
        .send(&w, layer_workspace::WorkspaceInput::Resume);
    pump(300);
    // Hold the host's pause long enough to deliver real input. Checking the
    // button signal catches native activation even if the model rejects it.
    let painter = named::<gtk::ToggleButton>(w.header.root.upcast_ref(), "workspace-switch-painter");
    let activations = Rc::new(Cell::new(0));
    painter.connect_clicked(glib::clone!(
        #[strong]
        activations,
        move |_| activations.set(activations.get() + 1)
    ));
    assert!(painter.grab_focus());
    let active = w.workspaces.manager().unwrap().active_id();
    w.workspaces.pause(&w, true);
    click(&w, &mut input, painter.upcast_ref());
    let point = screen_point(painter.upcast_ref(), &w.window, [0.5, 0.5]);
    input.perform(serde_json::json!([
        {"touch":"down", "point":point}, {"touch":"up"},
        {"key":0xff0d, "down":true}, {"key":0xff0d, "down":false},
        {"key":0x20, "down":true}, {"key":0x20, "down":false},
    ]));
    assert_eq!(
        activations.get(),
        0,
        "Paused editor must block mouse, touch and keyboard activation"
    );
    assert_eq!(w.workspaces.manager().unwrap().active_id(), active);
    w.workspaces.pause(&w, false);
    click(&w, &mut input, painter.upcast_ref());
    assert_eq!(
        activations.get(),
        1,
        "Input resumes without replacing widgets or losing their signals"
    );
    assert_eq!(
        w.workspaces.manager().unwrap().active_id().as_deref(),
        Some(DEFAULT_WORKSPACES[0].0)
    );
    pump(250);
    let busy_notices = notices.get();
    let switching_frames = frames.borrow().clone();
    capture_reference(&w, &input.dir.join("steady.png").to_string_lossy(), 1.0);
    // A real failure still needs visible recovery actions, without moving the
    // canvas or changing its viewport and GPU swapchain size.
    w.workspaces.report_error(&w, "Test storage failure");
    pump(200);
    assert!(w.workspaces.root.is_visible());
    capture_reference(&w, &input.dir.join("notice.png").to_string_lossy(), 1.0);
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
        input.dir.join("transitions.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "Workspace transitions: {} frames, {moved} resized frames, {} editor disables, {busy_notices} busy notices; notice bounds {error_geometry:?}",
        switching_frames.len(),
        disabled.get()
    );
    input.finish();
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
use layer_workspace::{DEFAULT_WORKSPACES, ManagerAction, ManagerPage};

#[test]
#[ignore = "GTK reference with real pointer input; --workspace-manager-visual"]
fn native_workspace_manager_visual() {
    let mut input = switcher_input();
    let output =
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or(input.dir.to_string_lossy().into_owned());
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.WorkspaceManagerVisual");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    input.ready();
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
        assert_eq!(state(&w).theme, theme);
        w.workspaces.ui.show(&w, ManagerPage::Workspaces);
        pump(500);
        let painter = row(&w, DEFAULT_WORKSPACES[0].0);
        let current = row(&w, DEFAULT_WORKSPACES[1].0);
        let grip = find_named(
            &painter,
            &format!("workspace-reorder-handle-{}", DEFAULT_WORKSPACES[0].0),
        )
        .unwrap();
        assert!(descendants::<gtk::Image>(&painter).iter().all(|image| crate::icons::name(image).as_deref() != Some("layer-more-symbolic")), "built-in rows have no options dots");
        let add = find_named(w.window.upcast_ref(), "workspace-manager-new").unwrap();
        let trash = find_named(w.window.upcast_ref(), "workspace-manager-delete").unwrap();
        let cancel = find_button(w.window.upcast_ref(), "Cancel").unwrap();
        for (state, point) in [
            ("normal", [10., 10.]),
            ("row", screen_point(&painter, &w.window, [0.4, 0.5])),
            ("grip", screen_point(&grip, &w.window, [0.5, 0.5])),
            ("current", screen_point(&current, &w.window, [0.4, 0.5])),
            ("new", screen_point(&add, &w.window, [0.5, 0.5])),
            ("trash", screen_point(&trash, &w.window, [0.5, 0.5])),
            (
                "cancel",
                screen_point(cancel.upcast_ref(), &w.window, [0.5, 0.5]),
            ),
        ] {
            input.perform(serde_json::json!([{"point":point}]));
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
        let id = DEFAULT_WORKSPACES[0].0;
        let pin = find_named(&painter, &format!("workspace-pin-{id}")).unwrap().downcast::<gtk::ToggleButton>().unwrap();
        let original = w.workspaces.view();
        let before = layer_ui::durable_layout(&state(&w).workspace.layout);
        for selected in [false, true] {
            click(&w, &mut input, pin.upcast_ref());
            wait_switcher(&w);
            assert_eq!(pin.is_active(), selected);
            assert_eq!(w.workspaces.view().selected, original.selected);
            assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), before);
            assert_eq!(find_named(&row(&w, id), &format!("workspace-pin-{id}")).unwrap(), pin.clone().upcast::<gtk::Widget>());
        }
        for count in [2, 3, 3, 4] {
            let selected = pin.is_active();
            let revision = w.workspaces.view().switcher_revision;
            let point = screen_point(pin.upcast_ref(), &w.window, [0.5, 0.5]);
            let mut events = vec![serde_json::json!({"point":point})];
            for _ in 0..count {
                events.extend([serde_json::json!({"down":true}), serde_json::json!({"down":false})]);
            }
            input.perform(serde_json::Value::Array(events));
            until(|| w.workspaces.view().switcher_revision >= revision + count, "every rapid pin activation saves");
            wait_switcher(&w);
            assert_eq!(pin.is_active(), if count % 2 == 0 { selected } else { !selected });
            assert_eq!(w.workspaces.view().selected, original.selected);
            assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), before);
        }
        assert!(pin.grab_focus());
        until(|| pin.is_focus(), "workspace pin focus");
        for selected in [false, true] {
            input.perform(serde_json::json!([{"key":0x20,"down":true},{"key":0x20,"down":false}]));
            until(|| pin.is_active() == selected, "keyboard workspace pin activation");
            wait_switcher(&w);
            assert_eq!(pin.is_active(), selected);
            assert!(pin.is_focus());
            assert_eq!(w.workspaces.view().selected, original.selected);
        }
        for (id, edge) in [(DEFAULT_WORKSPACES[0].0, "first"), (DEFAULT_WORKSPACES[2].0, "last")] {
            click(&w, &mut input, &row(&w, id));
            until(|| w.workspaces.view().selected.as_deref() == Some(id) && !w.workspaces.view().loading, "visual row preview settled");
            capture_reference(&w, &format!("{output}/gtk-{name}-selected-{edge}.png"), 1.);
            std::fs::write(format!("{output}/gtk-{name}-selected-{edge}.json"), serde_json::to_vec_pretty(&record(w.workspaces.ui.dialog.upcast_ref(), w.window.upcast_ref())).unwrap()).unwrap();
        }
        w.workspaces.ui.close(&w);
        pump(250);
    }
    input.finish();
    w.window.close();
    pump(300);
}

#[test]
#[ignore = "requires isolated workspace storage and a native GTK display"]
fn native_starting_layout_preview() {
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app("art.capycanvas.StartingLayoutPreview");
    let w = Workspace::new(&app);
    w.window.present();
    wait_workspaces(&w);
    let manager = w.workspaces.manager().unwrap();
    let baseline = manager
        .current()
        .unwrap()
        .starting_layout(layer_ui::Platform::Gtk, false)
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
        ui_session_mut(&w)
            .capture_workspace()
            .unwrap()
    };
    let before = capture();
    assert_ne!(before.history.layout(), &baseline);
    let id = manager.active_id().unwrap();
    for confirm in [false, true] {
        w.workspaces.send(
            &w,
            layer_workspace::WorkspaceInput::Form {
                action: layer_workspace::ManagerAction::Reset(id.clone()),
            },
        );
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
        until(
            || {
                let view = w.workspaces.view();
                view.prompt.is_none() && !view.busy
            },
            "starting layout dialog closes",
        );
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
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app("art.capycanvas.WorkspaceDelete");
    let w = Workspace::new(&app);
    w.window.present();
    wait_workspaces(&w);
    let manager = w.workspaces.manager().unwrap();
    let original = manager.current().unwrap().capture().unwrap();
    let create = |name: &str| {
        w.workspaces.send(
            &w,
            layer_workspace::WorkspaceInput::Form {
                action: layer_workspace::ManagerAction::New,
            },
        );
        w.workspaces.send(
            &w,
            layer_workspace::WorkspaceInput::Submit {
                name: name.into(),
                description: None,
                choice: None,
            },
        );
        until(
            || {
                let view = w.workspaces.view();
                view.name == name && !view.busy && view.prompt.is_none()
            },
            "workspace creation",
        );
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
        let context = named::<gtk::Popover>(&row(&w, &deleted), &format!("workspace-context-{deleted}"));
        context.popup();
        pump(100);
        find_button(context.upcast_ref(), "Delete…")
            .unwrap()
            .emit_clicked();
        let deadline = Instant::now() + Duration::from_secs(5);
        let button = loop {
            pump(20);
            assert!(
                find_named(w.window.upcast_ref(), "workspace-item-choice").is_none(),
                "deleting the active workspace should not require a replacement picker"
            );
            if let Some(button) = dialog_button(&w, "Delete") {
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
                .is_err();
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
                    .is_ok()
            );
            dialog_button(&w, "Cancel").unwrap().emit_clicked();
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
    w.workspaces.ui.close(&w);
    pump(300);
    w.window.close();
    until(|| !w.window.is_visible(), "acknowledged close");
    drop(w);
    pump(30);
    let reopened = Workspace::new(&app);
    reopened.window.present();
    wait_workspaces(&reopened);
    let manager = reopened.workspaces.manager().unwrap();
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

fn switcher_input() -> RemoteInput {
    RemoteInput::new().settle_ms(200).timeout_secs(8)
}

fn grip_drop_painted_order(w: &Rc<Workspace>, input: &mut RemoteInput, output: &std::path::Path) {
    use std::io::Write;
    let manager = w.workspaces.manager().unwrap();
    let initial_theme = state(w).theme;
    let [p, i, f] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let order = manager.workspace_ids();
    let mut moved = order.clone();
    moved[..3].clone_from_slice(&[i.clone(), f.clone(), p.clone()]);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(".grip-paint-source { background: #ff00ff; } .grip-paint-next { background: #00ff00; } .grip-paint-last { background: #0000ff; }");
    gtk::style_context_add_provider_for_display(&w.surface.display(), &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    let mut failures = Vec::new();
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.workspaces.ui.show(w, ManagerPage::Workspaces);
        wait_switcher(w);
        let source = named::<gtk::Label>(&row(w, &p), &format!("workspace-name-{p}"));
        let next = named::<gtk::Label>(&row(w, &i), &format!("workspace-name-{i}"));
        source.add_css_class("grip-paint-source");
        next.add_css_class("grip-paint-next");
        let last = named::<gtk::Label>(&row(w, &f), &format!("workspace-name-{f}"));
        last.add_css_class("grip-paint-last");
        for pending in [true, false] {
            let timing = if pending { "pending-after-end" } else { "same-move-ack-before-end" };
            let list = named::<gtk::ListBox>(w.window.upcast_ref(), "workspace-manager-items");
            let source_row = row(w, &p);
            let released = Rc::new(Cell::new(false));
            let frames = sample_after_paint(&list, glib::clone!(#[weak] w, #[weak] source_row, #[strong] released,
                #[upgrade_or_default] move || (released.get(), source_row.has_css_class("workspace-row-dragging"),
                    w.workspaces.view().switcher_busy, w.workspaces.view().rows.iter().map(|row|row.id.clone()).collect::<Vec<_>>())));
            let mut lock = pending.then(|| {
                let ready = input.dir.join(format!("drop-paint-lock-{name}"));
                let database = crate::storage::workspaces().unwrap().join("workspaces.sqlite3");
                let lock = std::process::Command::new("python3").arg("-c")
                    .arg("import pathlib,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('BEGIN IMMEDIATE'); pathlib.Path(sys.argv[2]).touch(); sys.stdin.readline(); c.rollback()")
                    .arg(database).arg(&ready).stdin(std::process::Stdio::piped()).spawn().unwrap();
                until(|| ready.exists(), "drop paint storage lock acquired");
                lock
            });
            let grip = find_named(&source_row, &format!("workspace-reorder-handle-{p}")).unwrap();
            let start = screen_point(&grip, &w.window, [0.5, 0.5]);
            let end = [start[0], screen_point(&row(w, &f), &w.window, [0.35, 0.85])[1]];
            let mut events = vec![contact("mouse", "down", start)];
            for t in [0.2, 0.5, 0.8, 1.] { events.push(contact("mouse", "move", [start[0], start[1] + (end[1]-start[1])*t])); }
            input.perform(serde_json::Value::Array(events));
            assert!(source_row.has_css_class("workspace-row-dragging"), "{name}/{timing}: real native drag remains active");
            if !pending {
                w.workspaces.send(w, layer_workspace::WorkspaceInput::EditSwitcher {
                    edit: layer_workspace::SwitcherEdit::Move { id: p.clone(), before: order.get(3).cloned() },
                });
                until(|| manager.workspace_ids()==moved && !w.workspaces.view().switcher_busy, "same move acknowledged while native contact held");
                assert!(source_row.has_css_class("workspace-row-dragging"), "acknowledgement precedes native drag_end");
            }
            released.set(true);
            input.perform(serde_json::json!([contact("mouse", "up", end)]));
            assert!(!source_row.has_css_class("workspace-row-dragging"), "native release completes drag_end");
            let pending_at_end = w.workspaces.view().switcher_busy;
            let frames = frames.finish();
            if let Some(lock) = lock.as_mut() {
                lock.stdin.take().unwrap().write_all(b"release\n").unwrap();
                assert!(lock.wait().unwrap().success());
            }
            let initial = frames.iter().find(|frame| !frame.state.0 && !frame.state.1
                && frame.color_bounds(&w.window, [255,0,255,255]).is_some()
                && frame.color_bounds(&w.window, [0,255,0,255]).is_some()
                && frame.color_bounds(&w.window, [255,0,0,255]).is_some());
            let first = frames.iter().find(|frame| frame.state.0 && !frame.state.1);
            let bounds = |frame: &PaintedSample<_>, color| frame.color_bounds(&w.window, color)
                .map(|b|[b.x(),b.y(),b.width(),b.height()]);
            let initial_bounds = initial.map(|frame|[bounds(frame,[255,0,255,255]),bounds(frame,[0,255,0,255]),bounds(frame,[255,0,0,255])]);
            let painted_bounds = first.map(|frame|[bounds(frame,[255,0,255,255]),bounds(frame,[0,255,0,255]),bounds(frame,[255,0,0,255])]);
            let correct = painted_bounds.is_some_and(|b| matches!(b,[Some(source),Some(next),Some(last)] if next[1]<last[1] && last[1]<source[1]));
            std::fs::write(output.join(format!("drop-paint-{name}-{timing}.json")),serde_json::to_vec_pretty(&serde_json::json!({
                "timing":timing,"pending_at_end":pending_at_end,"expected_order":moved,"initial_marker_bounds":initial_bounds,
                "first_post_end_marker_bounds":painted_bounds,"first_post_end_state":first.map(|frame|&frame.state),
                "first_post_end_time":first.map(|frame|frame.time),"painted_order_correct":correct,
                "frames":frames.iter().map(|frame|serde_json::json!({"time":frame.time,"state":frame.state})).collect::<Vec<_>>()
            })).unwrap()).unwrap();
            if let Some(frame) = first {
                w.window.renderer().unwrap().render_texture(&frame.node,Some(&frame.bounds))
                    .save_to_png(output.join(format!("drop-paint-{name}-{timing}.png"))).unwrap();
            }
            assert!(initial_bounds.is_some_and(|b| matches!(b,[Some(source),Some(next),Some(last)] if source[1]<next[1] && next[1]<last[1])), "{name}/{timing}: mapped initial cached list paints source/next/last identities in original order: {initial_bounds:?}");
            assert!(!pending || pending_at_end, "storage acknowledgement stays pending after drag_end");
            if !correct { failures.push(format!("{name}/{timing}: first post-drop painted order must be next,last,source; actual {painted_bounds:?}, state {:?}",first.map(|frame|&frame.state))); }
            until(|| manager.workspace_ids()==moved && !w.workspaces.view().switcher_busy, "drop move stored before fixture restore");
            w.workspaces.send(w, layer_workspace::WorkspaceInput::EditSwitcher {
                edit: layer_workspace::SwitcherEdit::Move { id: p.clone(), before: Some(i.clone()) },
            });
            until(|| manager.workspace_ids()==order && !w.workspaces.view().switcher_busy, "original order restored between painted drop cases");
        }
        source.remove_css_class("grip-paint-source");
        next.remove_css_class("grip-paint-next");
        last.remove_css_class("grip-paint-last");
    }
    gtk::style_context_remove_provider_for_display(&w.surface.display(), &provider);
    w.workspaces.ui.close(w);
    until(|| w.workspaces.view().page.is_none() && !w.workspaces.ui.dialog.is_mapped()
        && !w.workspaces.busy() && !w.workspaces.view().loading, "painted drop fixture closes before legacy grip journey");
    w.dispatch(UiAction::SetTheme { theme: Some(initial_theme) });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "isolated native input and SQLite; workspace-motion.sh gtk --native-test=native_workspace_grip_menus --native-storage"]
fn native_workspace_grip_menus() {
    let mut input = switcher_input();
    let app = native_test_app("art.capycanvas.WorkspaceGripMenus");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(300);
    input.ready();
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap_or_else(|| input.dir.clone().into_os_string()));
    std::fs::create_dir_all(&output).unwrap();
    let manager = w.workspaces.manager().unwrap();
    let original = manager.current().unwrap().capture().unwrap();
    grip_drop_painted_order(&w, &mut input, &output);
    let [p, i, f] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let grip = |id: &str| find_named(&row(&w, id), &format!("workspace-reorder-handle-{id}")).unwrap();
    let popup = |id: &str| grip(id).downcast::<gtk::MenuButton>().unwrap().popover().unwrap();
    let dots = |id: &str| descendants::<gtk::Image>(&row(&w, id)).into_iter().find(|image| crate::icons::name(image).as_deref() == Some("layer-more-symbolic"));
    let native_order = || {
        let list = named::<gtk::ListBox>(w.window.upcast_ref(), "workspace-manager-items");
        (0..manager.workspace_ids().len()).map(|index| list.row_at_index(index as i32).unwrap().widget_name().strip_prefix("workspace-row-").unwrap().to_string()).collect::<Vec<_>>()
    };
    let devices = if std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.ends_with("-tablet")) { &["mouse", "touch", "pen"][..] } else { &["mouse", "touch"][..] };
    let grip_drag = |input: &mut RemoteInput, target: &str, after: bool, device: &str, vertical: bool| {
        wait_switcher(&w);
        let start = screen_point(&grip(&p), &w.window, [0.5, 0.5]);
        let mut end = screen_point(&row(&w, target), &w.window, [0.35, if after { 0.85 } else { 0.15 }]);
        if vertical { end[0] = start[0]; }
        let mut events = vec![contact(device, "down", start)];
        for t in [0.2, 0.5, 0.8, 1.] { events.push(contact(device, "move", [start[0] + (end[0] - start[0]) * t, start[1] + (end[1] - start[1]) * t])); }
        events.push(contact(device, "up", end));
        input.perform(serde_json::Value::Array(events));
    };
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let entity = layer_workspace::Entity::workspace(&format!("Grip study {name}"), original.clone(), original.history.layout().clone(), now_ms());
        let custom = entity.id.clone();
        let mut entities: Vec<_> = (0..4).map(|index| layer_workspace::Entity::workspace(&format!("Grip scroll study {name} {index}"), original.clone(), original.history.layout().clone(), now_ms())).collect();
        entities.push(entity);
        let mutations = entities.into_iter().map(|entity| layer_workspace::Mutation::Create { entity, claim: false, name_policy: layer_workspace::NamePolicy::Exact }).collect();
        glib::MainContext::default().block_on(async {
            manager.store.request(layer_workspace::StoreRequest::Commit { batch: layer_workspace::CommitBatch::prepare(manager.owner.clone(), mutations).unwrap() }).await.unwrap();
            manager.refresh().await.unwrap();
        });
        w.workspaces.ui.show(&w, ManagerPage::Workspaces);
        pump(400);
        let scroll = find_named(w.window.upcast_ref(), "workspace-manager-items").unwrap().ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        for _ in 0..w.workspaces.view().rows.len() * 2 {
            if scroll.vadjustment().value() == 0. { break; }
            input.perform(serde_json::json!([{"point":screen_point(scroll.upcast_ref(), &w.window, [0.5, 0.5])},{"wheel":[0,-1]}]));
        }
        let create = named::<gtk::Button>(w.window.upcast_ref(), "workspace-manager-new");
        assert!(create.label().is_none());
        assert_eq!(create.tooltip_text().as_deref(), Some("New Workspace"));
        assert_eq!(crate::icons::name(&descendant::<gtk::Image>(&create).unwrap()).as_deref(), Some("layer-plus-symbolic"));
        let trash = named::<gtk::Button>(w.window.upcast_ref(), "workspace-manager-delete");
        assert_eq!(crate::icons::name(&descendant::<gtk::Image>(&trash).unwrap()).as_deref(), Some("layer-delete-symbolic"));
        assert!(create.ancestor(adw::HeaderBar::static_type()).is_none(), "New Workspace uses the list footer");
        assert!(w.workspaces.view().intro.is_empty());
        let create_bounds = create.compute_bounds(&w.window).unwrap();
        let list_bounds = scroll.compute_bounds(&w.window).unwrap();
        assert!(create_bounds.x() <= list_bounds.x() + 8., "New Workspace starts the list footer");
        assert!(create_bounds.y() >= list_bounds.y() + list_bounds.height(), "New Workspace sits below the list");
        let trash_bounds = trash.compute_bounds(&w.window).unwrap();
        assert!(trash_bounds.x() >= create_bounds.x() + create_bounds.width() && (trash_bounds.y() - create_bounds.y()).abs() < 0.5, "trash follows New Workspace in the footer");
        click(&w, &mut input, &row(&w, &f));
        until(|| !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk), "settled Photo preview");
        let preview = durable_layout(&state(&w).workspace.layout);
        std::fs::write(output.join(format!("initial-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"layout":preview})).unwrap()).unwrap();
        let pins: std::collections::BTreeSet<_> = manager.switcher_ids().into_iter().collect();
        let order = manager.workspace_ids();
        assert!(scroll.vadjustment().upper() > scroll.vadjustment().page_size(), "overflow fixtures expose the native scrollbar");
        capture_reference(&w, input.dir.join(format!("workspace-grips-top-{name}.png")).to_str().unwrap(), 1.);
        assert!(!trash.is_sensitive(), "included Photo cannot be deleted");
        for item in &w.workspaces.view().rows { assert!(dots(&item.id).is_none(), "workspace rows omit options dots"); }
        for item in &w.workspaces.view().rows {
            let bounds = row(&w, &item.id).compute_bounds(&w.window).unwrap();
            let handle = grip(&item.id).compute_bounds(&w.window).unwrap();
            let pin = find_named(&row(&w, &item.id), &format!("workspace-pin-{}", item.id)).unwrap().compute_bounds(&w.window).unwrap();
            assert!(handle.x() >= pin.x() + pin.width(), "grip follows the pin: {}", item.id);
            let spacing = handle.x() - pin.x() - pin.width();
            assert!((0. ..=12.).contains(&spacing), "trailing controls stay together without an empty menu slot: {} (spacing {spacing})", item.id);
            let gap = bounds.x() + bounds.width() - handle.x() - handle.width();
            assert!((0. ..=20.).contains(&gap), "grip sits at the row's right edge: {} (gap {gap})", item.id);
        }
        let unchanged = |phase: &str| {
            assert_eq!(manager.active_id().as_deref(), Some(i.as_str()));
            assert_eq!(w.workspaces.view().selected.as_deref(), Some(f.as_str()));
            let actual = durable_layout(&state(&w).workspace.layout);
            if actual != preview { std::fs::write(output.join(format!("failure-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"phase":phase,"view":w.workspaces.view(),"actual":actual,"expected":preview})).unwrap()).unwrap(); }
            assert_eq!(actual, preview, "{name}: {phase}; loading={}", w.workspaces.view().loading);
            assert_eq!(manager.switcher_ids().into_iter().collect::<std::collections::BTreeSet<_>>(), pins);
            assert_eq!(native_order(), manager.workspace_ids(), "{name}: {phase}; dragging rows={:?}", w.workspaces.view().rows.iter().filter(|item| row(&w, &item.id).has_css_class("workspace-row-dragging")).map(|item| &item.id).collect::<Vec<_>>());
        };
        click(&w, &mut input, create.upcast_ref());
        until(|| w.workspaces.view().prompt.as_ref().is_some_and(|prompt| prompt.name.is_some()) && find_named(w.window.upcast_ref(), "workspace-item-name").is_some_and(|entry| entry.is_mapped()), "New Workspace name form");
        click(&w, &mut input, dialog_button(&w, "Cancel").unwrap().upcast_ref());
        until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.view().loading, "cancelled New Workspace form");
        unchanged("cancelled New Workspace form");
        for _ in 0..w.workspaces.view().rows.len() * 2 {
            if scroll.vadjustment().value() == 0. { break; }
            input.perform(serde_json::json!([{"point":screen_point(scroll.upcast_ref(), &w.window, [0.5, 0.5])},{"wheel":[0,-1]}]));
        }
        for device in devices {
            let point = screen_point(&grip(&p), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact(device, "down", point), contact(device, "up", point)]));
            assert!(popup(&p).is_visible(), "{device}: a grip tap opens the menu");
            assert!(find_button(popup(&p).upcast_ref(), "Rename…").is_none());
            assert!(find_button(popup(&p).upcast_ref(), "Delete…").is_none());
            unchanged("grip tap");
            let down = find_button(popup(&p).upcast_ref(), "Move Down").unwrap();
            let point = screen_point(down.upcast_ref(), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact(device, "down", point), contact(device, "up", point)]));
            wait_switcher(&w);
            let mut moved = order.clone();moved.swap(0, 1);
            assert_eq!(manager.workspace_ids(), moved);
            unchanged("menu move down");
            let point = screen_point(&grip(&p), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact(device, "down", point), contact(device, "up", point)]));
            let up = find_button(popup(&p).upcast_ref(), "Move Up").unwrap();
            assert!(up.is_sensitive(), "reopened grip menu updates ordering availability");
            let point = screen_point(up.upcast_ref(), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact(device, "down", point), contact(device, "up", point)]));
            wait_switcher(&w);
            assert_eq!(manager.workspace_ids(), order);
            let mut dragged = order.clone();
            dragged[..3].clone_from_slice(&[i.clone(), f.clone(), p.clone()]);
            for vertical in [true, false] {
                grip_drag(&mut input, &f, true, device, vertical);
                until(|| manager.workspace_ids() == dragged && native_order() == dragged && !w.workspaces.view().switcher_busy, "forward grip order committed");
                assert!(!popup(&p).is_visible(), "immediate grip drag does not leave a menu");
                assert_eq!(manager.workspace_ids(), dragged, "{name}: {device} vertical={vertical} forward drag");
                unchanged("immediate grip drag");
                grip_drag(&mut input, &i, false, device, vertical);
                until(|| manager.workspace_ids() == order && native_order() == order && !w.workspaces.view().switcher_busy, "return grip order committed");
                assert_eq!(manager.workspace_ids(), order, "{name}: {device} vertical={vertical} return drag");
                unchanged("return grip drag");
            }
        }
        wait_switcher(&w);
        assert!(grip(&p).is_sensitive(), "grip is available before the cancellation probe");
        let cancelled_start = screen_point(&grip(&p), &w.window, [0.5, 0.5]);
        let cancelled_end = [cancelled_start[0], screen_point(&row(&w, &f), &w.window, [0.35, 0.85])[1]];
        input.perform(serde_json::json!([contact("touch", "down", cancelled_start), contact("touch", "move", [cancelled_start[0], cancelled_start[1] + 18.]), contact("touch", "move", cancelled_end), contact("touch", "move", cancelled_end)]));
        assert!(row(&w, &p).has_css_class("workspace-row-dragging"), "touch drag is admitted before cancellation");
        input.perform(serde_json::json!([{"key":0xff1b,"down":true}, {"key":0xff1b,"down":false}, contact("touch", "up", cancelled_end)]));
        assert!(w.workspaces.ui.dialog.is_visible(), "cancelling a grip drag keeps its manager open");
        assert!(!popup(&p).is_visible());
        assert_eq!(manager.workspace_ids(), order);
        unchanged("cancelled touch drag");
        for key in [0xff0d, 0x20] {
            assert!(grip(&p).grab_focus());
            input.key(key);
            assert!(popup(&p).is_visible(), "keyboard activation opens the grip menu");
            unchanged("keyboard grip menu");
            input.key(0xff1b);
            let focused = gtk::prelude::GtkWindowExt::focus(&w.window).unwrap();
            assert!(focused == grip(&p) || focused.is_ancestor(&grip(&p)), "Escape returns focus to the grip");
        }
        for id in [&p, &custom] {
            let target = row(&w, id);
            let before_scroll = scroll.vadjustment().value();
            for _ in 0..w.workspaces.view().rows.len() * 2 {
                let bounds = target.compute_bounds(&scroll).unwrap();
                if bounds.y() >= 0. && bounds.y() + bounds.height() <= scroll.height() as f32 { break; }
                input.perform(serde_json::json!([{"point":screen_point(scroll.upcast_ref(), &w.window, [0.5, 0.5])},{"wheel":[0,if bounds.y() < 0. {-1} else {1}]}]));
            }
            let visible = target.compute_bounds(&scroll).unwrap();
            assert!(visible.y() >= 0. && visible.y() + visible.height() <= scroll.height() as f32, "row is scrolled fully into view before hitting its right controls");
            if scroll.vadjustment().value() != before_scroll {
                click(&w, &mut input, &grip(id));
                assert!(popup(id).is_visible(), "right-edge grip stays hittable after native scrolling");
                input.key(0xff1b);
            }
            let controls = [find_named(&target, &format!("workspace-pin-{id}")).unwrap(), grip(id)];
            assert!(controls[0].grab_focus());
            for control in controls.iter().skip(1) {
                input.key(0xff09);
                let focused = gtk::prelude::GtkWindowExt::focus(&w.window).unwrap();
                assert!(focused == *control || focused.is_ancestor(control), "Tab follows visual pin/grip order");
            }
            for control in controls.iter().rev().skip(1) {
                input.perform(serde_json::json!([{"key":0xffe1,"down":true},{"key":0xff09,"down":true},{"key":0xff09,"down":false},{"key":0xffe1,"down":false}]));
                let focused = gtk::prelude::GtkWindowExt::focus(&w.window).unwrap();
                assert!(focused == *control || focused.is_ancestor(control), "ShiftTab reverses visual pin/grip order");
            }
            unchanged("keyboard traversal");
        }
        let reveal = |input: &mut RemoteInput, id: &str| {
            for _ in 0..w.workspaces.view().rows.len() * 2 {
                let bounds = row(&w, id).compute_bounds(&scroll).unwrap();
                if bounds.y() >= 0. && bounds.y() + bounds.height() <= scroll.height() as f32 { return; }
                input.perform(serde_json::json!([{"point":screen_point(scroll.upcast_ref(), &w.window, [0.5, 0.5])},{"wheel":[0,if bounds.y() < 0. {-1} else {1}]}]));
            }
            panic!("workspace row did not scroll into view: {id}");
        };
        let restore_photo = |input: &mut RemoteInput| {
            reveal(input, &f);
            click(&w, input, &row(&w, &f));
            until(|| w.workspaces.view().selected.as_deref() == Some(f.as_str()) && !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == preview, "Photo preview restored");
            unchanged("metadata interaction restored Photo");
        };
        let context = || named::<gtk::Popover>(&row(&w, &custom), &format!("workspace-context-{custom}"));
        let entry = || named::<gtk::Entry>(&row(&w, &custom), &format!("workspace-rename-{custom}"));
        for mode in ["right click", "touch hold", "Menu key"] {
            reveal(&mut input, &custom);
            let point = screen_point(&row(&w, &custom), &w.window, [0.35, 0.5]);
            match mode {
                "right click" => input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}])),
                "touch hold" => {
                    let mut events = vec![contact("touch", "down", point)];
                    events.extend((0..10).map(|_| serde_json::json!({})));
                    input.perform(serde_json::Value::Array(events));
                    assert!(context().is_visible());
                    input.perform(serde_json::json!([contact("touch", "up", point)]));
                }
                _ => { assert!(grip(&custom).grab_focus()); input.key(0xff67); }
            }
            assert!(context().is_visible(), "{mode} opens the row context menu");
            for label in ["Rename…", "Delete…", "Move Up", "Move Down"] { assert!(find_button(context().upcast_ref(), label).is_some(), "{mode}: {label}"); }
            unchanged("unselected row context menu");
            if mode == "right click" {
                click(&w, &mut input, find_button(context().upcast_ref(), "Rename…").unwrap().upcast_ref());
                until(|| entry().is_mapped(), "context inline rename");
                assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
                entry().set_text("Cancelled context rename");
                input.key(0xff1b);
                until(|| w.workspaces.view().prompt.is_none() && !entry().is_mapped(), "cancelled context rename");
                assert_eq!(w.workspaces.view().rows.iter().find(|item| item.id == custom).unwrap().title, format!("Grip study {name}"));
                unchanged("context rename cancellation");
            } else {
                input.key(0xff1b);
                assert!(!context().is_visible());
            }
        }
        reveal(&mut input, &custom);
        let title = named::<gtk::Label>(&row(&w, &custom), &format!("workspace-name-{custom}"));
        let point = screen_point(title.upcast_ref(), &w.window, [0.5, 0.5]);
        pump(gtk::Settings::for_display(&w.surface.display()).gtk_double_click_time().max(0) as u64 + 1);
        input.perform(serde_json::json!([{"point":point},{"down":true},{"down":false},{"down":true},{"down":false}]));
        until(|| entry().is_mapped() && !w.workspaces.view().loading, "double-click inline rename");
        assert_eq!(w.workspaces.view().selected.as_deref(), Some(custom.as_str()));
        let custom_preview = durable_layout(&state(&w).workspace.layout);
        capture_reference(&w, input.dir.join(format!("workspace-inline-{name}.png")).to_str().unwrap(), 1.);
        entry().set_text(&format!("Renamed grip study {name}"));
        input.key(0xff0d);
        until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy() && !entry().is_mapped() && w.workspaces.view().rows.iter().any(|item| item.id == custom && item.title == format!("Renamed grip study {name}")), "inline rename committed");
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        assert!(row(&w, &custom).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "F2 inline rename");
        entry().set_text("Cancelled F2 rename");
        if name == "dark" {
            w.workspaces.ui.invalidate_localization();
            w.workspaces.tick(&w);
            until(|| entry().is_mapped() && gtk::prelude::RootExt::focus(&w.window).is_some_and(|focus| focus.is_ancestor(&entry())), "rebuilt inline rename focus");
            assert_eq!(entry().text().as_str(), "Cancelled F2 rename", "row presentation rebuild retains the inline draft");
        }
        input.key(0xff1b);
        until(|| w.workspaces.view().prompt.is_none() && !entry().is_mapped(), "F2 rename cancelled");
        assert_eq!(w.workspaces.view().rows.iter().find(|item| item.id == custom).unwrap().title, format!("Renamed grip study {name}"));
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        assert!(row(&w, &custom).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "blur inline rename");
        let blurred_name = format!("Blur saved grip study {name}");
        entry().set_text(&blurred_name);
        click(&w, &mut input, create.upcast_ref());
        until(|| w.workspaces.view().rows.iter().any(|item| item.id == custom && item.title == blurred_name) && find_named(w.window.upcast_ref(), "workspace-item-name").is_some_and(|entry| entry.is_mapped()), "blur commits rename before New Workspace form");
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        click(&w, &mut input, dialog_button(&w, "Cancel").unwrap().upcast_ref());
        until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy() && !w.workspaces.view().loading, "cancelled New Workspace after blur rename");
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        assert!(row(&w, &custom).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "selection handoff inline rename");
        let selected_name = format!("Selection saved grip study {name}");
        entry().set_text(&selected_name);
        reveal(&mut input, &f);
        click(&w, &mut input, &row(&w, &f));
        until(|| w.workspaces.view().rows.iter().any(|item| item.id == custom && item.title == selected_name) && w.workspaces.view().selected.as_deref() == Some(f.as_str()) && !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == preview, "row selection commits rename and restores Photo");
        unchanged("rename row-selection handoff");
        reveal(&mut input, &custom);
        click(&w, &mut input, &row(&w, &custom));
        until(|| w.workspaces.view().selected.as_deref() == Some(custom.as_str()) && !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == custom_preview, "custom preview restored after rename handoff");
        assert!(trash.is_sensitive(), "selected custom workspace can be deleted");
        click(&w, &mut input, trash.upcast_ref());
        until(|| w.workspaces.view().prompt.as_ref().is_some_and(|prompt| prompt.name.is_none()), "footer delete confirmation");
        capture_reference(&w, input.dir.join(format!("workspace-delete-confirm-{name}.png")).to_str().unwrap(), 1.);
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        click(&w, &mut input, dialog_button(&w, "Cancel").unwrap().upcast_ref());
        until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.view().loading, "footer delete cancelled");
        assert!(manager.workspace_ids().contains(&custom));
        assert_eq!(w.workspaces.view().selected.as_deref(), Some(custom.as_str()));
        assert_eq!(durable_layout(&state(&w).workspace.layout), custom_preview);
        restore_photo(&mut input);
        assert!(!trash.is_sensitive(), "trash follows the selected included workspace");
        let swipe = |id: &str| descendant::<crate::swipe_row::SwipeRow>(&row(&w, id)).unwrap();
        let point = screen_point(&row(&w, &f), &w.window, [0.4, 0.5]);
        input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "move", [point[0]-18.,point[1]]),contact("touch", "move", [point[0]-65.,point[1]]),contact("touch", "up", [point[0]-65.,point[1]])]));
        assert!(!swipe(&f).is_open(), "included workspaces cannot reveal Delete");
        unchanged("included swipe rejected");
        for mode in ["reverse", "cancel", "delete"] {
            reveal(&mut input, &custom);
            let point = screen_point(&row(&w, &custom), &w.window, [0.4, 0.5]);
            input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "move", [point[0]-18.,point[1]]),contact("touch", "move", [point[0]-65.,point[1]]),contact("touch", "up", [point[0]-65.,point[1]])]));
            until(|| swipe(&custom).is_open(), "touch swipe exposes Delete");
            unchanged("unselected custom swipe reveal");
            if mode == "reverse" {
                let point = screen_point(&row(&w, &custom), &w.window, [0.4, 0.5]);
                input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "move", [point[0]+18.,point[1]]),contact("touch", "move", [point[0]+65.,point[1]]),contact("touch", "up", [point[0]+65.,point[1]])]));
                until(|| swipe(&custom).is_at_rest(), "reverse swipe closes Delete");
                unchanged("reverse swipe cancellation");
                continue;
            }
            capture_reference(&w, input.dir.join(format!("workspace-swipe-{name}-{mode}.png")).to_str().unwrap(), 1.);
            let point = screen_point(swipe(&custom).delete_button().upcast_ref(), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "up", point)]));
            until(|| w.workspaces.view().prompt.as_ref().is_some_and(|prompt| prompt.name.is_none()), "swipe delete confirmation");
            unchanged("swipe delete confirmation preserves Photo");
            let label = if mode == "cancel" { "Cancel".to_string() } else { w.workspaces.view().prompt.unwrap().confirm };
            let point = screen_point(dialog_button(&w, &label).unwrap().upcast_ref(), &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "up", point)]));
            until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy() && !w.workspaces.view().loading, "swipe delete confirmation settled");
            assert_eq!(manager.workspace_ids().contains(&custom), mode == "cancel");
            unchanged("swipe delete result");
            if mode == "cancel" { capture_reference(&w, input.dir.join(format!("workspace-grips-{name}.png")).to_str().unwrap(), 1.); }
        }
        w.workspaces.ui.close(&w);
        pump(250);
    }
    input.finish();
    w.window.close();
    until(|| !w.window.is_visible(), "workspace grip test close");
}

fn workspace_audit_click_observers(row: &gtk::Widget, trace: Rc<RefCell<Vec<serde_json::Value>>>) -> Vec<(gtk::GestureClick, glib::SignalHandlerId)> {
    let started = Instant::now();
    let weak_row = row.downgrade();
    let record = Rc::new(move |gesture: &gtk::GestureClick, phase: &str, count: Option<i32>, point: Option<[f64; 2]>| {
        let picked = point.and_then(|point| weak_row.upgrade().and_then(|row| row.pick(point[0], point[1], gtk::PickFlags::DEFAULT)));
        let event = serde_json::json!({"phase":phase,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"event_time":gesture.current_event().map(|event|event.time()),"count":count,"point":point,"picked":picked.as_ref().map(|widget|widget.widget_name().to_string()),"picked_type":picked.as_ref().map(|widget|widget.type_().name()),"name_hit":picked.as_ref().is_some_and(|widget|widget.has_css_class("workspace-name"))});
        eprintln!("workspace audit click {event}");
        trace.borrow_mut().push(event);
    });
    let controllers = row.observe_controllers();
    let mut handlers = Vec::new();
    for index in 0..controllers.n_items() {
        let Some(gesture) = controllers.item(index).and_downcast::<gtk::GestureClick>().filter(|gesture| gesture.button() == 1) else { continue; };
        let pressed = gesture.connect_pressed(glib::clone!(#[strong] record, move |gesture,count,x,y| record(gesture,"pressed",Some(count),Some([x,y]))));
        let released = gesture.connect_released(glib::clone!(#[strong] record, move |gesture,count,x,y| record(gesture,"released",Some(count),Some([x,y]))));
        let stopped = gesture.connect_stopped(glib::clone!(#[strong] record, move |gesture| record(gesture,"stopped",None,None)));
        let cancelled = gesture.connect_cancel(glib::clone!(#[strong] record, move |gesture,_| record(gesture,"cancel",None,None)));
        for handler in [pressed,released,stopped,cancelled] { handlers.push((gesture.clone(),handler)); }
    }
    handlers
}

#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_stationary_row_press --native-storage"]
fn native_workspace_stationary_row_press() {
    let (_app, w, mut input, _) = workspace_audit_fixture("art.capycanvas.WorkspaceStationaryRowPress", 0);
    let id = DEFAULT_WORKSPACES[2].0;
    for (theme, name) in [(Theme::Dark,"dark"),(Theme::Light,"light")] {
        workspace_audit_theme(&w,&input,theme);
        let target = row(&w,id);
        let trace = Rc::new(RefCell::new(Vec::new()));
        let observers = workspace_audit_click_observers(&target,trace.clone());
        let point = screen_point(&target,&w.window,[0.5,0.5]);
        input.perform(serde_json::json!([{"point":point},{"down":true}]));
        until(|| trace.borrow().iter().any(|event|event["phase"]=="stopped"), "native click-series timer stopped while mouse held");
        input.perform(serde_json::json!([{"point":point},{"down":false}]));
        let selected = w.workspaces.view().selected.as_deref() == Some(id);
        std::fs::write(input.dir.join(format!("stationary-row-{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"trace":*trace.borrow(),"selected_expected":selected})).unwrap()).unwrap();
        capture_reference(&w,input.dir.join(format!("stationary-row-{name}.png")).to_str().unwrap(),1.);
        assert!(selected,"stationary builtin row press must select on release after native click-series stopped");
        until(|| !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout)==layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk),"stationary row preview completed");
        let settings = gtk::Settings::default().unwrap();
        let native_time = settings.gtk_double_click_time();
        settings.set_gtk_double_click_time(2000);
        trace.borrow_mut().clear();
        let point = screen_point(named::<gtk::Label>(&target, &format!("workspace-name-{id}")).upcast_ref(), &w.window, [0.5,0.5]);
        input.click(point);
        assert!(row(&w,DEFAULT_WORKSPACES[1].0).grab_focus());
        input.key(0x20);
        until(|| w.workspaces.view().selected.as_deref()==Some(DEFAULT_WORKSPACES[1].0) && !w.workspaces.view().loading,"keyboard selects Paint between builtin name presses");
        input.click(point);
        let second_builtin_press = trace.borrow().iter().any(|event|event["phase"]=="pressed" && event["count"]==2 && event["name_hit"]==true);
        std::fs::write(input.dir.join(format!("builtin-double-press-{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"second_builtin_name_press":second_builtin_press,"view":w.workspaces.view(),"trace":*trace.borrow()})).unwrap()).unwrap();
        assert!(second_builtin_press,"audit must exercise a native second builtin-name press");
        until(|| w.workspaces.view().selected.as_deref()==Some(id) && !w.workspaces.view().loading,"second builtin-name press still selects Photo");
        settings.set_gtk_double_click_time(native_time);
        for (gesture,handler) in observers { gesture.disconnect(handler); }
    }
    input.finish();
    w.window.close();
}


fn workspace_audit_fixture(id: &str, count: usize) -> (NativeTestApp, Rc<Workspace>, RemoteInput, Vec<String>) {
    let app = native_test_app(id);
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    let input = switcher_input();
    input.ready();
    let manager = w.workspaces.manager().unwrap();
    let original = manager.current().unwrap().capture().unwrap();
    let entities: Vec<_> = (0..count).map(|index| layer_workspace::Entity::workspace(&format!("Workspace audit {index}"), original.clone(), original.history.layout().clone(), now_ms())).collect();
    let ids = entities.iter().map(|entity| entity.id.clone()).collect();
    let mutations = entities.into_iter().map(|entity| layer_workspace::Mutation::Create { entity, claim: false, name_policy: layer_workspace::NamePolicy::Exact }).collect();
    glib::MainContext::default().block_on(async {
        manager.store.request(layer_workspace::StoreRequest::Commit { batch: layer_workspace::CommitBatch::prepare(manager.owner.clone(), mutations).unwrap() }).await.unwrap();
        manager.refresh().await.unwrap();
    });
    (app, w, input, ids)
}

fn workspace_audit_theme(w: &Rc<Workspace>, input: &RemoteInput, theme: Theme) {
    if w.workspaces.view().page.is_some() { w.workspaces.ui.close(w); }
    until(|| w.workspaces.view().page.is_none() && !w.workspaces.busy() && !w.workspaces.view().loading && !w.workspaces.ui.dialog.is_mapped(), "audit manager rollback and native close completed");
    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
    assert_eq!(state(w).theme, theme, "audit uses the requested effective theme");
    w.workspaces.ui.show(w, ManagerPage::Workspaces);
    until(|| w.workspaces.ui.dialog.is_mapped(), "audit manager mapped");
    let photo = row(w, DEFAULT_WORKSPACES[2].0);
    let cached = screen_point(&photo, &w.window, [0.5, 0.5]);
    let frame = w.workspaces.ui.dialog.child().unwrap();
    let before = frame.compute_bounds(&w.window).unwrap();
    let painted = Rc::new(Cell::new(false));
    let previous = Rc::new(Cell::new(None::<[f32; 8]>));
    let clock = w.window.frame_clock().unwrap();
    let handler = clock.connect_after_paint(glib::clone!(#[weak] w, #[strong] painted, #[strong] previous, move |_| {
        let frame = w.workspaces.ui.dialog.child().unwrap().compute_bounds(&w.window).unwrap();
        let target = row(&w, DEFAULT_WORKSPACES[2].0).compute_bounds(&w.window).unwrap();
        let bounds = [frame.x(),frame.y(),frame.width(),frame.height(),target.x(),target.y(),target.width(),target.height()];
        if previous.replace(Some(bounds)) == Some(bounds) { painted.set(true); }
        else { w.window.queue_draw(); }
    }));
    w.window.queue_draw();
    until(|| painted.get(), "audit native frame and row allocation stable");
    clock.disconnect(handler);
    let current = screen_point(&photo, &w.window, [0.5, 0.5]);
    let hit = |point: [f32; 2]| w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).is_some_and(|picked| picked == photo || picked.is_ancestor(&photo));
    let after = frame.compute_bounds(&w.window).unwrap();
    std::fs::write(input.dir.join(format!("audit-theme-{theme:?}.json")), serde_json::to_vec_pretty(&serde_json::json!({"theme":format!("{:?}",state(w).theme),"cached_photo_point":cached,"painted_photo_point":current,"cached_point_hits_photo_after_paint":hit(cached),"painted_point_hits_photo":hit(current),"frame_before":[before.x(),before.y(),before.width(),before.height()],"frame_after":[after.x(),after.y(),after.width(),after.height()]})).unwrap()).unwrap();
    assert!(hit(current), "painted Photo row accepts its actual native point");
    until(|| w.workspaces.accepts_input(w), "audit manager accepts native input after catalog refresh");
}

#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_manager_lifecycle --native-storage"]
fn native_workspace_manager_lifecycle() {
    use std::io::Write;
    let (_app,w,mut input,_) = workspace_audit_fixture("art.capycanvas.WorkspaceManagerLifecycle",0);
    let manager = w.workspaces.manager().unwrap();
    for (theme,name) in [(Theme::Dark,"dark"),(Theme::Light,"light")] {
        workspace_audit_theme(&w,&input,theme);
        let search = named::<gtk::SearchEntry>(w.window.upcast_ref(),"workspace-manager-search");
        assert!(!gtk::prelude::WidgetExt::is_visible(&search));
        let add = named::<gtk::Button>(w.window.upcast_ref(),"workspace-manager-new");
        click(&w,&mut input,add.upcast_ref());
        until(|| find_named(w.window.upcast_ref(),"workspace-item-name").is_some_and(|entry|entry.is_mapped()),"New name field visible");
        named::<gtk::Entry>(w.window.upcast_ref(),"workspace-item-name").set_text(&format!("Created lifecycle {name}"));
        let confirm = w.workspaces.view().prompt.unwrap().confirm;
        click(&w,&mut input,dialog_button(&w,&confirm).unwrap().upcast_ref());
        until(|| !w.workspaces.busy() && !w.workspaces.view().loading && w.workspaces.view().prompt.is_none(),"created workspace adopted");
        let created = manager.active_id().unwrap();
        assert_eq!(w.workspaces.view().page,Some(ManagerPage::Workspaces));
        assert_eq!(w.workspaces.view().selected.as_ref(),Some(&created));
        assert!(w.workspaces.ui.dialog.is_mapped());
        capture_reference(&w,input.dir.join(format!("manager-created-{name}.png")).to_str().unwrap(),1.);
        let trash = named::<gtk::Button>(w.window.upcast_ref(),"workspace-manager-delete");
        click(&w,&mut input,trash.upcast_ref());
        until(|| matches!(w.workspaces.view().prompt_action,Some(ManagerAction::Delete(ref id)) if id==&created),"active workspace delete prompt");
        let confirm = w.workspaces.view().prompt.unwrap().confirm;
        click(&w,&mut input,dialog_button(&w,&confirm).unwrap().upcast_ref());
        until(|| !w.workspaces.busy() && !w.workspaces.view().loading && !manager.workspace_ids().contains(&created),"active workspace deleted and replacement adopted");
        assert_eq!(w.workspaces.view().page,Some(ManagerPage::Workspaces));
        assert_eq!(w.workspaces.view().selected,manager.active_id());
        capture_reference(&w,input.dir.join(format!("manager-active-deleted-{name}.png")).to_str().unwrap(),1.);
        input.key(0xff1b);
        let escape_snapshot=|phase: &str| {
            let focus=gtk::prelude::RootExt::focus(&w.window);
            std::fs::write(input.dir.join(format!("lifecycle-escape-{phase}-{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"accepts_input":w.workspaces.accepts_input(&w),"manager_mapped":w.workspaces.ui.dialog.is_mapped(),"manager_visible":w.workspaces.ui.dialog.is_visible(),"visible_dialog":w.window.visible_dialog().map(|dialog|serde_json::json!({"type":dialog.type_().name(),"name":dialog.widget_name().to_string()})),"focus_path":std::iter::successors(focus,|widget|widget.parent()).map(|widget|serde_json::json!({"type":widget.type_().name(),"name":widget.widget_name().to_string(),"mapped":widget.is_mapped()})).collect::<Vec<_>>()})).unwrap()).unwrap();
        };
        escape_snapshot("after");
        if let Err(panic)=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||until(|| !w.workspaces.ui.dialog.is_mapped() && w.workspaces.view().page.is_none(),"Escape closes idle manager"))) {
            escape_snapshot("timeout");
            capture_reference(&w,input.dir.join(format!("lifecycle-escape-timeout-{name}.png")).to_str().unwrap(),1.);
            std::panic::resume_unwind(panic);
        }
        workspace_audit_theme(&w,&input,theme);
        let target = DEFAULT_WORKSPACES.iter().find(|(id,_)|Some(*id)!=manager.active_id().as_deref()).unwrap().0;
        click(&w,&mut input,&row(&w,target));
        until(|| w.workspaces.view().selected.as_deref()==Some(target) && !w.workspaces.view().loading,"selection preview ready before Escape");
        input.key(0xff1b);
        until(|| !w.workspaces.ui.dialog.is_mapped() && !w.workspaces.busy(),"Escape closes selected preview and completes rollback");
        workspace_audit_theme(&w,&input,theme);
        let ready=input.dir.join(format!("lifecycle-lock-{name}"));
        let database=crate::storage::workspaces().unwrap().to_path_buf().join("workspaces.sqlite3");
        let mut lock=std::process::Command::new("python3").arg("-c").arg("import pathlib,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('BEGIN IMMEDIATE'); pathlib.Path(sys.argv[2]).touch(); sys.stdin.readline(); c.rollback()").arg(database).arg(&ready).stdin(std::process::Stdio::piped()).spawn().unwrap();
        until(||ready.exists(),"lifecycle storage lock acquired");
        click(&w,&mut input,add.upcast_ref());
        until(|| find_named(w.window.upcast_ref(),"workspace-item-name").is_some_and(|entry|entry.is_mapped()),"pending New name field");
        named::<gtk::Entry>(w.window.upcast_ref(),"workspace-item-name").set_text(&format!("Pending lifecycle {name}"));
        let confirm=w.workspaces.view().prompt.unwrap().confirm;
        click(&w,&mut input,dialog_button(&w,&confirm).unwrap().upcast_ref());
        assert!(w.workspaces.busy());
        input.key(0xff1b);
        let closed=!w.workspaces.ui.dialog.is_mapped();
        lock.stdin.take().unwrap().write_all(b"release\n").unwrap();
        assert!(lock.wait().unwrap().success());
        assert!(closed,"Escape closes manager during pending adoption");
        until(|| !w.workspaces.busy() && !w.workspaces.view().loading,"pending adoption finishes after dismissal");
        assert!(!w.workspaces.ui.dialog.is_mapped(),"async completion cannot reopen dismissed manager");
        assert!(w.workspaces.view().page.is_none());
    }
    input.finish();w.window.close();
}

#[test]
#[ignore = "private GTK input and storage; --native-test=native_workspace_inline_name_geometry --native-storage"]
fn native_workspace_inline_name_geometry() {
    let (_app,w,mut input,ids)=workspace_audit_fixture("art.capycanvas.WorkspaceInlineNameGeometry",1);
    let id=&ids[0];
    workspace_audit_theme(&w,&input,Theme::Dark);
    click(&w,&mut input,&row(&w,id));
    until(|| w.workspaces.view().selected.as_deref()==Some(id.as_str()) && !w.workspaces.view().loading,"geometry fixture preview");
    let apply=named::<gtk::Button>(w.window.upcast_ref(),"workspace-manager-apply");
    click(&w,&mut input,apply.upcast_ref());
    until(|| w.workspaces.manager().unwrap().active_id().as_ref()==Some(id) && !w.workspaces.busy() && !w.workspaces.view().loading && !w.workspaces.ui.dialog.is_mapped(),"geometry fixture current workspace");
    let paint=|| {
        let frames=Rc::new(Cell::new(0));
        let clock=w.window.frame_clock().unwrap();
        let handler=clock.connect_after_paint(glib::clone!(#[weak] w, #[strong] frames, move |_| {frames.set(frames.get()+1);if frames.get()<2 {w.window.queue_draw();}}));
        w.window.queue_draw();until(||frames.get()>=2,"inline geometry native frames committed");clock.disconnect(handler);
    };
    let bounds=|widget:&gtk::Widget| {let bounds=widget.compute_bounds(&w.window).unwrap();[bounds.x(),bounds.y(),bounds.width(),bounds.height()]};
    let mut results=Vec::new();
    let mut normal_line_height=0;
    for (font,large) in [("normal",false),("large",true)] {
        let provider=gtk::CssProvider::new();
        if large {provider.load_from_string("window.capy-workspace { --ui-text-size: 18pt; }");gtk::style_context_add_provider_for_display(&w.surface.display(),&provider,gtk::STYLE_PROVIDER_PRIORITY_APPLICATION+1);}
        for (theme,name) in [(Theme::Dark,"dark"),(Theme::Light,"light")] {
            workspace_audit_theme(&w,&input,theme);
            let target=row(&w,id);
            let title=named::<gtk::Label>(&target,&format!("workspace-name-{id}"));
            let entry=named::<gtk::Entry>(&target,&format!("workspace-rename-{id}"));
            let subtitle=descendants::<gtk::Label>(&target).into_iter().find(|label|label.has_css_class("subtitle")).unwrap();
            assert!(subtitle.is_mapped() && !subtitle.text().is_empty(),"current workspace provides visible subtitle");
            paint();
            let reference=descendants::<gtk::Entry>(w.window.upcast_ref()).into_iter().find(|entry|entry.has_css_class("number-entry") && entry.ancestor(gtk::Stack::static_type()).is_some_and(|stack|stack.is_mapped()) && std::iter::successors(entry.parent(),|widget|widget.parent()).any(|widget|widget.has_css_class("dock-panel"))).unwrap();
            let reference_stack=reference.ancestor(gtk::Stack::static_type()).unwrap().downcast::<gtk::Stack>().unwrap();
            let reference_page=reference_stack.visible_child_name();
            reference_stack.set_visible_child_name("entry");
            paint();
            assert!(reference.is_mapped(),"real existing compact numeric input is visible");
            let line_height=title.layout().pixel_size().1;
            if large {assert!(line_height>normal_line_height,"larger case changes actual Pango text metrics");} else {normal_line_height=line_height;}
            let before=[bounds(target.upcast_ref()),bounds(title.upcast_ref()),bounds(subtitle.upcast_ref())];
            let layout=title.layout();
            let offsets=title.layout_offsets();
            let label_cursor=layout.cursor_pos(0).0;
            let unit=gtk::pango::SCALE as f32;
            let label_origin=title.compute_point(&w.window,&gtk::graphene::Point::new(offsets.0 as f32+label_cursor.x() as f32/unit,offsets.1 as f32+label_cursor.y() as f32/unit)).unwrap();
            let label_baseline=title.compute_point(&w.window,&gtk::graphene::Point::new(0.,offsets.1 as f32+layout.baseline() as f32/unit)).unwrap();
            let label_text=[label_origin.x(),label_origin.y(),label_cursor.height() as f32/unit,label_baseline.y()];
            capture_reference(&w,input.dir.join(format!("inline-geometry-{font}-{name}-before.png")).to_str().unwrap(),1.);
            let list=named::<gtk::ListBox>(w.window.upcast_ref(),"workspace-manager-items");
            let scroll=list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
            let list_bounds=bounds(scroll.upcast_ref());
            let mut actions=Vec::new();
            for action in ["new","delete"] {
                let button=named::<gtk::Button>(w.window.upcast_ref(),&format!("workspace-manager-{action}"));
                let rect=bounds(button.upcast_ref());
                assert!((rect[1]-list_bounds[1]-list_bounds[3]-6.).abs()<=0.5,"workspace footer action has 6px gap: {font}/{name}/{action}");
                if large {assert!(rect[3]>=34.,"large text footer action fits naturally");} else {assert!((rect[3]-34.).abs()<=0.5,"normal footer action is34px high");}
                actions.push(serde_json::json!({"action":action,"bounds":rect,"list":list_bounds}));
                input.perform(serde_json::json!([{"point":screen_point(button.upcast_ref(),&w.window,[0.5,0.5])}]));
                paint();
                capture_reference(&w,input.dir.join(format!("inline-geometry-{font}-{name}-{action}-hover.png")).to_str().unwrap(),1.);
            }
            let point=screen_point(title.upcast_ref(),&w.window,[0.5,0.5]);
            input.perform(serde_json::json!([{"point":point},{"down":true},{"down":false},{"down":true},{"down":false}]));
            until(||entry.is_mapped(),"native doubleclick starts inline geometry editor");
            paint();
            let during=[bounds(target.upcast_ref()),bounds(entry.upcast_ref()),bounds(subtitle.upcast_ref())];
            let text=descendant::<gtk::Text>(&entry).unwrap();
            let text_bounds=bounds(text.upcast_ref());
            #[allow(deprecated)]
            let insets=|widget:&gtk::Widget| {let padding=widget.style_context().padding();let border=widget.style_context().border();serde_json::json!({"padding":[padding.left(),padding.top(),padding.right(),padding.bottom()],"border":[border.left(),border.top(),border.right(),border.bottom()],"vertical_measure":widget.measure(gtk::Orientation::Vertical,-1),"baseline":widget.baseline(),"font":widget.pango_context().font_description().map(|font|font.to_string().to_string())})};
            let render=|widget:&gtk::Widget,kind:&str| {
                let snapshot=gtk::Snapshot::new();widget.parent().unwrap().snapshot_child(widget,&snapshot);let node=snapshot.to_node().unwrap();
                std::fs::write(input.dir.join(format!("inline-{font}-{name}-{kind}.node")),node.serialize().as_ref()).unwrap();
                w.window.renderer().unwrap().render_texture(&node,None).save_to_png(input.dir.join(format!("inline-{font}-{name}-{kind}.png"))).unwrap();
            };
            let editor=serde_json::json!({"entry":insets(entry.upcast_ref()),"text":insets(text.upcast_ref()),"text_bounds":text_bounds,"label":insets(title.upcast_ref()),"label_line_height":line_height,"reference":insets(reference.upcast_ref())});
            assert_eq!(editor["entry"]["padding"][0],editor["reference"]["padding"][0],"workspace and existing compact input share horizontal inset");
            render(entry.upcast_ref(),"workspace-input");render(reference.upcast_ref(),"reference-number-input");
            capture_reference(&w,input.dir.join(format!("inline-geometry-{font}-{name}-during.png")).to_str().unwrap(),1.);
            input.key(0xff50);
            paint();
            assert_eq!(entry.selection_bounds(),None,"Home leaves an unselected insertion cursor");
            let cursor=text.compute_cursor_extents(0).0;
            let origin=text.compute_point(&w.window,&gtk::graphene::Point::new(cursor.x(),cursor.y())).unwrap();
            let text_layout=text.create_pango_layout(Some(entry.text().as_str()));
            let text_cursor=text_layout.cursor_pos(0).0;
            let editor_text=[origin.x(),origin.y(),cursor.height(),origin.y()+(text_layout.baseline()-text_cursor.y()) as f32/unit];
            capture_reference(&w,input.dir.join(format!("inline-geometry-{font}-{name}-cursor.png")).to_str().unwrap(),1.);
            input.key(0xff1b);
            until(||!entry.is_mapped() && w.workspaces.view().prompt.is_none(),"Escape restores workspace title");
            paint();
            let after=[bounds(target.upcast_ref()),bounds(title.upcast_ref()),bounds(subtitle.upcast_ref())];
            capture_reference(&w,input.dir.join(format!("inline-geometry-{font}-{name}-after.png")).to_str().unwrap(),1.);
            let stable=[0,2].iter().all(|index|before[*index].iter().zip(during[*index].iter()).all(|(a,b)|(a-b).abs()<=0.5)) && before.iter().flatten().zip(after.iter().flatten()).all(|(a,b)|(a-b).abs()<=0.5) && label_text.iter().zip(editor_text.iter()).all(|(a,b)|(a-b).abs()<=1.);
            let value=serde_json::json!({"font":font,"theme":name,"before":before,"during":during,"after":after,"label_text_origin_height_baseline":label_text,"editor_cursor_origin_height_baseline":editor_text,"editor":editor,"actions":actions,"stable":stable});
            reference_stack.set_visible_child_name(reference_page.as_deref().unwrap());
            std::fs::write(input.dir.join(format!("inline-geometry-{font}-{name}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            results.push(value);
        }
        w.workspaces.ui.close(&w);until(||!w.workspaces.ui.dialog.is_mapped() && !w.workspaces.busy(),"geometry manager closed");
        if large {gtk::style_context_remove_provider_for_display(&w.surface.display(),&provider);}
    }
    std::fs::write(input.dir.join("inline-geometry-results.json"),serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    assert!(results.iter().all(|result|result["stable"]==true),"native inline rename must preserve text origin, baseline, row and subtitle: {results:?}");
    input.finish();w.window.close();
}

#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_rename_pin_capture --native-storage"]
fn native_workspace_rename_pin_capture() {
    use std::io::Write;
    let (_app, w, mut input, ids) = workspace_audit_fixture("art.capycanvas.WorkspaceRenamePinCapture", 1);
    let id = &ids[0];
    let manager = w.workspaces.manager().unwrap();
    let entry = || named::<gtk::Entry>(&row(&w, id), &format!("workspace-rename-{id}"));
    let pin = || named::<gtk::ToggleButton>(&row(&w, id), &format!("workspace-pin-{id}"));
    let path = |widget: gtk::Widget| std::iter::successors(Some(widget), |widget| widget.parent()).collect::<Vec<_>>();
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        workspace_audit_theme(&w, &input, theme);
        click(&w, &mut input, &row(&w, id));
        until(|| w.workspaces.view().selected.as_deref() == Some(id.as_str()) && !w.workspaces.view().loading, "audit custom preview");
        let preview = durable_layout(&state(&w).workspace.layout);
        assert!(row(&w, id).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "audit inline rename");
        let mut name_value = format!("Captured rename {name}");
        entry().set_text(&name_value);
        let pressed = pin();
        let ancestors = path(pressed.clone().upcast());
        let selected = pressed.is_active();
        let revision = w.workspaces.view().switcher_revision;
        let point = screen_point(pressed.upcast_ref(), &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"down":true}]));
        until(|| w.workspaces.view().rows.iter().any(|item| item.id == *id && item.title == name_value) && !w.workspaces.busy() && w.workspaces.view().prompt.is_none(), "held pin contact saves inline name");
        let current = pin();
        let retained = current == pressed;
        let retained_ancestors = path(current.clone().upcast()) == ancestors;
        let describe = |widgets: Vec<gtk::Widget>| widgets.into_iter().map(|widget| serde_json::json!({"type":widget.type_().name(),"name":widget.widget_name().to_string(),"mapped":widget.is_mapped(),"rooted":widget.root().is_some()})).collect::<Vec<_>>();
        std::fs::write(input.dir.join(format!("rename-pin-held-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"retained_pin":retained,"retained_ancestors":retained_ancestors,"pressed_path":describe(path(pressed.clone().upcast())),"current_path":describe(path(current.clone().upcast())),"revision_before":revision})).unwrap()).unwrap();
        capture_reference(&w, input.dir.join(format!("rename-pin-held-{name}.png")).to_str().unwrap(), 1.);
        input.perform(serde_json::json!([{"point":point},{"down":false}]));
        if retained && retained_ancestors { until(|| w.workspaces.view().switcher_revision > revision && !w.workspaces.view().switcher_busy, "retained pin release saves toggle"); }
        std::fs::write(input.dir.join(format!("rename-pin-release-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"selected_before":selected,"selected_after":pin().is_active(),"revision_before":revision})).unwrap()).unwrap();
        assert!(retained && retained_ancestors, "{name}: inline name save must retain the pressed pin and every ancestor until release");
        assert_eq!(w.workspaces.view().switcher_revision, revision + 1, "one release produces one pin edit");
        assert_eq!(pin().is_active(), !selected);
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
        assert!(row(&w,id).grab_focus());
        input.key(0xffbf);
        until(||entry().is_mapped(),"pending-release inline rename");
        name_value=format!("Pending captured rename {name}");
        entry().set_text(&name_value);
        let ready=input.dir.join(format!("rename-pin-lock-{name}"));
        let database=crate::storage::workspaces().unwrap().to_path_buf().join("workspaces.sqlite3");
        let mut lock=std::process::Command::new("python3").arg("-c").arg("import pathlib,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('BEGIN IMMEDIATE'); pathlib.Path(sys.argv[2]).touch(); sys.stdin.readline(); c.rollback()").arg(database).arg(&ready).stdin(std::process::Stdio::piped()).spawn().unwrap();
        until(||ready.exists(),"pending-release rename storage lock");
        let pressed=pin();
        let ancestors=path(pressed.clone().upcast());
        let selected=pressed.is_active();
        let revision=w.workspaces.view().switcher_revision;
        let point=screen_point(pressed.upcast_ref(),&w.window,[0.5,0.5]);
        input.perform(serde_json::json!([{"point":point},{"down":true}]));
        until(|| w.workspaces.busy() && matches!(w.workspaces.view().prompt_action,Some(ManagerAction::Rename(ref target)) if target==id),"rename write held before pin release");
        let enabled=w.workspaces.view().rows.iter().find(|row|row.id==*id).unwrap().pin.as_ref().unwrap().enabled;
        input.perform(serde_json::json!([{"point":point},{"down":false}]));
        let view=w.workspaces.view();
        let captured=view.busy && view.switcher_busy && view.rows.iter().find(|row|row.id==*id).unwrap().pin.as_ref().unwrap().selected!=selected;
        let retained=pin()==pressed && path(pin().upcast())==ancestors;
        std::fs::write(input.dir.join(format!("rename-pin-before-ack-{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"view":view,"enabled_while_renaming":enabled,"captured_before_ack":captured,"retained":retained,"revision_before":revision})).unwrap()).unwrap();
        capture_reference(&w,input.dir.join(format!("rename-pin-before-ack-{name}.png")).to_str().unwrap(),1.);
        lock.stdin.take().unwrap().write_all(b"release\n").unwrap();
        assert!(lock.wait().unwrap().success());
        assert!(enabled,"pin remains available while Rename is saving");
        assert!(captured,"pin release before rename acknowledgement queues its intent immediately");
        assert!(retained,"pending rename preserves pin and every ancestor");
        until(|| !w.workspaces.busy() && !w.workspaces.view().switcher_busy && w.workspaces.view().prompt.is_none() && w.workspaces.view().rows.iter().any(|row|row.id==*id && row.title==name_value),"pending rename and pin toggle committed");
        assert_eq!(w.workspaces.view().switcher_revision,revision+1,"release before rename ack produces exactly one pin edit");
        assert_eq!(pin().is_active(),!selected);
        assert_eq!(durable_layout(&state(&w).workspace.layout),preview);
        assert!(row(&w, id).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "audit validation rename");
        entry().set_text("");
        input.key(0xff0d);
        until(|| w.workspaces.view().error.is_some() && !w.workspaces.busy(), "empty inline name validation");
        assert!(entry().is_mapped(), "invalid name keeps its inline editor");
        assert_eq!(w.workspaces.view().rows.iter().find(|item| item.id == *id).unwrap().title, name_value);
        let retry = format!("Retried rename {name}");
        entry().set_text(&retry);
        input.key(0xff0d);
        until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy() && w.workspaces.view().rows.iter().any(|item| item.id == *id && item.title == retry), "corrected inline name saved");
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    }
    assert_eq!(manager.active_id().as_deref(), Some(DEFAULT_WORKSPACES[1].0));
    input.finish();
    w.window.close();
}

#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_swipe_dismissal --native-storage"]
fn native_workspace_swipe_dismissal() {
    let (_app, w, mut input, ids) = workspace_audit_fixture("art.capycanvas.WorkspaceSwipeDismissal", 5);
    let id = &ids[0];
    let [_, i, f] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let swipe = || descendant::<crate::swipe_row::SwipeRow>(&row(&w, id)).unwrap();
    let pins = w.workspaces.manager().unwrap().switcher_ids();
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        workspace_audit_theme(&w, &input, theme);
        let search = named::<gtk::SearchEntry>(w.window.upcast_ref(), "workspace-manager-search");
        assert!(!gtk::prelude::WidgetExt::is_visible(&search), "Workspaces has no search bar");
        let click_trace = Rc::new(RefCell::new(Vec::new()));
        let click_observers = workspace_audit_click_observers(&row(&w,&f),click_trace.clone());
        let photo_point = screen_point(&row(&w, &f), &w.window, [0.5, 0.5]);
        let audit_output = input.dir.clone();
        let photo_snapshot = |phase: &str| {
            let list = named::<gtk::ListBox>(w.window.upcast_ref(), "workspace-manager-items");
            let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
            let path = |start: Option<gtk::Widget>| std::iter::successors(start, |widget| widget.parent()).map(|widget| serde_json::json!({"name":widget.widget_name().to_string(),"type":widget.type_().name(),"mapped":widget.is_mapped()})).collect::<Vec<_>>();
            let swipes: Vec<_> = descendants::<crate::swipe_row::SwipeRow>(w.workspaces.ui.dialog.upcast_ref()).into_iter().map(|swipe| serde_json::json!({"row":swipe.parent().map(|row|row.widget_name().to_string()),"open":swipe.is_open(),"at_rest":swipe.is_at_rest(),"progress":swipe.swipe_progress(),"content_x":find_css(swipe.upcast_ref(),"workspace-row-content").and_then(|content|content.compute_bounds(&swipe)).map(|bounds|bounds.x())})).collect();
            let popups: Vec<_> = descendants::<gtk::Popover>(w.window.upcast_ref()).into_iter().filter(|popup| popup.is_mapped()).map(|popup|serde_json::json!({"name":popup.widget_name().to_string(),"type":popup.type_().name()})).collect();
            let frame = w.workspaces.ui.dialog.child().unwrap().compute_bounds(&w.window).unwrap();
            let value = serde_json::json!({"phase":phase,"accepts_input":w.workspaces.accepts_input(&w),"lease_valid":w.workspaces.manager().unwrap().lease_valid(now_ms()),"click_trace":*click_trace.borrow(),"theme":format!("{:?}",state(&w).theme),"view":w.workspaces.view(),"native_selected":list.selected_row().map(|row|row.widget_name().to_string()),"actual":durable_layout(&state(&w).workspace.layout),"expected":layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk),"frame":[frame.x(),frame.y(),frame.width(),frame.height()],"point":photo_point,"pick_path":path(w.window.pick(photo_point[0] as f64,photo_point[1] as f64,gtk::PickFlags::DEFAULT)),"root_focus_path":path(gtk::prelude::RootExt::focus(&w.window)),"modal_dialog":w.window.visible_dialog().map(|dialog|dialog.widget_name().to_string()),"mapped_popovers":popups,"scroll":{"value":scroll.vadjustment().value(),"upper":scroll.vadjustment().upper(),"page_size":scroll.vadjustment().page_size()},"swipes":swipes,"search":{"text":search.text().to_string(),"mapped":search.is_mapped(),"visible":gtk::prelude::WidgetExt::is_visible(&search)}});
            std::fs::write(audit_output.join(format!("photo-{phase}-{name}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        };
        photo_snapshot("before-click");
        input.click(photo_point);
        photo_snapshot("after-click");
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| until(|| !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk), "audit Photo preview"))) {
            photo_snapshot("timeout");
            capture_reference(&w,input.dir.join(format!("photo-timeout-{name}.png")).to_str().unwrap(),1.);
            std::panic::resume_unwind(panic);
        }
        for target in [&f, id] {
            let handle = find_named(&row(&w, target), &format!("workspace-reorder-handle-{target}")).unwrap();
            assert!(handle.grab_focus());
            input.key(0xff67);
            let context = named::<gtk::Popover>(&row(&w, target), &format!("workspace-context-{target}"));
            assert!(context.is_visible());
            for label in ["Rename…", "Delete…"] {
                let action = find_button(context.upcast_ref(), label);
                assert_eq!(action.is_some(), target == id, "metadata menu availability: {target}/{label}");
                if let Some(action) = action { assert!(action.is_sensitive()); }
            }
            input.key(0xff1b);
        }
        let mut results = Vec::new();
        for mode in ["row", "footer", "capture-loss"] {
            swipe().reset();
            until(|| find_named(w.window.upcast_ref(), &format!("workspace-row-{f}")).is_some_and(|row| row.is_mapped() && row.width() > 0 && row.height() > 0), "audit Photo row allocated");
            click(&w, &mut input, &row(&w, &f));
            until(|| w.workspaces.view().selected.as_deref() == Some(f.as_str()) && !w.workspaces.view().loading, "audit Photo selected");
            let point = screen_point(&row(&w, id), &w.window, [0.4, 0.5]);
            input.perform(serde_json::json!([contact("touch", "down", point),contact("touch", "move", [point[0]-18.,point[1]]),contact("touch", "move", [point[0]-65.,point[1]])]));
            assert!(swipe().is_open());
            if mode != "capture-loss" { input.perform(serde_json::json!([contact("touch", "up", [point[0]-65.,point[1]])])); }
            match mode {
                "row" => {
                    click(&w, &mut input, &row(&w, &i));
                    until(|| w.workspaces.view().selected.as_deref() == Some(i.as_str()) && !w.workspaces.view().loading && durable_layout(&state(&w).workspace.layout) == layer_ui::WorkspacePreset::Illustrator.layout(Platform::Gtk), "outside row click still selects Paint");
                }
                "footer" => {
                    let add = named::<gtk::Button>(w.window.upcast_ref(), "workspace-manager-new");
                    click(&w, &mut input, add.upcast_ref());
                    until(|| matches!(w.workspaces.view().prompt_action, Some(ManagerAction::New)), "outside footer click opens New");
                },
                _ => {
                    w.window.set_visible(false);
                    until(|| !w.window.is_mapped(), "audit private window unmapped");
                    input.perform(serde_json::json!([contact("touch", "up", [point[0]-65.,point[1]])]));
                    w.window.present();
                    until(|| w.window.is_mapped(), "audit private window remapped");
                }
            }
            let closed = swipe().is_at_rest();
            results.push(serde_json::json!({"mode":mode,"closed":closed,"view":w.workspaces.view()}));
            std::fs::write(input.dir.join(format!("swipe-outside-{name}.json")), serde_json::to_vec_pretty(&results).unwrap()).unwrap();
            capture_reference(&w, input.dir.join(format!("swipe-outside-{name}-{mode}.png")).to_str().unwrap(), 1.);
            if mode == "footer" {
                click(&w, &mut input, dialog_button(&w, "Cancel").unwrap().upcast_ref());
                until(|| w.workspaces.view().prompt.is_none() && w.workspaces.accepts_input(&w), "outside footer form cancelled");
            }
            assert_eq!(w.workspaces.manager().unwrap().switcher_ids(), pins);
        }
        let list = named::<gtk::ListBox>(w.window.upcast_ref(), "workspace-manager-items");
        let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        for _ in 0..w.workspaces.view().rows.len()*2 {
            let adjustment=scroll.vadjustment();
            if adjustment.value()+adjustment.page_size()>=adjustment.upper()-0.5 { break; }
            input.perform(serde_json::json!([{"point":screen_point(scroll.upcast_ref(),&w.window,[0.5,0.5])},{"wheel":[0,1]}]));
        }
        let adjustment=scroll.vadjustment();
        assert!(adjustment.upper()>adjustment.page_size(), "audit list genuinely overflows");
        assert!(adjustment.value()+adjustment.page_size()>=adjustment.upper()-0.5,"native wheel reaches last row");
        capture_reference(&w,input.dir.join(format!("no-search-last-row-{name}.png")).to_str().unwrap(),1.);
        std::fs::write(input.dir.join(format!("no-search-last-row-{name}.json")),serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"scroll":{"value":adjustment.value(),"upper":adjustment.upper(),"page_size":adjustment.page_size()},"search_visible":gtk::prelude::WidgetExt::is_visible(&search)})).unwrap()).unwrap();
        std::fs::write(input.dir.join(format!("photo-presses-{name}.json")),serde_json::to_vec_pretty(&*click_trace.borrow()).unwrap()).unwrap();
        for (gesture,handler) in click_observers { gesture.disconnect(handler); }
        assert!(results.iter().all(|result| result["closed"] == true), "{name}: outside controls and capture loss must close the swipe without swallowing their action");
    }
    input.finish();
    w.window.close();
}


#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_inline_actions --native-storage"]
fn native_workspace_inline_actions() {
    let (_app, w, mut input, ids) = workspace_audit_fixture("art.capycanvas.WorkspaceInlineActions", 1);
    let id = &ids[0];
    let entry = || named::<gtk::Entry>(&row(&w, id), &format!("workspace-rename-{id}"));
    let begin = |input: &mut RemoteInput| {
        assert!(row(&w, id).grab_focus());
        input.key(0xffbf);
        until(|| entry().is_mapped(), "inline action editor");
    };
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        workspace_audit_theme(&w, &input, theme);
        click(&w, &mut input, &row(&w, id));
        until(|| w.workspaces.view().selected.as_deref() == Some(id.as_str()) && !w.workspaces.view().loading, "inline action preview");
        let preview = durable_layout(&state(&w).workspace.layout);
        input.key(0xffbf);
        until(|| entry().is_mapped(), "mouse-selected custom row owns F2 without manual focus");
        entry().set_text(&format!("Native text menu {name}"));
        entry().select_region(0, -1);
        let point = screen_point(entry().upcast_ref(), &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
        let context = named::<gtk::Popover>(&row(&w, id), &format!("workspace-context-{id}"));
        let text_menu = descendants::<gtk::Popover>(entry().upcast_ref()).into_iter().find(|popup| popup.is_mapped());
        let native_menu = !context.is_visible() && mapped_label(entry().upcast_ref(), "Copy").is_some() && mapped_label(entry().upcast_ref(), "Paste").is_some();
        std::fs::write(input.dir.join(format!("inline-text-menu-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"native_text_menu":native_menu,"workspace_context_visible":context.is_visible(),"view":w.workspaces.view(),"entry_mapped":entry().is_mapped(),"entry_text":entry().text().to_string()})).unwrap()).unwrap();
        if let Some(popup) = text_menu { capture_popover(popup.upcast_ref(), input.dir.join(format!("inline-text-menu-{name}.png")).to_str().unwrap()); }
        else if context.is_mapped() { capture_popover(context.upcast_ref(), input.dir.join(format!("inline-text-menu-{name}.png")).to_str().unwrap()); }
        input.key(0xff1b);
        until(|| !w.workspaces.busy(), "text menu settled");
        if entry().is_mapped() { entry().grab_focus(); input.key(0xff1b); }
        until(|| w.workspaces.view().prompt.is_none(), "text menu audit editor closed");
        for target in ["new", "delete"] {
            begin(&mut input);
            let valid = format!("Footer {target} {name}");
            entry().set_text(&valid);
            let button = named::<gtk::Button>(w.window.upcast_ref(), if target == "new" { "workspace-manager-new" } else { "workspace-manager-delete" });
            click(&w, &mut input, button.upcast_ref());
            until(|| {
                let view = w.workspaces.view();
                view.rows.iter().any(|row| row.id == *id && row.title == valid)
                    && if target == "new" { matches!(view.prompt_action, Some(ManagerAction::New)) } else { matches!(view.prompt_action, Some(ManagerAction::Delete(ref target)) if target == id) }
                    && dialog_button(&w, "Cancel").is_some_and(|button| button.is_mapped())
            }, "footer action saves inline name and opens requested form");
            assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
            click(&w, &mut input, dialog_button(&w, "Cancel").unwrap().upcast_ref());
            until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy() && !w.workspaces.view().loading, "footer action cancelled");
            begin(&mut input);
            entry().set_text("");
            click(&w, &mut input, button.upcast_ref());
            until(|| {
                let view = w.workspaces.view();
                !view.busy && (view.error.is_some() || !matches!(view.prompt_action, Some(ManagerAction::Rename(ref target)) if target == id))
            }, "invalid inline footer action processed");
            let view = w.workspaces.view();
            let preserved = entry().is_mapped() && entry().text().is_empty() && view.error.is_some() && matches!(view.prompt_action, Some(ManagerAction::Rename(ref target)) if target == id);
            std::fs::write(input.dir.join(format!("inline-invalid-{target}-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"preserved":preserved,"view":view,"entry_mapped":entry().is_mapped(),"entry_text":entry().text().to_string()})).unwrap()).unwrap();
            capture_reference(&w, input.dir.join(format!("inline-invalid-{target}-{name}.png")).to_str().unwrap(), 1.);
            assert!(preserved, "invalid inline name must keep its draft instead of opening {target}");
            entry().set_text(&format!("Recovered footer {target} {name}"));
            entry().grab_focus();
            input.key(0xff0d);
            until(|| w.workspaces.view().prompt.is_none() && !w.workspaces.busy(), "invalid footer name corrected");
        }
        assert!(native_menu, "right-click inside inline name must open native Copy/Paste, not workspace metadata");
    }
    input.finish();
    w.window.close();
}

#[test]
#[ignore = "isolated native input and SQLite; --native-test=native_workspace_rename_apply_capture --native-storage"]
fn native_workspace_rename_apply_capture() {
    let (_app, w, mut input, ids) = workspace_audit_fixture("art.capycanvas.WorkspaceRenameApplyCapture", 2);
    for ((theme, name), id) in [(Theme::Dark, "dark"), (Theme::Light, "light")].into_iter().zip(ids) {
        workspace_audit_theme(&w, &input, theme);
        click(&w, &mut input, &row(&w, &id));
        until(|| w.workspaces.view().selected.as_deref() == Some(id.as_str()) && !w.workspaces.view().loading && w.workspaces.view().enabled, "Apply audit preview ready");
        let preview = durable_layout(&state(&w).workspace.layout);
        assert!(row(&w, &id).grab_focus());
        input.key(0xffbf);
        let entry = named::<gtk::Entry>(&row(&w, &id), &format!("workspace-rename-{id}"));
        until(|| entry.is_mapped(), "Apply audit inline editor");
        let title = format!("Applied captured rename {name}");
        entry.set_text(&title);
        let apply = find_button(w.window.upcast_ref(), "Switch to Workspace").unwrap();
        let disabled = Rc::new(Cell::new(false));
        let handler = apply.connect_sensitive_notify(glib::clone!(#[strong] disabled, move |button| if !button.is_sensitive() { disabled.set(true); }));
        let point = screen_point(apply.upcast_ref(), &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"down":true}]));
        until(|| w.workspaces.view().rows.iter().any(|row| row.id == id && row.title == title) && !w.workspaces.busy(), "held Apply contact saves inline name");
        let lost = disabled.get();
        capture_reference(&w, input.dir.join(format!("rename-apply-held-{name}.png")).to_str().unwrap(), 1.);
        input.perform(serde_json::json!([{"point":point},{"down":false}]));
        if !lost { until(|| w.workspaces.manager().unwrap().active_id().as_deref() == Some(id.as_str()) && !w.workspaces.busy(), "retained Apply release adopts workspace"); }
        std::fs::write(input.dir.join(format!("rename-apply-{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({"disabled_while_pressed":lost,"active":w.workspaces.manager().unwrap().active_id(),"expected_active":id,"view":w.workspaces.view()})).unwrap()).unwrap();
        apply.disconnect(handler);
        assert!(!lost, "inline name save must not disable the already-pressed Apply button");
        assert_eq!(w.workspaces.manager().unwrap().active_id().as_deref(), Some(id.as_str()));
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    }
    input.finish();
    w.window.close();
}


fn row(w: &Workspace, id: &str) -> gtk::Widget {
    find_named(w.window.upcast_ref(), &format!("workspace-row-{id}")).unwrap()
}
fn dialog_button(w: &Workspace, label: &str) -> Option<gtk::Button> {
    w.window
        .visible_dialog()
        .and_then(|dialog| find_button(dialog.upcast_ref(), label))
}
fn click(w: &Workspace, input: &mut RemoteInput, widget: &gtk::Widget) {
    input.click(screen_point(widget, &w.window, [0.5, 0.5]));
}
fn switcher_buttons(w: &Workspace) -> Vec<gtk::ToggleButton> {
    descendants::<gtk::ToggleButton>(w.workspaces.switcher.upcast_ref())
        .into_iter().filter(|button| button.widget_name().starts_with("workspace-switch-")).collect()
}
fn switcher_names(w: &Workspace) -> Vec<String> {
    switcher_buttons(w)
        .iter()
        .map(|button| button.widget_name().to_string())
        .collect()
}
fn wait_switcher(w: &Workspace) {
    until(|| !w.workspaces.view().switcher_busy, "workspace order settled");
    pump(250);
}

#[test]
#[ignore = "isolated Mutter input and SQLite; GTK_A11Y=test workspace-motion.sh gtk --native-test=native_workspace_switcher_pending_menu --native-storage"]
fn native_workspace_switcher_pending_menu() {
    use std::io::Write;
    let mut input = switcher_input();
    let app = native_test_app("art.capycanvas.WorkspaceSwitcherPendingMenu");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(300);
    input.ready();
    let manager = w.workspaces.manager().unwrap();
    let active = manager.active_id().unwrap();
    let title = w.workspaces.view().switcher_display.iter().find(|row| row.id == active).unwrap().title.clone();
    let original = manager.current().unwrap().capture().unwrap();
    let menu_row = |popup: &gtk::PopoverMenu, label: &str| {
        let mut row = mapped_label(popup.upcast_ref(), label).unwrap();
        while !row.is_focusable() { row = row.parent().unwrap(); }
        row
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for presentation in 0..3 {
            if presentation == 2 {
                w.workspaces.send(&w, layer_workspace::WorkspaceInput::EditSwitcher {
                    edit: layer_workspace::SwitcherEdit::Show { id: active.clone(), visible: false },
                });
                wait_switcher(&w);
            }
            let ready = input.dir.join(format!("preference-lock-{theme:?}-{presentation}"));
            let database = crate::storage::workspaces().unwrap().to_path_buf().join("workspaces.sqlite3");
            let mut lock = std::process::Command::new("python3")
                .arg("-c").arg("import pathlib,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('BEGIN IMMEDIATE'); pathlib.Path(sys.argv[2]).touch(); sys.stdin.readline(); c.rollback()")
                .arg(database).arg(&ready).stdin(std::process::Stdio::piped()).spawn().unwrap();
            until(|| ready.exists(), "preference write lock");
            w.workspaces.send(&w, layer_workspace::WorkspaceInput::EditSwitcher {
                edit: layer_workspace::SwitcherEdit::Show { id: active.clone(), visible: false },
            });
            assert!(w.workspaces.view().switcher_busy);
            let options = named::<gtk::MenuButton>(w.window.upcast_ref(), "workspace-switcher-options");
            let popup = match presentation {
                0 => {
                    click(&w, &mut input, options.upcast_ref());
                    options.popover().unwrap().downcast::<gtk::PopoverMenu>().unwrap()
                }
                1 => {
                    let active_button = switcher_buttons(&w).into_iter().find(|button| button.is_active()).unwrap();
                    let point = screen_point(active_button.upcast_ref(), &w.window, [0.5, 0.5]);
                    input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
                    named::<gtk::PopoverMenu>(w.window.upcast_ref(), "workspace-switcher-options-context")
                }
                _ => {
                    let popup = w.workspaces.switcher_popup(&w);
                    options.set_popover(Some(&popup));
                    popup.popup();
                    pump(200);
                    popup
                }
            };
            if presentation==2 {
                let parent_page=popup.visible_submenu();
                let submenu=menu_row(&popup,"Show in top bar");
                assert!(submenu.is_sensitive(),"pending checklist remains browsable");
                click(&w,&mut input,&submenu);
                until(|| popup.visible_submenu()!=parent_page && mapped_label(popup.upcast_ref(),&title).is_some(),"pending sliding checklist page visible");
            }
            let row=menu_row(&popup,&title);
            assert!(row.is_sensitive(),"pending checkbox retains native focus and capture");
            let difference=unsafe { gtk::ffi::gtk_test_accessible_check_state(row.as_ptr().cast(),gtk::ffi::GTK_ACCESSIBLE_STATE_DISABLED,1i32) };
            let difference:Option<glib::GString>=unsafe { glib::translate::from_glib_full(difference) };
            assert_eq!(difference,None,"pending checkbox exposes disabled accessibility state");
            assert!(w.workspaces.view().switcher_options.sections[0].iter().all(|item| !item.enabled));
            let revision=w.workspaces.view().switcher_revision;
            click(&w,&mut input,&row);
            assert_eq!(w.workspaces.view().switcher_revision,revision,"pending native checkbox cannot submit");
            assert!(popup.is_visible());
            let focus_target=if presentation==2 { row.clone() } else { menu_row(&popup,"Manage Workspaces…") };
            assert!(focus_target.grab_focus());
            let model=popup.menu_model().unwrap();
            let page=popup.visible_submenu();
            lock.stdin.take().unwrap().write_all(b"release\n").unwrap();
            assert!(lock.wait().unwrap().success());
            until(|| !w.workspaces.view().switcher_busy,"preference write completed");
            assert!(popup.is_visible());
            if presentation!=2 { assert_eq!(popup.menu_model().unwrap(),model); }
            assert_eq!(popup.visible_submenu(),page);
            let enabled_row=menu_row(&popup,&title);
            assert_eq!(enabled_row,row);
            assert!(enabled_row.is_sensitive());
            let difference=unsafe { gtk::ffi::gtk_test_accessible_check_state(enabled_row.as_ptr().cast(),gtk::ffi::GTK_ACCESSIBLE_STATE_DISABLED,0i32) };
            let difference:Option<glib::GString>=unsafe { glib::translate::from_glib_full(difference) };
            assert_eq!(difference,None,"acknowledged checkbox clears disabled accessibility state");
            assert!(focus_target.has_focus(),"menu acknowledgement preserves focused native row: {theme:?} presentation{presentation}");
            let checklist=popup.clone();
            for visible in [true,false,true] {
                let clicked=menu_row(&popup,&title);
                let before_model=popup.menu_model().unwrap();
                let before_point=screen_point(&clicked,&w.window,[0.5,0.5]);
                click(&w,&mut input,&mapped_label(checklist.upcast_ref(),&title).unwrap());
                wait_switcher(&w);
                let after=menu_row(&popup,&title);
                let actual=manager.switcher_ids().contains(&active);
                let same_checkbox=clicked==after;
                let same_model=before_model==popup.menu_model().unwrap();
                if actual!=visible || !same_checkbox || !same_model {
                    std::fs::write(input.dir.join(format!("compact-toggle-{theme:?}-{presentation}-{visible}.json")),serde_json::to_vec_pretty(&serde_json::json!({"view":w.workspaces.view(),"expected":visible,"actual":actual,"same_checkbox":same_checkbox,"same_model":same_model,"old_mapped":clicked.is_mapped(),"old_parent":clicked.parent().map(|widget|widget.type_().name()),"before_point":before_point,"after_point":screen_point(&after,&w.window,[0.5,0.5]),"page":popup.visible_submenu().map(|page|page.to_string()),"focus":gtk::prelude::RootExt::focus(&w.window).map(|widget|serde_json::json!({"type":widget.type_().name(),"name":widget.widget_name().to_string()}))})).unwrap()).unwrap();
                    capture_popover(popup.upcast_ref(),input.dir.join(format!("compact-failure-{theme:?}-{presentation}.png")).to_str().unwrap());
                }
                assert!(same_checkbox,"visibility toggle retains native checkbox");
                assert!(same_model,"visibility toggle retains popup model");
                assert!(popup.is_visible());
                assert_eq!(popup.visible_submenu(),page,"visibility toggles retain sliding checklist page");
                assert_eq!(actual,visible);
            }
            assert!(menu_row(&popup,&title).grab_focus());
            for visible in [false,true] {
                input.key(0x20);
                until(|| !w.workspaces.view().switcher_busy && manager.switcher_ids().contains(&active)==visible,"checklist Space toggle saved");
                assert!(popup.is_visible());
                assert_eq!(popup.visible_submenu(),page);
                assert!(menu_row(&popup,&title).has_focus(),"checklist Space preserves native focus: {theme:?} presentation{presentation}");
            }
            capture_popover(checklist.upcast_ref(), input.dir.join(format!("workspace-pending-retained-{theme:?}-{presentation}.png")).to_str().unwrap());
            input.key(0xff1b);
            if popup.is_visible() { input.key(0xff1b); }
            assert!(!popup.is_visible());
            assert!(manager.switcher_ids().contains(&active));
            assert_eq!(manager.active_id().as_ref(), Some(&active));
            assert_eq!(manager.current().unwrap().capture().unwrap(), original);
        }
    }
    input.finish();
    w.window.close();
    pump(200);
}

#[test]
#[ignore = "isolated Mutter mouse/touch driver and SQLite; workspace-motion.sh gtk --native-test=native_workspace_switcher_options"]
fn native_workspace_switcher_options() {
    let mut input = switcher_input();
    let app = native_test_app("art.capycanvas.WorkspaceSwitcherOptions");
    let settings = gtk::Settings::default().unwrap();
    settings.set_gtk_enable_animations(false);
    settings.set_gtk_long_press_time(500);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(300);
    input.ready();
    let manager = w.workspaces.manager().unwrap();
    let active = manager.active_id().unwrap();
    let original = manager.current().unwrap().capture().unwrap();
    let target = DEFAULT_WORKSPACES.iter().find(|(id, _)| *id != active).unwrap().0;
    let target_name = w.workspaces.view().switcher_display.iter().find(|row| row.id == target).unwrap().title.clone();
    let options = || named::<gtk::MenuButton>(w.window.upcast_ref(), "workspace-switcher-options");
    let context = || named::<gtk::PopoverMenu>(w.window.upcast_ref(), "workspace-switcher-options-context");
    let dismiss = |input: &mut RemoteInput| input.key(0xff1b);
    let checklist = |popup: &gtk::PopoverMenu| {
        let model = popup.menu_model().unwrap();
        assert_eq!(model.n_items(), 2);
        assert!(model.item_attribute_value(0, "label", None).is_none());
        let rows = model.item_link(0, gtk::gio::MENU_LINK_SECTION).unwrap();
        assert_eq!(rows.n_items() as usize, manager.items().len());
        assert_eq!(manager.active_id().as_ref(), Some(&active));
        assert_eq!(manager.current().unwrap().capture().unwrap(), original);
        assert!(mapped_label(w.window.upcast_ref(), "Customize Title Bar…").is_none());
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        assert!(options().is_mapped());
        assert_eq!((options().width(), options().height()), (20, 26));
        let choices = w.workspaces.switcher.first_child().unwrap().compute_bounds(&w.window).unwrap();
        let more = options().compute_bounds(&w.window).unwrap();
        assert!((more.x() - choices.x() - choices.width() - 2.).abs() < 0.1);
        let entry = state(&w).workspace.layout.header.entries().find(|entry| entry.item == HeaderItem::Workspaces).unwrap().id;
        assert!(!find_named(w.window.upcast_ref(), &format!("header-item-{entry}")).unwrap().has_css_class("in-bar"));
        let grips: Vec<_> = descendants::<gtk::Image>(w.layer_panel.root.upcast_ref()).into_iter()
            .filter(|image| crate::icons::name(image).as_deref() == Some("layer-grip-symbolic")).collect();
        assert!(!grips.is_empty());
        assert!(grips.iter().all(|image| image.pixel_size() == 16));
        assert_eq!(w.workspaces.switcher.measure(gtk::Orientation::Vertical, -1).1, 36);
        assert_eq!(options().measure(gtk::Orientation::Vertical, -1).1, 26);
        until(|| !ui_session(&w).engine().backend().stats.lock().unwrap().presented.is_empty(), "first canvas presentation");
        crate::capture(&w, input.dir.join(format!("workspace-header-{theme:?}.png")).to_str().unwrap());
        click(&w, &mut input, options().upcast_ref());
        let popup = options().popover().unwrap().downcast::<gtk::PopoverMenu>().unwrap();
        checklist(&popup);
        capture_popover(popup.upcast_ref(), input.dir.join(format!("workspace-options-{theme:?}.png")).to_str().unwrap());
        let target_label = mapped_label(popup.upcast_ref(), &target_name).unwrap();
        let initial_check=target_label.ancestor(gtk::CheckButton::static_type()).unwrap().downcast::<gtk::CheckButton>().unwrap();
        click(&w, &mut input, &target_label);
        until(|| !w.workspaces.view().switcher_busy, "hide workspace");
        assert!(popup.is_visible(), "visibility toggle retains options popup");
        assert!(!manager.switcher_ids().contains(&target.to_string()));
        assert!(!manager.switcher_display_ids().contains(&target.to_string()));
        checklist(&popup);
        let target_label = mapped_label(popup.upcast_ref(), &target_name).unwrap();
        let check = target_label.ancestor(gtk::CheckButton::static_type()).unwrap().downcast::<gtk::CheckButton>().unwrap();
        assert_eq!(check,initial_check,"visibility edits retain the native checkbox");
        click(&w, &mut input, &target_label);
        until(|| !w.workspaces.view().switcher_busy, "restore workspace");
        let restored=manager.switcher_ids().contains(&target.to_string());
        if !restored { capture_popover(popup.upcast_ref(), input.dir.join(format!("visibility-failure-{theme:?}.png")).to_str().unwrap()); }
        assert!(restored);
        assert!(popup.is_visible());
        assert!(check.grab_focus());
        for visible in [false,true] {
            input.key(0x20);
            until(|| !w.workspaces.view().switcher_busy && manager.switcher_ids().contains(&target.to_string())==visible && check.is_active()==visible, "Space updates native and shared visibility");
            assert!(popup.is_visible());
            assert!(check.has_focus(), "visibility acknowledgement preserves keyboard focus");
        }
        capture_popover(popup.upcast_ref(), input.dir.join(format!("workspace-options-retained-{theme:?}.png")).to_str().unwrap());
        dismiss(&mut input);
        assert!(!popup.is_visible());

        let mut points: Vec<_> = switcher_buttons(&w).iter().map(|button| screen_point(button.upcast_ref(), &w.window, [0.5, 0.5])).collect();
        points.push(screen_point(options().upcast_ref(), &w.window, [0.5, 0.5]));
        let bounds = w.workspaces.switcher.compute_bounds(&w.window).unwrap();
        points.push([bounds.x() + bounds.width() - 2., bounds.y() + bounds.height() / 2.]);
        for point in points {
            input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
            let popup = context();
            assert!(popup.is_visible());
            checklist(&popup);
            for visible in [false,true] {
                click(&w, &mut input, &mapped_label(popup.upcast_ref(), &target_name).unwrap());
                until(|| !w.workspaces.view().switcher_busy, "context visibility toggle saved");
                assert!(popup.is_visible(), "context checklist remains open");
                assert_eq!(manager.switcher_ids().contains(&target.to_string()), visible);
            }
            capture_popover(popup.upcast_ref(), input.dir.join(format!("workspace-context-retained-{theme:?}.png")).to_str().unwrap());
            dismiss(&mut input);
            assert!(!popup.is_visible());
        }

        let inactive = switcher_buttons(&w).into_iter().find(|button| !button.is_active()).unwrap();
        let point = screen_point(inactive.upcast_ref(), &w.window, [0.5, 0.5]);
        let devices = if std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.ends_with("-tablet")) {
            &["touch", "pen"][..]
        } else { &["touch"][..] };
        for device in devices {
            let mut hold = vec![contact(device, "down", point)];
            hold.extend((0..10).map(|_| serde_json::json!({})));
            input.perform(serde_json::Value::Array(hold));
            assert!(context().is_visible());
            assert!(!context().is_autohide());
            input.perform(serde_json::json!([contact(device, "up", point)]));
            assert!(context().is_visible());
            assert!(context().is_autohide());
            checklist(&context());
            dismiss(&mut input);
            if *device == "pen" { input.perform(serde_json::json!([{"pen":"leave"}])); }
        }
        input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
        checklist(&context());
        dismiss(&mut input);
        assert!(inactive.grab_focus());
        input.key(0xff67);
        checklist(&context());
        dismiss(&mut input);
        assert!(inactive.has_focus());
        input.perform(serde_json::json!([{"key":0xffe1,"down":true},{"key":0xffc7,"down":true},{"key":0xffc7,"down":false},{"key":0xffe1,"down":false}]));
        assert!(context().is_visible());
        if devices.contains(&"pen") { dismiss(&mut input); }
        else { input.click([40., 30.]); }
        assert!(!context().is_visible());
        assert_eq!(manager.active_id().as_ref(), Some(&active));

        w.dispatch(HeaderAction::Edit { editing: true }.action());
        pump(200);
        let bounds = w.workspaces.switcher.compute_bounds(&w.window).unwrap();
        input.perform(serde_json::json!([{"point":[bounds.x()+bounds.width()/2., bounds.y()+bounds.height()/2.]},{"button":273,"down":true},{"button":273,"down":false}]));
        checklist(&context());
        dismiss(&mut input);
        w.dispatch(HeaderAction::Edit { editing: false }.action());
        pump(200);

        click(&w, &mut input, options().upcast_ref());
        let popup = options().popover().unwrap();
        let manage = mapped_label(popup.upcast_ref(), "Manage Workspaces…").unwrap();
        click(&w, &mut input, &manage);
        assert!(!popup.is_visible());
        let dialog = w.window.visible_dialog().unwrap();
        until(|| !w.workspaces.busy() && !w.workspaces.view().rows.is_empty(), "workspace manager rows");
        pump(200);
        let grips: Vec<_> = descendants::<gtk::Image>(dialog.upcast_ref()).into_iter()
            .filter(|image| crate::icons::name(image).as_deref() == Some("layer-grip-symbolic")).collect();
        assert!(!grips.is_empty());
        assert!(grips.iter().all(|image| image.pixel_size() == 16));
        let full_more: Vec<_> = descendants::<gtk::Image>(dialog.upcast_ref()).into_iter()
            .filter(|image| crate::icons::name(image).as_deref() == Some("layer-more-symbolic")).collect();
        assert!(full_more.is_empty(), "built-in workspace rows omit options dots");
        crate::capture(&w, input.dir.join(format!("workspace-manager-{theme:?}.png")).to_str().unwrap());
        w.workspaces.ui.close(&w);
        pump(200);
    }
    for id in manager.switcher_ids() {
        click(&w, &mut input, options().upcast_ref());
        let popup = options().popover().unwrap();
        let title = w.workspaces.view().switcher_options.sections[0].iter().find(|row|
            matches!(&row.action, Some(UiAction::WorkspaceManager { command: layer_ui::WorkspaceCommand::ShowInSwitcher { id: target, .. } }) if target == &id)).unwrap().label.clone();
        click(&w, &mut input, &mapped_label(popup.upcast_ref(), &title).unwrap());
        until(|| !w.workspaces.view().switcher_busy, "clear workspace pins");
        assert!(popup.is_visible());
        dismiss(&mut input);
    }
    assert!(manager.switcher_ids().is_empty());
    assert_eq!(switcher_buttons(&w).len(), 1);
    assert!(options().is_mapped());
    click(&w, &mut input, options().upcast_ref());
    let popup = options().popover().unwrap().downcast::<gtk::PopoverMenu>().unwrap();
    checklist(&popup);
    assert!(w.workspaces.view().switcher_options.sections[0].iter().all(|row| row.selected == Some(false)));
    dismiss(&mut input);
    input.finish();
    w.window.close();
    pump(200);
}
fn menu_button(widget: &gtk::Widget) -> Option<gtk::MenuButton> {
    descendant(widget)
}
fn drag(
    w: &Workspace,
    input: &mut RemoteInput,
    (id, target): (&str, &str),
    after: bool,
    touch: bool,
    handle: bool,
    hold: bool,
) {
    wait_switcher(w);
    let source = if handle {
        find_named(
            w.window.upcast_ref(),
            &format!("workspace-reorder-handle-{id}"),
        )
        .unwrap()
    } else {
        row(w, id)
    };
    let start = screen_point(&source, &w.window, [if handle { 0.5 } else { 0.35 }, 0.5]);
    let end = screen_point(
        &row(w, target),
        &w.window,
        [0.35, if after { 0.85 } else { 0.15 }],
    );
    let device = if touch { "touch" } else { "mouse" };
    let mut events = vec![contact(device, "down", start)];
    if hold {
        events.extend((0..10).map(|_| serde_json::json!({})));
    }
    for t in [0.2, 0.5, 0.8, 1.] {
        let point = [
            start[0] + (end[0] - start[0]) * t,
            start[1] + (end[1] - start[1]) * t,
        ];
        events.push(contact(device, "move", point));
    }
    events.push(contact(device, "up", end));
    input.perform(serde_json::Value::Array(events));
}

#[test]
#[ignore = "isolated Mutter mouse/touch driver and SQLite; workspace-motion.sh gtk --workspace-switcher"]
fn native_workspace_switcher_input() {
    let mut input = switcher_input();
    let app = native_test_app("art.capycanvas.WorkspaceSwitcherInput");
    let settings = gtk::Settings::default().unwrap();
    settings.set_gtk_enable_animations(false);
    settings.set_gtk_long_press_time(500);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(300);
    let manager = w.workspaces.manager().unwrap();
    let [p, i, f] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let original = manager.current().unwrap().capture().unwrap();
    let document = ui_session(&w)
        .engine()
        .document()
        .clone();
    w.workspaces.ui.show(&w, ManagerPage::Workspaces);
    pump(500);
    input.ready();
    click(&w, &mut input, &row(&w, &f));
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
    input.perform(serde_json::json!([
        {"point":screen_point(&row(&w, &p), &w.window, [0.35, 0.5])},
        {"button":273,"down":true}, {"button":273,"down":false}
    ]));
    assert!(popup.is_visible(), "right-click opens the row menu");
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    input.perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
    assert!(!popup.is_visible());
    // Only touch holds open menus; mouse release remains with the native row.
    for touch in [false, true] {
        let point = screen_point(&row(&w, &p), &w.window, [0.35, 0.5]);
        let mut events = if touch {
            vec![serde_json::json!({"touch":"down","point":point})]
        } else {
            vec![
                serde_json::json!({"point":point}),
                serde_json::json!({"down":true}),
            ]
        };
        events.extend((0..10).map(|_| serde_json::json!({})));
        input.perform(serde_json::Value::Array(events));
        assert_eq!(
            popup.is_visible(),
            touch,
            "only touch holds open the row menu"
        );
        assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
        if touch {
            assert!(!popup.is_autohide(), "hold retains the contact");
        }
        input.perform(serde_json::json!([if touch {
            serde_json::json!({"touch":"up"})
        } else {
            serde_json::json!({"down":false})
        }]));
        assert_eq!(
            popup.is_visible(),
            touch,
            "only touch release retains a menu"
        );
        if !touch {
            // Restore the preview after the native mouse release before testing
            // that touch hold/release preserves it.
            click(&w, &mut input, &row(&w, &f));
        }
        if touch {
            assert!(popup.is_autohide(), "released menu is dismissible");
            input
                .perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
        }
        assert!(!popup.is_visible());
        assert!(
            durable_layout(&state(&w).workspace.layout) == preview,
            "hold release must not change the preview"
        );
    }
    // Dragging any row must preserve the selected Photographer preview.
    drag(&w, &mut input, (&p, &f), true, false, false, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "mouse body drag"
    );
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    assert_eq!(manager.current().unwrap().capture().unwrap(), original);
    drag(&w, &mut input, (&p, &i), false, false, true, false);
    assert_eq!(
        manager.switcher_ids(),
        [p.clone(), i.clone(), f.clone()],
        "mouse handle drag"
    );
    drag(&w, &mut input, (&p, &f), true, true, true, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "touch handle without hold"
    );
    drag(&w, &mut input, (&p, &i), false, true, false, false);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), p.clone()],
        "ordinary touch swipe must not reorder"
    );
    let held_popup = menu_button(&row(&w, &p)).unwrap().popover().unwrap();
    drag(&w, &mut input, (&p, &i), false, true, false, true);
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
    click(&w, &mut input, menu.upcast_ref());
    let popup = menu.popover().unwrap();
    capture_popover(
        &popup,
        input.dir.join("switcher-options.png").to_str().unwrap(),
    );
    click(
        &w,
        &mut input,
        find_button(popup.upcast_ref(), "Move Down")
            .unwrap()
            .upcast_ref(),
    );
    assert_eq!(manager.switcher_ids(), [i.clone(), p.clone(), f.clone()]);
    let pin = find_named(
        &row(&w, &p),
        &format!("workspace-pin-{p}"),
    )
    .unwrap();
    click(&w, &mut input, &pin);
    assert_eq!(manager.switcher_ids(), [i.clone(), f.clone()]);
    assert!(
        find_named(
            w.window.upcast_ref(),
            &format!("workspace-reorder-handle-{p}")
        )
        .is_some()
    );
    drag(&w, &mut input, (&p, &f), true, true, true, false);
    assert_eq!(manager.workspace_ids(), [i.clone(), f.clone(), p.clone()]);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone()],
        "reordering a hidden row never pins it"
    );
    drag(&w, &mut input, (&p, &i), false, false, true, false);
    assert_eq!(manager.workspace_ids(), [p.clone(), i.clone(), f.clone()]);
    assert_eq!(durable_layout(&state(&w).workspace.layout), preview);
    capture_reference(
        &w,
        input.dir.join("switcher-manager.png").to_str().unwrap(),
        1.,
    );
    click(
        &w,
        &mut input,
        dialog_button(&w, "Cancel").unwrap().upcast_ref(),
    );
    assert_eq!(
        durable_layout(&state(&w).workspace.layout),
        *original.history.layout()
    );
    assert_eq!(
        ui_session(&w).engine().document(),
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
    click(&w, &mut input, &row(&w, &f));
    let toggle_pin = |id: &str, input: &mut RemoteInput| {
        wait_switcher(&w);
        let pin = find_named(
            &row(&w, id),
            &format!("workspace-pin-{id}"),
        )
        .unwrap();
        click(&w, input, &pin);
    };
    toggle_pin(&custom, &mut input);
    assert_eq!(
        manager.switcher_ids(),
        [i.clone(), f.clone(), custom.clone()]
    );
    for id in manager.switcher_ids() {
        toggle_pin(&id, &mut input);
    }
    assert!(manager.switcher_ids().is_empty());
    assert!(w.workspaces.switcher.is_visible());
    assert_eq!(switcher_names(&w), ["workspace-switch-illustrator"]);
    assert!(switcher_buttons(&w)[0].is_active());
    // All hidden rows still have grips and can move before being shown again.
    drag(&w, &mut input, (&custom, &p), false, false, true, false);
    assert_eq!(manager.workspace_ids()[0], custom);
    assert!(manager.switcher_ids().is_empty());
    let hidden_menu = menu_button(&row(&w, &custom)).unwrap();
    input.perform(serde_json::json!([
        {"point":screen_point(&row(&w, &custom), &w.window, [0.35, 0.5])},
        {"button":273,"down":true}, {"button":273,"down":false}
    ]));
    assert!(
        hidden_menu.popover().unwrap().is_visible(),
        "hidden rows have context menus too"
    );
    input.perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
    toggle_pin(&custom, &mut input);
    toggle_pin(&p, &mut input);
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
    click(&w, &mut input, menu.upcast_ref());
    let up = find_button(menu.popover().unwrap().upcast_ref(), "Move Up").unwrap();
    up.grab_focus();
    pump(100);
    assert!(up.has_focus() && up.is_sensitive());
    input.perform(serde_json::json!([{"key":32,"down":true},{"key":32,"down":false}]));
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
    drag(&w, &mut input, (&custom, &p), false, true, false, false);
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
    let start = screen_point(&row(&w, &p), &w.window, [0.35, 0.5]);
    let end = screen_point(&row(&w, &custom), &w.window, [0.35, 0.8]);
    input.perform(serde_json::json!([
        { "point":start }, { "down":true },
        { "point":[start[0], start[1]+18.] }, { "point":end }, { "point":end }
    ]));
    assert!(row(&w, &p).has_css_class("workspace-row-dragging"));
    input.perform(serde_json::json!([
        {"key":65307,"down":true}, {"key":65307,"down":false}, {"down":false}
    ]));
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
    capture_reference(
        &w,
        input.dir.join("switcher-custom.png").to_str().unwrap(),
        1.,
    );
    click(
        &w,
        &mut input,
        dialog_button(&w, "Cancel").unwrap().upcast_ref(),
    );
    let saved_order = manager.workspace_ids();
    let custom_button = find_named(
        w.header.root.upcast_ref(),
        &format!("workspace-switch-{custom}"),
    )
    .unwrap();
    click(&w, &mut input, &custom_button);
    until(
        || manager.active_id().as_ref() == Some(&custom),
        "custom workspace switch",
    );
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
    wait_workspaces(&reopened);
    assert_eq!(
        reopened.workspaces.manager().unwrap().switcher_ids(),
        [p, custom.clone()]
    );
    // This fixture may import its unbound legacy settings on reopening. Any
    // new entry follows the saved rows without disturbing their relative order.
    assert!(
        reopened
            .workspaces
            .manager()
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
    input.finish();
    w.window.close();
    pump(500);
    assert!(!w.window.is_visible());
}
