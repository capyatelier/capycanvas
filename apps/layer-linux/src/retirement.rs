use std::{cell::{Cell, RefCell}, future::Future, pin::Pin, rc::Rc, task::{Context, Poll, Waker}};

#[derive(Clone, Default)]
pub(crate) struct Retirement(Rc<Completion>);

#[derive(Default)]
struct Completion {
    done: Cell<bool>,
    waiters: RefCell<Vec<Waker>>,
}

impl Retirement {
    pub(crate) fn is_complete(&self) -> bool { self.0.done.get() }
    pub(crate) fn complete(&self) {
        if !self.0.done.replace(true) {
            for waiter in self.0.waiters.take() { waiter.wake(); }
        }
    }
}

impl Future for Retirement {
    type Output = ();
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.is_complete() { return Poll::Ready(()); }
        let mut waiters = self.0.waiters.borrow_mut();
        if !waiters.iter().any(|waiter| waiter.will_wake(context.waker())) {
            waiters.push(context.waker().clone());
        }
        Poll::Pending
    }
}

#[derive(Default)]
pub(crate) struct Owners {
    count: Cell<usize>,
    closing: Cell<bool>,
    drained: Retirement,
}

pub(crate) struct Owner(Rc<Owners>);

impl Owners {
    pub(crate) fn acquire(self: &Rc<Self>) -> Result<Owner, String> {
        if self.closing.get() { return Err("The canvas window is closing".into()); }
        self.count.set(self.count.get() + 1);
        Ok(Owner(self.clone()))
    }
    pub(crate) fn close(&self) -> Option<Retirement> {
        if self.closing.replace(true) { return None; }
        if self.count.get() == 0 { self.drained.complete(); }
        Some(self.drained.clone())
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.count.set(self.0.count.get() - 1);
        if self.0.closing.get() && self.0.count.get() == 0 { self.0.drained.complete(); }
    }
}

pub(crate) fn reap<T: Send + 'static, P: 'static>(thread: std::thread::JoinHandle<()>, resources: T, parent: P) -> Retirement {
    let completion = Retirement::default();
    let finished = completion.clone();
    let cleanup = gtk::gio::spawn_blocking(move || {
        let _ = thread.join();
        drop(resources);
    });
    gtk::glib::spawn_future_local(async move {
        let _ = cleanup.await;
        drop(parent);
        finished.complete();
    });
    completion
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn retirement_keeps_parent_until_worker_and_gpu_cleanup_exit() {
        let context = gtk::glib::MainContext::new();
        context.with_thread_default(|| {
            let owners = Rc::new(Owners::default());
            let parent = owners.acquire().unwrap();
            let (exit, wait) = mpsc::channel();
            let (destroyed, cleanup) = mpsc::channel();
            struct Resource(mpsc::Sender<std::thread::ThreadId>);
            impl Drop for Resource { fn drop(&mut self) { self.0.send(std::thread::current().id()).unwrap(); } }
            let thread = std::thread::spawn(move || { wait.recv().unwrap(); });
            let retired = reap(thread, Resource(destroyed), parent);
            let drained = owners.close().unwrap();
            assert!(!retired.is_complete());
            assert!(!drained.is_complete());
            assert!(owners.acquire().is_err());
            assert!(owners.close().is_none());
            assert!(cleanup.try_recv().is_err());
            exit.send(()).unwrap();
            context.block_on(retired.clone());
            assert!(retired.is_complete());
            assert!(drained.is_complete());
            assert_ne!(cleanup.recv().unwrap(), std::thread::current().id());
        }).unwrap();
    }

    #[test]
    fn native_parent_waits_for_every_retiring_generation() {
        let owners = Rc::new(Owners::default());
        let first = owners.acquire().unwrap();
        let second = owners.acquire().unwrap();
        let drained = owners.close().unwrap();
        drop(first);
        assert!(!drained.is_complete());
        drop(second);
        assert!(drained.is_complete());
    }
}
