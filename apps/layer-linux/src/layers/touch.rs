use super::*;
use layer_ui::{ContactPhase, UiInput};

fn reset(widget: &gtk::Widget) {
    let controllers = widget.observe_controllers();
    for i in 0..controllers.n_items() {
        if let Some(controller) = controllers.item(i).and_downcast::<gtk::EventController>() { controller.reset(); }
    }
    let mut child = widget.first_child();
    while let Some(widget) = child { child = widget.next_sibling(); reset(&widget); }
}

pub(super) fn install(list: &gtk::ScrolledWindow, owner: Rc<RefCell<Weak<Workspace>>>, rows: Rc<RefCell<HashMap<usize, Row>>>) {
    let contacts = Rc::new(RefCell::new(HashMap::<gdk::EventSequence, (u64, [f32; 2], Option<u64>)>::new()));
    let next = Cell::new(0u64);
    let claimed = Rc::new(Cell::new(false));
    let last_ns = Rc::new(Cell::new(0));
    let input = gtk::EventControllerLegacy::new();
    input.set_propagation_phase(gtk::PropagationPhase::Capture);
    input.connect_event(glib::clone!(#[weak] list, #[strong] contacts, #[strong] owner, #[strong] rows, #[strong] claimed, #[strong] last_ns,
        #[upgrade_or] glib::Propagation::Proceed,
        move |_, event| {
            let phase = match event.event_type() {
                gdk::EventType::TouchBegin => ContactPhase::Down, gdk::EventType::TouchUpdate => ContactPhase::Move,
                gdk::EventType::TouchEnd => ContactPhase::Up, gdk::EventType::TouchCancel => ContactPhase::Cancel,
                _ => return glib::Propagation::Proceed,
            };
            let Some(w) = owner.borrow().upgrade() else { return glib::Propagation::Proceed; };
            let point = event.position().and_then(|(x, y)| crate::input::widget_point(&list, x, y));
            let sequence = event.event_sequence();
            if phase == ContactPhase::Down {
                let Some(point) = point else { return glib::Propagation::Proceed; };
                let row = rows.borrow().values().find(|row| row.bound.get() && list.compute_point(&row.swipe, &point)
                    .is_some_and(|p| p.x() >= 0. && p.y() >= 0. && p.x() < row.swipe.width() as f32 && p.y() < row.swipe.height() as f32)).map(|row| row.id.get());
                if row.is_none() { return glib::Propagation::Proceed; }
                next.set(next.get() + 1);
                contacts.borrow_mut().insert(sequence.clone(), (next.get(), [point.x(), point.y()], row));
            }
            let Some((id, previous, row)) = contacts.borrow().get(&sequence).copied() else { return glib::Propagation::Proceed; };
            let position = point.map_or(previous, |p| [p.x(), p.y()]);
            if let Some(contact) = contacts.borrow_mut().get_mut(&sequence) { contact.1 = position; }
            let scale = list.scale_factor() as f32;
            last_ns.set(u64::from(event.time()) * 1_000_000);
            let reply = w.interact(UiInput::LayerTouch { surface: list.as_ptr() as usize as u64, id, phase,
                position: position.map(|v| v * scale), row, time_ns: last_ns.get() });
            if reply.handled && !claimed.replace(true) {
                let swipes: Vec<_> = rows.borrow().values().map(|row| row.swipe.clone()).collect();
                for swipe in swipes { swipe.reset(); reset(swipe.upcast_ref()); }
            }
            let handled = claimed.get();
            if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                contacts.borrow_mut().remove(&sequence);
                if contacts.borrow().is_empty() { claimed.set(false); }
            }
            if handled { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }));
    list.add_controller(input);
    list.connect_unmap(glib::clone!(#[strong] contacts, #[strong] owner, move |list| {
        let retired = std::mem::take(&mut *contacts.borrow_mut());
        claimed.set(false);
        let workspace = owner.borrow().upgrade();
        if let Some(w) = workspace {
            let scale = list.scale_factor() as f32;
            for (_, (id, position, row)) in retired {
                w.interact(UiInput::LayerTouch { surface: list.as_ptr() as usize as u64, id, phase: ContactPhase::Cancel,
                    position: position.map(|v| v * scale), row, time_ns: last_ns.get() });
            }
        }
    }));
}
