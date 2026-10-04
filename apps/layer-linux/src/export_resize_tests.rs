//! Native sizing choices deliver a copy and release their dialog state.
use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};

trait NumericEdit {
    fn edit_value(&self, value: f64);
}
impl NumericEdit for crate::number_control::NumberControl {
    fn edit_value(&self, value: f64) {
        descendant::<gtk::Stack>(self).unwrap().set_visible_child_name("entry");
        descendant::<gtk::Entry>(self).unwrap().set_text(&value.to_string());
        assert!(self.commit_text());
    }
}

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
    let mut project = new_drawing(192, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut project).color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    composition_mut(&mut project).resolution = Some(layer_core::ImageResolution::ppi(600));
    let paper = project.scene().order()[1]; project.artwork.occurrences.get_mut(paper).unwrap().visible = false;
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
    paint_at_mut(&mut project, 0).original = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    apply_fixture_theme(&w);
    ready(&w);
    // An actual edit makes dirty-state preservation observable.
    ui_session_mut(&w)
        .import_layer_source(
            "Mark",
            std::sync::Arc::unwrap_or_clone(layer_core::color::source::rgba8_source([1, 1], |_, _| {
                [255, 0, 0, 255]
            })),
        )
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    ready(&w);
    let before = snapshot(&w);
    let revision = ui_session(&w)
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
            let row = named::<crate::number_control::NumberControl>(dialog.upcast_ref(), name);
            assert!(row.is_visible());
            row.edit_value(f64::from(value));
        }
        let allow = named::<adw::SwitchRow>(dialog.upcast_ref(), "export-enlarge");
        allow.set_active(enlarge);
        // Physical density remains independent of delivery pixel dimensions.
        combo(&w, "export-resolution").set_selected(format);
        if format == 1 {
            named::<crate::number_control::NumberControl>(dialog.upcast_ref(), "export-ppi")
                .edit_value(300.);
        }
        let note = named::<gtk::Label>(dialog.upcast_ref(), "export-size-description");
        assert!(
            note.text()
                .contains(&format!("{} × {} px", expected[0], expected[1]))
        );
        assert_eq!(combo(&w, "export-preset").selected(), 3);
        assert_eq!(
            ui_session(&w)
                .engine()
                .document()
                .revision,
            revision
        );
        let after = named::<gtk::Picture>(dialog.upcast_ref(), "color-preview-after");
        let status = named::<gtk::Label>(dialog.upcast_ref(), "color-preview-status");
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
            named::<crate::number_control::NumberControl>(dialog.upcast_ref(), "export-jpeg-quality")
                .edit_value(55.);
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
                    let cell = layer_ui::TRANSPARENCY_CHECKER_CELL as usize;
                    let checker = f64::from(
                        crate::display_color::checker_linear()[(x / cell + y as usize / cell) % 2],
                    );
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
    library.remember(0, ExportRecipe::web_share().draft_canonical(layer_ui::ExportDraftAction::Format(layer_ui::ExportFormat::JpegHdr)).recipe).unwrap();
    std::fs::write(&path, library.encode().unwrap()).unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.present();
    ready(&w);
    apply_fixture_theme(&w);
    ready(&w);
    let master = snapshot(&w);
    let settings = state(&w).settings.clone();
    let widget =
        |name: &str| find_named(w.window.visible_dialog().unwrap().upcast_ref(), name).unwrap();
    let press = |name: &str| {
        super::new_photo::export_page(&w, "presets");
        widget(name).downcast::<adw::ButtonRow>().unwrap().emit_by_name::<()>("activated", &[]);
    };
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
    assert_eq!(combo(&w, "export-format").selected(), 2);
    assert!(!widget("export-validation").is_visible());
    assert!(super::new_photo::export_enabled(&w));
    combo(&w, "export-preset").set_selected(4);
    assert_eq!(super::new_photo::profile_name(&w, "export-space"), custom.profile.name);
    assert_eq!(combo(&w, "export-format").selected(), 1);
    assert_eq!(combo(&w, "export-depth").selected(), 1);
    assert_eq!(combo(&w, "export-resolution").selected(), 1);
    assert_eq!(
        widget("export-ppi")
            .downcast::<crate::number_control::NumberControl>()
            .unwrap()
            .value(),
        240.
    );
    assert_eq!(
        widget("export-width")
            .downcast::<crate::number_control::NumberControl>()
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
        .downcast::<crate::number_control::NumberControl>()
        .unwrap()
        .edit_value(21.);
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
    until(|| preview.paintable().is_some(), "saved preset preview");
    capture_ui(&w, &directory, "saved-presets.png");
    response(&w, "cancel");
    finish(&w);
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-preset").set_selected(5);
    assert_eq!(
        widget("export-width")
            .downcast::<crate::number_control::NumberControl>()
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
            .downcast::<crate::number_control::NumberControl>()
            .unwrap()
            .edit_value(31.);
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

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_export_webp_to_a_prechosen_file() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ExportWebp");
    let output = std::path::Path::new("../../artifacts/photo-m2/export-webp-native")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    let output = output.canonicalize().unwrap();
    let mut project = new_drawing(192, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut project).resolution = Some(layer_core::ImageResolution::ppi(144));
    let papers: Vec<_> = project.scene().constant_backdrop().to_vec();
    for h in papers { project.artwork.occurrences.get_mut(h).unwrap().visible = false; }
    let pixel = |x: u32, y: u32| [(x * 5 % 256) as u8, (y * 7 % 256) as u8, 180, if x < 96 { 255 } else { 0 }];
    let mut builder = SourceBuilder::new(
        [192, 128],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for y in 0..128 {
        builder.push_row(&(0..192).flat_map(|x| pixel(x, y)).collect::<Vec<_>>()).unwrap();
    }
    paint_at_mut(&mut project, 0).original = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let before = snapshot(&w);
    invoke(&w, CommandId::ExportDocument);
    let format = combo(&w, "export-format");
    let labels: Vec<_> = (0..format.model().unwrap().n_items())
        .map(|i| format.model().unwrap().item(i).and_downcast::<gtk::StringObject>().unwrap().string().to_string())
        .collect();
    assert_eq!(labels, ["PNG", "TIFF", "JPEG", "WebP · lossless"]);
    format.set_selected(3);
    let dialog = w.window.visible_dialog().unwrap();
    let widget = |name: &str| find_named(dialog.upcast_ref(), name).unwrap();
    assert!(!widget("export-jpeg-quality").is_visible(), "lossless WebP has no quality");
    assert!(!widget("export-depth").is_visible(), "WebP's 8-bit depth is fixed");
    assert_eq!(combo(&w, "export-background").selected(), 0, "WebP keeps transparency");
    assert!(super::new_photo::export_enabled(&w));
    let size = combo(&w, "export-size");
    size.set_selected(1);
    for name in ["export-width", "export-height"] {
        widget(name).downcast::<crate::number_control::NumberControl>().unwrap().edit_value(20000.);
    }
    widget("export-enlarge").downcast::<adw::SwitchRow>().unwrap().set_active(true);
    pump(100);
    let validation = widget("export-validation").downcast::<gtk::Label>().unwrap();
    assert!(validation.is_visible() && validation.text().contains("16,384 pixels per side"), "{}", validation.text());
    assert!(!super::new_photo::export_enabled(&w), "an oversized WebP is refused with its reason");
    size.set_selected(0);
    pump(100);
    assert!(!validation.is_visible());
    assert!(super::new_photo::export_enabled(&w));
    capture_ui(&w, &output, "webp-options.png");
    let file = output.join("copy.webp");
    crate::files::choose_next_save(file.clone());
    response(&w, "export");
    finish(&w);
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!((&bytes[..4], &bytes[8..12]), (&b"RIFF"[..], &b"WEBP"[..]));
    let result = layer_color::photo::read_photo(std::io::Cursor::new(bytes), Default::default()).unwrap();
    assert_eq!(result.extent, [192, 128]);
    assert_eq!(result.interpretation.channels, SourceChannels::Rgba);
    assert_eq!(result.interpretation.depth, SampleDepth::U8);
    assert_eq!(result.resolution, Some(layer_core::ImageResolution::ppi(144)));
    assert_eq!(
        layer_color::profile_bytes(&result.interpretation.profile).unwrap(),
        layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap()
    );
    let mut rows = result.rows();
    let mut row = vec![0; result.row_bytes()];
    for y in 0..128 {
        rows.read(y, &mut row).unwrap();
        for (x, actual) in row.chunks_exact(4).enumerate() {
            let expected = pixel(x as u32, y);
            assert_eq!(actual[3], expected[3], "({x},{y}) alpha");
            if expected[3] == 255 {
                assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1), "({x},{y}): {actual:?}");
            }
        }
    }
    assert_eq!(snapshot(&w), before);
    invoke(&w, CommandId::ExportDocument);
    assert_eq!(combo(&w, "export-format").selected(), 3, "the destination remembers WebP");
    response(&w, "cancel");
    finish(&w);
    w.window.destroy();
    pump(100);
}

