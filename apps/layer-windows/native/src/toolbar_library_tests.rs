use super::*;
use crate::workspace_service::manager_ui::Page;
use layer_ui::{Panel, PanelConfig};
use layer_workspace::{ItemKind, ManagerAction};

fn submit(f: &mut Fixture, name: Option<&str>, choice: Option<&str>) {
    input(
        f,
        Input::Submit {
            name: name.map(str::to_string),
            choice: choice.map(str::to_string),
        },
    );
    settle(f);
    assert!(view(f)["error"].is_null(), "{:?}", view(f));
}
fn page(f: &mut Fixture, page: Page) {
    input(f, Input::ToolbarPage { page });
    settle(f);
}
fn toolbar(f: &mut Fixture, action: ManagerAction) {
    input(f, Input::Toolbar { action });
    settle(f);
}
fn select_toolbar(f: &mut Fixture, panel: Panel) {
    input(
        f,
        Input::Select {
            id: Some(serde_json::to_string(&panel).unwrap()),
        },
    );
    settle(f);
}
fn named_panel(f: &Fixture, name: &str) -> PanelConfig {
    f.native
        .session
        .state()
        .workspace
        .layout
        .panels
        .iter()
        .find(|p| p.title() == name)
        .unwrap()
        .clone()
}
fn saved(f: &Fixture, name: &str) -> String {
    f.service
        .manager
        .items()
        .iter()
        .find(|i| {
            i.metadata.kind == ItemKind::Toolbar
                && i.metadata.name == name
                && i.metadata.deleted_at_ms.is_none()
        })
        .unwrap()
        .id
        .clone()
}
fn save_source(f: &mut Fixture) -> (PanelConfig, String) {
    let source = f
        .native
        .session
        .state()
        .workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .clone();
    open(f, Command::ManageToolbars);
    assert_eq!(view(f)["page"], "this_workspace");
    select_toolbar(f, source.id);
    toolbar(f, ManagerAction::SaveToolbar(source.id));
    submit(f, Some("Reusable review tools"), None);
    let id = saved(f, "Reusable review tools");
    (source, id)
}
fn restart(f: &mut Fixture) {
    f.close();
    f.service.stop();
    let (notify, notifications) = mpsc::channel();
    f.service = WorkspaceService::new(
        TestStore {
            worker: StoreWorker::shared(&f.directory).unwrap(),
            faults: f.faults.clone(),
        },
        f.directory.clone(),
        move || {
            let _ = notify.send(());
        },
    );
    f.notifications = notifications;
    f.native = NativeHost::new(Platform::Windows).unwrap();
    crate::workspace::initialize(&mut f.native).unwrap();
    f.service.start(wall());
    f.ready();
}
fn assert_copy(source: &PanelConfig, copy: &PanelConfig) {
    assert_ne!(source.id, copy.id);
    assert_eq!(source.tile_style, copy.tile_style);
    assert_eq!(source.hide_tab, copy.hide_tab);
    assert_eq!(
        source.tiles().iter().map(|t| t.control).collect::<Vec<_>>(),
        copy.tiles().iter().map(|t| t.control).collect::<Vec<_>>()
    );
}

#[test]
fn toolbar_visibility_changes_while_manager_is_open_are_saved_with_layout_history() {
    let mut f = Fixture::new();
    f.ready();
    let (source, _) = save_source(&mut f);
    let before = f.native.session.capture_workspace().unwrap();
    toolbar(&mut f, ManagerAction::ShowToolbar(source.id, false));
    assert!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel_group(source.id)
            .is_none()
    );
    assert_eq!(
        f.native
            .session
            .capture_workspace()
            .unwrap()
            .history
            .undo
            .len(),
        before.history.undo.len() + 1
    );
    assert_eq!(view(&f)["apply_label"], "Show");
    assert!(
        view(&f)["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["title"] == source.title() && r["subtitle"] == "Hidden")
    );
    assert_eq!(
        f.service
            .manager
            .current()
            .unwrap()
            .capture()
            .unwrap()
            .history
            .layout(),
        &f.native.session.state().workspace.layout
    );
    input(&mut f, Input::Cancel);
    restart(&mut f);
    assert!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel_group(source.id)
            .is_none()
    );
    open(&mut f, Command::ManageToolbars);
    select_toolbar(&mut f, source.id);
    toolbar(&mut f, ManagerAction::ShowToolbar(source.id, true));
    assert!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel_group(source.id)
            .is_some()
    );
    assert_eq!(view(&f)["apply_label"], "Hide");
    input(&mut f, Input::Cancel);
    f.close();
    f.dispose();
}

