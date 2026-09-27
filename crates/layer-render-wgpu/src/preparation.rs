//! Work done ahead of a drag, and recomposition after one, spread over
//! frames by how long the GPU spent on earlier frames that did the same kind
//! of work, so a drag that starts meanwhile does not wait behind it.
use crate::frame_timing::{GpuFrameSample, GpuFrameTimer};
use std::collections::VecDeque;
use std::time::Duration;

/// Kinds of preparation, each with its own cost per unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Work {
    /// Static layers composed around a moving one.
    Layers,
    /// A placed layer's own pixels copied for its drag.
    Placement,
    /// Pages of a transformed layer reduced for its drag.
    Reduce,
    /// Pages of a still transform preview drawn exactly into the display.
    Display,
    /// Pages of a still transform preview drawn at full resolution.
    Settle,
    /// Tiles recomposed after a drag.
    Recompose,
}
const KINDS: usize = 6;

/// GPU time a frame may spend on each kind of preparation: more on what a
/// drag waits for, less on what follows a release, which a drag does not.
const TARGET: [Duration; KINDS] = [
    Duration::from_millis(10),
    Duration::from_millis(10),
    Duration::from_millis(10),
    Duration::from_millis(5),
    Duration::from_millis(5),
    Duration::from_millis(5),
];
/// Units of each kind a frame prepares before any has been measured: a page
/// drawn exactly into the display or settled costs far more than a tile or a
/// reduced page. Without GPU timestamps a frame always prepares the most.
const FIRST_UNITS: [usize; KINDS] = [4, 4, 4, 1, 1, 4];
const MOST_UNITS: usize = 96;
/// Frames that prepared whose GPU time is still to be read.
const PENDING_FRAMES: usize = 8;

/// Units of each kind of preparation a frame may do: tiles or pages.
/// Measured from the start of a frame's first preparation to the end of its
/// last, and counted against the kind that started, a frame's GPU time sets
/// the count to the units that fit the kind's TARGET at the cost per unit it
/// measured: at most twice the units that frame was allowed, and no more
/// than them after a frame that did fewer. Frames still in flight when the
/// count changes report the units they were allowed, so late measurements
/// do not compound. Unmeasured, it stays where it began.
pub(super) struct Preparation {
    units: [usize; KINDS],
    measured: bool,
    timer: Option<GpuFrameTimer>,
    frame: u64,
    started: Option<Work>,
    ended: bool,
    filled: bool,
    pending: VecDeque<(u64, Work, usize, bool)>,
}
impl Preparation {
    /// Preparation measured by GPU timestamps when `measured` and the device
    /// has them, otherwise always the most.
    pub fn new(measured: bool) -> Self {
        Self {
            units: if measured { FIRST_UNITS } else { [MOST_UNITS; KINDS] },
            measured,
            timer: None,
            frame: 0,
            started: None,
            ended: false,
            filled: true,
            pending: VecDeque::new(),
        }
    }
    /// Take in the GPU time of earlier frames as this one begins.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.started = None;
        self.ended = false;
        self.filled = true;
        if !self.measured || !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return;
        }
        let timer = self.timer.get_or_insert_with(|| GpuFrameTimer::new(device, queue));
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
    /// The units of `work` this frame may do.
    pub fn units(&self, work: Work) -> usize {
        self.units[work as usize]
    }
    /// Measure the GPU time from here as `work` begins, unless this frame's
    /// preparation started earlier.
    pub fn start(&mut self, encoder: &mut wgpu::CommandEncoder, work: Work) {
        if self.started.is_some() {
            return;
        }
        if let Some(timer) = &mut self.timer {
            self.frame += 1;
            self.started = timer.begin_encoded(encoder, self.frame).then_some(work);
        }
    }
    /// Measure until here, right after a preparation, unless a later one
    /// ends further on.
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.started.is_some() {
            self.ended = true;
            self.timer.as_mut().unwrap().end_encoded(encoder);
        }
    }
    /// End as `end` after a preparation that did fewer units than allowed,
    /// held back by something else, so its count does not grow.
    pub fn end_short(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.filled = false;
        self.end(encoder);
    }
    /// Finish measuring this frame, here when no preparation ended earlier.
    pub fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(work) = self.started else {
            return;
        };
        if !self.ended {
            self.end(encoder);
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
        assert_eq!(Preparation::new(false).units(Work::Settle), MOST_UNITS);
    }

    #[test]
    fn measurements_of_frames_in_flight_do_not_compound() {
        let work = Work::Display;
        let target = TARGET[work as usize];
        let mut preparation = Preparation::new(true);
        preparation.pending.extend((1..=4).map(|frame| (frame, work, 1, true)));
        for frame in 1..=4 {
            preparation.measured(frame, Some(target / 10));
        }
        assert_eq!(preparation.units(work), 2, "four cheap frames of one page each allow two, not sixteen");
        preparation.pending.extend((5..=8).map(|frame| (frame, work, 8, true)));
        for frame in 5..=8 {
            preparation.measured(frame, Some(target * 3));
        }
        assert_eq!(preparation.units(work), 2, "four costly frames of eight pages each allow two, not one");
        preparation.measured(9, Some(target / 10));
        assert_eq!(preparation.units(work), 2, "an unknown frame changes nothing");
    }
}
