use super::*;
use crate::workspace_service::ManagerInput as Input;
use layer_ui::WorkspaceCommand as Command;
use serde_json::Value;

#[test]
fn another_owner_is_focused_without_claiming_or_switching_workspaces() {
    let mut f = Fixture::new();
    f.ready();
    let original = f.service.manager.active_id();
    let other = layer_workspace::WorkspaceManager::new(
        StoreWorker::shared(&f.directory).unwrap(),
        Platform::Windows,
    );
    let incoming = pollster::block_on(other.initialize(wall())).unwrap();
    let target = incoming.entity.id.clone();
    other.activate(incoming);
    open(&mut f, Command::Manage);
    input(
        &mut f,
        Input::Select {
            id: Some(target.clone()),
        },
    );
    settle(&mut f);
    assert_eq!(view(&f)["apply_label"], "Switch to Window");
    input(&mut f, Input::Apply);
    assert_eq!(view(&f)["focus_owner"], other.owner.id);
    assert_eq!(f.service.manager.active_id(), original);
    input(
        &mut f,
        Input::FocusResult {
            error: Some("Target window unavailable".into()),
        },
    );
    assert!(view(&f)["error"].is_string());
    assert_eq!(f.service.manager.active_id(), original);
    input(&mut f, Input::Select { id: Some(target) });
    settle(&mut f);
    input(&mut f, Input::Apply);
    input(&mut f, Input::FocusResult { error: None });
    assert!(view(&f).is_null());
    assert_eq!(f.service.manager.active_id(), original);
    pollster::block_on(other.close()).unwrap();
    drop(other);
    f.close();
    f.dispose();
}

fn view(f: &Fixture) -> Value {
    serde_json::to_value(f.service.manager_view()).unwrap()
}
fn settle(f: &mut Fixture) {
    f.pump(|f| {
        let v = view(f);
        v.is_null() || (v["busy"] == false && v["loading"] == false)
    });
}
fn open(f: &mut Fixture, command: Command) {
    f.native
        .dispatch(UiAction::WorkspaceManager { command })
        .unwrap();
    f.service.poll(&mut f.native, f.now, wall());
    settle(f);
}
fn input(f: &mut Fixture, command: Input) {
    let id = view(f)["id"].as_u64().unwrap();
    f.service.manager_input(&mut f.native, id, command).unwrap();
    f.service.poll(&mut f.native, f.now, wall());
}
fn layout(f: &mut Fixture, layout: DockLayout) {
    let before = f.native.session.state().revision;
    let change = f
        .native
        .session
        .restore_workspace_layout(layout, "Test arrangement")
        .unwrap();
    f.native.apply_change(before, change);
}
fn release_reads(f: &mut Fixture) {
    f.faults.hold_reads.set(false);
    if let Some(waker) = f.faults.read_waker.borrow_mut().take() {
        waker.wake();
    }
}

#[test]
fn included_workspace_preview_is_temporary_and_switch_restores_saved_edits() {
    let mut f = Fixture::new();
    f.ready();
    let original = f.service.manager.active_id().unwrap();
    assert_eq!(original, "builtin:workspace:illustrator");
    assert_eq!(f.service.status.defaults.len(), 3);
    let target = "builtin:workspace:painter".to_string();
    let saved = pollster::block_on(f.service.manager.load(&target))
        .unwrap()
        .entity
        .capture()
        .unwrap();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 87. })
        .unwrap();
    let before = f.native.session.capture_workspace().unwrap();
    open(&mut f, Command::Manage);
    input(
        &mut f,
        Input::Select {
            id: Some(target.clone()),
        },
    );
    settle(&mut f);
    assert_eq!(
        &f.native.session.state().workspace.layout,
        saved.history.layout()
    );
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    assert_eq!(
        f.service.manager.active_id().as_deref(),
        Some(original.as_str())
    );
    assert!(!f.service.accepts_input(wall()));
    input(&mut f, Input::Cancel);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    assert_eq!(
        &f.native.session.state().workspace.layout,
        before.history.layout()
    );
    open(&mut f, Command::Switch { id: target.clone() });
    f.native
        .dispatch(UiAction::SetBrushSize { value: 42. })
        .unwrap();
    layout(&mut f, DockLayout::default());
    let edited = f.native.session.capture_workspace().unwrap();
    open(&mut f, Command::Switch { id: original });
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    open(&mut f, Command::Switch { id: target });
    let restored = f.native.session.capture_workspace().unwrap();
    assert_eq!(restored.working, edited.working);
    assert_eq!(restored.history.layout(), edited.history.layout());
    assert_eq!(restored.history.undo, edited.history.undo);
    assert_eq!(restored.history.redo, edited.history.redo);
    assert_eq!(restored.history.current, edited.history.current);
    assert_eq!(f.service.status.id, f.service.manager.active_id());
    f.close();
    f.dispose();
}

