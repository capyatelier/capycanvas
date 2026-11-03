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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trim_completion_wakes_outside_the_state_lock() {
        use std::{sync::{Weak,mpsc},task::Wake,time::Duration};
        struct Observer(Weak<Completion>,mpsc::Sender<()>);
        impl Wake for Observer {
            fn wake(self:Arc<Self>) {assert!(self.0.upgrade().unwrap().state.try_lock().is_ok());self.1.send(()).unwrap();}
        }
        let mut wait=TrimWait::pending();let (send,receive)=mpsc::channel();
        let waker=Waker::from(Arc::new(Observer(Arc::downgrade(&wait.0),send)));
        assert!(Pin::new(&mut wait).poll(&mut Context::from_waker(&waker)).is_pending());
        wait.complete(Ok(()));receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(Pin::new(&mut wait).poll(&mut Context::from_waker(Waker::noop())),Poll::Ready(Ok(())));
    }
}
