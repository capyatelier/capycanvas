use crate::*;
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
fn normalize(result: Result<StoreResponse, StoreError>) -> serde_json::Value {
    let result = result.map(|mut r| {
        match &mut r {
            StoreResponse::List(items) => items.sort_by(|a, b| a.id.cmp(&b.id)),
            StoreResponse::Pending(items) => {
                items.sort_by(|a, b| a.operation_id.cmp(&b.operation_id))
            }
            _ => {}
        }
        r
    });
    match result {
        Ok(r) => serde_json::json!({"ok":r}),
        Err(e) => serde_json::json!({"error":e.kind}),
    }
}
#[test]
fn browser_transactions_match_sqlite_contract() {
    let directory = std::env::temp_dir().join(format!("capy-browser-contract-{}", new_id()));
    let clock = Arc::new(TestClock(AtomicU64::new(1000)));
    let mut sqlite = SqliteStore::with_clock(&directory.join("db.sqlite3"), clock.clone()).unwrap();
    let mut browser = BrowserDatabase::default();
    let mut fixture = Vec::new();
    let mut execute = |request: StoreRequest, now: u64| {
        clock.0.store(now, Ordering::SeqCst);
        let expected = normalize(sqlite.handle(request.clone()));
        let result = if let StoreRequest::Commit { batch } = &request {
            browser
                .prepare_delivery(batch)
                .and_then(|()| browser.execute(request.clone(), now))
        } else {
            browser.execute(request.clone(), now)
        };
        assert_eq!(normalize(result), expected, "{request:?}");
        fixture.push(serde_json::json!({"request":request,"now":now,"expected":expected}));
    };
    let owner = Owner::fresh();
    let other = Owner::fresh();
    let capture = layer_ui::WorkspaceCapture::from_template(&layer_ui::DockLayout::for_platform(
        layer_ui::Platform::Web,
    ))
    .unwrap();
    let first = Entity::workspace(
        "Drawing",
        capture.clone(),
        capture.history.layout().clone(),
        None,
        1000,
    );
    let second = Entity::workspace(
        "Painting",
        capture.clone(),
        capture.history.layout().clone(),
        None,
        1000,
    );
    let create = |entity: Entity, claim| Mutation::Create {
        entity,
        claim,
        name_policy: NamePolicy::Exact,
    };
    let mut batch = CommitBatch::prepare(
        owner.clone(),
        vec![create(first.clone(), true), create(second.clone(), false)],
    )
    .unwrap();
    batch
        .bindings
        .push(("last_workspace".into(), Some(first.id.clone())));
    batch
        .legacy_imports
        .push(("legacy:first".into(), first.id.clone()));
    let original = batch.clone();
    execute(
        StoreRequest::Commit {
            batch: batch.clone(),
        },
        1000,
    );
    execute(
        StoreRequest::Commit {
            batch: batch.clone(),
        },
        1001,
    );
    execute(StoreRequest::List, 1001);
    execute(StoreRequest::Switcher, 1001);
    let pins = vec![second.id.clone(), first.id.clone()];
    execute(
        StoreRequest::UpdateSwitcher {
            expected: None,
            ids: pins.clone(),
        },
        1001,
    );
    // Same delivery is idempotent; stale and invalid replacements cannot win.
    execute(
        StoreRequest::UpdateSwitcher {
            expected: None,
            ids: pins.clone(),
        },
        1001,
    );
    execute(
        StoreRequest::UpdateSwitcher {
            expected: None,
            ids: vec![],
        },
        1001,
    );
    execute(
        StoreRequest::UpdateSwitcher {
            expected: Some(pins.clone()),
            ids: vec![first.id.clone(), first.id.clone()],
        },
        1001,
    );
    execute(
        StoreRequest::UpdateSwitcher {
            expected: Some(pins),
            ids: vec![],
        },
        1001,
    );
    execute(StoreRequest::Switcher, 1001);
    execute(
        StoreRequest::Load {
            id: first.id.clone(),
        },
        1001,
    );
    execute(
        StoreRequest::Receipt {
            operation_id: batch.operation_id.clone(),
        },
        1001,
    );
    execute(StoreRequest::Pending, 1001);
    execute(
        StoreRequest::Acknowledge {
            operation_id: batch.operation_id.clone(),
        },
        1001,
    );
    execute(StoreRequest::Pending, 1001);
    execute(
        StoreRequest::Binding {
            key: "last_workspace".into(),
        },
        1001,
    );
    execute(
        StoreRequest::LegacyImport {
            source: "legacy:first".into(),
        },
        1001,
    );
    // Operation identity binds immutable content, including bindings/imports.
    batch.bindings.clear();
    execute(StoreRequest::Commit { batch }, 1002);
    execute(
        StoreRequest::Claim {
            id: first.id.clone(),
            owner: other.clone(),
        },
        1002,
    );
    execute(
        StoreRequest::Release {
            id: first.id.clone(),
            owner: other.clone(),
            fence: "1".into(),
        },
        1002,
    );
    execute(
        StoreRequest::Renew {
            id: first.id.clone(),
            owner: owner.clone(),
            fence: "1".into(),
        },
        1002,
    );
    let generations = Generations {
        metadata: 1,
        layout: 1,
        working: 1,
    };
    let update = |metadata, working| Mutation::Update {
        id: first.id.clone(),
        generations,
        fence: 1,
        metadata,
        content: None,
        working,
        name_policy: NamePolicy::Exact,
    };
    let mut working = capture.working.clone();
    working.zen_mode = true;
    let batch =
        CommitBatch::prepare(owner.clone(), vec![update(None, Some(working.clone()))]).unwrap();
    execute(StoreRequest::Commit { batch }, 1003);
    // Metadata and working generations are independent.
    let mut metadata = first.metadata.clone();
    metadata.name = "Ink".into();
    let batch = CommitBatch::prepare(owner.clone(), vec![update(Some(metadata), None)]).unwrap();
    execute(StoreRequest::Commit { batch }, 1003);
    let fresh = Entity::workspace(
        "Should roll back",
        capture.clone(),
        capture.history.layout().clone(),
        None,
        1000,
    );
    let batch = CommitBatch::prepare(
        owner.clone(),
        vec![
            create(fresh.clone(), false),
            update(None, Some(working.clone())),
        ],
    )
    .unwrap();
    execute(
        StoreRequest::Commit {
            batch: batch.clone(),
        },
        1004,
    );
    execute(
        StoreRequest::Load {
            id: fresh.id.clone(),
        },
        1004,
    );
    execute(StoreRequest::Pending, 1004);
    // Concurrent migration cannot insert an independent copy of the same source.
    let mut duplicate =
        CommitBatch::prepare(other.clone(), vec![create(fresh.clone(), false)]).unwrap();
    duplicate.legacy_imports = original.legacy_imports;
    // SQLite reports constraint errors with backend-specific kinds; test atomic
    // import rejection separately below instead of comparing error wording/kind.
    execute(
        StoreRequest::Claim {
            id: first.id.clone(),
            owner: other.clone(),
        },
        32000,
    );
    let stale = CommitBatch::prepare(owner.clone(), vec![update(None, Some(working))]).unwrap();
    execute(StoreRequest::Commit { batch: stale }, 32000);
    execute(
        StoreRequest::Release {
            id: first.id.clone(),
            owner,
            fence: "1".into(),
        },
        32000,
    );
    execute(
        StoreRequest::Renew {
            id: first.id.clone(),
            owner: other.clone(),
            fence: "2".into(),
        },
        32001,
    );
    execute(
        StoreRequest::Load {
            id: first.id.clone(),
        },
        32001,
    );
    drop(execute);
    if let Ok(path) = std::env::var("CAPY_STORE_CONTRACT_FIXTURE") {
        std::fs::write(path, serde_json::to_string(&fixture).unwrap()).unwrap();
    }
    let before = browser.encoded().unwrap();
    assert!(
        browser
            .execute(StoreRequest::Commit { batch: duplicate }, 32002)
            .is_err()
    );
    assert_eq!(browser.encoded().unwrap(), before);
    drop(sqlite);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn browser_preserves_newer_schemas_and_exact_large_counters() {
    let owner = Owner::fresh();
    let capture =
        layer_ui::WorkspaceCapture::from_template(&layer_ui::DockLayout::default()).unwrap();
    let entity = Entity::workspace(
        "Drawing",
        capture.clone(),
        capture.history.layout().clone(),
        None,
        1,
    );
    let id = entity.id.clone();
    let mut db = BrowserDatabase::default();
    let batch = CommitBatch::prepare(
        owner.clone(),
        vec![Mutation::Create {
            entity,
            claim: true,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    db.execute(StoreRequest::Commit { batch }, 1).unwrap();
    let mut encoded: serde_json::Value = serde_json::from_str(&db.encoded().unwrap()).unwrap();
    encoded["schema"] = serde_json::json!(999);
    assert_eq!(
        BrowserDatabase::decode(&encoded.to_string())
            .err()
            .unwrap()
            .kind,
        ErrorKind::UnsupportedSchema
    );
    encoded["schema"] = serde_json::json!(SCHEMA_VERSION);
    encoded["fences"][&id] = serde_json::json!("9007199254740993");
    encoded["items"][&id]["claim"]["fence"] = serde_json::json!("9007199254740993");
    let mut db = BrowserDatabase::decode(&encoded.to_string()).unwrap();
    let StoreResponse::Entity(s) = db
        .execute(StoreRequest::Claim { id, owner }, 100000)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(s.claim.unwrap().fence, 9007199254740994);
}

#[test]
fn browser_schema_two_upgrade_preserves_existing_records_and_defaults_switcher() {
    let mut old = BrowserDatabase::default();
    let layout = layer_ui::DockLayout::for_platform(layer_ui::Platform::Web);
    let entity = Entity::workspace(
        "Existing workspace",
        layer_ui::WorkspaceCapture::from_template(&layout).unwrap(),
        layout,
        None,
        1000,
    );
    let batch = CommitBatch::prepare(
        Owner::fresh(),
        vec![Mutation::Create {
            entity: entity.clone(),
            claim: false,
            name_policy: NamePolicy::Exact,
        }],
    )
    .unwrap();
    old.prepare_delivery(&batch).unwrap();
    old.execute(StoreRequest::Commit { batch }, 1000).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&old.encoded().unwrap()).unwrap();
    value["schema"] = serde_json::json!(2);
    value.as_object_mut().unwrap().remove("switcher");
    let mut upgraded = BrowserDatabase::decode(&value.to_string()).unwrap();
    assert!(
        matches!(upgraded.execute(StoreRequest::Load { id: entity.id.clone() }, 1000).unwrap(), StoreResponse::Entity(stored) if stored.entity == entity)
    );
    assert!(matches!(
        upgraded.execute(StoreRequest::Switcher, 1000).unwrap(),
        StoreResponse::Switcher(None)
    ));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&upgraded.encoded().unwrap()).unwrap()["schema"],
        SCHEMA_VERSION
    );
}
