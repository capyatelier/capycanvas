use gtk::{glib, prelude::*};
use std::{cell::{Cell, RefCell}, rc::Rc};
use layer_ui::{ContactPhase, CurveEditorAction, UiAction, UiState};
use crate::{number_control::NumberControl, workspace::Workspace};

pub(crate) struct CurveEditor {
    pub(crate) root: gtk::Box,
    pub(crate) area: gtk::DrawingArea,
    pub(crate) reset: gtk::Button,
    view: Rc<RefCell<layer_ui::CurveEditorView>>,
    presented: Cell<bool>,
    readouts: Option<CurveReadouts>,
    axes: [[gtk::Label; 3]; 2],
    histogram: Rc<RefCell<layer_ui::HistogramView>>,
    histogram_colors: Rc<Cell<[[u8;3];4]>>,
    footer: crate::histogram::Footer,
}
struct CurveReadouts {
    coordinates: [NumberControl; 2],
    labels: [gtk::Label; 2],
    ev: [gtk::Label; 2],
}

mod rotated_label {
    use super::*;
    use gtk::subclass::prelude::*;
    #[derive(Default)]
    pub struct RotatedLabel { pub child: RefCell<Option<gtk::Label>> }
    #[glib::object_subclass]
    impl ObjectSubclass for RotatedLabel {
        const NAME: &'static str = "CapyCurveAxisLabel";
        type Type = super::RotatedLabel;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for RotatedLabel {
        fn dispose(&self) { if let Some(child)=self.child.take() { child.unparent(); } }
    }
    impl WidgetImpl for RotatedLabel {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32,i32,i32,i32) {
            let child=self.child.borrow(); let child=child.as_ref().unwrap();
            let (minimum,natural,_,_)=child.measure(if orientation==gtk::Orientation::Horizontal { gtk::Orientation::Vertical } else { gtk::Orientation::Horizontal },for_size);
            (minimum,natural,-1,-1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let transform=gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(0.,height as f32)).rotate(-90.);
            self.child.borrow().as_ref().unwrap().allocate(height,width,-1,Some(transform));
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) { self.obj().snapshot_child(self.child.borrow().as_ref().unwrap(),snapshot); }
    }
}
glib::wrapper! { pub struct RotatedLabel(ObjectSubclass<rotated_label::RotatedLabel>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }
impl RotatedLabel {
    fn new(child: &gtk::Label) -> Self {
        use gtk::subclass::prelude::*;
        let widget:Self=glib::Object::new();child.set_parent(&widget);*widget.imp().child.borrow_mut()=Some(child.clone());widget
    }
}
impl CurveEditor {
    pub(crate) fn new(w: &Rc<Workspace>, target: layer_ui::CurveEditorTarget, name: &str, label: &str, control: &layer_ui::CurveEditorView) -> Self {
        let curve = &control.controls;
        let histogram=Rc::new(RefCell::new(layer_ui::HistogramView::default()));
        let histogram_colors=Rc::new(Cell::new([[0;3];4]));
        let area = gtk::DrawingArea::builder().content_width(64).content_height(200)
            .hexpand(true).focusable(true).build();
        area.set_widget_name(&format!("{name}-graph"));
        area.add_css_class("customizable-target");
        area.add_css_class("curve-key-scope");
        area.set_tooltip_text(Some(&curve.help));
        area.update_property(&[gtk::accessible::Property::Label(label)]);
        let view = Rc::new(RefCell::new(control.clone()));
        area.set_draw_func(glib::clone!(#[strong] view, #[strong] histogram, #[strong] histogram_colors, move |area, cr, width, height| {
            let view = view.borrow();
            let curve = &view.controls;
            let points = &view.points;
            let inset=f64::from(view.controls.inset); cr.translate(inset,inset);
            let (width, height) = (f64::from(width)-2.*inset, f64::from(height)-2.*inset);
            let c = area.color();
            let color = |alpha| cr.set_source_rgba(f64::from(c.red()), f64::from(c.green()), f64::from(c.blue()), alpha);
            color(0.12); cr.paint().ok();
            crate::histogram::draw(cr,&histogram.borrow(),histogram_colors.get(),width,height);
            color(0.2); cr.set_line_width(1.);
            for i in 1..4 {
                let t = f64::from(i) / 4.;
                cr.move_to(t * width, 0.); cr.line_to(t * width, height);
                cr.move_to(0., t * height); cr.line_to(width, t * height);
            }
            cr.stroke().ok();
            cr.set_dash(&[3., 3.], 0.);
            if let Some(white) = curve.axes[0].white {
                cr.move_to(f64::from(white) * width, 0.); cr.line_to(f64::from(white) * width, height);
            }
            if let Some(white) = curve.axes[1].white {
                cr.move_to(0., (1. - f64::from(white)) * height); cr.line_to(width, (1. - f64::from(white)) * height);
            }
            cr.stroke().ok(); cr.set_dash(&[], 0.);
            if view.controls.control_polygon {
                color(0.35); cr.set_line_width(1.);
                for (index, [x,y]) in points.iter().enumerate() {
                    let (x,y)=(f64::from(*x)*width,(1.-f64::from(*y))*height);
                    if index==0 {cr.move_to(x,y);} else {cr.line_to(x,y);}
                }
                cr.stroke().ok();
            }
            color(1.); cr.set_line_width(1.5);
            for (index, [x, y]) in view.plot.iter().enumerate() {
                let (x, y) = (f64::from(*x) * width, (1. - f64::from(*y)) * height);
                if index == 0 { cr.move_to(x, y); } else { cr.line_to(x, y); }
            }
            cr.stroke().ok();
            if let Some([x,y])=view.marker {
                color(0.8); cr.arc(f64::from(x)*width,(1.-f64::from(y))*height,5.,0.,std::f64::consts::TAU);cr.stroke().ok();
            }
            for (index, point) in points.iter().enumerate() {
                let (x, y) = (f64::from(point[0]) * width, (1. - f64::from(point[1])) * height);
                cr.arc(x, y, 3.5, 0., std::f64::consts::TAU); cr.fill().ok();
                if curve.selected == Some(index) {
                    cr.arc(x, y, 6., 0., std::f64::consts::TAU); cr.stroke().ok();
                }
            }
        }));

        let capture = Rc::new(Cell::new(None::<(u64, [f64; 2], [f64; 2], [f32; 2])>));
        let removing = Rc::new(Cell::new(false));
        let drag = gtk::GestureDrag::new(); drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(#[weak] w, #[weak] area, #[strong] view, #[strong] target, #[strong] capture, #[strong] removing, move |gesture, x, y| {
            if removing.get() { return; }
            let Some((sx, sy)) = gesture.current_event().and_then(|event| event.position()) else { return; };
            let epoch = view.borrow().controls.epoch;
            let inset=view.borrow().controls.inset;
            let extent = [area.width() as f32-2.*inset, area.height() as f32-2.*inset];
            area.grab_focus(); capture.set(Some((epoch, [x-f64::from(inset), y-f64::from(inset)], [sx, sy], extent)));
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Contact { epoch,
                phase: ContactPhase::Down, point: [x as f32-inset, y as f32-inset], extent } });
        }));
        for (phase, ending) in [(ContactPhase::Move, false), (ContactPhase::Up, true)] {
            let callback = glib::clone!(#[weak] w, #[strong] target, #[strong] capture, move |gesture: &gtk::GestureDrag, _: f64, _: f64| {
                let Some((epoch, [x, y], [sx, sy], extent)) = (if ending { capture.take() } else { capture.get() }) else { return; };
                let Some((px, py)) = gesture.current_event().and_then(|event| event.position()) else {
                    if ending { w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Contact { epoch,
                        phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } }); }
                    return;
                };
                w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Contact { epoch,
                    phase, point: [(x + (px - sx)) as f32, (y + (py - sy)) as f32], extent } });
            });
            if ending { drag.connect_drag_end(callback); } else { drag.connect_drag_update(callback); }
        }
        drag.connect_cancel(glib::clone!(#[weak] w, #[strong] target, #[strong] capture, move |_, _| {
            if let Some((epoch, ..)) = capture.take() {
                w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Contact { epoch,
                    phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } });
            }
        }));
        area.add_controller(drag.clone());
        let click = gtk::GestureClick::new(); click.set_button(0);
        let point_count = Rc::new(Cell::new(0));
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(#[weak] w, #[weak] area, #[strong] view, #[strong] target, #[strong] removing, #[strong] point_count, move |gesture, count, x, y| {
            if count == 1 { point_count.set(view.borrow().points.len()); }
            removing.set(gesture.current_button() == 3 || count == 2);
            if !removing.get() { return; }
            area.grab_focus();
            let epoch = view.borrow().controls.epoch; let inset=view.borrow().controls.inset;
            w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::RemoveAt { epoch,
                point: [x as f32-inset, y as f32-inset], extent: [area.width() as f32-2.*inset, area.height() as f32-2.*inset],
                point_count: (gesture.current_button() != 3).then(|| point_count.get()) } });
        }));
        area.add_controller(click.clone()); click.group_with(&drag);
        let keys = gtk::EventControllerKey::new();
        let dispatch_key = Rc::new(glib::clone!(#[weak] w, #[strong] view, #[strong] target, #[upgrade_or] false, move |native, pressed, modifiers| {
            let layer_ui::UiInput::Key { key: key_event, repeat, modifiers, .. } = crate::input::key_input(native, pressed, modifiers, false, None) else { return false; };
            if !matches!(key_event.as_str(), "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Delete" | "Backspace" | "Escape")
                || pressed && key_event != "Escape" && (modifiers.command || modifiers.alt) { return false; }
            let epoch = view.borrow().controls.epoch;
            w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Key { epoch, key_event, pressed, repeat, modifiers } });
            true
        }));
        keys.connect_key_pressed(glib::clone!(#[strong] dispatch_key, move |_, key, _, modifiers| {
            if dispatch_key(key, true, modifiers) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }));
        keys.connect_key_released(move |_, key, _, modifiers| { dispatch_key(key, false, modifiers); });
        area.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(#[weak] w, #[strong] view, #[strong] target, #[strong] capture, move |_| {
            let epoch = capture.take().map_or_else(|| view.borrow().controls.epoch, |(epoch, ..)| epoch);
            w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Contact { epoch,
                phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } });
        }));
        area.add_controller(focus);
        let reset = crate::icons::button("layer-reset-symbolic");
        reset.add_css_class("flat"); reset.add_css_class("circular");
        reset.set_halign(gtk::Align::End); reset.set_valign(gtk::Align::End);
        reset.set_widget_name("curve-reset");
        reset.set_tooltip_text(Some(&curve.reset_label));
        reset.connect_clicked(glib::clone!(#[weak] w, #[strong] target, move |_| {
            w.dispatch(UiAction::CurveEditor { target: target.clone(), action: CurveEditorAction::Reset });
        }));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_vexpand(false); root.set_valign(gtk::Align::Start);
        let graph = gtk::Grid::builder().column_spacing(6).row_spacing(4).build();
        let vertical = gtk::CenterBox::new(); vertical.set_orientation(gtk::Orientation::Vertical);
        let maximum = gtk::Label::new(Some(&curve.axes[1].maximum));
        let title = gtk::Label::new(None); title.set_widget_name(&format!("{name}-output-axis"));
        let minimum = gtk::Label::new(Some(&curve.axes[1].minimum));
        vertical.add_css_class("dim-label"); vertical.set_start_widget(Some(&maximum)); vertical.set_end_widget(Some(&minimum));
        if control.controls.coordinate_readouts { vertical.set_center_widget(Some(&title)); }
        else { vertical.set_center_widget(Some(&RotatedLabel::new(&title))); }
        let y_axis = [minimum.clone(), title.clone(), maximum.clone()];
        graph.attach(&vertical, 0, 0, 1, 1); graph.attach(&area, 1, 0, 1, 1);
        let horizontal = gtk::CenterBox::new();
        let minimum = gtk::Label::new(Some(&curve.axes[0].minimum));
        let title = gtk::Label::new(None); title.set_widget_name(&format!("{name}-input-axis"));
        let maximum = gtk::Label::new(Some(&curve.axes[0].maximum));
        horizontal.add_css_class("dim-label"); horizontal.set_start_widget(Some(&minimum)); horizontal.set_center_widget(Some(&title)); horizontal.set_end_widget(Some(&maximum));
        let axes = [[minimum.clone(), title.clone(), maximum.clone()], y_axis];
        graph.attach(&horizontal, 1, 1, 1, 1); root.append(&graph);
        let readouts = control.controls.coordinate_readouts.then(|| {
            let ev = std::array::from_fn(|_| { let label = gtk::Label::new(None); label.set_xalign(1.); label.add_css_class("dim-label"); label });
            let labels = std::array::from_fn(|index| gtk::Label::builder().label(&curve.axes[index].label).xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build());
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);row.set_homogeneous(true);
            let coordinates = std::array::from_fn(|index| {
                let input = NumberControl::value_only(curve.numeric.clone(), &curve.axes[index].label, w.localization().clone());
                input.set_widget_name(&format!("{name}-{}", if index == 0 { "input" } else { "output" }));
                input.bind_edit(w, glib::clone!(#[strong] view, move |value| {
                    CurveEditorAction::Number { epoch: view.borrow().controls.epoch,
                        axis: if index == 0 { layer_ui::CurveAxis::Input } else { layer_ui::CurveAxis::Output },
                        operation: layer_ui::NumericOperation::Value { value } }
                }),|phase,action|CurveEditorAction::Gesture {phase,action:Box::new(action)}, glib::clone!(#[strong] target, move |action|UiAction::CurveEditor {target:target.clone(),action}));
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                input.set_halign(gtk::Align::Fill);input.set_hexpand(true);
                column.set_hexpand(true);column.append(&labels[index]);column.append(&input);column.append(&ev[index]);
                row.append(&column);input
            });
            root.append(&row); CurveReadouts { coordinates, labels, ev }
        });
        let footer = crate::histogram::Footer::new("curve");footer.root.append(&reset);root.append(&footer.root);
        let editor = Self { root, area, reset, view, presented: Cell::new(false), readouts, axes, histogram, histogram_colors, footer };
        if matches!(target, layer_ui::CurveEditorTarget::Pressure) { editor.footer.root.set_visible(false); }
        editor.update(control, &w.localization());editor
    }
    pub(crate) fn refresh_histogram(&self,w:&Rc<Workspace>,state:&UiState) {
        *self.histogram.borrow_mut()=state.tonal_histogram.clone();
        self.histogram_colors.set(state.palette.histogram_colors().map(|color|color.0));
        self.footer.refresh(w,state,&state.tonal_histogram.status);self.area.queue_draw();
    }
    pub(crate) fn update(&self, control: &layer_ui::CurveEditorView, localization: &std::sync::Arc<layer_ui::Localizer>) {
        let presented = self.presented.replace(true);
        {
            let mut view = self.view.borrow_mut();
            if presented && view.controls == control.controls && view.points == control.points && view.plot == control.plot
                && view.modified == control.modified {
                if view.marker != control.marker { view.marker = control.marker; self.area.queue_draw(); }
                return;
            }
            *view = control.clone();
        }
        let curve = &control.controls;
        self.area.set_tooltip_text(Some(&curve.help));
        self.reset.set_tooltip_text(Some(&curve.reset_label));
        for (labels, axis) in self.axes.iter().zip(&curve.axes) {
            let title=if control.controls.coordinate_readouts { "" } else { axis.label.as_str() };
            for (label, text) in labels.iter().zip([axis.minimum.as_str(), title, axis.maximum.as_str()]) { label.set_label(text); }
        }
        if let Some(readouts)=&self.readouts { for (index, coordinate) in [&curve.input, &curve.output].into_iter().enumerate() {
            let input = &readouts.coordinates[index];
            readouts.labels[index].set_label(&curve.axes[index].label);
            readouts.labels[index].set_tooltip_text(Some(&curve.axes[index].label));
            input.set_caption(&curve.axes[index].label, "", localization.clone());
            input.set_sensitive(coordinate.as_ref().is_some_and(|value| !value.read_only));
            let (value, text) = coordinate.as_ref().map_or((0., ""), |value| (value.value, value.text.as_str()));
            input.set_presented_value(value, text);
            readouts.ev[index].set_label(coordinate.as_ref().and_then(|value| value.ev.as_deref()).unwrap_or(""));
            readouts.ev[index].set_visible(matches!(curve.domain, layer_ui::CurveDomain::LogHdr { .. }));
        } }
        self.reset.set_visible(control.modified); self.area.queue_draw();
    }
}
