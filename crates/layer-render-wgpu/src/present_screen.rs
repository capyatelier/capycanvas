use crate::GpuRasterError;
use layer_core::color::rgb::Matrix3;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

pub(crate) const UNIFORM_SIZE: u64 = 64;
pub(crate) const COUNTS_SIZE: u64 = 64 * 4;
const WORKGROUP: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenCheck {
    pub from_view: Option<Matrix3>,
    pub surface_clips: bool,
    pub bounded: bool,
    pub mark: bool,
}

impl ScreenCheck {
    pub fn for_view(
        assessment: &layer_color::screen::ScreenAssessment,
        view: layer_core::color::RgbSpace,
        hdr_surface: bool,
        mark: bool,
    ) -> Option<Self> {
        (assessment.basis != layer_color::screen::Basis::Pending).then(|| Self {
            from_view: assessment.from_view(view),
            surface_clips: !hdr_surface,
            bounded: !hdr_surface,
            mark,
        })
    }

    pub(crate) fn uniform(check: Option<Self>) -> [f32; 16] {
        let Some(check) = check else { return [0.; 16] };
        let flag = |on: bool| if on { 1. } else { 0. };
        let rows = check.from_view.unwrap_or([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        let mut uniform = [0.; 16];
        for (row, values) in rows.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                uniform[row * 4 + column] = *value as f32;
            }
        }
        uniform[12..].copy_from_slice(&[
            flag(check.from_view.is_some()),
            flag(check.surface_clips),
            flag(check.mark),
            flag(check.bounded),
        ]);
        uniform
    }

    pub(crate) fn counts(uniform: &[f32; 16]) -> bool {
        uniform[12] != 0. || uniform[13] != 0.
    }
}

const IDLE: u8 = 0;
const MAPPING: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

pub(crate) struct ScreenCounter {
    pipeline: wgpu::ComputePipeline,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    counted: Option<Vec<u32>>,
    pending: Option<Vec<u32>>,
}

impl ScreenCounter {
    pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule) -> Self {
        Self {
            pipeline: device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("screen gamut count"),
                layout: Some(layout),
                module: shader,
                entry_point: Some("screen_count"),
                compilation_options: Default::default(),
                cache: None,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screen gamut readback"),
                size: COUNTS_SIZE,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(IDLE)),
            counted: None,
            pending: None,
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn current(&self, signature: &[u32]) -> bool {
        self.pending.as_deref() == Some(signature) || self.counted.as_deref() == Some(signature)
    }

    pub(crate) fn start(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        group: &wgpu::BindGroup,
        counts: &wgpu::Buffer,
        viewport: [u32; 2],
        signature: Vec<u32>,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("screen gamut count"),
        });
        encoder.clear_buffer(counts, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("screen gamut count"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, group, &[0]);
            pass.dispatch_workgroups(viewport[0].div_ceil(WORKGROUP), viewport[1].div_ceil(WORKGROUP), 1);
        }
        encoder.copy_buffer_to_buffer(counts, 0, &self.readback, 0, COUNTS_SIZE);
        queue.submit([encoder.finish()]);
        self.state.store(MAPPING, Ordering::Release);
        let state = self.state.clone();
        self.readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            state.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
        self.pending = Some(signature);
    }

    pub(crate) fn finish(&mut self) -> Option<Result<bool, GpuRasterError>> {
        match self.state.load(Ordering::Acquire) {
            MAPPED => {
                let clipped = self
                    .readback
                    .slice(..)
                    .get_mapped_range()
                    .map(|bytes| bytes.chunks_exact(4).any(|word| word != [0; 4]))
                    .map_err(|e| GpuRasterError::Color(e.to_string()));
                self.readback.unmap();
                self.state.store(IDLE, Ordering::Release);
                self.counted = self.pending.take();
                Some(clipped)
            }
            FAILED => {
                self.state.store(IDLE, Ordering::Release);
                self.pending = None;
                Some(Err(GpuRasterError::Color("Screen gamut readback failed".into())))
            }
            _ => None,
        }
    }
}
