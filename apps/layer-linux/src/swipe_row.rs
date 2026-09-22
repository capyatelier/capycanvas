//! Native contact tracking and presentation; deletion stays a shared action.
use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};
use std::cell::{Cell, OnceCell, RefCell};

const REVEAL: f64 = 72.;
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct SwipeRow {
        pub content: OnceCell<gtk::Widget>,
        pub delete: OnceCell<gtk::Button>,
        pub offset: Cell<f64>,
        pub origin: Cell<f64>,
        pub admitted: Cell<bool>,
        pub animation: RefCell<Option<adw::TimedAnimation>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for SwipeRow {
        const NAME: &'static str = "CapySwipeRow";
        type Type = super::SwipeRow;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for SwipeRow {
        fn dispose(&self) {
            if let Some(a) = self.animation.take() { a.pause(); }
            while let Some(child) = self.obj().first_child() { child.unparent(); }
        }
    }
    impl WidgetImpl for SwipeRow {
        fn measure(&self, orientation: gtk::Orientation, size: i32) -> (i32, i32, i32, i32) {
            self.content.get().map_or((0, 0, -1, -1), |c| c.measure(orientation, size))
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let shift = |x| Some(gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x, 0.)));
            self.delete.get().unwrap().allocate(REVEAL as i32, height, baseline, shift(width as f32 - REVEAL as f32));
            self.content.get().unwrap().allocate(width, height, baseline, shift(-self.offset.get() as f32));
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            // Only the revealed area paints red; transparent row backgrounds
            // must not expose the action under an unopened row.
            snapshot.push_clip(&gtk::graphene::Rect::new(obj.width() as f32 - self.offset.get() as f32,
                0., self.offset.get() as f32, obj.height() as f32));
            obj.snapshot_child(self.delete.get().unwrap(), snapshot);
            snapshot.pop();
            obj.snapshot_child(self.content.get().unwrap(), snapshot);
        }
    }
}
glib::wrapper! {
    pub struct SwipeRow(ObjectSubclass<imp::SwipeRow>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl SwipeRow {
    pub fn new(content: &impl IsA<gtk::Widget>, remove: impl Fn() + 'static, opening: impl Fn(&Self) + 'static) -> Self {
        let row: Self = glib::Object::new();
        row.set_overflow(gtk::Overflow::Hidden);
        let delete = gtk::Button::with_label("Delete");
        delete.add_css_class("destructive-action");
        delete.add_css_class("layer-swipe-delete");
        delete.set_widget_name("layer-swipe-delete");
        delete.set_parent(&row);
        delete.connect_clicked(move |_| remove());
        row.imp().delete.set(delete).unwrap();
        content.as_ref().set_parent(&row);
        row.imp().content.set(content.as_ref().clone()).unwrap();
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(#[weak] row, move |g, x, y| {
            row.imp().admitted.set(false);
            let mut picked = row.pick(x, y, gtk::PickFlags::DEFAULT);
            let mut direct = false;
            while let Some(w) = picked {
                if w == row { break; }
                direct |= w.has_css_class("drag-immediate") || w.is::<gtk::Editable>()
                    || w == *row.imp().delete.get().unwrap();
                picked = w.parent();
            }
            if !crate::input::touch_or_pen(g) || direct || !row.imp().delete.get().unwrap().is_sensitive() {
                g.set_state(gtk::EventSequenceState::Denied); return;
            }
            if let Some(a) = row.imp().animation.take() { a.pause(); }
            row.imp().origin.set(row.imp().offset.get());
        }));
        drag.connect_drag_update(glib::clone!(#[weak] row, move |g, dx, dy| {
            if !row.imp().admitted.get() {
                if !row.drag_check_threshold(0, 0, dx as i32, dy as i32) { return; }
                if dy.abs() >= dx.abs() || (dx > 0. && row.imp().origin.get() == 0.) {
                    g.set_state(gtk::EventSequenceState::Denied); return;
                }
                row.imp().admitted.set(true);
                g.set_state(gtk::EventSequenceState::Claimed);
                opening(&row);
            }
            row.position((row.imp().origin.get() - dx).clamp(0., REVEAL));
        }));
        drag.connect_drag_end(glib::clone!(#[weak] row, move |_, _, _| {
            if row.imp().admitted.replace(false) { row.reveal(row.imp().offset.get() >= REVEAL * 0.4); }
        }));
        drag.connect_cancel(glib::clone!(#[weak] row, move |_, _| {
            row.imp().admitted.set(false);
            row.reveal(false);
        }));
        row.add_controller(drag);
        row.connect_unmap(|row| row.reset());
        row
    }
    fn position(&self, offset: f64) {
        self.imp().offset.set(offset);
        self.imp().delete.get().unwrap().set_can_target(offset > 0.);
        self.queue_allocate();
    }
    pub fn reset(&self) {
        if let Some(a) = self.imp().animation.take() { a.pause(); }
        self.position(0.);
    }
    pub fn set_can_delete(&self, enabled: bool) {
        self.imp().delete.get().unwrap().set_sensitive(enabled);
        if !enabled { self.reset(); }
    }
    pub fn reveal(&self, open: bool) {
        if let Some(a) = self.imp().animation.take() { a.pause(); }
        let target = adw::CallbackAnimationTarget::new(glib::clone!(#[weak(rename_to = row)] self, move |value| row.position(value)));
        let animation = adw::TimedAnimation::new(self, self.imp().offset.get(), if open { REVEAL } else { 0. }, 150, target);
        animation.play();
        *self.imp().animation.borrow_mut() = Some(animation);
    }
    pub fn is_open(&self) -> bool { self.imp().offset.get() > 0. }
    pub fn delete_button(&self) -> &gtk::Button { self.imp().delete.get().unwrap() }
}
