use super::*;
use crate::input::reset_controllers;

#[derive(Default)]
pub(super) struct State {
    pub progress: Cell<f64>,
    pages: RefCell<Option<(gtk::Widget, gtk::Widget)>>,
    animation: RefCell<Option<adw::TimedAnimation>>,
    rtl: Cell<bool>,
    drag: glib::WeakRef<gtk::GestureDrag>,
}

pub(super) fn cancel_pressed(widget: &gtk::Widget) {
    if let Some(stack) = widget.downcast_ref::<gtk::Stack>() {
        if let Some(page) = stack.visible_child() { cancel_pressed(&page); }
        return;
    }
    if widget.is::<gtk::Button>() {
        let flags = gtk::StateFlags::ACTIVE | gtk::StateFlags::PRELIGHT;
        if widget.state_flags().intersects(flags) { reset_controllers(widget); widget.unset_state_flags(flags); }
        return;
    }
    let mut child = widget.first_child();
    while let Some(widget) = child { child = widget.next_sibling(); cancel_pressed(&widget); }
}

fn reset(contents: &MenuContents) {
    let state = &contents.imp().swipe;
    if let Some((_, parent)) = state.pages.take() { parent.set_child_visible(false); }
    if let Some(stack) = contents.first_child() { stack.set_can_target(true); }
    state.progress.set(0.);
    contents.queue_allocate();
}

pub(super) fn cancel(contents: &MenuContents, contact: &RefCell<PhoneMenuContact>) {
    if let Some(animation) = contents.imp().swipe.animation.take() { animation.pause(); }
    if let Some(drag) = contents.imp().swipe.drag.upgrade() { drag.reset(); }
    contact.borrow_mut().cancel_press(); contact.borrow_mut().begin_swipe(); reset(contents);
}

pub(super) fn allocate(contents: &native::Contents, width: i32, height: i32, baseline: i32) {
    if let Some((_, parent)) = contents.swipe.pages.borrow().as_ref() {
        parent.set_child_visible(true);
        parent.allocate(width, height, baseline, None);
    }
}

pub(super) fn snapshot(contents: &native::Contents, snapshot: &gtk::Snapshot) {
    let obj = contents.obj();
    let Some(stack) = obj.first_child() else { return; };
    let pages = contents.swipe.pages.borrow();
    let Some((current, parent)) = pages.as_ref() else { obj.snapshot_child(&stack, snapshot); return; };
    snapshot.push_clip(&gtk::graphene::Rect::new(0., 0., obj.width() as f32, obj.height() as f32));
    let distance = obj.width() as f32 * contents.swipe.progress.get() as f32;
    let rtl = contents.swipe.rtl.get();
    let exposed = gtk::graphene::Rect::new(if rtl { obj.width() as f32 - distance } else { 0. }, 0., distance, obj.height() as f32);
    snapshot.push_clip(&exposed);
    snapshot.save();
    let offset = obj.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().map_or(0., |scroll| scroll.vadjustment().value());
    snapshot.translate(&gtk::graphene::Point::new(0., offset as f32));
    stack.snapshot_child(parent, snapshot);
    snapshot.restore();
    snapshot.pop();
    snapshot.save();
    snapshot.translate(&gtk::graphene::Point::new(if rtl { -distance } else { distance }, 0.));
    stack.snapshot_child(current, snapshot);
    snapshot.restore();
    snapshot.pop();
}

