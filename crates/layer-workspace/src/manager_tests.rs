use super::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Wake, Waker},
    time::{Duration, Instant},
};

struct TestStore {
    worker: StoreWorker,
    fail: Cell<bool>,
    gate: RefCell<Option<async_channel::Receiver<()>>>,
    waiting: Cell<bool>,
}
impl WorkspaceStore for TestStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        let commit = matches!(&request, StoreRequest::Commit { .. });
        if commit && self.fail.get() {
            return Err(StoreError::new(
                ErrorKind::FailedWrite,
                "Simulated failed write",
            ));
        }
        let response = self.worker.request(request).await?;
        if commit {
            let gate = self.gate.borrow_mut().take();
            if let Some(gate) = gate {
                self.waiting.set(true);
                gate.recv().await.unwrap();
                self.waiting.set(false);
            }
        }
        Ok(response)
    }
}
struct Fixture {
    directory: std::path::PathBuf,
    manager: WorkspaceManager<TestStore>,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("capy-workspace-manager-{}", new_id()));
        let worker = StoreWorker::shared(&directory).unwrap();
        let manager = WorkspaceManager::new(
            TestStore {
                worker,
                fail: Cell::new(false),
                gate: RefCell::new(None),
                waiting: Cell::new(false),
            },
            Platform::Gtk,
        );
        let incoming = pollster::block_on(manager.initialize(1_000)).unwrap();
        manager.activate(incoming);
        Self { directory, manager }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn manager_recovery_library_and_backup_round_trip() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let template = m
            .save_template("Illustration", "Original", 2_000)
            .await
            .unwrap();
        let painting = m
            .create_workspace("Painting", Some(&template), false, 3_000)
            .await
            .unwrap();
        m.activate(painting.clone());
        let original = painting.entity.capture().unwrap().history.layout().clone();
        let mut capture = painting.entity.capture().unwrap();
        let mut changed = original.clone();
        changed.bands[0].extent += 80.;
        capture.history.append(&changed, "Resize panels");
        capture.working.zen_mode = true;
        capture
            .working
            .tools
            .set_override(capture.working.preset, "size", 87.)
            .unwrap();
        m.observe(capture.clone(), 4_000);
        m.update_reusable(
            &template,
            ReusableContent::Layout {
                layout: changed.clone(),
            },
            5_000,
        )
        .await
        .unwrap();
        m.rename(&template, "New Illustration", "Changed", 6_000)
            .await
            .unwrap();
        m.delete_item(&template, None, 7_000).await.unwrap();
        let reset = m
            .change_layout(&painting.entity.id, None, 8_000)
            .await
            .unwrap();
        let reset_capture = reset.entity.capture().unwrap();
        assert_eq!(reset_capture.history.layout(), &original);
        assert_eq!(reset_capture.working, capture.working);
        m.activate(reset);
        let mut recovered = m.current().unwrap().capture().unwrap();
        assert!(recovered.history.undo());
        assert_eq!(recovered.history.layout(), &changed);
        m.observe(recovered.clone(), 9_000);
        m.flush().await.unwrap();
        let source = m.current().unwrap();
        let bytes = export_package(&source).unwrap();
        let restored = m.import_workspace_package(&bytes, 10_000).await.unwrap();
        assert_ne!(restored.entity.id, source.id);
        assert_ne!(restored.entity.metadata.name, source.metadata.name);
        assert_eq!(restored.entity.working, source.working);
        let mut backup = restored.entity.capture().unwrap();
        assert_eq!(backup.history.layout(), &changed);
        assert!(backup.history.redo());
        assert_eq!(backup.history.layout(), &original);
        assert!(
            backup
                .history
                .revisions
                .keys()
                .all(|id| !recovered.history.revisions.contains_key(id))
        );
        let ItemContent::Workspace { baseline, .. } = &restored.entity.content else {
            panic!()
        };
        assert_eq!(baseline, &original);
        m.release(&restored).await;
        m.restore_deleted(&template, 11_000).await.unwrap();
        let template_record = m.load(&template).await.unwrap();
        assert!(template_record.entity.metadata.deleted_at_ms.is_none());
        assert_eq!(
            template_record.entity.metadata.previous[0].name,
            "Illustration"
        );
        let ItemContent::Reusable { previous, .. } = &template_record.entity.content else {
            panic!()
        };
        assert_eq!(previous.len(), 1);
        m.restore_reusable_version(&template, &previous[0].id, 12_000)
            .await
            .unwrap();
        let template_bytes = export_package(&m.load(&template).await.unwrap().entity).unwrap();
        let imported_template =
            import_package(&template_bytes, PackageKind::Template, 13_000).unwrap();
        assert!(imported_template.working.is_none());
        assert!(
            matches!(imported_template.content,ItemContent::Reusable {ref previous,..} if previous.is_empty())
        );
        assert!(import_package(&template_bytes, PackageKind::WorkspaceBackup, 13_000).is_err());
        let toolbar_panel = original
            .panels
            .iter()
            .find(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
            .unwrap()
            .id;
        let library = m
            .save_toolbar(toolbar_panel, "Reusable Tools", 14_000)
            .await
            .unwrap();
        let toolbar_bytes = export_package(&m.load(&library).await.unwrap().entity).unwrap();
        let imported_library = m
            .import_reusable_package(&toolbar_bytes, PackageKind::Toolbar, 15_000)
            .await
            .unwrap();
        assert_eq!(
            m.load(&imported_library)
                .await
                .unwrap()
                .entity
                .metadata
                .name,
            "Reusable Tools (2)"
        );
        m.delete_item(&library, None, 16_000).await.unwrap();
        m.delete_permanently(&library).await.unwrap();
        assert!(m.load(&library).await.is_err());
        assert_eq!(
            m.current().unwrap().capture().unwrap().history.layout(),
            &changed
        );
    });
}

