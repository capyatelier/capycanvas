//! Point sampling copies four bytes from an existing GPU texture. No shader,
//! scene rebuild, full-image readback, blocking wait or per-request GPU allocation.
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
        let mut point = request.position;
        let inside = point[0] < self.document_extent[0] && point[1] < self.document_extent[1];
        let mut empty = [0.0; 4];
        let source = if inside {
            match request.source {
                ColorSampleSource::Composite => self.composite_texture.as_ref(),
                ColorSampleSource::Layer(id) => {
                    if let Some((_, color)) =
                        self.thumbnails.paper.filter(|(paper, _)| *paper == id)
                    {
                        empty = color;
                    }
                    let coordinate = point.map(|v| v / PAGE_SIZE);
                    point = point.map(|v| v % PAGE_SIZE);
                    self.paint_layers
                        .iter()
                        .find(|l| l.id == id)
                        .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
                        .map(|p| &p.active().texture)
                }
            }
        } else {
            None
        };
        let request_id = request.request_id;
        if let Some(source) = source {
            debug_assert_eq!(source.format(), wgpu::TextureFormat::Rgba8Unorm);
            let buffer = self.color_sampler.buffer.get_or_insert_with(|| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("single color sample"),
                    size: 4,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("color sample"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: point[0],
                        y: point[1],
                        z: 0,
                    },
                    ..source.as_image_copy()
                },
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout::default(),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);
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
                            let alpha = f32::from(data[3]);
                            let rgba = if alpha == 0.0 {
                                [0.0; 4]
                            } else {
                                [
                                    f32::from(data[0]) / alpha,
                                    f32::from(data[1]) / alpha,
                                    f32::from(data[2]) / alpha,
                                    alpha / 255.0,
                                ]
                            };
                            Ok(ColorSample { request_id, rgba })
                        });
                    ready.unmap();
                    let _ = tx.send(result);
                });
        } else {
            let _ = self.color_sampler.tx.send(Ok(ColorSample {
                request_id,
                rgba: empty,
            }));
        }
        self.color_sampler.pending = true;
        Ok(true)
    }
}
