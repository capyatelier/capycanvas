//! Compact native palettes. Shared Rust owns names, imports, definitions and
//! used-color history; this view owns focus, overlays and file dialogs.
use crate::{
    display_color::{ColorPatch, ViewColor},
    palette_grid::PaletteGrid,
    workspace::Workspace,
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_core::color::RgbColor;
use layer_ui::{
    ColorAction, ColorLibrary, ColorLibraryAction as Action, ColorSlot, PaletteCommand,
    PaletteFormat, PaletteMenuItem, PaletteMenuTarget, UiAction, UiState,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
#[path = "palette_drag.rs"]
mod drag;


pub struct PalettePanel {
    pub root: gtk::Box,
    workspace: RefCell<Weak<Workspace>>,
    normal: gtk::Box,
    history: PaletteGrid,
    expanded: PaletteGrid,
    swatches: PaletteGrid,
    add_color: gtk::Button,
    chooser: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    empty: gtk::Label,
    selector: gtk::Button,
    selector_label: gtk::Label,
    name: gtk::Button,
    name_label: gtk::Label,
    editor: gtk::Entry,
    name_stack: gtk::Stack,
    detail: gtk::Label,
    validation: gtk::Label,
    library: RefCell<Option<ColorLibrary>>,
    view: Cell<(ViewColor, f32)>,
    current: Cell<RgbColor>,
    selected: Cell<Option<u64>>,
    editing: Cell<bool>,
    tiles: RefCell<Vec<(u64, gtk::Button)>>,
    library_menu: gtk::PopoverMenu,
    context_menu: gtk::PopoverMenu,
    drag: RefCell<Option<drag::Contact>>,
    drag_host: RefCell<Option<drag::Host>>,
}
impl Drop for PalettePanel {
    fn drop(&mut self) {
        self.cancel_reorder();
        if self.context_menu.parent().is_some() {
            self.context_menu.unparent();
        }
    }
}
fn button(icon: &str, label: &str, id: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_widget_name(id);
    button.set_tooltip_text(Some(label));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button.add_css_class("flat");
    button
}
impl PalettePanel {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("palette-panel");
        root.set_width_request(280);
        let body = gtk::Overlay::new();
        body.set_vexpand(true);
        let normal = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let history = PaletteGrid::new(1);
        history.set_tooltip_text(Some("Recent colors — added only when used in artwork"));
        normal.append(&history);
        normal.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let swatches = PaletteGrid::new(0);
        let add_color = button(
            "list-add-symbolic",
            "Add current color to this palette",
            "palette-add-color",
        );
        add_color.add_css_class("palette-add");
        let scroll = crate::input::pen_scroller(
            gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vscrollbar_policy(gtk::PolicyType::Automatic)
                .min_content_height(84)
                .max_content_height(172)
                .propagate_natural_height(true)
                .child(&swatches)
                .build(),
        );
        normal.append(&scroll);
        body.set_child(Some(&normal));
        let expanded = PaletteGrid::new(4);
        expanded.set_valign(gtk::Align::Fill);
        expanded.add_css_class("palette-cover");
        expanded.set_visible(false);
        body.add_overlay(&expanded);
        let chooser = gtk::Box::new(gtk::Orientation::Vertical, 6);
        chooser.add_css_class("palette-cover");
        chooser.set_visible(false);
        chooser.set_widget_name("palette-browser");
        let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Find a palette"));
        search.set_hexpand(true);
        search.add_css_class("palette-entry");
        search.set_valign(gtk::Align::Center);
        search.set_widget_name("palette-search");
        search_row.append(&search);
        let add = gtk::MenuButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New or import palette")
            .build();
        add.add_css_class("flat");
        add.add_css_class("palette-icon");
        add.set_valign(gtk::Align::Center);
        add.set_widget_name("palette-library-add");
        let popover = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        popover.set_has_arrow(false);
        add.set_popover(Some(&popover));
        search_row.append(&add);
        chooser.append(&search_row);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("palette-list");
        let empty = gtk::Label::new(Some("No matching palettes"));
        empty.add_css_class("dim-label");
        empty.set_visible(false);
        empty.set_can_target(false);
        let lists = crate::input::pen_scroller(
            gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vscrollbar_policy(gtk::PolicyType::Automatic)
                .vexpand(true)
                .child(&list)
                .build(),
        );
        let results = gtk::Overlay::new();
        results.set_vexpand(true);
        results.set_child(Some(&lists));
        results.add_overlay(&empty);
        chooser.append(&results);
        body.add_overlay(&chooser);
        root.append(&body);
        let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
        divider.set_widget_name("palette-footer-divider");
        root.append(&divider);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        footer.add_css_class("palette-footer");
        let selector = gtk::Button::new();
        selector.add_css_class("flat");
        selector.add_css_class("palette-selector");
        selector.set_valign(gtk::Align::Center);
        selector.set_widget_name("palette-chooser");
        selector.set_tooltip_text(Some("Choose a palette"));
        let selector_content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let selector_label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(14)
            .xalign(0.)
            .build();
        selector_content.append(&selector_label);
        selector_content.append(&gtk::Image::from_icon_name("pan-up-symbolic"));
        selector.set_child(Some(&selector_content));
        footer.append(&selector);
        let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
        info.set_hexpand(true);
        let name_stack = gtk::Stack::new();
        name_stack.set_hhomogeneous(false);
        name_stack.set_vhomogeneous(false);
        let name = gtk::Button::new();
        name.add_css_class("flat");
        name.add_css_class("palette-name");
        name.set_widget_name("palette-color-name");
        let name_label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(16)
            .xalign(1.)
            .build();
        name.set_child(Some(&name_label));
        name.set_tooltip_text(Some("Name this color · Enter to save, Escape to cancel"));
        let editor = gtk::Entry::builder().max_length(64).width_chars(8).build();
        editor.set_widget_name("palette-name-editor");
        editor.add_css_class("palette-entry");
        name_stack.add_named(&name, Some("label"));
        name_stack.add_named(&editor, Some("edit"));
        info.append(&name_stack);
        let detail = gtk::Label::builder().xalign(1.).build();
        detail.set_widget_name("palette-color-detail");
        detail.add_css_class("caption");
        detail.add_css_class("palette-detail");
        detail.add_css_class("dim-label");
        info.append(&detail);
        footer.append(&info);
        root.append(&footer);
        let validation = gtk::Label::builder()
            .wrap(true)
            .xalign(0.)
            .visible(false)
            .build();
        validation.add_css_class("error");
        root.append(&validation);
        let panel = Rc::new(Self {
            drag: Default::default(),
            drag_host: Default::default(),
            root,
            workspace: Default::default(),
            normal,
            history,
            expanded,
            swatches,
            add_color,
            chooser,
            search,
            list,
            empty,
            selector,
            selector_label,
            name,
            name_label,
            editor,
            name_stack,
            detail,
            validation,
            library: Default::default(),
            view: Cell::new((ViewColor::default(), 1.)),
            current: Cell::new(RgbColor::BLACK),
            selected: Cell::new(None),
            editing: Cell::new(false),
            tiles: Default::default(),
            library_menu: popover.clone(),
            context_menu: gtk::PopoverMenu::from_model(None::<&gio::Menu>),
        });
        panel.selector.connect_clicked(glib::clone!(
            #[weak]
            panel,
            move |_| panel.browse(!panel.chooser.is_visible())
        ));
        panel.search.connect_changed(glib::clone!(
            #[weak]
            panel,
            move |_| panel.filter()
        ));
        panel.list.connect_row_activated(glib::clone!(
            #[weak]
            panel,
            move |_, row| {
                if panel.editing.get() {
                    panel.commit_name();
                    if panel.editing.get() {
                        return;
                    }
                }
                let id = panel
                    .library
                    .borrow()
                    .as_ref()
                    .and_then(|l| l.palettes.get(row.index() as usize))
                    .map(|p| p.id);
                if let Some(id) = id
                    && panel.apply(Action::SelectPalette { id })
                {
                    panel.browse(false);
                }
            }
        ));
        panel.name.connect_clicked(glib::clone!(
            #[weak]
            panel,
            move |_| panel.edit_name()
        ));
        panel.add_color.connect_clicked(glib::clone!(
            #[weak]
            panel,
            move |_| {
                if panel.editing.get() {
                    panel.commit_name();
                    if panel.editing.get() {
                        return;
                    }
                }
                let palette = panel.library.borrow().as_ref().unwrap().active_palette().id;
                panel.apply(Action::Store {
                    palette,
                    name: String::new(),
                    color: panel.current.get(),
                });
            }
        ));
        panel.editor.connect_activate(glib::clone!(
            #[weak]
            panel,
            move |_| panel.commit_name()
        ));
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak]
            panel,
            move |_| panel.commit_name()
        ));
        panel.editor.add_controller(focus);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            panel,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key != gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                if panel.workspace.borrow().upgrade().is_some_and(|w| {
                    gtk::prelude::GtkWindowExt::focus(&w.window)
                        .is_some_and(|focus| focus.native().is_some_and(|n| n.is::<gtk::Popover>()))
                }) {
                    return glib::Propagation::Proceed;
                }
                if panel.editing.replace(false) {
                    panel.name_stack.set_visible_child_name("label");
                    panel.error(None);
                    panel.name.grab_focus();
                } else if panel.chooser.is_visible() {
                    panel.browse(false);
                } else if panel.expanded.is_visible() {
                    panel.expand_history(false);
                } else {
                    if let Some(w) = panel.workspace.borrow().upgrade() {
                        w.interact(crate::input::key_input(
                            key,
                            true,
                            gdk::ModifierType::empty(),
                            false,
                            None,
                        ));
                    }
                }
                glib::Propagation::Stop
            }
        ));
        panel.root.add_controller(keys);
        panel.context_menu.set_has_arrow(false);
        panel.context_menu.set_widget_name("palette-context-menu");
        panel
    }
    pub fn bind(self: &Rc<Self>, workspace: &Rc<Workspace>) {
        *self.workspace.borrow_mut() = Rc::downgrade(workspace);
        self.bind_reorder(workspace);
        workspace.watch_popover(self.library_menu.upcast_ref());
        self.context_menu.add_css_class("palette-key-scope");
        self.root.connect_unmap(glib::clone!(
            #[weak(rename_to=menu)]
            self.context_menu,
            move |_| menu.popdown()
        ));
        self.root.connect_map(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            #[weak]
            workspace,
            move |_| {
                let state = workspace
                    .gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.state().clone());
                if let Some(state) = state {
                    panel.refresh(&state, workspace.view_color(), workspace.picker_headroom());
                }
            }
        ));
    }
    fn notice(&self, notice: Option<&str>) {
        self.error(notice);
        self.validation.remove_css_class("error");
        self.editor.remove_css_class("error");
    }
    fn error(&self, error: Option<&str>) {
        self.validation.add_css_class("error");
        self.validation.set_text(error.unwrap_or(""));
        self.validation.set_visible(error.is_some());
        if error.is_some() {
            self.editor.add_css_class("error");
        } else {
            self.editor.remove_css_class("error");
        }
    }
    fn apply(self: &Rc<Self>, action: Action) -> bool {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return false;
        };
        let stored = matches!(action, Action::Store { .. });
        let result = w.gpu.borrow_mut().as_mut().map(|g| {
            g.session.dispatch(UiAction::Color {
                action: ColorAction::Library { action },
            })
        });
        match result {
            Some(Ok(change)) => {
                if stored {
                    self.selected.set(w.gpu.borrow().as_ref().and_then(|g| {
                        g.session
                            .state()
                            .colors
                            .library
                            .active_palette()
                            .swatches
                            .last()
                            .map(|s| s.id)
                    }));
                }
                self.error(None);
                w.changed(Ok(change));
                true
            }
            Some(Err(error)) => {
                self.error(Some(&error));
                false
            }
            None => false,
        }
    }
    fn use_color(&self, color: RgbColor) {
        if let Some(w) = self.workspace.borrow().upgrade() {
            w.dispatch(UiAction::Color {
                action: ColorAction::Definition { color },
            });
        }
    }
    fn browse(self: &Rc<Self>, show: bool) {
        if show && self.editing.get() {
            self.commit_name();
            if self.editing.get() {
                return;
            }
        }
        self.expanded.set_visible(false);
        self.chooser.set_visible(show);
        self.normal.set_sensitive(!show);
        self.normal.set_opacity(if show { 0. } else { 1. });
        if show {
            self.search.grab_focus();
        } else {
            self.selector.grab_focus();
        }
    }
    fn expand_history(&self, expanded: bool) {
        self.chooser.set_visible(false);
        self.expanded.set_visible(expanded);
        self.normal.set_sensitive(!expanded);
        self.normal.set_opacity(if expanded { 0. } else { 1. });
        let grid = if expanded {
            &self.expanded
        } else {
            &self.history
        };
        if let Some(toggle) = grid.last_child() {
            toggle.grab_focus();
        }
    }
    fn filter(&self) {
        let query = self.search.text().trim().to_lowercase();
        self.empty
            .set_visible(self.library.borrow().as_ref().is_none_or(|l| {
                !l.palettes
                    .iter()
                    .any(|p| p.name.to_lowercase().contains(&query))
            }));
        self.list.set_filter_func(move |row| {
            row.tooltip_text()
                .is_some_and(|n| n.to_lowercase().contains(&query))
        });
    }
    fn edit_name(&self) {
        self.editing.set(true);
        let value = self
            .library
            .borrow()
            .as_ref()
            .and_then(|l| self.selected.get().and_then(|id| l.swatch(id)))
            .map(|s| s.name.clone())
            .or_else(|| {
                self.library
                    .borrow()
                    .as_ref()
                    .and_then(|l| l.current_name(self.current.get()).map(str::to_owned))
            })
            .unwrap_or_default();
        self.editor.set_text(&value);
        self.name_stack.set_visible_child_name("edit");
        self.editor.grab_focus();
        self.editor.select_region(0, -1);
    }
    fn commit_name(self: &Rc<Self>) {
        if !self.editing.replace(false) {
            return;
        }
        let action = if let Some(id) = self.selected.get() {
            Action::Rename {
                id,
                name: self.editor.text().into(),
            }
        } else {
            Action::NameCurrent {
                name: self.editor.text().into(),
                color: self.current.get(),
            }
        };
        if self.apply(action) {
            self.name_stack.set_visible_child_name("label");
        } else {
            self.editing.set(true);
        }
    }
    fn tile(self: &Rc<Self>, color: RgbColor, name: &str, id: Option<u64>) -> gtk::Button {
        let tile = gtk::Button::new();
        tile.add_css_class("palette-tile");
        tile.add_css_class("customizable-target");
        tile.set_widget_name(&id.map_or_else(
            || "palette-recent-color".into(),
            |id| format!("palette-swatch-{id}"),
        ));
        let patch = ColorPatch::new(false);
        let (view, headroom) = self.view.get();
        patch.set_display_color(color, view, headroom);
        patch.set_overflow(gtk::Overflow::Hidden);
        tile.set_child(Some(&patch));
        let detail = ColorLibrary::tile_detail(name, color);
        tile.set_tooltip_text(Some(&detail));
        tile.update_property(&[gtk::accessible::Property::Label(&detail)]);
        tile.connect_clicked(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            move |_| {
                panel.selected.set(id);
                if let Some(id) = id {
                    panel.apply(Action::Use { id });
                } else {
                    panel.use_color(color);
                }
            }
        ));
        if let Some(id) = id {
            tile.add_css_class("drag-immediate");
            tile.set_cursor_from_name(Some("grab"));
            self.install_menu(tile.upcast_ref(), PaletteMenuTarget::Color { id });
        }
        tile
    }
    fn populate_menu(self: &Rc<Self>, popup: &gtk::PopoverMenu, sections: Vec<Vec<PaletteMenuItem>>) {
        let actions = gio::SimpleActionGroup::new();
        let model = self.menu_model(popup, &actions, &sections, "item");
        popup.insert_action_group("palette", Some(&actions));
        popup.set_menu_model(Some(&model));
    }
    fn menu_model(
        self: &Rc<Self>,
        popup: &gtk::PopoverMenu,
        actions: &gio::SimpleActionGroup,
        sections: &[Vec<PaletteMenuItem>],
        prefix: &str,
    ) -> gio::Menu {
        let model = gio::Menu::new();
        for (section, entries) in sections.iter().enumerate() {
            let group = gio::Menu::new();
            for (index, item) in entries.iter().enumerate() {
                let id = format!("{prefix}-{section}-{index}");
                let Some(command) = item.command.clone() else {
                    group.append_submenu(
                        Some(item.label),
                        &self.menu_model(popup, actions, &item.sections, &id),
                    );
                    continue;
                };
                let action = gio::SimpleAction::new(&id, None);
                action.set_enabled(item.enabled);
                action.connect_activate(glib::clone!(
                    #[weak(rename_to = panel)]
                    self,
                    #[weak]
                    popup,
                    move |_, _| {
                        popup.popdown();
                        panel.menu_command(command.clone());
                    }
                ));
                actions.add_action(&action);
                group.append(Some(item.label), Some(&format!("palette.{id}")));
            }
            model.append_section(None, &group);
        }
        model
    }
    fn menu_command(self: &Rc<Self>, command: PaletteCommand) {
        let palette_name = |id| {
            self.library.borrow().as_ref().and_then(|l| {
                l.palettes
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
            })
        };
        match command {
            PaletteCommand::NewPalette => self.ask_name("New Palette", "", None),
            PaletteCommand::ImportPalette => self.import(),
            PaletteCommand::RenamePalette { id } => {
                if let Some(name) = palette_name(id) {
                    self.ask_name("Rename Palette", &name, Some(id))
                }
            }
            PaletteCommand::ExportPalette { id, format } => self.export(id, format),
            PaletteCommand::RemovePalette { id } => {
                if let Some(name) = palette_name(id) {
                    self.remove_palette(id, &name)
                }
            }
            PaletteCommand::RenameColor { id } => {
                let color = self
                    .library
                    .borrow()
                    .as_ref()
                    .and_then(|l| l.swatch(id))
                    .map(|s| s.color);
                if let Some(color) = color {
                    self.selected.set(Some(id));
                    self.use_color(color);
                    self.edit_name();
                }
            }
            PaletteCommand::Library { action } => {
                self.apply(action);
            }
        }
    }
    fn show_menu(
        self: &Rc<Self>,
        widget: &gtk::Widget,
        target: PaletteMenuTarget,
        point: [f32; 2],
        held: bool,
    ) {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return;
        };
        let menu = self.library.borrow().as_ref().map(|l| l.menu(target));
        let Some(Ok(sections)) = menu else {
            return;
        };
        self.populate_menu(&self.context_menu, sections);
        self.context_menu.set_autohide(!held);
        w.popup_at(self.context_menu.upcast_ref(), widget, point);
    }
    fn install_menu(self: &Rc<Self>, widget: &gtk::Widget, target: PaletteMenuTarget) {
        widget.add_css_class("customizable-target");
        let popup = glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[weak]
            widget,
            move |x: f64, y: f64, held: bool| panel.show_menu(
                &widget,
                target,
                [x as f32, y as f32],
                held
            )
        );
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed({
            let popup = popup.clone();
            move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                popup(x, y, false);
            }
        });
        widget.add_controller(click);
        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed({
            let popup = popup.clone();
            let panel = Rc::downgrade(self);
            move |gesture, x, y| {
                let Some(panel) = panel.upgrade() else {
                    return;
                };
                let direct = crate::input::touch_or_pen(gesture);
                match target {
                    PaletteMenuTarget::Color { id } if panel.hold_swatch(id) => {}
                    PaletteMenuTarget::Palette { .. } if direct => {}
                    _ => return,
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                if direct {
                    popup(x, y, true);
                }
            }
        });
        widget.add_controller(hold.clone());
        if let PaletteMenuTarget::Color { id } = target {
            let drag = self.reorder_gesture(widget.downcast_ref::<gtk::Button>().unwrap(), id);
            widget.add_controller(drag.clone());
            hold.group_with(&drag);
        }
        if matches!(target, PaletteMenuTarget::Palette { .. }) {
            // Like Layers, retain the original contact while held, then make
            // the menu modal on release. Claiming the hold suppresses selection.
            let release = gtk::EventControllerLegacy::new();
            release.set_propagation_phase(gtk::PropagationPhase::Capture);
            release.connect_event(glib::clone!(
                #[weak(rename_to = panel)]
                self,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, event| {
                    if matches!(
                        event.event_type(),
                        gdk::EventType::TouchEnd
                            | gdk::EventType::TouchCancel
                            | gdk::EventType::ButtonRelease
                    ) && panel.context_menu.is_visible()
                        && !panel.context_menu.is_autohide()
                    {
                        panel.context_menu.popdown();
                        panel.context_menu.set_autohide(true);
                        if event.event_type() != gdk::EventType::TouchCancel {
                            panel.context_menu.popup();
                        }
                    }
                    glib::Propagation::Proceed
                }
            ));
            widget.add_controller(release);
        }
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Menu
                || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK))
            {
                popup(20., 20., false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        widget.add_controller(keys);
    }
    pub fn refresh(self: &Rc<Self>, state: &UiState, view: ViewColor, headroom: f32) {
        if !self.root.is_mapped() {
            return;
        }
        let colors = state.display_colors();
        let current = colors.definition();
        let library = &state.colors.library;
        let palette = library.active_palette();
        let display_changed = self.view.replace((view, headroom)) != (view, headroom);
        let old = self.library.borrow();
        let palette_changed = old
            .as_ref()
            .is_none_or(|old| old.active_palette() != palette)
            || display_changed;
        let history_changed = old
            .as_ref()
            .is_none_or(|old| old.history != library.history)
            || display_changed;
        let palettes_changed = old.as_ref().is_none_or(|old| {
            old.palettes != library.palettes
                || old.active != library.active
                || old.pending_name != library.pending_name
        });
        drop(old);
        if palettes_changed {
            self.cancel_reorder();
            *self.library.borrow_mut() = Some(library.clone());
        } else if history_changed {
            self.library
                .borrow_mut()
                .as_mut()
                .unwrap()
                .history
                .clone_from(&library.history);
        }
        let current_changed = self.current.replace(current) != current;
        if (current_changed || palettes_changed) && self.editing.replace(false) {
            self.name_stack.set_visible_child_name("label");
            self.error(None);
        }
        self.selected
            .set(layer_ui::selected_swatch(palette, current, self.selected.get()));
        self.selector_label.set_text(&palette.name);
        self.selector
            .set_tooltip_text(Some(&format!("Choose a palette · {}", palette.name)));
        let name = self
            .selected
            .get()
            .and_then(|id| library.swatch(id))
            .map_or_else(|| library.color_name(current), |s| s.name.clone());
        self.name_label.set_text(&name);
        self.name
            .set_tooltip_text(Some(&format!("{name} · Click to rename")));
        self.detail.set_text(&ColorLibrary::color_detail(colors));
        self.detail.set_tooltip_text(Some("sRGB hex preview; saved colors retain their original color space, alpha and HDR intensity"));
        self.name.set_sensitive(!colors.transparent());
        self.add_color.set_sensitive(!colors.transparent());
        if palette_changed {
            let mut previous: std::collections::HashMap<_, _> =
                self.tiles.take().into_iter().collect();
            let mut children = Vec::with_capacity(palette.swatches.len() + 1);
            for swatch in &palette.swatches {
                let tile = previous
                    .remove(&swatch.id)
                    .unwrap_or_else(|| self.tile(swatch.color, &swatch.name, Some(swatch.id)));
                let detail = ColorLibrary::tile_detail(&swatch.name, swatch.color);
                tile.set_tooltip_text(Some(&detail));
                tile.update_property(&[gtk::accessible::Property::Label(&detail)]);
                tile.child()
                    .unwrap()
                    .downcast::<ColorPatch>()
                    .unwrap()
                    .set_display_color(swatch.color, view, headroom);
                children.push(tile.clone().upcast());
                self.tiles.borrow_mut().push((swatch.id, tile));
            }
            children.push(self.add_color.clone().upcast());
            self.swatches.reconcile(children);
        }
        for (id, tile) in self.tiles.borrow().iter() {
            if Some(*id) == self.selected.get() {
                tile.add_css_class("selected");
            } else {
                tile.remove_css_class("selected");
            }
        }
        if history_changed {
            for (grid, expanded) in [(&self.history, false), (&self.expanded, true)] {
                grid.clear();
                for color in &library.history {
                    grid.append(&self.tile(*color, "Recently used", None));
                }
                if library.history.is_empty() {
                    for _ in 0..5 {
                        let placeholder = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        placeholder.add_css_class("palette-empty");
                        placeholder.set_tooltip_text(Some("Colors appear here after painting"));
                        grid.append(&placeholder);
                    }
                }
                let toggle = button(
                    if expanded {
                        "pan-up-symbolic"
                    } else {
                        "pan-down-symbolic"
                    },
                    if expanded {
                        "Collapse color history"
                    } else {
                        "Expand color history"
                    },
                    if expanded {
                        "palette-history-collapse"
                    } else {
                        "palette-history-expand"
                    },
                );
                toggle.connect_clicked(glib::clone!(
                    #[weak(rename_to=panel)]
                    self,
                    move |_| {
                        panel.expand_history(!expanded);
                    }
                ));
                grid.append(&toggle);
            }
        }
        if palettes_changed || display_changed {
            self.rebuild_chooser(library);
        }
    }
    fn rebuild_chooser(self: &Rc<Self>, library: &ColorLibrary) {
        self.populate_menu(
            &self.library_menu,
            library.menu(PaletteMenuTarget::Library).unwrap(),
        );
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for palette in &library.palettes {
            let row = gtk::ListBoxRow::new();
            row.set_tooltip_text(Some(&palette.name));
            row.set_widget_name(&format!("palette-choice-{}", palette.id));
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::builder()
                .label(&palette.name)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .xalign(0.)
                .hexpand(true)
                .build();
            content.append(&label);
            let preview = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            preview.add_css_class("palette-preview-strip");
            preview.set_overflow(gtk::Overflow::Hidden);
            preview.set_valign(gtk::Align::Center);
            for color in ColorLibrary::preview_colors(palette) {
                let patch = ColorPatch::new(false);
                patch.set_size_request(12, 18);
                patch.set_display_color(color, self.view.get().0, self.view.get().1);
                preview.append(&patch);
            }
            content.append(&preview);
            if palette.id == library.active_palette().id {
                row.add_css_class("selected-tool");
            }
            row.update_state(&[gtk::accessible::State::Selected(Some(
                palette.id == library.active_palette().id,
            ))]);
            self.install_menu(row.upcast_ref(), PaletteMenuTarget::Palette { id: palette.id });
            row.set_child(Some(&content));
            self.list.append(&row);
        }
        self.filter();
    }
    fn ask_name(self: &Rc<Self>, heading: &str, initial: &str, id: Option<u64>) {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return;
        };
        let dialog = adw::AlertDialog::builder().heading(heading).build();
        dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("save"));
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let entry = gtk::Entry::builder()
            .text(initial)
            .max_length(64)
            .activates_default(true)
            .build();
        entry.set_widget_name("palette-library-name");
        let library = self.library.borrow().clone().unwrap();
        entry.connect_changed(glib::clone!(
            #[weak]
            dialog,
            move |entry| {
                let action = if let Some(id) = id {
                    Action::RenamePalette {
                        id,
                        name: entry.text().into(),
                    }
                } else {
                    Action::CreatePalette {
                        name: entry.text().into(),
                    }
                };
                let result = library.check(action);
                dialog.set_response_enabled("save", result.is_ok());
                entry.set_tooltip_text(result.err().as_deref());
            }
        ));
        dialog.set_extra_child(Some(&entry));
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            async move {
                if crate::alert::choose(dialog, &w.window).await == "save" {
                    let action = if let Some(id) = id {
                        Action::RenamePalette {
                            id,
                            name: entry.text().into(),
                        }
                    } else {
                        Action::CreatePalette {
                            name: entry.text().into(),
                        }
                    };
                    if panel.apply(action) {
                        panel.browse(false);
                    }
                }
            }
        ));
    }
    fn import(self: &Rc<Self>) {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return;
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Palettes"));
        for extension in PaletteFormat::IMPORT_EXTENSIONS {
            filter.add_suffix(extension);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Import Palette")
            .filters(&filters)
            .build();
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            async move {
                let file = match dialog.open_future(Some(&w.window)).await {
                    Ok(file) => file,
                    Err(_) => return,
                };
                let Some(path) = file.path() else {
                    panel.error(Some("Choose a local palette file"));
                    return;
                };
                let result = gio::spawn_blocking(move || {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)
                        .and_then(|f| {
                            f.take((ColorLibrary::MAX_IMPORT_BYTES + 1) as u64)
                                .read_to_end(&mut bytes)
                        })
                        .map_err(|e| e.to_string())?;
                    ColorLibrary::import_file(
                        &bytes,
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Imported palette"),
                    )
                })
                .await;
                match result {
                    Ok(Ok(action)) => {
                        if panel.apply(action) {
                            panel.browse(false);
                        }
                    }
                    Ok(Err(error)) => panel.error(Some(&error)),
                    Err(_) => panel.error(Some("Could not read the palette file")),
                }
            }
        ));
    }
    fn remove_palette(self: &Rc<Self>, id: u64, name: &str) {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading("Remove Palette?")
            .body(format!("Remove “{name}” and its saved colors?"))
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            async move {
                if crate::alert::choose(dialog, &w.window).await == "remove" {
                    panel.apply(Action::RemovePalette { id });
                }
            }
        ));
    }
    fn export(self: &Rc<Self>, id: u64, format: PaletteFormat) {
        let Some(w) = self.workspace.borrow().upgrade() else {
            return;
        };
        let export = match self.library.borrow().as_ref().unwrap().export_palette(id, format) {
            Ok(export) => export,
            Err(error) => {
                self.error(Some(&error));
                return;
            }
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(format.label()));
        filter.add_suffix(format.extension());
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Export Palette")
            .filters(&filters)
            .default_filter(&filter)
            .initial_name(&export.file_name)
            .build();
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak(rename_to=panel)]
            self,
            async move {
                let Ok(file) = dialog.save_future(Some(&w.window)).await else {
                    return;
                };
                match file
                    .replace_contents_future(
                        export.bytes,
                        None,
                        false,
                        gio::FileCreateFlags::REPLACE_DESTINATION,
                    )
                    .await
                {
                    Ok(_) => panel.notice(export.notice.as_deref()),
                    Err((_, error)) => panel.error(Some(&error.to_string())),
                }
            }
        ));
    }
}
pub fn owns_native_key(focus: &gtk::Widget, key: gdk::Key) -> bool {
    if !matches!(
        key,
        gdk::Key::Escape | gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter
    ) {
        return false;
    }
    let mut widget = Some(focus.clone());
    while let Some(current) = widget {
        if current.has_css_class("palette-panel") || current.has_css_class("palette-key-scope") {
            return true;
        }
        widget = current.parent();
    }
    false
}
pub fn show(workspace: &Rc<Workspace>, slot: ColorSlot) {
    if slot == ColorSlot::Transparent {
        return;
    }
    workspace.dispatch(UiAction::Color {
        action: ColorAction::Select { slot },
    });
    let result = workspace
        .gpu
        .borrow_mut()
        .as_mut()
        .map(|g| g.session.reveal_panel(layer_ui::Panel::Palettes));
    if let Some(result) = result {
        workspace.changed(result);
    }
}
