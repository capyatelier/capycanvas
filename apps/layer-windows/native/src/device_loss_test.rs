use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

struct State {
    remaining: usize,
    result: Option<Result<(), String>>,
}
struct Removal {
    state: Mutex<State>,
    ready: Condvar,
}
impl Removal {
    fn new(participants: usize) -> Self {
        Self { state: Mutex::new(State { remaining: participants, result: None }), ready: Condvar::new() }
    }
    fn arrive(&self, timeout: Duration, remove: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.result.is_none() {
            state.remaining -= 1;
            if state.remaining == 0 {
                state.result = Some(remove());
                self.ready.notify_all();
            } else {
                self.ready.notify_all();
                let (next, _) = self.ready.wait_timeout_while(state, timeout, |s| s.result.is_none()).unwrap();
                state = next;
                if state.result.is_none() {
                    state.result = Some(Err("Device-loss injection timed out waiting for the other windows".into()));
                    self.ready.notify_all();
                }
            }
        }
        state.result.clone().unwrap()
    }
}

pub(crate) fn together(token: u64, participants: usize, remove: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    static GROUPS: OnceLock<Mutex<HashMap<u64, Arc<Removal>>>> = OnceLock::new();
    if participants == 0 { return Err("Device-loss injection requires a window".into()); }
    let groups = GROUPS.get_or_init(Default::default);
    let group = groups.lock().unwrap().entry(token).or_insert_with(|| Arc::new(Removal::new(participants))).clone();
    let result = group.arrive(Duration::from_secs(10), remove);
    groups.lock().unwrap().remove(&token);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_waits_for_every_window_and_shares_the_result() {
        for expected in [Ok(()), Err("removal failed".into())] {
            let group = Arc::new(Removal::new(2));
            let first = group.clone();
            let worker = std::thread::spawn(move || first.arrive(Duration::from_secs(5), || panic!("Another window is still rendering")));
            let state = group.state.lock().unwrap();
            let (state, wait) = group.ready.wait_timeout_while(state, Duration::from_secs(5), |s| s.remaining == 2).unwrap();
            assert!(!wait.timed_out());
            assert!(state.result.is_none());
            drop(state);
            assert_eq!(group.arrive(Duration::from_secs(5), || expected.clone()), expected);
            assert_eq!(worker.join().unwrap(), expected);
        }
    }

    #[test]
    fn a_missing_window_aborts_removal_and_late_arrivals_observe_failure() {
        let group = Removal::new(2);
        let error = group.arrive(Duration::ZERO, || panic!("Missing window")).unwrap_err();
        assert_eq!(group.arrive(Duration::ZERO, || panic!("Already aborted")).unwrap_err(), error);
        assert!(together(1, 0, || panic!("No windows")).is_err());
        assert!(together(2, 1, || Ok(())).is_ok());
        assert!(together(2, 1, || Err("fresh group".into())).is_err());
    }
}
