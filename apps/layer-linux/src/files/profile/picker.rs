//! One native picker for saved, embedded and standard profiles in every role.
use super::*;

pub(crate) struct ProfileChooser {
    pub row: adw::ActionRow,
    picker: ProfilePicker,
}
impl std::ops::Deref for ProfileChooser {
    type Target = ProfilePicker;
    fn deref(&self) -> &Self::Target {
        &self.picker
    }
}

/// Reusable menu-backed profile control. The same loader serves compact panels
/// and dialog rows; their visible presentation is independent of profile state.
pub(crate) struct ProfilePicker {
    pub button: gtk::MenuButton,
    pub error: gtk::Label,
    pub selected: Rc<dyn Fn() -> Result<ExportProfile, String>>,
    pub selected_typed: Rc<dyn Fn() -> Result<ExportProfile, layer_ui::ColorFeatureError>>,
    pub restore: Rc<dyn Fn(ExportProfile)>,
    state: Rc<State>,
}

struct State {
    copy: RefCell<layer_ui::color_feature_copy::ProfileCopy>,
    localization: RefCell<std::sync::Arc<layer_ui::Localizer>>,
    w: std::rc::Weak<Workspace>,
    window: glib::WeakRef<adw::ApplicationWindow>,
    changed: RefCell<Vec<Rc<dyn Fn(&str)>>>,
    edits: RefCell<Vec<Rc<dyn Fn()>>>,
    error: glib::WeakRef<gtk::Label>,
    menu: glib::WeakRef<gtk::MenuButton>,
    value: RefCell<Option<ExportProfile>>,
    document: RefCell<Option<ExportProfile>>,
    failure: RefCell<Option<layer_ui::ColorFeatureError>>,
    busy: Cell<bool>,
    in_flight: Cell<bool>,
    listing: Cell<bool>,
    generation: Cell<u64>,
    working: RgbSpace,
    purpose: ProfilePurpose,
}

impl State {
    fn notify(&self) {
        self.refresh_copy();
        for changed in self.edits.borrow().clone() { changed(); }
    }
    fn refresh_copy(&self) {
        let subtitle = if self.busy.get() {
            self.copy.borrow().reading.as_ref().into()
        } else {
            self.value
                .borrow()
                .as_ref()
                .map_or_else(|| self.copy.borrow().choose.as_ref().into(), |p| p.display_name(&self.localization.borrow()))
        };
        if let Some(error) = self.error.upgrade() {
            error.set_label(&self.failure.borrow().as_ref().map(|reason| reason.profile_message(&self.localization.borrow())).unwrap_or_default());
            error.set_visible(self.failure.borrow().is_some());
        }
        if let Some(menu) = self.menu.upgrade() {
            menu.set_sensitive(!self.in_flight.get());
        }
        let listeners = self.changed.borrow().clone();
        for changed in listeners {
            changed(&subtitle);
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
    ) -> Result<Option<ExportProfile>, layer_ui::ColorFeatureError> {
        let purpose = self.purpose.clone();
        let working = self.working;
        if let Some(mut value) = value {
            return gio::spawn_blocking(move || {
                value.channels = layer_color::profile_channels(&value.profile)?;
                purpose.validate(&value, working)?;
                Ok::<_,layer_ui::ColorFeatureError>(Some(value))
            })
            .await
            .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()))?;
        }
        if let Some(path) = path {
            return gio::spawn_blocking(move || {
                library::read_entry(&path, working, &purpose).map(Some)
            })
            .await
            .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()))?;
        }
        let Some(window) = self.window.upgrade() else {
            return Ok(None);
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(self.copy.borrow().filter.as_ref()));
        filter.add_suffix("icc");
        filter.add_suffix("icm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(self.copy.borrow().add_profile.as_ref())
            .accept_label(self.copy.borrow().add.as_ref())
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        {
            let initial = self.localization.borrow().clone();
            let weak = dialog.downgrade(); let filter = filter.downgrade();
            crate::on_window_localization(&window, self.w.upgrade().as_ref(), &initial, move |localization| {
                let Some(dialog) = weak.upgrade() else { return false };
                let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
                dialog.set_title(&copy.add_profile); dialog.set_accept_label(Some(&copy.add));
                if let Some(filter) = filter.upgrade() { filter.set_name(Some(&copy.filter)); }
                true
            });
        }
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
            Err(e) => return Err(layer_ui::ColorFeatureError::Diagnostic(e.to_string())),
        };
        let path = file.path().ok_or(layer_ui::ColorFeatureError::ProfileChooseFile)?;
        gio::spawn_blocking(move || {
            let value = super::read(&path, working, &purpose)?;
            let ColorProfile::Icc(bytes) = &value.profile else {
                unreachable!();
            };
            if let Some(directory) = library::directory() {
                library::store(&directory, bytes, &value.name)?;
            }
            Ok::<_,layer_ui::ColorFeatureError>(Some(value))
        })
        .await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile import worker failed".into()))?
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

