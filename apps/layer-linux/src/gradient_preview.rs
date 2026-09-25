//! Managed gradient artwork; neutral stop handles keep native GTK styling.
use crate::display_color::ViewColor;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::{GradientStop, color::RgbSpace};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Preview {
        pub stops: RefCell<Vec<GradientStop>>,
        pub selected: Cell<usize>,
        pub color: Cell<Option<(RgbSpace, ViewColor)>>,
        pub textures: RefCell<Option<(usize, [gdk::Texture; 2])>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Preview {
        const NAME: &'static str = "CapyManagedGradientPreview";
        type Type = super::GradientPreview;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Preview {}
    impl WidgetImpl for Preview {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                (80, 160, -1, -1)
            } else {
                (44, 44, -1, -1)
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (width, height) = (obj.width() as f32, obj.height() as f32);
            let stops = self.stops.borrow();
            let Some((space, view)) = self.color.get() else {
                return;
            };
            if stops.len() < 2 || width <= 12. || height <= 12. {
                return;
            }
            let pixels = ((width - 12.) * obj.scale_factor() as f32).ceil() as usize;
            if self
                .textures
                .borrow()
                .as_ref()
                .is_none_or(|(size, _)| *size != pixels)
            {
                let mut rows = [
                    Vec::with_capacity(pixels * 8),
                    Vec::with_capacity(pixels * 8),
                ];
                for x in 0..pixels {
                    let position = x as f32 / pixels.saturating_sub(1).max(1) as f32;
                    let color = layer_core::gradient_value(&stops, position, space)
                        .expect("validated gradient");
                    for (row, rgba) in rows.iter_mut().zip(view.checker_colors(color)) {
                        row.extend(rgba.into_iter().flat_map(|v| {
                            half::f16::from_f32(v).to_bits().to_ne_bytes()
                        }));
                    }
                }
                *self.textures.borrow_mut() = Some((
                    pixels,
                    rows.map(|bytes| {
                        view.texture(
                            [pixels as u32, 1],
                            gdk::MemoryFormat::R16g16b16a16Float,
                            pixels * 8,
                            bytes,
                        )
                    }),
                ));
            }
            let textures = self.textures.borrow();
            let (_, textures) = textures.as_ref().unwrap();
            let bounds = gtk::graphene::Rect::new(6., 0., width - 12., height - 12.);
            snapshot.push_clip(&bounds);
            snapshot.append_texture(&textures[0], &bounds);
            let cell = layer_ui::TRANSPARENCY_CHECKER_CELL;
            for y in 0..((height - 12.) / cell).ceil() as i32 {
                for x in 0..((width - 12.) / cell).ceil() as i32 {
                    if (x + y) % 2 == 0 {
                        continue;
                    }
                    snapshot.push_clip(&gtk::graphene::Rect::new(
                        6. + x as f32 * cell,
                        y as f32 * cell,
                        cell,
                        cell,
                    ));
                    snapshot.append_texture(&textures[1], &bounds);
                    snapshot.pop();
                }
            }
            snapshot.pop();
            let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(0., 0., width, height));
            let color = obj.color();
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            );
            for (i, stop) in stops.iter().enumerate() {
                cr.arc(
                    (6. + stop.position * (width - 12.)) as f64,
                    (height - 5.) as f64,
                    if self.selected.get() == i { 4. } else { 2.5 },
                    0.,
                    std::f64::consts::TAU,
                );
                let _ = cr.fill();
            }
        }
    }
}
glib::wrapper! {
    pub struct GradientPreview(ObjectSubclass<imp::Preview>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl GradientPreview {
    pub fn new() -> Self {
        let obj: Self = glib::Object::new();
        obj.set_hexpand(true);
        obj
    }
    pub fn set_gradient(
        &self,
        stops: &[GradientStop],
        selected: usize,
        space: RgbSpace,
        view: ViewColor,
    ) {
        if self.imp().color.replace(Some((space, view))) != Some((space, view))
            || *self.imp().stops.borrow() != stops
        {
            *self.imp().stops.borrow_mut() = stops.to_vec();
            self.imp().textures.borrow_mut().take();
        }
        self.imp().selected.set(selected);
        self.queue_draw();
    }
}
