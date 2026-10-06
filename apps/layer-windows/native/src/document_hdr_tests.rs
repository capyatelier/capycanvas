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
    ready(&mut task, Action::Describe);
    ready(
        &mut task,
        Action::ExportOptions {
            recipe,
            profile_id: None,
        },
    );
    task.preview(0).unwrap(); task.preview(1).unwrap();
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
            s.capture_artwork().unwrap(),
            Default::default(),
        )
        .unwrap();
    capture.preview_linear_document([32, 24]).unwrap().pixels
}
#[test]
#[ignore = "Requires isolated CAPY_STORAGE_DIR and hardware D3D12; removes its own devices"]
fn d3d12_windows_hdr_documents_delivery_history_cancellation_and_recovery() {
    use layer_core::color::{
        DocumentColor, hdr,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    use std::{io::Cursor, sync::Arc};
    let directory = crate::test_support::isolated_storage();
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
            ..Default::default()
        }
        .project(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English))
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
        paint_mut(&mut project,0).base = Some(layer_core::PaintBase::new((Arc::new(source.finish().unwrap())).into()));
        let (gpu, mut state) = hdr_renderer(color);
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session = UiSession::from_project(gpu, project, None, [128, 96], Platform::Windows).unwrap();
        host.session.set_document_replacement(true);
        host.document_adopted();
        settle(&mut host);
        let master = hdr_linear(&host);
        assert!(master.iter().any(|p| p[0] > 1.));
        let checkpoint = host.session.engine().checkpoint();
        let original = host.session.engine().document().output().sdr;
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
        assert_eq!(host.session.engine().document().output().sdr, original);
        panel_edit(&mut host, epoch, "down");
        assert!(host.session.require_document_snapshot_idle().is_err());
        panel_edit(&mut host, epoch, "cancel");
        assert_eq!(host.session.engine().document().output().sdr, original);
        panel_edit(&mut host, epoch, "down");
        panel_edit(&mut host, epoch, "up");
        assert_eq!(host.session.engine().document().output().sdr, panel_recipe);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().output().sdr, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().output().sdr, panel_recipe);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().output().sdr, original);
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
        let recipe = host.session.engine().document().output().sdr;
        assert_ne!(recipe, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().output().sdr, original);
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().output().sdr, recipe);
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
            .draft_canonical(layer_ui::ExportDraftAction::Format(
                layer_ui::ExportFormat::PngHdr,
            ))
            .recipe;
        let pq = hdr_delivery(
            &mut host,
            pq_recipe.clone(),
            &directory.join(format!("{depth:?}-PQ.png")),
        );
        let photo = editable(layer_ui::read_import(
            Cursor::new(pq),
            layer_ui::ImportIntent::Open,
            Default::default(),
            layer_ui::photo_document_names("PQ", &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)),
            Default::default(),
            Default::default(),
            &Default::default(),
        )
        .unwrap());
        assert_eq!(photo.project.composition().color.depth, SampleDepth::F16);
        for format in [
            layer_ui::ExportFormat::JpegHdr,
            layer_ui::ExportFormat::JpegHdrMapped,
            layer_ui::ExportFormat::AvifHdr,
            layer_ui::ExportFormat::AvifHdrMapped,
        ] {
            let mut recipe = layer_ui::ExportRecipe::web_share()
                .draft_for_color_canonical(color, layer_ui::ExportDraftAction::Format(format))
                .recipe;
            if format.gainmap() == Some(layer_color::photo::GainMapFormat::Jpeg) {
                recipe = recipe
                    .draft_for_color_canonical(
                        color,
                        layer_ui::ExportDraftAction::Background(layer_ui::ExportBackground::White),
                    )
                    .recipe;
            }
            let path = directory.join(format!("{depth:?}-{format:?}.{}", format.extension()));
            let bytes = hdr_delivery(&mut host, recipe.clone(), &path);
            let imported = editable(layer_ui::read_import(
                Cursor::new(bytes.clone()),
                layer_ui::ImportIntent::Open,
                Default::default(),
                layer_ui::photo_document_names("gain-map photo", &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)),
                Default::default(),
                Default::default(),
                &Default::default(),
            )
            .unwrap());
            assert!(imported.project.composition().color.depth.is_float());
            assert_eq!(
                [
                    imported.project.composition().size[0],
                    imported.project.composition().size[1]
                ],
                [32, 24]
            );
            let source=imported.project.artwork.paint.iter().find_map(|(_,_,p)|p.base.as_ref().map(|base|&base.image)).unwrap();
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
        assert!(raster.prepare_owner(&mut host).unwrap().is_some());
        ready(&mut raster, Action::Compare);
        raster.commit(&mut host).unwrap();
        drop(raster);
        settle(&mut host);
        let layers = host.session.engine().document().clone();
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
        assert_authored_eq(host.session.engine().document(),&layers);
        assert_eq!(hdr_linear(&host), master);
        let file = directory.join(format!("HDR 日本語 {depth:?}.capy"));
        let saved = host.session.capture_artwork().unwrap();
        crate::document_io::atomic_write(&file, &Default::default(), |f| write_capture(&saved,f)).unwrap();
        let reopened =
            read_document(std::fs::File::open(&file).unwrap(), Default::default()).unwrap();
        assert_capture_eq(&reopened,&saved);
        assert_eq!(reopened.composition().color, color);
        assert_eq!(reopened.output().sdr, recipe);
        let environment = crate::documents::recovery_environment(&host.session).unwrap();
        let restored=crate::documents::prepare_package(environment, file, &Default::default()).unwrap();
        assert_authored_eq(restored.engine().document(),&layers);
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
        crate::gpu_recovery_tests::remove_device(host.session.engine().backend(), &state);
        drop(host.session.renderer_mut().0.take());
        let (replacement, next_state) = hdr_renderer(color);
        state = next_state;
        let revision = host.session.state().revision;
        let (old, change) = host.session.replace_renderer(replacement).unwrap();
        host.apply_change(revision, change);
        drop(old);
        host.startup = Default::default();
        settle(&mut host);
        assert_authored_eq(host.session.engine().document(),&layers);
        assert_eq!(hdr_linear(&host), master);
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
        assert_eq!(host.session.engine().document().composition().color.depth, target);
        let mut undo = begin(&mut host, CommandId::Undo);
        ready(&mut undo, Action::Describe);
        undo.commit(&mut host).unwrap();
        drop(undo);
        settle(&mut host);
        assert_eq!(host.session.engine().document().composition().color, color);
        assert_authored_eq(host.session.engine().document(),&layers);
        state.check().unwrap();
    }
}