#[test]
fn package_validation_and_failed_publication_never_expose_partial_imports() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let bytes = export_package(&m.current().unwrap()).unwrap();
        let before = m.items().len();
        let mut corrupt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let components = corrupt["components"].as_object_mut().unwrap();
        let key = components.keys().next().unwrap().clone();
        components[&key][0] = 0.into();
        assert!(
            m.import_workspace_package(&serde_json::to_vec(&corrupt).unwrap(), 2_000)
                .await
                .is_err()
        );
        let mut missing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        missing["components"].as_object_mut().unwrap().remove(&key);
        assert!(
            import_package(
                &serde_json::to_vec(&missing).unwrap(),
                PackageKind::WorkspaceBackup,
                2_000
            )
            .is_err()
        );
        let mut newer: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        newer["version"] = 99.into();
        assert_eq!(
            import_package(
                &serde_json::to_vec(&newer).unwrap(),
                PackageKind::WorkspaceBackup,
                2_000
            )
            .unwrap_err()
            .kind,
            ErrorKind::UnsupportedSchema
        );
        m.store.fail.set(true);
        assert!(m.import_workspace_package(&bytes, 2_000).await.is_err());
        m.store.fail.set(false);
        m.refresh().await.unwrap();
        assert_eq!(m.items().len(), before);
        assert_eq!(m.active_name().as_deref(), Some("My Workspace"));
        assert!(m.import_workspace_package(&bytes, 2_000).await.is_ok());
    });
}

