use super::*;
use gtk::gio;
use layer_workspace::{Entity, ManagerAction as A, PackageKind, StoreRequest};
type Result<T> = std::result::Result<T, StoreError>;

impl NativeWorkspaces {
    fn recovery_entity(&self, w: &Rc<Workspace>) -> Result<Entity> {
        let capture = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| StoreError::invalid("Canvas unavailable."))?
            .session
            .capture_workspace()
            .map_err(StoreError::invalid)?;
        if let Some(mut entity) = self.manager.as_ref().and_then(|m| m.current()) {
            if let layer_workspace::ItemContent::Workspace { history, .. } = &mut entity.content {
                *history = capture.history;
            }
            entity.working = Some(capture.working);
            Ok(entity)
        } else {
            let baseline = capture.history.layout().clone();
            Ok(Entity::workspace(
                "Recovered Workspace",
                capture,
                baseline,
                None,
                now_ms(),
            ))
        }
    }
    pub(super) async fn storage_action(&self, w: &Rc<Workspace>, action: A) -> Result<()> {
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| StoreError::invalid("Workspace storage is unavailable."))?;
        match action {
            A::Storage => {
                let description = match manager.storage_report(false).await {
                    Ok(report) => format!(
                        "Database: {:.1} MiB\nShared configuration data: {:.1} MiB\nEligible history (estimated): {:.1} MiB\nHistory target: 100 MiB\nRecently Deleted: retained for 30 days\n\nCurrent state, original reset layouts, and up to 100 undo and 100 redo entries per workspace are protected. Other open workspaces are cleaned by their own windows.\n\nWorkspace backups include latest tool values, original layouts, and retained history. Template exports contain layout and toolbar configuration only.",
                        report.database_bytes as f64 / 1048576.,
                        report.component_bytes as f64 / 1048576.,
                        report.eligible_history_bytes as f64 / 1048576.
                    ),
                    Err(error) => format!(
                        "Storage is unavailable: {error}\n\nThe current workspace remains in memory. Export a Workspace Backup to preserve its layout, tool values, reset baseline, and retained history, then Retry Storage."
                    ),
                };
                self.ui.storage(w, description);
            }
            A::Export(id) => {
                let entity = if manager.active_id().as_deref() == Some(&id) {
                    self.recovery_entity(w)?
                } else {
                    self.selected(&id).await?.entity
                };
                export(w, entity).await?;
            }
            A::ExportCurrent => {
                export(w, self.recovery_entity(w)?).await?;
            }
            A::ImportTemplate | A::ImportToolbar | A::ImportBackup => {
                let kind = match action {
                    A::ImportTemplate => PackageKind::Template,
                    A::ImportToolbar => PackageKind::Toolbar,
                    _ => PackageKind::WorkspaceBackup,
                };
                let Some(path) = choose_package(w, kind, None).await? else {
                    return Ok(());
                };
                let bytes = gio::spawn_blocking(move || {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    let file = std::fs::File::open(path)
                        .map_err(|e| StoreError::invalid(e.to_string()))?;
                    file.take(layer_workspace::MAX_PACKAGE_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| StoreError::invalid(e.to_string()))?;
                    layer_workspace::import_package(&bytes, kind, now_ms())?;
                    Ok::<_, StoreError>(bytes)
                })
                .await
                .map_err(|_| StoreError::invalid("Package reader stopped."))??;
                if kind == PackageKind::WorkspaceBackup {
                    let _operation = self.begin_operation(w).await?;
                    let incoming = manager.import_workspace_package(&bytes, now_ms()).await;
                    match incoming {
                        Ok(incoming) => {
                            self.ui.close();
                            self.adopt(w, Ok(incoming)).await;
                        }
                        Err(e) => {
                            self.finish_operation(w);
                            return Err(e);
                        }
                    }
                } else {
                    let id = manager
                        .import_reusable_package(&bytes, kind, now_ms())
                        .await?;
                    self.ui.show(
                        w,
                        if kind == PackageKind::Template {
                            layer_workspace::ManagerPage::Templates
                        } else {
                            layer_workspace::ManagerPage::ToolbarLibrary
                        },
                    );
                    self.ui.note.set_text(if kind == PackageKind::Template {"Template imported. Select it and choose New Workspace from Template to use it."} else {"Toolbar imported. Select it and choose Add to Workspace to use it."});
                    self.ui.note.set_visible(true);
                    let _ = id;
                }
            }
            A::ClearOlderHistory => {
                let report = manager.storage_report(true).await?;
                let message = format!(
                    "Remove {} older layout, metadata, and library versions, and {} expired items? Current state, original layouts, and active undo/redo entries remain available. This cannot be undone. {} workspaces open in other windows are deferred.",
                    report.versions_to_remove, report.expired_items, report.deferred_open_items
                );
                if !dialog::confirm(
                    w,
                    "Clear Older History",
                    &message,
                    "Clear Older History",
                    true,
                )
                .await
                {
                    return Ok(());
                }
                let _operation = self.begin_operation(w).await?;
                let result = manager.maintain_storage(true).await;
                match result {
                    Ok(Some(incoming)) => self.adopt(w, Ok(incoming)).await,
                    Ok(None) => self.finish_operation(w),
                    Err(e) => {
                        self.finish_operation(w);
                        return Err(e);
                    }
                }
            }
            A::DeletePermanently(id) => {
                let entity = self.selected(&id).await?.entity;
                if dialog::confirm(w,"Delete Permanently",&format!("Permanently delete {} and its retained history? Content used by other workspaces remains available. This cannot be undone.",entity.metadata.name),"Delete Permanently",true).await {
                    manager.delete_permanently(&id).await?;
                }
            }
            A::SaveAsNew => {
                let Some(values) = dialog::name_dialog(w,"Save as New Workspace","Preserve the current in-memory workspace under an independent name. This can recover edits after another window takes ownership.","Save and Switch","Recovered Workspace",None,&[],None,None).await else {return Ok(());};
                let _operation = self.begin_operation(w).await?;
                let result = async {
                    let capture = self.recovery_entity(w)?.capture()?;
                    manager.save_as_new(capture, &values.name, now_ms()).await
                }
                .await;
                match result {
                    Ok(incoming) => {
                        self.ui.close();
                        self.adopt(w, Ok(incoming)).await;
                    }
                    Err(e) => {
                        self.finish_operation(w);
                        return Err(e);
                    }
                }
            }
            A::RetryStorage => {
                manager.store.request(StoreRequest::Reopen).await?;
                if self.ready.get() {
                    self.save(w, true);
                } else {
                    self.start(w);
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}

async fn export(w: &Rc<Workspace>, entity: Entity) -> Result<()> {
    let kind = PackageKind::for_entity(&entity);
    let name = format!(
        "{}.{}",
        entity.metadata.name.replace(['/', '\\'], "_"),
        kind.extension()
    );
    let Some(path) = choose_package(w, kind, Some(&name)).await? else {
        return Ok(());
    };
    gio::spawn_blocking(move || {
        let bytes = layer_workspace::export_package(&entity)?;
        crate::files::atomic_write(&path, |file| {
            file.write_all(&bytes).map_err(|e| e.to_string())
        })
        .map_err(StoreError::invalid)
    })
    .await
    .map_err(|_| StoreError::invalid("Package writer stopped."))??;
    Ok(())
}
async fn choose_package(
    w: &Rc<Workspace>,
    kind: PackageKind,
    save: Option<&str>,
) -> Result<Option<std::path::PathBuf>> {
    let dialog = gtk::FileDialog::builder()
        .title(format!(
            "{} {}",
            if save.is_some() { "Export" } else { "Import" },
            kind.label()
        ))
        .modal(true)
        .build();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(kind.label()));
    filter.add_suffix(kind.extension());
    filter.add_suffix("json");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let result = if let Some(name) = save {
        dialog.set_initial_name(Some(name));
        dialog.save_future(Some(&w.window)).await
    } else {
        dialog.open_future(Some(&w.window)).await
    };
    match result {
        Ok(file) => file
            .path()
            .map(Some)
            .ok_or_else(|| StoreError::invalid("Choose a file on this device.")),
        Err(e)
            if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) =>
        {
            Ok(None)
        }
        Err(e) => Err(StoreError::invalid(e.to_string())),
    }
}
