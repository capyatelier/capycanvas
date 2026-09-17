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
#[allow(deprecated)]
fn deliver(w: &Rc<Workspace>, directory: &std::path::Path, name: &str, format: u32) {
    invoke(w, CommandId::ExportDocument);
    combo(w, "export-format").set_selected(format);
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
    combo(&w, "new-document-depth").set_selected(2);
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
    response(&photo, "apply");
    let entered = state(&photo)
        .colors
        .foreground
        .linear_in(RgbSpace::Srgb)
        .unwrap();
    assert!((entered[0] - 8.).abs() < 2e-5);
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
    invoke(&photo, CommandId::SdrRendition);
    response(&photo, "cancel");
    finish(&photo);
    assert_eq!(project(&photo).document.sdr_rendition, default);
    invoke(&photo, CommandId::SdrRendition);
    find_named(
        photo.window.visible_dialog().unwrap().upcast_ref(),
        "sdr-rendition-exposure",
    )
    .unwrap()
    .downcast::<adw::SpinRow>()
    .unwrap()
    .set_value(-0.5);
    capture_ui(&photo, &directory, "sdr-rendition-controls.png");
    response(&photo, "apply");
    finish(&photo);
    let recipe = SdrRendition {
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
    invoke(&photo, CommandId::PreviewSdr);
    ready(&photo);
    assert!(!state(&photo).document_file.modified);
    assert_eq!(pixels(&photo), painted);
    capture_ui(&photo, &directory, "hdr-sdr-preview.png");
    invoke(&photo, CommandId::PreviewSdr);
    ready(&photo);
    let reopened =
        layer_core::Project::read(std::fs::File::open(&master).unwrap(), Default::default())
            .unwrap();
    assert_eq!(reopened.document.sdr_rendition, recipe);
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.present();
    ready(&restored);
    assert_eq!(pixels(&restored), painted);
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
