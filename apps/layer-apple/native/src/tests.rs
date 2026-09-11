//! Test editor effects after UI action dispatch, without automating OS menus.
use super::*;
use layer_render::CanvasRenderer;
use serde_json::{Value, json};

struct App(*mut CapyApple);
#[path = "input_tests.rs"]
mod input;
#[path = "navigator_tests.rs"]
mod navigator;
#[path = "recovery_tests.rs"]
mod recovery;
#[path = "workspace_tests.rs"]
mod workspace;

#[test]
fn filter_property_models_edit_reset_and_undo_all_six_kinds_on_both_platforms() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let mut kinds = std::collections::BTreeSet::new();
        for (effect, key, value) in [
            (None, "blend", json!({"kind":"choice","value":2})),
            (
                Some("gaussian_blur"),
                "sigma",
                json!({"kind":"number","value":7}),
            ),
            (
                Some("black_white"),
                "tint",
                json!({"kind":"toggle","value":true}),
            ),
            (
                None,
                "tint_color",
                json!({"kind":"color","value":[0.7,0.2,0.3,0.6]}),
            ),
            (
                Some("curves"),
                "curve_0",
                json!({"kind":"curve","value":[[0,0],[0.5,0.75],[1,1]]}),
            ),
            (
                Some("gradient_map"),
                "gradient",
                json!({"kind":"gradient","value":[{"position":0,"color":[0,0,0,1]},
                {"position":0.3,"color":[1,0,0,1]},{"position":1,"color":[1,1,1,1]}]}),
            ),
        ] {
            if let Some(effect) = effect {
                app.action(json!({"type":"effect","action":{"op":"insert","effect":effect}}));
            }
            let state = app.state();
            let properties = &state["layer_properties"];
            assert_eq!(properties["enabled"], true);
            let layer = properties["layer"].as_u64().unwrap();
            let control = properties["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["key"] == key)
                .unwrap();
            let before = control["value"].clone();
            let default = control["default"].clone();
            kinds.insert(control["kind"]["kind"].as_str().unwrap().to_string());
            let current = || {
                app.state()["layer_properties"]["controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["key"] == key)
                    .unwrap()["value"]
                    .clone()
            };
            app.action(json!({"type":"effect","action":{"op":"set","layer":layer,"key":key,"value":value}}));
            let edited = current();
            assert_ne!(edited, before);
            app.invoke("undo");
            assert_eq!(current(), before);
            app.invoke("redo");
            assert_eq!(current(), edited);
            app.action(json!({"type":"effect","action":{"op":"reset","layer":layer,"key":key}}));
            assert_eq!(current(), default);
            if key == "curve_0" {
                app.action(json!({"type":"effect","action":{"op":"curve_point","layer":layer,"key":key,"index":null,"point":[0.5,0.8],"remove":false}}));
                assert_eq!(current()["value"].as_array().unwrap().len(), 3);
                let state = app.state();
                let plot = state["layer_properties"]["controls"][0]["plot"]
                    .as_array()
                    .unwrap();
                assert_eq!(plot.len(), 129);
                assert!((plot[64][1].as_f64().unwrap() - 0.8).abs() < 0.0001);
                app.action(json!({"type":"effect","action":{"op":"curve_point","layer":layer,"key":key,"index":1,"point":[0,0],"remove":true}}));
                assert_eq!(current(), default);
            }
            if key == "gradient" {
                app.action(json!({"type":"effect","action":{"op":"gradient_stop","layer":layer,"key":key,"index":null,"position":0.5,"color":null,"remove":false}}));
                assert_eq!(current()["value"][1]["color"], json!([0.5, 0.5, 0.5, 1.0]));
                app.action(json!({"type":"effect","action":{"op":"gradient_stop","layer":layer,"key":key,"index":1,"position":0.25,"color":null,"remove":false}}));
                assert_eq!(current()["value"][1]["position"], 0.25);
            }
        }
        assert_eq!(
            kinds,
            ["choice", "color", "curve", "gradient", "number", "toggle"]
                .map(String::from)
                .into()
        );
    }
}

#[test]
fn property_number_edits_change_metal_pixels_and_undo_exactly_on_both_platforms() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"gaussian_blur"}}));
        app.draw_frame();
        let before = app.pixels();
        let layer = app.state()["layer_properties"]["layer"].clone();
        app.action(
            json!({"type":"effect","action":{"op":"set","layer":layer,"key":"sigma",
            "value":{"kind":"number","value":12}}}),
        );
        app.draw_frame();
        let edited = app.pixels();
        assert_ne!(edited, before);
        app.invoke("undo");
        app.draw_frame();
        assert_eq!(app.pixels(), before);
        app.invoke("redo");
        app.draw_frame();
        assert_eq!(app.pixels(), edited);
    }
}

