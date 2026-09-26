//! Opt-in measurements, no changes to production rendering.
use super::*;
use layer_core::color::{DocumentColor, SampleDepth, source::*};
use std::time::Instant;

#[test]
#[ignore = "physical GPU two-pass workload control; no compositor"]
fn filter_microbench() {
    use wgpu::util::DeviceExt;
    let mut r = WgpuRasterizer::new_float32().unwrap();
    let extent = [5184, 3456];
    eprintln!("micro adapter={:?}", r.adapter.get_info());
    let texture = |label| {
        r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let textures = [
        texture("micro input"),
        texture("micro intermediate"),
        texture("micro output"),
    ];
    let views = textures
        .each_ref()
        .map(|t| t.create_view(&Default::default()));
    let photo = std::fs::read(std::env::var("CAPY_FILTER_PHOTO_RGBA").unwrap()).unwrap();
    let linear: Vec<u8> = photo
        .chunks_exact(4)
        .flat_map(|p| {
            let decode = |x: u8| {
                let v = x as f32 / 255.;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            [decode(p[0]), decode(p[1]), decode(p[2]), p[3] as f32 / 255.]
                .into_iter()
                .flat_map(f32::to_ne_bytes)
        })
        .collect();
    r.queue.write_texture(
        textures[0].as_image_copy(),
        &linear,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(extent[0] * 16),
            rows_per_image: Some(extent[1]),
        },
        textures[0].size(),
    );
    let layout = r
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("micro layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
    let pipeline_layout = r
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("micro pipelines"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let sampler = r.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = include_str!("filter_microbench.wgsl");
    r.set_telemetry_enabled(true);
    eprintln!("variant,sigma,frame,cpu_ms,complete_ms,gpu_ms");
    for sigma in [3f32, 21.] {
        let radius = (sigma * 3.).ceil() as usize;
        let weights: Vec<f32> = (0..=radius)
            .map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp())
            .collect();
        let norm = weights[0] + 2. * weights[1..].iter().sum::<f32>();
        let mut table = vec![[
            weights[0] / norm,
            radius.div_ceil(2) as f32,
            radius as f32,
            0.,
        ]];
        for i in (1..=radius).step_by(2) {
            let a = weights[i];
            let b = weights.get(i + 1).copied().unwrap_or(0.);
            table.push([i as f32 + b / (a + b), (a + b) / norm, 0., 0.]);
        }
        table.resize(34, [0.; 4]);
        table.extend(weights.iter().map(|w| [*w / norm, 0., 0., 0.]));
        let data: Vec<u8> = table
            .into_iter()
            .flatten()
            .flat_map(f32::to_ne_bytes)
            .collect();
        let table = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("micro Gaussian taps"),
                contents: &data,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let groups: Vec<_> = views[..2]
            .iter()
            .map(|v| {
                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("micro inputs"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(v),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: table.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        for variant in ["copy", "manual", "axis", "discrete", "hardware", "warp"] {
            if sigma == 21. && (variant == "copy" || variant == "warp") {
                continue;
            }
            let pipelines: Vec<_> = (0..2)
                .map(|axis| {
                    let source = shader
                        .replace("VARIANT", variant)
                        .replace("IS_COPY", if variant == "copy" { "true" } else { "false" })
                        .replace("IS_WARP", if variant == "warp" { "true" } else { "false" })
                        .replace(
                            "IS_DISCRETE",
                            if variant == "discrete" {
                                "true"
                            } else {
                                "false"
                            },
                        )
                        .replace(
                            "AXIS",
                            if axis == 0 {
                                "vec2<f32>(1.,0.)"
                            } else {
                                "vec2<f32>(0.,1.)"
                            },
                        );
                    let module = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(variant),
                        source: wgpu::ShaderSource::Wgsl(source.into()),
                    });
                    r.device
                        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                            label: Some(variant),
                            layout: Some(&pipeline_layout),
                            vertex: wgpu::VertexState {
                                module: &module,
                                entry_point: Some("vertex"),
                                buffers: &[],
                                compilation_options: Default::default(),
                            },
                            fragment: Some(wgpu::FragmentState {
                                module: &module,
                                entry_point: Some("fragment"),
                                compilation_options: Default::default(),
                                targets: &[Some(wgpu::ColorTargetState {
                                    format: wgpu::TextureFormat::Rgba32Float,
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
                })
                .collect();
            for frame in 0..24 {
                let start = Instant::now();
                let mut encoder = r.device.create_command_encoder(&Default::default());
                r.telemetry.begin(&r.device, &r.queue, &mut encoder);
                for pass in 0..if variant == "warp" { 1 } else { 2 } {
                    let attachments = [Some(wgpu::RenderPassColorAttachment {
                        view: &views[pass + 1],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })];
                    let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(variant),
                        color_attachments: &attachments,
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    p.set_pipeline(&pipelines[pass]);
                    p.set_bind_group(0, &groups[pass], &[]);
                    p.draw(0..3, 0..1);
                }
                r.telemetry.end(&mut encoder);
                r.queue.submit([encoder.finish()]);
                r.telemetry.submitted(&r.queue);
                let cpu = start.elapsed().as_secs_f64() * 1000.;
                r.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(READBACK_TIMEOUT),
                    })
                    .unwrap();
                let complete = start.elapsed().as_secs_f64() * 1000.;
                let telemetry = r.telemetry.completed_snapshot(&r.device, &r.queue);
                let gpu = telemetry.gpu.ordered().last().copied().unwrap_or(f32::NAN);
                if frame >= 4 {
                    eprintln!("{variant},{sigma},{frame},{cpu:.3},{complete:.3},{gpu:.3}");
                }
            }
        }
    }
}

#[test]
#[ignore = "physical GPU investigation; requires CAPY_FILTER_PHOTO_RGBA"]
fn photo_filter_frame_time() {
    let project = std::env::var("CAPY_FILTER_PROJECT").ok().map(|path| {
        layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap()
    });
    let extent = project.as_ref().map_or([5184, 3456], |p| [p.document.width, p.document.height]);
    let mut view = ViewState { width_px: extent[0], height_px: extent[1],
        background_rgba_linear: [0.; 4], ..test_view() };
    if project.is_some() {
        // A 61 MP document is not a 61 MP monitor. Keep the real filter extent,
        // but fit its presentation within the ordinary bounded display cache.
        let scale = (1600. / extent[0] as f32).min(1000. / extent[1] as f32);
        view.width_px = 1600;
        view.height_px = 1000;
        view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
    }
    let submit_frame = |r: &mut WgpuRasterizer, layers: &[Layer], time, reset, all| {
        r.submit(FramePacket { view, document_extent: extent, layers,
            dabs: &[], dab_batches: &[], restore_rasters: &[],
            time_seconds: time, reset_layers: reset, composite_all: all }).unwrap();
    };
    let bytes = if project.is_some() { Vec::new() } else {
        let bytes = std::fs::read(std::env::var("CAPY_FILTER_PHOTO_RGBA").unwrap()).unwrap();
        assert_eq!(bytes.len(), (extent[0] * extent[1] * 4) as usize);
        bytes
    };
    let mode = std::env::var("CAPY_FILTER_MODE").unwrap_or_else(|_| "native".into());
    let start = Instant::now();
    let mut r = match mode.as_str() {
        "legacy" => WgpuRasterizer::new_headless(),
        "float" => WgpuRasterizer::new_float32(),
        _ => WgpuRasterizer::new_native_headless(DocumentColor::default()),
    }
    .unwrap();
    if let Ok(mib) = std::env::var("CAPY_FILTER_LIMIT_MIB")
        && let Some(native) = &mut r.native_edit
    {
        native.image_pixel_bytes = Some(mib.parse::<u64>().unwrap() * 1024 * 1024);
    }
    if std::env::var("CAPY_FILTER_DISPLAY").as_deref() == Ok("dense") {
        r.native_edit.as_mut().unwrap().display_dense_bytes = u64::MAX;
    }
    if let Ok(mib) = std::env::var("CAPY_FILTER_SOURCE_MIB") {
        r.scene
            .as_mut()
            .unwrap()
            .admit_native_sources(mib.parse::<u64>().unwrap() * 4 * 1024 * 1024);
    }
    eprintln!(
        "adapter={:?} mode={mode} init_ms={:.3}",
        r.adapter.get_info(),
        start.elapsed().as_secs_f64() * 1000.
    );
    let mut base = Layer::paint(LayerId(1), "Water photo");
    if let Some(project) = project {
        assert_eq!(mode, "native");
        base = project.document.layers.into_iter().find(|layer| layer.source.is_some()).unwrap();
    } else if mode == "legacy" || mode == "float" {
        let asset = AssetId("investigation:water".into());
        r.prepare_asset(
            &asset,
            HostImage {
                width: extent[0],
                height: extent[1],
                stride: extent[0] * 4,
                format: PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
        base.asset = Some(asset);
    } else {
        let mut builder = SourceBuilder::new(
            extent,
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: SampleDepth::U8,
                profile: Default::default(),
                profile_assumed: true,
            },
            256 * 1024 * 1024,
        )
        .unwrap();
        for row in bytes.chunks_exact(extent[0] as usize * 4) {
            builder.push_row(row).unwrap();
        }
        base.source = Some(Arc::new(builder.finish().unwrap()));
    }
    if let Ok(path) = std::env::var("CAPY_FILTER_SOURCE_JPEG") {
        let source = layer_color::photo::read_photo(
            std::io::BufReader::new(std::fs::File::open(path).unwrap()),
            Default::default(),
        )
        .unwrap();
        let project = layer_color::photo_project(source, "Water", SampleDepth::U8).unwrap();
        base = project.document.layers[0].clone();
        eprintln!(
            "decoded_source_channels={:?} embedded_icc={} pose={:?}",
            base.source.as_ref().unwrap().interpretation.channels,
            matches!(
                base.source.as_ref().unwrap().interpretation.profile,
                layer_core::color::ColorProfile::Icc(_)
            ),
            base.properties.placement
        );
        if std::env::var_os("CAPY_FILTER_ASSUME_SRGB").is_some() {
            Arc::make_mut(base.source.as_mut().unwrap())
                .interpretation
                .profile = Default::default();
        }
    }
    let start = Instant::now();
    submit_frame(&mut r, &[base.clone()], 0., true, true);
    r.wait_idle().unwrap();
    eprintln!(
        "photo_loaded_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    eprintln!(
        "source_limits={:?}",
        r.scene.as_ref().unwrap().source_cache_limits()
    );
    r.set_telemetry_enabled(true);
    let name = std::env::var("CAPY_FILTER_NAME").unwrap_or_else(|_| "gaussian_blur".into());
    let mut layers = if name == "baseline" {
        vec![base]
    } else {
        vec![filter(fixture(&name)), base]
    };
    if name == "gaussian_blur" {
        let sigma = std::env::var("CAPY_FILTER_SIGMA")
            .ok()
            .map(|s| s.parse().unwrap())
            .unwrap_or(3.);
        Arc::make_mut(layers[0].effect.as_mut().unwrap())
            .set("sigma", EffectValue::Number(sigma))
            .unwrap();
    }
    let count = std::env::var("CAPY_FILTER_FRAMES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(8);
    eprintln!(
        "frame,case,cpu_ms,complete_ms,gpu_ms,window_submissions,display_submissions,source_misses,pass_pixels,image_bytes,cpu_phases"
    );
    for case in ["cold", "parameter", "cached", "animation"] {
        if case == "animation" && name != "domain_warp" {
            continue;
        }
        if name == "domain_warp" {
            Arc::make_mut(layers[0].effect.as_mut().unwrap())
                .set("animate", EffectValue::Toggle(case == "animation"))
                .unwrap();
        }
        for i in 0..if case == "cold" { 1 } else { count } {
            if case == "parameter" && name != "baseline" {
                layers[0].opacity = if i % 2 == 0 { 0.99 } else { 1. };
            }
            let before = r.metrics();
            let pixels = r.scene.as_ref().map_or(0, |s| s.image_pass_pixels());
            let start = Instant::now();
            submit_frame(
                &mut r,
                &layers,
                i as f32 / 60.,
                false,
                case == "cold" || case == "parameter",
            );
            let cpu = start.elapsed().as_secs_f64() * 1000.;
            r.wait_idle().unwrap();
            let complete = start.elapsed().as_secs_f64() * 1000.;
            let telemetry = r.telemetry.completed_snapshot(&r.device, &r.queue);
            let gpu = telemetry.gpu.ordered().last().copied().unwrap_or(f32::NAN);
            let m = r.metrics();
            eprintln!(
                "{i},{case},{cpu:.3},{complete:.3},{gpu:.3},{},{},{},{},{},{:?}",
                m.image_window_submissions - before.image_window_submissions,
                m.display_composition_submissions - before.display_composition_submissions,
                m.source_tile_misses - before.source_tile_misses,
                r.scene
                    .as_ref()
                    .map_or(0, |s| s.image_pass_pixels())
                    .saturating_sub(pixels),
                r.scene.as_ref().map_or(0, |s| s.image_cache_bytes()),
                m.frame_cpu_ms
            );
        }
    }
}
