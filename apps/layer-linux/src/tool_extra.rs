//! Native segmented choices from the same model used by Tool Options bars.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ToolOption, ToolbarContext, UiAction};
use std::{cell::Cell, rc::Rc};

pub struct ExtraField {
    pub root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
    checks: Vec<gtk::CheckButton>,
    choice: Option<gtk::DropDown>,
    gradient: Option<crate::effects::GradientEditor>,
    updating: Rc<Cell<bool>>,
}
impl Drop for ExtraField {
    fn drop(&mut self) {
        self.updating.set(true);
    }
}
impl ExtraField {
    pub fn new(w: &Rc<Workspace>, option: &ToolOption, context: ToolbarContext) -> Self {
        if let ToolOption::Gradient(control)=option {
            let editor=crate::effects::GradientEditor::new(w,control,crate::panel_controls::CONTROL_SPACING);
            editor.update_control(control);
            return Self {root:editor.root.clone(),gradient:Some(editor),buttons:Vec::new(),checks:Vec::new(),choice:None,updating:Rc::new(Cell::new(false))};
        }
        if let ToolOption::Checklist { id, label, items } = option {
            let root = gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
            let caption = gtk::Label::new(Some(label)); caption.set_xalign(0.); root.append(&caption);
            let updating = Rc::new(Cell::new(false));
            let checks = items.iter().enumerate().map(|(index, item)| {
                let check = gtk::CheckButton::with_label(&item.label);
                check.set_widget_name(&format!("tool-check-{id}-{index}"));
                let (action, updating) = (item.action.clone(), updating.clone());
                check.connect_toggled(glib::clone!(#[weak] w, move |_| {
                    if !updating.get() { w.dispatch(UiAction::ToolbarEdit { context, action: Box::new(action.clone()) }); }
                }));
                root.append(&check); check
            }).collect();
            let field = Self { root, buttons:Vec::new(), checks, choice:None, gradient:None, updating }; field.refresh(option); return field;
        }
        if let ToolOption::Choice { id, label, items, segmented:false, .. } = option {
            let choice = crate::panel_controls::command_dropdown(&[]);
            choice.set_widget_name(&format!("tool-choice-{id}"));
            choice.set_hexpand(true);
            let root = crate::panel_controls::row(label, &choice, crate::panel_controls::CONTROL_SPACING, None);
            let updating = Rc::new(Cell::new(false));
            let (actions, guard): (Vec<_>, _) = (items.iter().map(|item| item.action.clone()).collect(), updating.clone());
            choice.connect_selected_notify(glib::clone!(#[weak] w, move |choice| {
                if !guard.get() && let Some(action) = actions.get(choice.selected() as usize) {
                    w.dispatch(UiAction::ToolbarEdit { context, action: Box::new(action.clone()) });
                }
            }));
            let field = Self { root, buttons:Vec::new(), checks:Vec::new(), choice:Some(choice), gradient:None, updating }; field.refresh(option); return field;
        }
        let ToolOption::Choice {
            id, label, items, columns, labeled, segmented: true, ..
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
        let send = glib::clone!(#[weak] w, #[strong] updating, move |action: UiAction| {
            if !updating.get() { w.dispatch(UiAction::ToolbarEdit { context, action: Box::new(action) }); }
        });
        let mut buttons = if columns.is_none() {
            crate::workspace::toolbar_components::segment_buttons(&root, label, items, false, send.clone())
        } else { Vec::new() };
        if let (Some(grid), Some(columns)) = (&grid, columns) {
        for (index, item) in items.iter().enumerate() {
            let button = gtk::ToggleButton::new();
            let icon = crate::icons::image(&format!("layer-{}-symbolic", item.icon));
            icon.set_pixel_size(6);
            button.set_child(Some(&icon));
            button.set_size_request(18, 18);
            button.set_hexpand(true);
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            let (action, send) = (item.action.clone(), send.clone());
            button.connect_toggled(move |button| { if button.is_active() { send(action.clone()); } });
            let width = usize::from(*columns).max(1);
            grid.attach(&button, (index % width) as i32, (index / width) as i32, 1, 1);
            buttons.push(button);
        }
        }
        for (index, button) in buttons.iter().enumerate() {
            if columns.is_none() {
                button.set_size_request(layer_ui::PANEL_CONTROL_METRICS.control_height as i32, layer_ui::PANEL_CONTROL_METRICS.control_height as i32);
                button.set_hexpand(!*labeled);
                button.child().and_downcast::<gtk::Image>().unwrap().set_pixel_size(16);
            }
            button.set_widget_name(&format!("tool-choice-{id}-{index}"));
        }
        let root = if *labeled {
            let row = crate::panel_controls::row(label, &root, crate::panel_controls::CONTROL_SPACING, None);
            root.set_halign(gtk::Align::End);
            root.set_valign(gtk::Align::Center);
            if let Some(caption) = row.first_child().and_downcast::<gtk::Label>() {
                caption.set_width_chars(1);
                crate::panel_controls::two_lines(&caption);
            }
            row
        } else { root };
        let field = Self { root, buttons, updating, gradient:None, checks:Vec::new(), choice:None };
        field.refresh(option);
        field
    }
    pub fn refresh(&self, option: &ToolOption) {
        if let (Some(editor),ToolOption::Gradient(control))=(&self.gradient,option) {editor.update_control(control);return;}

        if let (Some(choice), ToolOption::Choice { label, items, .. }) = (&self.choice, option) {
            self.updating.set(true);
            if let Some(caption) = self.root.first_child().and_downcast::<gtk::Label>() { caption.set_label(label); }
            choice.update_property(&[gtk::accessible::Property::Label(label)]);
            crate::panel_controls::update_tool_choices(choice, items);
            choice.set_selected(items.iter().position(|item| item.selected).map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
            self.updating.set(false); return;
        }

        if let ToolOption::Checklist { label, items, .. } = option {
            if let Some(caption) = self.root.first_child().and_downcast::<gtk::Label>() { caption.set_label(label); }
            self.updating.set(true);
            for (check, item) in self.checks.iter().zip(items) { check.set_label(Some(&item.label)); check.set_sensitive(item.enabled); check.set_active(item.selected); }
            self.updating.set(false); return;
        }
        let ToolOption::Choice { label, items, .. } = option else {
            return;
        };
        self.root.update_property(&[gtk::accessible::Property::Label(label)]);
        if let Some(caption) = self.root.first_child().and_downcast::<gtk::Label>() { caption.set_label(label); }
        if let Some(bar) = self.root.last_child().and_downcast::<gtk::Box>() { bar.update_property(&[gtk::accessible::Property::Label(label)]); }
        self.updating.set(true);
        for (button, item) in self.buttons.iter().zip(items) {
            button.set_tooltip_text(Some(item.disabled_reason.as_deref().unwrap_or(&item.label)));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled);
            button.set_active(item.selected);
        }
        self.updating.set(false);
    }
}
