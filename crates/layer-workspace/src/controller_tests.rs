use crate::*;
use layer_ui::{Localizer, Platform, UiAction, UiLanguage};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Backend {
    database: RefCell<BrowserDatabase>,
    now: Cell<u64>,
    fail: Cell<bool>,
    fail_preferences: Cell<bool>,
    lose_reply: Cell<bool>,
    deliveries: RefCell<Vec<String>>,
    load_gate: RefCell<Option<async_channel::Receiver<()>>>,
    commit_gate: RefCell<Option<async_channel::Receiver<()>>>,
    switcher_gate: RefCell<Option<async_channel::Receiver<()>>>,
    order_gate: RefCell<Option<async_channel::Receiver<()>>>,
    fail_renew: RefCell<Option<ErrorKind>>,
}
#[derive(Clone)]
struct Store(Rc<Backend>);
impl WorkspaceStore for Store {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        if matches!(request, StoreRequest::UpdateWorkspaceOrder { .. }) {
            let gate = self.0.order_gate.borrow_mut().take();
            if let Some(gate) = gate { let _ = gate.recv().await; }
        }
        if self.0.fail_preferences.get() && matches!(request, StoreRequest::UpdateSwitcher { .. } | StoreRequest::UpdateWorkspaceOrder { .. }) {
            return Err(StoreError::new(ErrorKind::StorageFull, "Storage full"));
        }
        if matches!(request, StoreRequest::Load { .. }) {
            let gate = self.0.load_gate.borrow_mut().take();
            if let Some(gate) = gate {
                let _ = gate.recv().await;
            }
        }
        if matches!(request, StoreRequest::Renew { .. })
            && let Some(kind) = self.0.fail_renew.borrow().clone()
        {
            return Err(StoreError::new(kind, "Renewal failed"));
        }
        if matches!(request, StoreRequest::Switcher) {
            let gate = self.0.switcher_gate.borrow_mut().take();
            if let Some(gate) = gate { let _ = gate.recv().await; }
        }
        let commit = matches!(request, StoreRequest::Commit { .. });
        if commit {
            let gate = self.0.commit_gate.borrow_mut().take();
            if let Some(gate) = gate { let _ = gate.recv().await; }
        }
        if let StoreRequest::Commit { batch } = &request {
            self.0
                .deliveries
                .borrow_mut()
                .push(batch.operation_id.clone());
            self.0.database.borrow_mut().prepare_delivery(batch)?;
        }
        if self.0.fail.get() && (commit || matches!(request, StoreRequest::Receipt { .. })) {
            return Err(StoreError::new(ErrorKind::StorageFull, "Storage full"));
        }
        let result = self
            .0
            .database
            .borrow_mut()
            .execute(request, self.0.now.get());
        if commit && self.0.lose_reply.replace(false) {
            return Err(StoreError::new(
                ErrorKind::Unavailable,
                "Lost acknowledgement",
            ));
        }
        result
    }
}
#[test]
fn workspace_pins_remain_available_during_inline_rename_publication() {
    let mut f = Fixture::new();
    let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
    f.input(serde_json::json!({"type":"switch","id":DEFAULT_WORKSPACES[0].0}));
    f.save();
    let active = f.controller.manager.active_id();
    let order = f.controller.manager.workspace_ids();
    let photo = "builtin:workspace:photographer";
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"select","id":photo}));
    let preview = f.host.session.state().workspace.layout.clone();
    f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
    let (release, gate) = async_channel::bounded(1);
    *f.backend.commit_gate.borrow_mut() = Some(gate);
    f.input(serde_json::json!({"type":"submit","name":"Renamed While Unpinning"}));
    assert!(f.controller.view.busy);
    let pin = f.controller.view.rows.iter().find(|row| row.id == photo).unwrap().pin.clone().unwrap();
    assert!(pin.selected && pin.enabled, "inline Rename publication preserves accepted pin input");
    f.input(serde_json::json!({"type":"edit_switcher","edit":pin.edit}));
    assert!(!f.controller.view.rows.iter().find(|row| row.id == photo).unwrap().pin.as_ref().unwrap().selected);
    assert_eq!(f.host.session.state().workspace.layout, preview);
    release.try_send(()).unwrap();
    f.pump();
    assert!(!f.controller.manager.switcher_ids().iter().any(|id| id == photo));
    assert_eq!(f.controller.manager.items().into_iter().find(|item| item.id == custom).unwrap().metadata.name, "Renamed While Unpinning");
    assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
    assert_eq!(f.host.session.state().workspace.layout, preview);
    assert_eq!(f.controller.manager.active_id(), active);
    assert_eq!(f.controller.manager.workspace_ids(), order);
    assert!(f.controller.view.error.is_none());
}

#[test]
fn workspace_creation_keeps_the_manager_open() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.input(serde_json::json!({"type":"submit","name":"Managed Workspace"}));
    let created = f.controller.manager.active_id().unwrap();
    assert_eq!(f.controller.manager.active_name().as_deref(), Some("Managed Workspace"));
    assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
    assert_eq!(f.controller.view.selected.as_ref(), Some(&created));
    assert!(f.controller.view.rows.iter().any(|row| row.id == created && row.current));
    assert!(f.controller.view.error.is_none());
}

#[test]
fn active_workspace_deletion_keeps_the_manager_open() {
    let mut f = Fixture::new();
    let created = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
    f.input(serde_json::json!({"type":"switch","id":created}));
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"form","action":{"type":"delete","value":created}}));
    f.input(serde_json::json!({"type":"submit","name":""}));
    let fallback = f.controller.manager.active_id().unwrap();
    assert_ne!(fallback, created);
    assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
    assert_eq!(f.controller.view.selected.as_ref(), Some(&fallback));
    assert!(f.controller.view.rows.iter().any(|row| row.id == fallback && row.current));
    assert!(f.controller.manager.items().iter().all(|item| item.id != created));
    assert!(f.controller.view.error.is_none());
}

#[test]
fn workspace_creation_outside_or_dismissed_during_the_manager_stays_closed() {
    for dismiss_pending in [false, true] {
        let mut f = Fixture::new();
        if dismiss_pending { f.input(serde_json::json!({"type":"open","page":"workspaces"})); }
        f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
        let (release, gate) = async_channel::bounded(1);
        if dismiss_pending { *f.backend.commit_gate.borrow_mut() = Some(gate); }
        f.input(serde_json::json!({"type":"submit","name":"Created Outside Manager"}));
        if dismiss_pending {
            assert!(f.controller.view.busy);
            f.input(serde_json::json!({"type":"dismiss"}));
            assert!(f.controller.view.page.is_none());
            release.try_send(()).unwrap();
            f.pump();
        }
        assert_eq!(f.controller.manager.active_name().as_deref(), Some("Created Outside Manager"));
        assert!(f.controller.view.page.is_none());
        assert!(f.controller.view.prompt.is_none());
        assert!(f.controller.view.error.is_none());
    }
}

#[test]
fn invalid_inline_names_preserve_the_prompt_and_preview_before_follow_up_actions() {
    for follow_up in ["new", "delete", "confirm", "select", "open", "switch"] {
        let mut f = Fixture::new();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        f.input(serde_json::json!({"type":"select","id":"builtin:workspace:photographer"}));
        f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
        let prompt = serde_json::to_value(&f.controller.view.prompt).unwrap();
        let preview = f.host.session.state().workspace.layout.clone();
        let selected = f.controller.view.selected.clone();
        let active = f.controller.manager.active_id();
        let order = f.controller.manager.workspace_ids();
        let deliveries = f.backend.deliveries.borrow().len();
        let rejected = f.controller.input(&mut f.host.session,
            serde_json::from_value(serde_json::json!({"type":"submit","name":""})).unwrap(),
            f.backend.now.get());
        assert!(rejected.is_err());
        f.pump();
        assert!(!f.controller.view.busy);
        f.input(match follow_up {
            "new" => serde_json::json!({"type":"form","action":{"type":"new"}}),
            "delete" => serde_json::json!({"type":"action","action":{"type":"delete","value":custom}}),
            "confirm" => serde_json::json!({"type":"confirm"}),
            "select" => serde_json::json!({"type":"select","id":DEFAULT_WORKSPACES[1].0}),
            "open" => serde_json::json!({"type":"open","page":"this_workspace"}),
            "switch" => serde_json::json!({"type":"switch","id":DEFAULT_WORKSPACES[1].0}),
            _ => unreachable!(),
        });
        assert_eq!(f.controller.view.prompt_action, Some(ManagerAction::Rename(custom)), "{follow_up}");
        assert_eq!(serde_json::to_value(&f.controller.view.prompt).unwrap(), prompt, "{follow_up}");
        assert_eq!(f.host.session.state().workspace.layout, preview, "{follow_up}");
        assert_eq!(f.controller.view.selected, selected, "{follow_up}");
        assert_eq!(f.controller.manager.active_id(), active, "{follow_up}");
        assert_eq!(f.controller.manager.workspace_ids(), order, "{follow_up}");
        assert_eq!(f.backend.deliveries.borrow().len(), deliveries, "{follow_up}");
        assert!(!f.controller.view.busy, "{follow_up}");
    }
}

#[test]
fn failed_rename_does_not_execute_queued_workspace_inputs() {
    for follow_up in ["new", "delete", "confirm", "select"] {
        let mut f = Fixture::new();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        f.input(serde_json::json!({"type":"switch","id":DEFAULT_WORKSPACES[0].0}));
        f.save();
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        f.input(serde_json::json!({"type":"select","id":"builtin:workspace:photographer"}));
        let presented = f.host.session.state().workspace.layout.clone();
        f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
        let prompt = serde_json::to_value(&f.controller.view.prompt).unwrap();
        let (release, gate) = async_channel::bounded(1);
        *f.backend.commit_gate.borrow_mut() = Some(gate);
        f.input(serde_json::json!({"type":"submit","name":"Unsaved Rename"}));
        assert!(f.controller.view.busy);
        f.input(match follow_up {
            "new" => serde_json::json!({"type":"form","action":{"type":"new"}}),
            "delete" => serde_json::json!({"type":"action","action":{"type":"delete","value":custom}}),
            "confirm" => serde_json::json!({"type":"confirm"}),
            "select" => serde_json::json!({"type":"select","id":DEFAULT_WORKSPACES[1].0}),
            _ => unreachable!(),
        });
        f.backend.fail.set(true);
        release.try_send(()).unwrap();
        f.pump();
        assert_eq!(f.controller.view.prompt_action, Some(ManagerAction::Rename(custom)));
        assert_eq!(serde_json::to_value(&f.controller.view.prompt).unwrap(), prompt);
        assert!(f.controller.view.error.is_some());
        assert!(f.controller.manager.has_failed_operation());
        assert_eq!(f.host.session.state().workspace.layout, presented);
        assert_eq!(f.controller.view.selected.as_deref(), Some("builtin:workspace:photographer"));
    }
}

#[test]
fn failed_workspace_renames_retry_the_same_operation_without_replacing_the_preview() {
    for rename_active in [false, true] {
        let mut f = Fixture::new();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        let active = if rename_active { custom.as_str() } else { DEFAULT_WORKSPACES[0].0 };
        f.input(serde_json::json!({"type":"switch","id":active}));
        f.save();
        let order = f.controller.manager.workspace_ids();
        let pins = f.controller.manager.switcher_ids();
        let photo = "builtin:workspace:photographer";
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        f.input(serde_json::json!({"type":"select","id":photo}));
        let presented = f.host.session.state().workspace.layout.clone();
        f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
        f.backend.fail.set(true);
        f.input(serde_json::json!({"type":"submit","name":"Retried Rename"}));
        assert!(f.controller.view.error.is_some());
        assert!(f.controller.manager.has_failed_operation());
        let operation = f.backend.deliveries.borrow().last().unwrap().clone();
        let deliveries = f.backend.deliveries.borrow().len();
        assert_eq!(f.host.session.state().workspace.layout, presented);
        if !rename_active {
            let claim = pollster::block_on(f.controller.manager.load(&custom)).unwrap().claim.unwrap();
            f.backend.now.set(f.backend.now.get() + OWNER_RENEW_MS);
            f.pump();
            let renewed = pollster::block_on(f.controller.manager.load(&custom)).unwrap().claim.unwrap();
            assert_eq!(renewed.fence, claim.fence);
            assert!(renewed.expires_at_ms > claim.expires_at_ms, "failed metadata publication retains and renews its original fence");
            assert_eq!(f.host.session.state().workspace.layout, presented);
        }
        f.backend.fail.set(false);
        f.controller.input(&mut f.host.session, WorkspaceInput::Retry, f.backend.now.get()).unwrap();
        assert_eq!(f.host.session.state().workspace.layout, presented);
        for _ in 0..20 {
            f.controller.tick(&mut f.host.session, f.backend.now.get());
            assert_eq!(f.host.session.state().workspace.layout, presented, "rename retry preserves the Photo preview (active={rename_active})");
        }
        assert!(f.controller.view.error.is_none(), "{:?}", f.controller.manager.error());
        assert!(!f.controller.manager.has_failed_operation());
        assert_eq!(&f.backend.deliveries.borrow()[deliveries..], &[operation]);
        assert_eq!(f.controller.manager.active_id().as_deref(), Some(active));
        if rename_active { assert_eq!(f.controller.manager.active_name().as_deref(), Some("Retried Rename")); }
        assert_eq!(f.controller.manager.items().into_iter().find(|item| item.id == custom).unwrap().metadata.name, "Retried Rename");
        assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
        assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
        assert_eq!(f.controller.manager.workspace_ids(), order);
        assert_eq!(f.controller.manager.switcher_ids(), pins);
        if rename_active {
            let mut working = f.host.session.workspace_working_state();
            working.zen_mode = !working.zen_mode;
            f.controller.manager.observe_working(working.clone());
            pollster::block_on(f.controller.manager.flush()).unwrap();
            let saved = pollster::block_on(f.controller.manager.load(&custom)).unwrap();
            assert_eq!(saved.entity.capture().unwrap().working, working);
            assert_eq!(saved.entity.metadata.name, "Retried Rename");
            assert!(!f.controller.manager.dirty());
        }
    }
}

