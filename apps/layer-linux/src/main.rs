mod alert;
mod canvas;
mod documents;
mod display_color;
mod proof_view;
mod hdr;
mod hdr_color_scale;
mod effects;
mod files;
mod histogram;
mod color_editor;
mod color_picker;
mod color_preview_raster;
mod color_readout;
mod new_document;
mod color_library;
#[cfg(test)]
mod fullscreen_tests;
mod icons;
mod image_selector;
mod input;
mod layers;
mod selection_masks;
mod swipe_row;
mod navigator;
mod number_control;
mod panel_controls;
mod proof_dial;
mod local_tone_view;
mod preferences;
mod previews;
mod recovery;
mod render_thread;
mod squircle;
mod system_status;
mod tiles;
#[cfg(test)]
mod timing;
mod tool_panels;
mod tool_extra;
mod tooltips;
mod wayland;
mod workspace;

use adw::prelude::*;
use gtk::glib;
use std::{cell::RefCell, rc::Rc};

fn stylesheet() -> String {
    format!(
        "window {{ --ui-text-size: {}pt; }}\n{}",
        layer_ui::UI_TEXT_PT,
        include_str!("style.css")
    )
}

fn stylesheet_provider() -> gtk::CssProvider {
    let css = gtk::CssProvider::new();
    css.load_from_string(&stylesheet());
    // GTK media queries use the provider's preference, not the theme's.
    adw::StyleManager::for_display(&gtk::gdk::Display::default().unwrap())
        .bind_property("high-contrast", &css, "prefers-contrast")
        .transform_to(|_, contrast: bool| Some(if contrast {
            gtk::InterfaceContrast::More
        } else {
            gtk::InterfaceContrast::NoPreference
        }))
        .sync_create()
        .build();
    css
}

fn main() -> gtk::glib::ExitCode {
    // SAFETY: first operation, before GTK initialization or worker creation.
    unsafe { display_color::enable_gtk_color_management() };
    glib::set_application_name(layer_ui::APP_NAME);
    let (app, active) = application("art.capycanvas.CapyCanvas");
    let result = app.run();
    let windows = std::mem::take(&mut *active.borrow_mut());
    drop(windows);
    layer_render_wgpu::finish_shader_compiler_shutdown();
    result
}

fn application(id: &str) -> (adw::Application, Rc<RefCell<Vec<Rc<workspace::Workspace>>>>) {
    let app = adw::Application::builder()
        .application_id(id)
        // Review instances keep desktop settings and accept application file opens.
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN | if std::env::var_os("CAPY_NEW_INSTANCE").is_some() {
            gtk::gio::ApplicationFlags::NON_UNIQUE
        } else {
            gtk::gio::ApplicationFlags::FLAGS_NONE
        })
        .build();
    let active: Rc<RefCell<Vec<Rc<workspace::Workspace>>>> = Rc::default();
    app.connect_startup(|_| {
        let css = stylesheet_provider();
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
            // A file launch can be preparing its source before any canvas exists.
            if let Some(window) = app.active_window().or_else(|| app.windows().first().cloned()) {
                window.present();
                return;
            }
            app.activate_action("new-window", None);
            let workspace = active.borrow().last().unwrap().clone();
            recovery::offer_stale(&workspace);
        }
    ));
    (app, active)
}

fn install_actions(app: &adw::Application, active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>) {
    files::launch::install(app, active);
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
            open_workspace(&app, &active, None, None);
        }
    ));
    app.add_action(&new_window);
}

fn open_workspace(
    app: &adw::Application,
    active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>,
    project: Option<(layer_core::Project, Option<layer_ui::DocumentLocation>)>,
    recovered: Option<std::path::PathBuf>,
) {
    let settings = active.borrow().last().and_then(|w| {
        w.gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().settings.clone())
    });
    let workspace = match project {
        None => workspace::Workspace::new(app),
        Some(project) => workspace::Workspace::with_project(app, Some(project)),
    };
    let owner = Rc::downgrade(&workspace);
    *workspace.open_document.borrow_mut() = Some(Rc::new(move |project, location, recovered| {
        if let Some(w) = owner.upgrade() { w.documents.enqueue(&w, (project, location, recovered)); }
    }));
    workspace.recovery().recovered.set(recovered.is_some());
    if let Err(error) = workspace.recovery().set_origin(recovered) { eprintln!("Recovery ownership failed: {error}"); }
    active.borrow_mut().push(workspace.clone());
    workspace.window.present();
    if let Some(settings) = settings {
        workspace.dispatch(layer_ui::UiAction::RestoreSettings { settings });
    }
    // Capture the prepared document for both ordinary activation and file
    // launches. Starting here also excludes photo decoding from the delay.
    if let Ok(path) = std::env::var("LAYER_UI_CAPTURE") {
        let app = app.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || {
            assert!(workspace.gpu.borrow().is_some(), "GTK GPU initialization failed");
            capture(&workspace, &path);
            workspace.window.close();
            app.quit();
        });
    }
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
    let color = workspace.view_color();
    let image = workspace.gpu.borrow().as_ref().unwrap().session.engine().backend()
        .capture_in(color).expect("capture managed canvas");
    let canvas = color.texture([image.width, image.height],
        gtk::gdk::MemoryFormat::R8g8b8a8Premultiplied, image.stride as usize, image.bytes);
    workspace.area.set_paintable(Some(&canvas));
    let context = gtk::glib::MainContext::default();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(50);
    while std::time::Instant::now() < until {
        while context.pending() && std::time::Instant::now() < until {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let result = capture();
    workspace.area.set_paintable(None::<&gtk::gdk::Texture>);
    result
}

#[allow(deprecated)]
fn snapshot_window(window: &impl IsA<gtk::Window>, scale: f32) -> gtk::gdk::Texture {
    let window = window.as_ref();
    // Capture the current native scene, including pending allocations.
    for _ in 0..60 {
        // Explicit capture may run while Wayland has stopped frame callbacks
        // for an occluded window. Complete its pending native allocation at
        // the actual window size before asking GTK for the render tree.
        window.allocate(window.width(), window.height(), -1, None);
        let snapshot = gtk::Snapshot::new();
        snapshot.scale(scale, scale);
        // The toplevel paints its own background outside its child snapshot.
        // Include it for opaque inspector windows as well as canvas underlays.
        snapshot.render_background(&window.style_context(), 0., 0., window.width() as f64, window.height() as f64);
        // Snapshot the actual dialog host, including its modal sheets. A
        // WidgetPaintable of the toplevel can have an empty cached scene while
        // the compositor has occluded it; that is not an empty application.
        let mut child = window.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            window.snapshot_child(&widget, &snapshot);
        }
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
