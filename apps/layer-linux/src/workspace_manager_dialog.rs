//! Native projection of the shared workspace manager view.
use super::*;
use layer_workspace::{ManagerAction, ManagerButton, ManagerPage, ManagerPrompt, WorkspaceView};

#[path = "workspace_switcher_dialog.rs"]
mod switcher_controls;

pub(crate) struct ManagerUi {
    pub dialog: adw::Dialog,
    presented: Cell<bool>,
    closing: Cell<bool>,
    page: Cell<Option<ManagerPage>>,
    tabs: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    details: gtk::Box,
    pub note: gtk::Label,
    rows: RefCell<Vec<String>>,
    rows_key: RefCell<String>,
    row_captions: RefCell<std::collections::BTreeMap<String, (String, String)>>,
    details_key: RefCell<String>,
    rebuilding: Cell<bool>,
    actions: gtk::Box,
    intro: gtk::Label,
    split: gtk::Paned,
    sidebar: gtk::Box,
    details_view: gtk::ScrolledWindow,
    apply: gtk::Button,
    footer: gtk::Box,
    create: gtk::Button,
    switcher_pending: Cell<bool>,
    order: RefCell<Vec<String>>,
    dragged: RefCell<Option<String>>,
    prompt: RefCell<Option<adw::AlertDialog>>,
    prompt_key: RefCell<Option<String>>,
    prompt_choices: RefCell<Vec<String>>,
    draft: RefCell<Option<(String, String, Option<String>)>>,
}
impl ManagerUi {
    pub fn new() -> Self {
        let localization = crate::launch_localization();
        let dialog = adw::Dialog::builder()
            .title(&*localization.text(layer_ui::MessageId::WORKSPACE_WORKSPACES))
            .content_width(520)
            .content_height(540)
            .build();
        dialog.set_widget_name("workspace-manager");
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let create = crate::icons::button("layer-plus-symbolic");
        create.set_widget_name("workspace-manager-new");
        create.set_tooltip_text(Some(&localization.text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE)));
        create.update_property(&[gtk::accessible::Property::Label(&localization.text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE))]);
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
        note.add_css_class("error");
        note.set_visible(false);
        body.append(&note);
        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        split.set_position(280);
        split.set_vexpand(true);
        split.set_wide_handle(true);
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 10);
        sidebar.set_size_request(220, -1);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some(&localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_SEARCH)));
        search.set_widget_name("workspace-manager-search");
        sidebar.append(&search);
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 6);
        sidebar.append(&actions);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        list.set_widget_name("workspace-manager-items");
        sidebar.append(&crate::input::pen_scroller(
            gtk::ScrolledWindow::builder()
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&list)
                .build(),
        ));
        let details = gtk::Box::new(gtk::Orientation::Vertical, 12);
        details.set_widget_name("workspace-manager-details");
        details.set_margin_start(18);
        details.set_margin_end(8);
        split.set_start_child(Some(&sidebar));
        let details_view = crate::input::pen_scroller(
            gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&details)
                .build(),
        );
        split.set_end_child(Some(&details_view));
        body.append(&split);
        let apply = gtk::Button::with_label(&localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SWITCH));
        apply.set_widget_name("workspace-manager-apply");
        apply.add_css_class("suggested-action");
        apply.set_sensitive(false);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.set_homogeneous(true);
        let cancel = gtk::Button::with_label(&localization.text(layer_ui::MessageId::COMMON_CANCEL));
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
            closing: Cell::new(false),
            page: Cell::new(None),
            tabs,
            search,
            list,
            details,
            note,
            rows: RefCell::new(Vec::new()),
            rows_key: RefCell::new(String::new()),
            row_captions: RefCell::new(std::collections::BTreeMap::new()),
            details_key: RefCell::new(String::new()),
            rebuilding: Cell::new(false),
            actions,
            intro,
            split,
            sidebar,
            details_view,
            apply,
            footer,
            create,
            switcher_pending: Cell::new(false),
            order: RefCell::new(Vec::new()),
            dragged: RefCell::new(None),
            prompt: RefCell::new(None),
            prompt_key: RefCell::new(None),
            prompt_choices: RefCell::new(Vec::new()),
            draft: RefCell::new(None),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.bind_reorder_drop(w);
        crate::input::guard_editable_activation(&self.search);
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                let ui = &w.workspaces.ui;
                ui.presented.set(false);
                if !ui.closing.get() {
                    w.workspaces.send(&w, WorkspaceInput::Dismiss);
                }
            }
        ));
        self.apply.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| w.workspaces.send(&w, WorkspaceInput::Confirm)
        ));
        self.create.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.workspaces.send(
                    &w,
                    WorkspaceInput::Form {
                        action: ManagerAction::New,
                    },
                )
            }
        ));
        self.search.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |search| {
                if !w.workspaces.ui.rebuilding.get() {
                    w.workspaces.send(
                        &w,
                        WorkspaceInput::Search {
                            query: search.text().into(),
                        },
                    );
                }
            }
        ));
        self.list.connect_row_selected(glib::clone!(
            #[weak]
            w,
            move |_, row| {
                let ui = &w.workspaces.ui;
                if ui.rebuilding.get() {
                    return;
                }
                let id = row.and_then(|row| ui.rows.borrow().get(row.index() as usize).cloned());
                w.workspaces.send(&w, WorkspaceInput::Select { id });
            }
        ));
        self.list.connect_row_activated(glib::clone!(
            #[weak]
            w,
            move |_, row| {
                if matches!(
                    w.workspaces.ui.page.get(),
                    Some(ManagerPage::Workspaces | ManagerPage::History)
                ) {
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
    #[cfg(test)]
    pub fn show(&self, w: &Rc<Workspace>, page: ManagerPage) {
        w.workspaces.open(w, page);
    }
    #[cfg(test)]
    pub fn close(&self, w: &Rc<Workspace>) {
        w.workspaces.send(w, WorkspaceInput::Dismiss);
    }
    pub(crate) fn invalidate_localization(&self) {
        self.row_captions.borrow_mut().clear();
        self.rows_key.borrow_mut().clear();
        self.details_key.borrow_mut().clear();
        self.page.set(None);
    }

    pub fn render(&self, w: &Rc<Workspace>, view: &WorkspaceView) {
        let localization = w.localization();
        self.search.set_placeholder_text(Some(&localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_SEARCH)));
        self.create.set_tooltip_text(Some(&localization.text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE)));
        self.create.update_property(&[gtk::accessible::Property::Label(&localization.text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE))]);
        if let Some(cancel) = self.footer.first_child().and_downcast::<gtk::Button>() { cancel.set_label(&localization.text(layer_ui::MessageId::COMMON_CANCEL)); }
        self.switcher_pending.set(view.switcher_busy);
        *self.order.borrow_mut() = view.order.clone();
        let Some(page) = view.page else {
            self.page.set(None);
            self.rows_key.borrow_mut().clear();
            self.row_captions.borrow_mut().clear();
            if self.presented.get() {
                self.closing.set(true);
                self.dialog.close();
                self.closing.set(false);
            }
            self.render_prompt(w, view);
            return;
        };
        let compact = matches!(page, ManagerPage::Workspaces | ManagerPage::History);
        if self.page.replace(Some(page)) != Some(page) {
            self.configure(w, page);
        }
        self.dialog.set_title(&view.title);
        self.intro.set_text(&view.intro);
        self.intro.set_visible(!view.intro.is_empty());
        let note = view.error.as_ref().or(view.switcher_error.as_ref());
        self.note.set_text(note.map_or("", String::as_str));
        self.note.set_visible(note.is_some());
        self.rebuilding.set(true);
        self.search.set_visible(
            page != ManagerPage::History
                && (!compact || view.rows.len() > 7 || !view.query.is_empty()),
        );
        self.rebuilding.set(false);
        self.rows(w, view, page);
        if !compact {
            self.render_details(w, view);
        }
        self.apply.set_label(&view.primary);
        self.apply.set_sensitive(view.enabled && !view.busy);
        self.sidebar.set_sensitive(!view.busy);
        self.create.set_sensitive(!view.busy);
        if !self.presented.replace(true) {
            self.dialog.present(Some(&w.window));
        }
        self.render_prompt(w, view);
    }
    fn configure(&self, w: &Rc<Workspace>, page: ManagerPage) {
        let toolbar = matches!(
            page,
            ManagerPage::ThisWorkspace | ManagerPage::ToolbarLibrary
        );
        let compact = !toolbar;
        let empty = gtk::Label::new(Some(&w.localization().text(layer_ui::MessageId::NATIVE_HEADER_NO_ITEMS)));
        margins(&empty, 18);
        self.list.set_placeholder(Some(&empty));
        self.footer.set_visible(compact);
        self.create.set_visible(page == ManagerPage::Workspaces);
        self.dialog
            .set_content_width(if compact { 460 } else { 780 });
        self.dialog
            .set_content_height(if compact { 500 } else { 600 });
        self.split.set_end_child(if compact {
            None
        } else {
            Some(&self.details_view)
        });
        clear(&self.tabs);
        if toolbar {
            for tab in [ManagerPage::ThisWorkspace, ManagerPage::ToolbarLibrary] {
                let button = gtk::ToggleButton::with_label(&tab.label(&w.localization()));
                button.set_active(tab == page);
                button.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    move |_| w.workspaces.open(&w, tab)
                ));
                self.tabs.append(&button);
            }
        }
        self.tabs.set_visible(toolbar);
        clear(&self.actions);
        if page == ManagerPage::ThisWorkspace {
            let button = gtk::Button::with_label(&ManagerAction::NewToolbar(None).label(&w.localization()));
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.workspaces.send(
                    &w,
                    WorkspaceInput::Form {
                        action: ManagerAction::NewToolbar(None)
                    }
                )
            ));
            self.actions.append(&button);
        }
        self.details_key.borrow_mut().clear();
        clear(&self.details);
        self.rebuilding.set(true);
        self.search.set_text("");
        self.rebuilding.set(false);
    }
    fn rows(&self, w: &Rc<Workspace>, view: &WorkspaceView, page: ManagerPage) {
        let pinned: Vec<String> = view.switcher.iter().map(|row| row.id.clone()).collect();
        let key = serde_json::to_string(&(page, &view.rows, &pinned)).unwrap_or_default();
        self.rebuilding.set(true);
        if *self.rows_key.borrow() != key && self.dragged.borrow().is_none() {
            *self.rows_key.borrow_mut() = key;
            self.row_captions.borrow_mut().retain(|id, _| page == ManagerPage::Workspaces && view.rows.iter().any(|row| &row.id == id));
            let scroll = self
                .list
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>();
            let position = scroll.as_ref().map(|s| s.vadjustment().value());
            self.list.remove_all();
            self.rows.borrow_mut().clear();
            for item in &view.rows {
                let row = adw::ActionRow::builder()
                    .title(&item.title)
                    .subtitle(&item.subtitle)
                    .build();
                row.set_use_markup(false);
                row.set_title_lines(2);
                row.set_widget_name(&format!("workspace-row-{}", item.id));
                row.set_activatable(true);
                if page == ManagerPage::Workspaces {
                    self.workspace_row(w, &row, item, &pinned);
                }
                self.rows.borrow_mut().push(item.id.clone());
                self.list.append(&row);
            }
            if let (Some(scroll), Some(position)) = (scroll, position) {
                scroll.vadjustment().set_value(position);
            }
        }
        let index = view
            .selected
            .as_ref()
            .and_then(|id| self.rows.borrow().iter().position(|row| row == id));
        let row = index.and_then(|index| self.list.row_at_index(index as i32));
        if self.list.selected_row() != row {
            self.list.select_row(row.as_ref());
        }
        self.rebuilding.set(false);
    }
    fn workspace_row(
        &self,
        w: &Rc<Workspace>,
        row: &adw::ActionRow,
        item: &layer_workspace::WorkspaceRow,
        pinned: &[String],
    ) {
        row.add_css_class("workspace-manager-row");
        let handle = crate::icons::image("layer-grip-symbolic");
        handle.set_widget_name(&format!("workspace-reorder-handle-{}", item.id));
        handle.add_css_class("workspace-reorder-handle");
        handle.add_css_class("dim-label");
        handle.set_size_request(16, 44);
        handle.set_cursor_from_name(Some("grab"));
        handle.set_tooltip_text(Some(&w.localization().text(layer_ui::MessageId::WORKSPACE_HEADER_DRAG_ITEM)));
        handle.update_property(&[gtk::accessible::Property::Label(&w.localization().text(layer_ui::MessageId::WORKSPACE_HEADER_DRAG_ITEM))]);
        row.add_prefix(&handle);
        if item.current {
            row.add_suffix(&crate::icons::image("layer-check-symbolic"));
        }
        if pinned.contains(&item.id) {
            let pin = crate::icons::image("layer-pin-symbolic");
            pin.add_css_class("dim-label");
            pin.set_tooltip_text(Some(&w.localization().text(layer_ui::MessageId::NATIVE_HEADER_SHOWN_TOP)));
            pin.update_property(&[gtk::accessible::Property::Label(&w.localization().text(layer_ui::MessageId::NATIVE_HEADER_SHOWN_TOP))]);
            row.add_suffix(&pin);
        }
        let actions = item
            .actions
            .iter()
            .filter(|b| {
                b.enabled
                    && !b.primary
                    && matches!(
                        b.action,
                        ManagerAction::Rename(_) | ManagerAction::Delete(_)
                    )
            })
            .cloned()
            .collect();
        let mut captions = self.row_captions.borrow_mut();
        let caption = captions.entry(item.id.clone()).or_insert_with(|| (item.title.clone(), layer_ui::NativeCaption::OptionsFor { title: item.title.clone() }.message(&w.localization())));
        if caption.0 != item.title {
            *caption = (item.title.clone(), layer_ui::NativeCaption::OptionsFor { title: item.title.clone() }.message(&w.localization()));
        }
        let more = actions_menu(w, &caption.1, actions);
        self.add_switcher_actions(w, &more, &item.id, pinned);
        more.set_valign(gtk::Align::Center);
        row.add_suffix(&more);
        self.bind_reorder_row(w, row, &item.id, &more);
    }
    fn render_details(&self, w: &Rc<Workspace>, view: &WorkspaceView) {
        let key = serde_json::to_string(&(&view.details, view.busy, view.rows.is_empty()))
            .unwrap_or_default();
        if *self.details_key.borrow() == key {
            return;
        }
        *self.details_key.borrow_mut() = key;
        clear(&self.details);
        let Some(details) = &view.details else {
            if view.rows.is_empty() {
                self.details
                    .append(&gtk::Label::new(Some(&w.localization().text(layer_ui::MessageId::NATIVE_HEADER_NO_ITEMS))));
            }
            return;
        };
        let title = gtk::Label::new(Some(&details.title));
        title.add_css_class("title-2");
        title.set_xalign(0.);
        self.details.append(&title);
        let description = gtk::Label::new(Some(&details.description));
        description.set_xalign(0.);
        description.set_wrap(true);
        description.set_selectable(true);
        self.details.append(&description);
        let mut secondary = Vec::new();
        for button in details.actions.iter().cloned() {
            if !button.primary {
                secondary.push(button);
                continue;
            }
            let widget = action_button(w, &button);
            widget.set_sensitive(button.enabled && !view.busy);
            self.details.append(&widget);
        }
        if !secondary.is_empty() {
            let more = actions_menu(w, &w.localization().text(layer_ui::MessageId::COMMON_MORE), secondary);
            more.set_label(&w.localization().text(layer_ui::MessageId::COMMON_MORE));
            self.details.append(&more);
        }
    }
    fn render_prompt(&self, w: &Rc<Workspace>, view: &WorkspaceView) {
        let Some(prompt) = view.prompt.clone() else {
            *self.prompt_key.borrow_mut() = None;
            *self.draft.borrow_mut() = None;
            let dialog = self.prompt.borrow_mut().take();
            if let Some(dialog) = dialog {
                dialog.force_close();
            }
            return;
        };
        let key = serde_json::to_string(&(&view.prompt_action, prompt.name.is_some(), prompt.description.is_some(), prompt.choices.iter().map(|choice| &choice.id).collect::<Vec<_>>())).unwrap_or_default();
        if self.prompt_key.replace(Some(key.clone())).as_ref() != Some(&key) {
            *self.draft.borrow_mut() = None;
            let dialog = self.prompt.borrow_mut().take();
            if let Some(dialog) = dialog {
                dialog.force_close();
            }
        }
        if let Some(dialog) = self.prompt.borrow().as_ref() {
            dialog.set_heading(Some(&prompt.title));
            dialog.set_body(&prompt.message);
            dialog.set_response_label("confirm", &prompt.confirm);
            dialog.set_response_label("cancel", &w.localization().text(layer_ui::MessageId::COMMON_CANCEL));
            if let Some(localization) = unsafe { dialog.data::<Rc<RefCell<std::sync::Arc<layer_ui::Localizer>>>>("capy-prompt-localization") } {
                *unsafe { localization.as_ref() }.borrow_mut() = w.localization();
            }
            crate::text_language::visit(dialog.upcast_ref(), &mut |widget| {
                if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
                    let id = match entry.widget_name().as_str() {
                        "workspace-item-name" => Some(layer_ui::MessageId::COMMON_NAME),
                        "workspace-item-description" => Some(layer_ui::MessageId::COMMON_DESCRIPTION),
                        _ => None,
                    };
                    if let Some(id) = id { entry.set_placeholder_text(Some(&w.localization().text(id))); }
                }
                if widget.widget_name() == "workspace-item-choice-label" {
                    if let Some(label) = widget.downcast_ref::<gtk::Label>() { label.set_text(prompt.choice_label.as_deref().unwrap_or("")); }
                }
                if widget.widget_name() == "workspace-item-choice" {
                    if let Some(choice) = widget.downcast_ref::<gtk::DropDown>() {
                        let selected = choice.selected();
                        choice.set_model(Some(&gtk::StringList::new(&prompt.choices.iter().map(|choice| choice.label.as_str()).collect::<Vec<_>>())));
                        choice.set_selected(selected);
                    }
                }
            });
            return;
        }
        if view.busy { return; }
        let dialog = prompt_dialog(&prompt, self.draft.borrow().clone(), view.error.as_deref(), &w.localization());
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak]
                w,
                move |dialog, response| {
                    let ui = &w.workspaces.ui;
                    if ui.prompt.borrow().as_ref() != Some(dialog) {
                        return;
                    }
                    ui.prompt.borrow_mut().take();
                    if response != "confirm" {
                        w.workspaces.send(&w, WorkspaceInput::Cancel);
                        return;
                    }
                    let values = prompt_values(dialog, &ui.prompt_choices.borrow());
                    *ui.draft.borrow_mut() = Some(values.clone());
                    let (name, description, choice) = values;
                    w.workspaces.send(
                        &w,
                        WorkspaceInput::Submit {
                            name,
                            description: Some(description),
                            choice,
                        },
                    );
                }
            ),
        );
        *self.prompt_choices.borrow_mut() = prompt.choices.iter().map(|c| c.id.clone()).collect();
        *self.prompt.borrow_mut() = Some(dialog.clone());
        dialog.present(Some(&w.window));
    }
}
fn prompt_dialog(
    prompt: &ManagerPrompt,
    draft: Option<(String, String, Option<String>)>,
    error: Option<&str>,
    localization: &std::sync::Arc<layer_ui::Localizer>,
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(&prompt.title)
        .body(&prompt.message)
        .build();
    dialog.set_widget_name("workspace-prompt");
    dialog.add_responses(&[("cancel", &localization.text(layer_ui::MessageId::COMMON_CANCEL)), ("confirm", &prompt.confirm)]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("confirm"));
    dialog.set_response_appearance(
        "confirm",
        if prompt.destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    let form = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let (name, description, choice) = draft.unwrap_or_else(|| {
        (
            prompt.name.clone().unwrap_or_default(),
            prompt.description.clone().unwrap_or_default(),
            prompt.selected.clone(),
        )
    });
    let validation = gtk::Label::new(error);
    validation.set_wrap(true);
    validation.add_css_class("error");
    validation.set_visible(error.is_some());
    validation.set_widget_name("workspace-name-error");
    if prompt.name.is_some() {
        let entry = gtk::Entry::builder()
            .text(&name)
            .placeholder_text(&*localization.text(layer_ui::MessageId::COMMON_NAME))
            .activates_default(true)
            .build();
        entry.set_widget_name("workspace-item-name");
        crate::input::guard_entry_activation(&entry);
        let localization = Rc::new(RefCell::new(localization.clone()));
        unsafe { dialog.set_data("capy-prompt-localization", localization.clone()); }
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
                        .map_or_else(String::new, |reason| reason.localized_message(&localization.borrow())),
                );
                validation.set_visible(result.is_err());
            }
        ));
        dialog.set_response_enabled(
            "confirm",
            layer_workspace::validate_name(name.trim()).is_ok(),
        );
        form.append(&entry);
    }
    if prompt.description.is_some() {
        let entry = gtk::Entry::builder()
            .text(&description)
            .placeholder_text(&*localization.text(layer_ui::MessageId::COMMON_DESCRIPTION))
            .build();
        entry.set_widget_name("workspace-item-description");
        crate::input::guard_entry_activation(&entry);
        form.append(&entry);
    }
    if !prompt.choices.is_empty() {
        if let Some(text) = &prompt.choice_label {
            let label = gtk::Label::new(Some(text));
            label.set_widget_name("workspace-item-choice-label");
            label.set_xalign(0.);
            form.append(&label);
        }
        let labels: Vec<_> = prompt.choices.iter().map(|c| c.label.as_str()).collect();
        let select = gtk::DropDown::from_strings(&labels);
        select.set_selected(
            prompt
                .choices
                .iter()
                .position(|c| Some(&c.id) == choice.as_ref())
                .unwrap_or(0) as u32,
        );
        select.set_widget_name("workspace-item-choice");
        form.append(&select);
    }
    form.append(&validation);
    dialog.set_extra_child(Some(&form));
    dialog
}
fn prompt_values(
    dialog: &adw::AlertDialog,
    choices: &[String],
) -> (String, String, Option<String>) {
    let mut values = (String::new(), String::new(), None);
    let mut child = dialog.extra_child().and_then(|form| form.first_child());
    while let Some(widget) = child {
        match widget.widget_name().as_str() {
            "workspace-item-name" => {
                values.0 = widget
                    .downcast_ref::<gtk::Entry>()
                    .unwrap()
                    .text()
                    .trim()
                    .into()
            }
            "workspace-item-description" => {
                values.1 = widget.downcast_ref::<gtk::Entry>().unwrap().text().into()
            }
            "workspace-item-choice" => {
                let select = widget.downcast_ref::<gtk::DropDown>().unwrap();
                values.2 = choices.get(select.selected() as usize).cloned();
            }
            _ => {}
        }
        child = widget.next_sibling();
    }
    values
}
fn margins(widget: &impl IsA<gtk::Widget>, value: i32) {
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
    widget.set_margin_start(value);
    widget.set_margin_end(value);
}
fn actions_menu(w: &Rc<Workspace>, label: &str, actions: Vec<ManagerButton>) -> gtk::MenuButton {
    let menu = gtk::MenuButton::builder()
        .child(&crate::icons::image("layer-more-symbolic"))
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
                w.workspaces.send(
                    &w,
                    WorkspaceInput::Action {
                        action: item.action.clone(),
                    },
                );
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
fn action_button(w: &Rc<Workspace>, button: &ManagerButton) -> gtk::Button {
    let widget = gtk::Button::with_label(&button.label);
    if button.primary {
        widget.add_css_class("suggested-action");
    }
    if button.action.destructive() {
        widget.add_css_class("destructive-action");
    }
    let action = button.action.clone();
    widget.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| w.workspaces.send(
            &w,
            WorkspaceInput::Action {
                action: action.clone()
            }
        )
    ));
    widget
}