impl ProfilePicker {
    /// Reuse the bounded loader and its preset-generation arbitration for
    /// programmatic selections such as the document's saved print profile.
    pub fn validated_selection(&self) -> Rc<dyn Fn(ColorProfile, String)> {
        let state = self.state.clone();
        Rc::new(move |profile, name| {
            state.choose(
                Some(ExportProfile {
                    profile,
                    name,
                    channels: ProfileChannels::Rgb,
                }),
                None,
            )
        })
    }
    pub fn is_pending(&self) -> bool {
        self.state.in_flight.get() || self.state.busy.get()
    }
    pub fn restore_document(&self, profile: ExportProfile) {
        *self.state.document.borrow_mut() =
            matches!(profile.profile, ColorProfile::Icc(_)).then(|| profile.clone());
        self.state.set(profile);
    }

    pub fn connect_changed(&self, changed: impl Fn() + 'static) {
        self.state
            .edits
            .borrow_mut()
            .push(Rc::new(changed));
    }
    pub fn compact(
        w: &Rc<Workspace>,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
    ) -> Self {
        let picker = Self::build(&w.window, Some(w), working, purpose, w.localization());
        picker.button.set_widget_name(name);
        crate::panel_controls::menu_choice(&picker.button, &picker.state.copy.borrow().choose_short);
        picker.state.changed.borrow_mut().push(Rc::new(glib::clone!(
            #[weak(rename_to = button)]
            picker.button,
            move |text: &str| {
                button.set_label(text);
                button.set_tooltip_text(Some(text));
            }
        )));
        picker
    }
}

impl ProfileChooser {
    pub fn new(
        w: &Rc<Workspace>,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
    ) -> Self {
        Self::build(&w.window, Some(w), title, name, working, purpose, w.localization())
    }