#[test]
fn late_selection_and_dismissal_cannot_resurrect_a_preview() {
    let mut f = Fixture::new();
    f.ready();
    let first = "builtin:workspace:painter".to_string();
    let second = "builtin:workspace:photographer".to_string();
    let before = f.native.session.capture_workspace().unwrap();
    open(&mut f, Command::Manage);
    f.faults.hold_reads.set(true);
    input(
        &mut f,
        Input::Select {
            id: Some(first.clone()),
        },
    );
    f.pump(|f| f.faults.read_waiting.get());
    let obsolete_wake = f.faults.read_waker.borrow_mut().take().unwrap();
    input(
        &mut f,
        Input::Select {
            id: Some(second.clone()),
        },
    );
    f.faults.hold_reads.set(false);
    obsolete_wake.wake();
    settle(&mut f);
    assert_eq!(view(&f)["selected"], second);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    input(
        &mut f,
        Input::Search {
            query: "nothing matches".into(),
        },
    );
    assert!(view(&f)["selected"].is_null());
    assert_eq!(view(&f)["can_apply"], false);
    assert_eq!(
        &f.native.session.state().workspace.layout,
        before.history.layout()
    );
    input(
        &mut f,
        Input::Search {
            query: String::new(),
        },
    );
    f.faults.hold_reads.set(true);
    f.faults.read_waiting.set(false);
    input(&mut f, Input::Select { id: Some(first) });
    f.pump(|f| f.faults.read_waiting.get());
    let stale_dialog = view(&f)["id"].as_u64().unwrap();
    input(&mut f, Input::Cancel);
    release_reads(&mut f);
    for _ in 0..5 {
        f.service.poll(&mut f.native, f.now, wall());
    }
    assert!(view(&f).is_null());
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    open(&mut f, Command::Manage);
    f.service
        .manager_input(
            &mut f.native,
            stale_dialog,
            Input::Select { id: Some(second) },
        )
        .unwrap();
    assert_eq!(
        view(&f)["selected"].as_str(),
        f.service.manager.active_id().as_deref()
    );
    input(&mut f, Input::Cancel);
    f.close();
    f.dispose();
}

#[test]
fn new_workspace_uses_current_settings_and_switch_restores_each_workspace() {
    let mut f = Fixture::new();
    f.ready();
    let original = f.service.manager.active_id().unwrap();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 73. })
        .unwrap();
    open(&mut f, Command::New);
    assert!(view(&f)["prompt"]["choices"].as_array().unwrap().is_empty());
    input(
        &mut f,
        Input::Submit {
            name: Some("Painting".into()),
            choice: None,
        },
    );
    settle(&mut f);
    let created = f.service.manager.active_id().unwrap();
    assert_ne!(created, original);
    assert_eq!(f.native.session.state().brush.diameter, 73.);
    assert!(
        f.native
            .session
            .capture_workspace()
            .unwrap()
            .history
            .undo
            .is_empty()
    );
    f.native
        .dispatch(UiAction::SetBrushSize { value: 28. })
        .unwrap();
    open(&mut f, Command::Manage);
    assert_eq!(view(&f)["selected"], created);
    assert_eq!(view(&f)["can_apply"], false);
    input(
        &mut f,
        Input::Select {
            id: Some(original.clone()),
        },
    );
    settle(&mut f);
    assert_eq!(
        f.native.session.state().brush.diameter,
        28.,
        "Preview cannot load another workspace's working settings"
    );
    assert_eq!(
        f.service.manager.active_id().as_deref(),
        Some(created.as_str())
    );
    input(&mut f, Input::Apply);
    settle(&mut f);
    assert_eq!(
        f.service.manager.active_id().as_deref(),
        Some(original.as_str())
    );
    assert_eq!(f.native.session.state().brush.diameter, 73.);
    open(&mut f, Command::Switch { id: created });
    assert_eq!(f.native.session.state().brush.diameter, 28.);
    f.close();
    f.dispose();
}