#[test]
fn toolbar_library_save_restart_copy_rename_and_delete_preserve_independent_instances() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 67. })
        .unwrap();
    let working = f.native.session.workspace_working_state();
    let (source, id) = save_source(&mut f);
    input(&mut f, Input::Cancel);
    restart(&mut f);
    assert_eq!(saved(&f, "Reusable review tools"), id);
    open(&mut f, Command::NewToolbar { group: None });
    assert_eq!(view(&f)["prompt"]["title"], "New Toolbar");
    assert!(
        view(&f)["prompt"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == id)
    );
    let undo = f
        .native
        .session
        .capture_workspace()
        .unwrap()
        .history
        .undo
        .len();
    submit(&mut f, Some("Independent tools"), Some(&id));
    assert!(view(&f).is_null());
    let copy = named_panel(&f, "Independent tools");
    assert_copy(&source, &copy);
    assert_eq!(
        f.native
            .session
            .capture_workspace()
            .unwrap()
            .history
            .undo
            .len(),
        undo + 1
    );
    assert_eq!(f.native.session.workspace_working_state(), working);
    restart(&mut f);
    assert_eq!(named_panel(&f, "Independent tools"), copy);
    open(&mut f, Command::ManageToolbars);
    page(&mut f, Page::ToolbarLibrary);
    input(
        &mut f,
        Input::Select {
            id: Some(id.clone()),
        },
    );
    settle(&mut f);
    assert_eq!(view(&f)["apply_label"], "Add to Workspace");
    toolbar(&mut f, ManagerAction::Rename(id.clone()));
    submit(&mut f, Some("Renamed saved tools"), None);
    assert_eq!(named_panel(&f, "Independent tools"), copy);
    input(
        &mut f,
        Input::Select {
            id: Some(id.clone()),
        },
    );
    settle(&mut f);
    toolbar(&mut f, ManagerAction::AddToolbar(id.clone()));
    assert_copy(&source, &named_panel(&f, "Renamed saved tools"));
    open(&mut f, Command::ManageToolbars);
    page(&mut f, Page::ToolbarLibrary);
    input(
        &mut f,
        Input::Select {
            id: Some(id.clone()),
        },
    );
    settle(&mut f);
    toolbar(&mut f, ManagerAction::Delete(id.clone()));
    submit(&mut f, None, None);
    assert!(
        view(&f)["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != id)
    );
    input(&mut f, Input::Cancel);
    restart(&mut f);
    assert_eq!(named_panel(&f, "Independent tools"), copy);
    assert!(
        f.service
            .manager
            .items()
            .iter()
            .all(|i| i.id != id || i.metadata.deleted_at_ms.is_some())
    );
    f.close();
    f.dispose();
}

#[test]
fn empty_creation_and_library_replacement_are_single_reversible_layout_changes() {
    let mut f = Fixture::new();
    f.ready();
    let (source, id) = save_source(&mut f);
    input(&mut f, Input::Cancel);
    let group = f
        .native
        .session
        .state()
        .workspace
        .layout
        .panel_group(source.id)
        .unwrap();
    open(&mut f, Command::NewToolbar { group: Some(group) });
    submit(&mut f, Some("Empty review"), Some(""));
    let empty = named_panel(&f, "Empty review");
    assert!(empty.tiles().is_empty());
    let before = f.native.session.capture_workspace().unwrap();
    let placement = f
        .native
        .session
        .state()
        .workspace
        .layout
        .panel_group(empty.id);
    open(&mut f, Command::ManageToolbars);
    select_toolbar(&mut f, empty.id);
    toolbar(&mut f, ManagerAction::ReplaceToolbar(empty.id));
    submit(&mut f, None, Some(&id));
    let replaced = f
        .native
        .session
        .state()
        .workspace
        .layout
        .panel(empty.id)
        .unwrap()
        .clone();
    assert_copy(&source, &replaced);
    assert_eq!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel_group(empty.id),
        placement
    );
    assert_eq!(
        f.native
            .session
            .capture_workspace()
            .unwrap()
            .history
            .undo
            .len(),
        before.history.undo.len() + 1
    );
    f.native
        .dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::UndoWorkspace,
        })
        .unwrap();
    assert_eq!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel(empty.id)
            .unwrap(),
        &empty
    );
    f.native
        .dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::RedoWorkspace,
        })
        .unwrap();
    assert_eq!(
        f.native
            .session
            .state()
            .workspace
            .layout
            .panel(empty.id)
            .unwrap(),
        &replaced
    );
    f.close();
    f.dispose();
}

