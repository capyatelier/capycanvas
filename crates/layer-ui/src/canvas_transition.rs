use super::*;

#[derive(Default)]
pub(super) struct CanvasTransition {
    drawing: FadeEdge,
    navigator: FadeEdge,
}
#[derive(Default)]
struct FadeEdge {
    showing: bool,
    started: Option<u64>,
}
impl FadeEdge {
    fn advance(&mut self, showing: bool, now_ns: u64) -> bool {
        if !showing {
            let changed = self.showing;
            if changed { *self = Default::default(); }
            return changed;
        }
        if !self.showing {
            self.showing = true;
            self.started = Some(now_ns);
        }
        let Some(started) = self.started else { return false; };
        if now_ns.saturating_sub(started) >= 150_000_000 { self.started = None; }
        true
    }
    fn alpha(&self, now_ns: u64) -> Option<f32> {
        if !self.showing { return Some(1.); }
        self.started.map(|started| {
            let progress = now_ns.saturating_sub(started) as f32 / 150_000_000.;
            (1. - progress).powi(3)
        })
    }
}
impl CanvasTransition {
    pub(super) fn running(&self) -> bool { self.drawing.started.is_some() || self.navigator.started.is_some() }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn begin_canvas_transition(&mut self) -> UiChange {
        self.canvas_transition = Default::default();
        self.engine.set_canvas_transition(layer_render::CanvasFade::WAITING);
        UiChange { revision: self.state.revision, presentation_wake: true, ..Default::default() }
    }

    pub(super) fn advance_canvas_transition(&mut self, now_ns: u64) -> bool {
        if !self.canvas_transition.drawing.showing && self.engine.showing_drawing() {self.engine.document().memory().presented();}
        let drawing = self.canvas_transition.drawing.advance(self.engine.showing_drawing(), now_ns);
        let navigator = self.canvas_transition.navigator.advance(self.engine.showing_navigator(), now_ns);
        if drawing || navigator {
            self.engine.set_canvas_transition(layer_render::CanvasFade {
                drawing: self.canvas_transition.drawing.alpha(now_ns),
                navigator: self.canvas_transition.navigator.alpha(now_ns),
            });
        }
        drawing || navigator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::*;

    #[test]
    fn navigator_transition_starts_its_own_150_ms_clock() {
        let mut s = session(Platform::Gtk);
        s.renderer_mut().showing_drawing = true;
        s.frame(1_000_000_000, 1_000_000_000).unwrap();
        assert_eq!(s.renderer_mut().canvas_transition, layer_render::CanvasFade::WAITING);
        s.renderer_mut().showing_navigator = true;
        s.frame(1_050_000_000, 1_050_000_000).unwrap();
        let fade = s.renderer_mut().canvas_transition;
        assert!((fade.drawing.unwrap() - 8. / 27.).abs() < 0.000_001);
        assert_eq!(fade.navigator, Some(1.));
        s.frame(1_150_000_000, 1_150_000_000).unwrap();
        let fade = s.renderer_mut().canvas_transition;
        assert_eq!(fade.drawing, None);
        assert!((fade.navigator.unwrap() - 1. / 27.).abs() < 0.000_001);
        assert!(s.frame_demand() != layer_render::FrameDemand::Wait);
        s.frame(1_200_000_000, 1_200_000_000).unwrap();
        assert_eq!(s.renderer_mut().canvas_transition, layer_render::CanvasFade::default());
        assert!(s.frame_demand() == layer_render::FrameDemand::Wait);
    }

    #[test]
    fn waiting_canvas_transition_starts_at_showing_and_finishes_after_150_ms() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.frame(0, 0).unwrap();
            assert!(s.frame_demand() == layer_render::FrameDemand::Wait);
            assert!(s.begin_canvas_transition().presentation_wake);
            let waiting_updates = s.renderer_mut().canvas_transition_updates;
            let waiting = s.frame(1_000_000_000, 1_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(1.));
            assert_eq!(s.renderer_mut().canvas_transition_updates, waiting_updates);
            assert!(s.frame_demand() == layer_render::FrameDemand::Wait);
            assert!(!waiting.needs_present());

            s.renderer_mut().showing_drawing = true;
            let start = s.frame(2_000_000_000, 2_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(1.));
            assert!(start.presentation_wake && s.frame_demand() != layer_render::FrameDemand::Wait);
            let middle = s.frame(2_075_000_000, 2_075_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(0.125));
            assert!(middle.presentation_wake && s.frame_demand() != layer_render::FrameDemand::Wait);
            let end = s.frame(2_150_000_000, 2_150_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, None);
            assert!(end.presentation_wake);
            assert!(s.frame_demand() == layer_render::FrameDemand::Wait);
            assert!(!s.frame(2_151_000_000, 2_151_000_000).unwrap().needs_present());
        }
    }

    #[test]
    fn canvas_transition_keeps_the_clock_during_pen_and_camera_input() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.renderer_mut().showing_drawing = true;
            s.frame(0, 0).unwrap();
            s.dispatch(UiAction::SetZoom { zoom: 1.1 }).unwrap();
            assert_eq!(s.camera().zoom, 1.1);
            pen_at(&mut s, 1, PenPhase::Down, [100., 100.]);
            s.frame(25_000_000, 25_000_000).unwrap();
            pen_at(&mut s, 2, PenPhase::Move, [200., 200.]);
            s.frame(50_000_000, 50_000_000).unwrap();
            pen_at(&mut s, 3, PenPhase::Up, [200., 200.]);
            s.frame(75_000_000, 75_000_000).unwrap();
            assert!(s.renderer_mut().dabs > 0);
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(0.125));
            s.frame(100_000_000, 100_000_000).unwrap();
            assert!((s.renderer_mut().canvas_transition.drawing.unwrap() - 1. / 27.).abs() < 0.000_001);
            s.frame(150_000_000, 150_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, None);
        }
    }

    #[test]
    fn canvas_activation_and_renderer_waiting_rearm_the_transition() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.dispatch(UiAction::SetZoom { zoom: 2. }).unwrap();
            s.frame(1_000_000_000, 1_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(1.));
            assert!(s.canvas_transition.drawing.started.is_none());
            s.renderer_mut().showing_drawing = true;
            s.frame(2_000_000_000, 2_000_000_000).unwrap();
            s.frame(2_150_000_000, 2_150_000_000).unwrap();
            s.inherit_window_state(&session(platform)).unwrap();
            s.frame(3_000_000_000, 3_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(1.));
            assert!(s.frame_demand() != layer_render::FrameDemand::Wait);
            s.renderer_mut().showing_drawing = false;
            let showing_updates = s.renderer_mut().canvas_transition_updates;
            s.frame(3_025_000_000, 3_025_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(1.));
            assert_eq!(s.renderer_mut().canvas_transition_updates, showing_updates + 1);
            assert!(s.canvas_transition.drawing.started.is_none());
            assert!(s.frame_demand() == layer_render::FrameDemand::Wait);
            s.frame(3_050_000_000, 3_050_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition_updates, showing_updates + 1);
            s.renderer_mut().showing_drawing = true;
            s.frame(4_000_000_000, 4_000_000_000).unwrap();
            s.frame(4_075_000_000, 4_075_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition.drawing, Some(0.125));
        }
    }
}
