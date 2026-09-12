//! Serialized native transport for the shared workspace manager. Swift calls
//! this owner only from a separate workspace queue; SQLite has its own shared
//! worker. Prepared adoptions are acknowledged after the render owner accepts
//! them, so a late reply cannot replace another window's current workspace.
use super::*;
use layer_ui::{Platform, WorkspaceState, WorkspaceWorkingState};
use layer_workspace::*;
use std::path::Path;

type Result<T> = std::result::Result<T, StoreError>;
pub struct CapyWorkspaceLibrary {
    manager: WorkspaceManager<StoreWorker>,
    pending: Option<(String, StoredEntity)>,
    resume_key: String,
    resume_error: Option<StoreError>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Catalog,
    Initialize {
        now: u64,
        preferred: Option<String>,
    },
    Migrate {
        scenes: Vec<(String, WorkspaceState)>,
        fallback: Option<(String, WorkspaceState)>,
        now: u64,
    },
    LegacyMapping {
        source: String,
    },
    View {
        page: ManagerPage,
        query: String,
        selected: Option<String>,
        idle: bool,
        now: u64,
    },
    Load {
        id: String,
    },
    Toolbar {
        id: Option<String>,
        name: Option<String>,
    },
    Prompt {
        action: Value,
        now: u64,
    },
    History {
        id: String,
        mode: ManagerHistoryMode,
        selected: Option<String>,
        idle: Option<bool>,
        now: u64,
    },
    Observe {
        #[serde(deserialize_with = "json_field")]
        capture: Option<WorkspaceCapture>,
        #[serde(deserialize_with = "json_field")]
        working: WorkspaceWorkingState,
        now: u64,
    },
    Flush,
    Renew,
    Revalidate {
        now: u64,
    },
    Retry,
    Close,
    Detach,
    Activate {
        token: String,
    },
    Reject {
        token: String,
    },
    Operation {
        operation: Operation,
        now: u64,
    },
    Storage {
        clear_older: bool,
        apply: bool,
    },
    Interrupted {
        now: u64,
    },
    Recover {
        id: String,
        now: u64,
    },
    Export {
        id: Option<String>,
        #[serde(default, deserialize_with = "json_field")]
        capture: Option<WorkspaceCapture>,
    },
    Import {
        text: String,
        kind: PackageKind,
        now: u64,
    },
    Backup {
        path: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Operation {
    Switch {
        id: String,
    },
    New {
        name: String,
    },
    Duplicate {
        id: String,
        name: String,
    },
    Rename {
        id: String,
        name: String,
        description: String,
    },
    SaveToolbar {
        panel: Panel,
        name: String,
    },
    UpdateToolbar {
        id: String,
        panel: Panel,
    },
    Reset {
        id: String,
        revision: Option<String>,
    },
    OpenHistory {
        id: String,
        revision: String,
        name: String,
    },
    RestoreVersion {
        id: String,
        version: String,
    },
    RestoreMetadata {
        id: String,
        version: String,
    },
    Delete {
        id: String,
        replacement: Option<String>,
    },
    RestoreDeleted {
        id: String,
    },
    DeletePermanently {
        id: String,
    },
    SaveAsNew {
        #[serde(deserialize_with = "json_field")]
        capture: WorkspaceCapture,
        name: String,
    },
    DuplicateSnapshot {
        #[serde(deserialize_with = "json_field")]
        source: Entity,
        name: String,
    },
}

impl CapyWorkspaceLibrary {
    fn status(&self) -> Value {
        let items = self.manager.items();
        let defaults: Vec<_> = DEFAULT_WORKSPACES
            .iter()
            .map(|(id, preset)| {
                let name = items
                    .iter()
                    .find(|item| item.id == *id)
                    .map(|item| item.metadata.name.as_str())
                    .unwrap_or(preset.name());
                json!({"id":id,"name":name})
            })
            .collect();
        json!({"active_id":self.manager.active_id(),"name":self.manager.active_name(),
            "default_workspaces":defaults,
            "dirty":self.manager.dirty(),"saving":self.manager.saving(),"error":self.manager.error().or_else(||self.resume_error.clone()),
            "owner":self.manager.owner.id,"pending_adoption":self.pending.as_ref().map(|p|&p.0),
            "lease_expires_at_ms":self.manager.lease_expires_at_ms(),"renew_after_ms":OWNER_RENEW_MS})
    }
    async fn save_resume_key(&mut self) -> Result<()> {
        let result = self.manager.bind_resume_key(&self.resume_key).await;
        self.resume_error = result.as_ref().err().cloned();
        result
    }
    fn prepare(&mut self, incoming: StoredEntity) -> Result<Value> {
        let capture = incoming.entity.capture()?;
        // Validate fully before the native editor is asked to adopt it.
        PreparedWorkspace::new(capture.clone()).map_err(StoreError::invalid)?;
        let token = new_id();
        let result = json!({"adoption":{"token":token,"capture":capture,"id":incoming.entity.id}});
        self.pending = Some((token, incoming));
        Ok(result)
    }
    async fn request(&mut self, request: Request) -> Result<Value> {
        if self.pending.is_some()
            && !matches!(
                request,
                Request::Activate { .. }
                    | Request::Reject { .. }
                    | Request::View { .. }
                    | Request::Load { .. }
                    | Request::Detach
            )
        {
            return Err(StoreError::invalid(
                "A workspace change is waiting for the editor.",
            ));
        }
        match request {
            Request::Catalog => Ok(
                json!({"title":"Manage Workspaces","toolbar_title":"Manage Toolbars",
                    "switch_label":ManagerAction::Switch(String::new()).label(),
                    "description":"Workspaces save your tool settings and layout for different tasks.",
                    "pages":([ManagerPage::Workspaces,ManagerPage::ThisWorkspace,ManagerPage::ToolbarLibrary].map(|page|json!({"id":page,"label":page.label()})))
                }),
            ),
            Request::Initialize { now, preferred } => {
                let _ = self.manager.store.request(StoreRequest::Reopen).await?;
                let saved = match self
                    .manager
                    .store
                    .execute(StoreRequest::Binding {
                        key: self.resume_key.clone(),
                    })
                    .await?
                {
                    StoreResponse::Binding(id) => id.or(preferred),
                    _ => preferred,
                };
                // Resolve this scene before the global last-used fallback.
                // Otherwise reopening beside a live window creates an unused
                // workspace before switching back to the saved scene identity.
                let incoming = if let Some(id) = saved {
                    let candidate = match self.manager.load(&id).await {
                        Ok(value) if value.entity.metadata.deleted_at_ms.is_none() => Some(value),
                        Ok(_) => None,
                        Err(error) if error.kind == ErrorKind::NotFound => None,
                        Err(error) => return Err(error),
                    };
                    if let Some(candidate) = candidate {
                        self.manager.initialize_catalog(now).await?;
                        match self.manager.prepare_switch(&id, now).await {
                            Ok(incoming) => incoming,
                            Err(error) if error.kind == ErrorKind::OwnedElsewhere => {
                                self.manager
                                    .create_from_snapshot(
                                        candidate.entity,
                                        "My Workspace",
                                        true,
                                        now,
                                    )
                                    .await?
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        self.manager.initialize(now).await?
                    }
                } else {
                    self.manager.initialize(now).await?
                };
                self.prepare(incoming)
            }
            Request::Migrate {
                scenes,
                fallback,
                now,
            } => Ok(
                json!({"mappings":self.manager.migrate_legacy(&scenes, fallback.as_ref(), now).await?}),
            ),
            Request::LegacyMapping { source } => {
                let result = self
                    .manager
                    .store
                    .execute(StoreRequest::LegacyImport { source })
                    .await?;
                Ok(json!({"mapping":result}))
            }
            Request::View {
                page,
                query,
                selected,
                idle,
                now,
            } => {
                self.manager.refresh().await?;
                let rows = self.manager.rows(page, &query, now);
                let mut detail_error = None;
                let details =
                    if let Some(id) = selected.filter(|id| rows.iter().any(|row| row.id == *id)) {
                        if page == ManagerPage::ThisWorkspace {
                            Some(
                                self.manager
                                    .toolbar_details(serde_json::from_str(&id)?, idle)?,
                            )
                        } else {
                            let entity = if self.manager.active_id().as_deref() == Some(&id) {
                                self.manager
                                    .current_record()
                                    .ok_or_else(|| StoreError::invalid("No workspace is active."))
                            } else {
                                self.manager.load(&id).await
                            };
                            match entity {
                                Ok(entity) => {
                                    Some(self.manager.inspect_details(&entity, idle, now).await)
                                }
                                Err(error) => {
                                    detail_error = Some(error.to_string());
                                    None
                                }
                            }
                        }
                    } else {
                        None
                    };
                let preview = details.as_ref().and_then(|d| d.preview.as_ref()).map(|layout| {
                    let resolved = layout.workspace(1000., 700., layer_ui::HEADER_HEIGHT, layer_ui::STATUS_HEIGHT);
                    json!({"canvas":resolved.work_area,"groups":resolved.groups.iter().map(|g|g.bounds).collect::<Vec<_>>()})
                });
                let summaries = self.manager.items();
                let rows = rows
                    .into_iter()
                    .map(|row| {
                        let actions = if page == ManagerPage::ThisWorkspace {
                            serde_json::from_str(&row.id)
                                .ok()
                                .and_then(|panel| self.manager.toolbar_details(panel, idle).ok())
                                .map(|d| d.actions)
                                .unwrap_or_default()
                        } else {
                            summaries
                                .iter()
                                .find(|item| item.id == row.id)
                                .map(|item| self.manager.summary_actions(item, idle, now))
                                .unwrap_or_default()
                        };
                        let mut value = serde_json::to_value(row).unwrap();
                        value["actions"] = json!(actions);
                        value
                    })
                    .collect::<Vec<_>>();
                Ok(
                    json!({"rows":rows,"details":details,"detail_error":detail_error,"preview":preview,
                    "binding":self.manager.binding(),"idle":idle}),
                )
            }
            Request::Load { id } => {
                let stored = self.manager.load(&id).await?;
                Ok(json!({"entity":stored.entity,"claim":stored.claim}))
            }
            Request::Toolbar { id, name } => {
                let mut definition = if let Some(id) = id {
                    let entity = self.manager.load(&id).await?.entity;
                    if entity.metadata.deleted_at_ms.is_some() {
                        return Err(StoreError::invalid(
                            "Restore this toolbar from Recently Deleted first.",
                        ));
                    }
                    let ItemContent::Reusable { current, .. } = entity.content else {
                        return Err(StoreError::invalid("Choose a saved toolbar."));
                    };
                    let ReusableContent::Toolbar { definition } = current.content else {
                        return Err(StoreError::invalid("Choose a saved toolbar."));
                    };
                    definition
                } else {
                    ToolbarDefinition {
                        name: "New Toolbar".into(),
                        tiles: Vec::new(),
                        tile_style: layer_ui::TileStyle::Small,
                        hide_tab: false,
                    }
                };
                if let Some(name) = name {
                    definition.name = name.trim().into();
                }
                definition.validate()?;
                let config = layer_ui::PanelConfig {
                    id: Panel::Toolbar,
                    hide_tab: definition.hide_tab,
                    tile_style: definition.tile_style,
                    content: layer_ui::PanelContent::Toolbar {
                        name: definition.name,
                        tiles: definition.tiles,
                    },
                };
                Ok(json!({"config":config}))
            }
            Request::Prompt { action, now } => {
                self.manager.refresh().await?;
                let prompt = if action["type"] == "new_toolbar" {
                    self.manager.new_toolbar_prompt()
                } else {
                    self.manager
                        .prompt(&serde_json::from_value(action)?, now)
                        .await?
                };
                Ok(json!({"prompt":prompt}))
            }
            Request::History {
                id,
                mode,
                selected,
                idle,
                now,
            } => {
                let history = self
                    .manager
                    .history_view(&id, mode, selected.as_deref(), idle.unwrap_or(false), now)
                    .await?;
                let preview=history.preview.as_ref().map(|layout| {
                    let resolved=layout.workspace(1000.,700.,layer_ui::HEADER_HEIGHT,layer_ui::STATUS_HEIGHT);
                    json!({"canvas":resolved.work_area,"groups":resolved.groups.iter().map(|g|g.bounds).collect::<Vec<_>>()})
                });
                Ok(json!({"history":history,"preview":preview}))
            }
            Request::Observe {
                capture,
                working,
                now,
            } => {
                if let Some(capture) = capture {
                    self.manager.observe(capture, now);
                }
                self.manager.observe_working(working);
                Ok(Value::Null)
            }
            Request::Flush => {
                self.manager.flush().await?;
                if self.resume_error.is_some() {
                    self.save_resume_key().await?;
                }
                Ok(Value::Null)
            }
            Request::Renew => {
                self.manager.renew().await?;
                Ok(Value::Null)
            }
            Request::Revalidate { now } => {
                self.manager.revalidate_owner(now).await?;
                Ok(Value::Null)
            }
            Request::Retry => {
                if let Some(incoming) = self.manager.retry_failed_operation().await? {
                    self.prepare(incoming)
                } else {
                    self.manager.flush().await?;
                    if self.resume_error.is_some() {
                        self.save_resume_key().await?;
                    }
                    Ok(Value::Null)
                }
            }
            Request::Close => {
                if self.resume_error.is_some() {
                    self.save_resume_key().await?;
                }
                self.manager.close().await?;
                Ok(Value::Null)
            }
            Request::Detach => {
                if let Some((_, incoming)) = self.pending.take() {
                    self.manager.release(&incoming).await;
                }
                self.manager.release_owner().await?;
                Ok(Value::Null)
            }
            Request::Activate { token } => {
                if self.pending.as_ref().map(|p| p.0.as_str()) != Some(&token) {
                    return Err(StoreError::invalid("This workspace reply has expired."));
                }
                let (_, incoming) = self.pending.take().unwrap();
                let old = self.manager.activate(incoming);
                // Keep the new in-memory owner even if writing the resume key
                // fails; the editor already adopted it and can retry storage.
                if let Some(old) =
                    old.filter(|old| Some(&old.entity.id) != self.manager.active_id().as_ref())
                {
                    self.manager.release(&old).await;
                }
                // Adoption already succeeded. Report a failed resume-key write
                // in status while still returning the new binding to the UI.
                let _ = self.save_resume_key().await;
                Ok(json!({"binding":self.manager.binding()}))
            }
            Request::Reject { token } => {
                if self.pending.as_ref().map(|p| p.0.as_str()) != Some(&token) {
                    return Err(StoreError::invalid("This workspace reply has expired."));
                }
                let (_, incoming) = self.pending.take().unwrap();
                if self.manager.active_id().as_deref() != Some(&incoming.entity.id) {
                    self.manager.release(&incoming).await;
                }
                self.manager.finish_transition();
                Ok(Value::Null)
            }
            Request::Operation { operation, now } => self.operation(operation, now).await,
            Request::Storage { clear_older, apply } => {
                if apply {
                    if let Some(incoming) = self.manager.maintain_storage(clear_older).await? {
                        return self.prepare(incoming);
                    }
                }
                Ok(json!({"storage":self.manager.storage_report(clear_older).await?}))
            }
            Request::Interrupted { now } => {
                Ok(json!({"interrupted":self.manager.interrupted_changes(now).await?}))
            }
            Request::Recover { id, now } => {
                if let Some(incoming) = self.manager.recover_interrupted(&id, now).await? {
                    self.prepare(incoming)
                } else {
                    Ok(Value::Null)
                }
            }
            Request::Export { id, capture } => {
                let entity = if let Some(id) = id {
                    self.manager.load(&id).await?.entity
                } else if let Some(entity) = self.manager.current() {
                    entity
                } else if let Some(capture) = capture {
                    capture.validate().map_err(StoreError::invalid)?;
                    let baseline = capture.history.layout().clone();
                    Entity::workspace("Recovered Workspace", capture, baseline, None, 0)
                } else {
                    return Err(StoreError::invalid("No workspace is active."));
                };
                Ok(
                    json!({"name":entity.metadata.name,"extension":PackageKind::for_entity(&entity).extension(),
                    "text":String::from_utf8(export_package(&entity)?).map_err(|e|StoreError::invalid(e.to_string()))?}),
                )
            }
            Request::Import { text, kind, now } => {
                if kind == PackageKind::WorkspaceBackup {
                    let incoming = self
                        .manager
                        .import_workspace_package(text.as_bytes(), now)
                        .await?;
                    self.prepare(incoming)
                } else {
                    Ok(
                        json!({"selected":self.manager.import_reusable_package(text.as_bytes(), kind, now).await?}),
                    )
                }
            }
            Request::Backup { path } => {
                self.manager.store.backup_database(Path::new(&path)).await?;
                Ok(Value::Null)
            }
        }
    }
    // One future per operation keeps the debug poll frames bounded on Apple's
    // small Dispatch stacks. One giant async match allocates every branch's
    // temporaries in its poll frame even when the outer future is boxed.
    fn operation(
        &mut self,
        operation: Operation,
        now: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + '_>> {
        macro_rules! run { ($body:block) => { Box::pin(async move $body) }; }
        match operation {
            Operation::DuplicateSnapshot { source, name } => run!({
                source.validate()?;
                let incoming = self
                    .manager
                    .create_from_snapshot(source, &name, true, now)
                    .await?;
                self.prepare(incoming)
            }),
            Operation::Switch { id } => run!({
                let incoming = self.manager.prepare_switch(&id, now).await?;
                self.prepare(incoming)
            }),
            Operation::New { name } => run!({
                let incoming = self
                    .manager
                    .create_workspace(&name, None, false, now)
                    .await?;
                self.prepare(incoming)
            }),
            Operation::Duplicate { id, name } => run!({
                if self.manager.load(&id).await?.entity.metadata.kind == ItemKind::Workspace {
                    let incoming = self
                        .manager
                        .create_from_workspace(&id, &name, true, now)
                        .await?;
                    self.prepare(incoming)
                } else {
                    Ok(json!({"selected":self.manager.duplicate_reusable(&id, &name, now).await?}))
                }
            }),
            Operation::Rename {
                id,
                name,
                description,
            } => run!({
                self.manager.rename(&id, &name, &description, now).await?;
                Ok(json!({"binding":self.manager.binding()}))
            }),
            Operation::SaveToolbar { panel, name } => run!({
                Ok(json!({"selected":self.manager.save_toolbar(panel, &name, now).await?}))
            }),
            Operation::UpdateToolbar { id, panel } => run!({
                let current = self
                    .manager
                    .current()
                    .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
                let capture = current.capture()?;
                let definition = ToolbarDefinition::capture(
                    capture
                        .history
                        .layout()
                        .panel(panel)
                        .map_err(StoreError::invalid)?,
                )?;
                self.manager
                    .update_reusable(&id, ReusableContent::Toolbar { definition }, now)
                    .await?;
                Ok(Value::Null)
            }),
            Operation::Reset { id, revision } => run!({
                let updated = self
                    .manager
                    .change_layout(&id, revision.as_deref(), now)
                    .await?;
                if self.manager.active_id().as_deref() == Some(&id) {
                    self.prepare(updated)
                } else {
                    Ok(Value::Null)
                }
            }),
            Operation::OpenHistory { id, revision, name } => run!({
                let incoming = self
                    .manager
                    .open_history_as_workspace(&id, &revision, &name, now)
                    .await?;
                self.prepare(incoming)
            }),
            Operation::RestoreVersion { id, version } => run!({
                self.manager
                    .restore_reusable_version(&id, &version, now)
                    .await?;
                Ok(Value::Null)
            }),
            Operation::RestoreMetadata { id, version } => run!({
                let entity = self.manager.load(&id).await?.entity;
                let value = entity
                    .metadata
                    .previous
                    .iter()
                    .find(|v| v.id == version)
                    .ok_or_else(|| {
                        StoreError::invalid("This name and description are no longer retained.")
                    })?;
                self.manager
                    .rename(&id, &value.name, &value.description, now)
                    .await?;
                Ok(json!({"binding":self.manager.binding()}))
            }),
            Operation::Delete { id, replacement } => run!({
                match self
                    .manager
                    .delete_item(&id, replacement.as_deref(), now)
                    .await?
                {
                    Some(incoming) => self.prepare(incoming),
                    None => Ok(Value::Null),
                }
            }),
            Operation::RestoreDeleted { id } => run!({
                self.manager.restore_deleted(&id, now).await?;
                Ok(Value::Null)
            }),
            Operation::DeletePermanently { id } => run!({
                self.manager.delete_permanently(&id).await?;
                Ok(Value::Null)
            }),
            Operation::SaveAsNew { capture, name } => run!({
                let incoming = self.manager.save_as_new(capture, &name, now).await?;
                self.prepare(incoming)
            }),
        }
    }
}

/// # Safety
/// UTF-8 directory/resume-key strings live for this call. The returned library
/// must have one serial owner, separate from the drawing owner, and be freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_library_create(
    platform: u32,
    directory: *const c_char,
    resume_key: *const c_char,
) -> *mut CapyWorkspaceLibrary {
    catch_unwind(|| {
        if directory.is_null() || resume_key.is_null() {
            return None;
        }
        let platform = match platform {
            0 => Platform::Ios,
            1 => Platform::Mac,
            _ => return None,
        };
        let directory = unsafe { CStr::from_ptr(directory) }.to_str().ok()?;
        let resume_key = unsafe { CStr::from_ptr(resume_key) }
            .to_str()
            .ok()?
            .to_owned();
        let worker = StoreWorker::shared(Path::new(directory)).ok()?;
        Some(Box::into_raw(Box::new(CapyWorkspaceLibrary {
            manager: WorkspaceManager::new(worker, platform),
            pending: None,
            resume_key,
            resume_error: None,
        })))
    })
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Exclusively owned library and NUL-terminated UTF-8 request, valid for this
/// call. May wait for storage. Free the owned reply with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_library_request(
    library: *mut CapyWorkspaceLibrary,
    request: *const c_char,
) -> *mut c_char {
    let reply = catch_unwind(AssertUnwindSafe(|| {
        let Some(library) = (unsafe { library.as_mut() }) else { return json!({"error":{"kind":"unavailable","message":"Workspace storage is unavailable."}}); };
        let result = (|| {
            if request.is_null() { return Err(StoreError::invalid("Missing workspace request.")); }
            let source = unsafe { CStr::from_ptr(request) }.to_bytes();
            if source.len() > MAX_PACKAGE_BYTES { return Err(StoreError::invalid("Workspace request exceeds the supported size.")); }
            let request = serde_json::from_slice(source)?;
            // Dispatch workers have smaller stacks than Rust test threads.
            // Keep the manager's asynchronous state on the heap at the ABI.
            pollster::block_on(Box::pin(library.request(request)))
        })();
        match result {
            Ok(value) => json!({"value":value,"status":library.status()}),
            Err(error) => json!({"error":error,"status":library.status()}),
        }
    })).unwrap_or_else(|_|json!({"error":{"kind":"unavailable","message":"Workspace operation failed."}}));
    CString::new(reply.to_string())
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Exclusively owned library with no outstanding calls. Flush/close explicitly
/// first: destruction alone cannot confirm that dirty changes reached storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_library_destroy(library: *mut CapyWorkspaceLibrary) {
    if !library.is_null() {
        unsafe {
            drop(Box::from_raw(library));
        }
    }
}