#[test]
fn filter_preview_abi_keeps_owned_pixels_after_editor_teardown_without_document_edits() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let before = app.pixels();
        let revision = unsafe { &*app.0 }.host.session.filter_preview_revision();
        let status = app
            .request(
                2,
                json!({"type":"filter_previews","request":73,"revision":revision,
            "filters":["curves","gradient_map"],"size":[96,40]}),
            )
            .unwrap();
        assert_eq!(status["accepted"], true);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let atlas = loop {
            let atlas = unsafe { capy_apple_take_filter_previews(app.0) };
            assert!(unsafe { capy_apple_error(app.0) }.is_null());
            if !atlas.is_null() {
                break atlas;
            }
            assert!(std::time::Instant::now() < end, "GPU preview timed out");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(
            unsafe { &*app.0 }.host.session.filter_preview_revision(),
            revision
        );
        assert_eq!(app.pixels(), before);
        drop(app);
        let pointer = atlas as usize;
        std::thread::spawn(move || unsafe {
            let atlas = pointer as *mut CapyFilterPreviews;
            let mut info = std::mem::MaybeUninit::<CapyFilterPreviewInfo>::uninit();
            capy_filter_previews_read(atlas, info.as_mut_ptr());
            let info = info.assume_init();
            assert_eq!(
                (
                    info.request,
                    info.width,
                    info.height,
                    info.stride,
                    info.count
                ),
                (73, 96, 80, 384, 30720)
            );
            let filters: Value =
                serde_json::from_slice(CStr::from_ptr(info.filters).to_bytes()).unwrap();
            assert_eq!(filters, json!(["curves", "gradient_map"]));
            let pixels = std::slice::from_raw_parts(info.pixels, info.count);
            assert!(pixels.chunks_exact(4).any(|p| p[3] != 0));
            capy_filter_previews_free(atlas);
        })
        .join()
        .unwrap();
    }
}

struct ProjectJob(*mut CapyProjectTask);
impl Drop for ProjectJob {
    fn drop(&mut self) {
        unsafe { capy_project_free(self.0) }
    }
}
impl ProjectJob {
    fn new(app: &App, opening: bool) -> Self {
        if !opening {
            let pending: Vec<_> = unsafe { &*app.0 }
                .host
                .session
                .state()
                .requests
                .iter()
                .filter(|r| matches!(r.kind, layer_ui::HostRequestKind::Document { .. }))
                .map(|r| r.id)
                .collect();
            for id in pending {
                assert_eq!(unsafe { capy_apple_document_complete(app.0, id, 0) }, 0);
            }
            app.invoke("save_document");
        }
        let task = unsafe { capy_apple_project_task(app.0, u32::from(opening)) };
        assert!(!task.is_null());
        Self(task)
    }
    fn error(&self) -> Option<String> {
        let value = unsafe { capy_project_error(self.0) };
        if value.is_null() {
            None
        } else {
            let result = unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned();
            unsafe { capy_apple_string_free(value) };
            Some(result)
        }
    }
}

