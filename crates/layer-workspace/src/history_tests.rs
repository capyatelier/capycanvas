use crate::*;
use layer_ui::{
    CommandId, DockLayout, HeaderAction, HeaderSize, LayoutChange, LayoutEdit, LayoutHistory, LayoutRevision, Localizer, Platform,
    PreparedWorkspace, UiAction, UiLanguage, WorkspaceWorkingState,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

const NOW: u64 = 1_000_000;
const REVISIONS: u64 = 2_000;
const CURRENT: f32 = 60.;
const PREVIOUS: f32 = 40.;
const OLDER: f32 = 30.;
const CORRUPT: f32 = 20.;

fn shifted(amount: f32) -> DockLayout {
    let mut layout = DockLayout::default();
    layout.bands[0].extent += amount;
    layout
}
fn extent(layout: &DockLayout) -> f32 {
    layout.bands[0].extent - DockLayout::default().bands[0].extent
}
fn workspace(name: &str, history: LayoutHistory) -> Entity {
    Entity {
        id: new_id(),
        metadata: Metadata::new(ItemKind::Workspace, name, NOW),
        content: ItemContent::Workspace { history, baseline: Box::new(DockLayout::default()) },
        working: Some(WorkspaceWorkingState::default()),
    }
}
fn long_history() -> Entity {
    let mut history = LayoutHistory::new(&DockLayout::default());
    for n in 1..REVISIONS {
        let amount = match n {
            n if n == REVISIONS - 1 => CURRENT,
            n if n == REVISIONS - 2 || n == 7 => PREVIOUS,
            n if n == REVISIONS - 10 => OLDER,
            _ => CORRUPT,
        };
        let id = format!("r{n}");
        history.revisions.insert(id.clone(), LayoutRevision { id, layout: shifted(amount), description: LayoutChange::Restored, timestamp_ms: n });
    }
    history.generation = REVISIONS - 1;
    history.current = format!("r{}", REVISIONS - 1);
    history.undo = (REVISIONS - 101..REVISIONS - 1).map(|n| format!("r{n}")).collect();
    workspace("Long History", history)
}

trait Backend {
    type Store: WorkspaceStore + Clone + 'static;
    fn store(&self) -> Self::Store;
    fn make_unreadable(&self, layout: &DockLayout);
    fn content(&self, id: &str) -> (serde_json::Value, Result<ItemContent, StoreError>);
    fn seed(&self, entity: Entity) -> String {
        let id = entity.id.clone();
        let batch = CommitBatch::prepare(Owner::fresh(), vec![Mutation::Create { entity, claim: false, name_policy: NamePolicy::Exact }]).unwrap();
        pollster::block_on(self.store().execute(StoreRequest::Commit { batch })).unwrap();
        id
    }
    fn long(&self) -> String {
        let id = self.seed(long_history());
        self.make_unreadable(&shifted(CORRUPT));
        id
    }
}

struct Native {
    directory: std::path::PathBuf,
    worker: StoreWorker,
}
impl Native {
    fn new() -> Self {
        let directory = crate::test_support::temp_dir("workspace-history");
        Self { worker: StoreWorker::shared(&directory).unwrap(), directory }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.join("workspaces.sqlite3")).unwrap()
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
impl Backend for Native {
    type Store = StoreWorker;
    fn store(&self) -> StoreWorker {
        self.worker.clone()
    }
    fn make_unreadable(&self, layout: &DockLayout) {
        let packed = protocol::pack(serde_json::to_value(layout).unwrap(), &mut Default::default()).unwrap();
        let id = packed["$workspace_component"].as_str().unwrap().to_string();
        assert_eq!(self.sql().execute("UPDATE components SET json='{\"corrupt\":true}' WHERE id=?1", [id]).unwrap(), 1);
    }
    fn content(&self, id: &str) -> (serde_json::Value, Result<ItemContent, StoreError>) {
        let sql = self.sql();
        let text: String = sql.query_row("SELECT content FROM items WHERE id=?1", [id], |row| row.get(0)).unwrap();
        let resolved = protocol::resolve(&text, |component| {
            Ok(sql.query_row("SELECT json FROM components WHERE id=?1", [component], |row| row.get(0)).unwrap())
        });
        (serde_json::from_str(&text).unwrap(), resolved.and_then(ItemContent::read))
    }
}

#[derive(Clone, Default)]
struct Web(Rc<RefCell<BrowserDatabase>>);
impl WorkspaceStore for Web {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        if let StoreRequest::Commit { batch } = &request {
            self.0.borrow_mut().prepare_delivery(batch)?;
        }
        self.0.borrow_mut().execute(request, NOW)
    }
}
impl Web {
    fn database(&self) -> serde_json::Value {
        serde_json::from_str(&self.0.borrow().encoded().unwrap()).unwrap()
    }
}
impl Backend for Web {
    type Store = Web;
    fn store(&self) -> Web {
        self.clone()
    }
    fn make_unreadable(&self, layout: &DockLayout) {
        let layout = serde_json::to_value(layout).unwrap();
        let mut database = self.database();
        for item in database["items"].as_object_mut().unwrap().values_mut() {
            for revision in item["entity"]["content"]["history"]["revisions"].as_object_mut().into_iter().flat_map(|r| r.values_mut()) {
                if revision["layout"] == layout {
                    revision["layout"] = serde_json::json!({"corrupt": true});
                }
            }
        }
        *self.0.borrow_mut() = BrowserDatabase::decode(&database.to_string()).unwrap();
    }
    fn content(&self, id: &str) -> (serde_json::Value, Result<ItemContent, StoreError>) {
        let content = self.database()["items"][id]["entity"]["content"].take();
        (content.clone(), ItemContent::read(content))
    }
}

#[derive(Clone, Default)]
struct Gated {
    web: Web,
    reads: Rc<RefCell<Vec<(String, String)>>>,
    gate: Rc<RefCell<Option<async_channel::Receiver<()>>>>,
}
impl WorkspaceStore for Gated {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        let gate = match &request {
            StoreRequest::Revision { id, revision } => {
                self.reads.borrow_mut().push((id.clone(), revision.clone()));
                self.gate.borrow_mut().take()
            }
            _ => None,
        };
        let reply = self.web.execute(request).await;
        if let Some(gate) = gate {
            gate.recv().await.unwrap();
        }
        reply
    }
}

