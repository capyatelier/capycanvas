use std::sync::Arc;

pub(crate) struct Pin(Arc<dyn Fn()+Send+Sync>);
impl Pin {
    pub(crate) fn new(release:Arc<dyn Fn()+Send+Sync>)->Arc<Self> {Arc::new(Self(release))}
}
impl Drop for Pin {fn drop(&mut self) {(self.0)();}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_consumer_release_notifies_once() {
        let (send,receive)=std::sync::mpsc::channel();
        let pin=Pin::new(Arc::new(move||send.send(()).unwrap()));
        let weak=Arc::downgrade(&pin);let held=pin.clone();
        drop(pin);assert!(receive.try_recv().is_err());assert!(weak.upgrade().is_some());
        drop(held);receive.recv().unwrap();assert!(weak.upgrade().is_none());assert!(receive.try_recv().is_err());
    }
}
