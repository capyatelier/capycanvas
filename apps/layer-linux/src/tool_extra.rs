//! Native segmented choices from the same model used by Tool Options bars.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ToolOption, ToolbarContext, UiAction};
use std::{cell::Cell, rc::Rc};

pub struct ExtraField {
    pub root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
    segments: Option<adw::ToggleGroup>,
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
            return Self {root:editor.root.clone(),gradient:Some(editor),buttons:Vec::new(),segments:None,updating:Rc::new(Cell::new(false))};
        }
        let ToolOption::Choice {
            id, label, items, columns, labeled, segmented: true, ..
        } = option
        else {
            unreachable!("additional segmented tool choice")
        };
        if *labeled {
            let ids: Vec<_> = (0..items.len()).map(|i| i.to_string()).collect();
            let choices: Vec<_> = ids.iter().zip(items).map(|(id, item)| (id.as_str(), item.label.as_ref())).collect();
            let segments = crate::panel_controls::segmented(&format!("tool-choice-bar-{id}"), &choices);
            segments.set_homogeneous(false);
            segments.add_css_class("text-segments");
            segments.upcast_ref::<gtk::Widget>().update_property(&[gtk::accessible::Property::Label(label)]);
            let root = crate::panel_controls::row(label, &segments);
            if let Some(caption) = root.first_child().and_downcast::<gtk::Label>() { caption.set_width_chars(6); caption.set_wrap(true); }
            let updating = Rc::new(Cell::new(false));
            let actions: Vec<_> = items.iter().map(|item| item.action.clone()).collect();
            segments.connect_active_notify(glib::clone!(#[weak] w, #[strong] updating, move |group| {
                if !updating.get() && let Some(action) = actions.get(group.active() as usize) {
                    w.dispatch(UiAction::ToolbarEdit { context, action: Box::new(action.clone()) });
                }
            }));
            let field = Self { root, buttons:Vec::new(), segments:Some(segments), updating, gradient:None };
            field.refresh(option);
            return field;
        }
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
        let field = Self { root, buttons, segments:None, updating, gradient:None };
        field.refresh(option);
        field
    }
    pub fn refresh(&self, option: &ToolOption) {
        if let (Some(editor),ToolOption::Gradient(control))=(&self.gradient,option) {editor.update_control(control);return;}

        let ToolOption::Choice { label, items, .. } = option else {
            return;
        };
        self.root.update_property(&[gtk::accessible::Property::Label(label)]);
        self.updating.set(true);
        if let Some(segments) = &self.segments {
            if let Some(caption) = self.root.first_child().and_downcast::<gtk::Label>() { caption.set_label(label); }
            segments.upcast_ref::<gtk::Widget>().update_property(&[gtk::accessible::Property::Label(label)]);
            for (index, item) in items.iter().enumerate() {
                if let Some(toggle) = segments.toggle(index as u32) {
                    toggle.set_label(Some(&item.label));
                    if let Some(caption) = toggle.child().and_downcast::<gtk::Label>() { caption.set_label(&item.label); }
                    else {
                        let caption = gtk::Label::builder().label(&*item.label).wrap(true).wrap_mode(gtk::pango::WrapMode::Word).justify(gtk::Justification::Center).build();
                        toggle.set_child(Some(&caption));
                    }
                }
            }
            segments.set_active(items.iter().position(|item| item.selected).map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
        }
        for (button, item) in self.buttons.iter().zip(items) {
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_active(item.selected);
        }
        self.updating.set(false);
    }
}