#[test]
fn deleting_an_unselected_workspace_preserves_the_presented_preview_through_completion() {
    for fail_first in [false, true] {
        let mut f = Fixture::new();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        f.input(serde_json::json!({"type":"switch","id":DEFAULT_WORKSPACES[0].0}));
        f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":custom,"visible":true}}));
        let active = f.controller.manager.active_id();
        let mut order = f.controller.manager.workspace_ids();
        let mut pins = f.controller.manager.switcher_ids();
        order.retain(|id| id != &custom);
        pins.retain(|id| id != &custom);
        let photo = "builtin:workspace:photographer";
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        f.input(serde_json::json!({"type":"select","id":photo}));
        let presented = f.host.session.state().workspace.layout.clone();
        f.input(serde_json::json!({"type":"form","action":{"type":"delete","value":custom}}));
        let (release, gate) = async_channel::bounded(1);
        *f.backend.commit_gate.borrow_mut() = Some(gate);
        f.backend.fail.set(fail_first);
        f.controller.input(&mut f.host.session, WorkspaceInput::Submit {
            name: String::new(), description: None, choice: None,
        }, f.backend.now.get()).unwrap();
        assert_eq!(f.host.session.state().workspace.layout, presented, "accepting unrelated Delete preserves the presented preview synchronously");
        for _ in 0..20 {
            f.controller.tick(&mut f.host.session, f.backend.now.get());
            assert_eq!(f.host.session.state().workspace.layout, presented, "pending unrelated Delete preserves the presented preview");
        }
        assert!(f.controller.view.busy);
        release.try_send(()).unwrap();
        for _ in 0..20 {
            f.controller.tick(&mut f.host.session, f.backend.now.get());
            assert_eq!(f.host.session.state().workspace.layout, presented, "completing unrelated Delete preserves the presented preview");
        }
        assert!(!f.controller.view.busy);
        if fail_first {
            assert!(f.controller.view.error.is_some());
            assert!(f.controller.manager.has_failed_operation(), "{:?}", f.controller.manager.error());
            assert!(f.controller.manager.items().iter().any(|item| item.id == custom));
            f.backend.fail.set(false);
            f.controller.input(&mut f.host.session, WorkspaceInput::Retry, f.backend.now.get()).unwrap();
            assert_eq!(f.host.session.state().workspace.layout, presented, "retrying unrelated Delete preserves the presented preview synchronously");
            for _ in 0..20 {
                f.controller.tick(&mut f.host.session, f.backend.now.get());
                assert_eq!(f.host.session.state().workspace.layout, presented, "retrying unrelated Delete preserves the presented preview through completion");
            }
            assert!(!f.controller.view.busy);
            assert!(f.controller.view.error.is_none(), "retry view={:?}; manager={:?}; deliveries={:?}",
                f.controller.view.error, f.controller.manager.error(), f.backend.deliveries.borrow());
        }
        assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
        assert_eq!(f.controller.manager.active_id(), active);
        assert_eq!(f.controller.manager.workspace_ids(), order);
        assert_eq!(f.controller.manager.switcher_ids(), pins);
        assert!(f.controller.manager.items().iter().all(|item| item.id != custom));
        assert!(f.controller.view.error.is_none());
    }
}

#[test]
fn workspace_selection_and_confirmation_wait_for_an_accepted_rename() {
    for confirm in [false, true] {
        let mut f = Fixture::new();
        let active = f.controller.manager.active_id();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        let photo = "builtin:workspace:photographer";
        let expected = pollster::block_on(f.controller.manager.load(photo)).unwrap().entity.capture().unwrap().history.layout().clone();
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        if confirm { f.input(serde_json::json!({"type":"select","id":photo})); }
        let selected = f.controller.view.selected.clone();
        f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
        let (release, gate) = async_channel::bounded(1);
        *f.backend.load_gate.borrow_mut() = Some(gate);
        f.input(serde_json::json!({"type":"submit","name":"Renamed Before Switch"}));
        assert!(f.controller.view.busy);
        f.input(if confirm { serde_json::json!({"type":"confirm"}) }
            else { serde_json::json!({"type":"select","id":photo}) });
        assert!(f.controller.view.busy);
        assert_eq!(f.controller.view.selected, selected);
        release.try_send(()).unwrap();
        f.pump();
        assert_eq!(f.controller.manager.items().into_iter().find(|item| item.id == custom).unwrap().metadata.name, "Renamed Before Switch");
        assert_eq!(f.host.session.state().workspace.layout, expected);
        if confirm {
            assert_eq!(f.controller.manager.active_id().as_deref(), Some(photo));
            assert!(f.controller.view.page.is_none());
        } else {
            assert_eq!(f.controller.manager.active_id(), active);
            assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
            assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
        }
        assert!(f.controller.view.error.is_none());
    }
}

#[test]
fn workspace_metadata_prompts_preserve_settled_and_pending_selected_previews() {
    for pending in [false, true] {
        let mut f = Fixture::new();
        let active = f.controller.manager.active_id();
        let custom = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
        let photo = "builtin:workspace:photographer";
        let expected = pollster::block_on(f.controller.manager.load(photo)).unwrap().entity.capture().unwrap().history.layout().clone();
        let order = f.controller.manager.workspace_ids();
        let pins = f.controller.manager.switcher_ids();
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        let (release, gate) = async_channel::bounded(1);
        if pending { *f.backend.load_gate.borrow_mut() = Some(gate); }
        f.input(serde_json::json!({"type":"select","id":photo}));
        let presented = f.host.session.state().workspace.layout.clone();
        assert_eq!(f.controller.view.loading, pending);
        if !pending { assert_eq!(presented, expected); }
        for action in ["rename", "delete"] {
            f.input(serde_json::json!({"type":"form","action":{"type":action,"value":custom}}));
            assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
            assert_eq!(f.host.session.state().workspace.layout, presented, "opening {action} preserves the presented preview (pending={pending})");
            assert_eq!(f.controller.view.loading, pending, "opening {action} preserves the pending preview");
            f.input(serde_json::json!({"type":"cancel"}));
            assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
            assert_eq!(f.host.session.state().workspace.layout, presented, "cancelling {action} preserves the presented preview");
            assert_eq!(f.controller.view.loading, pending, "cancelling {action} preserves the pending preview");
            assert_eq!(f.controller.manager.active_id(), active);
            assert_eq!(f.controller.manager.workspace_ids(), order);
            assert_eq!(f.controller.manager.switcher_ids(), pins);
        }
        if pending { release.try_send(()).unwrap(); f.pump(); }
        assert!(!f.controller.view.loading);
        assert_eq!(f.host.session.state().workspace.layout, expected);
        f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":custom}}));
        f.input(serde_json::json!({"type":"submit","name":"Renamed Workspace"}));
        assert!(f.controller.view.prompt.is_none());
        assert_eq!(f.controller.view.selected.as_deref(), Some(photo));
        assert_eq!(f.host.session.state().workspace.layout, expected);
        assert_eq!(f.controller.manager.active_id(), active);
        assert_eq!(f.controller.manager.workspace_ids(), order);
        assert_eq!(f.controller.manager.switcher_ids(), pins);
        assert_eq!(f.controller.manager.items().into_iter().find(|item| item.id == custom).unwrap().metadata.name, "Renamed Workspace");
    }
}

#[test]
fn manager_workspace_options_contain_only_available_localized_metadata_actions() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    assert!(f.controller.manager.items().iter().any(|item| !item.metadata.builtin));
    for language in [UiLanguage::English, UiLanguage::Japanese] {
        let localization = Localizer::shared(language);
        f.controller.set_localization(localization.clone());
        f.pump();
        assert!(f.controller.view.intro.is_empty());
        for row in &f.controller.view.rows {
            let item = f.controller.manager.items().into_iter().find(|item| item.id == row.id).unwrap();
            let pin = row.pin.as_ref().unwrap();
            assert!(pin.enabled);
            assert!(matches!(&pin.edit, SwitcherEdit::Toggle { id } if id == &row.id));
            assert_eq!(pin.selected, f.controller.manager.switcher_ids().contains(&row.id));
            if item.metadata.builtin {
                assert!(row.options.is_empty());
            } else {
                assert_eq!(row.options.iter().map(|button| button.action.clone()).collect::<Vec<_>>(),
                    [ManagerAction::Rename(row.id.clone()), ManagerAction::Delete(row.id.clone())]);
                for button in &row.options {
                    assert!(button.enabled && !button.primary);
                    assert_eq!(button.label, localization.text(match button.action {
                        ManagerAction::Rename(_) => layer_ui::MessageId::WORKSPACE_ACTION_RENAME,
                        ManagerAction::Delete(_) => layer_ui::MessageId::WORKSPACE_ACTION_DELETE,
                        _ => unreachable!(),
                    }).as_ref());
                }
            }
        }
    }
    f.input(serde_json::json!({"type":"open","page":"this_workspace"}));
    let row = f.controller.view.rows[0].clone();
    let panel: layer_ui::Panel = serde_json::from_str(&row.id).unwrap();
    f.input(serde_json::json!({"type":"select","id":row.id}));
    f.input(action(ManagerAction::SaveToolbar(panel)));
    f.input(serde_json::json!({"type":"submit","name":"Saved Ink"}));
    for page in ["history", "this_workspace", "toolbar_library"] {
        f.input(serde_json::json!({"type":"open","page":page}));
        assert!(!f.controller.view.rows.is_empty(), "{page}");
        assert!(f.controller.view.rows.iter().all(|row| row.options.is_empty() && row.pin.is_none()), "{page}");
    }
}

#[test]
fn manager_workspace_options_exclude_actions_owned_by_another_window() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"switch","id":DEFAULT_WORKSPACES[0].0}));
    let id = f.controller.manager.items().into_iter().find(|item| !item.metadata.builtin).unwrap().id;
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let claimed = pollster::block_on(other.prepare_switch(&id, f.backend.now.get())).unwrap();
    other.activate(claimed);
    f.input(serde_json::json!({"type":"refresh_switcher"}));
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let row = f.controller.view.rows.iter().find(|row| row.id == id).unwrap();
    assert!(row.options.is_empty());
    assert!(row.pin.as_ref().unwrap().enabled);
    assert!(row.actions.iter().any(|button| button.enabled && matches!(&button.action,
        ManagerAction::SwitchToWindow(target) if target == &id)));
}

#[test]
fn manager_pin_activations_preserve_preview_and_compose_while_preferences_save() {
    let mut f = Fixture::new();
    let active = f.controller.view.switcher[0].id.clone();
    f.input(serde_json::json!({"type":"switch","id":active}));
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let preview_id = f.controller.view.rows.iter().find(|row| row.id != active).unwrap().id.clone();
    f.input(serde_json::json!({"type":"select","id":preview_id}));
    let layout = f.host.session.state().workspace.layout.clone();
    let original = f.controller.manager.current_record().unwrap();
    let order = f.controller.view.order.clone();
    let pin = |f: &Fixture| f.controller.view.rows.iter().find(|row| row.id == active).unwrap().pin.clone().unwrap();
    let before = pin(&f);
    let revision = f.controller.view.switcher_revision;
    assert!(before.selected && before.enabled);
    let (release, gate) = async_channel::bounded(1);
    *f.backend.switcher_gate.borrow_mut() = Some(gate);
    for selected in [false, true, false] {
        f.input(serde_json::json!({"type":"edit_switcher","edit":before.edit}));
        assert_eq!(pin(&f).selected, selected, "each activation updates the requested pin immediately");
        assert!(pin(&f).enabled, "a pending save must not discard the next activation");
        assert!(f.controller.view.switcher.iter().any(|row| row.id == active), "the saved switcher stays confirmed until storage replies");
        assert_eq!(f.controller.view.selected.as_ref(), Some(&preview_id));
        assert_eq!(f.host.session.state().workspace.layout, layout);
    }
    release.try_send(()).unwrap();
    f.pump();
    let after = pin(&f);
    assert_eq!(f.controller.view.switcher_revision, revision + 3);
    assert!(!after.selected && after.enabled);
    assert_ne!(after.icon, before.icon);
    assert_ne!(after.label, before.label);
    assert_eq!(f.controller.view.switcher_display[0].id, active);
    assert_eq!(f.controller.view.selected.as_ref(), Some(&preview_id));
    assert_eq!(f.controller.view.order, order);
    assert_eq!(f.host.session.state().workspace.layout, layout);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    f.backend.fail_preferences.set(true);
    let (release, gate) = async_channel::bounded(1);
    *f.backend.switcher_gate.borrow_mut() = Some(gate);
    for selected in [true, false, true] {
        f.input(serde_json::json!({"type":"edit_switcher","edit":after.edit}));
        assert_eq!(pin(&f).selected, selected);
        assert!(pin(&f).enabled);
    }
    release.try_send(()).unwrap();
    f.pump();
    assert!(f.controller.view.switcher_error.is_some());
    assert!(!pin(&f).selected && pin(&f).enabled);
    assert_eq!(f.controller.view.switcher_revision, revision + 3);
    assert_eq!(f.controller.view.selected.as_ref(), Some(&preview_id));
    f.backend.fail_preferences.set(false);
    f.input(serde_json::json!({"type":"edit_switcher","edit":pin(&f).edit}));
    assert!(pin(&f).selected);
    f.input(serde_json::json!({"type":"cancel"}));
    assert!(f.controller.manager.switcher_ids().contains(&active));
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    f.input(serde_json::json!({"type":"open","page":"history"}));
    assert!(f.controller.view.rows.iter().all(|row| row.pin.is_none()));
}

