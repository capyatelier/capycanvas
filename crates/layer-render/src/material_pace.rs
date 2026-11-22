#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialWork {
    pub committed: u64,
    pub prediction: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialPace {
    budget: Option<u64>,
}

impl MaterialPace {
    pub fn budget(&self) -> Option<u64> { self.budget }

    pub fn observe(&mut self, work: MaterialWork, gpu_ms: f64, interval_ms: f64) {
        let total = work.committed.saturating_add(work.prediction);
        if total == 0 || !(gpu_ms > 0.) { return; }
        if gpu_ms <= interval_ms && self.budget.is_none_or(|budget| work.committed < budget) {
            self.budget = None;
            return;
        }
        self.budget = Some(((total as f64 / gpu_ms * interval_ms) as u64).saturating_sub(work.prediction).max(work.prediction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL_MS: f64 = 1000. / 60.;

    fn frames(pace: &mut MaterialPace, count: usize, offered: u64, prediction: u64, gpu_ms: impl Fn(u64) -> f64) -> Vec<(u64, f64)> {
        (0..count).map(|_| {
            let committed = pace.budget().map_or(offered, |budget| budget.clamp(1, offered));
            let ms = gpu_ms(committed + prediction);
            pace.observe(MaterialWork { committed, prediction }, ms, INTERVAL_MS);
            (committed, ms)
        }).collect()
    }

    #[test]
    fn a_gpu_that_finishes_each_frame_within_its_interval_never_limits_material_work() {
        let mut pace = MaterialPace::default();
        let admitted = frames(&mut pace, 64, 400 << 20, 16 << 20, |work| 2. + work as f64 / 5e7);
        assert!(admitted.iter().all(|(committed, _)| *committed == 400 << 20) && pace.budget().is_none(), "{admitted:?}");
    }

    #[test]
    fn a_slow_gpu_admits_one_interval_of_work_once_a_frame_runs_late() {
        let mut pace = MaterialPace::default();
        let admitted = frames(&mut pace, 32, 400 << 20, 1 << 18, |work| 3. + work as f64 / 4e4);
        assert!(admitted[0].1 > 1000., "the first frame admits the whole offer: {:?}", admitted[0]);
        assert!(admitted[8..].iter().all(|(committed, ms)| *committed > 0 && *ms <= INTERVAL_MS * 1.01),
            "frames after the first late one stay within their interval: {admitted:?}");
    }

    #[test]
    fn a_gpu_whose_prediction_fills_the_interval_still_admits_as_much_committed_work() {
        let mut pace = MaterialPace::default();
        let admitted = frames(&mut pace, 16, 400 << 20, 1 << 20, |work| work as f64 / 4e4);
        assert!(admitted[1..].iter().all(|(committed, _)| *committed == 1 << 20), "{admitted:?}");
    }

    #[test]
    fn a_limit_lifts_once_a_frame_keeps_its_interval_without_reaching_it() {
        let mut pace = MaterialPace::default();
        frames(&mut pace, 8, 400 << 20, 1 << 18, |work| 3. + work as f64 / 4e4);
        let budget = pace.budget().unwrap();
        pace.observe(MaterialWork { committed: budget / 2, prediction: 1 << 18 }, INTERVAL_MS / 2., INTERVAL_MS);
        assert_eq!(pace.budget(), None);
    }
}
