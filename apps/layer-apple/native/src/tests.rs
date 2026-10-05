//! Test editor effects after UI action dispatch, without automating OS menus.
use super::*;
use serde_json::{Value, json};

fn stateless(call: unsafe extern "C" fn(*const c_char) -> *mut c_char, request: impl Into<Vec<u8>>) -> Value {
    let source = CString::new(request).unwrap();
    let output = unsafe { call(source.as_ptr()) };
    assert!(!output.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(output) };
    value
}
fn localized_stateless(call: unsafe extern "C" fn(*const c_char) -> *mut c_char, request: impl Into<Vec<u8>>) -> Value {
    fixture_localization();
    let request = request.into();
    let request = serde_json::from_slice::<Value>(&request).map(|request| {
        if request.get("language").is_some() && request.get("request").is_some() { request }
        else { json!({"language":"en", "request":request}) }
    }).map(|request| request.to_string().into_bytes()).unwrap_or(request);
    stateless(call, request)
}

struct App(*mut CapyApple);
#[path = "color_tests.rs"]
mod color;
#[path = "source_tests.rs"]
mod source;
#[path = "lookup_tests.rs"]
mod lookup;
#[path = "correction_tests.rs"]
mod correction;
#[path = "proof_tests.rs"]
mod proof;
#[path = "hdr_tests.rs"]
mod hdr;
#[path = "document_tabs_tests.rs"]
mod document_tabs;
#[path = "photo_tests.rs"]
mod photo;
fn native_renderer() -> Box<layer_render_wgpu::WgpuRasterizer> {
    layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).expect("Native SDR hardware GPU required").into()
}
#[path = "header_tests.rs"]
mod header;
#[path = "input_tests.rs"]
mod input;
#[path = "navigator_tests.rs"]
mod navigator;
#[path = "session_tests.rs"]
mod session;
#[path = "region_tests.rs"]
mod region;
#[path = "selection_tests.rs"]
mod selection;
#[path = "canvas_bar_tests.rs"]
mod canvas_bar;
#[path = "renderer_tests.rs"]
mod renderer;
#[path = "workspace_tests.rs"]
mod workspace;
#[path = "workspace_motion_tests.rs"]
mod workspace_motion;
#[path = "toolbar_component_tests.rs"]
mod toolbar_component;
#[path = "picker_tests.rs"]
mod picker;
#[path = "tonal_tests.rs"]
mod tonal;
#[path = "palette_tests.rs"]
mod palette;

mod fixtures {
    use super::*;

