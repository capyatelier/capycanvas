//! Native square controls; layout and wrapping belong to layer-ui.
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::{Axis, TILE_SIZE, tile_layout};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct TileStrip {
        pub children: RefCell<Vec<gtk::Widget>>,
        pub grip: RefCell<Option<gtk::Widget>>,
        pub vertical: Cell<bool>,
        pub tabbed: Cell<bool>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for TileStrip {
        const NAME: &'static str = "LayerTileStrip";
        type Type = super::TileStrip;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for TileStrip {
        fn dispose(&self) {
            for child in self.children.take() {
                child.unparent();
            }
            if let Some(grip) = self.grip.take() {
                grip.unparent();
            }
        }
    }
    impl WidgetImpl for TileStrip {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = TILE_SIZE as i32 + if self.tabbed.get() { 8 } else { 0 };
            (size, size, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let axis = if self.vertical.get() {
                Axis::Vertical
            } else {
                Axis::Horizontal
            };
            let children = self.children.borrow();
            let layout = tile_layout(
                width as f32,
                height as f32,
                axis,
                children.len(),
                !self.tabbed.get(),
            );
            let allocate = |child: &gtk::Widget, b: layer_ui::Bounds| {
                child.allocate(
                    b.width as i32,
                    b.height as i32,
                    -1,
                    Some(
                        gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(b.x, b.y)),
                    ),
                );
            };
            for (child, bounds) in children.iter().zip(layout.tiles) {
                allocate(child, bounds);
            }
            if let Some(grip) = self.grip.borrow().as_ref() {
                grip.set_child_visible(layout.grip.is_some());
                if let Some(bounds) = layout.grip {
                    allocate(grip, bounds);
                }
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.children.borrow().iter() {
                self.obj().snapshot_child(child, snapshot);
            }
            if let Some(grip) = self.grip.borrow().as_ref().filter(|g| g.is_child_visible()) {
                self.obj().snapshot_child(grip, snapshot);
            }
        }
    }
}
glib::wrapper! {
    pub struct TileStrip(ObjectSubclass<imp::TileStrip>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl TileStrip {
    pub fn new() -> Self {
        let strip: Self = glib::Object::new();
        strip.set_overflow(gtk::Overflow::Hidden);
        strip
    }
    pub fn append(&self, widget: &impl IsA<gtk::Widget>) {
        widget.add_css_class("tile-button");
        widget.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push(widget.clone().upcast());
    }
    pub fn set_grip(&self, widget: &impl IsA<gtk::Widget>) {
        widget.set_parent(self);
        *self.imp().grip.borrow_mut() = Some(widget.clone().upcast());
    }
    pub fn configure(&self, axis: Axis, standalone: bool) {
        self.imp().vertical.set(axis == Axis::Vertical);
        self.imp().tabbed.set(!standalone);
        if axis == Axis::Vertical {
            self.add_css_class("vertical");
        } else {
            self.remove_css_class("vertical");
        }
        self.queue_resize();
    }
}

/// One shared vector glyph; CSS rotates it with the strip orientation.
pub fn grip() -> gtk::Image {
    let grip = gtk::Image::from_icon_name("layer-grip-symbolic");
    grip.add_css_class("panel-grip");
    grip.set_tooltip_text(Some("Drag to move panel"));
    grip.set_cursor_from_name(Some("grab"));
    grip
}
