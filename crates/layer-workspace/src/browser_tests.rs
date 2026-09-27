use crate::test_support::*;
use crate::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
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
fn browser_claims_of_closed_documents_are_released_without_waiting_for_the_lease() {
    let mut browser = BrowserDatabase::default();
    let closed = Owner::fresh();
    let reopened = Owner::fresh();
    let entity = workspace("Browser lock");
    let id = entity.id.clone();
    let batch = CommitBatch::prepare(closed.clone(), vec![create(entity)]).unwrap();
    browser
        .execute(StoreRequest::Commit { batch }, 1000)
        .unwrap();
    let claim = StoreRequest::Claim {
        id: id.clone(),
        owner: reopened.clone(),
    };
    assert!(!browser.release_unlocked(&[closed.id.clone()], 1001));
    assert!(
        !browser.release_unlocked(&[], 999),
        "A claim made after the lock query stays"
    );
    assert_eq!(
        browser.execute(claim.clone(), 1002).unwrap_err().kind,
        ErrorKind::OwnedElsewhere
    );
    assert!(browser.release_unlocked(&[reopened.id.clone()], 1001));
    let StoreResponse::Entity(successor) = browser.execute(claim, 1002).unwrap() else {
        panic!()
    };
    assert_eq!(successor.claim.as_ref().unwrap().owner, reopened);
    assert_eq!(successor.claim.as_ref().unwrap().fence, 2);
    assert_eq!(
        browser
            .execute(
                StoreRequest::Renew {
                    id,
                    owner: closed,
                    fence: "1".into()
                },
                1003
            )
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
}
#[test]
fn browser_leases_still_expire_and_fence_stale_writers() {
    let mut browser = BrowserDatabase::default();
    let owner = Owner::fresh();
    let other = Owner::fresh();
    let entity = workspace("Browser lease");
    let id = entity.id.clone();
    let batch = CommitBatch::prepare(owner.clone(), vec![create(entity)]).unwrap();
    browser
        .execute(StoreRequest::Commit { batch }, 1000)
        .unwrap();
    let request = StoreRequest::Claim {
        id: id.clone(),
        owner: other.clone(),
    };
    assert_eq!(
        browser.execute(request.clone(), 1001).unwrap_err().kind,
        ErrorKind::OwnedElsewhere
    );
    let StoreResponse::Entity(successor) = browser.execute(request, 1000 + OWNER_LEASE_MS).unwrap()
    else {
        panic!()
    };
    assert_eq!(successor.claim.as_ref().unwrap().fence, 2);
    assert_eq!(
        browser
            .execute(
                StoreRequest::Renew {
                    id: id.clone(),
                    owner: owner.clone(),
                    fence: "1".into()
                },
                1000 + OWNER_LEASE_MS
            )
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    browser
        .execute(
            StoreRequest::Release {
                id: id.clone(),
                owner,
                fence: "1".into(),
            },
            1000 + OWNER_LEASE_MS,
        )
        .unwrap();
    let StoreResponse::Entity(current) = browser
        .execute(StoreRequest::Load { id }, 1000 + OWNER_LEASE_MS)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(current.claim, successor.claim);
}

#[test]
fn browser_transactions_match_sqlite_contract() {
    let directory = temp_dir("browser-contract");
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
        expected
    };
    let owner = Owner::fresh();
    let other = Owner::fresh();
    let first = workspace("Drawing");
    let second = workspace("Painting");
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
    execute(StoreRequest::WorkspaceOrder, 1001);
    let order = vec![first.id.clone(), second.id.clone()];
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: None,
            ids: order.clone(),
        },
        1001,
    );
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: None,
            ids: order.clone(),
        },
        1001,
    );
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: None,
            ids: vec![],
        },
        1001,
    );
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: Some(order.clone()),
            ids: vec![first.id.clone(), first.id.clone()],
        },
        1001,
    );
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: Some(order.clone()),
            ids: vec!["missing".into()],
        },
        1001,
    );
    execute(
        StoreRequest::UpdateWorkspaceOrder {
            expected: Some(order.clone()),
            ids: vec![second.id.clone(), first.id.clone()],
        },
        1001,
    );
    execute(StoreRequest::WorkspaceOrder, 1001);
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
    // Operation identity binds immutable content, including bindings.
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
    let mut working = first.working.clone().unwrap();
    working.zen_mode = true;
    let batch =
        CommitBatch::prepare(owner.clone(), vec![update(None, Some(working.clone()))]).unwrap();
    execute(StoreRequest::Commit { batch }, 1003);
    // Metadata and working generations are independent.
    let mut metadata = first.metadata.clone();
    metadata.name = "Ink".into();
    let batch = CommitBatch::prepare(owner.clone(), vec![update(Some(metadata), None)]).unwrap();
    execute(StoreRequest::Commit { batch }, 1003);
    let fresh = workspace("Should roll back");
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
    // A live native OS lock never expires. Exercise common release/takeover
    // here; browser-only timeout behavior has its own regression below.
    execute(
        StoreRequest::Release {
            id: first.id.clone(),
            owner: owner.clone(),
            fence: "1".into(),
        },
        1004,
    );
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
    // Pinning shares creation's transaction, rollback and idempotent receipt.
    let third = workspace("Sketching");
    let mut pinned =
        CommitBatch::prepare(other.clone(), vec![create(third.clone(), true)]).unwrap();
    pinned.pin_workspaces.push(third.id.clone());
    let mut failed =
        CommitBatch::prepare(other.clone(), vec![create(third.clone(), true)]).unwrap();
    failed.pin_workspaces.push(third.id.clone());
    failed
        .bindings
        .push(("last_workspace".into(), Some("missing".into())));
    execute(StoreRequest::Commit { batch: failed }, 32002);
    execute(
        StoreRequest::Load {
            id: third.id.clone(),
        },
        32002,
    );
    assert_eq!(
        execute(StoreRequest::Switcher, 32002),
        normalize(Ok(StoreResponse::Switcher(Some(vec![]))))
    );
    execute(
        StoreRequest::Commit {
            batch: pinned.clone(),
        },
        32003,
    );
    execute(
        StoreRequest::Commit {
            batch: pinned.clone(),
        },
        32004,
    );
    assert_eq!(
        execute(StoreRequest::Switcher, 32004),
        normalize(Ok(StoreResponse::Switcher(Some(vec![third.id.clone()]))))
    );
    execute(
        StoreRequest::UpdateSwitcher {
            expected: Some(vec![third.id.clone()]),
            ids: vec![],
        },
        32005,
    );
    execute(StoreRequest::Commit { batch: pinned }, 32006);
    assert_eq!(
        execute(StoreRequest::Switcher, 32006),
        normalize(Ok(StoreResponse::Switcher(Some(vec![]))))
    );
    let mut invalid = CommitBatch::prepare(other.clone(), vec![]).unwrap();
    invalid.pin_workspaces.push(first.id.clone());
    execute(StoreRequest::Commit { batch: invalid }, 32007);
    let delete = |fence| {
        let mutation = Mutation::Delete {
            id: third.id.clone(),
            generations,
            fence,
        };
        CommitBatch::prepare(other.clone(), vec![mutation]).unwrap()
    };
    execute(StoreRequest::Commit { batch: delete(2) }, 32008);
    execute(StoreRequest::Commit { batch: delete(1) }, 32008);
    execute(
        StoreRequest::Load {
            id: third.id.clone(),
        },
        32008,
    );
    let batch = CommitBatch::prepare(other, vec![create(third, true)]).unwrap();
    execute(StoreRequest::Commit { batch }, 32009);
    if let Ok(path) = std::env::var("CAPY_STORE_CONTRACT_FIXTURE") {
        std::fs::write(path, serde_json::to_string(&fixture).unwrap()).unwrap();
    }
    drop(sqlite);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn browser_snapshots_of_other_versions_decode_only_by_reset() {
    let owner = Owner::fresh();
    let mut db = BrowserDatabase::default();
    let batch = CommitBatch::prepare(owner.clone(), vec![create(workspace("Older"))]).unwrap();
    db.execute(StoreRequest::Commit { batch }, 1).unwrap();
    let mut encoded: serde_json::Value = serde_json::from_str(&db.encoded().unwrap()).unwrap();
    for schema in [
        serde_json::json!(SCHEMA_VERSION - 1),
        serde_json::Value::Null,
    ] {
        encoded["schema"] = schema;
        let text = encoded.to_string();
        assert_eq!(
            BrowserDatabase::decode(&text).err().unwrap().kind,
            ErrorKind::UnsupportedSchema
        );
        let mut db = BrowserDatabase::replace(Some(&text)).unwrap();
        let StoreResponse::List(items) = db.execute(StoreRequest::List, 2).unwrap() else {
            panic!()
        };
        assert!(items.is_empty());
        let batch =
            CommitBatch::prepare(owner.clone(), vec![create(workspace("Current"))]).unwrap();
        db.execute(StoreRequest::Commit { batch }, 3).unwrap();
        let written: serde_json::Value = serde_json::from_str(&db.encoded().unwrap()).unwrap();
        assert_eq!(written["schema"], SCHEMA_VERSION);
        assert_eq!(written["items"].as_object().unwrap().len(), 1);
    }
}

#[test]
fn browser_preserves_newer_schemas_and_exact_large_counters() {
    let owner = Owner::fresh();
    let entity = workspace("Drawing");
    let id = entity.id.clone();
    let mut db = BrowserDatabase::default();
    let batch = CommitBatch::prepare(owner.clone(), vec![create(entity)]).unwrap();
    db.execute(StoreRequest::Commit { batch }, 1).unwrap();
    let mut encoded: serde_json::Value = serde_json::from_str(&db.encoded().unwrap()).unwrap();
    encoded["schema"] = serde_json::json!(999);
    for result in [
        BrowserDatabase::decode(&encoded.to_string()),
        BrowserDatabase::replace(Some(&encoded.to_string())),
    ] {
        assert_eq!(result.err().unwrap().kind, ErrorKind::UnsupportedSchema);
    }
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
