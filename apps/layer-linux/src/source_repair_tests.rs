use super::new_photo::{capture_ui, chooser, finish, invoke, ready, response};
use super::place_source::{snapshot, source};
use super::*;
use layer_core::color::{ColorProfile, RgbSpace};

fn profile_dialog(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(20);
        if w.window
            .visible_dialog()
            .is_some_and(|d| d.widget_name() == "source-profile-dialog")
        {
            break;
        }
        assert!(Instant::now() < deadline, "source profile dialog");
    }
}
fn preview_ready(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(20);
        let dialog = w
            .window
            .visible_dialog()
            .unwrap()
            .downcast::<adw::AlertDialog>()
            .unwrap();
        if dialog.is_response_enabled("apply") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{}",
            find_named(dialog.upcast_ref(), "color-preview-status")
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .label()
        );
    }
}
fn layer(w: &Rc<Workspace>, id: layer_core::LayerId) -> layer_core::Layer {
    w.gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .layer(id)
        .unwrap()
        .clone()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_source_profile_repair_preserves_originals_and_baked_edits() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.SourceRepair");
    let mut project = new_drawing(256, 128).unwrap();
    let id = project.document.active_layer;
    let original = std::sync::Arc::new(source());
    project
        .document
        .layers
        .iter_mut()
        .find(|l| l.id == id)
        .unwrap()
        .source = Some(original);
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let directory = std::path::Path::new("../../artifacts/color-m2/color-preview-ui").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let gray =
        layer_color::profile_bytes(&layer_color::gray_profile(RgbSpace::Srgb).unwrap()).unwrap();
    let gray_path = directory.join("wrong-gray.icc");
    std::fs::write(&gray_path, gray).unwrap();
    let adobe = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    let adobe_path = directory.join("Adobe input.icc");
    std::fs::write(&adobe_path, &adobe).unwrap();
    let clean = snapshot(&w);
    let pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9901))
        .unwrap();
    // Exercise both the layer action and the File menu command routes.
    w.dispatch(UiAction::Layer {
        action: LayerAction::RepairSourceProfile { id: id.0 },
    });
    profile_dialog(&w);
    super::new_photo::profile_action(&w, "source", "builtin-0");
    preview_ready(&w);
    pump(200);
    capture_ui(&w, &directory, "untouched-source.png");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), clean);
    // Cancel an active preview and replace queued choices rapidly. Completion
    // must acknowledge cancellation without publishing any artwork or stale UI.
    invoke(&w, CommandId::RepairSourceProfile);
    profile_dialog(&w);
    super::new_photo::profile_action(&w, "source", "builtin-0");
    pump(1);
    for index in [1, 2, 0, 3] {
        super::new_photo::profile_action(&w, "source", &format!("builtin-{index}"));
    }
    let dialog = w
        .window
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(!dialog.is_response_enabled("apply"));
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), clean);
    invoke(&w, CommandId::RepairSourceProfile);
    profile_dialog(&w);
    let choose_profile = |path: &std::path::Path| {
        let dialog = w.window.visible_dialog().unwrap();
        super::new_photo::profile_action(&w, "source", "add");
        let file = chooser();
        file.set_file(&gtk::gio::File::for_path(path)).unwrap();
        pump(200);
        file.response(gtk::ResponseType::Accept);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            pump(20);
            let button = find_named(dialog.upcast_ref(), "source-profile-choose").unwrap();
            if button.is_sensitive() {
                break;
            }
            assert!(Instant::now() < deadline);
        }
    };
    choose_profile(&gray_path);
    let dialog = w
        .window
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(!dialog.is_response_enabled("apply"));
    let error = find_named(dialog.upcast_ref(), "source-profile-error")
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
    assert!(error.is_visible());
    assert!(error.label().contains("RGB"), "{}", error.label());
    assert_eq!(snapshot(&w), clean);
    pump(200);
    capture_ui(&w, &directory, "mismatched-source-profile.png");
    choose_profile(&adobe_path);
    preview_ready(&w);
    assert!(dialog.is_response_enabled("apply"));
    preview_ready(&w);
    response(&w, "apply");
    finish(&w);
    ready(&w);
    assert_ne!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&w, 9902))
            .unwrap()
            .bytes,
        pixels.bytes
    );
    invoke(&w, CommandId::Pen);
    w.dispatch(UiAction::SetBrushSize { value: 23. });
    ready(&w);
    native_pen_path(&w, &[[30., 40.], [50., 40.], [90., 40.]]);
    ready(&w);
    let baked = layer(&w, id);
    assert!(!baked.raster.wait_data().unwrap().tiles.is_empty());
    let baked_bytes = snapshot(&w);
    invoke(&w, CommandId::RepairSourceProfile);
    profile_dialog(&w);
    super::new_photo::profile_action(&w, "source", "builtin-3");
    preview_ready(&w);
    pump(200);
    capture_ui(&w, &directory, "baked-source-choice.png");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), baked_bytes);
    invoke(&w, CommandId::RepairSourceProfile);
    profile_dialog(&w);
    super::new_photo::profile_action(&w, "source", "builtin-3");
    preview_ready(&w);
    response(&w, "apply");
    finish(&w);
    ready(&w);
    let saved = snapshot(&w);
    let project =
        layer_core::Project::read(std::io::Cursor::new(saved.clone()), Default::default()).unwrap();
    let restored = Workspace::with_project(&app, Some((project, None)));
    restored.window.present();
    ready(&restored);
    assert_eq!(snapshot(&restored), saved);
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&restored, 9904))
            .unwrap()
            .bytes,
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&w, 9905))
            .unwrap()
            .bytes
    );
    restored.window.destroy();
    w.window.destroy();
    pump(100);
}
