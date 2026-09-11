//! In-process pipeline recipes. No disk cache; each device starts uncompiled.
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
type Factory<T> = Box<dyn FnOnce() -> T + Send>;
#[cfg(target_arch = "wasm32")]
type Factory<T> = Box<dyn FnOnce() -> T>;

struct Inner<T> {
    value: OnceLock<T>,
    #[cfg(target_arch = "wasm32")]
    validating: std::cell::Cell<bool>,
    factory: Mutex<Option<Factory<T>>>,
    priority: std::sync::atomic::AtomicU8,
}
pub(super) struct Deferred<T>(Arc<Inner<T>>);
impl<T> Clone for Deferred<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Deferred<T> {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(factory: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::boxed(Box::new(factory))
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new(factory: impl FnOnce() -> T + 'static) -> Self {
        Self::boxed(Box::new(factory))
    }
    fn boxed(factory: Factory<T>) -> Self {
        Self(Arc::new(Inner {
            value: OnceLock::new(),
            #[cfg(target_arch = "wasm32")]
            validating: std::cell::Cell::new(false),
            factory: Mutex::new(Some(factory)),
            priority: std::sync::atomic::AtomicU8::new(u8::MAX),
        }))
    }
    pub fn compile(&self) -> &T {
        self.0.value.get_or_init(|| {
            let factory = self
                .0
                .factory
                .lock()
                .unwrap()
                .take()
                .expect("pipeline recipe");
            factory()
        })
    }
    pub fn ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.0.validating.get() {
            return false;
        }
        self.0.value.get().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn validating(&self, value: bool) {
        self.0.validating.set(value);
    }
    pub fn promote(&self, priority: u8) -> bool {
        !self.ready()
            && self
                .0
                .priority
                .fetch_min(priority, std::sync::atomic::Ordering::Relaxed)
                > priority
    }
}
impl<T> std::ops::Deref for Deferred<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.compile()
    }
}
