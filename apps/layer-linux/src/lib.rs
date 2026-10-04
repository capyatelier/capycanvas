mod alert;
mod canvas;
mod canvas_bar;
mod canvas_size;
mod image_size;
mod documents;
mod display_color;
mod proof_view;
mod screen_view;
mod hdr;
mod hdr_color_scale;
mod effects;
mod files;
mod glass;
mod histogram;
mod color_editor;
mod color_picker;
mod color_preview_raster;
mod color_readout;
mod new_document;
mod color_library;
mod icons;
mod image_selector;
mod input;
mod layers;
mod preview_dialog;
mod swatch_selector;
mod swipe_row;
mod navigator;
mod notice;
mod number_control;
mod range_control;
mod palette_grid;
mod panel_controls;
mod panel_tabs;
mod proof_dial;
mod local_tone_view;
mod preferences;
mod command_bar;
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
mod text_language;
mod transparency_choice;
mod wayland;
mod workspace;
mod zoom_readout;

use adw::prelude::*;
use gtk::glib;
use std::{cell::RefCell, rc::Rc};

fn stylesheet() -> String {
    format!(
        "window {{ --ui-text-size: {}pt; --command-inset: {}px; --command-gap: {}px; --command-radius: {}px; }}\n{}",
        layer_ui::UI_TEXT_PT,
        layer_ui::COMMAND_SEARCH_STYLE.inset,
        layer_ui::COMMAND_SEARCH_STYLE.gap,
        layer_ui::COMMAND_SEARCH_STYLE.radius,
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

pub fn run() -> gtk::glib::ExitCode {
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

fn launch_localization() -> &'static std::sync::Arc<layer_ui::Localizer> {
    static ACTIVE: std::sync::OnceLock<std::sync::Arc<layer_ui::Localizer>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| {
        let preference = match preferences::load_language_preference() {
            Ok(preference) => preference,
            Err(error) => {
                eprintln!("{error}; using system language");
                layer_ui::LanguagePreference::System
            }
        };
        let languages = glib::language_names_with_category("LC_MESSAGES");
        let tags = languages.iter().map(|tag| tag.as_str()).collect::<Vec<_>>();
        layer_ui::Localizer::shared(layer_ui::resolve_launch_language(preference, &tags))
    })
}

struct ApplicationLocalization {
    localization: std::sync::Arc<layer_ui::Localizer>,
    settings: Option<layer_ui::Settings>,
    windows: Vec<Rc<WindowLocalization>>,
}

type LocalizationCallback = Box<dyn Fn(&std::sync::Arc<layer_ui::Localizer>) -> bool>;

struct WindowLocalization {
    window: glib::WeakRef<adw::ApplicationWindow>,
    transition: RefCell<layer_ui::LanguageTransition>,
    callbacks: RefCell<Vec<LocalizationCallback>>,
}

fn application_localization(app: &adw::Application) -> Rc<RefCell<ApplicationLocalization>> {
    if let Some(state) = unsafe { app.data::<Rc<RefCell<ApplicationLocalization>>>("capy-application-localization") } {
        return unsafe { state.as_ref() }.clone();
    }
    let state = Rc::new(RefCell::new(ApplicationLocalization { localization: launch_localization().clone(), settings: None, windows: Vec::new() }));
    unsafe { app.set_data("capy-application-localization", state.clone()); }
    state
}

pub(crate) fn on_window_localization(
    window: &adw::ApplicationWindow,
    workspace: Option<&Rc<workspace::Workspace>>,
    initial: &std::sync::Arc<layer_ui::Localizer>,
    callback: impl Fn(&std::sync::Arc<layer_ui::Localizer>) -> bool + 'static,
) {
    if let Some(workspace) = workspace { workspace.on_localization(callback); return; }
    let initial = window_localization(window, None, initial);
    if !callback(&initial) { return; }
    unsafe { window.set_data("capy-window-localization", initial.clone()); }
    let Some(app) = window.application().and_downcast::<adw::Application>() else { return };
    let state = application_localization(&app);
    let owner = {
        let mut state = state.borrow_mut();
        state.windows.retain(|owner| owner.window.upgrade().is_some());
        if let Some(owner) = state.windows.iter().find(|owner| owner.window.upgrade().as_ref() == Some(window)) { owner.clone() }
        else {
            let owner = Rc::new(WindowLocalization { window: window.downgrade(), transition: RefCell::new(layer_ui::LanguageTransition::new(initial)), callbacks: RefCell::default() });
            state.windows.push(owner.clone());
            owner
        }
    };
    owner.callbacks.borrow_mut().push(Box::new(callback));
    if let Some(settings) = state.borrow().settings.as_ref() { owner.request(settings.language); }
}

pub(crate) fn window_localization(window: &adw::ApplicationWindow, workspace: Option<&Rc<workspace::Workspace>>, fallback: &std::sync::Arc<layer_ui::Localizer>) -> std::sync::Arc<layer_ui::Localizer> {
    workspace.map(|workspace| workspace.localization()).or_else(|| unsafe {
        window.data::<std::sync::Arc<layer_ui::Localizer>>("capy-window-localization").map(|localization| localization.as_ref().clone())
    }).unwrap_or_else(|| fallback.clone())
}

impl WindowLocalization {
    fn request(self: &Rc<Self>, preference: layer_ui::LanguagePreference) {
        let languages = glib::language_names_with_category("LC_MESSAGES");
        let tags = languages.iter().map(|tag| tag.as_str()).collect::<Vec<_>>();
        let Some(request) = self.transition.borrow_mut().request(preference, &tags) else { return; };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || { let _ = sender.send(layer_ui::Localizer::shared(request.language)); });
        let owner = Rc::downgrade(self);
        let mut prepared = false;
        glib::timeout_add_local(std::time::Duration::from_millis(10), move || {
            let Some(owner) = owner.upgrade() else { return glib::ControlFlow::Break };
            let Some(window) = owner.window.upgrade() else { return glib::ControlFlow::Break };
            if !prepared {
                match receiver.try_recv() {
                    Ok(localization) => {
                        if !owner.transition.borrow_mut().prepared(request, localization) { return glib::ControlFlow::Break; }
                        prepared = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
            let localization = owner.transition.borrow_mut().publish(crate::input::localization_input_busy(&window));
            if let Some(localization) = localization {
                unsafe { window.set_data("capy-window-localization", localization.clone()); }
                crate::text_language::update(&window, &localization);
                let callbacks = std::mem::take(&mut *owner.callbacks.borrow_mut());
                let callbacks = callbacks.into_iter().filter(|callback| callback(&localization)).collect::<Vec<_>>();
                owner.callbacks.borrow_mut().extend(callbacks);
                glib::ControlFlow::Break
            } else if owner.transition.borrow().pending() { glib::ControlFlow::Continue }
            else { glib::ControlFlow::Break }
        });
    }
}

fn request_application_language(app: &adw::Application, preference: layer_ui::LanguagePreference) {
    let state = application_localization(app);
    state.borrow_mut().windows.retain(|owner| owner.window.upgrade().is_some());
    for owner in state.borrow().windows.clone() { owner.request(preference); }
}

fn application(id: &str) -> (adw::Application, Rc<RefCell<Vec<Rc<workspace::Workspace>>>>) {
    launch_localization();
    let app = adw::Application::builder()
        .application_id(id)
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let active: Rc<RefCell<Vec<Rc<workspace::Workspace>>>> = Rc::default();
    app.connect_startup(|app| {
        text_language::install(app, launch_localization());
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

        }
    ));
    (app, active)
}

fn install_actions(app: &adw::Application, active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>) {
    files::launch::install(app, active);
    let settings_changed =
        gtk::gio::SimpleAction::new("settings-changed", Some(glib::VariantTy::STRING));
    settings_changed.connect_activate(glib::clone!(
        #[weak]
        app,
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
            application_localization(&app).borrow_mut().settings = Some(settings.clone());
            request_application_language(&app, settings.language);
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
    let hold = app.hold();
    glib::spawn_future_local(glib::clone!(#[weak] app, #[strong] active, async move {
        let (settings, localization) = prepare_application_context(&app, &active).await;
        let offer_recovery = project.is_none() && active.borrow().is_empty();
        open_workspace_ready(&app, &active, project, recovered, settings, localization);
        if offer_recovery { if let Some(workspace) = active.borrow().last() { recovery::offer_stale(workspace); } }
        drop(hold);
    }));
}

pub(crate) async fn prepare_application_context(
    app: &adw::Application,
    active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>,
) -> (Option<layer_ui::Settings>, std::sync::Arc<layer_ui::Localizer>) {
    let state = application_localization(app);
    let settings = || state.borrow().settings.clone().or_else(|| active.borrow().last().and_then(|w| w.gpu.borrow().as_ref().map(|g| g.session.state().settings.clone())));
    loop {
        let requested = settings();
        let initial = state.borrow().localization.clone();
        let languages = glib::language_names_with_category("LC_MESSAGES");
        let tags = languages.iter().map(|tag| tag.as_str()).collect::<Vec<_>>();
        let language = requested.as_ref().map_or(initial.language(), |settings| layer_ui::resolve_launch_language(settings.language, &tags));
        let localization = if initial.language() == language { initial }
            else { gtk::gio::spawn_blocking(move || layer_ui::Localizer::shared(language)).await.unwrap() };
        let current = settings();
        if current.as_ref().is_some_and(|settings| layer_ui::resolve_launch_language(settings.language, &tags) != language) { continue; }
        state.borrow_mut().localization = localization.clone();
        return (current.or(requested), localization);
    }
}

fn open_workspace_ready(
    app: &adw::Application,
    active: &Rc<RefCell<Vec<Rc<workspace::Workspace>>>>,
    project: Option<(layer_core::Project, Option<layer_ui::DocumentLocation>)>,
    recovered: Option<std::path::PathBuf>,
    settings: Option<layer_ui::Settings>,
    localization: std::sync::Arc<layer_ui::Localizer>,
) {
    let workspace = match project {
        None => workspace::Workspace::new_localized(app, localization),
        Some(project) => workspace::Workspace::with_project_localized(app, Some(project), localization),
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
            let bounds = gtk::graphene::Rect::new(
                0.,
                0.,
                window.width() as f32 * scale,
                window.height() as f32 * scale,
            );
            return window
                .renderer()
                .unwrap()
                .render_texture(&node, Some(&bounds));
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
