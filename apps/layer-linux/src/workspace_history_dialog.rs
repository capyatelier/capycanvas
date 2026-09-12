//! A temporary layout preview, committed only by Restore This Version.
use super::*;

/// Owns the transient layout and its lease until a dialog closes or applies it.
/// Weak ownership lets a window disappear without keeping its widgets alive.
pub(super) struct Preview {
    workspace: std::rc::Weak<Workspace>,
    renewal: Option<glib::SourceId>,
    operation: Option<actions::OperationGuard>,
}
impl Preview {
    pub async fn begin(w: &Rc<Workspace>) -> Result<Self, StoreError> {
        let operation = w.workspaces.begin_operation(w).await?;
        w.workspaces.manager.as_ref().unwrap().flush().await?;
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .begin_workspace_layout_preview()
            .map_err(StoreError::invalid)?;
        let renewal = glib::timeout_add_local(
            Duration::from_millis(layer_workspace::OWNER_RENEW_MS),
            glib::clone!(
                #[weak]
                w,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        w,
                        async move {
                            if let Some(manager) = &w.workspaces.manager
                                && let Err(error) = manager.renew().await
                            {
                                w.workspaces.show_error(error);
                                w.workspaces.update_status();
                            }
                        }
                    ));
                    glib::ControlFlow::Continue
                }
            ),
        );
        Ok(Self {
            workspace: Rc::downgrade(w),
            renewal: Some(renewal),
            operation: Some(operation),
        })
    }
    fn restore(&self) {
        if let Some(w) = self.workspace.upgrade() {
            let change = w
                .gpu
                .borrow_mut()
                .as_mut()
                .map(|g| g.session.cancel_workspace_layout_preview());
            if let Some(change) = change {
                w.changed(Ok(change));
            }
        }
    }
    pub fn reset(&self) {
        self.restore();
        if let Some(w) = self.workspace.upgrade() {
            let result = w
                .gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .begin_workspace_layout_preview();
            if let Err(error) = result {
                w.workspaces.ui.error(&error);
            }
        }
    }
    /// Restore presentation while retaining the operation lock for a commit.
    fn finish(mut self) -> actions::OperationGuard {
        self.restore();
        self.operation.take().unwrap()
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(source) = self.renewal.take() {
            source.remove();
        }
        self.restore();
    }
}
pub(super) async fn show(w: &Rc<Workspace>, id: &str) -> Result<(), StoreError> {
    let manager = w.workspaces.manager.as_ref().unwrap();
    if manager.active_id().as_deref() != Some(id) {
        return Err(StoreError::invalid(
            "Switch to this workspace to view its layout history.",
        ));
    }
    let preview = Preview::begin(w).await?;
    let capture = manager
        .current()
        .ok_or_else(|| StoreError::invalid("Open a workspace first."))?
        .capture()?;
    let versions = layer_workspace::layout_history_versions(&capture.history);
    let current = capture.history.current;
    let selected = Rc::new(RefCell::new(current.clone()));
    let versions = Rc::new(versions);
    let dialog = adw::AlertDialog::builder()
        .heading(format!(
            "Layout History — {}",
            manager.active_name().unwrap_or_default()
        ))
        .build();
    dialog.set_widget_name("workspace-layout-history");
    dialog.add_responses(&[("cancel", "Cancel"), ("restore", "Restore This Version")]);
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("restore", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("restore", false);
    let list = gtk::ListBox::new();
    list.set_widget_name("workspace-history-items");
    list.add_css_class("boxed-list");
    for version in versions.iter() {
        let time = glib::DateTime::from_unix_local((version.timestamp_ms / 1000) as i64)
            .and_then(|date| date.format("%b %e, %Y · %H:%M:%S"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let subtitle = if version.id == current {
            format!("Current layout · {time}")
        } else {
            time
        };
        let row = adw::ActionRow::builder()
            .title(&version.description)
            .subtitle(subtitle)
            .build();
        row.set_use_markup(false);
        row.set_title_lines(2);
        list.append(&row);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let error = gtk::Label::new(None);
    error.set_wrap(true);
    error.add_css_class("error");
    error.set_visible(false);
    body.append(
        &gtk::ScrolledWindow::builder()
            .width_request(340)
            .height_request(360)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build(),
    );
    body.append(&error);
    dialog.set_extra_child(Some(&body));
    list.connect_row_selected(glib::clone!(
        #[weak]
        w,
        #[weak]
        dialog,
        #[weak]
        error,
        #[strong]
        versions,
        #[strong]
        selected,
        #[strong]
        current,
        move |_, row| {
            let Some(version) = row.and_then(|r| versions.get(r.index() as usize)) else {
                return;
            };
            let result = w
                .gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .preview_workspace_layout(&version.layout);
            match result {
                Ok(change) => {
                    w.changed(Ok(change));
                    *selected.borrow_mut() = version.id.clone();
                    error.set_visible(false);
                    dialog.set_response_enabled("restore", version.id != current);
                }
                Err(message) => {
                    error.set_text(&message);
                    error.set_visible(true);
                    dialog.set_response_enabled("restore", false);
                }
            }
        }
    ));
    list.select_row(
        list.row_at_index(versions.iter().position(|v| v.id == current).unwrap_or(0) as i32)
            .as_ref(),
    );
    w.workspaces.ui.close();
    let response = dialog.choose_future(Some(&w.window)).await;
    let _operation = preview.finish();
    if response == "restore" {
        let selected = selected.borrow().clone();
        match manager.change_layout(id, Some(&selected), now_ms()).await {
            Ok(incoming) => w.workspaces.adopt(w, Ok(incoming)).await,
            Err(error) => {
                w.workspaces.show_error(error.clone());
                w.workspaces.update_status();
                return Err(error);
            }
        }
    }
    Ok(())
}
