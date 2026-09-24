//! Reusable list, text, and information fields for Tools and Tool Options.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ToolListItem, ToolOption, ToolbarContext, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct ExtraField {
    pub root: gtk::Box,
    kind: Kind,
    updating: Rc<Cell<bool>>,
}
enum Kind {
    List {
        button: gtk::MenuButton,
        body: gtk::Box,
        items: Rc<RefCell<Vec<ToolListItem>>>,
        buttons: RefCell<Vec<gtk::CheckButton>>,
        multiple: bool,
    },
    Text {
        entry: gtk::Entry,
        value: Rc<RefCell<String>>,
        focus: gtk::EventControllerFocus,
    },
    Info(gtk::Label),
}
fn dispatch(w: &Rc<Workspace>, context: ToolbarContext, action: UiAction) {
    w.dispatch(UiAction::ToolbarEdit {
        context,
        action: Box::new(action),
    });
}
impl ExtraField {
    pub fn new(
        w: &Rc<Workspace>,
        option: &ToolOption,
        context: ToolbarContext,
        compact: bool,
    ) -> Self {
        let root = gtk::Box::new(
            if compact {
                gtk::Orientation::Horizontal
            } else {
                gtk::Orientation::Vertical
            },
            4,
        );
        root.set_hexpand(true);
        let updating = Rc::new(Cell::new(false));
        let label = |text: &str| {
            let l = gtk::Label::new(Some(text));
            l.set_xalign(0.);
            l.add_css_class("option-label");
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            root.append(&l);
        };
        let kind = match option {
            ToolOption::List {
                id,
                label: title,
                multiple,
                ..
            } => {
                label(title);
                let button = gtk::MenuButton::new();
                button.set_hexpand(true);
                button.set_label(title);
                button.set_widget_name(&format!("tool-list-{id}"));
                button.update_property(&[gtk::accessible::Property::Label(title)]);
                let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
                for edge in [
                    gtk::PositionType::Left,
                    gtk::PositionType::Right,
                    gtk::PositionType::Top,
                    gtk::PositionType::Bottom,
                ] {
                    match edge {
                        gtk::PositionType::Left => body.set_margin_start(8),
                        gtk::PositionType::Right => body.set_margin_end(8),
                        gtk::PositionType::Top => body.set_margin_top(8),
                        _ => body.set_margin_bottom(8),
                    }
                }
                let scroll = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .max_content_height(380)
                    .propagate_natural_height(true)
                    .child(&body)
                    .build();
                let popover = crate::squircle::Popover::new();
                popover.set_child(Some(&scroll));
                button.set_popover(Some(&popover));
                root.append(&button);
                Kind::List {
                    button,
                    body,
                    items: Rc::new(RefCell::new(Vec::new())),
                    buttons: RefCell::new(Vec::new()),
                    multiple: *multiple,
                }
            }
            ToolOption::Text {
                id,
                label: title,
                value: initial,
            } => {
                label(title);
                let entry = gtk::Entry::new();
                entry.set_hexpand(true);
                entry.set_width_chars(if compact { 12 } else { 16 });
                entry.set_max_width_chars(24);
                entry.set_max_length(128);
                entry.set_widget_name(&format!("tool-text-{id}"));
                entry.update_property(&[gtk::accessible::Property::Label(title)]);
                let value = Rc::new(RefCell::new(initial.clone()));
                let id = *id;
                let commit = Rc::new(glib::clone!(
                    #[weak]
                    w,
                    #[strong]
                    value,
                    #[strong]
                    updating,
                    move |entry: &gtk::Entry| {
                        if !updating.get() && entry.text().as_str() != value.borrow().as_str() {
                            dispatch(
                                &w,
                                context,
                                UiAction::SetToolText {
                                    id: id.into(),
                                    value: entry.text().to_string(),
                                },
                            );
                        }
                    }
                ));
                entry.connect_activate(glib::clone!(
                    #[strong]
                    commit,
                    move |entry| commit(entry)
                ));
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(glib::clone!(
                    #[weak]
                    entry,
                    move |_| commit(&entry)
                ));
                entry.add_controller(focus.clone());
                root.append(&entry);
                Kind::Text {
                    entry,
                    value,
                    focus,
                }
            }
            ToolOption::Info { id, .. } => {
                let info = gtk::Label::new(None);
                info.set_xalign(0.);
                info.add_css_class("dim-label");
                info.set_widget_name(&format!("tool-info-{id}"));
                if compact {
                    info.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    info.set_max_width_chars(24);
                } else {
                    info.set_wrap(true);
                    info.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                    info.set_max_width_chars(32);
                }
                root.append(&info);
                Kind::Info(info)
            }
            _ => unreachable!("additional tool field"),
        };
        let result = Self {
            root,
            kind,
            updating,
        };
        result.refresh(w, option, context);
        result
    }
    pub fn refresh(&self, w: &Rc<Workspace>, option: &ToolOption, context: ToolbarContext) {
        self.updating.set(true);
        match (&self.kind, option) {
            (
                Kind::Text {
                    entry,
                    value,
                    focus,
                },
                ToolOption::Text { value: next, .. },
            ) => {
                if *value.borrow() != *next {
                    value.replace(next.clone());
                }
                if !focus.contains_focus() && entry.text().as_str() != next {
                    entry.set_text(next);
                }
            }
            (Kind::Info(label), ToolOption::Info { text, .. }) => {
                label.set_text(text);
                label.set_tooltip_text(Some(text));
            }
            (
                Kind::List {
                    button,
                    body,
                    items,
                    buttons,
                    multiple,
                },
                ToolOption::List {
                    id,
                    label,
                    items: next,
                    ..
                },
            ) => {
                let same = items.borrow().len() == next.len()
                    && items
                        .borrow()
                        .iter()
                        .zip(next)
                        .all(|(a, b)| a.label == b.label && a.action == b.action);
                items.replace(next.clone());
                if !same {
                    while let Some(child) = body.first_child() {
                        body.remove(&child);
                    }
                    buttons.borrow_mut().clear();
                    for (index, item) in next.iter().enumerate() {
                        let check = gtk::CheckButton::with_label(&item.label);
                        check.set_size_request(-1, 36);
                        check.set_widget_name(&format!("tool-list-{id}-{index}"));
                        if !*multiple && let Some(first) = buttons.borrow().first() {
                            check.set_group(Some(first));
                        }
                        let multiple = *multiple;
                        check.connect_toggled(glib::clone!(
                            #[weak]
                            w,
                            #[weak]
                            button,
                            #[strong]
                            items,
                            #[strong(rename_to=updating)]
                            self.updating,
                            move |check| {
                                if !updating.get() && (multiple || check.is_active()) {
                                    let action =
                                        items.borrow().get(index).map(|i| i.action.clone());
                                    if let Some(action) = action {
                                        if !multiple {
                                            button.popdown();
                                        }
                                        dispatch(&w, context, action);
                                    }
                                }
                            }
                        ));
                        body.append(&check);
                        buttons.borrow_mut().push(check);
                    }
                }
                for (check, item) in buttons.borrow().iter().zip(next) {
                    check.set_active(item.selected);
                }
                let selected: Vec<_> = next
                    .iter()
                    .filter(|i| i.selected)
                    .map(|i| i.label.as_str())
                    .collect();
                let summary = if selected.is_empty() {
                    if *multiple {
                        "None".into()
                    } else {
                        label.to_string()
                    }
                } else if selected.len() == 1 {
                    selected[0].into()
                } else {
                    format!("{} + {}", selected[0], selected.len() - 1)
                };
                button.set_label(&summary);
                button.set_tooltip_text(Some(&selected.join(", ")));
            }
            _ => (),
        }
        self.updating.set(false);
    }
}
