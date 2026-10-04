fn screen_report(color: layer_color::screen::ScreenColor) -> layer_color::screen::ScreenReport {
    let monitor = layer_color::screen::edid::parse(include_bytes!("../../layer-color/tests/fixtures/edid/cintiq-pro-27.bin")).unwrap();
    layer_color::screen::ScreenReport {
        name: Some("Cintiq Pro 27".into()),
        color,
        hdr_capable: Some(monitor.pq_signal),
        monitor: Some(monitor),
        wide_color_off: false,
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
    let mut document = Document::new(layer_core::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.space = space;
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = depth;
    UiSession::new(Recorder { color: document.composition().color, ..Default::default() }, document, [32, 32], Platform::Gtk).unwrap()
}

fn chip(s: &UiSession<Recorder>) -> Option<(String, bool)> {
    s.screen_chip().map(|c| (c.label.to_string(), c.warning))
}

#[test]
fn screen_language_refresh_preserves_report_and_cached_headroom() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::Srgb, SampleDepth::F16);
    let mut report = screen_report(gnome("native"));
    report.name = Some("日本語 { $screen } 🎨".into());
    if let layer_color::screen::ScreenColor::Described(d) = &mut report.color {
        d.transfer = layer_color::screen::Transfer::Pq;
        d.reference_white = 200.;
        d.target_peak = Some(1000.);
    }
    s.set_screen_report(report.clone());
    s.set_hdr_display_available(true);
    let checkpoint = s.engine.checkpoint();
    for language in UiLanguage::ALL {
        s.set_localization(Localizer::shared(language));
        let first = s.screen_details().unwrap();
        assert_eq!(first.title, report.name.as_deref().unwrap());
        assert_eq!(first.headline, s.localization().text(MessageId::COMMON_SCREEN_HEADLINE_HDR));
        let mut args = FluentArgs::new();
        args.set("times", "5"); args.set("ev", "+2.3");
        assert_eq!(first.body.as_deref(), Some(s.localization().format(MessageId::COMMON_SCREEN_HDR_HEADROOM, &args).as_str()));
        let repeated = s.screen_details().unwrap();
        assert!(std::sync::Arc::ptr_eq(first.body.as_ref().unwrap(), repeated.body.as_ref().unwrap()));
        assert_eq!(s.state.screen.details, Some(first));
        assert_eq!(s.state.screen.report, report);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
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
    assert_eq!(chip(&s), Some(("Colors clipped".into(), true)));
    assert_eq!(s.state.screen.chip.as_ref().map(|c| c.label.as_ref()), Some("Colors clipped"), "hosts read the chip from state");
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
    assert_eq!(chip(&s), Some(("May not match print".into(), false)));
    s.set_screen_clipped(Some(true));
    assert_eq!(chip(&s), Some(("Colors clipped".into(), true)));
}

#[test]
fn hdr_drawings_keep_their_presentation_label_until_proofed() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::Srgb, SampleDepth::F16);
    s.set_screen_report(screen_report(gnome("default")));
    assert_eq!(chip(&s), Some(("Showing SDR".into(), false)));
    let details = s.screen_details().unwrap();
    assert_eq!(details.headline.as_ref(), "Showing the SDR version");
    assert_eq!(
        details.body.as_deref(),
        Some("HDR is off for this screen. Turn it on in your operating system’s display settings to see HDR highlights.")
    );
    s.set_screen_report(screen_report(gnome("hdr")));
    assert_eq!(chip(&s), Some(("Showing SDR".into(), false)));
    assert_eq!(
        s.screen_details().unwrap().body.as_deref(),
        Some("At your current screen brightness, regular content already uses all of this screen’s brightness, leaving nothing brighter for HDR highlights. Lower the screen brightness to see them.")
    );
    let mut television = screen_report(gnome("hdr"));
    television.monitor = Some(layer_color::screen::edid::parse(include_bytes!("../../layer-color/tests/fixtures/edid/lg-tv.bin")).unwrap());
    s.set_screen_report(television);
    s.set_hdr_display_available(true);
    assert_eq!(chip(&s), Some(("HDR".into(), false)));
    assert_eq!(s.screen_details().unwrap().headline.as_ref(), "Showing HDR");
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
    assert_eq!(chip(&s), Some(("SDR preview".into(), false)));
    s.state.preview_sdr = false;
    s.state.gamut_warning = true;
    assert_eq!(chip(&s), Some(("May not match print".into(), false)));
}

#[test]
fn details_explain_the_screen_briefly() {
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    s.state.soft_proof = true;
    s.set_screen_report(screen_report(gnome("hdr")));
    let details = s.screen_details().unwrap();
    assert_eq!(details.title, "Cintiq Pro 27");
    assert_eq!(details.headline.as_ref(), "The proof may not match the print");
    assert_eq!(
        details.body.as_deref(),
        Some("With HDR on, Capy Canvas can’t tell how this screen shows colors. Turn off HDR for this screen in your operating system’s display settings.")
    );
    assert_eq!(details.show_clipped, None);

    s.set_screen_report(screen_report(gnome("default")));
    assert_eq!(chip(&s), None, "sRGB sent unconverted is the convention, even to a wide monitor");
    assert_eq!(s.screen_details(), None);
    s.set_screen_report(layer_color::screen::ScreenReport { color: layer_color::screen::ScreenColor::Unmanaged, ..screen_report(gnome("default")) });
    assert_eq!(chip(&s), None, "an unmanaged screen shows sRGB unconverted too");

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
    assert_eq!(details.headline.as_ref(), "Some colors can’t be shown accurately on this screen");
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

#[test]
fn clipping_explains_a_color_setting_that_limits_apps_to_srgb() {
    use layer_color::screen::{Chromaticities, ScreenReport};
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = screen_session(RgbSpace::DisplayP3, SampleDepth::U8);
    let srgb = Chromaticities::of(RgbSpace::Srgb);
    s.set_screen_report(ScreenReport { wide_color_off: true, ..ScreenReport::managed(Some("Built-in Screen".into()), srgb, false, None, Some(true)) });
    assert_eq!(chip(&s), None, "sRGB-range colors follow the sRGB convention");
    s.state.soft_proof = true;
    assert_eq!(chip(&s), None);
    s.set_screen_clipped(Some(true));
    assert_eq!(chip(&s), Some(("Colors clipped".into(), true)));
    assert_eq!(
        s.screen_details().unwrap().body.as_deref(),
        Some("Your operating system is limiting apps to sRGB colors on this screen, although the screen can show more. Turn off saturated or vivid colors in your operating system’s display settings to show them.")
    );
}
