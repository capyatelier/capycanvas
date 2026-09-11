use adw::prelude::*;
use gtk::{gdk, glib};
use layer_ui::{ApplicationMenu, CommandId};
use std::time::{Duration, Instant};

fn pump() {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_millis(20);
    while context.pending() && Instant::now() < deadline {
        context.iteration(false);
    }
    std::thread::sleep(Duration::from_millis(5));
}
fn until(check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(
            Instant::now() < deadline,
            "Native fullscreen transition timed out"
        );
        pump();
    }
}
fn named(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if widget.widget_name() == name {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(w) = child {
        if let Some(found) = named(&w, name) {
            return Some(found);
        }
        child = w.next_sibling();
    }
    None
}
struct Windows(adw::Application);
impl Drop for Windows {
    fn drop(&mut self) {
        for w in self.0.windows() {
            w.destroy();
        }
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}

#[test]
#[ignore = "Wayland compositor and GPU: fullscreen window and native header"]
fn native_fullscreen_header_clock_and_battery() {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(&crate::stylesheet());
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id("art.capycanvas.FullscreenTest")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let _cleanup = Windows(app.clone());
    let w = crate::workspace::Workspace::new(&app);
    w.window.present();
    until(|| w.gpu.borrow().is_some());
    let status = named(w.window.upcast_ref(), "system-status").unwrap();
    let clock = named(&status, "system-clock")
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
    assert!(!status.is_visible());
    let menu = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .application_menu(ApplicationMenu::View);
    let item = menu
        .sections
        .iter()
        .flatten()
        .find(|i| i.label == "Full screen")
        .unwrap();
    assert_eq!(item.hint, "F11");
    w.dispatch(item.action.clone().unwrap());
    until(|| {
        w.window.is_fullscreen()
            && status.is_mapped()
            && w.gpu.borrow().as_ref().unwrap().session.state().fullscreen
    });
    assert!(!clock.text().is_empty());
    if let Ok(directory) = std::env::var("LAYER_TEST_ARTIFACTS") {
        until(|| {
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .backend()
                .startup
                .brush_ready
        });
        crate::capture(&w, &format!("{directory}/gtk-fullscreen.png"));
    }
    assert!(
        named(w.window.upcast_ref(), "header-status")
            .unwrap()
            .first_child()
            .unwrap()
            .has_css_class("document-title")
    );
    // Native window-manager changes must update the command checkmark too.
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .command(CommandId::Fullscreen)
            .selected
    );
    let settings = gtk::gio::Settings::new("org.gnome.desktop.interface");
    for preference in ["12h", "24h"] {
        settings.set_string("clock-format", preference).unwrap();
        until(|| {
            let time = glib::DateTime::now_local()
                .unwrap()
                .format(if preference == "12h" {
                    "%I:%M %p"
                } else {
                    "%H:%M"
                })
                .unwrap();
            clock.text()
                == if preference == "12h" {
                    time.trim_start_matches('0')
                } else {
                    &time
                }
        });
    }
    let reply = w
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .input(crate::input::key_input(
            gdk::Key::F11,
            true,
            gdk::ModifierType::empty(),
            false,
            None,
        ))
        .unwrap();
    assert!(reply.handled);
    w.changed(Ok(reply.change));
    until(|| !w.window.is_fullscreen() && !status.is_visible());
    w.window.fullscreen();
    until(|| {
        w.window.is_fullscreen() && w.gpu.borrow().as_ref().unwrap().session.state().fullscreen
    });
    let native = crate::system_status::SystemStatus::new();
    let probe = gtk::Window::builder()
        .application(&app)
        .child(&native.root)
        .build();
    native.root.set_visible(true);
    probe.present();
    until(|| native.root.is_mapped());
    native.show_battery(Some(crate::system_status::Battery {
        percent: 8,
        charging: false,
        low: true,
    }));
    let battery = named(native.root.upcast_ref(), "system-battery").unwrap();
    assert!(battery.is_visible() && battery.has_css_class("low"));
    native.show_battery(Some(crate::system_status::Battery {
        percent: 8,
        charging: true,
        low: true,
    }));
    assert!(battery.is_visible() && !battery.has_css_class("low"));
    native.show_battery(None);
    assert!(!battery.is_visible());
    probe.destroy();
    w.window.unfullscreen();
    until(|| !w.window.is_fullscreen() && !status.is_visible());
    assert!(
        !w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .command(CommandId::Fullscreen)
            .selected
    );
    w.window.destroy();
    assert!(w.gpu.borrow().is_none());
    // Finish compositor releases before libtest tears down the GTK owner thread.
    gdk::Display::default().unwrap().sync();
    pump();
}
