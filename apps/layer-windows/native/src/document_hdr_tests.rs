// Included in document_workflows::tests to use the same native transaction fixture.
fn hdr_renderer(
    color: layer_core::color::DocumentColor,
) -> (Renderer, std::sync::Arc<crate::device::DeviceState>) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            assert!(
                Instant::now() < deadline,
                "Removed D3D12 device remains retained"
            );
            drop(adapter);
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        eprintln!("Windows HDR {:?}: {:?}", color, adapter.get_info());
        let features = adapter.features()
            & (wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        let state = crate::device::DeviceState::observe(&device);
        let mut gpu =
            WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, color).unwrap();
        gpu.finish_startup_cache();
        return (Renderer(Some(gpu)), state);
    }
}
fn hdr_delivery(
    host: &mut NativeHost,
    recipe: layer_ui::ExportRecipe,
    path: &std::path::Path,
) -> Vec<u8> {
    settle(host);
    let checkpoint = host.session.engine().checkpoint();
    let mut task = begin(host, CommandId::ExportDocument);
    ready(
        &mut task,
        Action::ExportOptions {
            recipe,
            profile_id: None,
        },
    );
    assert!(task.preview(0).is_ok() && task.preview(1).is_ok());
    ready(
        &mut task,
        Action::ExportWrite {
            path: path.to_str().unwrap().into(),
        },
    );
    task.complete(host, true).unwrap();
    drop(task);
    assert_eq!(host.session.engine().checkpoint(), checkpoint);
    std::fs::read(path).unwrap()
}
fn hdr_linear(host: &NativeHost) -> Vec<[f32; 4]> {
    let s = &host.session;
    let mut capture = s
        .engine()
        .backend()
        .0
        .as_ref()
        .unwrap()
        .snapshot_gpu()
        .capture(
            s.capture_project_recovery().unwrap(),
            s.engine().view().background_rgba_linear,
            0.,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    capture.preview_linear_document([32, 24]).unwrap().pixels
}
fn hdr_wait_tone(service: &mut crate::tone::Service, host: &mut NativeHost, generation: u64) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        service.poll(host, generation).unwrap();
        assert!(service.error.is_none(), "{:?}", service.error);
        if service.guide.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "HDR analysis timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
#[ignore = "Requires isolated CAPY_SETTINGS_DIRECTORY and hardware D3D12; removes its own devices"]
fn d3d12_windows_hdr_documents_delivery_history_cancellation_and_recovery() {
    use layer_core::color::{
        DocumentColor, hdr,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    use std::{io::Cursor, sync::Arc};
    let directory = std::path::PathBuf::from(
        std::env::var_os("CAPY_SETTINGS_DIRECTORY").expect("isolated profile required"),
    );
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    for depth in [SampleDepth::F16, SampleDepth::F32] {
        let color = DocumentColor {
            space: RgbSpace::DisplayP3,
            depth,
        };
        let mut project = layer_ui::NewDocumentOptions {
            extent: [32, 24],
            color,
            background: layer_ui::DocumentBackground::Transparent,
        }
        .project()
        .unwrap();
        let input = [4.125, 2., 0.5, 0.5];
        let sample = if depth == SampleDepth::F16 {
            hdr::encode_pixel(input)
                .unwrap()
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            input.into_iter().flat_map(f32::to_le_bytes).collect()
        };
        let mut source = SourceBuilder::new(
            [32, 24],
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth,
                profile: ColorProfile::Builtin(color.space),
                profile_assumed: false,
            },
            1024 * 1024,
        )
        .unwrap();
        for _ in 0..24 {
            source.push_row(&sample.repeat(32)).unwrap();
        }
        project.document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
        let (gpu, mut state) = hdr_renderer(color);
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session = UiSession::from_project(gpu, project, None, [128, 96]).unwrap();
        host.session.set_platform(Platform::Windows);
        host.session.set_document_replacement(true);
        host.document_adopted();
        settle(&mut host);
        let master = hdr_linear(&host);
        assert!(master.iter().any(|p| p[0] > 1.));
        let checkpoint = host.session.engine().checkpoint();
        let mut tone = crate::tone::Service::new(Arc::new(|| {}));
        hdr_wait_tone(&mut tone, &mut host, 1);
        let original = host.session.engine().document().sdr_rendition;
        let mut cancel = begin(&mut host, CommandId::SdrRendition);
        ready(&mut cancel, Action::Describe);
        cancel.control.cancel();
        assert!(cancel.commit(&mut host).is_err());
        cancel.complete(&mut host, false).unwrap();
        drop(cancel);
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        let png_path = directory.join(format!("{depth:?}-SDR.png"));
        let sdr = hdr_delivery(&mut host, layer_ui::ExportRecipe::web_share(), &png_path);
        let mut settings = begin(&mut host, CommandId::SdrRendition);
        ready(
            &mut settings,
            Action::SdrOptions {
                recipe: hdr::SdrRendition {
                    exposure: -1.,
                    ..original
                },
                pad: [0.3, -0.25],
            },
        );
        settings.commit(&mut host).unwrap();
        assert!(
            !host
                .session
                .state()
                .requests
                .iter()
                .any(|r| r.id == settings.id)
        );
        drop(settings);
        let recipe = host.session.engine().document().sdr_rendition;
        assert_ne!(recipe, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().sdr_rendition, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().sdr_rendition, recipe);
        assert_eq!(hdr_linear(&host), master);
        assert_ne!(
            hdr_delivery(&mut host, layer_ui::ExportRecipe::web_share(), &png_path),
            sdr
        );
        let exr = hdr_delivery(
            &mut host,
            layer_ui::ExportRecipe::further_editing(color),
            &directory.join(format!("{depth:?}.exr")),
        );
        let image =
            layer_color::photo::read_photo(Cursor::new(exr.clone()), Default::default()).unwrap();
        assert_eq!(image.interpretation.depth, SampleDepth::F32);
        assert_eq!(
            image.interpretation.profile,
            ColorProfile::Builtin(color.space)
        );
        let mut row = vec![0; image.row_bytes()];
        image.rows().read(0, &mut row).unwrap();
        assert_eq!(
            hdr::decode_samples(SampleDepth::F32, &row[..16]).unwrap(),
            input
        );
        let pq_recipe = layer_ui::ExportRecipe::web_share()
            .draft(layer_ui::ExportDraftAction::Format(
                layer_ui::ExportFormat::PngHdr,
            ))
            .recipe;
        let pq = hdr_delivery(
            &mut host,
            pq_recipe.clone(),
            &directory.join(format!("{depth:?}-PQ.png")),
        );
        let photo = layer_ui::read_import(
            Cursor::new(pq),
            layer_ui::ImportIntent::Open,
            Default::default(),
            "PQ",
            Default::default(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(photo.project.document.color.depth, SampleDepth::F16);
        let changed = hdr_delivery(&mut host, layer_ui::ExportRecipe::web_share(), &png_path);
        let proof = layer_core::color::ProofRecipe::new(
            "sRGB".into(),
            ColorProfile::Builtin(RgbSpace::Srgb),
        );
        host.session.set_proof_recipe(Some(proof)).unwrap();
        assert_eq!(
            hdr_delivery(&mut host, layer_ui::ExportRecipe::web_share(), &png_path),
            changed,
            "proof viewing must not reach SDR export"
        );
        assert_eq!(
            hdr_delivery(
                &mut host,
                layer_ui::ExportRecipe::further_editing(color),
                &directory.join("proof.exr")
            ),
            exr
        );
        host.session
            .set_proof_mode(layer_ui::ProofMode::Off)
            .unwrap();
        // Rasterize through the native worker and exercise exact storage history.
        let mut raster = begin(&mut host, CommandId::RasterizeSource);
        ready(
            &mut raster,
            Action::Prepare {
                choice: Value::Null,
                copy: false,
            },
        );
        assert!(raster.prepare_owner(&host).unwrap());
        ready(&mut raster, Action::Compare);
        raster.commit(&mut host).unwrap();
        drop(raster);
        settle(&mut host);
        let layers = host.session.engine().document().layers.clone();
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        settle(&mut host);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        settle(&mut host);
        assert_eq!(host.session.engine().document().layers, layers);
        assert_eq!(hdr_linear(&host), master);
        let file = directory.join(format!("HDR 日本語 {depth:?}.capy"));
        let saved = host.session.capture_project_recovery().unwrap();
        crate::document_io::atomic_write(&file, &Default::default(), |f| saved.write(f)).unwrap();
        let reopened =
            Project::read(std::fs::File::open(&file).unwrap(), Default::default()).unwrap();
        assert_eq!(reopened.document.layers, saved.document.layers);
        assert_eq!(reopened.document.color, color);
        assert_eq!(reopened.document.sdr_rendition, recipe);
        let environment = crate::documents::Environment::capture(&host).unwrap();
        let restored =
            crate::documents::prepare_recovery(environment, file, &Default::default()).unwrap();
        assert_eq!(restored.engine().document().layers, layers);
        assert!(!restored.state().soft_proof);
        drop(restored);
        // A canceled export must leave an existing destination byte-for-byte intact.
        let mut cancelled = begin(&mut host, CommandId::ExportDocument);
        ready(
            &mut cancelled,
            Action::ExportOptions {
                recipe: pq_recipe,
                profile_id: None,
            },
        );
        cancelled.control.cancel();
        cancelled.work(Action::ExportWrite {
            path: png_path.to_str().unwrap().into(),
        });
        assert!(cancelled.error.is_some());
        cancelled.complete(&mut host, false).unwrap();
        drop(cancelled);
        assert_eq!(std::fs::read(&png_path).unwrap(), changed);
        // Stop a pending analysis before releasing a removed process device.
        tone.poll(&mut host, 2).unwrap();
        crate::gpu_recovery_tests::remove_device(host.session.engine().backend(), &state);
        tone.stop().unwrap();
        drop(host.session.renderer_mut().0.take());
        let (replacement, next_state) = hdr_renderer(color);
        state = next_state;
        let revision = host.session.state().revision;
        let (old, change) = host.session.replace_renderer(replacement).unwrap();
        host.apply_change(revision, change);
        drop(old);
        host.startup = Default::default();
        settle(&mut host);
        assert_eq!(host.session.engine().document().layers, layers);
        assert_eq!(hdr_linear(&host), master);
        hdr_wait_tone(&mut tone, &mut host, 3);
        tone.stop().unwrap();
        state.check().unwrap();
        assert_eq!(
            hdr_delivery(
                &mut host,
                layer_ui::ExportRecipe::further_editing(color),
                &directory.join("recovered.exr")
            ),
            exr
        );
        // Explicit precision change is one undoable shared transaction.
        let target = if depth == SampleDepth::F16 {
            SampleDepth::F32
        } else {
            SampleDepth::F16
        };
        let mut change = begin(&mut host, CommandId::ChangeBitDepth);
        ready(
            &mut change,
            Action::Prepare {
                choice: json!({"Depth":{"depth":target,"dither":"None"}}),
                copy: false,
            },
        );
        change.commit(&mut host).unwrap();
        drop(change);
        settle(&mut host);
        assert_eq!(host.session.engine().document().color.depth, target);
        let mut undo = begin(&mut host, CommandId::Undo);
        ready(&mut undo, Action::Describe);
        undo.commit(&mut host).unwrap();
        drop(undo);
        settle(&mut host);
        assert_eq!(host.session.engine().document().color, color);
        assert_eq!(host.session.engine().document().layers, layers);
        state.check().unwrap();
    }
}

#[test]
#[ignore = "Requires isolated CAPY_SETTINGS_DIRECTORY and hardware D3D12"]
fn d3d12_windows_float32_signed_range_rejects_lossy_demotion_and_protects_exports() {
    use layer_core::color::{
        DocumentColor, hdr,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    use std::{io::Cursor, sync::Arc};
    let directory = std::path::PathBuf::from(
        std::env::var_os("CAPY_SETTINGS_DIRECTORY").expect("isolated profile required"),
    );
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let color = DocumentColor {
        space: RgbSpace::Srgb,
        depth: SampleDepth::F32,
    };
    let mut project = layer_ui::NewDocumentOptions {
        extent: [3, 1],
        color,
        background: layer_ui::DocumentBackground::Transparent,
    }
    .project()
    .unwrap();
    let input = [
        [100000.125f32, -0.125, 2., 0.5],
        [4., 2., 1., 1.],
        [1e-20, -1., 4., 1. / 65536.],
    ];
    let mut builder = SourceBuilder::new(
        [3, 1],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    builder
        .push_row(
            &input
                .into_iter()
                .flatten()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    project.document.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    let (gpu, state) = hdr_renderer(color);
    let mut host = NativeHost::new(Platform::Windows).unwrap();
    host.session = UiSession::from_project(gpu, project, None, [128, 96]).unwrap();
    host.session.set_platform(Platform::Windows);
    host.session.set_document_replacement(true);
    host.document_adopted();
    settle(&mut host);
    let mut raster = begin(&mut host, CommandId::RasterizeSource);
    ready(
        &mut raster,
        Action::Prepare {
            choice: Value::Null,
            copy: false,
        },
    );
    assert!(raster.prepare_owner(&host).unwrap());
    ready(&mut raster, Action::Compare);
    raster.commit(&mut host).unwrap();
    drop(raster);
    settle(&mut host);
    let checkpoint = host.session.engine().checkpoint();
    let layers = host.session.engine().document().layers.clone();
    let exr = hdr_delivery(
        &mut host,
        layer_ui::ExportRecipe::further_editing(color),
        &directory.join("signed.exr"),
    );
    let decoded = layer_color::photo::read_photo(Cursor::new(exr), Default::default()).unwrap();
    let mut row = vec![0; decoded.row_bytes()];
    decoded.rows().read(0, &mut row).unwrap();
    for (bytes, expected) in row.as_chunks::<16>().0.iter().zip(input) {
        assert_eq!(
            hdr::decode_samples(SampleDepth::F32, bytes)
                .unwrap()
                .map(f32::to_bits),
            expected.map(f32::to_bits)
        );
    }
    let mut demote = begin(&mut host, CommandId::ChangeBitDepth);
    demote.work(Action::Prepare {
        choice: json!({"Depth":{"depth":"F16","dither":"None"}}),
        copy: false,
    });
    assert!(demote.error.is_some());
    assert!(demote.commit(&mut host).is_err());
    demote.complete(&mut host, false).unwrap();
    drop(demote);
    assert_eq!(host.session.engine().checkpoint(), checkpoint);
    assert_eq!(host.session.engine().document().layers, layers);
    let path = directory.join("protected.png");
    std::fs::write(&path, b"existing destination").unwrap();
    let mut export = begin(&mut host, CommandId::ExportDocument);
    ready(
        &mut export,
        Action::ExportOptions {
            recipe: layer_ui::ExportRecipe::web_share()
                .draft(layer_ui::ExportDraftAction::Format(
                    layer_ui::ExportFormat::PngHdr,
                ))
                .recipe,
            profile_id: None,
        },
    );
    export.work(Action::ExportWrite {
        path: path.to_str().unwrap().into(),
    });
    assert!(export.error.is_some());
    export.complete(&mut host, false).unwrap();
    drop(export);
    assert_eq!(std::fs::read(&path).unwrap(), b"existing destination");
    let mapped = hdr_delivery(
        &mut host,
        layer_ui::ExportRecipe::web_share()
            .draft(layer_ui::ExportDraftAction::Format(
                layer_ui::ExportFormat::PngHdrMapped,
            ))
            .recipe,
        &path,
    );
    assert_eq!(
        layer_color::photo::read_photo(Cursor::new(mapped), Default::default())
            .unwrap()
            .interpretation
            .depth,
        SampleDepth::F16
    );
    assert_eq!(host.session.engine().checkpoint(), checkpoint);
    assert_eq!(host.session.engine().document().layers, layers);
    state.check().unwrap();
}