struct Unavailable<S> {
    store: S,
    failing: std::cell::Cell<bool>,
}
impl<S: WorkspaceStore> WorkspaceStore for Unavailable<S> {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        if self.failing.get() && !matches!(request, StoreRequest::Release { .. }) {
            return Err(StoreError::new(ErrorKind::Unavailable, "Storage is unavailable"));
        }
        self.store.execute(request).await
    }
}

fn revision_ids(content: &serde_json::Value) -> Vec<String> {
    content["history"]["revisions"].as_object().unwrap().keys().cloned().collect()
}

struct Window<S: WorkspaceStore + 'static> {
    controller: WorkspaceController<S>,
    host: layer_host::NativeHost,
    now: u64,
}
impl<S: WorkspaceStore + 'static> Window<S> {
    fn start(store: S) -> Self {
        let mut host = crate::test_support::uncovered(layer_host::NativeHost::new(Platform::Gtk).unwrap());
        host.session.set_viewport([1200., 900.], [1200, 900]).unwrap();
        let controller = WorkspaceController::new_localized(store, Platform::Gtk, 1000, Localizer::shared(UiLanguage::English), Arc::new(|| {}));
        let mut window = Self { controller, host, now: 1000 };
        window.settle(|w| w.controller.view.ready);
        window
    }
    fn settle(&mut self, done: impl Fn(&Self) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            self.controller.tick(&mut self.host.session, self.now);
            if done(self) {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "{:?}", self.controller.view.error);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn input(&mut self, input: serde_json::Value) {
        self.controller.input(&mut self.host.session, serde_json::from_value(input).unwrap(), self.now).unwrap();
        self.settle(|w| !w.controller.view.busy);
    }
    fn switch(&mut self, id: &str) {
        self.input(serde_json::json!({"type":"switch","id":id}));
        self.settle(|w| w.controller.manager.active_id().as_deref() == Some(id) && !w.controller.view.busy);
    }
    fn dispatch(&mut self, action: UiAction) {
        self.host.dispatch(action).unwrap();
        self.controller.observe(&mut self.host.session, self.now);
    }
    fn invoke(&mut self, command: CommandId) {
        self.dispatch(UiAction::Invoke { command });
        self.settle(|w| w.host.session.workspace_layout_read().is_none());
        assert!(self.controller.view.error.is_none(), "{:?}", self.controller.view.error);
    }
    fn header_edit(&mut self) -> DockLayout {
        let before = self.layout();
        let size = if before.header.size == HeaderSize::Large { HeaderSize::Small } else { HeaderSize::Large };
        for action in [HeaderAction::Edit { editing: true }, HeaderAction::SetSize { size }, HeaderAction::Edit { editing: false }] {
            self.dispatch(action.action());
        }
        self.save();
        assert_ne!(self.layout(), before);
        before
    }
    fn layout(&self) -> DockLayout {
        layer_ui::durable_layout(&self.host.session.state().workspace.layout)
    }
    fn save(&mut self) {
        self.now += 300;
        self.settle(|w| !w.controller.manager.dirty() && !w.controller.manager.saving());
    }
    fn extent(&self) -> f32 {
        extent(&layer_ui::durable_layout(&self.host.session.state().workspace.layout))
    }
}

