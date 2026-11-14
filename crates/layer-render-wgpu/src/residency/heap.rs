use super::{Class, Demand};
use std::{cmp::Ordering, collections::BinaryHeap, sync::{Arc, Mutex, Weak, atomic::{AtomicBool, AtomicU64, Ordering::SeqCst}}};

const SCALE: u128 = 1 << 32;
pub(crate) fn rebuild_value(work: u64, bytes: u64) -> u64 { (u128::from(work) * SCALE / u128::from(bytes.max(1))).min(u128::from(u64::MAX)) as u64 }

#[derive(Default)]
pub(crate) struct Heap { queue: Mutex<Queue>, pending: AtomicU64, epoch: AtomicU64 }
#[derive(Default)]
struct Queue { inflation: u64, sequence: u64, live: usize, entries: BinaryHeap<Entry> }
struct Entry { priority: u64, sequence: u64, lease: Weak<Lease> }
impl Ord for Entry { fn cmp(&self, other: &Self) -> Ordering { (other.priority, other.sequence).cmp(&(self.priority, self.sequence)) } }
impl PartialOrd for Entry { fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) } }
impl PartialEq for Entry { fn eq(&self, other: &Self) -> bool { (self.priority, self.sequence) == (other.priority, other.sequence) } }
impl Eq for Entry {}

#[derive(Default)]
struct State { offered: Option<u64>, bytes: u64, evicted: bool, demand: Demand, guard: Option<Weak<dyn Send + Sync>> }
impl State {
    fn protected(&self, epoch: u64, class: Class) -> bool {
        self.demand.current(epoch, class) || self.guard.as_ref().is_some_and(|guard| guard.strong_count() != 0)
    }
}
pub(crate) struct Lease { heap: Arc<Heap>, notice: Arc<AtomicBool>, state: Mutex<State> }

impl Heap {
    pub(crate) fn lease(self: &Arc<Self>, notice: &Arc<AtomicBool>) -> Arc<Lease> {
        Arc::new(Lease { heap: self.clone(), notice: notice.clone(), state: Default::default() })
    }
    pub(crate) fn begin_frame(&self) { self.epoch.fetch_add(1, SeqCst); }
    pub(crate) fn epoch(&self) -> u64 { self.epoch.load(SeqCst) }
    pub(crate) fn pending(&self) -> u64 { self.pending.load(SeqCst) }
    pub(crate) fn evict(&self, bytes: u64, class: Class) -> u64 {
        let wanted = bytes.saturating_sub(self.pending());
        let (epoch, mut queue, mut marked, mut kept, mut seen) = (self.epoch(), self.queue.lock().unwrap(), 0, Vec::new(), Vec::new());
        while marked < wanted && let Some(entry) = queue.entries.pop() {
            let Some(lease) = entry.lease.upgrade() else { continue; };
            let mut state = lease.state.lock().unwrap();
            if state.offered == Some(entry.sequence) && state.protected(epoch, class) { kept.push(entry); }
            else if state.offered == Some(entry.sequence) {
                (state.offered, state.evicted) = (None, true);
                queue.live -= 1;
                queue.inflation = queue.inflation.max(entry.priority);
                marked += state.bytes;
                self.pending.fetch_add(state.bytes, SeqCst);
                lease.notice.store(true, SeqCst);
            }
            drop(state);
            seen.push(lease);
        }
        queue.entries.extend(kept);
        drop(queue);
        drop(seen);
        marked
    }
}