    pub(super) fn selection_app(platform: u32) -> App {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let project = ProjectJob::new(&app, true);
        assert_eq!(project.create([64, 64]), 0);
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, project.0, c"Selection check".as_ptr(), c"".as_ptr()) },
            0
        );
        app.draw_until_idle();
        app
    }

    pub(super) fn surface(app: &App, [x, y]: [f64; 2]) -> [f64; 2] {
        let m = unsafe { &*app.0 }.host.session.state().camera.document_to_surface();
        let [x, y] = [x as f32, y as f32];
        [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]].map(f64::from)
    }

    pub(super) fn drag(app: &App, id: u64, device: u32, from: [f64; 2], to: [f64; 2]) {
        let points = [from, [(from[0] + to[0]) / 2., (from[1] + to[1]) / 2.], to];
        let records: Vec<f64> = points.iter().enumerate().flat_map(|(i, &point)| {
            let [x, y] = surface(app, point);
            [x, y, 1., 0., 0., 0., 0., (1_000_000_000 + id * 10_000_000 + i as u64 * 1_000_000) as f64, (i + 1) as f64]
        }).collect();
        assert_eq!(
            unsafe {
                capy_apple_pointer(app.0, id, device, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0))
            },
            0
        );
        app.draw_until_idle();
    }

    pub(super) fn selection_bounds(app: &App) -> Option<[f32; 4]> {
        let session = &unsafe { &*app.0 }.host.session;
        session.engine().document().working.selection.as_ref().map(|selection| match &selection.shape {
            layer_core::SelectionShape::Pixels(mask) => mask.bounds().map(|v| v as f32),
            _ => {
                let b = selection.bounds();
                [b.min.x + 1., b.min.y + 1., b.max.x - 1., b.max.y - 1.]
            }
        })
    }

    pub(super) fn until(app: &App, what: &str, done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "{what}");
            app.draw_frame();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    pub(super) fn tempfile() -> std::fs::File {
        let path = std::env::temp_dir().join(format!("capy-apple-{}-{}", std::process::id(), layer_workspace::new_id()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        file
    }
}

type RasterSamples = (
    std::collections::BTreeMap<
        layer_core::raster::TileKey,
        (layer_core::color::PixelDescriptor, Vec<u8>),
    >,
    Option<layer_core::raster::RasterWatercolor>,
);

fn raster_samples(revision: &layer_core::raster::RasterRevision) -> RasterSamples {
    let data = revision.wait_data().unwrap();
    let tiles = data
        .tiles
        .iter()
        .map(|(key, tile)| {
            let blob = tile.wait_backing().unwrap();
            (*key, (blob.descriptor, blob.decode().unwrap()))
        })
        .collect();
    (tiles, data.watercolor)
}

fn write_capture(capture: &layer_core::authored::ArtworkCapture, output: &mut impl std::io::Write) {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    layer_core::package::codec::PreparedPackage::prepare(capture, None, &cancel).unwrap().write(output, &cancel).unwrap();
}
fn write_document(document: &layer_core::Document, output: &mut impl std::io::Write) {
    let capture = layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap();
    write_capture(&capture, output);
}
fn read_document(input: impl std::io::Read + std::io::Seek) -> layer_core::Document {
    layer_core::temp_files::set_directory(std::env::temp_dir()).unwrap();
    let outcome = layer_ui::read_import(input, layer_ui::ImportIntent::Open, Default::default(), layer_core::DocumentNames { paint: "Paint".into(), paper: "Paper".into() }, Default::default(), Default::default(), &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let layer_ui::ImportOutcome::Editable(imported) = outcome else { panic!("Expected editable artwork") };
    imported.project
}
fn occurrence_at(document: &layer_core::Document, index: usize) -> &layer_core::authored::Occurrence {
    document.scene().occurrence(document.scene().order()[index]).unwrap()
}
fn imported_occurrences(document: &layer_core::Document) -> impl Iterator<Item = layer_core::authored::OccurrenceHandle> + '_ {
    document.scene().order().iter().copied().filter(|h| document.scene().paint_source(*h).is_some_and(|source| source.original.is_some()))
}
fn first_original(document: &layer_core::Document) -> &layer_core::color::source::SourceImage {
    document.scene().paint_source(imported_occurrences(document).next().unwrap()).unwrap().original.as_deref().unwrap()
}
fn active_raster(document: &layer_core::Document) -> &layer_core::raster::RasterRevision {
    document.target_raster(document.working.target.unwrap()).unwrap()
}
fn source_samples(source: &layer_core::color::source::SourceImage) -> Vec<Vec<u8>> {
    source.tiles.values().map(|tile| tile.decode().unwrap()).collect()
}
fn assert_source_samples(actual: &layer_core::color::source::SourceImage, expected: &layer_core::color::source::SourceImage) {
    assert_eq!((actual.kind, actual.extent, actual.resolution, actual.interpretation.channels, actual.interpretation.depth, actual.interpretation.profile_assumed),
        (expected.kind, expected.extent, expected.resolution, expected.interpretation.channels, expected.interpretation.depth, expected.interpretation.profile_assumed));
    assert_eq!(layer_color::profile_bytes(&actual.interpretation.profile).unwrap(), layer_color::profile_bytes(&expected.interpretation.profile).unwrap());
    assert!(actual.tiles.keys().eq(expected.tiles.keys()));
    assert_eq!(source_samples(actual), source_samples(expected));
}
fn assert_project_document(actual: &layer_core::Document, expected: &layer_core::Document) {
    let prepare = |document: &layer_core::Document| {
        let capture = layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap();
        layer_core::package::codec::PreparedPackage::prepare(&capture, None, &std::sync::atomic::AtomicBool::new(false)).unwrap()
    };
    assert_eq!(prepare(actual).manifest(), prepare(expected).manifest(), "Authored metadata and payloads must round-trip exactly");
    assert_eq!(actual.artwork.paint.len(), expected.artwork.paint.len());
    for (_, id, original) in expected.artwork.paint.iter() {
        let current = actual.artwork.paint.get(actual.artwork.paint.resolve(id).unwrap()).unwrap();
        assert!(current.operations.is_empty() && original.operations.is_empty());
        assert_eq!(raster_samples(&current.raster), raster_samples(&original.raster));
        assert_eq!(current.original.is_some(), original.original.is_some());
        if let (Some(current), Some(original)) = (&current.original, &original.original) { assert_source_samples(current, original); }
    }
    assert_eq!(actual.artwork.coverage.len(), expected.artwork.coverage.len());
    for (_, id, original) in expected.artwork.coverage.iter() {
        let current = actual.artwork.coverage.get(actual.artwork.coverage.resolve(id).unwrap()).unwrap();
        assert!(current.operations.is_empty() && original.operations.is_empty());
        assert_eq!(raster_samples(&current.raster), raster_samples(&original.raster));
    }
}
fn assert_saved_document(actual: &layer_core::Document, expected: &layer_core::Document) {
    let mut captured = expected.clone();
    let mut context = actual.output().context.clone();
    for (handle, _) in std::sync::Arc::make_mut(&mut context.phases) {
        let id = actual.artwork.effects.id(*handle).unwrap();
        *handle = captured.artwork.effects.resolve(id).unwrap();
    }
    captured.artwork.outputs.get_mut(captured.artwork.default_output).unwrap().context = context;
    assert_project_document(actual, &captured);
}

#[test]
fn storage_names_every_store_within_the_folders_it_is_given() {
    let temp = std::env::temp_dir();
    let support = temp.join("capy-apple-storage/support");
    let installation = stateless(capy_apple_storage, json!({"platform": {"config": support, "data": support,
        "state": support.join("State"), "cache": temp.join("capy-apple-storage/caches"), "temp": temp}}).to_string());
    assert_eq!(installation["settings"], json!(support.join("settings.json")));
    assert_eq!(installation["workspaces"], json!(support.join("workspaces")));
    assert_eq!(installation["sessions"], json!(support.join("State/sessions")));
    assert_eq!(installation["shaders"], json!(temp.join("capy-apple-storage/caches/shaders")));
    assert_eq!(layer_core::temp_files::directory().unwrap(), temp);
    let private = stateless(capy_apple_storage, r#"{"directory":"/private/capy"}"#);
    assert_eq!(private["color_profiles"], "/private/capy/data/color-profiles");
    assert_eq!(private["export_presets"], "/private/capy/config/export-presets");
    assert_eq!(private["state"], "/private/capy/state");
    let relative = json!({"platform": {"config": "support", "data": support, "state": support, "cache": support, "temp": temp}});
    assert!(stateless(capy_apple_storage, relative.to_string())["error"].is_string());
}

#[test]
fn workspace_request_starts_resumes_scene_and_closes() {
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(
        std::env::temp_dir().join(format!("capy-apple-workspaces-{}", layer_workspace::new_id())),
    );
    let start = json!({"type":"start","directory":directory.0.to_str().unwrap(),"scene":"first"});
    let until = |app: &App, ready: &dyn Fn(&Value) -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let view = app.request(6, json!({"type":"tick"})).unwrap()["view"].clone();
            if ready(&view) {
                return view;
            }
            assert!(std::time::Instant::now() < deadline, "{view}");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    let settled = |view: &Value| view["ready"] == true && view["busy"] == false;
    let app = App::new(1);
    app.request(6, start.clone()).unwrap();
    let view = until(&app, &settled);
    let first = view["id"].as_str().unwrap().to_owned();
    let other = view["switcher"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["id"].as_str())
        .find(|id| *id != first)
        .unwrap()
        .to_owned();
    app.request(6, json!({"type":"switch","id":other})).unwrap();
    until(&app, &|view| settled(view) && view["id"] == other.as_str());
    app.request(6, json!({"type":"close"})).unwrap();
    until(&app, &|view| view["closed"] == true);
    drop(app);
    let app = App::new(1);
    app.request(6, start).unwrap();
    assert_eq!(until(&app, &settled)["id"], other.as_str());
    app.request(6, json!({"type":"close"})).unwrap();
    until(&app, &|view| view["closed"] == true);
}

#[test]
fn color_raster_ffi_validates_buffer_and_matches_shared_spaces() {
    let mut bytes = [23; 16];
    unsafe {
        for (side, hue, count) in [
            (0, 0., 16),
            (2, f32::NAN, 16),
            (2, 60., 15),
            (u32::MAX, 0., usize::MAX),
        ] {
            assert_eq!(
                capy_apple_color_field(side, hue, 2, c"Srgb".as_ptr(), false, bytes.as_mut_ptr(), count),
                0
            );
            assert_eq!(bytes, [23; 16]);
        }
        assert_eq!(
            capy_apple_color_field(2, 60., 2, c"Srgb".as_ptr(), false, std::ptr::null_mut(), 16),
            0
        );
        assert_eq!(
            capy_apple_color_field(2, 60., 2, c"Srgb".as_ptr(), false, bytes.as_mut_ptr(), 16),
            1
        );
    }
    let mut reference = [0; 16];
    assert!(layer_ui::render_color_field(2, layer_ui::ColorShape::Triangle, 60., layer_core::color::RgbSpace::Srgb, DISPLAY_SPACE, &mut reference));
    assert_eq!(bytes, reference);
    for space in layer_core::color::RgbSpace::ALL {
        let name = serde_json::to_value(space).unwrap();
        let name = CString::new(name.as_str().unwrap()).unwrap();
        for shape in 0..3 {
            for guide in [false, true] {
                let mut bytes = vec![0; 64 * 64 * 4];
                assert_eq!(unsafe { capy_apple_color_field(64, 42., shape, name.as_ptr(), guide, bytes.as_mut_ptr(), bytes.len()) }, 1);
                let mut expected = vec![0; bytes.len()];
                assert!(if guide {
                    layer_ui::render_hue_guide_in(64, color_shape(shape).unwrap(), space, DISPLAY_SPACE, &mut expected)
                } else {
                    layer_ui::render_color_field(64, color_shape(shape).unwrap(), 42., space, DISPLAY_SPACE, &mut expected)
                });
                assert!(bytes == expected, "{space:?}/{shape}/{guide}: shared display pixels");
                for (invalid_shape, invalid_space) in [(3, name.as_ptr()), (shape, c"Unknown".as_ptr()), (shape, std::ptr::null())] {
                    assert_eq!(unsafe { capy_apple_color_field(64, 42., invalid_shape, invalid_space, guide, bytes.as_mut_ptr(), bytes.len()) }, 0);
                    assert!(bytes == expected, "Invalid requests must leave pixels unchanged");
                }
            }
        }
    }
}

#[test]
fn compact_color_circle_picking_uses_shared_shapes() {
    // A circular field includes its horizontal rim, outside the HSV square.
    assert_eq!(capy_apple_color_hit(80., 50., 100., 0), 2);
    assert_eq!(capy_apple_color_hit(80., 50., 100., 1), 0);
    for shape in [
        layer_ui::ColorShape::Circle,
        layer_ui::ColorShape::Square,
        layer_ui::ColorShape::Triangle,
    ]
    {
        for platform in [0, 1] {
            let app = App::new(platform);
            app.action(json!({"type":"color","action":{"op":"shape","shape":shape}}));
            let view = app.full_snapshot()["color_panel"].clone();
            assert_eq!(view["shape"], serde_json::to_value(shape).unwrap());
            let mut state = layer_ui::ColorState::default();
            state.apply(layer_ui::ColorAction::Shape { shape }).unwrap();
            app.action(json!({"type":"color","action":{"op":"pick_wheel","part":"field","point":[0.65,0.45],"size":1}}));
            state
                .apply(layer_ui::ColorAction::PickWheel {
                    part: layer_ui::ColorWheelPart::Field,
                    point: [0.65, 0.45],
                    size: 1.,
                })
                .unwrap();
            for (actual, expected) in app.state()["brush"]["color"]
                .as_array()
                .unwrap()
                .iter()
                .zip(state.rgba())
            {
                assert!((actual.as_f64().unwrap() - expected as f64).abs() < 1e-6);
            }
            let before = app.state()["brush"]["color"].clone();
            app.action(json!({"type":"color","action":{"op":"toggle_readout"}}));
            assert_eq!(app.state()["brush"]["color"], before);
            assert_eq!(
                app.full_snapshot()["color_panel"]["readout_label"],
                "RGB"
            );
        }
    }
    let mut bytes = vec![0; 128 * 128 * 4];
    assert_eq!(
        unsafe { capy_apple_color_field(128, 264., 0, c"Srgb".as_ptr(), false, bytes.as_mut_ptr(), bytes.len()) },
        1
    );
    let mut expected = vec![0; bytes.len()];
    assert!(layer_ui::render_color_field(128, layer_ui::ColorShape::Circle, 264., layer_core::color::RgbSpace::Srgb, DISPLAY_SPACE, &mut expected));
    assert_eq!(bytes, expected);
    assert_eq!(
        unsafe { capy_apple_color_field(128, 264., 3, c"Srgb".as_ptr(), false, bytes.as_mut_ptr(), bytes.len()) },
        0
    );
    assert_eq!(
        bytes, expected,
        "Invalid shapes cannot alter the supplied buffer"
    );
}

#[test]
fn filter_property_edits_reset_and_undo_through_the_abi() {
    let app = App::new(0);
    app.action(json!({"type":"effect","action":{"op":"insert","effect":"gaussian_blur"}}));
    let layer = app.state()["layer_properties"]["layer"].as_u64().unwrap();
    let control = || {
        app.state()["layer_properties"]["controls"].as_array().unwrap().iter()
            .find(|c| c["key"] == "sigma").unwrap().clone()
    };
    let before = control();
    app.action(json!({"type":"effect","action":{"op":"set","layer":layer,"key":"sigma","value":{"kind":"number","value":7}}}));
    let edited = control()["value"].clone();
    assert_ne!(edited, before["value"]);
    app.invoke("undo");
    assert_eq!(control()["value"], before["value"]);
    app.invoke("redo");
    assert_eq!(control()["value"], edited);
    app.action(json!({"type":"effect","action":{"op":"reset","layer":layer,"key":"sigma"}}));
    assert_eq!(control()["value"], before["default"]);
}

#[test]
fn property_edits_and_gestures_preserve_exact_metal_history_on_both_platforms() {
    for platform in [0, 1] {
        for target in ["paint", "paper", "gaussian_blur", "split_tone"] {
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            app.draw_frame();
            app.stroke();
            app.draw_frame();
            if target == "paper" {
                let document = unsafe { &*app.0 }.host.session.engine().document();
                let paper = *document.scene().order().iter().find(|handle| document.scene().effect(**handle).is_some_and(|effect| effect.program.id.as_ref() == "solid_color")).unwrap();
                let id = layer_ui::occurrence_token(paper);
                app.action(json!({"type":"select_layer","id":id}));
            } else if target != "paint" {
                app.action(json!({"type":"effect","action":{"op":"insert","effect":target}}));
            }
            app.draw_frame();
            let key = match target {
                "gaussian_blur" => "sigma",
                "split_tone" => "shadows",
                "paper" => "color",
                _ => "opacity",
            };
            let state = app.state();
            let layer = state["layer_properties"]["layer"].clone();
            let original = state["layer_properties"]["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|control| control["key"] == key)
                .unwrap()["value"]
                .clone();
            let (middle, final_value) = match target {
                "gaussian_blur" => (
                    json!({"kind":"number","value":6}),
                    json!({"kind":"number","value":12}),
                ),
                "split_tone" | "paper" => (
                    json!({"kind":"color","value":{"space":"Srgb","rgba":[0.4,0.2,0.1,1]}}),
                    json!({"kind":"color","value":{"space":"Srgb","rgba":[0.8,0.2,0.1,1]}}),
                ),
                _ => (
                    json!({"kind":"number","value":0.6}),
                    json!({"kind":"number","value":0.25}),
                ),
            };
            let set = |value: &Value| json!({"op":"set","layer":layer,"key":key,"value":value});
            let gesture = |phase: &str, value: &Value| {
                app.action(
                json!({"type":"effect","action":{"op":"gesture","phase":phase,"action":set(value)}}))
            };
            let before = app.pixels();
            app.action(json!({"type":"effect","action":set(&final_value)}));
            app.draw_frame();
            let edited = app.pixels();
            assert_ne!(edited, before, "{platform}/{target} must change artwork");
            app.invoke("undo");
            app.draw_frame();
            assert_eq!(app.pixels(), before);
            gesture("down", &original);
            gesture("move", &middle);
            app.draw_frame();
            gesture("move", &final_value);
            app.draw_frame();
            assert_eq!(
                app.pixels(),
                edited,
                "Preview pixels must match a discrete edit"
            );
            gesture("up", &final_value);
            app.draw_frame();
            assert_eq!(app.pixels(), edited);
            app.invoke("undo");
            app.draw_frame();
            assert_eq!(app.pixels(), before, "One Undo restores the complete drag");
            app.invoke("redo");
            app.draw_frame();
            assert_eq!(app.pixels(), edited);
            app.invoke("undo");
            app.draw_frame();
            gesture("down", &original);
            gesture("move", &final_value);
            app.draw_frame();
            assert_eq!(app.pixels(), edited);
            gesture("cancel", &final_value);
            app.draw_frame();
            assert_eq!(app.pixels(), before, "Cancel restores every artwork pixel");
            app.invoke("redo");
            app.draw_frame();
            assert_eq!(app.pixels(), edited, "Cancel preserves the earlier Redo");
        }
    }
}

#[test]
fn changing_filter_preview_source_does_not_fail_the_editor_and_can_retry() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let query = json!({"type":"filter_previews", "filters":["curves"], "size":[96,40]});
        let first = app.request(2, query.clone()).unwrap();
        assert_eq!(first["pending"], false, "New work waits for idle admission");
        std::thread::sleep(std::time::Duration::from_millis(210));
        let status = app.request(2, query.clone()).unwrap();
        assert_eq!(status["pending"], true);
        app.action(json!({"type":"set_layer_opacity", "opacity":0.5}));
        app.draw_frame();
        let changed = app.request(2, query.clone()).unwrap();
        assert_ne!(changed["key"], status["key"]);
        assert_eq!(changed["pending"], false, "A source change retires its old request");
        assert!(changed["error"].is_null());
        let atlas = unsafe { capy_apple_take_filter_previews(app.0) };
        assert!(atlas.is_null(), "Stale preview pixels must not be published");
        assert!(unsafe { capy_apple_error(app.0) }.is_null());
        let pixels = app.pixels();
        let revision = unsafe { &*app.0 }.host.session.filter_preview_revision();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            app.request(2, query.clone()).unwrap();
            let atlas = unsafe { capy_apple_take_filter_previews(app.0) };
            assert!(unsafe { capy_apple_error(app.0) }.is_null());
            if !atlas.is_null() {
                let mut info = std::mem::MaybeUninit::<CapyFilterPreviewInfo>::uninit();
                unsafe { capy_filter_previews_read(atlas, info.as_mut_ptr()) };
                assert_eq!(unsafe { info.assume_init() }.request, 2);
                unsafe { capy_filter_previews_free(atlas) };
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(app.pixels(), pixels, "Preview retry cannot change artwork");
        assert_eq!(unsafe { &*app.0 }.host.session.filter_preview_revision(), revision);
        app.action(json!({"type":"color", "action":{"op":"definition",
            "color":{"space":"Srgb", "rgba":[1.0,0.0,0.0,1.0]}}}));
        app.stroke();
        app.draw_frame();
        assert_ne!(app.pixels(), pixels, "Drawing must still publish new ink");
        app.invoke("undo");
        app.draw_frame();
        assert_eq!(app.pixels(), pixels, "Drawing and Undo still work after cancellation");
    }
}

#[test]
fn filter_preview_abi_keeps_owned_pixels_after_editor_teardown_without_document_edits() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let before = app.pixels();
        let revision = unsafe { &*app.0 }.host.session.filter_preview_revision();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let atlas = loop {
            app.request(2, json!({"type":"filter_previews",
                "filters":["curves","gradient_map"],"size":[96,40]})).unwrap();
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
                (1, 96, 80, 384, 30720)
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
    fn create(&self, extent: [u32; 2]) -> i32 {
        let options = CString::new(serde_json::to_string(&layer_ui::NewDocumentOptions {
            extent, ..Default::default()
        }).unwrap()).unwrap();
        unsafe { capy_project_new(self.0, options.as_ptr()) }
    }
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
        let task = unsafe { capy_apple_project_task(app.0, u32::from(opening), std::ptr::null()) };
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
    use std::os::fd::AsRawFd;
    fn sendable<T: Send + Sync>() {}
    sendable::<CapyProjectTask>();
    for platform in [0, 1] {
        let mut file = fixtures::tempfile();
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
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
        let directory = layer_core::package::archive::Directory::read(&mut std::io::Cursor::new(&bytes),262144,64*1024*1024).unwrap();
        let member = directory.member("preview.png").expect("Normal save includes its captured preview");
        let preview = layer_core::package::preview::Preview::decode(directory.read_member(&mut std::io::Cursor::new(&bytes),member,8*1024*1024).unwrap().into()).unwrap();
        assert!(preview.size().iter().all(|side| *side<=1024));
        let saved = read_document(std::io::Cursor::new(bytes));
        assert_saved_document(&saved, &original);
        let stale = ProjectJob::new(&app, true);
        file.rewind().unwrap();
        let pointer = stale.0 as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                capy_project_read(pointer as *const CapyProjectTask, fd, c"Drawing.capy".as_ptr())
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
                capy_project_read(pointer as *const CapyProjectTask, fd, c"Drawing.capy".as_ptr())
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
        assert_eq!(unsafe { capy_project_read(new.0, -1, c"Drawing.capy".as_ptr()) }, 0);
        let blank = CString::new("Untitled").unwrap();
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, new.0, blank.as_ptr(), c"".as_ptr()) },
            0
        );
        app.draw_frame();
        assert!(app.pixels() != original_pixels);
        assert_eq!(app.state()["document_file"]["modified"], false);
        drop(file);
    }
}

