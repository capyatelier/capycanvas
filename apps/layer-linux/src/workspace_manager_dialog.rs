//! Native projection of shared workspace lists and item actions.
use super::*;
use layer_workspace::{
    ItemContent, ItemKind, ManagerAction, ManagerButton, ManagerDetails, ManagerPage,
    ReusableContent,
};

pub(crate) struct ManagerUi {
    pub dialog: adw::Dialog,
    presented: Cell<bool>,
    page: Cell<ManagerPage>,
    tabs: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    details: gtk::Box,
    pub note: gtk::Label,
    undo: gtk::Button,
    undo_id: RefCell<Option<String>>,
    rows: RefCell<Vec<String>>,
    selected: RefCell<Option<String>>,
    generation: Cell<u64>,
    rebuilding: Cell<bool>,
    history_mode: Cell<bool>,
    actions: gtk::Box,
    intro: gtk::Label,
    more: gtk::MenuButton,
    split: gtk::Paned,
    sidebar: gtk::Box,
    details_view: gtk::ScrolledWindow,
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
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More options")
            .build();
        more.add_css_class("flat");
        header.pack_end(&more);
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
        let undo = gtk::Button::with_label("Undo Deletion");
        undo.set_visible(false);
        undo.set_halign(gtk::Align::Start);
        body.append(&undo);
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
            undo,
            undo_id: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            selected: RefCell::new(None),
            generation: Cell::new(0),
            rebuilding: Cell::new(false),
            history_mode: Cell::new(false),
            actions,
            intro,
            more,
            split,
            sidebar,
            details_view,
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| w.workspaces.ui.presented.set(false)
        ));
        self.undo.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let id = w.workspaces.ui.undo_id.borrow_mut().take();
                w.workspaces.ui.undo.set_visible(false);
                if let Some(id) = id {
                    w.workspaces.ui.run(&w, ManagerAction::RestoreDeleted(id));
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
                    if let Some(id) = w.workspaces.ui.rows.borrow().get(row.index() as usize) {
                        w.workspaces.ui.run(&w, workspace_primary(&w, id));
                    }
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
        self.history_mode.set(false);
        self.page.set(page);
        self.search.set_text("");
        self.note.set_visible(false);
        let toolbar = matches!(
            page,
            ManagerPage::ThisWorkspace | ManagerPage::ToolbarLibrary
        );
        let compact = page == ManagerPage::Workspaces;
        self.list.set_selection_mode(if compact {
            gtk::SelectionMode::None
        } else {
            gtk::SelectionMode::Single
        });
        self.dialog
            .set_content_width(if compact { 520 } else { 780 });
        self.dialog
            .set_content_height(if compact { 540 } else { 600 });
        self.split.set_start_child(Some(&self.sidebar));
        self.split.set_end_child(if compact {
            None
        } else {
            Some(&self.details_view)
        });
        self.intro.set_text(match page {
            ManagerPage::Workspaces=>"Switch between layouts you use for different tasks. Your changes are saved automatically.",
            ManagerPage::Templates=>"Workspace Templates save the exact layout of your tools and panels so you can load it again whenever you want. Choose one to start a new workspace.",
            ManagerPage::ThisWorkspace=>"Show, hide, or customize the toolbars in this workspace.",
            ManagerPage::ToolbarLibrary=>"Save your favorite toolbar setups here, then add them to any workspace.",
            ManagerPage::RecentlyDeleted=>"Deleted items are kept for 30 days. Restore one to use it again.",
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
        if !toolbar && !compact {
            let back = gtk::Button::with_label("Back to Workspaces");
            back.add_css_class("flat");
            back.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.workspaces.ui.show(&w, ManagerPage::Workspaces)
            ));
            self.tabs.append(&back);
        }
        self.tabs.set_visible(!compact);
        let popup = gtk::Popover::new();
        let options = gtk::Box::new(gtk::Orientation::Vertical, 4);
        margins(&options, 6);
        for (title, page, action) in [
            ("Workspace Templates…", Some(ManagerPage::Templates), None),
            (
                "Recently Deleted…",
                Some(ManagerPage::RecentlyDeleted),
                None,
            ),
            ("Backups and Storage…", None, Some(ManagerAction::Storage)),
        ] {
            let button = gtk::Button::with_label(title);
            button.add_css_class("flat");
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                #[weak]
                popup,
                move |_| {
                    popup.popdown();
                    if let Some(action) = &action {
                        w.workspaces.ui.run(&w, action.clone());
                    } else {
                        w.workspaces.ui.show(&w, page.unwrap());
                    }
                }
            ));
            options.append(&button);
        }
        popup.set_child(Some(&options));
        self.more.set_popover(Some(&popup));
        clear(&self.actions);
        let action = match page {
            ManagerPage::Workspaces => Some(ManagerAction::New),
            ManagerPage::Templates => Some(ManagerAction::ImportTemplate),
            ManagerPage::ToolbarLibrary => Some(ManagerAction::ImportToolbar),
            _ => None,
        };
        if let Some(action) = action {
            self.actions.append(&action_button(w, action, false, true));
        }
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
        self.presented.set(true);
        self.dialog.present(Some(&w.window));
        let manager = w.workspaces.manager.as_ref().unwrap().clone();
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                match manager.refresh().await {
                    Ok(()) => {
                        w.workspaces.sync_binding(&w);
                        w.workspaces.ui.rows(&w);
                    }
                    Err(error) => w.workspaces.ui.error(&error.to_string()),
                }
            }
        ));
    }
    fn rows(&self, w: &Rc<Workspace>) {
        if self.history_mode.get() {
            return;
        }
        let Some(manager) = &w.workspaces.manager else {
            return;
        };
        let rows = manager.rows(self.page.get(), &self.search.text(), now_ms());
        let compact = self.page.get() == ManagerPage::Workspaces;
        self.search
            .set_visible(!compact || rows.len() > 7 || !self.search.text().is_empty());
        let selected = self.selected.borrow().clone();
        self.rebuilding.set(true);
        self.list.remove_all();
        self.rows.borrow_mut().clear();
        let mut selected_index = 0;
        for (index, item) in rows.into_iter().enumerate() {
            if selected.as_ref() == Some(&item.id) {
                selected_index = index;
            }
            let row = adw::ActionRow::builder()
                .title(&item.title)
                .subtitle(&item.subtitle)
                .build();
            row.set_use_markup(false);
            row.set_activatable(true);
            if compact {
                let active = manager.active_id().as_deref() == Some(&item.id);
                if active {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                } else {
                    let switch = action_button(w, workspace_primary(w, &item.id), false, true);
                    switch.set_valign(gtk::Align::Center);
                    row.add_suffix(&switch);
                }
                let elsewhere = manager.items().iter().any(|i| {
                    i.id == item.id
                        && i.claim
                            .as_ref()
                            .is_some_and(|c| c.owner != manager.owner && c.expires_at_ms > now_ms())
                });
                let more = actions_menu(
                    w,
                    &format!("Options for {}", item.title),
                    vec![
                        ManagerButton {
                            action: ManagerAction::Rename(item.id.clone()),
                            label: "Rename…".into(),
                            enabled: !elsewhere,
                            primary: false,
                        },
                        ManagerButton {
                            action: ManagerAction::Delete(item.id.clone()),
                            label: "Delete…".into(),
                            enabled: !elsewhere,
                            primary: false,
                        },
                    ],
                );
                more.set_valign(gtk::Align::Center);
                row.add_suffix(&more);
            }
            self.rows.borrow_mut().push(item.id);
            self.list.append(&row);
        }
        self.rebuilding.set(false);
        self.list
            .select_row(self.list.row_at_index(selected_index as i32).as_ref());
        if self.rows.borrow().is_empty() {
            clear(&self.details);
            self.details
                .append(&gtk::Label::new(Some("No items found.")));
        }
    }
    fn selection(&self, w: &Rc<Workspace>) {
        if self.page.get() == ManagerPage::Workspaces {
            return;
        }
        self.history_mode.set(false);
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
                description: format!(
                    "Toolbar in {}. Changes are saved with this workspace and can be recovered in Layout History.",
                    current.metadata.name
                ),
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
        if self.presented.replace(false) {
            self.dialog.close();
        }
    }
    pub fn offer_undo(&self, _w: &Rc<Workspace>, id: &str, name: &str) {
        *self.undo_id.borrow_mut() = Some(id.into());
        self.note.remove_css_class("error");
        self.note
            .set_text(&format!("{name} moved to Recently Deleted."));
        self.note.set_visible(true);
        self.undo.set_visible(true);
    }
    pub fn deletion_restored(&self, id: &str) {
        if self.undo_id.borrow().as_deref() == Some(id) {
            self.undo_id.borrow_mut().take();
            self.undo.set_visible(false);
            self.note.set_visible(false);
        }
    }
    pub fn storage(&self, w: &Rc<Workspace>, description: String) {
        self.show(w, ManagerPage::Workspaces);
        self.history_mode.set(true);
        self.generation.set(self.generation.get().wrapping_add(1));
        self.split.set_start_child(None::<&gtk::Widget>);
        self.split.set_end_child(Some(&self.details_view));
        self.intro.set_visible(false);
        self.dialog.set_title("Backups and Storage");
        clear(&self.details);
        let title = gtk::Label::new(Some("Storage and Backups"));
        title.add_css_class("title-2");
        title.set_xalign(0.);
        self.details.append(&title);
        let text = gtk::Label::new(Some(&description));
        text.set_wrap(true);
        text.set_xalign(0.);
        text.set_selectable(true);
        self.details.append(&text);
        for action in [ManagerAction::ExportCurrent, ManagerAction::ImportBackup] {
            self.details.append(&action_button(w, action, false, true));
        }
        let advanced = gtk::Expander::builder()
            .label("More storage options")
            .build();
        let options = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let explanation = gtk::Label::new(Some(
            "Recover unsaved changes, free space by clearing older history, or export all stored data for troubleshooting.",
        ));
        explanation.set_wrap(true);
        explanation.set_xalign(0.);
        options.append(&explanation);
        for action in [
            ManagerAction::ClearOlderHistory,
            ManagerAction::SaveAsNew,
            ManagerAction::RetryStorage,
            ManagerAction::RecoverInterrupted,
            ManagerAction::ExportDatabase,
        ] {
            options.append(&action_button(w, action, false, true));
        }
        advanced.set_child(Some(&options));
        self.details.append(&advanced);
    }
    pub fn run(&self, w: &Rc<Workspace>, action: ManagerAction) {
        self.note.set_visible(false);
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                if let Err(error) = w.workspaces.perform(&w, action).await {
                    w.workspaces.ui.error(&error.to_string());
                }
                w.workspaces.sync_binding(&w);
                w.workspaces.ui.rows(&w);
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
fn workspace_primary(w: &Workspace, id: &str) -> ManagerAction {
    let manager = w.workspaces.manager.as_ref().unwrap();
    if manager.items().iter().any(|i| {
        i.id == id
            && i.claim
                .as_ref()
                .is_some_and(|c| c.owner != manager.owner && c.expires_at_ms > now_ms())
    }) {
        ManagerAction::SwitchToWindow(id.into())
    } else {
        ManagerAction::Switch(id.into())
    }
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

impl ManagerUi {
    pub async fn history(
        &self,
        w: &Rc<Workspace>,
        id: &str,
        library: bool,
    ) -> Result<(), StoreError> {
        if !library {
            return super::history::show(w, id).await;
        }
        let manager = w.workspaces.manager.as_ref().unwrap().clone();
        let stored = w.workspaces.selected(id).await?;
        let page = match stored.entity.metadata.kind {
            ItemKind::Workspace => ManagerPage::Workspaces,
            ItemKind::Template => ManagerPage::Templates,
            ItemKind::Toolbar => ManagerPage::ToolbarLibrary,
        };
        if !self.presented.get() || self.page.get() != page {
            self.show(w, page);
        }
        *self.selected.borrow_mut() = Some(id.into());
        self.history_mode.set(false);
        self.rows(w);
        self.history_mode.set(true);
        self.generation.set(self.generation.get().wrapping_add(1));
        let mut versions: Vec<(String, String, u64, Option<DockLayout>)> =
            match &stored.entity.content {
                ItemContent::Workspace { history, .. } => history
                    .revisions
                    .values()
                    .map(|r| {
                        (
                            r.id.clone(),
                            r.description.clone(),
                            r.timestamp_ms,
                            Some(r.layout.clone()),
                        )
                    })
                    .collect(),
                ItemContent::Reusable { current, previous } => std::iter::once(current)
                    .chain(previous)
                    .map(|r| {
                        (
                            r.id.clone(),
                            r.name.clone(),
                            r.timestamp_ms,
                            if let ReusableContent::Layout { layout } = &r.content {
                                Some(layout.clone())
                            } else {
                                None
                            },
                        )
                    })
                    .collect(),
            };
        versions.sort_by_key(|v| std::cmp::Reverse(v.2));
        clear(&self.details);
        let back = gtk::Button::with_label("Back to Details");
        back.add_css_class("flat");
        back.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.workspaces.ui.history_mode.set(false);
                w.workspaces.ui.rows(&w);
            }
        ));
        self.details.append(&back);
        let heading = gtk::Label::new(Some(&format!(
            "{} · {}",
            if library {
                "Previous Versions"
            } else {
                "Layout History"
            },
            stored.entity.metadata.name
        )));
        heading.add_css_class("title-2");
        heading.set_xalign(0.);
        self.details.append(&heading);
        let description = gtk::Label::new(Some(if library {
            "Choose an earlier version to use for future additions."
        } else {
            "Select a version to preview its layout."
        }));
        description.set_wrap(true);
        description.set_xalign(0.);
        self.details.append(&description);
        let versions = Rc::new(versions);
        let selected = Rc::new(Cell::new(0usize));
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_widget_name("workspace-history-items");
        for (_, label, time, _) in versions.iter() {
            let row = adw::ActionRow::builder()
                .title(label)
                .subtitle(layer_workspace::date(*time))
                .build();
            row.set_use_markup(false);
            list.append(&row);
        }
        self.details.append(
            &gtk::ScrolledWindow::builder()
                .height_request(150)
                .max_content_height(220)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&list)
                .build(),
        );
        let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        self.details.append(&preview_box);
        list.connect_row_selected(glib::clone!(
            #[strong]
            selected,
            #[strong]
            versions,
            #[weak]
            preview_box,
            move |_, row| {
                let Some(row) = row else {
                    return;
                };
                selected.set(row.index() as usize);
                clear(&preview_box);
                if let Some((_, _, _, Some(layout))) = versions.get(selected.get()) {
                    preview_box.append(&preview(layout.clone()));
                }
            }
        ));
        list.select_row(list.row_at_index(0).as_ref());
        let restore = gtk::Button::with_label("Restore This Version");
        restore.add_css_class("suggested-action");
        restore.set_widget_name("workspace-history-restore");
        restore.set_sensitive(!stored.entity.metadata.builtin);
        let entity_id = id.to_string();
        restore.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[strong]
            selected,
            #[strong]
            versions,
            #[strong]
            entity_id,
            move |_| {
                let Some((version, _, _, _)) = versions.get(selected.get()) else {
                    return;
                };
                let version = version.clone();
                let id = entity_id.clone();
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    w,
                    async move {
                        let result: Result<(), StoreError> = async {
                            let _operation = w.workspaces.begin_operation(&w).await?;
                            let manager = w.workspaces.manager.as_ref().unwrap();
                            if library {
                                let result = manager
                                    .restore_reusable_version(&id, &version, now_ms())
                                    .await;
                                w.workspaces.finish_operation(&w);
                                result?;
                            } else {
                                let result =
                                    manager.change_layout(&id, Some(&version), now_ms()).await;
                                match result {
                                    Ok(incoming) if manager.active_id().as_deref() == Some(&id) => {
                                        w.workspaces.adopt(&w, Ok(incoming)).await
                                    }
                                    Ok(_) => w.workspaces.finish_operation(&w),
                                    Err(error) => {
                                        w.workspaces.finish_operation(&w);
                                        return Err(error);
                                    }
                                }
                            }
                            w.workspaces.ui.history(&w, &id, library).await
                        }
                        .await;
                        if let Err(error) = result {
                            w.workspaces.ui.error(&error.to_string());
                        }
                    }
                ));
            }
        ));
        self.details.append(&restore);
        let open = gtk::Button::with_label(if library {
            "Create Workspace from This Version…"
        } else {
            "Open as New Workspace…"
        });
        open.set_widget_name("workspace-history-open-new");
        open.set_sensitive(stored.entity.metadata.kind != ItemKind::Toolbar);
        let name = format!("{} Recovered", stored.entity.metadata.name);
        open.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[strong]
            selected,
            #[strong]
            versions,
            move |_| {
                let Some((version, _, _, _)) = versions.get(selected.get()) else {
                    return;
                };
                let version = version.clone();
                let id = entity_id.clone();
                let name = name.clone();
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    w,
                    async move {
                        let Some(values) = name_dialog(
                            &w,
                            "Open as New Workspace",
                            "Start a new workspace with this layout.",
                            "Create and Switch",
                            &name,
                            None,
                            &[],
                            None,
                            None,
                        )
                        .await
                        else {
                            return;
                        };
                        let result: Result<(), StoreError> = async {
                            let _operation = w.workspaces.begin_operation(&w).await?;
                            let manager = w.workspaces.manager.as_ref().unwrap();
                            let result = if library {
                                manager
                                    .create_from_library_version(
                                        &id,
                                        &version,
                                        &values.name,
                                        now_ms(),
                                    )
                                    .await
                            } else {
                                manager
                                    .open_history_as_workspace(
                                        &id,
                                        &version,
                                        &values.name,
                                        now_ms(),
                                    )
                                    .await
                            };
                            match result {
                                Ok(incoming) => {
                                    w.workspaces.ui.close();
                                    w.workspaces.adopt(&w, Ok(incoming)).await;
                                    Ok(())
                                }
                                Err(error) => {
                                    w.workspaces.finish_operation(&w);
                                    Err(error)
                                }
                            }
                        }
                        .await;
                        if let Err(error) = result {
                            w.workspaces.ui.error(&error.to_string());
                        }
                    }
                ));
            }
        ));
        self.details.append(&open);
        let _ = manager;
        Ok(())
    }
    pub async fn metadata(&self, w: &Rc<Workspace>, id: &str) -> Result<(), StoreError> {
        self.history_mode.set(true);
        self.generation.set(self.generation.get().wrapping_add(1));
        let stored = w.workspaces.selected(id).await?;
        clear(&self.details);
        let back = gtk::Button::with_label("Back to Details");
        back.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.workspaces.ui.history_mode.set(false);
                w.workspaces.ui.rows(&w);
            }
        ));
        self.details.append(&back);
        self.details
            .append(&gtk::Label::new(Some("Name and Description History")));
        for previous in stored.entity.metadata.previous.iter().rev() {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
            let label = gtk::Label::new(Some(&format!(
                "{} · {}\n{}",
                previous.name,
                layer_workspace::date(previous.timestamp_ms),
                previous.description
            )));
            label.set_xalign(0.);
            label.set_wrap(true);
            row.append(&label);
            let restore = gtk::Button::with_label("Restore Name and Description");
            let id = id.to_string();
            let previous = previous.clone();
            restore.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    let id = id.clone();
                    let previous = previous.clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        w,
                        async move {
                            let manager = w.workspaces.manager.as_ref().unwrap();
                            match manager
                                .rename(&id, &previous.name, &previous.description, now_ms())
                                .await
                            {
                                Ok(()) => {
                                    w.workspaces.sync_binding(&w);
                                    w.workspaces.ui.history_mode.set(false);
                                    w.workspaces.ui.rows(&w);
                                }
                                Err(error) => w.workspaces.ui.error(&error.to_string()),
                            }
                        }
                    ));
                }
            ));
            row.append(&restore);
            self.details.append(&row);
        }
        Ok(())
    }
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
