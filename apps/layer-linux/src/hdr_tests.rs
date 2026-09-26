//! Real GTK controls, immutable workers and PQ delivery; no physical-display claim.
use super::new_photo::{capture_ui, chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{RgbSpace, SampleDepth, hdr::SdrRendition};
use layer_ui::{ColorInputModel, ColorSlot, EffectAction};

#[path = "hdr_qualification_tests.rs"]
mod qualification;
#[path = "gpu_tone_tests.rs"]
mod gpu_tone;

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

fn appearance(w: &Rc<Workspace>) -> gtk::Window {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !w.proof_panel.root.is_mapped() {
        pump(20);
        assert!(Instant::now() < deadline, "Proof panel is visible");
    }
    assert!(w.window.visible_dialog().is_none(), "Proof must leave the canvas operable");
    w.window.clone().upcast()
}
fn appearance_button(window: &gtk::Window, name: &str) {
    find_named(window.upcast_ref(), &format!("sdr-appearance-{name}")).unwrap()
        .downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(100);
}
fn appearance_exposure(window: &gtk::Window, value: f64) {
    let control = find_named(window.upcast_ref(), "sdr-appearance-exposure").unwrap()
        .downcast::<gtk::Scale>().unwrap();
    control.set_value(value);
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
    let range=combo(w, "export-output");
    range.set_selected(u32::from(format >= 3));
    // Commit this explicit choice even when it equals the initial value.
    range.notify("selected");
    if format<3 {combo(w, "export-format").set_selected(format);}
    eprintln!("DELIVER {name}: output={} format={}",combo(w,"export-output").selected(),combo(w,"export-format").selected());
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
    eprintln!("DELIVER READY {name}: output={} format={}",combo(w,"export-output").selected(),combo(w,"export-format").selected());
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
    let intensity = find_named(photo.color_panel.root.upcast_ref(), "color-hdr-intensity-ramp").unwrap().downcast::<crate::hdr_color_scale::HdrColorScale>().unwrap();
    let previous = state(&photo).colors.foreground;
    photo.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition {
        color: layer_core::color::RgbColor::from_linear(RgbSpace::Srgb, [65504., 2., 1., 1.]).unwrap(),
    }});
    assert!((intensity.value() - f64::from(65504f32.log2())).abs() < 0.0001, "arc preserves sampled values beyond its default drag interval");
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
    // Live Proof edits are saved immediately; Off changes viewing only.
    let default = project(&photo).document.sdr_rendition;
    invoke(&photo, CommandId::SdrRendition);
    let window = appearance(&photo);
    let highlight=find_named(photo.proof_panel.root.upcast_ref(),"sdr-appearance-highlight_color").unwrap().downcast::<gtk::Scale>().unwrap();
    assert!(highlight.is_mapped());
    highlight.set_value(0.65);pump(50);
    assert_eq!(project(&photo).document.sdr_rendition.highlight_color,0.65);
    assert_eq!(pixels(&photo),painted);
    invoke(&photo,CommandId::Undo);ready(&photo);assert_eq!(project(&photo).document.sdr_rendition,default);
    invoke(&photo,CommandId::Redo);ready(&photo);assert_eq!(project(&photo).document.sdr_rendition.highlight_color,0.65);
    invoke(&photo,CommandId::Undo);ready(&photo);
    for removed in ["sdr-appearance-apply","sdr-appearance-cancel","sdr-appearance-compare","proof-preview-sdr","proof-advanced"] {
        assert!(find_named(photo.proof_panel.root.upcast_ref(),removed).is_none());
    }
    appearance_exposure(&window,-2.);
    assert_eq!(project(&photo).document.sdr_rendition.exposure,-2.);
    assert!(state(&photo).sdr_appearance_preview.is_none());
    let mode=find_named(photo.proof_panel.root.upcast_ref(),"proof-mode").unwrap().downcast::<adw::ToggleGroup>().unwrap();
    let saved=project(&photo).document.sdr_rendition;
    mode.set_active_name(Some("off"));pump(50);assert!(!state(&photo).preview_sdr);
    assert_eq!(project(&photo).document.sdr_rendition,saved);
    mode.set_active_name(Some("sdr"));pump(50);
    assert!(find_named(photo.proof_panel.root.upcast_ref(),"sdr-appearance-method").is_none());
    let field=find_named(photo.proof_panel.root.upcast_ref(),"sdr-tone-pad-surface").unwrap();
    let controllers=field.observe_controllers();let keys=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::EventControllerKey>()).unwrap();
    keys.emit_by_name::<bool>("key-pressed",&[&gdk::Key::Up,&0u32,&gdk::ModifierType::empty()]);keys.emit_by_name::<()>("key-released",&[&gdk::Key::Up,&0u32,&gdk::ModifierType::empty()]);pump(50);
    assert!(project(&photo).document.sdr_rendition.contrast>default.contrast);
    assert_eq!(pixels(&photo),painted);
    appearance_button(&window,"reset");assert_eq!(project(&photo).document.sdr_rendition,default);
    appearance_exposure(&window,-0.5);
    capture_ui(&photo,&directory,"sdr-appearance-canvas.png");
    crate::snapshot_window(&window,1.).save_to_png(directory.join("sdr-appearance-controls.png")).unwrap();
    let mut recipe=SdrRendition{exposure:-0.5,..default};
    assert_eq!(project(&photo).document.sdr_rendition,recipe);assert_eq!(pixels(&photo),painted);
    invoke(&photo,CommandId::Undo);ready(&photo);assert_eq!(project(&photo).document.sdr_rendition,default);
    invoke(&photo,CommandId::Redo);ready(&photo);assert_eq!(project(&photo).document.sdr_rendition,recipe);
    highlight.set_value(0.35);highlight.emit_by_name::<()>("value-changed",&[]);pump(50);
    recipe.highlight_color=0.35;assert_eq!(project(&photo).document.sdr_rendition,recipe);
    // The host eyedropper samples artwork, independent of mapped presentation.
    photo.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Definition {
            color: layer_core::color::RgbColor::WHITE,
        },
    });
    photo.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickVisible,
        },
    });
    photo.dispatch(UiAction::SetColorSampleSize { width: 1 });
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
    invoke(&restored, CommandId::SdrRendition);
    let window = appearance(&restored);
    appearance_exposure(&window, -0.75);
    recipe.exposure = -0.75;
    assert_eq!(project(&restored).document.sdr_rendition, recipe);
    invoke(&restored, CommandId::ExportDocument);
    let dialog=restored.window.visible_dialog().unwrap();
    assert!(find_named(dialog.upcast_ref(),"export-appearance").is_none());
    combo(&restored, "export-output").set_selected(0);
    assert_eq!(combo(&restored, "export-output").selected(), 0);
    pump(500);
    assert!(!find_named(dialog.upcast_ref(),"export-clipping-group").unwrap().is_visible(),"empty group must not leave a bottom shadow");
    capture_ui(&restored, &directory, "sdr-export.png");
    combo(&restored, "export-output").set_selected(1);
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
    let guide=layer_color::build_local_tone_guide([512,384],RgbSpace::Srgb,||false,|y,row|{row.copy_from_slice(&painted[y as usize*512..(y as usize+1)*512]);Ok(())}).unwrap();
    for y in [0, 150, 383] {
        sdr.rows().read(y, &mut row).unwrap();
        decoder.decode_pixels(&row, &mut actual).unwrap();
        for (x, p) in actual.iter().enumerate() {
            let expected = recipe.mapper(RgbSpace::Srgb,RgbSpace::Srgb).map_local_premultiplied(painted[y as usize*512+x],[x as f32+0.5,y as f32+0.5],&guide);
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
    combo(&w, "export-output").set_selected(1);
    let dialog = w.window.visible_dialog().unwrap();
    let status = find_named(dialog.upcast_ref(), "color-preview-status").unwrap().downcast::<gtk::Label>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !status.text().contains("exceed") { pump(20); assert!(Instant::now() < deadline, "{}", status.text()); }
    assert!(!super::new_photo::export_enabled(&w));
    let clip = find_named(dialog.upcast_ref(), "export-hdr-clip").unwrap().downcast::<adw::SwitchRow>().unwrap();
    assert!(find_named(dialog.upcast_ref(), "export-clipping-group").unwrap().is_mapped(),"range warning must still expose clipping choice");
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
    combo(&w, "export-output").set_selected(0);
    pump(600);
    let dialog = w.window.visible_dialog().unwrap();
    assert!(!dialog.is::<adw::AlertDialog>());
    assert!(!find_named(dialog.upcast_ref(), "export-bpc").unwrap().is_mapped());
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
    combo(&w, "export-output").set_selected(1);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !super::new_photo::export_enabled(&w) { pump(20); assert!(Instant::now() < deadline); }
    assert!(!find_named(dialog.upcast_ref(), "export-open-color").unwrap().is_visible());
    assert!(find_named(dialog.upcast_ref(), "export-appearance").is_none());
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
    combo(&w,"export-output").set_selected(0);
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
    combo(&w, "export-output").set_selected(1); wait();
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
#[ignore = "isolated Wayland display and GPU; run with no photo codec bundle"]
#[allow(deprecated)]
fn portable_gainmap_export_without_codec_bundle() {
    use layer_core::color::{ColorProfile, source::*};
    let app=native_test_app("art.capycanvas.GainmapExport");
    let output=std::path::Path::new("../../artifacts/color-m4/gainmap-ui");std::fs::create_dir_all(output).unwrap();let output=output.canonicalize().unwrap();
    // These are generated test fixtures. Clear previous outputs so repeating
    // the journey does not leave an unanswered native overwrite confirmation.
    for name in ["Opaque edited HDR.jpg", "Transparent edited HDR.avif"] {
        let path=output.join(name);if path.exists(){std::fs::remove_file(path).unwrap();}
    }
    for transparent in [false,true] {
        let mut p=new_drawing(64,48).unwrap();p.document.color.depth=SampleDepth::F16;p.document.layers[1].visible=false;
        let mut source=SourceBuilder::new([64,48],SourceInterpretation{channels:SourceChannels::Rgba,depth:SampleDepth::F16,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},1024*1024).unwrap();
        for _ in 0..48{let mut row=Vec::new();for x in 0..64{let a=if transparent{x as f32/63.}else{1.};let v=layer_core::color::hdr::encode_pixel([4.,0.5,0.2,a]).unwrap();row.extend(v.into_iter().flat_map(u16::to_le_bytes));}source.push_row(&row).unwrap();}
        p.document.layers[0].source=Some(std::sync::Arc::new(source.finish().unwrap()));
        let w=Workspace::with_project(&app,Some((p,None)));w.window.present();ready(&w);
        let headroom=project(&w).document.sdr_rendition.headroom;
        invoke(&w,CommandId::SdrRendition);let panel=appearance(&w);appearance_button(&panel,"reset");
        assert!(find_named(panel.upcast_ref(),"sdr-appearance-auto").is_none());
        assert_eq!(project(&w).document.sdr_rendition.headroom,headroom,"Reset preserves the source range");
        let original=snapshot(&w);
        invoke(&w,CommandId::ExportDocument);
        let wait=|| {let deadline=Instant::now()+Duration::from_secs(40);while !super::new_photo::export_enabled(&w){pump(20);assert!(Instant::now()<deadline,"encoded preview");}};
        wait();pump(300);wait();
        assert_eq!(combo(&w,"export-output").selected(),if transparent{3}else{2});
        let dialog=w.window.visible_dialog().unwrap();
        assert!(!find_named(dialog.upcast_ref(),"export-format").unwrap().is_visible());
        let toggle=find_named(dialog.upcast_ref(),"export-rendition-view").unwrap().downcast::<adw::ToggleGroup>().unwrap();
        let picture=find_named(dialog.upcast_ref(),"color-preview-after").unwrap().downcast::<gtk::Picture>().unwrap();
        let hdr=picture.paintable().unwrap();toggle.set_active_name(Some("sdr"));pump(30);assert_ne!(picture.paintable().unwrap(),hdr);toggle.set_active_name(Some("hdr"));pump(30);assert_eq!(picture.paintable().unwrap(),hdr);
        capture_ui(&w,&output,if transparent{"avif-main.png"}else{"jpeg-main.png"});
        let name=if transparent{"Transparent edited HDR.avif"}else{"Opaque edited HDR.jpg"};
        response(&w,"export");let file=chooser();file.set_current_folder(Some(&gtk::gio::File::for_path(&output))).unwrap();file.set_current_name(name);pump(150);file.response(gtk::ResponseType::Accept);finish(&w);
        let source=layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(output.join(name)).unwrap()),Default::default()).unwrap();assert_eq!(source.interpretation.depth,SampleDepth::F16);
        let mut bytes=vec![0;source.row_bytes()];source.rows().read(0,&mut bytes).unwrap();let i=32*8;let p=layer_core::color::hdr::decode_pixel(std::array::from_fn(|c|u16::from_le_bytes([bytes[i+c*2],bytes[i+c*2+1]]))).unwrap();assert!(p[0]>3.7&&p[0]<4.3,"{p:?}");assert!((p[3]-if transparent{32./63.}else{1.}).abs()<0.001);
        if transparent {
            invoke(&w,CommandId::ExportDocument);combo(&w,"export-output").set_selected(2);
            pump(700);assert!(!super::new_photo::export_enabled(&w));
            let dialog=w.window.visible_dialog().unwrap();let flatten=find_named(dialog.upcast_ref(),"export-flatten").unwrap().downcast::<adw::SwitchRow>().unwrap();assert!(flatten.is_visible());flatten.set_active(true);wait();capture_ui(&w,&output,"jpeg-flatten.png");response(&w,"cancel");finish(&w);
        }
        assert_eq!(snapshot(&w),original);w.window.destroy();pump(100);
    }
}

