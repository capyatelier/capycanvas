//! Work done ahead of a drag, and recomposition after one, spread over
//! frames by how long the GPU spent on earlier frames that did the same kind
//! of work, so a drag that starts meanwhile does not wait behind it.
use super::*;
use crate::frame_timing::{GpuFrameSample, GpuFrameTimer};
use std::collections::VecDeque;

/// Kinds of preparation, each with its own cost per unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Work {
    /// Static layers composed around a moving one.
    Layers,
    /// A placed layer's own pixels copied for its drag.
    Placement,
    /// Pages of a transformed layer reduced for its drag.
    Reduce,
    /// Pages of a still transform preview drawn at full resolution.
    Settle,
    /// Tiles recomposed after a drag.
    Recompose,
}
const KINDS: usize = 5;

/// GPU time a frame may spend on each kind of preparation: more on what a
/// drag waits for, less on what follows a release, which a drag does not.
const TARGET: [Duration; KINDS] = [
    Duration::from_millis(10),
    Duration::from_millis(10),
    Duration::from_millis(10),
    Duration::from_millis(5),
    Duration::from_millis(5),
];
/// Units of each kind a frame prepares before any has been measured: a
/// settled page costs far more than a tile or a reduced page.
const FIRST_UNITS: [usize; KINDS] = [4, 4, 4, 1, 4];
const MOST_UNITS: usize = 96;
/// CPU time a frame spends on its preparation from its first unit, after at
/// least one unit of each kind it prepares.
const CPU_TIME: Duration = Duration::from_millis(4);
/// Frames that prepared whose GPU time is still to be read.
const PENDING_FRAMES: usize = 8;

/// Units of each kind of preparation a frame may do: tiles or pages, within
/// CPU_TIME. Measured from the start of a frame's first preparation to the
/// end of its last, and counted against the kind that started, a frame's GPU
/// time sets the count to the units that fit the kind's TARGET at the cost
/// per unit it measured: at most twice the units that frame was allowed, and
/// no more than them after a frame that did fewer. Frames still in flight
/// when the count changes report the units they were allowed, so late
/// measurements do not compound. Without GPU timestamps a frame prepares
/// the most that fits CPU_TIME.
pub(super) struct Preparation {
    units: [usize; KINDS],
    timer: Option<GpuFrameTimer>,
    frame: u64,
    started: Option<Work>,
    ended: bool,
    filled: bool,
    deadline: Option<web_time::Instant>,
    pending: VecDeque<(u64, Work, usize, bool)>,
}
impl Preparation {
    /// Preparation measured by GPU timestamps when `measured` and `device`
    /// has them.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, measured: bool) -> Self {
        let timer = (measured && device.features().contains(wgpu::Features::TIMESTAMP_QUERY))
            .then(|| GpuFrameTimer::new(device, queue));
        Self {
            units: if timer.is_some() { FIRST_UNITS } else { [MOST_UNITS; KINDS] },
            timer,
            frame: 0,
            started: None,
            ended: false,
            filled: true,
            deadline: None,
            pending: VecDeque::new(),
        }
    }
    /// Take in the GPU time of earlier frames as this one begins.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.started = None;
        self.ended = false;
        self.filled = true;
        self.deadline = None;
        let Some(timer) = &mut self.timer else {
            return;
        };
        timer.poll(device, queue);
        let mut samples = [GpuFrameSample::default(); 4];
        let count = timer.take_into(&mut samples);
        for sample in &samples[..count] {
            let elapsed = (sample.status == 1).then(|| Duration::from_nanos(sample.elapsed_ns));
            self.measured(sample.frame, elapsed);
        }
    }
    /// Take in `frame`'s GPU time, when it could be read.
    fn measured(&mut self, frame: u64, elapsed: Option<Duration>) {
        let Some(index) = self.pending.iter().position(|(pending, ..)| *pending == frame) else {
            return;
        };
        let (_, work, units, filled) = self.pending.remove(index).unwrap();
        if let Some(elapsed) = elapsed {
            self.units[work as usize] = adjust(units, elapsed, TARGET[work as usize], filled);
        }
    }
    /// Measure the GPU time from here as `work` begins, unless this frame's
    /// preparation started earlier. Returns the units of `work` this frame
    /// may do.
    pub fn start(&mut self, encoder: &mut wgpu::CommandEncoder, work: Work) -> usize {
        self.deadline.get_or_insert_with(|| web_time::Instant::now() + CPU_TIME);
        if self.started.is_none()
            && let Some(timer) = &mut self.timer
        {
            self.frame += 1;
            self.started = timer.begin_encoded(encoder, self.frame).then_some(work);
        }
        self.units[work as usize]
    }
    /// Measure until here, right after a preparation, unless a later one
    /// ends further on. One that did fewer units than allowed, for lack of
    /// work, time or what it waits for, does not grow the count.
    fn end(&mut self, encoder: &mut wgpu::CommandEncoder, filled: bool) {
        self.filled &= filled;
        if self.started.is_some() {
            self.ended = true;
            self.timer.as_mut().unwrap().end_encoded(encoder);
        }
    }
    /// Finish measuring this frame, here when no preparation ended earlier.
    pub fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(work) = self.started else {
            return;
        };
        if !self.ended {
            self.end(encoder, true);
        }
        if self.pending.len() == PENDING_FRAMES {
            self.pending.pop_front();
        }
        self.pending.push_back((self.frame, work, self.units[work as usize], self.filled));
    }
    /// Note that the measured frame was submitted.
    pub fn submitted(&mut self, queue: &wgpu::Queue) {
        if let Some(timer) = &mut self.timer {
            timer.submitted(queue);
        }
    }
    #[cfg(test)]
    pub fn limit(&mut self, units: usize) {
        self.units = [units; KINDS];
    }
}

