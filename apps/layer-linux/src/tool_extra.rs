//! Native segmented choices from the same model used by Tool Options bars.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ToolOption, ToolbarContext, UiAction};
use std::{cell::Cell, rc::Rc};

pub struct ExtraField {
    pub root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
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
            id, label, items, columns, segmented: true, ..
        } = option
        else {
            unreachable!("additional segmented tool choice")
        };
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_homogeneous(true);
        if columns.is_none() { root.add_css_class("linked"); }
        root.add_css_class("selection-modes");
        root.set_widget_name(&format!("tool-choice-bar-{id}"));
        root.update_property(&[gtk::accessible::Property::Label(label)]);
        root.set_tooltip_text(Some(label));
        let grid = columns.map(|_| {
            root.add_css_class("choice-grid");
            root.set_hexpand(false);
            root.set_halign(gtk::Align::Start);
            root.set_valign(gtk::Align::Center);
            let grid = gtk::Grid::new();
            grid.set_column_homogeneous(true); grid.set_row_homogeneous(true);
            root.append(&grid); grid
        });
        let updating = Rc::new(Cell::new(false));
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let button = gtk::ToggleButton::new();
            let icon = crate::icons::image(&format!("layer-{}-symbolic", item.icon));
            icon.set_pixel_size(if columns.is_some() { 6 } else { 20 });
            button.set_child(Some(&icon));
            button.set_size_request(if columns.is_some() { 18 } else { 24 }, if columns.is_some() { 18 } else { 36 });
            button.set_hexpand(true);
            button.set_widget_name(&format!("tool-choice-{id}-{index}"));
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            let action = item.action.clone();
            button.connect_toggled(glib::clone!(
                #[weak]
                w,
                #[strong]
                updating,
                move |button| {
                    if !updating.get() && button.is_active() {
                        w.dispatch(UiAction::ToolbarEdit {
                            context,
                            action: Box::new(action.clone()),
                        });
                    }
                }
            ));
            if let (Some(grid), Some(columns)) = (&grid, columns) {
                let width = usize::from(*columns).max(1);
                grid.attach(&button, (index % width) as i32, (index / width) as i32, 1, 1);
            } else { root.append(&button); }
            buttons.push(button);
        }
        let field = Self { root, buttons, updating };
        field.refresh(option);
        field
    }
    pub fn refresh(&self, option: &ToolOption) {
        let ToolOption::Choice { label, items, .. } = option else {
            return;
        };
        self.root.update_property(&[gtk::accessible::Property::Label(label)]);
        self.updating.set(true);
        for (button, item) in self.buttons.iter().zip(items) {
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_active(item.selected);
        }
        self.updating.set(false);
    }
}
