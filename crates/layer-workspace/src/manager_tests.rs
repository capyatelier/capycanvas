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
    fail_release: Cell<bool>,
    lose_reply: Cell<bool>,
    block_receipts: Cell<bool>,
    gate: RefCell<Option<async_channel::Receiver<()>>>,
    waiting: Cell<bool>,
}
impl WorkspaceStore for TestStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        if matches!(&request, StoreRequest::Release { .. }) && self.fail_release.get() {
            return Err(StoreError::new(
                ErrorKind::FailedWrite,
                "Simulated release failure",
            ));
        }
        if matches!(request, StoreRequest::Receipt { .. }) && self.block_receipts.get() {
            return Err(StoreError::new(
                ErrorKind::Unavailable,
                "Receipt temporarily unavailable",
            ));
        }
        let commit = matches!(&request, StoreRequest::Commit { .. });
        if commit && self.fail.get() {
            return Err(StoreError::new(
                ErrorKind::FailedWrite,
                "Simulated failed write",
            ));
        }
        let response = self.worker.request(request).await?;
        if commit && self.lose_reply.replace(false) {
            return Err(StoreError::new(
                ErrorKind::FailedWrite,
                "Simulated lost acknowledgement",
            ));
        }
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
                fail_release: Cell::new(false),
                lose_reply: Cell::new(false),
                block_receipts: Cell::new(false),
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
fn concurrent_default_catalog_creation_retires_only_the_duplicate_seed() {
    struct RacingStore {
        worker: StoreWorker,
        race: Cell<bool>,
    }
    impl WorkspaceStore for RacingStore {
        async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
            if matches!(request, StoreRequest::Commit { .. }) && self.race.replace(false) {
                let other = WorkspaceManager::new(self.worker.clone(), Platform::Gtk);
                other.initialize_catalog(1_000).await?;
            }
            self.worker.execute(request).await
        }
    }
    pollster::block_on(async {
        let directory = std::env::temp_dir().join(format!("capy-defaults-race-{}", new_id()));
        let worker = StoreWorker::shared(&directory).unwrap();
        let m = WorkspaceManager::new(
            RacingStore {
                worker,
                race: Cell::new(true),
            },
            Platform::Gtk,
        );
        m.initialize_catalog(1_000).await.unwrap();
        assert_eq!(m.items().len(), 3);
        assert!(!m.has_failed_operation());
        assert!(
            matches!(m.store.execute(StoreRequest::Pending).await.unwrap(), StoreResponse::Pending(p) if p.is_empty())
        );
        let initial = m.initialize(2_000).await.unwrap();
        assert_eq!(initial.entity.id, DEFAULT_WORKSPACES[1].0);
        m.activate(initial);
        let prompt = m.prompt(&ManagerAction::New, 2_000).await.unwrap();
        assert!(prompt.choices.is_empty() && prompt.choice_label.is_none());
        assert_eq!(prompt.name.as_deref(), Some("New Workspace"));
        m.close().await.unwrap();
        drop(m);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn default_catalog_is_protected_and_workspace_edits_survive_switching_and_restart() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        assert_eq!(m.items().len(), 3);
        assert_eq!(m.active_id().as_deref(), Some(DEFAULT_WORKSPACES[1].0));
        for (id, preset) in DEFAULT_WORKSPACES {
            let workspace = m.load(id).await.unwrap();
            assert_eq!(workspace.entity.metadata.name, preset.name());
            assert!(workspace.entity.metadata.builtin && !workspace.entity.metadata.read_only());
            assert_eq!(
                workspace.entity.capture().unwrap().history.layout(),
                &preset.layout(Platform::Gtk)
            );
            assert!(m.delete_item(id, None, 2_000).await.is_err());
            let details = m.details(&workspace, true, 2_000);
            assert!(
                details
                    .actions
                    .iter()
                    .any(|a| matches!(a.action, ManagerAction::Rename(_)) && a.enabled)
            );
            assert!(
                !details
                    .actions
                    .iter()
                    .any(|a| matches!(a.action, ManagerAction::Delete(_)) && a.enabled)
            );
        }
        let painter = DEFAULT_WORKSPACES[0].0;
        let incoming = m.prepare_switch(painter, 3_000).await.unwrap();
        let outgoing = m.activate(incoming).unwrap();
        m.release(&outgoing).await;
        let mut capture = m.current().unwrap().capture().unwrap();
        let mut layout = capture.history.layout().clone();
        layout.bands[0].extent += 60.;
        capture.history.append(&layout, "Resize Tools toolbar");
        capture
            .working
            .tools
            .set_override(capture.working.preset, "size", 73.)
            .unwrap();
        m.observe(capture.clone(), 4_000);
        m.rename(painter, "My Painting", "", 5_000).await.unwrap();
        let incoming = m
            .prepare_switch(DEFAULT_WORKSPACES[2].0, 6_000)
            .await
            .unwrap();
        let outgoing = m.activate(incoming).unwrap();
        m.release(&outgoing).await;
        let restored = m.prepare_switch(painter, 7_000).await.unwrap();
        assert_eq!(restored.entity.metadata.name, "My Painting");
        assert_eq!(
            restored.entity.capture().unwrap(),
            m.load(painter).await.unwrap().entity.capture().unwrap()
        );
        assert_eq!(restored.entity.capture().unwrap().working, capture.working);
        assert_eq!(restored.entity.capture().unwrap().history.layout(), &layout);
        let outgoing = m.activate(restored).unwrap();
        m.release(&outgoing).await;
        m.close().await.unwrap();
        let reopened =
            WorkspaceManager::new(StoreWorker::shared(&f.directory).unwrap(), Platform::Gtk);
        let incoming = reopened.initialize(8_000).await.unwrap();
        assert_eq!(incoming.entity.id, painter);
        assert_eq!(incoming.entity.metadata.name, "My Painting");
        assert_eq!(incoming.entity.capture().unwrap().history.layout(), &layout);
        assert_eq!(incoming.entity.capture().unwrap().working, capture.working);
        assert_eq!(reopened.items().len(), 3);
        reopened.activate(incoming);
        reopened.close().await.unwrap();
    });
}

