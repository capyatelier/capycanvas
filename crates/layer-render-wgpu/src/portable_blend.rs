//! Float32 blending for devices without fixed-function float attachment blends.
//! Scratch belongs to one renderer; captures never share mutable GPU resources.
use super::*;
use std::sync::Mutex;
use wgpu::util::DeviceExt;

pub(crate) struct Pipelines {
    layouts: [wgpu::BindGroupLayout; 2],
    pub pipelines: [Deferred<wgpu::ComputePipeline>; 2],
}
impl Pipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let layouts = std::array::from_fn(|i| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("portable Float32 blend"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: if i == 0 {
                                wgpu::TextureFormat::Rgba32Float
                            } else {
                                wgpu::TextureFormat::R32Float
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            })
        });
        let pipelines = std::array::from_fn(|i| {
            let device = device.clone();
            let bind = layouts[i].clone();
            Deferred::pipeline(move |mode| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("portable Float32 blend"),
                    source: wgpu::ShaderSource::Wgsl(
                        include_str!("portable_blend.wgsl")
                            .replace(
                                "OUTPUT_FORMAT",
                                if i == 0 { "rgba32float" } else { "r32float" },
                            )
                            .into(),
                    ),
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("portable Float32 blend"),
                    bind_group_layouts: &[Some(&bind)],
                    immediate_size: 0,
                });
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some("portable Float32 blend"),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some("blend"),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
        });
        Self { layouts, pipelines }
    }
}
pub(super) struct Renderer {
    shared: Arc<Pipelines>,
    scratch: Mutex<Vec<(u8, wgpu::Texture)>>,
}
impl std::ops::Deref for Renderer {
    type Target = Pipelines;
    fn deref(&self) -> &Pipelines {
        &self.shared
    }
}
impl Renderer {
    pub fn new(device: &PipelineDevice) -> Self {
        let mut cached = device.blend_pipelines.lock().unwrap();
        let shared = cached.upgrade().unwrap_or_else(|| {
            let pipelines = Arc::new(Pipelines::new(device));
            *cached = Arc::downgrade(&pipelines);
            pipelines
        });
        Self {
            shared,
            scratch: Default::default(),
        }
    }
    fn texture(
        &self,
        device: &PipelineDevice,
        slot: u8,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        let size = target.texture().size();
        let mut scratch = self.scratch.lock().unwrap();
        if let Some((_, texture)) = scratch
            .iter()
            .find(|(s, t)| *s == slot && t.size() == size && t.format() == format)
        {
            return texture.clone();
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("portable blend scratch"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        // A renderer uses at most one extent per slot/format at a time. Encoded
        // commands retain replaced resources until their submissions complete.
        scratch.retain(|(s, t)| *s != slot || t.format() != format);
        scratch.push((slot, texture.clone()));
        texture
    }
    pub fn source(
        &self,
        device: &PipelineDevice,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        self.texture(device, 0, target, format)
            .create_view(&Default::default())
    }
    pub fn byte_len(&self) -> u64 {
        self.scratch
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| {
                u64::from(t.width())
                    * u64::from(t.height())
                    * u64::from(t.format().block_copy_size(None).unwrap())
            })
            .sum()
    }
    pub fn apply(
        &self,
        device: &PipelineDevice,
        encoder: &mut submission::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        rect: PixelRect,
        mode: u32,
    ) {
        let old = self.texture(device, 1, target, target.texture().format());
        let origin = wgpu::Origin3d {
            x: rect.min_x(),
            y: rect.min_y(),
            z: 0,
        };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                origin,
                ..target.texture().as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin,
                ..old.as_image_copy()
            },
            wgpu::Extent3d {
                width: rect.width(),
                height: rect.height(),
                depth_or_array_layers: 1,
            },
        );
        let params = [
            rect.min_x(),
            rect.min_y(),
            rect.width(),
            rect.height(),
            mode,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("portable blend region"),
            contents: params.map(u32::to_le_bytes).as_flattened(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let index = usize::from(target.texture().format() == wgpu::TextureFormat::R32Float);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("portable Float32 blend"),
            layout: &self.layouts[index],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &old.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(target),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipelines[index]);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(rect.width().div_ceil(8), rect.height().div_ceil(8), 1);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn shader_blends_preserve_float32_values_and_unaffected_pixels_without_hardware_blending() {
        // The legacy SDR device does not request Float32 attachment blending,
        // even on a GPU that supports it. This exercises the portable contract.
        let r = WgpuRasterizer::new_headless().unwrap();
        assert!(
            !r.device
                .features()
                .contains(wgpu::Features::FLOAT32_BLENDABLE)
        );
        let device = r
            .device
            .clone()
            .with_working_format(wgpu::TextureFormat::Rgba32Float)
            .unwrap();
        let a = Renderer::new(&device);
        let b = Renderer::new(&device);
        assert!(Arc::ptr_eq(&a.shared, &b.shared));
        let size = [8, 4];
        let rect = PixelRect::new(1, 1, 7, 3);
        let source: Vec<[f32; 4]> = (0..32)
            .map(|i| [4., -0.5, 1e20, (i % 5) as f32 / 4.])
            .collect();
        let (src, src_view) = create_target(
            &device,
            size,
            wgpu::TextureFormat::Rgba32Float,
            "blend test source",
        );
        let upload = |texture: &wgpu::Texture, values: &[f32]| {
            let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            r.queue.write_texture(
                texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(8 * texture.format().block_copy_size(None).unwrap()),
                    rows_per_image: Some(4),
                },
                texture.size(),
            );
        };
        upload(&src, source.as_flattened());
        for format in [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ] {
            let channels = if format == wgpu::TextureFormat::R32Float {
                1
            } else {
                4
            };
            let old = [-2., 0.75, -1e20, 0.5];
            let original: Vec<_> = (0..32)
                .flat_map(|_| old[..channels].iter().copied())
                .collect();
            for mode in 0..3 {
                let (target, view) = create_target(&device, size, format, "blend test destination");
                upload(&target, &original);
                let output = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("blend test readback"),
                    size: 4 * 256,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
                a.apply(&device, &mut encoder, &src_view, &view, rect, mode);
                encoder.copy_texture_to_buffer(
                    target.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &output,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(4),
                        },
                    },
                    target.size(),
                );
                encoder.submit(&r.queue);
                let (tx, rx) = std::sync::mpsc::channel();
                output
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result.map_err(|e| e.to_string()));
                    });
                crate::raster::wait_mapping(&device, &rx).unwrap();
                let bytes = output.slice(..).get_mapped_range().unwrap();
                for y in 0..4 {
                    for x in 0..8 {
                        for c in 0..channels {
                            let src = source[y * 8 + x];
                            let expected = if !(1..7).contains(&x) || !(1..3).contains(&y) {
                                old[c]
                            } else {
                                match mode {
                                    0 => src[c] + old[c] * (1. - src[3]),
                                    1 => old[c] * (1. - src[3]),
                                    _ => src[c].max(old[c]),
                                }
                            };
                            let offset = y * 256 + (x * channels + c) * 4;
                            let actual =
                                f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                            assert!(
                                (actual - expected).abs() <= 1e-6 * expected.abs().max(1.),
                                "{format:?} mode={mode} ({x},{y},{c}) {actual} != {expected}"
                            );
                        }
                    }
                }
                drop(bytes);
                output.unmap();
                let first = a.source(&device, &view, wgpu::TextureFormat::Rgba32Float);
                let second = b.source(&device, &view, wgpu::TextureFormat::Rgba32Float);
                assert_ne!(
                    first.texture(),
                    second.texture(),
                    "capture scratch must be private"
                );
            }
        }
    }
}
