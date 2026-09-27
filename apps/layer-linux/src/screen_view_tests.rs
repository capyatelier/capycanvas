use super::new_photo::{capture_ui, chooser, finish, ready};
use super::proof::wait_proof;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};

fn half_green_drawing() -> layer_core::Project {
    let mut project = new_drawing(96, 64).unwrap();
    project.document.color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
    let mut source = SourceBuilder::new(
        [96, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    let row: Vec<u8> = (0..96)
        .flat_map(|x| if x < 48 { [0u16, 65535, 0, 65535] } else { [30000, 30000, 30000, 65535] })
        .flat_map(u16::to_le_bytes)
        .collect();
    for _ in 0..64 {
        source.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    project
}

fn chip(w: &Rc<Workspace>, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while w.screen.chip_label().as_deref() != Some(label) {
        pump(20);
        assert!(
            Instant::now() < deadline,
            "screen chip {label}: {:?}, {:?}",
            w.screen.chip_label(),
            state(w).screen
        );
    }
}

fn canvas_pixel(w: &Rc<Workspace>, document: [f32; 2]) -> [u8; 4] {
    let capture = w.gpu.borrow().as_ref().unwrap().session.engine().backend().capture_in(w.view_color()).unwrap();
    let m = state(w).camera.document_to_surface();
    let x = (m[0] * document[0] + m[2] * document[1] + m[4]).round() as usize;
    let y = (m[1] * document[0] + m[3] * document[1] + m[5]).round() as usize;
    capture.bytes[y * capture.stride as usize + x * 4..][..4].try_into().unwrap()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_screen_status_marks_clipped_colors_and_follows_proofing() {
    let app = native_test_app("art.capycanvas.ScreenStatus");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "../../artifacts/screen-status/gtk".into());
    std::fs::create_dir_all(&output).unwrap();
    let w = Workspace::with_project(&app, Some((half_green_drawing(), None)));
    w.window.present();
    ready(&w);
    eprintln!("SCREEN_REPORT {:?}", state(&w).screen);
    chip(&w, "Colors clipped");
    capture_ui(&w, &output, "clipped.png");
    let gray = canvas_pixel(&w, [72., 32.]);
    let green = canvas_pixel(&w, [24., 32.]);

    w.screen.button.emit_clicked();
    pump(100);
    let popover = w.screen.popover();
    assert!(popover.is_visible());
    capture_popover(popover, output.join("clipped-details.png").to_str().unwrap());
    let mark = find_named(popover.upcast_ref(), "screen-mark-clipped").unwrap().downcast::<gtk::CheckButton>().unwrap();
    assert!(!mark.is_active());
    mark.set_active(true);
    pump(300);
    assert!(state(&w).screen.show_clipped);
    assert_eq!(canvas_pixel(&w, [72., 32.]), gray, "colors on the screen stay unmarked");
    let marked = canvas_pixel(&w, [24., 32.]);
    assert_ne!(marked, green);
    assert!(marked[2] > 200 && marked[1] < 150, "clipped colors are marked blue: {marked:?}");
    capture_popover(popover, output.join("clipped-marked-details.png").to_str().unwrap());
    popover.popdown();
    capture_ui(&w, &output, "clipped-marked.png");
    w.dispatch(UiAction::ShowClippedColors { visible: false });
    pump(300);
    assert_eq!(canvas_pixel(&w, [24., 32.]), green);

    w.dispatch(UiAction::Invoke { command: CommandId::SoftProofSetup });
    pump(150);
    find_named(w.proof_panel.root.upcast_ref(), "proof-mode")
        .unwrap()
        .downcast::<adw::ToggleGroup>()
        .unwrap()
        .set_active_name(Some("print"));
    super::new_photo::profile_action(&w, "proof", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path("/usr/share/color/icc/krita/cmyk.icm")).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    wait_proof(&w, "Proof:");
    let deadline = Instant::now() + Duration::from_secs(20);
    while state(&w).screen.clipped.is_none() {
        pump(20);
        assert!(Instant::now() < deadline, "proof check: {:?}", state(&w).screen);
    }
    assert_ne!(w.screen.chip_label().as_deref(), Some("May not match print"), "the desktop describes this screen");
    assert_eq!(w.screen.chip_label().is_some(), state(&w).screen.clipped == Some(true));
    capture_ui(&w, &output, "proof.png");
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_screen_status_describes_hdr_drawings() {
    let app = native_test_app("art.capycanvas.ScreenStatusHdr");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "../../artifacts/screen-status/gtk".into());
    std::fs::create_dir_all(&output).unwrap();
    let mut project = new_drawing(96, 64).unwrap();
    project.document.color.depth = SampleDepth::F16;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !matches!(w.screen.chip_label().as_deref(), Some("HDR" | "Showing SDR")) || state(&w).screen.assessment.basis == layer_color::screen::Basis::Pending {
        pump(20);
        assert!(Instant::now() < deadline, "HDR chip: {:?}", w.screen.chip_label());
    }
    eprintln!("SCREEN_REPORT {:?} chip {:?}", state(&w).screen, w.screen.chip_label());
    capture_ui(&w, &output, "hdr.png");
    w.screen.button.emit_clicked();
    pump(100);
    let details = w.gpu.borrow().as_ref().unwrap().session.screen_details().unwrap();
    assert!(details.headline.starts_with("Showing"));
    capture_popover(w.screen.popover(), output.join("hdr-details.png").to_str().unwrap());
    w.screen.popover().popdown();
}

fn monitor_report(name: &str, edid: &[u8], mode: &str) -> layer_color::screen::ScreenReport {
    use layer_color::screen::{Chromaticities, CompositorDescription, ScreenColor, ScreenReport, Transfer, edid};
    let monitor = edid::parse(edid).unwrap();
    let (target, transfer, white, peak) = match mode {
        "hdr" => (Chromaticities::BT2020, Transfer::Pq, if name == "LG TV SSCR2" { 426. } else { 416. }, 10_000.),
        "native" => (monitor.chromaticities, Transfer::Gamma(2.2), 80., 80.),
        _ => (Chromaticities::of(RgbSpace::Srgb), Transfer::Gamma(2.2), 80., 80.),
    };
    ScreenReport {
        name: Some(name.into()),
        color: ScreenColor::Described(CompositorDescription { primaries: target, target, transfer, reference_white: white, target_peak: peak }),
        monitor: Some(monitor),
    }
}

const CINTIQ: &[u8] = include_bytes!("../../../crates/layer-color/tests/fixtures/edid/cintiq-pro-27.bin");
const LG_TV: &[u8] = include_bytes!("../../../crates/layer-color/tests/fixtures/edid/lg-tv.bin");

fn force(w: &Rc<Workspace>, report: layer_color::screen::ScreenReport) {
    w.screen.forced.replace(Some(report));
    w.changed(Ok(layer_ui::UiChange { regions: layer_ui::regions::HOST, ..Default::default() }));
    let deadline = Instant::now() + Duration::from_secs(20);
    while state(w).screen.assessment.gamut.is_some() && state(w).screen.clipped.is_none() {
        pump(20);
        assert!(Instant::now() < deadline, "screen check: {:?}", state(w).screen);
    }
    pump(200);
}

fn gallery(w: &Rc<Workspace>, output: &std::path::Path, name: &str) {
    eprintln!("SCREEN_GALLERY {name}: chip {:?}, clipped {:?}", w.screen.chip_label(), state(w).screen.clipped);
    capture_ui(w, output, &format!("{name}.png"));
    if w.screen.button.is_visible() {
        w.screen.button.emit_clicked();
        pump(100);
        capture_popover(w.screen.popover(), output.join(format!("{name}-details.png")).to_str().unwrap());
        w.screen.popover().popdown();
        pump(50);
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_screen_status_gallery() {
    let app = native_test_app("art.capycanvas.ScreenGallery");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "../../artifacts/screen-status/gtk-gallery".into());
    std::fs::create_dir_all(&output).unwrap();
    let mut project = new_drawing(96, 64).unwrap();
    project.document.color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
    let mut source = SourceBuilder::new(
        [96, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    let row: Vec<u8> = (0..96)
        .flat_map(|x| if x < 48 { [65535u16, 26000, 3000, 65535] } else { [30000, 30000, 30000, 65535] })
        .flat_map(u16::to_le_bytes)
        .collect();
    for _ in 0..64 {
        source.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);

    force(&w, monitor_report("Cintiq Pro 27", CINTIQ, "default"));
    gallery(&w, &output, "1-cintiq-default");
    force(&w, monitor_report("Cintiq Pro 27", CINTIQ, "hdr"));
    gallery(&w, &output, "2-cintiq-hdr");

    w.dispatch(UiAction::Invoke { command: CommandId::SoftProofSetup });
    pump(150);
    find_named(w.proof_panel.root.upcast_ref(), "proof-mode")
        .unwrap()
        .downcast::<adw::ToggleGroup>()
        .unwrap()
        .set_active_name(Some("print"));
    super::new_photo::profile_action(&w, "proof", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path("/usr/share/color/icc/krita/cmyk.icm")).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    wait_proof(&w, "Proof:");
    force(&w, monitor_report("Cintiq Pro 27", CINTIQ, "native"));
    gallery(&w, &output, "3-proof-cintiq-native");
    force(&w, monitor_report("Cintiq Pro 27", CINTIQ, "hdr"));
    gallery(&w, &output, "4-proof-cintiq-hdr");
    force(&w, monitor_report("LG TV SSCR2", LG_TV, "hdr"));
    gallery(&w, &output, "5-proof-lg-hdr");
    force(&w, monitor_report("Cintiq Pro 27", CINTIQ, "default"));
    gallery(&w, &output, "6-proof-cintiq-default");
}
