use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, glib, gsk};
use std::cell::Cell;

fn outline(node: &gsk::RenderNode) -> Option<gsk::RoundedRect> {
    if let Some(shadow) = node.downcast_ref::<gsk::OutsetShadowNode>() {
        Some(shadow.outline())
    } else if let Some(clip) = node.downcast_ref::<gsk::RoundedClipNode>() {
        Some(clip.clip())
    } else if let Some(border) = node.downcast_ref::<gsk::BorderNode>() {
        Some(border.outline())
    } else if let Some(container) = node.downcast_ref::<gsk::ContainerNode>() {
        (0..container.n_children()).find_map(|i| outline(&container.child(i)))
    } else if let Some(debug) = node.downcast_ref::<gsk::DebugNode>() {
        outline(&debug.child())
    } else {
        None
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Window {
        pub radius: Cell<Option<([i32; 2], f32)>>,
        changed: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "CapyWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            unsafe {
                let widget_class = klass as *mut _ as *mut gtk::ffi::GtkWidgetClass;
                (*widget_class).css_changed = Some(css_changed);
            }
        }
    }

    unsafe extern "C" fn css_changed(widget: *mut gtk::ffi::GtkWidget, change: *mut gtk::ffi::GtkCssStyleChange) {
        unsafe {
            let instance = &*(widget as *mut <Window as ObjectSubclass>::Instance);
            let imp = instance.imp();
            imp.radius.set(None);
            let parent = Window::type_data().as_ref().parent_class() as *mut gtk::ffi::GtkWidgetClass;
            if let Some(changed) = (*parent).css_changed { changed(widget, change); }
            if !imp.changed.replace(true) {
                glib::idle_add_local_once(glib::clone!(#[weak(rename_to = window)] imp.obj(), move || {
                    window.imp().changed.set(false);
                    window.emit_by_name::<()>("outline-changed", &[]);
                }));
            }
        }
    }

    impl ObjectImpl for Window {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> = std::sync::OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("outline-changed").build()])
        }
    }
    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gdk::gio::ActionGroup, gdk::gio::ActionMap, gtk::Accessible,
            gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    #[allow(deprecated)]
    pub fn corner_radius(&self) -> f32 {
        let size = [self.width().max(1), self.height().max(1)];
        if let Some((cached, radius)) = self.imp().radius.get() && cached == size {
            return radius;
        }
        let snapshot = gtk::Snapshot::new();
        let context = self.style_context();
        snapshot.render_background(&context, 0., 0., size[0] as f64, size[1] as f64);
        snapshot.render_frame(&context, 0., 0., size[0] as f64, size[1] as f64);
        snapshot.render_focus(&context, 0., 0., size[0] as f64, size[1] as f64);
        let radius = snapshot.to_node().as_ref().and_then(outline)
            .map_or(0., |shape| (shape.corner()[0].width() + shape.bounds().x()).max(0.));
        self.imp().radius.set(Some((size, radius)));
        radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{native_test_app, pump, until};

    #[test]
    #[ignore = "private Wayland display"]
    fn native_window_corner_geometry() {
        let app = native_test_app("art.capycanvas.WindowCorners");
        let window: Window = glib::Object::builder().property("application", &*app)
            .property("default-width", 800).property("default-height", 600).build();
        window.set_widget_name("window-corner-test");
        let area = gtk::Picture::new();
        window.set_content(Some(&area));
        window.present();
        until(|| area.is_mapped(), "corner window mapped");
        let css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(&WidgetExt::display(&window), &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2);
        for scheme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            adw::StyleManager::for_display(&WidgetExt::display(&window)).set_color_scheme(scheme);
            for radius in [15., 23.5, 0., 31.25] {
                css.load_from_string(&format!("#window-corner-test:not(.maximized):not(.fullscreen):not(.tiled):not(.tiled-top):not(.tiled-right):not(.tiled-bottom):not(.tiled-left) {{ --window-radius: {radius}px; }}"));
                pump(100);
                assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, radius, "{scheme:?}");
                for class in ["tiled", "tiled-top", "tiled-right", "tiled-bottom", "tiled-left"] {
                    window.add_css_class(class);
                    pump(40);
                    assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 0., "{scheme:?}/{class}");
                    window.remove_css_class(class);
                    pump(40);
                    assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, radius);
                }
            }
            window.maximize();
            until(|| window.is_maximized(), "corner window maximized");
            pump(100);
            assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 0.);
            window.unmaximize();
            until(|| !window.is_maximized(), "corner window restored");
            pump(100);
            assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 31.25);
            window.fullscreen();
            until(|| window.is_fullscreen(), "corner window fullscreen");
            pump(100);
            assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 0.);
            window.unfullscreen();
            until(|| !window.is_fullscreen(), "corner window leaves fullscreen");
            pump(100);
            assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 31.25);
            css.load_from_string("#window-corner-test { border-radius: 21.5px; background: transparent; box-shadow: none; }");
            pump(100);
            assert_eq!(crate::wayland::Geometry::of(&area).corner_radius, 21.5, "shadowless frame outline");
        }
        gtk::style_context_remove_provider_for_display(&WidgetExt::display(&window), &css);
        window.destroy();
    }
}
