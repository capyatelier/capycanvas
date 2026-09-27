//! Work done ahead of a drag, and recomposition after one, spread over
//! frames by how long the GPU spent on earlier frames that did it, so a drag
//! that starts meanwhile does not wait behind it.
use crate::frame_timing::{GpuFrameSample, GpuFrameTimer};
use std::collections::VecDeque;
use std::time::Duration;

/// GPU time a frame that prepares should take.
const TARGET: Duration = Duration::from_millis(10);
/// Units a frame prepares before any has been measured, or always without
/// GPU timestamps, and at most.
const FIRST_UNITS: usize = 4;
const MOST_UNITS: usize = 96;
/// Frames that prepared whose GPU time is still to be read.
const PENDING_FRAMES: usize = 8;

/// Units of preparation, each about one 256 x 256 tile of GPU work, a frame
/// may do. Measured from where a frame's preparation starts, the count
/// doubles after preparation that took under half of TARGET on the GPU,
/// grows by one under TARGET, and halves after longer. Unmeasured, it stays
/// where it began.
pub(super) struct Preparation {
    units: usize,
    measured: bool,
    timer: Option<GpuFrameTimer>,
    frame: u64,
    timing: bool,
    pending: VecDeque<u64>,
}
impl Preparation {
    /// Preparation measured by GPU timestamps when `measured` and the device
    /// has them, otherwise always the most.
    pub fn new(measured: bool) -> Self {
        Self {
            units: if measured { FIRST_UNITS } else { MOST_UNITS },
            measured,
            timer: None,
            frame: 0,
            timing: false,
            pending: VecDeque::new(),
        }
    }
    /// The units this frame may prepare.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> usize {
        if !self.measured || !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return self.units;
        }
        let timer = self.timer.get_or_insert_with(|| GpuFrameTimer::new(device, queue));
        timer.poll(device, queue);
        let mut samples = [GpuFrameSample::default(); 4];
        let count = timer.take_into(&mut samples);
        for sample in &samples[..count] {
            let Some(index) = self.pending.iter().position(|frame| *frame == sample.frame) else {
                continue;
            };
            self.pending.remove(index);
            if sample.status == 1 {
                self.units = adjust(self.units, Duration::from_nanos(sample.elapsed_ns));
            }
        }
        self.units
    }
    /// Measure the GPU time from here until `end`, as preparation begins in
    /// this frame.
    pub fn start(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.timing {
            return;
        }
        if let Some(timer) = &mut self.timer {
            self.frame += 1;
            self.timing = timer.begin_encoded(encoder, self.frame);
        }
    }
    /// Finish measuring this frame's preparation.
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !std::mem::take(&mut self.timing) {
            return;
        }
        self.timer.as_mut().unwrap().end_encoded(encoder);
        if self.pending.len() == PENDING_FRAMES {
            self.pending.pop_front();
        }
        self.pending.push_back(self.frame);
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
        assert_eq!(Preparation::new(false).units, MOST_UNITS);
    }
}
