//! Controlled GPU kernel experiment, not an app-frame benchmark.
//! cargo ndk -t arm64-v8a -P 29 build -p layer-render-wgpu --release --example swept_brush_bench
//! swept_brush_bench OUTPUT_DIR [repetitions=30] [diameter: 32|256|2048|all] [reverse]
//! Outputs paired raw GPU timestamps, CPU submit durations, counters and PNGs.
//! Uses production DabGenerator and contact.wgsl, with an isolated deterministic
//! paper texture and no photo/composition/prediction. No existing presets change.
use layer_core::{
    DefaultBrushPreset, LayerId, Point, Stroke, StrokeId, StrokePoint, StrokeTool, default_brush,
};
use layer_engine::DabGenerator;
use layer_render::Dab;
use std::{
    collections::BTreeMap,
    error::Error,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const PAGE: u32 = 256;

// Dab is repr(C), contains only f32, and the size/layout matches WGSL's Dab.
fn dab_bytes(dabs: &[Dab]) -> &[u8] {
    assert_eq!(std::mem::size_of::<Dab>(), 128);
    unsafe { std::slice::from_raw_parts(dabs.as_ptr().cast(), std::mem::size_of_val(dabs)) }
}
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}
fn idle(device: &wgpu::Device) -> Result<()> {
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    Ok(())
}
fn read(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    idle(device)?;
    rx.recv()??;
    let bytes = buffer.get_mapped_range(..)?.to_vec();
    buffer.unmap();
    Ok(bytes)
}

// Merge only when all intermediate centers/radii agree with a single linear
// sweep to <=0.5 document pixel. A conservative RDP split; no whole-stroke
// closest-point scan is added to the renderer.
fn merge(dabs: &[Dab]) -> Vec<Dab> {
    fn recurse(dabs: &[Dab], lo: usize, hi: usize, out: &mut Vec<Dab>) {
        let a = dabs[lo];
        let b = dabs[hi];
        let delta = [b.center.x - a.center.x, b.center.y - a.center.y];
        let length2 = delta[0] * delta[0] + delta[1] * delta[1];
        let mut worst = (0.0f32, lo);
        for (i, d) in dabs.iter().enumerate().take(hi).skip(lo + 1) {
            let p = [d.center.x - a.center.x, d.center.y - a.center.y];
            let t = ((p[0] * delta[0] + p[1] * delta[1]) / length2.max(1e-6)).clamp(0., 1.);
            let error = (p[0] - t * delta[0]).hypot(p[1] - t * delta[1])
                + (d.radii[0] - (a.radii[0] + t * (b.radii[0] - a.radii[0]))).abs();
            if error > worst.0 {
                worst = (error, i);
            }
        }
        if worst.0 > 0.5 {
            recurse(dabs, lo, worst.1, out);
            recurse(dabs, worst.1, hi, out);
        } else {
            let mut segment = b;
            segment.motion = delta;
            segment.previous = [a.radii[0], a.radii[1], a.rotation[0], a.rotation[1]];
            segment.previous_contact = a.contact;
            out.push(segment);
        }
    }
    let mut output = Vec::new();
    if dabs.len() == 1 {
        return dabs.to_vec();
    }
    recurse(dabs, 0, dabs.len() - 1, &mut output);
    output
}

