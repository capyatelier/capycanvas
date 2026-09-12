//! Poll non-Send workspace manager futures on the exclusive canvas owner.
//! Only the wake notification crosses threads; SQLite runs on StoreWorker.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

struct Signal {
    ready: AtomicBool,
    callback: Mutex<Option<Box<dyn Fn() + Send>>>,
}
impl Signal {
    fn deactivate(&self) {
        // Holding this lock across the callback makes returning from close a
        // boundary: no retained storage waker can touch the window afterwards.
        self.callback.lock().unwrap().take();
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        if let Some(callback) = self.callback.lock().unwrap().as_ref() {
            callback();
        }
    }
}

pub(crate) struct AsyncTask<T> {
    future: Option<Pin<Box<dyn Future<Output = T>>>>,
    signal: Arc<Signal>,
    closed: bool,
}
impl<T> AsyncTask<T> {
    pub(crate) fn new(wake: impl Fn() + Send + 'static) -> Self {
        Self {
            future: None,
            closed: false,
            signal: Arc::new(Signal {
                ready: AtomicBool::new(false),
                callback: Mutex::new(Some(Box::new(wake))),
            }),
        }
    }
    pub(crate) fn busy(&self) -> bool {
        self.future.is_some()
    }
    /// Rejecting a second operation must never replace an accepted save.
    pub(crate) fn start<F: Future<Output = T> + 'static>(&mut self, future: F) -> Result<(), F> {
        if self.closed || self.busy() {
            return Err(future);
        }
        self.future = Some(Box::pin(future));
        self.signal.ready.store(true, Ordering::Release);
        Ok(())
    }
    pub(crate) fn poll(&mut self) -> Option<T> {
        let future = self.future.as_mut()?;
        if !self.signal.ready.swap(false, Ordering::AcqRel) {
            return None;
        }
        let waker = Waker::from(self.signal.clone());
        match future.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Pending => None,
            Poll::Ready(value) => {
                self.future = None;
                Some(value)
            }
        }
    }
    /// Cancel an obsolete read. Accepted writes must be retained until completion.
    /// A late read wake may poll a newer future, but cannot deliver the old result.
    pub(crate) fn cancel_read(&mut self) {
        self.future = None;
        self.signal.ready.store(false, Ordering::Release);
    }
    /// Call outside the native window's notification mutex, before destroying
    /// its callback context. Normal close first waits for durable completion.
    pub(crate) fn close(&mut self) {
        self.signal.deactivate();
        self.closed = true;
        self.future = None;
    }
}
impl<T> Drop for AsyncTask<T> {
    fn drop(&mut self) {
        self.close();
    }
}

// Cold file transport is rare and bounded. Its owned I/O job never
// borrows the live session and is joined before a window callback can expire.
pub(crate) struct BlockingTask<T> {
    state: Arc<Mutex<BlockingState<T>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
struct BlockingState<T> {
    result: Option<Result<T, ()>>,
    waker: Option<Waker>,
}
impl<T: Send + 'static> BlockingTask<T> {
    pub(crate) fn start(job: impl FnOnce() -> T + Send + 'static) -> std::io::Result<Self> {
        let state = Arc::new(Mutex::new(BlockingState {
            result: None,
            waker: None,
        }));
        let worker = state.clone();
        let thread = std::thread::Builder::new()
            .name("capy-file-io".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).map_err(|_| ());
                let waker = {
                    let mut state = worker.lock().unwrap();
                    state.result = Some(result);
                    state.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            })?;
        Ok(Self {
            state,
            thread: Some(thread),
        })
    }
}
impl<T> Future for BlockingTask<T> {
    type Output = Result<T, String>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.result.take() {
            Poll::Ready(result.map_err(|()| "File I/O worker stopped.".into()))
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl<T> Drop for BlockingTask<T> {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        future::poll_fn,
        rc::Rc,
        sync::{atomic::AtomicUsize, mpsc},
    };

    #[test]
    fn a_pending_local_future_is_retained_and_only_polled_when_woken() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let count = notifications.clone();
        let mut task = AsyncTask::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        // Rc deliberately proves that manager futures never require Send.
        let polls = Rc::new(Cell::new(0));
        let complete = Rc::new(Cell::new(false));
        let saved_waker = Arc::new(Mutex::new(None));
        let (seen, done, wake) = (polls.clone(), complete.clone(), saved_waker.clone());
        assert!(
            task.start(poll_fn(move |cx| {
                seen.set(seen.get() + 1);
                *wake.lock().unwrap() = Some(cx.waker().clone());
                if done.get() {
                    Poll::Ready(42)
                } else {
                    Poll::Pending
                }
            }))
            .is_ok()
        );
        assert_eq!(task.poll(), None);
        for _ in 0..240 {
            assert_eq!(task.poll(), None);
        }
        assert_eq!(polls.get(), 1);
        assert!(task.start(async { 99 }).is_err());
        complete.set(true);
        let waker = saved_waker.lock().unwrap().clone().unwrap();
        std::thread::spawn(move || waker.wake()).join().unwrap();
        assert_eq!(notifications.load(Ordering::Relaxed), 1);
        assert_eq!(task.poll(), Some(42));
        assert_eq!(polls.get(), 2);
        assert!(!task.busy());
        task.close();
        assert!(task.start(async { 99 }).is_err());
    }

    #[test]
    fn close_waits_for_an_inflight_notification_and_disarms_retained_wakers() {
        let (entered, inside) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let task = AsyncTask::<()>::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
            entered.send(()).unwrap();
            released.recv().unwrap();
        });
        let signal = task.signal.clone();
        let retained = Waker::from(signal.clone());
        let worker_waker = retained.clone();
        let worker = std::thread::spawn(move || worker_waker.wake());
        inside
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // Close on another thread exercises the notification lock directly;
        // the local future and task itself remain on their exclusive owner.
        let closer = std::thread::spawn(move || signal.deactivate());
        release.send(()).unwrap();
        closer.join().unwrap();
        worker.join().unwrap();
        retained.wake_by_ref();
        drop(task);
        retained.wake();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
