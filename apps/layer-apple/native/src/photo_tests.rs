use super::*;
use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace};
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};

impl App {
    pub(super) fn place_rgba(&self, name: &str, width: u32, height: u32, rgba: &[u8]) {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(rgba).unwrap();
        let job = place_job(self);
        read_bytes(&job, name, &bytes);
        adopt(self, &job, false);
    }
}
fn place_job(app: &App) -> ProjectJob {
    app.invoke("import_image");
    let job = unsafe { capy_apple_project_task(app.0, 3) };
    assert!(!job.is_null());
    ProjectJob(job)
}
fn read_bytes(job: &ProjectJob, name: &str, bytes: &[u8]) {
    let name = CString::new(name).unwrap();
    assert_eq!(unsafe { capy_project_read_bytes(job.0, bytes.as_ptr(), bytes.len(), name.as_ptr()) }, 0, "{:?}", job.error());
}
fn adopt(app: &App, job: &ProjectJob, opened: bool) {
    assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"Photo.tiff".as_ptr(),
        if opened { c"file:///source/Photo.tiff".as_ptr() } else { c"".as_ptr() }) }, 0, "{:?}", job.error());
    app.draw_until_idle();
}
fn source(space: RgbSpace, depth: IntegerDepth) -> SourceImage {
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgba, depth,
        profile: ColorProfile::Builtin(space), profile_assumed: false };
    let mut builder = SourceBuilder::new([13, 9], interpretation, usize::MAX).unwrap();
    for y in 0..9u16 {
        let mut row = Vec::new();
        for x in 0..13u16 {
            for sample in [1234 + x * 713, 54321 - y * 997, 31234, if x % 3 == 0 { 213 } else { 65535 }] {
                if depth == IntegerDepth::U16 { row.extend(sample.to_le_bytes()); }
                else { row.push((sample / 257) as u8); }
            }
        }
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}
fn source_samples(source: &SourceImage) -> Vec<Vec<u8>> {
    source.tiles.values().map(|t| t.decode().unwrap()).collect()
}

#[test]
fn photo_open_and_place_retain_source_depth_profile_samples_and_save_safety() {
    for platform in [0, 1] {
        for (space, depth) in [(RgbSpace::DisplayP3, IntegerDepth::U8), (RgbSpace::ProPhoto, IntegerDepth::U16)] {
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
            app.draw_until_idle();
            let original = source(space, depth);
            let mut file = std::io::Cursor::new(Vec::new());
            layer_color::photo::write_tiff(&mut file, &original).unwrap();
            let bytes = file.into_inner();
            // The reader may replace a built-in tag with the exact embedded ICC.
            let decoded = layer_color::photo::read_photo(std::io::Cursor::new(&bytes), Default::default()).unwrap();
            let job = ProjectJob::new(&app, true);
            read_bytes(&job, "Photo.tiff", &bytes);
            adopt(&app, &job, true);
            let document = unsafe { &*app.0 }.host.session.engine().document();
            assert_eq!(document.color.space, space);
            assert_eq!(document.color.depth, depth);
            assert_eq!([document.width, document.height], original.extent);
            assert!(app.state()["document_file"]["location"].is_null(), "Save must never overwrite an opened photograph");
            let retained = document.layers.iter().find_map(|l| l.source.as_ref()).unwrap();
            assert_eq!(**retained, decoded);
            assert_eq!(source_samples(retained), source_samples(&original));
            let before = app.pixels();
            app.stroke(); app.draw_until_idle();
            let mut painted = unsafe { &*app.0 }.host.session.engine().document().clone();
            let painted_pixels = app.pixels(); assert_ne!(painted_pixels, before);
            assert_eq!(painted.layers.iter().find_map(|l| l.source.as_ref()).unwrap().as_ref(), &decoded);
            app.invoke("undo"); app.draw_until_idle(); assert_eq!(app.pixels(), before);
            app.invoke("redo"); app.draw_until_idle();
            assert_eq!(app.pixels(), painted_pixels);
            painted.revision = unsafe { &*app.0 }.host.session.engine().document().revision;
            let saved = unsafe { &*app.0 }.host.session.capture_project_recovery().unwrap();
            let mut archive = Vec::new(); saved.write(&mut archive).unwrap();
            let reopened = layer_core::Project::read(archive.as_slice(), Default::default()).unwrap();
            assert_project_document(&reopened.document, &painted);

            let job = ProjectJob::new(&app, true);
            assert_eq!(job.create([67, 43]), 0); adopt(&app, &job, false);
            let target_color = unsafe { &*app.0 }.host.session.engine().document().color;
            let blank = app.pixels();
            let job = place_job(&app); read_bytes(&job, "Retained.tiff", &bytes); adopt(&app, &job, false);
            let document = unsafe { &*app.0 }.host.session.engine().document();
            assert_eq!(document.color, target_color, "Place must preserve the receiving document's space/depth");
            assert_eq!(source_samples(document.layers.iter().find_map(|l| l.source.as_ref()).unwrap()), source_samples(&original));
            let imported = app.pixels(); assert_ne!(blank, imported);
            app.invoke("undo"); app.draw_until_idle(); assert_eq!(app.pixels(), blank);
            app.invoke("redo"); app.draw_until_idle(); assert_eq!(app.pixels(), imported);
        }
    }
}