/// A little-endian Exif block: descriptive IFD0 tags, then the Exif and GPS directories.
fn camera_exif() -> Vec<u8> {
    type Entry = (u16, u16, Vec<u8>);
    let ascii = |tag, text: &str| (tag, 2, [text.as_bytes(), &[0]].concat());
    let rational = |tag, [n, d]: [u32; 2]| (tag, 5, [n.to_le_bytes(), d.to_le_bytes()].concat());
    let directories: [Vec<Entry>; 3] = [
        vec![ascii(0x010f, "Capycam"), ascii(0x0110, "C-1"), ascii(0x013b, "Ada Painter"), ascii(0x8298, "(c) 2026 Ada Painter")],
        vec![rational(0x829a, [1, 250]), ascii(0x9003, "2026:09:01 10:00:00"), ascii(0xa434, "Capy 35mm F1.8")],
        vec![ascii(0x0001, "N"), ascii(0x0012, "WGS-84")],
    ];
    let size = |entries: &[Entry]| {
        6 + 12 * entries.len() + entries.iter().filter(|e| e.2.len() > 4).map(|e| e.2.len().next_multiple_of(2)).sum::<usize>()
    };
    let exif_at = 8 + size(&directories[0]) + 24;
    let gps_at = exif_at + size(&directories[1]);
    let mut image = directories[0].clone();
    image.push((0x8769, 4, (exif_at as u32).to_le_bytes().to_vec()));
    image.push((0x8825, 4, (gps_at as u32).to_le_bytes().to_vec()));
    let mut out = b"II\x2a\0\x08\0\0\0".to_vec();
    for entries in [&image, &directories[1], &directories[2]] {
        let mut data_at = out.len() + 6 + 12 * entries.len();
        let mut data = Vec::new();
        out.extend((entries.len() as u16).to_le_bytes());
        for (tag, kind, value) in entries.iter() {
            let width = if *kind == 5 { 8 } else if *kind == 4 { 4 } else { 1 };
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(((value.len() / width) as u32).to_le_bytes());
            if value.len() <= 4 {
                out.extend(value.iter().copied().chain(std::iter::repeat(0)).take(4));
            } else {
                out.extend((data_at as u32).to_le_bytes());
                data.extend(value);
                if value.len() % 2 == 1 {
                    data.push(0);
                }
                data_at += value.len().next_multiple_of(2);
            }
        }
        out.extend(0u32.to_le_bytes());
        out.extend(data);
    }
    assert_eq!(out.len(), gps_at + size(&directories[2]));
    out
}