#[test]
fn photographer_size_upgrade_preserves_brush_edits_and_customized_layouts() {
    use layer_ui::{Panel, TileStyle, WorkspacePreset};
    pollster::block_on(async {
        for customized in [false, true] {
            let f = Fixture::new();
            let m = &f.manager;
            let id = DEFAULT_WORKSPACES[2].0;
            let stored = m.claim(id).await.unwrap();
            let mut previous = WorkspacePreset::Photographer.layout(Platform::Gtk);
            for panel in [Panel::Toolbar, Panel::Commands] {
                previous
                    .panels
                    .iter_mut()
                    .find(|p| p.id == panel)
                    .unwrap()
                    .tile_style = TileStyle::Medium;
            }
            previous.bands[0].extent += TileStyle::Medium.size()[0] - TileStyle::Small.size()[0];
            let mut history = layer_ui::LayoutHistory::new(&previous);
            if customized {
                let mut edited = previous.clone();
                edited.bands[1].extent += 60.;
                history.append(&edited, "Resize Layers column");
            }
            let content = ItemContent::Workspace {
                history,
                baseline: previous,
                origin: None,
            };
            let mut metadata = stored.entity.metadata.clone();
            metadata.rename("My Photos", "", 2_000).unwrap();
            let mut working = stored.entity.working.clone().unwrap();
            working
                .tools
                .set_override(working.preset, "size", 73.)
                .unwrap();
            m.publish(
                CommitBatch::prepare(
                    m.owner.clone(),
                    vec![
                        update(
                            &stored,
                            Some(metadata),
                            Some(content.clone()),
                            Some(working.clone()),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .await
            .unwrap();
            m.release(&stored).await;

            let reopened =
                WorkspaceManager::new(StoreWorker::shared(&f.directory).unwrap(), Platform::Gtk);
            let incoming = reopened.prepare_switch(id, 3_000).await.unwrap();
            assert_eq!(incoming.entity.metadata.name, "My Photos");
            assert_eq!(incoming.entity.working.as_ref(), Some(&working));
            if customized {
                assert_eq!(incoming.entity.content, content);
            } else {
                let ItemContent::Workspace {
                    history, baseline, ..
                } = &incoming.entity.content
                else {
                    panic!("Expected workspace");
                };
                assert_eq!(
                    baseline,
                    &WorkspacePreset::Photographer.layout(Platform::Gtk)
                );
                assert_eq!(history.layout(), baseline);
                assert_eq!(history.revisions.len(), 1);
                assert_eq!(history.generation, 0);
            }
            assert_eq!(reopened.load(id).await.unwrap().entity, incoming.entity);
            let saved_content = incoming.entity.content.clone();
            reopened.activate(incoming);
            reopened.close().await.unwrap();
            let incoming = reopened.initialize(4_000).await.unwrap();
            assert_eq!(incoming.entity.content, saved_content);
            reopened.activate(incoming);
            reopened.close().await.unwrap();
        }
    });
}

#[test]
fn default_catalog_upgrade_preserves_existing_workspace_and_name_collisions() {
    pollster::block_on(async {
        let directory = std::env::temp_dir().join(format!("capy-presets-upgrade-{}", new_id()));
        let worker = StoreWorker::shared(&directory).unwrap();
        let owner = Owner::fresh();
        let mut layout = DockLayout::for_platform(Platform::Gtk);
        layout.bands[0].extent += 80.;
        let user = Entity::workspace(
            "Painter",
            WorkspaceCapture::from_template(&layout).unwrap(),
            layout.clone(),
            None,
            500,
        );
        let user_id = user.id.clone();
        let mut legacy = Entity::reusable(
            "Default",
            "The standard editor layout.",
            ReusableContent::Layout {
                layout: DockLayout::for_platform(Platform::Gtk),
            },
            400,
        );
        legacy.id = DEFAULT_TEMPLATE_ID.into();
        legacy.metadata.builtin = true;
        worker
            .execute(StoreRequest::Commit {
                batch: CommitBatch::prepare(
                    owner,
                    vec![
                        Mutation::Create {
                            entity: user,
                            claim: false,
                            name_policy: NamePolicy::Exact,
                        },
                        Mutation::Create {
                            entity: legacy.clone(),
                            claim: false,
                            name_policy: NamePolicy::Exact,
                        },
                    ],
                )
                .unwrap(),
            })
            .await
            .unwrap();
        let manager = WorkspaceManager::new(worker, Platform::Gtk);
        let incoming = manager.initialize(1_000).await.unwrap();
        assert_eq!(incoming.entity.id, user_id);
        assert_eq!(incoming.entity.metadata.name, "Painter");
        assert_eq!(incoming.entity.capture().unwrap().history.layout(), &layout);
        assert_eq!(manager.items().len(), 4);
        assert_eq!(manager.rows(ManagerPage::Workspaces, "", 1_000).len(), 4);
        assert_eq!(
            manager.load(DEFAULT_TEMPLATE_ID).await.unwrap().entity,
            legacy
        );
        assert_eq!(
            manager
                .load(DEFAULT_WORKSPACES[0].0)
                .await
                .unwrap()
                .entity
                .metadata
                .name,
            "Painter (2)"
        );
        manager.activate(incoming);
        manager.close().await.unwrap();
        drop(manager);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn close_retains_ownership_until_release_is_acknowledged_and_can_be_retried() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let id = m.active_id().unwrap();
        let lease = m.lease_expires_at_ms();
        m.store.fail_release.set(true);
        assert!(m.close().await.is_err());
        assert_eq!(m.lease_expires_at_ms(), lease);
        assert!(m.load(&id).await.unwrap().claim.is_some());
        m.store.fail_release.set(false);
        m.close().await.unwrap();
        assert!(m.load(&id).await.unwrap().claim.is_none());
        assert!(m.lease_expires_at_ms().is_none());
        m.revalidate_owner(2_000).await.unwrap();
        assert!(m.load(&id).await.unwrap().claim.is_some());
    });
}

#[test]
fn legacy_scenes_migrate_atomically_once_with_fallback_aliases_and_original_baselines() {
    pollster::block_on(async {
        for platform in [Platform::Ios, Platform::Mac] {
            let f = Fixture::new();
            let m = &f.manager;
            let before = m.items().len();
            let mut workspace = layer_ui::WorkspaceState::for_platform(platform);
            workspace.zen_mode = true;
            let scenes = vec![
                ("apple:scene:one".into(), workspace.clone()),
                ("apple:scene:two".into(), workspace.clone()),
            ];
            let fallback = ("apple:fallback".into(), workspace.clone());
            let mapping = m
                .migrate_legacy(&scenes, Some(&fallback), 2000)
                .await
                .unwrap();
            assert_ne!(mapping["apple:scene:one"], mapping["apple:scene:two"]);
            assert_eq!(mapping["apple:fallback"], mapping["apple:scene:one"]);
            assert_eq!(m.items().len(), before + 2);
            let entity = m.load(&mapping["apple:scene:one"]).await.unwrap().entity;
            let capture = entity.capture().unwrap();
            assert!(capture.working.zen_mode && capture.history.undo.is_empty());
            assert_eq!(capture.history.layout(), &workspace.layout);
            let ItemContent::Workspace {
                baseline, origin, ..
            } = entity.content
            else {
                panic!()
            };
            assert_eq!(baseline, workspace.layout);
            assert!(origin.is_none());
            // A later legacy file cannot replace acknowledged database content.
            let mut stale = scenes.clone();
            stale[0].1.version = 999;
            assert_eq!(
                m.migrate_legacy(&stale, Some(&fallback), 3000)
                    .await
                    .unwrap(),
                mapping
            );
            assert_eq!(m.items().len(), before + 2);
            let invalid = vec![
                ("apple:new:valid".into(), workspace.clone()),
                ("apple:new:invalid".into(), stale[0].1.clone()),
            ];
            assert!(m.migrate_legacy(&invalid, None, 4000).await.is_err());
            assert!(matches!(
                m.store
                    .execute(StoreRequest::LegacyImport {
                        source: "apple:new:valid".into()
                    })
                    .await
                    .unwrap(),
                StoreResponse::Binding(None)
            ));
            let separate = (
                "apple:other:fallback".into(),
                layer_ui::WorkspaceState::for_platform(platform),
            );
            let other = m.migrate_legacy(&[], Some(&separate), 5000).await.unwrap();
            assert_ne!(other["apple:other:fallback"], mapping["apple:fallback"]);
            assert_eq!(m.items().len(), before + 3);
            m.bind_resume_key("apple:resume:one").await.unwrap();
            assert!(
                matches!(m.store.execute(StoreRequest::Binding { key: "apple:resume:one".into() }).await.unwrap(), StoreResponse::Binding(Some(id)) if Some(id.as_str())==m.active_id().as_deref())
            );
        }
    });
}

#[test]
fn concurrent_legacy_import_uses_the_winning_mapping_and_retires_its_duplicate_delivery() {
    struct RacingStore {
        worker: StoreWorker,
        scenes: Vec<(String, layer_ui::WorkspaceState)>,
        inject: Cell<bool>,
    }
    impl WorkspaceStore for RacingStore {
        async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
            if matches!(&request, StoreRequest::Commit { batch } if !batch.legacy_imports.is_empty())
                && self.inject.replace(false)
            {
                let other = WorkspaceManager::new(self.worker.clone(), Platform::Mac);
                other.migrate_legacy(&self.scenes, None, 2000).await?;
            }
            self.worker.execute(request).await
        }
    }
    pollster::block_on(async {
        for partial_overlap in [false, true] {
            let f = Fixture::new();
            let mut scenes = vec![(
                "apple:scene:race".into(),
                layer_ui::WorkspaceState::for_platform(Platform::Mac),
            )];
            let winner_sources = scenes.clone();
            if partial_overlap {
                scenes.push(("apple:scene:later".into(), scenes[0].1.clone()));
            }
            let fallback = ("apple:fallback".into(), scenes[0].1.clone());
            let m = WorkspaceManager::new(
                RacingStore {
                    worker: f.manager.store.worker.clone(),
                    scenes: winner_sources,
                    inject: Cell::new(true),
                },
                Platform::Mac,
            );
            let mapping = m
                .migrate_legacy(&scenes, Some(&fallback), 2000)
                .await
                .unwrap();
            assert_eq!(m.items().len(), f.manager.items().len() + scenes.len());
            assert_eq!(mapping["apple:fallback"], mapping["apple:scene:race"]);
            assert!(m.load(&mapping["apple:scene:race"]).await.is_ok());
            assert!(m.error().is_none() && !m.has_failed_operation());
            assert!(
                matches!(m.store.execute(StoreRequest::Pending).await.unwrap(), StoreResponse::Pending(pending) if pending.is_empty())
            );
        }
    });
}

#[test]
fn resumed_owner_revalidates_without_losing_dirty_edits_or_overwriting_successors() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let id = m.active_id().unwrap();
        let expire = || {
            m.state
                .borrow_mut()
                .saved
                .as_mut()
                .unwrap()
                .claim
                .as_mut()
                .unwrap()
                .expires_at_ms = 0;
            let sql = rusqlite::Connection::open(f.directory.join("workspaces.sqlite3")).unwrap();
            sql.execute("UPDATE items SET lease_until='0' WHERE id=?1", [&id])
                .unwrap();
        };
        let mut capture = m.current().unwrap().capture().unwrap();
        capture.working.zen_mode = true;
        m.observe(capture.clone(), 2_000);
        expire();
        m.revalidate_owner(1).await.unwrap();
        assert!(m.dirty());
        assert_eq!(m.current().unwrap().working, Some(capture.working.clone()));
        assert!(m.current_record().unwrap().claim.unwrap().fence > 1);
        m.flush().await.unwrap();
        expire();
        let other = Owner::fresh();
        let StoreResponse::Entity(successor) = m
            .store
            .worker
            .request(StoreRequest::Claim {
                id: id.clone(),
                owner: other.clone(),
            })
            .await
            .unwrap()
        else {
            panic!()
        };
        let mut working = capture.working.clone();
        working.zen_mode = false;
        m.store
            .worker
            .request(StoreRequest::Commit {
                batch: CommitBatch::prepare(
                    other.clone(),
                    vec![update(&successor, None, None, Some(working)).unwrap()],
                )
                .unwrap(),
            })
            .await
            .unwrap();
        assert_eq!(
            m.revalidate_owner(1).await.unwrap_err().kind,
            ErrorKind::OwnedElsewhere
        );
        assert_eq!(m.current().unwrap().working, Some(capture.working.clone()));
        m.store
            .worker
            .request(StoreRequest::Release {
                id: id.clone(),
                owner: other,
                fence: successor.claim.unwrap().fence.to_string(),
            })
            .await
            .unwrap();
        assert_eq!(
            m.revalidate_owner(1).await.unwrap_err().kind,
            ErrorKind::Conflict
        );
        let recovered = m
            .save_as_new(capture.clone(), "Recovered", 3_000)
            .await
            .unwrap();
        assert_ne!(recovered.entity.id, id);
        assert_eq!(recovered.entity.working, Some(capture.working));
        assert!(!m.load(&id).await.unwrap().entity.working.unwrap().zen_mode);
    });
}

