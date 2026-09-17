use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::*;
use layer_core::color::*;

fn dialog(w: &Rc<Workspace>, name: &str) -> adw::AlertDialog {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        pump(20);
        if let Some(dialog) = w
            .window
            .visible_dialog()
            .filter(|d| d.widget_name() == name)
        {
            return dialog.downcast().unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "waiting for {name}: {}",
            w.status.text()
        );
    }
}
#[allow(deprecated)]
fn choose(path: &std::path::Path) {
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(path)).unwrap();
    pump(200);
    file.response(gtk::ResponseType::Accept);
}
fn click_named(root: &gtk::Widget, name: &str) {
    click(
        &find_named(root, name)
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
}
fn preferences(w: &Rc<Workspace>) {
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Color,
    });
    pump(300);
    assert!(w.preferences.dialog.is_mapped());
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_color_preferences_profiles_and_untagged_photo_policy() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ColorPreferences");
    let w = Workspace::with_project(&app, Some((new_drawing(64, 64).unwrap(), None)));
    w.window.present();
    ready(&w);
    let created = Rc::new(RefCell::new(None));
    *w.open_document.borrow_mut() = Some({
        let created = created.clone();
        Rc::new(move |project, location, recovery| {
            assert!(location.is_none());
            assert!(recovery.is_none());
            *created.borrow_mut() = Some(project);
        })
    });
    let original = super::place_source::snapshot(&w);
    let output = std::path::PathBuf::from(format!(
        "../../artifacts/color-m2/color-preferences-ui/{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    let output = output.canonicalize().unwrap();
    preferences(&w);
    for (name, selected) in [
        ("new-color-space", 3),
        ("new-bit-depth", 1),
        ("new-background", 1),
        ("photo-depth", 1),
        ("missing-profile", 1),
    ] {
        let row = find_named(
            w.preferences.dialog.upcast_ref(),
            &format!("setting-{name}"),
        )
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap();
        row.set_selected(selected);
        pump(50);
    }
    finish(&w);
    assert_eq!(
        state(&w).settings.photo_open,
        PhotoOpenPolicy {
            promote_to_16: true,
            missing_profile: MissingProfilePolicy::Ask
        }
    );
    assert_eq!(
        state(&w).settings.new_document.defaults.color.space,
        RgbSpace::ProPhoto
    );
    assert_eq!(
        crate::preferences::load().unwrap().unwrap(),
        state(&w).settings
    );
    assert_eq!(super::place_source::snapshot(&w), original);
    capture_ui(&w, &output, "color-settings.png");
    click_named(w.preferences.dialog.upcast_ref(), "color-drawing-defaults");
    dialog(&w, "drawing-defaults-dialog");
    combo(&w, "new-document-preset").set_selected(2);
    let row = find_named(
        w.window.visible_dialog().unwrap().upcast_ref(),
        "new-document-width",
    )
    .unwrap()
    .downcast::<adw::SpinRow>()
    .unwrap();
    row.set_value(128.);
    response(&w, "create");
    finish(&w);
    assert!(
        created.borrow().is_none(),
        "editing defaults never creates a drawing"
    );
    assert_eq!(
        state(&w).settings.new_document.defaults.color,
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U8
        }
    );
    assert_eq!(state(&w).settings.new_document.defaults.extent[0], 128);

    let profile_bytes =
        layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    let profile = output.join("Adobe working profile.icc");
    std::fs::write(&profile, &profile_bytes).unwrap();
    click_named(w.preferences.dialog.upcast_ref(), "color-profile-library");
    let manager = dialog(&w, "profile-library-manager");
    click_named(manager.upcast_ref(), "profile-library-import");
    choose(&profile);
    let list = find_named(manager.upcast_ref(), "profile-library-list")
        .unwrap()
        .downcast::<gtk::ListBox>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while list.row_at_index(0).is_none() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    capture_ui(&w, &output, "profile-library.png");
    response(&w, "close");
    w.dispatch(UiAction::CloseSettings);
    pump(200);

    // A real untagged PNG, independent of the profile-aware writer's defaults.
    let path = output.join("Untagged photo.png");
    let pixels: Vec<u8> = (0..32 * 16).flat_map(|_| [60, 120, 180, 128]).collect();
    {
        let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 32, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    let source = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(&path).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert!(source.interpretation.profile_assumed);
    invoke(&w, CommandId::OpenDocument);
    choose(&path);
    dialog(&w, "untagged-profile-dialog");
    response(&w, "cancel");
    finish(&w);
    assert!(created.borrow().is_none());
    assert_eq!(super::place_source::snapshot(&w), original);
    invoke(&w, CommandId::OpenDocument);
    choose(&path);
    dialog(&w, "untagged-profile-dialog");
    super::new_photo::profile_action(&w, "source", "saved-0");
    dialog(&w, "untagged-profile-dialog");
    capture_ui(&w, &output, "untagged-profile.png");
    response(&w, "use");
    finish(&w);
    let project = created.borrow_mut().take().unwrap();
    assert_eq!(
        project.document.color,
        DocumentColor {
            space: RgbSpace::AdobeRgb,
            depth: IntegerDepth::U16
        }
    );
    let retained = project.document.layers[0].source.as_ref().unwrap();
    assert_eq!(retained.interpretation.depth, IntegerDepth::U8);
    let mut expected = source.clone();
    expected.interpretation = retained.interpretation.clone();
    assert_eq!(retained.as_ref(), &expected);
    assert!(!retained.interpretation.profile_assumed);
    assert_eq!(
        retained.interpretation.profile,
        ColorProfile::Icc(profile_bytes.clone().into())
    );

    // The same library entry serves delivery, with the exact profile embedded.
    invoke(&w, CommandId::ExportDocument);
    dialog(&w, "export-options");
    super::new_photo::profile_action(&w, "export", "saved-0");
    let export = dialog(&w, "export-options");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !export.is_response_enabled("export") {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    response(&w, "export");
    let file = chooser();
    #[allow(deprecated)]
    {
        file.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
            .unwrap();
        file.set_current_name("Library delivery.png");
        pump(200);
        file.response(gtk::ResponseType::Accept);
    }
    finish(&w);
    let delivery = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(output.join("Library delivery.png")).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        delivery.interpretation.profile,
        ColorProfile::Icc(profile_bytes.clone().into())
    );
    assert_eq!(super::place_source::snapshot(&w), original);

    preferences(&w);
    click_named(w.preferences.dialog.upcast_ref(), "color-profile-library");
    let manager = dialog(&w, "profile-library-manager");
    let list = find_named(manager.upcast_ref(), "profile-library-list")
        .unwrap()
        .downcast::<gtk::ListBox>()
        .unwrap();
    list.select_row(list.row_at_index(0).as_ref());
    click_named(manager.upcast_ref(), "profile-library-remove");
    let deadline = Instant::now() + Duration::from_secs(10);
    while list.row_at_index(0).is_some() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    response(&w, "close");
    w.dispatch(UiAction::CloseSettings);
    pump(200);
    assert_eq!(std::fs::read(&profile).unwrap(), profile_bytes);
    let mut archive = Vec::new();
    project.write(&mut archive).unwrap();
    assert_eq!(
        layer_core::Project::read(std::io::Cursor::new(archive), Default::default()).unwrap(),
        project
    );

    // Place/Paste use the same missing-profile policy without promoting the
    // destination or modifying source numbers. Cancellation leaves it exact.
    let provider = gtk::gdk::ContentProvider::for_bytes(
        "image/png",
        &glib::Bytes::from_owned(std::fs::read(&path).unwrap()),
    );
    w.window.clipboard().set_content(Some(&provider)).unwrap();
    invoke(&w, CommandId::PasteImage);
    dialog(&w, "untagged-profile-dialog");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(super::place_source::snapshot(&w), original);
    invoke(&w, CommandId::PasteImage);
    dialog(&w, "untagged-profile-dialog");
    super::new_photo::profile_action(&w, "source", "builtin-1");
    response(&w, "use");
    finish(&w);
    ready(&w);
    let gpu = w.gpu.borrow();
    let document = gpu.as_ref().unwrap().session.engine().document();
    assert_eq!(document.color, DocumentColor::default());
    let pasted = document
        .layer(document.active_layer)
        .unwrap()
        .source
        .as_ref()
        .unwrap();
    assert_eq!(
        pasted.interpretation.profile,
        ColorProfile::Builtin(RgbSpace::DisplayP3)
    );
    expected.interpretation = pasted.interpretation.clone();
    assert_eq!(pasted.as_ref(), &expected);
    drop(gpu);
    w.window
        .clipboard()
        .set_content(None::<&gtk::gdk::ContentProvider>)
        .unwrap();
    w.window.close();
    pump(100);
}