fn switch_and_save_read_only_the_current_layout(backend: impl Backend) {
    let id = backend.long();
    let m = WorkspaceManager::new(backend.store(), Platform::Gtk);
    pollster::block_on(async {
        let incoming = m.prepare_switch(&id, NOW).await.unwrap();
        assert!(!incoming.fallback && !incoming.replaced, "the current layout and baseline are readable");
        let mut capture = incoming.entity.capture().unwrap();
        assert_eq!(extent(capture.history.layout()), CURRENT);
        assert_eq!(capture.history.head.undo.len(), 100);
        PreparedWorkspace::new(capture.clone()).unwrap();
        m.activate(incoming);
        capture.history.append(&shifted(CURRENT + 20.), LayoutChange::Automatic);
        m.observe(capture, NOW + 1);
        m.flush().await.unwrap();
    });
    let (stored, _) = backend.content(&id);
    let revisions = revision_ids(&stored);
    assert_eq!(revisions.len() as u64, REVISIONS + 1);
    assert!((0..=REVISIONS).all(|n| revisions.contains(&format!("r{n}"))));
    assert_eq!(stored["history"]["current"], format!("r{REVISIONS}"));
    assert_eq!(stored["history"]["undo"].as_array().unwrap().last().unwrap(), &format!("r{}", REVISIONS - 1));
    assert_eq!(stored["history"]["undo"].as_array().unwrap().len(), 100);
}

#[test]
fn a_long_history_with_unreadable_old_layouts_still_switches_and_saves() {
    switch_and_save_read_only_the_current_layout(Native::new());
    switch_and_save_read_only_the_current_layout(Web::default());
}

fn undo_and_redo_read_one_stored_layout_each(backend: impl Backend) {
    let id = backend.long();
    let mut window = Window::start(backend.store());
    window.switch(&id);
    assert_eq!(window.extent(), CURRENT);
    window.invoke(CommandId::UndoWorkspace);
    assert_eq!(window.extent(), PREVIOUS);
    window.invoke(CommandId::UndoWorkspace);
    assert_eq!(window.extent(), OLDER, "unreadable revisions leave Undo");
    window.invoke(CommandId::RedoWorkspace);
    assert_eq!(window.extent(), PREVIOUS);
    window.invoke(CommandId::RedoWorkspace);
    assert_eq!(window.extent(), CURRENT);
    assert!(!window.host.session.command(CommandId::RedoWorkspace).enabled);
    window.save();
    let (stored, _) = backend.content(&id);
    let undo: Vec<String> = serde_json::from_value(stored["history"]["undo"].clone()).unwrap();
    assert_eq!(undo.len(), 93);
    assert_eq!(undo.last().unwrap(), &format!("r{}", REVISIONS - 2));
    assert!(!undo.contains(&format!("r{}", REVISIONS - 3)));
    assert_eq!(revision_ids(&stored).len() as u64, REVISIONS);
}

