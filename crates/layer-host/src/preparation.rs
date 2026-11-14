use std::sync::{Arc,Mutex,Weak,atomic::{AtomicBool,Ordering}};

#[derive(Default)]
enum Notification {#[default] Unregistered,Installed(Arc<dyn Fn()+Send+Sync>),Closed}
impl Notification {
    fn callback(&self)->Option<Arc<dyn Fn()+Send+Sync>> {
        match self {Self::Installed(notify)=>Some(notify.clone()),_=>None}
    }
}
#[derive(Default)]
struct Registration {generation:u64,notify:Notification}
#[derive(Default)]
struct State {registration:Mutex<Registration>,pending:AtomicBool}
#[derive(Clone,Default)]
pub struct PreparationWake(Arc<State>);
impl PreparationWake {
    pub fn new(notify:Arc<dyn Fn()+Send+Sync>)->Self {
        let wake=Self::default();wake.set_notify(notify);wake
    }
    pub fn set_notify(&self,notify:Arc<dyn Fn()+Send+Sync>) {
        let pending={
            let mut registration=self.0.registration.lock().unwrap();
            registration.generation=registration.generation.wrapping_add(1);
            registration.notify=Notification::Installed(notify.clone());
            self.0.pending.load(Ordering::Acquire)
        };
        if pending {notify();}
    }
    pub fn install(&self,renderer:&mut layer_render_wgpu::WgpuRasterizer) {
        if let Some(wake)=self.callback() {renderer.set_preparation_wake(wake);}
    }
    pub fn signal(&self)->Arc<dyn Fn()+Send+Sync> {
        let state=Arc::downgrade(&self.0);
        Arc::new(move || {
            let Some(state)=state.upgrade() else {return;};
            let notify={
                let registration=state.registration.lock().unwrap();
                if matches!(registration.notify,Notification::Closed) {return;}
                state.pending.store(true,Ordering::Release);
                registration.notify.callback()
            };
            if let Some(notify)=notify {notify();}
        })
    }
    fn callback(&self)->Option<Arc<dyn Fn()+Send+Sync>> {
        let mut registration=self.0.registration.lock().unwrap();
        registration.notify.callback()?;
        registration.generation=registration.generation.wrapping_add(1);
        let generation=registration.generation;
        let state=Arc::downgrade(&self.0);
        Some(Arc::new(move ||Self::notify(&state,generation)))
    }
    fn notify(state:&Weak<State>,generation:u64) {
        let Some(state)=state.upgrade() else {return;};
        let notify={
            let registration=state.registration.lock().unwrap();
            if registration.generation!=generation {return;}
            let Some(notify)=registration.notify.callback() else {return;};
            state.pending.store(true,Ordering::Release);
            notify
        };
        notify();
    }
    pub fn take(&self)->bool {self.0.pending.swap(false,Ordering::AcqRel)}
    pub fn retire(&self) {
        let mut registration=self.0.registration.lock().unwrap();
        registration.generation=registration.generation.wrapping_add(1);self.0.pending.store(false,Ordering::Release);
    }
    pub fn close(&self) {
        let mut registration=self.0.registration.lock().unwrap();
        registration.generation=registration.generation.wrapping_add(1);
        registration.notify=Notification::Closed;self.0.pending.store(false,Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn completion_latches_demand_before_notification_and_retires_old_callbacks() {
        let calls=Arc::new(AtomicUsize::new(0));
        let wake=PreparationWake::default();
        let observer=wake.clone();let count=calls.clone();
        wake.set_notify(Arc::new(move || {assert!(observer.0.pending.load(Ordering::Acquire));count.fetch_add(1,Ordering::Relaxed);}));
        let old=wake.callback().unwrap();old();assert!(wake.take());assert!(!wake.take());
        let current=wake.callback().unwrap();old();assert!(!wake.take());
        current();assert!(wake.take());assert_eq!(calls.load(Ordering::Relaxed),2);
        wake.close();old();current();assert!(!wake.take());assert_eq!(calls.load(Ordering::Relaxed),2);
    }
    #[test]
    fn storage_completion_survives_renderer_retirement_and_stops_when_closed() {
        let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
        let wake=PreparationWake::new(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);}));
        let storage=wake.signal();let renderer=wake.callback().unwrap();wake.retire();
        renderer();assert!(!wake.take());storage();assert!(wake.take());
        wake.callback();storage();assert!(wake.take());assert_eq!(calls.load(Ordering::Relaxed),2);
        wake.close();storage();assert!(!wake.take());assert_eq!(calls.load(Ordering::Relaxed),2);
    }

    #[test]
    fn a_completion_before_notification_is_registered_stays_latched() {
        let wake=PreparationWake::default();let signal=wake.signal();signal();
        assert!(wake.0.pending.load(Ordering::Acquire));
        let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
        wake.set_notify(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);}));
        assert_eq!(calls.load(Ordering::Relaxed),1);
        assert!(wake.take());assert!(!wake.take());
    }
    #[test]
    fn installing_a_new_renderer_preserves_an_unconsumed_completion() {
        let wake=PreparationWake::new(Arc::new(||{}));let old=wake.callback().unwrap();
        old();let current=wake.callback().unwrap();assert!(wake.take());
        old();assert!(!wake.take());current();assert!(wake.take());
    }
    #[test]
    fn completion_notification_runs_outside_the_registration_lock() {
        let wake=PreparationWake::default();let observer=wake.clone();
        wake.set_notify(Arc::new(move || {assert!(observer.0.registration.try_lock().is_ok());}));
        wake.signal()();assert!(wake.take());
        wake.callback().unwrap()();assert!(wake.take());
        wake.signal()();let observer=wake.clone();
        wake.set_notify(Arc::new(move || {assert!(observer.0.registration.try_lock().is_ok());}));
        assert!(wake.take());
    }
}
