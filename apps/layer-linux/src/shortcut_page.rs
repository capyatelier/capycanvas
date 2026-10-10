use super::{send, text_row};
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::keymaps::KeymapView;
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

struct ShortcutWidget {
    row: adw::ActionRow,
    keys: gtk::Label,
    reset: gtk::Button,
    signature: RefCell<String>,
}

struct Results {
    page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    layout: RefCell<String>,
}

struct Sheet {
    dialog: adw::Dialog,
    title: adw::WindowTitle,
    header: adw::HeaderBar,
    view: adw::ToolbarView,
    shown: Cell<bool>,
}

impl Sheet {
    fn new(name: &str, height: i32) -> Self {
        let dialog = adw::Dialog::builder().content_width(460).content_height(height).build();
        dialog.add_css_class("layer-preferences");
        dialog.set_widget_name(name);
        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        dialog.set_child(Some(&view));
        Self { dialog, title, header, view, shown: Cell::new(false) }
    }

    /// These sheets apply changes immediately, so a click beside one only closes it.
    fn dismiss_outside(&self, dismiss: impl Fn() + 'static) {
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let view = self.view.clone();
        let dialog = self.dialog.clone();
        click.connect_pressed(move |gesture, _, x, y| {
            let inside = view
                .compute_bounds(&dialog)
                .is_some_and(|b| b.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)));
            if !inside {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                dismiss();
            }
        });
        self.dialog.add_controller(click);
    }

    fn show(&self, open: bool, parent: &adw::Dialog) -> bool {
        match (self.shown.replace(open), open) {
            (false, true) => {
                self.dialog.present(Some(parent));
                true
            }
            (true, false) => {
                self.dialog.close();
                false
            }
            _ => false,
        }
    }
}

/// A sheet sized to its content: a summary line, groups, and inline recording.
struct Form {
    sheet: Sheet,
    summary: gtk::Label,
    error: gtk::Label,
    body: gtk::Box,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    signature: RefCell<String>,
    recording: RefCell<Option<adw::ActionRow>>,
}

impl Form {
    fn new(name: &str) -> Self {
        let sheet = Sheet::new(name, -1);
        let summary = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
        summary.add_css_class("dim-label");
        let error = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
        error.add_css_class("error");
        error.set_widget_name(&format!("{name}-error"));
        let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
        body.append(&summary);
        body.append(&error);
        body.set_margin_bottom(24);
        body.set_margin_start(12);
        body.set_margin_end(12);
        sheet.view.set_content(Some(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .child(&body)
                .build(),
        ));
        Self {
            sheet,
            summary,
            error,
            body,
            groups: RefCell::default(),
            signature: RefCell::default(),
            recording: RefCell::default(),
        }
    }

    fn texts(&self, title: &str, summary: &str, error: Option<&str>) {
        self.sheet.dialog.set_title(title);
        self.sheet.title.set_title(title);
        self.summary.set_text(summary);
        self.summary.set_visible(!summary.is_empty());
        self.error.set_text(error.unwrap_or(""));
        self.error.set_visible(error.is_some());
    }

    fn rebuild(&self, signature: String, build: impl FnOnce() -> Vec<adw::PreferencesGroup>) {
        if self.signature.replace(signature.clone()) == signature {
            return;
        }
        for group in self.groups.borrow_mut().drain(..) {
            self.body.remove(&group);
        }
        self.recording.borrow_mut().take();
        for group in build() {
            self.body.append(&group);
            self.groups.borrow_mut().push(group);
        }
    }

    fn present(&self, dialog: &adw::Dialog) {
        let opened = self.sheet.show(true, dialog);
        let (_, natural, _, _) = self.sheet.view.measure(gtk::Orientation::Vertical, self.sheet.dialog.content_width());
        if self.sheet.dialog.content_height() != natural {
            self.sheet.dialog.set_content_height(natural);
        }
        let focused = self.sheet.dialog.root().and_then(|root| root.focus()).is_some_and(|f| f.is_ancestor(&self.sheet.dialog));
        if let Some(row) = self.recording.borrow().as_ref().filter(|_| opened || !focused) {
            row.set_focusable(true);
            row.grab_focus();
        } else if !focused {
            self.body.child_focus(gtk::DirectionType::TabForward);
        }
    }

    fn close(&self, dialog: &adw::Dialog) {
        self.sheet.show(false, dialog);
    }
}

/// A slide page listing what a key or button does with each kind of tool.
struct PerTool {
    prefix: &'static str,
    page: adw::NavigationPage,
    content: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    signature: RefCell<String>,
}

struct PerToolView<'a> {
    title: &'a str,
    summary: String,
    per_tool: bool,
    actions: &'a [ModifierKeyAction],
    modified: bool,
    reset: PreferenceAction,
    same: Rc<dyn Fn(bool) -> PreferenceAction>,
    pick: Rc<dyn Fn(Option<ToolCategory>) -> PreferenceAction>,
    remove: Option<(&'a str, PreferenceAction)>,
}

impl PerTool {
    fn new(prefix: &'static str) -> Self {
        let content = adw::PreferencesPage::new();
        content.set_widget_name(&format!("{prefix}-page"));
        let page = adw::NavigationPage::with_tag(&content, "", prefix);
        Self { prefix, page, content, groups: RefCell::default(), signature: RefCell::default() }
    }

    fn render(&self, w: &Rc<Workspace>, view: PerToolView) {
        self.page.set_title(view.title);
        let signature = serde_json::to_string(&(view.per_tool, view.actions.iter().map(|a| a.category).collect::<Vec<_>>(), view.modified, view.remove.is_some())).unwrap();
        if self.signature.replace(signature.clone()) == signature {
            self.relabel(w, &view);
            return;
        }
        for group in self.groups.borrow_mut().drain(..) {
            self.content.remove(&group);
        }
        let group = adw::PreferencesGroup::new();
        group.set_description(Some(&glib::markup_escape_text(&view.summary)));
        if view.modified {
            let reset = icon_button("edit-undo-symbolic", "", &format!("{}-reset", self.prefix));
            let action = view.reset.clone();
            reset.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, action.clone())
            ));
            group.set_header_suffix(Some(&reset));
        }
        let same = adw::SwitchRow::builder().title("").active(!view.per_tool).build();
        same.set_widget_name(&format!("{}-same", self.prefix));
        let toggle = view.same.clone();
        same.connect_active_notify(glib::clone!(
            #[weak]
            w,
            move |row| send(&w, toggle(!row.is_active()))
        ));
        group.add(&same);
        for action in view.actions {
            let row = text_row(&action.label, "");
            let category = action.category.map_or("all".into(), |c| format!("{c:?}").to_lowercase());
            row.set_widget_name(&format!("{}-action-{category}", self.prefix));
            row.set_activatable(true);
            let value = gtk::Label::new(Some(&action.action));
            value.set_widget_name(&format!("{}-value-{category}", self.prefix));
            value.add_css_class("dim-label");
            row.add_suffix(&value);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let (pick, category) = (view.pick.clone(), action.category);
            row.connect_activated(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, pick(category))
            ));
            group.add(&row);
        }
        self.content.add(&group);
        self.groups.borrow_mut().push(group);
        if let Some((title, action)) = &view.remove {
            let action = action.clone();
            let remove = adw::PreferencesGroup::new();
            let row = button_row(title, "user-trash-symbolic", &format!("{}-remove", self.prefix), glib::clone!(
                #[weak]
                w,
                move || send(&w, action.clone())
            ));
            row.add_css_class("destructive-action");
            remove.add(&row);
            self.content.add(&remove);
            self.groups.borrow_mut().push(remove);
        }
        self.relabel(w, &view);
    }

    fn relabel(&self, w: &Workspace, view: &PerToolView) {
        let localization = w.localization();
        if let Some(group) = self.groups.borrow().first() {
            group.set_description(Some(&glib::markup_escape_text(&view.summary)));
        }
        relabel_controls(self.content.upcast_ref(), &localization);
        for action in view.actions {
            let category = action.category.map_or("all".into(), |c| format!("{c:?}").to_lowercase());
            crate::text_language::visit(self.content.upcast_ref(), &mut |widget| {
                if widget.widget_name().as_str() == format!("{}-action-{category}", self.prefix) {
                    if let Some(row) = widget.downcast_ref::<adw::ActionRow>() { row.set_title(&action.label); }
                } else if widget.widget_name().as_str() == format!("{}-value-{category}", self.prefix) {
                    if let Some(label) = widget.downcast_ref::<gtk::Label>() { label.set_text(&action.action); }
                }
            });
        }
    }
}