#[test]
fn undo_and_redo_read_only_their_target_and_skip_unreadable_ones() {
    undo_and_redo_read_one_stored_layout_each(Native::new());
    undo_and_redo_read_one_stored_layout_each(Web::default());
}

fn layout_history_lists_without_bodies_and_restores_one(backend: impl Backend) {
    let id = backend.long();
    let mut window = Window::start(backend.store());
    window.switch(&id);
    window.input(serde_json::json!({"type":"open","page":"history"}));
    let rows = &window.controller.view.rows;
    assert_eq!(rows.len() as u64, REVISIONS);
    assert_eq!(rows[0].id, format!("r{}", REVISIONS - 1));
    assert!(rows[0].current);
    window.input(serde_json::json!({"type":"select","id":"r7"}));
    window.settle(|w| w.controller.view.enabled);
    assert!(window.controller.view.error.is_none(), "{:?}", window.controller.view.error);
    assert_eq!(window.extent(), PREVIOUS, "the selected revision previews");
    assert_eq!(window.host.session.capture_workspace().unwrap().history.head.generation, REVISIONS - 1);
    window.input(serde_json::json!({"type":"confirm"}));
    assert!(window.controller.view.page.is_none());
    assert_eq!(window.extent(), PREVIOUS);
    let capture = window.host.session.capture_workspace().unwrap();
    assert_eq!(capture.history.head.current.description, LayoutChange::RestoredEarlier);
    assert_eq!(capture.history.head.undo.last().unwrap(), &format!("r{}", REVISIONS - 1));
    window.save();
    let (stored, _) = backend.content(&id);
    assert_eq!(revision_ids(&stored).len() as u64, REVISIONS + 1);
    assert_eq!(stored["history"]["current"], format!("r{REVISIONS}"));
    window.invoke(CommandId::UndoWorkspace);
    assert_eq!(window.extent(), CURRENT);
}

#[test]
fn layout_history_lists_revisions_without_their_layouts_and_restores_one() {
    layout_history_lists_without_bodies_and_restores_one(Native::new());
    layout_history_lists_without_bodies_and_restores_one(Web::default());
}

async fn save<S: WorkspaceStore>(m: &WorkspaceManager<S>, session: &mut layer_ui::WorkspaceCapture) {
    m.observe(session.clone(), NOW);
    m.flush().await.unwrap();
    session.history.acknowledge(&m.take_acknowledged());
}
fn edit_sequence(edit: &mut LayoutEdit, mut navigate: impl FnMut(&mut LayoutEdit, bool), mut saved: impl FnMut(&mut LayoutEdit)) {
    for n in 1..=105 {
        edit.append(&shifted(n as f32), LayoutChange::Restored);
        if n % 20 == 0 {
            saved(edit);
        }
    }
    saved(edit);
    for redo in [false, false, false, true] {
        navigate(edit, redo);
        saved(edit);
    }
    edit.append(&shifted(200.), LayoutChange::Restored);
    saved(edit);
}