#[test]
fn project_cancellation_and_invalid_input_preserve_live_artwork() {
    use std::io::{Seek, Write};
    use std::os::fd::AsRawFd;
    for platform in [0, 1] {
        let mut file = fixtures::tempfile();
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
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
        assert_eq!(unsafe { capy_project_read(open.0, file.as_raw_fd(), c"Drawing.capy".as_ptr()) }, -1);
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
        assert_eq!(unsafe { capy_project_read(cancelled.0, -1, c"Drawing.capy".as_ptr()) }, 0);
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
    }
}
#[test]
fn new_canvas_dimensions_and_worker_png_export_preserve_captured_pixels() {
    use std::{
        io::Seek,
        os::fd::AsRawFd,
    };
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        app.draw_frame();
        let invalid = ProjectJob::new(&app, true);
        assert_eq!(invalid.create([0, 47]), -1);
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().composition().size[0],
            2048
        );
        let new = ProjectJob::new(&app, true);
        assert_eq!(
            new.create([63, 47]),
            0,
            "{:?}",
            new.error()
        );
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, new.0, c"Untitled".as_ptr(), c"".as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().composition().size[0],
            63
        );
        assert_eq!(
            unsafe { &*app.0 }.host.session.engine().document().composition().size[1],
            47
        );
        app.action(json!({"type":"set_color","rgba":[0.8,0.2,0.5,0.6]}));
        // Prepared native canvases still reconcile the receiving window's
        // brush before accepting input. Wait for that owner frame first.
        app.draw_until_idle();
        app.stroke();
        app.draw_frame();
        let expected = app.pixels();
        assert!(expected.chunks_exact(4).any(|p| p != [255; 4]), "Export must include actual ink");
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
        assert!(app.pixels() != expected, "Changing opacity must change the rendered stroke");
        assert!(app.state()["document_file"]["modified"].as_bool().unwrap());
        assert!(app.state()["document_file"]["location"].is_null());
        drop(app);
        let mut file = fixtures::tempfile();
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
            Some(png::SrgbRenderingIntent::RelativeColorimetric)
        );
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!([info.width, info.height], [63, 47]);
        // Float32 output quantization and the display readback can differ by
        // one code value (one blue sample in this fixture). Export no longer
        // narrows through the display cache.
        assert_eq!(pixels.len(), expected.len());
        assert!(pixels.iter().zip(&expected).all(|(&a, &b)| a.abs_diff(b) <= 1));
        assert!(pixels.chunks_exact(4).zip(expected.chunks_exact(4)).all(|(a, b)| a[3] == b[3]));
        drop(reader);
        drop(file);
    }
}