fn sync_stack(navigation: &adw::NavigationView, desired: &[adw::NavigationPage]) {
    let model = navigation.navigation_stack();
    let stack: Vec<adw::NavigationPage> = (0..model.n_items()).filter_map(|i| model.item(i).and_downcast()).collect();
    if stack == desired {
        return;
    }
    if desired.len() < stack.len() && stack.starts_with(desired) {
        navigation.pop_to_page(desired.last().unwrap());
    } else if desired.starts_with(&stack) {
        for page in &desired[stack.len()..] {
            navigation.push(page);
        }
    } else {
        navigation.replace(desired);
    }
}

pub(super) struct ShortcutPage {
    pub(super) back: gtk::Button,
    navigation: adw::NavigationView,
    outer: RefCell<Option<adw::NavigationPage>>,
    category_page: adw::NavigationPage,
    main_results: RefCell<Option<Results>>,
    category_results: Results,
    keymap_combo: adw::ComboRow,
    keymap_group: adw::PreferencesGroup,
    keymap_ids: RefCell<Vec<String>>,
    details: Sheet,
    details_body: gtk::Box,
    details_signature: RefCell<String>,
    import: RefCell<Option<adw::AlertDialog>>,
    context: gtk::DropDown,
    contexts: RefCell<Vec<Option<ToolCategory>>>,
    show: gtk::DropDown,
    shows: RefCell<Vec<ShortcutShow>>,
    categories: gtk::ListBox,
    category_rows: RefCell<Vec<(String, adw::ActionRow, gtk::Label)>>,
    rows: RefCell<HashMap<String, ShortcutWidget>>,
    status: adw::StatusPage,
    input_page: RefCell<Option<adw::PreferencesPage>>,
    trigger_groups: RefCell<Vec<(String, adw::PreferencesGroup)>>,
    trigger_rows: RefCell<Vec<(String, adw::ActionRow, gtk::Label)>>,
    picker: Sheet,
    picker_search: gtk::SearchEntry,
    picker_page: adw::PreferencesPage,
    picker_groups: RefCell<Vec<adw::PreferencesGroup>>,
    picker_reset: gtk::Button,
    picker_signature: RefCell<String>,
    editor: Form,
    modifier: Form,
    modifier_page: PerTool,
    root: RefCell<Option<adw::NavigationPage>>,
    input_navigation: adw::NavigationView,
    input_root: RefCell<Option<adw::NavigationPage>>,
    pen_page: PerTool,
    picker_description: gtk::Label,
    modifiers_main: adw::PreferencesGroup,
    modifiers_category: adw::PreferencesGroup,
    modifier_rows: RefCell<Vec<adw::PreferencesRow>>,
    modifier_signature: RefCell<String>,
    recording: Cell<bool>,
}

fn current(w: &Workspace) -> Option<PreferencesView> {
    w.gpu.borrow().as_ref()?.session.preferences()
}

fn subtitle(row: &ShortcutRow) -> String {
    [row.detail.as_str(), &row.scope_caption].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn is_text_editing(key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> bool {
    use gtk::gdk::{Key, ModifierType as M};
    let only_control = modifiers & (M::CONTROL_MASK | M::ALT_MASK | M::SUPER_MASK | M::SHIFT_MASK) == M::CONTROL_MASK;
    only_control
        && matches!(
            key.to_lower(),
            Key::a | Key::c | Key::v | Key::x | Key::BackSpace | Key::Delete | Key::Left | Key::Right | Key::Home | Key::End
        )
}

fn detach(row: &adw::ActionRow) {
    if let Some(group) = row.ancestor(adw::PreferencesGroup::static_type()).and_downcast::<adw::PreferencesGroup>() {
        group.remove(row);
    }
}

fn icon_button(icon: &str, tooltip: &str, name: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tooltip));
    button.set_widget_name(name);
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    button.add_css_class("circular");
    button
}

fn dropdown(name: &str, tooltip: &str) -> gtk::DropDown {
    let dropdown = gtk::DropDown::from_strings(&[]);
    dropdown.set_widget_name(name);
    dropdown.set_tooltip_text(Some(tooltip));
    dropdown.set_valign(gtk::Align::Center);
    dropdown
}

fn refresh_options(dropdown: &gtk::DropDown, labels: &[&str]) {
    if let Some(model) = dropdown.model().and_downcast::<gtk::StringList>() {
        if model.n_items() as usize != labels.len() || labels.iter().enumerate().any(|(index, label)| model.string(index as u32).as_deref() != Some(*label)) {
            model.splice(0, model.n_items(), labels);
        }
    } else {
        dropdown.set_model(Some(&gtk::StringList::new(labels)));
    }
}

fn button_row(title: &str, icon: &str, name: &str, activate: impl Fn() + 'static) -> adw::ButtonRow {
    let row = adw::ButtonRow::builder().title(title).start_icon_name(icon).build();
    row.set_widget_name(name);
    row.connect_activated(move |_| activate());
    row
}

fn recording_row(w: &Rc<Workspace>, capture: &ShortcutCapture) -> adw::ActionRow {
    let row = text_row("", "");
    row.set_widget_name("shortcut-recording");
    let icon = gtk::Image::new();
    icon.set_widget_name("shortcut-recording-icon");
    row.add_prefix(&icon);
    let cancel = gtk::Button::new();
    cancel.set_widget_name("cancel-shortcut");
    cancel.set_valign(gtk::Align::Center);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| send(&w, PreferenceAction::CancelShortcut)
    ));
    let confirm = gtk::Button::new();
    confirm.set_widget_name("confirm-shortcut");
    confirm.set_valign(gtk::Align::Center);
    confirm.add_css_class("suggested-action");
    confirm.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| send(&w, PreferenceAction::ConfirmShortcut { replace: current(&w).and_then(|view| view.capture).is_some_and(|capture| capture.conflict.is_some()) })
    ));
    row.add_suffix(&cancel);
    row.add_suffix(&confirm);
    refresh_recording(&row, capture, &w.localization());
    row
}