#[test]
fn expired_owner_resolves_a_committed_save_before_reacquiring_its_claim() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let id = m.active_id().unwrap();
        let mut capture = m.current().unwrap().capture().unwrap();
        capture.working.zen_mode = true;
        m.observe(capture.clone(), 2_000);
        m.store.lose_reply.set(true);
        m.store.block_receipts.set(true);
        assert!(m.save_once().await.is_err());
        assert!(m.dirty());
        m.store.block_receipts.set(false);
        m.state
            .borrow_mut()
            .saved
            .as_mut()
            .unwrap()
            .claim
            .as_mut()
            .unwrap()
            .expires_at_ms = 0;
        let sql = rusqlite::Connection::open(f.directory.join("workspaces.sqlite3")).unwrap();
        sql.execute("UPDATE items SET lease_until='0' WHERE id=?1", [&id])
            .unwrap();
        m.revalidate_owner(1).await.unwrap();
        assert!(!m.dirty());
        assert!(m.error().is_none());
        assert_eq!(m.current().unwrap().working, Some(capture.working));
    });
}

#[test]
fn failed_named_creation_retries_its_identity_and_keeps_later_outgoing_edits() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let outgoing = m.active_id().unwrap();
        let before = m.items().len();
        m.store.lose_reply.set(true);
        m.store.block_receipts.set(true);
        assert!(
            m.create_workspace("Pending Creation", None, false, 2_000)
                .await
                .is_err()
        );
        assert!(m.has_failed_operation());
        assert_eq!(m.active_id(), Some(outgoing.clone()));
        let mut capture = m.current().unwrap().capture().unwrap();
        capture.working.zen_mode = true;
        m.observe(capture, 3_000);
        m.store.block_receipts.set(false);
        let incoming = m.retry_failed_operation().await.unwrap().unwrap();
        m.activate(incoming);
        assert_eq!(m.active_name().as_deref(), Some("Pending Creation"));
        assert_eq!(
            m.switcher_ids()
                .iter()
                .filter(|id| Some(*id) == m.active_id().as_ref())
                .count(),
            1
        );
        assert_eq!(m.items().len(), before + 1);
        assert!(!m.has_failed_operation());
        assert!(m.error().is_none());
        assert!(
            m.load(&outgoing)
                .await
                .unwrap()
                .entity
                .working
                .unwrap()
                .zen_mode
        );
        assert!(!m.current().unwrap().working.unwrap().zen_mode);
        m.store.fail.set(true);
        assert!(
            m.save_template("Pending Template", "", 4_000)
                .await
                .is_err()
        );
        assert!(m.has_failed_operation());
        m.store.fail.set(false);
        m.retry_failed_operation().await.unwrap();
        assert_eq!(
            m.items()
                .iter()
                .filter(|i| i.metadata.name == "Pending Template")
                .count(),
            1
        );
    });
}

