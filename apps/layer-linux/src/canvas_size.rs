//! Native presentation of the shared Canvas Size dialog. Sizes, units,
//! validation and history live in layer-ui; this only projects the view.
use crate::number_control::NumberControl;
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{CanvasAnchor, CanvasSizeAction, CanvasSizeUnit, CanvasSizeView, NumericControl, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// Each field's number control and the spec it was built for.
type Numbers = RefCell<[Option<(NumericControl, NumberControl)>; 2]>;

pub struct CanvasSizeDialog {
    pub(crate) dialog: adw::AlertDialog,
    shown: Rc<Cell<bool>>,
    updating: Rc<Cell<bool>>,
    workspace: RefCell<Weak<Workspace>>,
    fields: [gtk::Box; 2],
    numbers: Numbers,
    unit: gtk::DropDown,
    relative: gtk::CheckButton,
    anchor_label: gtk::Label,
    anchors: Vec<(CanvasAnchor, gtk::ToggleButton, gtk::Image)>,
    message: gtk::Label,
}

fn send(workspace: &Weak<Workspace>, action: CanvasSizeAction) {
    if let Some(w) = workspace.upgrade() {
        w.dispatch(UiAction::CanvasSize { action });
    }
}

impl CanvasSizeDialog {
    pub fn new() -> Self {
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_widget_name("canvas-size-dialog");
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("apply", "Apply");
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("apply"));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let fields = ["width", "height"].map(|name| {
            let field = gtk::Box::new(gtk::Orientation::Vertical, 0);
            field.set_widget_name(&format!("canvas-size-{name}"));
            content.append(&field);
            field
        });
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let units: Vec<_> = CanvasSizeUnit::ALL.iter().map(|u| u.label()).collect();
        let unit = gtk::DropDown::from_strings(&units);
        unit.set_widget_name("canvas-size-unit");
        unit.set_hexpand(true);
        options.append(&unit);
        let relative = gtk::CheckButton::new();
        relative.set_widget_name("canvas-size-relative");
        options.append(&relative);
        content.append(&options);
        let anchor_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let anchor_label = gtk::Label::new(None);
        anchor_label.set_xalign(0.);
        anchor_label.set_valign(gtk::Align::Center);
        anchor_label.set_hexpand(true);
        anchor_row.append(&anchor_label);
        let grid = gtk::Grid::new();
        grid.set_widget_name("canvas-size-anchor");
        grid.set_row_spacing(2);
        grid.set_column_spacing(2);
        grid.set_halign(gtk::Align::End);
        let mut group: Option<gtk::ToggleButton> = None;
        let anchors = CanvasAnchor::ALL
            .into_iter()
            .map(|anchor| {
                let button = gtk::ToggleButton::new();
                button.set_widget_name(&format!("canvas-size-anchor-{}", serde_json::to_value(anchor).unwrap().as_str().unwrap()));
                button.set_size_request(32, 32);
                button.set_focus_on_click(false);
                button.set_tooltip_text(Some(anchor.label()));
                button.update_property(&[gtk::accessible::Property::Label(anchor.label())]);
                button.set_group(group.as_ref());
                group.get_or_insert_with(|| button.clone());
                let image = crate::icons::image("layer-rectangle-fill-symbolic");
                button.set_child(Some(&image));
                let [column, row] = anchor.cell();
                grid.attach(&button, column as i32, row as i32, 1, 1);
                (anchor, button, image)
            })
            .collect();
        anchor_row.append(&grid);
        content.append(&anchor_row);
        let message = gtk::Label::new(None);
        message.set_widget_name("canvas-size-message");
        message.set_wrap(true);
        message.set_xalign(0.);
        message.add_css_class("dim-label");
        content.append(&message);
        dialog.set_extra_child(Some(&content));
        Self {
            dialog,
            shown: Rc::new(Cell::new(false)),
            updating: Rc::new(Cell::new(false)),
            workspace: RefCell::new(Weak::new()),
            fields,
            numbers: Numbers::default(),
            unit,
            relative,
            anchor_label,
            anchors,
            message,
        }
    }

    /// Typed values the fields have not committed yet.
    fn commit(numbers: &Numbers) {
        let controls: Vec<_> = numbers.borrow().iter().flatten().map(|(_, n)| n.clone()).collect();
        for number in controls {
            number.commit_text();
        }
    }

    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>) {
        *self.workspace.borrow_mut() = Rc::downgrade(w);
        let this = Rc::downgrade(self);
        self.unit.connect_selected_notify(glib::clone!(#[strong] this, move |unit| {
            let Some(this) = this.upgrade() else { return };
            if this.updating.get() { return; }
            let Some(&unit) = CanvasSizeUnit::ALL.get(unit.selected() as usize) else { return };
            Self::commit(&this.numbers);
            send(&this.workspace.borrow(), CanvasSizeAction::Unit { unit });
        }));
        self.relative.connect_toggled(glib::clone!(#[strong] this, move |check| {
            let Some(this) = this.upgrade() else { return };
            if this.updating.get() { return; }
            Self::commit(&this.numbers);
            send(&this.workspace.borrow(), CanvasSizeAction::Relative { relative: check.is_active() });
        }));
        for (anchor, button, _) in &self.anchors {
            let anchor = *anchor;
            button.connect_toggled(glib::clone!(#[strong] this, move |button| {
                let Some(this) = this.upgrade() else { return };
                if this.updating.get() || !button.is_active() { return; }
                Self::commit(&this.numbers);
                send(&this.workspace.borrow(), CanvasSizeAction::Anchor { anchor });
            }));
        }
        let shown = self.shown.clone();
        self.dialog.connect_response(None, glib::clone!(#[strong] this, move |_, response| {
            let Some(this) = this.upgrade() else { return };
            if response == "apply" {
                Self::commit(&this.numbers);
            }
            if !shown.replace(false) { return; }
            send(&this.workspace.borrow(), if response == "apply" {
                CanvasSizeAction::Apply
            } else {
                CanvasSizeAction::Cancel
            });
        }));
    }

    fn number(&self, axis: usize, view: &CanvasSizeView) -> NumberControl {
        let spec = &view.numeric[axis];
        if let Some((current, number)) = &self.numbers.borrow()[axis]
            && current == spec
        {
            return number.clone();
        }
        let number = NumberControl::new(spec.clone(), view.labels[axis], "");
        let workspace = self.workspace.borrow().clone();
        let updating = self.updating.clone();
        number.connect_value_changed(move |number| {
            if updating.get() { return; }
            let value = number.value();
            send(&workspace, if axis == 0 {
                CanvasSizeAction::Width { value }
            } else {
                CanvasSizeAction::Height { value }
            });
        });
        let field = &self.fields[axis];
        while let Some(child) = field.first_child() {
            field.remove(&child);
        }
        field.append(&number);
        self.numbers.borrow_mut()[axis] = Some((spec.clone(), number.clone()));
        number
    }

    pub fn refresh(&self, w: &Workspace, state: &layer_ui::UiState) {
        let Some(view) = &state.layer_tools.canvas_size else {
            if self.shown.replace(false) {
                self.dialog.close();
            }
            return;
        };
        self.updating.set(true);
        self.dialog.set_heading(Some(view.title));
        self.relative.set_label(Some(view.relative_label));
        self.anchor_label.set_text(view.anchor_label);
        for axis in 0..2 {
            self.number(axis, view).set_value(view.values[axis]);
        }
        self.unit.set_selected(CanvasSizeUnit::ALL.iter().position(|u| *u == view.unit).unwrap_or(0) as u32);
        self.relative.set_active(view.relative);
        for (anchor, button, image) in &self.anchors {
            button.set_active(*anchor == view.anchor);
            image.set_visible(*anchor == view.anchor);
        }
        self.message.set_text(&view.message);
        self.dialog.set_response_enabled("apply", view.can_apply);
        self.updating.set(false);
        if !self.shown.replace(true) {
            self.dialog.present(Some(&w.window));
        }
    }
}
