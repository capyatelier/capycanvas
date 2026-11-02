use gtk::{glib, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Content {
        pub body: gtk::Box,
        pub preparing: Cell<bool>,
        pub resized: RefCell<Option<Box<dyn Fn(i32)>>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Content {
        const NAME: &'static str = "CapyDialogContent";
        type Type = super::DialogContent;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Content {
        fn dispose(&self) { self.body.unparent(); }
    }
    impl WidgetImpl for Content {
        fn compute_expand(&self, hexpand: &mut bool, vexpand: &mut bool) {
            *hexpand = self.body.compute_expand(gtk::Orientation::Horizontal);
            *vexpand = self.body.compute_expand(gtk::Orientation::Vertical);
        }
        fn request_mode(&self) -> gtk::SizeRequestMode { gtk::SizeRequestMode::HeightForWidth }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical { self.prepare(for_size); }
            let (minimum, natural, min_baseline, nat_baseline) = self.body.measure(orientation, for_size);
            (if orientation == gtk::Orientation::Horizontal { 0 } else { minimum }, natural, min_baseline, nat_baseline)
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.prepare(width);
            self.body.allocate(width, height, baseline, None);
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) { self.obj().snapshot_child(&self.body, snapshot); }
    }
    impl Content {
        fn prepare(&self, width: i32) {
            if width <= 0 || self.preparing.replace(true) { return; }
            if let Some(resized) = self.resized.borrow().as_ref() { resized(width); }
            self.preparing.set(false);
        }
    }
}
glib::wrapper! {
    pub struct DialogContent(ObjectSubclass<imp::Content>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl DialogContent {
    pub fn orientation(&self) -> gtk::Orientation { self.imp().body.orientation() }
    pub fn append(&self, child: &impl IsA<gtk::Widget>) { self.imp().body.append(child); }
    pub fn set_orientation(&self, orientation: gtk::Orientation) { self.imp().body.set_orientation(orientation); }
    pub fn set_spacing(&self, spacing: i32) { self.imp().body.set_spacing(spacing); }
}
pub fn new(orientation: gtk::Orientation, spacing: i32) -> DialogContent {
    let content: DialogContent = glib::Object::new();
    content.imp().body.set_parent(&content);
    content.set_orientation(orientation);
    content.set_spacing(spacing);
    content
}
pub fn on_resize(content: &DialogContent, resized: impl Fn(i32) + 'static) {
    *content.imp().resized.borrow_mut() = Some(Box::new(resized));
}

#[cfg(test)]
#[path = "dialog_content_tests.rs"]
mod tests;
