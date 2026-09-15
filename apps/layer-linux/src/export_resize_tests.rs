//! Native sizing choices deliver a copy and release their dialog state.
use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};

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
        depth: IntegerDepth::U16,
    };
    project.document.layers[1].visible = false;
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: IntegerDepth::U16,
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
        assert_eq!(
            result.interpretation.depth,
            if format == 2 {
                IntegerDepth::U8
            } else {
                IntegerDepth::U16
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
