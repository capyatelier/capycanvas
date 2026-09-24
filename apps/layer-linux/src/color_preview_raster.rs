//! Bounded native raster work: one worker and one replaceable latest request.
//! Only immutable color inputs/textures cross threads; widgets stay on GTK.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

pub(crate) struct PreviewRaster<K, Q>(Rc<RefCell<Queue<K, Q>>>);
struct Queue<K, Q> {
    wanted: Option<K>,
    queued: Option<(K, Q)>,
    running: bool,
    generation: u64,
}
impl<K, Q> Default for PreviewRaster<K, Q> {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(Queue {
            wanted: None,
            queued: None,
            running: false,
            generation: 0,
        })))
    }
}
impl<K, Q> PreviewRaster<K, Q> {
    pub fn cancel(&self) {
        let mut queue = self.0.borrow_mut();
        queue.generation = queue.generation.wrapping_add(1);
        queue.wanted = None;
        queue.queued = None;
    }
}
impl<K: Copy + PartialEq + 'static, Q: Send + 'static> PreviewRaster<K, Q> {
    pub fn request<W: IsA<gtk::Widget>, R: Send + 'static>(
        &self,
        widget: &W,
        key: K,
        input: impl FnOnce() -> Option<Q>,
        work: fn(Q) -> R,
        install: fn(&W, K, R),
        compatible: fn(K, K) -> bool,
    ) {
        let mut queue = self.0.borrow_mut();
        if queue.wanted == Some(key) {
            return;
        }
        queue.wanted = Some(key);
        queue.queued = input().map(|input| (key, input));
        if queue.queued.is_none() {
            // The desired raster is already cached. Older jobs must not replace it.
            queue.generation = queue.generation.wrapping_add(1);
        }
        drop(queue);
        Self::start(self.0.clone(), widget, work, install, compatible);
    }
    fn start<W: IsA<gtk::Widget>, R: Send + 'static>(
        queue: Rc<RefCell<Queue<K, Q>>>,
        widget: &W,
        work: fn(Q) -> R,
        install: fn(&W, K, R),
        compatible: fn(K, K) -> bool,
    ) {
        let mut pending = queue.borrow_mut();
        if pending.running {
            return;
        }
        let Some((key, input)) = pending.queued.take() else {
            return;
        };
        pending.running = true;
        let generation = pending.generation;
        drop(pending);
        let weak = widget.downgrade();
        glib::MainContext::default().spawn_local(async move {
            let result = gtk::gio::spawn_blocking(move || work(input)).await;
            let Some(widget) = weak.upgrade() else { return };
            let mut pending = queue.borrow_mut();
            pending.running = false;
            if !widget.is_mapped() {
                pending.generation = pending.generation.wrapping_add(1);
                pending.queued = None;
                pending.wanted = None;
            }
            let accept = pending.generation == generation
                && pending.wanted.is_some_and(|wanted| compatible(wanted, key));
            if result.is_err() {
                pending.wanted = None;
            }
            if accept
                && pending
                    .queued
                    .as_ref()
                    .is_some_and(|(next, _)| *next == key)
            {
                pending.queued = None;
            }
            drop(pending);
            if let Ok(result) = result
                && accept
            {
                // Publish completed colors during motion without starving on an
                // exact-key match; cancellation invalidates the whole generation.
                install(&widget, key, result);
                widget.queue_draw();
            }
            Self::start(queue, &widget, work, install, compatible);
        });
    }
}
