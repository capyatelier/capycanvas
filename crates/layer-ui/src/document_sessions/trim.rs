use std::{future::Future, pin::Pin, sync::{Arc, Mutex}, task::{Context, Poll, Waker}};

type CompletionState = (Option<Result<(), String>>, Vec<Waker>);
#[derive(Default)]
struct Completion {
    state: Mutex<CompletionState>,
    #[cfg(not(target_arch = "wasm32"))]
    wake: std::sync::Condvar,
}
impl Completion {
    fn finish(&self, result: Result<(), String>) {
        let waiters = {
            let mut state = self.state.lock().unwrap();
            state.0 = Some(result);
            std::mem::take(&mut state.1)
        };
        #[cfg(not(target_arch = "wasm32"))]
        self.wake.notify_all();
        for waiter in waiters { waiter.wake(); }
    }
}

#[derive(Clone)]
pub struct TrimWait(Arc<Completion>);
impl Future for TrimWait {
    type Output = Result<(), String>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.state.lock().unwrap();
        if let Some(result) = &state.0 { return Poll::Ready(result.clone()); }
        if !state.1.iter().any(|waiter| waiter.will_wake(context.waker())) { state.1.push(context.waker().clone()); }
        Poll::Pending
    }
}
impl TrimWait {
    pub(super) fn pending()->Self {Self(Arc::default())}
    pub(super) fn complete(&self,result:Result<(),String>) {self.0.finish(result);}
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_on_worker(&self) -> Result<(), String> {
        let state = self.0.state.lock().unwrap();
        self.0.wake.wait_while(state, |state| state.0.is_none()).unwrap().0.as_ref().unwrap().clone()
    }
}

#[cfg(any(target_arch="wasm32",test))]
#[derive(Default)]
pub(super) struct Trim {
    active: Option<(u64, Arc<Completion>)>,
    pending: bool,
}
#[cfg(any(target_arch="wasm32",test))]
impl Trim {
    pub fn request(&mut self) -> Option<u64> {
        if self.active.is_some() { self.pending = true; return None; }
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
            .expect("Document trim identity exhausted");
        self.active = Some((token, Arc::default()));
        Some(token)
    }
    pub fn matches(&self, token: u64) -> bool { self.active.as_ref().is_some_and(|(current, _)| *current == token) }
    #[cfg(target_arch="wasm32")]
    pub fn in_flight(&self) -> bool { self.active.is_some() }
    pub fn wait(&self) -> Option<TrimWait> { self.active.as_ref().map(|(_, completion)| TrimWait(completion.clone())) }
    pub fn complete(&mut self, token: u64, result: Result<(), String>) -> bool {
        if !self.matches(token) { return false; }
        if result.is_ok() && std::mem::take(&mut self.pending) { return true; }
        self.pending = false;
        self.active.take().unwrap().1.finish(result);
        false
    }
}
#[cfg(any(target_arch="wasm32",test))]
impl Drop for Trim {
    fn drop(&mut self) {
        if let Some((_, completion)) = self.active.take() { completion.finish(Err("Drawing storage task stopped".into())); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poll(wait: &mut TrimWait) -> Poll<Result<(), String>> {
        Pin::new(wait).poll(&mut Context::from_waker(Waker::noop()))
    }
    #[test]
    fn trim_coalesces_requests_and_resolves_every_waiter() {
        let mut trim = Trim::default();
        let token = trim.request().unwrap();
        let mut first = trim.wait().unwrap();
        for _ in 0..32 { assert_eq!(trim.request(), None); }
        let mut second = trim.wait().unwrap();
        assert!(poll(&mut first).is_pending());
        assert!(trim.complete(token, Ok(())));
        assert!(poll(&mut second).is_pending());
        assert!(!trim.complete(token, Ok(())));
        assert_eq!(poll(&mut first), Poll::Ready(Ok(())));
        assert_eq!(poll(&mut second), Poll::Ready(Ok(())));
        let next = trim.request().unwrap();
        assert_ne!(token, next);
        assert!(!trim.complete(token, Ok(())));
        assert!(trim.matches(next));
        let replacement = Trim::default().request().unwrap();
        assert_ne!(next, replacement);
    }
    #[test]
    fn trim_completion_wakes_outside_the_state_lock() {
        use std::{sync::{Weak, mpsc}, task::Wake, time::Duration};
        struct Observer(Weak<Completion>, mpsc::Sender<()>);
        impl Wake for Observer {
            fn wake(self: Arc<Self>) {
                assert!(self.0.upgrade().unwrap().state.try_lock().is_ok());
                self.1.send(()).unwrap();
            }
        }
        let mut trim = Trim::default();
        let token = trim.request().unwrap();
        let mut wait = trim.wait().unwrap();
        let (signal, wake) = mpsc::channel();
        let waker = Waker::from(Arc::new(Observer(Arc::downgrade(&wait.0), signal)));
        assert!(Pin::new(&mut wait).poll(&mut Context::from_waker(&waker)).is_pending());
        assert!(!trim.complete(token, Ok(())));
        wake.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(poll(&mut wait), Poll::Ready(Ok(())));
    }
    #[test]
    fn trim_failure_and_owner_drop_release_waiters() {
        let mut trim = Trim::default();
        let token = trim.request().unwrap();
        let mut failed = trim.wait().unwrap();
        trim.request();
        assert!(!trim.complete(token, Err("disk full".into())));
        assert_eq!(poll(&mut failed), Poll::Ready(Err("disk full".into())));
        trim.request();
        let mut cancelled = trim.wait().unwrap();
        drop(trim);
        assert!(matches!(poll(&mut cancelled), Poll::Ready(Err(_))));
    }
}
