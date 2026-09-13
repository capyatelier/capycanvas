//! Equal Float32 sampling/blending kernels. Run in release on an idle GPU.
//! Reports serialized CPU submission and GPU-completed wall time separately.
use std::time::{Duration, Instant};

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
        .ok_or("No requested adapter")?;
    println!("Adapter: {:?}", adapter.get_info());
    let features = adapter.features()
        & (wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::FLOAT32_FILTERABLE
            | wgpu::Features::FLOAT32_BLENDABLE);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        })
        .await?;
    println!("Features: {features:?}");
    println!(
        "Format | pixels | kernel | bytes (two textures) | submit p50/p95/p99 ms | completed p50/p95/p99 ms"
    );
    for size in [256, 4096] {
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Rgba16Unorm,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureFormat::Rgba32Float,
        ] {
            let caps = adapter.get_texture_format_features(format);
            let usage =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            if !caps.allowed_usages.contains(usage)
                || !caps
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
                || !features.contains(format.required_features())
            {
                println!("{format:?}: unsupported ({caps:?})");
                continue;
            }
            let textures: Vec<_> = (0..2)
                .map(|_| {
                    device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("working format kernel"),
                        size: wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                })
                .collect();
            let views: Vec<_> = textures
                .iter()
                .map(|t| t.create_view(&Default::default()))
                .collect();
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            for blend in [false, true] {
                if blend
                    && !caps
                        .flags
                        .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
                {
                    continue;
                }
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Float32 sample and blend"),
                    source: wgpu::ShaderSource::Wgsl(format!(r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var sampling: sampler;
@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {{
    let p=array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(p[i],0.,1.);
}}
@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {{
    let uv=(p.xy+vec2<f32>(.25,.75))/vec2<f32>(textureDimensions(source));
    let a=textureSampleLevel(source,sampling,uv,0.);
    let b=textureSampleLevel(source,sampling,uv+vec2<f32>(1.,-1.)/vec2<f32>(textureDimensions(source)),0.);
    let value=mix(a,b,.35)*.9999+vec4<f32>(.00001);
    return {};
}}
"#, if blend { "value*.03125" } else { "value" }).into()),
                });
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("working format"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                let groups: Vec<_> = views
                    .iter()
                    .map(|v| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &pipeline.get_bind_group_layout(0),
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(v),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&sampler),
                                },
                            ],
                        })
                    })
                    .collect();
                let mut submit = Vec::new();
                let mut completed = Vec::new();
                // 50 warmups, 300 samples; 16 physical passes per sample.
                for sample in 0..350 {
                    let start = Instant::now();
                    let mut encoder = device.create_command_encoder(&Default::default());
                    for pass_index in 0..16 {
                        let target = pass_index % 2;
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &views[target],
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: if sample == 0 {
                                        wgpu::LoadOp::Clear(wgpu::Color {
                                            r: 0.2,
                                            g: 0.3,
                                            b: 0.4,
                                            a: 1.,
                                        })
                                    } else {
                                        wgpu::LoadOp::Load
                                    },
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &groups[1 - target], &[]);
                        pass.draw(0..3, 0..1);
                    }
                    let submission = queue.submit([encoder.finish()]);
                    let cpu = start.elapsed().as_secs_f64() * 1000.;
                    device.poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(30)),
                    })?;
                    if sample >= 50 {
                        submit.push(cpu);
                        completed.push(start.elapsed().as_secs_f64() * 1000.);
                    }
                }
                let percentiles = |mut times: Vec<f64>| {
                    times.sort_by(f64::total_cmp);
                    format!("{:.4}/{:.4}/{:.4}", times[150], times[285], times[297])
                };
                println!(
                    "{format:?} | {size}² | {} | {} | {} | {}",
                    if blend {
                        "sample + source-over"
                    } else {
                        "sample"
                    },
                    u64::from(size)
                        * u64::from(size)
                        * u64::from(format.block_copy_size(None).unwrap())
                        * 2,
                    percentiles(submit),
                    percentiles(completed)
                );
            }
        }
    }
    Ok(())
}
