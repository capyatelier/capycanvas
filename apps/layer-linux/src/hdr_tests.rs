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
    combo(&restored, "export-output").set_selected(0);
    response(&restored, "appearance");
    let window = appearance(&restored);
    appearance_exposure(&window, -0.75);
    find_named(restored.proof_panel.root.upcast_ref(),"proof-return-export").unwrap().downcast::<gtk::Button>().unwrap().emit_clicked();
    recipe.exposure = -0.75;
    assert_eq!(project(&restored).document.sdr_rendition, recipe);
    let deadline = Instant::now() + Duration::from_secs(30);
    while restored.window.visible_dialog().is_none() { pump(20); assert!(Instant::now() < deadline); }
    assert_eq!(combo(&restored, "export-output").selected(), 0);
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
#[ignore = "Wayland/GPU and LAYER_HDR_LARGE_INPUT pointing to a retained 60 MP HDR fixture"]
fn native_hdr_large_export_preview_and_cancellation() {
    large_export_preview_and_cancellation(1);
}
#[test]
#[ignore = "Wayland/GPU and LAYER_HDR_LARGE_INPUT pointing to a retained 60 MP HDR fixture"]
fn native_hdr_large_sdr_export_preview_and_cancellation() {
    large_export_preview_and_cancellation(0);
}
#[test]
#[ignore="60 MP HDR fixture, native GPU and pinned codecs"]
fn native_hdr_large_gainmap_export_preview_and_cancellation(){large_export_preview_and_cancellation(if std::env::var("LAYER_HDR_GAINMAP").as_deref()==Ok("avif"){3}else{2});}
fn large_export_preview_and_cancellation(output:u32) {
    let path = std::env::var_os("LAYER_HDR_LARGE_INPUT").expect("60 MP fixture path");
    let mut p = layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    if std::env::var_os("LAYER_HDR_DEFAULT_RENDITION").is_some() {
        p.document.sdr_rendition = Default::default();
    }
    eprintln!("HDR_LARGE_RENDITION {:?}", p.document.sdr_rendition);
    assert_eq!(p.document.color.depth, SampleDepth::F16);
    assert!(u64::from(p.document.width) * u64::from(p.document.height) >= 59_000_000);
    let app = native_test_app("art.capycanvas.HdrLargePreview");
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present(); ready(&w);
    let revision = w.gpu.borrow().as_ref().unwrap().session.engine().document().revision;
    invoke(&w, CommandId::ExportDocument);
    combo(&w, "export-output").set_selected(output);
    if output==2 {find_named(w.window.visible_dialog().unwrap().upcast_ref(),"export-flatten").unwrap().downcast::<adw::SwitchRow>().unwrap().set_active(true);}
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
        assert!(start.elapsed().as_secs() < 600, "60 MP preview: {}", status.text());
    }
    timer.remove();
    eprintln!("HDR_LARGE_PREVIEW output={output} elapsed_ms={:.2} heartbeat_count={} max_heartbeat_gap_ms={:.2} status={}",
        start.elapsed().as_secs_f64()*1000., heartbeat.borrow().1, heartbeat.borrow().2 as f64/1000., status.text());
    assert!(heartbeat.borrow().1 > 10, "no UI heartbeat while previewing");
    assert!(heartbeat.borrow().2 < 500_000, "main-thread stall during preview");
    for _ in 0..8 { combo(&w, "export-output").set_selected(0); combo(&w, "export-output").set_selected(1); }
    pump(50);
    let cancel = Instant::now();
    response(&w, "cancel"); finish(&w);
    eprintln!("HDR_LARGE_PREVIEW cancel_ms={:.2}", cancel.elapsed().as_secs_f64()*1000.);
    assert!(cancel.elapsed().as_secs_f64() < 3.);
    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().revision, revision);
    w.window.destroy(); pump(100);
}

