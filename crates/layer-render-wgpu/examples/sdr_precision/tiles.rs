//! Native straight integer tiles through physical Float32 working boundaries.
//! This measures the codec choice before it becomes the durable edit contract.
use super::*;
use layer_core::color::SampleDepth;

const SHADER: &str = r#"
struct Transfer { info:vec4<u32>, entries:array<vec2<f32>> }
@group(1) @binding(0) var<storage,read> transfer:Transfer;
override method:u32=0u;
@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(p[i],0.,1.);
}
fn boundary(code:u32)->f32 {
    return transfer.entries[code*transfer.info.z+(transfer.info.z-1u)/2u].y;
}
fn binary(value:f32)->u32 {
    var low=0u; var high=transfer.info.y;
    for(var step=0u;step<16u && low<high;step++) {
        let mid=(low+high)/2u;
        if value>=boundary(mid) {low=mid+1u;} else {high=mid;}
    }
    return low;
}
fn bracket(value:f32,code:u32)->bool {
    if code>0u && value<boundary(code-1u) {return false;}
    return code==transfer.info.y || value<boundary(code);
}
fn quantize(value:f32)->u32 {
    if method==1u {return binary(value);}
    var code=u32(floor(clamp(sdr_encode_component(value,transfer.info.x),0.,1.)*f32(transfer.info.y)+0.5));
    if method==0u {return code;}
    // Fast estimate, verified against exact Float32 decision boundaries. The
    // bounded fallback never assumes a driver's pow approximation error.
    for(var step=0u;step<2u;step++) {
        if bracket(value,code) {return code;}
        if code>0u && value<boundary(code-1u) {code--;} else {code++;}
    }
    if bracket(value,code) {return code;}
    return binary(value);
}
"#;
const DECODE: &str = r#"
@group(0) @binding(0) var input:texture_2d<u32>;
@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
    let code=textureLoad(input,vec2<i32>(p.xy),0);
    let maximum=f32(transfer.info.y);
    let alpha=select(f32(code.a)/maximum,1.,code.a==transfer.info.y);
    var linear:vec3<f32>;
    if method==0u {
        let rgb=select(vec3<f32>(code.rgb)/maximum,vec3(1.),code.rgb==vec3(transfer.info.y));
        linear=sdr_decode(rgb,transfer.info.x);
    } else {
        linear=vec3(transfer.entries[code.r*transfer.info.z].x,transfer.entries[code.g*transfer.info.z].x,transfer.entries[code.b*transfer.info.z].x);
    }
    return vec4(linear*alpha,alpha);
}
"#;
const ENCODE: &str = r#"
@group(0) @binding(0) var input:texture_2d<f32>;
@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<u32> {
    let value=textureLoad(input,vec2<i32>(p.xy),0);
    if value.a<=0. {return vec4(0u);}
    let straight=value.rgb/value.a;
    return vec4(quantize(straight.r),quantize(straight.g),quantize(straight.b),
        u32(floor(clamp(value.a,0.,1.)*f32(transfer.info.y)+0.5)));
}
"#;

