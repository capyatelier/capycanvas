use super::*;
use std::io::{Read, Seek};
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};

#[test]
fn project_recovery_preserves_captured_pixels_and_requires_a_durable_manual_save() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!(
            "capy-recovery-{}-{platform}.capy",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        let blank = app.pixels();
        app.stroke();
        // No surface frame follows pen-up. Lifecycle draining must commit the
        // queued ink before capture even when the drawable has disappeared.
        assert!(unsafe { capy_apple_project_task(app.0, 2) }.is_null());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let status = unsafe { capy_apple_recovery_flush_input(app.0, 1_030_000_000) };
            assert!(status >= 0);
            if status == 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let pixels = app.pixels();
        assert_ne!(pixels, blank);
        let original = unsafe { &*app.0 }.host.session.engine().document().clone();
        let task = ProjectJob(unsafe { capy_apple_project_task(app.0, 2) });
        assert!(!task.0.is_null());
        assert_eq!(app.state()["document_file"]["busy"], false);
        assert!(app.state()["requests"].as_array().unwrap().is_empty());
        app.action(json!({"type":"set_layer_opacity","opacity":0.25}));
        app.draw_frame();
        let pointer = task.0 as usize;
        let fd = file.as_raw_fd();
        assert_eq!(
            std::thread::spawn(move || unsafe {
                capy_project_write(pointer as *const CapyProjectTask, fd)
            })
            .join()
            .unwrap(),
            0,
            "{:?}",
            task.error()
        );
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let saved = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(saved.document, original);
        assert_eq!(
            unsafe {
                capy_apple_project_saved(
                    app.0,
                    task.0,
                    c"test.capy".as_ptr(),
                    c"file:///test.capy".as_ptr(),
                )
            },
            -1
        );
        assert_eq!(app.state()["document_file"]["modified"], true);

        let open = ProjectJob::new(&app, true);
        file.rewind().unwrap();
        let pointer = open.0 as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                capy_project_read(pointer as *const CapyProjectTask, fd)
            })
            .join()
            .unwrap(),
            0,
            "{:?}",
            open.error()
        );
        assert_eq!(unsafe { capy_apple_project_recover(app.0, open.0) }, 0);
        app.draw_frame();
        assert_eq!(app.pixels(), pixels);
        assert!(app.state()["document_file"]["location"].is_null());
        assert_eq!(app.state()["document_file"]["modified"], true);
        app.invoke("add_layer");
        app.invoke("undo");
        assert_eq!(
            app.state()["document_file"]["modified"],
            true,
            "Undo to recovered initial content still requires saving"
        );
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        assert_eq!(
            app.state()["requests"][0]["kind"]["request"]["type"],
            "confirm_close"
        );
        let id = app.state()["requests"][0]["id"].as_u64().unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_close(app.0, id, 3) }, 0);
        let manual = ProjectJob::new(&app, false);
        file.rewind().unwrap();
        file.set_len(0).unwrap();
        assert_eq!(unsafe { capy_project_write(manual.0, fd) }, 0);
        assert_eq!(
            unsafe {
                capy_apple_project_saved(
                    app.0,
                    manual.0,
                    c"saved.capy".as_ptr(),
                    c"file:///saved.capy".as_ptr(),
                )
            },
            0
        );
        assert_eq!(app.state()["document_file"]["modified"], false);
        app.invoke("add_layer");
        app.invoke("undo");
        assert_eq!(
            app.state()["document_file"]["modified"],
            false,
            "An acknowledged manual save establishes the normal checkpoint again"
        );
        std::fs::remove_file(path).unwrap();
    }
}
