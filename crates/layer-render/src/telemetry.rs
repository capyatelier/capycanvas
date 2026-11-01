//! Bounded, nonblocking renderer observations. Times are not display latency.
use std::sync::atomic::{AtomicU64, Ordering};

pub struct PresentationRate {
    timestamps: [AtomicU64; 1024],
    next: AtomicU64,
    latest: AtomicU64,
}
impl Default for PresentationRate {
    fn default() -> Self {
        Self {
            timestamps: std::array::from_fn(|_| AtomicU64::new(0)),
            next: AtomicU64::new(0),
            latest: AtomicU64::new(0),
        }
    }
}
impl PresentationRate {
    pub fn observe(&self, timestamp_ns: u64) {
        if timestamp_ns == 0 { return; }
        self.latest.fetch_max(timestamp_ns, Ordering::Relaxed);
        let index = self.next.fetch_add(1, Ordering::Relaxed) as usize % self.timestamps.len();
        self.timestamps[index].store(timestamp_ns, Ordering::Relaxed);
    }
    pub fn fps(&self, now_ns: u64) -> Option<f32> {
        if self.latest.load(Ordering::Relaxed) == 0 { return None; }
        let mut timestamps = [0; 1024];
        let mut count = 0;
        for sample in &self.timestamps {
            let timestamp = sample.load(Ordering::Relaxed);
            if timestamp > 0 && timestamp <= now_ns && now_ns - timestamp < 1_000_000_000 {
                timestamps[count] = timestamp;
                count += 1;
            }
        }
        timestamps[..count].sort_unstable();
        Some(if count == 0 { 0. } else {
            1. + timestamps[..count].windows(2).filter(|pair| pair[0] != pair[1]).count() as f32
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameRateSource { Presented, Submitted }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRate {
    pub fps: f32,
    pub source: FrameRateSource,
}

#[derive(Clone, Debug)]
pub struct TimingSamples {
    pub values: [f32; 120],
    pub count: u64,
}
impl Default for TimingSamples {
    fn default() -> Self {
        Self {
            values: [0.; 120],
            count: 0,
        }
    }
}
impl TimingSamples {
    pub fn push(&mut self, ms: f32) {
        if ms.is_finite() && ms >= 0. {
            self.values[self.count as usize % 120] = ms;
            self.count += 1;
        }
    }
    pub fn ordered(&self) -> Vec<f32> {
        let n = (self.count as usize).min(120);
        let start = if self.count >= 120 {
            self.count as usize % 120
        } else {
            0
        };
        (0..n).map(|i| self.values[(start + i) % 120]).collect()
    }
}
#[derive(Clone, Debug, Default)]
pub struct RendererTelemetry {
    pub frame_rate: Option<FrameRate>,
    pub cpu: TimingSamples,
    pub gpu: TimingSamples,
    pub submissions: u64,
    pub dabs: u64,
    pub dirty_pixels: u64,
    pub resident_bytes: u64,
    pub effect_passes: u64,
    pub compiled_effects: u64,
    pub gpu_timestamps: bool,
    pub memory: Option<MemorySnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessMemoryKind { Resident, PrivateCommit, PhysicalFootprint }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryPeak { Lifetime(u64), Sampled(u64) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessMemory {
    pub kind: ProcessMemoryKind,
    pub bytes: u64,
    pub peak: MemoryPeak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuMemoryKind { VulkanHeap, DxgiLocal, MetalAllocated }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuMemory {
    pub kind: GpuMemoryKind,
    pub bytes: u64,
    pub budget: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct MemorySnapshot {
    pub sampled_at: std::time::Instant,
    pub process: Option<ProcessMemory>,
    pub gpu: Option<GpuMemory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_rate_counts_a_full_second_and_expires_at_idle() {
        let rate = PresentationRate::default();
        assert_eq!(rate.fps(1_000_000_000), None);
        for frame in 1..=120 { rate.observe(1_000_000_000 + frame * 8_333_333); }
        assert_eq!(rate.fps(2_000_000_000), Some(120.));
        assert_eq!(rate.fps(2_500_000_000), Some(60.));
        assert_eq!(rate.fps(3_000_000_000), Some(0.));
        rate.observe(3_500_000_000);
        assert_eq!(rate.fps(3_500_000_000), Some(1.));
    }

    #[test]
    fn presentation_rate_keeps_stalls_and_reordered_feedback_without_duplicates() {
        let rate = PresentationRate::default();
        rate.observe(0);
        rate.observe(1_000_000_000);
        rate.observe(1_000_000_000);
        rate.observe(999_999_999);
        rate.observe(1_800_000_000);
        assert_eq!(rate.fps(1_900_000_000), Some(3.));
        assert_eq!(rate.fps(2_000_000_000), Some(1.));
        assert_eq!(rate.fps(2_800_000_000), Some(0.));
    }

    #[test]
    fn presentation_rate_reuses_storage_during_sustained_motion() {
        let rate = PresentationRate::default();
        for frame in 1..=2000 { rate.observe(frame * 4_000_000); }
        assert_eq!(rate.fps(8_000_000_000), Some(250.));
        assert_eq!(rate.fps(7_996_000_000), Some(250.));
    }

    #[test]
    fn concurrent_presentation_callbacks_keep_distinct_frames() {
        let rate = std::sync::Arc::new(PresentationRate::default());
        let workers: Vec<_> = (0..2).map(|parity| {
            let rate = rate.clone();
            std::thread::spawn(move || {
                for frame in (parity..240).step_by(2) { rate.observe(1_000_000_000 + frame * 4_000_000); }
            })
        }).collect();
        for worker in workers { worker.join().unwrap(); }
        assert_eq!(rate.fps(1_960_000_000), Some(240.));
    }
}
