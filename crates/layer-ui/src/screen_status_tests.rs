fn screen_report(color: layer_color::screen::ScreenColor) -> layer_color::screen::ScreenReport {
    let monitor = layer_color::screen::edid::parse(include_bytes!("../../layer-color/tests/fixtures/edid/cintiq-pro-27.bin")).unwrap();
    layer_color::screen::ScreenReport {
        name: Some("Cintiq Pro 27".into()),
        color,
        hdr_capable: Some(monitor.pq_signal),
        monitor: Some(monitor),
    }
}

fn gnome(mode: &str) -> layer_color::screen::ScreenColor {
    use layer_color::screen::{Chromaticities, CompositorDescription, ScreenColor, Transfer};
    let (target, transfer, white, peak) = match mode {
        "hdr" => (Chromaticities::BT2020, Transfer::Pq, 416., 10_000.),
        "native" => (screen_report(ScreenColor::Pending).monitor.unwrap().chromaticities, Transfer::Gamma(2.2), 80., 80.),
        _ => (Chromaticities::of(layer_core::color::RgbSpace::Srgb), Transfer::Gamma(2.2), 80., 80.),
    };
    ScreenColor::Described(CompositorDescription { primaries: target, target, transfer, reference_white: white, target_peak: Some(peak) })
}

fn screen_session(space: layer_core::color::RgbSpace, depth: layer_core::color::SampleDepth) -> UiSession<Recorder> {
    let mut document = Document::new("Screen", 32, 32);
    document.color.space = space;
    document.color.depth = depth;
    UiSession::new(Recorder { color: document.color, ..Default::default() }, document, [32, 32], Platform::Gtk).unwrap()
}

fn chip(s: &UiSession<Recorder>) -> Option<(&'static str, bool)> {
    s.screen_chip().map(|c| (c.label, c.warning))
}

#[test]
fn sdr_drawings_only_mention_the_screen_when_colors_are_clipped() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    assert!(s.set_screen_report(screen_report(gnome("default"))));
    assert!(!s.set_screen_report(screen_report(gnome("default"))));
    assert_eq!(chip(&s), None);
    s.set_screen_clipped(Some(false));
    assert_eq!(chip(&s), None);
    s.set_screen_clipped(Some(true));
    assert_eq!(chip(&s), Some(("Colors clipped", true)));
    assert_eq!(s.state.screen.chip.map(|c| c.label), Some("Colors clipped"), "hosts read the chip from state");
    assert_eq!(s.state.screen.details, s.screen_details());
    s.state.workspace.layout.canvas_info.visible = false;
    assert_eq!(chip(&s), None, "the footer is hidden");
}

#[test]
fn proofing_always_says_whether_the_screen_can_be_trusted() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::Srgb, SampleDepth::U8);
    s.state.soft_proof = true;
    assert_eq!(chip(&s), None, "no report yet");
    s.set_screen_report(screen_report(gnome("native")));
    assert_eq!(chip(&s), None, "a trustworthy screen needs no chip");
    assert_eq!(s.screen_details(), None);
    s.set_screen_report(screen_report(gnome("hdr")));
    assert_eq!(chip(&s), Some(("May not match print", false)));
    s.set_screen_clipped(Some(true));
    assert_eq!(chip(&s), Some(("Colors clipped", true)));
}