#[test]
fn toolbar_pages_cancel_late_reads_and_reject_actions_from_an_old_selection() {
    let mut f = Fixture::new();
    f.ready();
    let (source, id) = save_source(&mut f);
    let before = f.native.session.capture_workspace().unwrap();
    page(&mut f, Page::ToolbarLibrary);
    f.faults.hold_reads.set(true);
    input(
        &mut f,
        Input::Select {
            id: Some(id.clone()),
        },
    );
    f.pump(|f| f.faults.read_waiting.get());
    page(&mut f, Page::ThisWorkspace);
    release_reads(&mut f);
    settle(&mut f);
    toolbar(&mut f, ManagerAction::AddToolbar(id.clone()));
    input(&mut f, Input::Rename { id: id.clone() });
    input(&mut f, Input::Delete { id: id.clone() });
    assert!(view(&f)["prompt"].is_null());
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    select_toolbar(&mut f, source.id);
    page(&mut f, Page::ToolbarLibrary);
    toolbar(&mut f, ManagerAction::SaveToolbar(source.id));
    assert!(view(&f)["prompt"].is_null());
    input(&mut f, Input::Cancel);
    open(&mut f, Command::NewToolbar { group: None });
    let epoch = view(&f)["id"].as_u64().unwrap();
    input(&mut f, Input::Cancel);
    f.service
        .manager_input(
            &mut f.native,
            epoch,
            Input::Submit {
                name: Some("Cancelled tools".into()),
                choice: Some(id),
            },
        )
        .unwrap();
    assert_eq!(f.native.session.capture_workspace().unwrap(), before);
    f.close();
    f.dispose();
}

#[test]
fn failed_toolbar_save_retries_without_reinstalling_and_close_drains_the_accepted_copy() {
    for close_while_reading in [false, true] {
        let mut f = Fixture::new();
        f.ready();
        let (source, id) = save_source(&mut f);
        input(&mut f, Input::Cancel);
        open(&mut f, Command::NewToolbar { group: None });
        let initial = f.native.session.state().workspace.layout.panels.len();
        f.faults.hold_reads.set(true);
        input(
            &mut f,
            Input::Submit {
                name: Some("Accepted copy".into()),
                choice: Some(id),
            },
        );
        f.pump(|f| f.faults.read_waiting.get());
        if close_while_reading {
            f.service.request_close(&mut f.native);
            assert!(f.service.ui.has_accepted_write());
        } else {
            f.faults.fail.set(true);
        }
        release_reads(&mut f);
        settle(&mut f);
        if !close_while_reading {
            assert!(view(&f)["error"].is_string());
            assert_eq!(
                f.native.session.state().workspace.layout.panels.len(),
                initial + 1
            );
            let installed = named_panel(&f, "Accepted copy");
            let revision = f
                .native
                .session
                .capture_workspace()
                .unwrap()
                .history
                .current;
            f.faults.fail.set(false);
            input(&mut f, Input::Retry);
            settle(&mut f);
            assert!(view(&f).is_null());
            assert_eq!(named_panel(&f, "Accepted copy"), installed);
            assert_eq!(
                f.native
                    .session
                    .capture_workspace()
                    .unwrap()
                    .history
                    .current,
                revision
            );
            f.close();
        } else {
            f.pump(|f| f.service.status.close_ready);
        }
        assert_eq!(
            f.native.session.state().workspace.layout.panels.len(),
            initial + 1
        );
        assert_copy(&source, &named_panel(&f, "Accepted copy"));
        let stored = pollster::block_on(
            f.service
                .manager
                .load(&f.service.manager.active_id().unwrap()),
        )
        .unwrap();
        assert!(
            stored
                .entity
                .capture()
                .unwrap()
                .history
                .layout()
                .panels
                .iter()
                .any(|p| p.title() == "Accepted copy")
        );
        f.dispose();
    }
}
