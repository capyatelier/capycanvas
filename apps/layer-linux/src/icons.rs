//! The shared SVG bank, rendered as vectors with its original paint semantics.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static PAINTABLES: RefCell<HashMap<String, gtk::Svg>> = RefCell::default();
}

pub fn register() {
    static ICONS: std::sync::Once = std::sync::Once::new();
    ICONS.call_once(|| {
        gtk::gio::resources_register_include!("layer-icons.gresource").expect("bundled icons");
    });
}

fn paintable(name: &str, color: Option<layer_ui::HexColor>) -> Option<gtk::Svg> {
    register();
    PAINTABLES.with_borrow_mut(|cache| {
        let cache_key = format!("{name}:{color:?}");
        if let Some(svg) = cache.get(&cache_key) {
            return Some(svg.clone());
        }
        let bytes = gtk::gio::resources_lookup_data(
            &format!("/dev/layer/icons/scalable/actions/{name}.svg"),
            gtk::gio::ResourceLookupFlags::NONE,
        )
        .ok()?;
        let mut source = std::str::from_utf8(&bytes)
            .expect("SVG is UTF-8")
            .replace("currentColor", &color.map_or_else(|| "url(#gpa:foreground)".into(), |c| c.to_string()));
        if name == "layer-color-symbolic" {
            source = source.replace("#33d17a", "url(#gpa:success)");
        }
        let svg = gtk::Svg::new();
        // Traditional symbolic loading ignores explicit fill/stroke paints.
        // GtkSvg keeps SVG transforms, group opacity and fixed swatches;
        // symbolic paint servers follow the widget's native palette.
        svg.set_features(gtk::SvgFeatures::EXTENSIONS);
        let key = name.to_owned();
        svg.connect_error(move |_, error| eprintln!("Shared icon {key}: {error}"));
        svg.load_from_bytes(&glib::Bytes::from_owned(source.into_bytes()));
        cache.insert(cache_key, svg.clone());
        Some(svg)
    })
}

pub fn image(name: &str) -> gtk::Image {
    let image = gtk::Image::new();
    image.set_pixel_size(16);
    set(&image, Some(name));
    image
}

pub fn name(image: &gtk::Image) -> Option<glib::GString> {
    let name = image.widget_name();
    if name.starts_with("layer-") {
        Some(name)
    } else {
        image.icon_name()
    }
}

pub fn set(image: &gtk::Image, icon: Option<&str>) {
    set_colored(image, icon, None);
}

pub fn set_colored(image: &gtk::Image, icon: Option<&str>, color: Option<layer_ui::HexColor>) {
    if let Some(icon) = icon.filter(|n| n.starts_with("layer-"))
        && let Some(svg) = paintable(icon, color)
    {
        if image.paintable().as_ref() != Some(svg.upcast_ref()) { image.set_paintable(Some(&svg)); }
        image.set_widget_name(icon);
    } else {
        image.set_icon_name(icon);
        image.set_widget_name("GtkImage");
    }
}

pub fn button(name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_button(&button, name);
    button
}

pub fn set_button(button: &impl IsA<gtk::Button>, name: &str) {
    let button = button.as_ref();
    if let Some(image) = button.child().and_downcast::<gtk::Image>() {
        set(&image, Some(name));
    } else {
        button.set_child(Some(&image(name)));
    }
}