pub fn run(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), Box<dyn std::error::Error>> {
    let lut_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("native transfer decision table"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(16 + 256 * 8),
            },
            count: None,
        }],
    });
    let working = texture(device, wgpu::TextureFormat::Rgba32Float);
    let working_view = working.create_view(&Default::default());
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let maximum = depth.maximum();
        let format = if depth == SampleDepth::U8 {
            wgpu::TextureFormat::Rgba8Uint
        } else {
            wgpu::TextureFormat::Rgba16Uint
        };
        let encoded = texture(device, format);
        let encoded_view = encoded.create_view(&Default::default());
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("one native tile readback"),
            size: (COUNT * 4 * depth.bytes()) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipelines: Vec<_> = (0..3u32)
            .map(|method| {
                let decode = make_pipeline(
                    device,
                    &lut_layout,
                    wgpu::TextureFormat::Rgba32Float,
                    true,
                    method,
                );
                let encode = make_pipeline(device, &lut_layout, format, false, method);
                let bindings =
                    [(&decode, &encoded_view), (&encode, &working_view)].map(|(pipeline, view)| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &pipeline.get_bind_group_layout(0),
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(view),
                            }],
                        })
                    });
                (decode, encode, bindings)
            })
            .collect();
        for (space_id, space) in RgbSpace::ALL.into_iter().enumerate() {
            let mut bytes = Vec::with_capacity(16 + 65536 * 8);
            bytes.extend(
                [space_id as u32, maximum, 65535 / maximum, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            for code in 0..=65535 {
                let decoded = space.decode(f64::from(code) / 65535.) as f32;
                let boundary = space.decode((f64::from(code) + 0.5) / 65535.);
                let mut decision = boundary as f32;
                // A comparison against this ceiling classifies every finite
                // Float32 input exactly like the Float64 encoded half-code.
                if f64::from(decision) < boundary {
                    decision = decision.next_up();
                }
                bytes.extend(decoded.to_le_bytes());
                bytes.extend(decision.to_le_bytes());
            }
            let table = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native transfer values and boundaries"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: true,
            });
            table.get_mapped_range_mut(..)?.copy_from_slice(&bytes);
            table.unmap();
            let curve = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &lut_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: table.as_entire_binding(),
                }],
            });
            let alphas = if depth == SampleDepth::U8 {
                vec![0, 1, 2, 17, 128, 255]
            } else {
                vec![0, 1, 2, 17, 257, 32768, 65535]
            };
            for alpha in alphas {
                let source: Vec<[u32; 4]> = (0..COUNT)
                    .map(|i| {
                        let code = i as u32;
                        [
                            code & maximum,
                            code.wrapping_mul(617) & maximum,
                            code.wrapping_mul(251) & maximum,
                            alpha,
                        ]
                    })
                    .collect();
                let bytes: Vec<_> = source
                    .iter()
                    .flatten()
                    .flat_map(|v| v.to_le_bytes().into_iter().take(depth.bytes()))
                    .collect();
                for (method, (decode, encode, bindings)) in pipelines.iter().enumerate() {
                    for cycles in [1, 64] {
                        let mut times = Vec::new();
                        let samples = if cycles == 1 { 120 } else { 1 };
                        for sample in 0..samples {
                            queue.write_texture(
                                encoded.as_image_copy(),
                                &bytes,
                                wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(SIZE * 4 * depth.bytes() as u32),
                                    rows_per_image: Some(SIZE),
                                },
                                encoded.size(),
                            );
                            let start = Instant::now();
                            let mut commands = device.create_command_encoder(&Default::default());
                            for _ in 0..cycles {
                                draw_bound(
                                    &mut commands,
                                    decode,
                                    &bindings[0],
                                    &curve,
                                    &working_view,
                                );
                                draw_bound(
                                    &mut commands,
                                    encode,
                                    &bindings[1],
                                    &curve,
                                    &encoded_view,
                                );
                            }
                            queue.submit([commands.finish()]);
                            let cpu = start.elapsed().as_secs_f64() * 1000.;
                            device.poll(wgpu::PollType::Wait {
                                submission_index: None,
                                timeout: Some(Duration::from_secs(30)),
                            })?;
                            if sample >= 20 {
                                times.push([cpu, start.elapsed().as_secs_f64() * 1000.]);
                            }
                        }
                        let mut commands = device.create_command_encoder(&Default::default());
                        commands.copy_texture_to_buffer(
                            encoded.as_image_copy(),
                            wgpu::TexelCopyBufferInfo {
                                buffer: &staging,
                                layout: wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(SIZE * 4 * depth.bytes() as u32),
                                    rows_per_image: Some(SIZE),
                                },
                            },
                            encoded.size(),
                        );
                        queue.submit([commands.finish()]);
                        let (tx, rx) = mpsc::channel();
                        staging
                            .slice(..)
                            .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
                        device.poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(Duration::from_secs(30)),
                        })?;
                        rx.recv()??;
                        let actual = staging.slice(..).get_mapped_range()?;
                        let mut errors = [0u32; 4];
                        let mut changed = 0;
                        for (expected, actual) in
                            source.iter().zip(actual.chunks_exact(4 * depth.bytes()))
                        {
                            for c in 0..4 {
                                let value = if depth == SampleDepth::U8 {
                                    u32::from(actual[c])
                                } else {
                                    u32::from(u16::from_le_bytes(
                                        actual[c * 2..c * 2 + 2].try_into()?,
                                    ))
                                };
                                let expected = if alpha == 0 { 0 } else { expected[c] };
                                let error = value.abs_diff(expected);
                                errors[c] = errors[c].max(error);
                                changed += usize::from(error != 0);
                            }
                        }
                        drop(actual);
                        staging.unmap();
                        let mut percentiles = [[0.; 2]; 2];
                        if !times.is_empty() {
                            for axis in 0..2 {
                                times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
                                percentiles[axis] = [times[94][axis], times[98][axis]];
                            }
                        }
                        println!(
                            "TILE_BOUNDARY depth={depth:?} space={space:?} alpha={alpha} method={} cycles={cycles} max={errors:?} changed={changed} cpu_p95_p99={:.4?} complete_p95_p99={:.4?} table_bytes={} timed_samples={}",
                            ["analytic", "binary", "refined"][method],
                            percentiles[0],
                            percentiles[1],
                            table.size(),
                            times.len()
                        );
                        if method != 0 && errors != [0; 4] {
                            return Err(
                                "Native transfer table failed exact integer round trip".into()
                            );
                        }
                    }
                }
            }
            verify_boundaries(
                device,
                queue,
                depth,
                space,
                &pipelines,
                &curve,
                &working,
                &encoded,
                &encoded_view,
                &staging,
            )?;
        }
    }
    Ok(())
}

