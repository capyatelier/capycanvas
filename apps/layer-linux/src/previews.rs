//! Bundled real-engine swatches, shared byte-for-byte with the browser host.
use gtk::{gdk, glib};
use layer_ui::Theme;

macro_rules! swatches {
    ($theme:literal) => {
        [
            include_bytes!(concat!("../../layer-web/brush-previews/1-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/2-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/3-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/4-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/5-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/6-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/7-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/8-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!("../../layer-web/brush-previews/9-", $theme, ".png"))
                as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/10-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/11-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/12-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/13-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/14-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/15-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/16-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/17-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/18-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/19-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/20-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/21-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/22-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/23-",
                $theme,
                ".png"
            )) as &'static [u8],
            include_bytes!(concat!(
                "../../layer-web/brush-previews/24-",
                $theme,
                ".png"
            )) as &'static [u8],
        ]
    };
}
pub fn texture(id: u32, theme: Theme) -> gdk::Texture {
    let images = if theme == Theme::Dark {
        swatches!("dark")
    } else {
        swatches!("light")
    };
    gdk::Texture::from_bytes(&glib::Bytes::from_static(images[(id - 1) as usize]))
        .expect("bundled brush preview is a valid PNG")
}
