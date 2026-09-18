//! Real GTK controls, immutable workers and PQ delivery; no physical-display claim.
use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{RgbSpace, SampleDepth, hdr::SdrRendition};
use layer_ui::{ColorInputModel, ColorSlot, EffectAction};

fn project(w: &Rc<Workspace>) -> layer_core::Project {
    layer_core::Project::read(std::io::Cursor::new(snapshot(w)), Default::default()).unwrap()
}
fn pixels(w: &Rc<Workspace>) -> Vec<[f32; 4]> {
    let project = project(w);
    let extent = [project.document.width, project.document.height];
    let gpu = w.snapshot_gpu().unwrap();
    glib::MainContext::default()
        .block_on(gtk::gio::spawn_blocking(move || {
            let mut renderer = gpu
                .capture(project, [0.; 4], 0., Default::default(), Default::default())
                .unwrap();
            renderer.read_region([0, 0, extent[0], extent[1]]).unwrap()
        }))
        .unwrap()
}
fn effect(w: &Rc<Workspace>, name: &str, key: &str, value: layer_core::EffectValue) {
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: name.into(),
        },
    });
    ready(w);
    w.dispatch(UiAction::Effect {
        action: EffectAction::Set {
            layer: state(w).layer_properties.layer.unwrap(),
            key: key.into(),
            value,
        },
    });
    ready(w);
}