#[test]
fn custom_catalog_load_preserves_document_filters() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"unsharp_mask"}}));
        app.draw_frame();
        let before = unsafe { &*app.0 }.host.session.engine().document().clone();
        let checkpoint = unsafe { &*app.0 }.host.session.engine().document().revision;
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/filters/tent-blur");
        let manifest = std::fs::read_to_string(directory.join("manifest.json")).unwrap();
        let host = &mut unsafe { &mut *app.0 }.host;
        let change = host.session.load_effect_package(
            &manifest,
            |name| std::fs::read_to_string(directory.join(name))
                .map(std::sync::Arc::from).map_err(|error| error.to_string()),
            layer_core::EffectInstallMode::Add,
        ).unwrap();
        host.dirty |= change.canvas_wake;
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
            unsafe { &*app.0 }.host.session.engine().document().revision,
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
            Some(native_renderer());
        app.draw_until_idle();
        app.stroke();
        app.draw_until_idle();
        let ink = app.pixels();
        let clear = item(&app, "clear_layer");
        assert_eq!(clear["enabled"], true);
        app.action(clear["action"].clone());
        app.draw_until_idle();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_until_idle();
        assert_eq!(app.pixels(), ink);
        app.action(item(&app, "select_all")["action"].clone());
        app.draw_until_idle();
        app.action(item(&app, "fill_selection")["action"].clone());
        app.draw_until_idle();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_until_idle();
        assert_eq!(app.pixels(), ink);
        app.action(item(&app, "deselect")["action"].clone());
        app.draw_until_idle();
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
        app.draw_until_idle();
        assert_ne!(app.pixels(), ink);
        app.action(item(&app, "undo")["action"].clone());
        app.draw_until_idle();
        assert_eq!(app.pixels(), ink);
    }
}

