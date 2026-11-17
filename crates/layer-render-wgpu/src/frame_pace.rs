use std::{collections::VecDeque, sync::{Arc, Mutex, OnceLock}, time::Duration};
use web_time::Instant;

pub(crate) const FASTEST_INTERVAL_MS: f64 = 1000. / 120.;
const SLOWEST_INTERVAL_MS: f64 = 1000. / 60.;
const SPACINGS: usize = 32;
pub(crate) const MOVING_TAPS: u64 = 1 << 20;
pub(crate) const IDLE_TAPS: u64 = 8 << 20;
const UNIT: u64 = crate::scene::PASS_TAPS;
const RECOVERY_FRAMES: u64 = 8;

fn milliseconds(duration: Duration) -> f64 { duration.as_secs_f64() * 1000. }

fn busy_ms(earlier_done: Option<Instant>, submitted: Instant, done: Instant) -> f64 {
    milliseconds(done.saturating_duration_since(earlier_done.map_or(submitted, |earlier| earlier.max(submitted))))
}

#[derive(Clone, Copy)]
struct Grant { kind: usize, taps: u64, generation: u64 }
impl Grant {
    fn full(&self) -> bool { self.taps > UNIT }
}

#[derive(Clone, Copy)]
struct Chunk { taps: u64, cap: u64, adapts: bool, generation: u64, floor_ms: Option<[f64; 2]>, resume: u64, full_gpu_ms: Option<f64> }
impl Chunk {
    const fn new(cap: u64, adapts: bool) -> Self { Self { taps: cap, cap, adapts, generation: 0, floor_ms: None, resume: 0, full_gpu_ms: None } }
    fn work_ms(&self) -> f64 { self.full_gpu_ms.unwrap_or(f64::INFINITY) }
    fn observe(&mut self, grant: Grant, ms: [f64; 2], interval_ms: f64) {
        if !grant.full() {
            if self.floor_ms.replace(ms).is_none() { self.taps = self.taps.max(std::mem::take(&mut self.resume)); }
        } else {
            self.full_gpu_ms = Some(ms[1]);
            if self.floor_ms.is_some_and(|floor_ms| (0..2).any(|resource| ms[resource] < floor_ms[resource])) { self.floor_ms = None; }
        }
        if !self.adapts { return; }
        let floor_ms = self.floor_ms.unwrap_or_default();
        if (0..2).all(|resource| ms[resource] <= interval_ms + floor_ms[resource]) { self.taps = (self.taps + (self.cap / RECOVERY_FRAMES).min(self.taps)).min(self.cap); }
        else if grant.generation == self.generation {
            self.generation += 1;
            if self.floor_ms.is_some() { self.taps = (self.taps / 2).max(UNIT); } else { self.resume = std::mem::replace(&mut self.taps, UNIT); }
        }
    }
}

struct Chunks { kinds: [Chunk; 2], in_flight: [usize; 2] }
impl Default for Chunks {
    fn default() -> Self { Self::new([MOVING_TAPS, IDLE_TAPS]) }
}
impl Chunks {
    fn new([moving, idle]: [u64; 2]) -> Self { Self { kinds: [Chunk::new(moving, true), Chunk::new(idle, false)], in_flight: [0; 2] } }
    fn outstanding_ms(&self) -> f64 { self.kinds.iter().zip(self.in_flight).filter(|(_, frames)| *frames > 0).map(|(chunk, frames)| chunk.work_ms() * frames as f64).sum() }
    fn grant(&self, motion: bool, interval_ms: f64) -> Grant {
        let kind = usize::from(!motion);
        let chunk = self.kinds[kind];
        Grant { kind, taps: if self.outstanding_ms() > interval_ms { 0 } else { chunk.taps }, generation: chunk.generation }
    }
    fn submit(&mut self, grant: Grant) -> Grant {
        self.in_flight[grant.kind] += usize::from(grant.full());
        grant
    }
    fn complete(&mut self, grant: Grant, ms: [f64; 2], interval_ms: f64) {
        self.in_flight[grant.kind] -= usize::from(grant.full());
        self.kinds[grant.kind].observe(grant, ms, interval_ms);
    }
}