#[test]
#[ignore = "isolated Wayland display, GPU and pinned HDR codecs"]
#[allow(deprecated)]
fn native_gainmap_export_choices_preview_flatten_and_reopen() {
    use layer_core::color::{ColorProfile, source::*};
    assert!(layer_color::photo::gainmap_available());
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
#[ignore = "private Wayland/GPU and downloaded local-tone review fixtures"]
fn native_local_tone_pad_and_five_hdr_photos() {
    let app=native_test_app("art.capycanvas.LocalToneReview");
    let directory=std::path::Path::new("../../artifacts/color-m4/local-tone/images/review").canonicalize().unwrap();
    let evidence=std::path::Path::new("../../artifacts/color-m4/proof-polish/native");std::fs::create_dir_all(evidence).unwrap();let evidence=evidence.canonicalize().unwrap();
    for name in ["abandoned_hall_01","venice_sunset","neon_photostudio","kiara_1_dawn","studio_small_09"] {
        let path=directory.join(format!("{name}_2k.capy"));
        let mut p=layer_core::Project::read(std::fs::File::open(&path).unwrap(),Default::default()).unwrap();
        p.document.sdr_rendition=SdrRendition {headroom:p.document.sdr_rendition.headroom,..Default::default()};
        let recipe=p.document.sdr_rendition;let source=p.document.layers[0].source.clone();
        let w=Workspace::with_project(&app,Some((p,None)));w.window.present();ready(&w);invoke(&w,CommandId::SdrRendition);appearance(&w);
        let start=Instant::now();
        while w.local_tone.ready_count().is_none(){pump(10);assert!(start.elapsed()<Duration::from_secs(60),"analysis: {}",w.local_tone.label.text());}
        let count=w.local_tone.ready_count();eprintln!("LOCAL_PHOTO {name} analysis_ready_ms={:.2}",start.elapsed().as_secs_f64()*1000.);
        capture_ui(&w,&evidence,&format!("{name}.png"));
        let pad=find_named(w.proof_panel.root.upcast_ref(),"sdr-tone-pad-surface").unwrap();assert!(pad.is_mapped());assert!(pad.width()>70 && pad.height()>=60);
        let controllers=pad.observe_controllers();let drag=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::GestureDrag>()).unwrap();
        let keys=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::EventControllerKey>()).unwrap();
        let clicks=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::GestureClick>()).unwrap();
        let geometry=layer_ui::parameter_pad::ParameterDialGeometry::new(pad.width().min(pad.height()) as f32).unwrap();
        // Exercise the visible cardinal positions, not just changed settings.
        // Each axis edit reuses the guide and one Undo restores the exact baseline.
        for (label,axis,expected) in [
            ("low",[0.5,0.001],[0.,0.5]),
            ("high",[0.5,0.999],[0.,2.]),
            ("macro",[0.001,0.5],[-1.,1.]),
            ("micro",[0.999,0.5],[1.,1.]),
        ] {
            let point=geometry.field.disc_marker(axis);
            drag.emit_by_name::<()>("drag-begin",&[&(point[0] as f64),&(point[1] as f64)]);
            drag.emit_by_name::<()>("drag-end",&[&0f64,&0f64]);pump(150);ready(&w);
            let value=w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition;
            assert!((value.balance-expected[0]).abs()<1e-5 && (value.contrast-expected[1]).abs()<1e-5,"{label}: {value:?}");
            capture_ui(&w,&evidence,&format!("{name}-{label}.png"));
            assert_eq!(w.local_tone.ready_count(),count);
            invoke(&w,CommandId::Undo);ready(&w);
            assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition,recipe);
        }
        let start=geometry.field.disc_marker([0.2,0.5]);let end=geometry.field.disc_marker([0.8,0.9]);
        let delta=[(end[0]-start[0]) as f64,(end[1]-start[1]) as f64];
        let gesture=Instant::now();drag.emit_by_name::<()>("drag-begin",&[&(start[0] as f64),&(start[1] as f64)]);
        for i in 1..=30 {drag.emit_by_name::<()>("drag-update",&[&(delta[0]*i as f64/30.),&(delta[1]*i as f64/30.)]);pump(5);}
        drag.emit_by_name::<()>("drag-end",&[&delta[0],&delta[1]]);ready(&w);
        eprintln!("LOCAL_PHOTO {name} 30_pad_updates_ms={:.2}",gesture.elapsed().as_secs_f64()*1000.);
        let edited=project(&w).document.sdr_rendition;assert_ne!(edited.contrast,recipe.contrast);assert_ne!(edited.balance,recipe.balance);
        assert_eq!(w.local_tone.ready_count(),count,"pad must reuse analysis");
        invoke(&w,CommandId::Undo);ready(&w);assert_eq!(project(&w).document.sdr_rendition,recipe,"one undo per drag");
        invoke(&w,CommandId::Redo);ready(&w);assert_eq!(project(&w).document.sdr_rendition,edited);
        assert!(keys.emit_by_name::<bool>("key-pressed",&[&gdk::Key::Left,&0u32,&gdk::ModifierType::empty()]));
        assert_ne!(w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition,edited);
        keys.emit_by_name::<bool>("key-pressed",&[&gdk::Key::Escape,&0u32,&gdk::ModifierType::empty()]);pump(20);assert_eq!(project(&w).document.sdr_rendition,edited,"Escape restores gesture");
        clicks.emit_by_name::<()>("pressed",&[&2i32,&(geometry.field.center[0] as f64),&(geometry.field.center[1] as f64)]);pump(20);
        clicks.emit_by_name::<()>("released",&[&2i32,&(geometry.field.center[0] as f64),&(geometry.field.center[1] as f64)]);
        let reset=project(&w).document.sdr_rendition;assert_eq!((reset.contrast,reset.balance),(recipe.contrast,recipe.balance));
        assert_eq!(project(&w).document.layers[0].source,source,"proof must not edit source");
        assert_eq!(w.local_tone.ready_count(),count);
        if name=="abandoned_hall_01" {
            assert!(find_named(w.proof_panel.root.upcast_ref(),"sdr-appearance-auto").is_none());
            assert!(find_named(w.proof_panel.root.upcast_ref(),"sdr-tone-pad-tone").is_none());
            for (i,key) in ["exposure","highlight_color"].into_iter().enumerate() {
                let arc=find_named(w.proof_panel.root.upcast_ref(),&format!("sdr-appearance-{key}")).unwrap().downcast::<gtk::Scale>().unwrap();
                let geometry=layer_ui::parameter_pad::ParameterDialGeometry::new(arc.width().min(arc.height()) as f32).unwrap();
                let g=geometry.arcs[i];
                let parent=arc.parent().unwrap();
                let point=arc.compute_point(&parent,&gtk::graphene::Point::new(g.point(0.5)[0],g.point(0.5)[1])).unwrap();
                let picked=parent.pick(point.x() as f64,point.y() as f64,gtk::PickFlags::DEFAULT).unwrap();
                assert!(picked==arc.clone().upcast::<gtk::Widget>() || picked.is_ancestor(&arc),"visible arc must receive its own pointer hits: {}",picked.widget_name());
                assert!(!arc.contains(g.center[0] as f64,g.center[1] as f64),"arc cannot steal circle contacts");
                let controllers=arc.observe_controllers();
                let drag=(0..controllers.n_items()).filter_map(|j|controllers.item(j).and_downcast::<gtk::GestureDrag>()).find(|g|g.propagation_phase()==gtk::PropagationPhase::Capture).unwrap();
                let from=g.point(0.2);let to=g.point(0.8);let delta=[(to[0]-from[0]) as f64,(to[1]-from[1]) as f64];
                let before=project(&w).document.sdr_rendition;
                drag.emit_by_name::<()>("drag-begin",&[&(from[0] as f64),&(from[1] as f64)]);
                drag.emit_by_name::<()>("drag-update",&[&delta[0],&delta[1]]);
                drag.emit_by_name::<()>("drag-end",&[&delta[0],&delta[1]]);pump(60);
                let edited=project(&w).document.sdr_rendition;
                assert_ne!(edited,before);assert_eq!((edited.contrast,edited.balance,edited.headroom),(before.contrast,before.balance,before.headroom));
                invoke(&w,CommandId::Undo);ready(&w);assert_eq!(project(&w).document.sdr_rendition,before,"one undo per arc gesture");
                drag.emit_by_name::<()>("drag-begin",&[&(to[0] as f64),&(to[1] as f64)]);
                let keys=(0..controllers.n_items()).filter_map(|j|controllers.item(j).and_downcast::<gtk::EventControllerKey>()).find(|g|g.propagation_phase()==gtk::PropagationPhase::Capture).unwrap();
                keys.emit_by_name::<bool>("key-pressed",&[&gdk::Key::Escape,&0u32,&gdk::ModifierType::empty()]);pump(30);
                assert_eq!(project(&w).document.sdr_rendition,before,"Escape cancels arc gesture");
                assert_eq!(w.local_tone.ready_count(),count,"arcs reuse full-image analysis");
            }
            invoke(&w,CommandId::ExportDocument);combo(&w,"export-output").set_selected(0);
            let deadline=Instant::now()+Duration::from_secs(60);while !super::new_photo::export_enabled(&w){pump(20);assert!(Instant::now()<deadline);}
            capture_ui(&w,&evidence,"local-sdr-export.png");response(&w,"cancel");finish(&w);
        }
        w.window.destroy();pump(100);
    }
}

