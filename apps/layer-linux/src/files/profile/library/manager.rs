//! Profile library management follows the workspace manager's native layout.
use super::*;

struct Manager {
    copy: layer_ui::color_feature_copy::ProfileCopy,
    localization: std::sync::Arc<layer_ui::Localizer>,
    w: std::rc::Weak<Workspace>,
    window: glib::WeakRef<adw::ApplicationWindow>,
    dialog: glib::WeakRef<adw::Dialog>,
    list: glib::WeakRef<gtk::ListBox>,
    search: glib::WeakRef<gtk::SearchEntry>,
    add: glib::WeakRef<gtk::Button>,
    note: glib::WeakRef<gtk::Label>,
    entries: RefCell<Vec<Entry>>,
    busy: Cell<bool>,
}

enum Operation {
    Add,
    Remove(PathBuf),
    Show(PathBuf, bool),
}

impl Manager {
    fn render(self: &Rc<Self>) {
        let (Some(list), Some(search)) = (self.list.upgrade(), self.search.upgrade()) else {
            return;
        };
        let query = search.text().to_lowercase();
        let entries = self.entries.borrow();
        search.set_visible(entries.len() > 7 || !query.is_empty());
        list.remove_all();
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name.to_lowercase().contains(&query))
        {
            let row = adw::ActionRow::builder()
                .title(entry.display_name(&self.localization))
                .subtitle(entry.description(&self.localization))
                .use_markup(false)
                .build();
            if entry.visible {
                let pin = crate::icons::image("layer-pin-symbolic");
                pin.add_css_class("dim-label");
                pin.set_tooltip_text(Some(self.copy.shown.as_ref()));
                pin.update_property(&[gtk::accessible::Property::Label(self.copy.shown.as_ref())]);
                row.add_suffix(&pin);
            }
            let menu = gio::Menu::new();
            let visibility = gio::Menu::new();
            visibility.append(Some(self.copy.show.as_ref()), Some("saved.show"));
            menu.append_section(None, &visibility);
            let removal = gio::Menu::new();
            removal.append(Some(self.copy.remove.as_ref()), Some("saved.remove"));
            menu.append_section(None, &removal);
            let popup = gtk::PopoverMenu::from_model(Some(&menu));
            let actions = gio::SimpleActionGroup::new();
            let show = gio::SimpleAction::new_stateful("show", None, &entry.visible.to_variant());
            let remove = gio::SimpleAction::new("remove", None);
            for (action, removing) in [(&show, false), (&remove, true)] {
                action.connect_activate(glib::clone!(
                    #[strong(rename_to = state)]
                    self,
                    #[weak]
                    popup,
                    #[strong(rename_to = path)]
                    entry.path,
                    #[strong(rename_to = visible)]
                    entry.visible,
                    move |_, _| {
                        popup.popdown();
                        state.run(if removing {
                            Operation::Remove(path.clone())
                        } else {
                            Operation::Show(path.clone(), !visible)
                        });
                    }
                ));
                actions.add_action(action);
            }
            popup.insert_action_group("saved", Some(&actions));
            let more = gtk::MenuButton::builder()
                .child(&crate::icons::image("layer-more-symbolic"))
                .tooltip_text(layer_ui::color_feature_copy::named(&self.localization, layer_ui::MessageId::COLOR_FEATURES_PROFILE_OPTIONS, &entry.display_name(&self.localization)))
                .valign(gtk::Align::Center)
                .popover(&popup)
                .build();
            more.add_css_class("flat");
            more.set_widget_name(&format!("profile-library-menu-{index}"));
            row.add_suffix(&more);
            list.append(&row);
            if let Some(w) = self.w.upgrade() {
                w.watch_popover(popup.upcast_ref());
            }
        }
    }

    fn run(self: &Rc<Self>, operation: Operation) {
        if self.busy.replace(true) {
            return;
        }
        if let Some(add) = self.add.upgrade() {
            add.set_sensitive(false);
        }
        if let Some(list) = self.list.upgrade() {
            list.set_sensitive(false);
        }
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.set_can_close(false);
        }
        if let Some(note) = self.note.upgrade() {
            note.set_visible(false);
        }
        glib::MainContext::default().spawn_local(glib::clone!(
            #[strong(rename_to = state)]
            self,
            async move {
                let result = match operation {
                    Operation::Add => state.add().await,
                    Operation::Remove(path) => {
                        gio::spawn_blocking(move || remove(&directory(), &path))
                            .await
                            .map_err(|_| ColorFeatureError::Diagnostic("Could not remove the profile".into()))
                            .and_then(|r| r)
                            .map(Some)
                    }
                    Operation::Show(path, visible) => {
                        gio::spawn_blocking(move || set_visible(&directory(), &path, visible))
                            .await
                            .map_err(|_| ColorFeatureError::Diagnostic("Could not update the profile".into()))
                            .and_then(|r| r)
                            .map(Some)
                    }
                };
                match result {
                    Ok(Some(entries)) => {
                        *state.entries.borrow_mut() = entries;
                        state.render();
                    }
                    Ok(None) => (),
                    Err(message) => {
                        if let Some(note) = state.note.upgrade() {
                            note.set_text(&message.profile_message(&state.localization));
                            note.set_visible(true);
                        }
                    }
                }
                state.busy.set(false);
                if let Some(add) = state.add.upgrade() {
                    add.set_sensitive(true);
                }
                if let Some(list) = state.list.upgrade() {
                    list.set_sensitive(true);
                }
                if let Some(dialog) = state.dialog.upgrade() {
                    dialog.set_can_close(true);
                }
            }
        ));
    }

    async fn add(&self) -> Result<Option<Vec<Entry>>, ColorFeatureError> {
        let Some(window) = self.window.upgrade() else {
            return Ok(None);
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(self.copy.filter.as_ref()));
        filter.add_suffix("icc");
        filter.add_suffix("icm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(self.copy.add_profile.as_ref())
            .accept_label(self.copy.add.as_ref())
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        match crate::files::chooser::open(
            &chooser,
            &window,
            crate::files::chooser::Folder::Profiles,
        )
        .await
        {
            Ok(file) => {
                let path = file.path().ok_or(ColorFeatureError::ProfileChooseFile)?;
                gio::spawn_blocking(move || import(&directory(), &path))
                    .await
                    .map_err(|_| ColorFeatureError::Diagnostic("Could not add the profile".into()))
                    .and_then(|r| r)
                    .map(Some)
            }
            Err(e)
                if e.matches(gtk::DialogError::Dismissed)
                    || e.matches(gtk::DialogError::Cancelled) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.to_string().into()),
        }
    }
}