    // Application file opens can ask for a source profile before a canvas exists.
    pub fn for_window(
        window: &adw::ApplicationWindow,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Self {
        Self::build(window, None, title, name, working, purpose, localization)
    }

    fn build(
        window: &adw::ApplicationWindow,
        workspace: Option<&Rc<Workspace>>,
        title: &str,
        name: &str,
        working: RgbSpace,
        purpose: ProfilePurpose,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Self {
        let picker = ProfilePicker::build(window, workspace, working, purpose, localization);
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(picker.state.copy.borrow().choose.as_ref())
            .use_markup(false)
            .build();
        row.set_widget_name(name);
        row.add_suffix(&picker.button);
        row.set_activatable_widget(Some(&picker.button));
        picker.state.changed.borrow_mut().push(Rc::new(glib::clone!(
            #[weak]
            row,
            move |text: &str| {
                // Existing dialog consumers observe subtitle notifications.
                let _guard = row.freeze_notify();
                row.set_subtitle(text);
                row.notify("subtitle");
            }
        )));
        Self { row, picker }
    }
}

impl ProfilePicker {
    fn build(
        window: &adw::ApplicationWindow,
        workspace: Option<&Rc<Workspace>>,
        working: RgbSpace,
        purpose: ProfilePurpose,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Self {
        let copy = layer_ui::color_feature_copy::ProfileCopy::new(&localization);
        let prefix = match purpose {
            ProfilePurpose::Proof => "proof",
            ProfilePurpose::Output => "export",
            ProfilePurpose::Source(_) => "source",
        };
        let menu = gtk::MenuButton::builder()
            .icon_name("pan-down-symbolic")
            .valign(gtk::Align::Center)
            .build();
        menu.set_widget_name(&format!("{prefix}-profile-choose"));
        menu.set_tooltip_text(Some(copy.choose_add.as_ref()));
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.)
            .visible(false)
            .build();
        error.add_css_class("error");
        error.set_widget_name(&format!("{prefix}-profile-error"));
        let state = Rc::new(State {
            copy: RefCell::new(copy.clone()), localization: RefCell::new(localization),
            w: workspace.map_or_else(std::rc::Weak::new, Rc::downgrade),
            window: window.downgrade(),
            changed: RefCell::default(),
            edits: RefCell::default(),
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
            copy.add_profile_dialog.as_ref(),
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
            copy.manage.as_ref(),
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
                                let localization = state.localization.borrow().clone();
                                if let Err(error) =
                                    library::manage_for_window(&window, state.w.upgrade().as_ref(), localization)
                                        .await
                                {
                                    *state.failure.borrow_mut() = Some(layer_ui::ColorFeatureError::Diagnostic(error));
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
                choices.append(Some(state.copy.borrow().loading.as_ref()), None);
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
                            let mut entries = library::directory().map_or(Ok(Vec::new()), |directory| library::list(&directory));
                            if let Some(ExportProfile {
                                profile: ColorProfile::Icc(bytes),
                                ..
                            }) = &document
                            {
                                let key = layer_ui::profile_library::profile_identity(bytes);
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
                                let key = layer_ui::profile_library::profile_identity(bytes);
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
                        let copy = state.copy.borrow().clone();
                        for (profile, label, action) in [
                            (document, copy.document.as_ref(), "document"),
                            (current, copy.current.as_ref(), "current"),
                        ] {
                            let Some(profile) = profile else { continue };
                            let section = gio::Menu::new();
                            let title = profile.display_name(&state.localization.borrow());
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
                                        &entry.display_name(&state.localization.borrow()),
                                        &format!("saved-{index}"),
                                        glib::clone!(
                                            #[strong]
                                            state,
                                            move || state.choose(None, Some(entry.path.clone()))
                                        ),
                                    );
                                }
                                if saved.n_items() > 0 {
                                    choices.append_section(Some(state.copy.borrow().saved.as_ref()), &saved);
                                }
                            }
                            Err(reason) => choices.append(Some(&reason.profile_message(&state.localization.borrow())), None),
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
                            choices.append_section(Some(state.copy.borrow().standard.as_ref()), &standard);
                        }
                    }
                ));
            }
        ));
        {
            let initial = state.localization.borrow().clone();
            let weak = Rc::downgrade(&state);
            let popup = popover.downgrade();
            let footer = footer.downgrade();
            crate::on_window_localization(window, workspace, &initial, move |localization| {
                let Some(state) = weak.upgrade().filter(|state| state.menu.upgrade().is_some()) else { return false };
                let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
                if let Some(menu) = state.menu.upgrade() {
                    menu.set_tooltip_text(Some(&copy.choose_add));
                    menu.update_property(&[gtk::accessible::Property::Label(&copy.choose_add)]);
                }
                if let Some(footer) = footer.upgrade() {
                    for (index, label) in [&copy.add_profile_dialog, &copy.manage].into_iter().enumerate() {
                        let item = gio::MenuItem::from_model(&footer, index as i32);
                        item.set_label(Some(label));
                        footer.remove(index as i32); footer.insert_item(index as i32, &item);
                    }
                }
                *state.copy.borrow_mut() = copy;
                *state.localization.borrow_mut() = localization.clone();
                state.refresh_copy();
                if let Some(popup) = popup.upgrade().filter(|popup| popup.is_visible()) { popup.emit_by_name::<()>("show", &[]); }
                true
            });
        }
        let selected = Rc::new({
            let state = state.clone();
            move || {
                if state.busy.get() {
                    return Err(state.copy.borrow().reading.to_string());
                }
                if let Some(error) = state.failure.borrow().clone() {
                    return Err(error.profile_message(&state.localization.borrow()));
                }
                state
                    .value
                    .borrow()
                    .clone()
                    .ok_or_else(|| state.copy.borrow().choose.to_string())
            }
        });
        let selected_typed = Rc::new({
            let state = state.clone();
            move || {
                if state.busy.get() || state.in_flight.get() { return Err(layer_ui::ColorFeatureError::ProfileMissing); }
                if let Some(error) = state.failure.borrow().clone() { return Err(error); }
                state.value.borrow().clone().ok_or(layer_ui::ColorFeatureError::ProfileMissing)
            }
        });
        let restore = Rc::new({
            let state = state.clone();
            move |value| state.set(value)
        });
        Self {
            button: menu,
            error,
            selected,
            selected_typed,
            restore,
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_selection_preserves_metadata_and_reprojects_absent_names() {
        let source = SourceInterpretation { channels: layer_core::color::source::SourceChannels::Rgba, depth: layer_core::color::SampleDepth::U8, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
        for purpose in [ProfilePurpose::Output, ProfilePurpose::Proof, ProfilePurpose::Source(source)] {
            let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
            let state = State {
                copy: RefCell::new(layer_ui::color_feature_copy::ProfileCopy::new(&localization)), localization: RefCell::new(localization),
                w: std::rc::Weak::new(), window: glib::WeakRef::new(), changed: RefCell::default(), edits: RefCell::default(), error: glib::WeakRef::new(), menu: glib::WeakRef::new(),
                value: RefCell::default(), document: RefCell::default(), failure: RefCell::default(), busy: Cell::new(false), in_flight: Cell::new(false), listing: Cell::new(false), generation: Cell::new(0), working: RgbSpace::Srgb, purpose,
            };
            let presented = Rc::new(RefCell::new(String::new()));
            let output = presented.clone();
            state.changed.borrow_mut().push(Rc::new(move |name| *output.borrow_mut() = name.to_string()));
            for name in ["", "Embedded ICC profile", "Tiếng Việt ไทย Русский {name} 🎨"] {
                let profile = ExportProfile { profile: ColorProfile::Builtin(RgbSpace::Srgb), channels: ProfileChannels::Rgb, name: name.into() };
                state.set(profile.clone());
                assert_eq!(state.value.borrow().as_ref(), Some(&profile));
                for language in layer_ui::UiLanguage::ALL {
                    let localization = layer_ui::Localizer::shared(language);
                    *state.copy.borrow_mut() = layer_ui::color_feature_copy::ProfileCopy::new(&localization);
                    *state.localization.borrow_mut() = localization.clone();
                    state.refresh_copy();
                    assert_eq!(state.value.borrow().as_ref(), Some(&profile));
                    let expected = if name.is_empty() { localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string() } else { name.to_string() };
                    assert_eq!(presented.borrow().as_str(), expected);
                }
            }
        }
    }
    #[test]
    #[ignore = "private display and hardware GPU retained native profile selectors"]
    fn native_profile_picker_live_language() {
        let (app, active) = crate::application("art.capycanvas.ProfilePickerLanguages");
        app.register(None::<&gio::Cancellable>).unwrap();
        app.activate_action("new-window", None);
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !predicate() {
                assert!(std::time::Instant::now() < deadline, "native profile state ready");
                while glib::MainContext::default().pending() { glib::MainContext::default().iteration(false); }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        wait(&|| !active.borrow().is_empty());
        let w = active.borrow().last().unwrap().clone();
        wait(&|| w.gpu.borrow().is_some());
        let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap();
        let profile = ColorProfile::Icc(bytes.into());
        let source = SourceInterpretation { channels: layer_core::color::source::SourceChannels::Rgba, depth: layer_core::color::SampleDepth::U8, profile: profile.clone(), profile_assumed: false };
        for theme in [layer_ui::Theme::Light, layer_ui::Theme::Dark] {
            w.dispatch(layer_ui::UiAction::SetTheme { theme: Some(theme) });
            for purpose in [ProfilePurpose::Output, ProfilePurpose::Proof, ProfilePurpose::Source(source.clone())] {
                let picker = ProfileChooser::new(&w, "ICC", "language-profile", RgbSpace::Srgb, purpose);
                let group = adw::PreferencesGroup::new(); group.add(&picker.row);
                let dialog = adw::AlertDialog::builder().heading("ICC").extra_child(&group).build();
                dialog.add_response("close", "×"); dialog.set_close_response("close");
                dialog.present(Some(&w.window));
                let row = picker.row.clone(); let button = picker.button.clone();
                for name in ["", "Embedded ICC profile", "Tiếng Việt ไทย Русский {name} 🎨"] {
                    picker.validated_selection()(profile.clone(), name.into());
                    wait(&|| !picker.is_pending());
                    let selected = (picker.selected_typed)().unwrap();
                    assert_eq!(selected.name, name); assert_eq!(selected.profile, profile);
                    let engine = w.gpu.borrow().as_ref().unwrap().session.engine() as *const _ as usize;
                    let checkpoint = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
                    for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                        let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
                        w.dispatch(layer_ui::UiAction::Preferences { action: layer_ui::PreferenceAction::Edit { id: layer_ui::PreferenceId::Language, value: layer_ui::PreferenceValue::Choice(choice) } });
                        wait(&|| w.localization().language() == language);
                        assert_eq!(picker.row, row); assert_eq!(picker.button, button);
                        assert_eq!(row.subtitle().as_deref(), Some(selected.display_name(&w.localization()).as_str()));
                        assert_eq!((picker.selected_typed)().unwrap(), selected);
                        if name.is_empty() {
                            let standalone = ProfileChooser::for_window(&w.window, "ICC", "prepared-profile", RgbSpace::Srgb, ProfilePurpose::Output, w.localization());
                            (standalone.restore)(selected.clone());
                            assert_eq!(standalone.row.subtitle().as_deref(), Some(selected.display_name(&w.localization()).as_str()));
                            assert_eq!((standalone.selected_typed)().unwrap(), selected);
                        }
                        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine() as *const _ as usize, engine);
                        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                    }
                }
                dialog.force_close();
            }
        }
        w.window.close();
        while glib::MainContext::default().pending() { glib::MainContext::default().iteration(false); }
    }

    #[test]
    #[ignore = "private display with native bare window language publication"]
    fn native_bare_profile_language_transition() {
        let (app, active) = crate::application("art.capycanvas.BareProfileLanguages");
        app.register(None::<&gio::Cancellable>).unwrap();
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !predicate() {
                assert!(std::time::Instant::now() < deadline, "bare native language ready");
                while glib::MainContext::default().pending() { glib::MainContext::default().iteration(false); }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let request = |language| {
            let settings = layer_ui::Settings { language: layer_ui::LanguagePreference::Explicit(language), ..Default::default() };
            app.activate_action("settings-changed", Some(&serde_json::to_string(&settings).unwrap().to_variant()));
        };
        let profile = ExportProfile { profile: ColorProfile::Builtin(RgbSpace::Srgb), channels: ProfileChannels::Rgb, name: String::new() };
        for theme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            app.style_manager().set_color_scheme(theme);
            request(layer_ui::UiLanguage::English);
            let initial = glib::MainContext::default().block_on(crate::prepare_application_context(&app, &active)).1;
            let window = adw::ApplicationWindow::builder().application(&app).default_width(480).default_height(300).build();
            let picker = ProfileChooser::for_window(&window, "ICC", "bare-profile", RgbSpace::Srgb, ProfilePurpose::Output, initial.clone());
            (picker.restore)(profile.clone());
            let entry = gtk::Entry::new(); entry.set_text("tiếng ไทย Русский {draft} 🎨");
            let guard = crate::input::guard_entry_activation(&entry);
            let group = adw::PreferencesGroup::new(); group.add(&picker.row);
            let body = gtk::Box::new(gtk::Orientation::Vertical, 8); body.append(&group); body.append(&entry);
            window.set_content(Some(&body)); window.present();
            wait(&|| window.is_mapped()); entry.grab_focus(); entry.select_region(1, 4);
            let selection = entry.selection_bounds(); let row = picker.row.clone(); let button = picker.button.clone();
            for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                request(language);
                wait(&|| crate::window_localization(&window, None, &initial).language() == language);
                let localization = crate::window_localization(&window, None, &initial);
                assert_eq!(row.subtitle().as_deref(), Some(profile.display_name(&localization).as_str()));
                assert_eq!(picker.row, row); assert_eq!(picker.button, button);
                assert_eq!((picker.selected_typed)().unwrap(), profile);
                assert_eq!(entry.text(), "tiếng ไทย Русский {draft} 🎨"); assert_eq!(entry.selection_bounds(), selection);
            }
            request(layer_ui::UiLanguage::English);
            wait(&|| crate::window_localization(&window, None, &initial).language() == layer_ui::UiLanguage::English);
            let mut editable = entry.upcast_ref::<gtk::Editable>().clone();
            while let Some(delegate) = editable.delegate() { editable = delegate; }
            let text = editable.downcast::<gtk::Text>().unwrap();
            text.emit_by_name::<()>("preedit-changed", &[&"に"]);
            assert!(guard.active());
            request(layer_ui::UiLanguage::Russian);
            let future = glib::MainContext::default().block_on(crate::prepare_application_context(&app, &active)).1;
            assert_eq!(future.language(), layer_ui::UiLanguage::Russian);
            let late_window = adw::ApplicationWindow::builder().application(&app).default_width(480).default_height(300).build();
            let late_picker = ProfileChooser::for_window(&late_window, "ICC", "late-bare-profile", RgbSpace::Srgb, ProfilePurpose::Output, initial.clone());
            (late_picker.restore)(profile.clone());
            let late_group = adw::PreferencesGroup::new(); late_group.add(&late_picker.row); late_window.set_content(Some(&late_group)); late_window.present();
            wait(&|| crate::window_localization(&late_window, None, &initial).language() == layer_ui::UiLanguage::Russian);
            assert_eq!(late_picker.row.subtitle().as_deref(), Some(profile.display_name(&future).as_str()));
            assert_eq!(crate::window_localization(&window, None, &initial).language(), layer_ui::UiLanguage::English);
            let future_window = adw::ApplicationWindow::builder().application(&app).default_width(480).default_height(300).build();
            let future_picker = ProfileChooser::for_window(&future_window, "ICC", "future-bare-profile", RgbSpace::Srgb, ProfilePurpose::Output, future.clone());
            (future_picker.restore)(profile.clone());
            let group = adw::PreferencesGroup::new(); group.add(&future_picker.row); future_window.set_content(Some(&group)); future_window.present();
            request(layer_ui::UiLanguage::English);
            wait(&|| crate::window_localization(&future_window, None, &future).language() == layer_ui::UiLanguage::English);
            assert!(guard.active());
            assert_eq!(crate::window_localization(&window, None, &initial).language(), layer_ui::UiLanguage::English);
            text.emit_by_name::<()>("preedit-changed", &[&""]);
            assert!(!guard.active());
            for &language in layer_ui::localization::SHIPPED_LANGUAGES { request(language); }
            request(layer_ui::UiLanguage::Vietnamese);
            wait(&|| crate::window_localization(&window, None, &initial).language() == layer_ui::UiLanguage::Vietnamese && crate::window_localization(&future_window, None, &future).language() == layer_ui::UiLanguage::Vietnamese);
            assert_eq!((future_picker.selected_typed)().unwrap(), profile); assert_eq!((picker.selected_typed)().unwrap(), profile);
            assert_eq!(entry.text(), "tiếng ไทย Русский {draft} 🎨"); assert_eq!(entry.selection_bounds(), selection);
            eprintln!("GTK bare language {:?}: all fifteen, dirty literal/selection, synthetic preedit owner cancellation and future-window rapid publication passed", theme);
            future_window.destroy(); late_window.destroy(); window.destroy();
        }
    }

}