#[test]
fn immediate_receipt_recovery_prevents_duplicate_named_actions() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let before = m.items().len();
        m.store.lose_reply.set(true);
        let incoming = m
            .create_workspace("Accepted Once", None, false, 2_000)
            .await
            .unwrap();
        assert_eq!(incoming.entity.metadata.name, "Accepted Once");
        assert_eq!(m.items().len(), before + 1);
        assert!(!m.has_failed_operation());
    });
}

#[test]
fn interrupted_publication_recovers_after_reopen_and_cancels_delayed_delivery_atomically() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let mut capture = m.current().unwrap().capture().unwrap();
        capture.working.zen_mode = true;
        m.observe(capture, 2_000);
        let sql = rusqlite::Connection::open(f.directory.join("workspaces.sqlite3")).unwrap();
        sql.execute_batch("CREATE TRIGGER interrupt_creation BEFORE INSERT ON items WHEN NEW.name='Interrupted Copy' BEGIN SELECT RAISE(ABORT,'interrupted publication'); END;").unwrap();
        assert!(
            m.create_workspace("Interrupted Copy", None, true, 3_000)
                .await
                .is_err()
        );
        let StoreResponse::Pending(pending) =
            m.store.worker.request(StoreRequest::Pending).await.unwrap()
        else {
            panic!()
        };
        let original = pending
            .iter()
            .find(|b| {
                b.writes.iter().any(|w| {
                    w.metadata
                        .as_ref()
                        .is_some_and(|m| m.name == "Interrupted Copy")
                })
            })
            .unwrap()
            .clone();
        sql.execute_batch("DROP TRIGGER interrupt_creation; UPDATE items SET owner=NULL,epoch=NULL,lease_until=NULL;").unwrap();
        let reopened = WorkspaceManager::new(m.store.worker.clone(), Platform::Gtk);
        let incoming = reopened.initialize(4_000).await.unwrap();
        reopened.activate(incoming);
        let choices = reopened.interrupted_changes(4_000).await.unwrap();
        assert!(choices.iter().any(|(id, _)| id == &original.operation_id));
        let before = reopened.items().len();
        sql.execute_batch("CREATE TRIGGER interrupt_recovery BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT,'interrupted recovery'); END;").unwrap();
        assert!(
            reopened
                .recover_interrupted(&original.operation_id, 5_000)
                .await
                .is_err()
        );
        assert_eq!(
            sql.query_row(
                "SELECT count(*) FROM cancelled_operations WHERE id=?1",
                [&original.operation_id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        reopened.refresh().await.unwrap();
        assert_eq!(reopened.items().len(), before);
        sql.execute_batch("DROP TRIGGER interrupt_recovery;")
            .unwrap();
        let recovered = reopened.retry_failed_operation().await.unwrap().unwrap();
        assert_eq!(recovered.entity.metadata.name, "Interrupted Copy Recovered");
        assert!(recovered.entity.working.as_ref().unwrap().zen_mode);
        assert_ne!(recovered.entity.id, original.writes[0].id);
        assert_eq!(reopened.items().len(), before + 1);
        assert_eq!(
            m.store
                .worker
                .request(StoreRequest::Commit {
                    batch: original.clone()
                })
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        assert!(
            reopened
                .interrupted_changes(6_000)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(reopened.load(&original.writes[0].id).await.is_err());
    });
}

#[test]
fn workspace_template_replaces_current_layout_without_creating_a_workspace() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let original = m.current().unwrap();
        let template = m.save_template("Inking", "", 2_000).await.unwrap();
        let saved_template = m.load(&template).await.unwrap().entity;
        let mut capture = original.capture().unwrap();
        let mut changed = capture.history.layout().clone();
        changed.bands[0].extent += 80.;
        capture.history.append(&changed, "Resized Tools toolbar");
        capture.working.zen_mode = true;
        capture
            .working
            .tools
            .set_override(capture.working.preset, "size", 87.)
            .unwrap();
        m.observe(capture.clone(), 3_000);
        m.flush().await.unwrap();
        let count = m.items().len();
        let before = m.current().unwrap();
        let applied = m.apply_template(&template, 4_000).await.unwrap();
        assert_eq!(applied.entity.id, original.id);
        assert_eq!(applied.entity.metadata, before.metadata);
        let after = applied.entity.capture().unwrap();
        assert_eq!(after.working, capture.working);
        assert_eq!(
            after.history.layout(),
            original.capture().unwrap().history.layout()
        );
        assert_eq!(after.history.generation, capture.history.generation + 1);
        assert_eq!(
            after.history.revisions.len(),
            capture.history.revisions.len() + 1
        );
        assert_eq!(
            after.history.revisions[&after.history.current].description,
            "Loaded “Inking” layout"
        );
        if let (
            ItemContent::Workspace {
                baseline: a,
                origin: ao,
                ..
            },
            ItemContent::Workspace {
                baseline: b,
                origin: bo,
                ..
            },
        ) = (&before.content, &applied.entity.content)
        {
            assert_eq!(a, b);
            assert_eq!(ao, bo);
        } else {
            panic!("Expected workspaces");
        }
        assert_eq!(m.load(&original.id).await.unwrap().entity, applied.entity);
        m.activate(applied);
        m.refresh().await.unwrap();
        assert_eq!(m.items().len(), count);
        assert_eq!(m.load(&template).await.unwrap().entity, saved_template);
        let mut undo = after.history.clone();
        assert!(undo.undo());
        assert_eq!(undo.layout(), &changed);
        assert!(undo.redo());
        assert_eq!(undo.layout(), after.history.layout());
        let same = m.apply_template(&template, 5_000).await.unwrap();
        assert_eq!(same.entity.capture().unwrap().history, after.history);
    });
}