#[test]
fn failed_create_retries_the_same_item_and_keeps_the_outgoing_workspace() {
    let mut f = Fixture::new();
    f.ready();
    let outgoing = f.service.manager.active_id();
    open(&mut f, Command::New);
    f.faults.fail.set(true);
    input(
        &mut f,
        Input::Submit {
            name: Some("Retry creation".into()),
            choice: None,
        },
    );
    settle(&mut f);
    assert!(view(&f)["error"].is_string());
    assert_eq!(view(&f)["can_retry"], true);
    assert_eq!(f.service.manager.active_id(), outgoing);
    assert!(f.service.manager.has_failed_operation());
    f.faults.fail.set(false);
    input(&mut f, Input::Retry);
    settle(&mut f);
    assert!(view(&f).is_null());
    assert_eq!(
        f.service.manager.active_name().as_deref(),
        Some("Retry creation")
    );
    assert_eq!(
        f.service
            .manager
            .items()
            .into_iter()
            .filter(|i| i.metadata.name == "Retry creation")
            .count(),
        1
    );
    f.close();
    f.dispose();
}

#[test]
fn history_selection_is_temporary_and_restore_preserves_current_working_values() {
    let mut f = Fixture::new();
    f.ready();
    layout(&mut f, DockLayout::default());
    f.native
        .dispatch(UiAction::SetBrushSize { value: 91. })
        .unwrap();
    let before = f.native.session.capture_workspace().unwrap();
    open(&mut f, Command::LayoutHistory);
    assert_eq!(view(&f)["selected"], before.history.current);
    assert_eq!(view(&f)["can_apply"], false);
    let revision = view(&f)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["current"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    input(
        &mut f,
        Input::Select {
            id: Some(revision.clone()),
        },
    );
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    input(&mut f, Input::Cancel);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    open(&mut f, Command::LayoutHistory);
    input(
        &mut f,
        Input::Select {
            id: Some(revision.clone()),
        },
    );
    input(&mut f, Input::Apply);
    settle(&mut f);
    let after = f.native.session.capture_workspace().unwrap();
    assert_eq!(after.working, before.working);
    assert_eq!(
        after.history.layout(),
        &before.history.revisions[&revision].layout
    );
    assert_eq!(after.history.undo.len(), before.history.undo.len() + 1);
    f.close();
    f.dispose();
}

#[test]
fn included_workspaces_allow_rename_reject_delete_and_preserve_invalid_name_drafts() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::Manage);
    let builtin = view(&f)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "builtin:workspace:illustrator")
        .unwrap()
        .clone();
    assert_eq!(builtin["rename"], true);
    assert_eq!(builtin["delete"], false);
    let id = "builtin:workspace:illustrator".to_string();
    let dialog = view(&f)["id"].as_u64().unwrap();
    assert!(
        f.service
            .manager_input(&mut f.native, dialog, Input::Delete { id: id.clone() })
            .is_err()
    );
    input(&mut f, Input::Rename { id: id.clone() });
    settle(&mut f);
    input(
        &mut f,
        Input::Submit {
            name: Some("My illustration".into()),
            choice: None,
        },
    );
    settle(&mut f);
    assert_eq!(
        f.service
            .status
            .defaults
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .name,
        "My illustration"
    );
    input(&mut f, Input::Create);
    let dialog = view(&f)["id"].as_u64().unwrap();
    let error = f
        .service
        .manager_input(
            &mut f.native,
            dialog,
            Input::Submit {
                name: Some("   ".into()),
                choice: None,
            },
        )
        .unwrap_err();
    f.service.report_error(&mut f.native, error);
    assert_eq!(view(&f)["prompt"]["name"], "   ");
    assert!(view(&f)["error"].is_string());
    input(&mut f, Input::Back);
    assert!(view(&f)["prompt"].is_null());
    input(&mut f, Input::Cancel);
    assert!(f.service.accepts_input(wall()));
    f.close();
    f.dispose();
}

