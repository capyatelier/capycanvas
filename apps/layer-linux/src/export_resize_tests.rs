//! Native sizing choices deliver a copy and release their dialog state.
use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_export_sizes_preserve_master_and_release_cancelled_dialogs() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ExportSizes");
    let output = std::path::Path::new("../../artifacts/color-m2/export-resize-native")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    let output = output.canonicalize().unwrap();
    let mut project = new_drawing(192, 128).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    project.document.resolution = Some(layer_core::ImageResolution::ppi(600));
    project.document.layers[1].visible = false;
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U16,
        profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
        profile_assumed: false,
    };
    let mut builder = SourceBuilder::new([192, 128], interpretation, 1024 * 1024).unwrap();
    for y in 0..128u16 {
        let row: Vec<u8> = (0..192u16)
            .flat_map(|x| {
                [
                    x * 341,
                    y * 511,
                    if x % 16 < 8 { 65535 } else { 7000 },
                    if y % 16 < 8 { 65535 } else { 32768 },
                ]
                .into_iter()
                .flat_map(u16::to_le_bytes)
            })
            .collect();
        builder.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    // An actual edit makes dirty-state preservation observable.
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_asset(
            "Mark",
            layer_core::ProjectAsset {
                extent: [1, 1],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: vec![255, 0, 0, 255].into(),
            },
        )
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    ready(&w);
    let before = snapshot(&w);
    let revision = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .revision;
    assert!(state(&w).document_file.modified);
    for cancel in 0..3 {
        invoke(&w, CommandId::ExportDocument);
        let size = combo(&w, "export-size");
        assert_eq!(size.selected(), 0);
        size.set_selected(1);
        let weak = size.downgrade();
        drop(size);
        response(&w, "cancel");
        finish(&w);
        let deadline = Instant::now() + Duration::from_secs(3);
        while weak.upgrade().is_some() && Instant::now() < deadline {
            pump(20);
        }
        assert!(
            weak.upgrade().is_none(),
            "cancelled dialog {cancel} retained sizing widgets"
        );
        assert_eq!(snapshot(&w), before);
    }
    for (format, bounds, enlarge, expected, extension) in [
        (0, [75, 75], false, [75, 50], "png"),
        (1, [500, 500], false, [192, 128], "tif"),
        (2, [300, 300], true, [300, 200], "jpg"),
    ] {
        invoke(&w, CommandId::ExportDocument);
        combo(&w, "export-preset").set_selected(2);
        combo(&w, "export-format").set_selected(format);
        let size = combo(&w, "export-size");
        size.set_selected(1);
        let dialog = w.window.visible_dialog().unwrap();
        for (name, value) in ["export-width", "export-height"].into_iter().zip(bounds) {
            let row = find_named(dialog.upcast_ref(), name)
                .unwrap()
                .downcast::<adw::SpinRow>()
                .unwrap();
            assert!(row.is_visible());
            row.set_value(f64::from(value));
        }
        let allow = find_named(dialog.upcast_ref(), "export-enlarge")
            .unwrap()
            .downcast::<adw::SwitchRow>()
            .unwrap();
        allow.set_active(enlarge);
        // Physical density remains independent of delivery pixel dimensions.
        combo(&w, "export-resolution").set_selected(format);
        if format == 1 {
            find_named(dialog.upcast_ref(), "export-ppi")
                .unwrap()
                .downcast::<adw::SpinRow>()
                .unwrap()
                .set_value(300.);
        }
        let note = find_named(dialog.upcast_ref(), "export-size-description")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert!(
            note.text()
                .contains(&format!("{} × {} px", expected[0], expected[1]))
        );
        assert_eq!(combo(&w, "export-preset").selected(), 3);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .revision,
            revision
        );
        let after = find_named(dialog.upcast_ref(), "color-preview-after")
            .unwrap()
            .downcast::<gtk::Picture>()
            .unwrap();
        let status = find_named(dialog.upcast_ref(), "color-preview-status")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while after.paintable().is_none() {
            assert!(Instant::now() < deadline, "preview: {}", status.text());
            pump(20);
        }
        assert!(status.text().starts_with("Output preview"));
        let texture = after
            .paintable()
            .unwrap()
            .downcast::<gdk::Texture>()
            .unwrap();
        let preview_extent = if format == 2 { [220, 147] } else { expected };
        assert_eq!(
            [texture.width() as u32, texture.height() as u32],
            preview_extent
        );
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_color_state(&w.view_color().state());
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (preview_bytes, preview_stride) = downloader.download_bytes();
        assert_eq!(
            find_named(dialog.upcast_ref(), "export-preview-compression")
                .unwrap()
                .is_visible(),
            format == 2
        );
        if format == 2 {
            find_named(dialog.upcast_ref(), "export-jpeg-quality")
                .unwrap()
                .downcast::<adw::SpinRow>()
                .unwrap()
                .set_value(55.);
            pump(50);
            assert_eq!(
                after
                    .paintable()
                    .unwrap()
                    .downcast::<gdk::Texture>()
                    .unwrap(),
                texture,
                "excluded JPEG compression cannot invalidate the color preview"
            );
        }
        pump(100);
        capture_ui(&w, &output, &format!("{extension}-sizing.png"));
        response(&w, "export");
        let file = chooser();
        file.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
            .unwrap();
        file.set_current_name(&format!("copy.{extension}"));
        pump(350);
        file.response(gtk::ResponseType::Accept);
        finish(&w);
        let result = layer_color::photo::read_photo(
            std::io::BufReader::new(
                std::fs::File::open(output.join(format!("copy.{extension}"))).unwrap(),
            ),
            Default::default(),
        )
        .unwrap();
        assert_eq!(result.extent, expected);
        match format {
            0 => assert!((result.resolution.unwrap().pixels_per_inch()[0] - 600.).abs() < 0.013),
            1 => assert_eq!(
                result.resolution,
                Some(layer_core::ImageResolution::ppi(300))
            ),
            _ => assert!(result.resolution.is_none()),
        }
        if format != 2 {
            // Both lossless copies fit the preview without further reduction.
            // Interpret actual file samples and independently composite the
            // native checker; allow two view codes for ICC/texture rounding.
            let decoder = layer_color::WorkingDecoder::new(
                &result.interpretation,
                w.view_color().space(),
                Default::default(),
            )
            .unwrap();
            let mut rows = result.rows();
            let mut row = vec![0; result.row_bytes()];
            let mut pixels = vec![[0.; 4]; result.extent[0] as usize];
            for y in 0..result.extent[1] {
                rows.read(y, &mut row).unwrap();
                decoder.decode_pixels(&row, &mut pixels).unwrap();
                for (x, pixel) in pixels.iter().enumerate() {
                    let checker = if (x / 8 + y as usize / 8) % 2 == 0 {
                        0.94
                    } else {
                        0.80
                    };
                    let actual = &preview_bytes[y as usize * preview_stride + x * 4..][..4];
                    assert_eq!(actual[3], 255);
                    for c in 0..3 {
                        let linear = f64::from(pixel[c]) * f64::from(pixel[3])
                            + checker * (1. - f64::from(pixel[3]));
                        let expected = (w.view_color().space().encode(linear).clamp(0., 1.) * 255.)
                            .round() as u8;
                        assert!(
                            actual[c].abs_diff(expected) <= 2,
                            "{extension} ({x},{y}) c{c}: {} vs {expected}",
                            actual[c]
                        );
                    }
                }
            }
        }
        assert_eq!(
            result.interpretation.depth,
            if format == 2 {
                SampleDepth::U8
            } else {
                SampleDepth::U16
            }
        );
        assert_eq!(
            layer_color::profile_bytes(&result.interpretation.profile).unwrap(),
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::ProPhoto)).unwrap()
        );
        assert_eq!(snapshot(&w), before);
        assert!(state(&w).document_file.modified);
        assert!(state(&w).document_file.location.is_none());
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display"]
fn native_alert_wait_releases_responses_and_abandoned_futures() {
    use std::future::Future;
    let app = native_test_app("art.capycanvas.AlertLifetime");
    let parent = adw::ApplicationWindow::builder()
        .application(&*app)
        .default_width(640)
        .default_height(480)
        .build();
    // As in the editor, the parent has a focus target to restore on dismissal.
    let anchor = gtk::Button::with_label("Editor content");
    parent.set_content(Some(&anchor));
    parent.present();
    anchor.grab_focus();
    pump(500);
    for end in ["accept", "cancel", "drop"] {
        let child = gtk::Label::new(Some("Owned dialog content"));
        let weak_child = child.downgrade();
        let dialog = adw::AlertDialog::builder()
            .heading("Lifetime check")
            .extra_child(&child)
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("accept", "Accept")]);
        dialog.set_close_response("cancel");
        let weak_dialog = dialog.downgrade();
        drop(child);
        let mut future = Box::pin(crate::alert::choose(dialog, &parent));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        pump(100);
        if end != "drop" {
            let dialog = weak_dialog.upgrade().unwrap();
            if end == "accept" {
                find_button(dialog.upcast_ref(), "Accept")
                    .unwrap()
                    .emit_clicked();
            } else {
                dialog.force_close();
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            let result = loop {
                if let std::task::Poll::Ready(response) = future.as_mut().poll(&mut context) {
                    break response;
                }
                assert!(
                    Instant::now() < deadline,
                    "{end} response was not delivered"
                );
                pump(20);
            };
            assert_eq!(result, end);
        }
        drop(future);
        let deadline = Instant::now() + Duration::from_secs(3);
        while (weak_dialog.upgrade().is_some() || weak_child.upgrade().is_some())
            && Instant::now() < deadline
        {
            pump(20);
        }
        assert!(weak_dialog.upgrade().is_none(), "{end} retained its dialog");
        assert!(weak_child.upgrade().is_none(), "{end} retained its content");
        assert!(parent.visible_dialog().is_none());
    }
    parent.destroy();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_export_presets_save_update_remove_reset_and_remember_after_delivery() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ExportPresets");
    let directory = std::path::Path::new("../../artifacts/color-m2/export-presets-native")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let path = std::env::var_os("LAYER_SETTINGS_FILE")
        .map(std::path::PathBuf::from)
        .unwrap()
        .with_file_name("export-presets.json");
    let mut library = ExportPresets::default();
    let mut custom = ExportRecipe::further_editing(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    custom.profile.profile = ColorProfile::Icc(
        layer_color::profile_bytes(&custom.profile.profile)
            .unwrap()
            .into(),
    );
    custom.profile.name = "Embedded lab RGB".into();
    custom.resolution = ExportResolution::Ppi(240);
    custom.size = ExportSize::Fit {
        bounds: [37, 29],
        enlarge: false,
    };
    library.save("Lab RGB", custom.clone()).unwrap();
    std::fs::write(&path, library.encode().unwrap()).unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing(64, 48).unwrap(), None)));
    w.window.present();
    ready(&w);
    let master = snapshot(&w);
    let settings = state(&w).settings.clone();
    let widget =
        |name: &str| find_named(w.window.visible_dialog().unwrap().upcast_ref(), name).unwrap();
    let press = |name: &str| click(&widget(name).downcast::<gtk::Button>().unwrap());
    let wait_saved = || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            pump(20);
            if let Some(dialog) = w.window.visible_dialog()
                && dialog.widget_name() == "export-options"
                && let Some(status) = find_named(dialog.upcast_ref(), "export-presets-status")
                && status.is_visible()
                && find_named(dialog.upcast_ref(), "export-preset-save")
                    .unwrap()
                    .is_sensitive()
            {
                let row = status.downcast::<adw::ActionRow>().unwrap();
                assert!(!row.has_css_class("error"), "{}", row.title());
                return;
            }
            assert!(Instant::now() < deadline, "preset save");
        }
    };
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-preset").set_selected(4);
    assert_eq!(super::new_photo::profile_name(&w, "export-space"), custom.profile.name);
    assert_eq!(combo(&w, "export-format").selected(), 1);
    assert_eq!(combo(&w, "export-depth").selected(), 1);
    assert_eq!(combo(&w, "export-resolution").selected(), 1);
    assert_eq!(
        widget("export-ppi")
            .downcast::<adw::SpinRow>()
            .unwrap()
            .value(),
        240.
    );
    assert_eq!(
        widget("export-width")
            .downcast::<adw::SpinRow>()
            .unwrap()
            .value(),
        37.
    );
    press("export-preset-save");
    pump(150);
    widget("export-preset-name")
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text("Lab copy");
    response(&w, "save");
    wait_saved();
    assert_eq!(combo(&w, "export-preset").selected(), 5);
    widget("export-width")
        .downcast::<adw::SpinRow>()
        .unwrap()
        .set_value(21.);
    assert_eq!(combo(&w, "export-preset").selected(), 3);
    press("export-preset-update");
    wait_saved();
    let persisted = ExportPresets::decode(&std::fs::read(&path).unwrap()).unwrap();
    let saved = persisted.recipe(5, Default::default()).unwrap();
    assert_eq!(
        saved.size,
        ExportSize::Fit {
            bounds: [21, 29],
            enlarge: false
        }
    );
    assert_eq!(saved.profile, custom.profile);
    assert_eq!(saved.resolution, custom.resolution);
    let preview = widget("color-preview-after")
        .downcast::<gtk::Picture>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while preview.paintable().is_none() {
        pump(20);
        assert!(Instant::now() < deadline, "saved preset preview");
    }
    capture_ui(&w, &directory, "saved-presets.png");
    response(&w, "cancel");
    finish(&w);
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-preset").set_selected(5);
    assert_eq!(
        widget("export-width")
            .downcast::<adw::SpinRow>()
            .unwrap()
            .value(),
        21.
    );
    // Cancelling a file chooser must not remember temporary destination changes.
    combo(&w, "export-preset").set_selected(1);
    combo(&w, "export-depth").set_selected(1);
    response(&w, "export");
    chooser().response(gtk::ResponseType::Cancel);
    finish(&w);
    assert_eq!(
        ExportPresets::decode(&std::fs::read(&path).unwrap()).unwrap(),
        persisted
    );
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-preset").set_selected(1);
    assert_eq!(combo(&w, "export-depth").selected(), 0);
    combo(&w, "export-size").set_selected(1);
    for name in ["export-width", "export-height"] {
        widget(name)
            .downcast::<adw::SpinRow>()
            .unwrap()
            .set_value(31.);
    }
    response(&w, "export");
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&directory)))
        .unwrap();
    file.set_current_name("remembered.png");
    pump(200);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    let remembered = ExportPresets::decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        remembered.recipe(1, Default::default()).unwrap().size,
        ExportSize::Fit {
            bounds: [31, 31],
            enlarge: false
        }
    );
    let image = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(directory.join("remembered.png")).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(image.extent, [31, 23]);
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-preset").set_selected(1);
    assert_eq!(combo(&w, "export-size").selected(), 1);
    press("export-preset-reset");
    wait_saved();
    assert_eq!(combo(&w, "export-size").selected(), 0);
    combo(&w, "export-preset").set_selected(5);
    press("export-preset-remove");
    wait_saved();
    assert_eq!(
        ExportPresets::decode(&std::fs::read(&path).unwrap())
            .unwrap()
            .names()
            .collect::<Vec<_>>(),
        vec!["Lab RGB"]
    );
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), master);
    assert_eq!(state(&w).settings, settings);
    assert!(!state(&w).document_file.modified);
    w.window.destroy();
    pump(200);
}