fn edits_write_what_the_complete_history_holds(backend: impl Backend) {
    let history = LayoutHistory::new(&DockLayout::default());
    let id = backend.seed(Entity::workspace("Semantics", history, WorkspaceWorkingState::default(), DockLayout::default(), NOW));
    let m = WorkspaceManager::new(backend.store(), Platform::Gtk);
    let mut session = pollster::block_on(m.prepare_switch(&id, NOW)).unwrap().entity.capture().unwrap();
    m.activate(pollster::block_on(m.load(&id)).unwrap());
    let reads = std::cell::Cell::new(0);
    let mut history = session.history.clone();
    edit_sequence(
        &mut history,
        |edit, redo| {
            if !edit.step(redo) {
                let target = edit.target(redo).unwrap().clone();
                reads.set(reads.get() + 1);
                assert!(edit.navigate(redo, pollster::block_on(m.layout_revision(&id, &target)).unwrap()));
            }
        },
        |edit| {
            session.history = edit.clone();
            pollster::block_on(save(&m, &mut session));
            *edit = session.history.clone();
        },
    );
    assert!(reads.get() > 0, "navigation read stored revisions");
    let mut expected = LayoutEdit::starting(&DockLayout::default());
    edit_sequence(&mut expected, |edit, redo| assert!(edit.step(redo)), |_| {});
    let expected = expected.into_history();
    let (_, Ok(ItemContent::Workspace { history, .. })) = backend.content(&id) else { panic!() };
    assert_eq!(
        (history.version, history.generation, &history.current, &history.undo, &history.redo),
        (expected.version, expected.generation, &expected.current, &expected.undo, &expected.redo)
    );
    assert_eq!((history.undo.len(), history.undo[0].as_str()), (99, "r5"));
    assert!(history.redo.is_empty());
    assert_eq!(history.revisions.keys().collect::<Vec<_>>(), expected.revisions.keys().collect::<Vec<_>>());
    for (id, revision) in &history.revisions {
        let wanted = &expected.revisions[id];
        assert_eq!((&revision.layout, &revision.description), (&wanted.layout, &wanted.description), "{id}");
        assert_ne!(revision.timestamp_ms, 0, "{id}");
    }
}

#[test]
fn layout_edits_append_navigate_and_cap_exactly_like_the_complete_history() {
    edits_write_what_the_complete_history_holds(Native::new());
    edits_write_what_the_complete_history_holds(Web::default());
}

#[test]
fn retention_measures_stored_revisions_in_full_and_removes_the_oldest_first() {
    let backend = Native::new();
    let mut history = LayoutHistory::new(&DockLayout::default());
    for n in 1..=6u64 {
        let id = format!("r{n}");
        let mut layout = shifted(n as f32);
        layout.add_toolbar(None, &format!("Tools {n}"), &[]).unwrap();
        history.revisions.insert(id.clone(), LayoutRevision { id, layout, description: LayoutChange::Restored, timestamp_ms: 10 - n });
    }
    let id = backend.seed(workspace("Retained", history));
    let sql = backend.sql();
    let text: String = sql.query_row("SELECT content FROM items WHERE id=?1", [&id], |row| row.get(0)).unwrap();
    let mut read = |component: &str| sql.query_row("SELECT json FROM components WHERE id=?1", [component], |row| row.get::<_, String>(0)).ok();
    let content = crate::stored_workspace::StoredWorkspace::parse(&text).unwrap();
    let mut lengths = Default::default();
    let sizes: Vec<(String, u64)> = content
        .unreachable()
        .map(|(id, raw)| (id.clone(), crate::stored_workspace::resolved_len(raw.get(), &mut read, &mut lengths)))
        .collect();
    assert_eq!(sizes.len(), 6);
    for (id, size) in &sizes {
        let raw = serde_json::to_value(&content).unwrap()["history"]["revisions"][id].to_string();
        let resolved = protocol::resolve(&raw, |component| Ok(read(component).unwrap())).unwrap();
        assert_eq!(*size, serde_json::to_string(&resolved).unwrap().len() as u64, "{id}");
    }
    let total: u64 = sizes.iter().map(|(_, size)| size).sum();
    let oldest = |count: usize| {
        let mut sizes = sizes.clone();
        sizes.sort_by_key(|(id, _)| std::cmp::Reverse(id[1..].parse::<u64>().unwrap()));
        sizes.into_iter().take(count).collect::<Vec<_>>()
    };
    for count in [1, 2] {
        let removed: u64 = oldest(count).iter().map(|(_, size)| size).sum();
        let mut items = vec![crate::retention::Retained { id: id.clone(), claim: None, content: crate::stored_workspace::StoredWorkspace::parse(&text).unwrap() }];
        let changed = crate::retention::retention_plan(&mut items, None, NOW, total - removed, |raw| {
            crate::stored_workspace::resolved_len(raw.get(), &mut read, &mut Default::default())
        });
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), [0]);
        let kept: Vec<String> = items[0].content.unreachable().map(|(id, _)| id.clone()).collect();
        assert_eq!(kept.len(), 6 - count);
        assert!(oldest(count).iter().all(|(id, _)| !kept.contains(id)));
    }
}