#[test]
#[ignore = "private Wayland display; GTK widget picking including Scale children"]
fn native_proof_dial_hit_regions() {
    let _app = native_test_app("art.capycanvas.ProofDialHits");
    let mut shared_pattern = None;
    for size in [128, 160, 226, 320, 400] {
        let dial = crate::proof_dial::ProofDial::new();
        assert!(!dial.field.has_tooltip() && !dial.reset.has_tooltip() && dial.arcs.iter().all(|arc| !arc.has_tooltip()));
        let window = gtk::Window::builder().default_width(size).default_height(size).child(&dial.root).build();
        window.present(); pump(150);
        let field = &dial.field;
        let g = layer_ui::parameter_pad::ParameterDialGeometry::new(field.width().min(field.height()) as f32).unwrap();
        for y in (0..field.height()).step_by(2) {
            for x in (0..field.width()).step_by(2) {
                let distance = (x as f32-g.field.center[0]).hypot(y as f32-g.field.center[1]);
                if distance < g.field.disc_radius()-0.5 {
                    let p = field.compute_point(&dial.root, &gtk::graphene::Point::new(x as f32,y as f32)).unwrap();
                    let hit = dial.root.pick(p.x() as f64,p.y() as f64,gtk::PickFlags::DEFAULT).unwrap();
                    assert_eq!(hit,field.clone().upcast::<gtk::Widget>(),"size {size}, circle ({x},{y}) was stolen by {}",hit.widget_name());
                }
            }
        }
        for (i,arc) in dial.arcs.iter().enumerate() {
            for n in 0..=100 {
                let point=g.arcs[i].point(n as f32/100.);
                let p=arc.compute_point(&dial.root,&gtk::graphene::Point::new(point[0],point[1])).unwrap();
                let hit=dial.root.pick(p.x() as f64,p.y() as f64,gtk::PickFlags::DEFAULT).unwrap();
                assert_eq!(hit,arc.clone().upcast::<gtk::Widget>(),"visible arc {i} at {n}%");
            }
        }
        let deadline=Instant::now()+Duration::from_secs(3);
        while crate::proof_dial::pattern_cache_metrics().2.is_none() {pump(5);assert!(Instant::now()<deadline,"asynchronous glass texture");}
        pump(50);
        let (builds,generation_ms,texture)=crate::proof_dial::pattern_cache_metrics();
        let texture=texture.unwrap();assert_eq!(builds,1);
        if let Some(previous)=&shared_pattern {assert_eq!(previous,&texture,"share one texture across panels and sizes");}
        shared_pattern=Some(texture.clone());
        let arcs_before=dial.arc_snapshot_counts();
        let captions_before=dial.readout_cache_counts();
        let marker_before=dial.marker_node_identity().expect("selector is rendered");
        let start=Instant::now();
        for i in 0..120 {
            dial.set_recipe(layer_ui::proof_panel::sdr_from_pad(SdrRendition::default(),[(i as f64*0.1).sin(),i as f64/60.-1.]));
            pump(2);
            assert_eq!(crate::proof_dial::pattern_cache_metrics().2.as_ref(),Some(&texture));
        }
        pump(40);
        assert_eq!(dial.arc_snapshot_counts(),arcs_before,"circle movement must reuse both arc snapshots");
        assert_eq!(dial.marker_node_identity(),Some(marker_before),"movement translates the retained selector node");
        let captions_after=dial.readout_cache_counts();
        assert_eq!(&captions_before[2..],&captions_after[2..],"circle movement must reuse unchanged arc readouts");
        assert!(captions_after[0]>captions_before[0] && captions_after[1]>captions_before[1],"changing side values must still repaint");
        assert_eq!(crate::proof_dial::pattern_cache_metrics().0,1,"no rebuild during motion");
        eprintln!("PROOF_CACHE size={size} builds={builds} worker_ms={generation_ms:.2} updates=120 elapsed_with_event_pumping_ms={:.2} arc_redraws=0",start.elapsed().as_secs_f64()*1000.);
        dial.set_recipe(SdrRendition::default());
        // Repeated contacts inside the same quantized value must not dispatch
        // additional edits or redraw the full application.
        let moves=Rc::new(Cell::new(0));
        dial.connect_changed({let moves=moves.clone();move |phase,_| if phase==layer_ui::ContactPhase::Move {moves.set(moves.get()+1);}});
        let controllers=field.observe_controllers();
        let drag=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::GestureDrag>()).unwrap();
        let point=g.field.disc_marker([0.7,0.5]);
        drag.emit_by_name::<()>("drag-begin",&[&(point[0] as f64),&(point[1] as f64)]);
        for _ in 0..120 {drag.emit_by_name::<()>("drag-update",&[&0f64,&0f64]);}
        drag.emit_by_name::<()>("drag-end",&[&0f64,&0f64]);
        assert_eq!(moves.get(),1,"identical pad input should dispatch only one edit");
        dial.set_recipe(SdrRendition::default());pump(30);
        let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("../../artifacts/color-m4/proof-polish/controls"));
        std::fs::create_dir_all(&output).unwrap();
        let snap=gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&dial.root)).snapshot(&snap,dial.root.width() as f64,dial.root.height() as f64);
        window.renderer().unwrap().render_texture(&snap.to_node().unwrap(),None).save_to_png(output.join(format!("dial-{size}.png"))).unwrap();
        window.destroy();pump(20);
    }
}


