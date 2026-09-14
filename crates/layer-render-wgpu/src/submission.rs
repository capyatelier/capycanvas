//! Bound the native command buffers materialized by a large document replay.
//! wgpu can expand each render pass into multiple Metal command buffers at
//! finish. Keep chunks unfinalized until all uploads are closed, then finish
//! and submit each chunk in order before materializing the next one.
use std::ops::{Deref, DerefMut};

/// A cached GPU value is usable in queue order while its producing commands
/// are pending. Discarding those commands must invalidate the CPU cache key.
pub(crate) struct CacheWrite {
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tracked: std::sync::atomic::AtomicBool,
}
impl CacheWrite {
    pub fn new() -> Self {
        Self {
            valid: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            tracked: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub fn validity(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.valid.clone()
    }
    pub fn track(&self, encoder: &CommandEncoder) {
        use std::sync::atomic::Ordering;
        assert!(!self.tracked.swap(true, Ordering::Relaxed));
        let guard = DiscardedCacheWrite(Some(self.valid.clone()));
        encoder.on_submitted_work_done(move || guard.complete());
    }
}
impl Drop for CacheWrite {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        if !self.tracked.load(Ordering::Relaxed) {
            self.valid.store(false, Ordering::Release);
        }
    }
}
struct DiscardedCacheWrite(Option<std::sync::Arc<std::sync::atomic::AtomicBool>>);
impl DiscardedCacheWrite {
    fn complete(mut self) {
        self.0 = None;
    }
}
impl Drop for DiscardedCacheWrite {
    fn drop(&mut self) {
        if let Some(valid) = &self.0 {
            valid.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

pub(crate) struct CommandEncoder {
    device: wgpu::Device,
    current: wgpu::CommandEncoder,
    earlier: Vec<wgpu::CommandEncoder>,
    passes: usize,
}

impl CommandEncoder {
    // Leave ample space below Metal's 4096-buffer limit for wgpu's prepasses,
    // query handling and other encoders sharing the device.
    const PASSES_PER_SUBMISSION: usize = 512;

    pub fn new(device: &wgpu::Device, descriptor: &wgpu::CommandEncoderDescriptor<'_>) -> Self {
        Self {
            device: device.clone(),
            current: device.create_command_encoder(descriptor),
            earlier: Vec::new(),
            passes: 0,
        }
    }
    fn begin_pass(&mut self) {
        self.reserve_passes(1);
    }
    /// Account for a bounded helper that records raw wgpu passes. Rotate before
    /// the helper so its complete batch fits the same command-buffer ceiling.
    pub fn reserve_passes(&mut self, count: usize) {
        assert!(count <= Self::PASSES_PER_SUBMISSION);
        if self.passes + count > Self::PASSES_PER_SUBMISSION {
            let next = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("continued document submission"),
                });
            self.earlier
                .push(std::mem::replace(&mut self.current, next));
            self.passes = 0;
        }
        self.passes += count;
    }
    pub fn begin_render_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'a> {
        self.begin_pass();
        self.current.begin_render_pass(descriptor)
    }
    pub fn begin_compute_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'a> {
        self.begin_pass();
        self.current.begin_compute_pass(descriptor)
    }
    /// Finish staging uploads before calling this. Their completion callbacks
    /// belong to the final encoder, after every earlier chunk on this queue.
    pub fn submit(self, queue: &wgpu::Queue) -> wgpu::SubmissionIndex {
        for encoder in self.earlier {
            queue.submit([encoder.finish()]);
        }
        queue.submit([self.current.finish()])
    }
    #[cfg(test)]
    pub fn submit_timed(self, queue: &wgpu::Queue) -> [f64; 2] {
        let mut timing = [0.; 2];
        for encoder in self
            .earlier
            .into_iter()
            .chain(std::iter::once(self.current))
        {
            let start = std::time::Instant::now();
            let commands = encoder.finish();
            timing[0] += start.elapsed().as_secs_f64() * 1000.;
            let start = std::time::Instant::now();
            queue.submit([commands]);
            timing[1] += start.elapsed().as_secs_f64() * 1000.;
        }
        timing
    }
}

// Copies, upload callbacks and query commands do not start render/compute
// passes. Forward them to the current chunk without changing their ordering.
impl Deref for CommandEncoder {
    type Target = wgpu::CommandEncoder;
    fn deref(&self) -> &Self::Target {
        &self.current
    }
}
impl DerefMut for CommandEncoder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.current
    }
}
