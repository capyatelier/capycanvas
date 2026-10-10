//! Native segmented choices from the same model used by Tool Options bars.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ToolOption, ToolbarContext, UiAction};
use std::{cell::Cell, rc::Rc};

pub struct ExtraField {
    pub root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
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
            let editor=crate::effects::GradientEditor::new(w,control);
            editor.update_control(control);
            return Self {root:editor.root.clone(),gradient:Some(editor),buttons:Vec::new(),updating:Rc::new(Cell::new(false))};
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
                let size = if *labeled { layer_ui::tool_choice_style(*labeled, layer_ui::TileStyle::Small).size() } else { [24., 36.] };
                button.set_size_request(size[0] as i32, size[1] as i32);
                button.set_hexpand(!*labeled);
                button.child().and_downcast::<gtk::Image>().unwrap().set_pixel_size(
                    if *labeled { layer_ui::tool_choice_style(*labeled, layer_ui::TileStyle::Small).icon_size() as i32 } else { 20 });
            }
            button.set_widget_name(&format!("tool-choice-{id}-{index}"));
        }
        let root = if *labeled {
            let row = crate::panel_controls::row(label, &root);
            root.set_halign(gtk::Align::End);
            root.set_valign(gtk::Align::Center);
            if let Some(caption) = row.first_child().and_downcast::<gtk::Label>() { caption.set_wrap(true); }
            row
        } else { root };
        let field = Self { root, buttons, updating, gradient:None };
        field.refresh(option);
        field
    }
    pub fn refresh(&self, option: &ToolOption) {
        if let (Some(editor),ToolOption::Gradient(control))=(&self.gradient,option) {editor.update_control(control);return;}

        let ToolOption::Choice { label, items, .. } = option else {
            return;
        };
        self.root.update_property(&[gtk::accessible::Property::Label(label)]);
        if let Some(caption) = self.root.first_child().and_downcast::<gtk::Label>() { caption.set_label(label); }
        if let Some(bar) = self.root.last_child().and_downcast::<gtk::Box>() { bar.update_property(&[gtk::accessible::Property::Label(label)]); }
        self.updating.set(true);
        for (button, item) in self.buttons.iter().zip(items) {
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled);
            button.set_active(item.selected);
        }
        self.updating.set(false);
    }
}
