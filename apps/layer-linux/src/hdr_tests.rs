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
        let dialog = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !dialog.is_response_enabled("export") { pump(20); assert!(Instant::now() < deadline, "HDR preflight"); }
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
    combo(&restored, "export-range").set_selected(1);
    response(&restored, "appearance");
    let window = appearance();
    appearance_exposure(&window, -0.75);
    appearance_button(&window, "apply");
    recipe.exposure = -0.75;
    assert_eq!(project(&restored).document.sdr_rendition, recipe);
    let deadline = Instant::now() + Duration::from_secs(30);
    while restored.window.visible_dialog().is_none() { pump(20); assert!(Instant::now() < deadline); }
    assert_eq!(combo(&restored, "export-range").selected(), 1);
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
    let dialog = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
    let status = find_named(dialog.upcast_ref(), "color-preview-status").unwrap().downcast::<gtk::Label>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !status.text().contains("exceed") { pump(20); assert!(Instant::now() < deadline, "{}", status.text()); }
    assert!(!dialog.is_response_enabled("export"));
    let clip = find_named(dialog.upcast_ref(), "export-hdr-clip").unwrap().downcast::<adw::SwitchRow>().unwrap();
    clip.set_active(true);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dialog.is_response_enabled("export") { pump(20); assert!(Instant::now() < deadline, "{}", status.text()); }
    assert!(status.text().contains("clipped"));
    clip.set_active(false);
    assert!(!dialog.is_response_enabled("export"), "stale successful check cannot authorize a new range choice");
    response(&w, "cancel"); finish(&w);
    assert_eq!(snapshot(&w), original);
    w.window.destroy(); pump(100);
}