impl App {
    fn new(platform: u32) -> Self {
        layer_core::temp_files::set_directory(std::env::temp_dir()).unwrap();
        let app = Self(capy_apple_create(platform));
        assert!(!app.0.is_null());
        assert_eq!(unsafe { capy_apple_resize(app.0, 1200, 900, 1.) }, 0);
        app
    }
    fn try_request(&self, kind: u32, value: &Value) -> Result<Option<Value>, String> {
        let text = CString::new(value.to_string()).unwrap();
        let result = unsafe { capy_apple_request(self.0, kind, text.as_ptr()) };
        let reply = (!result.is_null()).then(|| {
            let reply = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
            unsafe { capy_apple_string_free(result) };
            reply
        });
        let error = unsafe { capy_apple_error(self.0) };
        if error.is_null() {
            Ok(reply)
        } else {
            Err(unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned())
        }
    }
    fn request(&self, kind: u32, value: Value) -> Option<Value> {
        self.try_request(kind, &value)
            .unwrap_or_else(|error| panic!("request {kind} {value}: {error}"))
    }
    fn full_snapshot(&self) -> Value {
        unsafe { (*self.0).host.invalidate_snapshot() };
        self.request(7, Value::Null).unwrap()
    }
    fn action(&self, action: Value) {
        self.request(0, action).unwrap();
    }
    fn invoke(&self, command: &str) {
        self.action(json!({"type": "invoke", "command": command}));
        if command == "scale_rotate" { self.draw_until_transform(); }
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
    fn draw_until_transform(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !self.state()["tool_settings"].as_array().unwrap().iter().any(|setting| setting["id"] == "transform_x") {
            self.draw_frame();
            assert!(std::time::Instant::now() < deadline, "Transform bounds did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn draw_until_idle(&self) {
        self.draw_until_prepared(false);
    }
    fn draw_until_prepared(&self, active_operation: bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            self.draw_frame();
            let session = &unsafe { &*self.0 }.host.session;
            let engine = session.engine();
            if unsafe { &*self.0 }.host.startup.brush_ready
                && !engine.backend().0.as_ref().unwrap().startup_needs_update(
                    engine.document(), engine.brush(), engine.transform_preview().is_some())
                && (active_operation || session.require_document_idle().is_ok())
                && !engine.has_pending_document_edits()
                && !layer_render::CanvasRenderer::has_pending_work(engine.backend())
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Canvas did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
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
        renderer.readback_srgb_rgba8().unwrap()
    }
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe { capy_apple_destroy(self.0) }
    }
}

#[test]
fn apple_raster_project_preserves_exact_pixels_in_a_fresh_gpu_session() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        app.draw_frame();
        let paper = app.pixels();
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
        app.place_rgba("Embedded source", 64, 64, &pixels);
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.action(
            json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::Pencil as u32}),
        );
        app.action(json!({"type":"set_color","rgba":[0.7,0.2,0.9,1]}));
        app.stroke();
        app.draw_until_idle();
        app.layer_action(json!({"op":"add_mask","id":id,"replace":false}));
        app.action(json!({"type":"set_color","rgba":[0,0,0,1]}));
        app.stroke();
        app.draw_until_idle();
        app.layer_action(json!({"op":"apply_mask","id":id}));
        app.draw_until_idle();
        app.invoke("scale_rotate");
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        app.invoke("apply_transform");
        app.draw_until_idle();
        app.layer_action(json!({"op":"add_mask","id":id,"replace":false}));
        app.layer_action(json!({"op":"select","id":id,"mask":true}));
        app.stroke();
        app.draw_until_idle();
        app.layer_action(json!({"op":"select","id":id,"mask":false}));
        let expected = app.pixels();
        assert!(expected != paper, "Fixture must contain visible artwork");
        let source = unsafe { &*app.0 };
        let engine = source.host.session.engine();
        let selected = layer_ui::occurrence_handle(id).unwrap();
        let occurrence_id = engine.document().artwork.occurrences.id(selected).unwrap();
        let mask_handle = engine.document().scene().mask(selected).unwrap().0.source;
        let mask_id = engine.document().artwork.coverage.id(mask_handle).unwrap();
        let original = engine.capture_artwork(0).unwrap();
        assert!(original.artwork.paint.iter().any(|(_, _, source)| source.original.is_some()),
            "The imported original stays retained alongside edited raster pixels");
        let coverage = raster_samples(&original.artwork.coverage.get(mask_handle).unwrap().raster).0;
        assert!(!coverage.is_empty(), "Fixture must contain retained mask pixels");
        assert!(coverage.values().all(|(descriptor, _)| *descriptor == layer_core::color::PixelDescriptor::COVERAGE8));
        let mut bytes = Vec::new();
        write_capture(&original, &mut bytes);
        let document = read_document(std::io::Cursor::new(bytes));
        assert_eq!(document.working, layer_core::authored::WorkingState::default());
        let captured = layer_core::Document::from_artwork((*original.artwork).clone()).unwrap();
        assert_project_document(&document, &captured);
        let restored_occurrence = document.artwork.occurrences.resolve(occurrence_id).unwrap();
        let restored_mask = document.artwork.coverage.resolve(mask_id).unwrap();
        assert_eq!(document.scene().mask(restored_occurrence).unwrap().0.source, restored_mask);
        assert_eq!(raster_samples(&document.artwork.coverage.get(restored_mask).unwrap().raster).0, coverage);
        let gpu = native_renderer();
        let restored = App::new(platform);
        let host = &mut unsafe { &mut *restored.0 }.host;
        host.session =
            layer_ui::UiSession::new(layer_host::Renderer(Some(gpu.into())), document, [1200, 900], source.host.session.state().platform)
                .unwrap();
        host.resize(1200, 900, 1.).unwrap();
        restored.action(json!({"type":"select_layer","id":layer_ui::occurrence_token(restored_occurrence)}));
        restored.draw_until_idle();
        assert!(
            restored.pixels() == expected,
            "Fresh GPU must restore every document pixel, platform {platform}"
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
        assert!(!first.full_snapshot()["preferences"].is_null());
        first.action(json!({"type": "close_settings"}));
        assert!(first.full_snapshot()["preferences"].is_null());
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
            Some(native_renderer());
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        app.action(json!({"type": "set_brush_size", "value": 32}));
        app.draw_until_idle();
        let initial = app.pixels();
        app.stroke();
        app.draw_until_idle();
        let painted = app.pixels();
        assert!(painted != initial, "Pen-up must leave real document pixels");
        app.invoke("undo");
        app.draw_until_idle();
        assert!(
            app.pixels() == initial,
            "Undo must restore every document byte"
        );
        app.invoke("redo");
        app.draw_until_idle();
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
        let reference = native_renderer();
        let staged = WgpuRasterizer::from_wgpu_native_staged(
            reference.adapter().clone(),
            reference.device().clone(),
            reference.queue().clone(),
            Default::default(),
        )
        .unwrap();
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(reference.into());
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        // Accepted engine work must survive the first paper-only submission.
        app.stroke();
        {
            let host = &mut unsafe { &mut *app.0 }.host;
            host.session.renderer_mut().0 = Some(staged.into());
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
fn image_import_changes_gpu_pixels_is_undoable_and_produces_a_thumbnail() {
    use std::time::{Duration, Instant};
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        app.draw_until_idle();
        let paper = app.pixels();
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0];
        app.place_rgba("Test image", 2, 2, &rgba);
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        assert_eq!(app.layer(id)["label"], "Test image");
        app.draw_until_idle();
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
        app.draw_until_idle();
        assert!(app.pixels() == paper);
        app.invoke("redo");
        app.draw_until_idle();
        assert!(app.pixels() == imported);
    }
}

