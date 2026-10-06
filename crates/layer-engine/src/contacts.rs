//! Contacts that begin before painting is ready are held whole and delivered
//! once it is, so a stroke or drag is neither dropped nor started midway.
use crate::{PenEvent, PenPhase, SampleFlags};
use std::collections::{BTreeMap, VecDeque};
use web_time::{Duration, Instant};

/// How long a contact is held for painting to become ready.
const HOLD: Duration = Duration::from_secs(5);
/// Samples held across every contact.
const SAMPLES: usize = 4096;

#[derive(Default)]
pub struct DeferredContacts {
    held: VecDeque<PenEvent>,
    contacts: BTreeMap<u64, Contact>,
}
struct Contact {
    began: Instant,
    ended: bool,
    discarded: bool,
}
impl DeferredContacts {
    /// The events to deliver now for `event`, in order, given whether
    /// painting is `ready`. A contact whose Down arrives before it is held
    /// until it is; then every held event is delivered before this one.
    /// Predictions of a held contact are dropped.
    pub fn admit(&mut self, event: PenEvent, ready: bool, now: Instant) -> Vec<PenEvent> {
        let id = event.device_id;
        let predicted = event.flags.contains(SampleFlags::PREDICTED);
        let ends = matches!(event.phase, PenPhase::Up | PenPhase::Cancel);
        match self.contacts.get_mut(&id) {
            None if event.phase == PenPhase::Down
                && !predicted
                && !event.flags.contains(SampleFlags::CORRECTION)
                && !ready =>
            {
                self.contacts.insert(id, Contact { began: now, ended: false, discarded: false });
                self.held.push_back(event);
                self.expire(now);
                return Vec::new();
            }
            None => {
                let mut events = self.release(ready, now);
                events.push(event);
                return events;
            }
            Some(contact) if contact.discarded => {
                if ends && !predicted {
                    self.contacts.remove(&id);
                }
                return Vec::new();
            }
            Some(_) if predicted => return Vec::new(),
            Some(_) if event.flags.contains(SampleFlags::CORRECTION) => self.held.push_back(event),
            Some(contact) => {
                contact.ended = ends;
                if event.phase == PenPhase::Cancel {
                    self.discard(id);
                    return Vec::new();
                }
                self.held.push_back(event);
            }
        }
        self.expire(now);
        self.release(ready, now)
    }
    /// Every held event, in the order it arrived, once painting is `ready`;
    /// contacts held too long, or beyond the sample limit, are discarded.
    pub fn release(&mut self, ready: bool, now: Instant) -> Vec<PenEvent> {
        self.expire(now);
        if !ready || self.held.is_empty() {
            return Vec::new();
        }
        self.contacts.retain(|_, contact| contact.discarded);
        self.held.drain(..).collect()
    }
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }
    /// Whether samples wait for painting to become ready.
    pub fn holding(&self) -> bool {
        !self.held.is_empty()
    }
    pub fn clear(&mut self) {
        self.held.clear();
        self.contacts.clear();
    }
    fn expire(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .contacts
            .iter()
            .filter(|(_, c)| !c.discarded && now.duration_since(c.began) > HOLD)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.discard(id);
        }
        while self.held.len() > SAMPLES {
            let id = self.held.front().unwrap().device_id;
            self.discard(id);
        }
    }
    /// Drop contact `id`'s held events; a contact still down keeps being
    /// ignored until it ends.
    fn discard(&mut self, id: u64) {
        self.held.retain(|e| e.device_id != id);
        match self.contacts.get_mut(&id) {
            Some(contact) if !contact.ended => contact.discarded = true,
            _ => {
                self.contacts.remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::Point;
    use crate::ToolKind;

    fn sample(id: u64, sequence: u64, phase: PenPhase) -> PenEvent {
        PenEvent {
            device_id: id,
            sequence,
            timestamp_ns: sequence * 1_000_000,
            view_revision: 0,
            surface_position: Point { x: sequence as f32, y: 0. },
            pressure: 0.5,
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }
    }
    fn sequences(events: &[PenEvent]) -> Vec<u64> {
        events.iter().map(|e| e.sequence).collect()
    }

    #[test]
    fn a_contact_completed_before_readiness_is_replayed_exactly_once() {
        let mut contacts = DeferredContacts::default();
        let t = Instant::now();
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            assert!(contacts.admit(sample(1, sequence, phase), false, t).is_empty());
        }
        let predicted = PenEvent { flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0), ..sample(1, 9, PenPhase::Move) };
        assert!(contacts.admit(predicted, false, t).is_empty(), "predictions of a held contact are dropped");
        assert!(contacts.release(false, t).is_empty());
        assert_eq!(sequences(&contacts.release(true, t)), [1, 2, 3]);
        assert!(contacts.release(true, t).is_empty(), "replayed once");
        assert!(contacts.is_empty());
        assert_eq!(sequences(&contacts.admit(sample(1, 4, PenPhase::Down), true, t)), [4]);
    }

    #[test]
    fn a_contact_that_becomes_ready_midway_continues_live() {
        let mut contacts = DeferredContacts::default();
        let t = Instant::now();
        assert!(contacts.admit(sample(1, 1, PenPhase::Down), false, t).is_empty());
        assert!(contacts.admit(sample(2, 2, PenPhase::Down), false, t).is_empty());
        assert!(contacts.admit(sample(1, 3, PenPhase::Move), false, t).is_empty());
        assert_eq!(sequences(&contacts.admit(sample(1, 4, PenPhase::Move), true, t)), [1, 2, 3, 4]);
        assert_eq!(sequences(&contacts.admit(sample(2, 5, PenPhase::Up), true, t)), [5]);
        assert!(contacts.is_empty());
    }

    #[test]
    fn cancelled_expired_and_oversized_contacts_are_discarded() {
        let mut contacts = DeferredContacts::default();
        let t = Instant::now();
        contacts.admit(sample(1, 1, PenPhase::Down), false, t);
        contacts.admit(sample(1, 2, PenPhase::Cancel), false, t);
        assert!(contacts.release(true, t).is_empty() && contacts.is_empty(), "a cancelled contact is dropped");

        contacts.admit(sample(1, 3, PenPhase::Down), false, t);
        let late = t + HOLD + Duration::from_millis(1);
        assert!(contacts.release(true, late).is_empty(), "a contact never ready in time is dropped");
        assert!(contacts.admit(sample(1, 4, PenPhase::Move), true, late).is_empty(), "the rest of it is ignored");
        assert!(contacts.admit(sample(1, 5, PenPhase::Up), true, late).is_empty());
        assert!(contacts.is_empty());

        contacts.admit(sample(1, 6, PenPhase::Down), false, t);
        for sequence in 7..8 + SAMPLES as u64 {
            contacts.admit(sample(1, sequence, PenPhase::Move), false, t);
        }
        assert!(contacts.release(true, t).is_empty(), "an oversized contact is dropped");
        assert!(!contacts.is_empty());
        contacts.admit(sample(1, 9 + SAMPLES as u64, PenPhase::Up), true, t);
        assert!(contacts.is_empty());
    }
}
