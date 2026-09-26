//! Native focus, IME and popup capture around the shared command search model.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, glib};
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct CommandBar {
    popup: gtk::Popover,
    entry: gtk::SearchEntry,
    unit: gtk::Label,
    list: gtk::ListBox,
    detail: gtk::Label,
    empty: gtk::Label,
    view: RefCell<Option<CommandSearchView>>,
    signature: RefCell<String>,
    updating: Cell<bool>,
    previous_focus: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
}

impl CommandBar {
    pub fn is_open(&self) -> bool {
        self.view.borrow().is_some()
    }
    pub fn new() -> Self {
        let popup = gtk::Popover::new();
        popup.set_widget_name("command-bar");
        popup.set_has_arrow(false);
        popup.set_position(gtk::PositionType::Bottom);
        popup.add_css_class("command-bar");
        let body = gtk::Box::new(gtk::Orientation::Vertical, COMMAND_SEARCH_STYLE.gap);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, COMMAND_SEARCH_STYLE.gap);
        let entry = gtk::SearchEntry::new();
        entry.set_widget_name("command-search");
        entry.set_placeholder_text(Some("Search commands"));
        entry.set_hexpand(true);
        entry.update_property(&[gtk::accessible::Property::Label("Search commands")]);
        let unit = gtk::Label::new(None);
        unit.add_css_class("dim-label");
        unit.set_visible(false);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_widget_name("command-search-close");
        close.set_tooltip_text(Some("Close command search"));
        close.add_css_class("flat");
        close.connect_clicked(glib::clone!(
            #[weak]
            popup,
            move |_| popup.popdown()
        ));
        header.append(&entry);
        header.append(&unit);
        header.append(&close);
        body.append(&header);
        let list = gtk::ListBox::new();
        list.set_widget_name("command-results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        list.add_css_class("navigation-sidebar");
        body.append(&list);
        let empty = gtk::Label::new(Some("No matching commands"));
        empty.add_css_class("dim-label");
        empty.set_margin_top(12);
        empty.set_margin_bottom(12);
        body.append(&empty);
        let detail = gtk::Label::new(None);
        detail.add_css_class("dim-label");
        detail.add_css_class("caption");
        detail.set_xalign(0.);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_margin_start(12);
        detail.set_margin_end(12);
        detail.set_height_request(20);
        body.append(&detail);
        popup.set_child(Some(&body));
        Self {
            popup,
            entry,
            unit,
            list,
            detail,
            empty,
            view: Default::default(),
            signature: Default::default(),
            updating: Cell::new(false),
            previous_focus: Default::default(),
        }
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        self.popup.set_parent(&w.surface);
        w.watch_popover(&self.popup);
        self.entry.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if !w.command_bar.updating.get()
                    && w.command_bar
                        .view
                        .borrow()
                        .as_ref()
                        .is_some_and(|v| v.parameter.is_none())
                {
                    w.dispatch(UiAction::CommandSearch {
                        action: CommandSearchAction::Query {
                            text: entry.text().into(),
                        },
                    });
                }
            }
        ));
        self.entry.connect_activate(glib::clone!(
            #[weak]
            w,
            move |_| w.command_bar.execute(&w, None)
        ));
        self.list.connect_row_activated(glib::clone!(
            #[weak]
            w,
            move |_, row| w.command_bar.execute(&w, Some(row.index() as usize))
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let action = match key {
                    gdk::Key::Up => Some(CommandSearchAction::Move { delta: -1 }),
                    gdk::Key::Down => Some(CommandSearchAction::Move { delta: 1 }),
                    gdk::Key::Escape
                        if w.command_bar
                            .view
                            .borrow()
                            .as_ref()
                            .is_some_and(|v| v.parameter.is_some()) =>
                    {
                        Some(CommandSearchAction::Back)
                    }
                    gdk::Key::Escape => {
                        w.command_bar.popup.popdown();
                        return glib::Propagation::Stop;
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    w.dispatch(UiAction::CommandSearch { action });
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak]
            w,
            move |_, key, _, modifiers| {
                w.interact(crate::input::key_input(key, false, modifiers, true, None));
            }
        ));
        self.popup.add_controller(keys);
        self.popup.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                let canceled = w.command_bar.view.borrow().is_some();
                if canceled {
                    w.dispatch(UiAction::CommandSearch {
                        action: CommandSearchAction::Close,
                    });
                    if let Some(focus) = w
                        .command_bar
                        .previous_focus
                        .borrow_mut()
                        .take()
                        .and_then(|f| f.upgrade())
                    {
                        focus.grab_focus();
                    }
                }
            }
        ));
    }

    fn execute(&self, w: &Rc<Workspace>, index: Option<usize>) {
        if index.is_none() {
            w.dispatch(UiAction::CommandSearch {
                action: CommandSearchAction::Commit {
                    text: self.entry.text().into(),
                },
            });
            return;
        }
        let request = self.view.borrow().as_ref().and_then(|v| {
            let d = v
                .parameter
                .as_ref()
                .or_else(|| v.results.get(index.unwrap_or(v.selected)))?;
            Some(CommandSearchAction::Execute {
                id: d.id.clone(),
                value: v.parameter.as_ref().map(|_| self.entry.text().into()),
            })
        });
        if let Some(action) = request {
            w.dispatch(UiAction::CommandSearch { action });
        }
    }

    pub fn refresh(&self, w: &Rc<Workspace>, view: Option<CommandSearchView>) {
        self.updating.set(true);
        let was_open = self.view.borrow().is_some();
        let old_parameter = self
            .view
            .borrow()
            .as_ref()
            .and_then(|v| v.parameter.as_ref().map(|p| p.id.clone()));
        *self.view.borrow_mut() = view.clone();
        let Some(view) = view else {
            self.popup.popdown();
            if was_open && w.window.visible_dialog().is_none() {
                if let Some(focus) = self
                    .previous_focus
                    .borrow_mut()
                    .take()
                    .and_then(|f| f.upgrade())
                    .filter(|f| f.is_mapped())
                {
                    focus.grab_focus();
                } else {
                    w.area.grab_focus();
                }
            }
            self.updating.set(false);
            return;
        };
        let parameter = view.parameter.as_ref();
        let unit = parameter
            .and_then(|p| p.parameter.as_ref())
            .map(|p| p.numeric.unit.as_str())
            .unwrap_or("");
        self.unit.set_text(unit);
        self.unit.set_visible(!unit.is_empty());
        self.entry
            .update_property(&[gtk::accessible::Property::Label(
                parameter
                    .map(|p| p.label.as_str())
                    .unwrap_or("Search commands"),
            )]);
        if !was_open || parameter.map(|p| &p.id) != old_parameter.as_ref() {
            self.entry.set_text(
                parameter
                    .and_then(|p| p.parameter.as_ref())
                    .map(|p| p.text.as_str())
                    .unwrap_or(&view.query),
            );
            if parameter.is_some() {
                self.entry.select_region(0, -1);
            }
        }
        self.entry
            .set_placeholder_text(Some(if parameter.is_some() {
                "Enter a value"
            } else {
                "Search commands"
            }));
        let signature = serde_json::to_string(&view.results).unwrap();
        if *self.signature.borrow() != signature {
            *self.signature.borrow_mut() = signature;
            while let Some(row) = self.list.first_child() {
                self.list.remove(&row);
            }
            for (i, command) in view.results.iter().enumerate() {
                let row = gtk::ListBoxRow::new();
                row.set_height_request(COMMAND_SEARCH_STYLE.row_height);
                row.set_widget_name(&format!("command-result-{i}"));
                let content =
                    gtk::Box::new(gtk::Orientation::Horizontal, COMMAND_SEARCH_STYLE.inset);
                let label = gtk::Label::new(Some(&command.label));
                label.set_xalign(0.);
                label.set_hexpand(true);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                if !command.enabled {
                    label.add_css_class("dim-label");
                }
                content.append(&label);
                if command.selected {
                    let check = crate::icons::image("layer-check-symbolic");
                    check.set_pixel_size(16);
                    content.append(&check);
                }
                let shortcut = gtk::Label::new(Some(&command.shortcut));
                shortcut.add_css_class("dim-label");
                shortcut.add_css_class("caption");
                content.append(&shortcut);
                row.set_child(Some(&content));
                row.update_property(&[gtk::accessible::Property::Description(
                    command
                        .disabled_reason
                        .as_deref()
                        .unwrap_or(&command.category),
                )]);
                let motion = gtk::EventControllerMotion::new();
                let id = command.id.clone();
                motion.connect_motion(glib::clone!(
                    #[weak]
                    w,
                    move |_, _, _| {
                        let selected = w
                            .command_bar
                            .view
                            .borrow()
                            .as_ref()
                            .and_then(|v| v.results.get(v.selected))
                            .is_some_and(|d| d.id == id);
                        if !selected {
                            w.dispatch(UiAction::CommandSearch {
                                action: CommandSearchAction::Select { id: id.clone() },
                            });
                        }
                    }
                ));
                row.add_controller(motion);
                self.list.append(&row);
            }
        }
        self.list.set_visible(parameter.is_none());
        self.empty
            .set_visible(parameter.is_none() && view.results.is_empty());
        if let Some(row) = self.list.row_at_index(view.selected as i32) {
            self.list.select_row(Some(&row));
            if parameter.is_none() {
                self.entry
                    .update_relation(&[gtk::accessible::Relation::ActiveDescendant(
                        row.upcast_ref(),
                    )]);
            }
        }
        if parameter.is_some() || view.results.is_empty() {
            self.entry
                .reset_relation(gtk::AccessibleRelation::ActiveDescendant);
        }
        let selected = parameter.or_else(|| view.results.get(view.selected));
        self.detail.set_text(
            view.error
                .as_deref()
                .or_else(|| selected.and_then(|d| d.disabled_reason.as_deref()))
                .unwrap_or_else(|| {
                    selected
                        .map(|d| {
                            if parameter.is_some() {
                                d.label.as_str()
                            } else {
                                d.category.as_str()
                            }
                        })
                        .unwrap_or("")
                }),
        );
        if !was_open {
            *self.previous_focus.borrow_mut() =
                gtk::prelude::GtkWindowExt::focus(&w.window).map(|f| f.downgrade());
            self.popup.set_width_request(
                (w.surface.width() - COMMAND_SEARCH_STYLE.inset * 4)
                    .clamp(240, COMMAND_SEARCH_STYLE.width),
            );
            self.popup
                .set_pointing_to(Some(&gdk::Rectangle::new(w.surface.width() / 2, 72, 0, 0)));
            self.popup.popup();
            self.entry.grab_focus();
        }
        self.updating.set(false);
    }
}

impl Drop for CommandBar {
    fn drop(&mut self) {
        self.popup.unparent();
    }
}
