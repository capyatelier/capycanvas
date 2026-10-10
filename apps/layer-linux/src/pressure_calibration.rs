use gtk::{glib, prelude::*};
use layer_ui::{Bounds, ContactPhase, CurveEditorAction, CurveEditorTarget, PressureCalibrationAction, PressureCalibrationView, UiAction};
use std::{cell::{Cell, RefCell}, rc::Rc};
use crate::{curve_editor::CurveEditor, workspace::{PanelColumns, Workspace}};

pub(crate) struct PressurePanel {
    pub root: PanelColumns,
    column: gtk::Box,
    title: gtk::Label,
    close: gtk::Button,
    body: gtk::Box,
    firmer: gtk::Button,
    lighter: gtk::Button,
    reset: gtk::Button,
    cancel: gtk::Button,
    apply: gtk::Button,
    editor: RefCell<Option<CurveEditor>>,
    bounds: Cell<Option<Bounds>>,
    pending_bounds: Cell<Option<Bounds>>,
    presented: RefCell<Option<PressureCalibrationView>>,
    measured: Cell<Option<([f32; 2], [f32; 2])>>,
    bound: Cell<bool>,
}

impl PressurePanel {
    pub fn new() -> Rc<Self> {
        let column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).accessible_role(gtk::AccessibleRole::Dialog).build();
        let root = PanelColumns::new(&column);
        root.add_css_class("floating-panel"); root.add_css_class("utility-panel"); root.set_widget_name("pen-pressure-panel");
        root.set_visible(false);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("dock-tabs"); header.add_css_class("utility-header"); header.set_height_request(layer_ui::TAB_BAR_HEIGHT as i32);
        let title = gtk::Label::builder().hexpand(true).xalign(0.).margin_start(12).ellipsize(gtk::pango::EllipsizeMode::End).build();
        title.set_widget_name("pen-pressure-title"); title.set_cursor_from_name(Some("grab"));
        title.add_css_class("utility-title");
        let close = crate::icons::button("layer-close-symbolic"); close.add_css_class("flat"); close.set_widget_name("pen-pressure-close");
        close.add_css_class("utility-close"); close.set_valign(gtk::Align::Center);
        close.set_margin_start(4); close.set_margin_end(6); close.set_margin_top(4); close.set_margin_bottom(4);
        header.append(&title); header.append(&close); column.append(&header);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body.set_margin_start(12); body.set_margin_end(12); body.set_margin_top(12); body.set_margin_bottom(12);
        let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true).vexpand(true).child(&body).build();
        column.append(&scroller);
        let sensitivity = gtk::Box::new(gtk::Orientation::Horizontal, 6); sensitivity.set_homogeneous(true);
        let firmer = gtk::Button::new(); firmer.set_widget_name("pen-pressure-firmer");
        let lighter = gtk::Button::new(); lighter.set_widget_name("pen-pressure-lighter");
        sensitivity.append(&firmer); sensitivity.append(&lighter); body.append(&sensitivity);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        footer.set_margin_start(12); footer.set_margin_end(12); footer.set_margin_bottom(12);
        let reset = gtk::Button::new(); reset.set_widget_name("pen-pressure-reset"); reset.add_css_class("flat");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0); spacer.set_hexpand(true);
        let cancel = gtk::Button::new(); cancel.set_widget_name("pen-pressure-cancel");
        let apply = gtk::Button::new(); apply.set_widget_name("pen-pressure-apply"); apply.add_css_class("suggested-action");
        footer.append(&reset); footer.append(&spacer); footer.append(&cancel); footer.append(&apply); column.append(&footer);
        Rc::new(Self { root, column, title, close, body, firmer, lighter, reset, cancel, apply,
            editor: RefCell::new(None), bounds: Cell::new(None), pending_bounds: Cell::new(None), presented: RefCell::new(None), measured: Cell::new(None), bound: Cell::new(false) })
    }
    pub fn bounds(&self) -> Option<Bounds> { self.bounds.get() }
    fn bind(&self, w: &Rc<Workspace>) {
        if self.bound.replace(true) { return; }
        for (button, action) in [(&self.close, PressureCalibrationAction::Cancel), (&self.cancel, PressureCalibrationAction::Cancel),
            (&self.apply, PressureCalibrationAction::Apply), (&self.firmer, PressureCalibrationAction::Sensitivity { lighter: false }),
            (&self.lighter, PressureCalibrationAction::Sensitivity { lighter: true })] {
            button.connect_clicked(glib::clone!(#[weak] w, move |_| { w.dispatch(UiAction::PressureCalibration { action: action.clone() }); }));
        }
        self.reset.connect_clicked(glib::clone!(#[weak] w, move |_| {
            w.dispatch(UiAction::CurveEditor { target: CurveEditorTarget::Pressure, action: CurveEditorAction::Reset });
        }));
        let drag = gtk::GestureDrag::new(); drag.set_button(1);
        let capture = Rc::new(Cell::new(false));
        drag.connect_drag_begin(glib::clone!(#[weak] w, #[strong] capture, move |gesture, _, _| {
            let Some((x,y)) = gesture.current_event().and_then(|event| event.position()) else { return; };
            capture.set(true); gesture.set_state(gtk::EventSequenceState::Claimed);
            w.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Drag { phase: ContactPhase::Down,
                position: [x as f32,y as f32], viewport: [w.surface.width() as f32,w.surface.height() as f32] } });
        }));
        for (phase, ending) in [(ContactPhase::Move,false),(ContactPhase::Up,true)] {
            let callback = glib::clone!(#[weak] w, #[strong] capture, move |gesture: &gtk::GestureDrag, _: f64, _: f64| {
                if !capture.get() { return; }
                if ending { capture.set(false); }
                let position = gesture.current_event().and_then(|event|event.position());
                let phase = if position.is_some() { phase } else { ContactPhase::Cancel };
                let (x,y) = position.unwrap_or_default();
                w.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Drag { phase, position: [x as f32,y as f32],
                    viewport: [w.surface.width() as f32,w.surface.height() as f32] } });
                if ending { w.pressure_calibration.present_motion(&w); }
            });
            if ending { drag.connect_drag_end(callback); } else { drag.connect_drag_update(callback); }
        }
        drag.connect_cancel(glib::clone!(#[weak] w, move |_, _| {
            if capture.replace(false) { w.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Drag {
                phase: ContactPhase::Cancel, position: [0.;2], viewport: [w.surface.width() as f32,w.surface.height() as f32] } }); }
            w.pressure_calibration.present_motion(&w);
        }));
        self.title.add_controller(drag);
    }
    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, view: Option<&PressureCalibrationView>) {
        self.bind(w);
        let Some(view) = view else {
            self.bounds.set(None); self.pending_bounds.set(None); self.presented.borrow_mut().take(); self.root.set_visible(false); self.measured.set(None); return;
        };
        let moved = self.bounds.replace(Some(view.bounds)) != Some(view.bounds);
        if moved { self.pending_bounds.set(Some(view.bounds)); w.queue_workspace_frame(); w.surface.queue_draw(); }
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        {
            let mut presented = self.presented.borrow_mut();
            if let Some(previous) = &mut *presented { previous.bounds = view.bounds; }
            if presented.as_ref() == Some(view) && self.measured.get().is_some_and(|(_, previous)| previous == viewport) { return; }
            *presented = Some(view.clone());
        }
        self.root.set_visible(true);
        self.title.set_label(&view.title); self.column.update_property(&[gtk::accessible::Property::Label(&view.title),gtk::accessible::Property::Modal(false)]);
        self.close.set_tooltip_text(Some(&view.close)); self.close.update_property(&[gtk::accessible::Property::Label(&view.close)]);
        set_button_content(&self.reset, "layer-reset-symbolic", &view.reset);
        set_button_content(&self.firmer, "layer-minus-symbolic", &view.firmer);
        set_button_content(&self.lighter, "layer-plus-symbolic", &view.lighter);
        self.firmer.set_sensitive(view.firmer_enabled); self.lighter.set_sensitive(view.lighter_enabled);
        self.cancel.set_label(&view.cancel); self.apply.set_label(&view.apply);
        if self.editor.borrow().is_none() {
            let editor = CurveEditor::new(w, CurveEditorTarget::Pressure, "pen-pressure", &view.title, &view.editor);
            self.body.prepend(&editor.root); *self.editor.borrow_mut() = Some(editor);
        }
        self.editor.borrow().as_ref().unwrap().update(&view.editor, &w.localization());
        let width = 336f32.max(self.column.measure(gtk::Orientation::Horizontal,-1).0 as f32).min(viewport[0]);
        let extent = [width,360f32.max(self.column.measure(gtk::Orientation::Vertical,width as i32).1 as f32).min(viewport[1])];
        if self.measured.get() != Some((extent, viewport)) {
            self.measured.set(Some((extent, viewport)));
            glib::idle_add_local_once(glib::clone!(#[weak] w, move || {
                w.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Measure { extent, viewport } });
            }));
        }
    }
    pub(super) fn present_motion(&self, w: &Workspace) {
        if let Some(bounds) = self.pending_bounds.take() {
            crate::workspace::allocate_at(self.root.upcast_ref(), bounds); w.surface.queue_draw();
        }
    }
}

fn set_button_content(button: &gtk::Button, icon: &str, label: &str) {
    if button.child().and_then(|w|w.downcast::<gtk::Box>().ok()).and_then(|b|b.last_child())
        .and_then(|w|w.downcast::<gtk::Label>().ok()).is_some_and(|l|l.label()==label) { return; }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6); content.set_halign(gtk::Align::Center);
    content.append(&gtk::Image::from_icon_name(icon)); content.append(&gtk::Label::new(Some(label))); button.set_child(Some(&content));
}