#[test]
fn switcher_menu_toggles_saved_visibility_without_opening_or_editing_a_workspace() {
    let mut f = Fixture::new();
    let id = f.controller.view.switcher[0].id.clone();
    f.input(serde_json::json!({"type":"switch","id":id}));
    f.controller.observe(&mut f.host.session, f.backend.now.get());
    f.pump();
    let active = f.controller.view.id.clone().unwrap();
    let original = f.controller.manager.current_record().unwrap();
    let layout = f.host.session.state().workspace.layout.clone();
    let order = f.controller.view.order.clone();
    let options = &f.controller.view.switcher_options;
    assert!(f.controller.view.page.is_none());
    assert!(f.controller.view.rows.is_empty());
    let ids: Vec<_> = options.sections[0].iter().map(|item| match item.action.as_ref().unwrap() {
        UiAction::WorkspaceManager { command: layer_ui::WorkspaceCommand::ShowInSwitcher { id, .. } } => id.clone(),
        action => panic!("{action:?}"),
    }).collect();
    assert_eq!(ids, order);
    let index = ids.iter().position(|id| id == &active).unwrap();
    assert_eq!(options.sections[0][index].selected, Some(true));
    let hide = options.sections[0][index].action.clone().unwrap();
    f.action(serde_json::to_value(hide).unwrap());
    f.pump();
    assert_eq!(f.controller.view.switcher_options.sections[0][index].selected, Some(false));
    assert_eq!(f.controller.view.switcher_display[0].id, active);
    assert_eq!(f.controller.view.order, order);
    assert!(f.controller.view.page.is_none());
    assert_eq!(f.host.session.state().workspace.layout, layout);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    assert_eq!(f.controller.view.switcher_menu.sections[0][0].selected, Some(true));
    assert_eq!(f.controller.view.switcher_menu.sections[1][0].sections[0][index].selected, Some(false));

    let show = f.controller.view.switcher_options.sections[0][index].action.clone().unwrap();
    f.action(serde_json::to_value(show).unwrap());
    f.pump();
    assert_eq!(f.controller.view.switcher_options.sections[0][index].selected, Some(true));
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    let manage = f.controller.view.switcher_options.sections.last().unwrap()[0].action.clone().unwrap();
    f.action(serde_json::to_value(manage).unwrap());
    f.pump();
    assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
    assert_eq!(f.controller.view.selected.as_deref(), Some(active.as_str()));
}

#[test]
fn switcher_menu_preserves_confirmed_checks_during_pending_and_failed_writes() {
    let mut f = Fixture::new();
    let id = f.controller.view.switcher[0].id.clone();
    let action = f.controller.view.switcher_options.sections[0][0].action.clone().unwrap();
    let (release, gate) = async_channel::bounded(1);
    *f.backend.switcher_gate.borrow_mut() = Some(gate);
    f.backend.fail_preferences.set(true);
    f.action(serde_json::to_value(action.clone()).unwrap());
    f.pump();
    assert!(f.controller.view.switcher_busy);
    let item = &f.controller.view.switcher_options.sections[0][0];
    assert_eq!(item.selected, Some(true));
    assert!(!item.enabled);
    release.try_send(()).unwrap();
    f.pump();
    assert!(!f.controller.view.switcher_busy);
    assert!(f.controller.view.switcher_error.is_some());
    assert_eq!(f.host.session.state().notice.as_ref().map(|notice| &notice.text), f.controller.view.switcher_error.as_ref());
    let notice = f.host.session.state().notice.clone().unwrap();
    let japanese = Localizer::shared(UiLanguage::Japanese);
    f.host.session.set_localization(japanese.clone());
    f.controller.set_localization(japanese);
    f.pump();
    let translated = f.host.session.state().notice.as_ref().unwrap();
    assert_eq!(translated.id, notice.id);
    assert_ne!(translated.text, notice.text);
    assert_eq!(Some(&translated.text), f.controller.view.switcher_error.as_ref());
    assert_eq!(f.controller.view.switcher_options.sections[0][0].selected, Some(true));
    assert!(f.controller.view.switcher_options.sections[0][0].enabled);
    assert!(f.controller.view.switcher.iter().any(|row| row.id == id));
    f.backend.fail_preferences.set(false);
    f.action(serde_json::to_value(action).unwrap());
    f.pump();
    assert!(f.controller.view.switcher_error.is_none());
    assert_eq!(f.controller.view.switcher_options.sections[0][0].selected, Some(false));
}

#[test]
fn switcher_menu_localizes_chrome_without_changing_custom_names_or_actions() {
    let mut f = Fixture::new();
    let original = f.controller.view.switcher_options.clone();
    let order = f.controller.view.order.clone();
    let japanese = Localizer::shared(UiLanguage::Japanese);
    f.controller.set_localization(japanese.clone());
    let translated = &f.controller.view.switcher_options;
    assert_eq!(translated.title, japanese.text(layer_ui::MessageId::WORKSPACE_SHOW_IN_TOP_BAR).as_ref());
    assert_ne!(translated.title, original.title);
    assert_eq!(f.controller.view.switcher_options_label, japanese.text(layer_ui::MessageId::WORKSPACE_SWITCHER_OPTIONS).as_ref());
    for (a, b) in original.sections.iter().flatten().zip(translated.sections.iter().flatten()) {
        assert_eq!(a.action, b.action);
        assert_eq!(a.selected, b.selected);
    }
    assert_eq!(translated.sections[0].last().unwrap().label, "My Workspace");
    assert_eq!(f.controller.view.order, order);
    assert!(f.controller.view.page.is_none());
}
#[test]
fn controllers_begin_with_independent_languages() {
    use layer_ui::{Localizer, UiLanguage};
    use std::sync::Arc;
    let japanese = Localizer::shared(UiLanguage::Japanese);
    let korean = Localizer::shared(UiLanguage::Korean);
    let first = WorkspaceController::new_localized(Store(Rc::new(Backend::default())), Platform::Gtk, 0, japanese.clone());
    let second = WorkspaceController::new_owned_localized(Store(Rc::new(Backend::default())), Platform::Gtk, Owner::fresh(), 0, korean.clone());
    assert!(Arc::ptr_eq(first.localization(), &japanese));
    assert!(Arc::ptr_eq(second.localization(), &korean));
    assert_eq!(first.localization().language(), UiLanguage::Japanese);
    assert_eq!(second.localization().language(), UiLanguage::Korean);
}

struct Fixture {
    backend: Rc<Backend>,
    controller: WorkspaceController<Store>,
    host: layer_host::NativeHost,
}
impl Fixture {
    fn new() -> Self {
        let backend = Rc::new(Backend::default());
        backend.now.set(1000);
        let mut host = layer_host::NativeHost::new(Platform::Web).unwrap();
        let capture = host.session.capture_workspace().unwrap();
        let baseline = capture.history.layout().clone();
        let entity = Entity::workspace("My Workspace", capture, baseline, 1000);
        let batch = CommitBatch::prepare(
            Owner::fresh(),
            vec![Mutation::Create {
                entity,
                claim: false,
                name_policy: NamePolicy::Unique,
            }],
        )
        .unwrap();
        pollster::block_on(Store(backend.clone()).execute(StoreRequest::Commit { batch })).unwrap();
        let controller = WorkspaceController::new_localized(Store(backend.clone()), Platform::Web, 1000, Localizer::shared(UiLanguage::English));
        let mut f = Self {
            backend,
            controller,
            host,
        };
        f.pump();
        assert!(f.controller.view.ready);
        f
    }
    fn pump(&mut self) {
        for _ in 0..20 {
            self.controller
                .tick(&mut self.host.session, self.backend.now.get());
        }
    }
    fn input(&mut self, json: serde_json::Value) {
        self.controller
            .input(
                &mut self.host.session,
                serde_json::from_value(json).unwrap(),
                self.backend.now.get(),
            )
            .unwrap();
        self.pump();
    }
    fn action(&mut self, json: serde_json::Value) {
        self.host
            .dispatch(serde_json::from_value::<UiAction>(json).unwrap())
            .unwrap();
        self.controller
            .observe(&mut self.host.session, self.backend.now.get());
    }
    fn save(&mut self) {
        self.backend.now.set(self.backend.now.get() + 300);
        self.pump();
    }
}
#[test]
fn restoring_a_bound_window_claims_without_republishing_its_selection() {
    let mut f = Fixture::new();
    let id = f.controller.view.id.clone().unwrap();
    let owner = f.controller.manager.owner.clone();
    let before = f.controller.manager.current_record().unwrap();
    let other_id = DEFAULT_WORKSPACES[1].0;
    assert_ne!(id, other_id);
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let selected = pollster::block_on(other.prepare_switch(other_id, 1500)).unwrap();
    other.activate(selected);
    let deliveries = f.backend.deliveries.borrow().len();
    f.backend.now.set(2000);
    f.controller =
        WorkspaceController::new_owned_localized(Store(f.backend.clone()), Platform::Web, owner.clone(), 2000, Localizer::shared(UiLanguage::English));
    f.pump();
    assert!(f.controller.view.ready, "{:?}", f.controller.view.error);
    assert_eq!(f.controller.view.id.as_deref(), Some(id.as_str()));
    let restored = f.controller.manager.current_record().unwrap();
    assert_eq!(restored.entity, before.entity);
    assert_eq!(restored.generations, before.generations);
    assert_eq!(restored.claim.as_ref().unwrap().owner, owner);
    assert!(restored.claim.as_ref().unwrap().expires_at_ms > before.claim.as_ref().unwrap().expires_at_ms);
    assert_eq!(f.backend.deliveries.borrow().len(), deliveries);
    let binding = f.backend.database.borrow_mut().execute(
        StoreRequest::Binding { key: "last_workspace".into() }, 2000,
    ).unwrap();
    assert!(matches!(binding, StoreResponse::Binding(Some(bound)) if bound == other_id));
    // Explicit selection still updates recency and durably publishes bindings.
    f.backend.now.set(3000);
    let switched = pollster::block_on(f.controller.manager.prepare_switch(&id, 3000)).unwrap();
    assert_eq!(switched.entity.metadata.last_used_ms, 3000);
    assert!(switched.generations.metadata > before.generations.metadata);
    assert!(f.backend.deliveries.borrow().len() > deliveries);
}

#[test]
fn startup_with_an_occupied_window_binding_reuses_an_available_default() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone().unwrap();
    let owner = f.controller.manager.owner.clone();
    f.input(serde_json::json!({"type":"suspend"}));
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let claimed = pollster::block_on(other.prepare_switch(&original, 1_000)).unwrap();
    other.activate(claimed);
    f.controller =
        WorkspaceController::new_owned_localized(Store(f.backend.clone()), Platform::Web, owner, 1_000, Localizer::shared(UiLanguage::English));
    f.pump();
    assert!(f.controller.view.ready, "{:?}", f.controller.view.error);
    assert_eq!(
        f.controller.view.id.as_deref(),
        Some(DEFAULT_WORKSPACES[1].0)
    );
    assert_eq!(f.controller.manager.items().len(), 4);
    assert_eq!(other.active_id().as_deref(), Some(original.as_str()));
    let saved = pollster::block_on(other.load(&original)).unwrap();
    assert_eq!(saved.claim.unwrap().owner, other.owner);
}