#[test]
fn project_jobs_save_specific_revisions_and_adopt_only_unchanged_editors() {
    use std::io::{Read, Seek};
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    fn sendable<T: Send + Sync>() {}
    sendable::<CapyProjectTask>();
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!(
            "capy-project-{}-{}.capy",
            std::process::id(),
            platform
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
        app.stroke();
        app.draw_frame();
        let original_pixels = app.pixels();
        let original = unsafe { &*app.0 }.host.session.engine().document().clone();
        let save = ProjectJob::new(&app, false);
        app.action(json!({"type":"set_layer_opacity","opacity":0.25}));
        app.draw_frame();
        let pointer = save.0 as usize;
        let fd = file.as_raw_fd();
        assert_eq!(
            std::thread::spawn(move || unsafe {
                capy_project_write(pointer as *const CapyProjectTask, fd)
            })
            .join()
            .unwrap(),
            0,
            "{:?}",
            save.error()
        );
        assert_eq!(unsafe { capy_project_begin_commit(save.0) }, 0);
        let title = CString::new("Saved.capy").unwrap();
        assert_eq!(
            unsafe {
                capy_apple_project_saved(
                    app.0,
                    save.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            0
        );
        assert_eq!(
            app.state()["document_file"]["modified"],
            true,
            "A late save must not mark newer edits clean"
        );
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let saved = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(saved.document, original);
        let stale = ProjectJob::new(&app, true);
        file.rewind().unwrap();
        let pointer = stale.0 as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                capy_project_read(pointer as *const CapyProjectTask, fd)
            })
            .join()
            .unwrap(),
            0,
            "{:?}",
            stale.error()
        );
        app.action(json!({"type":"set_layer_opacity","opacity":0.5}));
        app.draw_frame();
        let changed = app.pixels();
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(
                    app.0,
                    stale.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            -1
        );
        app.draw_frame();
        assert!(
            app.pixels() == changed,
            "Late open must preserve intervening edits"
        );
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
        let workspace = app.state()["workspace"].clone();
        let old_view = unsafe { capy_apple_camera_revision(app.0) };
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(
                    app.0,
                    open.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            0
        );
        assert!(unsafe { capy_apple_camera_revision(app.0) } > old_view);
        app.stroke_at(old_view);
        app.draw_frame();
        assert!(app.pixels() == original_pixels);
        assert_eq!(app.state()["workspace"], workspace);
        assert_eq!(app.state()["document_file"]["modified"], false);
        assert_eq!(
            app.state()["document_file"]["location"]["name"],
            "Saved.capy"
        );
        assert_eq!(
            unsafe {
                capy_apple_project_saved(
                    app.0,
                    save.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            -1,
            "Old save cannot name a replacement document"
        );
        let new = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_read(new.0, -1) }, 0);
        let blank = CString::new("Untitled").unwrap();
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, new.0, blank.as_ptr(), c"".as_ptr()) },
            0
        );
        app.draw_frame();
        assert!(app.pixels() != original_pixels);
        assert_eq!(app.state()["document_file"]["modified"], false);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn project_cancellation_and_invalid_input_preserve_live_artwork() {
    use std::io::{Seek, Write};
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    for platform in [0, 1] {
        let path = std::env::temp_dir().join(format!(
            "capy-project-failure-{}-{}.capy",
            std::process::id(),
            platform
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
        app.stroke();
        app.draw_frame();
        let before = app.pixels();
        let save = ProjectJob::new(&app, false);
        unsafe { capy_project_cancel(save.0) };
        assert_eq!(unsafe { capy_project_write(save.0, file.as_raw_fd()) }, -1);
        assert_eq!(file.metadata().unwrap().len(), 0);
        assert_eq!(unsafe { capy_project_begin_commit(save.0) }, -1);
        file.write_all(b"not a project").unwrap();
        file.rewind().unwrap();
        let open = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_read(open.0, file.as_raw_fd()) }, -1);
        let title = CString::new("Broken.capy").unwrap();
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(
                    app.0,
                    open.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            -1
        );
        let cancelled = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_read(cancelled.0, -1) }, 0);
        unsafe { capy_project_cancel(cancelled.0) };
        assert_eq!(
            unsafe {
                capy_apple_project_adopt(
                    app.0,
                    cancelled.0,
                    title.as_ptr(),
                    c"file:///fixture.capy".as_ptr(),
                )
            },
            -1
        );
        app.draw_frame();
        assert!(app.pixels() == before);
        let committed = ProjectJob::new(&app, false);
        assert_eq!(unsafe { capy_project_begin_commit(committed.0) }, 0);
        unsafe { capy_project_cancel(committed.0) };
        assert_eq!(
            unsafe { capy_project_begin_commit(committed.0) },
            0,
            "Cancellation cannot retract publication"
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn new_canvas_dimensions_and_worker_png_export_preserve_captured_pixels() {
    use std::{
        io::Seek,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    };
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        let invalid = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_new(invalid.0, 0, 47) }, -1);
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().width,
            2048
        );
        let new = ProjectJob::new(&app, true);
        assert_eq!(
            unsafe { capy_project_new(new.0, 63, 47) },
            0,
            "{:?}",
            new.error()
        );
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, new.0, c"Untitled".as_ptr(), c"".as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().width,
            63
        );
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().height,
            47
        );
        app.action(json!({"type":"set_color","rgba":[0.8,0.2,0.5,0.6]}));
        app.stroke();
        app.draw_frame();
        let expected = app.pixels();
        app.invoke("export_document");
        let id = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "export")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        let mut pointer = std::ptr::null_mut();
        assert_eq!(
            unsafe { capy_apple_export_task(app.0, id, 2_000_000_000, &mut pointer) },
            1
        );
        let export = ProjectJob(pointer);
        // Later GPU work must not alter the copied snapshot. The worker also
        // owns everything it needs after the editor and renderer are destroyed.
        app.action(json!({"type":"set_layer_opacity","opacity":0.25}));
        app.draw_frame();
        assert_ne!(app.pixels(), expected);
        assert!(app.state()["document_file"]["modified"].as_bool().unwrap());
        assert!(app.state()["document_file"]["location"].is_null());
        drop(app);
        let path =
            std::env::temp_dir().join(format!("capy-export-{}-{platform}.png", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        assert_eq!(
            unsafe { capy_project_write(export.0, file.as_raw_fd()) },
            0,
            "{:?}",
            export.error()
        );
        file.rewind().unwrap();
        let mut reader = png::Decoder::new(&file).read_info().unwrap();
        assert_eq!(
            reader.info().srgb,
            Some(png::SrgbRenderingIntent::Perceptual)
        );
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!([info.width, info.height], [63, 47]);
        assert_eq!(pixels, expected);
        drop(reader);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn bundled_library_refresh_waits_without_migrating_document_filters() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"unsharp_mask"}}));
        app.draw_frame();
        let before = unsafe { &*app.0 }.host.session.engine().document().clone();
        let checkpoint = unsafe { &*app.0 }.host.session.engine().checkpoint();
        let catalog = layer_core::bundled_effect_catalog();
        let mut definition = catalog.get("unsharp_mask").unwrap().clone();
        std::sync::Arc::make_mut(&mut definition.program).label = "Updated library".into();
        let package = layer_core::EffectPackage {
            format: 1,
            categories: catalog.categories().to_vec(),
            filters: vec![definition],
        };
        app.request(2, json!({"type":"load_filter_package", "manifest":serde_json::to_string(&package).unwrap(),
            "modules":{}, "mode":"replace", "library":true})).unwrap();
        assert_eq!(unsafe { capy_apple_project_ready(app.0) }, 1);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.state()["filter_load"]["pending"] == true {
            assert!(std::time::Instant::now() < end);
            app.draw_frame();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(unsafe { capy_apple_project_ready(app.0) }, 0);
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document(), &before);
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().checkpoint(),
            checkpoint
        );
    }
}