#[test]
#[ignore = "private Wayland/GPU and LAYER_HDR_LARGE_INPUT 60 MP fixture"]
fn native_hdr_large_proof_dial_responsiveness() {
    let app = native_test_app("art.capycanvas.LargeProofDial");
    let path = std::env::var_os("LAYER_HDR_LARGE_INPUT").unwrap();
    let mut p = layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    assert!(u64::from(p.document.width) * u64::from(p.document.height) >= 59_000_000);
    p.document.sdr_rendition = Default::default();
    let layers = p.document.layers.clone();
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present(); ready(&w); invoke(&w, CommandId::SdrRendition); appearance(&w);
    let heartbeat = Rc::new(RefCell::new((Instant::now(), 0u128)));
    let timer = glib::timeout_add_local(std::time::Duration::from_millis(10), {
        let heartbeat=heartbeat.clone(); move || {
            let mut h=heartbeat.borrow_mut(); h.1=h.1.max(h.0.elapsed().as_micros());h.0=Instant::now();glib::ControlFlow::Continue
        }
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while w.local_tone.ready_count().is_none() { pump(10); assert!(Instant::now()<deadline); }
    let count=w.local_tone.ready_count();
    let field=find_named(w.proof_panel.root.upcast_ref(),"sdr-tone-pad-surface").unwrap();
    let geometry=layer_ui::parameter_pad::ParameterDialGeometry::new(field.width().min(field.height()) as f32).unwrap();
    let start=geometry.field.disc_marker([0.2,0.4]);let end=geometry.field.disc_marker([0.8,0.9]);
    let controllers=field.observe_controllers();
    let drag=(0..controllers.n_items()).find_map(|i|controllers.item(i).and_downcast::<gtk::GestureDrag>()).unwrap();
    let rendition=||w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition;
    let before=rendition();
    let started=Instant::now();
    drag.emit_by_name::<()>("drag-begin",&[&(start[0] as f64),&(start[1] as f64)]);
    for i in 1..=60 {
        drag.emit_by_name::<()>("drag-update",&[&((end[0]-start[0]) as f64*i as f64/60.),&((end[1]-start[1]) as f64*i as f64/60.)]);pump(16);
    }
    drag.emit_by_name::<()>("drag-end",&[&((end[0]-start[0]) as f64),&((end[1]-start[1]) as f64)]);pump(100);
    let elapsed=started.elapsed().as_secs_f64()*1000.;
    timer.remove();
    eprintln!("PROOF_DIAL_60MP updates=60 elapsed_ms={elapsed:.2} max_heartbeat_gap_ms={:.2}",heartbeat.borrow().1 as f64/1000.);
    assert!(heartbeat.borrow().1<500_000,"UI stalled during local proof adjustment");
    assert_eq!(w.local_tone.ready_count(),count,"drag must not rescan the 60 MP master");
    assert_ne!(rendition(),before);
    invoke(&w,CommandId::Undo);ready(&w);assert_eq!(rendition(),before);
    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().layers,layers,"Proof must preserve the master");
    w.window.destroy();pump(100);
}


#[test]
#[ignore = "private Wayland display; GTK widget picking including Scale children"]
fn native_proof_dial_hit_regions() {
    let _app = native_test_app("art.capycanvas.ProofDialHits");
    for size in [128, 160, 226, 320, 400] {
        let dial = crate::proof_dial::ProofDial::new();
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
