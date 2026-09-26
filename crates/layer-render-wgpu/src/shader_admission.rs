//! Admission policy shared by the native worker and browser task runner.
//! Input can postpone optional jobs, but never dependencies needed to become
//! drawable. A running driver call cannot be interrupted.
use std::time::Duration;
use web_time::Instant;

const QUIET: Duration = Duration::from_millis(200);

pub(super) struct Admission {
    pub idle: bool,
    quiet_until: Option<Instant>,
}
impl Default for Admission {
    fn default() -> Self { Self { idle: true, quiet_until: None } }
}
impl Admission {
    pub fn input(&mut self) {
        self.quiet_until = Some(Instant::now() + QUIET);
    }
    pub fn delay(&self) -> Duration {
        self.quiet_until.map_or(Duration::ZERO, |until| until.saturating_duration_since(Instant::now()))
    }
    pub fn allows(&self, priority: u8) -> bool {
        priority < super::OTHER || (self.idle && self.delay().is_zero())
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
        admission.quiet_until = Some(Instant::now() - QUIET);
        admission.idle = false;
        assert!(!admission.allows(super::super::OTHER));
        admission.idle = true;
        assert!(admission.allows(super::super::OTHER));
        admission.input();
        assert!(!admission.allows(super::super::OTHER));
    }
}