#[test]
fn application_menu_actions_change_real_pixels_on_both_apple_platforms() {
    fn item(app: &App, command: &str) -> Value {
        fn find(value: &Value, command: &str) -> Option<Value> {
            if value["action"]["command"] == command {
                return Some(value.clone());
            }
            match value {
                Value::Array(items) => items.iter().find_map(|v| find(v, command)),
                Value::Object(map) => map.values().find_map(|v| find(v, command)),
                _ => None,
            }
        }
        layer_ui::ApplicationMenu::ALL
            .into_iter()
            .find_map(|menu| {
                find(
                    &app.request(2, json!({"type":"application_menu","menu":menu}))
                        .unwrap(),
                    command,
                )
            })
            .unwrap()
    }
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let ink = app.pixels();
        let clear = item(&app, "clear_layer");
        assert_eq!(clear["enabled"], true);
        app.action(clear["action"].clone());
        app.draw_frame();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_frame();
        assert_eq!(app.pixels(), ink);
        app.action(item(&app, "select_all")["action"].clone());
        app.draw_frame();
        app.action(item(&app, "fill_selection")["action"].clone());
        app.draw_frame();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_frame();
        assert_eq!(app.pixels(), ink);
        app.action(item(&app, "deselect")["action"].clone());
        app.draw_frame();
        assert_eq!(item(&app, "deselect")["enabled"], false);
        let filters = app
            .request(2, json!({"type":"application_menu","menu":"filter"}))
            .unwrap();
        let action = filters["sections"][0]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|category| category["sections"][0].as_array().unwrap())
            .find(|item| item["action"]["action"]["effect"] == "gaussian_blur")
            .unwrap()["action"]
            .clone();
        app.action(action);
        app.draw_frame();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_frame();
        assert_eq!(app.pixels(), ink);
    }
}

impl App {
    fn new(platform: u32) -> Self {
        let app = Self(capy_apple_create(platform));
        assert!(!app.0.is_null());
        assert_eq!(unsafe { capy_apple_resize(app.0, 1200, 900, 1.) }, 0);
        app
    }
    fn request(&self, kind: u32, value: Value) -> Option<Value> {
        let text = CString::new(value.to_string()).unwrap();
        let result = unsafe { capy_apple_request(self.0, kind, text.as_ptr()) };
        let error = unsafe { capy_apple_error(self.0) };
        assert!(
            error.is_null(),
            "{}",
            unsafe { CStr::from_ptr(error) }.to_string_lossy()
        );
        if result.is_null() {
            return None;
        }
        let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(result) };
        Some(value)
    }
    fn action(&self, action: Value) {
        self.request(0, action).unwrap();
    }
    fn invoke(&self, command: &str) {
        self.action(json!({"type": "invoke", "command": command}));
    }
    fn state(&self) -> Value {
        serde_json::to_value(unsafe { &*self.0 }.host.session.state()).unwrap()
    }
    fn draw_frame(&self) {
        let app = unsafe { &mut *self.0 };
        app.host
            .prepare_canvas_frame(2_000_000_000, 2_000_000_000, true)
            .unwrap();
    }
    fn layer_action(&self, action: Value) {
        self.action(json!({"type": "layer", "action": action}));
    }
    fn layer(&self, id: u64) -> Value {
        self.state()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == id)
            .unwrap()
            .clone()
    }
    fn stroke(&self) {
        let revision = unsafe { capy_apple_camera_revision(self.0) };
        self.stroke_at(revision);
    }
    fn stroke_at(&self, revision: u64) {
        let records = [
            500.,
            400.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_000_000_000.,
            1.,
            600.,
            500.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_010_000_000.,
            2.,
            650.,
            520.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_020_000_000.,
            3.,
        ];
        assert_eq!(
            unsafe {
                capy_apple_pointer(
                    self.0,
                    1,
                    1,
                    0,
                    records.as_ptr(),
                    records.len(),
                    0,
                    revision,
                )
            },
            0
        );
    }
    fn pixels(&self) -> Vec<u8> {
        let renderer = unsafe { &mut *self.0 }
            .host
            .session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap();
        renderer.request_readback(1).unwrap();
        renderer
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .unwrap();
        renderer
            .take_readback()
            .expect("GPU readback completed")
            .unwrap()
            .bytes
    }
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe { capy_apple_destroy(self.0) }
    }
}

