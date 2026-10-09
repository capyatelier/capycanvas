//! Admission policy shared by the native worker and browser task runner.
//! Input can postpone optional jobs, but never dependencies needed to become
//! drawable. A running driver call cannot be interrupted.
use std::time::Duration;
use web_time::Instant;

const QUIET: Duration = Duration::from_millis(200);
const BACKGROUND_QUIET: Duration = Duration::from_secs(1);

pub(super) struct Admission {
    pub idle: bool,
    pub speculative_idle: bool,
    last_activity: Option<Instant>,
}
impl Default for Admission {
    fn default() -> Self { Self { idle: true, speculative_idle: true, last_activity: None } }
}
impl Admission {
    pub fn input(&mut self) {
        self.last_activity = Some(Instant::now());
    }
    pub fn finished(&mut self, priority: u8) {
        if priority < super::OTHER { self.input(); }
    }
    fn remaining(&self, priority: u8) -> Duration {
        let quiet = if priority == super::OTHER { QUIET } else { BACKGROUND_QUIET };
        self.last_activity.map_or(Duration::ZERO, |activity| (activity + quiet).saturating_duration_since(Instant::now()))
    }
    pub fn delay(&self, priorities: impl Iterator<Item = u8>) -> Duration {
        priorities.filter(|&p| p >= super::OTHER).map(|p| self.remaining(p)).min()
            .unwrap_or_else(|| self.remaining(super::BACKGROUND))
    }
    pub fn allows(&self, priority: u8) -> bool {
        priority < super::OTHER || (self.idle && (priority == super::OTHER || self.speculative_idle)
            && self.remaining(priority).is_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_and_held_gestures_only_defer_optional_jobs() {
        let mut admission = Admission::default();
        assert!(admission.allows(super::super::OTHER));
        admission.input();
        assert!(!admission.allows(super::super::OTHER));
        for priority in 0..super::super::OTHER { assert!(admission.allows(priority)); }
        admission.last_activity = Some(Instant::now() - QUIET);
        admission.idle = false;
        assert!(!admission.allows(super::super::OTHER));
        admission.idle = true;
        assert!(admission.allows(super::super::OTHER));
        admission.input();
        assert!(!admission.allows(super::super::OTHER));
    }

    #[test]
    fn short_pauses_admit_visible_previews_but_keep_background_work_parked() {
        use super::super::{BRUSH, OTHER, BACKGROUND};
        let mut admission = Admission::default();
        admission.last_activity = Some(Instant::now() - Duration::from_millis(500));
        assert!(admission.allows(BRUSH) && admission.allows(OTHER));
        assert!(!admission.allows(BACKGROUND));
        assert_eq!(admission.delay([OTHER, BACKGROUND].into_iter()), Duration::ZERO);
        assert!(!admission.delay([BACKGROUND].into_iter()).is_zero());
        assert!(!admission.delay(std::iter::empty()).is_zero());
        admission.last_activity = Some(Instant::now() - BACKGROUND_QUIET);
        assert!(admission.allows(BACKGROUND));
        admission.speculative_idle = false;
        assert!(admission.allows(OTHER));
        assert!(!admission.allows(BACKGROUND));
        assert!(admission.allows(BRUSH));
        admission.idle = false;
        assert!(!admission.allows(BACKGROUND));
        assert!(admission.allows(BRUSH));
    }
}
