use super::*;
use gtk::gio;
use layer_workspace::{Entity, ManagerAction as A, PackageKind, StoreRequest};
type Result<T> = std::result::Result<T, StoreError>;

impl NativeWorkspaces {
    pub(super) fn recover_close(&self, w: &Rc<Workspace>) {
        if self.close_prompt.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let prompt = adw::AlertDialog::builder().heading("Workspace Changes Aren’t Saved")
                .body("Your latest workspace changes couldn’t be saved. Save a backup before closing, or keep this window open and try again.").build();
                prompt.set_widget_name("workspace-close-recovery");
                prompt.add_responses(&[
                    ("cancel", "Keep Open"),
                    ("discard", "Discard Unsaved Changes"),
                    ("export", "Save Backup and Close…"),
                ]);
                prompt.set_close_response("cancel");
                prompt.set_default_response(Some("export"));
                prompt.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                prompt.set_response_appearance("export", adw::ResponseAppearance::Suggested);
                let response = prompt.choose_future(Some(&w.window)).await;
                let _operation = if response == "discard" || response == "export" {
                    match w.workspaces.begin_operation(&w).await {
                        Ok(guard) => Some(guard),
                        Err(error) => {
                            w.workspaces.close_prompt.set(false);
                            if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                                gpu.session.reset_document_close();
                            }
                            w.workspaces.show_error(error);
                            w.workspaces.update_status();
                            return;
                        }
                    }
                } else {
                    None
                };
                let close = if response == "discard" {
                    Ok(true)
                } else if response == "export" {
                    match w.workspaces.recovery_entity(&w) {
                        Ok(entity) => export(&w, entity).await,
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(false)
                };
                w.workspaces.close_prompt.set(false);
                match close {
                    Ok(true) => {
                        if let Some(manager) = &w.workspaces.manager
                            && let Some(current) = manager.current_record()
                        {
                            manager.release(&current).await;
                        }
                        w.workspaces.close_ready.set(true);
                        w.window.close();
                    }
                    Ok(false) => {
                        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                            gpu.session.reset_document_close();
                        }
                    }
                    Err(error) => {
                        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                            gpu.session.reset_document_close();
                        }
                        w.workspaces.show_error(error);
                        w.workspaces.update_status();
                    }
                }
            }
        ));
    }
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
            A::RecoverInterrupted => {
                let choices = manager.interrupted_changes(now_ms()).await?;
                if choices.is_empty() {
                    self.ui
                        .note
                        .set_text("There are no interrupted changes to recover.");
                    self.ui.note.set_visible(true);
                    return Ok(());
                }
                let Some(operation)=dialog::choice_dialog(w,"Recover Interrupted Changes","Some changes couldn’t finish saving. Choose an item to recover as a new workspace or saved setup.","Recover",&choices).await else {return Ok(());};
                let _operation = self.begin_operation(w).await?;
                match manager.recover_interrupted(&operation, now_ms()).await? {
                    Some(incoming) => self.adopt(w, Ok(incoming)).await,
                    None => (),
                }
                self.refresh_interrupted().await;
            }
            A::ExportDatabase => {
                let picker = gtk::FileDialog::builder()
                    .title("Export All Stored Data")
                    .initial_name("capycanvas-workspaces.sqlite3")
                    .modal(true)
                    .build();
                match picker.save_future(Some(&w.window)).await {
                    Ok(file) => {
                        let path = file
                            .path()
                            .ok_or_else(|| StoreError::invalid("Choose a file on this device."))?;
                        manager.store.backup_database(&path).await?;
                    }
                    Err(e)
                        if e.matches(gtk::DialogError::Dismissed)
                            || e.matches(gtk::DialogError::Cancelled) =>
                    {
                        ()
                    }
                    Err(e) => return Err(StoreError::invalid(e.to_string())),
                }
            }
            A::Storage => {
                let description = match manager.storage_report(false).await {
                    Ok(report) => format!(
                        "Save a backup so you can restore this workspace later or move it to another computer. Backups include your layout, saved settings, and history.\n\nStorage used: {:.1} MiB",
                        report.database_bytes as f64 / 1048576.,
                    ),
                    Err(error) => format!(
                        "Your workspace couldn’t be saved: {error}\n\nSave a backup to keep your changes. Open More storage options to try saving again."
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
                    self.ui.note.set_text(if kind == PackageKind::Template {"Workspace Template imported. Choose Use Layout to start a workspace with it."} else {"Toolbar imported. Select it and choose Add to Workspace to use it."});
                    self.ui.note.set_visible(true);
                    let _ = id;
                }
            }
            A::ClearOlderHistory => {
                let report = manager.storage_report(true).await?;
                let message = format!(
                    "Delete {} older versions and {} expired deleted items to free storage? You’ll keep your current layouts and recent undo steps. This cannot be undone.",
                    report.versions_to_remove, report.expired_items
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
                if dialog::confirm(
                    w,
                    "Delete Permanently",
                    &format!(
                        "Permanently delete “{}” and its history? This cannot be undone.",
                        entity.metadata.name
                    ),
                    "Delete Permanently",
                    true,
                )
                .await
                {
                    manager.delete_permanently(&id).await?;
                }
            }
            A::SaveAsNew => {
                let Some(values) = dialog::name_dialog(
                    w,
                    "Save as New Workspace",
                    "Keep the changes you can see in a new workspace.",
                    "Save and Switch",
                    "Recovered Workspace",
                    None,
                    &[],
                    None,
                    None,
                )
                .await
                else {
                    return Ok(());
                };
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
                if self.ready.get() && manager.has_failed_operation() {
                    let _operation = self.begin_operation(w).await?;
                    match manager.retry_failed_operation().await {
                        Ok(Some(incoming)) => self.adopt(w, Ok(incoming)).await,
                        Ok(None) => (),
                        Err(error) => return Err(error),
                    }
                } else if self.ready.get() && self.owner_lost.get() {
                    self.revalidate(w);
                } else if self.ready.get() {
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

async fn export(w: &Rc<Workspace>, entity: Entity) -> Result<bool> {
    let kind = PackageKind::for_entity(&entity);
    let name = format!(
        "{}.{}",
        entity.metadata.name.replace(['/', '\\'], "_"),
        kind.extension()
    );
    let Some(path) = choose_package(w, kind, Some(&name)).await? else {
        return Ok(false);
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
    Ok(true)
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