#[test]
fn apple_document_archive_replays_exact_pixels_in_a_fresh_gpu_session() {
    use layer_core::{Project, ProjectAssetFormat, ProjectLimits};
    use layer_render::{HostImage, PixelFormat};
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        let paper = app.pixels();
        let name = CString::new("Embedded source").unwrap();
        let pixels: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                [
                    (i * 37) as u8,
                    (i * 13) as u8,
                    (i * 7) as u8,
                    (i % 256) as u8,
                ]
            })
            .collect();
        assert_eq!(
            unsafe {
                capy_apple_import_layer(app.0, name.as_ptr(), 64, 64, pixels.as_ptr(), pixels.len())
            },
            0
        );
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.action(
            json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::Pencil as u32}),
        );
        app.action(json!({"type":"set_color","rgba":[0.7,0.2,0.9,1]}));
        app.stroke();
        app.draw_frame();
        app.layer_action(json!({"op":"add_mask","id":id,"replace":false}));
        app.action(json!({"type":"set_color","rgba":[0,0,0,1]}));
        app.stroke();
        app.draw_frame();
        app.layer_action(json!({"op":"apply_mask","id":id}));
        app.draw_frame();
        app.invoke("scale_rotate");
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        app.invoke("apply_transform");
        app.draw_frame();
        let expected = app.pixels();
        assert!(expected != paper, "Fixture must contain visible artwork");
        let source = unsafe { &*app.0 };
        let engine = source.host.session.engine();
        let original =
            Project::snapshot_with(engine.document(), |id| engine.backend().source_asset(id))
                .unwrap();
        assert!(
            original
                .assets
                .values()
                .any(|a| a.format == ProjectAssetFormat::Rgba8Srgb)
        );
        assert!(
            original
                .assets
                .values()
                .any(|a| a.format == ProjectAssetFormat::R8Unorm)
        );
        let mut bytes = Vec::new();
        original.write(&mut bytes).unwrap();
        let Project { document, assets } =
            Project::read(bytes.as_slice(), ProjectLimits::default()).unwrap();
        assert_eq!(&original.document, &document);
        let mut gpu =
            layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required");
        for (id, asset) in &assets {
            gpu.prepare_asset(
                id,
                HostImage {
                    width: asset.extent[0],
                    height: asset.extent[1],
                    stride: asset.extent[0] * asset.format.channels(),
                    bytes: &asset.bytes,
                    format: match asset.format {
                        ProjectAssetFormat::R8Unorm => PixelFormat::R8Unorm,
                        ProjectAssetFormat::Rgba8Srgb => PixelFormat::Rgba8Srgb,
                    },
                },
            )
            .unwrap();
            let retained = gpu.source_asset(id).unwrap();
            assert_eq!(retained, *asset);
            let second = gpu.source_asset(id).unwrap();
            assert!(
                std::sync::Arc::ptr_eq(&retained.bytes, &second.bytes),
                "Source access must share storage"
            );
        }
        let restored = App::new(platform);
        let host = &mut unsafe { &mut *restored.0 }.host;
        host.session =
            layer_ui::UiSession::new(layer_host::Renderer(Some(gpu)), document, [1200, 900])
                .unwrap();
        host.session
            .set_platform(source.host.session.state().platform);
        host.resize(1200, 900, 1.).unwrap();
        restored.draw_frame();
        assert!(
            restored.pixels() == expected,
            "Fresh GPU replay must match every document byte, platform {platform}"
        );
        restored.action(json!({"type":"set_color","rgba":[1,0,0,1]}));
        restored.stroke();
        restored.draw_frame();
        assert!(
            restored.pixels() != expected,
            "Reopened artwork remains editable"
        );
        restored.invoke("undo");
        restored.draw_frame();
        assert!(
            restored.pixels() == expected,
            "New edits undo to the reopened artwork"
        );
        assert!(
            app.pixels() == expected,
            "Save/reopen must not change the original session"
        );
    }
}

