use super::*;
use std::{path::Path, os::fd::AsRawFd};

struct SessionJob(*mut CapySessionTask);
impl Drop for SessionJob { fn drop(&mut self) { unsafe { capy_session_free(self.0) }; } }
impl SessionJob {
    fn run(&self) {
        assert_eq!(unsafe { capy_session_work(self.0, None) }, 0, "{:?}", self.error());
    }
    fn error(&self) -> Option<String> {
        let text = unsafe { capy_session_error(self.0) };
        if text.is_null() { return None; }
        let error = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
        unsafe { capy_apple_string_free(text) };
        Some(error)
    }
}
fn open(app: &App, path: &Path, adopt: bool) -> SessionJob {
    let sessions = CString::new(path.parent().unwrap().to_str().unwrap()).unwrap();
    let scene = CString::new(path.file_name().unwrap().to_str().unwrap()).unwrap();
    SessionJob(unsafe { capy_apple_session_open(app.0, sessions.as_ptr(), scene.as_ptr(), adopt, false) })
}
fn stored_files(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() { visit(root, &entry.path(), files); }
            else if entry.file_name() != ".lock" {
                files.insert(entry.path().strip_prefix(root).unwrap().to_owned(), std::fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = Default::default();
    visit(path, path, &mut files);
    files
}
fn assert_stored_files_preserved(root: &Path, files: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
    fn contains(path: &Path, files: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) -> bool {
        if files.iter().all(|(relative, bytes)| std::fs::read(path.join(relative)).is_ok_and(|found| found == *bytes)) { return true; }
        std::fs::read_dir(path).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_type().unwrap().is_dir() && contains(&entry.path(), files)
        })
    }
    assert!(!files.is_empty());
    assert!(contains(root, files), "The complete original session files must remain byte-for-byte intact");
}
fn launch(platform: u32, path: &Path) -> App {
    launch_observed(platform, path, None, false)
}
fn launch_observed(platform: u32, path: &Path, observe: Option<SessionDestinationObserver>, adopt: bool) -> App {
    launch_report(platform, path, observe, adopt).0
}
fn launch_report(platform: u32, path: &Path, observe: Option<SessionDestinationObserver>, adopt: bool) -> (App, Option<String>) {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    let task = open(&app, path, adopt);
    assert!(!task.0.is_null(), "{:?}", unsafe { &*app.0 }.error);
    assert_eq!(unsafe { capy_session_work(task.0, observe) }, 0, "{:?}", task.error());
    let index = path.join("window.json");
    let pending = layer_ui::SessionManifest::read(&index).unwrap().map(|manifest| manifest.restoring).unwrap_or_default();
    assert_eq!(unsafe { capy_apple_session_adopt(app.0, task.0) }, 0, "{:?}", unsafe { &*app.0 }.error);
    assert_eq!(layer_ui::SessionManifest::read(&index).unwrap().map(|manifest| manifest.restoring).unwrap_or_default(), pending);
    assert_eq!(unsafe { capy_session_restore_finished(task.0) }, 0);
    assert!(layer_ui::SessionManifest::read(&index).unwrap().is_none_or(|manifest| manifest.restoring.is_empty()));
    app.draw_until_idle();
    (app, task.error())
}
unsafe extern "C" fn observe_original(uri: *const std::ffi::c_char) -> *mut CapySessionDestination {
    let uri = unsafe { CStr::from_ptr(uri) }.to_str().unwrap();
    let Some(path) = uri.strip_prefix("file://") else { return std::ptr::null_mut(); };
    let Ok(file) = std::fs::File::open(path) else { return std::ptr::null_mut(); };
    unsafe { capy_session_destination_read(file.as_raw_fd()) }
}
#[test]
fn apple_saved_session_checks_original_before_clean_close() {
    for platform in [0, 1] {
        let root = std::env::temp_dir().join(format!("capy-apple-original-{}", layer_core::PortableId::random()));
        let path = root.join("session");
        let original = root.join("drawing.capy");
        let uri = CString::new(format!("file://{}", original.display())).unwrap();
        let app = launch(platform, &path);
        app.stroke();
        app.draw_until_idle();
        let pixels = app.pixels();
        let save = ProjectJob::new(&app, false);
        let file = std::fs::File::create(&original).unwrap();
        assert_eq!(unsafe { capy_project_write(save.0, file.as_raw_fd()) }, 0);
        file.sync_all().unwrap();
        assert_eq!(unsafe { capy_apple_project_saved(app.0, save.0, c"drawing.capy".as_ptr(), uri.as_ptr()) }, 0);
        checkpoint(&app, 0, true);
        drop((save, file, app));
        let restored = launch_observed(platform, &path, Some(observe_original), false);
        assert_eq!(restored.pixels(), pixels);
        assert_eq!(restored.state()["document_file"]["modified"], false);
        checkpoint(&restored, 0, true);
        drop(restored);
        let unavailable = launch(platform, &path);
        assert_eq!(unavailable.pixels(), pixels);
        assert_eq!(unavailable.state()["document_file"]["modified"], true);
        assert_eq!(unavailable.state()["document_file"]["location"]["uri"], uri.to_str().unwrap());
        drop(unavailable);
        let mut bytes = std::fs::read(&original).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&original, bytes).unwrap();
        let changed = launch_observed(platform, &path, Some(observe_original), false);
        assert_eq!(changed.pixels(), pixels);
        assert_eq!(changed.state()["document_file"]["modified"], true);
        assert_eq!(changed.state()["document_file"]["recovered"], true);
        drop(changed);
        std::fs::remove_file(&original).unwrap();
        let missing = launch_observed(platform, &path, Some(observe_original), false);
        assert_eq!(missing.pixels(), pixels);
        assert_eq!(missing.state()["document_file"]["modified"], true);
        drop(missing);
        std::fs::remove_dir_all(root).unwrap();
    }
}
fn checkpoint(app: &App, exclusion: u64, clean: bool) -> SessionJob {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while unsafe { capy_apple_prepare_recovery(app.0, 2_000_000_000) } == 1 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let task = SessionJob(unsafe { capy_apple_session_capture(app.0, exclusion, clean) });
    assert!(!task.0.is_null(), "{:?}", unsafe { &*app.0 }.error);
    task.run();
    assert!(unsafe { capy_session_committed(task.0) });
    task
}

