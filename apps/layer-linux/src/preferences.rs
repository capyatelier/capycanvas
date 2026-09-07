//! Native controls for the Rust preferences view. All edits and shortcut
//! recording go back to UiSession; no validation or keymap lives in GTK.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

// Multiple windows share one preferences file. Serialize atomic writes and
// discard older queued snapshots; the GTK thread never takes this I/O lock.
static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SAVE_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

enum Field {
    Choice(adw::ComboRow),
    Number(adw::SpinRow),
    Switch(adw::SwitchRow),
    Info(adw::ActionRow),
}
impl Field {
    fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Choice(w) => w.upcast_ref(),
            Self::Number(w) => w.upcast_ref(),
            Self::Switch(w) => w.upcast_ref(),
            Self::Info(w) => w.upcast_ref(),
        }
    }
    fn update(&self, row: &PreferenceRow) {
        self.widget().set_sensitive(row.enabled);
        self.widget().set_visible(row.visible);
        match (self, &row.kind) {
            (Self::Choice(w), PreferenceKind::Choice { selected, .. }) => w.set_selected(*selected),
            (Self::Number(w), PreferenceKind::Number { value, .. }) => w.set_value(*value as f64),
            (Self::Switch(w), PreferenceKind::Switch { active }) => w.set_active(*active),
            _ => {}
        }
    }
}
pub struct Preferences {
    pub dialog: adw::Dialog,
    stack: adw::ViewStack,
    split: adw::NavigationSplitView,
    content_page: adw::NavigationPage,
    search: gtk::SearchEntry,
    empty: gtk::Label,
    error: gtk::Label,
    apply: gtk::Button,
    fields: RefCell<BTreeMap<PreferenceId, Field>>,
    groups: RefCell<Vec<(SettingsPage, usize, adw::PreferencesGroup)>>,
    shortcuts: adw::PreferencesGroup,
    shortcut_rows: RefCell<Vec<(String, adw::ActionRow, gtk::Label, gtk::Button)>>,
    capture: adw::Dialog,
    capture_label: gtk::Label,
    capture_key: gtk::Label,
    capture_error: gtk::Label,
    confirm: gtk::Button,
    updating: Cell<bool>,
    servicing: Cell<bool>,
}
fn margins(widget: &impl IsA<gtk::Widget>, value: i32) {
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
    widget.set_margin_start(value);
    widget.set_margin_end(value);
}
fn send(w: &Rc<Workspace>, action: PreferenceAction) {
    if !w.preferences.updating.get() {
        w.dispatch(UiAction::Preferences { action });
    }
}
fn action_button(label: &str, w: &Rc<Workspace>, action: UiAction) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| w.dispatch(action.clone())
    ));
    button
}
impl Preferences {
    pub fn new() -> Self {
        let dialog = adw::Dialog::builder()
            .title("Preferences")
            .content_width(800)
            .content_height(620)
            .width_request(360)
            .height_request(360)
            .build();
        dialog.add_css_class("layer-preferences");
        let stack = adw::ViewStack::new();
        let sidebar = adw::ViewSwitcherSidebar::builder().stack(&stack).build();
        let sidebar_view = adw::ToolbarView::new();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_show_end_title_buttons(false);
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.set_content(Some(&sidebar));
        let content_view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        content_view.add_top_bar(&header);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search preferences")
            .build();
        search.set_widget_name("settings-search");
        margins(&search, 12);
        content_view.add_top_bar(&search);
        let empty = gtk::Label::new(Some("No matching preferences"));
        empty.add_css_class("dim-label");
        empty.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&empty);
        content_view.set_content(Some(&overlay));
        let content_page = adw::NavigationPage::new(&content_view, "Appearance");
        let split = adw::NavigationSplitView::builder()
            .sidebar(&adw::NavigationPage::new(&sidebar_view, "Preferences"))
            .content(&content_page)
            .min_sidebar_width(190.0)
            .max_sidebar_width(210.0)
            .sidebar_width_fraction(0.26)
            .vexpand(true)
            .build();
        sidebar.connect_activated(glib::clone!(
            #[weak]
            split,
            move |_| split.set_show_content(true)
        ));
        let breakpoint =
            adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 620sp").unwrap());
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        breakpoint.add_setter(&sidebar, "mode", Some(&adw::SidebarMode::Page.to_value()));
        dialog.add_breakpoint(breakpoint);
        let error = gtk::Label::builder().wrap(true).xalign(0.0).build();
        error.add_css_class("error");
        let capture = adw::Dialog::builder()
            .title("Set Shortcut")
            .content_width(410)
            .content_height(250)
            .build();
        capture.add_css_class("layer-preferences");
        capture.set_widget_name("shortcut-capture");
        let capture_label = gtk::Label::builder().wrap(true).build();
        let capture_key = gtk::Label::new(None);
        capture_key.add_css_class("title-2");
        let capture_error = gtk::Label::builder().wrap(true).build();
        capture_error.add_css_class("warning");
        Self {
            dialog,
            stack,
            split,
            content_page,
            search,
            empty,
            error,
            apply: gtk::Button::with_label("Apply"),
            fields: RefCell::new(BTreeMap::new()),
            groups: RefCell::default(),
            shortcuts: adw::PreferencesGroup::new(),
            shortcut_rows: RefCell::default(),
            capture,
            capture_label,
            capture_key,
            capture_error,
            confirm: gtk::Button::with_label("Set Shortcut"),
            updating: Cell::new(false),
            servicing: Cell::new(false),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&self.split);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        margins(&footer, 12);
        self.error.set_hexpand(true);
        footer.append(&self.error);
        footer.append(&action_button("Cancel", w, UiAction::CancelSettings));
        self.apply.add_css_class("suggested-action");
        self.apply.set_widget_name("apply-settings");
        self.apply.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| w.dispatch(UiAction::ApplySettings)
        ));
        footer.append(&self.apply);
        body.append(&footer);
        self.dialog.set_child(Some(&body));
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().settings_draft.is_some())
                {
                    w.dispatch(UiAction::CancelSettings);
                }
            }
        ));
        self.stack.connect_visible_child_name_notify(glib::clone!(
            #[weak]
            w,
            move |stack| {
                if let Some(page) = SettingsPage::ALL
                    .into_iter()
                    .find(|p| Some(p.key()) == stack.visible_child_name().as_deref())
                {
                    send(&w, PreferenceAction::Page { page });
                }
            }
        ));
        self.search.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |entry| send(
                &w,
                PreferenceAction::Search {
                    query: entry.text().into()
                }
            )
        ));
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.append(&adw::HeaderBar::new());
        for label in [&self.capture_label, &self.capture_key, &self.capture_error] {
            margins(label, 6);
            body.append(label);
        }
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        margins(&footer, 12);
        for (label, action) in [
            ("Cancel", PreferenceAction::CancelShortcut),
            ("Clear", PreferenceAction::ClearShortcut),
        ] {
            footer.append(&action_button(label, w, UiAction::Preferences { action }));
        }
        self.confirm.add_css_class("suggested-action");
        self.confirm.set_widget_name("confirm-shortcut");
        self.confirm.set_hexpand(true);
        self.confirm.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let replace = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .and_then(|g| g.session.preferences())
                    .and_then(|v| v.capture)
                    .is_some_and(|c| c.conflict.is_some());
                send(&w, PreferenceAction::ConfirmShortcut { replace });
            }
        ));
        footer.append(&self.confirm);
        body.append(&footer);
        self.capture.set_child(Some(&body));
        self.capture.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().preferences.capture.is_some())
                {
                    send(&w, PreferenceAction::CancelShortcut);
                }
            }
        ));
    }
    fn build(&self, w: &Rc<Workspace>, view: &PreferencesView) {
        for page in &view.pages {
            let content = adw::PreferencesPage::new();
            for (index, group) in page.groups.iter().enumerate() {
                let native = adw::PreferencesGroup::builder().title(&group.title).build();
                for row in &group.rows {
                    let id = row.id;
                    let field = match &row.kind {
                        PreferenceKind::Choice { options, .. } => {
                            let model = gtk::StringList::new(
                                &options.iter().map(String::as_str).collect::<Vec<_>>(),
                            );
                            let control = adw::ComboRow::builder()
                                .use_markup(false)
                                .title(&row.title)
                                .subtitle(&row.description)
                                .model(&model)
                                .build();
                            control.connect_selected_notify(glib::clone!(
                                #[weak]
                                w,
                                move |c| send(
                                    &w,
                                    PreferenceAction::Edit {
                                        id,
                                        value: PreferenceValue::Choice(c.selected())
                                    }
                                )
                            ));
                            Field::Choice(control)
                        }
                        PreferenceKind::Number { control, .. } => {
                            let number =
                                adw::SpinRow::with_range(control.min, control.max, control.step);
                            number.set_title(&row.title);
                            number.set_use_markup(false);
                            number.set_subtitle(&row.description);
                            number.set_digits(control.digits);
                            number.connect_value_notify(glib::clone!(
                                #[weak]
                                w,
                                move |c| send(
                                    &w,
                                    PreferenceAction::Edit {
                                        id,
                                        value: PreferenceValue::Number(c.value() as f32)
                                    }
                                )
                            ));
                            crate::workspace::shared_spin_icons(number.upcast_ref());
                            Field::Number(number)
                        }
                        PreferenceKind::Switch { .. } => {
                            let control = adw::SwitchRow::builder()
                                .use_markup(false)
                                .title(&row.title)
                                .subtitle(&row.description)
                                .build();
                            control.connect_active_notify(glib::clone!(
                                #[weak]
                                w,
                                move |c| send(
                                    &w,
                                    PreferenceAction::Edit {
                                        id,
                                        value: PreferenceValue::Bool(c.is_active())
                                    }
                                )
                            ));
                            Field::Switch(control)
                        }
                        PreferenceKind::Info { value } => {
                            let control = adw::ActionRow::builder()
                                .use_markup(false)
                                .title(&row.title)
                                .subtitle(&row.description)
                                .build();
                            let text = gtk::Label::new(Some(value));
                            text.set_selectable(true);
                            control.add_suffix(&text);
                            Field::Info(control)
                        }
                    };
                    field
                        .widget()
                        .set_widget_name(&format!("setting-{}", id.key()));
                    native.add(field.widget());
                    self.fields.borrow_mut().insert(id, field);
                }
                content.add(&native);
                self.groups.borrow_mut().push((page.id, index, native));
            }
            if page.id == SettingsPage::Shortcuts {
                self.shortcuts.set_title("Shortcuts");
                self.shortcuts.set_description(Some("Select an action to record a shortcut. Escape cancels recording. Unassigned actions show Disabled."));
                let reset = action_button(
                    "Reset All",
                    w,
                    UiAction::Preferences {
                        action: PreferenceAction::ResetAllShortcuts,
                    },
                );
                reset.set_valign(gtk::Align::Center);
                self.shortcuts.set_header_suffix(Some(&reset));
                content.add(&self.shortcuts);
            }
            self.stack.add_titled_with_icon(
                &content,
                Some(page.id.key()),
                &page.title,
                &format!("layer-{}-symbolic", page.icon),
            );
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, view: Option<PreferencesView>) {
        self.updating.set(true);
        if let Some(view) = view {
            if self.fields.borrow().is_empty() {
                self.build(w, &view);
            }
            self.content_page.set_title(view.page.title());
            self.empty.set_visible(view.empty);
            self.stack.set_visible(!view.empty);
            self.stack.set_visible_child_name(view.page.key());
            if self.search.text().as_str() != view.query {
                self.search.set_text(&view.query);
            }
            for row in view
                .pages
                .iter()
                .flat_map(|p| &p.groups)
                .flat_map(|g| &g.rows)
            {
                if let Some(field) = self.fields.borrow().get(&row.id) {
                    field.update(row);
                }
            }
            for (id, index, group) in self.groups.borrow().iter() {
                group.set_visible(
                    view.pages.iter().find(|p| p.id == *id).unwrap().groups[*index]
                        .rows
                        .iter()
                        .any(|r| r.visible),
                );
            }
            self.error.set_text(view.error.as_deref().unwrap_or(""));
            self.apply
                .set_sensitive(view.dirty && view.capture.is_none());
            if self
                .shortcut_rows
                .borrow()
                .iter()
                .map(|r| &r.0)
                .ne(view.shortcuts.iter().map(|r| &r.id))
            {
                for (_, row, _, _) in self.shortcut_rows.borrow_mut().drain(..) {
                    self.shortcuts.remove(&row);
                }
                for spec in &view.shortcuts {
                    let row = adw::ActionRow::builder()
                        .use_markup(false)
                        .title(&spec.label)
                        .subtitle(&spec.group)
                        .activatable(true)
                        .build();
                    let id = spec.id.clone();
                    row.connect_activated(glib::clone!(
                        #[weak]
                        w,
                        move |_| send(&w, PreferenceAction::BeginShortcut { id: id.clone() })
                    ));
                    let binding = gtk::Label::new(None);
                    binding.add_css_class("dim-label");
                    row.add_suffix(&binding);
                    let reset = action_button(
                        "Reset",
                        w,
                        UiAction::Preferences {
                            action: PreferenceAction::ResetShortcut {
                                id: spec.id.clone(),
                            },
                        },
                    );
                    reset.set_valign(gtk::Align::Center);
                    row.add_suffix(&reset);
                    row.set_widget_name(&format!("shortcut-{}", spec.id));
                    self.shortcuts.add(&row);
                    self.shortcut_rows
                        .borrow_mut()
                        .push((spec.id.clone(), row, binding, reset));
                }
            }
            for ((_, row, binding, reset), spec) in
                self.shortcut_rows.borrow().iter().zip(&view.shortcuts)
            {
                row.set_visible(spec.visible);
                reset.set_sensitive(spec.modified);
                binding.set_text(if spec.shortcut.is_empty() {
                    "Disabled"
                } else {
                    &spec.shortcut
                });
            }
            if self.dialog.root().is_none() {
                self.dialog.present(Some(&w.window));
                self.split.set_show_content(true);
            }
            if let Some(capture) = view.capture {
                self.capture_label.set_text(&capture.label);
                self.capture_key.set_text(&capture.shortcut);
                let conflict = capture
                    .conflict
                    .as_ref()
                    .map(|label| format!("Already assigned to {label}. Replace its shortcut?"));
                self.capture_error.set_text(
                    capture
                        .error
                        .as_deref()
                        .or(conflict.as_deref())
                        .unwrap_or(""),
                );
                self.capture_error
                    .set_visible(capture.error.is_some() || capture.conflict.is_some());
                self.confirm
                    .set_sensitive(capture.chord.is_some() && capture.error.is_none());
                self.confirm.set_label(if capture.conflict.is_some() {
                    "Replace Shortcut"
                } else {
                    "Set Shortcut"
                });
                if self.capture.root().is_none() {
                    self.capture.present(Some(&self.dialog));
                }
            } else if self.capture.root().is_some() {
                self.capture.close();
            }
        } else {
            if self.capture.root().is_some() {
                self.capture.close();
            }
            if self.dialog.root().is_some() {
                self.dialog.close();
            }
        }
        self.updating.set(false);
    }
    /// Ordered host services. Disk I/O runs on GIO's pool, never on the drawing
    /// event loop. A request stays in the core until the host acknowledges it.
    pub fn service(&self, w: &Rc<Workspace>) {
        if self.servicing.replace(true) {
            return;
        }
        // Finish an acknowledged Apply even if the last window closes while
        // its atomic write is in flight.
        let hold = w.window.application().map(|app| app.hold());
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak]
            w,
            async move {
                let _hold = hold;
                loop {
                    let request = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .and_then(|g| g.session.state().requests.first().cloned());
                    let Some(request) = request else {
                        break;
                    };
                    let result: Result<(), String> = match request.kind {
                        HostRequestKind::NewWindow => w
                            .window
                            .application()
                            .ok_or("Application unavailable".into())
                            .and_then(|app| {
                                let action = app
                                    .lookup_action("new-window")
                                    .ok_or("New Window is unavailable")?;
                                action.activate(None);
                                Ok(())
                            }),
                        HostRequestKind::SaveSettings { settings } => {
                            let current = w
                                .gpu
                                .borrow()
                                .as_ref()
                                .is_some_and(|g| g.session.state().settings == *settings);
                            if !current {
                                Ok(())
                            } else {
                                if let Some(action) = w
                                    .window
                                    .application()
                                    .and_then(|app| app.lookup_action("settings-changed"))
                                {
                                    action.activate(Some(
                                        &serde_json::to_string(&settings).unwrap().to_variant(),
                                    ));
                                }
                                let revision = SAVE_REVISION
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                gtk::gio::spawn_blocking(move || {
                                    let _lock = SAVE_LOCK.lock().map_err(|e| e.to_string())?;
                                    if SAVE_REVISION.load(std::sync::atomic::Ordering::Relaxed)
                                        != revision
                                    {
                                        return Ok(());
                                    }
                                    save(&settings)
                                })
                                .await
                                .unwrap_or_else(|_| Err("Settings writer failed".into()))
                            }
                        }
                    };
                    w.dispatch(UiAction::CompleteRequest {
                        id: request.id,
                        error: result.err(),
                    });
                }
                w.preferences.servicing.set(false);
            }
        ));
    }
}

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("LAYER_SETTINGS_FILE")
        .map(Into::into)
        .or_else(|| (!cfg!(test)).then(|| glib::user_config_dir().join("layer/settings.json")))
}
pub fn load() -> Result<Option<Settings>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot read preferences: {e}")),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > 1_048_576 {
        return Err("Preferences file is too large".into());
    }
    let settings: Settings =
        serde_json::from_reader(file).map_err(|e| format!("Cannot read preferences: {e}"))?;
    settings.validate()?;
    Ok(Some(settings))
}
fn save(settings: &Settings) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    gtk::gio::File::for_path(path)
        .replace_contents(
            &bytes,
            None,
            false,
            gtk::gio::FileCreateFlags::PRIVATE | gtk::gio::FileCreateFlags::REPLACE_DESTINATION,
            gtk::gio::Cancellable::NONE,
        )
        .map(|_| ())
        .map_err(|e| format!("Cannot save preferences: {e}"))
}
