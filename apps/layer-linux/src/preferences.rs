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
    Text(adw::ActionRow, gtk::Entry, gtk::EventControllerFocus),
    Choice(adw::ComboRow),
    ImageChoice(gtk::ListBoxRow, crate::image_selector::ImageSelector),
    Spin(adw::SpinRow),
    Number(gtk::ListBoxRow, crate::number_control::NumberControl),
    Switch(adw::SwitchRow),
    Info(adw::ActionRow),
}
impl Field {
    fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Text(w, _, _) => w.upcast_ref(),
            Self::Choice(w) => w.upcast_ref(),
            Self::ImageChoice(w, _) => w.upcast_ref(),
            Self::Number(w, _) => w.upcast_ref(),
            Self::Spin(w) => w.upcast_ref(),
            Self::Switch(w) => w.upcast_ref(),
            Self::Info(w) => w.upcast_ref(),
        }
    }
    fn update(&self, row: &PreferenceRow) {
        self.widget().set_sensitive(row.enabled);
        self.widget().set_visible(row.visible);
        match (self, &row.kind) {
            (Self::Text(_, entry, focus), PreferenceKind::Text { value, .. }) => {
                if !focus.contains_focus() && entry.text().as_str() != value {
                    entry.set_text(value);
                }
            }
            (Self::Choice(w), PreferenceKind::Choice { selected, .. }) => w.set_selected(*selected),
            (Self::ImageChoice(_, w), PreferenceKind::Choice { selected, .. }) => {
                w.set_selected(*selected)
            }
            (Self::Number(_, w), PreferenceKind::Number { value, .. }) => {
                w.set_value(*value as f64)
            }
            (Self::Spin(w), PreferenceKind::Number { value, control }) => {
                w.set_value(*value as f64 * control.scale)
            }
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
    content_view: adw::ToolbarView,
    sidebar: adw::ViewSwitcherSidebar,
    search_toggle: gtk::ToggleButton,
    search_bar: gtk::SearchBar,
    search_results: gtk::ListBox,
    search: gtk::SearchEntry,
    search_focus: Cell<u64>,
    reveal: Cell<Option<PreferenceId>>,
    shortcut_search: gtk::SearchEntry,
    empty: gtk::Label,
    error: gtk::Label,
    fields: RefCell<BTreeMap<PreferenceId, Field>>,
    groups: RefCell<Vec<(SettingsPage, usize, adw::PreferencesGroup)>>,
    shortcuts: adw::PreferencesGroup,
    shortcut_rows: RefCell<Vec<(String, adw::ActionRow, gtk::Label)>>,
    editor: adw::Dialog,
    editor_body: gtk::Box,
    editor_signature: RefCell<String>,
    capture: adw::Dialog,
    capture_label: gtk::Label,
    capture_key: gtk::Label,
    capture_error: gtk::Label,
    confirm: gtk::Button,
    shown: Cell<[bool; 3]>,
    updating: Cell<bool>,
    servicing: Cell<bool>,
    context: gtk::PopoverMenu,
    context_reset: RefCell<Option<(PreferenceId, gtk::Button)>>,
}
impl Drop for Preferences {
    fn drop(&mut self) {
        self.context.unparent();
    }
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

fn preference(w: &Workspace, id: PreferenceId) -> Option<PreferenceRow> {
    w.gpu
        .borrow()
        .as_ref()?
        .session
        .preferences()?
        .pages
        .into_iter()
        .flat_map(|p| p.groups)
        .flat_map(|g| g.rows)
        .find(|r| r.id == id)
}

fn commit_text(w: &Rc<Workspace>, id: PreferenceId, entry: &gtk::Entry) {
    send(
        w,
        PreferenceAction::Edit {
            id,
            value: PreferenceValue::Text(entry.text().into()),
        },
    );
    if w.gpu
        .borrow()
        .as_ref()
        .is_some_and(|g| g.session.state().preferences.error.is_none())
        && let Some(PreferenceRow {
            kind: PreferenceKind::Text { value, .. },
            ..
        }) = preference(w, id)
    {
        entry.set_text(&value);
    }
}

fn install_reset_menu(w: &Rc<Workspace>, widget: &gtk::Widget, id: PreferenceId) {
    let click = gtk::GestureClick::new();
    click.set_name(Some("preference-context-click"));
    click.set_button(3);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    click.connect_pressed(glib::clone!(
        #[weak]
        w,
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(widget) = gesture.widget() {
                show_reset_menu(&w, &widget, id, x, y);
            }
        }
    ));
    widget.add_controller(click);
    let hold = gtk::GestureLongPress::new();
    hold.set_name(Some("preference-context-hold"));
    hold.set_touch_only(true);
    hold.set_propagation_phase(gtk::PropagationPhase::Capture);
    hold.connect_pressed(glib::clone!(
        #[weak]
        w,
        move |gesture, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(widget) = gesture.widget() {
                show_reset_menu(&w, &widget, id, x, y);
            }
        }
    ));
    widget.add_controller(hold);
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        w,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |controller, key, _, mods| {
            if key == gtk::gdk::Key::Menu
                || (key == gtk::gdk::Key::F10 && mods == gtk::gdk::ModifierType::SHIFT_MASK)
            {
                if let Some(widget) = controller.widget() {
                    show_reset_menu(
                        &w,
                        &widget,
                        id,
                        widget.width() as f64 / 2.0,
                        widget.height() as f64 / 2.0,
                    );
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    widget.add_controller(keys);
}

fn show_reset_menu(w: &Rc<Workspace>, widget: &gtk::Widget, id: PreferenceId, x: f64, y: f64) {
    let Some(reset) = preference(w, id).and_then(|r| r.reset) else {
        return;
    };
    let popup = &w.preferences.context;
    let root = gtk::gio::Menu::new();
    let actions = gtk::gio::SimpleActionGroup::new();
    // Delegate text operations to GtkText. The native editor still owns
    // selection, clipboard transfers and undo; we only present its actions.
    let mut picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
    let mut editor = None;
    while let Some(hit) = picked {
        if let Ok(text) = hit.clone().downcast::<gtk::Text>() {
            editor = Some(text);
            break;
        }
        if hit == *widget {
            break;
        }
        picked = hit.parent();
    }
    if editor.is_none() {
        editor = widget
            .root()
            .and_then(|r| r.focus())
            .and_downcast::<gtk::Text>()
            .filter(|text| text.is_ancestor(widget));
    }
    if let Some(text) = editor {
        let editing = gtk::gio::Menu::new();
        for item in layer_ui::text_edit_menu(layer_ui::Platform::Gtk) {
            use layer_ui::TextEditAction as E;
            let (name, command, enabled) = match item.action {
                E::Cut => (
                    "cut",
                    "clipboard.cut",
                    text.selection_bounds().is_some() && text.is_editable(),
                ),
                E::Copy => ("copy", "clipboard.copy", text.selection_bounds().is_some()),
                E::Paste => ("paste", "clipboard.paste", text.is_editable()),
                E::SelectAll => (
                    "select-all",
                    "selection.select-all",
                    !text.text().is_empty(),
                ),
            };
            let action = gtk::gio::SimpleAction::new(name, None);
            action.set_enabled(enabled);
            action.connect_activate(glib::clone!(
                #[weak]
                text,
                move |_, _| {
                    let _ = text.activate_action(command, None);
                }
            ));
            actions.add_action(&action);
            let entry = gtk::gio::MenuItem::new(Some(item.label), Some(&format!("field.{name}")));
            entry.set_attribute_value(
                "accel",
                Some(&crate::workspace::native_accelerator(&item.key).to_variant()),
            );
            editing.append_item(&entry);
        }
        root.append_section(None, &editing);
    }
    let section = gtk::gio::Menu::new();
    let item = gtk::gio::MenuItem::new(None, None);
    item.set_attribute_value("custom", Some(&"reset".to_variant()));
    section.append_item(&item);
    root.append_section(None, &section);
    popup.insert_action_group("field", Some(&actions));
    popup.set_menu_model(Some(&root));
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("preference-reset");
    button.set_widget_name("preference-reset");
    button.set_sensitive(reset.enabled);
    let labels = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    let title = gtk::Label::builder()
        .label(&reset.label)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let value = gtk::Label::new(Some(&reset.hint));
    value.add_css_class("dim-label");
    labels.append(&title);
    labels.append(&value);
    button.set_child(Some(&labels));
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| {
            w.preferences.context.popdown();
            if let Some(Field::Number(_, number)) = w.preferences.fields.borrow().get(&id) {
                number.cancel_edit();
            }
            send(&w, PreferenceAction::Reset { id });
            if let Some(Field::Text(_, entry, _)) = w.preferences.fields.borrow().get(&id)
                && let Some(PreferenceRow {
                    kind: PreferenceKind::Text { value, .. },
                    ..
                }) = preference(&w, id)
            {
                entry.set_text(&value);
            }
        }
    ));
    popup.add_child(&button, "reset");
    *w.preferences.context_reset.borrow_mut() = Some((id, button));
    if let Some(point) = widget.compute_point(
        &w.preferences.content_view,
        &gtk::graphene::Point::new(x as f32, y as f32),
    ) {
        popup.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        popup.popup();
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
fn text_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    // GObject may apply builder text properties before use-markup. Set plain
    // text mode first, including for user-defined shortcut names containing &.
    row.set_use_markup(false);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row
}
impl Preferences {
    pub fn new() -> Self {
        let dialog = adw::Dialog::builder()
            .title("Preferences")
            .content_width(1000)
            .content_height(744)
            .width_request(360)
            .height_request(360)
            .build();
        dialog.add_css_class("layer-preferences");
        let stack = adw::ViewStack::new();
        let sidebar = adw::ViewSwitcherSidebar::builder().stack(&stack).build();
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.set_widget_name("preferences-sidebar");
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_show_end_title_buttons(false);
        let search_toggle = gtk::ToggleButton::builder()
            .icon_name("edit-find-symbolic")
            .tooltip_text("Search preferences")
            .build();
        search_toggle.set_widget_name("preferences-search-toggle");
        sidebar_header.pack_start(&search_toggle);
        sidebar_view.add_top_bar(&sidebar_header);
        let content_view = adw::ToolbarView::new();
        content_view.set_widget_name("preferences-content");
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        content_view.add_top_bar(&header);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search preferences")
            .build();
        search.set_widget_name("settings-search");
        margins(&search, 6);
        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(&search));
        search_bar.connect_entry(&search);
        sidebar_view.add_top_bar(&search_bar);
        let search_results = gtk::ListBox::new();
        search_results.set_selection_mode(gtk::SelectionMode::None);
        search_results.add_css_class("navigation-sidebar");
        let sidebar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_body.append(&sidebar);
        sidebar_body.append(&search_results);
        let empty = gtk::Label::new(Some("No matching preferences"));
        empty.add_css_class("dim-label");
        empty.set_visible(false);
        sidebar_body.append(&empty);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&sidebar_body)
            .vexpand(true)
            .build();
        sidebar_view.set_content(Some(&scroll));
        content_view.set_content(Some(&stack));
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
        let editor = adw::Dialog::builder()
            .title("Keyboard Shortcut")
            .content_width(460)
            .build();
        editor.add_css_class("layer-preferences");
        editor.set_widget_name("shortcut-editor");
        let editor_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let editor_view = adw::ToolbarView::new();
        editor_view.add_top_bar(&adw::HeaderBar::new());
        margins(&editor_body, 18);
        editor_view.set_content(Some(&editor_body));
        editor.set_child(Some(&editor_view));
        let shortcut_search = gtk::SearchEntry::builder()
            .placeholder_text("Search shortcuts")
            .build();
        shortcut_search.set_widget_name("shortcuts-search");
        let context = gtk::PopoverMenu::from_model(None::<&gtk::gio::Menu>);
        context.set_parent(&content_view);
        context.set_widget_name("preference-context-menu");
        Self {
            dialog,
            context,
            context_reset: RefCell::new(None),
            stack,
            split,
            content_page,
            content_view,
            sidebar,
            search_toggle,
            search_bar,
            search_results,
            search,
            search_focus: Cell::new(0),
            reveal: Cell::new(None),
            shortcut_search,
            empty,
            error,
            fields: RefCell::new(BTreeMap::new()),
            groups: RefCell::default(),
            shortcuts: adw::PreferencesGroup::new(),
            shortcut_rows: RefCell::default(),
            editor,
            editor_body,
            editor_signature: RefCell::default(),
            capture,
            capture_label,
            capture_key,
            capture_error,
            confirm: gtk::Button::with_label("Set Shortcut"),
            shown: Cell::new([false; 3]),
            updating: Cell::new(false),
            servicing: Cell::new(false),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        // Touch has no hover position. Native mouse emulation can leave a row
        // prelit; suppress that visual until an actual pointing device returns.
        let pointer = gtk::EventControllerLegacy::new();
        pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
        pointer.connect_event(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                if matches!(
                    event.event_type(),
                    gtk::gdk::EventType::TouchBegin
                        | gtk::gdk::EventType::TouchUpdate
                        | gtk::gdk::EventType::TouchEnd
                ) {
                    w.preferences.dialog.add_css_class("touch-input");
                } else if !event.is_pointer_emulated()
                    && matches!(
                        event.event_type(),
                        gtk::gdk::EventType::MotionNotify | gtk::gdk::EventType::ButtonPress
                    )
                {
                    w.preferences.dialog.remove_css_class("touch-input");
                }
                glib::Propagation::Proceed
            }
        ));
        self.dialog.add_controller(pointer);
        margins(&self.error, 12);
        self.error.set_visible(false);
        self.content_view.add_top_bar(&self.error);
        self.dialog.set_child(Some(&self.split));
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().settings_open)
                {
                    w.dispatch(UiAction::CloseSettings);
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
        self.search_toggle.connect_toggled(glib::clone!(
            #[weak]
            w,
            move |button| {
                send(
                    &w,
                    PreferenceAction::ToggleSearch {
                        open: button.is_active(),
                    },
                );
            }
        ));
        self.search_bar
            .connect_search_mode_enabled_notify(glib::clone!(
                #[weak]
                w,
                move |bar| {
                    send(
                        &w,
                        PreferenceAction::ToggleSearch {
                            open: bar.is_search_mode(),
                        },
                    );
                }
            ));
        self.shortcut_search.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                send(
                    &w,
                    PreferenceAction::SearchShortcuts {
                        query: entry.text().into(),
                    },
                );
            }
        ));
        self.editor.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                send(&w, PreferenceAction::CloseShortcutEditor);
            }
        ));
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.append(&adw::HeaderBar::new());
        for label in [&self.capture_label, &self.capture_key, &self.capture_error] {
            margins(label, 6);
            body.append(label);
        }
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        margins(&footer, 12);
        footer.append(&action_button(
            "Cancel",
            w,
            UiAction::Preferences {
                action: PreferenceAction::CancelShortcut,
            },
        ));
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
        // Dialogs have their own shortcut scope. Record before its native
        // bindings consume Escape, Space, arrows or accelerators.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                w.interact(crate::input::key_input(key, true, modifiers, false, None));
                glib::Propagation::Stop
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak]
            w,
            move |_, key, _, modifiers| {
                w.interact(crate::input::key_input(key, false, modifiers, false, None));
            }
        ));
        self.capture.add_controller(keys);
    }
    fn build(&self, w: &Rc<Workspace>, view: &PreferencesView) {
        for page in &view.pages {
            let content = adw::PreferencesPage::new();
            for (index, group) in page.groups.iter().enumerate() {
                let native = adw::PreferencesGroup::builder().title(&group.title).build();
                for row in &group.rows {
                    let id = row.id;
                    let field = match &row.kind {
                        PreferenceKind::Choice {
                            options,
                            icons,
                            presentation: ChoicePresentation::ImageTiles { columns },
                            ..
                        } => {
                            let native_row = gtk::ListBoxRow::new();
                            native_row.set_activatable(false);
                            native_row.add_css_class("image-preference");
                            let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
                            let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
                            let title = gtk::Label::builder().label(&row.title).xalign(0.0).build();
                            text.append(&title);
                            if !row.description.is_empty() {
                                let description = gtk::Label::builder()
                                    .label(&row.description)
                                    .xalign(0.0)
                                    .wrap(true)
                                    .build();
                                description.add_css_class("subtitle");
                                description.add_css_class("dim-label");
                                text.append(&description);
                            }
                            body.append(&text);
                            let selector = crate::image_selector::ImageSelector::new(
                                options,
                                icons,
                                *columns,
                                glib::clone!(
                                    #[weak]
                                    w,
                                    move |selected| send(
                                        &w,
                                        PreferenceAction::Edit {
                                            id,
                                            value: PreferenceValue::Choice(selected)
                                        }
                                    )
                                ),
                            );
                            // Center the choices; no import control in this version.
                            body.append(&selector.widget);
                            native_row.set_child(Some(&body));
                            Field::ImageChoice(native_row, selector)
                        }
                        PreferenceKind::Choice { options, icons, .. } => {
                            let model = gtk::StringList::new(
                                &options.iter().map(String::as_str).collect::<Vec<_>>(),
                            );
                            let control = adw::ComboRow::builder()
                                .use_markup(false)
                                .title(&row.title)
                                .subtitle(&row.description)
                                .model(&model)
                                .build();
                            if !icons.is_empty() {
                                let factory = gtk::SignalListItemFactory::new();
                                factory.connect_setup(|_, item| {
                                    let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                                    row.append(&gtk::Image::builder().pixel_size(24).build());
                                    row.append(&gtk::Label::new(None));
                                    item.set_child(Some(&row));
                                });
                                let options = options.clone();
                                let icons = icons.clone();
                                factory.connect_bind(move |_, item| {
                                    let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                                    let text = item
                                        .item()
                                        .and_downcast::<gtk::StringObject>()
                                        .unwrap()
                                        .string();
                                    let row = item.child().unwrap();
                                    let image =
                                        row.first_child().and_downcast::<gtk::Image>().unwrap();
                                    let label =
                                        row.last_child().and_downcast::<gtk::Label>().unwrap();
                                    label.set_text(&text);
                                    if let Some(index) =
                                        options.iter().position(|s| s == text.as_str())
                                    {
                                        image.set_icon_name(Some(&format!(
                                            "layer-{}-symbolic",
                                            icons[index]
                                        )));
                                    }
                                });
                                control.set_factory(Some(&factory));
                                control.set_list_factory(Some(&factory));
                            }
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
                        PreferenceKind::Text {
                            max_length,
                            placeholder,
                            ..
                        } => {
                            let native_row = text_row(&row.title, &row.description);
                            let entry = gtk::Entry::builder()
                                .width_chars(9)
                                .max_width_chars(9)
                                .max_length(*max_length as i32)
                                .placeholder_text(placeholder)
                                .valign(gtk::Align::Center)
                                .build();
                            entry.add_css_class("preference-entry");
                            entry.set_widget_name(&format!("setting-text-{}", id.key()));
                            entry.connect_activate(glib::clone!(
                                #[weak]
                                w,
                                move |entry| commit_text(&w, id, entry)
                            ));
                            let focus = gtk::EventControllerFocus::new();
                            focus.connect_leave(glib::clone!(
                                #[weak]
                                w,
                                #[weak]
                                entry,
                                move |_| send(
                                    &w,
                                    PreferenceAction::Edit {
                                        id,
                                        value: PreferenceValue::Text(entry.text().into())
                                    }
                                )
                            ));
                            entry.add_controller(focus.clone());
                            native_row.add_suffix(&entry);
                            native_row.set_activatable_widget(Some(&entry));
                            Field::Text(native_row, entry, focus)
                        }
                        PreferenceKind::Number { control, .. }
                            if control.kind == NumericKind::Number =>
                        {
                            let spin = adw::SpinRow::with_range(
                                control.min * control.scale,
                                control.max * control.scale,
                                control.step * control.scale,
                            );
                            spin.set_title(&row.title);
                            spin.set_subtitle(&row.description);
                            spin.set_use_markup(false);
                            spin.set_digits(control.digits);
                            spin.set_numeric(false);
                            spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
                            let format = control.clone();
                            spin.connect_output(move |spin| {
                                let value = format
                                    .resolve(spin.value() / format.scale, NumericOperation::Format)
                                    .unwrap();
                                spin.set_text(&value.text);
                                true
                            });
                            let spec = control.clone();
                            spin.connect_input(move |spin| {
                                Some(
                                    spec.resolve(
                                        0.0,
                                        NumericOperation::Expression {
                                            text: spin.text().into(),
                                        },
                                    )
                                    .map(|v| v.value * spec.scale)
                                    .map_err(|_| ()),
                                )
                            });
                            let scale = control.scale;
                            spin.connect_value_notify(glib::clone!(
                                #[weak]
                                w,
                                move |spin| send(
                                    &w,
                                    PreferenceAction::Edit {
                                        id,
                                        value: PreferenceValue::Number(
                                            (spin.value() / scale) as f32
                                        )
                                    }
                                )
                            ));
                            Field::Spin(spin)
                        }
                        PreferenceKind::Number { control, .. } => {
                            let native_row = gtk::ListBoxRow::new();
                            native_row.set_activatable(false);
                            native_row.add_css_class("number-preference");
                            let number = crate::number_control::NumberControl::new(
                                control.clone(),
                                &row.title,
                                &row.description,
                            );
                            number.set_widget_name(&format!("setting-{}", id.key()));
                            number.connect_value_changed(glib::clone!(
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
                            native_row.set_child(Some(&number));
                            Field::Number(native_row, number)
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
                        PreferenceKind::Info { .. } | PreferenceKind::Link { .. } => {
                            let control = text_row(&row.title, &row.description);
                            if let PreferenceKind::Link { label, url } = &row.kind {
                                let link = gtk::LinkButton::with_label(url, label);
                                link.set_valign(gtk::Align::Center);
                                control.add_suffix(&link);
                                control.set_activatable_widget(Some(&link));
                            } else if let PreferenceKind::Info { value } = &row.kind {
                                let text = gtk::Label::new(Some(value));
                                text.set_selectable(true);
                                control.add_suffix(&text);
                            }
                            Field::Info(control)
                        }
                    };
                    field.widget().set_widget_name(&format!(
                        "{}-{}",
                        if matches!(field, Field::Number(..)) {
                            "preference"
                        } else {
                            "setting"
                        },
                        id.key()
                    ));
                    if row.reset.is_some() {
                        install_reset_menu(w, field.widget(), id);
                    }
                    native.add(field.widget());
                    self.fields.borrow_mut().insert(id, field);
                }
                content.add(&native);
                self.groups.borrow_mut().push((page.id, index, native));
            }
            if page.id == SettingsPage::Shortcuts {
                let search_group = adw::PreferencesGroup::new();
                search_group.add(&self.shortcut_search);
                content.add(&search_group);
                self.shortcuts.set_title("Shortcuts");
                self.shortcuts
                    .set_description(Some("Select an action to edit its shortcuts."));
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
        // Losing editor focus can commit while a menu opens. Update its state
        // instead of closing it in response to that ordinary model refresh.
        if view
            .as_ref()
            .is_none_or(|v| self.stack.visible_child_name().as_deref() != Some(v.page.key()))
        {
            self.context.popdown();
        }
        if let Some((id, button)) = self.context_reset.borrow().as_ref() {
            button.set_sensitive(
                view.as_ref()
                    .and_then(|v| {
                        v.pages
                            .iter()
                            .flat_map(|p| &p.groups)
                            .flat_map(|g| &g.rows)
                            .find(|r| r.id == *id)
                    })
                    .and_then(|r| r.reset.as_ref())
                    .is_some_and(|r| r.enabled),
            );
        }
        self.updating.set(true);
        let open = [
            view.is_some(),
            view.as_ref().is_some_and(|v| v.shortcut_editor.is_some()),
            view.as_ref().is_some_and(|v| v.capture.is_some()),
        ];
        let was_open = self.shown.replace(open);
        if !open[0] {
            self.reveal.set(None);
        }
        if let Some(view) = view {
            if self.fields.borrow().is_empty() {
                self.build(w, &view);
            }
            self.content_page.set_title(view.page.title());
            self.empty.set_visible(view.empty);
            self.stack.set_visible_child_name(view.page.key());
            self.search_toggle.set_active(view.searching);
            let opening_search = view.searching && !self.search_bar.is_search_mode();
            self.search_bar.set_search_mode(view.searching);
            self.sidebar.set_visible(view.query.is_empty());
            self.search_results.set_visible(!view.query.is_empty());
            if self.search.text().as_str() != view.query {
                self.search.set_text(&view.query);
            }
            if opening_search {
                self.search.grab_focus();
            }
            if self.search_focus.replace(view.search_focus) != view.search_focus
                && view.search_focus != 0
            {
                self.split.set_show_content(false);
                self.search.grab_focus();
                self.search.set_position(-1);
            }
            self.search_results.remove_all();
            for result in &view.search_results {
                let row = text_row(&result.title, &result.description);
                row.set_activatable(true);
                let action = result.action.clone();
                row.connect_activated(glib::clone!(
                    #[weak]
                    w,
                    move |_| {
                        send(&w, action.clone());
                        w.preferences.split.set_show_content(true);
                    }
                ));
                self.search_results.append(&row);
            }
            if self.shortcut_search.text().as_str() != view.shortcut_query {
                self.shortcut_search.set_text(&view.shortcut_query);
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
            self.error.set_visible(view.error.is_some());
            if self
                .shortcut_rows
                .borrow()
                .iter()
                .map(|r| &r.0)
                .ne(view.shortcuts.iter().map(|r| &r.id))
            {
                for (_, row, _) in self.shortcut_rows.borrow_mut().drain(..) {
                    self.shortcuts.remove(&row);
                }
                for spec in &view.shortcuts {
                    let row = text_row(&spec.label, &spec.group);
                    row.set_activatable(true);
                    let id = spec.id.clone();
                    row.connect_activated(glib::clone!(
                        #[weak]
                        w,
                        move |_| send(&w, PreferenceAction::EditShortcut { id: id.clone() })
                    ));
                    let binding = gtk::Label::new(None);
                    binding.add_css_class("dim-label");
                    row.add_suffix(&binding);
                    row.set_widget_name(&format!("shortcut-{}", spec.id));
                    self.shortcuts.add(&row);
                    self.shortcut_rows
                        .borrow_mut()
                        .push((spec.id.clone(), row, binding));
                }
            }
            for ((_, row, binding), spec) in self.shortcut_rows.borrow().iter().zip(&view.shortcuts)
            {
                row.set_visible(spec.visible);
                binding.set_text(&spec.shortcut);
                if spec.modified {
                    binding.add_css_class("heading");
                } else {
                    binding.remove_css_class("heading");
                }
            }
            // A closing dialog remains rooted during its animation. Present
            // on the model's closed -> open transition, even while rooted.
            if !was_open[0] {
                self.dialog.present(Some(&w.window));
                self.split.set_show_content(true);
            }
            if self.reveal.replace(view.reveal) != view.reveal
                && let Some(id) = view.reveal
                && let Some(field) = self.fields.borrow().get(&id)
            {
                // Native focus navigation scrolls the row into view. The core
                // supplies the page/target, independent of GTK's widget tree.
                field.widget().child_focus(gtk::DirectionType::TabForward);
            }
            if let Some(editor) = &view.shortcut_editor {
                let signature = serde_json::to_string(&(editor, &view.error)).unwrap();
                if *self.editor_signature.borrow() != signature {
                    while let Some(child) = self.editor_body.first_child() {
                        self.editor_body.remove(&child);
                    }
                    self.editor.set_title(&editor.label);
                    let description = gtk::Label::new(Some(&editor.group));
                    description.add_css_class("dim-label");
                    self.editor_body.append(&description);
                    let list = gtk::ListBox::new();
                    list.set_selection_mode(gtk::SelectionMode::None);
                    list.add_css_class("boxed-list");
                    for (index, binding) in editor.bindings.iter().enumerate() {
                        let row = text_row(binding, "");
                        let remove = action_button(
                            "Remove",
                            w,
                            UiAction::Preferences {
                                action: PreferenceAction::RemoveShortcut {
                                    id: editor.id.clone(),
                                    index,
                                },
                            },
                        );
                        remove.set_valign(gtk::Align::Center);
                        row.add_suffix(&remove);
                        list.append(&row);
                    }
                    self.editor_body.append(&list);
                    let defaults = gtk::Label::builder()
                        .label(format!(
                            "Default: {}",
                            if editor.defaults.is_empty() {
                                "Disabled".into()
                            } else {
                                editor.defaults.join(" / ")
                            }
                        ))
                        .wrap(true)
                        .xalign(0.0)
                        .build();
                    defaults.add_css_class("dim-label");
                    self.editor_body.append(&defaults);
                    if let Some(error) = &view.error {
                        let label = gtk::Label::builder().label(error).wrap(true).build();
                        label.add_css_class("error");
                        self.editor_body.append(&label);
                    }
                    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    for (label, action, enabled) in [
                        (
                            "Reset",
                            PreferenceAction::ResetShortcut {
                                id: editor.id.clone(),
                            },
                            editor.modified,
                        ),
                        (
                            "Add Shortcut",
                            PreferenceAction::BeginShortcut {
                                id: editor.id.clone(),
                            },
                            editor.can_add,
                        ),
                        ("Done", PreferenceAction::CloseShortcutEditor, true),
                    ] {
                        let button = action_button(label, w, UiAction::Preferences { action });
                        button.set_sensitive(enabled);
                        button.set_hexpand(true);
                        if label == "Add Shortcut" {
                            button.set_widget_name("add-shortcut");
                        }
                        buttons.append(&button);
                    }
                    self.editor_body.append(&buttons);
                    *self.editor_signature.borrow_mut() = signature;
                }
                if !was_open[1] {
                    self.editor.present(Some(&self.dialog));
                }
            }
            if let Some(capture) = view.capture {
                self.capture_label.set_text(&capture.label);
                self.capture_key.set_text(&capture.shortcut);
                self.capture_error.set_text(&capture.notice);
                self.capture_error
                    .set_visible(capture.error.is_some() || capture.conflict.is_some());
                self.confirm
                    .set_sensitive(capture.chord.is_some() && capture.error.is_none());
                self.confirm.set_label(if capture.conflict.is_some() {
                    "Replace Shortcut"
                } else {
                    "Set Shortcut"
                });
                if !was_open[2] {
                    self.capture.present(Some(if self.editor.root().is_some() {
                        &self.editor
                    } else {
                        &self.dialog
                    }));
                }
            }
        }
        for (index, dialog) in [&self.dialog, &self.editor, &self.capture]
            .into_iter()
            .enumerate()
            .rev()
        {
            if was_open[index] && !open[index] {
                dialog.close();
            }
        }
        self.updating.set(false);
    }
    pub fn recording(&self) -> bool {
        self.capture.root().is_some()
    }
    /// Ordered host services. Disk I/O runs on GIO's pool, never on the drawing
    /// event loop. A request stays in the core until the host acknowledges it.
    pub fn service(&self, w: &Rc<Workspace>) {
        if self.servicing.replace(true) {
            return;
        }
        // Finish an accepted settings edit even if the last window closes while
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
    let mut reader = serde_json::Deserializer::from_reader(file);
    let settings = Settings::deserialize_saved(&mut reader)
        .map_err(|e| format!("Cannot read preferences: {e}"))?;
    reader
        .end()
        .map_err(|e| format!("Cannot read preferences: {e}"))?;
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