#[test]
fn active_deletion_uses_available_defaults_and_persists_the_replacement() {
    let defaults = [
        DEFAULT_WORKSPACES[1].0,
        DEFAULT_WORKSPACES[0].0,
        DEFAULT_WORKSPACES[2].0,
    ];
    for occupied in 0..=defaults.len() {
        let mut f = Fixture::new();
        f.input(serde_json::json!({"type":"switch","id":defaults[0]}));
        f.action(serde_json::json!({"type":"set_brush_size","value":73.0}));
        f.save();
        f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
        f.input(serde_json::json!({"type":"submit","name":"Delete Me"}));
        let deleted = f.controller.view.id.clone().unwrap();
        let capture = f.host.session.capture_workspace().unwrap();
        let count = f.controller.manager.items().len();
        let owner = Owner::fresh();
        for id in &defaults[..occupied] {
            pollster::block_on(Store(f.backend.clone()).execute(StoreRequest::Claim {
                id: (*id).into(),
                owner: owner.clone(),
            }))
            .unwrap();
        }
        f.input(serde_json::json!({"type":"open","page":"workspaces"}));
        f.input(serde_json::json!({"type":"form","action":{"type":"delete","value":deleted}}));
        f.input(serde_json::json!({"type":"cancel"}));
        assert_eq!(f.controller.view.id.as_ref(), Some(&deleted));
        assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
        assert!(f.controller.manager.switcher_ids().contains(&deleted));
        f.input(serde_json::json!({"type":"form","action":{"type":"delete","value":deleted}}));
        f.input(serde_json::json!({"type":"submit","name":""}));
        let record = pollster::block_on(f.controller.manager.load(&deleted));
        if occupied == defaults.len() {
            assert!(f.controller.view.error.is_some());
            assert_eq!(f.controller.view.id.as_ref(), Some(&deleted));
            assert!(record.is_ok());
            assert_eq!(f.controller.manager.items().len(), count, "Deletion never adds a replacement row");
            assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
        } else {
            let replacement = defaults[occupied];
            assert!(
                f.controller.view.error.is_none(),
                "{:?}",
                f.controller.view.error
            );
            assert_eq!(f.controller.view.id.as_deref(), Some(replacement));
            assert_eq!(record.unwrap_err().kind, ErrorKind::NotFound);
            assert!(
                !f.controller
                    .manager
                    .switcher_display_ids()
                    .contains(&deleted)
            );
            assert_eq!(
                f.controller.manager.items().len(),
                count - 1,
                "Deletion must not create a new workspace"
            );
            let saved = pollster::block_on(f.controller.manager.load(replacement))
                .unwrap()
                .entity
                .capture()
                .unwrap();
            let displayed = saved.clone();
            assert_eq!(f.host.session.capture_workspace().unwrap(), displayed);
            if occupied == 0 {
                assert_eq!(saved.working, capture.working);
            }
            f.input(serde_json::json!({"type":"open","page":"workspaces"}));
            assert!(!f.controller.view.rows.iter().any(|r| r.id == deleted));
            f.input(serde_json::json!({"type":"cancel"}));
            f.input(serde_json::json!({"type":"suspend"}));
            let reopened = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
            let incoming = pollster::block_on(reopened.initialize(f.backend.now.get())).unwrap();
            assert_eq!(incoming.entity.id, replacement);
            assert_eq!(incoming.entity.capture().unwrap(), displayed);
        }
        for id in &defaults[..occupied] {
            let saved = pollster::block_on(f.controller.manager.load(id)).unwrap();
            assert_eq!(
                saved.claim.unwrap().owner,
                owner,
                "Deletion must not take another window’s workspace"
            );
        }
    }
}

#[test]
fn starting_layout_dialog_previews_without_saving_and_restore_is_undoable() {
    for builtin in [None, Some(DEFAULT_WORKSPACES[0]), Some(DEFAULT_WORKSPACES[1]), Some(DEFAULT_WORKSPACES[2])] {
        let mut f = Fixture::new();
        let baseline = if let Some((id, preset)) = builtin {
            // Persist an older baseline with user edits, so ordinary adoption must
            // preserve it and only explicit Restore selects the current default.
            let mut old = layer_ui::DockLayout::for_platform(Platform::Web);
            old.header.size = layer_ui::HeaderSize::Large;
            let mut edited = old.clone();
            edited.header.size = layer_ui::HeaderSize::Small;
            let mut history = layer_ui::LayoutHistory::new(&old);
            history.append(&edited, layer_ui::LayoutChange::Automatic);
            assert!(history.undo());
            let mut database: serde_json::Value = serde_json::from_str(&f.backend.database.borrow().encoded().unwrap()).unwrap();
            database["items"][id]["entity"]["content"] = serde_json::to_value(ItemContent::Workspace {
                history, baseline: Box::new(old.clone()),
            }).unwrap();
            *f.backend.database.borrow_mut() = BrowserDatabase::decode(&database.to_string()).unwrap();
            f.input(serde_json::json!({"type":"switch", "id":id}));
            assert_eq!(f.host.session.capture_workspace().unwrap().history.layout(), &old);
            assert!(f.host.session.command(layer_ui::CommandId::ResetLayout).enabled,
                "An unchanged old baseline must still offer the latest default");
            preset.layout(Platform::Web)
        } else {
            f.controller.manager.current().unwrap().starting_layout(Platform::Web, false).unwrap()
        };
        f.action(serde_json::json!({"type":"move_panel","panel":"toolbar","viewport":[1200,900],"target":{"kind":"float","position":[480,220]}}));
        f.action(serde_json::json!({"type":"set_brush_size","value":73}));
        f.save();
        let before = f.host.session.capture_workspace().unwrap();
        let before_layout = f.host.session.state().workspace.layout.clone();
        assert_ne!(before.history.layout(), &baseline);
        for confirm in [false, true] {
            let saved = f.controller.manager.current().unwrap();
            f.input(serde_json::json!({"type":"form","action":{"type":"reset","value":saved.id}}));
            assert_eq!(layer_ui::durable_layout(&f.host.session.state().workspace.layout), baseline);
            assert_eq!(f.host.session.capture_workspace().unwrap(), before);
            f.save();
            assert_eq!(
                f.controller.manager.current().unwrap(),
                saved,
                "preview is never persisted"
            );
            let prompt = f.controller.view.prompt.as_ref().unwrap();
            let expected = f
                .controller
                .manager
                .form_prompt(
                    &ManagerAction::Reset(saved.id.clone()),
                    Some(&saved.metadata),
                )
                .unwrap();
            assert_eq!(prompt.message, expected.message);
            assert!(prompt.message.contains("Window → Undo Workspace"));
            if builtin.is_some() {
                assert!(prompt.message.contains("latest default"));
            } else {
                assert!(prompt.message.contains("saved starting layout"));
            }
            if confirm {
                f.controller
                    .input(&mut f.host.session, serde_json::from_value(serde_json::json!({"type":"submit","name":""})).unwrap(), f.backend.now.get())
                    .unwrap();
                assert_eq!(
                    layer_ui::durable_layout(&f.host.session.state().workspace.layout),
                    baseline,
                    "the outgoing layout is never presented between confirmation and adoption"
                );
                f.pump();
            } else {
                f.input(serde_json::json!({"type":"cancel"}));
            }
            if !confirm {
                assert_eq!(f.host.session.capture_workspace().unwrap(), before);
                assert_eq!(
                    f.host.session.state().workspace.layout,
                    before_layout
                );
            }
        }
        let restored = f.host.session.capture_workspace().unwrap();
        assert!(!f.host.session.command(layer_ui::CommandId::ResetLayout).enabled);
        assert_eq!(restored.history.layout(), &baseline);
        assert_eq!(restored.working, before.working);
        assert_eq!(restored.history.undo.len(), before.history.undo.len() + 1);
        f.action(serde_json::json!({"type":"invoke","command":"undo_workspace"}));
        assert_eq!(
            &layer_ui::durable_layout(&f.host.session.state().workspace.layout),
            before.history.layout()
        );
        f.action(serde_json::json!({"type":"invoke","command":"redo_workspace"}));
        assert_eq!(layer_ui::durable_layout(&f.host.session.state().workspace.layout), baseline);
    }
}

#[test]
fn previews_cancel_pending_replies_and_never_publish_temporary_layouts() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone().unwrap();
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.input(serde_json::json!({"type":"submit","name":"Painting"}));
    let painting = f.controller.view.id.clone().unwrap();
    assert_ne!(painting, original);
    f.action(serde_json::json!({"type":"move_panel","panel":"layers","viewport":[1200,900],"target":{"kind":"float","position":[480,220]}}));
    f.action(serde_json::json!({"type":"invoke","command":"eraser"}));
    f.save();
    let saved = f.controller.manager.current().unwrap();
    let capture = f.host.session.capture_workspace().unwrap();
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let (release, gate) = async_channel::bounded(1);
    *f.backend.load_gate.borrow_mut() = Some(gate);
    f.input(serde_json::json!({"type":"select","id":original}));
    assert!(!f.controller.view.enabled);
    f.input(serde_json::json!({"type":"cancel"}));
    let _ = release.try_send(());
    f.pump();
    assert!(f.controller.view.page.is_none());
    assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
    assert_eq!(f.controller.manager.current().unwrap(), saved);
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"select","id":original}));
    assert_eq!(f.controller.view.id.as_deref(), Some(painting.as_str()));
    assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
    f.input(serde_json::json!({"type":"cancel"}));
    f.input(serde_json::json!({"type":"open","page":"history"}));
    f.input(serde_json::json!({"type":"select","id":"r0"}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
    f.input(serde_json::json!({"type":"confirm"}));
    let restored = f.host.session.capture_workspace().unwrap();
    assert_eq!(restored.working, capture.working);
    assert_eq!(restored.history.undo.len(), capture.history.undo.len() + 1);
    f.action(serde_json::json!({"type":"invoke","command":"undo_workspace"}));
    assert_eq!(
        f.host.session.capture_workspace().unwrap().history.layout(),
        capture.history.layout()
    );
}
#[test]
fn save_failure_lost_ack_suspend_and_restart_preserve_working_history() {
    let mut f = Fixture::new();
    let id = f.controller.view.id.clone();
    f.backend.lose_reply.set(true);
    f.action(serde_json::json!({"type":"invoke","command":"eraser"}));
    f.save();
    assert!(!f.controller.manager.dirty());
    assert!(f.controller.view.error.is_none());
    f.backend.fail.set(true);
    f.action(serde_json::json!({"type":"move_panel","panel":"layers","viewport":[1200,900],"target":{"kind":"float","position":[480,220]}}));
    f.save();
    assert!(f.controller.manager.dirty());
    assert!(f.controller.view.error.is_some());
    let expected = f.host.session.capture_workspace().unwrap();
    f.input(serde_json::json!({"type":"suspend"}));
    assert!(f.controller.view.error.is_some());
    assert_eq!(f.host.session.capture_workspace().unwrap(), expected);
    f.backend.fail.set(false);
    f.input(serde_json::json!({"type":"resume"}));
    f.input(serde_json::json!({"type":"retry"}));
    assert!(!f.controller.manager.dirty());
    assert!(f.controller.view.error.is_none());
    f.input(serde_json::json!({"type":"suspend"}));
    assert!(!f.controller.manager.lease_valid(f.backend.now.get()));
    f.input(serde_json::json!({"type":"resume"}));
    // Renewal is deliberately due on resume, even before the old lease deadline.
    assert!(f.controller.manager.lease_valid(f.backend.now.get()));
    f.backend.now.set(12000);
    f.pump();
    assert!(f.controller.manager.lease_valid(12000));
    assert!(f.controller.view.error.is_none());
    f.input(serde_json::json!({"type":"suspend"}));
    let manager = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let incoming = pollster::block_on(manager.initialize(12000)).unwrap();
    assert_eq!(Some(incoming.entity.id.clone()), id);
    let saved = incoming.entity.capture().unwrap();
    assert_eq!(saved.working, expected.working);
    assert_eq!(saved.history.layout(), expected.history.layout());
    assert_eq!(saved.history.undo, expected.history.undo);
}

#[test]
fn default_switches_preserve_editing_and_brush_reset_is_shared() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone().unwrap();
    let initial = f.host.session.capture_workspace().unwrap();
    let painter = "builtin:workspace:painter";
    f.input(serde_json::json!({"type":"switch","id":painter}));
    f.action(serde_json::json!({"type":"set_brush_size","value":73.0}));
    f.action(serde_json::json!({"type":"invoke","command":"eraser"}));
    f.action(serde_json::json!({"type":"set_brush_size","value":91.0}));
    f.action(serde_json::json!({"type":"set_color","rgba":[0.2,0.3,0.4,1.0]}));
    f.save();
    let edited = f.host.session.capture_workspace().unwrap();
    let editing = f.host.session.editing_state();
    f.input(serde_json::json!({"type":"switch","id":"builtin:workspace:photographer"}));
    assert_eq!(f.host.session.editing_state(), editing);
    f.input(serde_json::json!({"type":"switch","id":painter}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), edited);
    f.input(serde_json::json!({"type":"form","action":{"type":"reset_brushes"}}));
    f.input(serde_json::json!({"type":"cancel"}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), edited);
    f.input(serde_json::json!({"type":"form","action":{"type":"reset_brushes"}}));
    f.input(serde_json::json!({"type":"submit","name":""}));
    let reset = f.host.session.capture_workspace().unwrap();
    assert_eq!(reset.history, edited.history);
    let mut expected = editing;
    assert!(!expected.tools.overrides.is_empty());
    expected.tools.overrides.clear();
    assert_eq!(reset, edited);
    assert_eq!(f.host.session.editing_state(), expected);
    assert_eq!(*f.controller.manager.editing().unwrap(), expected);
    f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":painter}}));
    f.input(serde_json::json!({"type":"submit","name":"My Painter"}));
    assert!(
        f.controller
            .view
            .error
            .as_deref()
            .is_some_and(|error| error.contains("cannot be renamed"))
    );
    assert_eq!(
        f.controller
            .manager
            .items()
            .iter()
            .find(|i| i.id == painter)
            .unwrap()
            .metadata
            .name,
        "Sketch"
    );
    f.input(serde_json::json!({"type":"cancel"}));
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let row = f
        .controller
        .view
        .rows
        .iter()
        .find(|r| r.id == painter)
        .unwrap();
    assert!(!row.actions.iter().any(|b| {
        b.enabled && matches!(b.action, ManagerAction::Rename(_) | ManagerAction::Delete(_))
    }));
    f.input(serde_json::json!({"type":"cancel"}));
    assert!(
        pollster::block_on(
            f.controller
                .manager
                .delete_item(painter, None, f.backend.now.get())
        )
        .is_err()
    );
    f.input(serde_json::json!({"type":"switch","id":original}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), initial);
}

