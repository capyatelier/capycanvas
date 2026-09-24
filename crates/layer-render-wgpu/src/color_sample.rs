//! Point/area sampling from artwork textures before view transforms. At most
//! 10,201 texels, one reusable buffer and one asynchronous request are resident.
//! Source-backed pixels use the bounded working cache; no paint materialization
//! or full-image readback is needed.
use super::*;
use layer_render::{ColorSample, ColorSampleRequest, ColorSampleSource};

pub(super) struct ColorSampler {
    capture: artwork::Capture,
    buffer: Option<wgpu::Buffer>,
    tx: mpsc::Sender<Result<ColorSample, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ColorSample, GpuRasterError>>,
    pending: bool,
}
impl ColorSampler {
    pub(super) fn storage_bytes(&self) -> u64 {
        self.buffer.as_ref().map_or(0, |b| b.size()) + self.capture.storage_bytes()
    }
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            capture: Default::default(),
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
        let extent = match request.source { ColorSampleSource::Layer(id) => self.target_extent(id), _ => self.document_extent };
        let inside = x < extent[0] && y < extent[1];
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
        let right = x.saturating_add(radius + 1).min(extent[0]);
        let bottom = y.saturating_add(radius + 1).min(extent[1]);
        let width = right - left;
        let count = width * (bottom - top);
        let stride = if matches!(request.source, ColorSampleSource::Layer(id) if self.tiled_sources.contains_key(&id))
        {
            16
        } else {
            self.device.working_format().block_copy_size(None).unwrap()
        };
        let capacity = u64::from(count * stride).div_ceil(256) * 256;
        if self
            .color_sampler
            .buffer
            .as_ref()
            .is_some_and(|b| b.size() < capacity)
        {
            self.color_sampler.buffer = None;
        }
        let buffer = self
            .color_sampler
            .buffer
            .get_or_insert_with(|| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bounded artwork color sample"),
                    size: capacity,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .clone();
        let mut formats = vec![false; count as usize];
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("artwork color sample"),
            },
        );
        // Sparse missing pages contribute transparent black, never old buffer
        // contents. Copy each contiguous row segment across page boundaries.
        encoder.clear_buffer(&buffer, 0, None);
        let composite = if request.source == ColorSampleSource::Composite {
            let frame = self
                .artwork_frame
                .clone()
                .ok_or(GpuRasterError::InvalidExtent)?;
            let mut capture = mem::take(&mut self.color_sampler.capture);
            let region = PixelRect::new(left, top, right, bottom);
            let result = capture.region(
                self,
                frame.packet(self.document_extent),
                region,
                [width, bottom - top],
                &mut encoder,
            );
            self.color_sampler.capture = capture;
            Some(result?.texture)
        } else {
            None
        };
        for row in top..bottom {
            let mut column = left;
            while column < right {
                let (source, origin, mut end) = match request.source {
                    ColorSampleSource::Composite => {
                        (composite.clone(), [column - left, row - top], right)
                    }
                    ColorSampleSource::Layer(id) => {
                        let coordinate = [column / PAGE_SIZE, row / PAGE_SIZE];
                        let source = self
                            .raw_layer_tile(id, coordinate, &mut encoder)?
                            .map(|tile| tile.texture);
                        (
                            source,
                            [column % PAGE_SIZE, row % PAGE_SIZE],
                            right.min((coordinate[0] + 1) * PAGE_SIZE),
                        )
                    }
                };
                if let Some(source) = source {
                    let float = source.format() == wgpu::TextureFormat::Rgba32Float;
                    let bytes = source.format().block_copy_size(None).unwrap();
                    // An area may cross edited integer paint and untouched
                    // Float32 source. Pad integer texels individually in that case.
                    if bytes != stride {
                        end = column + 1;
                    }
                    let index = ((row - top) * width + column - left) as usize;
                    formats[index..index + (end - column) as usize].fill(float);
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
                            buffer: &buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: u64::from(((row - top) * width + column - left) * stride),
                                // Metal needs an explicit row pitch even for these
                                // single-row copies. Offsets pack at most 10,201 texels.
                                bytes_per_row: Some((width * stride).div_ceil(256) * 256),
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
        self.uploads.finish(&encoder);
        encoder.submit(&self.queue);
        let ready = buffer.clone();
        let tx = self.color_sampler.tx.clone();
        let space = self.document_color.space;
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
                        let mut sum = [0f64; 4];
                        let mut weight = 0.;
                        let perceptual = request.area.perceptual();
                        let to_srgb = space.linear_transform(layer_core::color::RgbSpace::Srgb);
                        let from_srgb = layer_core::color::RgbSpace::Srgb.linear_transform(space);
                        for (i, texel) in data[..count as usize * stride as usize]
                            .chunks_exact(stride as usize)
                            .enumerate()
                        {
                            let dx = (left + i as u32 % width) as f64 - x as f64;
                            let dy = (top + i as u32 / width) as f64 - y as f64;
                            if perceptual && dx * dx + dy * dy > (request.area.width() as f64 * 0.5).powi(2) {
                                continue;
                            }
                            weight += 1.;
                            let color: [f32; 4] = if formats[i] {
                                std::array::from_fn(|c| {
                                    f32::from_ne_bytes(texel[c * 4..c * 4 + 4].try_into().unwrap())
                                })
                            } else {
                                [
                                    layer_core::color::srgb_decode(f32::from(texel[0]) / 255.),
                                    layer_core::color::srgb_decode(f32::from(texel[1]) / 255.),
                                    layer_core::color::srgb_decode(f32::from(texel[2]) / 255.),
                                    f32::from(texel[3]) / 255.,
                                ]
                            };
                            if color[3] > 0. {
                                let alpha = color[3] as f64;
                                if perceptual {
                                    let rgb = [color[0], color[1], color[2]].map(|v| v as f64 / alpha);
                                    let lab = layer_core::color::oklab::to_lab(layer_core::color::rgb::apply(to_srgb, rgb));
                                    for c in 0..3 { sum[c] += lab[c] * alpha; }
                                    sum[3] += alpha;
                                } else {
                                    for c in 0..4 { sum[c] += color[c] as f64; }
                                }
                            }
                        }
                        let rgba = if sum[3] > 0. {
                            let mut rgb = [sum[0], sum[1], sum[2]].map(|v| v / sum[3]);
                            if perceptual {
                                rgb = layer_core::color::rgb::apply(from_srgb, layer_core::color::oklab::from_lab(rgb));
                            }
                            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, (sum[3] / weight) as f32]
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
