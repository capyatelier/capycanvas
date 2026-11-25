use crate::frame_demand::{Allowance, Input, Outstanding, Turn, UnitWait};
use crate::residency::{Blocked, Charge, Class, Reservation, Tier};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Closure { pub bytes: u64, pub seats: u32, pub seat: u64 }
impl Closure {
    pub(crate) fn total(&self) -> u64 { self.bytes.saturating_add(u64::from(self.seats).saturating_mul(self.seat)) }
}

pub(crate) struct Budget { reservation: Reservation, free: u32, seat: u64 }
impl Budget {
    pub(crate) fn spend(&mut self, tier: &Arc<Tier>, bytes: u64) -> Option<Charge> {
        let spare = self.reservation.bytes().saturating_sub(u64::from(self.free) * self.seat);
        if bytes > spare { return None; }
        let paid = self.reservation.split(bytes)?;
        let charge = tier.charge(bytes);
        drop(paid);
        Some(charge)
    }
    pub(crate) fn seat(&mut self) -> Option<Reservation> {
        if self.free == 0 { return None; }
        let seat = self.reservation.split(self.seat)?;
        self.free -= 1;
        Some(seat)
    }
    pub(crate) fn vacate(&mut self, seat: Reservation) { self.free += 1; self.reservation.merge(seat); }
    pub(crate) fn shed(&mut self, kept: u32) { while self.free > kept && let Some(seat) = self.seat() { drop(seat); } }
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> u64 { self.reservation.bytes() }
}

#[derive(Clone)]
pub(crate) struct Waiter(Arc<AtomicBool>);
impl Waiter {
    pub(crate) fn settled(&self) -> bool { self.0.load(Ordering::Acquire) }
}

enum Phase { Wanted, Admitted(Budget), Running(Budget) }
struct Entry { owner: u64, tag: u64, class: Class, obligation: bool, closure: Closure, sequence: u64, wanted: u64, phase: Phase, blocked: Option<Input>, waiter: Weak<AtomicBool> }
impl Drop for Entry {
    fn drop(&mut self) { if let Some(settled) = self.waiter.upgrade() { settled.store(true, Ordering::Release); } }
}
impl Entry {
    fn best_effort(&self) -> bool { self.class != Class::Authored && !self.obligation }
    fn rank(&self) -> (u8, u64) {
        (if self.class == Class::Authored { 0 } else if self.obligation { 1 } else { 2 + self.class as u8 }, self.sequence)
    }
}

type Ranked = ((u8, u64), u64);
enum Stop { Memory(UnitWait), Full }
#[derive(Default)]
struct Core { entries: BTreeMap<u64, Entry>, queue: BTreeSet<Ranked>, active: BTreeSet<Ranked>, queued: BTreeMap<u64, usize>, next: u64, stop: Option<Stop>, epoch: u64, dirty: bool, lapsed: Vec<(u64, u64)> }
impl Core {
    fn count(&mut self, owner: u64, queued: bool) {
        let count = self.queued.entry(owner).or_default();
        if queued { *count += 1; } else { *count -= 1; }
        if *count == 0 { self.queued.remove(&owner); }
    }
    fn remove(&mut self, id: u64) -> Option<Entry> {
        let entry = self.entries.remove(&id)?;
        if self.queue.remove(&(entry.rank(), id)) { self.count(entry.owner, false); }
        self.active.remove(&(entry.rank(), id));
        Some(entry)
    }
    fn enter(&mut self, id: u64, phase: Phase) -> Option<Phase> {
        let entry = self.entries.get_mut(&id)?;
        let (ranked, owner) = ((entry.rank(), id), entry.owner);
        let wanted = matches!(phase, Phase::Wanted);
        if wanted { self.active.remove(&ranked); } else { self.active.insert(ranked); entry.blocked = None; }
        let previous = std::mem::replace(&mut entry.phase, phase);
        let moved = if wanted { self.queue.insert(ranked) } else { self.queue.remove(&ranked) };
        if moved { self.count(owner, wanted); }
        Some(previous)
    }
    fn admitted(&self, owner: u64) -> impl Iterator<Item = &Entry> { self.active.iter().map(|(_, id)| &self.entries[id]).filter(move |entry| entry.owner == owner) }
}