pub(super) fn bind(w: &Rc<Workspace>, contents: &MenuContents, stack: &gtk::Stack, contact: Rc<RefCell<PhoneMenuContact>>) {
    let drag = gtk::GestureDrag::new();
    contents.imp().swipe.drag.set(Some(&drag));
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    drag.connect_drag_begin(glib::clone!(#[weak] w, #[weak] contents, #[weak] stack, #[strong] contact, move |gesture, _, _| {
        if stack.is_transition_running() || contents.imp().swipe.pages.borrow().is_some() {
            gesture.set_state(gtk::EventSequenceState::Denied); return;
        }
        let phone = w.gpu.borrow().as_ref().unwrap().session.state().phone.clone();
        contact.borrow_mut().begin_swipe_at(&phone.menu_navigation.current(), gesture.current_event().map_or(0, |event| event.time()), !phone.menu_navigation.is_root());
        contents.imp().swipe.rtl.set(contents.direction() == gtk::TextDirection::Rtl);
    }));
    drag.connect_drag_update(glib::clone!(#[weak] w, #[weak] contents, #[weak] stack, #[strong] contact, move |gesture, dx, dy| {
        let rtl = contents.imp().swipe.rtl.get();
        let past_slop = contents.drag_check_threshold(0, 0, dx as i32, dy as i32);
        let result = contact.borrow_mut().sample_swipe([dx as f32, dy as f32], contents.width() as f32, past_slop, rtl, gesture.current_event().map_or(0, |event| event.time()));
        match result {
            PhoneMenuSwipe::Pending => (),
            PhoneMenuSwipe::Scroll => { cancel_pressed(stack.upcast_ref()); gesture.set_state(gtk::EventSequenceState::Denied); }
            PhoneMenuSwipe::Back(progress) => {
                if contents.imp().swipe.pages.borrow().is_none() { cancel_pressed(stack.upcast_ref()); }
                if contents.imp().swipe.pages.borrow().is_none() {
                    let current = stack.visible_child().unwrap();
                    let name = stack.visible_child_name().unwrap();
                    let menu = route(&w).name();
                    if menu != name { gesture.set_state(gtk::EventSequenceState::Denied); return; }
                    let parent = w.gpu.borrow().as_ref().unwrap().session.state().phone.menu_navigation.parent().unwrap();
                    let parent = stack.child_by_name(&parent.name()).unwrap();
                    *contents.imp().swipe.pages.borrow_mut() = Some((current, parent));
                    stack.set_can_target(false);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    contents.queue_allocate();
                }
                contents.imp().swipe.progress.set(progress as f64);
                contents.queue_draw();
            }
        }
    }));
    drag.connect_drag_end(glib::clone!(#[weak] w, #[weak] contents, #[weak] stack, #[strong] contact, move |gesture, _, _| {
        if contents.imp().swipe.pages.borrow().is_none() { contact.borrow_mut().begin_swipe(); return; }
        let rtl = contents.imp().swipe.rtl.get();
        let settle = contact.borrow_mut().release_swipe(&route(&w), gesture.current_event().map_or(0, |event| event.time()), contents.width() as f32, rtl);
        let complete = settle.action.is_some();
        let from = contents.imp().swipe.progress.get();
        let target = adw::CallbackAnimationTarget::new(glib::clone!(#[weak] contents, move |progress| {
            contents.imp().swipe.progress.set(progress); contents.queue_draw();
        }));
        let animation = adw::TimedAnimation::new(&contents, from, if complete { 1. } else { 0. }, settle.duration_ms, target);
        animation.connect_done(glib::clone!(#[weak] w, #[weak] contents, #[weak] stack, move |_| {
            reset(&contents);
            if complete {
                let transition = stack.transition_type(); stack.set_transition_type(gtk::StackTransitionType::None);
                if let Some(action) = &settle.action { w.dispatch(UiAction::Phone { action: action.clone() }); }
                stack.set_transition_type(transition);
            }
        }));
        animation.play();
        *contents.imp().swipe.animation.borrow_mut() = Some(animation);
    }));
    drag.connect_cancel(glib::clone!(#[weak] contents, #[weak] stack, #[strong] contact, move |_, _| {
        contact.borrow_mut().cancel_press(); contact.borrow_mut().begin_swipe(); cancel_pressed(stack.upcast_ref()); reset(&contents);
    }));
    contents.add_controller(drag);
    contents.connect_unmap(glib::clone!(#[strong] contact, move |contents| cancel(contents, &contact)));
}
