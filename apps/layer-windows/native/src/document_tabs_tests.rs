use super::*;
use layer_ui::{CommandId, Platform, UiAction};
fn command(host: &mut NativeHost, command: CommandId) {
    host.dispatch(UiAction::Invoke { command }).unwrap();
}
fn request(host: &NativeHost) -> (u32, u64, u64) {
    let file = &host.session.state().document_file;
    (
        host.session
            .state()
            .requests
            .iter()
            .find(|r| matches!(r.kind, HostRequestKind::Document { .. }))
            .unwrap()
            .id,
        file.epoch,
        file.revision,
    )
}
fn settle(service: &mut DocumentService, host: &mut NativeHost) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        service.poll(host).unwrap();
        if host.session.engine().backend().0.is_some() {
            host.prepare_canvas_frame(0, 0, true).unwrap();
        }
        if service.idle()
            && !service.close_next
            && !host.session.state().document_file.busy
            && !(host.session.state().document_file.close_ready
                && service.window.documents.order().len() > 1)
            && host.session.can_park_document()
            && host
                .session
                .retained_document_tiles()
                .try_blobs()
                .unwrap()
                .is_some()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "drawing transition did not settle: {:?}",
            service.tabs_view(host)
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn save(service: &mut DocumentService, host: &mut NativeHost, path: &std::path::Path) {
    command(host, CommandId::SaveDocumentAs);
    let (id, _, _) = request(host);
    service
        .dispatch(
            host,
            DocumentAction::Save {
                id,
                path: path.to_str().unwrap().into(),
            },
        )
        .unwrap();
    settle(service, host);
}
#[test]
#[ignore = "Requires hardware D3D12 and an isolated CAPY_SETTINGS_DIRECTORY"]
fn d3d12_retained_drawing_tabs_spill_history_save_close_and_cancel() {
    use layer_core::color::{
        ColorProfile, SampleDepth,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    let directory = crate::settings::data_directory().unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & (wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .unwrap();
    let mut gpu =
        WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, Default::default())
            .unwrap();
    gpu.finish_startup_cache();
    assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
    assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
    let mut project = layer_ui::new_drawing(32, 24).unwrap();
    let mut source = SourceBuilder::new(
        [32, 24],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(project.document.color.space),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for _ in 0..24 {
        source.push_row(&[230, 45, 20, 180].repeat(32)).unwrap();
    }
    project.document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
    let mut host = NativeHost::new(Platform::Windows).unwrap();
    host.session = UiSession::from_project(Renderer(Some(gpu.into())), project, None, [96, 72], Platform::Windows).unwrap();
    host.resize(96, 72, 1.).unwrap();
    let mut service = DocumentService::open(|| {}).unwrap();
    service.start_recovery().unwrap();
    settle(&mut service, &mut host);
    host.dispatch(UiAction::SetLayerOpacity {
        id: None,
        opacity: 0.25,
    })
    .unwrap();
    settle(&mut service, &mut host);
    let saved = directory.join("first 日本語.capy");
    save(&mut service, &mut host, &saved);
    host.dispatch(UiAction::SetLayerOpacity {
        id: None,
        opacity: 0.75,
    })
    .unwrap();
    settle(&mut service, &mut host);
    let first = host.session.engine().checkpoint();
    let original_pixels = super::super::gpu_tests::image(&mut host);
    command(&mut host, CommandId::NewDocument);
    let (id, _, _) = request(&host);
    assert!(
        matches!(
            DocumentService::request(&host, id).unwrap(),
            DocumentRequest::New
        ),
        "New must retain dirty artwork without a discard prompt"
    );
    service
        .dispatch(&mut host, DocumentAction::Cancel { id })
        .unwrap();
    assert_eq!(service.window.documents.order(), [1]);
    assert_eq!(host.session.engine().checkpoint(), first);
    command(&mut host, CommandId::NewDocument);
    let (id, epoch, revision) = request(&host);
    service
        .dispatch(
            &mut host,
            DocumentAction::Create {
                id,
                epoch,
                revision,
                options: layer_ui::NewDocumentOptions {
                    extent: [40, 30],
                    ..Default::default()
                },
                preset: String::new(),
                defaults: false,
            },
        )
        .unwrap();
    settle(&mut service, &mut host);
    assert_eq!(service.window.documents.order(), [1, 2]);
    assert_eq!(service.tabs_view(&host)["parked_renderers"], 0);
    assert!(
        service
            .window
            .documents
            .parked_owner_mut(1)
            .unwrap()
            .session
            .state()
            .document_file
            .modified
    );
    let backing = directory.join("drawing-backing");
    std::fs::write(&backing, b"fixture blocks storage directory").unwrap();
    service.window.documents.budget.inactive_ram = 0;
    settle(&mut service, &mut host);
    assert!(service.window.documents.storage_error().is_some());
    assert!(service.window.documents.resident_bytes() > 0);
    assert_eq!(service.window.documents.order(), [1, 2]);
    std::fs::remove_file(&backing).unwrap();
    service.tab_action(&mut host, Action::RetryStorage).unwrap();
    settle(&mut service, &mut host);
    assert!(service.window.documents.storage_error().is_none());
    assert_eq!(service.window.documents.resident_bytes(), 0);
    let hits = |order: [u64; 2]| {
        order
            .iter()
            .enumerate()
            .map(|(i, &id)| layer_ui::DocumentTabHit {
                id,
                bounds: layer_ui::Bounds { x: 10. + i as f32 * 106., y: 1., width: 100., height: 32. },
            })
            .collect::<Vec<_>>()
    };
    let clip = layer_ui::Bounds { x: 10., y: 1., width: 206., height: 32. };
    let slide = |order, point| Action::Slide { id: 1, hits: hits(order), clip, press: [40., 17.], point };
    service.tab_action(&mut host, slide([1, 2], [500., 50.])).unwrap();
    assert_eq!(service.window.documents.order(), [1, 2]);
    service.tab_action(&mut host, slide([2, 1], [500., 17.])).unwrap();
    assert_eq!(service.window.documents.order(), [1, 2]);
    service.tab_action(&mut host, slide([1, 2], [500., 17.])).unwrap();
    assert_eq!(service.window.documents.order(), [2, 1]);
    service
        .tab_action(&mut host, Action::History { redo: false })
        .unwrap();
    assert_eq!(service.window.documents.order(), [1, 2]);
    service
        .tab_action(
            &mut host,
            Action::Reorder {
                id: 2,
                before: Some(1),
            },
        )
        .unwrap();
    assert_eq!(service.window.documents.order(), [2, 1]);
    service
        .tab_action(&mut host, Action::History { redo: false })
        .unwrap();
    assert_eq!(service.window.documents.order(), [1, 2]);
    service
        .tab_action(&mut host, Action::History { redo: true })
        .unwrap();
    assert_eq!(service.window.documents.order(), [2, 1]);
    service
        .tab_action(&mut host, Action::Select { id: 1 })
        .unwrap();
    settle(&mut service, &mut host);
    assert_eq!(host.session.engine().checkpoint(), first);
    assert_eq!(
        super::super::gpu_tests::image(&mut host).bytes,
        original_pixels.bytes
    );
    command(&mut host, CommandId::Undo);
    settle(&mut service, &mut host);
    assert!(!host.session.state().document_file.modified);
    assert!((host.session.engine().document().layers[0].opacity - 0.25).abs() < 1e-6);
    command(&mut host, CommandId::Redo);
    settle(&mut service, &mut host);
    assert_eq!(host.session.engine().checkpoint(), first);
    service
        .tab_action(&mut host, Action::Select { id: 2 })
        .unwrap();
    settle(&mut service, &mut host);
    host.dispatch(UiAction::SetLayerOpacity {
        id: None,
        opacity: 0.4,
    })
    .unwrap();
    settle(&mut service, &mut host);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        service.poll(&mut host).unwrap();
        let copies: Vec<_> = std::fs::read_dir(directory.join("recovery"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "capy"))
            .collect();
        let mut snapshots = Vec::new();
        for copy in copies {
            if let Ok(file) = std::fs::File::open(copy.path())
                && let Ok(p) = Project::read(file, ProjectLimits::default())
            {
                snapshots.push((p.document.width, p.document.layers[0].opacity));
            }
        }
        if snapshots.contains(&(32, 0.75)) && snapshots.contains(&(40, 0.4)) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "active and parked drawings need independent durable checkpoints: {snapshots:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    service
        .tab_action(&mut host, Action::Close { id: 2 })
        .unwrap();
    let (id, epoch, revision) = request(&host);
    service
        .dispatch(
            &mut host,
            DocumentAction::RespondClose {
                id,
                epoch,
                revision,
                decision: CloseDecision::Cancel,
            },
        )
        .unwrap();
    assert_eq!(service.window.documents.order().len(), 2);
    assert_eq!(service.window.documents.selected(), 2);
    service
        .tab_action(&mut host, Action::Close { id: 2 })
        .unwrap();
    let (id, epoch, revision) = request(&host);
    service
        .dispatch(
            &mut host,
            DocumentAction::RespondClose {
                id,
                epoch,
                revision,
                decision: CloseDecision::Save,
            },
        )
        .unwrap();
    let (id, _, _) = request(&host);
    let second = directory.join("second.capy");
    service
        .dispatch(
            &mut host,
            DocumentAction::Save {
                id,
                path: second.to_str().unwrap().into(),
            },
        )
        .unwrap();
    settle(&mut service, &mut host);
    assert_eq!(service.window.documents.order(), [1]);
    assert_eq!(host.session.engine().checkpoint(), first);
    assert!(
        !service.window.documents.can_undo(),
        "close invalidates order history without resurrecting a drawing"
    );
    command(&mut host, CommandId::OpenDocument);
    let (id, epoch, revision) = request(&host);
    service
        .dispatch(
            &mut host,
            DocumentAction::Open {
                id,
                epoch,
                revision,
                path: second.to_str().unwrap().into(),
            },
        )
        .unwrap();
    settle(&mut service, &mut host);
    assert_eq!(service.window.documents.order(), [1, 3]);
    assert!((host.session.engine().document().layers[0].opacity - 0.4).abs() < 1e-6);
    assert!(!host.session.state().document_file.modified);
    // Cancel an accepted background Open and preserve both existing editors.
    command(&mut host, CommandId::OpenDocument);
    let (id, epoch, revision) = request(&host);
    service
        .dispatch(
            &mut host,
            DocumentAction::Open {
                id,
                epoch,
                revision,
                path: saved.to_str().unwrap().into(),
            },
        )
        .unwrap();
    service
        .dispatch(&mut host, DocumentAction::Cancel { id })
        .unwrap();
    settle(&mut service, &mut host);
    assert_eq!(service.window.documents.order(), [1, 3]);
    // Failed activation retains selectable/saveable CPU editors and their backing.
    host.session.suspend_renderer().unwrap();
    service.renderer_unavailable(&mut host).unwrap();
    service
        .worker
        .retire_renderer(Renderer(host.session.renderer_mut().0.take()));
    service
        .tab_action(&mut host, Action::Select { id: 1 })
        .unwrap();
    assert_eq!(service.window.documents.selected(), 1);
    assert_eq!(host.document_count, 2);
    assert!(host.session.rendering_suspended());
    assert!(host.error.is_some());
    assert_eq!(host.session.engine().checkpoint(), first);
    let rescue = directory.join("saved without GPU.capy");
    save(&mut service, &mut host, &rescue);
    let reopened = Project::read(
        std::fs::File::open(rescue).unwrap(),
        ProjectLimits::default(),
    )
    .unwrap();
    assert_eq!(reopened.document.layers[0].opacity, 0.75);
    service.stop_worker().unwrap();
}
