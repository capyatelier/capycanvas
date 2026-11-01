use super::*;
use crate::test_support::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn claim_of<'a>(items: &'a [ItemSummary], id: &str) -> Option<&'a Claim> {
    items.iter().find(|i| i.id == id).unwrap().claim.as_ref()
}

fn wait(reply: StoreReply) -> Result<StoreResponse> {
    pollster::block_on(reply.into_future())
}
struct Fixture {
    directory: std::path::PathBuf,
    store: SqliteStore,
    clock: Arc<TestClock>,
    owner: Owner,
}
impl Fixture {
    fn new() -> Self {
        Self::in_directory(temp_dir("workspace-test"))
    }
    fn in_directory(directory: std::path::PathBuf) -> Self {
        let clock = Arc::new(TestClock(AtomicU64::new(1_000_000)));
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
            .commit(CommitBatch::prepare(self.owner.clone(), vec![create(entity)]).unwrap())
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

#[test]
fn unreadable_rows_deliveries_and_preferences_survive_maintenance() {
    for content in [r#"{"type":"future"}"#, "not json"] {
        let mut f = Fixture::new();
        let mut browser = BrowserDatabase::default();
        let good = workspace("Readable");
        let bad = workspace("Unreadable");
        let batch = CommitBatch::prepare(f.owner.clone(), vec![create(good.clone()),create(bad.clone())]).unwrap();
        f.store.commit(batch.clone()).unwrap();
        browser.execute(StoreRequest::Commit {batch:batch.clone()},1_000_000).unwrap();
        f.store.handle(StoreRequest::Acknowledge {operation_id:batch.operation_id}).unwrap();
        let pending = CommitBatch::prepare(f.owner.clone(), vec![create(workspace("Interrupted"))]).unwrap();
        let mut payload = serde_json::to_value(&pending).unwrap();
        if content == "not json" { payload["writes"][0]["content_json"] = "not json".into(); }
        else { payload["writes"][0]["metadata"]["kind"] = "future_kind".into(); }
        let extra = r#"{"kept_for_future":true}"#;
        let extra_id = content_id(extra.as_bytes());
        payload["future_reference"] = serde_json::json!({"$workspace_component":extra_id});
        for (id,json) in pending.components.iter().chain(std::iter::once((&extra_id,&extra.to_string()))) {
            f.store.connection.execute("INSERT OR IGNORE INTO components(id,json) VALUES(?1,?2)",params![id,json]).unwrap();
        }
        let orphan = r#"{"orphan":true}"#;
        let orphan_id = content_id(orphan.as_bytes());
        f.store.connection.execute("INSERT INTO components(id,json) VALUES(?1,?2)",params![orphan_id,orphan]).unwrap();
        f.store.connection.execute("UPDATE items SET content=?2 WHERE id=?1",params![bad.id,content]).unwrap();
        f.store.connection.execute("INSERT INTO pending(id,hash,payload) VALUES(?1,'future',?2)",params![pending.operation_id,payload.to_string()]).unwrap();
        for table in ["workspace_switcher","workspace_order"] {
            f.store.connection.execute(&format!("INSERT INTO {table}(id,workspace_ids) VALUES(1,'true')"),[]).unwrap();
        }
        let mut encoded: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
        encoded["items"][&bad.id]["entity"]["content"] = serde_json::json!({"type":"future"});
        encoded["pending"][&pending.operation_id] = payload.clone();
        encoded["switcher"] = true.into();
        encoded["workspace_order"] = true.into();
        let mut browser = BrowserDatabase::decode(&encoded.to_string()).unwrap();
        for request in [StoreRequest::Maintenance {owner:None,clear_older:false},StoreRequest::Pending,StoreRequest::Switcher,StoreRequest::WorkspaceOrder] {
            assert_eq!(serde_json::to_value(f.store.handle(request.clone()).unwrap()).unwrap(),serde_json::to_value(browser.execute(request,1_000_000).unwrap()).unwrap());
        }
        assert!(matches!(f.store.handle(StoreRequest::Load {id:good.id.clone()}).unwrap(),StoreResponse::Entity(_)));
        assert!(matches!(browser.execute(StoreRequest::Load {id:good.id},1_000_000).unwrap(),StoreResponse::Entity(_)));
        assert!(f.store.list().unwrap().iter().any(|item|item.id==bad.id));
        assert!(f.store.load(&bad.id).is_err());
        assert_eq!(f.store.connection.query_row("SELECT content FROM items WHERE id=?1",[&bad.id],|row|row.get::<_,String>(0)).unwrap(),content);
        assert_eq!(f.store.connection.query_row("SELECT payload FROM pending WHERE id=?1",[&pending.operation_id],|row|row.get::<_,String>(0)).unwrap(),payload.to_string());
        for id in pending.components.keys().chain(std::iter::once(&extra_id)) {
            assert!(component(&f.store.connection,id).is_ok());
        }
        assert_eq!(component(&f.store.connection,&orphan_id).is_ok(),content=="not json");
        let kept: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
        assert_eq!(kept["pending"],encoded["pending"]);
        assert_eq!(kept["items"][&bad.id],encoded["items"][&bad.id]);
        assert_eq!(kept["switcher"],true);
        assert_eq!(kept["workspace_order"],true);
        assert!(matches!(f.store.handle(StoreRequest::Pending).unwrap(),StoreResponse::Pending(batches) if batches.is_empty()));
        assert!(matches!(browser.execute(StoreRequest::Pending,1_000_000).unwrap(),StoreResponse::Pending(batches) if batches.is_empty()));
        payload["owner"]["future"] = true.into();
        f.store.connection.execute("UPDATE pending SET payload=?2 WHERE id=?1",params![pending.operation_id,payload.to_string()]).unwrap();
        encoded["pending"][&pending.operation_id] = payload;
        browser = BrowserDatabase::decode(&encoded.to_string()).unwrap();
        let mut abandon = CommitBatch::prepare(f.owner.clone(),vec![]).unwrap();
        abandon.abandon_operations.push(pending.operation_id.clone());
        assert!(f.store.commit(abandon.clone()).is_err());
        assert!(browser.execute(StoreRequest::Commit {batch:abandon},1_000_000).is_err());
        let kept: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
        assert_eq!(kept["pending"],encoded["pending"]);
    }
}

#[test]
fn editing_reads_restore_fields_with_the_requested_language_without_writing() {
    let mut f = Fixture::new();
    let mut expected = layer_ui::EditingState::new_localized(&layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese));
    expected.colors.set_rgba([0.2,0.4,0.7,1.]).unwrap();
    let mut value = serde_json::to_value(&expected).unwrap();
    value["color_library"]["palettes"] = "unreadable".into();
    let original = value.to_string();
    f.store.connection.execute("INSERT INTO editing(id,json) VALUES(1,?1)",[&original]).unwrap();
    let mut value = serde_json::to_value(BrowserDatabase::default()).unwrap();
    value["editing"] = original.clone().into();
    let mut browser = BrowserDatabase::decode(&value.to_string()).unwrap();
    for response in [f.store.handle(StoreRequest::Editing {language:layer_ui::UiLanguage::Japanese}).unwrap(),browser.execute(StoreRequest::Editing {language:layer_ui::UiLanguage::Japanese},1_000_000).unwrap()] {
        let StoreResponse::Editing {state:Some(restored),dropped:true} = response else {panic!()};
        assert_eq!(*restored,expected);
    }
    assert_eq!(f.store.connection.query_row("SELECT json FROM editing",[],|row|row.get::<_,String>(0)).unwrap(),original);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&browser.encoded().unwrap()).unwrap()["editing"],original);
}

