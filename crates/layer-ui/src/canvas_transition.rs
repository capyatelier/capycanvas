use super::*;

#[derive(Default)]
pub(super) struct CanvasTransition {
    showing: bool,
    pub(super) started: Option<u64>,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn begin_canvas_transition(&mut self) -> UiChange {
        self.canvas_transition = Default::default();
        self.engine.set_canvas_transition(Some(1.));
        UiChange { revision: self.state.revision, presentation_wake: true, ..Default::default() }
    }

    pub(super) fn advance_canvas_transition(&mut self, now_ns: u64) -> bool {
        if !self.engine.showing_drawing() {
            let changed = self.canvas_transition.showing;
            if changed {
                self.canvas_transition = Default::default();
                self.engine.set_canvas_transition(Some(1.));
            }
            return changed;
        }
        if !self.canvas_transition.showing {
            self.canvas_transition.showing = true;
            self.canvas_transition.started = Some(now_ns);
        }
        let Some(started) = self.canvas_transition.started else { return false; };
        let elapsed = now_ns.saturating_sub(started);
        if elapsed >= 150_000_000 {
            self.canvas_transition.started = None;
            self.engine.set_canvas_transition(None);
        } else {
            let progress = elapsed as f32 / 150_000_000.;
            self.engine.set_canvas_transition(Some((1. - progress).powi(3)));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::*;

    #[test]
    fn waiting_canvas_transition_starts_at_showing_and_finishes_after_150_ms() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.frame(0, 0).unwrap();
            assert!(!s.wants_continuous_frames());
            assert!(s.begin_canvas_transition().presentation_wake);
            let waiting_updates = s.renderer_mut().canvas_transition_updates;
            let waiting = s.frame(1_000_000_000, 1_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(1.));
            assert_eq!(s.renderer_mut().canvas_transition_updates, waiting_updates);
            assert!(!s.wants_continuous_frames());
            assert!(!waiting.needs_present());

            s.renderer_mut().showing_drawing = true;
            let start = s.frame(2_000_000_000, 2_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(1.));
            assert!(start.presentation_wake && s.wants_continuous_frames());
            let middle = s.frame(2_075_000_000, 2_075_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(0.125));
            assert!(middle.presentation_wake && s.wants_continuous_frames());
            let end = s.frame(2_150_000_000, 2_150_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, None);
            assert!(end.presentation_wake);
            assert!(!s.wants_continuous_frames());
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
            assert_eq!(s.renderer_mut().canvas_transition, Some(0.125));
            s.frame(100_000_000, 100_000_000).unwrap();
            assert!((s.renderer_mut().canvas_transition.unwrap() - 1. / 27.).abs() < 0.000_001);
            s.frame(150_000_000, 150_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, None);
        }
    }

    #[test]
    fn canvas_activation_and_renderer_waiting_rearm_the_transition() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.dispatch(UiAction::SetZoom { zoom: 2. }).unwrap();
            s.frame(1_000_000_000, 1_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(1.));
            assert!(s.canvas_transition.started.is_none());
            s.renderer_mut().showing_drawing = true;
            s.frame(2_000_000_000, 2_000_000_000).unwrap();
            s.frame(2_150_000_000, 2_150_000_000).unwrap();
            s.inherit_window_state(&session(platform)).unwrap();
            s.frame(3_000_000_000, 3_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(1.));
            assert!(s.wants_continuous_frames());
            s.renderer_mut().showing_drawing = false;
            let showing_updates = s.renderer_mut().canvas_transition_updates;
            s.frame(3_025_000_000, 3_025_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(1.));
            assert_eq!(s.renderer_mut().canvas_transition_updates, showing_updates + 1);
            assert!(s.canvas_transition.started.is_none());
            assert!(!s.wants_continuous_frames());
            s.frame(3_050_000_000, 3_050_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition_updates, showing_updates + 1);
            s.renderer_mut().showing_drawing = true;
            s.frame(4_000_000_000, 4_000_000_000).unwrap();
            s.frame(4_075_000_000, 4_075_000_000).unwrap();
            assert_eq!(s.renderer_mut().canvas_transition, Some(0.125));
        }
    }
}
