use super::*;
use layer_ui::{DockLayout, WorkspaceCapture};
use std::sync::atomic::{AtomicU64, Ordering};

struct FakeClock(AtomicU64);
impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
struct Fixture {
    directory: std::path::PathBuf,
    store: SqliteStore,
    clock: Arc<FakeClock>,
    owner: Owner,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("capy-workspace-test-{}", new_id()));
        let clock = Arc::new(FakeClock(AtomicU64::new(1_000_000)));
        let store =
            SqliteStore::with_clock(&directory.join("workspaces.sqlite3"), clock.clone()).unwrap();
        Self {
            directory,
            store,
            clock,
            owner: Owner::fresh(),
        }
    }
    fn create(&mut self, name: &str) -> StoredEntity {
        let entity = workspace(name);
        let id = entity.id.clone();
        self.store
            .commit(
                CommitBatch::prepare(
                    self.owner.clone(),
                    vec![Mutation::Create {
                        entity,
                        claim: true,
                        name_policy: NamePolicy::Exact,
                    }],
                )
                .unwrap(),
            )
            .unwrap();
        self.store.load(&id).unwrap()
    }
    fn connection(&self) -> SqliteStore {
        SqliteStore::with_clock(
            &self.directory.join("workspaces.sqlite3"),
            self.clock.clone(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn workspace(name: &str) -> Entity {
    let layout = DockLayout::default();
    Entity::workspace(
        name,
        WorkspaceCapture::from_template(&layout).unwrap(),
        layout,
        None,
        1_000_000,
    )
}

#[test]
fn original_database_export_includes_wal_and_preserves_unsupported_payloads() {
    let mut f = Fixture::new();
    let entity = f.create("Future Workspace");
    let opaque = "{\"type\":\"future_workspace\",\"private_extension\":123}";
    f.store
        .connection
        .execute(
            "UPDATE items SET content=?1 WHERE id=?2",
            params![opaque, entity.entity.id],
        )
        .unwrap();
    f.store
        .connection
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let worker = StoreWorker::shared(&f.directory).unwrap();
    assert_eq!(
        worker.request(StoreRequest::List).wait().unwrap_err().kind,
        ErrorKind::UnsupportedSchema
    );
    let destination = f.directory.join("original-backup.sqlite3");
    worker.backup_database(&destination).wait().unwrap();
    let backup = Connection::open(&destination).unwrap();
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        99
    );
    assert_eq!(
        backup
            .query_row(
                "SELECT content FROM items WHERE id=?1",
                [&entity.entity.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        opaque
    );
    assert!(
        worker
            .backup_database(&f.directory.join("workspaces.sqlite3"))
            .wait()
            .is_err()
    );
    assert_eq!(
        f.store
            .connection
            .query_row(
                "SELECT content FROM items WHERE id=?1",
                [&entity.entity.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        opaque
    );
}

#[test]
fn failed_database_export_keeps_the_existing_destination() {
    let directory = std::env::temp_dir().join(format!("capy-workspace-invalid-{}", new_id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("workspaces.sqlite3"),
        b"not a sqlite database",
    )
    .unwrap();
    let destination = directory.join("previous-backup.sqlite3");
    std::fs::write(&destination, b"previous successful backup").unwrap();
    let worker = StoreWorker::shared(&directory).unwrap();
    assert!(worker.backup_database(&destination).wait().is_err());
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"previous successful backup"
    );
    assert_eq!(
        std::fs::read(directory.join("workspaces.sqlite3")).unwrap(),
        b"not a sqlite database"
    );
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn acknowledged_receipts_are_bounded_while_pending_delivery_remains_resolvable() {
    let mut f = Fixture::new();
    let initial = f.create("Receipt Test");
    let pending: String = f
        .store
        .connection
        .query_row("SELECT id FROM pending LIMIT 1", [], |r| r.get(0))
        .unwrap();
    for index in 0..270 {
        let receipt = CommitReceipt {
            operation_id: format!("ack-{index}"),
            items: vec![(initial.entity.id.clone(), initial.generations)],
        };
        f.store
            .connection
            .execute(
                "INSERT INTO receipts(id,hash,receipt,owner,epoch) VALUES(?1,'test',?2,?3,?4)",
                params![
                    receipt.operation_id,
                    serde_json::to_string(&receipt).unwrap(),
                    f.owner.id,
                    f.owner.epoch
                ],
            )
            .unwrap();
        f.store
            .handle(StoreRequest::Acknowledge {
                operation_id: receipt.operation_id,
            })
            .unwrap();
    }
    let acknowledged: i64 = f
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM receipts WHERE acknowledged=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(acknowledged, 256);
    assert!(matches!(
        f.store
            .handle(StoreRequest::Receipt {
                operation_id: pending
            })
            .unwrap(),
        StoreResponse::Receipt(Some(_))
    ));
}

#[test]
fn sqlite_full_preserves_protected_records_and_retries_the_same_delivery() {
    let mut f = Fixture::new();
    let saved = f.create("Protected Workspace");
    let mut metadata = saved.entity.metadata.clone();
    for index in 0..32 {
        metadata.previous.push(MetadataVersion {
            id: new_id(),
            name: format!("Earlier Name {index}"),
            description: "x".repeat(16_384),
            timestamp_ms: 1_000_000,
        });
    }
    let batch = CommitBatch::prepare(
        f.owner.clone(),
        vec![change(&saved, Some(metadata.clone()), None, None)],
    )
    .unwrap();
    let pages: i64 = f
        .store
        .connection
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    f.store
        .connection
        .pragma_update(None, "max_page_count", pages + 2)
        .unwrap();
    let error = f
        .store
        .handle(StoreRequest::Commit {
            batch: batch.clone(),
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::StorageFull);
    let after = f.store.load(&saved.entity.id).unwrap();
    assert_eq!(after.entity, saved.entity);
    assert_eq!(after.generations, saved.generations);
    assert!(matches!(
        f.store
            .handle(StoreRequest::Receipt {
                operation_id: batch.operation_id.clone()
            })
            .unwrap(),
        StoreResponse::Receipt(None)
    ));
    f.store
        .connection
        .pragma_update(None, "max_page_count", pages + 4096)
        .unwrap();
    let StoreResponse::Committed(receipt) = f
        .store
        .handle(StoreRequest::Commit {
            batch: batch.clone(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(receipt.operation_id, batch.operation_id);
    let saved_again = f.store.load(&saved.entity.id).unwrap();
    assert_eq!(saved_again.entity.metadata, metadata);
    assert_eq!(saved_again.entity.content, saved.entity.content);
    assert_eq!(saved_again.entity.working, saved.entity.working);
    assert!(
        matches!(f.store.handle(StoreRequest::Commit {batch}).unwrap(),StoreResponse::Committed(repeated) if repeated==receipt)
    );
}

#[test]
fn schema_one_upgrade_keeps_entities_and_existing_delivery_hashes() {
    let mut f = Fixture::new();
    let entity = f.create("Legacy Database");
    let hashes: Vec<(String, String)> = f
        .store
        .connection
        .prepare("SELECT id,hash FROM receipts")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    f.store
        .connection
        .execute_batch("DROP TABLE cancelled_operations; PRAGMA user_version=1;")
        .unwrap();
    let mut upgraded = f.connection();
    assert_eq!(
        upgraded.load(&entity.entity.id).unwrap().entity,
        entity.entity
    );
    assert_eq!(
        upgraded
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    for (id, hash) in hashes {
        assert_eq!(
            upgraded
                .connection
                .query_row("SELECT hash FROM receipts WHERE id=?1", [id], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            hash
        );
    }
}

#[test]
fn maintenance_preserves_navigation_baselines_shared_content_and_fences() {
    let mut f = Fixture::new();
    let current = f.create("Retained");
    let mut content = current.entity.content.clone();
    let ItemContent::Workspace {
        history, baseline, ..
    } = &mut content
    else {
        panic!()
    };
    let original = baseline.clone();
    let mut first = original.clone();
    first.bands[0].extent += 20.;
    history.append(&first, "First");
    let abandoned = history.current.clone();
    history.undo();
    let mut second = original.clone();
    second.bands[0].extent += 40.;
    history.append(&second, "Second");
    let navigation = (
        history.current.clone(),
        history.undo.clone(),
        history.redo.clone(),
    );
    let mut metadata = current.entity.metadata.clone();
    metadata.rename("Renamed", "", 2_000_000).unwrap();
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&current, Some(metadata), Some(content), None)],
            )
            .unwrap(),
        )
        .unwrap();
    let snapshot = f.store.load(&current.entity.id).unwrap();
    let other = Owner::fresh();
    let StoreResponse::Storage(deferred) = f
        .store
        .handle(StoreRequest::Maintenance {
            owner: Some(other),
            clear_older: true,
            apply: true,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(deferred.versions_to_remove, 0);
    let request = StoreRequest::Maintenance {
        owner: Some(f.owner.clone()),
        clear_older: true,
        apply: false,
    };
    let StoreResponse::Storage(preview) = f.store.handle(request).unwrap() else {
        panic!()
    };
    assert_eq!(preview.versions_to_remove, 2);
    f.store
        .handle(StoreRequest::Maintenance {
            owner: Some(f.owner.clone()),
            clear_older: true,
            apply: true,
        })
        .unwrap();
    let cleaned = f.store.load(&current.entity.id).unwrap();
    let ItemContent::Workspace {
        history, baseline, ..
    } = &cleaned.entity.content
    else {
        panic!()
    };
    assert_eq!(baseline, &original);
    assert_eq!(history.layout(), &second);
    assert_eq!(
        (&history.current, &history.undo, &history.redo),
        (&navigation.0, &navigation.1, &navigation.2)
    );
    assert!(!history.revisions.contains_key(&abandoned));
    assert_eq!(cleaned.entity.working, current.entity.working);
    assert!(cleaned.entity.metadata.previous.is_empty());
    assert!(cleaned.generations.layout > snapshot.generations.layout);
    let copy = f.create("Shares original");
    let mut metadata = cleaned.entity.metadata.clone();
    metadata.deleted_at_ms = Some(1_000_000);
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&cleaned, Some(metadata), None, None)],
            )
            .unwrap(),
        )
        .unwrap();
    f.clock
        .0
        .store(1_000_000 + TRASH_LIFETIME_MS + 1, Ordering::Relaxed);
    f.store
        .handle(StoreRequest::Maintenance {
            owner: None,
            clear_older: false,
            apply: true,
        })
        .unwrap();
    assert!(f.store.load(&cleaned.entity.id).is_err());
    assert_eq!(
        f.store
            .load(&copy.entity.id)
            .unwrap()
            .entity
            .capture()
            .unwrap()
            .history
            .layout(),
        &original
    );
    assert!(
        f.store
            .commit(
                CommitBatch::prepare(
                    f.owner.clone(),
                    vec![change(
                        &snapshot,
                        None,
                        Some(snapshot.entity.content.clone()),
                        None
                    )]
                )
                .unwrap()
            )
            .is_err()
    );
}
fn change(
    stored: &StoredEntity,
    metadata: Option<Metadata>,
    content: Option<ItemContent>,
    working: Option<layer_ui::WorkspaceWorkingState>,
) -> Mutation {
    Mutation::Update {
        id: stored.entity.id.clone(),
        generations: stored.generations,
        fence: stored.claim.as_ref().unwrap().fence,
        metadata,
        content,
        working,
        name_policy: NamePolicy::Exact,
    }
}

#[test]
fn atomic_create_load_and_shared_immutable_components() {
    let mut f = Fixture::new();
    let first = f.create("Painting");
    let second = f.create("Inking");
    assert_ne!(first.entity.id, second.entity.id);
    assert_eq!(
        first.entity.capture().unwrap(),
        second.entity.capture().unwrap()
    );
    let components: i64 = f
        .store
        .connection
        .query_row("SELECT count(*) FROM components", [], |r| r.get(0))
        .unwrap();
    assert_eq!(components, DockLayout::default().panels.len() as i64 + 1);
    let mut reopened = f.connection();
    assert_eq!(reopened.load(&first.entity.id).unwrap(), first);
    assert_eq!(reopened.list().unwrap().len(), 2);
}
#[test]
fn layout_and_working_generations_are_independent_and_stale_writes_fail() {
    let mut f = Fixture::new();
    let old = f.create("Painting");
    let mut working = old.entity.working.clone().unwrap();
    working.zen_mode = true;
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&old, None, None, Some(working.clone()))],
            )
            .unwrap(),
        )
        .unwrap();
    let mut content = old.entity.content.clone();
    let ItemContent::Workspace { history, .. } = &mut content else {
        unreachable!()
    };
    let mut layout = history.layout().clone();
    layout.bands[0].extent += 50.;
    history.append(&layout, "Resize panel column");
    // The old working generation does not invalidate an independent layout write.
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&old, None, Some(content.clone()), None)],
            )
            .unwrap(),
        )
        .unwrap();
    let latest = f.store.load(&old.entity.id).unwrap();
    assert_eq!(latest.entity.content, content);
    assert_eq!(latest.entity.working, Some(working));
    assert_eq!(
        latest.generations,
        Generations {
            metadata: 1,
            layout: 2,
            working: 2
        }
    );
    let error = f
        .store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&old, None, Some(old.entity.content.clone()), None)],
            )
            .unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(f.store.load(&old.entity.id).unwrap(), latest);
}
#[test]
fn successful_sql_requests_do_not_publish_an_aborted_transaction() {
    let mut f = Fixture::new();
    let entity = workspace("Painting");
    let id = entity.id.clone();
    let batch = CommitBatch::prepare(
        f.owner.clone(),
        vec![Mutation::Create {
            entity,
            claim: true,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    f.store.connection.execute_batch("CREATE TEMP TRIGGER fail_receipt BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT, 'simulated storage failure'); END;").unwrap();
    assert!(f.store.commit(batch.clone()).is_err());
    assert!(f.store.load(&id).is_err());
    let count: i64 = f
        .store
        .connection
        .query_row("SELECT count(*) FROM components", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        matches!(f.store.handle(StoreRequest::Pending).unwrap(), StoreResponse::Pending(p) if p.len() == 1)
    );
    f.store
        .connection
        .execute_batch("DROP TRIGGER fail_receipt")
        .unwrap();
    f.store.commit(batch).unwrap();
    assert_eq!(f.store.load(&id).unwrap().entity.metadata.name, "Painting");
}
#[test]
fn lost_acknowledgements_and_repeated_delivery_keep_one_original_receipt() {
    let mut f = Fixture::new();
    let entity = workspace("Painting");
    let batch = CommitBatch::prepare(
        f.owner.clone(),
        vec![Mutation::Create {
            entity,
            claim: true,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    let original = f.store.commit(batch.clone()).unwrap();
    let mut reopened = f.connection();
    assert_eq!(reopened.commit(batch.clone()).unwrap(), original);
    let mut changed = batch.clone();
    changed.bindings.push(("other".into(), None));
    assert_eq!(
        reopened.commit(changed).unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(reopened.list().unwrap().len(), 1);
    reopened
        .handle(StoreRequest::Acknowledge {
            operation_id: batch.operation_id.clone(),
        })
        .unwrap();
    assert!(
        matches!(reopened.handle(StoreRequest::Pending).unwrap(), StoreResponse::Pending(p) if p.is_empty())
    );
    assert_eq!(reopened.commit(batch).unwrap(), original);
}
#[test]
fn owner_takeover_fences_suspended_writers_and_delayed_releases() {
    let mut f = Fixture::new();
    let old = f.create("Painting");
    let id = old.entity.id.clone();
    let second = Owner::fresh();
    let mut other = f.connection();
    assert_eq!(
        other.claim(&id, second.clone()).unwrap_err().kind,
        ErrorKind::OwnedElsewhere
    );
    f.clock.0.fetch_add(OWNER_LEASE_MS + 1, Ordering::Relaxed);
    let claimed = other.claim(&id, second.clone()).unwrap();
    assert!(claimed.claim.as_ref().unwrap().fence > old.claim.as_ref().unwrap().fence);
    assert!(
        f.store
            .renew(&id, &f.owner, old.claim.as_ref().unwrap().fence)
            .is_err()
    );
    f.store
        .release(&id, &f.owner, old.claim.as_ref().unwrap().fence)
        .unwrap();
    assert_eq!(other.load(&id).unwrap().claim.unwrap().owner, second);
    let mut working = old.entity.working.clone().unwrap();
    working.zen_mode = true;
    assert_eq!(
        f.store
            .commit(
                CommitBatch::prepare(
                    f.owner.clone(),
                    vec![change(&old, None, None, Some(working))]
                )
                .unwrap()
            )
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(other.load(&id).unwrap().entity.working, old.entity.working);
}
#[test]
fn deletion_replacement_and_restore_are_atomic_and_delayed_writes_cannot_resurrect() {
    let mut f = Fixture::new();
    let old = f.create("Painting");
    let replacement = f.create("Inking");
    let mut metadata = old.entity.metadata.clone();
    metadata.deleted_at_ms = Some(f.clock.now_ms());
    let mut batch = CommitBatch::prepare(
        f.owner.clone(),
        vec![change(&old, Some(metadata), None, None)],
    )
    .unwrap();
    batch
        .bindings
        .push(("last_workspace".into(), Some(replacement.entity.id.clone())));
    f.store.commit(batch).unwrap();
    assert!(
        matches!(f.store.handle(StoreRequest::Binding { key: "last_workspace".into() }).unwrap(), StoreResponse::Binding(Some(id)) if id == replacement.entity.id)
    );
    assert!(
        f.store
            .commit(
                CommitBatch::prepare(
                    f.owner.clone(),
                    vec![change(&old, None, None, old.entity.working.clone())]
                )
                .unwrap()
            )
            .is_err()
    );
    let _same_name = f.create("Painting");
    let deleted = f.store.claim(&old.entity.id, f.owner.clone()).unwrap();
    let mut metadata = deleted.entity.metadata.clone();
    metadata.deleted_at_ms = None;
    let mutation = Mutation::Update {
        id: old.entity.id.clone(),
        generations: deleted.generations,
        fence: deleted.claim.unwrap().fence,
        metadata: Some(metadata),
        content: None,
        working: None,
        name_policy: NamePolicy::Unique,
    };
    f.store
        .commit(CommitBatch::prepare(f.owner.clone(), vec![mutation]).unwrap())
        .unwrap();
    let restored = f.store.load(&old.entity.id).unwrap();
    assert_eq!(restored.entity.metadata.name, "Painting (2)");
    assert_eq!(
        restored.entity.capture().unwrap(),
        old.entity.capture().unwrap()
    );
}
#[test]
fn undo_revisits_content_but_never_reuses_a_write_generation() {
    let mut f = Fixture::new();
    let original = f.create("Painting");
    let mut content = original.entity.content.clone();
    if let ItemContent::Workspace { history, .. } = &mut content {
        let mut layout = history.layout().clone();
        layout.bands[0].extent += 80.;
        history.append(&layout, "Resize panels");
    }
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&original, None, Some(content.clone()), None)],
            )
            .unwrap(),
        )
        .unwrap();
    let latest = f.store.load(&original.entity.id).unwrap();
    if let ItemContent::Workspace { history, .. } = &mut content {
        assert!(history.undo());
    }
    f.store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&latest, None, Some(content), None)],
            )
            .unwrap(),
        )
        .unwrap();
    let undo = f.store.load(&original.entity.id).unwrap();
    assert_eq!(
        undo.entity.capture().unwrap().history.layout(),
        original.entity.capture().unwrap().history.layout()
    );
    assert_eq!(undo.generations.layout, 3);
    assert_eq!(
        f.store
            .commit(
                CommitBatch::prepare(
                    f.owner.clone(),
                    vec![change(
                        &original,
                        None,
                        Some(original.entity.content.clone()),
                        None
                    )]
                )
                .unwrap()
            )
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
}
#[test]
fn migration_source_mapping_and_import_publish_once() {
    let mut f = Fixture::new();
    let entity = workspace("My Workspace");
    let id = entity.id.clone();
    let mut batch = CommitBatch::prepare(
        f.owner.clone(),
        vec![Mutation::Create {
            entity,
            claim: true,
            name_policy: NamePolicy::Unique,
        }],
    )
    .unwrap();
    batch
        .legacy_imports
        .push(("apple:scene:one".into(), id.clone()));
    f.store.commit(batch.clone()).unwrap();
    f.store.commit(batch).unwrap();
    let another = workspace("My Workspace");
    let another_id = another.id.clone();
    let mut duplicate = CommitBatch::prepare(
        f.owner.clone(),
        vec![Mutation::Create {
            entity: another,
            claim: true,
            name_policy: NamePolicy::Unique,
        }],
    )
    .unwrap();
    duplicate
        .legacy_imports
        .push(("apple:scene:one".into(), another_id.clone()));
    assert!(f.store.commit(duplicate).is_err());
    assert!(f.store.load(&another_id).is_err());
    assert!(
        matches!(f.store.handle(StoreRequest::LegacyImport { source: "apple:scene:one".into() }).unwrap(), StoreResponse::Binding(Some(found)) if found == id)
    );
    assert_eq!(f.store.list().unwrap().len(), 1);
}
#[test]
fn newer_schemas_and_corrupt_items_are_preserved() {
    let mut f = Fixture::new();
    let good = f.create("Good");
    let bad = f.create("Broken");
    f.store
        .connection
        .execute(
            "UPDATE items SET metadata='unknown newer payload' WHERE id=?1",
            [&bad.entity.id],
        )
        .unwrap();
    let list = f.store.list().unwrap();
    assert_eq!(list.len(), 2);
    assert!(
        list.iter()
            .find(|i| i.id == bad.entity.id)
            .unwrap()
            .error
            .is_some()
    );
    assert!(f.store.load(&bad.entity.id).is_err());
    assert_eq!(f.store.load(&good.entity.id).unwrap(), good);
    assert!(
        matches!(f.store.handle(StoreRequest::Raw { id: bad.entity.id }).unwrap(), StoreResponse::Raw(v) if v.contains("unknown newer payload"))
    );
    f.store
        .connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        SqliteStore::open(&f.directory.join("workspaces.sqlite3")),
        Err(StoreError {
            kind: ErrorKind::UnsupportedSchema,
            ..
        })
    ));
    let version: u32 = f
        .store
        .connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}
