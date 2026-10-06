use super::*;
use serde::Deserialize;
use layer_ui::localization::SHIPPED_LANGUAGES;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    scenes: Vec<Scene>,
    languages: Vec<layer_ui::UiLanguage>,
    themes: Vec<Theme>,
    width: i32,
    height: i32,
    scale: u32,
    output: PathBuf,
    source_revision: String,
    source_dirty: bool,
    executable_sha256: String,
    executable_override: bool,
    recipe_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    id: String,
    source: PathBuf,
    workspace: String,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    Action { action: UiAction },
    Frame { zoom: f32, focus: [f32; 2] },
    SelectLayer { name: String },
    LayerVisibility { name: String, visible: bool },
    ShowPanel { panel: Panel },
    Effect { effect: String, parameters: BTreeMap<String, layer_core::EffectValue> },
    WaitHistogram,
}

fn layer_named(w: &Workspace, name: &str) -> u64 {
    let matches = state(w).layers.into_iter().filter(|layer| layer.label == name).collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "unique authored layer named {name}");
    matches[0].id
}

fn stage(w: &Rc<Workspace>, step: &Step) {
    match step {
        Step::Action { action } => w.dispatch(action.clone()),
        Step::Frame { zoom, focus } => {
            assert!(zoom.is_finite() && *zoom > 0. && focus.iter().all(|value| value.is_finite()));
            let camera = state(w).camera;
            let transform = camera.document_to_surface();
            let from = [transform[0] * focus[0] + transform[2] * focus[1] + transform[4],
                transform[1] * focus[0] + transform[3] * focus[1] + transform[5]];
            let target = zoom * camera.viewport[0] as f32 / w.area.width() as f32;
            let change = ui_session_mut(w).gesture(from,
                [camera.viewport[0] as f32 / 2., camera.viewport[1] as f32 / 2.], target / camera.zoom, 0.);
            w.changed(change);
        }
        Step::SelectLayer { name } => w.dispatch(UiAction::Layer {
            action: layer_ui::LayerAction::Select { id: layer_named(w, name), mask: false },
        }),
        Step::LayerVisibility { name, visible } => w.dispatch(UiAction::SetLayerVisibility {
            id: layer_named(w, name), visible: *visible,
        }),
        Step::ShowPanel { panel } => {
            w.dispatch(UiAction::Customize { action: layer_ui::CustomizationAction::SetPanelVisible { panel: *panel, visible: true } });
            let layout = state(w).workspace.layout;
            if layout.active_panel(*panel) != Some(*panel) {
                let group = layout.panel_group(*panel).expect("visible panel group");
                w.dispatch(UiAction::SelectPanelTab { group, panel: *panel });
            }
            w.dispatch(UiAction::Customize { action: layer_ui::CustomizationAction::CloseExpanded });
        }
        Step::Effect { effect, parameters } => {
            let before = state(w).layers.len();
            w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Insert { effect: effect.as_str().into() } });
            new_photo::ready(w);
            assert_eq!(state(w).layers.len(), before + 1, "insert effect {effect}");
            let layer = state(w).layer_properties.layer.expect("selected effect");
            for (key, value) in parameters {
                w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Set { layer, key: key.clone(), value: value.clone() } });
            }
        }
        Step::WaitHistogram => until(|| {
            let histogram = state(w).histogram;
            histogram.data.is_some() && histogram.status == w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_EXACT)
        }, "exact histogram"),
    }
    new_photo::finish(w);
    new_photo::ready(w);
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
}