#[test]
fn reset_brushes_confirms_once_preserves_other_state_and_retries_the_accepted_capture() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 73. })
        .unwrap();
    f.native
        .dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::Eraser,
        })
        .unwrap();
    f.native
        .dispatch(UiAction::SetBrushOpacity { value: 0.35 })
        .unwrap();
    f.native
        .dispatch(UiAction::SetColor {
            rgba: [0.2, 0.4, 0.6, 1.],
        })
        .unwrap();
    let before = f.native.session.capture_workspace().unwrap();
    let document = f.native.session.engine().document().clone();
    assert_eq!(before.working.tools.overrides.len(), 2);
    open(&mut f, Command::ResetBrushes);
    assert_eq!(view(&f)["prompt"]["title"], "Reset All Brushes?");
    input(&mut f, Input::Cancel);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    open(&mut f, Command::New);
    input(
        &mut f,
        Input::Submit {
            name: Some("Keep brush settings".into()),
            choice: None,
        },
    );
    settle(&mut f);
    let copy = f.service.manager.active_id().unwrap();
    open(
        &mut f,
        Command::Switch {
            id: "builtin:workspace:illustrator".into(),
        },
    );
    open(&mut f, Command::ResetBrushes);
    f.faults.fail.set(true);
    input(
        &mut f,
        Input::Submit {
            name: None,
            choice: None,
        },
    );
    settle(&mut f);
    assert!(view(&f)["error"].is_string());
    assert_eq!(view(&f)["can_retry"], true);
    let reset = f.native.session.capture_workspace().unwrap();
    assert!(reset.working.tools.overrides.is_empty());
    assert_eq!(reset.history, before.history);
    assert_eq!(reset.working.colors, before.working.colors);
    assert_eq!(reset.working.preset, before.working.preset);
    assert_eq!(reset.working.canvas_tool, before.working.canvas_tool);
    assert_eq!(f.native.session.engine().document(), &document);
    f.faults.fail.set(false);
    input(&mut f, Input::Retry);
    settle(&mut f);
    assert!(view(&f).is_null());
    assert_eq!(f.native.session.capture_workspace().unwrap(), reset);
    open(&mut f, Command::Switch { id: copy });
    assert_eq!(
        f.native.session.capture_workspace().unwrap().working,
        before.working
    );
    f.close();
    f.dispose();
}

#[test]
fn obsolete_layout_library_commands_have_no_native_dialog_route() {
    let mut f = Fixture::new();
    f.ready();
    for command in [Command::ManageTemplates, Command::SaveAsTemplate] {
        open(&mut f, command);
        assert!(view(&f).is_null());
        assert!(
            f.service
                .status
                .error
                .as_deref()
                .unwrap()
                .contains("Use workspaces")
        );
    }
    f.service.status.error = None;
    f.close();
    f.dispose();
}

#[test]
fn close_drains_a_confirmed_create_queued_behind_autosave_and_a_submitted_rename() {
    for rename in [false, true] {
        let mut f = Fixture::new();
        f.ready();
        f.native
            .dispatch(UiAction::SetBrushSize { value: 73. })
            .unwrap();
        if !rename {
            f.faults.hold.set(true);
            f.service.poll(&mut f.native, f.now, wall());
            f.now += Duration::from_millis(300);
            f.pump(|f| f.faults.waiting.get());
            open(&mut f, Command::New);
        } else {
            open(&mut f, Command::Manage);
            let id = f.service.manager.active_id().unwrap();
            input(&mut f, Input::Rename { id });
            settle(&mut f);
            f.faults.hold.set(true);
        }
        input(
            &mut f,
            Input::Submit {
                name: Some("Confirmed before close".into()),
                choice: None,
            },
        );
        f.pump(|f| f.faults.waiting.get());
        assert!(f.service.ui.has_accepted_write());
        f.service.request_close(&mut f.native);
        assert!(
            f.service.ui.active(),
            "Close must retain the accepted operation"
        );
        f.faults.hold.set(false);
        if let Some(wake) = f.faults.waker.borrow_mut().take() {
            wake.wake();
        }
        f.pump(|f| f.service.status.close_ready);
        assert!(view(&f).is_null());
        assert_eq!(
            f.service.manager.active_name().as_deref(),
            Some("Confirmed before close")
        );
        let stored = pollster::block_on(
            f.service
                .manager
                .load(&f.service.manager.active_id().unwrap()),
        )
        .unwrap();
        assert_eq!(stored.entity.metadata.name, "Confirmed before close");
        assert_eq!(
            stored.entity.capture().unwrap().working,
            f.native.session.workspace_working_state()
        );
        f.dispose();
    }
}

#[test]
fn failed_manager_write_during_close_can_keep_the_window_open() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::New);
    let dialog = view(&f)["id"].as_u64().unwrap();
    f.faults.fail.set(true);
    // Queue the accepted action and close before polling its failure.
    f.service
        .manager_input(
            &mut f.native,
            dialog,
            Input::Submit {
                name: Some("Failed before close".into()),
                choice: None,
            },
        )
        .unwrap();
    f.service.request_close(&mut f.native);
    settle(&mut f);
    assert!(view(&f)["error"].is_string());
    input(&mut f, Input::Cancel);
    assert!(!f.service.status.close_requested);
    assert!(view(&f).is_null());
    assert!(f.service.accepts_input(wall()));
    f.faults.fail.set(false);
    f.close();
    f.dispose();
}