#[test]
fn template_creation_duplication_switching_and_original_baselines_are_independent() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        assert_eq!(m.active_name().as_deref(), Some("My Workspace"));
        let initial = m.current().unwrap();
        let mut capture = initial.capture().unwrap();
        let mut layout = capture.history.layout().clone();
        layout.bands[0].extent += 100.;
        capture.history.append(&layout, "Resize panels");
        capture.working.zen_mode = true;
        capture
            .working
            .tools
            .set_override(capture.working.preset, "size", 91.)
            .unwrap();
        m.observe(capture.clone(), 2_000);
        let template_id = m
            .save_template("Illustration", "Layout only", 3_000)
            .await
            .unwrap();
        let template = m.load(&template_id).await.unwrap();
        assert!(template.entity.working.is_none());
        let duplicate = m
            .create_workspace("Experiment", None, true, 4_000)
            .await
            .unwrap();
        assert_eq!(
            duplicate.entity.capture().unwrap().history.revisions.len(),
            2
        );
        assert_eq!(duplicate.entity.capture().unwrap().working, capture.working);
        let ItemContent::Workspace { baseline, .. } = duplicate.entity.content else {
            unreachable!()
        };
        let ItemContent::Workspace {
            baseline: original, ..
        } = initial.content
        else {
            unreachable!()
        };
        assert_eq!(baseline, original);
        let fresh = m
            .create_workspace("Inking", Some(&template_id), false, 5_000)
            .await
            .unwrap();
        let fresh_capture = fresh.entity.capture().unwrap();
        assert_eq!(fresh_capture.history.layout(), &layout);
        assert_eq!(fresh_capture.history.revisions.len(), 1);
        assert!(!fresh_capture.working.zen_mode);
        assert!(fresh_capture.working.tools.overrides.is_empty());
        let outgoing = m.activate(fresh).unwrap();
        m.release(&outgoing).await;
        let original = m.prepare_switch(&initial.id, 6_000).await.unwrap();
        assert_eq!(original.entity.capture().unwrap().working, capture.working);
        assert_eq!(
            original.entity.capture().unwrap().history.revisions.len(),
            2
        );
        m.close().await.unwrap();
    });
}

#[test]
fn failed_outgoing_save_prevents_switch_and_retains_accepted_edits_for_retry() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let source = m.current().unwrap();
        let target = m
            .create_workspace("Inking", None, false, 2_000)
            .await
            .unwrap();
        m.release(&target).await;
        let mut working = source.working.unwrap();
        working.zen_mode = true;
        m.observe_working(working.clone());
        m.store.fail.set(true);
        assert!(m.prepare_switch(&target.entity.id, 3_000).await.is_err());
        assert_eq!(m.active_id().as_deref(), Some(source.id.as_str()));
        assert!(m.dirty());
        assert_eq!(m.current().unwrap().working, Some(working.clone()));
        m.store.fail.set(false);
        let mut latest = working;
        latest.colors.foreground = [0.3, 0.4, 0.5, 1.];
        m.observe_working(latest.clone());
        m.flush().await.unwrap();
        assert!(!m.dirty());
        assert!(m.error().is_none());
        assert_eq!(
            m.load(&source.id).await.unwrap().entity.working,
            Some(latest)
        );
        let missing = m.prepare_switch("missing", 4_000).await.unwrap_err();
        assert_eq!(missing.kind, ErrorKind::NotFound);
        assert_eq!(m.active_id().as_deref(), Some(source.id.as_str()));
        m.close().await.unwrap();
    });
}
struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}
#[test]
fn late_save_completion_keeps_newer_dirty_values_and_unrelated_errors() {
    let f = Fixture::new();
    let m = &f.manager;
    let mut working = m.current().unwrap().working.unwrap();
    working.zen_mode = true;
    m.observe_working(working.clone());
    let (open, gate) = async_channel::bounded(1);
    *m.store.gate.borrow_mut() = Some(gate);
    let mut saving = Box::pin(m.save_once());
    let waker = Waker::from(Arc::new(Noop));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !m.store.waiting.get() {
        assert!(saving.as_mut().poll(&mut context).is_pending());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    working
        .tools
        .set_override(working.preset, "size", 137.)
        .unwrap();
    m.observe_working(working.clone());
    let newer_error = StoreError::new(ErrorKind::Conflict, "Ownership needs revalidation");
    m.set_error(newer_error.clone());
    open.try_send(()).unwrap();
    pollster::block_on(saving).unwrap();
    assert!(m.dirty());
    assert_eq!(m.error(), Some(newer_error));
    assert_eq!(m.current().unwrap().working, Some(working.clone()));
    pollster::block_on(m.save_once()).unwrap();
    assert_eq!(
        pollster::block_on(m.load(&m.active_id().unwrap()))
            .unwrap()
            .entity
            .working,
        Some(working)
    );
    pollster::block_on(m.close()).unwrap();
}
