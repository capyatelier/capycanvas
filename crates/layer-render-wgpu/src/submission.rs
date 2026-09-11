//! Bound the native command buffers materialized by a large document replay.
//! wgpu can expand each render pass into multiple Metal command buffers at
//! finish. Keep chunks unfinalized until all uploads are closed, then finish
//! and submit each chunk in order before materializing the next one.
use std::ops::{Deref, DerefMut};

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
        if self.passes == Self::PASSES_PER_SUBMISSION {
            let next = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("continued document submission"),
                });
            self.earlier
                .push(std::mem::replace(&mut self.current, next));
            self.passes = 0;
        }
        self.passes += 1;
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