#[test]
fn hdr_drawings_keep_their_presentation_label_until_proofed() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::Srgb, SampleDepth::F16);
    s.set_screen_report(screen_report(gnome("default")));
    assert_eq!(chip(&s), Some(("Showing SDR", false)));
    let details = s.screen_details().unwrap();
    assert_eq!(details.headline, "Showing the SDR version");
    assert_eq!(
        details.body.as_deref(),
        Some("HDR is off for this screen. Turn it on in your operating system’s display settings to see HDR highlights.")
    );
    s.set_screen_report(screen_report(gnome("hdr")));
    assert_eq!(chip(&s), Some(("Showing SDR", false)));
    assert_eq!(
        s.screen_details().unwrap().body.as_deref(),
        Some("At your current screen brightness, regular content already uses all of this screen’s brightness, leaving nothing brighter for HDR highlights. Lower the screen brightness to see them.")
    );
    let mut television = screen_report(gnome("hdr"));
    television.monitor = Some(layer_color::screen::edid::parse(include_bytes!("../../layer-color/tests/fixtures/edid/lg-tv.bin")).unwrap());
    s.set_screen_report(television);
    s.set_hdr_display_available(true);
    assert_eq!(chip(&s), Some(("HDR", false)));
    assert_eq!(s.screen_details().unwrap().headline, "Showing HDR");
    assert_eq!(
        s.screen_details().unwrap().body.as_deref(),
        Some("Capy Canvas can’t tell how bright this screen can get, so the brightest highlights may look dimmer than they are.")
    );
    let mut described = screen_report(gnome("native"));
    if let layer_color::screen::ScreenColor::Described(d) = &mut described.color {
        d.transfer = layer_color::screen::Transfer::Pq;
        d.reference_white = 200.;
        d.target_peak = Some(1000.);
    }
    s.set_screen_report(described);
    assert_eq!(
        s.screen_details().unwrap().body.as_deref(),
        Some("This screen can show highlights up to 5× (+2.3 EV).")
    );
    s.set_screen_report(screen_report(gnome("hdr")));
    s.state.preview_sdr = true;
    assert_eq!(chip(&s), Some(("SDR preview", false)));
    s.state.preview_sdr = false;
    s.state.gamut_warning = true;
    assert_eq!(chip(&s), Some(("May not match print", false)));
}

#[test]
fn details_explain_the_screen_briefly() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    s.state.soft_proof = true;
    s.set_screen_report(screen_report(gnome("hdr")));
    let details = s.screen_details().unwrap();
    assert_eq!(details.title, "Cintiq Pro 27");
    assert_eq!(details.headline, "The proof may not match the print");
    assert_eq!(
        details.body.as_deref(),
        Some("With HDR on, Capy Canvas can’t tell how this screen shows colors. Turn off HDR for this screen in your operating system’s display settings.")
    );
    assert_eq!(details.show_clipped, None);

    s.set_screen_report(screen_report(gnome("default")));
    assert!(s.screen_details().unwrap().body.unwrap().starts_with("Your operating system is sending colors as if this monitor’s color mode were sRGB."));

    s.state.soft_proof = false;
    s.set_screen_report(screen_report(gnome("native")));
    s.set_screen_clipped(Some(true));
    assert_eq!(s.screen_details().unwrap().body, None, "the headline and highlight switch say it all");
}

#[test]
fn clipped_colors_can_be_marked_from_the_details() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    s.set_screen_report(screen_report(gnome("default")));
    s.set_screen_clipped(Some(true));
    let details = s.screen_details().unwrap();
    assert!(details.warning);
    assert_eq!(details.headline, "Some colors can’t be shown accurately on this screen");
    assert_eq!(details.show_clipped, Some(false));
    let change = s.dispatch(UiAction::ShowClippedColors { visible: true }).unwrap();
    assert!(change.regions & regions::HOST != 0 && change.canvas_wake);
    assert!(s.state.screen.show_clipped);
    assert_eq!(s.screen_details().unwrap().show_clipped, Some(true));
    s.set_screen_clipped(Some(false));
    assert_eq!(s.screen_details(), None);
    assert!(s.state.screen.show_clipped, "highlighting resumes if colors clip again");
    s.dispatch(UiAction::ShowClippedColors { visible: false }).unwrap();
    assert!(!s.state.screen.show_clipped);
}

#[test]
fn a_new_drawing_in_the_window_keeps_the_screen() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    s.set_screen_report(screen_report(gnome("default")));
    s.set_screen_clipped(Some(true));
    s.dispatch(UiAction::ShowClippedColors { visible: true }).unwrap();
    let mut next = screen_session(RgbSpace::Srgb, SampleDepth::U8);
    next.inherit_window_state(&s).unwrap();
    assert_eq!(next.state.screen.report, s.state.screen.report);
    assert_eq!(next.state.screen.assessment, s.state.screen.assessment);
    assert!(next.state.screen.show_clipped);
    assert_eq!(next.state.screen.clipped, None, "the new drawing is checked again");
}
