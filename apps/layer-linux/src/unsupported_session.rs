use adw::prelude::*;
use gtk::{gdk, gio, glib};

pub(crate) fn supported(display: &gdk::Display) -> bool {
    display.type_().name() == "GdkWaylandDisplay"
}

pub(crate) fn run(id: &str, localization: &layer_ui::Localizer) -> glib::ExitCode {
    let view = layer_ui::unsupported_session_view(localization);
    eprintln!("Capy Canvas needs a Wayland session; X11 isn't supported yet: {}", view.link);
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && let Some(backend) = std::env::var_os("GDK_BACKEND").filter(|backend| !backend.is_empty())
    {
        eprintln!("GDK_BACKEND={} selected X11; start without it to use the Wayland session.", backend.to_string_lossy());
    }
    glib::set_prgname(Some(id));
    application(id, view).run_with_args(&[id]);
    glib::ExitCode::FAILURE
}

pub(crate) fn application(id: &str, view: layer_ui::UnsupportedSessionView) -> adw::Application {
    let app = adw::Application::builder().application_id(id).build();
    app.connect_activate(move |app| match app.active_window() {
        Some(window) => window.present(),
        None => window(app, &view).present(),
    });
    app
}

fn window(app: &adw::Application, view: &layer_ui::UnsupportedSessionView) -> adw::ApplicationWindow {
    let learn_more = gtk::Button::with_label(&view.learn_more);
    learn_more.set_widget_name("unsupported-session-learn-more");
    let quit = gtk::Button::with_label(&view.quit);
    quit.set_widget_name("unsupported-session-quit");
    quit.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_homogeneous(true);
    buttons.set_halign(gtk::Align::Center);
    for button in [&learn_more, &quit] {
        button.add_css_class("pill");
        buttons.append(button);
    }
    let page = adw::StatusPage::builder()
        .title(view.title.as_ref())
        .description(glib::markup_escape_text(&view.body).as_str())
        .child(&buttons)
        .css_classes(["compact"])
        .build();
    page.set_paintable(crate::icons::paintable("layer-zen-looking-up-symbolic", None).as_ref());
    let content = adw::ToolbarView::builder().extend_content_to_top_edge(true).build();
    content.add_top_bar(&adw::HeaderBar::builder().show_title(false).css_classes(["flat"]).build());
    content.set_content(Some(&page));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(view.app_name)
        .default_width(420)
        .default_height(400)
        .content(&content)
        .default_widget(&quit)
        .build();
    window.set_widget_name("unsupported-session-window");
    GtkWindowExt::set_focus(&window, Some(&quit));
    let link = view.link;
    learn_more.connect_clicked(glib::clone!(#[weak] window, move |_| {
        gtk::UriLauncher::new(link).launch(Some(&window), gio::Cancellable::NONE, move |result| {
            if let Err(error) = result { eprintln!("Opening {link} failed: {error}"); }
        });
    }));
    quit.connect_clicked(glib::clone!(#[weak] app, move |_| app.quit()));
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    window.add_controller(shortcuts);
    window
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{find_named, pump, until};

    fn listing(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut entries = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() { pending.push(path.clone()); }
                entries.push(path);
            }
        }
        entries.sort();
        entries
    }

    fn labels(widget: &gtk::Widget) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(label) = widget.downcast_ref::<gtk::Label>() && label.is_mapped() {
            found.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            found.extend(labels(&current));
            child = current.next_sibling();
        }
        found
    }

    #[test]
    #[ignore = "private X11 display: gtk-raster.sh with LAYER_TEST_X11=1"]
    fn native_x11_session_refusal() {
        let private = std::env::var("LAYER_TEST_X11_DISPLAY").expect("gtk-raster.sh LAYER_TEST_X11=1 display");
        assert_eq!(std::env::var("DISPLAY").ok().as_deref(), Some(private.as_str()));
        adw::init().unwrap();
        let display = gdk::Display::default().unwrap();
        assert!(!supported(&display), "{} is not an X11 display", display.type_().name());
        let scheme = match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref() {
            Ok("light") => adw::ColorScheme::ForceLight,
            Ok("dark") => adw::ColorScheme::ForceDark,
            _ => adw::ColorScheme::Default,
        };
        adw::StyleManager::default().set_color_scheme(scheme);
        let storage = std::path::PathBuf::from(std::env::var_os("CAPY_STORAGE_DIR").unwrap());
        let before = listing(&storage);
        let data = glib::user_data_dir();
        assert_ne!(data, glib::home_dir().join(".local/share"), "the test browser needs a private XDG_DATA_HOME");
        let applications = data.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let opened = data.join("opened-url");
        let browser = data.join("record-url");
        std::fs::write(&browser, format!("#!/bin/sh\nprintf %s \"$1\" > '{}'\n", opened.display())).unwrap();
        std::fs::set_permissions(&browser, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        std::fs::write(applications.join("capy-test-browser.desktop"), format!(
            "[Desktop Entry]\nType=Application\nName=Test browser\nExec={} %u\nMimeType=x-scheme-handler/https;\nNoDisplay=true\n",
            browser.display())).unwrap();
        std::fs::write(applications.join("mimeapps.list"), "[Default Applications]\nx-scheme-handler/https=capy-test-browser.desktop\n").unwrap();
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let view = layer_ui::unsupported_session_view(&localization);
        let app = application(&format!("art.capycanvas.UnsupportedSession.p{}", std::process::id()), view.clone());
        app.register(gio::Cancellable::NONE).unwrap();
        app.activate();
        until(|| app.active_window().is_some_and(|window| window.is_mapped()), "refusal window maps");
        app.activate();
        pump(100);
        assert_eq!(app.windows().len(), 1, "a second launch presents the same window");
        let window = app.active_window().unwrap();
        assert_eq!(window.title().as_deref(), Some("Capy Canvas"));
        let shown = labels(window.upcast_ref());
        for text in [&*view.title, &*view.body, &*view.learn_more, &*view.quit] {
            assert!(shown.iter().any(|label| label == text), "{text:?} missing from {shown:?}");
        }
        assert_eq!(view.link, "https://github.com/capyatelier/capycanvas/issues/35#issuecomment-7057532889");
        let quit = window.default_widget().expect("Quit is the default action");
        assert_eq!(quit.widget_name(), "unsupported-session-quit");
        assert_eq!(GtkWindowExt::focus(&window).map(|focus| focus.widget_name()).as_deref(), Some("unsupported-session-quit"));
        if let Some(output) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
            let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "system".into());
            crate::snapshot_window(&window, window.scale_factor() as f32)
                .save_to_png(std::path::Path::new(&output).join(format!("x11-refusal-{theme}.png"))).unwrap();
        }
        find_named(window.upcast_ref(), "unsupported-session-learn-more").unwrap()
            .downcast::<gtk::Button>().unwrap().emit_clicked();
        until(|| std::fs::read_to_string(&opened).is_ok_and(|url| url == view.link), "Learn More opens the X11 support issue");
        assert!(gtk::prelude::WidgetExt::activate_action(&window, "window.close", None).is_ok());
        until(|| app.windows().is_empty(), "Escape's window.close closes the only window");
        assert_eq!(listing(&storage), before, "the refusal writes no app storage");
    }
}
