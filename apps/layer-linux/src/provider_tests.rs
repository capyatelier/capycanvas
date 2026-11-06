use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider { Export, Clipboard, Color }

struct Pending {
    id: Arc<()>,
    provider: Provider,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
static NEXT: Mutex<Option<Pending>> = Mutex::new(None);

pub(crate) struct Hold {
    id: Arc<()>,
    entered: mpsc::Receiver<()>,
    release: Option<mpsc::Sender<()>>,
    started: bool,
}
impl Hold {
    pub(crate) fn started(&mut self) -> bool {
        self.started |= match self.entered.try_recv() {
            Ok(()) => true,
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => self.started,
        };
        self.started
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        self.release.take();
        let mut next = NEXT.lock().unwrap();
        if next.as_ref().is_some_and(|next| Arc::ptr_eq(&next.id, &self.id)) { next.take(); }
    }
}

pub(crate) fn hold(provider: Provider) -> Hold {
    let (entered, receiver) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut next = NEXT.lock().unwrap();
    assert!(next.is_none(), "one provider hold at a time");
    let id = Arc::new(());
    *next = Some(Pending { id: id.clone(), provider, entered, release: wait });
    Hold { id, entered: receiver, release: Some(release), started: false }
}
pub(crate) fn enter(provider: Provider) {
    let pending = {
        let mut next = NEXT.lock().unwrap();
        if next.as_ref().is_none_or(|next| next.provider != provider) { return; }
        next.take().unwrap()
    };
    if pending.entered.send(()).is_ok() { let _ = pending.release.recv(); }
}

#[test]
fn only_the_selected_provider_is_held_and_release_is_one_shot() {
    let mut gate = hold(Provider::Export);
    enter(Provider::Clipboard);
    assert!(!gate.started());
    let worker = std::thread::spawn(|| enter(Provider::Export));
    gate.entered.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(!worker.is_finished());
    drop(gate);
    worker.join().unwrap();
    enter(Provider::Export);
}
