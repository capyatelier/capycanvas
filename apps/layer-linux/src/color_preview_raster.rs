//! Bounded native raster work: one worker and one replaceable latest request.
//! Only immutable color inputs/textures cross threads; widgets stay on GTK.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn current<K: PartialEq>(wanted: K, result: K) -> bool { wanted == result }

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
        Self::start(self.0.clone(), widget, work, install);
    }
    fn start<W: IsA<gtk::Widget>, R: Send + 'static>(
        queue: Rc<RefCell<Queue<K, Q>>>,
        widget: &W,
        work: fn(Q) -> R,
        install: fn(&W, K, R),
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
                && pending.wanted.is_some_and(|wanted| current(wanted, key));
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
                install(&widget, key, result);
                widget.queue_draw();
            }
            Self::start(queue, &widget, work, install);
        });
    }
}

#[cfg(test)]
pub(crate) fn assert_current_pixels(admitted: bool, old: &gtk::gdk::Texture, current: &gtk::gdk::Texture) {
    let width = old.width() as usize;
    assert_eq!((old.width(), old.height()), (current.width(), current.height()));
    let mut old_pixels = vec![0; width * old.height() as usize * 4];
    let mut current_pixels = old_pixels.clone();
    old.download(&mut old_pixels, width * 4);
    current.download(&mut current_pixels, width * 4);
    let index = old_pixels.chunks_exact(4).zip(current_pixels.chunks_exact(4)).enumerate()
        .max_by_key(|(_, (a, b))| a[..3].iter().zip(&b[..3]).map(|(a,b)|a.abs_diff(*b) as u32).sum::<u32>()).unwrap().0 * 4;
    let old = &old_pixels[index..index+4];
    let current = &current_pixels[index..index+4];
    assert!(old[..3].iter().zip(&current[..3]).any(|(a,b)| a.abs_diff(*b) >= 24),
        "the two input values must produce visibly different pixels: {old:?} / {current:?}");
    if admitted { assert_eq!(old, current, "obsolete raster admitted under newer input at pixel {}", index/4); }
}