#[test]
fn failed_creation_keeps_recovery_visible_and_retries_immutable_delivery() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone();
    let pins = f.controller.manager.switcher_ids();
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.backend.fail.set(true);
    f.input(serde_json::json!({"type":"submit","name":"Painting"}));
    assert!(f.controller.view.error.is_some());
    assert!(f.controller.view.retry);
    assert_eq!(f.controller.view.id, original);
    assert_eq!(f.controller.manager.switcher_ids(), pins);
    let failed = f.backend.deliveries.borrow().last().unwrap().clone();
    f.input(serde_json::json!({"type":"cancel"}));
    assert!(
        f.controller.view.error.is_some(),
        "Cancel keeps the failed delivery available for recovery"
    );
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.backend.fail.set(false);
    f.input(serde_json::json!({"type":"submit","name":"Painting"}));
    assert!(f.controller.view.error.is_none());
    assert_ne!(f.controller.view.id, original);
    assert_eq!(f.controller.view.name, "Painting");
    assert!(
        f.controller
            .manager
            .switcher_ids()
            .contains(f.controller.view.id.as_ref().unwrap())
    );
    assert_eq!(
        f.backend
            .deliveries
            .borrow()
            .iter()
            .filter(|id| **id == failed)
            .count(),
        2
    );
    assert_eq!(
        f.controller
            .manager
            .items()
            .iter()
            .filter(|i| i.metadata.name == "Painting")
            .count(),
        1
    );
}

#[test]
fn new_workspaces_are_pinned_without_repinning_hidden_workspaces() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone().unwrap();
    let painter = DEFAULT_WORKSPACES[0].0;
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":painter,"visible":false}}));
    let pins = f.controller.manager.switcher_ids();
    let revision = f.controller.view.switcher_revision;
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.input(serde_json::json!({"type":"cancel"}));
    assert_eq!(f.controller.manager.switcher_ids(), pins);
    f.input(serde_json::json!({"type":"form","action":{"type":"new"}}));
    f.input(serde_json::json!({"type":"submit","name":"Sketching"}));
    let created = f.controller.view.id.clone().unwrap();
    let expected: Vec<_> = pins.into_iter().chain([created.clone()]).collect();
    assert_eq!(f.controller.manager.switcher_ids(), expected);
    assert!(
        f.controller.view.switcher_revision > revision,
        "creation notifies other windows of the new pin"
    );
    f.input(serde_json::json!({"type":"switch","id":original}));
    assert_eq!(f.controller.manager.switcher_ids(), expected);
    let reopened = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    pollster::block_on(async {
        reopened.refresh().await.unwrap();
        reopened.refresh_switcher().await.unwrap();
    });
    assert_eq!(reopened.switcher_ids(), expected);
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":created,"visible":false}}));
    f.input(serde_json::json!({"type":"switch","id":created}));
    assert!(
        !f.controller.manager.switcher_ids().contains(&created),
        "switching back does not repin an explicitly hidden workspace"
    );
}

#[test]
fn closing_before_adoption_drains_claims() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"suspend"}));
    let mut closing = WorkspaceController::new_localized(Store(f.backend.clone()), Platform::Web, 1000, Localizer::shared(UiLanguage::English));
    closing
        .input(&mut f.host.session, WorkspaceInput::Close, 1000)
        .unwrap();
    for _ in 0..20 {
        closing.tick(&mut f.host.session, 1000);
    }
    assert!(!closing.view.busy);
    assert!(
        !closing.view.ready,
        "A closing host must not wait for a canvas adoption"
    );
    assert!(closing.view.error.is_none());
}

#[test]
fn workspace_order_presents_accepted_moves_until_store_completion() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let [p, i, h] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    f.input(serde_json::json!({"type":"select","id":h}));
    let original = f.controller.manager.current_record().unwrap();
    let preview = f.host.session.state().workspace.layout.clone();
    let revision = f.controller.view.switcher_revision;
    let initial = f.controller.manager.workspace_ids();
    let custom = initial.iter().find(|id| ![&p, &i, &h].contains(id)).unwrap().clone();
    let ids = |rows: &[WorkspaceRow]| rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let assert_order = |f: &Fixture, order: &[String]| {
        assert_eq!(f.controller.view.order, order, "accepted order is visible before its store acknowledgement");
        assert_eq!(ids(&f.controller.view.rows), order, "manager rows follow the accepted order");
        let pins = f.controller.manager.switcher_ids();
        let mut display: Vec<_> = order.iter().filter(|id| pins.contains(id)).cloned().collect();
        assert_eq!(ids(&f.controller.view.switcher), display);
        if let Some(active) = f.controller.manager.active_id() && !display.contains(&active) { display.insert(0, active); }
        assert_eq!(ids(&f.controller.view.switcher_display), display);
        assert_eq!(f.controller.view.selected.as_ref(), Some(&h));
        assert_eq!(f.host.session.state().workspace.layout, preview);
        assert_eq!(f.controller.manager.current_record().unwrap(), original);
    };
    let edit = |f: &mut Fixture, id: &str, before: Option<&str>| {
        f.controller.input(&mut f.host.session, WorkspaceInput::EditSwitcher {
            edit: SwitcherEdit::Move { id: id.into(), before: before.map(str::to_string) },
        }, f.backend.now.get()).unwrap();
    };
    let (first, gate) = async_channel::bounded(1);
    *f.backend.order_gate.borrow_mut() = Some(gate);
    edit(&mut f, &h, Some(&p));
    let first_order = [h.clone(), p.clone(), i.clone(), custom.clone()];
    assert_order(&f, &first_order);
    f.pump();
    assert!(f.controller.view.switcher_busy);
    assert_eq!(f.controller.manager.workspace_ids(), initial);
    edit(&mut f, &p, None);
    let second_order = [h.clone(), i.clone(), custom.clone(), p.clone()];
    assert_order(&f, &second_order);
    f.pump();
    assert_order(&f, &second_order);
    let (second, gate) = async_channel::bounded(1);
    *f.backend.order_gate.borrow_mut() = Some(gate);
    first.try_send(()).unwrap();
    f.pump();
    assert_eq!(f.controller.manager.workspace_ids(), first_order);
    assert_order(&f, &second_order);
    second.try_send(()).unwrap();
    f.pump();
    assert_eq!(f.controller.manager.workspace_ids(), second_order);
    assert_order(&f, &second_order);
    assert!(!f.controller.view.switcher_busy);
    assert_eq!(f.controller.view.switcher_revision, revision + 2);

    let (failed, gate) = async_channel::bounded(1);
    *f.backend.order_gate.borrow_mut() = Some(gate);
    edit(&mut f, &p, Some(&h));
    let retried_order = [p.clone(), h.clone(), i.clone(), custom];
    assert_order(&f, &retried_order);
    f.pump();
    f.backend.fail_preferences.set(true);
    failed.try_send(()).unwrap();
    f.pump();
    assert_order(&f, &second_order);
    assert_eq!(f.controller.manager.workspace_ids(), second_order);
    assert!(f.controller.view.switcher_error.is_some());
    assert!(f.controller.view.error.is_none());
    assert!(!f.controller.view.switcher_busy);
    assert_eq!(f.controller.view.switcher_revision, revision + 2);
    f.backend.fail_preferences.set(false);
    let (retry, gate) = async_channel::bounded(1);
    *f.backend.order_gate.borrow_mut() = Some(gate);
    edit(&mut f, &p, Some(&h));
    assert_order(&f, &retried_order);
    assert!(f.controller.view.switcher_error.is_none());
    f.pump();
    retry.try_send(()).unwrap();
    f.pump();
    assert_order(&f, &retried_order);
    assert_eq!(f.controller.manager.workspace_ids(), retried_order);
    assert_eq!(f.controller.view.switcher_revision, revision + 3);
    assert!(!f.controller.view.switcher_busy);
    f.pump();
    assert_order(&f, &retried_order);
}

#[test]
fn switcher_edits_preserve_pending_preview_and_workspace_contents() {
    let mut f = Fixture::new();
    let revision = f.controller.view.switcher_revision;
    let original = f.controller.manager.current_record().unwrap();
    let [painter, illustrator, photographer] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    assert_eq!(
        f.controller
            .view
            .switcher
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        [&painter, &illustrator, &photographer]
    );
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let (release, gate) = async_channel::bounded(1);
    *f.backend.load_gate.borrow_mut() = Some(gate);
    f.input(serde_json::json!({"type":"select","id":painter}));
    assert!(!f.controller.view.enabled);
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":photographer,"visible":false}}));
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"move","id":photographer,"before":painter}}));
    assert_eq!(f.controller.view.selected.as_ref(), Some(&painter));
    assert!(
        !f.controller.view.enabled,
        "preference acknowledgement must not restart a pending preview"
    );
    assert_eq!(f.controller.view.order[0], photographer);
    assert_eq!(f.controller.view.switcher.len(), 2);
    assert_eq!(f.controller.view.switcher_revision, revision + 2);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    release.try_send(()).unwrap();
    f.pump();
    let preview = f.host.session.state().workspace.layout.clone();
    assert!(f.controller.view.enabled);
    // Another window updates app preferences without claiming this workspace.
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    pollster::block_on(other.edit_switcher(SwitcherEdit::Move {
        id: illustrator.clone(),
        before: Some(painter.clone()),
    }))
    .unwrap();
    f.input(serde_json::json!({"type":"refresh_switcher"}));
    assert_eq!(f.controller.view.switcher[0].id, illustrator);
    assert_eq!(
        f.controller.view.switcher_revision,
        revision + 2,
        "refresh must not broadcast another edit"
    );
    assert_eq!(f.controller.view.selected.as_ref(), Some(&painter));
    assert_eq!(f.host.session.state().workspace.layout, preview);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"move","id":"missing","before":null}}));
    assert!(f.controller.view.switcher_error.is_some());
    assert!(
        f.controller.view.error.is_none(),
        "preference failures are separate from workspace saves"
    );
    assert_eq!(f.host.session.state().workspace.layout, preview);
    f.input(serde_json::json!({"type":"cancel"}));
    assert_eq!(f.controller.view.switcher[0].id, illustrator);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);
    assert_eq!(
        f.host.session.state().workspace.layout,
        *original.entity.capture().unwrap().history.layout()
    );
}

#[test]
fn switcher_edit_during_autosave_is_acknowledged_without_interrupting_the_save() {
    let mut f = Fixture::new();
    f.action(serde_json::json!({"type":"move_panel","panel":"layers","viewport":[1200,900],"target":{"kind":"float","position":[480,220]}}));
    let capture = f.host.session.capture_workspace().unwrap();
    let revision = f.controller.view.switcher_revision;
    let (release, gate) = async_channel::bounded(1);
    *f.backend.commit_gate.borrow_mut() = Some(gate);
    f.save();
    assert!(f.controller.manager.saving());
    assert!(!f.controller.view.busy);
    let id = f.controller.view.switcher[0].id.clone();
    f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":id,"visible":false}}));
    assert_eq!(f.controller.view.switcher_revision, revision + 1);
    assert!(!f.controller.view.switcher.iter().any(|row| row.id == id));
    assert!(f.controller.manager.saving());
    release.try_send(()).unwrap();
    f.pump();
    assert!(!f.controller.manager.saving());
    assert!(!f.controller.manager.dirty());
    assert_eq!(f.host.session.capture_workspace().unwrap(), capture);
    assert!(f.controller.view.error.is_none());
}

#[test]
fn switcher_edits_wait_for_refresh_and_close_drains_them_in_order() {
    let mut f = Fixture::new();
    let revision = f.controller.view.switcher_revision;
    let id = f.controller.view.switcher[0].id.clone();
    let (release, gate) = async_channel::bounded(1);
    *f.backend.switcher_gate.borrow_mut() = Some(gate);
    f.input(serde_json::json!({"type":"refresh_switcher"}));
    assert!(f.controller.view.switcher_busy);
    for _ in 0..3 {
        f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"toggle","id":id}}));
    }
    assert_eq!(f.controller.view.switcher_revision, revision);
    f.input(serde_json::json!({"type":"close"}));
    assert!(!f.controller.view.closed);
    release.try_send(()).unwrap();
    f.pump();
    assert_eq!(f.controller.view.switcher_revision, revision + 3);
    assert!(f.controller.view.closed);
    assert!(!f.controller.view.switcher.iter().any(|row| row.id == id));
    assert!(!f.controller.view.switcher_busy);
    assert!(f.controller.view.error.is_none());
    assert!(f.controller.view.switcher_error.is_none());
}