#[test]
fn stateless_color_editors_preserve_tagged_precision_and_convert_previews() {
    let resolve = |request: Value| localized_stateless(capy_apple_color_ui, request.to_string());
    let mut colors = layer_ui::ColorState::default();
    colors.set_rgb_space(layer_core::color::RgbSpace::ProPhoto).unwrap();
    for space in layer_core::color::RgbSpace::ALL {
        let color = layer_core::color::RgbColor::new(space, [0.12, 0.8, 31234. / 65535., 213. / 65535.]).unwrap();
        let mut editor = resolve(json!({"type":"editor_open","colors":colors,"color":color}));
        for (row, forms) in layer_ui::COLOR_FORM_FAMILIES.iter().enumerate() {
            for form in *forms {
                editor = resolve(json!({"type":"editor","editor":editor["editor"],"action":{"op":"form","row":row,"form":form}}));
                assert_eq!(serde_json::from_value::<layer_core::color::RgbColor>(editor["view"]["value"].clone()).unwrap(), color);
            }
        }
        let invalid = resolve(json!({"type":"editor","editor":editor["editor"],"action":{"op":"value","row":0,"index":0,"text":"invalid"}}));
        assert!(invalid["error"].is_string());
        assert_eq!(invalid["editor"], editor["editor"]);
    }
    let preview = resolve(json!({"type":"preview","colors":[{"space":"DisplayP3","rgba":[1.,0.,0.,0.25]}]}));
    assert_eq!(preview[0]["space"], "Srgb");
    assert_eq!(preview[0]["rgba"], json!([1.,0.,0.,0.25]));
    assert_eq!(preview[0]["in_gamut"], false);
    let gradient = resolve(json!({"type":"gradient","document_space":"ProPhoto","gradient":{"interpolation":"Classic","stops":[
        {"position":0.,"color":{"space":"ProPhoto","rgba":[0.,0.,0.,1.]}},
        {"position":1.,"color":{"space":"ProPhoto","rgba":[1.,1.,1.,1.]}}
    ]}}));
    assert_eq!(gradient.as_array().unwrap().len(), 257);
    // Encoded ProPhoto midpoint -> linear (gamma 1.8) -> encoded sRGB.
    let expected = 1.055 * 0.5_f64.powf(1.8 / 2.4) - 0.055;
    for channel in 0..3 { assert!((gradient[128]["rgba"][channel].as_f64().unwrap() - expected).abs() < 0.0001); }
    assert!(resolve(json!({"type":"preview","colors":[[1.,0.,0.,1.]]}))["error"].is_string(), "Retired untagged requests must fail");
}

