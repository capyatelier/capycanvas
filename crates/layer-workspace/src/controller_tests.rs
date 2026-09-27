use crate::*;
use layer_ui::{Platform, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Backend {
    database: RefCell<BrowserDatabase>,
    now: Cell<u64>,
    fail: Cell<bool>,
    lose_reply: Cell<bool>,
    deliveries: RefCell<Vec<String>>,
    load_gate: RefCell<Option<async_channel::Receiver<()>>>,
    fail_renew: RefCell<Option<ErrorKind>>,
}
#[derive(Clone)]
struct Store(Rc<Backend>);
impl WorkspaceStore for Store {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
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
        let commit = matches!(request, StoreRequest::Commit { .. });
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
        let controller = WorkspaceController::new(Store(backend.clone()), Platform::Web, 1000);
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
        WorkspaceController::new_owned(Store(f.backend.clone()), Platform::Web, owner.clone(), 2000);
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
        WorkspaceController::new_owned(Store(f.backend.clone()), Platform::Web, owner, 1_000);
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
            let mut displayed = saved.clone();
            displayed.working.colors.set_document_depth(f.host.session.engine().document().color.depth).unwrap();
            displayed.working.colors.library.ensure_starters();
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
            history.append(&edited, "Customize old default");
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
            f.controller.manager.current().unwrap().starting_layout(Platform::Web).unwrap()
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
            f.input(if confirm {
                serde_json::json!({"type":"submit","name":""})
            } else {
                serde_json::json!({"type":"cancel"})
            });
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
fn default_switches_preserve_edits_and_brush_reset_is_working_state_only() {
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
    f.input(serde_json::json!({"type":"switch","id":"builtin:workspace:photographer"}));
    f.input(serde_json::json!({"type":"switch","id":painter}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), edited);
    f.input(serde_json::json!({"type":"form","action":{"type":"reset_brushes"}}));
    f.input(serde_json::json!({"type":"cancel"}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), edited);
    f.input(serde_json::json!({"type":"form","action":{"type":"reset_brushes"}}));
    f.input(serde_json::json!({"type":"submit","name":""}));
    let reset = f.host.session.capture_workspace().unwrap();
    assert_eq!(reset.history, edited.history);
    let mut expected = edited.working.clone();
    assert!(!expected.tools.overrides.is_empty());
    expected.tools.overrides.clear();
    assert_eq!(reset.working, expected);
    assert_eq!(
        f.controller
            .manager
            .current()
            .unwrap()
            .capture()
            .unwrap()
            .working,
        expected
    );
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
    let mut closing = WorkspaceController::new(Store(f.backend.clone()), Platform::Web, 1000);
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
    let mut displayed = original.clone();
    let colors = &mut displayed.entity.working.as_mut().unwrap().colors;
    colors.set_document_depth(f.host.session.engine().document().color.depth).unwrap();
    colors.library.ensure_starters();
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
    assert!(f.host.session.state().customization.is_open());
    assert!(f.controller.view.page.is_none());
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
        .map(|p| p.title().to_string())
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
    let before = f.host.session.state().workspace.layout.clone();
    f.input(serde_json::json!({"type":"form","action":{"type":"new_toolbar","value":null}}));
    let prompt = f.controller.view.prompt.clone().unwrap();
    assert_eq!(prompt.name.as_deref(), Some("New Toolbar"));
    assert_eq!(prompt.selected.as_deref(), Some(""));
    let existing = toolbar_names(&f)[0].clone();
    f.input(serde_json::json!({"type":"submit","name":existing}));
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
            .any(|p| p.title() == "Saved Ink")
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
