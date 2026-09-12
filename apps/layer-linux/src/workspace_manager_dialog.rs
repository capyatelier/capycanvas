//! Native projection of shared workspace lists and item actions.
use super::*;
use layer_workspace::{ManagerAction, ManagerButton, ManagerDetails, ManagerPage};

#[path = "workspace_switcher_dialog.rs"]
mod switcher_controls;

pub(crate) struct ManagerUi {
    pub dialog: adw::Dialog,
    presented: Cell<bool>,
    page: Cell<ManagerPage>,
    tabs: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    details: gtk::Box,
    pub note: gtk::Label,
    rows: RefCell<Vec<String>>,
    selected: RefCell<Option<String>>,
    generation: Cell<u64>,
    rebuilding: Cell<bool>,
    actions: gtk::Box,
    intro: gtk::Label,
    split: gtk::Paned,
    sidebar: gtk::Box,
    details_view: gtk::ScrolledWindow,
    apply: gtk::Button,
    footer: gtk::Box,
    create: gtk::Button,
    create_action: RefCell<Option<ManagerAction>>,
    primary: RefCell<Option<ManagerAction>>,
    preview: RefCell<Option<history::Preview>>,
    preview_epoch: Cell<u64>,
    action_pending: Cell<bool>,
    switcher_pending: Cell<bool>,
    preserve_preview: Cell<bool>,
    dragged: RefCell<Option<String>>,
}
impl ManagerUi {
    pub fn new() -> Self {
        let dialog = adw::Dialog::builder()
            .title("Manage Workspaces")
            .content_width(520)
            .content_height(540)
            .build();
        dialog.set_widget_name("workspace-manager");
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let create = gtk::Button::from_icon_name("list-add-symbolic");
        create.set_widget_name("workspace-manager-new");
        header.pack_end(&create);
        view.add_top_bar(&header);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        margins(&body, 18);
        let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        body.append(&tabs);
        let intro = gtk::Label::new(None);
        intro.set_xalign(0.);
        intro.set_wrap(true);
        body.append(&intro);
        let note = gtk::Label::new(None);
        note.set_xalign(0.);
        note.set_wrap(true);
        note.set_visible(false);
        body.append(&note);
        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        split.set_position(280);
        split.set_vexpand(true);
        split.set_wide_handle(true);
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 10);
        sidebar.set_size_request(220, -1);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search"));
        search.set_widget_name("workspace-manager-search");
        sidebar.append(&search);
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 6);
        sidebar.append(&actions);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        list.set_widget_name("workspace-manager-items");
        sidebar.append(
            &gtk::ScrolledWindow::builder()
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&list)
                .build(),
        );
        let details = gtk::Box::new(gtk::Orientation::Vertical, 12);
        details.set_widget_name("workspace-manager-details");
        details.set_margin_start(18);
        details.set_margin_end(8);
        split.set_start_child(Some(&sidebar));
        let details_view = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&details)
            .build();
        split.set_end_child(Some(&details_view));
        body.append(&split);
        let apply = gtk::Button::with_label("Switch to Workspace");
        apply.set_widget_name("workspace-manager-apply");
        apply.add_css_class("suggested-action");
        apply.set_sensitive(false);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.set_homogeneous(true);
        let cancel = gtk::Button::with_label("Cancel");
        cancel.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                dialog.close();
            }
        ));
        footer.append(&cancel);
        footer.append(&apply);
        body.append(&footer);
        view.set_content(Some(&body));
        dialog.set_child(Some(&view));
        Self {
            dialog,
            presented: Cell::new(false),
            page: Cell::new(ManagerPage::Workspaces),
            tabs,
            search,
            list,
            details,
            note,
            rows: RefCell::new(Vec::new()),
            selected: RefCell::new(None),
            generation: Cell::new(0),
            rebuilding: Cell::new(false),
            actions,
            intro,
            split,
            sidebar,
            details_view,
            apply,
            footer,
            create,
            create_action: RefCell::new(None),
            primary: RefCell::new(None),
            preview: RefCell::new(None),
            preview_epoch: Cell::new(0),
            action_pending: Cell::new(false),
            switcher_pending: Cell::new(false),
            preserve_preview: Cell::new(false),
            dragged: RefCell::new(None),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.bind_reorder_drop(w);
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.workspaces.ui.presented.set(false);
                w.workspaces.ui.stop_preview();
            }
        ));
        self.apply.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let action = w.workspaces.ui.primary.borrow().clone();
                if let Some(action) = action {
                    w.workspaces.ui.run(&w, action);
                }
            }
        ));
        self.create.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let action = w.workspaces.ui.create_action.borrow().clone();
                if let Some(action) = action {
                    w.workspaces.ui.run(&w, action);
                }
            }
        ));
        self.search.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |_| w.workspaces.ui.rows(&w)
        ));
        self.list.connect_row_selected(glib::clone!(
            #[weak]
            w,
            move |_, row| {
                let ui = &w.workspaces.ui;
                if ui.rebuilding.get() {
                    return;
                }
                *ui.selected.borrow_mut() =
                    row.and_then(|row| ui.rows.borrow().get(row.index() as usize).cloned());
                ui.selection(&w);
            }
        ));
        self.list.connect_row_activated(glib::clone!(
            #[weak]
            w,
            move |_, row| {
                if w.workspaces.ui.page.get() == ManagerPage::Workspaces {
                    w.workspaces.ui.list.select_row(Some(row));
                    return;
                }
                // Enter selects a row; the explicitly labeled primary button applies it.
                w.workspaces
                    .ui
                    .details
                    .child_focus(gtk::DirectionType::TabForward);
            }
        ));
    }
    pub fn show(&self, w: &Rc<Workspace>, page: ManagerPage) {
        // The shared enum still includes layouts while its core API is retired.
        if page == ManagerPage::Templates {
            return;
        }
        self.stop_preview();
        self.presented.set(true);
        *self.selected.borrow_mut() = None;
        self.page.set(page);
        self.search.set_text("");
        self.note.set_visible(false);
        let toolbar = matches!(
            page,
            ManagerPage::ThisWorkspace | ManagerPage::ToolbarLibrary
        );
        self.generation.set(self.generation.get().wrapping_add(1));
        let compact = page == ManagerPage::Workspaces;
        let empty = gtk::Label::new(Some(match page {
            ManagerPage::Workspaces => "No matching workspaces.",
            _ => "No matching toolbars.",
        }));
        margins(&empty, 18);
        self.list.set_placeholder(Some(&empty));
        self.list.set_selection_mode(gtk::SelectionMode::Single);
        self.footer.set_visible(compact);
        self.create.set_visible(compact);
        self.apply.set_label("Switch to Workspace");
        self.dialog
            .set_content_width(if compact { 460 } else { 780 });
        self.dialog
            .set_content_height(if compact { 500 } else { 600 });
        self.split.set_start_child(Some(&self.sidebar));
        self.split.set_end_child(if compact {
            None
        } else {
            Some(&self.details_view)
        });
        self.intro.set_text(match page {
            ManagerPage::Workspaces => {
                "Workspaces save your tool settings and layout for different tasks."
            }
            ManagerPage::Templates => unreachable!(),
            ManagerPage::ThisWorkspace => "Arrange the toolbars in this workspace.",
            ManagerPage::ToolbarLibrary => "Save toolbars to reuse in any workspace.",
        });
        self.intro.set_visible(true);
        self.dialog.set_title(if toolbar {
            "Manage Toolbars"
        } else {
            page.label()
        });
        clear(&self.tabs);
        for page in if toolbar {
            vec![ManagerPage::ThisWorkspace, ManagerPage::ToolbarLibrary]
        } else {
            Vec::new()
        } {
            let button = gtk::ToggleButton::with_label(page.label());
            button.set_active(page == self.page.get());
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.workspaces.ui.show(&w, page)
            ));
            self.tabs.append(&button);
        }
        self.tabs.set_visible(toolbar);
        clear(&self.actions);
        let action = match page {
            ManagerPage::Workspaces => Some(ManagerAction::New),
            _ => None,
        };
        *self.create_action.borrow_mut() = action;
        let create_label = "New Workspace";
        self.create.set_tooltip_text(Some(create_label));
        self.create
            .update_property(&[gtk::accessible::Property::Label(create_label)]);
        if page == ManagerPage::ThisWorkspace {
            let button = gtk::Button::with_label("New Toolbar…");
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.dispatch(UiAction::WorkspaceManager {
                    command: WorkspaceCommand::NewToolbar { group: None }
                })
            ));
            self.actions.append(&button);
        }
        self.rows(w);
        self.dialog.present(Some(&w.window));
        self.start_preview(w);
        let epoch = self.preview_epoch.get();
        let manager = w.workspaces.manager.as_ref().unwrap().clone();
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let result = manager.refresh().await;
                let result = match result {
                    Ok(()) => manager.refresh_switcher().await,
                    Err(error) => Err(error),
                };
                if !w.workspaces.ui.presented.get() || w.workspaces.ui.preview_epoch.get() != epoch
                {
                    return;
                }
                match result {
                    Ok(()) => {
                        w.workspaces.sync_binding(&w);
                        w.workspaces.update_switcher();
                        w.workspaces.ui.rows(&w);
                    }
                    Err(error) => w.workspaces.ui.error(&error.to_string()),
                }
            }
        ));
    }
    fn rows(&self, w: &Rc<Workspace>) {
        let Some(manager) = &w.workspaces.manager else {
            return;
        };
        let rows = manager.rows(self.page.get(), &self.search.text(), now_ms());
        let pinned = manager.switcher_ids();
        let compact = self.page.get() == ManagerPage::Workspaces;
        self.search
            .set_visible(!compact || rows.len() > 7 || !self.search.text().is_empty());
        let selected = self.selected.borrow().clone().or_else(|| {
            (self.page.get() == ManagerPage::Workspaces)
                .then(|| manager.active_id())
                .flatten()
        });
        self.rebuilding.set(true);
        self.list.remove_all();
        self.rows.borrow_mut().clear();
        let mut selected_index = if compact { None } else { Some(0) };
        for (index, item) in rows.into_iter().enumerate() {
            if selected.as_ref() == Some(&item.id) {
                selected_index = Some(index);
            }
            let row = adw::ActionRow::builder()
                .title(&item.title)
                .subtitle(&item.subtitle)
                .build();
            row.set_use_markup(false);
            row.set_widget_name(&format!("workspace-row-{}", item.id));
            row.set_activatable(true);
            if compact {
                if pinned.contains(&item.id) {
                    let handle = gtk::Image::from_icon_name("layer-grip-symbolic");
                    handle.set_widget_name(&format!("workspace-reorder-handle-{}", item.id));
                    handle.add_css_class("workspace-reorder-handle");
                    handle.set_size_request(28, 44);
                    handle.set_cursor_from_name(Some("grab"));
                    handle.set_tooltip_text(Some("Drag to reorder in top bar"));
                    row.add_prefix(&handle);
                    let pin = gtk::Image::from_icon_name("layer-pin-symbolic");
                    pin.add_css_class("dim-label");
                    pin.set_tooltip_text(Some("Shown in top bar"));
                    pin.update_property(&[gtk::accessible::Property::Label("Shown in top bar")]);
                    row.add_suffix(&pin);
                    self.bind_reorder_row(w, &row, &item.id);
                }
                let active = manager.active_id().as_deref() == Some(&item.id);
                if active {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                }
                let elsewhere = manager.items().iter().any(|i| {
                    i.id == item.id
                        && i.claim
                            .as_ref()
                            .is_some_and(|c| c.owner != manager.owner && c.expires_at_ms > now_ms())
                });
                {
                    let mut actions = vec![ManagerButton {
                        action: ManagerAction::Rename(item.id.clone()),
                        label: "Rename…".into(),
                        enabled: !elsewhere,
                        primary: false,
                    }];
                    if !item.builtin {
                        actions.push(ManagerButton {
                            action: ManagerAction::Delete(item.id.clone()),
                            label: "Delete…".into(),
                            enabled: !elsewhere,
                            primary: false,
                        });
                    }
                    let more = actions_menu(w, &format!("Options for {}", item.title), actions);
                    self.add_switcher_actions(w, &more, &item.id, &pinned);
                    more.set_valign(gtk::Align::Center);
                    row.add_suffix(&more);
                }
            }
            self.rows.borrow_mut().push(item.id);
            self.list.append(&row);
        }
        let row = selected_index.and_then(|index| self.list.row_at_index(index as i32));
        self.list.select_row(row.as_ref());
        *self.selected.borrow_mut() =
            row.and_then(|row| self.rows.borrow().get(row.index() as usize).cloned());
        self.rebuilding.set(false);
        if !self.preserve_preview.get() {
            self.selection(w);
        }
        if self.rows.borrow().is_empty() {
            clear(&self.details);
            self.details
                .append(&gtk::Label::new(Some("No items found.")));
        }
    }
    fn selection(&self, w: &Rc<Workspace>) {
        if self.page.get() == ManagerPage::Workspaces {
            self.preview_selection(w);
            return;
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let Some(id) = self.selected.borrow().clone() else {
            return;
        };
        let manager = w.workspaces.manager.as_ref().unwrap().clone();
        if self.page.get() == ManagerPage::ThisWorkspace {
            let Ok(panel) = serde_json::from_str::<Panel>(&id) else {
                return;
            };
            let Some(current) = manager.current() else {
                return;
            };
            let Ok(capture) = current.capture() else {
                return;
            };
            let Ok(config) = capture.history.layout().panel(panel) else {
                return;
            };
            let visible = capture.history.layout().panel_group(panel).is_some();
            let details = ManagerDetails {
                title: config.title().into(),
                description: format!("Toolbar in {}.", current.metadata.name),
                preview: None,
                actions: [
                    ManagerAction::ShowToolbar(panel, !visible),
                    ManagerAction::RenameToolbar(panel),
                    ManagerAction::SaveToolbar(panel),
                    ManagerAction::ReplaceToolbar(panel),
                    ManagerAction::DeleteToolbar(panel),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, action)| ManagerButton {
                    label: action.label().into(),
                    action,
                    enabled: true,
                    primary: i == 0,
                })
                .collect(),
            };
            self.render(w, details);
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let stored = if manager.active_id().as_deref() == Some(&id) {
                    manager
                        .current_record()
                        .ok_or_else(|| StoreError::invalid("No workspace is active."))
                } else {
                    manager.load(&id).await
                };
                if w.workspaces.ui.generation.get() != generation {
                    return;
                }
                match stored {
                    Ok(stored) => {
                        let idle = w
                            .gpu
                            .borrow()
                            .as_ref()
                            .is_some_and(|g| g.session.require_workspace_idle().is_ok());
                        let details = manager.inspect_details(&stored, idle, now_ms()).await;
                        if w.workspaces.ui.generation.get() == generation {
                            w.workspaces.ui.render(&w, details);
                        }
                    }
                    Err(error) => {
                        clear(&w.workspaces.ui.details);
                        w.workspaces.ui.error(&error.to_string());
                    }
                }
            }
        ));
    }
    fn stop_preview(&self) {
        self.preview_epoch
            .set(self.preview_epoch.get().wrapping_add(1));
        self.generation.set(self.generation.get().wrapping_add(1));
        self.apply.set_sensitive(false);
        *self.primary.borrow_mut() = None;
        let preview = self.preview.borrow_mut().take();
        drop(preview);
    }
    fn start_preview(&self, w: &Rc<Workspace>) {
        if !self.presented.get()
            || self.action_pending.get()
            || self.page.get() != ManagerPage::Workspaces
        {
            return;
        }
        self.stop_preview();
        self.sidebar.set_sensitive(false);
        self.create.set_sensitive(false);
        let epoch = self.preview_epoch.get();
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let ui = &w.workspaces.ui;
                while w.workspaces.busy.get() {
                    if ui.preview_epoch.get() != epoch || !ui.presented.get() {
                        return;
                    }
                    glib::timeout_future(Duration::from_millis(10)).await;
                }
                if ui.preview_epoch.get() != epoch || !ui.presented.get() {
                    return;
                }
                let preview = history::Preview::begin(&w).await;
                if ui.preview_epoch.get() != epoch || !ui.presented.get() {
                    return;
                }
                ui.sidebar.set_sensitive(true);
                ui.create.set_sensitive(true);
                match preview {
                    Ok(preview) => {
                        *ui.preview.borrow_mut() = Some(preview);
                        ui.preview_selection(&w);
                    }
                    Err(error) => ui.error(&error.to_string()),
                }
            }
        ));
    }
    fn preview_selection(&self, w: &Rc<Workspace>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        self.apply.set_sensitive(false);
        *self.primary.borrow_mut() = None;
        if !self.presented.get() || self.action_pending.get() {
            return;
        }
        let preview = self.preview.borrow();
        let Some(preview) = preview.as_ref() else {
            return;
        };
        preview.reset();
        let Some(id) = self.selected.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let stored = w.workspaces.selected(&id).await;
                let ui = &w.workspaces.ui;
                if ui.generation.get() != generation
                    || !ui.presented.get()
                    || ui.preview.borrow().is_none()
                {
                    return;
                }
                let result = stored.and_then(|stored| {
                    if stored.entity.metadata.deleted_at_ms.is_some() {
                        return Err(StoreError::invalid(
                            "This workspace was deleted. Choose another workspace.",
                        ));
                    }
                    let details =
                        w.workspaces
                            .manager
                            .as_ref()
                            .unwrap()
                            .details(&stored, true, now_ms());
                    let layout = details.preview.ok_or_else(|| {
                        StoreError::invalid("This item has no layout to preview.")
                    })?;
                    let change = w
                        .gpu
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .session
                        .preview_workspace_layout(&layout)
                        .map_err(StoreError::invalid)?;
                    w.changed(Ok(change));
                    Ok(details.actions.into_iter().find(|action| action.primary))
                });
                match result {
                    Ok(Some(primary)) => {
                        ui.apply.set_label(match &primary.action {
                            ManagerAction::SwitchToWindow(_) => "Switch to Window",
                            _ => "Switch to Workspace",
                        });
                        ui.apply.set_sensitive(primary.enabled);
                        *ui.primary.borrow_mut() = primary.enabled.then_some(primary.action);
                    }
                    Ok(None) => (),
                    Err(error) => ui.error(&error.to_string()),
                }
            }
        ));
    }
    fn render(&self, w: &Rc<Workspace>, details: ManagerDetails) {
        clear(&self.details);
        let title = gtk::Label::new(Some(&details.title));
        title.add_css_class("title-2");
        title.set_xalign(0.);
        self.details.append(&title);
        if let Some(layout) = details.preview {
            self.details.append(&preview(layout));
        }
        let description = gtk::Label::new(Some(&details.description));
        description.set_xalign(0.);
        description.set_wrap(true);
        description.set_selectable(true);
        self.details.append(&description);
        let mut secondary = Vec::new();
        for button in details.actions {
            if !button.primary {
                secondary.push(button);
                continue;
            }
            let widget = action_button(w, button.action, button.primary, button.enabled);
            widget.set_label(&button.label);
            self.details.append(&widget);
        }
        if !secondary.is_empty() {
            let more = actions_menu(w, "More options", secondary);
            more.set_label("More…");
            self.details.append(&more);
        }
    }
    pub fn error(&self, error: &str) {
        self.note.set_text(error);
        self.note.add_css_class("error");
        self.note.set_visible(true);
    }
    pub fn close(&self) {
        self.stop_preview();
        if self.presented.replace(false) {
            self.dialog.close();
        }
    }
    pub fn run(&self, w: &Rc<Workspace>, action: ManagerAction) {
        if self.action_pending.replace(true) {
            return;
        }
        self.stop_preview();
        self.sidebar.set_sensitive(false);
        self.create.set_sensitive(false);
        self.note.set_visible(false);
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                // A cancelled preview may still be waiting for an earlier save.
                while w.workspaces.busy.get() {
                    glib::timeout_future(Duration::from_millis(10)).await;
                }
                if let Err(error) = w.workspaces.perform(&w, action).await {
                    w.workspaces.ui.error(&error.to_string());
                }
                w.workspaces.ui.action_pending.set(false);
                w.workspaces.ui.sidebar.set_sensitive(true);
                w.workspaces.ui.create.set_sensitive(true);
                w.workspaces.sync_binding(&w);
                if w.workspaces.ui.presented.get() {
                    w.workspaces.ui.rows(&w);
                    w.workspaces.ui.start_preview(&w);
                }
            }
        ));
    }
}
fn margins(widget: &impl IsA<gtk::Widget>, value: i32) {
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
    widget.set_margin_start(value);
    widget.set_margin_end(value);
}
fn actions_menu(w: &Rc<Workspace>, label: &str, actions: Vec<ManagerButton>) -> gtk::MenuButton {
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(label)
        .build();
    menu.add_css_class("flat");
    let popup = gtk::Popover::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    margins(&content, 6);
    for item in actions {
        let button = gtk::Button::with_label(&item.label);
        button.add_css_class("flat");
        button.set_sensitive(item.enabled);
        if item.action.destructive() {
            button.add_css_class("destructive-action");
        }
        button.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[weak]
            popup,
            move |_| {
                popup.popdown();
                w.workspaces.ui.run(&w, item.action.clone());
            }
        ));
        content.append(&button);
    }
    popup.set_child(Some(&content));
    menu.set_popover(Some(&popup));
    menu
}
fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
fn action_button(
    w: &Rc<Workspace>,
    action: ManagerAction,
    primary: bool,
    enabled: bool,
) -> gtk::Button {
    let button = gtk::Button::with_label(action.label());
    button.set_sensitive(enabled);
    if primary {
        button.add_css_class("suggested-action");
    }
    if action.destructive() {
        button.add_css_class("destructive-action");
    }
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| w.workspaces.ui.run(&w, action.clone())
    ));
    button
}
fn preview(layout: DockLayout) -> gtk::DrawingArea {
    let view = gtk::DrawingArea::new();
    view.set_content_height(200);
    view.set_hexpand(true);
    view.set_widget_name("workspace-layout-preview");
    view.set_draw_func(move |_, cr, width, height| {
        let resolved = layout.workspace(1000., 700., HEADER_HEIGHT, STATUS_HEIGHT);
        cr.scale(width as f64 / 1000., height as f64 / 700.);
        cr.set_source_rgb(0.19, 0.20, 0.22);
        let _ = cr.paint();
        let canvas = resolved.work_area;
        cr.set_source_rgb(0.88, 0.89, 0.90);
        cr.rectangle(
            canvas.x as f64,
            canvas.y as f64,
            canvas.width as f64,
            canvas.height as f64,
        );
        let _ = cr.fill();
        for group in &resolved.groups {
            let b = group.bounds;
            cr.set_source_rgb(0.42, 0.46, 0.51);
            cr.rectangle(b.x as f64, b.y as f64, b.width as f64, b.height as f64);
            let _ = cr.fill();
            cr.set_source_rgb(0.61, 0.65, 0.70);
            cr.rectangle(
                b.x as f64 + 3.,
                b.y as f64 + 3.,
                (b.width as f64 - 6.).max(0.),
                14.,
            );
            let _ = cr.fill();
        }
    });
    view
}