fn refresh_recording(row: &adw::ActionRow, capture: &ShortcutCapture, localization: &Localizer) {
    row.set_title(&capture.shortcut);
    row.set_subtitle(&capture.notice);
    crate::text_language::visit(row.upcast_ref(), &mut |widget| match widget.widget_name().as_str() {
        "cancel-shortcut" => if let Some(button) = widget.downcast_ref::<gtk::Button>() { button.set_label(&localization.text(MessageId::COMMON_CANCEL)); },
        "confirm-shortcut" => if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            button.set_label(&localization.text(match (capture.existing, capture.conflict.is_some()) {
                (true, _) => MessageId::NATIVE_SHORTCUTS_OPEN,
                (false, true) => MessageId::NATIVE_SHORTCUTS_REASSIGN,
                (false, false) => MessageId::NATIVE_SHORTCUTS_ADD,
            }));
            button.set_sensitive(capture.chord.is_some() && capture.error.is_none());
        },
        "shortcut-recording-icon" => if let Some(icon) = widget.downcast_ref::<gtk::Image>() {
            icon.set_icon_name(Some(match (capture.existing, capture.notice.is_empty()) {
                (true, _) => "dialog-information-symbolic",
                (false, true) => "input-keyboard-symbolic",
                (false, false) => "dialog-warning-symbolic",
            }));
            if !capture.existing && !capture.notice.is_empty() { icon.add_css_class("warning"); }
            else { icon.remove_css_class("warning"); }
        },
        _ => {},
    });
}

fn relabel_controls(widget: &gtk::Widget, localization: &Localizer) {
    crate::text_language::visit(widget, &mut |widget| {
        let message = match widget.widget_name().as_str() {
            "shortcut-editor-reset" | "modifier-reset" | "pen-button-reset" => Some(MessageId::SETTINGS_RESET_TO_DEFAULT),
            name if name.starts_with("remove-shortcut-") => Some(MessageId::NATIVE_SHORTCUTS_REMOVE_SHORTCUT),
            "add-shortcut" => Some(MessageId::NATIVE_SHORTCUTS_ADD_SHORTCUT),
            "add-modifier-key" => Some(MessageId::NATIVE_SHORTCUTS_ADD_MODIFIER),
            "modifier-remove" => Some(MessageId::NATIVE_SHORTCUTS_REMOVE_MODIFIER),
            "modifier-same" | "pen-button-same" => Some(MessageId::NATIVE_SHORTCUTS_SAME_ALL_TOOLS),
            _ => None,
        };
        let Some(message) = message else { return; };
        let text = localization.text(message);
        if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() { row.set_title(&text); }
        else if let Some(button) = widget.downcast_ref::<gtk::Button>() { button.set_tooltip_text(Some(&text)); }
    });
}

impl Results {
    fn new(page: adw::PreferencesPage) -> Self {
        Self { page, groups: RefCell::default(), layout: RefCell::default() }
    }

    fn show(&self, owner: &ShortcutPage, w: &Rc<Workspace>, layout: &[(String, Vec<&ShortcutRow>)]) {
        let signature = serde_json::to_string(
            &layout.iter().map(|(_, rows)| rows.iter().map(|r| &r.id).collect::<Vec<_>>()).collect::<Vec<_>>(),
        )
        .unwrap();
        if self.layout.replace(signature.clone()) == signature {
            for (group, (title, _)) in self.groups.borrow().iter().zip(layout) {
                group.set_title(title);
            }
            for spec in layout.iter().flat_map(|(_, specs)| specs) {
                owner.shortcut_row(w, spec);
            }
            return;
        }
        for widget in owner.rows.borrow().values() {
            if widget.row.is_ancestor(&self.page) {
                detach(&widget.row);
            }
        }
        for group in self.groups.borrow_mut().drain(..) {
            self.page.remove(&group);
        }
        for (title, specs) in layout {
            let group = adw::PreferencesGroup::new();
            group.set_title(title);
            for spec in specs {
                let row = owner.shortcut_row(w, spec);
                detach(&row);
                group.add(&row);
            }
            self.page.add(&group);
            self.groups.borrow_mut().push(group);
        }
    }
}

impl ShortcutPage {
    pub(super) fn new() -> Self {
        let back = gtk::Button::from_icon_name("go-previous-symbolic");
        back.set_widget_name("shortcut-category-back");
        back.set_visible(false);
        let navigation = adw::NavigationView::new();
        navigation.set_widget_name("shortcut-navigation");
        let category_content = adw::PreferencesPage::new();
        category_content.set_widget_name("shortcut-category-page");
        let category_page = adw::NavigationPage::with_tag(&category_content, "", "category");
        let keymap_combo = adw::ComboRow::new();
        keymap_combo.set_widget_name("keymap-preset");
        let keymap_group = adw::PreferencesGroup::new();
        let details = Sheet::new("keymap-details", 600);
        let details_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        super::margins(&details_body, 12);
        details.view.set_content(Some(&details_body));
        let picker = Sheet::new("action-picker", 600);
        let picker_reset = gtk::Button::new();
        picker_reset.set_widget_name("action-picker-reset");
        picker.header.pack_start(&picker_reset);
        let picker_search = gtk::SearchEntry::builder().hexpand(true).build();
        picker_search.set_widget_name("action-picker-search");
        let picker_description = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
        picker_description.add_css_class("dim-label");
        picker_description.set_widget_name("action-picker-description");
        let top = gtk::Box::new(gtk::Orientation::Vertical, 12);
        top.append(&picker_description);
        top.append(&picker_search);
        let search_bar = adw::Clamp::builder().maximum_size(400).child(&top).build();
        search_bar.set_margin_top(6);
        search_bar.set_margin_bottom(6);
        search_bar.set_margin_start(12);
        search_bar.set_margin_end(12);
        picker.view.add_top_bar(&search_bar);
        let picker_page = adw::PreferencesPage::new();
        picker.view.set_content(Some(&picker_page));
        let editor = Form::new("shortcut-editor");
        let modifier = Form::new("modifier-key");
        let modifiers_main = adw::PreferencesGroup::new();
        modifiers_main.set_widget_name("modifier-results");
        let modifiers_category = adw::PreferencesGroup::new();
        modifiers_category.set_widget_name("modifier-keys");
        category_content.add(&modifiers_category);
        let categories = gtk::ListBox::new();
        categories.set_selection_mode(gtk::SelectionMode::None);
        categories.add_css_class("boxed-list");
        categories.set_widget_name("shortcut-categories");
        let status = adw::StatusPage::builder().icon_name("edit-find-symbolic").build();
        status.add_css_class("compact");
        status.set_widget_name("shortcut-empty");
        Self {
            back,
            navigation,
            outer: RefCell::default(),
            category_page,
            main_results: RefCell::default(),
            category_results: Results::new(category_content),
            keymap_combo,
            keymap_group,
            keymap_ids: RefCell::default(),
            details,
            details_body,
            details_signature: RefCell::default(),
            import: RefCell::default(),
            context: dropdown("shortcut-context", ""),
            contexts: RefCell::default(),
            show: dropdown("shortcut-show", ""),
            shows: RefCell::default(),
            categories,
            category_rows: RefCell::default(),
            rows: RefCell::default(),
            status,
            input_page: RefCell::default(),
            trigger_groups: RefCell::default(),
            trigger_rows: RefCell::default(),
            picker,
            picker_search,
            picker_page,
            picker_groups: RefCell::default(),
            picker_reset,
            picker_signature: RefCell::default(),
            editor,
            modifier,
            modifier_page: PerTool::new("modifier"),
            root: RefCell::default(),
            input_navigation: {
                let navigation = adw::NavigationView::new();
                navigation.set_widget_name("input-navigation");
                navigation
            },
            input_root: RefCell::default(),
            pen_page: PerTool::new("pen-button"),
            picker_description,
            modifiers_main,
            modifiers_category,
            modifier_rows: RefCell::default(),
            modifier_signature: RefCell::default(),
            recording: Cell::new(false),
        }
    }

    pub(super) fn recording(&self) -> bool {
        self.recording.get()
    }

    pub(super) fn editing(&self) -> bool {
        self.editor.sheet.shown.get() || self.modifier.sheet.shown.get()
    }

