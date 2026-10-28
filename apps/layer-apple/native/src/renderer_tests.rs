use super::*;
use std::io::Seek;
use std::os::fd::AsRawFd;

fn install(app: &App) {
    let gpu = native_renderer();
    let mut lease_app=app.borrow();let app=&mut *lease_app;
    app.metal.install_renderer(&mut app.host, gpu).unwrap();
}

#[test]
fn apple_idle_paper_presentation_notifies_once_and_settles_without_input() {
    let reference = native_renderer();
    for platform in [0, 1] {
        let app = App::new(platform);
        let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(app.document().scene(), None).unwrap());
        app.action(json!({"type":"select_layer","id":paper}));
        app.action(json!({"type":"effect","action":{"op":"set","layer":paper,"key":"texture","value":{"kind":"choice","value":2}}}));
        app.action(json!({"type":"effect","action":{"op":"set","layer":paper,"key":"relief","value":{"kind":"number","value":150}}}));
        let gpu = layer_host::GpuContext::of(&reference).rasterizer(Default::default(), &Default::default(), true).unwrap();
        app.with(|native| native.metal.install_renderer(&mut native.host, gpu).unwrap());
        assert_eq!(unsafe { capy_apple_resize(app.0, 128, 128, 1.) }, 0);
        app.draw_until_idle();
        let mut native = app.borrow();
        let host = &mut native.host;
        let gpu = host.session.engine().backend().0.as_ref().unwrap();
        gpu.shader_idle(false, false);
        let view = host.session.state().camera.view();
        let surround = host.session.state().palette.surround_linear;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("Apple idle paper"),
            size: wgpu::Extent3d { width: view.width_px, height: view.height_px, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
        });
        let target = texture.create_view(&Default::default());
        let mut presenter = layer_render_wgpu::ViewportPresenter::for_surface(gpu, format,
            layer_render_wgpu::SdrSurfaceColor::Srgb).unwrap();
        pollster::block_on(presenter.prepare(gpu)).unwrap();
        presenter.present(gpu, &target, view, surround).unwrap();
        assert!(!presenter.paper_lighting_drawn());
        host.dirty = false;
        crate::metal::MetalHost::poll_presenter(host, &mut presenter).unwrap();
        assert!(!host.dirty, "parked lighting must not request a frame");
        assert!(!crate::metal::MetalHost::poll_presenter(host, &mut presenter).unwrap());
        let revision = host.session.engine().document().revision;
        host.session.engine().backend().0.as_ref().unwrap().shader_idle(true, false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let notify = crate::metal::MetalHost::poll_presenter(host, &mut presenter).unwrap();
            if host.dirty { assert!(notify, "the idle health poll must wake the Swift frame driver"); break; }
            assert!(std::time::Instant::now() < deadline, "idle paper lighting did not become ready");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!crate::metal::MetalHost::poll_presenter(host, &mut presenter).unwrap(), "an outstanding frame needs no duplicate notification");
        let gpu = host.session.engine().backend().0.as_ref().unwrap();
        presenter.present(gpu, &target, view, surround).unwrap();
        assert!(presenter.paper_lighting_drawn());
        host.dirty = false;
        for _ in 0..4 {
            assert!(!crate::metal::MetalHost::poll_presenter(host, &mut presenter).unwrap());
            assert!(!host.dirty, "settled paper must remain idle");
        }
        assert_eq!(host.session.engine().document().revision, revision);
    }
}

