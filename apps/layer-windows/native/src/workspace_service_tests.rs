use super::*;
use layer_ui::{DockLayout, UiAction};
use layer_workspace::{OWNER_LEASE_MS, StoreResponse, StoreWorker, new_id};
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    sync::mpsc,
    task::{Poll, Waker},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Faults {
    fail: Cell<bool>,
    hold: Cell<bool>,
    waiting: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}
struct TestStore {
    worker: StoreWorker,
    faults: Rc<Faults>,
}
impl WorkspaceStore for TestStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        let commit = matches!(request, StoreRequest::Commit { .. });
        if commit && self.faults.fail.get() {
            return Err(StoreError::new(
                layer_workspace::ErrorKind::FailedWrite,
                "Simulated write failure",
            ));
        }
        let response = self.worker.request(request).await?;
        if commit && self.faults.hold.get() {
            self.faults.waiting.set(true);
            poll_fn(|cx| {
                if self.faults.hold.get() {
                    *self.faults.waker.borrow_mut() = Some(cx.waker().clone());
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            self.faults.waiting.set(false);
        }
        Ok(response)
    }
}
fn wall() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
struct Fixture {
    directory: std::path::PathBuf,
    service: WorkspaceService<TestStore>,
    native: NativeHost,
    faults: Rc<Faults>,
    notifications: mpsc::Receiver<()>,
    now: Instant,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("capy-windows-workspaces-{}", new_id()));
        let worker = StoreWorker::shared(&directory).unwrap();
        let faults = Rc::new(Faults::default());
        let (notify, notifications) = mpsc::channel();
        let mut service = WorkspaceService::new(
            TestStore {
                worker,
                faults: faults.clone(),
            },
            directory.clone(),
            move || {
                let _ = notify.send(());
            },
        );
        let mut native = NativeHost::new(Platform::Windows).unwrap();
        crate::workspace::initialize(&mut native).unwrap();
        service.start(wall());
        Self {
            directory,
            service,
            native,
            faults,
            notifications,
            now: Instant::now(),
        }
    }
    fn pump(&mut self, until: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.service.poll(&mut self.native, self.now, wall());
            if until(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Workspace operation timed out: {:?}",
                self.service.status()
            );
            let _ = self.notifications.recv_timeout(Duration::from_millis(5));
        }
    }
    fn ready(&mut self) {
        self.pump(|f| f.service.status().ready);
    }
    fn close(&mut self) {
        self.service.request_close(&mut self.native);
        self.pump(|f| f.service.status().close_ready);
    }
    fn dispose(mut self) {
        self.service.stop();
        let directory = self.directory.clone();
        drop(self);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn startup_waits_for_canvas_idle_then_close_restores_layout_and_working_values() {
    let mut f = Fixture::new();
    f.native.startup.complete = false;
    f.native.startup.brush_ready = false;
    assert!(!f.service.accepts_input(wall()));
    f.pump(|f| f.service.incoming.is_some());
    assert!(!f.service.status().ready);
    // Optional shader warmup can continue after the active brush is ready.
    f.native.startup.brush_ready = true;
    f.ready();
    assert!(f.service.accepts_input(wall()));
    assert_eq!(
        f.native.session.state().workspace.layout,
        DockLayout::for_platform(Platform::Windows)
    );
    f.native
        .dispatch(UiAction::SetBrushSize { value: 47. })
        .unwrap();
    let previous = f.native.session.state().revision;
    let change = f
        .native
        .session
        .restore_workspace_layout(DockLayout::default(), "Test layout")
        .unwrap();
    f.native.apply_change(previous, change);
    let expected = f.native.session.capture_workspace().unwrap();
    let id = f.service.manager.active_id().unwrap();
    f.close();
    let worker = StoreWorker::shared(&f.directory).unwrap();
    let manager = WorkspaceManager::new(worker, Platform::Windows);
    let restored = pollster::block_on(manager.initialize(wall())).unwrap();
    assert_eq!(restored.entity.id, id);
    let actual = restored.entity.capture().unwrap();
    assert_eq!(actual.history.layout(), expected.history.layout());
    assert_eq!(actual.working, expected.working);
    assert_eq!(
        actual.history.revisions.len(),
        expected.history.revisions.len()
    );
    pollster::block_on(manager.release(&restored));
    drop(manager);
    f.dispose();
}

#[test]
fn edits_accepted_while_an_immutable_save_is_pending_are_included_in_close() {
    let mut f = Fixture::new();
    f.ready();
    f.faults.hold.set(true);
    f.native
        .dispatch(UiAction::SetBrushSize { value: 38. })
        .unwrap();
    f.service.poll(&mut f.native, f.now, wall());
    f.now += Duration::from_millis(300);
    f.pump(|f| f.faults.waiting.get());
    assert!(f.service.status().saving);
    assert!(f.service.accepts_input(wall()));
    f.native
        .dispatch(UiAction::SetBrushSize { value: 61. })
        .unwrap();
    f.service.poll(&mut f.native, f.now, wall());
    let expected = f.native.session.workspace_working_state();
    f.service.request_close(&mut f.native);
    assert!(!f.service.accepts_input(wall()));
    assert!(!f.service.status().close_ready);
    f.faults.hold.set(false);
    f.faults.waker.borrow_mut().take().unwrap().wake();
    f.pump(|f| f.service.status().close_ready);
    let id = f.service.manager.active_id().unwrap();
    let saved = pollster::block_on(f.service.manager.load(&id)).unwrap();
    assert_eq!(saved.entity.working, Some(expected));
    f.dispose();
}

#[test]
fn failed_close_keeps_the_window_and_dirty_values_available_for_retry() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 72. })
        .unwrap();
    let expected = f.native.session.workspace_working_state();
    f.faults.fail.set(true);
    f.service.request_close(&mut f.native);
    f.pump(|f| f.service.status().error.is_some());
    assert!(!f.service.status().close_ready);
    assert!(f.service.status().dirty);
    f.service.keep_open(&mut f.native);
    assert!(f.service.accepts_input(wall()));
    assert_eq!(f.native.session.workspace_working_state(), expected);
    f.faults.fail.set(false);
    f.service.retry(&mut f.native, wall());
    f.close();
    let id = f.service.manager.active_id().unwrap();
    let saved = pollster::block_on(f.service.manager.load(&id)).unwrap();
    assert_eq!(saved.entity.working, Some(expected));
    f.dispose();
}