#[test]
fn unpinned_current_workspace_is_temporary_and_previews_do_not_replace_it() {
    let mut f = Fixture::new();
    let [p, i, h] = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string());
    let shown = |f: &Fixture| {
        f.controller
            .view
            .switcher_display
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    };
    f.input(serde_json::json!({"type":"switch","id":i}));
    let original = f.controller.manager.current_record().unwrap();
    let order = f.controller.view.order.clone();
    f.input(
        serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":i,"visible":false}}),
    );
    assert_eq!(shown(&f), [i.clone(), p.clone(), h.clone()]);
    assert_eq!(f.controller.manager.switcher_ids(), [p.clone(), h.clone()]);
    assert_eq!(f.controller.view.order, order);
    assert_eq!(f.controller.manager.current_record().unwrap(), original);

    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"select","id":p}));
    assert_eq!(shown(&f), [i.clone(), p.clone(), h.clone()]);
    f.input(serde_json::json!({"type":"cancel"}));
    let displayed = original.clone();
    assert_eq!(f.controller.manager.current_record().unwrap(), displayed);

    // Pinning returns it to the saved order without duplicating it.
    f.input(
        serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":i,"visible":true}}),
    );
    assert_eq!(shown(&f), [p.clone(), i.clone(), h.clone()]);
    f.input(
        serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":i,"visible":false}}),
    );
    f.input(serde_json::json!({"type":"switch","id":p}));
    assert_eq!(shown(&f), [p.clone(), h.clone()]);
    f.input(serde_json::json!({"type":"switch","id":i}));
    assert_eq!(shown(&f), [i.clone(), p.clone(), h.clone()]);

    for id in [&p, &h] {
        f.input(serde_json::json!({"type":"edit_switcher","edit":{"type":"show","id":id,"visible":false}}));
    }
    assert_eq!(shown(&f).as_slice(), std::slice::from_ref(&i));
    assert!(f.controller.view.switcher.is_empty());
    f.input(serde_json::json!({"type":"switch","id":"missing-workspace"}));
    assert!(f.controller.view.error.is_some());
    assert_eq!(shown(&f), [i]);
    f.input(serde_json::json!({"type":"switch","id":p}));
    assert_eq!(shown(&f), [p]);
    assert!(f.controller.view.switcher.is_empty());
    assert_eq!(f.controller.view.order, order);

    // A window without an adopted workspace sees only the saved pins.
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    pollster::block_on(async {
        other.refresh().await.unwrap();
        other.refresh_switcher().await.unwrap();
    });
    assert!(other.switcher_display_ids().is_empty());
}

#[test]
fn host_workspace_requests_complete_once() {
    let mut f = Fixture::new();
    let pending = |f: &Fixture| {
        f.host
            .session
            .state()
            .requests
            .iter()
            .filter(|r| matches!(r.kind, layer_ui::HostRequestKind::Workspace { .. }))
            .count()
    };
    f.action(serde_json::json!({"type":"workspace_manager","command":{"type":"manage"}}));
    f.action(serde_json::json!({"type":"workspace_manager","command":{"type":"new"}}));
    assert_eq!(pending(&f), 2);
    f.pump();
    assert_eq!(pending(&f), 0);
    assert_eq!(f.controller.view.page, Some(ManagerPage::Workspaces));
    assert_eq!(f.controller.view.prompt_action, Some(ManagerAction::New));
    f.input(serde_json::json!({"type":"cancel"}));
    f.input(serde_json::json!({"type":"cancel"}));
    f.pump();
    assert!(f.controller.view.page.is_none());
    assert!(
        f.controller.view.prompt.is_none(),
        "a completed request is never replayed"
    );
    f.action(serde_json::json!({"type":"workspace_manager","command":{"type":"manage_toolbars"}}));
    f.pump();
    assert_eq!(pending(&f), 0);
    assert!(!f.host.session.state().customization.is_open());
    assert_eq!(f.controller.view.page, Some(ManagerPage::ThisWorkspace));
}

#[test]
fn transient_renew_error_keeps_editing_but_takeover_goes_read_only() {
    let mut f = Fixture::new();
    let id = f.controller.view.id.clone().unwrap();
    let brush = |size: f64| serde_json::json!({"type":"set_brush_size","value":size});
    *f.backend.fail_renew.borrow_mut() = Some(ErrorKind::Unavailable);
    f.backend.now.set(f.backend.now.get() + OWNER_RENEW_MS);
    f.pump();
    assert!(f.controller.view.error.is_some());
    assert!(!f.controller.view.owner_lost);
    f.action(brush(41.0));
    *f.backend.fail_renew.borrow_mut() = None;
    f.backend.now.set(f.backend.now.get() + OWNER_RENEW_MS);
    f.pump();
    assert!(f.controller.view.error.is_none());
    f.save();
    assert!(!f.controller.manager.dirty());
    f.backend.now.set(f.backend.now.get() + OWNER_LEASE_MS + 1);
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let taken = pollster::block_on(other.prepare_switch(&id, f.backend.now.get())).unwrap();
    other.activate(taken);
    f.pump();
    assert!(f.controller.view.owner_lost);
    assert!(f.controller.view.error.is_some());
    assert!(
        f.host
            .dispatch(serde_json::from_value::<UiAction>(brush(52.0)).unwrap())
            .is_err()
    );
}

fn action(action: ManagerAction) -> serde_json::Value {
    serde_json::json!({"type":"action","action":action})
}

fn toolbar_names(f: &Fixture) -> Vec<String> {
    f.host
        .session
        .state()
        .workspace
        .layout
        .panels
        .iter()
        .filter(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
        .map(|p| p.canonical_title().to_string())
        .collect()
}

fn saved_toolbar(f: &Fixture, name: &str) -> Option<ItemSummary> {
    f.controller
        .manager
        .items()
        .into_iter()
        .find(|i| i.metadata.kind == ItemKind::Toolbar && i.metadata.name == name)
}

#[test]
fn toolbar_install_is_one_undo_step_and_retry_flushes_without_reinstalling() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"form","action":{"type":"new_toolbar","value":null}}));
    f.input(serde_json::json!({"type":"submit","name":"Taken"}));
    f.save();
    let before = f.host.session.state().workspace.layout.clone();
    f.input(serde_json::json!({"type":"form","action":{"type":"new_toolbar","value":null}}));
    let prompt = f.controller.view.prompt.clone().unwrap();
    assert_eq!(prompt.name.as_deref(), Some("New Toolbar"));
    assert_eq!(prompt.selected.as_deref(), Some(""));
    f.input(serde_json::json!({"type":"submit","name":"Taken"}));
    assert!(
        f.controller.view.error.is_some(),
        "typed toolbar names must not collide"
    );
    assert_eq!(f.host.session.state().workspace.layout, before);
    f.backend.fail.set(true);
    f.input(serde_json::json!({"type":"submit","name":"Inks"}));
    f.save();
    assert!(f.controller.view.error.is_some());
    assert!(f.controller.view.retry);
    assert!(f.controller.view.prompt.is_none());
    let installed = |f: &Fixture| toolbar_names(f).iter().filter(|n| *n == "Inks").count();
    assert_eq!(installed(&f), 1);
    f.backend.fail.set(false);
    f.input(serde_json::json!({"type":"retry"}));
    assert!(f.controller.view.error.is_none());
    assert!(!f.controller.manager.dirty());
    assert_eq!(installed(&f), 1, "retry flushes without reinstalling");
    let saved = f.controller.manager.current().unwrap().capture().unwrap();
    let shown = f.host.session.capture_workspace().unwrap();
    assert_eq!(saved.history.layout(), shown.history.layout());
    f.action(serde_json::json!({"type":"invoke","command":"undo_workspace"}));
    assert_eq!(
        layer_ui::durable_layout(&f.host.session.state().workspace.layout),
        layer_ui::durable_layout(&before)
    );
}

#[test]
fn saved_toolbar_save_add_rename_delete_are_independent_copies() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"open","page":"this_workspace"}));
    let row = f.controller.view.rows[0].clone();
    let panel: layer_ui::Panel = serde_json::from_str(&row.id).unwrap();
    f.input(serde_json::json!({"type":"select","id":row.id}));
    assert!(f.controller.view.details.is_some());
    f.input(action(ManagerAction::SaveToolbar(panel)));
    assert_eq!(
        f.controller.view.prompt.as_ref().unwrap().name.as_deref(),
        Some(row.title.as_str())
    );
    f.input(serde_json::json!({"type":"submit","name":"Saved Ink"}));
    let saved = saved_toolbar(&f, "Saved Ink").unwrap().id;
    f.input(serde_json::json!({"type":"open","page":"toolbar_library"}));
    f.input(serde_json::json!({"type":"select","id":saved}));
    let details = f.controller.view.details.clone().unwrap();
    assert!(
        details
            .actions
            .iter()
            .any(|b| b.action == ManagerAction::UpdateToolbar(saved.clone()) && b.enabled)
    );
    assert_eq!(f.controller.view.primary, "Add to Workspace");
    f.input(serde_json::json!({"type":"confirm"}));
    assert!(f.controller.view.page.is_none());
    assert!(toolbar_names(&f).contains(&"Saved Ink".to_string()));
    f.input(serde_json::json!({"type":"open","page":"toolbar_library"}));
    f.input(serde_json::json!({"type":"form","action":{"type":"rename","value":saved}}));
    assert!(f.controller.view.prompt.as_ref().unwrap().description.is_some());
    f.input(serde_json::json!({"type":"submit","name":"Renamed Ink","description":"Pens"}));
    let renamed = saved_toolbar(&f, "Renamed Ink").unwrap();
    assert_eq!(renamed.metadata.description, "Pens");
    assert!(toolbar_names(&f).contains(&"Saved Ink".to_string()));
    f.input(serde_json::json!({"type":"form","action":{"type":"delete","value":saved}}));
    f.input(serde_json::json!({"type":"submit"}));
    assert!(saved_toolbar(&f, "Renamed Ink").is_none());
    assert!(toolbar_names(&f).contains(&"Saved Ink".to_string()));
    f.input(serde_json::json!({"type":"dismiss"}));
    f.save();
    let stored = f.controller.manager.current().unwrap().capture().unwrap();
    assert!(
        stored
            .history
            .layout()
            .panels
            .iter()
            .any(|p| p.canonical_title() == "Saved Ink")
    );
}

#[test]
fn toolbar_pages_skip_layout_preview_and_reject_stale_actions() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"open","page":"this_workspace"}));
    let row = f.controller.view.rows[0].clone();
    let panel: layer_ui::Panel = serde_json::from_str(&row.id).unwrap();
    f.input(serde_json::json!({"type":"select","id":row.id}));
    f.input(action(ManagerAction::SaveToolbar(panel)));
    f.input(serde_json::json!({"type":"submit","name":"Library Ink"}));
    let saved = saved_toolbar(&f, "Library Ink").unwrap().id;
    let layout = f.host.session.state().workspace.layout.clone();
    f.input(serde_json::json!({"type":"open","page":"toolbar_library"}));
    let (release, gate) = async_channel::bounded(1);
    *f.backend.load_gate.borrow_mut() = Some(gate);
    f.input(serde_json::json!({"type":"select","id":saved}));
    assert!(f.controller.view.loading);
    assert_eq!(f.host.session.state().workspace.layout, layout);
    f.input(serde_json::json!({"type":"open","page":"this_workspace"}));
    let _ = release.try_send(());
    f.pump();
    assert!(
        f.controller.view.details.is_none(),
        "a late library read cannot replace the current page"
    );
    f.input(serde_json::json!({"type":"select","id":row.id}));
    assert_eq!(f.host.session.state().workspace.layout, layout);
    let names = toolbar_names(&f);
    f.input(action(ManagerAction::AddToolbar(saved.clone())));
    f.input(action(ManagerAction::DeleteToolbar(layer_ui::Panel::Layers)));
    assert_eq!(toolbar_names(&f), names);
    assert_eq!(f.controller.view.page, Some(ManagerPage::ThisWorkspace));
    let visible = f.host.session.state().workspace.layout.panel_group(panel).is_some();
    f.input(action(ManagerAction::ShowToolbar(panel, visible)));
    assert_eq!(
        f.host.session.state().workspace.layout.panel_group(panel).is_some(),
        visible
    );
    f.input(action(ManagerAction::ShowToolbar(panel, !visible)));
    assert_eq!(
        f.host.session.state().workspace.layout.panel_group(panel).is_some(),
        !visible
    );
    assert_eq!(
        f.controller.view.details.as_ref().unwrap().actions[0].action,
        ManagerAction::ShowToolbar(panel, visible)
    );
}

#[test]
fn discard_close_releases_claims_without_saving_and_resume_cancels() {
    let mut f = Fixture::new();
    let id = f.controller.view.id.clone().unwrap();
    let saved = f.controller.manager.current_record().unwrap();
    f.backend.fail.set(true);
    f.action(serde_json::json!({"type":"set_brush_size","value":61.0}));
    f.input(serde_json::json!({"type":"close"}));
    assert!(f.controller.view.closing);
    assert!(!f.controller.view.closed);
    assert!(f.controller.view.error.is_some());
    f.input(serde_json::json!({"type":"resume"}));
    assert!(!f.controller.view.closing);
    f.pump();
    assert!(f.controller.manager.lease_valid(f.backend.now.get()));
    f.input(serde_json::json!({"type":"close"}));
    assert!(f.controller.view.closing);
    assert!(f.controller.view.error.is_some());
    f.input(serde_json::json!({"type":"discard_close"}));
    assert!(f.controller.view.closed);
    assert!(!f.controller.view.closing);
    let stored = pollster::block_on(f.controller.manager.load(&id)).unwrap();
    assert!(stored.claim.is_none());
    assert_eq!(stored.entity, saved.entity);
    f.input(serde_json::json!({"type":"resume"}));
    assert!(f.controller.view.closed, "a finished close cannot be resumed");
}

