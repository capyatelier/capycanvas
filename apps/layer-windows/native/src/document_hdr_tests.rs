// Included in document_workflows::tests to use the same native transaction fixture.
use crate::device::D3d12Watch;
fn hdr_renderer(
    color: layer_core::color::DocumentColor,
) -> (Renderer, layer_host::DeviceWatch) {
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
        let state = layer_host::DeviceWatch::observe(&device);
        let mut gpu =
            WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, color).unwrap();
        gpu.finish_startup_cache();
        return (Renderer(Some(gpu.into())), state);
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
fn hdr_wait_tone(service: &mut layer_host::tone::ToneService, host: &mut NativeHost) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        service.tick(host).unwrap();
        assert!(service.error.is_none(), "{:?}", service.error);
        if service.status()["ready"] == true {
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
        host.session = UiSession::from_project(gpu, project, None, [128, 96], Platform::Windows).unwrap();
        host.session.set_document_replacement(true);
        host.document_adopted();
        settle(&mut host);
        let master = hdr_linear(&host);
        assert!(master.iter().any(|p| p[0] > 1.));
        let checkpoint = host.session.engine().checkpoint();
        let mut tone = layer_host::tone::ToneService::new(Some(Arc::new(|| {})));
        hdr_wait_tone(&mut tone, &mut host);
        let downloaded = tone
            .guide
            .as_ref()
            .unwrap()
            .download(host.session.engine().backend().0.as_ref().unwrap().queue())
            .unwrap();
        let mut reference = hdr::LocalToneBuilder::new([32, 24], color.space).unwrap();
        for row in master.as_chunks::<32>().0 {
            reference.push(row).unwrap();
        }
        let reference = reference.finish(|| false).unwrap();
        assert_eq!(downloaded.extent, reference.extent);
        for (gpu, cpu) in downloaded.samples.iter().zip(&reference.samples) {
            for c in 0..3 {
                assert!(
                    (gpu[c] - cpu[c]).abs() < 0.01,
                    "GPU tone guide differs from CPU oracle"
                );
            }
        }
        let publications = tone.status()["publications"].clone();
        let original = host.session.engine().document().sdr_rendition;
        let epoch = host.session.state().document_file.epoch;
        let panel_recipe = hdr::SdrRendition {
            exposure: -0.5,
            ..original
        };
        let panel_edit = |host: &mut NativeHost, epoch: u64, phase: &str| {
            serde_json::from_value::<crate::actions::Action>(serde_json::json!({
                "windows_epoch":epoch.to_string(),
                "windows_proof_action":{"type":"rendition","phase":phase,"recipe":panel_recipe}
            }))
            .unwrap()
            .dispatch(host)
            .unwrap();
        };
        panel_edit(&mut host, epoch + 1, "down");
        panel_edit(&mut host, epoch + 1, "up");
        assert_eq!(host.session.engine().document().sdr_rendition, original);
        panel_edit(&mut host, epoch, "down");
        assert!(host.session.require_document_snapshot_idle().is_err());
        panel_edit(&mut host, epoch, "cancel");
        assert_eq!(host.session.engine().document().sdr_rendition, original);
        panel_edit(&mut host, epoch, "down");
        panel_edit(&mut host, epoch, "up");
        assert_eq!(host.session.engine().document().sdr_rendition, panel_recipe);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().sdr_rendition, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().sdr_rendition, panel_recipe);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().sdr_rendition, original);
        host.dispatch(UiAction::Invoke { command: CommandId::SdrRendition }).unwrap();
        assert!(host.session.state().requests.is_empty());
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        let png_path = directory.join(format!("{depth:?}-SDR.png"));
        let sdr = hdr_delivery(&mut host, layer_ui::ExportRecipe::web_share(), &png_path);
        let adjusted = layer_ui::proof_panel::sdr_from_pad(hdr::SdrRendition { exposure: -1., ..original }, [0.3, -0.25]);
        for phase in [layer_ui::ContactPhase::Down, layer_ui::ContactPhase::Up] {
            let change = layer_ui::proof_panel::apply(&mut host.session, layer_ui::proof_panel::ProofAction::Rendition { phase, recipe: adjusted }).unwrap();
            host.apply_change(host.session.state().revision, change);
        }
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
        tone.tick(&host).unwrap();
        assert_eq!(
            tone.status()["publications"],
            publications,
            "SDR recipe changes reuse the GPU guide"
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
        for format in [
            layer_ui::ExportFormat::JpegHdr,
            layer_ui::ExportFormat::JpegHdrMapped,
            layer_ui::ExportFormat::AvifHdr,
            layer_ui::ExportFormat::AvifHdrMapped,
        ] {
            let mut recipe = layer_ui::ExportRecipe::web_share()
                .draft_for_color(color, layer_ui::ExportDraftAction::Format(format))
                .recipe;
            if format.gainmap() == Some(layer_color::photo::GainMapFormat::Jpeg) {
                recipe = recipe
                    .draft_for_color(
                        color,
                        layer_ui::ExportDraftAction::Background(layer_ui::ExportBackground::White),
                    )
                    .recipe;
            }
            let path = directory.join(format!("{depth:?}-{format:?}.{}", format.extension()));
            let bytes = hdr_delivery(&mut host, recipe.clone(), &path);
            let imported = layer_ui::read_import(
                Cursor::new(bytes.clone()),
                layer_ui::ImportIntent::Open,
                Default::default(),
                "gain-map photo",
                Default::default(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
            assert!(imported.project.document.color.depth.is_float());
            assert_eq!(
                [
                    imported.project.document.width,
                    imported.project.document.height
                ],
                [32, 24]
            );
            let source = imported
                .project
                .document
                .layers
                .iter()
                .find_map(|l| l.source.as_ref())
                .unwrap();
            let mut row = vec![0; source.row_bytes()];
            source.rows().read(0, &mut row).unwrap();
            assert!(
                hdr::decode_samples(
                    source.interpretation.depth,
                    &row[..source.interpretation.depth.bytes() * 4]
                )
                .unwrap()[0]
                    > 1.
            );
            let mut canceled = begin(&mut host, CommandId::ExportDocument);
            ready(
                &mut canceled,
                Action::ExportOptions {
                    recipe,
                    profile_id: None,
                },
            );
            canceled.control.cancel();
            canceled.work(Action::ExportWrite {
                path: path.to_str().unwrap().into(),
            });
            assert!(canceled.error.is_some());
            canceled.complete(&mut host, false).unwrap();
            drop(canceled);
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "Canceled gain-map output replaced its destination"
            );
        }
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
        let environment = crate::documents::recovery_environment(&host.session).unwrap();
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
        tone.clear();
        tone.tick(&host).unwrap();
        assert!(
            tone.guide.is_none(),
            "Old-device guide survived a device retirement"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while tone.status()["pending"] != true {
            tone.tick(&host).unwrap();
            assert!(
                Instant::now() < deadline,
                "Recovery fixture never started analysis"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        crate::gpu_recovery_tests::remove_device(host.session.engine().backend(), &state);
        tone.clear();
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
        hdr_wait_tone(&mut tone, &mut host);
        tone.clear();
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
    host.session = UiSession::from_project(gpu, project, None, [128, 96], Platform::Windows).unwrap();
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
