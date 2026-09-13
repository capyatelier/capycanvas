//! Corrupt stored bytes, not just in-memory layouts: this is the failure users
//! encounter when opening a default saved by an incompatible development build.
use crate::*;
use layer_ui::Platform;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

struct TestClock(AtomicU64);
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
struct Fixture {
    directory: std::path::PathBuf,
    sqlite: SqliteStore,
    browser: BrowserDatabase,
    clock: Arc<TestClock>,
    owner: Owner,
    entities: Vec<Entity>,
}
impl Fixture {
    fn new(platform: Platform) -> Self {
        let directory = std::env::temp_dir().join(format!("capy-default-recovery-{}", new_id()));
        let clock = Arc::new(TestClock(AtomicU64::new(1000)));
        let mut f = Self {
            sqlite: SqliteStore::with_clock(&directory.join("db.sqlite3"), clock.clone()).unwrap(),
            browser: BrowserDatabase::default(),
            directory,
            clock,
            owner: Owner::fresh(),
            entities: Vec::new(),
        };
        for (id, _) in DEFAULT_WORKSPACES {
            let mut entity = Entity::included_workspace(id, platform, 1000).unwrap();
            entity.working.as_mut().unwrap().zen_mode = true;
            f.entities.push(entity);
        }
        let mut custom = f.entities[0].clone();
        custom.id = new_id();
        custom.metadata.builtin = false;
        custom.metadata.name = "My Painting".into();
        f.entities.push(custom);
        let mut batch = CommitBatch::prepare(
            f.owner.clone(),
            f.entities
                .iter()
                .cloned()
                .map(|entity| Mutation::Create {
                    entity,
                    claim: true,
                    name_policy: NamePolicy::Exact,
                })
                .collect(),
        )
        .unwrap();
        batch.bindings.push((
            "last_workspace".into(),
            Some(DEFAULT_WORKSPACES[0].0.into()),
        ));
        f.both(StoreRequest::Commit { batch }).unwrap();
        let pins: Vec<_> = f.entities.iter().map(|e| e.id.clone()).rev().collect();
        f.both(StoreRequest::UpdateSwitcher {
            expected: None,
            ids: pins.clone(),
        })
        .unwrap();
        f.both(StoreRequest::UpdateWorkspaceOrder {
            expected: None,
            ids: pins,
        })
        .unwrap();
        f
    }
    fn both(&mut self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        let native = self.sqlite.handle(request.clone());
        let browser = self.browser.execute(request.clone(), self.clock.now_ms());
        match (&native, &browser) {
            (Ok(a), Ok(b)) => assert_eq!(
                serde_json::to_value(a).unwrap(),
                serde_json::to_value(b).unwrap(),
                "{request:?}"
            ),
            (Err(a), Err(b)) => assert_eq!(a.kind, b.kind, "{request:?}"),
            _ => panic!("{request:?}: native {native:?}, browser {browser:?}"),
        }
        native
    }
    fn claim(&mut self, id: &str) -> Result<StoredEntity, StoreError> {
        match self.both(StoreRequest::Claim {
            id: id.into(),
            owner: self.owner.clone(),
            reset_invalid_default: Some(Platform::Gtk),
        })? {
            StoreResponse::Entity(item) => Ok(*item),
            _ => panic!(),
        }
    }
    fn corrupt(&mut self, id: &str, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_str(&self.browser.encoded().unwrap()).unwrap();
        let entity = &mut value["items"][id]["entity"];
        change(entity);
        let db = rusqlite::Connection::open(self.directory.join("db.sqlite3")).unwrap();
        db.execute(
            "UPDATE items SET metadata=?2,content=?3,working=?4 WHERE id=?1",
            rusqlite::params![
                id,
                entity["metadata"].to_string(),
                entity["content"].to_string(),
                (!entity["working"].is_null()).then(|| entity["working"].to_string())
            ],
        )
        .unwrap();
        self.browser = BrowserDatabase::decode(&value.to_string()).unwrap();
    }
    fn snapshots(&mut self) -> Vec<StoredEntity> {
        self.entities
            .iter()
            .map(|e| self.sqlite.load(&e.id).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn included_name_updates_allow_only_canonical_names_on_both_backends() {
    let mut f = Fixture::new(Platform::Gtk);
    for (id, preset) in DEFAULT_WORKSPACES {
        f.corrupt(id, |e| {
            e["metadata"]["name"] = json!(format!("Old {}", preset.name()))
        });
        let saved = f.claim(id).unwrap();
        for name in ["Arbitrary rename", preset.name()] {
            let mut metadata = saved.entity.metadata.clone();
            metadata.name = name.into();
            let batch = CommitBatch::prepare(
                f.owner.clone(),
                vec![Mutation::Update {
                    id: id.into(),
                    generations: saved.generations,
                    fence: saved.claim.as_ref().unwrap().fence,
                    metadata: Some(metadata),
                    content: None,
                    working: None,
                    name_policy: NamePolicy::Unique,
                }],
            )
            .unwrap();
            let result = f.both(StoreRequest::Commit { batch });
            if name == "Arbitrary rename" {
                assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidData);
            } else {
                result.unwrap();
            }
        }
        let updated = f.claim(id).unwrap();
        assert_eq!(updated.entity.metadata.name, preset.name());
        assert_eq!(updated.entity.content, saved.entity.content);
        assert_eq!(updated.entity.working, saved.entity.working);
        assert_eq!(updated.generations.layout, saved.generations.layout);
        assert_eq!(updated.generations.working, saved.generations.working);
    }
}

#[test]
fn invalid_defaults_reset_individually_and_persist_on_both_backends() {
    for platform in [Platform::Gtk, Platform::Web] {
        for (index, (id, _)) in DEFAULT_WORKSPACES.iter().enumerate() {
            for corruption in 0..5 {
                let mut f = Fixture::new(platform);
                let before = f.snapshots();
                let pins = f.both(StoreRequest::Switcher).unwrap();
                let order = f.both(StoreRequest::WorkspaceOrder).unwrap();
                f.corrupt(id, |e| match corruption {
                    0 => e["working"]["colors"]["shape"] = json!("wheel"),
                    1 => e["content"]["unknown_layout_field"] = json!(true),
                    2 => e["working"]["version"] = json!(999),
                    3 => e["working"] = Value::Null,
                    _ => e["metadata"]["unknown_field"] = json!(true),
                });
                // Catalog inspection and ordinary reads never rewrite data.
                let damaged = f.browser.encoded().unwrap();
                f.sqlite.handle(StoreRequest::List).unwrap();
                f.browser.execute(StoreRequest::List, 1000).unwrap();
                assert_eq!(
                    f.both(StoreRequest::Load { id: (*id).into() })
                        .unwrap_err()
                        .kind,
                    ErrorKind::InvalidData
                );
                assert_eq!(f.browser.encoded().unwrap(), damaged);
                let request = StoreRequest::Claim {
                    id: (*id).into(),
                    owner: f.owner.clone(),
                    reset_invalid_default: Some(platform),
                };
                let StoreResponse::Entity(repaired) = f.both(request.clone()).unwrap() else {
                    panic!()
                };
                assert_eq!(
                    repaired.entity,
                    Entity::included_workspace(id, platform, 1000).unwrap()
                );
                assert_eq!(
                    repaired.generations,
                    Generations {
                        metadata: 2,
                        layout: 2,
                        working: 2
                    }
                );
                assert!(
                    repaired.claim.as_ref().unwrap().fence
                        > before[index].claim.as_ref().unwrap().fence
                );
                // A lost reply/repeated claim must not reset a healthy item again.
                let StoreResponse::Entity(again) = f.both(request).unwrap() else {
                    panic!()
                };
                assert_eq!(repaired, again);
                for (other, old) in before.iter().enumerate() {
                    if other != index {
                        let StoreResponse::Entity(kept) = f
                            .both(StoreRequest::Load {
                                id: old.entity.id.clone(),
                            })
                            .unwrap()
                        else {
                            panic!()
                        };
                        assert_eq!(*kept, *old);
                    }
                }
                for (request, expected) in [
                    (StoreRequest::Switcher, pins),
                    (StoreRequest::WorkspaceOrder, order),
                ] {
                    assert_eq!(
                        serde_json::to_value(f.both(request).unwrap()).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
                let persisted = SqliteStore::open(&f.directory.join("db.sqlite3"))
                    .unwrap()
                    .load(id)
                    .unwrap();
                assert_eq!(persisted, *repaired);
                f.browser = BrowserDatabase::decode(&f.browser.encoded().unwrap()).unwrap();
                f.both(StoreRequest::Load { id: (*id).into() }).unwrap();
                assert!(
                    matches!(f.both(StoreRequest::Binding { key: "last_workspace".into() }).unwrap(), StoreResponse::Binding(Some(id)) if id == DEFAULT_WORKSPACES[0].0)
                );
            }
        }
    }
}

#[test]
fn recovery_respects_live_owners_and_fences_stale_writes() {
    let mut f = Fixture::new(Platform::Gtk);
    let id = DEFAULT_WORKSPACES[0].0;
    let before = f.sqlite.load(id).unwrap();
    let old_owner = f.owner.clone();
    f.corrupt(id, |e| e["working"]["colors"]["shape"] = json!("wheel"));
    let damaged = f.browser.encoded().unwrap();
    f.owner = Owner::fresh();
    assert_eq!(f.claim(id).unwrap_err().kind, ErrorKind::OwnedElsewhere);
    assert_eq!(f.browser.encoded().unwrap(), damaged);
    f.clock.0.store(1000 + OWNER_LEASE_MS, Ordering::SeqCst);
    let repaired = f.claim(id).unwrap();
    assert!(repaired.claim.unwrap().fence > before.claim.as_ref().unwrap().fence);
    let batch = CommitBatch::prepare(
        old_owner,
        vec![Mutation::Update {
            id: id.into(),
            generations: before.generations,
            fence: before.claim.unwrap().fence,
            metadata: None,
            content: None,
            working: before.entity.working,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    assert_eq!(
        f.both(StoreRequest::Commit { batch }).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert!(!f.sqlite.load(id).unwrap().entity.working.unwrap().zen_mode);
}

#[test]
fn recovery_never_replaces_custom_workspaces_or_healthy_default_edits() {
    let mut f = Fixture::new(Platform::Gtk);
    for (id, _) in DEFAULT_WORKSPACES {
        let before = f.sqlite.load(id).unwrap();
        assert_eq!(f.claim(id).unwrap(), before);
    }
    let custom = f.entities[3].id.clone();
    f.corrupt(&custom, |e| {
        e["working"]["colors"]["shape"] = json!("wheel")
    });
    let damaged = f.browser.encoded().unwrap();
    assert_eq!(f.claim(&custom).unwrap_err().kind, ErrorKind::InvalidData);
    assert_eq!(f.browser.encoded().unwrap(), damaged);
    // Even a stable default ID must actually be marked as an included workspace.
    let id = DEFAULT_WORKSPACES[0].0;
    f.corrupt(id, |e| {
        e["metadata"]["builtin"] = json!(false);
        e["working"]["version"] = json!(999);
    });
    rusqlite::Connection::open(f.directory.join("db.sqlite3"))
        .unwrap()
        .execute("UPDATE items SET builtin=0 WHERE id=?1", [id])
        .unwrap();
    assert_eq!(f.claim(id).unwrap_err().kind, ErrorKind::InvalidData);
}

#[test]
fn sqlite_reset_rolls_back_on_write_failure_and_retries_cleanly() {
    let mut f = Fixture::new(Platform::Gtk);
    let id = DEFAULT_WORKSPACES[0].0;
    f.corrupt(id, |e| e["working"]["colors"]["shape"] = json!("wheel"));
    let db = rusqlite::Connection::open(f.directory.join("db.sqlite3")).unwrap();
    let snapshot = || {
        db.query_row("SELECT metadata,content,working,metadata_generation,layout_generation,working_generation,fence,owner FROM items WHERE id=?1", [id], |r| (0..8).map(|i| r.get::<_, String>(i)).collect::<rusqlite::Result<Vec<_>>>()).unwrap()
    };
    let before = snapshot();
    db.execute_batch("CREATE TRIGGER fail_reset BEFORE UPDATE OF working ON items BEGIN SELECT RAISE(FAIL, 'simulated disk failure'); END;").unwrap();
    let request = StoreRequest::Claim {
        id: id.into(),
        owner: f.owner.clone(),
        reset_invalid_default: Some(Platform::Gtk),
    };
    assert_eq!(
        f.sqlite.handle(request.clone()).unwrap_err().kind,
        ErrorKind::FailedWrite
    );
    assert_eq!(snapshot(), before);
    db.execute_batch("DROP TRIGGER fail_reset").unwrap();
    f.both(request).unwrap();
}

#[test]
fn sqlite_reset_replaces_malformed_json_and_missing_components() {
    for content in ["{bad-json", r#"{"$workspace_component":"missing"}"#] {
        let mut f = Fixture::new(Platform::Gtk);
        let id = DEFAULT_WORKSPACES[0].0;
        let db = rusqlite::Connection::open(f.directory.join("db.sqlite3")).unwrap();
        db.execute("UPDATE items SET content=?2 WHERE id=?1", [id, content])
            .unwrap();
        let StoreResponse::Entity(item) = f
            .sqlite
            .handle(StoreRequest::Claim {
                id: id.into(),
                owner: f.owner.clone(),
                reset_invalid_default: Some(Platform::Gtk),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            item.entity,
            Entity::included_workspace(id, Platform::Gtk, 1000).unwrap()
        );
    }
}

#[test]
fn manager_recovers_startup_and_switch_without_resetting_other_workspaces() {
    pollster::block_on(async {
        let directory =
            std::env::temp_dir().join(format!("capy-manager-default-recovery-{}", new_id()));
        let worker = StoreWorker::shared(&directory).unwrap();
        let manager = WorkspaceManager::new(worker.clone(), Platform::Gtk);
        let first = manager.initialize(1000).await.unwrap();
        manager.activate(first);
        manager.close().await.unwrap();
        let db = rusqlite::Connection::open(directory.join("workspaces.sqlite3")).unwrap();
        let corrupt = |id| {
            db.execute(
                "UPDATE items SET working=json_set(working,'$.colors.shape','wheel') WHERE id=?1",
                [id],
            )
            .unwrap()
        };
        corrupt(DEFAULT_WORKSPACES[1].0);
        corrupt(DEFAULT_WORKSPACES[0].0);
        let restarted = WorkspaceManager::new(worker.clone(), Platform::Gtk);
        let incoming = restarted.initialize(2000).await.unwrap();
        assert_eq!(incoming.entity.id, DEFAULT_WORKSPACES[1].0);
        assert_eq!(
            incoming.entity.capture().unwrap().history.layout(),
            Entity::included_workspace(DEFAULT_WORKSPACES[1].0, Platform::Gtk, 2000)
                .unwrap()
                .capture()
                .unwrap()
                .history
                .layout()
        );
        restarted.activate(incoming);
        assert_eq!(
            restarted
                .store
                .execute(StoreRequest::Load {
                    id: DEFAULT_WORKSPACES[0].0.into()
                })
                .await
                .unwrap_err()
                .kind,
            ErrorKind::InvalidData
        );
        let painter = restarted
            .prepare_switch(DEFAULT_WORKSPACES[0].0, 3000)
            .await
            .unwrap();
        assert_eq!(
            painter.entity.capture().unwrap().history.layout(),
            Entity::included_workspace(DEFAULT_WORKSPACES[0].0, Platform::Gtk, 3000)
                .unwrap()
                .capture()
                .unwrap()
                .history
                .layout()
        );
        let previous = restarted.activate(painter).unwrap();
        restarted.release(&previous).await;
        // Selecting a workspace in the manager first loads a preview. That
        // path repairs too, but must release its temporary ownership afterward.
        corrupt(DEFAULT_WORKSPACES[2].0);
        let preview = restarted.load(DEFAULT_WORKSPACES[2].0).await.unwrap();
        assert_eq!(
            preview.entity.capture().unwrap().history.layout(),
            &layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk)
        );
        assert!(preview.claim.is_none());
        let StoreResponse::Entity(stored) = restarted
            .store
            .execute(StoreRequest::Load {
                id: DEFAULT_WORKSPACES[2].0.into(),
            })
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(
            stored.claim.is_none(),
            "preview must not hold an inactive workspace"
        );
        assert_eq!(
            restarted.active_id().as_deref(),
            Some(DEFAULT_WORKSPACES[0].0)
        );
        restarted.close().await.unwrap();
        let reopened = WorkspaceManager::new(worker, Platform::Gtk);
        let final_item = reopened.initialize(4000).await.unwrap();
        assert_eq!(final_item.entity.id, DEFAULT_WORKSPACES[0].0);
        reopened.activate(final_item);
        reopened.close().await.unwrap();
        drop((manager, restarted, reopened, db));
        std::fs::remove_dir_all(directory).unwrap();
    });
}
