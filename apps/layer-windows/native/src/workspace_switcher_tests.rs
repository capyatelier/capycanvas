use super::*;
use layer_workspace::{SwitcherEdit, WorkspaceManager};

fn preferences(f: &mut Fixture) {
    f.pump(|f| !f.service.preferences.busy() && !f.service.refresh_preferences);
}
fn edit(f: &mut Fixture, edit: SwitcherEdit) {
    input(f, Input::EditSwitcher { edit });
    preferences(f);
}
fn stored_order(f: &Fixture) -> (Vec<String>, Vec<String>) {
    let manager = WorkspaceManager::new(
        StoreWorker::shared(&f.directory).unwrap(),
        Platform::Windows,
    );
    pollster::block_on(async {
        manager.refresh().await.unwrap();
        manager.refresh_switcher().await.unwrap();
    });
    (manager.workspace_ids(), manager.switcher_ids())
}

#[test]
fn pins_order_and_current_fallback_preserve_preview_cancel_and_persist() {
    let mut f = Fixture::new();
    f.ready();
    let before = f.native.session.capture_workspace().unwrap();
    let active = f.service.status.id.clone().unwrap();
    let original = f.service.status.order.clone();
    open(&mut f, Command::Manage);
    preferences(&mut f);
    let preview = original[0].clone();
    assert_ne!(preview, active);
    input(
        &mut f,
        Input::Select {
            id: Some(preview.clone()),
        },
    );
    settle(&mut f);
    let preview_layout = f.native.session.state().workspace.layout.clone();
    assert_ne!(&preview_layout, before.history.layout());
    edit(
        &mut f,
        SwitcherEdit::Show {
            id: active.clone(),
            visible: false,
        },
    );
    assert_eq!(f.service.status.order, original);
    assert!(!f.service.status.switcher.iter().any(|s| s.id == active));
    assert_eq!(f.service.status.switcher_display[0].id, active);
    edit(
        &mut f,
        SwitcherEdit::Move {
            id: original[2].clone(),
            before: Some(original[0].clone()),
        },
    );
    assert_eq!(view(&f)["rows"][0]["id"], original[2]);
    assert_eq!(view(&f)["selected"], preview);
    assert_eq!(f.native.session.state().workspace.layout, preview_layout);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    let saved = (
        f.service.status.order.clone(),
        f.service.manager.switcher_ids(),
    );
    input(&mut f, Input::Cancel);
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    assert_eq!(
        layer_ui::durable_layout(&f.native.session.state().workspace.layout),
        *before.history.layout()
    );
    assert_eq!(stored_order(&f), saved);

    open(
        &mut f,
        Command::Switch {
            id: preview.clone(),
        },
    );
    assert_eq!(f.service.status.id.as_ref(), Some(&preview));
    assert!(
        !f.service
            .status
            .switcher_display
            .iter()
            .any(|s| s.id == active)
    );
    open(&mut f, Command::Manage);
    preferences(&mut f);
    for id in original {
        edit(&mut f, SwitcherEdit::Show { id, visible: false });
    }
    assert!(f.service.status.switcher.is_empty());
    assert_eq!(f.service.status.switcher_display.len(), 1);
    assert_eq!(f.service.status.switcher_display[0].id, preview);
    input(&mut f, Input::Cancel);
    f.close();
    assert!(stored_order(&f).1.is_empty());
    f.dispose();
}

#[test]
fn cancel_keeps_preference_write_alive_and_close_waits_for_its_completion() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::Manage);
    preferences(&mut f);
    let active = f.service.status.id.clone().unwrap();
    f.faults.hold.set(true);
    input(
        &mut f,
        Input::EditSwitcher {
            edit: SwitcherEdit::Show {
                id: active.clone(),
                visible: false,
            },
        },
    );
    f.pump(|f| f.faults.waiting.get());
    input(&mut f, Input::Cancel);
    assert!(view(&f).is_null());
    assert!(f.service.preferences.busy());
    assert!(f.service.accepts_input(wall()));
    f.service.request_close(&mut f.native);
    f.service.poll(&mut f.native, f.now, wall());
    assert!(!f.service.status.close_ready);
    f.faults.hold.set(false);
    f.faults.waker.borrow_mut().take().unwrap().wake();
    f.pump(|f| f.service.status.close_ready);
    assert!(!stored_order(&f).1.contains(&active));
    f.dispose();
}