fn presented_pixel(app: &App) -> [u8; 4] {
    let host = &unsafe { &*app.0 }.host;
    let view = host.session.state().camera.view();
    let renderer = host.session.engine().backend().0.as_ref().unwrap();
    let device = renderer.device();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("session presented pixel"),
        size: wgpu::Extent3d { width: view.width_px, height: view.height_px, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut presenter = layer_render_wgpu::ViewportPresenter::for_surface(renderer,
        wgpu::TextureFormat::Rgba8UnormSrgb, layer_render_wgpu::SdrSurfaceColor::Srgb).unwrap();
    presenter.present(renderer, &texture.create_view(&Default::default()), view,
        host.session.state().palette.surround_linear).unwrap();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("session presented pixel readback"), size: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0,
        origin: wgpu::Origin3d { x: view.width_px / 2, y: view.height_px / 2, z: 0 },
        aspect: wgpu::TextureAspect::All }, wgpu::TexelCopyBufferInfo { buffer: &buffer,
        layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT), rows_per_image: Some(1) } },
        wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
    renderer.queue().submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| { send.send(result).unwrap(); });
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(30)) }).unwrap();
    receive.recv_timeout(std::time::Duration::from_secs(30)).unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let pixel = mapped[..4].try_into().unwrap();
    drop(mapped); buffer.unmap(); pixel
}