fn make_pipeline(
    device: &wgpu::Device,
    lut: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    decode: bool,
    method: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("native integer boundary measurement"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{SHARED}\n{SHADER}\n{}",
                if decode { DECODE } else { ENCODE }
            )
            .into(),
        ),
    });
    let input = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: if decode {
                    wgpu::TextureSampleType::Uint
                } else {
                    wgpu::TextureSampleType::Float { filterable: false }
                },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&input), Some(lut)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native integer boundary"),
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
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("method", f64::from(method))],
                ..Default::default()
            },
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
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
fn draw_bound(
    commands: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    input: &wgpu::BindGroup,
    curve: &wgpu::BindGroup,
    target: &wgpu::TextureView,
) {
    let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("native integer boundary"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, input, &[]);
    pass.set_bind_group(1, curve, &[]);
    pass.draw(0..3, 0..1);
}

#[allow(clippy::too_many_arguments)]
fn verify_boundaries(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    depth: SampleDepth,
    space: RgbSpace,
    pipelines: &[(
        wgpu::RenderPipeline,
        wgpu::RenderPipeline,
        [wgpu::BindGroup; 2],
    )],
    curve: &wgpu::BindGroup,
    working: &wgpu::Texture,
    encoded: &wgpu::Texture,
    encoded_view: &wgpu::TextureView,
    staging: &wgpu::Buffer,
) -> Result<(), Box<dyn std::error::Error>> {
    let maximum = depth.maximum();
    for kind in ["below", "ceiling", "above", "extended_random"] {
        let values: Vec<[f32; 4]> = (0..COUNT as u32)
            .map(|i| {
                let channels = [i, i.wrapping_mul(617), i.wrapping_mul(251)].map(|code| {
                    if kind == "extended_random" {
                        let mixed = code.wrapping_mul(747796405).wrapping_add(2891336453);
                        return (mixed as f64 / f64::from(u32::MAX) * 1.2 - 0.1) as f32;
                    }
                    let boundary =
                        space.decode((f64::from(code % maximum) + 0.5) / f64::from(maximum));
                    let mut value = boundary as f32;
                    if f64::from(value) < boundary {
                        value = value.next_up();
                    }
                    match kind {
                        "below" => value.next_down(),
                        "above" => value.next_up(),
                        _ => value,
                    }
                });
                [channels[0], channels[1], channels[2], 1.]
            })
            .collect();
        let bytes: Vec<_> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        queue.write_texture(
            working.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 16),
                rows_per_image: Some(SIZE),
            },
            working.size(),
        );
        for (method, (_, encode, bindings)) in pipelines.iter().enumerate() {
            let mut commands = device.create_command_encoder(&Default::default());
            draw_bound(&mut commands, encode, &bindings[1], curve, encoded_view);
            commands.copy_texture_to_buffer(
                encoded.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE * 4 * depth.bytes() as u32),
                        rows_per_image: Some(SIZE),
                    },
                },
                encoded.size(),
            );
            queue.submit([commands.finish()]);
            let (tx, rx) = mpsc::channel();
            staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
            device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })?;
            rx.recv()??;
            let actual = staging.slice(..).get_mapped_range()?;
            let mut worst = 0;
            let mut changed = 0;
            let mut first = None;
            for (pixel, (values, actual)) in values
                .iter()
                .zip(actual.chunks_exact(4 * depth.bytes()))
                .enumerate()
            {
                for c in 0..3 {
                    let value = if depth == SampleDepth::U8 {
                        u32::from(actual[c])
                    } else {
                        u32::from(u16::from_le_bytes(actual[c * 2..c * 2 + 2].try_into()?))
                    };
                    let expected = (space.encode(f64::from(values[c])).clamp(0., 1.)
                        * f64::from(maximum))
                    .round() as u32;
                    let error = value.abs_diff(expected);
                    worst = worst.max(error);
                    changed += usize::from(error != 0);
                    if error != 0 && first.is_none() {
                        first = Some((pixel, c, values[c], expected, value));
                    }
                }
            }
            drop(actual);
            staging.unmap();
            println!(
                "TILE_QUANTIZE depth={depth:?} space={space:?} method={} kind={kind} max={worst} changed={changed} first={first:?}",
                ["analytic", "binary", "refined"][method]
            );
            if method != 0 && worst != 0 {
                return Err("Native quantizer disagrees with Float64 code reference".into());
            }
        }
    }
    Ok(())
}
