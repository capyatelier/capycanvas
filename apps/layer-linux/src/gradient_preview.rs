//! Managed gradient artwork; neutral stop handles keep native GTK styling.
use crate::display_color::ViewColor;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::{GradientDefinition, color::DocumentColor};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Preview {
        pub gradient: RefCell<GradientDefinition>,
        pub selected: Cell<usize>,
        pub color: Cell<Option<(DocumentColor, ViewColor)>>,
        pub textures: RefCell<Option<([u32;2], [gdk::Texture; 2])>>,
        pub compact: Cell<bool>,
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
                let height=if self.compact.get() {24} else {44};
                (height, height, -1, -1)
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (width, height) = (obj.width() as f32, obj.height() as f32);
            let gradient = self.gradient.borrow();
            let stops=&gradient.stops;
            let Some((document, view)) = self.color.get() else {
                return;
            };
            if stops.len() < 2 || width <= 12. || height <= 12. {
                return;
            }
            let inset=if self.compact.get() {0.} else {6.};
            let bottom=if self.compact.get() {0.} else {12.};
            let scale=obj.scale_factor() as f32;
            let extent=[((width-2.*inset)*scale).ceil().clamp(1.,4096.) as u32,((height-bottom)*scale).ceil().clamp(1.,256.) as u32];
            let pixels=extent[0] as usize*extent[1] as usize;
            if self
                .textures
                .borrow()
                .as_ref()
                .is_none_or(|(size, _)| *size != extent)
            {
                let mut rows = [
                    Vec::with_capacity(pixels * 8),
                    Vec::with_capacity(pixels * 8),
                ];
                let colors=gradient.preview(extent,document.space,document.depth).expect("validated gradient");
                let mut mapped=std::collections::HashMap::new();
                for color in colors {
                    let pair=*mapped.entry((color.space,color.rgba.map(f32::to_bits))).or_insert_with(||view.checker_colors(color));
                    for (row, rgba) in rows.iter_mut().zip(pair) {
                        row.extend(rgba.into_iter().flat_map(|v| {
                            half::f16::from_f32(v).to_bits().to_ne_bytes()
                        }));
                    }
                }
                *self.textures.borrow_mut() = Some((
                    extent,
                    rows.map(|bytes| {
                        view.texture(
                            extent,
                            gdk::MemoryFormat::R16g16b16a16Float,
                            extent[0] as usize * 8,
                            bytes,
                        )
                    }),
                ));
            }
            let textures = self.textures.borrow();
            let (_, textures) = textures.as_ref().unwrap();
            let bounds = gtk::graphene::Rect::new(inset, 0., width - 2.*inset, height - bottom);
            snapshot.push_clip(&bounds);
            snapshot.append_texture(&textures[0], &bounds);
            let cell = layer_ui::TRANSPARENCY_CHECKER_CELL;
            for y in 0..((height - bottom) / cell).ceil() as i32 {
                for x in 0..((width - 2.*inset) / cell).ceil() as i32 {
                    if (x + y) % 2 == 0 {
                        continue;
                    }
                    snapshot.push_clip(&gtk::graphene::Rect::new(
                        inset + x as f32 * cell,
                        y as f32 * cell,
                        cell,
                        cell,
                    ));
                    snapshot.append_texture(&textures[1], &bounds);
                    snapshot.pop();
                }
            }
            snapshot.pop();
            if self.compact.get() {return;}
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
    pub fn set_compact(&self,compact:bool) {
        if self.imp().compact.replace(compact)!=compact {self.imp().textures.borrow_mut().take();self.queue_resize();}
    }
    pub fn set_gradient(
        &self,
        gradient: &GradientDefinition,
        selected: usize,
        document: DocumentColor,
        view: ViewColor,
    ) {
        if self.imp().color.replace(Some((document, view))) != Some((document, view))
            || *self.imp().gradient.borrow() != *gradient
        {
            *self.imp().gradient.borrow_mut() = gradient.clone();
            self.imp().textures.borrow_mut().take();
        }
        self.imp().selected.set(selected);
        self.queue_draw();
    }
}