fn appearance() -> gtk::Window {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(20);
        if let Some(window) = gtk::Window::list_toplevels().into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.is_visible() && w.widget_name() == "sdr-appearance-window") { return window; }
        assert!(Instant::now() < deadline, "SDR Appearance window");
    }
}
fn appearance_button(window: &gtk::Window, name: &str) {
    find_named(window.upcast_ref(), &format!("sdr-appearance-{name}")).unwrap()
        .downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(100);
}
fn appearance_exposure(window: &gtk::Window, value: f64) {
    let control = find_named(window.upcast_ref(), "sdr-appearance-exposure").unwrap()
        .downcast::<crate::number_control::NumberControl>().unwrap();
    control.set_value(value); control.emit_by_name::<()>("value-changed", &[]);
    pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_delivery_failure_and_cancellation_preserve_destination() {
    let app = native_test_app("art.capycanvas.HdrDeliveryAtomicity");
    let mut p = new_drawing(64, 64).unwrap();
    p.document.color.depth = SampleDepth::F16;
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present();
    ready(&w);
    let before = snapshot(&w);
    let path = std::env::temp_dir().join(format!("capy-hdr-atomic-{}.png", std::process::id()));
    std::fs::write(&path, b"existing destination").unwrap();
    let run = |format, cancelled| {
        let gpu = w.snapshot_gpu().unwrap();
        let snapshot = DocumentExport {
            project: project(&w),
            background: [100., 100., 100., 1.],
            time: 0.,
        };
        let recipe = ExportRecipe {
            format,
            depth: SampleDepth::U16,
            ..ExportRecipe::web_share()
        };
        let job = crate::files::export::ExportJob::default();
        if cancelled {
            assert!(job.cancel());
        }
        let path = path.clone();
        glib::MainContext::default()
            .block_on(gtk::gio::spawn_blocking(move || {
                crate::files::export::write_snapshot(gpu, snapshot, recipe, &path, &job)
            }))
            .unwrap()
    };
    assert!(
        run(ExportFormat::PngHdr, false)
            .unwrap_err()
            .contains("exceeds BT.2020 PQ")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"existing destination");
    assert!(
        run(ExportFormat::PngHdrMapped, true)
            .unwrap_err()
            .to_lowercase()
            .contains("cancel")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"existing destination");
    assert_eq!(run(ExportFormat::PngHdrMapped, false).unwrap(), 64 * 64 * 3);
    let delivered = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(&path).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(delivered.interpretation.depth, SampleDepth::F16);
    assert_eq!(snapshot(&w), before);
    w.window.destroy();
    pump(100);
    std::fs::remove_file(path).unwrap();
}
#[allow(deprecated)]
fn deliver(w: &Rc<Workspace>, directory: &std::path::Path, name: &str, format: u32) {
    invoke(w, CommandId::ExportDocument);
    combo(w, "export-range").set_selected(u32::from(format >= 3));
    combo(w, "export-format").set_selected(format.min(2));
    if format >= 3 {
        assert!(!combo(w, "export-depth").is_visible());
        assert!(!combo(w, "export-format").is_visible());
        let deadline = Instant::now() + Duration::from_secs(30);
        while !super::new_photo::export_enabled(&w) { pump(20); assert!(Instant::now() < deadline, "HDR preflight"); }
    }
    if format == 0 {
        combo(w, "export-depth").set_selected(0);
    }
    pump(200);
    response(w, "export");
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(directory)))
        .unwrap();
    file.set_current_name(name);
    pump(250);
    file.response(gtk::ResponseType::Accept);
    finish(w);
}
#[test]
#[ignore = "private Wayland display, hardware GPU; optional LAYER_HDR_INPUT from another application"]
#[allow(deprecated)]
fn native_hdr_open_edit_rendition_save_and_deliver() {
    let app = native_test_app("art.capycanvas.HdrJourney");
    let directory = std::env::var_os("LAYER_HDR_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("capy-hdr-{}", std::process::id())));
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let input = std::env::var_os("LAYER_HDR_INPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let path = directory.join("input-pq.png");
            layer_color::photo::write_hdr_png_rows(
                std::fs::File::create(&path).unwrap(),
                [512, 384],
                RgbSpace::Srgb,
                None,
                false,
                |y, row| {
                    for (x, p) in row.iter_mut().enumerate() {
                        *p = [
                            0.05 + 8. * x as f32 / 511.,
                            0.05 + 2. * y as f32 / 383.,
                            0.25,
                            1.,
                        ];
                    }
                    Ok(())
                },
            )
            .unwrap();
            path
        });
    let original = std::fs::read(&input).unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256).unwrap(), None)));
    let opened = Rc::new(RefCell::new(None));
    let result = opened.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |p, l, _| {
        result.replace(Some((p, l)));
    }));
    w.window.present();
    ready(&w);
    invoke(&w, CommandId::NewDocument);
    combo(&w, "new-document-preset").set_selected(4);
    assert_eq!(combo(&w, "new-document-depth").selected(), 2);
    response(&w, "create");
    finish(&w);
    assert_eq!(
        opened.borrow_mut().take().unwrap().0.document.color.depth,
        SampleDepth::F16
    );
    invoke(&w, CommandId::OpenDocument);
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&input)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    let (p, location) = opened.borrow_mut().take().unwrap();
    assert_eq!(p.document.color.depth, SampleDepth::F16);
    assert!(location.is_none());
    let photo = Workspace::with_project(&app, Some((p, None)));
    photo.window.present();
    ready(&photo);
    assert!(photo.hdr_status.is_visible());
    let status_bounds = photo.hdr_status.compute_bounds(&photo.window).unwrap();
    assert!(status_bounds.height() >= 24.);
    assert!(status_bounds.y() >= 0. && status_bounds.y() + status_bounds.height() <= photo.window.height() as f32);
    photo.hdr_status.emit_clicked(); pump(100); response(&photo, "close");
    let before = pixels(&photo);
    assert!(before.iter().any(|p| p[0] > 1.));
    assert!(before.iter().all(|p| p[3] == 1.));
    capture_ui(&photo, &directory, "hdr-opened.png");
    effect(
        &photo,
        "exposure",
        "exposure",
        layer_core::EffectValue::Number(1.),
    );
    let exposed = pixels(&photo);
    for (a, b) in before.iter().zip(&exposed) {
        for c in 0..3 {
            assert!((b[c] - a[c] * 2.).abs() <= 2e-6 + a[c].abs() * 2e-5);
        }
    }
    effect(
        &photo,
        "curves",
        "curve_0",
        layer_core::EffectValue::Curve(vec![[0., 0.], [1., 0.75]]),
    );
    let curved = pixels(&photo);
    assert!(curved.iter().any(|p| p[0] > 1.));
    let curve = project(&photo)
        .document
        .layers
        .into_iter()
        .find_map(|l| l.effect)
        .unwrap();
    assert_eq!(
        curve.value("domain"),
        Some(&layer_core::EffectValue::Choice(1))
    );
    capture_ui(&photo, &directory, "hdr-curves.png");
    // Numeric HDR input and actual pen publication on an editable paint layer.
    invoke(&photo, CommandId::AddLayer);
    crate::color_editor::show(&photo, ColorSlot::Foreground);
    pump(100);
    combo(&photo, "edit-color-model").set_selected(
        ColorInputModel::ALL
            .iter()
            .position(|m| *m == ColorInputModel::LinearRgb)
            .unwrap() as u32,
    );
    for (i, value) in ["8", "2", "1", "100"].iter().enumerate() {
        find_named(
            photo.window.visible_dialog().unwrap().upcast_ref(),
            &format!("edit-color-value-{i}"),
        )
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text(value);
    }
    capture_ui(&photo, &directory, "hdr-color-entry.png");
    response(&photo, "apply");
    let entered = state(&photo)
        .colors
        .foreground
        .linear_in(RgbSpace::Srgb)
        .unwrap();
    assert!((entered[0] - 8.).abs() < 2e-5);
    let brightness = find_named(photo.color_panel.root.upcast_ref(), "color-hdr-brightness").unwrap().downcast::<crate::number_control::NumberControl>().unwrap();
    let previous = state(&photo).colors.foreground;
    photo.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition {
        color: layer_core::color::RgbColor::from_linear(RgbSpace::Srgb, [65504., 2., 1., 1.]).unwrap(),
    }});
    assert!((brightness.value() - f64::from(65504f32.log2())).abs() < 0.0001, "readout preserves values beyond the slider's editing range");
    photo.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition { color: previous }});
    photo.dispatch(UiAction::SetBrushSize { value: 40. });
    ready(&photo);
    native_pen_path(&photo, &[[100., 150.], [180., 150.], [260., 150.]]);
    ready(&photo);
    let painted = pixels(&photo);
    assert_ne!(painted, curved);
    std::fs::write(
        directory.join("edited-linear-srgb-rgba32le.bin"),
        painted
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    invoke(&photo, CommandId::Undo);
    ready(&photo);
    assert_eq!(pixels(&photo), curved);
    invoke(&photo, CommandId::Redo);
    ready(&photo);
    assert_eq!(pixels(&photo), painted);
    // Cancel is inert; applying the authored rendition is one reversible edit.
    let default = project(&photo).document.sdr_rendition;
    let checkpoint = photo.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
    invoke(&photo, CommandId::SdrRendition);
    let window = appearance();
    appearance_exposure(&window, -2.);
    assert_eq!(project(&photo).document.sdr_rendition, default);
    assert_eq!(photo.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
    assert_eq!(state(&photo).sdr_appearance_preview.unwrap().exposure, -2.);
    let compare = find_named(window.upcast_ref(), "sdr-appearance-compare").unwrap().downcast::<gtk::CheckButton>().unwrap();
    compare.set_active(true); pump(50);
    assert_eq!(state(&photo).sdr_appearance_preview, Some(default));
    compare.set_active(false); pump(50);
    assert_eq!(state(&photo).sdr_appearance_preview.unwrap().exposure, -2.);
    appearance_button(&window, "reset");
    assert_eq!(state(&photo).sdr_appearance_preview, Some(default));
    appearance_exposure(&window, -1.);
    appearance_button(&window, "cancel");
    finish(&photo);
    assert_eq!(project(&photo).document.sdr_rendition, default);
    invoke(&photo, CommandId::SdrRendition);
    let window = appearance();
    appearance_exposure(&window, -0.5);
    capture_ui(&photo, &directory, "sdr-appearance-canvas.png");
    crate::snapshot_window(&window, 1.).save_to_png(directory.join("sdr-appearance-controls.png")).unwrap();
    appearance_button(&window, "apply");
    finish(&photo);
    let mut recipe = SdrRendition {
        exposure: -0.5,
        ..default
    };
    assert_eq!(project(&photo).document.sdr_rendition, recipe);
    assert_eq!(pixels(&photo), painted);
    invoke(&photo, CommandId::Undo);
    ready(&photo);
    assert_eq!(project(&photo).document.sdr_rendition, default);
    invoke(&photo, CommandId::Redo);
    ready(&photo);
    assert_eq!(project(&photo).document.sdr_rendition, recipe);
    // The host eyedropper samples artwork, independent of mapped presentation.
    photo.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickVisible,
        },
    });
    photo.dispatch(UiAction::SetColorSampleSize { width: 1 });
    photo.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Definition {
            color: layer_core::color::RgbColor::WHITE,
        },
    });
    native_pen_path(&photo, &[[400.5, 200.5], [400.5, 200.5]]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while state(&photo).colors.definition() == layer_core::color::RgbColor::WHITE {
        pump(10);
        assert!(Instant::now() < deadline, "HDR eyedropper completion");
    }
    let sampled = state(&photo)
        .colors
        .definition()
        .linear_in(RgbSpace::Srgb)
        .unwrap();
    let expected = painted[200 * 512 + 400];
    for (actual, expected) in sampled.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 2e-6 + expected.abs() * 2e-5);
    }
    assert!(sampled[..3].iter().any(|value| *value > 1.));
    assert_eq!(pixels(&photo), painted);
    invoke(&photo, CommandId::Histogram);
    finish(&photo);
    let inspector = photo.histogram.borrow().as_ref().unwrap().clone();
    let deadline = Instant::now() + Duration::from_secs(30);
    while inspector.result.borrow().is_none() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let histogram = inspector.result.borrow().clone().unwrap();
    assert_eq!(histogram.color.depth, SampleDepth::F16);
    assert!(histogram.channels[0].above > 0);
    crate::snapshot_window(&inspector.window, 1.).save_to_png(directory.join("hdr-histogram.png")).unwrap();
    inspector.window.close();
    let master = directory.join("HDR master.capy");
    invoke(&photo, CommandId::SaveDocument);
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&directory)))
        .unwrap();
    file.set_current_name("HDR master.capy");
    pump(250);
    file.response(gtk::ResponseType::Accept);
    finish(&photo);
    assert!(!state(&photo).document_file.modified);
    assert!(!state(&photo).hdr_display_available);
    assert!(!photo.gpu.borrow().as_ref().unwrap().session.command(CommandId::PreviewSdr).enabled);
    assert!(!state(&photo).document_file.modified);
    assert_eq!(pixels(&photo), painted);
    capture_ui(&photo, &directory, "hdr-sdr-preview.png");
    let reopened =
        layer_core::Project::read(std::fs::File::open(&master).unwrap(), Default::default())
            .unwrap();
    assert_eq!(reopened.document.sdr_rendition, recipe);
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.present();
    ready(&restored);
    assert_eq!(pixels(&restored), painted);
    invoke(&restored, CommandId::ExportDocument);
    combo(&restored, "export-range").set_selected(0);
    response(&restored, "appearance");
    let window = appearance();
    appearance_exposure(&window, -0.75);
    appearance_button(&window, "apply");
    recipe.exposure = -0.75;
    assert_eq!(project(&restored).document.sdr_rendition, recipe);
    let deadline = Instant::now() + Duration::from_secs(30);
    while restored.window.visible_dialog().is_none() { pump(20); assert!(Instant::now() < deadline); }
    assert_eq!(combo(&restored, "export-range").selected(), 0);
    combo(&restored, "export-range").set_selected(1);
    pump(500);
    capture_ui(&restored, &directory, "hdr-export.png");
    response(&restored, "cancel"); finish(&restored);
    deliver(&restored, &directory, "Edited HDR.png", 3);
    deliver(&restored, &directory, "SDR rendition.png", 0);
    let hdr = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(directory.join("Edited HDR.png")).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(hdr.interpretation.depth, SampleDepth::F16);
    let sdr = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(directory.join("SDR rendition.png")).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(sdr.interpretation.depth, SampleDepth::U8);
    let decoder =
        layer_color::WorkingDecoder::new(&sdr.interpretation, RgbSpace::Srgb, Default::default())
            .unwrap();
    let mut row = vec![0; sdr.row_bytes()];
    let mut actual = vec![[0.; 4]; 512];
    for y in [0, 150, 383] {
        sdr.rows().read(y, &mut row).unwrap();
        decoder.decode_pixels(&row, &mut actual).unwrap();
        for (x, p) in actual.iter().enumerate() {
            let expected = recipe.map_premultiplied(painted[y as usize * 512 + x]);
            for c in 0..3 {
                assert!(
                    (p[c] - expected[c]).abs() < 0.01,
                    "SDR {x},{y}: {p:?} != {expected:?}"
                );
            }
        }
    }
    capture_ui(&restored, &directory, "hdr-reopened.png");
    assert_eq!(std::fs::read(input).unwrap(), original);
    for w in [&restored, &photo, &w] {
        w.window.destroy();
    }
    pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_export_preflight_rejects_range_and_allows_explicit_clipping() {
    use layer_core::color::{source::*, hdr};
    let app = native_test_app("art.capycanvas.HdrPreflight");
    let mut p = new_drawing(64, 64).unwrap();
    p.document.color.depth = SampleDepth::F16;
    p.document.layers[1].visible = false;
    let mut source = SourceBuilder::new([64, 64], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::F16,
        profile: Default::default(), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    let row: Vec<_> = (0..64).flat_map(|_| hdr::encode_pixel([100., 100., 100., 1.]).unwrap()).flat_map(u16::to_le_bytes).collect();
    for _ in 0..64 { source.push_row(&row).unwrap(); }
    p.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present(); ready(&w);
    let original = snapshot(&w);
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-range").set_selected(1);
    let dialog = w.window.visible_dialog().unwrap();
    let status = find_named(dialog.upcast_ref(), "color-preview-status").unwrap().downcast::<gtk::Label>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !status.text().contains("exceed") { pump(20); assert!(Instant::now() < deadline, "{}", status.text()); }
    assert!(!super::new_photo::export_enabled(&w));
    let clip = find_named(dialog.upcast_ref(), "export-hdr-clip").unwrap().downcast::<adw::SwitchRow>().unwrap();
    clip.set_active(true);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !super::new_photo::export_enabled(&w) { pump(20); assert!(Instant::now() < deadline, "{}", status.text()); }
    assert!(status.text().contains("clipped"));
    clip.set_active(false);
    assert!(!super::new_photo::export_enabled(&w), "stale successful check cannot authorize a new range choice");
    response(&w, "cancel"); finish(&w);
    assert_eq!(snapshot(&w), original);
    w.window.destroy(); pump(100);
}

#[test]
#[ignore = "Wayland display and hardware GPU; optional LAYER_EXPECT_HDR=1 physical qualification"]
fn native_hdr_display_negotiation_and_export_navigation() {
    let app = native_test_app("art.capycanvas.HdrDisplayNavigation");
    let w = Workspace::with_project(&app, Some((new_drawing(192, 128).unwrap(), None)));
    w.window.present(); ready(&w);
    // Promote the existing window, so HDR cannot depend on reopening the file.
    invoke(&w, CommandId::ChangeBitDepth);
    combo(&w, "document-color-depth").set_selected(2);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(20);
        let d = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        if d.is_response_enabled("apply") { break; }
        assert!(Instant::now() < deadline);
    }
    response(&w, "apply"); finish(&w); ready(&w);
    assert_eq!(project(&w).document.color.depth, SampleDepth::F16);
    let expected_hdr = std::env::var_os("LAYER_EXPECT_HDR").is_some();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pump(50);
        let gpu = w.gpu.borrow();
        let backend = gpu.as_ref().unwrap().session.engine().backend();
        if backend.display_encoding.is_some() && (!expected_hdr || backend.display_headroom > 1.) {
            eprintln!("HDR_DISPLAY_QUALIFICATION encoding={:?} headroom={:.4} label={:?}", backend.display_encoding, backend.display_headroom, w.hdr_status.label());
            break;
        }
        assert!(Instant::now() < deadline, "HDR negotiation: {:?}, {}", backend.display_encoding, backend.display_headroom);
    }
    if expected_hdr {
        let deadline = Instant::now() + Duration::from_secs(2);
        while w.hdr_status.label().as_deref() != Some("HDR") { pump(20); assert!(Instant::now() < deadline, "HDR feedback did not refresh idle UI"); }
    }
    let output = std::env::var_os("LAYER_HDR_OUTPUT").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../artifacts/color-m4/feedback-navigation"));
    std::fs::create_dir_all(&output).unwrap();
    capture_ui(&w, &output, "hdr-display.png");
    if expected_hdr {
        invoke(&w, CommandId::PreviewSdr); ready(&w);
        assert_eq!(w.hdr_status.label().as_deref(), Some("SDR preview"));
        invoke(&w, CommandId::PreviewSdr); ready(&w);
        assert_eq!(w.hdr_status.label().as_deref(), Some("HDR"));
    }
    let before = snapshot(&w);
    invoke(&w, CommandId::ExportDocument);
    pump(600);
    let dialog = w.window.visible_dialog().unwrap();
    assert!(!dialog.is::<adw::AlertDialog>());
    assert!(find_named(dialog.upcast_ref(), "export-bpc").is_none());
    let scroll = find_named(dialog.upcast_ref(), "export-main-scroll").unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    let adjustment = scroll.vadjustment();
    assert!(adjustment.upper() <= adjustment.page_size() + 1., "main page requires scrolling: {} / {}", adjustment.upper(), adjustment.page_size());
    capture_ui(&w, &output, "export-sdr-main.png");
    super::new_photo::export_page(&w, "size");
    combo(&w, "export-size").set_selected(1);
    for name in ["export-width", "export-height"] {
        find_named(dialog.upcast_ref(), name).unwrap().downcast::<adw::SpinRow>().unwrap().set_value(100.);
    }
    capture_ui(&w, &output, "export-size.png");
    super::new_photo::export_page(&w, "main");
    let size = find_named(dialog.upcast_ref(), "export-open-size").unwrap().downcast::<adw::ActionRow>().unwrap();
    assert_eq!(size.subtitle().as_deref(), Some("100 × 67 px"));
    super::new_photo::export_page(&w, "color");
    combo(&w, "export-depth").set_selected(1);
    capture_ui(&w, &output, "export-color.png");
    super::new_photo::export_page(&w, "presets");
    assert!(find_named(dialog.upcast_ref(), "export-preset-save").unwrap().is::<adw::ButtonRow>());
    capture_ui(&w, &output, "export-presets.png");
    super::new_photo::export_page(&w, "main");
    assert_eq!(combo(&w, "export-depth").selected(), 1);
    combo(&w, "export-range").set_selected(1);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !super::new_photo::export_enabled(&w) { pump(20); assert!(Instant::now() < deadline); }
    assert!(!find_named(dialog.upcast_ref(), "export-open-color").unwrap().is_visible());
    assert!(!find_named(dialog.upcast_ref(), "export-appearance").unwrap().is_visible());
    assert!(!find_named(dialog.upcast_ref(), "export-hdr-clip").unwrap().is_visible());
    capture_ui(&w, &output, "export-hdr-main.png");
    response(&w, "cancel"); finish(&w);
    assert_eq!(snapshot(&w), before);
    w.window.destroy(); pump(100);
}

