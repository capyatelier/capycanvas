mod canvas;
mod input;
mod preferences;
mod previews;
mod render_thread;
mod tiles;
#[cfg(test)]
mod timing;
mod wayland;
mod workspace;

use adw::prelude::*;
use gtk::glib;
use std::{cell::RefCell, rc::Rc};

fn main() -> gtk::glib::ExitCode {
    glib::set_application_name(layer_ui::APP_NAME);
    let app = adw::Application::builder()
        .application_id("dev.layer.Copilot")
        .build();
    let active: Rc<RefCell<Vec<Rc<workspace::Workspace>>>> = Rc::default();
    app.connect_startup(|_| {
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
    install_actions(&app, &active);
    app.connect_activate(gtk::glib::clone!(
        #[strong]
        active,
        move |app| {
            if let Some(workspace) = active.borrow().last() {
                workspace.window.present();
                return;
            }
            app.activate_action("new-window", None);
            let workspace = active.borrow().last().unwrap().clone();
            if let Ok(path) = std::env::var("LAYER_UI_CAPTURE") {
                let app = app.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || {
                    assert!(
                        workspace.gpu.borrow().is_some(),
                        "GTK GPU initialization failed"
                    );
                    capture(&workspace, &path);
                    workspace.window.close();
                    app.quit();
                });
            }
        }
    ));
    let result = app.run();
    let windows = std::mem::take(&mut *active.borrow_mut());
    drop(windows);
    result
}

fn install_actions(app: &adw::Application, active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>) {
    let settings_changed =
        gtk::gio::SimpleAction::new("settings-changed", Some(glib::VariantTy::STRING));
    settings_changed.connect_activate(glib::clone!(
        #[strong]
        active,
        move |_, value| {
            let Some(settings) = value
                .and_then(|v| v.str())
                .and_then(|text| serde_json::from_str::<layer_ui::Settings>(text).ok())
            else {
                return;
            };
            // GTK's style manager is display-wide. Keep all windows' Rust settings
            // consistent too; RestoreSettings does not echo a persistence request.
            let windows = active.borrow().clone();
            for w in windows {
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().settings != settings)
                {
                    w.dispatch(layer_ui::UiAction::RestoreSettings {
                        settings: settings.clone(),
                    });
                }
            }
        }
    ));
    app.add_action(&settings_changed);
    app.connect_window_removed(glib::clone!(
        #[weak]
        active,
        move |_, window| {
            active
                .borrow_mut()
                .retain(|w| w.window.upcast_ref::<gtk::Window>() != window);
        }
    ));
    let new_window = gtk::gio::SimpleAction::new("new-window", None);
    new_window.connect_activate(glib::clone!(
        #[weak]
        app,
        #[strong]
        active,
        move |_, _| {
            let settings = active.borrow().last().and_then(|w| {
                w.gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.state().settings.clone())
            });
            let workspace = workspace::Workspace::new(&app);
            active.borrow_mut().push(workspace.clone());
            workspace.window.present();
            if let Some(settings) = settings {
                workspace.dispatch(layer_ui::UiAction::RestoreSettings { settings });
            }
        }
    ));
    app.add_action(&new_window);
}

fn capture(workspace: &workspace::Workspace, path: &str) {
    snapshot(workspace)
        .save_to_png(path)
        .expect("save GTK visual test");
}

fn snapshot(workspace: &workspace::Workspace) -> gtk::gdk::Texture {
    with_canvas_snapshot(workspace, || snapshot_window(&workspace.window, 1.0))
}

fn with_canvas_snapshot<T>(workspace: &workspace::Workspace, capture: impl FnOnce() -> T) -> T {
    // Explicit screenshot: GTK snapshots omit app-owned child surfaces.
    let image = workspace
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .capture()
        .expect("capture canvas");
    let pixels = gtk::glib::Bytes::from_owned(image.bytes);
    let canvas = gtk::gdk::MemoryTexture::new(
        image.width as i32,
        image.height as i32,
        gtk::gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &pixels,
        image.stride as usize,
    );
    workspace.area.set_paintable(Some(&canvas));
    let context = gtk::glib::MainContext::default();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(50);
    while std::time::Instant::now() < until {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let result = capture();
    workspace.area.set_paintable(None::<&gtk::gdk::Texture>);
    result
}

fn snapshot_window(window: &adw::ApplicationWindow, scale: f32) -> gtk::gdk::Texture {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    // A queued native allocation can briefly invalidate WidgetPaintable's
    // scene. Capture a fresh frame, never an old screenshot or fabricated pixels.
    for _ in 0..60 {
        let snapshot = gtk::Snapshot::new();
        snapshot.scale(scale, scale);
        paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
        if let Some(node) = snapshot.to_node() {
            return window.renderer().unwrap().render_texture(&node, None);
        }
        window.queue_draw();
        let context = gtk::glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    panic!("mapped GTK window did not produce a capture scene within one second");
}
