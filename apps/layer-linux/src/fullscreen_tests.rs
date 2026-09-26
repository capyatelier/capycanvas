use super::*;
use layer_ui::{ApplicationMenu, CommandId, HeaderAction, HeaderItem, HeaderZone};

#[test]
#[ignore = "Wayland compositor and GPU: fullscreen window and native header"]
fn native_fullscreen_header_clock_and_battery() {
    let app = native_test_app("art.capycanvas.FullscreenTest");
    let w = Workspace::new(&app);
    w.window.present();
    until(|| w.gpu.borrow().is_some(), "canvas startup");
    let clock = find_named(w.window.upcast_ref(), "system-clock")
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
    until(
        || {
            find_named(w.window.upcast_ref(), "workspace-window-bar")
                .is_some_and(|bar| bar.is_mapped())
        },
        "window bar mapped",
    );
    assert!(
        !clock.is_mapped(),
        "windowed status does not occupy title-bar space"
    );
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
    until(
        || {
            w.window.is_fullscreen()
                && clock.is_mapped()
                && w.gpu.borrow().as_ref().unwrap().session.state().fullscreen
        },
        "fullscreen clock",
    );
    assert!(!clock.text().is_empty());
    if let Ok(directory) = std::env::var("LAYER_TEST_ARTIFACTS") {
        until(
            || {
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .backend()
                    .startup
                    .brush_ready
            },
            "brush readiness",
        );
        crate::capture(&w, &format!("{directory}/gtk-fullscreen.png"));
    }
    assert!(
        find_named(w.window.upcast_ref(), "workspace-window-bar")
            .unwrap()
            .is_mapped()
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
        until(
            || {
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
            },
            &format!("{preference} clock format"),
        );
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
    until(
        || !w.window.is_fullscreen() && !clock.is_mapped(),
        "leave fullscreen",
    );
    w.window.fullscreen();
    until(
        || w.window.is_fullscreen() && w.gpu.borrow().as_ref().unwrap().session.state().fullscreen,
        "fullscreen state",
    );
    let native = crate::system_status::SystemStatus::simulated_power();
    let probe = gtk::Window::builder()
        .application(&*app)
        .child(&native.root)
        .build();
    native.set_visibility(true);
    probe.present();
    until(|| native.root.is_mapped(), "status probe mapped");
    native.show_battery(Some(crate::system_status::Battery {
        percent: 8,
        charging: false,
        low: true,
    }));
    let battery = find_named(native.root.upcast_ref(), "system-battery").unwrap();
    assert!(battery.is_visible() && battery.has_css_class("low"));
    let percent = battery
        .first_child()
        .unwrap()
        .last_child()
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
    until(
        || percent.is_mapped() && battery.width() > 0,
        "battery percentage mapped",
    );
    // The embedded face must resolve in the real GTK font map; otherwise
    // desktop font substitutions silently change the compact icon's numerals.
    assert_eq!(
        percent
            .layout()
            .iter()
            .run_readonly()
            .unwrap()
            .item()
            .analysis()
            .font()
            .describe()
            .family()
            .as_deref(),
        Some("Capy Battery Numerals")
    );
    assert_eq!(battery.width(), layer_ui::TILE_SIZE as i32);
    assert_eq!(battery.height(), layer_ui::TILE_SIZE as i32);
    for size in layer_ui::HeaderSize::ALL {
        native.set_header_size(size);
        until(
            || {
                assert!(battery.is_visible());
                battery.width() >= size.tile() as i32 && battery.height() >= size.tile() as i32
            },
            "battery header size",
        );
    }
    native.set_visibility(false);
    assert!(!battery.is_visible() && !native.root.is_visible());
    native.set_visibility(true);
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
    until(
        || !w.window.is_fullscreen() && !clock.is_mapped(),
        "leave fullscreen",
    );
    assert!(
        !w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .command(CommandId::Fullscreen)
            .selected
    );
    // GTK visibility requires both a workspace component and fullscreen.
    // Removing/readding retains the live observer.
    let id = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .state()
        .workspace
        .layout
        .header
        .entries()
        .find(|e| e.item == HeaderItem::Clock)
        .unwrap()
        .id;
    w.dispatch(HeaderAction::Remove { id }.action());
    until(|| !clock.is_mapped(), "clock removed");
    w.window.fullscreen();
    until(|| w.window.is_fullscreen(), "window fullscreen");
    assert!(!clock.is_mapped());
    w.dispatch(
        HeaderAction::Add {
            zone: HeaderZone::Right,
            before: None,
            item: HeaderItem::Clock,
        }
        .action(),
    );
    until(|| clock.is_mapped(), "clock restored");
    w.window.unfullscreen();
    until(
        || !w.window.is_fullscreen() && !clock.is_mapped(),
        "leave fullscreen",
    );
    w.window.destroy();
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .is_none_or(|g| g.session.engine().backend().worker_is_joined())
    );
    // Finish compositor releases before libtest tears down the GTK owner thread.
    gdk::Display::default().unwrap().sync();
    pump(20);
}
