//! Native contact tracking and presentation for shared layer actions.
use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};
use std::cell::{Cell, OnceCell, RefCell};

const REVEAL: f64 = layer_ui::PANEL_CONTROL_METRICS.layer_swipe_action_width as f64;
const THRESHOLD: f64 = layer_ui::PANEL_CONTROL_METRICS.layer_swipe_threshold as f64;
type Changed = Box<dyn Fn(&SwipeRow)>;
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct SwipeRow {
        pub content: OnceCell<gtk::Widget>,
        pub delete: OnceCell<gtk::Button>,
        pub lock: OnceCell<gtk::Button>,
        pub duplicate: OnceCell<gtk::Button>,
        pub actions: OnceCell<gtk::Box>,
        pub offset: Cell<f64>,
        pub origin: Cell<f64>,
        pub admitted: Cell<bool>,
        pub can_swipe_right: Cell<bool>,
        pub animation: RefCell<Option<adw::TimedAnimation>>,
        pub changed: OnceCell<Changed>,
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
            let reveal = self.obj().reveal_width() as i32;
            self.actions.get().unwrap().allocate(reveal, height, baseline, shift((width - reveal) as f32));
            self.content.get().unwrap().allocate(width, height, baseline, shift(-self.offset.get() as f32));
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let reveal = self.offset.get().max(0.) as f32;
            snapshot.push_clip(&gtk::graphene::Rect::new(obj.width() as f32 - reveal,
                0., reveal, obj.height() as f32));
            if reveal > 0. { obj.snapshot_child(self.actions.get().unwrap(), snapshot); }
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
    pub fn new(content: &impl IsA<gtk::Widget>, remove: impl Fn() + 'static, lock: impl Fn() + 'static, duplicate: impl Fn() + 'static, swipe_right: impl Fn() + 'static, opening: impl Fn(&Self) + 'static, changed: impl Fn(&Self) + 'static) -> Self {
        let row: Self = glib::Object::new();
        let _ = row.imp().changed.set(Box::new(changed));
        row.set_overflow(gtk::Overflow::Hidden);
        let copy = layer_ui::CommonCopy::new(crate::launch_localization());
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions.add_css_class("layer-swipe-actions");
        let button = |name: &str, text: &str| {
            let button = gtk::Button::new();
            button.set_hexpand(true);
            button.set_child(Some(&gtk::Label::builder().label(text).ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).build()));
            button.set_widget_name(name); button.add_css_class(name);
            actions.append(&button); button
        };
        let lock_button = button("layer-swipe-lock", "");
        let duplicate_button = button("layer-swipe-duplicate", "");
        let delete_button = button("layer-swipe-delete", &copy.delete);
        delete_button.add_css_class("destructive-action");
        for (button, action) in [(&lock_button, Box::new(lock) as Box<dyn Fn()>), (&duplicate_button, Box::new(duplicate)), (&delete_button, Box::new(remove))] {
            button.connect_clicked(glib::clone!(#[weak] row, move |_| { row.reveal(false); action(); }));
        }
        row.imp().lock.set(lock_button).unwrap();
        row.imp().duplicate.set(duplicate_button).unwrap();
        row.imp().delete.set(delete_button).unwrap();
        actions.set_parent(&row); actions.set_child_visible(false);
        row.imp().actions.set(actions).unwrap();
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
                direct |= w.has_css_class("drag-immediate") || w.is::<gtk::Editable>();
                picked = w.parent();
            }
            if !crate::input::touch_or_pen(g) || direct {
                g.set_state(gtk::EventSequenceState::Denied); return;
            }
            if let Some(a) = row.imp().animation.take() { a.pause(); }
            row.imp().origin.set(row.imp().offset.get().max(0.));
        }));
        drag.connect_drag_update(glib::clone!(#[weak] row, move |g, dx, dy| {
            if !row.imp().admitted.get() {
                if !row.drag_check_threshold(0, 0, dx as i32, dy as i32) { return; }
                let allowed = if dx < 0. { [row.lock_button(), row.duplicate_button(), row.delete_button()].iter().any(|button| button.is_visible() && button.is_sensitive()) }
                    else { row.imp().origin.get() > 0. || row.imp().can_swipe_right.get() };
                if dy.abs() >= dx.abs() || !allowed {
                    g.set_state(gtk::EventSequenceState::Denied); return;
                }
                row.imp().admitted.set(true);
                g.set_state(gtk::EventSequenceState::Claimed);
                opening(&row);
            }
            let minimum = if row.imp().origin.get() == 0. && row.imp().can_swipe_right.get() { -REVEAL } else { 0. };
            row.position((row.imp().origin.get() - dx).clamp(minimum, row.reveal_width()));
        }));
        drag.connect_drag_end(glib::clone!(#[weak] row, move |g, _, _| {
            if row.imp().admitted.replace(false) {
                let released = g.current_event().is_some_and(|e| matches!(e.event_type(), gtk::gdk::EventType::ButtonRelease | gtk::gdk::EventType::TouchEnd));
                let toggle = released && row.imp().offset.get() <= -REVEAL * THRESHOLD;
                let threshold = if row.imp().origin.get() > 0. { row.imp().origin.get() - REVEAL * THRESHOLD } else { REVEAL * THRESHOLD };
                row.reveal(released && row.imp().offset.get() >= threshold);
                if toggle { swipe_right(); }
            }
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
        self.imp().actions.get().unwrap().set_child_visible(offset > 0.);
        self.queue_allocate();
        if let Some(changed) = self.imp().changed.get() { changed(self); }
    }
    pub fn reset(&self) {
        if let Some(a) = self.imp().animation.take() { a.pause(); }
        self.position(0.);
    }
    pub fn set_actions(&self, state: &layer_ui::LayerState) {
        for (id, button) in [("lock", self.lock_button()), ("duplicate", self.duplicate_button()), ("delete", self.delete_button())] {
            let model = state.swipe_actions.iter().find(|action| action.id == id);
            button.set_visible(model.is_some());
            let Some(model) = model else { continue; };
            let label = &model.label;
            let child = button.child().and_downcast::<gtk::Label>().unwrap();
            if child.text().as_str() != label.as_ref() { child.set_label(label); }
            if button.tooltip_text().as_deref() != Some(label.as_ref()) {
                button.set_tooltip_text(Some(label));
                button.update_property(&[gtk::accessible::Property::Label(label)]);
            }
            button.set_sensitive(model.enabled);
        }
        let mut previous: Option<&gtk::Button> = None;
        for model in &state.swipe_actions {
            let button = match model.id {"lock" => self.lock_button(), "duplicate" => self.duplicate_button(), _ => self.delete_button()};
            self.imp().actions.get().unwrap().reorder_child_after(button, previous);
            previous = Some(button);
        }
        self.imp().can_swipe_right.set(state.right_swipe.is_some());
        if (!state.swipe_actions.iter().any(|action| action.enabled) && self.is_open()) || (state.right_swipe.is_none() && self.imp().offset.get() < 0.) { self.reset(); }
    }
    pub fn reveal(&self, open: bool) {
        if let Some(a) = self.imp().animation.take() { a.pause(); }
        let target = adw::CallbackAnimationTarget::new(glib::clone!(#[weak(rename_to = row)] self, move |value| row.position(value)));
        let animation = adw::TimedAnimation::new(self, self.imp().offset.get(), if open { self.reveal_width() } else { 0. }, 150, target);
        animation.play();
        *self.imp().animation.borrow_mut() = Some(animation);
    }
    pub fn is_open(&self) -> bool { self.imp().offset.get() > 0. }
    pub fn is_at_rest(&self) -> bool { self.imp().offset.get() == 0. }
    pub fn swipe_progress(&self) -> f64 {
        let offset = self.imp().offset.get();
        (offset.abs() / if offset > 0. { self.reveal_width() } else { REVEAL }).clamp(0., 1.)
    }
    pub fn delete_button(&self) -> &gtk::Button { self.imp().delete.get().unwrap() }
    pub fn lock_button(&self) -> &gtk::Button { self.imp().lock.get().unwrap() }
    pub fn duplicate_button(&self) -> &gtk::Button { self.imp().duplicate.get().unwrap() }
    pub fn is_action(&self, widget: &gtk::Widget) -> bool {
        let actions = self.imp().actions.get().unwrap();
        widget == actions.upcast_ref::<gtk::Widget>() || widget.is_ancestor(actions)
    }
    fn reveal_width(&self) -> f64 {
        (REVEAL * [self.lock_button(), self.duplicate_button(), self.delete_button()].iter().filter(|button| button.is_visible()).count() as f64).min(self.width().max(1) as f64)
    }
}