#[derive(Clone, Default)]
pub(crate) struct Schedule(Arc<Mutex<Core>>);
pub(crate) struct Group { core: Weak<Mutex<Core>>, id: u64 }
impl Drop for Group {
    fn drop(&mut self) {
        let entry = self.core.upgrade().and_then(|core| { let mut core = core.lock().unwrap(); core.dirty = true; core.remove(self.id) });
        drop(entry);
    }
}

impl Schedule {
    pub(crate) fn want(&self, owner: u64, tag: u64, class: Class, obligation: bool, closure: Closure, epoch: u64) -> Group {
        let mut core = self.0.lock().unwrap();
        let id = core.next;
        core.next += 1;
        let entry = Entry { owner, tag, class, obligation, closure, sequence: id, wanted: epoch, phase: Phase::Wanted, blocked: None, waiter: Weak::new() };
        core.queue.insert((entry.rank(), id));
        core.count(owner, true);
        core.entries.insert(id, entry);
        core.dirty = true;
        Group { core: Arc::downgrade(&self.0), id }
    }
    pub(crate) fn rewant(&self, group: &Group, class: Class, epoch: u64) {
        let mut core = self.0.lock().unwrap();
        let Core { entries, queue, active, dirty, .. } = &mut *core;
        let Some(entry) = entries.get_mut(&group.id) else { return; };
        entry.wanted = entry.wanted.max(epoch);
        if class >= entry.class { return; }
        let ranks = if matches!(entry.phase, Phase::Wanted) { queue } else { active };
        ranks.remove(&(entry.rank(), group.id));
        entry.class = class;
        ranks.insert((entry.rank(), group.id));
        *dirty = true;
    }
    pub(crate) fn waiter(&self, group: &Group) -> Option<Waiter> {
        let mut core = self.0.lock().unwrap();
        let entry = core.entries.get_mut(&group.id)?;
        Some(Waiter(entry.waiter.upgrade().unwrap_or_else(|| { let settled = Arc::new(AtomicBool::new(false)); entry.waiter = Arc::downgrade(&settled); settled })))
    }
    pub(crate) fn lapsed(&self, owner: u64) -> Vec<u64> {
        let mut core = self.0.lock().unwrap();
        let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut core.lapsed).into_iter().partition(|(lapsed, _)| *lapsed == owner);
        core.lapsed = others;
        mine.into_iter().map(|(_, tag)| tag).collect()
    }
    pub(crate) fn admitted(&self, group: &Group) -> bool {
        self.0.lock().unwrap().entries.get(&group.id).is_some_and(|entry| !matches!(entry.phase, Phase::Wanted))
    }
    pub(crate) fn admit(&self, epoch: u64, observe: impl Fn(&UnitWait) -> Turn, working: u64, mut reserve: impl FnMut(Class, u64, Allowance) -> Result<Reservation, Blocked>) -> Option<Blocked> {
        let mut allowance = Allowance::Fresh;
        let (previous, mut admitted) = loop {
            let (expired, stop) = {
                let mut core = self.0.lock().unwrap();
                let mut expired = Vec::new();
                if core.epoch < epoch {
                    core.epoch = epoch;
                    let stale: Vec<u64> = core.entries.iter()
                        .filter(|(_, entry)| !entry.obligation && !matches!(entry.phase, Phase::Running(_)) && entry.wanted.saturating_add(2) < epoch && entry.waiter.strong_count() == 0)
                        .map(|(id, _)| *id).collect();
                    core.dirty |= !stale.is_empty();
                    expired.extend(stale.into_iter().filter_map(|id| core.remove(id)));
                    let lapsed: Vec<_> = expired.iter().map(|entry: &Entry| (entry.owner, entry.tag)).collect();
                    core.lapsed.extend(lapsed);
                }
                let stop = match &core.stop {
                    _ if core.dirty => None,
                    Some(Stop::Memory(wait)) => Some(Some(wait.clone())),
                    _ => Some(None),
                };
                (expired, stop)
            };
            drop(expired);
            let observed = match stop {
                Some(None) => return None,
                Some(Some(wait)) => Some((observe(&wait), wait)),
                None => None,
            };
            let mut core = self.0.lock().unwrap();
            if let Some((turn, wait)) = observed {
                if core.dirty || !matches!(&core.stop, Some(Stop::Memory(current)) if *current == wait) { continue; }
                match turn {
                    Turn::Enter(entered) => allowance = entered,
                    Turn::Sleep(_) | Turn::Dormant | Turn::Deliver => return wait.need().cloned(),
                }
            }
            core.dirty = false;
            let admitted = core.active.iter().map(|(_, id)| &core.entries[id]).filter(|entry| entry.best_effort())
                .fold((0, 0), |(count, bytes): (usize, u64), entry| (count + 1, bytes.saturating_add(entry.closure.total())));
            break (core.stop.take(), admitted);
        };
        drop(previous);
        let below = |head: Ranked| -> Vec<u64> { self.0.lock().unwrap().active.range((Excluded(head), Unbounded)).rev().map(|(_, id)| *id).collect() };
        let preempt = |victim: u64, best_effort_only: bool, admitted: &mut (usize, u64)| -> bool {
            let mut core = self.0.lock().unwrap();
            let Some((best_effort, total)) = core.entries.get(&victim)
                .filter(|entry| matches!(entry.phase, Phase::Admitted(_)) && (!best_effort_only || entry.best_effort())).map(|entry| (entry.best_effort(), entry.closure.total())) else { return false; };
            let phase = core.enter(victim, Phase::Wanted);
            drop(core);
            if best_effort { admitted.0 -= 1; admitted.1 -= total; }
            drop(phase);
            true
        };
        loop {
            let head = { let core = self.0.lock().unwrap(); core.queue.first().map(|&head| { let entry = &core.entries[&head.1]; (head, entry.class, entry.closure, entry.best_effort()) }) };
            let Some((head, class, closure, best_effort)) = head else { return None; };
            let crowded = |admitted: &(usize, u64)| admitted.0 > 0 && admitted.1.saturating_add(closure.total()) > working / 4;
            if best_effort && crowded(&admitted) {
                for victim in below(head) { preempt(victim, true, &mut admitted); }
                if crowded(&admitted) { self.0.lock().unwrap().stop = Some(Stop::Full); return None; }
            }
            let mut result = reserve(class, closure.total(), allowance);
            for victim in below(head) {
                if result.is_ok() { break; }
                if preempt(victim, false, &mut admitted) { result = reserve(class, closure.total(), allowance); }
            }
            let mut core = self.0.lock().unwrap();
            match result {
                Ok(reservation) => {
                    allowance = Allowance::Fresh;
                    if core.entries.contains_key(&head.1) {
                        core.enter(head.1, Phase::Admitted(Budget { reservation, free: closure.seats, seat: closure.seat }));
                        if best_effort { admitted.0 += 1; admitted.1 += closure.total(); }
                    } else { drop(core); drop(reservation); }
                },
                Err(blocked) => { core.stop = Some(Stop::Memory(UnitWait::parked(blocked.clone(), allowance))); return Some(blocked); }
            }
        }
    }
    pub(crate) fn runnable(&self, owner: u64) -> Vec<u64> {
        let mut core = self.0.lock().unwrap();
        let Core { entries, active, .. } = &mut *core;
        active.iter().filter_map(|(_, id)| entries.get_mut(id).filter(|entry| entry.owner == owner).map(|entry| { entry.blocked = None; entry.tag })).collect()
    }
    pub(crate) fn start(&self, group: &Group) -> bool {
        let mut core = self.0.lock().unwrap();
        let Some(entry) = core.entries.get_mut(&group.id) else { return false; };
        entry.phase = match std::mem::replace(&mut entry.phase, Phase::Wanted) {
            Phase::Admitted(budget) | Phase::Running(budget) => Phase::Running(budget),
            Phase::Wanted => return false,
        };
        true
    }
    pub(crate) fn budget<T>(&self, group: &Group, use_: impl FnOnce(&mut Budget) -> T) -> Option<T> {
        match &mut self.0.lock().unwrap().entries.get_mut(&group.id)?.phase { Phase::Running(budget) => Some(use_(budget)), _ => None }
    }
    pub(crate) fn block(&self, group: &Group, input: Input) {
        if let Some(entry) = self.0.lock().unwrap().entries.get_mut(&group.id) { entry.blocked = Some(input); }
    }
    pub(crate) fn hold(&self, owner: u64, input: Input) {
        let mut core = self.0.lock().unwrap();
        let Core { entries, active, .. } = &mut *core;
        for (_, id) in active.iter() { if let Some(entry) = entries.get_mut(id).filter(|entry| entry.owner == owner) { entry.blocked = Some(input); } }
    }
    pub(crate) fn demand(&self, owner: u64, waits: impl Fn(Input) -> bool, observe: impl Fn(&UnitWait) -> Turn) -> Option<Outstanding> {
        let (stop, blocked) = {
            let core = self.0.lock().unwrap();
            let blocked = core.admitted(owner).map(|entry| entry.blocked).collect::<Vec<_>>();
            let stop = match &core.stop {
                _ if !core.queued.contains_key(&owner) => None,
                Some(Stop::Memory(wait)) => Some(Some(wait.clone())),
                Some(Stop::Full) if !core.active.is_empty() => None,
                _ => Some(None),
            };
            (stop, blocked)
        };
        let blocked = blocked.into_iter().map(|input| input.filter(|&input| waits(input))).collect::<Vec<_>>();
        if blocked.iter().any(Option::is_none) { return Some(Outstanding::Runnable); }
        match stop.map(|wait| wait.map(|wait| observe(&wait))) {
            Some(None | Some(Turn::Enter(_))) => return Some(Outstanding::Runnable),
            Some(Some(Turn::Sleep(input))) => return Some(Outstanding::Blocked(input)),
            Some(Some(Turn::Dormant | Turn::Deliver)) | None => {},
        }
        blocked.into_iter().flatten().next().map(Outstanding::Blocked)
    }
}

#[cfg(test)]
impl Schedule {
    pub(crate) fn admitted_owners(&self) -> BTreeSet<u64> {
        let core = self.0.lock().unwrap();
        core.active.iter().map(|(_, id)| core.entries[id].owner).collect()
    }
    pub(crate) fn alive(&self, group: &Group) -> bool { self.0.lock().unwrap().entries.contains_key(&group.id) }
    pub(crate) fn census(&self) -> ([usize; 3], bool, bool) {
        let core = self.0.lock().unwrap();
        let mut census = [0; 3];
        for entry in core.entries.values() { census[match entry.phase { Phase::Wanted => 0, Phase::Admitted(_) => 1, Phase::Running(_) => 2 }] += 1; }
        (census, matches!(core.stop, Some(Stop::Memory(_))), matches!(core.stop, Some(Stop::Full)))
    }
    pub(crate) fn reserved(&self) -> u64 {
        self.0.lock().unwrap().entries.values().map(|entry| match &entry.phase { Phase::Admitted(budget) | Phase::Running(budget) => budget.bytes(), Phase::Wanted => 0 }).sum()
    }
}
#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
