//! Profile library management follows the workspace manager's native layout.
use super::*;

fn saved_profiles() -> Result<PathBuf, ColorFeatureError> {
    directory().ok_or_else(|| ColorFeatureError::Diagnostic("Saved profiles are unavailable".into()))
}

struct Manager {
    copy: RefCell<layer_ui::color_feature_copy::ProfileCopy>,
    localization: RefCell<std::sync::Arc<layer_ui::Localizer>>,
    w: std::rc::Weak<Workspace>,
    window: glib::WeakRef<adw::ApplicationWindow>,
    dialog: glib::WeakRef<adw::Dialog>,
    list: glib::WeakRef<gtk::ListBox>,
    search: glib::WeakRef<gtk::SearchEntry>,
    add: glib::WeakRef<gtk::Button>,
    note: glib::WeakRef<gtk::Label>,
    entries: RefCell<Vec<Entry>>,
    rows: RefCell<Vec<(PathBuf, ProfileRow)>>,
    busy: Cell<bool>,
    failure: RefCell<Option<ColorFeatureError>>,
}

struct ProfileRow {
    row: adw::ActionRow,
    pin: Option<gtk::Widget>,
    more: gtk::MenuButton,
    visibility: gio::Menu,
    removal: gio::Menu,
}

impl ProfileRow {
    fn refresh(&self, entry: &Entry, localization: &layer_ui::Localizer, copy: &layer_ui::color_feature_copy::ProfileCopy) {
        let name = entry.display_name(localization);
        self.row.set_title(&name); self.row.set_subtitle(&entry.description(localization));
        if let Some(pin) = &self.pin {
            pin.set_tooltip_text(Some(&copy.shown)); pin.update_property(&[gtk::accessible::Property::Label(&copy.shown)]);
        }
        self.visibility.remove_all(); self.visibility.append(Some(&copy.show), Some("saved.show"));
        self.removal.remove_all(); self.removal.append(Some(&copy.remove), Some("saved.remove"));
        self.more.set_tooltip_text(Some(&layer_ui::color_feature_copy::named(localization, layer_ui::MessageId::COLOR_FEATURES_PROFILE_OPTIONS, &name)));
    }
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
        let query = search.text();
        let entries = self.entries.borrow();
        search.set_visible(entries.len() > 7 || !query.is_empty());
        let mut rows = self.rows.borrow_mut();
        let mut visible = Vec::new();
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.matches(&query, &self.localization.borrow()))
        {
            if let Some((_, row)) = rows.iter().find(|(path, _)| path == &entry.path) {
                row.refresh(entry, &self.localization.borrow(), &self.copy.borrow());
                visible.push(row.row.clone());
                continue;
            }
            let row = adw::ActionRow::builder().use_markup(false).build();
            let pin = if entry.visible {
                let pin = crate::icons::image("layer-pin-symbolic");
                pin.add_css_class("dim-label");
                pin.set_tooltip_text(Some(self.copy.borrow().shown.as_ref()));
                pin.update_property(&[gtk::accessible::Property::Label(self.copy.borrow().shown.as_ref())]);
                row.add_suffix(&pin);
                Some(pin.upcast::<gtk::Widget>())
            } else { None };
            let menu = gio::Menu::new();
            let visibility = gio::Menu::new();
            visibility.append(Some(self.copy.borrow().show.as_ref()), Some("saved.show"));
            menu.append_section(None, &visibility);
            let removal = gio::Menu::new();
            removal.append(Some(self.copy.borrow().remove.as_ref()), Some("saved.remove"));
            menu.append_section(None, &removal);
            let popup = gtk::PopoverMenu::from_model(Some(&menu));
            let actions = gio::SimpleActionGroup::new();
            let show = gio::SimpleAction::new_stateful("show", None, &entry.visible.to_variant());
            let remove = gio::SimpleAction::new("remove", None);
            let owner = Rc::downgrade(self);
            for (action, removing) in [(&show, false), (&remove, true)] {
                action.connect_activate(glib::clone!(
                    #[strong]
                    owner,
                    #[weak]
                    popup,
                    #[strong(rename_to = path)]
                    entry.path,
                    #[strong(rename_to = visible)]
                    entry.visible,
                    move |_, _| {
                        let Some(state) = owner.upgrade() else { return };
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
                .tooltip_text(layer_ui::color_feature_copy::named(&self.localization.borrow(), layer_ui::MessageId::COLOR_FEATURES_PROFILE_OPTIONS, &entry.display_name(&self.localization.borrow())))
                .valign(gtk::Align::Center)
                .popover(&popup)
                .build();
            more.add_css_class("flat");
            more.set_widget_name(&format!("profile-library-menu-{index}"));
            row.add_suffix(&more);
            let projected = ProfileRow { row: row.clone(), pin, more, visibility, removal };
            projected.refresh(entry, &self.localization.borrow(), &self.copy.borrow());
            rows.push((entry.path.clone(), projected)); visible.push(row);
            if let Some(w) = self.w.upgrade() {
                w.watch_popover(popup.upcast_ref());
            }
        }
        let mut child = list.first_child();
        let unchanged = visible.iter().all(|row| {
            let same = child.as_ref() == Some(row.upcast_ref());
            child = child.as_ref().and_then(|child| child.next_sibling());
            same
        }) && child.is_none();
        if !unchanged { list.remove_all(); for row in visible { list.append(&row); } }
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
        self.failure.borrow_mut().take();
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
                        gio::spawn_blocking(move || remove(&saved_profiles()?, &path))
                            .await
                            .map_err(|_| ColorFeatureError::Diagnostic("Could not remove the profile".into()))
                            .and_then(|r| r)
                            .map(Some)
                    }
                    Operation::Show(path, visible) => {
                        gio::spawn_blocking(move || set_visible(&saved_profiles()?, &path, visible))
                            .await
                            .map_err(|_| ColorFeatureError::Diagnostic("Could not update the profile".into()))
                            .and_then(|r| r)
                            .map(Some)
                    }
                };
                match result {
                    Ok(Some(entries)) => {
                        *state.entries.borrow_mut() = entries;
                        state.rows.borrow_mut().clear();
                        state.render();
                    }
                    Ok(None) => (),
                    Err(message) => {
                        *state.failure.borrow_mut() = Some(message.clone());
                        if let Some(note) = state.note.upgrade() {
                            note.set_text(&message.profile_message(&state.localization.borrow()));
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
        filter.set_name(Some(self.copy.borrow().filter.as_ref()));
        filter.add_suffix("icc");
        filter.add_suffix("icm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(self.copy.borrow().add_profile.as_ref())
            .accept_label(self.copy.borrow().add.as_ref())
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        {
            let initial = self.localization.borrow().clone();
            let weak = chooser.downgrade(); let filter = filter.downgrade();
            crate::on_window_localization(&window, self.w.upgrade().as_ref(), &initial, move |localization| {
                let Some(chooser) = weak.upgrade() else { return false };
                let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
                chooser.set_title(&copy.add_profile); chooser.set_accept_label(Some(&copy.add));
                if let Some(filter) = filter.upgrade() { filter.set_name(Some(&copy.filter)); }
                true
            });
        }
        match crate::files::chooser::open(
            &chooser,
            &window,
            crate::files::chooser::Folder::Profiles,
        )
        .await
        {
            Ok(file) => {
                let path = file.path().ok_or(ColorFeatureError::ProfileChooseFile)?;
                gio::spawn_blocking(move || import(&saved_profiles()?, &path))
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
    manage_for_window(&w.window, Some(w), w.localization()).await
}

pub(crate) async fn manage_for_window(
    window: &adw::ApplicationWindow,
    workspace: Option<&Rc<Workspace>>,
    localization: std::sync::Arc<layer_ui::Localizer>,
) -> Result<(), String> {
    let copy = layer_ui::color_feature_copy::ProfileCopy::new(&localization);
    let entries = gio::spawn_blocking(|| list(&saved_profiles()?))
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
    crate::input::guard_editable_activation(&search);
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
        copy: RefCell::new(copy), localization: RefCell::new(localization.clone()),
        w: workspace.map_or_else(std::rc::Weak::new, Rc::downgrade),
        window: window.downgrade(),
        dialog: dialog.downgrade(),
        list: list.downgrade(),
        search: search.downgrade(),
        add: add.downgrade(),
        note: note.downgrade(),
        entries: RefCell::new(entries), rows: RefCell::default(),
        busy: Cell::new(false),
        failure: RefCell::new(None),
    });
    {
        let weak = Rc::downgrade(&state);
        let intro = intro.downgrade(); let empty = empty.downgrade(); let done = done.downgrade();
        crate::on_window_localization(window, workspace, &localization, move |localization| {
            let Some(state) = weak.upgrade() else { return false };
            let Some(dialog) = state.dialog.upgrade() else { return false };
            let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
            dialog.set_title(&copy.manage_title);
            if let Some(add) = state.add.upgrade() {
                add.set_tooltip_text(Some(&copy.add_profile));
                add.update_property(&[gtk::accessible::Property::Label(&copy.add_profile)]);
            }
            if let Some(search) = state.search.upgrade() { search.set_placeholder_text(Some(&copy.search)); search.update_property(&[gtk::accessible::Property::Label(&copy.search)]); }
            if let Some(intro) = intro.upgrade() { intro.set_label(&copy.menu_help); }
            if let Some(empty) = empty.upgrade() { empty.set_label(&copy.empty); }
            if let Some(done) = done.upgrade() { done.set_label(&copy.common.done); }
            *state.copy.borrow_mut() = copy;
            *state.localization.borrow_mut() = localization.clone();
            state.render();
            if let Some(note) = state.note.upgrade() { if let Some(reason) = state.failure.borrow().as_ref() { note.set_text(&reason.profile_message(localization)); } }
            true
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "private display with retained profile library controls"]
    fn native_profile_library_live_language() {
        let (app, active) = crate::application("art.capycanvas.ProfileLibraryLanguages");
        app.register(None::<&gio::Cancellable>).unwrap();
        let directory = saved_profiles().unwrap(); assert!(directory.starts_with(std::env::var_os(layer_host::storage::STORAGE_OVERRIDE).unwrap()));
        std::fs::create_dir_all(&directory).unwrap();
        let id = "0".repeat(64); let path = directory.join(format!("{id}.icc"));
        let literal = "tiếng ไทย Русский {profile} 🎨";
        std::fs::write(&path, b"invalid ICC literal fixture").unwrap();
        std::fs::write(path.with_extension("name"), literal).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !predicate() {
                assert!(std::time::Instant::now() < deadline, "native library ready");
                while glib::MainContext::default().pending() { glib::MainContext::default().iteration(false); }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let pump = |duration| { let start = std::time::Instant::now(); while start.elapsed() < std::time::Duration::from_millis(duration) { while glib::MainContext::default().pending() { glib::MainContext::default().iteration(false); } std::thread::sleep(std::time::Duration::from_millis(5)); } };
        let request = |language| {
            let settings = layer_ui::Settings { language: layer_ui::LanguagePreference::Explicit(language), ..Default::default() };
            app.activate_action("settings-changed", Some(&serde_json::to_string(&settings).unwrap().to_variant()));
        };
        for theme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            app.style_manager().set_color_scheme(theme); request(layer_ui::UiLanguage::English);
            let initial = glib::MainContext::default().block_on(crate::prepare_application_context(&app, &active)).1;
            let window = adw::ApplicationWindow::builder().application(&app).default_width(640).default_height(700).build();
            window.present();
            glib::MainContext::default().block_on(manage_for_window(&window, None, initial.clone())).unwrap();
            let dialog = window.visible_dialog().unwrap();
            let mut controls = Vec::new(); crate::text_language::visit(dialog.upcast_ref(), &mut |widget| controls.push(widget.clone()));
            let search = controls.iter().find_map(|widget| widget.downcast_ref::<gtk::SearchEntry>()).unwrap().clone();
            let list = controls.iter().find_map(|widget| widget.downcast_ref::<gtk::ListBox>()).unwrap().clone();
            search.set_text("000"); search.select_region(1, 3); search.grab_focus();
            wait(&|| list.first_child().is_some());
            let row = list.first_child().unwrap().downcast::<adw::ActionRow>().unwrap();
            let more = controls.iter().find_map(|widget| widget.downcast_ref::<gtk::MenuButton>()).unwrap().clone();
            let popup = more.popover().unwrap();
            let selection = search.selection_bounds();
            for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                let old = crate::window_localization(&window, None, &initial).language();
                more.popup(); wait(&|| popup.is_mapped()); request(language); pump(100);
                assert_eq!(crate::window_localization(&window, None, &initial).language(), old, "native popup holds publication");
                more.popdown(); wait(&|| crate::window_localization(&window, None, &initial).language() == language);
                assert_eq!(list.first_child().as_ref(), Some(row.upcast_ref())); assert_eq!(more.popover().as_ref(), Some(&popup));
                let localization = crate::window_localization(&window, None, &initial);
                assert_eq!(row.title(), layer_ui::color_feature_copy::profile_unavailable(&localization, &id[..12]));
                assert_eq!(search.text(), "000"); assert_eq!(search.selection_bounds(), selection);
                search.grab_focus(); assert!(search.has_focus() || search.focus_child().is_some());
            }
            assert_eq!(std::fs::read(&path).unwrap(), bytes); assert_eq!(std::fs::read_to_string(path.with_extension("name")).unwrap(), literal);
            eprintln!("GTK library {:?}: all fifteen row/button/popover identities, raw profiles, search draft/selection and menu deferral passed", theme);
            dialog.force_close(); window.destroy();
        }
    }
}
