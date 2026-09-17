//! One native picker for saved, embedded and standard profiles in every role.
use super::*;

pub(crate) struct ProfileChooser {
    pub row: adw::ActionRow,
    pub error: gtk::Label,
    pub selected: Rc<dyn Fn() -> Result<ExportProfile, String>>,
    pub restore: Rc<dyn Fn(ExportProfile)>,
    state: Rc<State>,
}

struct State {
    w: std::rc::Weak<Workspace>,
    window: glib::WeakRef<adw::ApplicationWindow>,
    row: glib::WeakRef<adw::ActionRow>,
    error: glib::WeakRef<gtk::Label>,
    menu: glib::WeakRef<gtk::MenuButton>,
    value: RefCell<Option<ExportProfile>>,
    document: RefCell<Option<ExportProfile>>,
    failure: RefCell<Option<String>>,
    busy: Cell<bool>,
    in_flight: Cell<bool>,
    listing: Cell<bool>,
    generation: Cell<u64>,
    working: RgbSpace,
    purpose: ProfilePurpose,
}

impl State {
    fn notify(&self) {
        let Some(row) = self.row.upgrade() else {
            return;
        };
        let subtitle = if self.busy.get() {
            "Reading profile…".into()
        } else {
            self.value
                .borrow()
                .as_ref()
                .map_or_else(|| "Choose a profile…".into(), |p| p.name.clone())
        };
        // Callers observe subtitle notifications for validation and previews.
        // Freeze ensures a repeated choice still publishes exactly once.
        let _guard = row.freeze_notify();
        row.set_subtitle(&subtitle);
        row.notify("subtitle");
        if let Some(error) = self.error.upgrade() {
            error.set_label(self.failure.borrow().as_deref().unwrap_or(""));
            error.set_visible(self.failure.borrow().is_some());
        }
        if let Some(menu) = self.menu.upgrade() {
            menu.set_sensitive(!self.in_flight.get());
        }
    }
    fn set(&self, value: ExportProfile) {
        self.generation.set(self.generation.get().wrapping_add(1));
        *self.value.borrow_mut() = Some(value);
        self.failure.borrow_mut().take();
        self.busy.set(false);
        self.notify();
    }
    fn compatible(&self, channels: ProfileChannels) -> bool {
        use layer_core::color::source::SourceChannels;
        match &self.purpose {
            ProfilePurpose::Output => true,
            ProfilePurpose::Proof => channels != ProfileChannels::Gray,
            ProfilePurpose::Source(source) => {
                channels
                    == match source.channels {
                        SourceChannels::Rgb | SourceChannels::Rgba => ProfileChannels::Rgb,
                        SourceChannels::Gray | SourceChannels::GrayAlpha => ProfileChannels::Gray,
                        SourceChannels::Cmyk => ProfileChannels::Cmyk,
                    }
            }
        }
    }
    fn close(&self) {
        if let Some(menu) = self.menu.upgrade() {
            menu.popdown();
        }
    }
    fn choose(self: &Rc<Self>, value: Option<ExportProfile>, path: Option<std::path::PathBuf>) {
        self.close();
        if self.in_flight.replace(true) {
            return;
        }
        self.busy.set(true);
        self.failure.borrow_mut().take();
        self.notify();
        let state = self.clone();
        let generation = self.generation.get();
        glib::MainContext::default().spawn_local(async move {
            let result = state.load(value, path).await;
            state.in_flight.set(false);
            state.busy.set(false);
            // Selecting another export preset while a read completes must keep
            // that preset's profile. A single worker remains in flight.
            if state.generation.get() != generation {
                if let Some(menu) = state.menu.upgrade() {
                    menu.set_sensitive(true);
                }
                return;
            }
            match result {
                Ok(Some(profile)) => {
                    state.set(profile);
                    return;
                }
                Ok(None) => (),
                Err(error) => {
                    *state.failure.borrow_mut() = Some(error);
                }
            }
            state.notify();
        });
    }
    async fn load(
        &self,
        value: Option<ExportProfile>,
        path: Option<std::path::PathBuf>,
    ) -> Result<Option<ExportProfile>, String> {
        let purpose = self.purpose.clone();
        let working = self.working;
        if let Some(value) = value {
            return gio::spawn_blocking(move || {
                purpose.validate(&value, working)?;
                Ok(Some(value))
            })
            .await
            .map_err(|_| "Profile reader failed")?;
        }
        if let Some(path) = path {
            return gio::spawn_blocking(move || {
                library::read_entry(&path, working, &purpose).map(Some)
            })
            .await
            .map_err(|_| "Profile reader failed")?;
        }
        let Some(window) = self.window.upgrade() else {
            return Ok(None);
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("ICC color profiles"));
        filter.add_suffix("icc");
        filter.add_suffix("icm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Add Profile")
            .accept_label("Add")
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let file = match super::super::chooser::open(
            &dialog,
            &window,
            super::super::chooser::Folder::Profiles,
        )
        .await
        {
            Ok(file) => file,
            Err(e)
                if e.matches(gtk::DialogError::Dismissed)
                    || e.matches(gtk::DialogError::Cancelled) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e.to_string()),
        };
        let path = file.path().ok_or("Choose a local ICC profile file")?;
        gio::spawn_blocking(move || {
            let value = super::read(&path, working, &purpose)?;
            let ColorProfile::Icc(bytes) = &value.profile else {
                unreachable!();
            };
            library::store(&library::directory(), bytes, &value.name)?;
            Ok(Some(value))
        })
        .await
        .map_err(|_| "Profile import worker failed")?
    }
}