    pub(super) fn build(
        &self,
        w: &Rc<Workspace>,
        content: &adw::PreferencesPage,
        search: &gtk::SearchEntry,
        outer: &adw::NavigationPage,
    ) -> adw::NavigationView {
        *self.outer.borrow_mut() = Some(outer.clone());
        *self.main_results.borrow_mut() = Some(Results::new(content.clone()));
        let quiet = |w: &Rc<Workspace>| w.preferences.updating.get();
        self.back.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let Some(view) = current(&w) else { return };
                send(&w, if view.page == SettingsPage::Input && view.pen_button_editor.is_some() {
                    PreferenceAction::ClosePenButton
                } else if view.modifier_editor.is_some() {
                    PreferenceAction::CloseModifierKey
                } else {
                    PreferenceAction::ShortcutCategory { id: None }
                });
            }
        ));
        let root = adw::NavigationPage::with_tag(content, "", "shortcuts");
        self.navigation.add(&root);
        *self.root.borrow_mut() = Some(root);
        self.navigation.connect_popped(glib::clone!(
            #[weak]
            w,
            move |_, popped| {
                let Some(view) = current(&w) else { return };
                if *popped == w.preferences.shortcut_page.modifier_page.page && view.modifier_editor.is_some() {
                    send(&w, PreferenceAction::CloseModifierKey);
                } else if *popped == w.preferences.shortcut_page.category_page && view.shortcut_page.category.is_some() {
                    send(&w, PreferenceAction::ShortcutCategory { id: None });
                }
            }
        ));
        self.keymap_combo.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |combo| {
                if quiet(&w) {
                    return;
                }
                let id = w.preferences.shortcut_page.keymap_ids.borrow().get(combo.selected() as usize).cloned();
                if let Some(id) = id {
                    send(&w, PreferenceAction::SelectKeymap { id });
                }
            }
        ));
        let menu = gtk::gio::Menu::new();
        let actions = gtk::gio::SimpleActionGroup::new();
        for (name, action) in [
            ("import", PreferenceAction::ChooseKeymapFile),
            ("export", PreferenceAction::ExportKeymap),
            ("details", PreferenceAction::KeymapDetails { open: true }),
            ("reset", PreferenceAction::ResetAllShortcuts),
        ] {
            let simple = gtk::gio::SimpleAction::new(name, None);
            simple.connect_activate(glib::clone!(
                #[weak]
                w,
                move |_, _| send(&w, action.clone())
            ));
            actions.add_action(&simple);
        }
        let more = gtk::MenuButton::builder().icon_name("view-more-symbolic").menu_model(&menu).build();
        more.set_widget_name("keymap-menu");
        more.set_valign(gtk::Align::Center);
        more.add_css_class("flat");
        more.insert_action_group("keymap", Some(&actions));
        self.keymap_combo.add_suffix(&more);
        self.keymap_group.add(&self.keymap_combo);
        content.add(&self.keymap_group);

        search.set_hexpand(true);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                use gtk::gdk::ModifierType as M;
                let named = key.name().is_some_and(|n| {
                    (n.starts_with('F') && n[1..].parse::<u8>().is_ok()) || n.starts_with("XF86") || n.starts_with("Audio")
                });
                let chorded = modifiers.intersects(M::CONTROL_MASK | M::ALT_MASK | M::SUPER_MASK | M::META_MASK);
                if (!chorded && !named) || is_text_editing(key, modifiers) {
                    return glib::Propagation::Proceed;
                }
                let UiInput::Key { key: name, modifiers, .. } = crate::input::key_input(key, true, modifiers, false, None) else {
                    return glib::Propagation::Proceed;
                };
                let chord = KeyChord::new(&name, modifiers);
                if KeyChord::modifier(&chord.key) {
                    return glib::Propagation::Proceed;
                }
                send(&w, PreferenceAction::SearchShortcutKey { chord });
                glib::Propagation::Stop
            }
        ));
        search.add_controller(keys);
        self.context.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |dropdown| {
                if quiet(&w) {
                    return;
                }
                let category = w.preferences.shortcut_page.contexts.borrow().get(dropdown.selected() as usize).copied();
                if let Some(category) = category {
                    send(&w, PreferenceAction::ShortcutContext { category });
                }
            }
        ));
        self.show.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |dropdown| {
                if quiet(&w) {
                    return;
                }
                let show = w.preferences.shortcut_page.shows.borrow().get(dropdown.selected() as usize).copied();
                if let Some(show) = show {
                    send(&w, PreferenceAction::ShortcutShow { show });
                }
            }
        ));
        let filters = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        filters.set_widget_name("shortcut-filters");
        filters.append(search);
        filters.append(&self.context);
        filters.append(&self.show);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.append(&filters);
        body.append(&self.categories);
        body.append(&self.status);
        let group = adw::PreferencesGroup::new();
        group.add(&body);
        content.add(&group);

        self.picker_search.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if !quiet(&w) {
                    send(&w, PreferenceAction::SearchActionPicker { query: entry.text().into() });
                }
            }
        ));
        let picker = self.picker.dialog.clone();
        self.picker_search.connect_stop_search(move |_| {
            picker.close();
        });
        self.picker_reset.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let trigger = current(&w).and_then(|v| v.shortcut_page.picker).map(|p| p.trigger);
                if let Some(trigger) = trigger {
                    send(&w, PreferenceAction::ResetTrigger { trigger });
                }
            }
        ));
        self.picker.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if current(&w).is_some_and(|v| v.shortcut_page.picker.is_some()) {
                    send(&w, PreferenceAction::CloseActionPicker);
                }
            }
        ));
        self.details.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if current(&w).is_some_and(|v| v.keymap.details) {
                    send(&w, PreferenceAction::KeymapDetails { open: false });
                }
            }
        ));
        for form in [&self.editor, &self.modifier] {
            let dialog = form.sheet.dialog.clone();
            form.sheet.dismiss_outside(glib::clone!(
                #[weak]
                w,
                move || {
                    if w.preferences.shortcut_page.recording() {
                        send(&w, PreferenceAction::CancelShortcut);
                    } else {
                        dialog.close();
                    }
                }
            ));
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                w,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, modifiers| {
                    if !w.preferences.shortcut_page.recording() {
                        return glib::Propagation::Proceed;
                    }
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
            form.sheet.dialog.add_controller(keys);
            form.sheet.dialog.add_controller(crate::input::pad_buttons(w));
        }
        for sheet in [&self.picker, &self.details] {
            let dialog = sheet.dialog.clone();
            sheet.dismiss_outside(move || {
                dialog.close();
            });
        }
        self.editor.sheet.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if current(&w).is_some_and(|v| v.shortcut_editor.is_some()) {
                    send(&w, PreferenceAction::CloseShortcutEditor);
                }
            }
        ));
        self.modifier.sheet.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                let open = current(&w).is_some_and(|v| {
                    v.modifier_editor.is_some() || v.capture.is_some_and(|c| c.id == MODIFIER_CAPTURE)
                });
                if open {
                    send(&w, PreferenceAction::CloseModifierKey);
                }
            }
        ));
        content.add(&self.modifiers_main);
        w.on_localization(glib::clone!(
            #[weak] w,
            #[weak] menu,
            #[weak] more,
            #[weak] group,
            #[weak] search,
            #[upgrade_or] false,
            move |localization| {
                let copy = NativeCopy::new(localization).shortcuts;
                let page = &w.preferences.shortcut_page;
                if let Some(root) = page.root.borrow().as_ref() { root.set_title(&copy.title); }
                page.back.set_tooltip_text(Some(&copy.title));
                page.keymap_group.set_title(&copy.keymap);
                page.keymap_combo.set_title(&copy.preset);
                page.picker_reset.set_label(&localization.text(MessageId::COMMON_RESET));
                page.picker_search.set_placeholder_text(Some(&copy.search_actions));
                page.context.set_tooltip_text(Some(&copy.tool_shortcuts));
                page.show.set_tooltip_text(Some(&copy.choose_actions));
                page.modifiers_main.set_title(&localization.text(MessageId::SHORTCUT_MODIFIER_KEYS));
                page.modifiers_category.set_description(Some(&copy.hold_key_help));
                group.set_title(&copy.title);
                search.set_placeholder_text(Some(&copy.search_or_press));
                more.set_tooltip_text(Some(&copy.keymap_options));
                menu.remove_all();
                for (name, label) in [("import", &copy.import_menu), ("export", &copy.export_menu), ("details", &copy.differences), ("reset", &copy.reset_all)] {
                    menu.append(Some(label), Some(&format!("keymap.{name}")));
                }
                true
            }
        ));
        self.navigation.clone()
    }

    pub(super) fn build_triggers(&self, w: &Rc<Workspace>, content: &adw::PreferencesPage) -> adw::NavigationView {
        *self.input_page.borrow_mut() = Some(content.clone());
        let root = adw::NavigationPage::with_tag(content, "", "input");
        self.input_navigation.add(&root);
        *self.input_root.borrow_mut() = Some(root);
        self.input_navigation.connect_popped(glib::clone!(
            #[weak]
            w,
            move |_, popped| {
                if *popped == w.preferences.shortcut_page.pen_page.page
                    && current(&w).is_some_and(|v| v.pen_button_editor.is_some())
                {
                    send(&w, PreferenceAction::ClosePenButton);
                }
            }
        ));
        self.input_navigation.clone()
    }

    pub(super) fn close_dialogs(&self) {
        self.recording.set(false);
        for sheet in [&self.editor.sheet, &self.modifier.sheet, &self.picker, &self.details] {
            if sheet.shown.replace(false) {
                sheet.dialog.close();
            }
        }
        if let Some(alert) = self.import.take() {
            alert.close();
        }
    }

    pub(super) fn refresh(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, view: &PreferencesView) {
        self.refresh_keymap(w, dialog, &view.keymap);
        self.refresh_triggers(w, &view.shortcut_page.triggers);
        self.refresh_picker(w, dialog, view.shortcut_page.picker.as_ref());
        self.refresh_editor(w, dialog, view);
        self.refresh_modifier(w, dialog, view);
        self.refresh_modifier_rows(w, &view.shortcut_page);
        let page = &view.shortcut_page;
        self.refresh_pages(w, view);
        let contexts: Vec<_> = page.contexts.iter().map(|c| c.category).collect();
        let labels: Vec<_> = page.contexts.iter().map(|c| c.label.as_str()).collect();
        refresh_options(&self.context, &labels);
        *self.contexts.borrow_mut() = contexts;
        if let Some(index) = self.contexts.borrow().iter().position(|c| *c == page.context) {
            self.context.set_selected(index as u32);
        }
        let shows: Vec<_> = page.shows.iter().map(|s| s.show).collect();
        let labels: Vec<_> = page.shows.iter().map(|s| s.label.as_str()).collect();
        refresh_options(&self.show, &labels);
        *self.shows.borrow_mut() = shows;
        if let Some(index) = self.shows.borrow().iter().position(|s| *s == page.show) {
            self.show.set_selected(index as u32);
        }
        let search = &w.preferences.shortcut_search;
        if page.key.as_deref().is_some_and(|key| search.text().as_str() == key) {
            search.select_region(0, -1);
        }
        let showing_categories = !page.filtering;
        self.categories.set_visible(showing_categories);
        if showing_categories {
            self.refresh_categories(w, &page.categories);
        }
        self.status.set_visible(page.empty.is_some());
        if let Some(empty) = &page.empty {
            self.status.set_title(&empty.title);
            self.status.set_description(Some(&empty.description));
        }
        self.refresh_results(w, view);
    }

    fn refresh_pages(&self, w: &Rc<Workspace>, view: &PreferencesView) {
        let localization = w.localization();
        let page = &view.shortcut_page;
        if let Some(root) = self.root.borrow().as_ref() { root.set_title(&SettingsPage::Shortcuts.localized_title(&localization)); }
        if let Some(root) = self.input_root.borrow().as_ref() { root.set_title(&SettingsPage::Input.localized_title(&localization)); }
        if let Some(category) = &page.category {
            self.category_page.set_title(&page.categories.iter().find(|c| &c.id == category).map_or_else(|| category.clone(), |c| c.label.clone()));
        }
        let remove_modifier = localization.text(MessageId::NATIVE_SHORTCUTS_REMOVE_MODIFIER);
        if let Some(editor) = &view.modifier_editor {
            let key = editor.key.clone();
            let (same, pick) = (key.clone(), key.clone());
            self.modifier_page.render(w, PerToolView {
                title: &editor.label,
                summary: modifier_hold_help(&localization, &editor.label),
                per_tool: editor.per_tool,
                actions: &editor.actions,
                modified: editor.modified,
                reset: PreferenceAction::ResetModifierKey { key: key.clone() },
                same: Rc::new(move |per_tool| PreferenceAction::ModifierKeyPerTool { key: same.clone(), per_tool }),
                pick: Rc::new(move |category| PreferenceAction::OpenModifierPicker { key: pick.clone(), category }),
                remove: Some((&remove_modifier, PreferenceAction::RemoveModifierKey { key })),
            });
        }
        if let Some(editor) = &view.pen_button_editor {
            let (same, pick) = (editor.trigger.clone(), editor.trigger.clone());
            self.pen_page.render(w, PerToolView {
                title: &editor.label,
                summary: localization.text(MessageId::SHORTCUT_PEN_ACTION_HELP).to_string(),
                per_tool: editor.per_tool,
                actions: &editor.actions,
                modified: editor.modified,
                reset: PreferenceAction::ResetTrigger { trigger: editor.trigger.clone() },
                same: Rc::new(move |per_tool| PreferenceAction::PenButtonPerTool { trigger: same.clone(), per_tool }),
                pick: Rc::new(move |category| PreferenceAction::OpenPenButtonPicker { trigger: pick.clone(), category }),
                remove: None,
            });
        }
        if let Some(root) = self.root.borrow().clone() {
            let mut desired = vec![root];
            if page.category.is_some() {
                if desired.len() == 1 && self.navigation.visible_page() != Some(self.category_page.clone()) {
                    self.category_results.page.scroll_to_top();
                }
                desired.push(self.category_page.clone());
            }
            if view.modifier_editor.is_some() {
                desired.push(self.modifier_page.page.clone());
            }
            sync_stack(&self.navigation, &desired);
        }
        if let Some(root) = self.input_root.borrow().clone() {
            let mut desired = vec![root];
            if view.pen_button_editor.is_some() {
                desired.push(self.pen_page.page.clone());
            }
            sync_stack(&self.input_navigation, &desired);
        }
        let title = match view.page {
            SettingsPage::Shortcuts => view.modifier_editor.as_ref().map(|e| e.label.clone()).or_else(|| page.category.as_ref().map(|category| page.categories.iter().find(|c| &c.id == category).map_or_else(|| category.clone(), |c| c.label.clone()))),
            SettingsPage::Input => view.pen_button_editor.as_ref().map(|e| e.label.clone()),
            _ => None,
        };
        self.back.set_visible(title.is_some());
        if let (Some(title), Some(outer)) = (title, &*self.outer.borrow()) {
            outer.set_title(&title);
        }
    }

    fn refresh_categories(&self, w: &Rc<Workspace>, categories: &[ShortcutCategoryView]) {
        if self.category_rows.borrow().iter().map(|r| &r.0).ne(categories.iter().map(|c| &c.id)) {
            for (_, row, _) in self.category_rows.borrow_mut().drain(..) {
                self.categories.remove(&row);
            }
            for category in categories {
                let row = text_row(&category.id, "");
                row.set_widget_name(&format!("shortcut-category-{}", category.id));
                row.set_activatable(true);
                let count = gtk::Label::new(None);
                count.add_css_class("dim-label");
                count.add_css_class("numeric");
                row.add_suffix(&count);
                row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
                let id = category.id.clone();
                row.connect_activated(glib::clone!(
                    #[weak]
                    w,
                    move |_| send(&w, PreferenceAction::ShortcutCategory { id: Some(id.clone()) })
                ));
                self.categories.append(&row);
                self.category_rows.borrow_mut().push((category.id.clone(), row, count));
            }
        }
        for ((_, row, count), category) in self.category_rows.borrow().iter().zip(categories) {
            row.set_title(&category.label);
            count.set_text(&category.count.to_string());
        }
    }

    fn shortcut_row(&self, w: &Rc<Workspace>, spec: &ShortcutRow) -> adw::ActionRow {
        let mut rows = self.rows.borrow_mut();
        let widget = rows.entry(spec.id.clone()).or_insert_with(|| {
            let row = text_row(&spec.label, "");
            row.set_activatable(true);
            row.set_widget_name(&format!("shortcut-{}", spec.id));
            let id = spec.id.clone();
            row.connect_activated(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, PreferenceAction::EditShortcut { id: id.clone() })
            ));
            let keys = gtk::Label::builder().ellipsize(gtk::pango::EllipsizeMode::End).max_width_chars(24).build();
            keys.add_css_class("dim-label");
            row.add_suffix(&keys);
            let reset = icon_button("edit-undo-symbolic", "", &format!("shortcut-reset-{}", spec.id));
            let id = spec.id.clone();
            reset.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, PreferenceAction::ResetShortcut { id: id.clone() })
            ));
            row.add_suffix(&reset);
            ShortcutWidget { row, keys, reset, signature: RefCell::default() }
        });
        widget.reset.set_tooltip_text(Some(&w.localization().text(MessageId::SETTINGS_RESET_TO_DEFAULT)));
        let signature = serde_json::to_string(&(&spec.label, &spec.shortcut, &spec.detail, &spec.scope_caption, spec.modified)).unwrap();
        if widget.signature.replace(signature.clone()) != signature {
            widget.row.set_title(&spec.label);
            widget.reset.set_visible(spec.modified);
            widget.row.set_subtitle(&subtitle(spec));
            let disabled = spec.shortcut.is_empty() && spec.modified;
            let disabled_label = w.localization().text(MessageId::SHORTCUT_DISABLED);
            widget.keys.set_text(if disabled { &disabled_label } else { &spec.shortcut });
        }
        widget.row.clone()
    }

    fn refresh_results(&self, w: &Rc<Workspace>, view: &PreferencesView) {
        let page = &view.shortcut_page;
        let mut layout: Vec<(String, Vec<&ShortcutRow>)> = Vec::new();
        for spec in view.shortcuts.iter().filter(|r| r.visible) {
            let title = if page.category.is_some() && !page.filtering { spec.subgroup.clone() } else { spec.group.clone() };
            match layout.last_mut() {
                Some((last, rows)) if *last == title => rows.push(spec),
                _ => layout.push((title, vec![spec])),
            }
        }
        let main = self.main_results.borrow();
        let Some(main) = main.as_ref() else {
            return;
        };
        if page.category.is_some() {
            main.show(self, w, &[]);
            self.category_results.show(self, w, &layout);
        } else {
            main.show(self, w, &layout);
        }
    }

    fn refresh_editor(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, view: &PreferencesView) {
        let localization = w.localization();
        let capture = view.capture.as_ref();
        self.recording.set(capture.is_some());
        let Some(editor) = view.shortcut_editor.as_ref() else {
            self.editor.close(dialog);
            return;
        };
        let capture = capture.filter(|c| c.id == editor.id);
        let error = view.error.as_ref().filter(|_| capture.is_none());
        self.editor.texts(&editor.label, &editor.description, error.map(String::as_str));
        let signature = w.gpu.borrow().as_ref().map(|g| serde_json::to_string(&(&editor.id, &g.session.state().settings.shortcuts, &g.session.state().settings.keymap, &g.session.state().settings.gestures, &g.session.state().settings.pen_buttons, editor.modified, editor.can_add, capture.is_some())).unwrap()).unwrap_or_default();
        self.editor.rebuild(signature, || {
            let keys = adw::PreferencesGroup::new();
            let default = shortcut_default_caption(&localization, &editor.defaults);
            let description: Vec<_> = std::iter::once(default).chain(editor.overlaps.iter().cloned()).collect();
            keys.set_description(Some(&glib::markup_escape_text(&description.join("\n"))));
            let reset = icon_button("edit-undo-symbolic", "", "shortcut-editor-reset");
            reset.set_visible(editor.modified);
            let id = editor.id.clone();
            reset.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, PreferenceAction::ResetShortcut { id: id.clone() })
            ));
            keys.set_header_suffix(Some(&reset));
            for (index, binding) in editor.bindings.iter().enumerate() {
                let row = text_row(binding, "");
                row.set_widget_name(&format!("shortcut-binding-{index}"));
                let remove = icon_button("user-trash-symbolic", "", &format!("remove-shortcut-{index}"));
                let id = editor.id.clone();
                remove.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    move |_| send(&w, PreferenceAction::RemoveShortcut { id: id.clone(), index })
                ));
                row.add_suffix(&remove);
                keys.add(&row);
            }
            match capture {
                Some(capture) => {
                    let row = recording_row(w, capture);
                    keys.add(&row);
                    *self.editor.recording.borrow_mut() = Some(row);
                }
                None if editor.can_add => {
                    let id = editor.id.clone();
                    keys.add(&button_row("", "list-add-symbolic", "add-shortcut", glib::clone!(
                        #[weak]
                        w,
                        move || send(&w, PreferenceAction::BeginShortcut { id: id.clone() })
                    )));
                }
                None => {}
            }
            let mut groups = vec![keys];
            if !editor.gestures.is_empty() {
                let gestures = adw::PreferencesGroup::new();
                gestures.set_widget_name("shortcut-editor-gestures");
                for (index, gesture) in editor.gestures.iter().enumerate() {
                    let row = text_row(gesture, "");
                    row.set_widget_name(&format!("shortcut-gesture-{index}"));
                    gestures.add(&row);
                }
                groups.push(gestures);
            }
            groups
        });
        if let Some(group) = self.editor.groups.borrow().first() {
            let description: Vec<_> = std::iter::once(shortcut_default_caption(&localization, &editor.defaults)).chain(editor.overlaps.iter().cloned()).collect();
            group.set_description(Some(&glib::markup_escape_text(&description.join("\n"))));
        }
        relabel_controls(self.editor.body.upcast_ref(), &localization);
        crate::text_language::visit(self.editor.body.upcast_ref(), &mut |widget| {
            let name = widget.widget_name();
            if let Some(index) = name.strip_prefix("shortcut-binding-").and_then(|index| index.parse::<usize>().ok()) {
                if let (Some(row), Some(label)) = (widget.downcast_ref::<adw::ActionRow>(), editor.bindings.get(index)) { row.set_title(label); }
            } else if let Some(index) = name.strip_prefix("shortcut-gesture-").and_then(|index| index.parse::<usize>().ok()) {
                if let (Some(row), Some(label)) = (widget.downcast_ref::<adw::ActionRow>(), editor.gestures.get(index)) { row.set_title(label); }
            } else if name.as_str() == "shortcut-editor-gestures" {
                if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
                    group.set_title(&localization.text(MessageId::NATIVE_SHORTCUTS_PEN_TOUCH));
                    group.set_description(Some(&glib::markup_escape_text(&localization.text(MessageId::NATIVE_SHORTCUTS_PEN_PAGE_HELP))));
                }
            }
        });
        if let (Some(row), Some(capture)) = (self.editor.recording.borrow().as_ref(), capture) { refresh_recording(row, capture, &localization); }
        self.editor.present(dialog);
    }

    fn refresh_modifier(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, view: &PreferencesView) {
        let Some(capture) = view.capture.as_ref().filter(|c| c.id == MODIFIER_CAPTURE) else {
            self.modifier.close(dialog);
            return;
        };
        let localization = w.localization();
        self.modifier.texts(&capture.label, &localization.text(MessageId::NATIVE_SHORTCUTS_PRESS_HOLD_KEY), None);
        let signature = capture.id.clone();
        self.modifier.rebuild(signature, || {
            let group = adw::PreferencesGroup::new();
            let row = recording_row(w, capture);
            group.add(&row);
            *self.modifier.recording.borrow_mut() = Some(row);
            vec![group]
        });
        if let Some(row) = self.modifier.recording.borrow().as_ref() { refresh_recording(row, capture, &localization); }
        self.modifier.present(dialog);
    }

    fn refresh_modifier_rows(&self, w: &Rc<Workspace>, page: &ShortcutPageView) {
        let visible: Vec<_> = page.modifiers.iter().filter(|m| m.visible).collect();
        let on_category = page.category.as_deref() == Some(MODIFIER_SECTION);
        let signature = serde_json::to_string(&(visible.iter().map(|row| &row.key).collect::<Vec<_>>(), on_category)).unwrap();
        self.modifiers_main.set_visible(page.filtering && !visible.is_empty());
        self.modifiers_category.set_visible(on_category);
        if self.modifier_signature.replace(signature.clone()) == signature {
            self.relabel_modifier_rows(w, &visible);
            return;
        }
        for row in self.modifier_rows.borrow_mut().drain(..) {
            if let Some(group) = row.ancestor(adw::PreferencesGroup::static_type()).and_downcast::<adw::PreferencesGroup>() {
                group.remove(&row);
            }
        }
        let group = if on_category { &self.modifiers_category } else { &self.modifiers_main };
        for modifier in &visible {
            let row = text_row(&modifier.label, &modifier.detail);
            row.set_widget_name(&format!("modifier-{}", modifier.key.label(Platform::Gtk)));
            row.set_activatable(true);
            let action = gtk::Label::new(Some(&modifier.action));
            action.set_widget_name("modifier-action");
            action.add_css_class("dim-label");
            row.add_suffix(&action);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let key = modifier.key.clone();
            row.connect_activated(glib::clone!(
                #[weak]
                w,
                move |_| send(&w, PreferenceAction::EditModifierKey { key: key.clone() })
            ));
            group.add(&row);
            self.modifier_rows.borrow_mut().push(row.upcast());
        }
        if on_category {
            let add = button_row("", "list-add-symbolic", "add-modifier-key", glib::clone!(
                #[weak]
                w,
                move || send(&w, PreferenceAction::AddModifierKey)
            ));
            group.add(&add);
            self.modifier_rows.borrow_mut().push(add.upcast());
        }
        self.relabel_modifier_rows(w, &visible);
    }

    fn relabel_modifier_rows(&self, w: &Workspace, rows: &[&ModifierKeyRow]) {
        for (widget, spec) in self.modifier_rows.borrow().iter().zip(rows) {
            widget.set_title(&spec.label);
            if let Some(row) = widget.downcast_ref::<adw::ActionRow>() { row.set_subtitle(&spec.detail); }
            crate::text_language::visit(widget.upcast_ref(), &mut |widget| {
                if widget.widget_name().as_str() == "modifier-action" {
                    if let Some(label) = widget.downcast_ref::<gtk::Label>() { label.set_text(&spec.action); }
                }
            });
        }
        relabel_controls(self.modifiers_category.upcast_ref(), &w.localization());
    }

    fn refresh_triggers(&self, w: &Rc<Workspace>, triggers: &[TriggerRow]) {
        let Some(page) = self.input_page.borrow().clone() else {
            return;
        };
        if self.trigger_rows.borrow().iter().map(|t| &t.0).ne(triggers.iter().map(|t| &t.id)) {
            for (_, group) in self.trigger_groups.borrow_mut().drain(..) {
                page.remove(&group);
            }
            self.trigger_rows.borrow_mut().clear();
            for trigger in triggers {
                let known = self.trigger_groups.borrow().iter().any(|(section, _)| *section == trigger.section);
                if !known {
                    let group = adw::PreferencesGroup::new();
                    group.set_title(&trigger.section);
                    group.set_widget_name(&format!("triggers-{}", trigger.section.to_lowercase().replace(' ', "-")));
                    page.add(&group);
                    self.trigger_groups.borrow_mut().push((trigger.section.clone(), group));
                }
                let row = text_row(&trigger.label, "");
                row.set_widget_name(&format!("trigger-{}", trigger.id));
                row.set_activatable(true);
                let action = gtk::Label::new(None);
                action.add_css_class("dim-label");
                row.add_suffix(&action);
                row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
                let id = trigger.id.clone();
                let pen = GESTURE_TRIGGERS.iter().any(|t| t.id == trigger.id && t.held);
                row.connect_activated(glib::clone!(
                    #[weak]
                    w,
                    move |_| send(&w, if pen {
                        PreferenceAction::EditPenButton { trigger: id.clone() }
                    } else {
                        PreferenceAction::OpenActionPicker { trigger: id.clone() }
                    })
                ));
                if let Some((_, group)) = self.trigger_groups.borrow().iter().find(|(s, _)| *s == trigger.section) {
                    group.add(&row);
                }
                self.trigger_rows.borrow_mut().push((trigger.id.clone(), row, action));
            }
        }
        for ((_, row, action), trigger) in self.trigger_rows.borrow().iter().zip(triggers) {
            row.set_title(&trigger.label);
            action.set_text(&trigger.action);
            row.set_subtitle(&trigger.detail);
        }
        let mut sections = Vec::new();
        for trigger in triggers {
            if !sections.contains(&trigger.section.as_str()) { sections.push(trigger.section.as_str()); }
        }
        for ((_, group), section) in self.trigger_groups.borrow().iter().zip(sections) { group.set_title(section); }
    }

    fn refresh_picker(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, picker: Option<&ActionPickerView>) {
        let Some(picker) = picker else {
            self.picker.show(false, dialog);
            return;
        };
        let localization = w.localization();
        self.picker.dialog.set_title(&picker.title);
        self.picker.title.set_title(&picker.title);
        self.picker_description.set_text(&picker.description);
        if self.picker_search.text().as_str() != picker.query {
            self.picker_search.set_text(&picker.query);
        }
        self.picker_reset.set_visible(picker.modified);
        let query = picker.query.trim().to_lowercase();
        let nothing = localization.text(MessageId::SHORTCUT_NOTHING);
        let show_nothing = "nothing".contains(&query);
        let signature = serde_json::to_string(&(show_nothing, picker.sections.iter().map(|section| section.actions.iter().map(|action| &action.id).collect::<Vec<_>>()).collect::<Vec<_>>())).unwrap();
        if self.picker_signature.replace(signature.clone()) != signature {
            for group in self.picker_groups.borrow_mut().drain(..) {
                self.picker_page.remove(&group);
            }
            let choose = |id: String, title: &str, detail: &str, selected: bool| {
                let row = text_row(title, detail);
                row.set_widget_name(&format!("action-{id}"));
                row.set_activatable(true);
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.set_widget_name(&format!("action-selected-{id}"));
                check.set_visible(selected);
                row.add_suffix(&check);
                row.connect_activated(glib::clone!(
                    #[weak]
                    w,
                    move |_| send(&w, PreferenceAction::ChooseAction { id: id.clone() })
                ));
                row
            };
            if show_nothing {
                let group = adw::PreferencesGroup::new();
                group.add(&choose(String::new(), &nothing, "", picker.nothing));
                self.picker_page.add(&group);
                self.picker_groups.borrow_mut().push(group);
            }
            for section in &picker.sections {
                let group = adw::PreferencesGroup::new();
                group.set_title(&section.title);
                for action in &section.actions {
                    group.add(&choose(action.id.clone(), &action.label, &action.detail, action.selected));
                }
                self.picker_page.add(&group);
                self.picker_groups.borrow_mut().push(group);
            }
            if self.picker_groups.borrow().is_empty() {
                let group = adw::PreferencesGroup::new();
                let status = adw::StatusPage::builder()
                    .icon_name("edit-find-symbolic")
                    .title(localization.text(MessageId::SHORTCUT_SEARCH_EMPTY_TITLE).as_ref())
                    .description(localization.text(MessageId::SHORTCUT_SEARCH_EMPTY_HELP).as_ref())
                    .build();
                status.set_widget_name("action-picker-empty");
                status.add_css_class("compact");
                group.add(&status);
                self.picker_page.add(&group);
                self.picker_groups.borrow_mut().push(group);
            }
        }
        let groups = self.picker_groups.borrow();
        for (group, section) in groups.iter().skip(usize::from(show_nothing)).zip(&picker.sections) { group.set_title(&section.title); }
        crate::text_language::visit(self.picker_page.upcast_ref(), &mut |widget| {
            let name = widget.widget_name();
            if let Some(id) = name.strip_prefix("action-selected-") {
                if let Some(check) = widget.downcast_ref::<gtk::Image>() { check.set_visible(if id.is_empty() { picker.nothing } else { picker.sections.iter().flat_map(|section| &section.actions).any(|action| action.id == id && action.selected) }); }
            } else if let Some(id) = name.strip_prefix("action-") {
                if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                    if id.is_empty() { row.set_title(&nothing); }
                    else if let Some(action) = picker.sections.iter().flat_map(|section| &section.actions).find(|action| action.id == id) {
                        row.set_title(&action.label);
                        row.set_subtitle(&action.detail);
                    }
                }
            }
            if name.as_str() == "action-picker-empty" {
                if let Some(status) = widget.downcast_ref::<adw::StatusPage>() {
                    status.set_title(&localization.text(MessageId::SHORTCUT_SEARCH_EMPTY_TITLE));
                    status.set_description(Some(&localization.text(MessageId::SHORTCUT_SEARCH_EMPTY_HELP)));
                }
            }
        });
        if self.picker.show(true, dialog) {
            self.picker_search.grab_focus();
        }
    }

    fn refresh_keymap(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, keymap: &KeymapView) {
        let localization = w.localization();
        let ids: Vec<_> = keymap.presets.iter().map(|p| p.id.clone()).collect();
        if *self.keymap_ids.borrow() != ids {
            let titles: Vec<_> = keymap.presets.iter().map(|p| p.title.as_str()).collect();
            self.keymap_combo.set_model(Some(&gtk::StringList::new(&titles)));
            *self.keymap_ids.borrow_mut() = ids;
        }
        if let Some(index) = self.keymap_ids.borrow().iter().position(|id| *id == keymap.selected) {
            self.keymap_combo.set_selected(index as u32);
        }
        let updated = localization.text(MessageId::NATIVE_SHORTCUTS_UPDATED);
        self.keymap_combo.set_subtitle(if keymap.outdated { &updated } else { "" });
        let signature = serde_json::to_string(&(&keymap.selected, &keymap.differences)).unwrap();
        if self.details_signature.replace(signature.clone()) != signature {
            while let Some(child) = self.details_body.first_child() {
                self.details_body.remove(&child);
            }
            self.details.title.set_title(&keymap.title);
            let source = gtk::Label::builder().label(&keymap.source).wrap(true).xalign(0.).build();
            source.add_css_class("dim-label");
            self.details_body.append(&source);
            let links = gtk::FlowBox::builder().selection_mode(gtk::SelectionMode::None).max_children_per_line(3).build();
            for link in &keymap.links {
                let name = link.trim_end_matches('/').rsplit('/').next().unwrap_or(link);
                let name = name.split('.').next().unwrap_or(name).replace(['_', '-'], " ");
                links.append(&gtk::LinkButton::with_label(link, &name));
            }
            links.set_visible(!keymap.links.is_empty());
            self.details_body.append(&links);
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::None);
            list.add_css_class("boxed-list");
            for difference in &keymap.differences {
                list.append(&text_row(&difference.trigger, &difference.note));
            }
            if keymap.differences.is_empty() {
                let row = text_row(&localization.text(MessageId::NATIVE_SHORTCUTS_NO_DIFFERENCES), &localization.text(MessageId::NATIVE_SHORTCUTS_DEFAULTS_HELP));
                row.set_widget_name("keymap-defaults");
                list.append(&row);
            }
            let scroller = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&list)
                .build();
            self.details_body.append(&scroller);
        }
        self.details.dialog.set_title(&keymap.title);
        self.details.title.set_title(&keymap.title);
        crate::text_language::visit(self.details_body.upcast_ref(), &mut |widget| {
            if widget.widget_name().as_str() == "keymap-defaults" {
                if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                    row.set_title(&localization.text(MessageId::NATIVE_SHORTCUTS_NO_DIFFERENCES));
                    row.set_subtitle(&localization.text(MessageId::NATIVE_SHORTCUTS_DEFAULTS_HELP));
                }
            }
        });
        self.details.show(keymap.details, dialog);
        let shown = self.import.borrow().clone();
        match (&keymap.import, shown) {
            (Some(preview), None) => {
                let alert = adw::AlertDialog::builder()
                    .heading(&preview.heading)
                    .body(&preview.body)
                    .build();
                alert.add_responses(&[("cancel", &preview.cancel_label), ("import", &preview.import_label)]);
                alert.set_close_response("cancel");
                alert.set_response_appearance("import", adw::ResponseAppearance::Suggested);
                alert.connect_response(None, glib::clone!(
                    #[weak]
                    w,
                    move |_, response| send(&w, if response == "import" {
                        PreferenceAction::ConfirmKeymapImport
                    } else {
                        PreferenceAction::CancelKeymapImport
                    })
                ));
                alert.present(Some(dialog));
                *self.import.borrow_mut() = Some(alert);
            }
            (Some(preview), Some(alert)) => {
                alert.set_heading(Some(&preview.heading));
                alert.set_body(&preview.body);
                alert.set_response_label("cancel", &preview.cancel_label);
                alert.set_response_label("import", &preview.import_label);
            }
            (None, Some(alert)) => {
                self.import.borrow_mut().take();
                alert.close();
            }
            _ => {}
        }
    }
}
