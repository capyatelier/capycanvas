//! Equal Float32 arithmetic through different physical working formats. This
//! selects a precision strategy; complete renderer operations need separate tests.
use layer_core::color::RgbSpace;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[path = "sdr_precision/tiles.rs"]
mod tiles;

const SIZE: u32 = 256;
const COUNT: usize = 65536;
const SHARED: &str = include_str!("../src/sdr_color.wgsl");
const VERTEX: &str = r#"
@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    return vec4<f32>(p[i],0.,1.);
}
@group(0) @binding(0) var source: texture_2d<f32>;
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
    let native_tiles = std::env::args().nth(1).as_deref() == Some("tiles");
    let features = if native_tiles {
        wgpu::Features::empty()
    } else {
        wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::FLOAT32_BLENDABLE
    };
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        })
        .await?;
    println!(
        "Adapter: {:?}; requested features: {features:?}",
        adapter.get_info()
    );
    if native_tiles {
        return tiles::run(&device, &queue);
    }
    let encoded = texture(&device, wgpu::TextureFormat::Rgba16Unorm);
    let output = texture(&device, wgpu::TextureFormat::Rgba16Unorm);
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("one precision tile readback"),
        size: (COUNT * 8) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    println!(
        "Working | profile | operation | max RGB code error | RMS code error | changed RGB codes | alpha max error | complete p95/p99 ms"
    );
    for working in [
        wgpu::TextureFormat::Rgba16Unorm,
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Rgba32Float,
    ] {
        let a = texture(&device, working);
        let b = texture(&device, working);
        for (space_id, space) in RgbSpace::ALL.into_iter().enumerate() {
            let mut blend_reference = None::<Vec<u8>>;
            for operation in [
                "identity",
                "exposure_chain",
                "low_alpha_over",
                "fixed_blend_over",
                "fixed_blend_batched",
                "resampling",
            ] {
                let fixed_blend = operation.starts_with("fixed_blend");
                let batched = operation == "fixed_blend_batched";
                let low_alpha = operation == "low_alpha_over" || fixed_blend;
                let source: Vec<[u16; 4]> = (0..COUNT)
                    .map(|i| {
                        if operation == "identity" || operation == "exposure_chain" {
                            [i as u16; 4]
                        } else {
                            [
                                i as u16,
                                (i as u16).wrapping_mul(617),
                                (i as u16).wrapping_mul(251),
                                65535,
                            ]
                        }
                    })
                    .map(|mut p| {
                        p[3] = if low_alpha { 257 } else { 65535 };
                        p
                    })
                    .collect();
                let bytes: Vec<_> = source
                    .iter()
                    .flatten()
                    .flat_map(|v| v.to_le_bytes())
                    .collect();
                queue.write_texture(
                    encoded.as_image_copy(),
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE * 8),
                        rows_per_image: Some(SIZE),
                    },
                    encoded.size(),
                );
                let decode = pipeline(
                    &device,
                    working,
                    None,
                    &format!(
                        "{SHARED}\n{VERTEX}\n@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {{\nlet v=textureLoad(source,vec2<i32>(p.xy),0); return vec4<f32>(sdr_decode(v.rgb,{space_id}u)*v.a,v.a); }}"
                    ),
                );
                let edit_body = match operation {
                    "identity" => "return v;",
                    "exposure_chain" => "return vec4<f32>(v.rgb * 1.0001, v.a);",
                    "low_alpha_over" => {
                        "let a=1.0/65535.0; return vec4<f32>(vec3<f32>(0.25,0.5,0.75)*a+v.rgb*(1.0-a),a+v.a*(1.0-a));"
                    }
                    "fixed_blend_over" | "fixed_blend_batched" => {
                        "let a=1.0/65535.0; return vec4<f32>(vec3<f32>(0.25,0.5,0.75)*a,a);"
                    }
                    "resampling" => {
                        "let right=textureLoad(source,vec2<i32>((i.x+1)%256,i.y),0); return mix(v,right,0.375);"
                    }
                    _ => unreachable!(),
                };
                let edit = pipeline(
                    &device,
                    working,
                    fixed_blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    &format!(
                        "{VERTEX}\n@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {{let i=vec2<i32>(p.xy);let v=textureLoad(source,i,0);{edit_body}}}"
                    ),
                );
                let encode = pipeline(
                    &device,
                    wgpu::TextureFormat::Rgba16Unorm,
                    None,
                    &format!(
                        "{SHARED}\n{VERTEX}\n@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {{let v=textureLoad(source,vec2<i32>(p.xy),0);let straight=v.rgb/max(v.a,1e-20); return vec4<f32>(sdr_encode(straight,{space_id}u),v.a);}}"
                    ),
                );
                let n = if operation == "identity" {
                    0
                } else if operation == "resampling" {
                    1
                } else {
                    64
                };
                let mut times = Vec::new();
                for sample in 0..110 {
                    let start = Instant::now();
                    let mut encoder = device.create_command_encoder(&Default::default());
                    draw(&device, &mut encoder, &decode, &encoded, &a);
                    if batched {
                        draw_with_load(
                            &device,
                            &mut encoder,
                            &edit,
                            &encoded,
                            &a,
                            wgpu::LoadOp::Load,
                            n as u32,
                        );
                    }
                    for pass in 0..if batched { 0 } else { n } {
                        if fixed_blend {
                            draw_with_load(
                                &device,
                                &mut encoder,
                                &edit,
                                &encoded,
                                &a,
                                wgpu::LoadOp::Load,
                                1,
                            );
                            continue;
                        }
                        draw(
                            &device,
                            &mut encoder,
                            &edit,
                            if pass % 2 == 0 { &a } else { &b },
                            if pass % 2 == 0 { &b } else { &a },
                        );
                    }
                    draw(
                        &device,
                        &mut encoder,
                        &encode,
                        if fixed_blend || n % 2 == 0 { &a } else { &b },
                        &output,
                    );
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
                encoder.copy_texture_to_buffer(
                    output.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &staging,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(SIZE * 8),
                            rows_per_image: Some(SIZE),
                        },
                    },
                    output.size(),
                );
                queue.submit([encoder.finish()]);
                let (tx, rx) = mpsc::channel();
                staging.map_async(wgpu::MapMode::Read, .., move |r| {
                    let _ = tx.send(r);
                });
                device.poll(wgpu::PollType::wait_indefinitely())?;
                rx.recv()??;
                let mapped = staging.get_mapped_range(..)?;
                let mut max_rgb = 0i32;
                let mut max_alpha = 0i32;
                let mut sum = 0f64;
                let mut changed = 0;
                let mut blend_difference = 0;
                for (i, pixel) in mapped.chunks_exact(8).enumerate() {
                    let mut expected = source[i].map(|v| f64::from(v) / 65535.);
                    let alpha = expected[3];
                    let mut linear =
                        [expected[0], expected[1], expected[2]].map(|v| space.decode(v) * alpha);
                    if operation == "exposure_chain" {
                        linear = linear.map(|v| v * 1.0001f64.powi(n));
                    }
                    if low_alpha {
                        let keep = (1. - 1. / 65535f64).powi(n);
                        for c in 0..3 {
                            linear[c] = linear[c] * keep + [0.25, 0.5, 0.75][c] * (1. - keep);
                        }
                        expected[3] = 1. - (1. - alpha) * keep;
                    }
                    if operation == "resampling" {
                        let right = source[i / 256 * 256 + (i % 256 + 1) % 256];
                        for c in 0..3 {
                            linear[c] = linear[c] * 0.625
                                + space.decode(f64::from(right[c]) / 65535.) * 0.375;
                        }
                    }
                    for c in 0..3 {
                        expected[c] = space.encode(linear[c] / expected[3]);
                    }
                    for (c, code) in pixel.chunks_exact(2).enumerate() {
                        let actual = i32::from(u16::from_le_bytes([code[0], code[1]]));
                        if fixed_blend && let Some(reference) = &blend_reference {
                            let offset = i * 8 + c * 2;
                            let reference = i32::from(u16::from_le_bytes([
                                reference[offset],
                                reference[offset + 1],
                            ]));
                            blend_difference = blend_difference.max((actual - reference).abs());
                        }
                        let reference = (expected[c].clamp(0., 1.) * 65535.).round() as i32;
                        let error = (actual - reference).abs();
                        if c < 3 {
                            max_rgb = max_rgb.max(error);
                            sum += f64::from(error).powi(2);
                            changed += u64::from(error != 0);
                        } else {
                            max_alpha = max_alpha.max(error);
                        }
                    }
                }
                if working == wgpu::TextureFormat::Rgba32Float && operation == "low_alpha_over" {
                    blend_reference = Some(mapped.to_vec());
                }
                drop(mapped);
                staging.unmap();
                times.sort_by(f64::total_cmp);
                println!(
                    "{working:?} | {space:?} | {operation} | {max_rgb} | {:.4} | {changed} | {max_alpha} | {:.4}/{:.4}",
                    (sum / (COUNT * 3) as f64).sqrt(),
                    times[95],
                    times[99]
                );
                if working == wgpu::TextureFormat::Rgba32Float && (max_rgb > 2 || max_alpha > 1) {
                    return Err(
                        "Float32 kernel exceeded declared 2 RGB / 1 alpha code tolerance".into(),
                    );
                }
                if fixed_blend && blend_reference.is_some() {
                    println!(
                        "Float32 {operation} vs manual physical passes: max code difference {blend_difference}"
                    );
                    if blend_difference > 1 {
                        return Err(
                            "Fixed/blended pass partition exceeded 1 code difference".into()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn texture(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bounded SDR precision tile"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("SDR precision kernel"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
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
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
fn draw(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    source: &wgpu::Texture,
    target: &wgpu::Texture,
) {
    draw_with_load(
        device,
        encoder,
        pipeline,
        source,
        target,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        1,
    );
}
fn draw_with_load(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    source: &wgpu::Texture,
    target: &wgpu::Texture,
    load: wgpu::LoadOp<wgpu::Color>,
    instances: u32,
) {
    let view = target.create_view(&Default::default());
    let input = source.create_view(&Default::default());
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&input),
        }],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.draw(0..3, 0..instances);
}
