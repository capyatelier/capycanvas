use super::*;
use layer_workspace::{ManagerAction as A, StoreRequest};
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
                    .body("Your latest layout changes couldn’t be saved. Keep this window open and try saving again.").build();
                prompt.set_widget_name("workspace-close-recovery");
                prompt.add_responses(&[
                    ("cancel", "Keep Open"),
                    ("discard", "Discard Unsaved Changes"),
                ]);
                prompt.set_close_response("cancel");
                prompt.set_default_response(Some("cancel"));
                prompt.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                let response = prompt.choose_future(Some(&w.window)).await;
                w.workspaces.close_prompt.set(false);
                if response != "discard" {
                    if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                        gpu.session.reset_document_close();
                    }
                    return;
                }
                let _operation = match w.workspaces.begin_operation(&w).await {
                    Ok(guard) => guard,
                    Err(error) => {
                        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                            gpu.session.reset_document_close();
                        }
                        w.workspaces.show_error(error);
                        w.workspaces.update_status();
                        return;
                    }
                };
                if let Some(manager) = &w.workspaces.manager
                    && let Some(current) = manager.current_record()
                {
                    manager.release(&current).await;
                }
                w.workspaces.close_ready.set(true);
                w.window.close();
            }
        ));
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
                    let capture = w
                        .gpu
                        .borrow_mut()
                        .as_mut()
                        .ok_or_else(|| StoreError::invalid("Canvas unavailable."))?
                        .session
                        .capture_workspace()
                        .map_err(StoreError::invalid)?;
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