impl WgpuRasterizer {
    /// Do up to this frame's units of `work`, a unit per `step`, until a step
    /// has nothing to do or the frame's preparation has taken CPU_TIME.
    pub(super) fn prepare(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        work: Work,
        mut step: impl FnMut(&mut Self, &mut crate::submission::CommandEncoder) -> Result<bool, GpuRasterError>,
    ) -> Result<(), GpuRasterError> {
        let units = self.preparation.start(encoder, work);
        let mut done = 0;
        let result = loop {
            match step(self, encoder) {
                Ok(true) => done += 1,
                other => break other.map(drop),
            }
            if done == units || self.preparation.deadline.is_some_and(|d| web_time::Instant::now() >= d) {
                break Ok(());
            }
        };
        self.preparation.end(encoder, done == units);
        result
    }
}

/// The units that fit `target` after a frame allowed `units` took `elapsed`
/// on the GPU, having done all of them when `filled`.
fn adjust(units: usize, elapsed: Duration, target: Duration, filled: bool) -> usize {
    let most = if filled { units * 2 } else { units }.min(MOST_UNITS);
    if elapsed.is_zero() {
        return most;
    }
    let fitting = units as f64 * target.as_secs_f64() / elapsed.as_secs_f64();
    (fitting as usize).clamp(1, most)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> (wgpu::Device, wgpu::Queue) {
        let instance = WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        pollster::block_on(adapter.request_device(&Default::default())).unwrap()
    }

    #[test]
    fn units_follow_the_measured_gpu_time() {
        let target = Duration::from_millis(10);
        assert_eq!(adjust(4, target / 4, target, true), 8, "a count at most doubles");
        assert_eq!(adjust(4, target * 3 / 4, target, true), 5);
        assert_eq!(adjust(4, target * 2, target, true), 2);
        assert_eq!(adjust(9, target * 4, target, true), 2);
        assert_eq!(adjust(1, target * 10, target, true), 1, "a frame always prepares something");
        assert_eq!(adjust(MOST_UNITS, Duration::ZERO, target, true), MOST_UNITS);
        assert_eq!(adjust(4, target / 4, target, false), 4, "a frame held back by something else does not grow it");
        assert_eq!(adjust(4, target * 2, target, false), 2);
    }

    #[test]
    fn devices_without_timestamps_prepare_the_most_that_fits_the_cpu_time() {
        let (device, queue) = device();
        assert!(!device.features().contains(wgpu::Features::TIMESTAMP_QUERY));
        let mut preparation = Preparation::new(&device, &queue, true);
        for work in [Work::Layers, Work::Placement, Work::Reduce, Work::Settle, Work::Recompose] {
            for _ in 0..3 {
                preparation.begin(&device, &queue);
                let mut encoder = device.create_command_encoder(&Default::default());
                assert_eq!(preparation.start(&mut encoder, work), MOST_UNITS, "{work:?}");
                preparation.end(&mut encoder, false);
                preparation.finish(&mut encoder);
                queue.submit([encoder.finish()]);
                preparation.submitted(&queue);
            }
        }
    }

    #[test]
    fn measurements_of_frames_in_flight_do_not_compound() {
        let (device, queue) = device();
        let work = Work::Settle;
        let target = TARGET[work as usize];
        let mut preparation = Preparation::new(&device, &queue, false);
        preparation.pending.extend((1..=4).map(|frame| (frame, work, 1, true)));
        for frame in 1..=4 {
            preparation.measured(frame, Some(target / 10));
        }
        assert_eq!(preparation.units[work as usize], 2, "four cheap frames of one page each allow two, not sixteen");
        preparation.pending.extend((5..=8).map(|frame| (frame, work, 8, true)));
        for frame in 5..=8 {
            preparation.measured(frame, Some(target * 3));
        }
        assert_eq!(preparation.units[work as usize], 2, "four costly frames of eight pages each allow two, not one");
        preparation.measured(9, Some(target / 10));
        assert_eq!(preparation.units[work as usize], 2, "an unknown frame changes nothing");
    }
}