struct Work {
    group: wgpu::BindGroup,
    pages: u32,
    evaluations: u64,
}
fn work(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    style: &wgpu::Buffer,
    dabs: &[Dab],
    paper: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    background: &wgpu::TextureView,
    output: &wgpu::TextureView,
    extent: [u32; 2],
) -> Work {
    let mut pages = BTreeMap::<[u32; 2], [u32; 2]>::new();
    for (index, d) in dabs.iter().enumerate() {
        // Same conservative endpoint-radius capsule envelope for both kernels.
        let r = d.radii[0].max(d.previous[0]) + 2.;
        let from = [d.center.x - d.motion[0], d.center.y - d.motion[1]];
        let min = [d.center.x.min(from[0]) - r, d.center.y.min(from[1]) - r];
        let max = [d.center.x.max(from[0]) + r, d.center.y.max(from[1]) + r];
        for y in (min[1].max(0.) as u32 / PAGE)..=((max[1] as u32).min(extent[1] - 1) / PAGE) {
            for x in (min[0].max(0.) as u32 / PAGE)..=((max[0] as u32).min(extent[0] - 1) / PAGE) {
                pages
                    .entry([y, x])
                    .and_modify(|v| v[1] = index as u32 + 1)
                    .or_insert([index as u32, index as u32 + 1]);
            }
        }
    }
    let evaluations = pages
        .values()
        .map(|v| u64::from(v[1] - v[0]) * u64::from(PAGE * PAGE))
        .sum();
    let records: Vec<u8> = pages
        .iter()
        .flat_map(|([y, x], [first, end])| [x * PAGE, y * PAGE, *first, end - first])
        .flat_map(u32::to_le_bytes)
        .collect();
    let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("page contact ranges"),
        contents: &records,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let geometry = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("resolved geometry"),
        contents: dab_bytes(dabs),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let entries = [
        style.as_entire_binding(),
        geometry.as_entire_binding(),
        list.as_entire_binding(),
        wgpu::BindingResource::TextureView(paper),
        wgpu::BindingResource::Sampler(sampler),
        wgpu::BindingResource::TextureView(background),
        wgpu::BindingResource::TextureView(output),
    ];
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("kernel benchmark"),
        layout,
        entries: &entries
            .into_iter()
            .enumerate()
            .map(|(binding, resource)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource,
            })
            .collect::<Vec<_>>(),
    });
    Work {
        group,
        pages: pages.len() as u32,
        evaluations,
    }
}

fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    extent: [u32; 2],
    path: &Path,
) -> Result<()> {
    let stride = extent[0] * 16;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("visual proof"),
        size: u64::from(stride) * u64::from(extent[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(extent[1]),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let data = read(device, &buffer)?;
    // Export compact previews; native values are checked before downsampling.
    let mut painted = 0usize;
    for pixel in data.chunks_exact(16) {
        let value = f32::from_le_bytes(pixel[..4].try_into()?);
        assert!(value.is_finite() && (0.0..=1.001).contains(&value));
        if value > 0. && value < 0.99 {
            painted += 1;
        }
    }
    assert!(painted > 0, "kernel must actually paint");
    let step = extent[0].max(extent[1]).div_ceil(1200).max(1);
    let size = extent.map(|v| v.div_ceil(step));
    let mut pixels = Vec::new();
    for y in (0..extent[1]).step_by(step as usize) {
        for x in (0..extent[0]).step_by(step as usize) {
            let at = ((u64::from(y) * u64::from(extent[0]) + u64::from(x)) * 16) as usize;
            let value = f32::from_le_bytes(data[at..at + 4].try_into()?);
            // Undispatched texture pixels are zero-initialized, shown as white.
            let alpha = f32::from_le_bytes(data[at + 12..at + 16].try_into()?);
            let value = if alpha == 0. { 1. } else { value };
            let srgb = if value <= 0.0031308 {
                value * 12.92
            } else {
                1.055 * value.powf(1. / 2.4) - 0.055
            };
            pixels.push((srgb.clamp(0., 1.) * 255.).round() as u8);
        }
    }
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(path)?),
        size[0],
        size[1],
    );
    encoder.set_color(png::ColorType::Grayscale);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let directory = args.next().ok_or("OUTPUT_DIR required")?;
    let repetitions: u32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(30);
    let filter = args.next().unwrap_or_else(|| "all".into());
    let reverse = args.next().as_deref() == Some("reverse");
    assert!(repetitions >= 5);
    std::fs::create_dir_all(&directory)?;
    let directory = Path::new(&directory);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        force_fallback_adapter: false,
        ..Default::default()
    }))?;
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "physical GPU required"
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TIMESTAMP_QUERY,
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    std::fs::write(
        directory.join("adapter.txt"),
        format!("{info:#?}\nrepetitions={repetitions}\nreverse={reverse}\n"),
    )?;
    println!("adapter={info:?}");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("swept brush experiment"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                include_str!("swept_brush/kernel.wgsl"),
                include_str!("../src/contact.wgsl")
            )
            .into(),
        ),
    });
    let bindings: Vec<_> = (0..7)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            count: None,
            ty: match binding {
                0..=2 => wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: true }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                3 | 5 => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: binding == 3,
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                4 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                _ => wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba32Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
            },
        })
        .collect();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &bindings,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let texture = |label, extent: [u32; 2], format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let paper = texture(
        "fixed synthetic paper",
        [1024; 2],
        wgpu::TextureFormat::R8Unorm,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let grain: Vec<u8> = (0..1024u32 * 1024)
        .map(|v| {
            let mut h = v.wrapping_mul(1597334677);
            h = (h ^ (h >> 16)).wrapping_mul(2246822519);
            (32 + (h >> 24) * 3 / 4) as u8
        })
        .collect();
    queue.write_texture(
        paper.as_image_copy(),
        &grain,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(1024),
            rows_per_image: Some(1024),
        },
        paper.size(),
    );
    let paper = paper.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let query = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let timestamps = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut csv = std::io::BufWriter::new(std::fs::File::create(directory.join("samples.csv"))?);
    writeln!(
        csv,
        "brush,diameter,path,length,variant,repeat,contacts,pages,pixel_contact_tests,cpu_prepare_ms,cpu_submit_ms,gpu_ms"
    )?;
    let mut presets = [DefaultBrushPreset::GPen, DefaultBrushPreset::Pencil];
    let mut diameters = [32u32, 256, 2048];
    let mut paths = [("straight", 512.), ("curve", 512.), ("backlog", 4096.)];
    if reverse {
        presets.reverse();
        diameters.reverse();
        paths.reverse();
    }
    for preset in presets {
        let pencil = preset == DefaultBrushPreset::Pencil;
        let pipelines: Vec<_> = [false, true]
            .into_iter()
            .map(|simple| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("paired brush kernel"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("SIMPLE", f64::from(simple)), ("PENCIL", f64::from(pencil))],
                        ..Default::default()
                    },
                    cache: None,
                })
            })
            .collect();
        for diameter in diameters {
            if filter != "all" && filter != diameter.to_string() {
                continue;
            }
            for (path, length) in paths {
                if path == "backlog" && diameter < 2048 {
                    continue;
                }
                let mut brush = default_brush(preset);
                brush.diameter = diameter as f32;
                let margin = diameter as f32 * 0.5 + 8.;
                let points: Vec<_> = (0..=128)
                    .map(|i| {
                        let t = i as f32 / 128.;
                        StrokePoint {
                            position: Point {
                                x: margin + length * t,
                                y: margin
                                    + 64.
                                    + if path == "curve" {
                                        (t * std::f32::consts::TAU).sin() * 64.
                                    } else {
                                        0.
                                    },
                            },
                            pressure: 1.,
                            tilt: [0.; 2],
                            twist: 0.,
                            elapsed_micros: i * 4167,
                        }
                    })
                    .collect();
                let stroke = Stroke::new(
                    StrokeId(1),
                    LayerId(1),
                    StrokeTool::Brush,
                    brush.clone(),
                    points,
                )?;
                let start = Instant::now();
                let mut dabs = Vec::new();
                DabGenerator::generate(&stroke, Default::default(), &mut dabs);
                let generate_ms = start.elapsed().as_secs_f64() * 1000.;
                let start = Instant::now();
                let merged = merge(&dabs);
                let merge_ms = start.elapsed().as_secs_f64() * 1000.;
                let extent = [
                    (margin * 2. + length).ceil() as u32,
                    (margin * 2. + 128.).ceil() as u32,
                ]
                .map(|v| v.div_ceil(PAGE) * PAGE);
                let output = texture(
                    "kernel output",
                    extent,
                    wgpu::TextureFormat::Rgba32Float,
                    wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                );
                let background = texture(
                    "fixed white backing",
                    extent,
                    wgpu::TextureFormat::Rgba32Float,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                );
                let output_view = output.create_view(&Default::default());
                let background_view = background.create_view(&Default::default());
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &background_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                queue.submit([encoder.finish()]);
                idle(&device)?;
                let c = brush.contact.unwrap();
                let style = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    usage: wgpu::BufferUsages::UNIFORM,
                    contents: &floats([
                        1.,
                        c.paper,
                        c.tip_bias,
                        c.edge_roughness,
                        c.edge_scale,
                        c.fibers,
                        c.fiber_strength,
                        c.pooling,
                        c.pressure_gain,
                        c.depletion,
                        c.tilt_shading,
                        0.,
                        1.,
                        0.,
                        1.,
                        0.,
                    ]),
                });
                let start = Instant::now();
                let original = work(
                    &device,
                    &layout,
                    &style,
                    &dabs,
                    &paper,
                    &sampler,
                    &background_view,
                    &output_view,
                    extent,
                );
                let original_prepare = generate_ms + start.elapsed().as_secs_f64() * 1000.;
                let start = Instant::now();
                let compact = work(
                    &device,
                    &layout,
                    &style,
                    &merged,
                    &paper,
                    &sampler,
                    &background_view,
                    &output_view,
                    extent,
                );
                let compact_prepare =
                    generate_ms + merge_ms + start.elapsed().as_secs_f64() * 1000.;
                let variants = [
                    (
                        "contact",
                        &original,
                        &pipelines[0],
                        dabs.len(),
                        original_prepare,
                    ),
                    (
                        "simple",
                        &original,
                        &pipelines[1],
                        dabs.len(),
                        original_prepare,
                    ),
                    (
                        "merged",
                        &compact,
                        &pipelines[1],
                        merged.len(),
                        compact_prepare,
                    ),
                ];
                println!(
                    "{preset:?} {diameter} {path}: {} contacts -> {} segments, {} -> {} page pixel tests",
                    dabs.len(),
                    merged.len(),
                    original.evaluations,
                    compact.evaluations
                );
                for iteration in 0..repetitions + 5 {
                    // Rotate order to reduce thermal/clock bias. Warm each variant.
                    for order in 0..3 {
                        let (name, w, p, count, prepare) =
                            variants[((iteration + order) % 3) as usize];
                        let start = Instant::now();
                        let mut encoder = device.create_command_encoder(&Default::default());
                        {
                            let mut pass =
                                encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                                    label: Some(name),
                                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                                        query_set: &query,
                                        beginning_of_pass_write_index: Some(0),
                                        end_of_pass_write_index: Some(1),
                                    }),
                                });
                            pass.set_pipeline(p);
                            pass.set_bind_group(0, &w.group, &[]);
                            pass.dispatch_workgroups(PAGE / 8, PAGE / 8, w.pages);
                        }
                        encoder.resolve_query_set(&query, 0..2, &resolve, 0);
                        encoder.copy_buffer_to_buffer(&resolve, 0, &timestamps, 0, 16);
                        queue.submit([encoder.finish()]);
                        let cpu = start.elapsed().as_secs_f64() * 1000.;
                        let raw = read(&device, &timestamps)?;
                        let from = u64::from_le_bytes(raw[..8].try_into()?);
                        let to = u64::from_le_bytes(raw[8..].try_into()?);
                        assert!(to > from, "invalid GPU timestamps");
                        let gpu =
                            (to - from) as f64 * f64::from(queue.get_timestamp_period()) / 1e6;
                        if iteration >= 5 {
                            writeln!(
                                csv,
                                "{preset:?},{diameter},{path},{length},{name},{},{count},{},{},{prepare:.6},{cpu:.6},{gpu:.6}",
                                iteration - 5,
                                w.pages,
                                w.evaluations
                            )?;
                        }
                        if iteration == 5 {
                            capture(
                                &device,
                                &queue,
                                &output,
                                extent,
                                &directory.join(format!("{preset:?}-{diameter}-{path}-{name}.png")),
                            )?;
                        }
                    }
                }
                csv.flush()?;
            }
        }
    }
    Ok(())
}