#[test]
fn applying_a_workspace_template_preserves_layout_on_failure_and_retries_once() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        let template = m.save_template("Inking", "", 2_000).await.unwrap();
        let deleted = m.save_template("Removed", "", 2_001).await.unwrap();
        m.delete_item(&deleted, None, 2_002).await.unwrap();
        let mut capture = m.current().unwrap().capture().unwrap();
        let mut changed = capture.history.layout().clone();
        changed.bands[0].extent += 80.;
        capture.history.append(&changed, "Resized Tools toolbar");
        m.observe(capture.clone(), 3_000);
        m.flush().await.unwrap();
        let before = m.current().unwrap();
        assert!(m.apply_template(&deleted, 3_001).await.is_err());
        assert_eq!(m.current().unwrap(), before);
        m.store.fail.set(true);
        assert!(m.apply_template(&template, 4_000).await.is_err());
        assert_eq!(m.current().unwrap(), before);
        assert_eq!(m.load(&before.id).await.unwrap().entity, before);
        m.store.fail.set(false);
        let applied = m.retry_failed_operation().await.unwrap().unwrap();
        assert_eq!(
            applied.entity.capture().unwrap().history.generation,
            capture.history.generation + 1
        );
        assert_eq!(applied.entity.id, before.id);
        assert!(m.retry_failed_operation().await.unwrap().is_none());
    });
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
        let details = m
            .inspect_details(&m.current_record().unwrap(), true, 6_000)
            .await;
        assert_eq!(
            details
                .actions
                .iter()
                .map(|a| a.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Current workspace", "Rename…", "Delete…"]
        );
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
        assert_eq!(m.active_name().as_deref(), Some("Illustrator"));
        assert!(m.import_workspace_package(&bytes, 2_000).await.is_ok());
    });
}

