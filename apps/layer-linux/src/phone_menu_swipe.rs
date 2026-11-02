use super::*;

#[derive(Default)]
pub(super) struct State {
    pub progress: Cell<f64>,
    pages: RefCell<Option<(gtk::Widget, gtk::Widget, gtk::Widget)>>,
    animation: RefCell<Option<adw::TimedAnimation>>,
    rtl: Cell<bool>,
}

pub(super) fn release_activation(row: &gtk::Widget) {
    let controllers = row.observe_controllers().iter::<glib::Object>().filter_map(|object| object.ok()?.downcast::<gtk::GestureClick>().ok()).collect::<Vec<_>>();
    for controller in controllers { row.remove_controller(&controller); }
    let click = gtk::GestureClick::new();
    click.set_button(1);
    let armed = Rc::new(Cell::new(false));
    click.connect_pressed(glib::clone!(#[strong] armed, move |_, _, _, _| armed.set(true)));
    let cancel = Rc::new(glib::clone!(#[weak] row, #[strong] armed, move || {
        armed.set(false);
        row.unset_state_flags(gtk::StateFlags::ACTIVE | gtk::StateFlags::PRELIGHT);
    }));
    click.connect_stopped(glib::clone!(#[strong] cancel, move |_| cancel()));
    click.connect_cancel(move |_, _| cancel());
    click.connect_released(glib::clone!(#[weak] row, move |_, _, x, y| {
        row.unset_state_flags(gtk::StateFlags::ACTIVE);
        if armed.replace(false) && row.contains(x, y) { row.grab_focus(); row.activate(); }
    }));
    row.add_controller(click);
}

fn reset(contents: &MenuContents) {
    let state = &contents.imp().swipe;
    if let Some((_, parent, _)) = state.pages.take() { parent.set_child_visible(false); }
    if let Some(stack) = contents.first_child() { stack.set_can_target(true); }
    state.progress.set(0.);
    contents.queue_allocate();
}

pub(super) fn allocate(contents: &native::Contents, width: i32, height: i32, baseline: i32) {
    if let Some((_, parent, _)) = contents.swipe.pages.borrow().as_ref() {
        parent.set_child_visible(true);
        parent.allocate(width, height, baseline, None);
    }
}

pub(super) fn snapshot(contents: &native::Contents, snapshot: &gtk::Snapshot) {
    let obj = contents.obj();
    let Some(stack) = obj.first_child() else { return; };
    let pages = contents.swipe.pages.borrow();
    let Some((current, parent, _)) = pages.as_ref() else { obj.snapshot_child(&stack, snapshot); return; };
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

pub(super) fn bind(contents: &MenuContents, stack: &gtk::Stack, navigation: Rc<RefCell<PhoneMenuNavigation>>) {
    let drag = gtk::GestureDrag::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let velocity = Rc::new(Cell::new((0., 0u32, 0.)));
    drag.connect_drag_begin(glib::clone!(#[weak] contents, #[weak] stack, #[strong] navigation, #[strong] velocity, move |gesture, _, _| {
        if navigation.borrow().is_root() || stack.is_transition_running() || contents.imp().swipe.pages.borrow().is_some() {
            gesture.set_state(gtk::EventSequenceState::Denied); return;
        }
        navigation.borrow_mut().begin_swipe();
        contents.imp().swipe.rtl.set(contents.direction() == gtk::TextDirection::Rtl);
        velocity.set((0., gesture.current_event().map_or(0, |event| event.time()), 0.));
    }));
    drag.connect_drag_update(glib::clone!(#[weak] contents, #[weak] stack, #[strong] navigation, #[strong] velocity, move |gesture, dx, dy| {
        let rtl = contents.imp().swipe.rtl.get();
        let past_slop = contents.drag_check_threshold(0, 0, dx as i32, dy as i32);
        let result = navigation.borrow_mut().swipe([dx as f32, dy as f32], contents.width() as f32, past_slop, rtl);
        match result {
            PhoneMenuSwipe::Pending => (),
            PhoneMenuSwipe::Scroll => { gesture.set_state(gtk::EventSequenceState::Denied); }
            PhoneMenuSwipe::Back(progress) => {
                if contents.imp().swipe.pages.borrow().is_none() {
                    let current = stack.visible_child().unwrap();
                    let name = stack.visible_child_name().unwrap();
                    let menu = navigation.borrow().name();
                    if menu != name { gesture.set_state(gtk::EventSequenceState::Denied); return; }
                    let mut parent = navigation.borrow().clone(); parent.back();
                    let parent = stack.child_by_name(&parent.name()).unwrap();
                    let mut rows = Vec::new(); descendants(&current, &mut rows);
                    let back = rows.into_iter().find(|row| row.widget_name() == "phone-menu-back").unwrap();
                    *contents.imp().swipe.pages.borrow_mut() = Some((current, parent, back));
                    stack.set_can_target(false);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    contents.queue_allocate();
                }
                let time = gesture.current_event().map_or(0, |event| event.time());
                let (last, previous, speed) = velocity.get();
                let elapsed = time.wrapping_sub(previous);
                velocity.set((dx, time, if elapsed > 0 { (dx - last) * 1000. / elapsed as f64 } else { speed }));
                contents.imp().swipe.progress.set(progress as f64);
                contents.queue_draw();
            }
        }
    }));
    drag.connect_drag_end(glib::clone!(#[weak] contents, #[weak] stack, #[strong] navigation, move |gesture, _, _| {
        if contents.imp().swipe.pages.borrow().is_none() { navigation.borrow_mut().begin_swipe(); return; }
        let (_, time, speed) = velocity.get();
        let elapsed = gesture.current_event().map_or(time, |event| event.time()).wrapping_sub(time);
        let speed = if elapsed > 150 { 0. } else { speed };
        let rtl = contents.imp().swipe.rtl.get();
        let complete = navigation.borrow_mut().end_swipe((speed * if rtl { -1. } else { 1. } / contents.width().max(1) as f64) as f32);
        let from = contents.imp().swipe.progress.get();
        let target = adw::CallbackAnimationTarget::new(glib::clone!(#[weak] contents, move |progress| {
            contents.imp().swipe.progress.set(progress); contents.queue_draw();
        }));
        let animation = adw::TimedAnimation::new(&contents, from, if complete { 1. } else { 0. }, 150, target);
        animation.connect_done(glib::clone!(#[weak] contents, #[weak] stack, move |_| {
            let back = contents.imp().swipe.pages.borrow().as_ref().map(|(_, _, back)| back.clone());
            reset(&contents);
            if complete && let Some(back) = back {
                let transition = stack.transition_type(); stack.set_transition_type(gtk::StackTransitionType::None);
                back.activate(); stack.set_transition_type(transition);
                if let Some(scroll) = contents.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>() { scroll.vadjustment().set_value(0.); }
            }
        }));
        animation.play();
        *contents.imp().swipe.animation.borrow_mut() = Some(animation);
    }));
    drag.connect_cancel(glib::clone!(#[weak] contents, #[strong] navigation, move |_, _| { navigation.borrow_mut().begin_swipe(); reset(&contents); }));
    contents.add_controller(drag);
    contents.connect_unmap(|contents| {
        if let Some(animation) = contents.imp().swipe.animation.take() { animation.pause(); }
        reset(contents);
    });
}
