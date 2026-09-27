//! Native controls for the Rust preferences view. All edits and shortcut
//! recording go back to UiSession; no validation or keymap lives in GTK.
use crate::workspace::Workspace;
#[path = "shortcut_page.rs"]
mod shortcut_page;
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
    ImageChoice(gtk::ListBoxRow, crate::image_selector::ImageSelector),
    Circles(adw::ActionRow, crate::transparency_choice::TransparencyChoice),
    Swatches(gtk::ListBoxRow, Rc<crate::swatch_selector::SwatchSelector>),
    Spin(adw::SpinRow),
    Number(gtk::ListBoxRow, crate::number_control::NumberControl),
    Switch(adw::SwitchRow),
    Info(adw::ActionRow),
}
impl Field {
    fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Choice(w) => w.upcast_ref(),
            Self::ImageChoice(w, _) => w.upcast_ref(),
            Self::Circles(w, _) => w.upcast_ref(),
            Self::Swatches(w, _) => w.upcast_ref(),
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
            (Self::Choice(w), PreferenceKind::Choice { selected, .. }) => w.set_selected(*selected),
            (Self::ImageChoice(_, w), PreferenceKind::Choice { selected, .. }) => {
                w.set_selected(*selected)
            }
            (Self::Circles(_, w), PreferenceKind::Choice { selected, .. }) => w.set_selected(*selected),
            (
                Self::Swatches(_, w),
                PreferenceKind::Swatches {
                    swatches,
                    selected,
                    value,
                    custom,
                    ..
                },
            ) => w.update(swatches, *selected, value, custom),
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
    display: RefCell<Option<adw::ActionRow>>,
    groups: RefCell<Vec<(SettingsPage, usize, adw::PreferencesGroup)>>,
    shortcut_page: shortcut_page::ShortcutPage,
    shown: Cell<bool>,
    updating: Cell<bool>,
    context: gtk::PopoverMenu,
    context_reset: RefCell<Option<(PreferenceId, gtk::gio::SimpleAction)>>,
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

fn commit_swatch(w: &Rc<Workspace>, id: PreferenceId, selector: &crate::swatch_selector::SwatchSelector) {
    send(
        w,
        PreferenceAction::Edit {
            id,
            value: PreferenceValue::Text(selector.entry.text().into()),
        },
    );
    if w.gpu
        .borrow()
        .as_ref()
        .is_some_and(|g| g.session.state().preferences.error.is_none())
    {
        selector.entry.set_text(&selector.custom());
    }
}

fn titled_row(row: &PreferenceRow) -> (gtk::ListBoxRow, gtk::Box) {
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
    native_row.set_child(Some(&body));
    (native_row, body)
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
    hold.set_touch_only(false);
    hold.set_propagation_phase(gtk::PropagationPhase::Capture);
    hold.connect_pressed(glib::clone!(
        #[weak]
        w,
        move |gesture, x, y| {
            if !crate::input::touch_or_pen(gesture) { return; }
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
    let item = gtk::gio::MenuItem::new(
        Some(&format!("{} ({})", reset.label, reset.value)),
        Some("field.reset"),
    );
    if let Some(key) = w.gpu.borrow().as_ref().and_then(|g| {
        g.session.state().settings.action_keys(
            &UiAction::Preferences { action: PreferenceAction::Reset { id } },
            Platform::Gtk,
        ).into_iter().next()
    }) {
        item.set_attribute_value(
            "accel",
            Some(&crate::workspace::native_accelerator(&key).to_variant()),
        );
    }
    section.append_item(&item);
    root.append_section(None, &section);
    let action = gtk::gio::SimpleAction::new("reset", None);
    action.set_enabled(reset.enabled);
    action.connect_activate(glib::clone!(
        #[weak]
        w,
        move |_, _| {
            w.preferences.context.popdown();
            if let Some(Field::Number(_, number)) = w.preferences.fields.borrow().get(&id) {
                number.cancel_edit();
            }
            send(&w, PreferenceAction::Reset { id });
        }
    ));
    actions.add_action(&action);
    popup.insert_action_group("field", Some(&actions));
    popup.set_menu_model(Some(&root));
    *w.preferences.context_reset.borrow_mut() = Some((id, action));
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
        let shortcut_page = shortcut_page::ShortcutPage::new();
        header.pack_start(&shortcut_page.back);
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
        let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&sidebar_body)
            .vexpand(true)
            .build());
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
            display: RefCell::new(None),
            groups: RefCell::default(),
            shortcut_page,
            shown: Cell::new(false),
            updating: Cell::new(false),
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
                            let (native_row, body) = titled_row(row);
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
                            Field::ImageChoice(native_row, selector)
                        }
                        PreferenceKind::Choice {
                            options,
                            presentation: ChoicePresentation::Circles { .. },
                            ..
                        } => {
                            let native_row = text_row(&row.title, &row.description);
                            let choice = crate::transparency_choice::TransparencyChoice::new(
                                id.key(),
                                options,
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
                            native_row.add_suffix(&choice.widget);
                            Field::Circles(native_row, choice)
                        }
                        PreferenceKind::Swatches {
                            swatches,
                            placeholder,
                            inline,
                            ..
                        } => {
                            let selector = crate::swatch_selector::SwatchSelector::new(
                                &w.window.widget_name(),
                                id.key(),
                                &row.title,
                                swatches,
                                placeholder,
                                *inline,
                                glib::clone!(
                                    #[weak]
                                    w,
                                    move |value| send(
                                        &w,
                                        PreferenceAction::Edit {
                                            id,
                                            value: PreferenceValue::Text(value)
                                        }
                                    )
                                ),
                            );
                            let weak = Rc::downgrade(&selector);
                            selector.entry.connect_activate(glib::clone!(
                                #[weak]
                                w,
                                move |_| {
                                    if let Some(selector) = weak.upgrade() {
                                        commit_swatch(&w, id, &selector);
                                    }
                                }
                            ));
                            let weak = Rc::downgrade(&selector);
                            selector.focus.connect_leave(glib::clone!(
                                #[weak]
                                w,
                                move |_| {
                                    if let Some(selector) = weak.upgrade()
                                        && selector.entry.text().trim() != selector.custom()
                                    {
                                        commit_swatch(&w, id, &selector);
                                    }
                                }
                            ));
                            let native_row = if *inline {
                                let native_row = text_row(&row.title, &row.description);
                                native_row.add_suffix(&selector.widget);
                                native_row.upcast()
                            } else {
                                let (native_row, body) = titled_row(row);
                                body.append(&selector.widget);
                                native_row
                            };
                            Field::Swatches(native_row, selector)
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
                                        crate::icons::set(
                                            &image,
                                            Some(&format!("layer-{}-symbolic", icons[index])),
                                        );
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
            let child: gtk::Widget = match page.id {
                SettingsPage::Shortcuts => {
                    self.shortcut_page.build(w, &content, &self.shortcut_search, &self.content_page).upcast()
                }
                SettingsPage::Input => self.shortcut_page.build_triggers(w, &content).upcast(),
                _ => content.clone().upcast(),
            };
            if page.id == SettingsPage::Color {
                let group = adw::PreferencesGroup::new();
                let row = text_row("Drawing defaults and presets", "Choose dimensions, use a saved preset, or manage your drawing presets.");
                let button = gtk::Button::with_label("Configure…");
                button.set_widget_name("color-drawing-defaults");
                button.set_valign(gtk::Align::Center);
                button.connect_clicked(glib::clone!(#[weak] w, move |_| {
                    glib::MainContext::default().spawn_local(glib::clone!(#[strong] w, async move {
                        if let Err(error) = crate::new_document::configure(&w, true).await { w.changed(Err(error)); }
                    }));
                }));
                row.add_suffix(&button);
                row.set_activatable_widget(Some(&button));
                group.add(&row);
                let row = text_row("Color profiles", "Choose which profiles appear in profile menus.");
                let button = gtk::Button::with_label("Manage…");
                button.set_widget_name("color-profile-library");
                button.set_valign(gtk::Align::Center);
                button.connect_clicked(glib::clone!(#[weak] w, move |_| {
                    glib::MainContext::default().spawn_local(glib::clone!(#[strong] w, async move {
                        if let Err(error) = crate::files::profile::manage(&w).await { w.changed(Err(error)); }
                    }));
                }));
                row.add_suffix(&button);
                row.set_activatable_widget(Some(&button));
                group.add(&row);
                let display = text_row("Canvas display", &w.display_description());
                display.set_widget_name("color-display-details");
                group.add(&display);
                *self.display.borrow_mut() = Some(display);
                content.add(&group);
            }
            self.stack.add_titled_with_icon(
                &child,
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
        if let Some((id, action)) = self.context_reset.borrow().as_ref() {
            action.set_enabled(
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
        if let Some(row) = self.display.borrow().as_ref() { row.set_subtitle(&w.display_description()); }
        let open = view.is_some();
        let was_open = self.shown.replace(open);
        if !open {
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
            self.shortcut_page.refresh(w, &self.dialog, &view);
            // A closing dialog remains rooted during its animation. Present
            // on the model's closed -> open transition, even while rooted.
            if !was_open {
                self.dialog.present(Some(&w.window));
                self.split.set_show_content(true);
                // A retained dialog can remember a focus widget that was
                // hidden on close. Re-establish native focus after presenting
                // the content page so Tab and Escape work on every opening.
                self.content_view.child_focus(gtk::DirectionType::TabForward);
            }
            if self.reveal.replace(view.reveal) != view.reveal
                && let Some(id) = view.reveal
                && let Some(field) = self.fields.borrow().get(&id)
            {
                // Native focus navigation scrolls the row into view. The core
                // supplies the page/target, independent of GTK's widget tree.
                field.widget().child_focus(gtk::DirectionType::TabForward);
            }
        }
        if !open {
            self.shortcut_page.close_dialogs();
            if was_open {
                self.dialog.close();
            }
        }
        self.updating.set(false);
    }
    pub fn recording(&self) -> bool {
        self.shortcut_page.recording()
    }
    pub fn editing_shortcut(&self) -> bool {
        self.shortcut_page.editing()
    }
}

pub(crate) async fn export_keymap(w: &Rc<Workspace>, name: String, text: String) -> Result<(), String> {
    let dialog = gtk::FileDialog::builder().title("Export Keymap").initial_name(name.as_str()).build();
    let file = match dialog.save_future(Some(&w.window)).await {
        Ok(file) => file,
        Err(e) if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    let path = file.path().ok_or("Choose a local file")?;
    gtk::gio::spawn_blocking(move || std::fs::write(path, text).map_err(|e| e.to_string()))
        .await
        .map_err(|_| "Could not save the keymap".to_string())?
}

pub(crate) async fn import_keymap(w: &Rc<Workspace>) -> Result<(), String> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Keymaps"));
    filter.add_suffix("capykeys");
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Import Keymap").filters(&filters).build();
    let file = match dialog.open_future(Some(&w.window)).await {
        Ok(file) => file,
        Err(e) if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    let path = file.path().ok_or("Choose a local keymap file")?;
    let text = gtk::gio::spawn_blocking(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|f| f.take(1 << 20).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "The keymap is not UTF-8 text".to_string())
    })
    .await
    .map_err(|_| "Could not read the keymap".to_string())??;
    send(w, PreferenceAction::ImportKeymap { text });
    Ok(())
}

pub(crate) async fn persist(w: &Workspace, settings: Box<Settings>) -> Result<(), String> {
    if !w
        .gpu
        .borrow()
        .as_ref()
        .is_some_and(|g| g.session.state().settings == *settings)
    {
        return Ok(());
    }
    if let Some(action) = w
        .window
        .application()
        .and_then(|app| app.lookup_action("settings-changed"))
    {
        action.activate(Some(
            &serde_json::to_string(&settings).unwrap().to_variant(),
        ));
    }
    let revision = SAVE_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    gtk::gio::spawn_blocking(move || {
        let _lock = SAVE_LOCK.lock().map_err(|e| e.to_string())?;
        if SAVE_REVISION.load(std::sync::atomic::Ordering::Relaxed) != revision {
            return Ok(());
        }
        save(&settings)
    })
    .await
    .unwrap_or_else(|_| Err("Settings writer failed".into()))
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
    let settings: Settings = serde_json::from_reader(file)
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
