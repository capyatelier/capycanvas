use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering::SeqCst}};

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;
enum Listener { Wake(Wake), Edge { _later: Hold } }

#[derive(Default)]
struct Cell { settled: AtomicBool, listeners: Mutex<Vec<Listener>> }
struct Producer(Arc<Cell>);
impl Drop for Producer {
    fn drop(&mut self) {
        let listeners = { let mut listeners = self.0.listeners.lock().unwrap(); self.0.settled.store(true, SeqCst); std::mem::take(&mut *listeners) };
        let mut woken: Vec<Wake> = Vec::new();
        for listener in listeners {
            if let Listener::Wake(wake) = listener && !woken.iter().any(|earlier| Arc::ptr_eq(earlier, &wake)) { wake(); woken.push(wake); }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Batch(Arc<Cell>);
impl std::fmt::Debug for Batch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("Batch").field(&self.settled()).finish() }
}
impl PartialEq for Batch {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}
impl Eq for Batch {}
impl Batch {
    pub(crate) fn settled(&self) -> bool { self.0.settled.load(SeqCst) }
    pub(crate) fn pending_with(&self, wake: Option<&Wake>) -> bool {
        let mut listeners = self.0.listeners.lock().unwrap();
        if self.settled() { return false; }
        if let Some(wake) = wake && !listeners.iter().any(|listener| matches!(listener, Listener::Wake(old) if Arc::ptr_eq(old, wake))) {
            listeners.push(Listener::Wake(wake.clone()));
        }
        true
    }
    fn join(&self, hold: Hold) -> Option<Hold> {
        let mut listeners = self.0.listeners.lock().unwrap();
        if self.settled() { return Some(hold); }
        listeners.push(Listener::Edge { _later: hold });
        None
    }
    pub(crate) fn joined(batches: impl IntoIterator<Item = Batch>) -> Option<Batch> {
        let (mut pending, settled): (Vec<_>, Vec<_>) = batches.into_iter().partition(|batch| !batch.settled());
        match pending.len() {
            0 => settled.into_iter().next(),
            1 => pending.pop(),
            _ => Some(Search::open(pending).seal()),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Hold { _producer: Arc<Producer> }

pub(crate) struct Search { batch: Batch, seal: Hold }
impl Search {
    pub(crate) fn open(earlier: impl IntoIterator<Item = Batch>) -> Self {
        let cell = Arc::new(Cell::default());
        let seal = Hold { _producer: Arc::new(Producer(cell.clone())) };
        for batch in earlier { drop(batch.join(seal.clone())); }
        Self { batch: Batch(cell), seal }
    }
    pub(crate) fn hold(&self) -> Hold { self.seal.clone() }
    pub(crate) fn batch(&self) -> Batch { self.batch.clone() }
    pub(crate) fn seal(self) -> Batch { let Self { batch, seal } = self; drop(seal); batch }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn counter() -> (Wake, Arc<std::sync::atomic::AtomicUsize>) {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = count.clone();
        (Arc::new(move || { counted.fetch_add(1, SeqCst); }), count)
    }
    #[test]
    fn an_empty_search_settles_when_it_seals_and_a_late_reader_reads_the_result() {
        let batch = Search::open([]).seal();
        let (wake, count) = counter();
        assert!(batch.settled() && !batch.pending_with(Some(&wake)));
        assert_eq!(count.load(SeqCst), 0, "a settled batch is read, not waited on");
    }
    #[test]
    fn a_search_that_settles_before_its_consumer_joins_it_is_kept_for_the_grant_only_check() {
        let search = Search::open([]);
        let hold = search.hold();
        let own = search.seal();
        let earlier = Search::open([]);
        let pending = earlier.hold();
        let earlier = earlier.seal();
        drop(hold);
        let joined = Batch::joined([own.clone()]).expect("a search that already retired its victims is not a definitive miss");
        assert!(joined.settled(), "its consumer reads the settled search and makes one grant-only check");
        let both = Batch::joined([own, earlier]).expect("a settled search joined with a pending one waits for the pending one");
        assert!(!both.settled());
        drop(pending);
        assert!(both.settled());
    }
    #[test]
    fn a_batch_settles_once_after_its_last_hold_and_wakes_each_reader_once() {
        let search = Search::open([]);
        let [first, second] = [search.hold(), search.hold()];
        let batch = search.seal();
        let (wake, count) = counter();
        assert!(batch.pending_with(Some(&wake)) && batch.pending_with(Some(&wake)), "a reader registers once however often it asks");
        drop(first);
        assert!(!batch.settled());
        let reader = batch.clone();
        drop(second);
        assert!(batch.settled() && reader.settled());
        assert_eq!(count.load(SeqCst), 1);
    }
    #[test]
    fn a_later_batch_that_joins_an_earlier_one_waits_for_both_and_never_extends_it() {
        let earlier = Search::open([]);
        let held = earlier.hold();
        let earlier = earlier.seal();
        let later = Search::open([earlier.clone()]);
        let own = later.hold();
        let later = later.seal();
        drop(own);
        assert!(!later.settled(), "the later batch waits on the earlier retirement it joined");
        let (wake, count) = counter();
        assert!(earlier.pending_with(Some(&wake)) && later.pending_with(Some(&wake)));
        drop(held);
        assert!(earlier.settled() && later.settled());
        assert_eq!(count.load(SeqCst), 2, "each settled batch posts its readers once");
    }
    #[test]
    fn a_reader_that_registers_while_the_last_hold_drops_on_another_thread_is_woken_or_reads_settled() {
        for _ in 0..200 {
            let search = Search::open([]);
            let hold = search.hold();
            let batch = search.seal();
            let (wake, count) = counter();
            let dropper = std::thread::spawn(move || drop(hold));
            let pending = batch.pending_with(Some(&wake));
            dropper.join().unwrap();
            assert!(batch.settled());
            assert_eq!(count.load(SeqCst), usize::from(pending), "a registration that saw the batch pending is woken exactly once");
        }
    }
}