#[test]
fn stateless_numeric_input_uses_shared_policy_without_a_session() {
    let launch = CString::new(json!({"saved":"", "preferred_languages":["en"]}).to_string()).unwrap();
    let prepared = App(unsafe { capy_apple_launch(0, launch.as_ptr(), std::ptr::null_mut()) });
    assert!(!prepared.0.is_null());
    let control = serde_json::to_value(layer_ui::ui_catalog()).unwrap()["layer_opacity"].clone();
    let resolve = |operation| localized_stateless(capy_apple_numeric,
        json!({"control":control,"value":1.,"operation":operation}).to_string());
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
fn apple_ruler_toggles_flip_their_checkable_commands() {
    for platform in [0, 1] {
        let app = App::new(platform);
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
            Some(native_renderer());
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let painted = app.pixels();
        app.invoke("scale_rotate");
        assert_eq!(app.state()["tool_settings"].as_array().unwrap().len(), 6);
        for command in ["apply_transform", "cancel_transform"] {
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
        assert!(
            app.try_request(0, &json!({"type":"set_tool_setting","id":"transform_width","value":0}))
                .is_err(),
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
fn apple_color_hit_classifies_wheel_parts() {
    for space in [0, 1, 2] {
        assert_eq!(capy_apple_color_hit(95., 50., 100., space), 1);
        assert_eq!(capy_apple_color_hit(50., 50., 100., space), 2);
        assert_eq!(capy_apple_color_hit(0., 0., 100., space), 0);
        assert_eq!(capy_apple_color_hit(f32::NAN, 50., 100., space), 0);
    }
    assert_eq!(capy_apple_color_hit(50., 50., 0., 0), 0);
    assert_eq!(capy_apple_color_hit(50., 50., 100., 3), 0);
}

#[test]
fn apple_color_actions_change_real_paint_and_transparent_eraser_pixels() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
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

#[test]
fn apple_launch_resolves_saved_preference_before_native_session() {
    for platform in [0, 1] {
        let saved = json!({"language": {"Explicit": "en"}}).to_string();
        let source = CString::new(json!({"saved": saved, "preferred_languages": ["ja-JP", "ko-KR", "en-US"]}).to_string()).unwrap();
        let app = App(unsafe { capy_apple_launch(platform, source.as_ptr(), std::ptr::null_mut()) });
        assert!(!app.0.is_null());
        let localization = unsafe { (*app.0).host.session.localization().clone() };
        assert_eq!(localization.language(), layer_ui::UiLanguage::English);
        let bootstrap = app.request(2, json!({"type": "bootstrap"})).unwrap();
        assert_eq!(bootstrap["active_tag"], "en");
        assert_eq!(bootstrap["shipped_tags"], json!(layer_ui::localization::SHIPPED_LANGUAGES.iter().map(|language| language.tag()).collect::<Vec<_>>()));
        assert_eq!(bootstrap["preparing_canvas"], localization.text(layer_ui::MessageId::COMMON_PREPARING_CANVAS).as_ref());
        app.action(json!({"type": "restore_saved_settings", "saved": "{\"language\":{\"Explicit\":\"ja\"}}"}));
        assert!(std::sync::Arc::ptr_eq(&localization, unsafe { (*app.0).host.session.localization() }));
    }
}

#[test]
fn apple_launch_rejects_invalid_transport_without_a_session() {
    assert!(unsafe { capy_apple_launch(0, std::ptr::null(), std::ptr::null_mut()) }.is_null());
    let malformed = CString::new("{}").unwrap();
    assert!(unsafe { capy_apple_launch(0, malformed.as_ptr(), std::ptr::null_mut()) }.is_null());
    let valid = CString::new(r#"{"saved":"","preferred_languages":[]}"#).unwrap();
    assert!(unsafe { capy_apple_launch(9, valid.as_ptr(), std::ptr::null_mut()) }.is_null());
}

#[test]
fn apple_launch_prepared_context_survives_restore_and_invalid_platform_keeps_bootstrap_copy() {
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
    let app = App(apple_launch_localized(1, "", localization.clone()));
    assert!(!app.0.is_null());
    app.action(json!({"type": "restore_saved_settings", "saved": "{\"language\":{\"Explicit\":\"en\"}}"}));
    assert!(std::sync::Arc::ptr_eq(&localization, unsafe { (*app.0).host.session.localization() }));
    assert_eq!(app.request(2, json!({"type": "bootstrap"})).unwrap()["active_tag"], "ja");
    let source = CString::new(r#"{"saved":"","preferred_languages":["en"]}"#).unwrap();
    let mut bootstrap = std::ptr::null_mut();
    assert!(unsafe { capy_apple_launch(9, source.as_ptr(), &mut bootstrap) }.is_null());
    assert!(!bootstrap.is_null());
    let view: Value = serde_json::from_slice(unsafe { CStr::from_ptr(bootstrap) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(bootstrap) };
    assert_eq!(view["canvas_init_failed"], "Could not initialize canvas");
    assert_eq!(view["canvas_ready"], "Canvas ready");
}

#[test]
fn stateless_native_captions_keep_the_prepared_context_and_literal_titles() {
    let localization = fixture_localization().clone();
    let title = "{ $title } 🖌 日本語\u{2068}user\u{2069}";
    let value = localized_stateless(capy_apple_native_caption, json!({"type":"close_drawing", "title":title}).to_string());
    assert_eq!(value["text"], format!("Close {title}"));
    assert!(std::sync::Arc::ptr_eq(&localization, &layer_ui::Localizer::prepared(layer_ui::UiLanguage::English).unwrap()));
    let appearance = localized_stateless(capy_apple_document_appearance, serde_json::to_string(&layer_ui::NewDocumentOptions::default()).unwrap());
    assert!(appearance["summary"].as_str().is_some_and(|value| !value.is_empty()));
}

#[test]
fn apple_language_publication_is_atomic_deferred_and_window_local() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let other = App::new(platform);
        let begin = |language: &str| {
            app.action(json!({"type":"restore_saved_settings", "saved":json!({"language":{"Explicit":language}}).to_string()}));
            let task = unsafe { capy_apple_language_request(app.0, c"[\"en\"]".as_ptr()) };
            assert!(!task.is_null());
            task
        };
        let document = unsafe { (*app.0).host.session.engine().document().revision };
        let epoch = app.state()["document_file"]["epoch"].clone();
        let camera = app.state()["camera"].clone();
        let old = begin("ja");
        let latest = begin("ko");
        unsafe { capy_language_prepare(old) };
        assert!(!unsafe { capy_apple_language_prepared(app.0, old) });
        unsafe { capy_language_free(old); capy_language_prepare(latest) };
        assert!(unsafe { capy_apple_language_prepared(app.0, latest) });
        unsafe { capy_language_free(latest) };
        assert_eq!(unsafe { capy_apple_language_publish(app.0, true) }, 0);
        assert_eq!(app.request(2, json!({"type":"bootstrap"})).unwrap()["active_tag"], "en");
        unsafe { (*app.0).chrome_facts.held = true };
        assert_eq!(unsafe { capy_apple_language_publish(app.0, false) }, 0);
        unsafe { (*app.0).chrome_facts.held = false };
        assert_eq!(unsafe { capy_apple_language_publish(app.0, false) }, 1);
        let snapshot = app.request(7, Value::Null).unwrap();
        assert_eq!(snapshot["bootstrap"]["active_tag"], "ko");
        assert_eq!(snapshot["language_generation"], 1);
        assert!(snapshot["catalog"]["native_copy"].is_object());
        assert_eq!(snapshot["state"]["document_file"]["epoch"], epoch);
        assert_eq!(snapshot["state"]["camera"], camera);
        assert_eq!(unsafe { (*app.0).host.session.engine().document().revision }, document);
        assert_eq!(other.request(2, json!({"type":"bootstrap"})).unwrap()["active_tag"], "en");
        let title = "user 日本語 🖌";
        for language in [layer_ui::UiLanguage::English, layer_ui::UiLanguage::Korean] {
            let actual = localized_stateless(capy_apple_native_caption, json!({"language":language,"request":{"type":"close_drawing","title":title}}).to_string());
            let expected = layer_ui::NativeCaption::CloseDrawing { title: title.into() }.message(&layer_ui::Localizer::shared(language));
            assert_eq!(actual["text"], expected);
        }
    }
}

#[test]
fn apple_launch_uses_each_scene_settings_instead_of_first_process_language() {
    for (platform, language) in [(0, "ja"), (1, "ko"), (0, "en")] {
        let source = CString::new(json!({"saved":json!({"language":{"Explicit":language}}).to_string(),"preferred_languages":["en"]}).to_string()).unwrap();
        let app = App(unsafe { capy_apple_launch(platform, source.as_ptr(), std::ptr::null_mut()) });
        assert!(!app.0.is_null());
        assert_eq!(app.request(2, json!({"type":"bootstrap"})).unwrap()["active_tag"], language);
    }
}
