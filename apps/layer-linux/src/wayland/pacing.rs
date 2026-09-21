//! Learn a stroke submission margin from actual presentation, without extra frames.

#[derive(Clone, Copy, Debug)]
pub(crate) struct StrokeTarget {
    pub presentation_ns: u64,
    pub period_ns: u64,
    pub lead_ns: u64,
}

#[derive(Default)]
pub(super) struct StrokePacer {
    period: u64,
    lead: u64,
    floor: u64,
    good: u32,
    misses: u8,
    last_target: u64,
}

impl StrokePacer {
    /// Feedback may arrive after the policy changed. Only evaluate frames
    /// actually submitted with the current margin. Large desktop pauses don't
    /// teach us the normal compositor latch deadline.
    pub fn observe(&mut self, sample: StrokeTarget, at: u64, refresh: u64) -> Option<u64> {
        if refresh == 0 || sample.period_ns != refresh {
            return None;
        }
        if self.period != refresh
            || sample.presentation_ns.saturating_sub(self.last_target) > 2_000_000_000
        {
            self.period = refresh;
            self.lead = refresh * 3 / 4;
            self.floor = refresh / 4;
            self.good = 0;
            self.misses = 0;
            self.last_target = 0;
        }
        if sample.presentation_ns <= self.last_target {
            return Some(self.lead);
        }
        self.last_target = sample.presentation_ns;
        if sample.lead_ns != self.lead {
            return Some(self.lead);
        }
        let late = at.saturating_sub(sample.presentation_ns);
        let drift = at.abs_diff(sample.presentation_ns) % refresh;
        if late > refresh * 2
            || at.saturating_add(refresh / 2) < sample.presentation_ns
            || drift.min(refresh - drift) > refresh / 8
        {
            self.good = 0;
            self.misses = 0;
            return Some(self.lead);
        }
        let step = (refresh / 32).max(1);
        self.misses = (self.misses << 1) | u8::from(late > refresh / 2);
        if self.misses.count_ones() >= 2 {
            // A single scheduling outlier does not establish the cutoff. Two
            // misses in eight frames do. Never grow past the original margin.
            self.lead = (self.lead + 2 * step).min(refresh * 3 / 4);
            // Remember the failed probe. Do not keep probing past that limit
            // and introducing another dropped refresh every few dozen frames.
            self.floor = self.lead;
            self.good = 0;
            self.misses = 0;
        } else {
            self.good += u32::from(late <= refresh / 2);
            if self.good >= 32 {
                self.lead = self.lead.saturating_sub(step).max(self.floor);
                self.good = 0;
                self.misses = 0;
            }
        }
        Some(self.lead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PERIOD: u64 = 8_000_000;

    fn feedback(p: &mut StrokePacer, frame: u64, lead: u64, late: u64) -> u64 {
        let target = frame * PERIOD;
        p.observe(
            StrokeTarget {
                presentation_ns: target,
                period_ns: PERIOD,
                lead_ns: lead,
            },
            target + late,
            PERIOD,
        )
        .unwrap()
    }

    #[test]
    fn learns_later_margin_and_remembers_failed_probe() {
        let mut p = StrokePacer::default();
        let mut lead = 6_000_000;
        for frame in 1..=256 {
            lead = feedback(&mut p, frame, lead, 0);
        }
        assert_eq!(lead, 4_000_000);
        lead = feedback(&mut p, 257, lead, PERIOD);
        assert_eq!(lead, 4_000_000);
        lead = feedback(&mut p, 258, lead, PERIOD);
        assert_eq!(lead, 4_500_000);
        for frame in 259..=600 {
            lead = feedback(&mut p, frame, lead, 0);
        }
        assert_eq!(lead, 4_500_000, "do not repeatedly probe a failed deadline");
    }

    #[test]
    fn old_feedback_and_long_desktop_stall_do_not_change_margin() {
        let mut p = StrokePacer::default();
        let mut lead = 6_000_000;
        for frame in 1..=32 {
            lead = feedback(&mut p, frame, lead, 0);
        }
        assert_eq!(lead, 5_750_000);
        assert_eq!(feedback(&mut p, 33, 6_000_000, PERIOD), lead);
        assert_eq!(feedback(&mut p, 34, lead, 100_000_000), lead);
        assert_eq!(feedback(&mut p, 32, lead, PERIOD), lead);
    }

    #[test]
    fn sustained_load_backs_off_and_idle_resets_training() {
        let mut p = StrokePacer::default();
        let mut lead = 6_000_000;
        for frame in 1..=20 {
            lead = feedback(&mut p, frame, lead, PERIOD);
        }
        assert_eq!(lead, PERIOD * 3 / 4);
        assert_eq!(feedback(&mut p, 400, lead, 0), PERIOD * 3 / 4);
    }

    #[test]
    fn monitor_change_ignores_old_period_then_retrains() {
        let mut p = StrokePacer::default();
        assert_eq!(
            p.observe(
                StrokeTarget {
                    presentation_ns: 8_000_000,
                    period_ns: PERIOD,
                    lead_ns: 6_000_000
                },
                8_000_000,
                PERIOD * 2
            ),
            None
        );
        assert_eq!(
            p.observe(
                StrokeTarget {
                    presentation_ns: 16_000_000,
                    period_ns: PERIOD * 2,
                    lead_ns: 12_000_000
                },
                16_000_000,
                PERIOD * 2
            ),
            Some(12_000_000)
        );
    }

    #[test]
    fn isolated_misses_do_not_pin_the_policy_to_an_early_deadline() {
        let mut p = StrokePacer::default();
        let mut lead = 6_000_000;
        for frame in 1..=64 {
            lead = feedback(
                &mut p,
                frame,
                lead,
                if frame % 16 == 0 { PERIOD } else { 0 },
            );
        }
        assert!(lead < 6_000_000);
        assert_eq!(p.floor, PERIOD / 4);
    }

    #[test]
    fn a_display_phase_jump_is_not_a_missed_refresh() {
        let mut p = StrokePacer::default();
        let mut lead = 6_000_000;
        for frame in 1..=32 {
            lead = feedback(&mut p, frame, lead, 0);
        }
        for frame in 33..=40 {
            assert_eq!(feedback(&mut p, frame, lead, 5_000_000), lead);
        }
        assert_eq!(p.floor, PERIOD / 4);
    }
}
