//! Bundled real-engine swatches, shared byte-for-byte with the browser host.
use gtk::{gdk, glib};
use layer_ui::Theme;

macro_rules! swatches {
    ($theme:literal) => {
        swatches!(
            $theme; 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 36, 37, 38, 39, 40, 41, 42
        )
    };
    ($theme:literal; $($id:literal),*) => {
        [$((
            $id,
            include_bytes!(concat!("../../layer-web/brush-previews/", $id, "-", $theme, ".png"))
                as &'static [u8],
        )),*]
    };
}
pub fn texture(id: u32, theme: Theme) -> gdk::Texture {
    let images = if theme == Theme::Dark {
        swatches!("dark")
    } else {
        swatches!("light")
    };
    let (_, bytes) = images
        .into_iter()
        .find(|(preset, _)| *preset == id)
        .expect("every brush preset has a bundled preview");
    gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes))
        .expect("bundled brush preview is a valid PNG")
}
