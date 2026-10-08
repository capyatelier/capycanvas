use crate::GpuRasterError;
use layer_core::color::rgb::Matrix3;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

pub(crate) const UNIFORM_SIZE: u64 = 64;
const COUNTS_SIZE: u64 = 64 * 4;
const WORKGROUP: u32 = 8;
pub(crate) const SAMPLE_STRIDE: u32 = 4;

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
    pub(crate) pipeline: crate::deferred::Deferred<wgpu::ComputePipeline>,
    counts: wgpu::Buffer,
    group: wgpu::BindGroup,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    counted: Option<Vec<u32>>,
    pending: Option<Vec<u32>>,
}

impl ScreenCounter {
    pub(crate) fn new(device: &crate::PipelineDevice, viewport: &wgpu::BindGroupLayout, shader: &wgpu::ShaderModule) -> Self {
        let layout = crate::bindings::layout(device, "screen gamut counts", &[crate::bindings::buffer(
            0,
            wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Storage { read_only: false },
            false,
            wgpu::BufferSize::new(COUNTS_SIZE),
        )]);
        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen gamut counts"),
            size: COUNTS_SIZE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen gamut count"),
            bind_group_layouts: &[Some(viewport), Some(&layout)],
            immediate_size: 0,
        });
        let (compiler, module) = (device.clone(), shader.clone());
        Self {
            pipeline: crate::deferred::Deferred::pipeline(move |mode| {
                mode.compute(&compiler, &wgpu::ComputePipelineDescriptor {
                    label: Some("screen gamut count"),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some("screen_count"),
                    compilation_options: Default::default(),
                    cache: None,
                })
            }),
            group: crate::bindings::group(device, "screen gamut counts", &layout, [counts.as_entire_binding()]),
            counts,
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
        viewport: &wgpu::BindGroup,
        extent: [u32; 2],
        signature: Vec<u32>,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("screen gamut count"),
        });
        encoder.clear_buffer(&self.counts, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("screen gamut count"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, viewport, &[0]);
            pass.set_bind_group(1, &self.group, &[]);
            pass.dispatch_workgroups(
                extent[0].div_ceil(SAMPLE_STRIDE * WORKGROUP),
                extent[1].div_ceil(SAMPLE_STRIDE * WORKGROUP),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&self.counts, 0, &self.readback, 0, COUNTS_SIZE);
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
                    .map(|bytes| bytes.as_chunks::<4>().0.iter().any(|word| *word != [0; 4]))
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
