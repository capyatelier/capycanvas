//! Bundled real-engine swatches, shared byte-for-byte with the browser host.
use gtk::{gdk, gio, glib, graphene, prelude::*, subclass::prelude::*};
use layer_ui::Theme;
use std::{cell::{Cell, RefCell}, collections::HashMap};

macro_rules! swatches {
    ($theme:literal) => {
        swatches!(
            $theme; 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43,
            44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
            65, 66
        )
    };
    ($theme:literal; $($id:literal),*) => {
        [$((
            $id,
            include_bytes!(concat!("../../layer-web/brush-previews/", $id, "-", $theme, ".webp"))
                as &'static [u8],
        )),*]
    };
}
fn bytes(id: u32, theme: Theme) -> Option<&'static [u8]> {
    let images = if theme == Theme::Dark {
        swatches!("dark")
    } else {
        swatches!("light")
    };
    images.into_iter().find(|(preset, _)| *preset == id).map(|(_, bytes)| bytes)
}
fn filter_bytes(id: &str) -> Option<&'static [u8]> {
    let swatches: &[(&str, &'static [u8])] = include!(concat!(env!("OUT_DIR"), "/filter_swatches.rs"));
    swatches.iter()
    .find(|(filter, _)| *filter == id)
    .map(|(_, bytes)| *bytes)
}
pub fn filter_texture(id: &str) -> gdk::Paintable {
    swatch(filter_bytes(id).expect("every built-in filter has a bundled preview"))
}
pub fn texture(id: u32, theme: Theme) -> gdk::Paintable {
    swatch(bytes(id, theme).expect("every brush preset has a bundled preview"))
}
pub fn redraw_when_loaded(paintable: &gdk::Paintable, widget: &impl IsA<gtk::Widget>) {
    if let Some(swatch) = paintable.downcast_ref::<Swatch>() && swatch.imp().texture.borrow().is_none() {
        swatch.imp().waiting.borrow_mut().push(widget.upcast_ref::<gtk::Widget>().downgrade());
    }
}
pub fn append(snapshot: &gtk::Snapshot, paintable: &gdk::Paintable, bounds: &graphene::Rect) {
    snapshot.save();
    snapshot.translate(&bounds.top_left());
    paintable.snapshot(snapshot, f64::from(bounds.width()), f64::from(bounds.height()));
    snapshot.restore();
}
fn swatch(bytes: &'static [u8]) -> gdk::Paintable {
    thread_local! { static SWATCHES: RefCell<HashMap<usize, glib::WeakRef<Swatch>>> = Default::default(); }
    SWATCHES.with_borrow_mut(|swatches| {
        let key = bytes.as_ptr() as usize;
        if let Some(swatch) = swatches.get(&key).and_then(glib::WeakRef::upgrade) {
            return swatch.upcast();
        }
        let swatch = Swatch::load(bytes);
        swatches.insert(key, swatch.downgrade());
        swatch.upcast()
    })
}
glib::wrapper! {
    pub struct Swatch(ObjectSubclass<imp::Swatch>) @implements gdk::Paintable;
}
impl Swatch {
    fn load(bytes: &'static [u8]) -> Self {
        let swatch: Self = glib::Object::new();
        let [width, height] = layer_color::photo::webp_extent(bytes).expect("bundled swatch is a valid WebP");
        swatch.imp().size.set([width as i32, height as i32]);
        let weak = swatch.downgrade();
        glib::spawn_future_local(async move {
            let decoded = gio::spawn_blocking(move || layer_color::photo::decode_rgba8_webp(bytes)).await;
            let Ok(Ok((_, pixels))) = decoded else { panic!("bundled swatch decodes") };
            let Some(swatch) = weak.upgrade() else { return };
            let texture = gdk::MemoryTexture::new(width as i32, height as i32, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(pixels), width as usize * 4);
            swatch.imp().texture.replace(Some(texture.upcast()));
            swatch.invalidate_contents();
            for widget in swatch.imp().waiting.take() {
                if let Some(widget) = widget.upgrade() { widget.queue_draw(); }
            }
        });
        swatch
    }
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Swatch {
        pub(super) size: Cell<[i32; 2]>,
        pub(super) texture: RefCell<Option<gdk::Texture>>,
        pub(super) waiting: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Swatch {
        const NAME: &'static str = "CapySwatch";
        type Type = super::Swatch;
        type Interfaces = (gdk::Paintable,);
    }
    impl ObjectImpl for Swatch {}
    impl PaintableImpl for Swatch {
        fn intrinsic_width(&self) -> i32 { self.size.get()[0] }
        fn intrinsic_height(&self) -> i32 { self.size.get()[1] }
        fn flags(&self) -> gdk::PaintableFlags { gdk::PaintableFlags::STATIC_SIZE }
        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            if let Some(texture) = self.texture.borrow().as_ref() { texture.snapshot(snapshot, width, height); }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_built_in_filter_has_a_bundled_preview() {
        for filter in layer_core::bundled_effect_catalog().filters() {
            let bytes = super::filter_bytes(filter.id()).unwrap_or_else(|| panic!("filter {} has no bundled preview", filter.id()));
            let (extent, pixels) = layer_color::photo::decode_rgba8_webp(bytes).unwrap();
            assert_eq!(extent, layer_render::FILTER_PREVIEW_SIZE, "filter {}", filter.id());
            assert_eq!(pixels.len(), extent.iter().product::<u32>() as usize * 4);
        }
    }
    #[test]
    fn every_brush_preset_has_a_bundled_preview_in_both_themes() {
        for choice in layer_ui::brush_catalog() {
            for theme in [layer_ui::Theme::Light, layer_ui::Theme::Dark] {
                let bytes = super::bytes(choice.id, theme).unwrap_or_else(|| panic!("preset {} has no {theme:?} preview", choice.id));
                assert_eq!(layer_color::photo::webp_extent(bytes).unwrap(), layer_color::photo::decode_rgba8_webp(bytes).unwrap().0);
            }
        }
    }
}
