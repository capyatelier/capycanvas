use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use layer_ui::{ContactPhase, UiInput};

use crate::workspace::Workspace;

#[derive(Default)]
pub(crate) struct ColorTouches {
    contacts: RefCell<HashMap<gdk::EventSequence, (u64, [f32; 2])>>,
    next: Cell<u64>,
    claimed: Cell<bool>,
}

impl ColorTouches {
    pub(crate) fn phase(event: &gdk::Event) -> Option<ContactPhase> {
        match event.event_type() {
            gdk::EventType::TouchBegin => Some(ContactPhase::Down),
            gdk::EventType::TouchUpdate => Some(ContactPhase::Move),
            gdk::EventType::TouchEnd => Some(ContactPhase::Up),
            gdk::EventType::TouchCancel => Some(ContactPhase::Cancel),
            _ => None,
        }
    }

    pub(crate) fn forward(&self, w: &Rc<Workspace>, widget: &gtk::Widget, event: &gdk::Event, at: Option<[f32; 2]>, inside: bool) -> bool {
        let Some(phase) = Self::phase(event) else { return false };
        let sequence = event.event_sequence();
        if phase == ContactPhase::Down {
            let Some(at) = at.filter(|_| inside) else { return false };
            self.next.set(self.next.get() + 1);
            self.contacts.borrow_mut().insert(sequence.clone(), (self.next.get(), at));
        }
        let Some((id, previous)) = self.contacts.borrow().get(&sequence).copied() else { return false };
        let position = at.unwrap_or(previous);
        if let Some(contact) = self.contacts.borrow_mut().get_mut(&sequence) {
            contact.1 = position;
        }
        let scale = widget.scale_factor() as f32;
        let reply = w.interact(UiInput::ColorTouch {
            surface: widget.as_ptr() as usize as u64,
            id,
            phase,
            position: position.map(|v| v * scale),
            time_ns: u64::from(event.time()) * 1_000_000,
        });
        self.claimed.set(self.claimed.get() || reply.handled);
        let claimed = self.claimed.get();
        if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            self.contacts.borrow_mut().remove(&sequence);
            if self.contacts.borrow().is_empty() {
                self.claimed.set(false);
            }
        }
        claimed
    }

    pub(crate) fn cancel(&self, w: &Rc<Workspace>, widget: &gtk::Widget) {
        let retired = std::mem::take(&mut *self.contacts.borrow_mut());
        self.claimed.set(false);
        let scale = widget.scale_factor() as f32;
        for (_, (id, position)) in retired {
            w.interact(UiInput::ColorTouch {
                surface: widget.as_ptr() as usize as u64,
                id,
                phase: ContactPhase::Cancel,
                position: position.map(|v| v * scale),
                time_ns: 0,
            });
        }
    }
}