#[test]
#[ignore = "private Mutter input and LAYER_NATIVE_CAPTURE_DIR compositor captures"]
fn native_proof_dial_composited_motion() {
    let app = native_test_app("art.capycanvas.ProofDialComposited");
    let mut project=new_drawing(64,64).unwrap();project.document.color.depth=SampleDepth::F16;
    let w=Workspace::with_project(&app,Some((project,None)));
    w.window.maximize();w.window.present();ready(&w);
    invoke(&w,CommandId::SdrRendition);appearance(&w);pump(300);
    let window=&w.window;
    let field=find_named(w.proof_panel.root.upcast_ref(),"sdr-tone-pad-surface").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while crate::proof_dial::pattern_cache_metrics().2.is_none() {pump(5);assert!(Instant::now()<deadline);}
    let cached = crate::proof_dial::pattern_cache_metrics().2.unwrap();
    let g = layer_ui::parameter_pad::ParameterDialGeometry::new(field.width() as f32).unwrap();
    let at = |p: [f32; 2]| {
        let p = field.compute_point(window, &gtk::graphene::Point::new(p[0],p[1])).unwrap();
        [p.x(),p.y()]
    };
    let dir = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let captures = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap());
    std::fs::create_dir_all(&captures).unwrap();
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut step=0;
    let mut perform = |events: serde_json::Value| {
        let path=dir.join(format!("step-{step}.json"));
        std::fs::write(path.with_extension("tmp"),serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(path.with_extension("tmp"),path).unwrap();
        let deadline=Instant::now()+Duration::from_secs(15);
        while !dir.join(format!("done-{step}")).exists() {pump(1);assert!(Instant::now()<deadline,"capture/input step {step}");}
        step+=1;
    };
    let center=at(g.field.center);
    perform(serde_json::json!([{"point":center,"down":true},{"wait_ms":250},{"capture":"before"}]));
    let load = |name: &str| {
        let texture=gtk::gdk::Texture::from_file(&gtk::gio::File::for_path(captures.join(format!("{name}.png")))).unwrap();
        let stride=texture.width() as usize*4;
        let mut bytes=vec![0;stride*texture.height() as usize];
        texture.download(&mut bytes,stride);
        (bytes,stride)
    };
    let (before,stride)=load("before");
    let scale=window.scale_factor() as f32;
    let radius=g.field.disc_radius();
    let mut worst=0;
    for i in 0..24 {
        let angle=i as f32*std::f32::consts::TAU/24.;
        let to=at([g.field.center[0]+radius*0.65*angle.cos(),g.field.center[1]+radius*0.65*angle.sin()]);
        let name=format!("motion-{i:02}");
        perform(serde_json::json!([{"point":to},{"wait_ms":24},{"capture":name}]));
        let (after,next_stride)=load(&name);assert_eq!(stride,next_stride);
        let recipe=w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition;
        let fraction=layer_ui::proof_panel::sdr_tone_pad().fractions(layer_ui::proof_panel::sdr_pad_values(recipe));
        let marker=at(g.field.disc_marker(fraction.map(|f| f as f32)));
        let mut changed=0;
        let mut selector_pixels=0;
        // The guide is immutable. Only the old and current marker footprints
        // may differ; intermediate markers must have been cleared by damage.
        for y in ((center[1]-radius)*scale) as usize..((center[1]+radius)*scale) as usize {
            for x in ((center[0]-radius)*scale) as usize..((center[0]+radius)*scale) as usize {
                let p=[(x as f32+0.5)/scale,(y as f32+0.5)/scale];
                let distance=|q:[f32;2]| (p[0]-q[0]).hypot(p[1]-q[1]);
                let at=y*stride+x*4;
                let different=before[at..at+3].iter().zip(&after[at..at+3]).any(|(a,b)|a.abs_diff(*b)>2);
                if distance(marker)<18. && different {selector_pixels+=1;}
                if distance(center)>radius-2. || distance(center)<18. || distance(marker)<18. {continue;}
                if different {changed+=1;}
            }
        }
        worst=worst.max(changed);
        assert_eq!(changed,0,"stale selector pixels in composited frame {i}");
        assert!(selector_pixels>20,"capture {i} must include the new selector in front of the guide");
    }
    perform(serde_json::json!([{"down":false}]));
    assert_eq!(crate::proof_dial::pattern_cache_metrics().2.as_ref(),Some(&cached));
    assert_eq!(crate::proof_dial::pattern_cache_metrics().0,1);
    std::fs::write(dir.join("finished"),"finished").unwrap();
    eprintln!("PROOF_COMPOSITED frames=24 scale={scale} stale_pixels={worst} texture_builds=1");
    window.destroy();pump(50);
}