#[test]
fn ui_actions_change_only_the_addressed_apple_session() {
    for platform in [0, 1] {
        let first = App::new(platform);
        let second = App::new(platform);
        let second_before = second.state();
        first.action(json!({"type":"customize","action":{"type":"insert_tools","panel":"toolbar","before":null}}));
        for command in ["new_window", "close_document"] {
            first.action(json!({"type":"customize","action":{"type":"picker_select",
                "control":{"kind":"command","command":command},"selected":true}}));
        }
        first.action(json!({"type":"customize","action":{"type":"confirm_tools"}}));
        first.action(json!({"type": "set_brush_size", "value": 42}));
        first.action(json!({"type": "set_brush_opacity", "value": 0.4}));
        assert_eq!(first.state()["brush"]["diameter"], 42.);
        assert!((first.state()["brush"]["opacity"].as_f64().unwrap() - 0.4).abs() < 0.00001);
        let zoom = first.state()["camera"]["zoom"].as_f64().unwrap();
        first.invoke("zoom_in");
        assert!(first.state()["camera"]["zoom"].as_f64().unwrap() > zoom);
        first.invoke("settings");
        assert!(!first.request(3, Value::Null).unwrap()["preferences"].is_null());
        first.action(json!({"type": "close_settings"}));
        assert!(first.request(3, Value::Null).unwrap()["preferences"].is_null());
        let menu = first
            .request(2, json!({"type":"application_menu","menu":"file"}))
            .unwrap();
        let new_window = menu["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["action"]["command"] == "new_window")
            .unwrap();
        assert_eq!(new_window["enabled"], true);
        first.action(new_window["action"].clone());
        let state = first.state();
        let request = state["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["type"] == "new_window")
            .unwrap();
        first.action(json!({"type":"complete_request","id":request["id"],"error":null}));
        first.invoke("add_layer");
        first.invoke("add_layer");
        assert_eq!(first.state()["layers"].as_array().unwrap().len(), 4);
        first.invoke("undo");
        assert_eq!(first.state()["layers"].as_array().unwrap().len(), 3);
        assert_eq!(
            second.state(),
            second_before,
            "Actions must stay in their editor session"
        );
    }
}

#[test]
fn apple_paint_undo_redo_restores_exact_document_pixels() {
    for platform in [0, 1] {
        let app = App::new(platform);
        // A real hardware renderer with no window: these are document/output
        // checks, not display timing or native physical-input acceptance.
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        app.action(json!({"type": "set_brush_size", "value": 32}));
        app.draw_frame();
        let initial = app.pixels();
        app.stroke();
        app.draw_frame();
        let painted = app.pixels();
        assert!(painted != initial, "Pen-up must leave real document pixels");
        app.invoke("undo");
        app.draw_frame();
        assert!(
            app.pixels() == initial,
            "Undo must restore every document byte"
        );
        app.invoke("redo");
        app.draw_frame();
        assert!(
            app.pixels() == painted,
            "Redo must reproduce the committed stroke exactly"
        );
    }
}

#[test]
fn staged_paper_preserves_pending_ink_and_reaches_brush_readiness() {
    use layer_render_wgpu::WgpuRasterizer;
    use std::time::{Duration, Instant};
    for platform in [0, 1] {
        let app = App::new(platform);
        let reference = WgpuRasterizer::new_headless().expect("Hardware GPU required");
        let staged = WgpuRasterizer::from_wgpu_staged(
            reference.adapter().clone(),
            reference.device().clone(),
            reference.queue().clone(),
        )
        .unwrap();
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(reference);
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        // Accepted engine work must survive the first paper-only submission.
        app.stroke();
        {
            let host = &mut unsafe { &mut *app.0 }.host;
            host.session.renderer_mut().0 = Some(staged);
            host.startup = Default::default();
            host.prepare_canvas_frame(2_000_000_000, 2_000_000_000, false)
                .unwrap();
            assert!(!host.startup.canvas_ready);
            assert!(host.dirty, "Startup must keep the frame driver awake");
        }
        let paper = app.pixels();
        app.action(json!({"type": "set_brush_opacity", "value": 0.4}));
        assert!((app.state()["brush"]["opacity"].as_f64().unwrap() - 0.4).abs() < 0.00001);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let host = &mut unsafe { &mut *app.0 }.host;
            host.prepare_canvas_frame(2_100_000_000, 2_100_000_000, true)
                .unwrap();
            if host.startup.brush_ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Staged brush readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            app.pixels() != paper,
            "The initial paper frame must retain pending stroke work"
        );
        app.invoke("undo");
        app.draw_frame();
        assert!(
            app.pixels() == paper,
            "Replayed ink remains exactly undoable"
        );
    }
}

#[test]
fn layer_panel_actions_preserve_targets_masks_hierarchy_and_menu_policy() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let original = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"new","group":false,"clipped":false}));
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"begin_rename","id":id}));
        assert_eq!(app.state()["layer_tools"]["rename_layer"], id);
        app.layer_action(json!({"op":"rename","id":id,"name":"Test ink"}));
        assert_eq!(app.layer(id)["label"], "Test ink");
        app.layer_action(json!({"op":"blend","id":id,"value":2}));
        assert_eq!(app.layer(id)["blend"], 2);
        app.action(json!({"type":"set_layer_opacity","opacity":0.35}));
        assert!((app.layer(id)["opacity"].as_f64().unwrap() - 0.35).abs() < 0.00001);
        app.layer_action(json!({"op":"alpha_lock","id":id,"value":true}));
        assert_eq!(app.layer(id)["alpha_locked"], true);
        app.layer_action(json!({"op":"lock","id":id,"value":true}));
        assert_eq!(app.state()["layer_tools"]["controls"]["opacity"], false);
        let locked = app
            .request(2, json!({"type":"layer_menu","id":id,"mask":false}))
            .unwrap();
        let rename = locked["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["action"]["action"]["op"] == "begin_rename")
            .unwrap();
        assert_eq!(
            rename["enabled"], false,
            "Menu capabilities must come from shared policy"
        );
        app.layer_action(json!({"op":"lock","id":id,"value":false}));
        app.layer_action(json!({"op":"toggle_selection","id":original}));
        assert_eq!(app.state()["layer_tools"]["editing_layer"]["id"], id);
        assert_eq!(app.layer(original)["selected"], true);
        app.layer_action(json!({"op":"context","id":id,"mask":false}));
        assert_eq!(
            app.layer(original)["selected"],
            true,
            "Context on a selected row keeps checked selection"
        );
        app.layer_action(json!({"op":"reference_selection"}));
        assert_eq!(app.layer(original)["reference"], true);
        assert_eq!(app.layer(id)["reference"], true);
        app.layer_action(json!({"op":"add_mask","id":id,"replace":false}));
        assert_eq!(app.layer(id)["has_mask"], true);
        app.layer_action(json!({"op":"select","id":id,"mask":true}));
        assert_eq!(app.layer(id)["mask_selected"], true);
        app.layer_action(json!({"op":"link_mask","id":id,"value":false}));
        app.layer_action(json!({"op":"enable_mask","id":id,"value":false}));
        assert_eq!(app.layer(id)["mask_linked"], false);
        let menu = app
            .request(2, json!({"type":"layer_menu","id":id,"mask":true}))
            .unwrap();
        let enabled = menu["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["label"] == "Enable mask")
            .unwrap();
        assert_eq!(enabled["selected"], false);
        assert_eq!(enabled["enabled"], true);
        app.layer_action(json!({"op":"delete_mask","id":id}));
        assert_eq!(app.layer(id)["has_mask"], false);
        app.invoke("undo");
        assert_eq!(app.layer(id)["has_mask"], true);
        app.layer_action(json!({"op":"new","group":true,"clipped":false}));
        let group = app.state()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["group"] == true)
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"drop","id":original,"target":group,"fraction":0.5}));
        assert_eq!(app.layer(original)["depth"], 1);
        app.layer_action(json!({"op":"collapse","id":group}));
        assert!(
            !app.state()["layers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l["id"] == original)
        );
        app.layer_action(json!({"op":"collapse","id":group}));
        assert_eq!(app.layer(original)["depth"], 1);
    }
}