#[test]
fn another_windows_preferences_refresh_without_reselecting_or_broadcasting_again() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::Manage);
    preferences(&mut f);
    let ids = f.service.status.order.clone();
    input(
        &mut f,
        Input::Select {
            id: Some(ids[0].clone()),
        },
    );
    settle(&mut f);
    let preview = f.native.session.state().workspace.layout.clone();
    let saved = f.native.session.capture_workspace().unwrap();
    let revision = f.service.status.switcher_revision;
    let other = WorkspaceManager::new(
        StoreWorker::shared(&f.directory).unwrap(),
        Platform::Windows,
    );
    pollster::block_on(other.edit_switcher(SwitcherEdit::Move {
        id: ids[2].clone(),
        before: Some(ids[0].clone()),
    }))
    .unwrap();
    f.service.refresh_switcher();
    preferences(&mut f);
    assert_eq!(view(&f)["selected"], ids[0]);
    assert_eq!(view(&f)["rows"][0]["id"], ids[2]);
    assert_eq!(f.native.session.state().workspace.layout, preview);
    assert_eq!(f.native.session.capture_workspace().unwrap(), saved);
    assert_eq!(f.service.status.switcher_revision, revision);
    input(&mut f, Input::Cancel);
    drop(other);
    f.close();
    f.dispose();
}

#[test]
fn a_focus_refresh_cannot_swallow_an_enabled_menu_action() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::Manage);
    preferences(&mut f);
    let active = f.service.status.id.clone().unwrap();
    f.faults.hold_switcher_reads.set(true);
    f.service.refresh_switcher();
    f.pump(|f| f.faults.switcher_read_waiting.get());
    assert!(f.service.preferences.busy());
    // The UI's enabled action is already in transit when the refresh starts.
    // Leave its future pending, but allow the replacement edit's reads to finish.
    f.faults.hold_switcher_reads.set(false);
    edit(
        &mut f,
        SwitcherEdit::Show {
            id: active.clone(),
            visible: false,
        },
    );
    assert!(!stored_order(&f).1.contains(&active));
    let revision = f.service.status.switcher_revision;
    f.faults
        .switcher_read_waker
        .borrow_mut()
        .take()
        .unwrap()
        .wake();
    f.service.poll(&mut f.native, f.now, wall());
    assert_eq!(f.service.status.switcher_revision, revision);
    assert!(!f.service.preferences.busy());
    input(&mut f, Input::Cancel);
    f.close();
    f.dispose();
}

#[test]
fn failed_pin_write_is_reported_without_disturbing_the_preview_and_can_be_retried() {
    let mut f = Fixture::new();
    f.ready();
    open(&mut f, Command::Manage);
    preferences(&mut f);
    let active = f.service.status.id.clone().unwrap();
    let saved = f.native.session.capture_workspace().unwrap();
    f.faults.fail.set(true);
    edit(
        &mut f,
        SwitcherEdit::Show {
            id: active.clone(),
            visible: false,
        },
    );
    assert!(f.service.status.switcher_error.is_some());
    assert_eq!(view(&f)["selected"], active);
    assert_eq!(f.native.session.capture_workspace().unwrap(), saved);
    f.faults.fail.set(false);
    edit(
        &mut f,
        SwitcherEdit::Show {
            id: active.clone(),
            visible: false,
        },
    );
    assert!(f.service.status.switcher_error.is_none());
    assert!(!stored_order(&f).1.contains(&active));
    input(&mut f, Input::Cancel);
    f.close();
    f.dispose();
}