fn item(
    section: &gio::Menu,
    actions: &gio::SimpleActionGroup,
    label: &str,
    name: &str,
    activate: impl Fn() + 'static,
) {
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, _| activate());
    actions.add_action(&action);
    // Profile names are literal text, not menu mnemonics.
    section.append(
        Some(&label.replace('_', "__")),
        Some(&format!("profile.{name}")),
    );
}

impl ProfileChooser {
    pub fn restore_document(&self, profile: ExportProfile) {
        *self.state.document.borrow_mut() =
            matches!(profile.profile, ColorProfile::Icc(_)).then(|| profile.clone());
        self.state.set(profile);
    }

    pub fn new(
        w: &Rc<Workspace>,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
    ) -> Self {
        Self::build(&w.window, Some(w), title, name, working, purpose)
    }

    // Application file opens can ask for a source profile before a canvas exists.
    pub fn for_window(
        window: &adw::ApplicationWindow,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
    ) -> Self {
        Self::build(window, None, title, name, working, purpose)
    }

    fn build(
        window: &adw::ApplicationWindow,
        workspace: Option<&Rc<Workspace>>,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
    ) -> Self {
        let prefix = match purpose {
            ProfilePurpose::Proof => "proof",
            ProfilePurpose::Output => "export",
            ProfilePurpose::Source(_) => "source",
        };
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle("Choose a profile…")
            .use_markup(false)
            .build();
        row.set_widget_name(name);
        let menu = gtk::MenuButton::builder()
            .icon_name("pan-down-symbolic")
            .valign(gtk::Align::Center)
            .build();
        menu.set_widget_name(&format!("{prefix}-profile-choose"));
        menu.set_tooltip_text(Some("Choose or add a profile"));
        row.add_suffix(&menu);
        row.set_activatable_widget(Some(&menu));
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.)
            .visible(false)
            .build();
        error.add_css_class("error");
        error.set_widget_name(&format!("{prefix}-profile-error"));
        let state = Rc::new(State {
            w: workspace.map_or_else(std::rc::Weak::new, Rc::downgrade),
            window: window.downgrade(),
            row: row.downgrade(),
            error: error.downgrade(),
            menu: menu.downgrade(),
            value: Default::default(),
            document: Default::default(),
            failure: Default::default(),
            busy: Cell::new(false),
            in_flight: Cell::new(false),
            listing: Cell::new(false),
            generation: Cell::new(0),
            working,
            purpose,
        });
        let model = gio::Menu::new();
        let choices = gio::Menu::new();
        let footer = gio::Menu::new();
        let actions = gio::SimpleActionGroup::new();
        model.append_section(None, &choices);
        model.append_section(None, &footer);
        item(
            &footer,
            &actions,
            "Add Profile…",
            "add",
            glib::clone!(
                #[strong]
                state,
                move || state.choose(None, None)
            ),
        );
        item(
            &footer,
            &actions,
            "Manage Profiles…",
            "manage",
            glib::clone!(
                #[strong]
                state,
                move || {
                    state.close();
                    if let Some(window) = state.window.upgrade() {
                        glib::MainContext::default().spawn_local(glib::clone!(
                            #[strong]
                            state,
                            async move {
                                if let Err(error) =
                                    library::manage_for_window(&window, state.w.upgrade().as_ref())
                                        .await
                                {
                                    *state.failure.borrow_mut() = Some(error);
                                    state.notify();
                                }
                            }
                        ));
                    }
                }
            ),
        );
        let popover = gtk::PopoverMenu::from_model(Some(&model));
        if let Some(w) = workspace {
            w.watch_popover(popover.upcast_ref());
        }
        popover.insert_action_group("profile", Some(&actions));
        menu.set_popover(Some(&popover));
        popover.connect_show(glib::clone!(
            #[strong]
            state,
            #[strong]
            choices,
            #[strong]
            actions,
            move |_| {
                if state.listing.replace(true) {
                    return;
                }
                choices.remove_all();
                for name in actions.list_actions() {
                    if name != "add" && name != "manage" {
                        actions.remove_action(&name);
                    }
                }
                choices.append(Some("Loading profiles…"), None);
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[strong]
                    state,
                    #[strong]
                    choices,
                    #[strong]
                    actions,
                    async move {
                        let current = state.value.borrow().clone();
                        let document = state.document.borrow().clone();
                        let (entries, document, current) = gio::spawn_blocking(move || {
                            let mut entries = library::list(&library::directory());
                            if let Some(ExportProfile {
                                profile: ColorProfile::Icc(bytes),
                                ..
                            }) = &document
                            {
                                let key = glib::compute_checksum_for_data(
                                    glib::ChecksumType::Sha256,
                                    bytes,
                                )
                                .unwrap();
                                if let Ok(entries) = &mut entries {
                                    entries.retain(|entry| {
                                        entry.path.file_stem().and_then(|s| s.to_str())
                                            != Some(key.as_str())
                                    });
                                }
                            }
                            let current = current.filter(|p| {
                                if document.as_ref().is_some_and(|d| d.profile == p.profile) {
                                    return false;
                                }
                                let ColorProfile::Icc(bytes) = &p.profile else {
                                    return false;
                                };
                                let key = glib::compute_checksum_for_data(
                                    glib::ChecksumType::Sha256,
                                    bytes,
                                )
                                .unwrap();
                                !entries.as_ref().is_ok_and(|entries| {
                                    entries.iter().any(|entry| {
                                        entry.issue.is_none()
                                            && entry.path.file_stem().and_then(|s| s.to_str())
                                                == Some(key.as_str())
                                    })
                                })
                            });
                            (entries, document, current)
                        })
                        .await
                        .unwrap_or_else(|_| {
                            (Err("Could not load saved profiles".into()), None, None)
                        });
                        state.listing.set(false);
                        choices.remove_all();
                        for (profile, label, action) in [
                            (document, "Document Profile", "document"),
                            (current, "Current Profile", "current"),
                        ] {
                            let Some(profile) = profile else { continue };
                            let section = gio::Menu::new();
                            let title = profile.name.clone();
                            item(
                                &section,
                                &actions,
                                &title,
                                action,
                                glib::clone!(
                                    #[strong]
                                    state,
                                    move || state.choose(Some(profile.clone()), None)
                                ),
                            );
                            choices.append_section(Some(label), &section);
                        }
                        match entries {
                            Ok(entries) => {
                                let saved = gio::Menu::new();
                                for (index, entry) in entries
                                    .into_iter()
                                    .filter(|e| {
                                        e.visible
                                            && e.issue.is_none()
                                            && e.channels.is_some_and(|c| state.compatible(c))
                                    })
                                    .enumerate()
                                {
                                    item(
                                        &saved,
                                        &actions,
                                        &entry.name,
                                        &format!("saved-{index}"),
                                        glib::clone!(
                                            #[strong]
                                            state,
                                            move || state.choose(None, Some(entry.path.clone()))
                                        ),
                                    );
                                }
                                if saved.n_items() > 0 {
                                    choices.append_section(Some("Saved Profiles"), &saved);
                                }
                            }
                            Err(error) => choices.append(Some(&error), None),
                        }
                        if state.compatible(ProfileChannels::Rgb) {
                            let standard = gio::Menu::new();
                            for (index, space) in RgbSpace::ALL.into_iter().enumerate() {
                                let value = ExportProfile::builtin(space);
                                let title = value.name.clone();
                                item(
                                    &standard,
                                    &actions,
                                    &title,
                                    &format!("builtin-{index}"),
                                    glib::clone!(
                                        #[strong]
                                        state,
                                        move || state.choose(Some(value.clone()), None)
                                    ),
                                );
                            }
                            choices.append_section(Some("Standard Color Spaces"), &standard);
                        }
                    }
                ));
            }
        ));
        let selected = Rc::new({
            let state = state.clone();
            move || {
                if state.busy.get() {
                    return Err("Reading profile…".into());
                }
                if let Some(error) = state.failure.borrow().clone() {
                    return Err(error);
                }
                state
                    .value
                    .borrow()
                    .clone()
                    .ok_or_else(|| "Choose a profile".into())
            }
        });
        let restore = Rc::new({
            let state = state.clone();
            move |value| state.set(value)
        });
        Self {
            row,
            error,
            selected,
            restore,
            state,
        }
    }
}
