use super::Bounds;
use serde::{Deserialize, Serialize};

/// Where the canvas notice sits, in workspace units: hosts centre the bubble's
/// bottom edge on `x`, `y`, keep it within `max_width`, and give the actions
/// their own row under the text when the text would be narrower than
/// `min_text_width` beside them. That is the whole `max_width`, so the text
/// never wraps beside the actions: it shares their row only on one line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NoticePlacement {
    pub x: f32,
    pub y: f32,
    pub max_width: f32,
    pub min_text_width: f32,
    /// The canvas area is too small for the bubble, so it sits at the
    /// window's bottom over the panels.
    pub window: bool,
    /// A canvas bar reaching below this line lifts the bubble above the bar.
    pub bar_floor: f32,
}

impl NoticePlacement {
    pub const MARGIN: f32 = 12.;
    const MAX_WIDTH: f32 = 960.;
    const MIN_WIDTH: f32 = 320.;
    const BAR_REACH: f32 = 72.;

    pub fn resolve(viewport: [f32; 2], work_area: Bounds, status: Bounds) -> Self {
        let floor = (work_area.y + work_area.height).min(if status.height > 0. { status.y } else { f32::INFINITY });
        let room = work_area.width - 2. * Self::MARGIN;
        let window = room < Self::MIN_WIDTH || floor - work_area.y < Self::MIN_WIDTH * 0.5;
        let (x, y, room) = if window {
            (viewport[0] * 0.5, viewport[1] - Self::MARGIN, viewport[0] - 2. * Self::MARGIN)
        } else {
            (work_area.x + work_area.width * 0.5, floor - Self::MARGIN, room)
        };
        let max_width = room.clamp(0., Self::MAX_WIDTH);
        Self { x, y, max_width, min_text_width: max_width, window, bar_floor: if window { viewport[1] } else { floor - Self::BAR_REACH } }
    }

    /// The anchor above a canvas bar placed along the canvas bottom.
    pub fn above(self, bar: Option<Bounds>) -> Self {
        match bar {
            Some(bar) if bar.y + bar.height > self.bar_floor => Self { y: self.y.min(bar.y - Self::MARGIN), ..self },
            _ => self,
        }
    }

    /// Whether actions `actions` wide keep the text's row in a bubble whose
    /// content is `content` wide with `gap` between text and actions.
    pub fn actions_beside(&self, text: f32, content: f32, actions: f32, gap: f32) -> bool {
        actions == 0. || text + gap + actions <= content || content - actions - gap >= self.min_text_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DockLayout;

    fn area(x: f32, width: f32) -> Bounds { Bounds { x, y: 60., width, height: 800. } }

    #[test]
    fn notices_stay_inside_the_canvas_area_and_move_to_the_window_bottom_when_it_is_too_small() {
        let status = Bounds { x: 300., y: 836., width: 1000., height: 24. };
        let wide = NoticePlacement::resolve([1600., 900.], area(300., 1000.), status);
        assert_eq!((wide.x, wide.y, wide.max_width, wide.window), (800., 824., 960., false));
        let snug = NoticePlacement::resolve([900., 900.], area(250., 400.), Bounds::default());
        assert_eq!((snug.x, snug.y, snug.max_width, snug.window), (450., 848., 376., false), "the bubble keeps to the canvas area");
        let squeezed = NoticePlacement::resolve([1100., 900.], area(450., 150.), Bounds::default());
        assert_eq!((squeezed.x, squeezed.y, squeezed.max_width, squeezed.window), (550., 888., 960., true), "a sliver of canvas sends the bubble to the window bottom");
        let phone = NoticePlacement::resolve([400., 900.], area(0., 30.), Bounds::default());
        assert_eq!((phone.x, phone.max_width, phone.window), (200., 376., true));
        assert!(phone.min_text_width <= phone.max_width);
    }

    #[test]
    fn text_that_needs_a_second_line_puts_the_actions_on_their_own_row() {
        for placement in [NoticePlacement::resolve([1600., 900.], area(300., 1000.), Bounds::default()), NoticePlacement::resolve([400., 900.], area(0., 30.), Bounds::default())] {
            assert_eq!(placement.min_text_width, placement.max_width);
            assert!(!placement.actions_beside(placement.max_width, placement.max_width, 120., 2.), "text never wraps beside the actions");
        }
    }

    #[test]
    fn intrinsic_notice_text_shares_the_exact_available_row() {
        for (content, expected) in [(301., false), (302., true), (303., true)] {
            let placement = NoticePlacement { min_text_width: content, ..NoticePlacement::default() };
            let text = 220.; let actions = 80.; let gap = 2.;
            assert_eq!(placement.actions_beside(text, content, actions, gap), expected);
            let request = serde_json::from_value(serde_json::json!({"type":"notice_layout","text":text,"content":content,"actions":actions,"gap":gap,"min_text_width":content})).unwrap();
            let reply = crate::toolbar_ui(request, &crate::Localizer::shared(crate::UiLanguage::English)).unwrap();
            assert_eq!(reply["beside"], expected);
        }
        let placement = NoticePlacement { min_text_width: 100., ..NoticePlacement::default() };
        assert!(placement.actions_beside(1000., 182., 80., 2.));
        assert!(!placement.actions_beside(1000., 181., 80., 2.));
        assert!(placement.actions_beside(1000., 0., 0., 2.));
    }

    #[test]
    fn a_canvas_bar_along_the_bottom_lifts_the_notice_above_it() {
        let placement = NoticePlacement::resolve([1600., 900.], area(300., 1000.), Bounds::default());
        let bar = Bounds { x: 600., y: 800., width: 400., height: 44. };
        assert_eq!(placement.above(Some(bar)).y, 788.);
        assert_eq!(placement.above(Some(Bounds { y: 200., ..bar })).y, placement.y, "a bar near the top leaves the notice");
        assert_eq!(placement.above(None).y, placement.y);
        let returned = Bounds { y: 760., ..bar };
        assert_eq!(placement.above(Some(returned)).y, 748., "a returning bar uses its current bounds");
    }

    #[test]
    fn every_viewport_publishes_a_notice_that_fits_the_window() {
        for viewport in [[1600., 1000.], [1440., 900.], [1280., 900.], [640., 900.], [1200., 1920.]] {
            let layout = DockLayout::default().resolved(viewport);
            let notice = layout.notice;
            assert!(notice.max_width > 0. && notice.max_width <= viewport[0] - 2. * NoticePlacement::MARGIN, "{viewport:?} {notice:?}");
            assert!(notice.x - notice.max_width * 0.5 >= 0. && notice.x + notice.max_width * 0.5 <= viewport[0], "{viewport:?} {notice:?}");
            assert!(notice.y <= viewport[1], "{viewport:?} {notice:?}");
            assert!(notice.window || notice.max_width <= layout.work_area.width - 2. * NoticePlacement::MARGIN, "{viewport:?} {notice:?}");
        }
    }
}
