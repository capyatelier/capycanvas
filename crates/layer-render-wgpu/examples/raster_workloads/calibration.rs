use super::*;
use wgpu::util::DeviceExt;

fn texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("composition calibration"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    })
}

fn pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, entry: &str) -> wgpu::ComputePipeline {
    let layout = (entry != "initialize").then(|| {
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, count: None,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, count: None,
                ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 } },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, count: None,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: std::num::NonZeroU64::new(16) } },
        ];
        if entry == "blend" {
            entries.push(wgpu::BindGroupLayoutEntry { binding: 1, ..entries[0] });
        }
        let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(entry), entries: &entries });
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(entry), bind_group_layouts: &[Some(&group)], immediate_size: 0 })
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry), layout: layout.as_ref(), module: shader, entry_point: Some(entry),
        compilation_options: Default::default(), cache: None,
    })
}

pub(super) fn run(output: &Path) -> Result<()> {
    let r = WgpuRasterizer::new_native_headless(DocumentColor::default())?;
    let device = r.device();
    let queue = r.queue();
    let adapter = r.adapter().get_info();
    std::fs::write(output.join("calibration-adapter.txt"), format!("{adapter:#?}\n{:?}\n", device.limits()))?;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("composition cost calibration"),
        source: wgpu::ShaderSource::Wgsl(include_str!("calibration.wgsl").into()),
    });
    let initialize = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("calibration input"), source: wgpu::ShaderSource::Wgsl(r#"
@group(0) @binding(0) var output: texture_storage_2d<rgba32float, write>;
@compute @workgroup_size(8,8)
fn initialize(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(output)) { return; }
    var n = id.x * 1664525u + id.y * 1013904223u;
    n = (n ^ (n >> 16u)) * 2246822519u;
    let c = vec3<f32>(f32(n & 255u), f32((n >> 8u) & 255u), f32((n >> 16u) & 255u)) / 510.;
    textureStore(output, vec2<i32>(id.xy), vec4<f32>(c, .5));
}"#.into()),
    });
    let initialize = pipeline(device, &initialize, "initialize");
    let kernels: Vec<_> = ["copy", "blend", "reduce", "blur"].into_iter()
        .map(|entry| pipeline(device, &shader, entry)).collect();
    let query = device.features().contains(wgpu::Features::TIMESTAMP_QUERY).then(|| {
        device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("calibration timestamps"), ty: wgpu::QueryType::Timestamp, count: 2,
        })
    });
    let resolved = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("calibration resolve"), size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("calibration readback"), size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut csv = BufWriter::new(std::fs::File::create(output.join("calibration.csv"))?);
    writeln!(csv, "kernel,width,height,side,radius,repeat,output_pixels,minimum_bytes,texture_samples,cpu_ms,completed_ms,gpu_ms")?;
    for extent in [[1, 1], [256, 256], [1024, 1024], [2048, 2048], [4248, 2832]] {
        let inputs = [texture(device, extent), texture(device, extent)];
        let views = inputs.each_ref().map(|t| t.create_view(&Default::default()));
        let mut encoder = device.create_command_encoder(&Default::default());
        for view in &views {
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("calibration input"), layout: &initialize.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&initialize);
            pass.set_bind_group(0, &binding, &[]);
            pass.dispatch_workgroups(extent[0].div_ceil(8), extent[1].div_ceil(8), 1);
        }
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(60)) })?;
        for (kernel, side, radius) in [(0, 1u32, 0u32), (1, 1, 0), (2, 4, 0), (2, 8, 0), (2, 16, 0), (3, 1, 3), (3, 1, 12), (3, 1, 21)] {
            let name = ["copy", "blend", "reduce", "blur"][kernel];
            let size = extent.map(|n| n.div_ceil(side));
            let pixels = u64::from(size[0]) * u64::from(size[1]);
            let source_pixels = u64::from(extent[0]) * u64::from(extent[1]);
            let bytes = 16 * (source_pixels * if kernel == 1 { 2 } else { 1 } + pixels);
            let samples = if kernel == 2 { source_pixels } else {
                pixels * match kernel { 1 => 2, 3 => u64::from(radius * 2 + 1), _ => 1 }
            };
            let target = texture(device, size).create_view(&Default::default());
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("calibration settings"),
                contents: &[size[0], size[1], side, radius].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&target) },
                wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
            ];
            if kernel == 1 { entries.push(wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1]) }); }
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name), layout: &kernels[kernel].get_bind_group_layout(0), entries: &entries,
            });
            for repeat in 0..34 {
                let start = Instant::now();
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some(name), timestamp_writes: query.as_ref().map(|query_set| wgpu::ComputePassTimestampWrites {
                            query_set, beginning_of_pass_write_index: Some(0), end_of_pass_write_index: Some(1),
                        }),
                    });
                    pass.set_pipeline(&kernels[kernel]);
                    pass.set_bind_group(0, &binding, &[]);
                    pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
                }
                if let Some(query) = &query {
                    encoder.resolve_query_set(query, 0..2, &resolved, 0);
                    encoder.copy_buffer_to_buffer(&resolved, 0, &readback, 0, 16);
                }
                let submission = queue.submit([encoder.finish()]);
                let cpu = ms(start);
                device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(60)) })?;
                let completed = ms(start);
                let mut gpu = f64::NAN;
                if query.is_some() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    readback.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
                    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(60)) })?;
                    rx.recv()??;
                    let data = readback.slice(..).get_mapped_range()?;
                    let a = u64::from_le_bytes(data[..8].try_into()?);
                    let b = u64::from_le_bytes(data[8..].try_into()?);
                    gpu = b.saturating_sub(a) as f64 * f64::from(queue.get_timestamp_period()) / 1e6;
                    drop(data);
                    readback.unmap();
                }
                if repeat >= 4 {
                    writeln!(csv, "{name},{},{},{side},{radius},{},{pixels},{bytes},{samples},{cpu:.6},{completed:.6},{gpu:.6}", extent[0], extent[1], repeat - 4)?;
                }
            }
            csv.flush()?;
            println!("calibrated {name} {extent:?} side={side} radius={radius}");
        }
    }
    Ok(())
}
