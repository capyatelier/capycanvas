//! Point/area sampling from artwork textures before view transforms. At most
//! 25 texels, one reusable buffer and one asynchronous request are resident.
//! No scene rebuild, shader, full-image readback or blocking wait is needed.
use super::*;
use layer_render::{ColorSample, ColorSampleRequest, ColorSampleSource};

pub(super) struct ColorSampler {
    buffer: Option<wgpu::Buffer>,
    tx: mpsc::Sender<Result<ColorSample, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ColorSample, GpuRasterError>>,
    pending: bool,
}
impl ColorSampler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            buffer: None,
            tx,
            rx,
            pending: false,
        }
    }
    pub fn take(&mut self) -> Option<Result<ColorSample, GpuRasterError>> {
        if !self.pending {
            return None;
        }
        let result = self.rx.try_recv().ok()?;
        self.pending = false;
        Some(result)
    }
}
impl WgpuRasterizer {
    pub fn color_sample_pending(&self) -> bool {
        self.color_sampler.pending
    }

    pub(super) fn start_color_sample(
        &mut self,
        request: ColorSampleRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.color_sampler.pending {
            return Ok(false);
        }
        let request_id = request.request_id;
        let [x, y] = request.position;
        let inside = x < self.document_extent[0] && y < self.document_extent[1];
        let paper = match request.source {
            ColorSampleSource::Layer(id) => self
                .thumbnails
                .paper
                .filter(|(paper, _)| *paper == id)
                .map(|(_, color)| color),
            ColorSampleSource::Composite => None,
        };
        if !inside || paper.is_some() {
            let _ = self.color_sampler.tx.send(Ok(ColorSample {
                request_id,
                rgba: if inside {
                    paper.unwrap_or([0.; 4])
                } else {
                    [0.; 4]
                },
            }));
            self.color_sampler.pending = true;
            return Ok(true);
        }
        let radius = request.area.width() / 2;
        let left = x.saturating_sub(radius);
        let top = y.saturating_sub(radius);
        let right = x.saturating_add(radius + 1).min(self.document_extent[0]);
        let bottom = y.saturating_add(radius + 1).min(self.document_extent[1]);
        let width = right - left;
        let count = width * (bottom - top);
        let buffer = self.color_sampler.buffer.get_or_insert_with(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bounded artwork color sample"),
                size: 128,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("artwork color sample"),
            },
        );
        // Sparse missing pages contribute transparent black, never old buffer
        // contents. Copy each contiguous row segment across page boundaries.
        encoder.clear_buffer(buffer, 0, None);
        for row in top..bottom {
            let mut column = left;
            while column < right {
                let (source, origin, end) = match request.source {
                    ColorSampleSource::Composite => {
                        (self.composite_texture.as_ref(), [column, row], right)
                    }
                    ColorSampleSource::Layer(id) => {
                        let coordinate = [column / PAGE_SIZE, row / PAGE_SIZE];
                        let source = self
                            .paint_layers
                            .iter()
                            .find(|l| l.id == id)
                            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
                            .map(|p| &p.active().texture);
                        (
                            source,
                            [column % PAGE_SIZE, row % PAGE_SIZE],
                            right.min((coordinate[0] + 1) * PAGE_SIZE),
                        )
                    }
                };
                if let Some(source) = source {
                    debug_assert_eq!(source.format(), COLOR_FORMAT);
                    encoder.copy_texture_to_buffer(
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d {
                                x: origin[0],
                                y: origin[1],
                                z: 0,
                            },
                            ..source.as_image_copy()
                        },
                        wgpu::TexelCopyBufferInfo {
                            buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: u64::from(((row - top) * width + column - left) * 4),
                                ..Default::default()
                            },
                        },
                        wgpu::Extent3d {
                            width: end - column,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                column = end;
            }
        }
        encoder.submit(&self.queue);
        let ready = buffer.clone();
        let tx = self.color_sampler.tx.clone();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let result = result
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                    .and_then(|_| {
                        let data = ready
                            .slice(..)
                            .get_mapped_range()
                            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                        let mut sum = [0.; 4];
                        for texel in data[..count as usize * 4].chunks_exact(4) {
                            let alpha = f32::from(texel[3]) / 255.;
                            if alpha > 0. {
                                for channel in 0..3 {
                                    sum[channel] += layer_core::color::srgb_decode(
                                        f32::from(texel[channel]) / 255.,
                                    );
                                }
                                sum[3] += alpha;
                            }
                        }
                        let rgba = if sum[3] > 0. {
                            [
                                sum[0] / sum[3],
                                sum[1] / sum[3],
                                sum[2] / sum[3],
                                sum[3] / count as f32,
                            ]
                        } else {
                            [0.; 4]
                        };
                        Ok(ColorSample { request_id, rgba })
                    });
                ready.unmap();
                let _ = tx.send(result);
            });
        self.color_sampler.pending = true;
        Ok(true)
    }
}