#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_proof_dial_pointer_input"]
fn native_proof_dial_pointer_input() {
    let app = native_test_app("art.capycanvas.ProofDialPointer");
    let mut p = new_drawing(64,64).unwrap(); p.document.color.depth=SampleDepth::F16;
    let w=Workspace::with_project(&app,Some((p,None)));
    w.window.maximize(); w.window.present(); ready(&w);
    invoke(&w,CommandId::SdrRendition); appearance(&w); pump(300);
    let field=find_named(w.proof_panel.root.upcast_ref(),"sdr-tone-pad-surface").unwrap();
    let g=layer_ui::parameter_pad::ParameterDialGeometry::new(field.width().min(field.height()) as f32).unwrap();
    let at=|point:[f32;2]| {let p=field.compute_point(&w.window,&gtk::graphene::Point::new(point[0],point[1])).unwrap();[p.x(),p.y()]};
    let rendition=||w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition;
    let dir=std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let mut step=0;
    std::fs::write(dir.join("ready"),"ready").unwrap();
    let mut perform=|events:serde_json::Value| {
        let path=dir.join(format!("step-{step}.json"));
        std::fs::write(path.with_extension("tmp"),serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(path.with_extension("tmp"),path).unwrap();
        let until=Instant::now()+Duration::from_secs(20);
        while !dir.join(format!("done-{step}")).exists() {pump(2);assert!(Instant::now()<until);}
        step+=1;pump(80);
    };
    for touch in [false,true] {
        let event=|phase:&str,point:[f32;2]| if touch {serde_json::json!({"touch":phase,"point":point})}
            else {match phase {"down"=>serde_json::json!({"point":point,"down":true}),"up"=>serde_json::json!({"down":false}),_=>serde_json::json!({"point":point})}};
        // Actual down/move/up, including the horizontal strip stolen by the old
        // invisible GtkRange children. Drag across an arc without changing owner.
        for start in [[0.5,0.5],[0.2,0.5],[0.8,0.5],[0.5,0.2],[0.5,0.8]] {
            let before=rendition();
            let from=at(g.field.disc_marker(start));let to=at(g.arcs[0].point(0.7));
            perform(serde_json::json!([event("down",from),event("move",to),event("up",to)]));
            let after=rendition();assert_ne!(after,before,"circle contact must change contrast/scale; touch={touch}");
            assert_eq!((after.exposure,after.highlight_color),(before.exposure,before.highlight_color),"circle contact must not edit arcs");
            invoke(&w,CommandId::Undo);ready(&w);assert_eq!(rendition(),before,"one undo per circle gesture");
        }
        for i in 0..2 {
            let before=rendition();let from=at(g.arcs[i].point(0.2));let to=at(g.arcs[i].point(0.8));
            perform(serde_json::json!([event("down",from),event("move",to),event("up",to)]));
            let after=rendition();assert_eq!((after.contrast,after.balance),(before.contrast,before.balance));
            if i==0 {assert_ne!(after.exposure,before.exposure);assert_eq!(after.highlight_color,before.highlight_color);}
            else {assert_eq!(after.exposure,before.exposure);assert_ne!(after.highlight_color,before.highlight_color);}
            invoke(&w,CommandId::Undo);ready(&w);assert_eq!(rendition(),before,"one undo per arc gesture");
        }
    }
    // Double-click color resets to 30%, not zero, through real GTK delivery.
    let color=find_named(w.proof_panel.root.upcast_ref(),"sdr-appearance-highlight_color").unwrap().downcast::<gtk::Scale>().unwrap();
    color.set_value(0.8);pump(50);
    let point=at(g.arcs[1].point(0.6));
    perform(serde_json::json!([{"point":point,"down":true},{"down":false},{"down":true},{"down":false}]));
    assert_eq!(rendition().highlight_color,0.3);
    std::fs::write(dir.join("finished"),"done").unwrap();
    w.window.destroy();pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_float32_new_open_edit_save_and_exr_export() {
    let app = native_test_app("art.capycanvas.Float32Journey");
    let directory = std::env::temp_dir().join(format!("capy-float32-{}",std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing(64,64).unwrap(),None)));
    let opened = Rc::new(RefCell::new(None)); let result = opened.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |p,l,_| { result.replace(Some((p,l))); }));
    w.window.present(); ready(&w);
    invoke(&w, CommandId::NewDocument);
    combo(&w,"new-document-depth").set_selected(3);
    response(&w,"create"); finish(&w);
    assert_eq!(opened.borrow_mut().take().unwrap().0.document.color.depth, SampleDepth::F32);
    let path = directory.join("source.exr");
    layer_color::photo::write_exr_rows(std::fs::File::create(&path).unwrap(),[64,48],RgbSpace::DisplayP3,None,|_,row| {
        for (x,p) in row.iter_mut().enumerate() { *p=[70000.125+x as f32/32.,-0.125,1.0000001,1.]; } Ok(())
    }).unwrap();
    invoke(&w,CommandId::OpenDocument);
    let file=chooser(); file.set_file(&gtk::gio::File::for_path(&path)).unwrap(); pump(100);file.response(gtk::ResponseType::Accept);finish(&w);
    let (p,_) = opened.borrow_mut().take().unwrap();
    assert_eq!(p.document.color.depth,SampleDepth::F32);
    assert_eq!(p.document.color.space,RgbSpace::DisplayP3);
    let photo=Workspace::with_project(&app,Some((p,None)));photo.window.present();ready(&photo);
    effect(&photo,"exposure","exposure",layer_core::EffectValue::Number(-1.));
    let master=project(&photo);
    let before=pixels(&photo);
    assert!((before[0][0]-35000.0625).abs()<0.1,"{:?}",before[0]);
    invoke(&photo,CommandId::ExportDocument);
    let range=combo(&photo,"export-output");
    range.set_selected(range.model().unwrap().n_items()-1);range.notify("selected");
    let deadline=Instant::now()+Duration::from_secs(45);
    while !super::new_photo::export_enabled(&photo) {pump(20);assert!(Instant::now()<deadline,"EXR preview: {}",photo.status.text());}
    response(&photo,"export");let file=chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&directory))).unwrap();file.set_current_name("edited.exr");pump(150);file.response(gtk::ResponseType::Accept);finish(&photo);
    let result=layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(directory.join("edited.exr")).unwrap()),Default::default()).unwrap();
    assert_eq!(result.interpretation.depth,SampleDepth::F32);
    assert_eq!(result.interpretation.profile,layer_core::color::ColorProfile::Builtin(RgbSpace::DisplayP3));
    let mut row=vec![0;result.row_bytes()];result.rows().read(0,&mut row).unwrap();
    let pixel=layer_core::color::hdr::decode_samples(SampleDepth::F32,&row[..16]).unwrap();
    assert!((pixel[0]-35000.0625).abs()<0.1,"{pixel:?}");
    assert_eq!(project(&photo),master);
    photo.window.destroy();w.window.destroy();pump(100);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_close_cancels_pending_local_analysis() {
    let app = native_test_app("art.capycanvas.HdrCloseAnalysis");
    for depth in [SampleDepth::F16, SampleDepth::F32] {
        let mut project = new_drawing(4096, 4096).unwrap();
        project.document.color.depth = depth;
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.present();
        ready(&w);
        // Startup may already have completed its first guide. Invalidate it
        // through an ordinary document edit before observing the next job.
        w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
        let deadline = Instant::now() + Duration::from_secs(30);
        while w.local_tone.worker_state().1 != Some(false) {
            pump(5);
            assert!(Instant::now() < deadline, "local analysis never started");
        }
        w.window.destroy();
        assert_eq!(w.local_tone.worker_state(), (true, Some(true)), "canvas unrealize must cancel before the future releases its window reference (visible={}, mapped={}, realized={})", w.window.is_visible(), w.window.is_mapped(), w.window.is_realized());
        while w.local_tone.worker_state().0 {
            pump(5);
            assert!(Instant::now() < deadline, "cancelled local analysis did not finish");
        }
        pump(50);
    }
}