#[test]
fn apple_fill_selection_survives_background_checkpoint_undo_redo_and_restart() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-fill-restart-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        let blank = app.pixels();
        app.invoke("add_layer");
        app.action(json!({"type":"set_layer_opacity","opacity":0.42}));
        app.action(json!({"type":"set_color","rgba":[0.1,0.3,0.9,1]}));
        app.invoke("select_all");
        app.invoke("fill_selection");
        let extent = unsafe { &*app.0 }.host.session.engine().document().composition().size;
        let center = ((extent[1] / 2 * extent[0] + extent[0] / 2) * 4) as usize;
        let sample = |pixels: &[u8]| <[u8; 4]>::try_from(&pixels[center..center + 4]).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let painted = loop {
            app.draw_frame();
            let pixels = app.pixels();
            let pixel = sample(&pixels);
            if i32::from(pixel[2]) > i32::from(pixel[0]) + 20 { break pixels; }
            assert!(std::time::Instant::now() < deadline, "platform {platform}: fill is not blue: {pixel:?}");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        checkpoint(&app, 0, false);
        assert!(app.pixels() == painted, "platform {platform}: background checkpoint changed the fill");
        for (command, expected) in [("undo", &blank), ("redo", &painted)] {
            app.invoke(command);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                app.draw_frame();
                let actual = app.pixels();
                if actual == *expected { break; }
                assert!(std::time::Instant::now() < deadline,
                    "platform {platform}: {command} expected {:?}, got {:?}", sample(expected), sample(&actual));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        app.invoke("deselect");
        checkpoint(&app, 0, false);
        let document = unsafe { &*app.0 }.host.session.engine().document().clone();
        assert_eq!(app.state()["layers"].as_array().unwrap().len(), 3);
        drop(app);
        let restored = launch(platform, &path);
        assert_eq!(restored.state()["layers"].as_array().unwrap().len(), 3);
        assert_saved_document(unsafe { &*restored.0 }.host.session.engine().document(), &document);
        let actual = restored.pixels();
        assert!(actual == painted,
            "platform {platform}: restart expected {:?}, restored {:?}", sample(&painted), sample(&actual));
        drop(restored);
        let first_presented = App::new(platform);
        let gpu = native_renderer();
        let staged = layer_host::GpuContext::of(&gpu).rasterizer(Default::default(), &Default::default(), false).unwrap();
        unsafe { &mut *first_presented.0 }.host.session.renderer_mut().0 = Some(staged.into());
        unsafe { &mut *first_presented.0 }.host.prepare_canvas_frame(2_000_000_000, 2_000_000_000, true).unwrap();
        assert!(!unsafe { &*first_presented.0 }.host.startup.complete);
        let task = open(&first_presented, &path, false);
        task.run();
        assert_eq!(unsafe { capy_apple_session_adopt(first_presented.0, task.0) }, 0);
        assert_eq!(unsafe { capy_session_restore_finished(task.0) }, 0);
        let prepared = first_presented.pixels();
        assert!(prepared == painted, "platform {platform}: adopted prepared drawing is not blue");
        let before = presented_pixel(&first_presented);
        assert!(i32::from(before[2]) > i32::from(before[0]) + 20,
            "platform {platform}: prepared viewport is not blue: {before:?}");
        unsafe { &mut *first_presented.0 }.host.prepare_canvas_frame(2_000_000_000, 2_000_000_000, false).unwrap();
        let backdrop = presented_pixel(&first_presented);
        first_presented.draw_until_idle();
        let viewport = presented_pixel(&first_presented);
        eprintln!("platform={platform} prepared={before:?} first={backdrop:?} settled={viewport:?}");
        assert_eq!(backdrop, before, "platform {platform}: the first frame must show the prepared drawing");
        assert_eq!(viewport, before, "platform {platform}: first presentation must retain the prepared viewport");
        let presented = first_presented.pixels();
        assert!(presented == painted,
            "platform {platform}: first presented frame expected {:?}, restored {:?}", sample(&painted), sample(&presented));
        drop((task, first_presented));
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn apple_unreadable_sessions_preserve_copies_and_allow_editing_checkpointing_and_retry() {
    for platform in [0, 1] {
        for peer in [false, true] {
            let path = std::env::temp_dir().join(format!("capy-apple-unreadable-{}", layer_core::PortableId::random()));
            let app = launch(platform, &path);
            app.stroke(); app.draw_until_idle();
            let painted = app.pixels();
            if peer {
                app.invoke("new_document");
                let project = ProjectJob::new(&app, true);
                assert_eq!(project.create([96, 64]), 0);
                assert_eq!(unsafe { capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr()) }, 0);
                app.draw_until_idle();
            }
            checkpoint(&app, 0, true);
            drop(app);
            let index = path.join("window.json");
            let original = layer_ui::SessionManifest::read(&index).unwrap().unwrap();
            let drawing = &original.drawings[0];
            let head_path = path.join(&drawing.key).join("head.json");
            let original_head = std::fs::read(&head_path).unwrap();
            let mut head: serde_json::Value = serde_json::from_slice(&original_head).unwrap();
            let metadata_path = path.join(&drawing.key).join("generations").join(format!("{}.json", head["current"]["id"].as_str().unwrap()));
            let original_metadata = std::fs::read(&metadata_path).unwrap();
            let mut metadata: serde_json::Value = serde_json::from_slice(&original_metadata).unwrap();
            metadata["current"]["working"].as_object_mut().unwrap().remove("view_origin").unwrap();
            let incompatible = serde_json::to_vec(&metadata).unwrap();
            head["current"]["sha256"] = layer_ui::DestinationFingerprint::read(incompatible.as_slice()).unwrap().sha256.into();
            head["previous"] = serde_json::Value::Null;
            let incompatible_head = serde_json::to_vec(&head).unwrap();
            std::fs::write(&metadata_path, &incompatible).unwrap();
            std::fs::write(&head_path, &incompatible_head).unwrap();
            let (app, initial_failure) = launch_report(platform, &path, None, false);
            assert!(initial_failure.as_ref().is_some_and(|error| error.contains("view_origin")));
            let failed = layer_ui::SessionManifest::read(&index).unwrap().unwrap();
            assert_eq!(failed.blocked.len(), 1);
            assert_eq!(failed.drawings.iter().find(|row| failed.blocked.contains(&row.id)).unwrap().key, drawing.key);
            assert_eq!(unsafe { &*app.0 }.window.documents.order().len(), 1);
            assert!(!failed.blocked.contains(&unsafe { &*app.0 }.window.documents.selected()));
            app.invoke("add_layer"); app.draw_until_idle();
            let live = unsafe { &*app.0 }.host.session.engine().document().clone();
            checkpoint(&app, 0, true);
            assert_eq!(std::fs::read(&metadata_path).unwrap(), incompatible);
            assert_eq!(std::fs::read(&head_path).unwrap(), incompatible_head);
            drop(app);
            let (app, restarted_failure) = launch_report(platform, &path, None, false);
            assert_eq!(restarted_failure, initial_failure, "A known drawing failure must not become an unexplained interrupted restart");
            assert_saved_document(unsafe { &*app.0 }.host.session.engine().document(), &live);
            checkpoint(&app, 0, false);
            let sessions = CString::new(path.parent().unwrap().to_str().unwrap()).unwrap();
            let scene = CString::new(path.file_name().unwrap().to_str().unwrap()).unwrap();
            let retry = || SessionJob(unsafe { capy_apple_session_open(app.0, sessions.as_ptr(), scene.as_ptr(), false, true) });
            let failed_retry = retry(); failed_retry.run();
            assert!(failed_retry.error().unwrap().contains("view_origin"));
            assert_eq!(unsafe { capy_apple_session_adopt(app.0, failed_retry.0) }, 0);
            assert_eq!(unsafe { capy_session_restore_finished(failed_retry.0) }, 0);
            checkpoint(&app, 0, false);
            drop(failed_retry);
            std::fs::write(&metadata_path, &original_metadata).unwrap();
            std::fs::write(&head_path, &original_head).unwrap();
            let repaired = retry(); repaired.run();
            assert_eq!(repaired.error(), None);
            assert_eq!(unsafe { capy_apple_session_adopt(app.0, repaired.0) }, 0, "{:?}", unsafe { &*app.0 }.error);
            assert_eq!(unsafe { capy_session_restore_finished(repaired.0) }, 0, "{:?}", repaired.error());
            app.draw_until_idle();
            assert_saved_document(unsafe { &*app.0 }.host.session.engine().document(), &live);
            assert_eq!(unsafe { &*app.0 }.window.documents.order().len(), 2);
            assert!(layer_ui::SessionManifest::read(&index).unwrap().unwrap().blocked.is_empty());
            let recovered = *unsafe { &*app.0 }.window.documents.order().last().unwrap();
            assert_ne!(recovered, unsafe { &*app.0 }.window.documents.selected());
            checkpoint(&app, 0, true);
            drop((repaired, app));
            let restored = launch(platform, &path);
            assert_eq!(unsafe { &*restored.0 }.window.documents.order().len(), 2);
            super::document_tabs::switch(&restored, recovered, false);
            assert_eq!(restored.pixels(), painted);
            assert!(unsafe { &*restored.0 }.host.session.engine().can_undo());
            drop(restored);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}

#[test]
fn apple_closing_new_work_keeps_unrestored_session_copies() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-unrestored-close-{}", layer_core::PortableId::random()));
        std::fs::create_dir_all(path.join("preserved")).unwrap();
        let head = path.join("preserved/head.json");
        std::fs::write(&head, b"unsupported head").unwrap();
        let manifest = layer_ui::SessionManifest::default().stage(vec![layer_ui::SessionDrawing { id: 1, key: "preserved".into() }], 1).unwrap();
        let pending = manifest.begin_restore(1).unwrap();
        let manifest = pending.finish_restore(pending.restoring[0], false).unwrap();
        manifest.publish(&path.join("window.json")).unwrap();
        let app = launch(platform, &path);
        app.stroke(); app.draw_until_idle();
        checkpoint(&app, 0, false);
        checkpoint(&app, u64::MAX, true);
        let preserved = layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap();
        assert_eq!(preserved.drawings.len(), 1);
        assert_eq!(preserved.drawings[0].key, "preserved");
        assert_eq!(preserved.blocked, vec![preserved.drawings[0].id]);
        assert_eq!(std::fs::read(&head).unwrap(), b"unsupported head");
        drop(app);
        let app = launch(platform, &path);
        checkpoint(&app, 0, true);
        assert_eq!(layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap().drawings.len(), 2);
        assert_eq!(std::fs::read(&head).unwrap(), b"unsupported head");
        drop(app);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn apple_shared_image_objects_keep_f64_poses_pixels_and_history_across_workers_and_restart() {
    use std::io::Seek;
    let packages: [(&str, &[u8]); 3] = [
        ("builtin", include_bytes!("../../../layer-web/fixtures/shared-image-f64-builtin.capy")),
        ("icc", include_bytes!("../../../layer-web/fixtures/shared-image-f64-icc.capy")),
        ("nearest", include_bytes!("../../../layer-web/fixtures/shared-image-f64-nearest.capy")),
    ];
    for platform in [0, 1] {
        for (name, bytes) in packages {
            let path = std::env::temp_dir().join(format!("capy-apple-objects-{}", layer_core::PortableId::random()));
            let app = launch(platform, &path);
            let open = ProjectJob::new(&app, true);
            assert_eq!(unsafe { capy_project_read_bytes(open.0, bytes.as_ptr(), bytes.len(), c"Objects.capy".as_ptr()) }, 0, "{:?}", open.error());
            assert_eq!(unsafe { capy_apple_project_adopt(app.0, open.0, c"Objects.capy".as_ptr(), c"".as_ptr()) }, 0);
            app.draw_until_prepared(true);
            let expected = unsafe { &*app.0 }.host.session.engine().document().clone();
            let objects: Vec<_> = expected.artwork.objects.iter().map(|(_, _, object)| object).collect();
            assert_eq!(objects.len(), 3);
            assert!(objects.iter().all(|object| object.image.same_owner(&objects[0].image)));
            assert!(objects.iter().any(|object| object.affine.0[4].to_bits() == 16777217.125f64.to_bits()));
            assert!(expected.artwork.paint.iter().all(|(_, _, paint)| paint.base.as_ref().unwrap().image.same_owner(&objects[0].image)));
            assert_eq!(expected.scene().order().iter().filter(|handle| expected.scene().object_layer(**handle).is_some()).count(), 3);
            let owner = expected.scene().order().iter().copied().find(|handle|
                expected.scene().occurrence(*handle).unwrap().name.as_ref() == "Images").unwrap();
            let visible = app.pixels();
            assert!(visible.chunks_exact(4).any(|pixel| pixel[3] > 0), "{platform} {name}: object pixels");
            app.layer_action(json!({"op":"visibility","id":layer_ui::occurrence_token(owner),"value":false}));
            app.draw_until_idle();
            let hidden = app.pixels();
            assert!(hidden.chunks_exact(4).all(|pixel| pixel[3] == 0), "{platform} {name}: only objects were visible");
            for (command, pixels) in [("undo", &visible), ("redo", &hidden), ("undo", &visible)] {
                app.invoke(command); app.draw_until_idle(); assert_eq!(&app.pixels(), pixels);
            }
            let mut file = fixtures::tempfile();
            let save = ProjectJob::new(&app, false);
            assert_eq!(unsafe { capy_project_write(save.0, file.as_raw_fd()) }, 0, "{:?}", save.error());
            file.rewind().unwrap();
            let reopened = App::new(platform);
            unsafe { &mut *reopened.0 }.host.session.renderer_mut().0 = Some(native_renderer());
            reopened.draw_until_idle();
            let read = ProjectJob::new(&reopened, true);
            assert_eq!(unsafe { capy_project_read(read.0, file.as_raw_fd(), c"Objects.capy".as_ptr()) }, 0, "{:?}", read.error());
            assert_eq!(unsafe { capy_apple_project_adopt(reopened.0, read.0, c"Objects.capy".as_ptr(), c"".as_ptr()) }, 0);
            reopened.draw_until_prepared(true);
            assert_saved_document(unsafe { &*reopened.0 }.host.session.engine().document(), &expected);
            assert_eq!(reopened.pixels(), visible);
            assert_eq!(unsafe { capy_apple_suspend_renderer(reopened.0) }, 0);
            let native = unsafe { &mut *reopened.0 };
            let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(expected.composition().color).unwrap();
            native.metal.install_renderer(&mut native.host, gpu.into()).unwrap();
            reopened.draw_until_prepared(true);
            assert_eq!(reopened.pixels(), visible);
            checkpoint(&app, 0, true);
            drop((read, reopened, save, open, app));
            let restored = launch(platform, &path);
            assert_saved_document(unsafe { &*restored.0 }.host.session.engine().document(), &expected);
            assert_eq!(restored.pixels(), visible);
            restored.invoke("redo"); restored.draw_until_idle(); assert_eq!(restored.pixels(), hidden);
            restored.invoke("undo"); restored.draw_until_idle(); assert_eq!(restored.pixels(), visible);
            drop(restored);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}

#[test]
fn apple_session_restart_preserves_pixels_history_selection_and_close_membership() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-session-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.invoke("zoom_in");
        app.invoke("rotate_right");
        app.draw_until_idle();
        let camera = app.state()["camera"].clone();
        let blank = app.pixels();
        app.stroke();
        app.draw_until_idle();
        let painted = app.pixels();
        app.invoke("select_all");
        app.draw_until_idle();
        let working = unsafe { &*app.0 }.host.session.engine().document().working.clone();
        let file = app.state()["document_file"].clone();
        checkpoint(&app, 0, true);
        let first_key = layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap().drawings[0].key.clone();
        drop(app);
        let restored = launch(platform, &path);
        assert_eq!(restored.pixels(), painted);
        assert_eq!(unsafe { &*restored.0 }.host.session.engine().document().working, working);
        assert_eq!(restored.state()["document_file"]["modified"], file["modified"]);
        assert_eq!(restored.state()["document_file"]["recovered"], false);
        for axis in [0, 1] {
            assert!((restored.state()["camera"]["translation"][axis].as_f64().unwrap() - camera["translation"][axis].as_f64().unwrap()).abs() < 0.001);
        }
        for key in ["zoom", "rotation", "flipped"] { assert_eq!(restored.state()["camera"][key], camera[key]); }
        restored.invoke("undo");
        restored.draw_until_idle();
        restored.invoke("undo");
        restored.draw_until_idle();
        assert_eq!(restored.pixels(), blank);
        restored.invoke("redo");
        restored.draw_until_idle();
        assert_eq!(restored.pixels(), painted);
        checkpoint(&restored, 0, false);
        drop(restored);
        let recovered = launch(platform, &path);
        assert_eq!(recovered.state()["document_file"]["recovered"], true);
        let save = ProjectJob::new(&recovered, false);
        let file = fixtures::tempfile();
        assert_eq!(unsafe { capy_project_write(save.0, file.as_raw_fd()) }, 0);
        assert_eq!(unsafe { capy_apple_project_saved(recovered.0, save.0, c"Saved.capy".as_ptr(), c"file:///session-fixture.capy".as_ptr()) }, 0);
        assert_eq!(recovered.state()["document_file"]["recovered"], false);
        checkpoint(&recovered, u64::MAX, true);
        drop((save, recovered));
        let fresh = launch(platform, &path);
        assert!(!unsafe { &*fresh.0 }.host.session.engine().can_undo());
        checkpoint(&fresh, 0, true);
        assert_ne!(layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap().drawings[0].key, first_key);
        drop(fresh);
        std::fs::remove_dir_all(path).unwrap();
    }
}
#[test]
fn apple_session_stale_startup_appends_without_overwriting_new_input() {
    for (platform, interrupted_publication) in [0, 1].into_iter().flat_map(|platform| [false, true].map(|failure| (platform, failure))) {
        let path = std::env::temp_dir().join(format!("capy-apple-session-stale-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.invoke("add_layer");
        app.draw_until_idle();
        let saved = unsafe { &*app.0 }.host.session.engine().document().clone();
        checkpoint(&app, 0, false);
        drop(app);
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let task = open(&app, &path, false);
        assert!(!task.0.is_null());
        app.invoke("add_layer");
        app.stroke(); app.draw_until_idle();
        let selected = unsafe { &*app.0 }.window.documents.selected();
        let pixels = app.pixels();
        let current = unsafe { &*app.0 }.host.session.engine().document().clone();
        task.run();
        assert_eq!(unsafe { capy_apple_session_adopt(app.0, task.0) }, 0, "{:?}", unsafe { &*app.0 }.error);
        if interrupted_publication {
            let index = path.join("window.json");
            let held = path.join("held-window.json");
            std::fs::rename(&index, &held).unwrap();
            std::fs::create_dir(&index).unwrap();
            assert_eq!(unsafe { capy_session_restore_finished(task.0) }, -1);
            assert!(task.error().is_some());
            std::fs::remove_dir(&index).unwrap();
            std::fs::rename(&held, &index).unwrap();
        } else {
            assert_eq!(unsafe { capy_session_restore_finished(task.0) }, 0, "{:?}", task.error());
        }
        assert_eq!(unsafe { &*app.0 }.window.documents.selected(), selected);
        assert_eq!(unsafe { &*app.0 }.window.documents.order().len(), 2);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &current);
        assert_eq!(app.pixels(), pixels);
        assert!(unsafe { &*app.0 }.host.session.engine().can_undo());
        checkpoint(&app, 0, true);
        let restored_id = *unsafe { &*app.0 }.window.documents.order().iter().find(|id| **id != selected).unwrap();
        super::document_tabs::switch(&app, restored_id, false);
        assert_saved_document(unsafe { &*app.0 }.host.session.engine().document(), &saved);
        super::document_tabs::switch(&app, selected, false);
        checkpoint(&app, 0, true);
        drop((task, app));
        let reopened = launch(platform, &path);
        assert_eq!(unsafe { &*reopened.0 }.window.documents.selected(), selected);
        assert_eq!(unsafe { &*reopened.0 }.window.documents.order().len(), 2);
        assert_eq!(reopened.pixels(), pixels);
        assert!(unsafe { &*reopened.0 }.host.session.engine().can_undo());
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
}
#[test]
fn apple_session_failed_peer_checkpoint_keeps_closing_drawing_indexed() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-session-close-failure-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.invoke("new_document");
        let project = ProjectJob::new(&app, true);
        assert_eq!(project.create([96, 64]), 0);
        assert_eq!(unsafe { capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr()) }, 0);
        app.draw_until_idle();
        checkpoint(&app, 0, false);
        let manifest = layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap();
        assert_eq!(manifest.drawings.len(), 2);
        let closing = manifest.drawings[0].id;
        let peer = manifest.drawings.iter().find(|drawing| drawing.id == manifest.active).unwrap();
        app.invoke("add_layer");
        app.draw_until_idle();
        let generation = path.join(&peer.key).join("generations");
        let held = path.join(&peer.key).join("held-generations");
        std::fs::rename(&generation, &held).unwrap();
        std::fs::write(&generation, b"blocked").unwrap();
        let task = SessionJob(unsafe { capy_apple_session_capture(app.0, closing, false) });
        assert!(!task.0.is_null());
        assert_eq!(unsafe { capy_session_work(task.0, None) }, -1);
        assert!(!unsafe { capy_session_committed(task.0) });
        let retained = layer_ui::SessionManifest::read(&path.join("window.json")).unwrap().unwrap();
        assert_eq!(retained.drawings, manifest.drawings);
        assert!(path.join(&manifest.drawings[0].key).join("head.json").is_file());
        std::fs::remove_file(&generation).unwrap();
        std::fs::rename(&held, &generation).unwrap();
        checkpoint(&app, 0, true);
        drop((task, project, app));
        std::fs::remove_dir_all(path).unwrap();
    }
}
#[test]
fn apple_new_window_adopts_the_newest_unlocked_session_with_drawings() {
    let sessions = std::env::temp_dir().join(format!("capy-apple-sessions-{}", layer_core::PortableId::random()));
    let pause = || std::thread::sleep(std::time::Duration::from_millis(20));
    let older = launch(1, &sessions.join("older"));
    checkpoint(&older, 0, false);
    drop(older);
    pause();
    let newer = launch(1, &sessions.join("newer"));
    newer.stroke();
    newer.draw_until_idle();
    let painted = newer.pixels();
    checkpoint(&newer, 0, false);
    drop(newer);
    pause();
    let closed = launch(1, &sessions.join("closed"));
    checkpoint(&closed, u64::MAX, true);
    drop(closed);
    pause();
    let busy = launch(1, &sessions.join("busy"));
    checkpoint(&busy, 0, false);
    let unadopted = launch(1, &sessions.join("unadopted"));
    assert!(sessions.join("newer").is_dir());
    drop(unadopted);
    let adopted = launch_observed(1, &sessions.join("adopted"), None, true);
    assert_eq!(adopted.pixels(), painted);
    assert!(!sessions.join("newer").exists());
    assert!(sessions.join("busy").is_dir() && sessions.join("closed").is_dir());
    checkpoint(&adopted, 0, true);
    drop(adopted);
    let restored = launch_observed(1, &sessions.join("adopted"), None, true);
    assert_eq!(restored.pixels(), painted);
    assert!(sessions.join("older").is_dir());
    let second = launch_observed(1, &sessions.join("second"), None, true);
    assert!(!sessions.join("older").exists());
    checkpoint(&second, u64::MAX, true);
    drop(second);
    let fresh = launch_observed(1, &sessions.join("fresh"), None, true);
    assert_eq!(layer_ui::SessionManifest::read(&sessions.join("fresh/window.json")).unwrap(), None);
    assert!(sessions.join("busy/window.json").is_file() && sessions.join("adopted/window.json").is_file());
    drop((fresh, restored, busy));
    std::fs::remove_dir_all(sessions).unwrap();
}

#[test]
fn apple_scene_adoption_preserves_own_unindexed_drawing_heads() {
    for platform in [0, 1] {
        for missing in [false, true] {
            let sessions = std::env::temp_dir().join(format!("capy-apple-unindexed-{}", layer_core::PortableId::random()));
            let own = sessions.join("own");
            let app = launch(platform, &own);
            app.stroke(); app.draw_until_idle(); checkpoint(&app, 0, false);
            drop(app);
            if missing { std::fs::remove_file(own.join("window.json")).unwrap(); }
            else { std::fs::write(own.join("window.json"), serde_json::to_vec(&layer_ui::SessionManifest::default()).unwrap()).unwrap(); }
            let preserved = stored_files(&own);
            let other = launch(platform, &sessions.join("other"));
            other.invoke("add_layer"); other.draw_until_idle(); checkpoint(&other, 0, true);
            drop(other);
            let app = launch_observed(platform, &own, None, true);
            assert_stored_files_preserved(&sessions, &preserved);
            app.invoke("add_layer"); app.draw_until_idle(); checkpoint(&app, 0, true);
            assert_stored_files_preserved(&sessions, &preserved);
            drop(app);
            let reopened = launch(platform, &own);
            checkpoint(&reopened, 0, true);
            assert_stored_files_preserved(&sessions, &preserved);
            drop(reopened);
            std::fs::remove_dir_all(sessions).unwrap();
        }
    }
}

#[test]
fn apple_invalid_window_membership_preserves_all_files_and_allows_new_checkpoints() {
    for platform in [0, 1] {
        for corruption in ["truncated", "active", "duplicate", "missing"] {
            let sessions = std::env::temp_dir().join(format!("capy-apple-invalid-membership-{}", layer_core::PortableId::random()));
            let path = sessions.join("own");
            let app = launch(platform, &path);
            app.stroke(); app.draw_until_idle(); checkpoint(&app, 0, true);
            drop(app);
            let index = path.join("window.json");
            let mut manifest: Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            match corruption {
                "active" => manifest["active"] = json!(999),
                "duplicate" => {
                    let duplicate = manifest["drawings"][0].clone();
                    manifest["drawings"].as_array_mut().unwrap().push(duplicate);
                }
                "missing" => { manifest.as_object_mut().unwrap().remove("drawings"); }
                _ => {},
            }
            let bytes = if corruption == "truncated" { b"{\"generation\":".to_vec() } else { serde_json::to_vec(&manifest).unwrap() };
            std::fs::write(&index, bytes).unwrap();
            let preserved = stored_files(&path);
            let fresh = launch_observed(platform, &path, None, true);
            assert_stored_files_preserved(&sessions, &preserved);
            assert!(!unsafe { &*fresh.0 }.host.session.engine().can_undo());
            fresh.stroke(); fresh.draw_until_idle();
            let pixels = fresh.pixels();
            checkpoint(&fresh, 0, true);
            assert_stored_files_preserved(&sessions, &preserved);
            drop(fresh);
            let reopened = launch(platform, &path);
            assert_eq!(reopened.pixels(), pixels);
            assert!(unsafe { &*reopened.0 }.host.session.engine().can_undo());
            checkpoint(&reopened, 0, true);
            assert_stored_files_preserved(&sessions, &preserved);
            drop(reopened);
            std::fs::remove_dir_all(sessions).unwrap();
        }
    }
}

#[test]
fn apple_recovery_reports_each_known_failure_separately_from_interrupted_attempts() {
    for platform in [0, 1] {
        let sessions = std::env::temp_dir().join(format!("capy-apple-restore-reasons-{}", layer_core::PortableId::random()));
        let path = sessions.join("known");
        for (key, bytes) in [("invalid-json", b"not json".as_slice()), ("missing-head-field", b"{}".as_slice())] {
            std::fs::create_dir_all(path.join(key)).unwrap();
            std::fs::write(path.join(key).join("head.json"), bytes).unwrap();
        }
        let drawings = vec![layer_ui::SessionDrawing { id: 1, key: "invalid-json".into() },
            layer_ui::SessionDrawing { id: 2, key: "missing-head-field".into() }];
        layer_ui::SessionManifest::default().stage(drawings, 1).unwrap().publish(&path.join("window.json")).unwrap();
        let (app, failure) = launch_report(platform, &path, None, false);
        let failure = failure.unwrap();
        assert!(failure.contains("expected ident"), "Missing first drawing's exact decode error: {failure}");
        assert!(failure.contains("missing field"), "Missing second drawing's exact decode error: {failure}");
        assert!(failure.contains("invalid-json") && failure.contains("missing-head-field"), "Failure details must identify both preserved drawings: {failure}");
        checkpoint(&app, 0, true);
        drop(app);
        let (app, reopened_failure) = launch_report(platform, &path, None, false);
        assert_eq!(reopened_failure.as_deref(), Some(failure.as_str()));
        drop(app);
        let interrupted = sessions.join("interrupted");
        std::fs::create_dir_all(&interrupted).unwrap();
        let manifest = layer_ui::SessionManifest::default().stage(vec![layer_ui::SessionDrawing { id: 1, key: "interrupted-drawing".into() }], 1).unwrap().begin_restore(1).unwrap();
        manifest.publish(&interrupted.join("window.json")).unwrap();
        let (app, interrupted_failure) = launch_report(platform, &interrupted, None, false);
        assert!(interrupted_failure.unwrap().to_ascii_lowercase().contains("interrupted"));
        drop(app);
        std::fs::remove_dir_all(sessions).unwrap();
    }
}

#[test]
fn apple_restored_window_reserves_failed_ids_before_creating_another_drawing() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-reserved-restore-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.stroke(); app.draw_until_idle(); checkpoint(&app, 0, true);
        drop(app);
        let index = path.join("window.json");
        let original = layer_ui::SessionManifest::read(&index).unwrap().unwrap();
        let failed = original.drawings[0].id + 1;
        let damaged = path.join("failed-drawing");
        std::fs::create_dir_all(&damaged).unwrap();
        let head = damaged.join("head.json");
        std::fs::write(&head, b"{}").unwrap();
        original.stage(vec![layer_ui::SessionDrawing { id: failed, key: "failed-drawing".into() }], original.active).unwrap().publish(&index).unwrap();
        let app = launch(platform, &path);
        app.invoke("new_document");
        let project = ProjectJob::new(&app, true);
        assert_eq!(project.create([96, 64]), 0);
        assert_eq!(unsafe { capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr()) }, 0);
        app.draw_until_idle();
        assert_ne!(unsafe { &*app.0 }.window.documents.selected(), failed);
        checkpoint(&app, 0, true);
        assert_eq!(std::fs::read(&head).unwrap(), b"{}");
        let saved = layer_ui::SessionManifest::read(&index).unwrap().unwrap();
        assert!(saved.blocked.contains(&failed));
        assert_eq!(saved.drawings.len(), 3);
        drop((project, app));
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn apple_unrelated_cleanup_failure_does_not_block_restoration_or_new_checkpoints() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-cleanup-warning-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.stroke(); app.draw_until_idle();
        let pixels = app.pixels();
        checkpoint(&app, 0, true);
        drop(app);
        let orphan = path.join("unreferenced-copy");
        std::fs::create_dir_all(orphan.join("resources")).unwrap();
        std::fs::create_dir_all(orphan.join("generations")).unwrap();
        std::fs::write(orphan.join(".lock"), b"").unwrap();
        std::fs::write(orphan.join(".retiring"), b"invalid").unwrap();
        let preserved = stored_files(&orphan);
        let (app, warning) = launch_report(platform, &path, None, false);
        assert_eq!(warning, None);
        assert_eq!(app.pixels(), pixels);
        for _ in 0..2 {
            app.invoke("add_layer"); app.draw_until_idle();
            let task = checkpoint(&app, 0, true);
            let warning = task.error();
            assert!(warning.as_ref().is_some_and(|warning| warning.contains("Invalid drawing retirement intent")), "{warning:?}");
            assert_stored_files_preserved(&path, &preserved);
        }
        let document = unsafe { &*app.0 }.host.session.engine().document().clone();
        drop(app);
        let app = launch(platform, &path);
        assert_saved_document(unsafe { &*app.0 }.host.session.engine().document(), &document);
        assert_stored_files_preserved(&path, &preserved);
        drop(app);
        std::fs::remove_dir_all(path).unwrap();
    }
}
