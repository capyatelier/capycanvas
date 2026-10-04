use super::*;
use layer_color::screen::{self, Basis, ScreenAssessment, ScreenReport};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScreenState {
    pub report: ScreenReport,
    pub assessment: ScreenAssessment,
    pub clipped: Option<bool>,
    pub show_clipped: bool,
    pub chip: Option<ScreenChip>,
    pub details: Option<ScreenDetails>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScreenChip {
    pub label: std::sync::Arc<str>,
    pub warning: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScreenDetails {
    pub title: String,
    pub headline: std::sync::Arc<str>,
    pub body: Option<std::sync::Arc<str>>,
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

    pub(crate) fn refresh_screen_view(&mut self) -> u32 {
        let (chip, details) = (self.screen_chip(), self.screen_details());
        if self.state.screen.chip == chip && self.state.screen.details == details {
            return 0;
        }
        self.state.screen.chip = chip;
        self.state.screen.details = details;
        regions::HOST
    }

    pub(crate) fn show_clipped_colors(&mut self, visible: bool) -> u32 {
        if std::mem::replace(&mut self.state.screen.show_clipped, visible) == visible { 0 } else { regions::HOST }
    }

    fn screen_proofing(&self) -> bool {
        self.state.soft_proof || self.state.gamut_warning
    }

    fn screen_hdr_document(&self) -> bool {
        self.engine.document().composition().color.depth.is_float()
    }

    fn hdr_view_label(&self) -> MessageId {
        match (self.state.hdr_display_available, self.state.preview_sdr) {
            (true, true) => MessageId::COMMON_SCREEN_CHIP_SDR_PREVIEW,
            (true, false) => MessageId::COMMON_SCREEN_CHIP_HDR,
            (false, _) => MessageId::COMMON_SCREEN_CHIP_SHOWING_SDR,
        }
    }

    pub fn screen_chip(&self) -> Option<ScreenChip> {
        if !self.state.workspace.layout.canvas_info.visible {
            return None;
        }
        let screen = &self.state.screen;
        if screen.clipped == Some(true) {
            return Some(ScreenChip { label: self.localization().text(MessageId::COMMON_SCREEN_CHIP_CLIPPED), warning: true });
        }
        if self.screen_hdr_document() && !self.screen_proofing() {
            return Some(ScreenChip { label: self.localization().text(self.hdr_view_label()), warning: false });
        }
        (self.screen_proofing() && proof_caveat(&screen.assessment).is_some())
            .then(|| ScreenChip { label: self.localization().text(MessageId::COMMON_SCREEN_CHIP_PROOF_CAVEAT), warning: false })
    }

    pub fn screen_details(&self) -> Option<ScreenDetails> {
        let screen = &self.state.screen;
        let assessment = &screen.assessment;
        let proofing = self.screen_proofing();
        let clipped = screen.clipped == Some(true);
        let (headline, body) = if clipped {
            (MessageId::COMMON_SCREEN_HEADLINE_CLIPPED, clipped_reason(assessment).map(|id| self.localization().text(id)))
        } else if self.screen_hdr_document() && !proofing {
            self.hdr_view(assessment, screen.report.hdr_capable)
        } else if proofing && let Some(caveat) = proof_caveat(assessment) {
            (MessageId::COMMON_SCREEN_HEADLINE_PROOF, Some(self.localization().text(caveat)))
        } else {
            return None;
        };
        Some(ScreenDetails {
            title: screen.report.name.clone().unwrap_or_else(|| self.localization().text(MessageId::COMMON_SCREEN_TITLE).to_string()),
            headline: self.localization().text(headline),
            body,
            warning: clipped,
            show_clipped: (clipped || screen.show_clipped).then_some(screen.show_clipped),
        })
    }

    fn hdr_view(&self, assessment: &ScreenAssessment, hdr_capable: Option<bool>) -> (MessageId, Option<std::sync::Arc<str>>) {
        let l = self.localization();
        if self.state.hdr_display_available && self.state.preview_sdr {
            return (MessageId::COMMON_SCREEN_HEADLINE_SDR, Some(l.text(MessageId::COMMON_SCREEN_SDR_PREVIEW)));
        }
        if self.state.hdr_display_available {
            let body = if assessment.peak.is_some() {
                self.hdr_headroom_body(assessment.headroom())
            } else {
                l.text(MessageId::COMMON_SCREEN_HDR_UNKNOWN_PEAK)
            };
            return (MessageId::COMMON_SCREEN_HEADLINE_HDR, Some(body));
        }
        let body = if assessment.white_at_peak() {
            Some(MessageId::COMMON_SCREEN_WHITE_AT_PEAK)
        } else if hdr_capable == Some(false) {
            Some(MessageId::COMMON_SCREEN_NO_HDR)
        } else if hdr_capable == Some(true) && !assessment.hdr_signal {
            Some(MessageId::COMMON_SCREEN_HDR_OFF)
        } else {
            None
        };
        (MessageId::COMMON_SCREEN_HEADLINE_SDR, body.map(|id| l.text(id)))
    }

    fn hdr_headroom_body(&self, headroom: f32) -> std::sync::Arc<str> {
        let l = self.localization();
        let key = (l.language(), headroom.to_bits());
        let mut cached = self.screen_headroom.borrow_mut();
        if let Some((language, bits, body)) = cached.as_ref()
            && (*language, *bits) == key
        {
            return body.clone();
        }
        let mut args = FluentArgs::new();
        args.set("times", times(headroom));
        args.set("ev", format!("{:+.1}", headroom.log2()));
        let body: std::sync::Arc<str> = l.format(MessageId::COMMON_SCREEN_HDR_HEADROOM, &args).into();
        *cached = Some((key.0, key.1, body.clone()));
        body
    }
}

fn times(value: f32) -> String {
    let tenths = (value * 10.).round();
    if tenths % 10. == 0. { format!("{:.0}", tenths / 10.) } else { format!("{:.1}", tenths / 10.) }
}

fn clipped_reason(assessment: &ScreenAssessment) -> Option<MessageId> {
    if assessment.wide_color_off {
        Some(MessageId::COMMON_SCREEN_WIDE_COLOR_OFF)
    } else if assessment.srgb_on_wide_monitor {
        Some(MessageId::COMMON_SCREEN_SRGB_WIDE_MONITOR)
    } else if assessment.basis == Basis::Unmanaged {
        Some(MessageId::COMMON_SCREEN_UNMANAGED)
    } else {
        None
    }
}

fn proof_caveat(assessment: &ScreenAssessment) -> Option<MessageId> {
    let caveat = match assessment.basis {
        Basis::Monitor | Basis::Unknown if assessment.hdr_signal => MessageId::COMMON_SCREEN_PROOF_HDR,
        Basis::Unknown => MessageId::COMMON_SCREEN_PROOF_UNKNOWN,
        Basis::System if assessment.white_at_peak() => MessageId::COMMON_SCREEN_PROOF_WHITE,
        Basis::Monitor | Basis::System | Basis::Unmanaged | Basis::Pending => return None,
    };
    Some(caveat)
}