#[test]
fn editing_merge_keeps_unreadable_fields_and_rejects_invalid_submissions() {
    let palettes = serde_json::json!([{"id":17,"name":"Future palette","swatches":"unreadable"}]);
    let mut partial = serde_json::to_value(layer_ui::EditingState::default()).unwrap();
    partial["preset"] = "unreadable".into();
    partial["color_library"]["palettes"] = palettes.clone();
    let partial = partial.to_string();
    for current in ["not json", partial.as_str()] {
        let mut f = Fixture::new();
        f.store.connection.execute("INSERT INTO editing(id,json) VALUES(1,?1)",[current]).unwrap();
        let mut snapshot = serde_json::to_value(BrowserDatabase::default()).unwrap();
        snapshot["editing"] = current.into();
        let mut browser = BrowserDatabase::decode(&snapshot.to_string()).unwrap();
        let mut incoming = layer_ui::EditingState::default();
        incoming.tools.set_override(incoming.preset,"size",91.,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let json = serde_json::to_string(&incoming).unwrap();
        let mut batch = CommitBatch::prepare(f.owner.clone(),vec![]).unwrap();
        batch.editing_json = Some(json.clone());
        batch.editing_fields = Some(vec!["/tools/overrides/2".into()]);
        f.store.commit(batch.clone()).unwrap();
        browser.execute(StoreRequest::Commit {batch},1_000_000).unwrap();
        let native: String = f.store.connection.query_row("SELECT json FROM editing",[],|row|row.get(0)).unwrap();
        let web = serde_json::from_str::<serde_json::Value>(&browser.encoded().unwrap()).unwrap()["editing"].as_str().unwrap().to_owned();
        if current == partial {
            for saved in [&native, &web] {
                let value: serde_json::Value = serde_json::from_str(saved).unwrap();
                assert_eq!(value["color_library"]["palettes"].to_string(), palettes.to_string());
                assert_eq!(value["preset"], "unreadable");
            }
        }
        for result in [f.store.handle(StoreRequest::Editing {language:layer_ui::UiLanguage::English}).unwrap(),browser.execute(StoreRequest::Editing {language:layer_ui::UiLanguage::English},1_000_000).unwrap()] {
            let StoreResponse::Editing {state:Some(stored),..} = result else {panic!()};
            assert_eq!(*stored,incoming);
        }
        let mut invalid = CommitBatch::prepare(f.owner.clone(),vec![]).unwrap();
        invalid.editing_json = Some("{}".into());
        invalid.editing_fields = Some(vec!["/tools".into()]);
        assert!(f.store.commit(invalid.clone()).is_err());
        assert!(browser.execute(StoreRequest::Commit {batch:invalid},1_000_000).is_err());
        for result in [f.store.handle(StoreRequest::Editing {language:layer_ui::UiLanguage::English}).unwrap(),browser.execute(StoreRequest::Editing {language:layer_ui::UiLanguage::English},1_000_000).unwrap()] {
            let StoreResponse::Editing {state:Some(stored),..} = result else {panic!()};
            assert_eq!(*stored,incoming);
        }
    }
}

#[test]
fn editing_paths_keep_arrays_and_null_as_leaves_and_remove_map_entries() {
    let mut previous = serde_json::to_value(layer_ui::EditingState::default()).unwrap();
    previous["future"] = serde_json::json!({"a/b~c":{"removed":1,"null":1},"array":[1]});
    let mut incoming = previous.clone();
    incoming["future"] = serde_json::json!({"a/b~c":{"null":null},"array":[2]});
    let paths = crate::store_rules::changed_editing_paths(&previous,&incoming);
    assert_eq!(paths, ["/future/a~1b~0c/null", "/future/a~1b~0c/removed", "/future/array"]);
    previous["future"]["another_window"] = true.into();
    let mut batch = CommitBatch::prepare(Owner::fresh(),vec![]).unwrap();
    batch.editing_json = Some(incoming.to_string());
    batch.editing_fields = Some(paths);
    let merged = crate::store_rules::merge_editing(Some(&previous.to_string()),&batch).unwrap().unwrap();
    let merged: serde_json::Value = serde_json::from_str(&merged).unwrap();
    incoming["future"]["another_window"] = true.into();
    assert_eq!(merged,incoming);
    batch.editing_fields = Some(vec!["/future/bad~2escape".into()]);
    assert!(crate::store_rules::merge_editing(Some(&previous.to_string()),&batch).is_err());
}

#[test]
fn sqlite_rejects_oversized_values_before_row_materialization() {
    let f = Fixture::new();
    let result = f.store.connection.query_row("SELECT zeroblob(?1)", [MAX_ROW_BYTES as i64 + 1], |row| {
        row.get_ref(0).map(|_| ())
    });
    assert!(result.is_err());
    f.store.connection.execute("INSERT INTO pending(id,hash,payload) VALUES('row-limit','hash',zeroblob(?1))", [MAX_PACKAGE_BYTES as i64]).unwrap();
    f.store.connection.execute("DELETE FROM pending WHERE id='row-limit'", []).unwrap();
    use rusqlite::config::DbConfig;
    assert!(f.store.connection.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap());
    assert!(!f.store.connection.db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA).unwrap());
}

