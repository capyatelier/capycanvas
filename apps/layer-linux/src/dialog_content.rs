use gtk::{glib, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Content {
        pub body: gtk::Box,
        pub width: Cell<i32>,
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
            let (minimum, natural, min_baseline, nat_baseline) = self.body.measure(orientation, for_size);
            (if orientation == gtk::Orientation::Horizontal { 0 } else { minimum }, natural, min_baseline, nat_baseline)
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if width > 0 && self.width.replace(width) != width {
                if let Some(resized) = self.resized.borrow().as_ref() { resized(width); }
                let content = self.obj().downgrade();
                glib::idle_add_local_once(move || if let Some(content) = content.upgrade() { content.queue_resize(); });
            }
            self.body.allocate(width, height, baseline, None);
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) { self.obj().snapshot_child(&self.body, snapshot); }
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
