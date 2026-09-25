//! Native radio presentation of the same choices used by Tool Options bars.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};
use layer_ui::{ToolOption, ToolbarContext, UiAction};
use std::{cell::Cell, rc::Rc};

mod radio {
    use super::*;
    #[derive(Default)]
    pub struct Button;
    #[glib::object_subclass]
    impl ObjectSubclass for Button {
        const NAME: &'static str = "CapyToolRadio";
        type Type = super::RadioButton;
        type ParentType = gtk::CheckButton;
    }
    impl ObjectImpl for Button {}
    impl WidgetImpl for Button {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            // Radio circles communicate one choice. Preserve them through the
            // workspace's normal conversion of rounded controls to squircles.
            let round = gtk::Snapshot::new();
            self.parent_snapshot(&round);
            crate::squircle::append_round(snapshot, round);
        }
    }
    impl CheckButtonImpl for Button {}
}
glib::wrapper! {
    pub struct RadioButton(ObjectSubclass<radio::Button>) @extends gtk::CheckButton, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

pub struct ExtraField {
    pub root: gtk::Box,
    buttons: Vec<gtk::CheckButton>,
    updating: Rc<Cell<bool>>,
}
impl Drop for ExtraField {
    fn drop(&mut self) {
        self.updating.set(true);
    }
}
impl ExtraField {
    pub fn new(w: &Rc<Workspace>, option: &ToolOption, context: ToolbarContext) -> Self {
        let ToolOption::Choice {
            id, label, items, ..
        } = option
        else {
            unreachable!("additional tool choice")
        };
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.update_property(&[gtk::accessible::Property::Label(label)]);
        let grid = gtk::FlowBox::builder()
            .column_spacing(4)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(2)
            .selection_mode(gtk::SelectionMode::None)
            .build();
        grid.add_css_class("tool-choices");
        let updating = Rc::new(Cell::new(false));
        let mut buttons: Vec<gtk::CheckButton> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let check = glib::Object::builder::<RadioButton>()
                .property("label", item.label)
                .build()
                .upcast::<gtk::CheckButton>();
            check.set_size_request(-1, 30);
            check.set_widget_name(&format!("tool-choice-{id}-{index}"));
            if let Some(first) = buttons.first() {
                check.set_group(Some(first));
            }
            let action = item.action.clone();
            check.connect_toggled(glib::clone!(
                #[weak]
                w,
                #[strong]
                updating,
                move |check| {
                    if !updating.get() && check.is_active() {
                        w.dispatch(UiAction::ToolbarEdit {
                            context,
                            action: Box::new(action.clone()),
                        });
                    }
                }
            ));
            grid.insert(&check, -1);
            buttons.push(check);
        }
        root.append(&grid);
        let field = Self {
            root,
            buttons,
            updating,
        };
        field.refresh(option);
        field
    }
    pub fn refresh(&self, option: &ToolOption) {
        let ToolOption::Choice { items, .. } = option else {
            return;
        };
        self.updating.set(true);
        for (button, item) in self.buttons.iter().zip(items) {
            button.set_active(item.selected);
        }
        self.updating.set(false);
    }
}
