use gtk::prelude::*;
use std::{cell::RefCell, sync::OnceLock};

#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct Counts {
    pub(crate) snapshots: usize,
    queue_draw: usize,
    queue_allocate: usize,
    queue_resize: usize,
    allocate: usize,
    css_calls: usize,
    css_changes: usize,
}

#[derive(serde::Serialize)]
pub(crate) struct Event { phase: usize, operation: &'static str, widget: usize, css_changed: bool }

#[derive(Default)]
struct State { active: bool, depth: usize, phase: usize, counts: [Counts; 4], events: Vec<Event> }
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub(crate) struct SnapshotScope;
impl SnapshotScope {
    pub(crate) fn enter() -> Self {
        STATE.with_borrow_mut(|state| {
            state.depth += 1;
            if state.active && state.depth == 1 { state.counts[state.phase].snapshots += 1; }
        });
        Self
    }
}
impl Drop for SnapshotScope {
    fn drop(&mut self) { STATE.with_borrow_mut(|state| state.depth -= 1); }
}

pub(crate) struct Audit;
#[derive(serde::Serialize)]
pub(crate) struct Report { pub(crate) counts: [Counts; 4], pub(crate) events: Vec<Event> }
impl Audit {
    pub(crate) fn start() -> Self {
        STATE.with_borrow_mut(|state| { assert!(!state.active); *state = State {active:true, ..State::default()}; });
        Self
    }
    pub(crate) fn phase(&self, phase: usize) { STATE.with_borrow_mut(|state| state.phase = phase); }
    pub(crate) fn finish(self) -> Report {
        STATE.with_borrow_mut(|state| Report {counts:state.counts.clone(), events:std::mem::take(&mut state.events)})
    }
}
impl Drop for Audit {
    fn drop(&mut self) { STATE.with_borrow_mut(|state| state.active = false); }
}

fn record(widget: *mut gtk::ffi::GtkWidget, operation: &'static str, css_changed: bool) {
    STATE.with_borrow_mut(|state| {
        if !state.active || state.depth == 0 { return; }
        let counts = &mut state.counts[state.phase];
        match operation {
            "queue_draw" => counts.queue_draw += 1,
            "queue_allocate" => counts.queue_allocate += 1,
            "queue_resize" => counts.queue_resize += 1,
            "allocate" => counts.allocate += 1,
            _ => { counts.css_calls += 1; counts.css_changes += usize::from(css_changed); },
        }
        state.events.push(Event {phase:state.phase, operation, widget:widget as usize, css_changed});
    });
}

fn classes(widget: *mut gtk::ffi::GtkWidget) -> Option<Vec<String>> {
    if widget.is_null() || !STATE.with_borrow(|state| state.active && state.depth > 0) { return None; }
    let widget: glib::translate::Borrowed<gtk::Widget> = unsafe { glib::translate::from_glib_borrow(widget) };
    Some(widget.css_classes().iter().map(ToString::to_string).collect())
}

fn address(name: &'static [u8]) -> usize {
    let address = unsafe { libc::dlsym(libc::RTLD_NEXT, name.as_ptr().cast()) } as usize;
    assert_ne!(address, 0, "GTK audit forwarding symbol");
    address
}

macro_rules! forward {
    ($name:ident, $operation:literal, $css:literal $(, $arg:ident: $type:ty)*) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name(widget: *mut gtk::ffi::GtkWidget $(, $arg: $type)*) {
            static ORIGINAL: OnceLock<usize> = OnceLock::new();
            let original: unsafe extern "C" fn(*mut gtk::ffi::GtkWidget $(, $type)*) = unsafe {
                std::mem::transmute(*ORIGINAL.get_or_init(|| address(concat!(stringify!($name), "\0").as_bytes())))
            };
            let before = if $css { classes(widget) } else { None };
            if !$css { record(widget, $operation, false); }
            unsafe { original(widget $(, $arg)*); }
            if $css { record(widget, $operation, before.is_some_and(|before| Some(before) != classes(widget))); }
        }
    };
}
forward!(gtk_widget_queue_draw, "queue_draw", false);
forward!(gtk_widget_queue_allocate, "queue_allocate", false);
forward!(gtk_widget_queue_resize, "queue_resize", false);
forward!(gtk_widget_allocate, "allocate", false, width: i32, height: i32, baseline: i32, transform: *mut gtk::gsk::ffi::GskTransform);
forward!(gtk_widget_add_css_class, "add_css_class", true, class: *const std::ffi::c_char);
forward!(gtk_widget_remove_css_class, "remove_css_class", true, class: *const std::ffi::c_char);
forward!(gtk_widget_set_css_classes, "set_css_classes", true, css_classes: *const *const std::ffi::c_char);

#[test]
fn gtk_snapshot_audit_forwards_the_intercepted_entry_points() {
    for (binding, replacement, name) in [
        (gtk::ffi::gtk_widget_queue_draw as *const () as usize, gtk_widget_queue_draw as *const () as usize, c"gtk_widget_queue_draw"),
        (gtk::ffi::gtk_widget_queue_allocate as *const () as usize, gtk_widget_queue_allocate as *const () as usize, c"gtk_widget_queue_allocate"),
        (gtk::ffi::gtk_widget_queue_resize as *const () as usize, gtk_widget_queue_resize as *const () as usize, c"gtk_widget_queue_resize"),
        (gtk::ffi::gtk_widget_allocate as *const () as usize, gtk_widget_allocate as *const () as usize, c"gtk_widget_allocate"),
        (gtk::ffi::gtk_widget_add_css_class as *const () as usize, gtk_widget_add_css_class as *const () as usize, c"gtk_widget_add_css_class"),
        (gtk::ffi::gtk_widget_remove_css_class as *const () as usize, gtk_widget_remove_css_class as *const () as usize, c"gtk_widget_remove_css_class"),
        (gtk::ffi::gtk_widget_set_css_classes as *const () as usize, gtk_widget_set_css_classes as *const () as usize, c"gtk_widget_set_css_classes"),
    ] {
        assert_eq!(binding, replacement);
        assert_ne!(address(name.to_bytes_with_nul()), replacement);
    }
}