impl Lease {
    pub(crate) fn offer(self: &Arc<Self>, bytes: u64, value: u64) {
        let mut queue = self.heap.queue.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if state.evicted { return; }
        queue.sequence += 1;
        let sequence = queue.sequence;
        let priority = queue.inflation.saturating_add(value);
        if state.offered.replace(sequence).is_none() { queue.live += 1; }
        state.bytes = bytes;
        drop(state);
        queue.entries.push(Entry { priority, sequence, lease: Arc::downgrade(self) });
        if queue.entries.len() > 4 * queue.live + 64 {
            let mut seen = Vec::new();
            queue.entries.retain(|entry| entry.lease.upgrade().is_some_and(|lease| {
                let current = lease.state.lock().unwrap().offered == Some(entry.sequence);
                seen.push(lease);
                current
            }));
            drop(queue);
            drop(seen);
        }
    }
    pub(crate) fn hold(&self) {
        let mut queue = self.heap.queue.lock().unwrap();
        if self.state.lock().unwrap().offered.take().is_some() { queue.live -= 1; }
    }
    pub(crate) fn evicted(&self) -> bool { self.state.lock().unwrap().evicted }
    pub(crate) fn evict(&self) {
        let mut queue = self.heap.queue.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if state.offered.take().is_none() { return; }
        queue.live -= 1;
        state.evicted = true;
        self.heap.pending.fetch_add(state.bytes, SeqCst);
        self.notice.store(true, SeqCst);
    }
    pub(crate) fn restore(&self) {
        let mut state = self.state.lock().unwrap();
        if std::mem::take(&mut state.evicted) { self.heap.pending.fetch_sub(state.bytes, SeqCst); }
    }
    pub(crate) fn demand(&self) -> Option<Class> { self.state.lock().unwrap().demand.class_at(self.heap.epoch()) }
    pub(crate) fn recent(&self) -> bool { self.state.lock().unwrap().demand.protects(self.heap.epoch(), Class::Refinement) }
    pub(crate) fn stamp(&self, class: Class) { self.state.lock().unwrap().demand.stamp(self.heap.epoch(), class); }
    pub(crate) fn guard(&self, guard: Weak<dyn Send + Sync>) { self.state.lock().unwrap().guard = Some(guard); }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap();
        if state.evicted { self.heap.pending.fetch_sub(state.bytes, SeqCst); }
        else if state.offered.is_some() { self.heap.queue.lock().unwrap().live -= 1; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn leases(heap: &Arc<Heap>, notice: &Arc<AtomicBool>, offers: &[(u64, u64)]) -> Vec<Arc<Lease>> {
        offers.iter().map(|&(bytes, work)| { let lease = heap.lease(notice); lease.offer(bytes, rebuild_value(work, bytes)); lease }).collect()
    }
    #[test]
    fn eviction_takes_the_cheapest_rebuild_per_byte_first_and_counts_marks_until_they_drop() {
        let (heap, notice) = (Arc::new(Heap::default()), Arc::new(AtomicBool::new(false)));
        let leases = leases(&heap, &notice, &[(4, 40), (4, 4), (8, 0), (4, 400)]);
        assert_eq!(heap.evict(10, Class::Fallback), 12);
        assert!(notice.load(SeqCst));
        assert_eq!(leases.iter().map(|lease| lease.evicted()).collect::<Vec<_>>(), [false, true, true, false]);
        assert_eq!(heap.evict(12, Class::Fallback), 0, "marked bytes count until their leases drop");
        let [first, _, _, last] = leases.try_into().ok().unwrap();
        assert_eq!(heap.pending(), 0);
        assert_eq!(heap.evict(1, Class::Fallback), 4);
        assert!(first.evicted() && !last.evicted());
    }
    #[test]
    fn a_used_lease_ages_behind_older_equal_value_leases_and_inflation_ranks_new_offers() {
        let (heap, notice) = (Arc::new(Heap::default()), Arc::new(AtomicBool::new(false)));
        let leases = leases(&heap, &notice, &[(4, 4), (4, 4), (4, 4)]);
        leases[0].offer(4, rebuild_value(4, 4));
        assert_eq!(heap.evict(4, Class::Fallback), 4);
        assert!(leases[1].evicted());
        let cheap = heap.lease(&notice);
        cheap.offer(4, 0);
        assert_eq!(heap.evict(8, Class::Fallback), 4, "the first mark is still pending");
        assert!(leases[2].evicted() && !cheap.evicted(), "a new cheap offer starts at the inflated priority");
    }
    #[test]
    fn held_demanded_and_guarded_leases_are_skipped_and_stay_offered() {
        let (heap, notice) = (Arc::new(Heap::default()), Arc::new(AtomicBool::new(false)));
        let leases = leases(&heap, &notice, &[(4, 0), (4, 0), (4, 0), (4, 8)]);
        heap.begin_frame();
        leases[0].hold();
        leases[1].stamp(Class::Visible);
        let pin: Arc<dyn Send + Sync> = Arc::new(());
        leases[2].guard(Arc::downgrade(&pin));
        assert_eq!(heap.evict(16, Class::Visible), 4);
        assert!(leases[3].evicted() && leases[..3].iter().all(|lease| !lease.evicted()));
        drop(pin);
        assert_eq!(heap.evict(16, Class::Authored), 8, "a more important requester takes demanded leases");
        assert!(!leases[0].evicted());
    }
    #[test]
    fn a_stamp_from_an_earlier_frame_does_not_protect() {
        let (heap, notice) = (Arc::new(Heap::default()), Arc::new(AtomicBool::new(false)));
        let leases = leases(&heap, &notice, &[(4, 0), (4, 0)]);
        leases.iter().for_each(|lease| lease.stamp(Class::Visible));
        heap.begin_frame();
        leases[0].stamp(Class::Visible);
        assert_eq!(heap.evict(8, Class::Fallback), 4);
        assert!(!leases[0].evicted() && leases[1].evicted());
    }
    #[test]
    fn repeated_use_keeps_the_heap_within_four_times_its_live_leases() {
        let (heap, notice) = (Arc::new(Heap::default()), Arc::new(AtomicBool::new(false)));
        let leases = leases(&heap, &notice, &[(4, 4); 8]);
        for round in 0..1000 { leases[round % 8].offer(4, 1); }
        assert!(heap.queue.lock().unwrap().entries.len() <= 4 * 8 + 64 + 1);
        drop(leases);
        assert_eq!(heap.queue.lock().unwrap().live, 0);
    }
}