#[test]
fn photo_policy_prompt_retry_cancel_and_stale_publication_preserve_the_drawing() {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
    encoder.set_color(png::ColorType::Rgba); encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[32, 64, 128, 213, 0, 0, 0, 0]).unwrap();
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        app.action(json!({"type":"preferences","action":{"type":"edit","id":"missing_profile","value":1}}));
        app.action(json!({"type":"preferences","action":{"type":"edit","id":"photo_depth","value":1}}));
        let before = unsafe { &*app.0 }.host.session.engine().document().clone();
        let job = ProjectJob::new(&app, true); read_bytes(&job, "Untagged.png", &bytes);
        let profile = unsafe { capy_project_profile(job.0) };
        let prompt: Value = serde_json::from_slice(unsafe { CStr::from_ptr(profile) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(profile) };
        assert_eq!(prompt["profile_assumed"], true); assert_eq!(prompt["depth"], "U8");
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &before);
        assert_eq!(unsafe { capy_project_assume_profile(job.0, c"{\"Icc\":[]}".as_ptr()) }, -1);
        assert!(job.error().is_some());
        assert_eq!(unsafe { capy_project_assume_profile(job.0, c"{\"Builtin\":\"DisplayP3\"}".as_ptr()) }, 0, "{:?}", job.error());
        adopt(&app, &job, true);
        let document = unsafe { &*app.0 }.host.session.engine().document();
        assert_eq!(document.color.space, RgbSpace::DisplayP3);
        assert_eq!(document.color.depth, IntegerDepth::U16);
        let source = document.layers.iter().find_map(|l| l.source.as_ref()).unwrap();
        assert_eq!(source.interpretation.depth, IntegerDepth::U8, "Promotion affects future edits, not the retained original");
        assert!(!source.interpretation.profile_assumed);
        app.action(json!({"type":"preferences","action":{"type":"edit","id":"missing_profile","value":0}}));

        for condition in ["cancel", "edit", "target", "invalid", "gpu"] {
            let job = place_job(&app);
            if condition == "invalid" {
                assert_eq!(unsafe { capy_project_read_bytes(job.0, bytes.as_ptr(), 8, c"Broken.png".as_ptr()) }, -1);
            } else { read_bytes(&job, "Image.png", &bytes); }
            match condition {
                "cancel" => unsafe { capy_project_cancel(job.0) },
                "edit" => { app.action(json!({"type":"set_layer_opacity","opacity":0.5})); app.draw_until_idle(); },
                "target" => app.layer_action(json!({"op":"select","id":2,"mask":false})),
                "gpu" => {
                    let session = &mut unsafe { &mut *app.0 }.host.session;
                    let gpu = session.engine().backend().0.as_ref().unwrap();
                    // Headless constructors share a device cache. Request a real
                    // replacement device, with the receiving document's color.
                    let adapter = gpu.adapter().clone();
                    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                        required_features: gpu.device().features(),
                        required_limits: gpu.device().limits(),
                        ..Default::default()
                    })).unwrap();
                    session.renderer_mut().0 = Some(layer_render_wgpu::WgpuRasterizer::from_wgpu_native_staged(
                        adapter, device, queue, session.engine().document().color,
                    ).unwrap());
                },
                _ => (),
            }
            let expected = unsafe { &*app.0 }.host.session.engine().document().clone();
            assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"Image.png".as_ptr(), c"".as_ptr()) }, -1, "{condition}");
            assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &expected);
            let id = app.state()["requests"].as_array().unwrap().iter().find(|r| r["kind"]["type"] == "document").unwrap()["id"].as_u64().unwrap();
            assert_eq!(unsafe { capy_apple_document_complete(app.0, id as u32, 0) }, 0);
            app.layer_action(json!({"op":"select","id":1,"mask":false}));
        }
    }
}
