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
        let controller = WorkspaceController::new(
            Store(backend.clone()),
            Platform::Web,
            "legacy:test".into(),
            Some(capture),
            1000,
        );
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
fn previews_cancel_pending_replies_and_never_publish_temporary_layouts() {
    let mut f = Fixture::new();
    let original = f.controller.view.id.clone().unwrap();
    f.input(serde_json::json!({"type":"form","kind":"new","id":null}));
    f.input(serde_json::json!({"type":"submit","name":"Painting","source":null}));
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
    f.input(serde_json::json!({"type":"filter","query":"Does not match anything"}));
    assert!(f.controller.view.selected.is_none());
    assert!(!f.controller.view.enabled);
    assert_eq!(
        f.host.session.state().workspace.layout,
        capture.history.layout().clone()
    );
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
    let defaults: Vec<_> = f
        .controller
        .view
        .defaults
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(defaults.len(), 3);
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
    f.input(serde_json::json!({"type":"form","kind":"reset_brushes"}));
    f.input(serde_json::json!({"type":"cancel"}));
    assert_eq!(f.host.session.capture_workspace().unwrap(), edited);
    f.input(serde_json::json!({"type":"form","kind":"reset_brushes"}));
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
    f.input(serde_json::json!({"type":"form","kind":"rename","id":painter}));
    f.input(serde_json::json!({"type":"submit","name":"My Painter"}));
    assert_eq!(
        f.controller
            .view
            .defaults
            .iter()
            .find(|r| r.id == painter)
            .unwrap()
            .title,
        "My Painter"
    );
    f.input(serde_json::json!({"type":"open","page":"workspaces"}));
    let row = f
        .controller
        .view
        .rows
        .iter()
        .find(|r| r.id == painter)
        .unwrap();
    assert!(row.options);
    assert!(!row.delete);
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
    f.input(serde_json::json!({"type":"form","kind":"new"}));
    f.backend.fail.set(true);
    f.input(serde_json::json!({"type":"submit","name":"Painting"}));
    assert!(f.controller.view.error.is_some());
    assert!(f.controller.view.retry);
    assert_eq!(f.controller.view.id, original);
    let failed = f.backend.deliveries.borrow().last().unwrap().clone();
    f.input(serde_json::json!({"type":"cancel"}));
    assert!(
        f.controller.view.error.is_some(),
        "Cancel keeps the failed delivery available for recovery"
    );
    f.input(serde_json::json!({"type":"form","kind":"new"}));
    f.backend.fail.set(false);
    f.input(serde_json::json!({"type":"submit","name":"Painting"}));
    assert!(f.controller.view.error.is_none());
    assert_ne!(f.controller.view.id, original);
    assert_eq!(f.controller.view.name, "Painting");
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
fn interrupted_capture_migration_retries_the_same_operation_once() {
    let backend = Rc::new(Backend::default());
    backend.now.set(1000);
    let store = Store(backend.clone());
    let manager = WorkspaceManager::new(store.clone(), Platform::Web);
    let mut host = layer_host::NativeHost::new(Platform::Web).unwrap();
    host.dispatch(
        serde_json::from_value(serde_json::json!({"type":"set_brush_size","value":73.0})).unwrap(),
    )
    .unwrap();
    let capture = host.session.capture_workspace().unwrap();
    backend.fail.set(true);
    assert!(
        pollster::block_on(manager.migrate_legacy_capture("interrupted", capture.clone(), 1000))
            .is_err()
    );
    backend.fail.set(false);
    // A new process discovers the durable pending delivery rather than creating
    // a second batch/import/history from whatever defaults it loaded this time.
    let restarted = WorkspaceManager::new(store, Platform::Web);
    pollster::block_on(restarted.migrate_legacy_capture("interrupted", capture.clone(), 1001))
        .unwrap();
    pollster::block_on(manager.migrate_legacy_capture("interrupted", capture.clone(), 1002))
        .unwrap();
    let deliveries = backend.deliveries.borrow();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[0], deliveries[1]);
    let StoreResponse::Binding(Some(id)) = backend
        .database
        .borrow_mut()
        .execute(
            StoreRequest::LegacyImport {
                source: "interrupted".into(),
            },
            1002,
        )
        .unwrap()
    else {
        panic!()
    };
    let stored = pollster::block_on(manager.load(&id)).unwrap();
    let mut expected = capture;
    for revision in expected.history.revisions.values_mut() {
        revision.timestamp_ms = 1000;
    }
    assert_eq!(stored.entity.capture().unwrap(), expected);
}

#[test]
fn closing_before_adoption_drains_claims_and_unreadable_legacy_stays_intact() {
    let mut f = Fixture::new();
    f.input(serde_json::json!({"type":"suspend"}));
    let mut closing = WorkspaceController::new(
        Store(f.backend.clone()),
        Platform::Web,
        "legacy:test".into(),
        None,
        1000,
    );
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
    let backend = Rc::new(Backend::default());
    backend.now.set(1000);
    let mut broken = WorkspaceController::new(
        Store(backend.clone()),
        Platform::Web,
        "unreadable".into(),
        None,
        1000,
    );
    broken.legacy_error("Unsupported legacy version".into(), 1000);
    for _ in 0..20 {
        broken.tick(&mut f.host.session, 1000);
    }
    assert!(!broken.view.ready);
    assert!(
        broken
            .view
            .error
            .as_ref()
            .unwrap()
            .contains("Unsupported legacy version")
    );
    let StoreResponse::List(rows) = backend
        .database
        .borrow_mut()
        .execute(StoreRequest::List, 1000)
        .unwrap()
    else {
        panic!()
    };
    assert!(
        rows.is_empty(),
        "Unrecognized legacy data must not be replaced with defaults"
    );
}