#[derive(Default)]
pub(crate) struct FramePace {
    chunks: Arc<Mutex<Chunks>>,
    spacings: [VecDeque<f64>; 2],
    starts: [Option<Instant>; 2],
    begun: Option<(Instant, Arc<OnceLock<Instant>>)>,
    grant: Option<Grant>,
}
impl FramePace {
    pub fn start(&mut self, device: &crate::PipelineDevice, queue: &wgpu::Queue) {
        let earlier = Arc::new(OnceLock::new());
        let done = earlier.clone();
        device.on_queue_done(queue, move || { let _ = done.set(Instant::now()); });
        self.grant = None;
        self.begun = Some((Instant::now(), earlier));
    }
    pub fn begin(&mut self, motion: bool) -> u64 {
        let kind = usize::from(!motion);
        if let Some((begun, _)) = self.begun && let Some(previous) = self.starts[kind].replace(begun) { self.space(kind, milliseconds(begun.saturating_duration_since(previous))); }
        let grant = self.chunks.lock().unwrap().grant(motion, self.interval_ms(kind));
        self.grant = Some(grant);
        grant.taps
    }
    pub fn submitted(&mut self, device: &crate::PipelineDevice, queue: &wgpu::Queue, submitted: Instant) {
        let (Some(grant), Some((begun, earlier))) = (self.grant.take(), self.begun.clone()) else { return };
        let cpu_ms = milliseconds(submitted.saturating_duration_since(begun));
        let (chunks, interval_ms) = (self.chunks.clone(), self.interval_ms(grant.kind));
        chunks.lock().unwrap().submit(grant);
        device.on_queue_done(queue, move || {
            let gpu_ms = busy_ms(earlier.get().copied(), submitted, Instant::now());
            chunks.lock().unwrap().complete(grant, [cpu_ms, gpu_ms], interval_ms);
        });
    }
    #[cfg(test)]
    pub(crate) fn pin(&mut self, interval_ms: f64, taps: [u64; 2]) {
        let mut chunks = Chunks::new(taps);
        for chunk in &mut chunks.kinds { chunk.full_gpu_ms = Some(0.); }
        self.chunks = Arc::new(Mutex::new(chunks));
        for kind in 0..2 { for _ in 0..SPACINGS { self.space(kind, interval_ms); } }
    }
    fn space(&mut self, kind: usize, spacing_ms: f64) {
        let spacings = &mut self.spacings[kind];
        if spacings.len() == SPACINGS { spacings.pop_front(); }
        spacings.push_back(spacing_ms);
    }
    fn interval_ms(&self, kind: usize) -> f64 {
        self.spacings[kind].iter().copied().reduce(f64::min).map_or(FASTEST_INTERVAL_MS, |spacing| spacing.clamp(FASTEST_INTERVAL_MS, SLOWEST_INTERVAL_MS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(chunks: &mut Chunks, motion: bool, frames: usize, cost: impl Fn(u64) -> f64) -> Vec<u64> {
        (0..frames).map(|_| {
            let grant = chunks.submit(chunks.grant(motion, FASTEST_INTERVAL_MS));
            chunks.complete(grant, [0., cost(grant.taps)], FASTEST_INTERVAL_MS);
            grant.taps
        }).collect()
    }

    #[test]
    fn a_host_meeting_its_interval_keeps_the_fixed_budgets() {
        let mut chunks = Chunks::default();
        let moving = frames(&mut chunks, true, 64, |taps| 1.5 + taps as f64 / 2e5);
        let idle = frames(&mut chunks, false, 64, |taps| 1.5 + taps as f64 / 2e6);
        assert!(moving.iter().all(|taps| *taps == MOVING_TAPS) && idle.iter().all(|taps| *taps == IDLE_TAPS), "frames within their interval admitted {moving:?} moving and {idle:?} idle");
    }

    #[test]
    fn one_slow_frame_after_open_does_not_hold_the_next_frames_to_one_unit() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 150.);
        let admitted = frames(&mut chunks, false, RECOVERY_FRAMES as usize, |taps| 1. + taps as f64 / 2e7);
        assert_eq!(admitted.last(), Some(&IDLE_TAPS), "after one 150 ms frame, eight frames that cost 1 ms plus 2e7 taps a millisecond admitted {admitted:?}");
    }

    #[test]
    fn a_frame_cost_above_its_interval_still_admits_more_than_one_unit_an_idle_frame() {
        let mut chunks = Chunks::default();
        let admitted = frames(&mut chunks, false, 64, |taps| 12. + taps as f64 / 2e7);
        assert!(admitted[32..].iter().all(|taps| *taps >= 1 << 20), "frames that cost 12 ms plus 2e7 taps a millisecond admitted {:?}", &admitted[32..]);
    }

    #[test]
    fn a_fixed_gpu_cost_above_its_interval_behind_a_frame_in_flight_admits_the_chunk_at_least_every_other_frame() {
        let (mut chunks, mut previous) = (Chunks::default(), None::<Grant>);
        let admitted: Vec<u64> = (0..64).map(|_| {
            let grant = chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS));
            if let Some(previous) = previous.replace(grant) { chunks.complete(previous, [1., 12. + previous.taps as f64 / 2e7], FASTEST_INTERVAL_MS); }
            grant.taps
        }).collect();
        assert!(admitted[32..].chunks(2).all(|pair| pair.contains(&IDLE_TAPS)), "frames that cost 12 ms of GPU plus 2e7 taps a millisecond, one in flight, admitted {:?}", &admitted[32..]);
    }

    #[test]
    fn one_slow_first_gpu_frame_does_not_pin_the_budget_below_its_cap() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 30.);
        let admitted = frames(&mut chunks, false, 32, |taps| taps as f64 / 1e7);
        assert!(admitted.iter().any(|taps| *taps >= 8 << 20), "frames that ran at 1e7 taps a millisecond after one 30 ms first frame admitted {admitted:?}");
    }

    #[test]
    fn two_slow_frames_after_open_do_not_hold_the_next_frames_to_one_unit_for_good() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 150.);
        frames(&mut chunks, false, 1, |_| 100.);
        let admitted = frames(&mut chunks, false, 64, |taps| 1. + taps as f64 / 2e7);
        assert!(admitted[32..].iter().all(|taps| *taps >= 1 << 20), "after a 150 ms and a 100 ms frame, frames that cost 1 ms plus 2e7 taps a millisecond admitted {:?}", &admitted[32..]);
    }

    #[test]
    fn frames_that_grow_slower_at_the_same_taps_shrink_the_next_frames_work() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, true, 64, |taps| 1. + taps as f64 / 2e7);
        let admitted = frames(&mut chunks, true, 64, |taps| 1. + taps as f64 / 1e5);
        let ms = 1. + *admitted.last().unwrap() as f64 / 1e5;
        assert!(ms < 2. * FASTEST_INTERVAL_MS, "after the cost per tap rose 200x, the 64th frame admitted {} taps and took {ms} ms", admitted.last().unwrap());
    }

    #[test]
    fn idle_frames_after_a_stroke_reach_half_their_steady_work_within_four_frames() {
        let (mut chunks, rate) = (Chunks::default(), 1e6);
        frames(&mut chunks, true, 100, |taps| 5. + taps as f64 / rate);
        let admitted = frames(&mut chunks, false, 16, |taps| 0.5 + taps as f64 / rate);
        let steady = (FASTEST_INTERVAL_MS - 0.5) * rate;
        assert!(admitted[4..].iter().all(|taps| *taps as f64 >= steady / 2.), "after a stroke whose dabs took 5 ms of GPU a frame, idle frames that cost 0.5 ms plus 1e6 taps a millisecond admitted {admitted:?}, not {steady}");
    }

    #[test]
    fn a_presentation_finished_before_the_next_frame_does_not_hold_a_pan_frame_to_one_unit() {
        let (mut chunks, start) = (Chunks::default(), Instant::now());
        let at = |ms: f64| start + Duration::from_secs_f64(ms / 1000.);
        let admitted: Vec<u64> = (0..16).map(|frame| {
            let now = frame as f64 * FASTEST_INTERVAL_MS;
            let presented = (frame > 0).then(|| at(now - FASTEST_INTERVAL_MS + 1.55));
            let grant = chunks.submit(chunks.grant(true, FASTEST_INTERVAL_MS));
            let busy = busy_ms(presented, at(now + 0.5), at(now + 0.5 + grant.taps as f64 / 1e7));
            chunks.complete(grant, [0.5, busy], FASTEST_INTERVAL_MS);
            grant.taps
        }).collect();
        assert!(admitted.iter().all(|taps| *taps == MOVING_TAPS), "a pan at 120 Hz on a 1e7 taps a millisecond GPU, presenting 1 ms after each frame, admitted {admitted:?}");
    }

    #[test]
    fn moving_frames_in_flight_together_halve_the_budget_once_and_recover_within_eight_frames() {
        let mut chunks = Chunks::default();
        let frame = |chunks: &mut Chunks, ms: [f64; 2]| { let grant = chunks.submit(chunks.grant(true, FASTEST_INTERVAL_MS)); chunks.complete(grant, ms, FASTEST_INTERVAL_MS); grant.taps };
        let probed = [frame(&mut chunks, [40., 0.]), frame(&mut chunks, [1., 0.])];
        assert_eq!([probed[0], probed[1], chunks.kinds[0].taps], [MOVING_TAPS, UNIT, MOVING_TAPS], "a first miss measures the fixed cost in one frame at the floor and resumes its budget");
        let grants: Vec<_> = (0..2).map(|_| chunks.submit(chunks.grant(true, FASTEST_INTERVAL_MS))).collect();
        for grant in grants { chunks.complete(grant, [40., 0.], FASTEST_INTERVAL_MS); }
        assert_eq!(chunks.kinds[0].taps, MOVING_TAPS / 2, "two slow frames in flight together");
        let recovered = frames(&mut chunks, true, RECOVERY_FRAMES as usize, |_| 1.);
        assert_eq!(recovered.last(), Some(&MOVING_TAPS), "after one halving, on-time frames admitted {recovered:?}");
    }

    #[test]
    fn idle_frames_past_their_interval_keep_their_chunk_and_moving_frames_halve_theirs() {
        let mut chunks = Chunks::default();
        let mut frame = |motion: bool| {
            let grant = chunks.submit(chunks.grant(motion, FASTEST_INTERVAL_MS));
            let ms = 1. + grant.taps as f64 / 2e4;
            chunks.complete(grant, [ms, ms], FASTEST_INTERVAL_MS);
            grant.taps
        };
        let idle: Vec<u64> = (0..16).map(|_| frame(false)).collect();
        assert!(idle.iter().all(|taps| *taps == IDLE_TAPS), "idle frames costing 1 ms plus 2e4 taps a millisecond of CPU and GPU admitted {idle:?}");
        let moving: Vec<u64> = (0..16).map(|_| frame(true)).collect();
        assert!(moving[8..].iter().all(|taps| 1. + *taps as f64 / 2e4 < 2. * FASTEST_INTERVAL_MS), "moving frames costing 1 ms plus 2e4 taps a millisecond of CPU and GPU admitted {moving:?}");
    }

    #[test]
    fn a_pen_down_behind_idle_frames_slower_than_their_interval_waits_for_at_most_one_interval_beyond_one_idle_frame() {
        for gpu_ms_per_mi in [0.5, 2., 6., 20.] {
            let (mut chunks, interval) = (Chunks::default(), FASTEST_INTERVAL_MS);
            let (mut in_flight, mut gpu_free) = (VecDeque::<(Grant, f64, f64)>::new(), 0f64);
            let (idle_frame_ms, unit_ms) = ((IDLE_TAPS >> 20) as f64 * gpu_ms_per_mi, UNIT as f64 / (1 << 20) as f64 * gpu_ms_per_mi);
            let bound = interval + idle_frame_ms + (idle_frame_ms / interval).ceil() * unit_ms;
            for frame in 0..96 {
                let now = frame as f64 * interval;
                while in_flight.front().is_some_and(|(.., end)| *end <= now) {
                    let (grant, busy, _) = in_flight.pop_front().unwrap();
                    chunks.complete(grant, [1., busy], interval);
                }
                let pen_down = frame >= 32 && frame % 8 == 0;
                let grant = chunks.submit(chunks.grant(pen_down, interval));
                let busy = if pen_down { 1. } else { grant.taps.max(UNIT) as f64 / (1 << 20) as f64 * gpu_ms_per_mi };
                let submitted = now + 1.;
                let ahead = (gpu_free - submitted).max(0.);
                assert!(!pen_down || ahead <= bound, "at {gpu_ms_per_mi} ms of GPU a Mi, a pen-down waited behind {ahead} ms of queued work, more than one interval, one {idle_frame_ms} ms idle frame and the one-unit floors queued behind it ({bound} ms)");
                gpu_free = gpu_free.max(submitted) + busy;
                in_flight.push_back((grant, busy, gpu_free));
            }
        }
    }

    #[test]
    fn a_pen_down_behind_idle_frames_with_a_fixed_gpu_cost_waits_for_at_most_one_interval_beyond_one_idle_frame() {
        let (mut chunks, interval, fixed_ms, gpu_ms_per_mi) = (Chunks::default(), FASTEST_INTERVAL_MS, 8., 0.125);
        let (mut in_flight, mut gpu_free, mut worst) = (VecDeque::<(Grant, f64, f64)>::new(), 0f64, 0f64);
        let idle_frame_ms = fixed_ms + (IDLE_TAPS >> 20) as f64 * gpu_ms_per_mi;
        let floor_frame_ms = fixed_ms + UNIT as f64 / (1 << 20) as f64 * gpu_ms_per_mi;
        let bound = interval + idle_frame_ms + (idle_frame_ms / interval).ceil() * floor_frame_ms;
        for frame in 0..512 {
            let now = frame as f64 * interval;
            while in_flight.front().is_some_and(|(.., end)| *end <= now) {
                let (grant, busy, _) = in_flight.pop_front().unwrap();
                chunks.complete(grant, [1., busy], interval);
            }
            let pen_down = frame >= 256 && frame % 64 == 0;
            let grant = chunks.submit(chunks.grant(pen_down, interval));
            let busy = if pen_down { 1. } else { fixed_ms + grant.taps.max(UNIT) as f64 / (1 << 20) as f64 * gpu_ms_per_mi };
            let submitted = now + 1.;
            if pen_down { worst = worst.max((gpu_free - submitted).max(0.)); }
            gpu_free = gpu_free.max(submitted) + busy;
            in_flight.push_back((grant, busy, gpu_free));
        }
        assert!(worst <= bound, "idle frames with {fixed_ms} ms of fixed GPU cost and {idle_frame_ms} ms in all left a pen-down {worst} ms of queued GPU work, more than one interval, one idle frame and the floor frames queued behind it ({bound} ms)");
    }

    #[test]
    fn a_floor_frame_slowed_by_a_stall_does_not_hide_later_misses_for_good() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, true, 1, |_| 150.);
        frames(&mut chunks, true, 1, |_| 100.);
        let admitted = frames(&mut chunks, true, 64, |taps| 1. + taps as f64 / 2e4);
        let ms = 1. + *admitted.last().unwrap() as f64 / 2e4;
        assert!(ms < 2. * FASTEST_INTERVAL_MS, "after a 150 ms frame and a 100 ms frame at the floor, moving frames that cost 1 ms plus 2e4 taps a millisecond still admitted {} taps and took {ms} ms", admitted.last().unwrap());
    }

    #[test]
    fn a_gated_frame_completing_does_not_open_the_gate_behind_full_frames_in_flight() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 5.);
        let queued = [0; 3].map(|_| chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS)));
        assert_eq!(queued.map(|grant| grant.taps), [IDLE_TAPS, IDLE_TAPS, 0]);
        for grant in &queued[..2] { chunks.complete(*grant, [0., 5.], FASTEST_INTERVAL_MS); }
        let behind = chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS));
        assert_eq!(behind.taps, IDLE_TAPS);
        chunks.complete(queued[2], [0., 0.5], FASTEST_INTERVAL_MS);
        let next = [0; 4].map(|_| chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS)).taps);
        assert!(next.iter().filter(|taps| **taps > UNIT).count() <= 1, "with one 8 Mi frame of 4.5 ms best-effort work still in flight, the next frames admitted {next:?}");
    }

    #[test]
    fn best_effort_waits_while_the_frames_in_flight_hold_more_than_one_interval_of_gpu_work() {
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 5.);
        let behind_presentations = [0; 5].map(|_| chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS)).taps);
        assert_eq!(behind_presentations[..2], [IDLE_TAPS; 2], "frames whose own GPU work fits one interval wait only for each other");
        assert_eq!([chunks.grant(true, FASTEST_INTERVAL_MS).taps, chunks.grant(false, FASTEST_INTERVAL_MS).taps], [0, 0], "best-effort behind more than one interval of queued frames");
        let mut chunks = Chunks::default();
        frames(&mut chunks, false, 1, |_| 1.);
        let behind_presentations = [0; 8].map(|_| chunks.submit(chunks.grant(false, FASTEST_INTERVAL_MS)).taps);
        assert_eq!(behind_presentations, [IDLE_TAPS; 8], "frames queued behind presentations with 1 ms of their own work each");
    }
}
