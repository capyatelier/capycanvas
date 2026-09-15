//! The shared color-pair icon geometry with managed artwork fills.
use super::{ViewColor, append_checker};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::RgbColor;
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Pair {
        pub size: Cell<i32>,
        pub key: Cell<Option<([RgbColor; 2], ViewColor)>>,
        pub textures: RefCell<Option<[[gdk::Texture; 2]; 2]>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Pair {
        const NAME: &'static str = "CapyManagedColorPair";
        type Type = super::ColorPair;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Pair {}
    impl WidgetImpl for Pair {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (self.size.get(), self.size.get(), -1, -1)
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(textures) = self.textures.borrow().clone() else {
                return;
            };
            let size = self.size.get() as f32;
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(
                (obj.width() as f32 - size) * 0.5,
                (obj.height() as f32 - size) * 0.5,
            ));
            snapshot.scale(size / 16., size / 16.);
            // Exact viewBox coordinates from layer-colors-symbolic.svg. Theme
            // frame rounding must not turn the small square swatches into discs.
            for (i, x, width) in [(1, 6.5, 8.75), (0, 0.75, 9.5)] {
                append_checker(
                    snapshot,
                    gtk::graphene::Rect::new(x, x, width, width),
                    1.,
                    &textures[i],
                );
                snapshot.append_border(
                    &gtk::gsk::RoundedRect::from_rect(
                        gtk::graphene::Rect::new(x - 0.5, x - 0.5, width + 1., width + 1.),
                        1.5,
                    ),
                    &[1.; 4],
                    &[obj.color(); 4],
                );
            }
            snapshot.restore();
        }
    }
}
glib::wrapper! {
    pub struct ColorPair(ObjectSubclass<imp::Pair>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl ColorPair {
    pub fn new(size: i32) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().size.set(size);
        obj.set_can_target(false);
        obj.set_widget_name("layer-colors-symbolic");
        obj
    }
    pub fn set_colors(&self, colors: [RgbColor; 2], view: ViewColor) {
        if self.imp().key.replace(Some((colors, view))) == Some((colors, view)) {
            return;
        }
        *self.imp().textures.borrow_mut() =
            Some(colors.map(|color| view.checker_colors(color).map(|rgba| view.solid(rgba))));
        self.queue_draw();
    }
}
