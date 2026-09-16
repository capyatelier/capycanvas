use super::*;
use std::io::Seek;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

fn install(app: &App) {
    let gpu = native_renderer();
    let app = unsafe { &mut *app.0 };
    app.metal.install_renderer(&mut app.host, gpu).unwrap();
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
            app.draw_until_idle();
            let imported = app.pixels();
            app.stroke();
            app.draw_until_idle();
            let painted = app.pixels();
            assert_ne!(painted, imported);
            for layer in &unsafe { &*app.0 }.host.session.engine().document().layers {
                raster_samples(&layer.raster);
            }
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), imported);
            app.action(json!({"type":"set_brush_size","value":47}));
            let state = app.state();
            let session = &unsafe { &*app.0 }.host.session;
            let committed = session.engine().document().clone();
            let checkpoint = session.engine().checkpoint();
            let device = session.engine().backend().0.as_ref().unwrap().device().clone();
            let revision = unsafe { capy_apple_camera_revision(app.0) };
            let down = [450.,350.,1.,0.,0.,0.,0.,3_000_000_000.,1.];
            assert_eq!(unsafe { capy_apple_pointer(app.0, 9, 0, 0, down.as_ptr(), down.len(), 0, revision) }, 0);
            app.draw_frame();
            assert!(unsafe { &*app.0 }.host.session.engine().has_active_stroke());
            let document = unsafe { &*app.0 }.host.session.engine().document().clone();
            assert_eq!(document.layers, committed.layers);
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
            while !unsafe { &*app.0 }.host.session.rendering_suspended() {
                assert_eq!(unsafe { capy_apple_frame(app.0, 3_010_000_000, 3_010_000_000, std::ptr::null_mut()) }, 0);
                assert!(std::time::Instant::now() < deadline, "{platform} {fault}: loss callback did not arrive");
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let stopped = app.request(3, Value::Null).unwrap();
            assert_eq!(stopped["gpu_ready"], false, "{platform} {fault}");
            assert!(!stopped["error"].is_null());
            let session = &unsafe { &*app.0 }.host.session;
            assert!(session.rendering_suspended());
            assert!(!session.engine().has_active_stroke());
            assert_eq!(session.engine().document(), &document);
            assert_eq!(session.engine().checkpoint(), checkpoint);
            assert!(!session.command(layer_ui::CommandId::Redo).enabled);
            assert!(session.command(layer_ui::CommandId::SaveDocumentAs).enabled);

            let task = ProjectJob::new(&app, false);
            let request = app.state()["requests"][0]["id"].clone();
            let path = std::env::temp_dir().join(format!("capy-gpu-recovery-{}-{platform}-{fault}.capy", std::process::id()));
            let mut file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path).unwrap();
            let fd = file.as_raw_fd();
            let pointer = task.0 as usize;
            assert_eq!(std::thread::spawn(move || unsafe { capy_project_write(pointer as *const CapyProjectTask, fd) }).join().unwrap(), 0, "{:?}", task.error());
            file.sync_all().unwrap();
            file.rewind().unwrap();
            let saved = layer_core::Project::read(&mut file, Default::default()).unwrap();
            assert_project_document(&saved.document, &document);
            assert!(saved.document.layers.iter().any(|layer| layer.source.is_some()));
            std::fs::remove_file(path).unwrap();

            install(&app);
            app.draw_until_idle();
            assert!(!unsafe { &*app.0 }.host.session.rendering_suspended());
            assert_eq!(app.pixels(), imported, "{platform} {fault}: restored raster/source pixels");
            assert_eq!(app.state()["workspace"], state["workspace"]);
            assert_eq!(app.state()["brush"], state["brush"]);
            assert_eq!(app.state()["camera"], state["camera"]);
            assert_eq!(app.state()["requests"][0]["id"], request);
            // Retired callbacks and the old unfinished contact cannot affect
            // the new device or insert a history entry.
            device.destroy();
            let _ = device.poll(wgpu::PollType::Poll);
            app.request(3, Value::Null);
            assert!(!unsafe { &*app.0 }.host.session.rendering_suspended());
            let up = [480.,370.,1.,0.,0.,0.,0.,3_010_000_000.,3.];
            assert_eq!(unsafe { capy_apple_pointer(app.0, 9, 0, 0, up.as_ptr(), up.len(), 0, revision) }, 0);
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