#[test]
#[ignore = "Wayland/GPU; LAYER_EXPECT_HDR=1 also verifies HDR texture and GSK transport"]
fn native_hdr_export_preview_preserves_master_and_tracks_display() {
    use layer_core::color::{ColorProfile, source::*};
    let app = native_test_app("art.capycanvas.HdrExportPreview");
    let mut p = new_drawing(64, 64).unwrap();
    p.document.color.depth = SampleDepth::F16;
    p.document.layers[1].visible = false;
    p.document.sdr_rendition.exposure = -4.;
    let mut source = SourceBuilder::new([64, 64], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::F16,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    let bits = layer_core::color::hdr::encode_pixel([8., 2., 0.5, 0.5]).unwrap();
    let row = bits.into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>().repeat(64);
    for _ in 0..64 { source.push_row(&row).unwrap(); }
    p.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present(); ready(&w); pump(300);
    let original = snapshot(&w);
    let physical = std::env::var_os("LAYER_EXPECT_HDR").is_some();
    let headroom = w.gpu.borrow().as_ref().unwrap().session.engine().backend().display_headroom;
    if physical {
        assert!(headroom > 1.);
        // Temporary canvas SDR viewing must not replace the export master.
        invoke(&w, CommandId::PreviewSdr); ready(&w);
    }
    invoke(&w, CommandId::ExportDocument);
    let picture = |name: &str| find_named(w.window.visible_dialog().unwrap().upcast_ref(), name)
        .unwrap().downcast::<gtk::Picture>().unwrap();
    let texture = |name: &str| picture(name).paintable().and_downcast::<gdk::Texture>().unwrap();
    let wait = || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            pump(30);
            if picture("color-preview-before").paintable().is_some() && picture("color-preview-after").paintable().is_some() && super::new_photo::export_enabled(&w) { break; }
            assert!(Instant::now() < deadline, "HDR export preview did not finish");
        }
    };
    let pixel = |texture: &gdk::Texture| {
        let mut download = gdk::TextureDownloader::new(texture);
        download.set_color_state(&gdk::ColorState::srgb_linear());
        download.set_format(gdk::MemoryFormat::R32g32b32a32Float);
        let (bytes, stride) = download.download_bytes();
        let index = texture.height() as usize / 2 * stride + texture.width() as usize / 2 * 16;
        std::array::from_fn::<_, 4, _>(|c| f32::from_ne_bytes(bytes[index+c*4..index+c*4+4].try_into().unwrap()))
    };
    wait();
    pump(250); // Let GTK snapshot the newly published texture.
    let master = texture("color-preview-before");
    assert!(pixel(&texture("color-preview-after"))[0] < 1., "SDR delivery must stay SDR");
    if physical {
        assert_eq!(master.color_state(), gdk::ColorState::rec2100_linear());
        assert!((pixel(&master)[0] - 4.47).abs() < 0.01, "HDR master lost above-white/alpha values: {:?}", pixel(&master));
        let image = picture("color-preview-before");
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&image)).snapshot(&snapshot, image.width() as f64, image.height() as f64);
        let rendered = w.window.renderer().unwrap().render_texture(&snapshot.to_node().unwrap(), None);
        // GTK scales the checker; the center can interpolate its two tones.
        for (c, base) in [4., 1., 0.25].into_iter().enumerate() {
            assert!((base + 0.395..=base + 0.475).contains(&pixel(&rendered)[c]), "GSK lost HDR values: {:?}", pixel(&rendered));
        }
        eprintln!("HDR_PREVIEW_MASTER {:?}; GSK {:?}", pixel(&master), pixel(&rendered));
    } else {
        assert_ne!(master.color_state(), gdk::ColorState::rec2100_linear());
        assert!(pixel(&master)[0] <= 1.);
    }
    combo(&w, "export-range").set_selected(1); wait();
    if physical {
        let output = texture("color-preview-after");
        assert_eq!(output.color_state(), gdk::ColorState::rec2100_linear());
        for c in 0..3 { assert!((pixel(&output)[c] - pixel(&master)[c]).abs() < 0.01); }
        let directory = std::env::var_os("LAYER_HDR_OUTPUT").map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("../../artifacts/color-m4/hdr-preview-desktop"));
        std::fs::create_dir_all(&directory).unwrap();
        capture_ui(&w, &directory, "hdr-export-preview.png");
        // Inject a capability transition, not a change to OS display settings.
        w.gpu.borrow_mut().as_mut().unwrap().session.renderer_mut().display_headroom = 1.;
        pump(400); wait();
        assert_ne!(texture("color-preview-before").color_state(), gdk::ColorState::rec2100_linear());
        assert_eq!(picture("color-preview-before").alternative_text().as_deref(), Some("Master (SDR preview)"));
        w.gpu.borrow_mut().as_mut().unwrap().session.renderer_mut().display_headroom = headroom;
        pump(400); wait();
        assert_eq!(texture("color-preview-before").color_state(), gdk::ColorState::rec2100_linear());
    }
    response(&w, "cancel"); finish(&w);
    assert_eq!(snapshot(&w), original);
    w.window.destroy(); pump(100);
}