#[test]
fn a_failed_startup_can_be_kept_open_and_retried_without_authorizing_close() {
    let mut f = Fixture::new();
    f.faults.fail.set(true);
    f.pump(|f| f.service.status().error.is_some());
    assert!(!f.service.status().ready);
    assert!(!f.service.accepts_input(wall()));
    let error = f.service.status().error.clone();
    f.service.request_close(&mut f.native);
    assert_eq!(f.service.status().error, error);
    assert!(!f.service.status().busy);
    assert!(!f.service.status().close_ready);
    f.service.keep_open(&mut f.native);
    assert!(!f.service.status().close_requested);
    f.faults.fail.set(false);
    f.service.retry(&mut f.native, wall());
    f.ready();
    f.close();
    f.dispose();
}

#[test]
fn backup_preserves_unsaved_values_without_claiming_that_a_failed_save_succeeded() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 79. })
        .unwrap();
    let expected = f.native.session.workspace_working_state();
    f.faults.fail.set(true);
    f.service.request_close(&mut f.native);
    f.pump(|f| f.service.status().error.is_some());
    f.service.keep_open(&mut f.native);
    let error = f.service.status().error.clone();
    let path = f.directory.join("working.capyworkspace");
    f.service
        .export_backup(&mut f.native, path.to_string_lossy().into_owned())
        .unwrap();
    f.pump(|f| f.service.status().notice.is_some() && !f.service.status().busy);
    let imported = layer_workspace::import_package(
        &std::fs::read(path).unwrap(),
        layer_workspace::PackageKind::WorkspaceBackup,
        wall(),
    )
    .unwrap();
    assert_eq!(imported.working, Some(expected));
    assert_eq!(f.service.status().error, error);
    assert!(f.service.status().dirty);
    f.service.request_close(&mut f.native);
    f.service.discard_close(&mut f.native).unwrap();
    f.pump(|f| f.service.status().close_ready);
    f.dispose();
}

#[test]
fn retry_preserves_the_identity_of_a_failed_save_as_new_operation() {
    let mut f = Fixture::new();
    f.ready();
    let original = f.service.manager.active_id().unwrap();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 66. })
        .unwrap();
    f.faults.fail.set(true);
    f.service
        .save_as_new(&mut f.native, "Recovered workspace".into(), wall())
        .unwrap();
    f.pump(|f| f.service.status().error.is_some());
    assert_eq!(f.service.manager.active_id().unwrap(), original);
    assert!(f.service.manager.has_failed_operation());
    f.faults.fail.set(false);
    f.service.retry(&mut f.native, wall());
    f.pump(|f| {
        f.service.manager.active_id().as_deref() != Some(original.as_str())
            && !f.service.status().busy
    });
    assert_eq!(
        f.service.status().name.as_deref(),
        Some("Recovered workspace")
    );
    assert_eq!(f.native.session.state().brush.diameter, 66.);
    assert!(!f.service.manager.has_failed_operation());
    assert_eq!(
        f.service
            .manager
            .items()
            .iter()
            .filter(|i| i.metadata.name == "Recovered workspace")
            .count(),
        1
    );
    f.close();
    f.dispose();
}