#[test]
fn storage_enforces_included_workspace_name_and_delete_protection() {
    let mut f = Fixture::new();
    let mut browser = BrowserDatabase::default();
    let mut entity = workspace("Painter");
    entity.metadata.builtin = true;
    let id = entity.id.clone();
    let create = CommitBatch::prepare(f.owner.clone(), vec![create(entity)]).unwrap();
    browser.prepare_delivery(&create).unwrap();
    browser
        .execute(
            StoreRequest::Commit {
                batch: create.clone(),
            },
            1_000_000,
        )
        .unwrap();
    f.store.commit(create).unwrap();
    let stored = f.store.load(&id).unwrap();
    for deleting in [false, true] {
        let mut metadata = stored.entity.metadata.clone();
        metadata.name = "Renamed".into();
        let mutation = if deleting {
            delete(&stored)
        } else {
            change(&stored, Some(metadata), None, None)
        };
        let batch = CommitBatch::prepare(f.owner.clone(), vec![mutation]).unwrap();
        browser.prepare_delivery(&batch).unwrap();
        assert_eq!(
            browser
                .execute(
                    StoreRequest::Commit {
                        batch: batch.clone()
                    },
                    1_000_000
                )
                .unwrap_err()
                .kind,
            ErrorKind::InvalidData
        );
        assert_eq!(
            f.store.commit(batch).unwrap_err().kind,
            ErrorKind::InvalidData
        );
        assert_eq!(f.store.load(&id).unwrap().entity, stored.entity);
        let StoreResponse::Entity(saved) = browser
            .execute(StoreRequest::Load { id: id.clone() }, 1_000_000)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(saved.entity, stored.entity);
    }
}

#[test]
fn original_database_export_includes_wal_and_preserves_unsupported_payloads() {
    let f = Fixture::new();
    let destination = f.directory.join("original-backup.sqlite3");
    original_database_export(f, destination);
}