#[test]
fn template_creation_duplication_switching_and_original_baselines_are_independent() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        assert_eq!(m.active_name().as_deref(), Some("Illustrator"));
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

#[test]
fn switcher_preferences_survive_restart_and_do_not_edit_or_claim_workspaces() {
    pollster::block_on(async {
        let f = Fixture::new();
        let m = &f.manager;
        m.refresh_switcher().await.unwrap();
        let defaults = DEFAULT_WORKSPACES.map(|(id, _)| id.to_string()).to_vec();
        assert_eq!(m.switcher_ids(), defaults);
        let initial = m.current_record().unwrap();
        let other =
            WorkspaceManager::new(StoreWorker::shared(&f.directory).unwrap(), Platform::Gtk);
        other.refresh().await.unwrap();
        other.refresh_switcher().await.unwrap();
        // Reordering a workspace held by another window needs no workspace lease.
        other
            .edit_switcher(SwitcherEdit::Move {
                id: defaults[1].clone(),
                before: Some(defaults[0].clone()),
            })
            .await
            .unwrap();
        m.edit_switcher(SwitcherEdit::Show {
            id: defaults[2].clone(),
            visible: false,
        })
        .await
        .unwrap();
        assert_eq!(m.switcher_ids(), [defaults[1].clone(), defaults[0].clone()]);
        assert_eq!(m.load(&initial.entity.id).await.unwrap(), initial);
        assert!(!m.dirty());
        assert!(other.active_id().is_none());
        let row_ids = m
            .rows(ManagerPage::Workspaces, "", 10_000)
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(
            row_ids,
            [
                defaults[1].clone(),
                defaults[0].clone(),
                defaults[2].clone()
            ]
        );
        let custom = m
            .create_from_snapshot(initial.entity.clone(), "Sketching", false, 11_000)
            .await
            .unwrap();
        m.release(&custom).await;
        assert!(m.switcher_ids().contains(&custom.entity.id));
        m.edit_switcher(SwitcherEdit::Show {
            id: custom.entity.id.clone(),
            visible: true,
        })
        .await
        .unwrap();
        m.edit_switcher(SwitcherEdit::Show {
            id: custom.entity.id.clone(),
            visible: true,
        })
        .await
        .unwrap();
        assert_eq!(m.switcher_ids().last(), Some(&custom.entity.id));
        assert_eq!(m.switcher_ids().len(), 3);
        let pins = m.switcher_ids();
        // Hidden rows can move anywhere without becoming visible in the bar.
        m.edit_switcher(SwitcherEdit::Move {
            id: defaults[2].clone(),
            before: Some(defaults[1].clone()),
        })
        .await
        .unwrap();
        assert_eq!(m.workspace_ids()[0], defaults[2]);
        assert_eq!(m.switcher_ids(), pins);
        assert_eq!(m.load(&initial.entity.id).await.unwrap(), initial);

        let reopened =
            WorkspaceManager::new(StoreWorker::shared(&f.directory).unwrap(), Platform::Gtk);
        reopened.refresh().await.unwrap();
        reopened.refresh_switcher().await.unwrap();
        assert_eq!(reopened.switcher_ids(), m.switcher_ids());
        assert_eq!(reopened.workspace_ids(), m.workspace_ids());
        for id in reopened.switcher_ids() {
            reopened
                .edit_switcher(SwitcherEdit::Show { id, visible: false })
                .await
                .unwrap();
        }
        m.refresh_switcher().await.unwrap();
        assert!(m.switcher_ids().is_empty());
        assert!(
            matches!(m.store.execute(StoreRequest::Switcher).await.unwrap(), StoreResponse::Switcher(Some(ids)) if ids.is_empty())
        );
        // Failed publication keeps the acknowledged configuration available.
        assert!(
            m.store
                .execute(StoreRequest::UpdateSwitcher {
                    expected: None,
                    ids: defaults.clone()
                })
                .await
                .is_err()
        );
        assert!(
            m.store
                .execute(StoreRequest::UpdateSwitcher {
                    expected: Some(vec![]),
                    ids: vec![defaults[0].clone(), defaults[0].clone()]
                })
                .await
                .is_err()
        );
        assert!(
            m.store
                .execute(StoreRequest::UpdateSwitcher {
                    expected: Some(vec![]),
                    ids: vec!["missing".into()]
                })
                .await
                .is_err()
        );
        assert!(
            matches!(m.store.execute(StoreRequest::Switcher).await.unwrap(), StoreResponse::Switcher(Some(ids)) if ids.is_empty())
        );
        m.close().await.unwrap();
    });
}