#[test]
fn pending_preferences_can_complete_while_workspace_ownership_is_read_only() {
    let mut native = NativeHost::new(Platform::Windows).unwrap();
    native
        .dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ToggleTheme,
        })
        .unwrap();
    let id = native
        .session
        .state()
        .requests
        .iter()
        .find(|r| matches!(r.kind, layer_ui::HostRequestKind::SaveSettings { .. }))
        .unwrap()
        .id;
    native.session.begin_workspace_transition().unwrap();
    native.session.set_workspace_read_only(true);
    native
        .dispatch(UiAction::CompleteRequest { id, error: None })
        .unwrap();
    native.dispatch(UiAction::CloseSettings).unwrap();
    assert!(native.session.state().requests.is_empty());
    assert!(
        native
            .dispatch(UiAction::SetBrushSize { value: 99. })
            .is_err()
    );
    native.session.end_workspace_transition();
}

#[test]
fn close_during_a_failing_autosave_reaches_a_recoverable_decision() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 83. })
        .unwrap();
    f.faults.fail.set(true);
    f.service.poll(&mut f.native, f.now, wall());
    f.now += Duration::from_millis(300);
    f.service.poll(&mut f.native, f.now, wall());
    assert!(f.service.status().saving);
    f.service.request_close(&mut f.native);
    f.pump(|f| f.service.status().error.is_some());
    assert!(!f.service.status().close_ready);
    assert!(
        !f.service.status().busy,
        "An ended save must expose close recovery"
    );
    assert!(f.service.status().dirty);
    f.service.keep_open(&mut f.native);
    f.faults.fail.set(false);
    f.service.retry(&mut f.native, wall());
    f.close();
    f.dispose();
}

#[test]
fn exporting_a_workspace_cannot_replace_the_live_database_or_its_sidecars() {
    let mut f = Fixture::new();
    f.ready();
    let id = f.service.manager.active_id().unwrap();
    for name in [
        "workspaces.sqlite3",
        "workspaces.sqlite3-wal",
        "workspaces.sqlite3-shm",
    ] {
        let path = f.directory.join(name);
        f.service
            .export_backup(&mut f.native, path.to_string_lossy().into_owned())
            .unwrap();
        f.pump(|f| f.service.status().notice.is_some() && !f.service.status().busy);
        assert!(
            f.service
                .status()
                .notice
                .as_ref()
                .unwrap()
                .contains("outside the live workspace database")
        );
        assert_eq!(
            pollster::block_on(f.service.manager.load(&id))
                .unwrap()
                .entity
                .id,
            id
        );
    }
    f.close();
    f.dispose();
}

#[test]
fn owner_takeover_blocks_editing_and_never_overwrites_the_successor() {
    let mut f = Fixture::new();
    f.ready();
    f.native
        .dispatch(UiAction::SetBrushSize { value: 53. })
        .unwrap();
    f.service.poll(&mut f.native, f.now, wall());
    let expected = f.native.session.workspace_working_state();
    let old = f.service.manager.current_record().unwrap();
    pollster::block_on(f.service.manager.release(&old));
    let other = WorkspaceManager::new(
        StoreWorker::shared(&f.directory).unwrap(),
        Platform::Windows,
    );
    let incoming = pollster::block_on(other.prepare_switch(&old.entity.id, wall())).unwrap();
    let generations = incoming.generations;
    other.activate(incoming);
    // Advance the observed wall clock past the cached lease. The actual
    // SQLite owner has already changed, so rejection must invalidate that
    // cached claim even if the clock subsequently moves backwards.
    let expired = wall() + OWNER_LEASE_MS + 1;
    f.service.poll(&mut f.native, f.now, expired);
    assert!(!f.service.accepts_input(expired));
    f.pump(|f| f.service.status().error.is_some());
    assert!(f.service.status().owner_lost);
    assert_eq!(f.native.session.workspace_working_state(), expected);
    let saved = pollster::block_on(other.load(&old.entity.id)).unwrap();
    assert_eq!(saved.generations, generations);
    assert_ne!(saved.entity.working, Some(expected));
    pollster::block_on(other.close()).unwrap();
    drop(other);
    f.dispose();
}