#[test]
fn an_interrupted_layout_save_recovers_onto_the_stored_history() {
    let backend = Native::new();
    let id = backend.long();
    let m = WorkspaceManager::new(backend.store(), Platform::Gtk);
    pollster::block_on(async {
        let incoming = m.prepare_switch(&id, NOW).await.unwrap();
        let mut capture = incoming.entity.capture().unwrap();
        m.activate(incoming);
        assert_eq!(
            m.layout_revision(&id, "r6").await.unwrap_err().known,
            Some(WorkspaceRefusal::ThisLayoutVersionIsNoLongerRetained)
        );
        backend.sql().execute_batch("CREATE TRIGGER interrupt_layout BEFORE UPDATE OF content ON items BEGIN SELECT RAISE(ABORT,'interrupted save'); END;").unwrap();
        capture.history.append(&shifted(CURRENT + 5.), LayoutChange::Restored);
        m.observe(capture, NOW + 1);
        assert!(m.flush().await.is_err());
        backend.sql().execute_batch("DROP TRIGGER interrupt_layout;").unwrap();
        m.release_owner().await.unwrap();
        let other = WorkspaceManager::new(backend.store(), Platform::Gtk);
        other.refresh().await.unwrap();
        let changes = other.interrupted_changes(NOW).await.unwrap();
        assert_eq!(changes.len(), 1);
        let recovered = other.recover_interrupted(&changes[0].0, NOW).await.unwrap().unwrap();
        assert_eq!(extent(recovered.entity.layout().unwrap()), CURRENT + 5.);
        let ItemContent::Workspace { history, .. } = other.item(&recovered.entity.id).await.unwrap().content else { panic!() };
        assert_eq!(history.revisions.len(), 6, "only readable revisions copy");
        assert_eq!(history.current, format!("r{REVISIONS}"));
        assert_eq!(history.undo.iter().rev().take(3).collect::<Vec<_>>(), [&format!("r{}", REVISIONS - 1), &format!("r{}", REVISIONS - 2), &format!("r{}", REVISIONS - 10)]);
    });
}

#[test]
fn an_undo_reply_applies_only_to_the_workspace_that_asked_for_it() {
    const PAINT: &str = DEFAULT_WORKSPACES[1].0;
    const PHOTO: &str = DEFAULT_WORKSPACES[2].0;
    let store = Gated::default();
    let mut window = Window::start(store.clone());
    window.switch(PAINT);
    let paint = window.header_edit();
    window.switch(PHOTO);
    let photo = window.header_edit();
    assert_ne!(paint, photo);
    window.switch(PAINT);
    let (release, gate) = async_channel::bounded(1);
    *store.gate.borrow_mut() = Some(gate);
    window.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    window.switch(PHOTO);
    window.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    let _ = release.try_send(());
    window.settle(|w| w.host.session.workspace_layout_read().is_none() && !w.controller.view.busy);
    assert_eq!(window.controller.manager.active_id().as_deref(), Some(PHOTO));
    assert!(window.layout() == photo, "Photo's Undo restores Photo's layout; Paint's layout: {}", window.layout() == paint);
    assert_eq!(store.reads.borrow().as_slice(), [(PAINT.to_string(), "r0".to_string()), (PHOTO.to_string(), "r0".to_string())]);
}

#[test]
fn undo_of_a_stored_revision_reads_it_during_the_dispatch_that_asked() {
    let store = Gated::default();
    let mut window = Window::start(store.clone());
    let before = window.header_edit();
    window.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    assert_eq!(store.reads.borrow().len(), 1, "the store read starts without waiting for a tick");
    let requests = serde_json::to_string(&window.host.session.state().requests).unwrap();
    assert!(requests.contains("read_layout"), "the host is asked to service the read: {requests}");
    window.controller.tick(&mut window.host.session, window.now);
    assert_eq!(window.layout(), before, "one tick applies the stored layout");
}