#[test]
fn image_import_changes_gpu_pixels_is_undoable_and_produces_a_thumbnail() {
    use std::time::{Duration, Instant};
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().unwrap());
        app.draw_frame();
        let paper = app.pixels();
        let name = CString::new("Test image").unwrap();
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0];
        let before = app.state();
        assert_eq!(
            unsafe { capy_apple_import_layer(app.0, name.as_ptr(), 2, 2, rgba.as_ptr(), 15) },
            -1
        );
        assert_eq!(
            app.state(),
            before,
            "Incomplete pixels cannot mutate the document"
        );
        assert_eq!(
            unsafe {
                capy_apple_import_layer(app.0, name.as_ptr(), 2, 2, rgba.as_ptr(), rgba.len())
            },
            0
        );
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        assert_eq!(app.layer(id)["label"], "Test image");
        app.draw_frame();
        let imported = app.pixels();
        assert!(imported != paper, "Import must reach the GPU document");
        let mut reply = app
            .request(2, json!({"type":"layer_thumbnails","requests":[[99,id]]}))
            .unwrap();
        assert_eq!(reply["accepted"], json!([99]));
        let deadline = Instant::now() + Duration::from_secs(5);
        while reply["images"].as_array().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "Thumbnail readback timed out");
            std::thread::sleep(Duration::from_millis(1));
            reply = app
                .request(2, json!({"type":"layer_thumbnails","requests":[]}))
                .unwrap();
        }
        let image = &reply["images"][0];
        assert_eq!(image[0], 99);
        assert_eq!(
            image[3].as_array().unwrap().len(),
            image[1].as_u64().unwrap() as usize * image[2].as_u64().unwrap() as usize * 4
        );
        app.invoke("undo");
        app.draw_frame();
        assert!(app.pixels() == paper);
        app.invoke("redo");
        app.draw_frame();
        assert!(app.pixels() == imported);
    }
}

#[test]
fn stateless_numeric_input_uses_shared_policy_without_a_session() {
    let control = serde_json::to_value(layer_ui::ui_catalog()).unwrap()["layer_opacity"].clone();
    let resolve = |operation| {
        let json =
            CString::new(json!({"control":control,"value":1.,"operation":operation}).to_string())
                .unwrap();
        let output = unsafe { capy_apple_numeric(json.as_ptr()) };
        assert!(!output.is_null());
        let result: Value =
            serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(output) };
        result
    };
    assert_eq!(
        resolve(json!({"type":"expression","text":"25+25"}))["value"],
        0.5
    );
    assert_eq!(
        resolve(json!({"type":"position","position":0.25}))["value"],
        0.25
    );
    assert_eq!(resolve(json!({"type":"format"}))["text"], "100");
    assert!(resolve(json!({"type":"expression","text":"invalid"}))["error"].is_string());
}

#[test]
fn apple_tool_panels_edit_every_visible_brush_setting_through_the_abi() {
    for platform in [0, 1] {
        let app = App::new(platform);
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":false}}));
        let snapshot = app.request(3, Value::Null).unwrap();
        let menu_action = snapshot["workspace_menu"]["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["action"]["action"]["panel"] == "tool_settings")
            .expect("Tool Settings must be reachable from Workspace")["action"]
            .clone();
        app.action(menu_action);
        let snapshot = app.request(3, Value::Null).unwrap();
        assert!(
            snapshot["panels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "tool_settings"
                    && p["controls"][0]["control"] == "tool_settings")
        );
        let catalog = app.request(2, json!({"type":"catalog"})).unwrap();
        let mut edited = 0;
        for category in catalog["brush_categories"].as_array().unwrap() {
            for brush in category["brushes"].as_array().unwrap() {
                app.action(json!({"type":"select_brush","id":brush["id"]}));
                let state = app.state();
                assert!(
                    state["tool_set"]["subtools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["preview"] == brush["id"] && item["selected"] == true)
                );
                for setting in state["tool_settings"].as_array().unwrap() {
                    // Re-select to prevent a previous edit changing the schema.
                    app.action(json!({"type":"select_brush","id":brush["id"]}));
                    let resolved = app
                        .request(
                            4,
                            json!({"control":setting["numeric"],
                        "value":setting["value"],"operation":{"type":"position","position":0.37}}),
                        )
                        .unwrap();
                    app.action(json!({"type":"set_tool_setting","id":setting["id"],"value":resolved["value"]}));
                    let after = app.state();
                    let actual = after["tool_settings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|s| s["id"] == setting["id"])
                        .unwrap();
                    assert_eq!(
                        actual["value"].as_f64().unwrap() as f32,
                        resolved["value"].as_f64().unwrap() as f32,
                        "{} {}",
                        brush["label"],
                        setting["id"]
                    );
                    edited += 1;
                }
            }
        }
        assert!(
            edited > 100,
            "Exercise the complete catalog, including wet and liquify controls"
        );
        app.invoke("ruler");
        for id in ["snap_rulers", "show_rulers"] {
            let before = app.state();
            assert!(
                before["tool_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["command"] == id && a["checkable"] == true)
            );
            let checked = before["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()["selected"]
                .clone();
            app.invoke(id);
            let after = app.state();
            assert_ne!(
                after["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == id)
                    .unwrap()["selected"],
                checked
            );
        }
    }
}

#[test]
fn apple_transform_settings_and_actions_preserve_pixel_transactions() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let painted = app.pixels();
        app.invoke("scale_rotate");
        assert_eq!(app.state()["tool_settings"].as_array().unwrap().len(), 5);
        for command in ["transform_aspect", "apply_transform", "cancel_transform"] {
            assert!(
                app.state()["tool_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["command"] == command)
            );
        }
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        app.draw_frame();
        assert!(
            app.pixels() != painted,
            "Numeric edits must update the live GPU preview"
        );
        app.invoke("cancel_transform");
        app.draw_frame();
        assert!(
            app.pixels() == painted,
            "Cancel must restore all original document pixels"
        );
        app.invoke("scale_rotate");
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        let before = app.state()["tool_settings"].clone();
        let invalid = CString::new(
            json!({"type":"set_tool_setting","id":"transform_width","value":0}).to_string(),
        )
        .unwrap();
        assert!(unsafe { capy_apple_request(app.0, 0, invalid.as_ptr()) }.is_null());
        assert!(
            !unsafe { capy_apple_error(app.0) }.is_null(),
            "Semantic rejection must return field feedback"
        );
        assert_eq!(
            app.state()["tool_settings"],
            before,
            "Rejected scale must preserve accepted fields"
        );
        app.invoke("apply_transform");
        app.draw_frame();
        let transformed = app.pixels();
        assert!(transformed != painted);
        app.invoke("undo");
        app.draw_frame();
        assert!(app.pixels() == painted);
        app.invoke("redo");
        app.draw_frame();
        assert!(app.pixels() == transformed);
    }
}

#[test]
fn optional_gpu_timing_has_explicit_uninitialized_state_and_bounded_abi() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let mut stats = layer_render_wgpu::GpuFrameTimingStats::default();
        unsafe {
            assert_eq!(capy_apple_gpu_timing(app.0, 1), 0);
            assert_eq!(capy_apple_frame(app.0, 1, 1, std::ptr::null_mut()), 0);
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 0, &mut stats),
                0
            );
            assert_eq!(
                stats.support, 0,
                "No renderer must not report available timestamps"
            );
            assert_eq!(stats.requested, 0);
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 1, &mut stats),
                -1
            );
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 257, &mut stats),
                -1
            );
            assert_eq!(capy_apple_gpu_timing(app.0, 2), -1);
            assert_eq!(capy_apple_gpu_timing(app.0, 0), 0);
        }
    }
}