#[test]
fn canvas_update_counter_observes_paint_without_advancing_it() {
    assert_eq!(unsafe { capy_apple_submitted_canvas_updates(0) }, 0);
    for platform in [0, 1] {
        let app = App::new(platform);
        assert_eq!(unsafe { capy_apple_submitted_canvas_updates(app.0) }, 0);
        install(&app);
        app.draw_until_idle();
        let before = unsafe { capy_apple_submitted_canvas_updates(app.0) };
        let pixels = app.pixels();
        app.stroke();
        app.draw_until_idle();
        let painted = unsafe { capy_apple_submitted_canvas_updates(app.0) };
        assert!(painted > before);
        assert_ne!(app.pixels(), pixels);
        assert_eq!(unsafe { capy_apple_submitted_canvas_updates(app.0) }, painted);
        assert_eq!(unsafe { capy_apple_frame(app.0, 3_000_000_000, 3_000_000_000, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { capy_apple_submitted_canvas_updates(app.0) }, painted);
    }
}

#[test]
fn renderer_failure_retains_sources_history_settings_and_durable_recovery_on_both_apple_hosts() {
    for platform in [0, 1] {
        for fault in ["restart", "device_loss", "validation"] {
            let app = App::new(platform);
            install(&app);
            app.draw_until_idle();
            let source: Vec<u8> = (0..64 * 64).flat_map(|i| [180, (i % 200) as u8, 75, 220]).collect();
            app.place_rgba("Retained source", 64, 64, &source);
            app.invoke("add_layer");
            app.invoke("pen");
            app.draw_until_idle();
            let imported = app.pixels();
            app.stroke();
            app.draw_until_idle();
            let painted = app.pixels();
            assert_ne!(painted, imported);
            for (_, _, source) in app.document().artwork.paint.iter() {
                raster_samples(&source.raster);
            }
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), imported);
            app.action(json!({"type":"set_brush_size","value":47}));
            let state = app.state();
            let (committed,checkpoint,device)=app.with(|native| {
                let session=&native.host.session;
                (session.engine().document().clone(),session.engine().checkpoint(),session.engine().backend().0.as_ref().unwrap().device().clone())
            });
            let revision = unsafe { capy_apple_camera_revision(app.0) };
            let down = [450.,350.,1.,0.,0.,0.,0.,3_000_000_000.,1.];
            assert_eq!(unsafe { capy_apple_pointer(app.0, 9, 0, 0, down.as_ptr(), down.len(), 0, revision, 0) }, 0);
            app.draw_frame();
            assert!(app.borrow().host.session.engine().has_active_stroke());
            let document = app.document().clone();
            assert_project_document(&document, &committed);
            assert_eq!(document.revision, committed.revision);
            // Starting a contact reserves its unique ID even if later canceled.
            match fault {
                "restart" => assert_eq!(unsafe { capy_apple_suspend_renderer(app.0) }, 0),
                "device_loss" => assert_eq!(unsafe { capy_apple_test_gpu_fault(app.0, 0) }, 0),
                "validation" => assert_eq!(unsafe { capy_apple_test_gpu_fault(app.0, 1) }, 0),
                _ => unreachable!(),
            }
            // wgpu reports destruction after the old queue finishes. Observe
            // that real callback through the owner; do not assume it is inline.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !app.borrow().host.session.rendering_suspended() {
                assert_eq!(unsafe { capy_apple_frame(app.0, 3_010_000_000, 3_010_000_000, std::ptr::null_mut()) }, 0);
                assert!(std::time::Instant::now() < deadline, "{platform} {fault}: loss callback did not arrive");
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let stopped = app.full_snapshot();
            assert_eq!(stopped["gpu_ready"], false, "{platform} {fault}");
            assert!(!stopped["error"].is_null());
            app.with(|native| {
            let session = &native.host.session;
            assert!(session.rendering_suspended());
            assert!(!session.engine().has_active_stroke());
            assert_eq!(session.engine().document(), &document);
            assert_eq!(session.engine().checkpoint(), checkpoint);
            assert!(!session.command(layer_ui::CommandId::Redo).enabled);
            assert!(session.command(layer_ui::CommandId::SaveDocumentAs).enabled);

            });
            let task = ProjectJob::new(&app, false);
            let request = app.state()["requests"][0]["id"].clone();
            let mut file = fixtures::tempfile();
            let fd = file.as_raw_fd();
            let pointer = task.0;
            assert_eq!(std::thread::spawn(move || unsafe { capy_project_write(pointer, fd) }).join().unwrap(), 0, "{:?}", task.error());
            file.sync_all().unwrap();
            file.rewind().unwrap();
            let saved = read_document(&mut file);
            assert_saved_document(&saved, &document);
            assert!(saved.artwork.occurrences.iter().any(|(_, _, occurrence)| occurrence.name.as_ref() == "Retained source"));

            install(&app);
            app.draw_until_idle();
            assert!(!app.borrow().host.session.rendering_suspended());
            assert_eq!(app.pixels(), imported, "{platform} {fault}: restored raster/source pixels");
            assert_eq!(app.state()["workspace"], state["workspace"]);
            assert_eq!(app.state()["brush"], state["brush"]);
            assert_eq!(app.state()["camera"], state["camera"]);
            assert_eq!(app.state()["requests"][0]["id"], request);
            // Retired callbacks and the old unfinished contact cannot affect
            // the new device or insert a history entry.
            device.destroy();
            let _ = device.poll(wgpu::PollType::Poll);
            app.full_snapshot();
            assert!(!app.borrow().host.session.rendering_suspended());
            let up = [480.,370.,1.,0.,0.,0.,0.,3_010_000_000.,3.];
            assert_eq!(unsafe { capy_apple_pointer(app.0, 9, 0, 0, up.as_ptr(), up.len(), 0, revision, 0) }, 0);
            app.draw_until_idle();
            assert_eq!(app.pixels(), imported);
            assert_eq!(unsafe { capy_apple_document_complete(app.0, request.as_u64().unwrap() as u32, 0) }, 0);
            app.invoke("redo"); app.draw_until_idle();
            assert_eq!(app.pixels(), painted);
            app.invoke("undo"); app.draw_until_idle();
            assert_eq!(app.pixels(), imported);
            app.stroke(); app.draw_until_idle();
            assert_ne!(app.pixels(), imported);
        }
    }
}

#[test]
fn unsupported_metal_adapter_keeps_localized_explanation_and_native_cause_on_both_apple_hosts() {
    layer_core::temp_files::set_directory(std::env::temp_dir()).unwrap();
    let gpu = native_renderer();
    let adapter = gpu.adapter();
    let limits = layer_render_wgpu::rendering_limits(adapter.limits());
    let beyond = wgpu::Limits { max_bind_groups: adapter.limits().max_bind_groups + 1, ..limits.clone() };
    let cause = layer_host::unsupported_adapter(adapter, &beyond).unwrap();
    assert!(cause.contains("max_bind_groups"), "{cause}");
    for platform in [0, 1] {
        for language in [layer_ui::UiLanguage::English, layer_ui::UiLanguage::Japanese] {
            let localization = layer_ui::Localizer::shared(language);
            let app = App(apple_launch_localized(platform, "", localization.clone()).unwrap());
            let mut lease_native=app.borrow();let native=&mut *lease_native;
            assert!(metal::check_adapter(&native.host, adapter, &limits).is_ok());
            let expected = layer_ui::gpu_unsupported(&localization, &format!("Metal adapter requirements: {cause}"));
            assert!(native.perform(|a| a.gpu_operation(|a| metal::check_adapter(&a.host, adapter, &beyond))).is_none());
            drop(lease_native);
            let error = handles::take_text(unsafe {capy_apple_error(app.0)});
            assert_eq!(error.to_str().unwrap(), expected);
            let snapshot = app.full_snapshot();
            assert_eq!(snapshot["gpu_ready"], false);
            assert_eq!(snapshot["error"], expected);
        }
    }
}
