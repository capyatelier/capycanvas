//! Fractional bilinear interpolation through a physical Float32 texture.
//! Separates hardware filter weight precision from storage/math precision.
use layer_core::color::RgbSpace;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const SIDE: u32 = 256;
const COUNT: usize = (SIDE * SIDE) as usize;
const SHADER: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<storage, read_write> result: array<vec4<f32>>;

// Integer coordinates denote texel centers. Clamp-to-edge, matching the sampler.
fn explicit_linear(p: vec2<f32>) -> vec4<f32> {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let maximum = vec2<i32>(textureDimensions(source)) - vec2<i32>(1);
    let a = textureLoad(source, clamp(i, vec2<i32>(0), maximum), 0);
    let b = textureLoad(source, clamp(i + vec2<i32>(1, 0), vec2<i32>(0), maximum), 0);
    let c = textureLoad(source, clamp(i + vec2<i32>(0, 1), vec2<i32>(0), maximum), 0);
    let d = textureLoad(source, clamp(i + vec2<i32>(1, 1), vec2<i32>(0), maximum), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
@compute @workgroup_size(64)
fn hardware(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let fraction = vec2<f32>(f32(i), f32((i * 251u) % 65536u)) / 65535.0;
    result[i] = textureSampleLevel(source, linear_sampler, (fraction + 0.5) / 2.0, 0.0);
}
@compute @workgroup_size(64)
fn manual(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let fraction = vec2<f32>(f32(i), f32((i * 251u) % 65536u)) / 65535.0;
    result[i] = explicit_linear(fraction);
}
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pollster::block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let index = std::env::var("LAYER_GPU_INDEX")
        .unwrap_or("0".into())
        .parse::<usize>()?;
    let adapter = instance
        .enumerate_adapters(wgpu::Backends::PRIMARY)
        .await
        .into_iter()
        .nth(index)
        .ok_or("No adapter")?;
    println!("Adapter: {:?}", adapter.get_info());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::FLOAT32_FILTERABLE,
            ..Default::default()
        })
        .await?;
    let source = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("four reference pixels"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let result = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fractional sampling results"),
        size: (COUNT * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sampling readback"),
        size: result.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let view = source.create_view(&Default::default());
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: result.as_entire_binding(),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("SDR fractional sampling precision"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipelines = ["hardware", "manual"].map(|name| {
        (
            name,
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            }),
        )
    });
    println!(
        "method | space | fixture | max RGB code error | RMS RGB code error | alpha code error | max linear error | completed p95/p99 ms"
    );
    for space in RgbSpace::ALL {
        for (name, codes) in [
            (
                "opaque_edge",
                [
                    [0, 0, 0, 65535],
                    [65535, 65535, 65535, 65535],
                    [0, 0, 0, 65535],
                    [65535, 65535, 65535, 65535],
                ],
            ),
            (
                "four_colors",
                [
                    [1200, 65535, 300, 65535],
                    [65000, 22, 22000, 65535],
                    [9000, 16000, 42000, 65535],
                    [65535, 40000, 0, 65535],
                ],
            ),
            (
                "low_alpha",
                [
                    [1200, 65535, 300, 1],
                    [65000, 22, 22000, 13],
                    [9000, 16000, 42000, 17],
                    [65535, 40000, 0, 257],
                ],
            ),
            (
                "transparent_edge",
                [
                    [65535, 0, 65535, 0],
                    [65535, 65535, 65535, 65535],
                    [65535, 0, 65535, 0],
                    [65535, 65535, 65535, 65535],
                ],
            ),
        ] {
            let pixels = codes.map(|v| {
                let alpha = f64::from(v[3]) / 65535.;
                [
                    space.decode(f64::from(v[0]) / 65535.) * alpha,
                    space.decode(f64::from(v[1]) / 65535.) * alpha,
                    space.decode(f64::from(v[2]) / 65535.) * alpha,
                    alpha,
                ]
            });
            let bytes: Vec<_> = pixels
                .iter()
                .flatten()
                .flat_map(|v| (*v as f32).to_le_bytes())
                .collect();
            queue.write_texture(
                source.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(32),
                    rows_per_image: Some(2),
                },
                source.size(),
            );
            for (method, pipeline) in &pipelines {
                let mut times = Vec::new();
                for sample in 0..110 {
                    let start = Instant::now();
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &binding, &[]);
                        pass.dispatch_workgroups(COUNT as u32 / 64, 1, 1);
                    }
                    let submitted = queue.submit([encoder.finish()]);
                    device.poll(wgpu::PollType::Wait {
                        submission_index: Some(submitted),
                        timeout: Some(Duration::from_secs(30)),
                    })?;
                    if sample >= 10 {
                        times.push(start.elapsed().as_secs_f64() * 1000.);
                    }
                }
                let mut encoder = device.create_command_encoder(&Default::default());
                encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, result.size());
                queue.submit([encoder.finish()]);
                let (tx, rx) = mpsc::channel();
                readback.map_async(wgpu::MapMode::Read, .., move |r| {
                    let _ = tx.send(r);
                });
                device.poll(wgpu::PollType::wait_indefinitely())?;
                rx.recv()??;
                let mapped = readback.get_mapped_range(..)?;
                let (mut max_rgb, mut max_alpha, mut sum, mut max_linear) = (0, 0, 0., 0f64);
                for (i, bytes) in mapped.chunks_exact(16).enumerate() {
                    let x = i as f64 / 65535.;
                    let y = ((i * 251) % 65536) as f64 / 65535.;
                    let reference: [f64; 4] = std::array::from_fn(|c| {
                        (pixels[0][c] * (1. - x) + pixels[1][c] * x) * (1. - y)
                            + (pixels[2][c] * (1. - x) + pixels[3][c] * x) * y
                    });
                    let actual: [f64; 4] = std::array::from_fn(|c| {
                        f64::from(f32::from_le_bytes(
                            bytes[c * 4..c * 4 + 4].try_into().unwrap(),
                        ))
                    });
                    for c in 0..4 {
                        max_linear = max_linear.max((actual[c] - reference[c]).abs());
                        let code = |p: [f64; 4]| -> i32 {
                            let v = if c == 3 {
                                p[3]
                            } else if p[3] > 0. {
                                space.encode(p[c] / p[3])
                            } else {
                                0.
                            };
                            (v.clamp(0., 1.) * 65535.).round() as i32
                        };
                        let error = (code(actual) - code(reference)).abs();
                        if c < 3 {
                            max_rgb = max_rgb.max(error);
                            sum += f64::from(error).powi(2);
                        } else {
                            max_alpha = max_alpha.max(error);
                        }
                    }
                }
                drop(mapped);
                readback.unmap();
                times.sort_by(f64::total_cmp);
                println!(
                    "{method} | {space:?} | {name} | {max_rgb} | {:.4} | {max_alpha} | {max_linear:.9} | {:.4}/{:.4}",
                    (sum / (COUNT * 3) as f64).sqrt(),
                    times[95],
                    times[99]
                );
                if *method == "manual" && (max_rgb > 2 || max_alpha > 1) {
                    return Err(
                        "Explicit Float32 sampling exceeded 2 RGB / 1 alpha code tolerance".into(),
                    );
                }
            }
        }
    }
    Ok(())
}
