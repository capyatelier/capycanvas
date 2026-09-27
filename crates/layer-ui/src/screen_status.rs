use super::*;
use layer_color::screen::edid::Edid;
use layer_color::screen::{self, Basis, ScreenAssessment, ScreenReport};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScreenState {
    pub report: ScreenReport,
    pub assessment: ScreenAssessment,
    pub clipped: Option<bool>,
    pub show_clipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ScreenChip {
    pub label: &'static str,
    pub warning: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScreenDetails {
    pub title: String,
    pub headline: &'static str,
    pub body: Option<String>,
    pub warning: bool,
    pub show_clipped: Option<bool>,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_screen_report(&mut self, report: ScreenReport) -> bool {
        if self.state.screen.report == report {
            return false;
        }
        let assessment = screen::assess(&report);
        if assessment.gamut != self.state.screen.assessment.gamut {
            self.state.screen.clipped = None;
        }
        self.state.screen.assessment = assessment;
        self.state.screen.report = report;
        self.changed(regions::HOST, false);
        true
    }

    pub fn set_screen_clipped(&mut self, clipped: Option<bool>) -> bool {
        if self.state.screen.clipped == clipped {
            return false;
        }
        self.state.screen.clipped = clipped;
        self.changed(regions::HOST, false);
        true
    }

    pub(crate) fn show_clipped_colors(&mut self, visible: bool) -> u32 {
        if std::mem::replace(&mut self.state.screen.show_clipped, visible) == visible { 0 } else { regions::HOST }
    }

    fn screen_proofing(&self) -> bool {
        self.state.soft_proof || self.state.gamut_warning
    }

    fn screen_hdr_document(&self) -> bool {
        self.engine.document().color.depth.is_float()
    }

    fn hdr_view_label(&self) -> &'static str {
        match (self.state.hdr_display_available, self.state.preview_sdr) {
            (true, true) => "SDR preview",
            (true, false) => "HDR",
            (false, _) => "Showing SDR",
        }
    }

    pub fn screen_chip(&self) -> Option<ScreenChip> {
        if !self.state.workspace.layout.canvas_info.visible {
            return None;
        }
        let screen = &self.state.screen;
        if screen.clipped == Some(true) {
            return Some(ScreenChip { label: "Colors clipped", warning: true });
        }
        if self.screen_hdr_document() && !self.screen_proofing() {
            return Some(ScreenChip { label: self.hdr_view_label(), warning: false });
        }
        (self.screen_proofing() && proof_caveat(&screen.assessment).is_some())
            .then_some(ScreenChip { label: "May not match print", warning: false })
    }

    pub fn screen_details(&self) -> Option<ScreenDetails> {
        let screen = &self.state.screen;
        let assessment = &screen.assessment;
        let proofing = self.screen_proofing();
        let clipped = screen.clipped == Some(true);
        let (headline, body) = if clipped {
            ("Some colors can’t be shown accurately on this screen", clipped_reason(assessment))
        } else if self.screen_hdr_document() && !proofing {
            self.hdr_view(assessment, screen.report.monitor.as_ref())
        } else if proofing && let Some(caveat) = proof_caveat(assessment) {
            ("The proof may not match the print", Some(caveat))
        } else {
            return None;
        };
        Some(ScreenDetails {
            title: screen.report.name.clone().unwrap_or_else(|| "This screen".into()),
            headline,
            body,
            warning: clipped,
            show_clipped: (clipped || screen.show_clipped).then_some(screen.show_clipped),
        })
    }

    fn hdr_view(&self, assessment: &ScreenAssessment, monitor: Option<&Edid>) -> (&'static str, Option<String>) {
        const SDR: &str = "Showing the SDR version";
        if self.state.hdr_display_available && self.state.preview_sdr {
            return (SDR, Some("This is the SDR version you’ll export. Select Off in the Proof panel to see HDR.".into()));
        }
        if self.state.hdr_display_available {
            let body = if assessment.peak.is_some() {
                let headroom = assessment.headroom();
                format!("This screen can show highlights up to {}× ({:+.1} EV).", times(headroom), headroom.log2())
            } else {
                "Capy Canvas can’t tell how bright this screen can get, so the brightest highlights may look dimmer than they are.".into()
            };
            return ("Showing HDR", Some(body));
        }
        let body = if assessment.white_at_peak() {
            Some("At your current screen brightness, regular content already uses all of this screen’s brightness, leaving nothing brighter for HDR highlights. Lower the screen brightness to see them.".into())
        } else if monitor.is_some_and(|m| !m.pq_signal) {
            Some("This screen can’t show HDR.".into())
        } else if assessment.basis != Basis::Pending && !assessment.hdr_signal {
            Some(format!("HDR is off for this screen. Turn it on in {DISPLAY_SETTINGS} to see HDR highlights."))
        } else {
            None
        };
        (SDR, body)
    }
}

const DISPLAY_SETTINGS: &str = "your operating system’s display settings";
const SRGB_MODE: &str = "Your operating system is sending colors as if this monitor’s color mode were sRGB. Use the buttons on the monitor to set its color mode to sRGB, if it isn’t already.";

fn times(value: f32) -> String {
    let tenths = (value * 10.).round();
    if tenths % 10. == 0. { format!("{:.0}", tenths / 10.) } else { format!("{:.1}", tenths / 10.) }
}

fn clipped_reason(assessment: &ScreenAssessment) -> Option<String> {
    if assessment.srgb_on_wide_monitor {
        Some(format!(
            "Your operating system is treating this monitor as a standard sRGB screen, although the monitor can show more colors. Turn on HDR for this monitor in {DISPLAY_SETTINGS} to show them."
        ))
    } else if assessment.basis == Basis::Unmanaged {
        Some("Your operating system shows only sRGB colors on this screen.".into())
    } else {
        None
    }
}

fn proof_caveat(assessment: &ScreenAssessment) -> Option<String> {
    let caveat = match assessment.basis {
        Basis::System if assessment.srgb_on_wide_monitor => SRGB_MODE.into(),
        Basis::Monitor | Basis::Unknown if assessment.hdr_signal => format!(
            "With HDR on, Capy Canvas can’t tell how this screen shows colors. Turn off HDR for this screen in {DISPLAY_SETTINGS}."
        ),
        Basis::Unknown => "Capy Canvas can’t tell which colors this screen can show.".into(),
        Basis::Unmanaged => SRGB_MODE.into(),
        Basis::System if assessment.white_at_peak() => {
            "At your current screen brightness, the lightest tones look the same. Lower the screen brightness to tell them apart.".into()
        }
        Basis::Monitor | Basis::System | Basis::Pending => return None,
    };
    Some(caveat)
}