#[cfg(windows)]
#[test]
fn deep_database_paths_preserve_wal_and_original_export() {
    use std::os::windows::ffi::OsStrExt;
    let mut directory = temp_dir("workspace-deep-path");
    while directory.as_os_str().encode_wide().count() < 280 {
        directory.push("deep-workspace-path");
    }
    let f = Fixture::in_directory(directory);
    assert_eq!(
        f.store.connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0)).unwrap(),
        "wal"
    );
    let exports = f.directory.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    original_database_export(f, exports.join("original-backup.sqlite3"));
}

fn original_database_export(mut f: Fixture, destination: std::path::PathBuf) {
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
    assert!(f.directory.join("workspaces.sqlite3-wal").is_file());
    let worker = StoreWorker::shared(&f.directory).unwrap();
    assert_eq!(
        wait(worker.request(StoreRequest::List)).unwrap_err().kind,
        ErrorKind::UnsupportedSchema
    );
    #[cfg(windows)]
    for name in [
        "WORKSPACES.SQLITE3",
        "WORKSPACES.SQLITE3-WAL",
        "WORKSPACES.SQLITE3-SHM",
        "WORKSPACES.SQLITE3-LOCK",
    ] {
        assert!(wait(worker.backup_database(&f.directory.join(name))).is_err());
    }
    // Native save pickers can authorize replacing an earlier backup.
    std::fs::write(&destination, b"previous successful backup").unwrap();
    wait(worker.backup_database(&destination)).unwrap();
    let backup = Connection::open(std::fs::canonicalize(&destination).unwrap()).unwrap();
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
    for suffix in ["", "-wal", "-shm", "-lock"] {
        assert!(
            wait(worker.backup_database(&f.directory.join(format!("workspaces.sqlite3{suffix}"))))
                .is_err()
        );
    }
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
    let directory = temp_dir("workspace-invalid");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("workspaces.sqlite3"),
        b"not a sqlite database",
    )
    .unwrap();
    let destination = directory.join("previous-backup.sqlite3");
    std::fs::write(&destination, b"previous successful backup").unwrap();
    let worker = StoreWorker::shared(&directory).unwrap();
    assert!(wait(worker.backup_database(&destination)).is_err());
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
    metadata.description = "x".repeat(16_384);
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
    let original = baseline.as_ref().clone();
    let mut first = original.clone();
    first.bands[0].extent += 20.;
    history.append(&first, layer_ui::LayoutChange::Automatic);
    let abandoned = history.current.clone();
    history.undo();
    let mut second = original.clone();
    second.bands[0].extent += 40.;
    history.append(&second, layer_ui::LayoutChange::Automatic);
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
    f.store
        .handle(StoreRequest::Maintenance {
            owner: Some(Owner::fresh()),
            clear_older: true,
        })
        .unwrap();
    assert_eq!(f.store.load(&current.entity.id).unwrap(), snapshot);
    f.store
        .handle(StoreRequest::Maintenance {
            owner: Some(f.owner.clone()),
            clear_older: true,
        })
        .unwrap();
    let cleaned = f.store.load(&current.entity.id).unwrap();
    let ItemContent::Workspace {
        history, baseline, ..
    } = &cleaned.entity.content
    else {
        panic!()
    };
    assert_eq!(baseline.as_ref(), &original);
    assert_eq!(history.layout(), &second);
    assert_eq!(
        (&history.current, &history.undo, &history.redo),
        (&navigation.0, &navigation.1, &navigation.2)
    );
    assert!(!history.revisions.contains_key(&abandoned));
    assert_eq!(cleaned.entity.working, current.entity.working);
    assert!(cleaned.generations.layout > snapshot.generations.layout);
    let copy = f.create("Shares original");
    f.store
        .commit(CommitBatch::prepare(f.owner.clone(), vec![delete(&cleaned)]).unwrap())
        .unwrap();
    f.store
        .handle(StoreRequest::Maintenance {
            owner: None,
            clear_older: false,
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
fn delete(stored: &StoredEntity) -> Mutation {
    Mutation::Delete {
        id: stored.entity.id.clone(),
        generations: stored.generations,
        fence: stored.claim.as_ref().unwrap().fence,
    }
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
    assert_eq!(
        components,
        layer_ui::DockLayout::default().panels.len() as i64 + 1
    );
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
    history.append(&layout, layer_ui::LayoutChange::Automatic);
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
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(entity)]).unwrap();
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
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(entity)]).unwrap();
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
fn native_lock_protects_suspended_writers_and_fences_released_owners() {
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
    assert_eq!(
        other.claim(&id, second.clone()).unwrap_err().kind,
        ErrorKind::OwnedElsewhere
    );
    let live = other
        .list()
        .unwrap()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap();
    assert_eq!(live.claim.as_ref().unwrap().owner, f.owner);
    assert!(live.claim.unwrap().expires_at_ms > f.clock.now_ms());
    // Drop the owning connection without releasing the database claim. Other
    // native clients are still running; reclamation must not require reopening.
    f.store = f.connection();
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
fn deletion_replacement_is_atomic_and_delayed_writes_cannot_resurrect() {
    let mut f = Fixture::new();
    let old = f.create("Painting");
    let replacement = f.create("Inking");
    let mut batch = CommitBatch::prepare(f.owner.clone(), vec![delete(&old)]).unwrap();
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
    assert_eq!(
        f.store.load(&old.entity.id).unwrap_err().kind,
        ErrorKind::NotFound
    );
    let mut recreated = workspace("Painting");
    recreated.id = old.entity.id.clone();
    let recreate = CommitBatch::prepare(f.owner.clone(), vec![create(recreated)]).unwrap();
    assert_eq!(
        f.store.commit(recreate).unwrap_err().kind,
        ErrorKind::Conflict
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
        history.append(&layout, layer_ui::LayoutChange::Automatic);
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
    f.store.handle(StoreRequest::Maintenance {
        owner: None,
        clear_older: false
    }).unwrap();
    assert_eq!(f.store.load(&good.entity.id).unwrap(), good);
    assert!(f.store.load(&bad.entity.id).is_err());
    f.store
        .connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let mut newer = f.connection();
    for request in [StoreRequest::List, StoreRequest::Reset] {
        assert_eq!(
            newer.handle(request).unwrap_err().kind,
            ErrorKind::UnsupportedSchema
        );
    }
    assert_eq!(newer.list().unwrap().len(), 2);
    let version: u32 = f
        .store
        .connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}

#[test]
fn stores_of_other_versions_serve_only_reset() {
    for version in [SCHEMA_VERSION - 1, 0] {
        let mut f = Fixture::new();
        f.create("Older");
        f.store
            .connection
            .execute_batch("CREATE TABLE retired (id TEXT PRIMARY KEY)")
            .unwrap();
        f.store
            .connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        let mut store = f.connection();
        assert_eq!(
            store.handle(StoreRequest::List).unwrap_err().kind,
            ErrorKind::UnsupportedSchema,
            "version {version}"
        );
        assert_eq!(store.list().unwrap().len(), 1);
        store.handle(StoreRequest::Reset).unwrap();
        let StoreResponse::List(items) = store.handle(StoreRequest::List).unwrap() else {
            panic!()
        };
        assert!(items.is_empty());
        let kept = Connection::open(std::fs::canonicalize(f.directory.join("workspaces.unreadable.sqlite3")).unwrap()).unwrap();
        let older: u32 = kept.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(older, 1, "version {version}");
        let retired: u32 = store
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='retired'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(retired, 0);
        let current: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(current, SCHEMA_VERSION);
        let saved = f.create("Current");
        assert_eq!(store.load(&saved.entity.id).unwrap(), saved);
    }
}

#[test]
fn the_last_native_client_drains_accepted_requests_and_joins_sqlite() {
    let directory = temp_dir("workspace-joined");
    let first = StoreWorker::shared(&directory).unwrap();
    let last = first.clone();
    wait(first.request(StoreRequest::List)).unwrap();
    drop(first);
    // Closing one window leaves another window's shared worker operational.
    wait(last.request(StoreRequest::List)).unwrap();
    let entity = workspace("Final accepted workspace");
    let id = entity.id.clone();
    let batch = CommitBatch::prepare(
        Owner::fresh(),
        vec![Mutation::Create {
            entity,
            claim: false,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    let reply = last.request(StoreRequest::Commit { batch });
    drop(last);
    // No waiting here: the last client's Drop must have joined the worker.
    let mut reply = reply.into_future();
    assert!(matches!(
        reply.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())),
        std::task::Poll::Ready(Ok(StoreResponse::Committed(_)))
    ));
    let mut reopened = SqliteStore::open(&directory.join("workspaces.sqlite3")).unwrap();
    assert_eq!(
        reopened.load(&id).unwrap().entity.metadata.name,
        "Final accepted workspace"
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
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
                let result = store.claim(&id, Owner::fresh());
                // Keep the winning lock alive until both attempts complete.
                barrier.wait();
                result
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
fn killed_native_process_restores_without_waiting_for_its_lease() {
    const CHILD_DIRECTORY: &str = "CAPY_WORKSPACE_RESTART_TEST_DIRECTORY";
    let clock = || Arc::new(TestClock(AtomicU64::new(1_000_000)));
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let directory = std::path::PathBuf::from(directory);
        let mut store =
            SqliteStore::with_clock(&directory.join("workspaces.sqlite3"), clock()).unwrap();
        let entity = workspace("Saved before crash");
        let id = entity.id.clone();
        store
            .commit(CommitBatch::prepare(Owner::fresh(), vec![create(entity)]).unwrap())
            .unwrap();
        let saved = store.load(&id).unwrap();
        let mut working = saved.entity.working.clone().unwrap();
        working.zen_mode = true;
        let pending = CommitBatch::prepare(
            saved.claim.as_ref().unwrap().owner.clone(),
            vec![change(&saved, None, None, Some(working))],
        )
        .unwrap();
        // The delivery is durable, but publication is interrupted after its
        // updates and before the receipt. Neither updates nor locks may leak.
        store.connection.execute_batch("CREATE TRIGGER interrupt_save BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT, 'interrupted save'); END;").unwrap();
        assert!(store.commit(pending.clone()).is_err());
        store
            .connection
            .execute_batch("DROP TRIGGER interrupt_save")
            .unwrap();
        std::fs::write(
            directory.join("ready.json"),
            serde_json::to_vec(&(saved, pending)).unwrap(),
        )
        .unwrap();
        // Keep the real process and its storage handle alive until the parent
        // kills it. Killing must bypass Rust teardown and lease release.
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        panic!("The parent must kill the child, not close its input");
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = temp_dir("workspace-killed");
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sqlite::tests::killed_native_process_restores_without_waiting_for_its_lease",
                "--nocapture",
            ])
            .env(CHILD_DIRECTORY, &directory)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let started = std::time::Instant::now();
    let (saved, pending): (StoredEntity, CommitBatch) = loop {
        if let Ok(bytes) = std::fs::read(directory.join("ready.json"))
            && let Ok(saved) = serde_json::from_slice(&bytes)
        {
            break saved;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Child exited before saving"
        );
        assert!(
            started.elapsed().as_secs() < 15,
            "Child did not finish saving"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let path = directory.join("workspaces.sqlite3");
    let mut concurrent = SqliteStore::with_clock(&path, clock()).unwrap();
    assert_eq!(concurrent.load(&saved.entity.id).unwrap(), saved);
    assert_eq!(
        concurrent
            .claim(&saved.entity.id, Owner::fresh())
            .unwrap_err()
            .kind,
        ErrorKind::OwnedElsewhere
    );
    let survivor = workspace("Still open in the other process");
    let survivor_id = survivor.id.clone();
    let survivor_owner = Owner::fresh();
    concurrent
        .commit(CommitBatch::prepare(survivor_owner.clone(), vec![create(survivor)]).unwrap())
        .unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let catalog = concurrent.list().unwrap();
    assert!(claim_of(&catalog, &saved.entity.id).is_none());
    assert_eq!(
        claim_of(&catalog, &survivor_id).unwrap().owner,
        survivor_owner
    );
    assert!(matches!(concurrent.handle(StoreRequest::Pending).unwrap(),
        StoreResponse::Pending(deliveries) if deliveries.iter().any(|p| p.encoded().unwrap() == pending.encoded().unwrap())));
    let mut restarted = SqliteStore::with_clock(&path, clock()).unwrap();
    assert_eq!(
        restarted.load(&saved.entity.id).unwrap().entity,
        saved.entity
    );
    let successor = restarted.claim(&saved.entity.id, Owner::fresh()).unwrap();
    let old_claim = saved.claim.unwrap();
    assert!(successor.claim.as_ref().unwrap().fence > old_claim.fence);
    assert_eq!(
        restarted.commit(pending).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert!(
        restarted
            .renew(&saved.entity.id, &old_claim.owner, old_claim.fence)
            .is_err()
    );
    restarted
        .release(&saved.entity.id, &old_claim.owner, old_claim.fence)
        .unwrap();
    assert_eq!(restarted.load(&saved.entity.id).unwrap(), successor);
    drop(restarted);
    drop(concurrent);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn dropped_window_retires_all_its_claims_after_queued_writes_not_other_windows() {
    let mut f = Fixture::new();
    let surviving = f.create("Surviving window");
    let closing = f.create("Closing window");
    f.store
        .release(&closing.entity.id, &f.owner, closing.claim.unwrap().fence)
        .unwrap();
    let worker = StoreWorker::shared(&f.directory).unwrap();
    let manager = WorkspaceManager::new(worker.clone(), layer_ui::Platform::Gtk);
    let incoming =
        pollster::block_on(manager.prepare_switch(&closing.entity.id, 1_000_000)).unwrap();
    manager.activate(incoming);
    // An accepted creation may finish after its window disappears, before its
    // result is adopted. Teardown must retire that claim as well as the active one.
    let pending = workspace("Unadopted queued creation");
    let pending_id = pending.id.clone();
    let reply = worker.request(StoreRequest::Commit {
        batch: CommitBatch::prepare(manager.owner.clone(), vec![create(pending)]).unwrap(),
    });
    drop(manager);
    let StoreResponse::List(items) = wait(worker.request(StoreRequest::List)).unwrap() else {
        panic!()
    };
    assert!(claim_of(&items, &closing.entity.id).is_none());
    assert!(claim_of(&items, &pending_id).is_none());
    assert_eq!(
        claim_of(&items, &surviving.entity.id).unwrap().owner,
        f.owner
    );
    assert!(matches!(wait(reply).unwrap(), StoreResponse::Committed(_)));
    assert!(f.store.claim(&closing.entity.id, Owner::fresh()).is_ok());
}

#[test]
fn failed_teardown_cleanup_does_not_pin_a_dead_windows_kernel_locks() {
    let mut f = Fixture::new();
    let saved = f.create("Failed cleanup");
    f.store.connection.execute_batch("CREATE TRIGGER fail_cleanup BEFORE UPDATE OF owner ON items BEGIN SELECT RAISE(ABORT, 'cleanup unavailable'); END;").unwrap();
    f.store.retire_owner(&f.owner);
    assert!(
        header(&f.store.connection, &saved.entity.id)
            .unwrap()
            .claim
            .is_some()
    );
    f.store
        .connection
        .execute_batch("DROP TRIGGER fail_cleanup")
        .unwrap();
    let mut other = f.connection();
    let successor = other.claim(&saved.entity.id, Owner::fresh()).unwrap();
    assert!(successor.claim.unwrap().fence > saved.claim.unwrap().fence);
}

#[test]
fn failed_claim_and_creation_drop_unpublished_locks() {
    let mut f = Fixture::new();
    let saved = f.create("Rollback ownership");
    let id = &saved.entity.id;
    f.store
        .release(id, &f.owner, saved.claim.unwrap().fence)
        .unwrap();
    f.store.connection.execute_batch("CREATE TRIGGER fail_claim BEFORE UPDATE OF owner ON items BEGIN SELECT RAISE(ABORT, 'claim failed'); END;").unwrap();
    assert!(f.store.claim(id, f.owner.clone()).is_err());
    f.store
        .connection
        .execute_batch("DROP TRIGGER fail_claim")
        .unwrap();
    let mut other = f.connection();
    let other_owner = Owner::fresh();
    let claimed = other.claim(id, other_owner.clone()).unwrap();
    assert_eq!(claimed.claim.unwrap().owner, other_owner);

    let entity = workspace("Rolled back creation");
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(entity.clone())]).unwrap();
    f.store.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT, 'receipt failed'); END;").unwrap();
    assert!(f.store.commit(batch).is_err());
    f.store
        .connection
        .execute_batch("DROP TRIGGER fail_receipt")
        .unwrap();
    other
        .commit(CommitBatch::prepare(other_owner, vec![create(entity.clone())]).unwrap())
        .unwrap();
    assert_eq!(other.load(&entity.id).unwrap().entity, entity);
}

#[test]
fn ownership_io_errors_fail_closed_and_exports_cannot_replace_lock_files() {
    let mut f = Fixture::new();
    let saved = f.create("Protected lock");
    let database = f.directory.join("workspaces.sqlite3");
    let lock_dir = ownership::lock_directory(&database);
    let lock_file = lock_dir.join(content_id(saved.entity.id.as_bytes()));
    assert!(validate_database_export_destination(&database, &lock_file).is_err());
    assert!(validate_database_export_destination(&database, &lock_dir.join("new-file")).is_err());
    assert!(
        validate_database_export_destination(&database, &f.directory.join("backup.sqlite3"))
            .is_ok()
    );
    let mut other = f.connection();
    // Point only this test client's probe at a non-directory. Do not delete or
    // replace the real guard file, which would break the ownership protocol.
    std::fs::write(f.directory.join("not-a-directory"), b"blocked").unwrap();
    other.ownership.directory = f.directory.join("not-a-directory");
    assert_eq!(
        other
            .claim(&saved.entity.id, Owner::fresh())
            .unwrap_err()
            .kind,
        ErrorKind::Unavailable
    );
    assert_eq!(f.store.load(&saved.entity.id).unwrap().claim, saved.claim);
}

#[cfg(unix)]
#[test]
fn database_symlinks_share_native_ownership() {
    let mut f = Fixture::new();
    let saved = f.create("Aliased database");
    let alias = f.directory.join("alias.sqlite3");
    std::os::unix::fs::symlink(f.directory.join("workspaces.sqlite3"), &alias).unwrap();
    let mut other = SqliteStore::with_clock(&alias, f.clock.clone()).unwrap();
    assert_eq!(
        other
            .claim(&saved.entity.id, Owner::fresh())
            .unwrap_err()
            .kind,
        ErrorKind::OwnedElsewhere
    );
    assert!(
        validate_database_export_destination(
            &alias,
            &ownership::lock_directory(&f.directory.join("workspaces.sqlite3")).join("new-file")
        )
        .is_err()
    );
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
                history.append(&layout, layer_ui::LayoutChange::Automatic);
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
    let root = temp_dir("workspace-unavailable");
    std::fs::write(&root, b"not a directory").unwrap();
    let worker = StoreWorker::shared(&root).unwrap();
    assert_eq!(
        wait(worker.request(StoreRequest::List)).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    std::fs::remove_file(&root).unwrap();
    wait(worker.request(StoreRequest::Reopen)).unwrap();
    assert!(
        matches!(wait(worker.request(StoreRequest::List)).unwrap(), StoreResponse::List(v) if v.is_empty())
    );
    drop(worker);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cold_storage_lists_preserve_known_refusal_identity() {
    let mut f = Fixture::new();
    let stored = f.create("Literal 🖌 {name}");
    let mut browser = BrowserDatabase::default();
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(stored.entity.clone())]).unwrap();
    browser.prepare_delivery(&batch).unwrap();
    browser.execute(StoreRequest::Commit { batch }, 1_000_000).unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
    snapshot["items"][&stored.entity.id]["entity"]["metadata"]["name"] = serde_json::json!("");
    let browser = BrowserDatabase::decode(&snapshot.to_string()).unwrap();
    let mut metadata = stored.entity.metadata.clone();
    metadata.name.clear();
    f.store.connection.execute("UPDATE items SET metadata=?1 WHERE id=?2", (serde_json::to_string(&metadata).unwrap(), &stored.entity.id)).unwrap();
    let StoreResponse::List(browser_items) = browser.clone().execute(StoreRequest::List, 1_000_000).unwrap() else { panic!() };
    let sqlite_items = f.store.list().unwrap();
    let english = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    for items in [browser_items, sqlite_items] {
        let reason = items.into_iter().find(|i| i.id == stored.entity.id).unwrap().error.unwrap();
        assert_eq!(reason.known, Some(WorkspaceRefusal::EnterANameOf1100CharactersWithoutLeadingOrTrailingSpaces));
        assert_eq!(reason.localized_message(&english), reason.known.unwrap().message(&english).as_ref());
        assert_ne!(reason.localized_message(&english), reason.message);
    }
}

#[test]
fn browser_cold_list_retains_toolbar_size_range_payload() {
    let f = Fixture::new();
    let entity = Entity::toolbar(ToolbarDefinition { name: "literal toolbar 🖌".into(), tiles: vec![layer_ui::ToolbarTile { slider_style: Default::default(), id: 1, control: layer_ui::ToolbarControl::Size { tenths: 120 } }], tile_style: Default::default(), hide_tab: false }, 1);
    let id = entity.id.clone();
    let mut browser = BrowserDatabase::default();
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(entity)]).unwrap();
    browser.prepare_delivery(&batch).unwrap();
    browser.execute(StoreRequest::Commit { batch }, 1_000_000).unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
    snapshot["items"][&id]["entity"]["content"]["definition"]["tiles"][0]["control"]["tenths"] = serde_json::json!(0);
    let mut browser = BrowserDatabase::decode(&snapshot.to_string()).unwrap();
    let StoreResponse::List(items) = browser.execute(StoreRequest::List, 1_000_000).unwrap() else { panic!() };
    let reason = items.into_iter().find(|i| i.id == id).unwrap().error.unwrap();
    let expected = layer_ui::NumericError::Range { label: layer_ui::MessageId::TOOL_SETTING_SIZE.into(), min: 0.5, max: 2048.0 };
    assert_eq!(reason.numeric, Some(expected.clone()));
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
    assert_eq!(reason.localized_message(&localization), expected.message(&localization));
}

#[test]
fn corrupt_browser_summary_marks_missing_titles_without_reclassifying_literal_names() {
    let f = Fixture::new();
    let entity = workspace("Unreadable workspace");
    let id = entity.id.clone();
    let mut browser = BrowserDatabase::default();
    let batch = CommitBatch::prepare(f.owner.clone(), vec![create(entity)]).unwrap();
    browser.prepare_delivery(&batch).unwrap();
    browser.execute(StoreRequest::Commit { batch }, 1_000_000).unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_str(&browser.encoded().unwrap()).unwrap();
    snapshot["items"][&id]["entity"]["metadata"]["builtin"] = serde_json::json!({});
    for missing_name in [false, true] {
        let mut source = snapshot.clone();
        if missing_name { source["items"][&id]["entity"]["metadata"].as_object_mut().unwrap().remove("name"); }
        let mut browser = BrowserDatabase::decode(&source.to_string()).unwrap();
        let StoreResponse::List(items) = browser.execute(StoreRequest::List, 1_000_000).unwrap() else { panic!() };
        let item = items.into_iter().find(|i| i.id == id).unwrap();
        assert_eq!(item.unavailable_name, missing_name);
        assert!(item.error.is_some());
        assert_eq!(item.metadata.name, if missing_name { "" } else { "Unreadable workspace" });
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        assert_eq!(item.display_name(&localization), if missing_name { "読み取れないワークスペース" } else { "Unreadable workspace" });
        let restored: ItemSummary = serde_json::from_slice(&serde_json::to_vec(&item).unwrap()).unwrap();
        assert_eq!(restored, item);
    }
}