pub(crate) async fn manage(w: &Rc<Workspace>) -> Result<(), String> {
    manage_for_window(&w.window, Some(w)).await
}

pub(crate) async fn manage_for_window(
    window: &adw::ApplicationWindow,
    workspace: Option<&Rc<Workspace>>,
) -> Result<(), String> {
    let localization = workspace.map(|w| w.localization.clone()).unwrap_or_else(|| crate::launch_localization().clone());
    let copy = layer_ui::color_feature_copy::ProfileCopy::new(&localization);
    let entries = gio::spawn_blocking(|| list(&directory()))
        .await
        .map_err(|_| ColorFeatureError::Diagnostic("Could not load saved profiles".into()).profile_message(&localization))?.map_err(|reason|reason.profile_message(&localization))?;
    let dialog = adw::Dialog::builder()
        .title(copy.manage_title.as_ref())
        .content_width(480)
        .content_height(500)
        .build();
    dialog.set_widget_name("profile-library-manager");
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let add = crate::icons::button("layer-plus-symbolic");
    add.set_tooltip_text(Some(copy.add_profile.as_ref()));
    add.update_property(&[gtk::accessible::Property::Label(copy.add_profile.as_ref())]);
    add.set_widget_name("profile-library-import");
    header.pack_end(&add);
    view.add_top_bar(&header);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    let intro = gtk::Label::builder()
        .label(copy.menu_help.as_ref())
        .xalign(0.)
        .wrap(true)
        .build();
    body.append(&intro);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(copy.search.as_ref()));
    search.set_widget_name("profile-library-search");
    body.append(&search);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    list.set_widget_name("profile-library-list");
    let empty = gtk::Label::builder()
        .label(copy.empty.as_ref())
        .margin_top(18)
        .margin_bottom(18)
        .build();
    list.set_placeholder(Some(&empty));
    body.append(
        &crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build()),
    );
    let note = gtk::Label::builder()
        .xalign(0.)
        .wrap(true)
        .visible(false)
        .build();
    note.add_css_class("error");
    note.set_widget_name("profile-library-status");
    body.append(&note);
    let done = gtk::Button::with_label(copy.common.done.as_ref());
    done.set_widget_name("profile-library-done");
    dialog
        .bind_property("can-close", &done, "sensitive")
        .sync_create()
        .build();
    done.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));
    body.append(&done);
    view.set_content(Some(&body));
    dialog.set_child(Some(&view));
    let state = Rc::new(Manager {
        copy, localization,
        w: workspace.map_or_else(std::rc::Weak::new, Rc::downgrade),
        window: window.downgrade(),
        dialog: dialog.downgrade(),
        list: list.downgrade(),
        search: search.downgrade(),
        add: add.downgrade(),
        note: note.downgrade(),
        entries: RefCell::new(entries),
        busy: Cell::new(false),
    });
    add.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.run(Operation::Add)
    ));
    search.connect_search_changed(glib::clone!(
        #[strong]
        state,
        move |_| state.render()
    ));
    state.render();
    dialog.present(Some(window));
    Ok(())
}
