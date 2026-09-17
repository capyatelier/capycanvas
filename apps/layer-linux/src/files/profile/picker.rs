//! One native picker for saved, embedded and standard profiles in every role.
use super::*;

pub(crate) struct ProfileChooser {
    pub row: adw::ActionRow,
    pub error: gtk::Label,
    pub selected: Rc<dyn Fn() -> Result<ExportProfile, String>>,
    pub restore: Rc<dyn Fn(ExportProfile)>,
}

struct State {
    w: std::rc::Weak<Workspace>,
    row: glib::WeakRef<adw::ActionRow>,
    error: glib::WeakRef<gtk::Label>,
    menu: glib::WeakRef<gtk::MenuButton>,
    value: RefCell<Option<ExportProfile>>,
    failure: RefCell<Option<String>>,
    busy: Cell<bool>,
    in_flight: Cell<bool>,
    listing: Cell<bool>,
    generation: Cell<u64>,
    working: RgbSpace,
    purpose: ProfilePurpose,
    prefix: &'static str,
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
        let Some(w) = self.w.upgrade() else {
            return Ok(None);
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("ICC color profiles"));
        filter.add_suffix("icc");
        filter.add_suffix("icm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Add profile")
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let file = match dialog.open_future(Some(&w.window)).await {
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

fn heading(body: &gtk::Box, text: &str) {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.)
        .margin_top(8)
        .margin_start(8)
        .margin_bottom(4)
        .build();
    label.add_css_class("dim-label");
    body.append(&label);
}
fn button(body: &gtk::Box, label: &str, name: &str, action: impl Fn() + 'static) {
    let button = gtk::Button::with_label(label);
    button.set_widget_name(name);
    button.add_css_class("flat");
    let label = button.child().unwrap().downcast::<gtk::Label>().unwrap();
    label.set_xalign(0.);
    label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    label.set_max_width_chars(48);
    button.connect_clicked(move |_| action());
    body.append(&button);
}

impl ProfileChooser {
    pub fn new(
        w: &Rc<Workspace>,
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
            w: Rc::downgrade(w),
            row: row.downgrade(),
            error: error.downgrade(),
            menu: menu.downgrade(),
            value: Default::default(),
            failure: Default::default(),
            busy: Cell::new(false),
            in_flight: Cell::new(false),
            listing: Cell::new(false),
            generation: Cell::new(0),
            working,
            purpose,
            prefix,
        });
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.set_width_request(340);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(420)
            .min_content_width(320)
            .child(&body)
            .build();
        let picker = gtk::Box::new(gtk::Orientation::Vertical, 0);
        picker.append(&scroll);
        picker.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        button(
            &picker,
            "Add profile…",
            &format!("{}-profile-add", state.prefix),
            glib::clone!(
                #[strong]
                state,
                move || state.choose(None, None)
            ),
        );
        button(
            &picker,
            "Manage saved profiles…",
            &format!("{}-profile-manage", state.prefix),
            glib::clone!(
                #[strong]
                state,
                move || {
                    state.close();
                    if let Some(w) = state.w.upgrade() {
                        glib::MainContext::default().spawn_local(glib::clone!(
                            #[strong]
                            state,
                            async move {
                                if let Err(error) = library::manage(&w).await {
                                    *state.failure.borrow_mut() = Some(error);
                                    state.notify();
                                }
                            }
                        ));
                    }
                }
            ),
        );
        let popover = gtk::Popover::builder().child(&picker).build();
        menu.set_popover(Some(&popover));
        popover.connect_show(glib::clone!(
            #[strong]
            state,
            #[weak]
            body,
            move |_| {
                if state.listing.replace(true) {
                    return;
                }
                while let Some(child) = body.first_child() {
                    body.remove(&child);
                }
                let loading = gtk::Label::new(Some("Loading profiles…"));
                body.append(&loading);
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[strong]
                    state,
                    #[weak]
                    body,
                    async move {
                        let current = state.value.borrow().clone();
                        let (entries, current) = gio::spawn_blocking(move || {
                            let entries = library::list(&library::directory());
                            let current = current.filter(|p| {
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
                            (entries, current)
                        })
                        .await
                        .unwrap_or_else(|_| (Err("Profile library reader failed".into()), None));
                        state.listing.set(false);
                        while let Some(child) = body.first_child() {
                            body.remove(&child);
                        }
                        if let Some(current) = current {
                            heading(&body, "Current profile");
                            let title = current.name.clone();
                            button(
                                &body,
                                &title,
                                &format!("{}-profile-current", state.prefix),
                                glib::clone!(
                                    #[strong]
                                    state,
                                    move || state.choose(Some(current.clone()), None)
                                ),
                            );
                        }
                        heading(&body, "Saved profiles");
                        match entries {
                            Ok(entries) => {
                                let mut shown = 0;
                                for entry in entries.into_iter().filter(|e| {
                                    e.issue.is_none()
                                        && e.channels.is_some_and(|c| state.compatible(c))
                                }) {
                                    button(
                                        &body,
                                        &entry.name,
                                        &format!("{}-profile-saved-{shown}", state.prefix),
                                        glib::clone!(
                                            #[strong]
                                            state,
                                            move || state.choose(None, Some(entry.path.clone()))
                                        ),
                                    );
                                    shown += 1;
                                }
                                if shown == 0 {
                                    body.append(&gtk::Label::new(Some(
                                        "No matching saved profiles",
                                    )));
                                }
                            }
                            Err(error) => {
                                let label = gtk::Label::builder().label(error).wrap(true).build();
                                label.add_css_class("error");
                                body.append(&label);
                            }
                        }
                        if state.compatible(ProfileChannels::Rgb) {
                            heading(&body, "Standard color spaces");
                            for (index, space) in RgbSpace::ALL.into_iter().enumerate() {
                                let value = ExportProfile::builtin(space);
                                let title = value.name.clone();
                                button(
                                    &body,
                                    &title,
                                    &format!("{}-profile-builtin-{index}", state.prefix),
                                    glib::clone!(
                                        #[strong]
                                        state,
                                        move || state.choose(Some(value.clone()), None)
                                    ),
                                );
                            }
                        }
                        if state
                            .menu
                            .upgrade()
                            .and_then(|menu| menu.popover())
                            .is_some_and(|popover| popover.is_visible())
                        {
                            body.child_focus(gtk::DirectionType::TabForward);
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
        let restore = Rc::new(move |value| state.set(value));
        Self {
            row,
            error,
            selected,
            restore,
        }
    }
}