#[test]
fn apple_color_wheel_slots_and_channel_edits_use_shared_policy() {
    let close = |actual: &Value, expected: [f32; 4]| {
        for i in 0..4 {
            assert!(
                (actual[i].as_f64().unwrap() - expected[i] as f64).abs() < 1e-5,
                "{actual} != {expected:?}"
            );
        }
    };
    for platform in [0, 1] {
        let app = App::new(platform);
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":false}}));
        let initial = app.request(3, Value::Null).unwrap();
        let open = initial["workspace_menu"]["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|section| section.as_array().unwrap())
            .find(|item| item["action"]["action"]["panel"] == "color")
            .unwrap()["action"]
            .clone();
        app.action(open);
        let snapshot = app.request(3, Value::Null).unwrap();
        assert!(
            snapshot["panels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "color")
        );
        let action = |value| app.action(json!({"type":"color","action":value}));
        app.action(json!({"type":"set_color","rgba":[1,0,0,1]}));
        action(json!({"op":"component","index":0,"value":180}));
        close(&app.state()["brush"]["color"], [0., 1., 1., 1.]);
        action(json!({"op":"select","slot":"background"}));
        for (index, value) in [0.25, 0.5, 0.75].into_iter().enumerate() {
            action(json!({"op":"rgba_component","index":index,"value":value}));
        }
        close(&app.state()["brush"]["color"], [0.25, 0.5, 0.75, 1.]);
        close(&app.state()["colors"]["foreground"], [0., 1., 1., 1.]);
        action(json!({"op":"select","slot":"transparent"}));
        action(json!({"op":"pick","part":"hue","point":[0.95,0.5],"size":1}));
        assert_eq!(app.state()["colors"]["slot"], "background");
        close(&app.state()["brush"]["color"], [0.25, 0.75, 0.5, 1.]);
        action(json!({"op":"toggle_space"}));
        action(json!({"op":"pick","part":"field","point":[0.5,0.5],"size":1}));
        close(&app.state()["brush"]["color"], [1. / 3., 2. / 3., 0.5, 1.]);
        let snapshot = app.request(3, Value::Null).unwrap();
        assert_eq!(snapshot["color_panel"]["components"][1]["label"], "L");
        assert_eq!(snapshot["color_panel"]["swatches"][1]["selected"], true);
        action(json!({"op":"swap"}));
        close(&app.state()["brush"]["color"], [0., 1., 1., 1.]);
        close(
            &app.state()["colors"]["foreground"],
            [1. / 3., 2. / 3., 0.5, 1.],
        );
    }
    for space in [0, 1] {
        assert_eq!(capy_apple_color_hit(95., 50., 100., space), 1);
        assert_eq!(capy_apple_color_hit(50., 50., 100., space), 2);
        assert_eq!(capy_apple_color_hit(0., 0., 100., space), 0);
        assert_eq!(capy_apple_color_hit(f32::NAN, 50., 100., space), 0);
    }
    assert_eq!(capy_apple_color_hit(50., 50., 0., 0), 0);
    assert_eq!(capy_apple_color_hit(50., 50., 100., 2), 0);
}

#[test]
fn apple_color_actions_change_real_paint_and_transparent_eraser_pixels() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type":"set_color","rgba":[1,0,0,1]}));
        let brush = app.state()["brush"].clone();
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let painted = app.pixels();
        let reds = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .filter(|p| p[0] > 200 && p[1] < 50 && p[2] < 50)
                .count()
        };
        assert!(
            reds(&painted) > 0,
            "Explicit color must reach the GPU brush"
        );
        app.action(json!({"type":"color","action":{"op":"select","slot":"transparent"}}));
        assert_eq!(app.state()["brush"]["preset"], brush["preset"]);
        assert_eq!(app.state()["brush"]["diameter"], brush["diameter"]);
        app.stroke();
        app.draw_frame();
        assert!(
            reds(&app.pixels()) < reds(&painted),
            "Transparent paint must erase with the current tip"
        );
        app.invoke("undo");
        app.draw_frame();
        assert!(app.pixels() == painted);
    }
}