#[test]
fn wake_fires_on_store_reply_and_stop_disarms() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut f = Fixture::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    f.controller.set_wake(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    for (index, id) in [DEFAULT_WORKSPACES[0].0, DEFAULT_WORKSPACES[2].0]
        .into_iter()
        .enumerate()
    {
        if index == 1 {
            f.controller.stop();
        }
        let (release, gate) = async_channel::bounded(1);
        *f.backend.load_gate.borrow_mut() = Some(gate);
        f.input(serde_json::json!({"type":"select","id":id}));
        assert!(f.controller.view.loading);
        let before = wakes.load(Ordering::SeqCst);
        release.try_send(()).unwrap();
        assert_eq!(wakes.load(Ordering::SeqCst), before + usize::from(index == 0));
        f.pump();
        assert!(!f.controller.view.loading);
    }
}

#[test]
fn focus_target_carries_owner_and_failed_focus_sets_error() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone();
    let painter = DEFAULT_WORKSPACES[0].0;
    let other = WorkspaceManager::new(Store(f.backend.clone()), Platform::Web);
    let taken = pollster::block_on(other.prepare_switch(painter, f.backend.now.get())).unwrap();
    other.activate(taken);
    let target = Some(FocusTarget {
        id: painter.into(),
        owner: other.owner.id.clone(),
    });
    f.input(serde_json::json!({"type":"switch","id":painter}));
    assert_eq!(f.controller.view.focus_window, target);
    assert_eq!(f.controller.view.id, original);
    f.input(serde_json::json!({"type":"focus_failed","error":"The other window is unavailable."}));
    assert_eq!(
        f.controller.view.error.as_deref(),
        Some("The other window is unavailable.")
    );
    assert!(f.controller.view.focus_window.is_none());
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    f.input(serde_json::json!({"type":"select","id":painter}));
    assert_eq!(f.controller.view.primary, "Switch to Window");
    f.input(serde_json::json!({"type":"confirm"}));
    assert_eq!(f.controller.view.focus_window, target);
    assert!(f.controller.view.page.is_none());
    assert_eq!(f.controller.view.id, original);
}

fn binding(f: &Fixture, key: &str) -> Option<String> {
    match pollster::block_on(
        Store(f.backend.clone()).execute(StoreRequest::Binding { key: key.into() }),
    )
    .unwrap()
    {
        StoreResponse::Binding(id) => id,
        _ => None,
    }
}
#[test]
fn resume_key_restores_the_scene_workspace_before_last_used() {
    let mut f = Fixture::new();
    let first = f.controller.view.id.clone().unwrap();
    let key = "apple:scene:one".to_string();
    let mut host = layer_host::NativeHost::new(Platform::Web).unwrap();
    let now = f.backend.now.get();
    let mut run = |c: &mut WorkspaceController<Store>, input: Option<serde_json::Value>| {
        if let Some(input) = input {
            c.input(&mut host.session, serde_json::from_value(input).unwrap(), now)
                .unwrap();
        }
        for _ in 0..20 {
            c.tick(&mut host.session, now);
        }
    };
    let start = |f: &Fixture| {
        WorkspaceController::new_localized(Store(f.backend.clone()), Platform::Web, now, Localizer::shared(UiLanguage::English))
            .with_resume_key(key.clone(), now)
    };
    let mut scene = start(&f);
    run(&mut scene, None);
    assert!(scene.view.ready, "{:?}", scene.view.error);
    let restored = scene.view.id.clone().unwrap();
    assert_ne!(restored, first);
    assert_eq!(binding(&f, &key).as_deref(), Some(restored.as_str()));
    run(&mut scene, Some(serde_json::json!({"type":"close"})));
    assert!(scene.view.closed);
    let other = DEFAULT_WORKSPACES
        .iter()
        .map(|w| w.0)
        .find(|id| *id != first && *id != restored)
        .unwrap();
    f.input(serde_json::json!({"type":"switch","id":other}));
    assert_eq!(f.controller.view.id.as_deref(), Some(other));
    f.input(serde_json::json!({"type":"close"}));
    assert!(f.controller.view.closed);
    assert_eq!(binding(&f, "last_workspace").as_deref(), Some(other));
    let mut scene = start(&f);
    run(&mut scene, None);
    assert_eq!(scene.view.id.as_deref(), Some(restored.as_str()));
    run(&mut scene, Some(serde_json::json!({"type":"detach"})));
    assert!(scene.view.closed);
    let stored = pollster::block_on(f.controller.manager.load(&restored)).unwrap();
    assert!(stored.claim.is_none());
}
#[test]
fn import_toolbar_package_selects_it_and_backup_adopts() {
    let mut f = Fixture::new();
    let current = f.controller.manager.current().unwrap();
    let panel = current
        .capture()
        .unwrap()
        .history
        .layout()
        .panels
        .iter()
        .find(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
        .unwrap()
        .id;
    let m = f.controller.manager.clone();
    let saved = pollster::block_on(m.save_toolbar(panel, "Shared Ink", 1000)).unwrap();
    let package = export_package(&pollster::block_on(m.load(&saved)).unwrap().entity).unwrap();
    f.input(serde_json::json!({
        "type":"import","kind":"toolbar","text":String::from_utf8(package).unwrap()
    }));
    assert_eq!(f.controller.view.page, Some(ManagerPage::ToolbarLibrary));
    let selected = f.controller.view.selected.clone().unwrap();
    assert_ne!(selected, saved);
    assert!(f.controller.view.rows.iter().any(|row| row.id == selected));
    assert!(f.controller.view.details.is_some());
    f.input(serde_json::json!({"type":"dismiss"}));
    let before = f.controller.view.id.clone();
    let backup = String::from_utf8(export_package(&current).unwrap()).unwrap();
    f.input(serde_json::json!({"type":"import","kind":"workspace_backup","text":backup}));
    assert!(f.controller.view.error.is_none(), "{:?}", f.controller.view.error);
    assert_ne!(f.controller.view.id, before);
    assert!(f.controller.view.name.starts_with("My Workspace"));
    assert!(!f.controller.view.busy && f.controller.view.page.is_none());
}

/// Answers like `Store` until request `fail_from`, then fails every request
/// with `kind`. A reset heals it when `reset_heals`, as replacing unreadable data does.
struct Faulty {
    database: RefCell<BrowserDatabase>,
    requests: Cell<usize>,
    fail_from: usize,
    reset_heals: bool,
    kind: ErrorKind,
    healed: Cell<bool>,
    reset: Cell<bool>,
}
#[derive(Clone)]
struct FaultyStore(Rc<Faulty>);
impl FaultyStore {
    fn new(fail_from: usize, reset_heals: bool) -> Self {
        Self::failing(fail_from, reset_heals, ErrorKind::InvalidData)
    }
    fn failing(fail_from: usize, reset_heals: bool, kind: ErrorKind) -> Self {
        Self(Rc::new(Faulty {
            database: RefCell::default(),
            requests: Cell::new(0),
            fail_from,
            reset_heals,
            kind,
            healed: Cell::new(false),
            reset: Cell::new(false),
        }))
    }
}
impl WorkspaceStore for FaultyStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        let f = &self.0;
        let index = f.requests.replace(f.requests.get() + 1);
        let reset = matches!(request, StoreRequest::Reset);
        f.reset.set(f.reset.get() || reset);
        if index >= f.fail_from && !f.healed.get() && !(reset && f.reset_heals) {
            return Err(StoreError::new(f.kind.clone(), "invalid type: map, expected a boolean"));
        }
        f.healed.set(f.healed.get() || reset);
        if let StoreRequest::Commit { batch } = &request {
            f.database.borrow_mut().prepare_delivery(batch)?;
        }
        f.database.borrow_mut().execute(request, 1000)
    }
}
fn start<S: WorkspaceStore + 'static>(
    store: S,
    platform: Platform,
) -> (WorkspaceController<S>, layer_host::NativeHost) {
    let mut host = layer_host::NativeHost::new(platform).unwrap();
    let mut controller = WorkspaceController::new_localized(store, platform, 1000, Localizer::shared(UiLanguage::English));
    for _ in 0..20 {
        if controller.view.ready {
            break;
        }
        controller.tick(&mut host.session, 1000);
    }
    (controller, host)
}

#[test]
fn obsolete_or_incomplete_editing_states_start_with_defaults_without_a_notice() {
    for version in [1, 2, 3] {
        let obsolete = version < 3;
        let mut editing = layer_ui::EditingState::default();
        editing.version = version;
        if obsolete {
            editing.region_values.insert("smoothing".into(), 0.5);
            assert_eq!(editing.validate(), Err("Unsupported editing-state version".into()));
        }
        let mut editing = serde_json::to_value(editing).unwrap();
        for field in if obsolete { &["fill_scope", "region_smooth_edges"][..] } else { &["preset"][..] } {
            editing.as_object_mut().unwrap().remove(*field);
        }
        let backend = Rc::new(Backend::default());
        let mut stored: serde_json::Value = serde_json::from_str(&backend.database.borrow().encoded().unwrap()).unwrap();
        stored["editing"] = serde_json::Value::String(editing.to_string());
        *backend.database.borrow_mut() = BrowserDatabase::decode(&stored.to_string()).unwrap();
        let (controller, mut host) = start(Store(backend), Platform::Web);
        assert_started(&controller, &mut host, "invalid editing state");
        assert_eq!(notice(&host), None);
        assert!(controller.manager.editing().is_none());
        assert_eq!(host.session.editing_state().version, 3);
        assert_eq!(host.session.editing_state().fill_scope, layer_ui::FillScope::Connected);
        assert!(host.session.editing_state().region_smooth_edges);
        assert!(host.session.editing_state().region_stop_at_colors);
    }
}

#[test]
fn unknown_saved_slots_use_a_builtin_layout_without_aliasing_or_resetting_storage() {
    for slot in ["lasso_fill", "unknown_slot"] {
        let backend = Rc::new(Backend::default());
        let layout = layer_ui::WorkspacePreset::Illustrator.layout(Platform::Web);
        let capture = layer_ui::WorkspaceCapture::from_template(&layout).unwrap();
        let entity = Entity::workspace("Saved Paint", capture, layout, 1000);
        let id = entity.id.clone();
        let batch = CommitBatch::prepare(Owner::fresh(), vec![Mutation::Create { entity, claim: false, name_policy: NamePolicy::Unique }]).unwrap();
        pollster::block_on(Store(backend.clone()).execute(StoreRequest::Commit { batch })).unwrap();
        let stored = backend.database.borrow().encoded().unwrap().replace("\"slot\":\"fill\"", &format!("\"slot\":\"{slot}\""));
        *backend.database.borrow_mut() = BrowserDatabase::decode(&stored).unwrap();
        let (controller, mut host) = start(Store(backend), Platform::Web);
        assert_started(&controller, &mut host, slot);
        assert_eq!(notice(&host), None);
        assert_eq!(host.session.capture_workspace().unwrap().history.layout(), &layer_ui::WorkspacePreset::Illustrator.layout(Platform::Web));
        assert!(pollster::block_on(controller.manager.load(&id)).is_err());
        assert_eq!(controller.manager.items().len(), DEFAULT_WORKSPACES.len() + 1);
        assert!(!controller.manager.in_memory());
    }
}
#[test]
fn startup_notices_relabel_in_place_and_do_not_resurrect_dismissed_or_replaced_notices() {
    for (reset_heals, replace) in [(false, false), (false, true), (true, false), (true, true)] {
        let (mut controller, mut host) = start(FaultyStore::new(0, reset_heals), Platform::Web);
        assert!(controller.view.ready);
        let before = host.session.state().notice.clone().unwrap();
        let original = controller.manager.current().unwrap();
        let japanese = Localizer::shared(UiLanguage::Japanese);
        host.session.set_localization(japanese.clone());
        controller.set_localization(japanese);
        controller.tick(&mut host.session, 1000);
        let after = host.session.state().notice.as_ref().unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.actions.iter().map(|a| a.id).collect::<Vec<_>>(), before.actions.iter().map(|a| a.id).collect::<Vec<_>>());
        assert_ne!(after.text, before.text);
        assert_eq!(controller.manager.current().unwrap(), original);
        let copy = after.text.clone();
        controller.tick(&mut host.session, 1000);
        assert_eq!(host.session.state().notice.as_ref().unwrap().text, copy);
        if replace {
            host.session.notify("a later literal notice");
        } else {
            host.session.dispatch(UiAction::Notice { id: before.id, accept: false, action: None }).unwrap();
        }
        let replacement = host.session.state().notice.clone();
        let korean = Localizer::shared(UiLanguage::Korean);
        host.session.set_localization(korean.clone());
        controller.set_localization(korean);
        controller.tick(&mut host.session, 1000);
        let current = host.session.state().notice.as_ref();
        assert_eq!(current.map(|n| (n.id, &n.text, &n.actions)), replacement.as_ref().map(|n| (n.id, &n.text, &n.actions)));
        if let (Some(current), Some(replacement)) = (current, replacement.as_ref()) {
            assert_ne!(current.name, replacement.name);
        }
    }
}
fn notice(host: &layer_host::NativeHost) -> Option<String> {
    host.session.state().notice.as_ref().map(|n| n.text.clone())
}
fn assert_started<S: WorkspaceStore + 'static>(
    controller: &WorkspaceController<S>,
    host: &mut layer_host::NativeHost,
    context: &str,
) {
    assert!(
        controller.view.ready,
        "{context}: {:?}",
        controller.view.error
    );
    assert!(controller.view.error.is_none(), "{context}");
    assert!(!controller.view.busy, "{context}");
    assert_eq!(
        controller.manager.current().map(|e| e.id),
        Some(DEFAULT_WORKSPACES.iter().find(|(_, preset)| *preset == layer_ui::WorkspacePreset::initial(host.session.state().platform, false)).unwrap().0.to_string()),
        "{context}"
    );
    assert!(host.session.capture_workspace().is_ok(), "{context}");
    assert!(host.session.state().workspace_ready, "{context}");
}