const CAMERA_XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" photoshop:City="Lisbon" exif:GPSLatitude="38,42.5N" xmpMM:InstanceID="xmp.iid:1"><dc:creator><rdf:Seq><rdf:li>Ada Painter</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"#;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_export_keeps_camera_lens_and_copyright_without_location() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ExportMetadata");
    let output = std::path::Path::new("../../artifacts/photo-m3/export-metadata-native")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    let output = output.canonicalize().unwrap();
    let rgb = SourceInterpretation {
        channels: SourceChannels::Rgb,
        depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    };
    let camera = output.join("camera.jpg");
    let taken = layer_color::photo::DeliveryMetadata {
        resolution: Some(layer_core::ImageResolution::ppi(300)),
        photo: layer_core::PhotoMetadata {
            exif: Some(camera_exif().into()),
            xmp: Some(CAMERA_XMP.as_bytes().into()),
            iptc: None,
        },
        policy: layer_ui::ExportMetadata { keep: layer_ui::MetadataKeep::All, remove_location: false },
    };
    layer_color::photo::write_jpeg_rows(
        std::fs::File::create(&camera).unwrap(), [160, 120], &rgb, &taken,
        layer_color::photo::JpegEncodeOptions::from_memory_budget(92, layer_color::photo::PhotoMemoryBudget::current()),
        |y, row| { for (x, p) in row.chunks_exact_mut(3).enumerate() { p.copy_from_slice(&[x as u8, y as u8 * 2, 90]); } Ok(()) },
    )
    .unwrap();
    let camera_bytes = std::fs::read(&camera).unwrap();
    assert!(camera_bytes.windows(6).any(|w| w == b"WGS-84"), "the source photo carries a location");
    let (project, location) = crate::files::open::read(
        &camera,
        layer_ui::DocumentLocation { uri: gtk::gio::File::for_path(&camera).uri().into(), name: "camera.jpg".into() },
        Default::default(),
        layer_ui::photo_document_names("camera.jpg", &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)),
        Default::default(),
    )
    .unwrap();
    let layer_ui::ImportOutcome::Editable(imported) = project else { panic!("editable photo") };
    let project = imported.project;
    assert!(project.artwork.metadata.exif.is_some() && project.artwork.metadata.xmp.is_some());
    let w = Workspace::with_project(&app, Some((project, location)));
    w.window.present();
    ready(&w);
    let before = snapshot(&w);
    let exported = |format: u32, name: &str| {
        invoke(&w, CommandId::ExportDocument);
        combo(&w, "export-format").set_selected(format);
        pump(100);
        let dialog = w.window.visible_dialog().unwrap();
        let widget = |name: &str| find_named(dialog.upcast_ref(), name).unwrap();
        let metadata = combo(&w, "export-metadata");
        let labels: Vec<_> = (0..metadata.model().unwrap().n_items())
            .map(|i| metadata.model().unwrap().item(i).and_downcast::<gtk::StringObject>().unwrap().string().to_string())
            .collect();
        assert_eq!(labels, ["All", "Copyright & Contact", "None"]);
        assert!(metadata.is_visible());
        assert_eq!(metadata.selected(), 0, "all metadata is kept by default");
        let remove_location = widget("export-remove-location").downcast::<adw::SwitchRow>().unwrap();
        assert!(remove_location.is_visible() && remove_location.is_active(), "location is removed by default");
        assert!(!widget("export-metadata-note").is_visible());
        metadata.set_selected(1);
        pump(50);
        assert!(!remove_location.is_visible(), "Copyright & Contact never keeps a location");
        metadata.set_selected(0);
        pump(50);
        assert!(remove_location.is_visible());
        for (scheme, theme) in [(adw::ColorScheme::ForceLight, "light"), (adw::ColorScheme::ForceDark, "dark")] {
            adw::StyleManager::default().set_color_scheme(scheme);
            pump(200);
            capture_ui(&w, &output, &format!("{name}-options-{theme}.png"));
        }
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
        let file = output.join(name);
        crate::files::choose_next_save(file.clone());
        response(&w, "export");
        finish(&w);
        std::fs::read(&file).unwrap()
    };
    for (format, name) in [(2, "copy.jpg"), (3, "copy.webp")] {
        let bytes = exported(format, name);
        assert!(!bytes.windows(6).any(|w| w == b"WGS-84"), "{name}: no GPS");
        assert!(!bytes.windows(6).any(|w| w == b"Lisbon"), "{name}: no place name");
        let photo = layer_color::photo::read_photo_detailed(std::io::Cursor::new(&bytes), Default::default()).unwrap();
        assert_eq!(photo.source.extent, [160, 120]);
        let exif = photo.metadata.exif.expect(name);
        for kept in [b"Capycam".as_slice(), b"C-1", b"Capy 35mm F1.8", b"Ada Painter", b"(c) 2026 Ada Painter", b"2026:09:01 10:00:00"] {
            assert!(exif.windows(kept.len()).any(|w| w == kept), "{name}: {}", String::from_utf8_lossy(kept));
        }
        let xmp = String::from_utf8(photo.metadata.xmp.expect(name).to_vec()).unwrap();
        assert!(xmp.contains("Ada Painter") && !xmp.contains("GPSLatitude") && !xmp.contains("InstanceID"), "{name}: {xmp}");
    }
    assert_eq!(snapshot(&w), before, "exporting never edits the master");
    w.window.destroy();
    pump(100);
}