#[test]
fn native_worker_shares_storage_across_window_clients() {
    let directory = std::env::temp_dir().join(format!("capy-workspace-worker-{}", new_id()));
    let first = StoreWorker::shared(&directory).unwrap();
    let second = StoreWorker::shared(&directory).unwrap();
    let entity = workspace("Painting");
    let id = entity.id.clone();
    let batch = CommitBatch::prepare(
        Owner::fresh(),
        vec![Mutation::Create {
            entity,
            claim: true,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    assert!(matches!(
        first
            .request(StoreRequest::Commit { batch })
            .wait()
            .unwrap(),
        StoreResponse::Committed(_)
    ));
    assert!(matches!(
        second.request(StoreRequest::Load { id }).wait().unwrap(),
        StoreResponse::Entity(_)
    ));
    drop(first);
    drop(second);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn concurrent_claims_have_exactly_one_editable_owner() {
    let mut f = Fixture::new();
    let item = f.create("Painting");
    f.store
        .release(&item.entity.id, &f.owner, item.claim.unwrap().fence)
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let joins: Vec<_> = (0..2)
        .map(|_| {
            let path = f.directory.join("workspaces.sqlite3");
            let clock = f.clock.clone();
            let id = item.entity.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = SqliteStore::with_clock(&path, clock).unwrap();
                barrier.wait();
                store.claim(&id, Owner::fresh())
            })
        })
        .collect();
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results.iter().any(|r| matches!(
        r,
        Err(StoreError {
            kind: ErrorKind::OwnedElsewhere,
            ..
        })
    )));
}

#[test]
fn simultaneous_readers_observe_complete_layout_and_working_publication() {
    let mut f = Fixture::new();
    let initial = f.create("Painting");
    let path = f.directory.join("workspaces.sqlite3");
    let clock = f.clock.clone();
    let owner = f.owner.clone();
    let id = initial.entity.id.clone();
    let writer = std::thread::spawn(move || {
        let mut store = SqliteStore::with_clock(&path, clock).unwrap();
        for index in 1..=30 {
            let old = store.load(&id).unwrap();
            let mut content = old.entity.content.clone();
            let mut working = old.entity.working.clone().unwrap();
            working.zen_mode = index % 2 == 1;
            if let ItemContent::Workspace { history, .. } = &mut content {
                let mut layout = history.layout().clone();
                layout.bands[0].extent = 100. + index as f32;
                history.append(&layout, "Resize column");
            }
            store
                .commit(
                    CommitBatch::prepare(
                        owner.clone(),
                        vec![change(&old, None, Some(content), Some(working))],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
    });
    for _ in 0..60 {
        let read = f.store.load(&initial.entity.id).unwrap();
        let capture = read.entity.capture().unwrap();
        assert_eq!(read.generations.layout, read.generations.working);
        if read.generations.layout > 1 {
            let index = read.generations.layout - 1;
            assert_eq!(
                capture.history.layout().bands[0].extent,
                100. + index as f32
            );
            assert_eq!(capture.working.zen_mode, index % 2 == 1);
        }
    }
    writer.join().unwrap();
}

#[test]
fn large_generations_round_trip_without_javascript_precision_loss() {
    let mut f = Fixture::new();
    let original = f.create("Painting");
    let generation = 9_007_199_254_740_993u64;
    f.store
        .connection
        .execute(
            "UPDATE items SET working_generation=?2 WHERE id=?1",
            params![original.entity.id, generation.to_string()],
        )
        .unwrap();
    let stored = f.store.load(&original.entity.id).unwrap();
    let text = serde_json::to_string(&stored).unwrap();
    assert!(text.contains("\"9007199254740993\""));
    let decoded: StoredEntity = serde_json::from_str(&text).unwrap();
    let receipt = f
        .store
        .commit(
            CommitBatch::prepare(
                f.owner.clone(),
                vec![change(&decoded, None, None, decoded.entity.working.clone())],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.items[0].1.working, generation + 1);
}

#[test]
fn failed_worker_open_can_be_retried_after_storage_becomes_available() {
    let root = std::env::temp_dir().join(format!("capy-workspace-unavailable-{}", new_id()));
    std::fs::write(&root, b"not a directory").unwrap();
    let worker = StoreWorker::shared(&root).unwrap();
    assert_eq!(
        worker.request(StoreRequest::List).wait().unwrap_err().kind,
        ErrorKind::Unavailable
    );
    std::fs::remove_file(&root).unwrap();
    worker.request(StoreRequest::Reopen).wait().unwrap();
    assert!(
        matches!(worker.request(StoreRequest::List).wait().unwrap(), StoreResponse::List(v) if v.is_empty())
    );
    drop(worker);
    let _ = std::fs::remove_dir_all(root);
}