#[test]
#[ignore = "Wayland/GPU and LAYER_HDR_LARGE_INPUT pointing to a retained 60 MP HDR fixture"]
fn native_hdr_large_export_preview_and_cancellation() {
    let path = std::env::var_os("LAYER_HDR_LARGE_INPUT").expect("60 MP fixture path");
    let p = layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    assert_eq!(p.document.color.depth, SampleDepth::F16);
    assert!(u64::from(p.document.width) * u64::from(p.document.height) >= 59_000_000);
    let app = native_test_app("art.capycanvas.HdrLargePreview");
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present(); ready(&w);
    let revision = w.gpu.borrow().as_ref().unwrap().session.engine().document().revision;
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-range").set_selected(1);
    let dialog = w.window.visible_dialog().unwrap();
    let after = find_named(dialog.upcast_ref(), "color-preview-after").unwrap().downcast::<gtk::Picture>().unwrap();
    let status = find_named(dialog.upcast_ref(), "color-preview-status").unwrap().downcast::<gtk::Label>().unwrap();
    let start = Instant::now();
    let heartbeat = Rc::new(RefCell::new((Instant::now(), 0u64, 0u128)));
    let timer = glib::timeout_add_local(std::time::Duration::from_millis(10), {
        let heartbeat = heartbeat.clone();
        move || {
            let mut h = heartbeat.borrow_mut();
            h.2 = h.2.max(h.0.elapsed().as_micros()); h.0 = Instant::now(); h.1 += 1;
            glib::ControlFlow::Continue
        }
    });
    while after.paintable().is_none() {
        pump(5);
        assert!(start.elapsed().as_secs() < 90, "60 MP preview: {}", status.text());
    }
    timer.remove();
    eprintln!("HDR_LARGE_PREVIEW elapsed_ms={:.2} heartbeat_count={} max_heartbeat_gap_ms={:.2} status={}",
        start.elapsed().as_secs_f64()*1000., heartbeat.borrow().1, heartbeat.borrow().2 as f64/1000., status.text());
    assert!(heartbeat.borrow().1 > 10, "no UI heartbeat while previewing");
    assert!(heartbeat.borrow().2 < 500_000, "main-thread stall during preview");
    for _ in 0..8 { combo(&w, "export-range").set_selected(0); combo(&w, "export-range").set_selected(1); }
    pump(50);
    let cancel = Instant::now();
    response(&w, "cancel"); finish(&w);
    eprintln!("HDR_LARGE_PREVIEW cancel_ms={:.2}", cancel.elapsed().as_secs_f64()*1000.);
    assert!(cancel.elapsed().as_secs_f64() < 3.);
    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().revision, revision);
    w.window.destroy(); pump(100);
}