pub(crate) async fn confirm(
    w: &Workspace,
    title: &str,
    message: &str,
    label: &str,
    destructive: bool,
) -> bool {
    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(message)
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", label)]);
    dialog.set_close_response("cancel");
    dialog.set_response_appearance(
        "confirm",
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    dialog.choose_future(Some(&w.window)).await == "confirm"
}

pub(crate) struct NamedValues {
    pub name: String,
    pub description: String,
    pub choice: Option<String>,
}
pub(crate) async fn name_dialog(
    w: &Workspace,
    title: &str,
    message: &str,
    confirm: &str,
    name: &str,
    description: Option<&str>,
    choices: &[(String, String)],
    selected: Option<&str>,
    error: Option<&str>,
) -> Option<NamedValues> {
    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(message)
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", confirm)]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("confirm"));
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let entry = gtk::Entry::builder()
        .text(name)
        .placeholder_text("Name")
        .activates_default(true)
        .build();
    entry.set_widget_name("workspace-item-name");
    form.append(&entry);
    let desc = gtk::Entry::builder()
        .text(description.unwrap_or_default())
        .placeholder_text("Description (optional)")
        .build();
    desc.set_widget_name("workspace-item-description");
    if description.is_some() {
        form.append(&desc);
    }
    let options: Vec<_> = choices.iter().map(|(_, name)| name.as_str()).collect();
    let choice = gtk::DropDown::from_strings(&options);
    if !choices.is_empty() {
        let label = gtk::Label::new(Some("Start with"));
        label.set_xalign(0.);
        form.append(&label);
        choice.set_selected(
            choices
                .iter()
                .position(|(id, _)| Some(id.as_str()) == selected)
                .unwrap_or(0) as u32,
        );
        choice.set_widget_name("workspace-start-with");
        form.append(&choice);
    }
    let validation = gtk::Label::new(error);
    validation.set_wrap(true);
    validation.add_css_class("error");
    validation.set_visible(error.is_some());
    validation.set_widget_name("workspace-name-error");
    form.append(&validation);
    entry.connect_changed(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        validation,
        move |entry| {
            let result = layer_workspace::validate_name(entry.text().trim());
            dialog.set_response_enabled("confirm", result.is_ok());
            validation.set_text(
                &result
                    .as_ref()
                    .err()
                    .map_or_else(String::new, ToString::to_string),
            );
            validation.set_visible(result.is_err());
        }
    ));
    dialog.set_extra_child(Some(&form));
    dialog.set_response_enabled(
        "confirm",
        layer_workspace::validate_name(name.trim()).is_ok(),
    );
    if dialog.choose_future(Some(&w.window)).await != "confirm" {
        return None;
    }
    Some(NamedValues {
        name: entry.text().trim().into(),
        description: desc.text().into(),
        choice: choices
            .get(choice.selected() as usize)
            .map(|(id, _)| id.clone()),
    })
}

pub(crate) async fn choice_dialog(
    w: &Workspace,
    title: &str,
    message: &str,
    confirm: &str,
    choices: &[(String, String)],
) -> Option<String> {
    if choices.is_empty() {
        return None;
    }
    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(message)
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", confirm)]);
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    let strings: Vec<_> = choices.iter().map(|(_, name)| name.as_str()).collect();
    let select = gtk::DropDown::from_strings(&strings);
    select.set_widget_name("workspace-item-choice");
    dialog.set_extra_child(Some(&select));
    if dialog.choose_future(Some(&w.window)).await == "confirm" {
        choices
            .get(select.selected() as usize)
            .map(|(id, _)| id.clone())
    } else {
        None
    }
}
