//! Bounded, nonblocking renderer observations. Times are not display latency.
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
    pub cpu: TimingSamples,
    pub gpu: TimingSamples,
    pub submissions: u64,
    pub dabs: u64,
    pub dirty_pixels: u64,
    pub resident_bytes: u64,
    pub effect_passes: u64,
    pub compiled_effects: u64,
    pub gpu_timestamps: bool,
}