#[test]
fn export_falls_back_to_the_held_layouts_when_storage_cannot_be_read() {
    let backend = Native::new();
    let id = backend.long();
    let m = WorkspaceManager::new(Unavailable { store: backend.store(), failing: Default::default() }, Platform::Gtk);
    pollster::block_on(async {
        let incoming = m.prepare_switch(&id, NOW).await.unwrap();
        let mut capture = incoming.entity.capture().unwrap();
        m.activate(incoming);
        capture.history.append(&shifted(CURRENT + 5.), LayoutChange::Restored);
        m.observe(capture, NOW + 1);
        m.store.failing.set(true);
        let entity = m.complete_current().await.unwrap();
        let imported = import_package(&export_package(&entity).unwrap(), PackageKind::WorkspaceBackup, NOW).unwrap();
        let ItemContent::Workspace { history, .. } = imported.content else { panic!() };
        assert_eq!(extent(history.layout()), CURRENT + 5.);
        m.store.failing.set(false);
    });
}

#[test]
fn restoring_a_workspace_layout_appends_to_the_stored_history() {
    let store = Web::default();
    let mut window = Window::start(store.clone());
    let id = window.controller.manager.active_id().unwrap();
    let edited = { window.header_edit(); window.layout() };
    let mut workspace = window.host.session.state().workspace.clone();
    workspace.layout = layer_ui::durable_layout(&workspace.layout);
    workspace.layout.canvas_bar = !workspace.layout.canvas_bar;
    let restored = workspace.layout.clone();
    window.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    window.now += 300;
    window.settle(|w| !w.controller.manager.saving() && (w.controller.manager.error().is_some() || !w.controller.manager.dirty()));
    assert_eq!(window.controller.manager.error(), None);
    let (stored, _) = store.content(&id);
    assert_eq!(revision_ids(&stored), ["r0", "r1", "r2"]);
    assert_eq!(stored["history"]["current"], "r2");
    assert_eq!(window.layout(), restored);
    window.invoke(CommandId::UndoWorkspace);
    assert_eq!(window.layout(), edited);
}

#[test]
fn undo_and_redo_of_stored_revisions_return_exactly_the_layouts_they_left() {
    let store = Web::default();
    let base = DockLayout::default();
    assert!(base.phone.is_none());
    let mut history = LayoutHistory::new(&base);
    let mut edit = LayoutEdit::starting(&base);
    edit.append(&shifted(10.), LayoutChange::Restored);
    history.revisions.extend(edit.unsaved.clone());
    (history.current, history.generation, history.undo) = (edit.head.current.id.clone(), 1, vec!["r0".into()]);
    let id = store.seed(Entity::workspace("Phone defaults", history, WorkspaceWorkingState::default(), base.clone(), NOW));
    let mut window = Window::start(store.clone());
    window.switch(&id);
    let viewport = [1200., 900.];
    let gesture = |window: &mut Window<Web>, position: [f32; 2]| {
        let before = window.layout();
        window.dispatch(UiAction::MovePanel { panel: layer_ui::Panel::Layers, target: layer_ui::DockTarget::Float { position }, viewport });
        window.save();
        let after = window.layout();
        assert_ne!(after, before);
        window.invoke(CommandId::UndoWorkspace);
        assert_eq!(window.layout(), before, "Undo returns exactly the layout before the gesture");
        window.invoke(CommandId::RedoWorkspace);
        assert_eq!(window.layout(), after, "Redo returns the gesture");
    };
    assert!(window.layout().phone.is_some());
    gesture(&mut window, [480., 220.]);
    window.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(layer_ui::WorkspaceState { layout: base.clone(), ..Default::default() }) });
    window.save();
    assert!(window.layout().phone.is_none());
    gesture(&mut window, [520., 260.]);
}