#[test]
fn startup_on_every_platform_adopts_from_empty_or_unusable_storage() {
    for platform in Platform::ALL {
        let (controller, mut host) = start(FaultyStore::new(usize::MAX, false), platform);
        assert_started(&controller, &mut host, &format!("{platform:?} empty"));
        assert_eq!(controller.view.name, if matches!(platform, Platform::Android | Platform::Ios) { "Sketch" } else { "Paint" });
        assert!(!controller.manager.in_memory());
        assert_eq!(notice(&host), None);

        let (controller, mut host) = start(FaultyStore::new(0, false), platform);
        assert_started(&controller, &mut host, &format!("{platform:?} unusable"));
        assert!(controller.manager.in_memory());
        assert_eq!(
            notice(&host).as_deref(),
            Some(
                "Workspace changes in this window won't be saved: This workspace cannot be opened. Choose another workspace or reset this one."
            )
        );
    }
}

#[test]
fn mobile_browsers_start_in_sketch_and_desktop_browsers_start_in_paint() {
    for (mobile, expected) in [(false, "Paint"), (true, "Sketch")] {
        let store = Store(Rc::new(Backend::default()));
        let mut host = layer_host::NativeHost::new(Platform::Web).unwrap();
        let mut controller = WorkspaceController::new_localized(store, Platform::Web, 1000, Localizer::shared(UiLanguage::English))
            .with_mobile_browser(mobile);
        assert!(!host.session.state().workspace_ready);
        for _ in 0..20 { controller.tick(&mut host.session, 1000); }
        assert!(host.session.state().workspace_ready);
        assert_eq!(controller.view.name, expected);
        controller.input(&mut host.session, WorkspaceInput::Switch { id: DEFAULT_WORKSPACES[1].0.into() }, 1100).unwrap();
        for _ in 0..20 { controller.tick(&mut host.session, 1100); }
        assert_eq!(layer_ui::durable_layout(&host.session.state().workspace.layout),
            layer_ui::WorkspacePreset::Illustrator.layout_for_device(Platform::Web, mobile));
    }
}

#[test]
fn workspace_controls_become_ready_with_the_restored_layout_on_every_platform() {
    for platform in Platform::ALL {
        let store = Store(Rc::new(Backend::default()));
        store.0.now.set(1000);
        let manager = WorkspaceManager::new(store.clone(), platform);
        let incoming = pollster::block_on(manager.initialize(1000)).unwrap();
        manager.activate(incoming);
        let saved = pollster::block_on(manager.prepare_switch(DEFAULT_WORKSPACES[2].0, 1000)).unwrap();
        let expected = saved.entity.capture().unwrap().history.layout().clone();
        manager.activate(saved);
        pollster::block_on(manager.close()).unwrap();
        let (release, hold) = async_channel::bounded(1);
        *store.0.switcher_gate.borrow_mut() = Some(hold);
        let mut host = layer_host::NativeHost::new(platform).unwrap();
        let mut controller = WorkspaceController::new_localized(store, platform, 1000, Localizer::shared(UiLanguage::English));
        for _ in 0..20 {
            controller.tick(&mut host.session, 1000);
            assert!(!controller.view.ready, "{platform:?}");
            assert!(!host.session.state().workspace_ready, "{platform:?}");
        }
        release.try_send(()).unwrap();
        for _ in 0..20 {
            controller.tick(&mut host.session, 1000);
            if host.session.state().workspace_ready {
                assert!(controller.view.ready, "{platform:?}");
                assert_eq!(controller.view.name, "Photo", "{platform:?}");
                assert_eq!(layer_ui::durable_layout(&host.session.state().workspace.layout), layer_ui::durable_layout(&expected), "{platform:?}");
                break;
            }
        }
        assert!(host.session.state().workspace_ready, "{platform:?}");
    }
}

#[test]
fn startup_on_every_platform_restores_the_last_selected_workspace() {
    pollster::block_on(async {
        for platform in Platform::ALL {
            let store = Store(Rc::new(Backend::default()));
            store.0.now.set(1000);
            let mut manager = WorkspaceManager::new(store.clone(), platform);
            let incoming = manager.initialize(1000).await.unwrap();
            manager.activate(incoming);
            let custom = manager.create_workspace("My Workspace", 1000).await.unwrap();
            let custom_id = custom.entity.id.clone();
            manager.release(&custom).await;
            for id in [DEFAULT_WORKSPACES[1].0, DEFAULT_WORKSPACES[2].0, DEFAULT_WORKSPACES[0].0, &custom_id] {
                let incoming = manager.prepare_switch(id, 1000).await.unwrap();
                let expected = incoming.entity.capture().unwrap();
                let outgoing = manager.activate(incoming).unwrap();
                manager.release(&outgoing).await;
                manager.close().await.unwrap();

                manager = WorkspaceManager::new(store.clone(), platform);
                let restored = manager.initialize(1000).await.unwrap();
                assert_eq!(restored.entity.id, id, "{platform:?}");
                assert_eq!(restored.entity.capture().unwrap(), expected, "{platform:?}: {id}");
                manager.activate(restored);
            }
            manager.close().await.unwrap();
        }
    });
}

#[test]
fn startup_survives_a_failure_at_every_storage_request() {
    let clean = FaultyStore::new(usize::MAX, false);
    let (_, _) = start(clean.clone(), Platform::Web);
    let requests = clean.0.requests.get();
    assert!(requests > 10);
    for fail_from in 0..requests {
        for reset_heals in [true, false] {
            let store = FaultyStore::new(fail_from, reset_heals);
            let (mut controller, mut host) = start(store.clone(), Platform::Web);
            let context = format!("failing from request {fail_from}, reset heals: {reset_heals}");
            assert_started(&controller, &mut host, &context);
            assert!(store.0.reset.get() || fail_from > 0, "{context}");
            if !store.0.reset.get() {
                assert!(!controller.manager.in_memory(), "{context}");
                assert_eq!(notice(&host), None, "{context}");
                continue;
            }
            assert_eq!(controller.manager.in_memory(), !reset_heals, "{context}");
            let expected = if reset_heals {
                "Saved workspaces couldn't be opened, so they were reset."
            } else {
                "Workspace changes in this window won't be saved: This workspace cannot be opened. Choose another workspace or reset this one."
            };
            assert_eq!(notice(&host).as_deref(), Some(expected), "{context}");
            let before = host.session.editing_state();
            host.dispatch(UiAction::Invoke {
                command: layer_ui::CommandId::Eraser,
            })
            .unwrap();
            controller.observe(&mut host.session, 2000);
            for _ in 0..3 {
                controller.tick(&mut host.session, 2400);
            }
            assert!(controller.view.error.is_none(), "{context}");
            assert!(!controller.manager.dirty(), "{context}");
            assert_ne!(
                *controller.manager.editing().unwrap(),
                before
            );
            if reset_heals {
                let StoreResponse::List(items) = store
                    .0
                    .database
                    .borrow_mut()
                    .execute(StoreRequest::List, 1000)
                    .unwrap()
                else {
                    panic!()
                };
                assert_eq!(items.len(), DEFAULT_WORKSPACES.len(), "{context}");
            }
        }
    }
}

#[test]
fn startup_replaces_only_storage_it_cannot_read() {
    let clean = FaultyStore::new(usize::MAX, false);
    let (_, _) = start(clean.clone(), Platform::Web);
    for fail_from in 0..clean.0.requests.get() {
        for kind in [ErrorKind::Unavailable, ErrorKind::Conflict, ErrorKind::OwnedElsewhere, ErrorKind::FailedWrite, ErrorKind::StorageFull] {
            let store = FaultyStore::failing(fail_from, true, kind.clone());
            let (controller, mut host) = start(store.clone(), Platform::Web);
            let context = format!("{kind:?} from request {fail_from}");
            assert_started(&controller, &mut host, &context);
            assert!(!store.0.reset.get(), "{context}");
        }
    }
}

fn start_native(
    directory: &std::path::Path,
) -> (WorkspaceController<StoreWorker>, layer_host::NativeHost) {
    let mut host = layer_host::NativeHost::new(Platform::Gtk).unwrap();
    let mut controller =
        WorkspaceController::new_localized(StoreWorker::shared(directory).unwrap(), Platform::Gtk, 1000, Localizer::shared(UiLanguage::English));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !controller.view.ready && std::time::Instant::now() < deadline {
        controller.tick(&mut host.session, 1000);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    (controller, host)
}
fn close_native(
    mut controller: WorkspaceController<StoreWorker>,
    host: &mut layer_host::NativeHost,
) {
    controller
        .input(&mut host.session, WorkspaceInput::Close, 1000)
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !controller.view.closed && std::time::Instant::now() < deadline {
        controller.tick(&mut host.session, 1000);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(controller.view.closed);
}

#[test]
fn sqlite_startup_replaces_workspaces_of_the_same_version_it_cannot_read() {
    let directory = crate::test_support::temp_dir("workspace-stale");
    let (controller, mut host) = start_native(&directory);
    assert_started(&controller, &mut host, "first start");
    close_native(controller, &mut host);
    let db = rusqlite::Connection::open(directory.join("workspaces.sqlite3")).unwrap();
    let stale = db
        .execute(
            "UPDATE items SET working=json_set(working,'$.zen_mode',json('{}'))",
            [],
        )
        .unwrap();
    assert_eq!(stale, DEFAULT_WORKSPACES.len());

    let (controller, mut host) = start_native(&directory);
    assert_started(&controller, &mut host, "stale start");
    assert!(!controller.manager.in_memory());
    assert_eq!(
        notice(&host).as_deref(),
        Some("Saved workspaces couldn't be opened, so they were reset.")
    );
    close_native(controller, &mut host);

    let (controller, mut host) = start_native(&directory);
    assert_started(&controller, &mut host, "restart");
    assert_eq!(notice(&host), None);
    close_native(controller, &mut host);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn sqlite_startup_keeps_a_newer_store_and_runs_in_memory() {
    let directory = crate::test_support::temp_dir("workspace-newer");
    let (controller, mut host) = start_native(&directory);
    close_native(controller, &mut host);
    let db = rusqlite::Connection::open(directory.join("workspaces.sqlite3")).unwrap();
    db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let rows = || -> u32 {
        db.query_row("SELECT count(*) FROM items", [], |r| r.get(0))
            .unwrap()
    };
    let before = rows();

    let (controller, mut host) = start_native(&directory);
    assert_started(&controller, &mut host, "newer store");
    assert!(controller.manager.in_memory());
    assert!(
        notice(&host)
            .unwrap()
            .contains("A newer version of Capy Canvas updated workspace storage.")
    );
    close_native(controller, &mut host);
    let version: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
    assert_eq!(rows(), before);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn workspace_projection_uses_one_identity_without_publishing_or_dirtying() {
    for language in layer_ui::UiLanguage::ALL {
        let backend = Rc::new(Backend::default());
        backend.now.set(1000);
        let localization = layer_ui::Localizer::shared(language);
        let manager = WorkspaceManager::new_localized(Store(backend.clone()), Platform::Gtk, localization.clone());
        let mut stored = pollster::block_on(manager.initialize(1000)).unwrap();
        stored.entity.metadata.name = "stored name suffix 2".into();
        let id = stored.entity.id.clone();
        manager.activate(stored.clone());
        let before = serde_json::to_string(&manager.current().unwrap()).unwrap();
        let publications = backend.deliveries.borrow().len();
        let expected = workspace_display_name(&id, &stored.entity.metadata, &localization);
        assert_eq!(manager.active_name().as_deref(), Some(expected.as_str()));
        assert_eq!(manager.binding().unwrap().name, expected);
        assert_eq!(manager.details(&stored, true, 1000).title, expected);
        let rows = manager.rows(ManagerPage::Workspaces, "Ｐａｉｎｔ", 1000);
        assert_eq!(rows[0].title, expected);
        let prompt = manager.form_prompt(&ManagerAction::Delete(id), Some(&stored.entity.metadata)).unwrap();
        assert!(prompt.message.contains(&expected));
        assert!(!prompt.message.contains("stored name suffix 2"));
        assert!(!manager.dirty());
        assert_eq!(serde_json::to_string(&manager.current().unwrap()).unwrap(), before);
        assert_eq!(backend.deliveries.borrow().len(), publications);
    }
}
