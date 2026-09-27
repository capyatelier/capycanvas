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
    /// Blocks of a still transform preview drawn exactly into the display.
    Display,
    /// Pages of a still transform preview drawn at full resolution.
    Settle,
    /// Tiles recomposed after a drag.
    Recompose,
}
const KINDS: usize = 6;

/// GPU time a frame that prepares should take.
const TARGET: Duration = Duration::from_millis(10);
/// Units a frame prepares before any has been measured, or always without
/// GPU timestamps, and at most.
const FIRST_UNITS: usize = 4;
const MOST_UNITS: usize = 96;
/// Frames that prepared whose GPU time is still to be read.
const PENDING_FRAMES: usize = 8;

/// Units of each kind of preparation a frame may do: tiles, pages or blocks.
/// Measured from the start of a frame's first preparation to the end of its
/// last, and counted against the kind that started, the count doubles after
/// preparation that took under half of TARGET on the GPU, grows by one under
/// TARGET, and halves after longer. Unmeasured, it stays where it began.
pub(super) struct Preparation {
    units: [usize; KINDS],
    measured: bool,
    timer: Option<GpuFrameTimer>,
    frame: u64,
    started: Option<Work>,
    ended: bool,
    pending: VecDeque<(u64, Work)>,
}
impl Preparation {
    /// Preparation measured by GPU timestamps when `measured` and the device
    /// has them, otherwise always the most.
    pub fn new(measured: bool) -> Self {
        Self {
            units: [if measured { FIRST_UNITS } else { MOST_UNITS }; KINDS],
            measured,
            timer: None,
            frame: 0,
            started: None,
            ended: false,
            pending: VecDeque::new(),
        }
    }
    /// Take in the GPU time of earlier frames as this one begins.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.started = None;
        self.ended = false;
        if !self.measured || !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return;
        }
        let timer = self.timer.get_or_insert_with(|| GpuFrameTimer::new(device, queue));
        timer.poll(device, queue);
        let mut samples = [GpuFrameSample::default(); 4];
        let count = timer.take_into(&mut samples);
        for sample in &samples[..count] {
            let Some(index) = self.pending.iter().position(|(frame, _)| *frame == sample.frame) else {
                continue;
            };
            let (_, work) = self.pending.remove(index).unwrap();
            if sample.status == 1 {
                let units = &mut self.units[work as usize];
                *units = adjust(*units, Duration::from_nanos(sample.elapsed_ns));
            }
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
        self.pending.push_back((self.frame, work));
    }
    /// Note that the measured frame was submitted.
    pub fn submitted(&mut self, queue: &wgpu::Queue) {
        if let Some(timer) = &mut self.timer {
            timer.submitted(queue);
        }
    }
}

/// The units after a frame that prepared `units` took `elapsed` on the GPU.
fn adjust(units: usize, elapsed: Duration) -> usize {
    if elapsed > TARGET {
        (units / 2).max(1)
    } else if elapsed <= TARGET / 2 {
        (units * 2).min(MOST_UNITS)
    } else {
        (units + 1).min(MOST_UNITS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_follow_the_measured_gpu_time() {
        assert_eq!(adjust(FIRST_UNITS, TARGET / 4), FIRST_UNITS * 2);
        assert_eq!(adjust(FIRST_UNITS, TARGET * 3 / 4), FIRST_UNITS + 1);
        assert_eq!(adjust(FIRST_UNITS, TARGET * 2), FIRST_UNITS / 2);
        assert_eq!(adjust(1, TARGET * 10), 1, "a frame always prepares something");
        assert_eq!(adjust(MOST_UNITS, Duration::ZERO), MOST_UNITS);
        assert_eq!(Preparation::new(false).units(Work::Settle), MOST_UNITS);
    }
}
