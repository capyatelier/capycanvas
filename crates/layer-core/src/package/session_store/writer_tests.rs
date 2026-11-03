use super::*;
use std::{future::Future,pin::Pin,sync::atomic::AtomicUsize,task::{Context,Poll,Wake,Waker}};

#[derive(Default)]
struct Signal(AtomicUsize);
impl Wake for Signal {
    fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::SeqCst);}
    fn wake_by_ref(self:&Arc<Self>) {self.0.fetch_add(1,Ordering::SeqCst);}
}
fn poll<F:Future>(future:Pin<&mut F>,signal:&Arc<Signal>)->Poll<F::Output> {
    future.poll(&mut Context::from_waker(&Waker::from(signal.clone())))
}

#[test]
fn queued_writers_acquire_fifo_and_release_without_holding_the_state_lock() {
    let slot=Mutex::new(WriterSlot::default());let signal=Arc::new(Signal::default());
    let mut first=Box::pin(WriterLease::take(&slot));
    let Poll::Ready(Ok(first))=poll(first.as_mut(),&signal) else {panic!("first writer unavailable")};
    assert!(slot.try_lock().is_ok());
    let second_signal=Arc::new(Signal::default());let third_signal=Arc::new(Signal::default());
    let mut second=Box::pin(WriterLease::take(&slot));let mut third=Box::pin(WriterLease::take(&slot));
    assert!(poll(second.as_mut(),&second_signal).is_pending());
    assert!(poll(third.as_mut(),&third_signal).is_pending());
    drop(first);assert_eq!(second_signal.0.load(Ordering::SeqCst),1);assert_eq!(third_signal.0.load(Ordering::SeqCst),0);
    assert!(poll(third.as_mut(),&third_signal).is_pending());
    let Poll::Ready(Ok(second))=poll(second.as_mut(),&second_signal) else {panic!("second writer unavailable")};
    assert!(slot.try_lock().is_ok());drop(second);
    assert_eq!(third_signal.0.load(Ordering::SeqCst),1);
    let Poll::Ready(Ok(third))=poll(third.as_mut(),&third_signal) else {panic!("third writer unavailable")};
    drop(third);assert!(!slot.lock().unwrap().busy);assert!(slot.lock().unwrap().waiting.is_empty());
}

#[test]
fn cancelling_the_notified_front_writer_wakes_the_next_waiter() {
    let slot=Mutex::new(WriterSlot::default());let signal=Arc::new(Signal::default());
    let mut first=Box::pin(WriterLease::take(&slot));
    let Poll::Ready(Ok(first))=poll(first.as_mut(),&signal) else {panic!("first writer unavailable")};
    let mut cancelled=Box::pin(WriterLease::take(&slot));let mut next=Box::pin(WriterLease::take(&slot));
    assert!(poll(cancelled.as_mut(),&signal).is_pending());assert!(poll(next.as_mut(),&signal).is_pending());
    drop(first);let notified=signal.0.load(Ordering::SeqCst);drop(cancelled);
    assert_eq!(signal.0.load(Ordering::SeqCst),notified+1);
    let Poll::Ready(Ok(next))=poll(next.as_mut(),&signal) else {panic!("remaining writer stranded")};
    drop(next);assert!(slot.lock().unwrap().waiting.is_empty());
}

#[test]
fn cancelling_queued_writers_keeps_the_remaining_order() {
    let slot=Mutex::new(WriterSlot::default());let signal=Arc::new(Signal::default());
    let mut first=Box::pin(WriterLease::take(&slot));
    let Poll::Ready(Ok(first))=poll(first.as_mut(),&signal) else {panic!("first writer unavailable")};
    let mut second=Box::pin(WriterLease::take(&slot));let mut cancelled=Box::pin(WriterLease::take(&slot));let mut last=Box::pin(WriterLease::take(&slot));
    assert!(poll(second.as_mut(),&signal).is_pending());assert!(poll(cancelled.as_mut(),&signal).is_pending());assert!(poll(last.as_mut(),&signal).is_pending());
    drop(cancelled);drop(first);assert!(poll(last.as_mut(),&signal).is_pending());
    let Poll::Ready(Ok(second))=poll(second.as_mut(),&signal) else {panic!("second writer unavailable")};drop(second);
    let Poll::Ready(Ok(last))=poll(last.as_mut(),&signal) else {panic!("last writer unavailable")};drop(last);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_checkpoint_writer_waits_for_an_existing_store_operation() {
    struct Directory(PathBuf);
    impl Drop for Directory {fn drop(&mut self) {let _=std::fs::remove_dir_all(&self.0);}}
    let directory=Directory(std::env::temp_dir().join(format!("capy-writer-{}",PortableId::random())));
    let binding=DrawingStore::new(&directory.0,Some("drawing")).unwrap();
    let held=binding.clone();let (entered_tx,entered_rx)=std::sync::mpsc::channel();let (release_tx,release_rx)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move ||pollster::block_on(held.with_store(async |store| {
        entered_tx.send(store.path.clone()).unwrap();release_rx.recv().unwrap();
        Err::<(),_>("operation failed".to_owned())
    })));
    let path=entered_rx.recv().unwrap();
    let mut checkpoint=Box::pin(binding.with_store(async |store| {assert_eq!(store.path,path);Ok(store.generation())}));
    let signal=Arc::new(Signal::default());
    assert!(poll(checkpoint.as_mut(),&signal).is_pending());
    release_tx.send(()).unwrap();assert_eq!(worker.join().unwrap().unwrap_err(),"operation failed");
    assert_eq!(signal.0.load(Ordering::SeqCst),1);
    assert_eq!(pollster::block_on(checkpoint).unwrap(),None);
    assert!(!binding.0.writer.lock().unwrap().busy);
    drop(binding);
}