#[test]
#[ignore = "capture helper capabilities"]
fn store_capture_capabilities() {
    let output = std::env::var_os("CAPY_GTK_STORE_CAPABILITIES").expect("capability output path");
    std::fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({
        "languages": SHIPPED_LANGUAGES.iter().map(|language| language.tag()).collect::<Vec<_>>(),
        "themes": ["light", "dark"],
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "private Wayland display, hardware GPU and website-owned scene recipe"]
fn native_store_capture() {
    assert!(std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.starts_with("layer-bench-")));
    assert_eq!(std::env::var("GSETTINGS_BACKEND").as_deref(), Ok("memory"));
    let path = std::env::var_os("CAPY_GTK_STORE_JOB").expect("capture job");
    let job: Job = serde_json::from_slice(&std::fs::read(path).unwrap()).expect("valid capture job");
    assert_eq!((job.scenes.len(), job.languages.len(), job.themes.len()), (1, 1, 1), "fresh app state per capture");
    let (scene, language, theme) = (&job.scenes[0], job.languages[0], job.themes[0]);
    assert!(SHIPPED_LANGUAGES.contains(&language));
    let (workspace_id, _) = layer_workspace::DEFAULT_WORKSPACES.into_iter()
        .find(|(id, _)| id.rsplit(':').next() == Some(scene.workspace.as_str())).expect("included workspace");
    let theme_name = match theme { Theme::Light => "light", Theme::Dark => "dark" };
    let stem = format!("{}-{}-{theme_name}", scene.id, language.tag().to_ascii_lowercase());
    assert!(stem.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'));
    assert_eq!(PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap()).canonicalize().unwrap(), job.output.join("images").canonicalize().unwrap());
    let app = native_test_app("art.capycanvas.StoreCapture");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::new_localized(&app, layer_ui::Localizer::shared(language));
    w.dispatch(UiAction::RestoreSettings { settings: layer_ui::Settings {
        theme: Some(theme), language: layer_ui::LanguagePreference::Explicit(language), ..Default::default()
    }});
    w.window.set_default_size(job.width, job.height);
    let windows = Rc::new(std::cell::RefCell::new(vec![w.clone()]));
    crate::files::launch::install(&app, &windows);
    w.window.present();
    wait_workspaces(&w);
    new_photo::ready(&w);
    w.dispatch(UiAction::WorkspaceManager { command: layer_ui::WorkspaceCommand::Switch { id: workspace_id.into() } });
    until(|| w.workspaces.ready() && !w.workspaces.busy()
        && w.workspaces.manager().unwrap().active_id().as_deref() == Some(workspace_id), "selected workspace");
    let initial_document = w.documents.selected();
    crate::files::launch::open_in(&w, vec![gtk::gio::File::for_path(&scene.source)]);
    until(|| w.documents.selected() != initial_document && w.gpu.borrow().is_some()
        && !w.documents.has_pending_open(), "source document admitted");
    new_photo::finish(&w);
    new_photo::ready(&w);
    w.documents.select(&w, initial_document, true);
    until(|| w.documents.len() == 1 && w.gpu.borrow().is_some(), "unused initial drawing closed");
    new_photo::ready(&w);
    w.window.unmaximize();
    w.window.set_default_size(job.width, job.height);
    until(|| !w.window.is_maximized() && w.window.width() == job.width && w.window.height() == job.height,
        "requested unmaximized window geometry");
    for step in &scene.steps { stage(&w, step); }
    for (panel, inspector) in [(layer_ui::Panel::Histogram, &w.histogram), (layer_ui::Panel::Waveform, &w.waveform)] {
        if inspector.root.is_mapped() { super::histogram::monitor_bounds(&w, panel); }
    }
    until(|| widgets(w.layer_panel.root.upcast_ref()).filter(|widget| widget.is_mapped())
        .filter_map(|widget| widget.downcast::<gtk::Picture>().ok()).all(|picture| picture.paintable().is_some()), "visible layer thumbnails");
    let paints = Rc::new(std::cell::Cell::new(0));
    w.window.add_tick_callback(glib::clone!(#[strong] paints, move |_, _| {
        paints.set(paints.get() + 1);
        if paints.get() >= 3 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
    }));
    until(|| {
        let gpu = w.gpu.borrow();
        let stats = gpu.as_ref().unwrap().session.engine().backend().stats.lock().unwrap();
        paints.get() >= 3 && w.frame_timer.borrow().is_none()
            && stats.camera_views.last().is_some_and(|(frame, _, _)| stats.presented.iter().any(|feedback| feedback[0] >= *frame && feedback[3] == 1))
    }, "final canvas presentation and GTK frames");
    assert_eq!(w.localization().language(), language);
    assert_eq!(state(&w).theme, theme);
    assert_eq!((w.window.width(), w.window.height()), (job.width, job.height), "requested logical window size");
    assert_eq!(w.window.scale_factor(), job.scale as i32, "real Wayland monitor scale");
    assert!(!w.window.is_maximized() && w.window.visible_dialog().is_none());
    assert!(state(&w).customization.expanded.is_none(), "panel configuration closed");
    assert!(w.popovers.borrow().iter().filter_map(|popover| popover.upgrade()).all(|popover| !popover.is_visible()), "popovers closed");
    assert_eq!(gtk::Window::list_toplevels().iter().filter(|window| window.is_mapped()).count(), 1, "single capture window");
    assert!(!w.status.is_visible(), "{}", w.status.text());
    let mut labels = 0;
    for widget in widgets(w.window.upcast_ref()).filter(|widget| widget.is_mapped()) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            assert_eq!(label.layout().unknown_glyphs_count(), 0, "missing glyphs in {}: {}", language.tag(), label.text());
            labels += 1;
        }
    }
    let mut input = RemoteInput::new().timeout_secs(60);
    input.ready();
    w.window.present();
    until(|| w.window.is_active(), "focused capture window");
    input.perform(serde_json::json!([{ "capture_window": stem }]));
    input.finish();
    let state = state(&w);
    let manifest = serde_json::json!({
        "source_revision": job.source_revision, "recipe_sha256": job.recipe_sha256,
        "source_dirty": job.source_dirty, "executable_sha256": job.executable_sha256,
        "executable_override": job.executable_override,
        "width": job.width, "height": job.height, "scale": job.scale,
        "languages": [language.tag()], "themes": [theme_name],
        "supported_languages": SHIPPED_LANGUAGES.iter().map(|language| language.tag()).collect::<Vec<_>>(),
        "captures": [{"scene": scene.id, "language": language.tag(), "theme": theme_name,
            "image": format!("images/{stem}.png"), "sidecar": format!("images/{stem}.json"),
            "window": [w.window.width(), w.window.height()], "camera": state.camera,
            "document": state.document_file.title(), "layer_count": state.layers.len(),
            "visible_labels": labels, "missing_glyphs": 0}],
    });
    std::fs::write(job.output.join("capture.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    w.window.close();
    until(|| !w.window.is_visible(), "capture window closed");
}
