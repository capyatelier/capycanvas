use super::new_photo::{capture_ui, chooser, finish, invoke, ready, response};
use super::place_source::{snapshot, source};
use super::*;
use layer_core::color::{ColorProfile, RgbSpace};

fn layer(w: &Rc<Workspace>, id: layer_core::OccurrenceHandle) -> layer_core::PaintSource {
    ui_session(w).engine().document().scene().paint_source(id).unwrap().clone()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_source_profile_repair_preserves_originals_and_baked_edits() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.SourceRepair");
    let mut project = new_drawing(256, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let id = project.working.occurrence.unwrap();
    let original = std::sync::Arc::new(source());
    let target = project.working.target.unwrap();
    let layer_core::SourceTarget::Paint(handle) = target else { panic!("Paint source") };
    let paint = project.artwork.paint.get_mut(handle).unwrap();
    paint.domain = original.extent;
    paint.original = Some(original);
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
        action: LayerAction::RepairSourceProfile { id: layer_ui::occurrence_token(id) },
    });
    apply_dialog(&w, "source-profile-dialog", false);
    super::new_photo::profile_action(&w, "source", "builtin-0");
    apply_dialog(&w, "source-profile-dialog", true);
    pump(200);
    capture_ui(&w, &directory, "untouched-source.png");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), clean);
    // Cancel an active preview and replace queued choices rapidly. Completion
    // must acknowledge cancellation without publishing any artwork or stale UI.
    invoke(&w, CommandId::RepairSourceProfile);
    apply_dialog(&w, "source-profile-dialog", false);
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
    apply_dialog(&w, "source-profile-dialog", false);
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
    let error = named::<gtk::Label>(dialog.upcast_ref(), "source-profile-error");
    assert!(error.is_visible());
    assert!(error.label().contains("RGB"), "{}", error.label());
    assert_eq!(snapshot(&w), clean);
    pump(200);
    capture_ui(&w, &directory, "mismatched-source-profile.png");
    choose_profile(&adobe_path);
    apply_dialog(&w, "source-profile-dialog", true);
    assert!(dialog.is_response_enabled("apply"));
    apply_dialog(&w, "source-profile-dialog", true);
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
    apply_dialog(&w, "source-profile-dialog", false);
    super::new_photo::profile_action(&w, "source", "builtin-3");
    apply_dialog(&w, "source-profile-dialog", true);
    pump(200);
    capture_ui(&w, &directory, "baked-source-choice.png");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), baked_bytes);
    invoke(&w, CommandId::RepairSourceProfile);
    apply_dialog(&w, "source-profile-dialog", false);
    super::new_photo::profile_action(&w, "source", "builtin-3");
    apply_dialog(&w, "source-profile-dialog", true);
    response(&w, "apply");
    finish(&w);
    ready(&w);
    let saved = snapshot(&w);
    let project =
        open_native_document(std::io::Cursor::new(saved.clone()));
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
