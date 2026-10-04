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
fn launch(platform: u32, path: &Path) -> App {
    launch_observed(platform, path, None)
}
fn launch_observed(platform: u32, path: &Path, observe: Option<SessionDestinationObserver>) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let task = SessionJob(unsafe { capy_apple_session_open(app.0, path.as_ptr(), false) });
    assert!(!task.0.is_null(), "{:?}", unsafe { &*app.0 }.error);
    assert_eq!(unsafe { capy_session_work(task.0, observe) }, 0, "{:?}", task.error());
    let index = std::path::PathBuf::from(path.to_str().unwrap()).join("window.json");
    let pending = layer_ui::SessionManifest::read(&index).unwrap().map(|manifest| manifest.restoring).unwrap_or_default();
    assert_eq!(unsafe { capy_apple_session_adopt(app.0, task.0) }, 0, "{:?}", unsafe { &*app.0 }.error);
    assert_eq!(layer_ui::SessionManifest::read(&index).unwrap().map(|manifest| manifest.restoring).unwrap_or_default(), pending);
    assert_eq!(unsafe { capy_session_restore_finished(task.0) }, 0);
    assert!(layer_ui::SessionManifest::read(&index).unwrap().is_none_or(|manifest| manifest.restoring.is_empty()));
    app.draw_until_idle();
    app
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
        let restored = launch_observed(platform, &path, Some(observe_original));
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
        let changed = launch_observed(platform, &path, Some(observe_original));
        assert_eq!(changed.pixels(), pixels);
        assert_eq!(changed.state()["document_file"]["modified"], true);
        assert_eq!(changed.state()["document_file"]["recovered"], true);
        drop(changed);
        std::fs::remove_file(&original).unwrap();
        let missing = launch_observed(platform, &path, Some(observe_original));
        assert_eq!(missing.pixels(), pixels);
        assert_eq!(missing.state()["document_file"]["modified"], true);
        drop(missing);
        std::fs::remove_dir_all(root).unwrap();
    }
}
fn checkpoint(app: &App, exclusion: u64, clean: bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while unsafe { capy_apple_prepare_recovery(app.0, 2_000_000_000) } == 1 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let task = SessionJob(unsafe { capy_apple_session_capture(app.0, exclusion, clean) });
    assert!(!task.0.is_null(), "{:?}", unsafe { &*app.0 }.error);
    task.run();
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
fn apple_session_stale_startup_never_overwrites_new_input() {
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!("capy-apple-session-stale-{}", layer_core::PortableId::random()));
        let app = launch(platform, &path);
        app.invoke("add_layer");
        app.draw_until_idle();
        checkpoint(&app, 0, false);
        drop(app);
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let title = CString::new(path.to_str().unwrap()).unwrap();
        let task = SessionJob(unsafe { capy_apple_session_open(app.0, title.as_ptr(), false) });
        assert!(!task.0.is_null());
        app.invoke("add_layer");
        let current = unsafe { &*app.0 }.host.session.engine().document().clone();
        task.run();
        assert_eq!(unsafe { capy_apple_session_adopt(app.0, task.0) }, -1);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &current);
        assert!(path.join("window.json").is_file());
        drop((task, app));
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
