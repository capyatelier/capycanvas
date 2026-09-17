use super::*;
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace};
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
        self.invoke("apply_transform"); self.draw_until_idle();
    }
}
fn place_job(app: &App) -> ProjectJob {
    place_at(app, None)
}
fn place_at(app: &App, placement: Option<Value>) -> ProjectJob {
    app.invoke("import_image");
    let text = placement.map(|value| CString::new(value.to_string()).unwrap());
    let job = unsafe { capy_apple_project_task(app.0, 3, text.as_ref().map_or(std::ptr::null(), |t| t.as_ptr())) };
    assert!(!job.is_null());
    ProjectJob(job)
}

#[test]
fn photo_drop_captures_document_point_and_reuses_shared_row_validation() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    let original = source(RgbSpace::DisplayP3, SampleDepth::U8);
    layer_color::photo::write_tiff(&mut encoded, &original).unwrap();
    let bytes = encoded.into_inner();
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let new = ProjectJob::new(&app, true);
        assert_eq!(new.create([200, 150]), 0); adopt(&app, &new, false);
        app.invoke("zoom_in"); app.invoke("rotate_right"); app.invoke("flip_horizontal");
        let point = layer_core::Point { x: 615., y: 430. };
        let expected = unsafe { &*app.0 }.host.session.state().camera.input_transform().map(point);
        let before = unsafe { &*app.0 }.host.session.engine().document().layers.clone();
        let job = place_at(&app, Some(json!({"screen": point})));
        // Provider delivery is asynchronous; later navigation must not move
        // the captured document target or cause insertion at the canvas center.
        app.invoke("zoom_out"); app.invoke("rotate_left");
        read_bytes(&job, "Drop.tiff", &bytes); adopt(&app, &job, false);
        let doc = unsafe { &*app.0 }.host.session.engine().document();
        let placed = doc.layers.iter().find(|l| l.source.is_some()).unwrap();
        let center = doc.layer_transform(placed.id).map(layer_core::Point { x: 6.5, y: 4.5 });
        assert!((center.x - expected.x).abs() < 0.0001 && (center.y - expected.y).abs() < 0.0001);
        assert_eq!(source_samples(placed.source.as_ref().unwrap()), source_samples(&original));
        app.invoke("apply_transform"); app.draw_until_idle();
        let pixels = app.pixels();
        app.invoke("undo"); app.draw_until_idle();
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before);
        assert!(!unsafe { &*app.0 }.host.session.engine().can_undo());
        app.invoke("redo"); app.draw_until_idle(); assert_eq!(app.pixels(), pixels);
        app.invoke("undo"); app.draw_until_idle();

        app.layer_action(json!({"op":"group_selected"})); app.draw_until_idle();
        let group = unsafe { &*app.0 }.host.session.engine().document().layers.iter()
            .find(|l| l.kind == layer_core::LayerKind::Group).unwrap().id;
        let before = unsafe { &*app.0 }.host.session.engine().document().layers.clone();
        for (fraction, position, index) in [(0.1, "above", 0), (0.5, "into", 1), (0.9, "below", 2)] {
            assert_eq!(app.request(2, json!({"type":"image_layer_drop","target":group.0,"fraction":fraction})).unwrap()["position"], position);
            let job = place_at(&app, Some(json!({"layer":{"target":group.0,"fraction":fraction}})));
            read_bytes(&job, "Row drop.tiff", &bytes); adopt(&app, &job, false);
            let doc = unsafe { &*app.0 }.host.session.engine().document();
            assert!(doc.layers[index].source.is_some(), "{position} insertion order");
            assert_eq!(doc.layers[index].properties.parent, (position == "into").then_some(group));
            app.invoke("cancel_transform"); app.draw_until_idle();
            assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before);
        }
        app.layer_action(json!({"op":"lock","id":group.0,"value":true})); app.draw_until_idle();
        assert!(app.request(2, json!({"type":"image_layer_drop","target":group.0,"fraction":0.5})).unwrap()["position"].is_null());
        let before = unsafe { &*app.0 }.host.session.engine().document().clone();
        for placement in [json!({"layer":{"target":group.0,"fraction":0.5}}),
            json!({"layer":{"target":99999,"fraction":0.1}}),
            json!({"screen":point,"layer":{"target":group.0,"fraction":0.1}})] {
            app.invoke("import_image");
            let text = CString::new(placement.to_string()).unwrap();
            assert!(unsafe { capy_apple_project_task(app.0, 3, text.as_ptr()) }.is_null());
            assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &before);
            let id = app.state()["requests"].as_array().unwrap().iter().find(|r| r["kind"]["type"] == "document").unwrap()["id"].as_u64().unwrap();
            assert_eq!(unsafe { capy_apple_document_complete(app.0, id as u32, 0) }, 0);
        }
    }
}
fn read_bytes(job: &ProjectJob, name: &str, bytes: &[u8]) {
    let name = CString::new(name).unwrap();
    assert_eq!(unsafe { capy_project_read_bytes(job.0, bytes.as_ptr(), bytes.len(), name.as_ptr()) }, 0, "{:?}", job.error());
}
fn adopt(app: &App, job: &ProjectJob, opened: bool) {
    assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"Photo.tiff".as_ptr(),
        if opened { c"file:///source/Photo.tiff".as_ptr() } else { c"".as_ptr() }) }, 0, "{:?}", job.error());
    app.draw_until_prepared(true);
}
fn source(space: RgbSpace, depth: SampleDepth) -> SourceImage {
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgba, depth,
        profile: ColorProfile::Builtin(space), profile_assumed: false };
    let mut builder = SourceBuilder::new([13, 9], interpretation, usize::MAX).unwrap();
    for y in 0..9u16 {
        let mut row = Vec::new();
        for x in 0..13u16 {
            for sample in [1234 + x * 713, 54321 - y * 997, 31234, if x % 3 == 0 { 213 } else { 65535 }] {
                if depth == SampleDepth::U16 { row.extend(sample.to_le_bytes()); }
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

/// Run separately with CAPY_APPLE_PHOTO_JPEG pointing to a disposable
/// 9504×6336 JPEG. Both policies run on this machine's Metal backend; this
/// checks data integrity, not physical iPad input or presentation cadence.
#[test]
#[ignore = "61 MP Metal painting/save/device recovery; requires CAPY_APPLE_PHOTO_JPEG"]
fn large_jpeg_gpen_preserves_photo_through_save_and_gpu_recovery() {
    use std::io::Seek;
    use std::os::fd::AsRawFd;
    use layer_core::raster::RasterPlane;

    fn document(app: &App) -> layer_core::Document {
        unsafe { &*app.0 }.host.session.engine().document().clone()
    }
    fn check(app: &App, expected: &layer_core::Document) {
        let mut actual = document(app);
        actual.revision = expected.revision;
        assert_project_document(&actual, expected);
    }

    let path = std::env::var("CAPY_APPLE_PHOTO_JPEG").expect("Supply the 61 MP JPEG fixture");
    let bytes = std::fs::read(&path).unwrap();
    let decoded = layer_color::photo::read_photo(std::io::Cursor::new(&bytes), Default::default()).unwrap();
    assert_eq!(decoded.extent, [9504, 6336]);
    assert_eq!(decoded.interpretation.channels, SourceChannels::Rgb);
    assert_eq!(decoded.interpretation.depth, SampleDepth::U8);
    for platform in [0, 1] {
        let app = App::new(platform);
        let owner = unsafe { &mut *app.0 };
        owner.metal.install_renderer(&mut owner.host, native_renderer()).unwrap();
        app.draw_until_idle();
        {
            let open = ProjectJob::new(&app, true);
            read_bytes(&open, "Large-photo.jpg", &bytes);
            adopt(&app, &open, true);
        }
        app.invoke("fit_canvas");
        app.action(json!({"type":"select_brush","id":1}));
        app.action(json!({"type":"set_color","rgba":[1.,0.,0.7,1./3.]}));
        app.action(json!({"type":"set_brush_size","value":32}));
        app.draw_until_idle();
        let mut original = document(&app);
        assert_eq!(original.color.space, RgbSpace::Srgb);
        assert_eq!(original.color.depth, SampleDepth::U8);
        assert_eq!(original.layers.iter().find_map(|l| l.source.as_deref()), Some(&decoded));
        let before = app.pixels();
        app.stroke(); app.draw_until_idle();
        let painted = document(&app);
        // Undo restores artwork but never reuses an allocated contact ID.
        original.allocate_stroke_id();
        assert_eq!(original.next_stroke_id(), painted.next_stroke_id());
        let ink = app.pixels();
        assert_ne!(ink, before, "G-Pen must visibly paint the imported photograph");
        let layer = painted.layers.iter().find(|l| l.source.is_some()).unwrap();
        let (tiles, _) = raster_samples(&layer.raster);
        assert!(!tiles.is_empty(), "Painting must publish native backing");
        for (key, (descriptor, pixels)) in &tiles {
            assert_eq!(key.plane, RasterPlane::Color);
            assert_eq!(descriptor.bits_per_channel, 8);
            let coordinate = key.coordinate.map(|x| u32::try_from(x).unwrap());
            let source = decoded.tiles[&coordinate].decode().unwrap();
            let mut untouched = 0;
            for (paint, source) in pixels.chunks_exact(4).zip(source.chunks_exact(3)) {
                assert_eq!(paint[3], 255, "Painting must retain photo opacity in every touched tile");
                if paint[..3] == *source { untouched += 1; }
            }
            assert!(untouched > 256 * 256 / 2, "Each narrow-stroke tile must preserve its surrounding photo samples");
        }
        app.invoke("undo"); app.draw_until_idle(); check(&app, &original);
        assert_eq!(app.pixels(), before);
        app.invoke("redo"); app.draw_until_idle(); check(&app, &painted);
        assert_eq!(app.pixels(), ink);
        let file_path = std::env::temp_dir().join(format!("capy-61mp-{}-{platform}.capy", std::process::id()));
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&file_path).unwrap();
        {
            let save = ProjectJob::new(&app, false);
            assert_eq!(unsafe { capy_project_write(save.0, file.as_raw_fd()) }, 0, "{:?}", save.error());
            assert_eq!(unsafe { capy_project_begin_commit(save.0) }, 0);
            assert_eq!(unsafe { capy_apple_project_saved(app.0, save.0, c"Painted.capy".as_ptr(), c"file:///Painted.capy".as_ptr()) }, 0);
        }
        file.rewind().unwrap();
        {
            let open = ProjectJob::new(&app, true);
            assert_eq!(unsafe { capy_project_read(open.0, file.as_raw_fd(), c"Painted.capy".as_ptr()) }, 0, "{:?}", open.error());
            adopt(&app, &open, true);
        }
        check(&app, &painted); assert_eq!(app.pixels(), ink);
        assert_eq!(unsafe { capy_apple_test_gpu_fault(app.0, 0) }, 0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !unsafe { &*app.0 }.host.session.rendering_suspended() {
            assert_eq!(unsafe { capy_apple_frame(app.0, 3_000_000_000, 3_000_000_000, std::ptr::null_mut()) }, 0);
            assert!(std::time::Instant::now() < deadline, "GPU loss callback did not arrive");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        check(&app, &painted);
        let owner = unsafe { &mut *app.0 };
        owner.metal.install_renderer(&mut owner.host, native_renderer()).unwrap();
        app.draw_until_idle(); check(&app, &painted); assert_eq!(app.pixels(), ink);
        assert!(!unsafe { &*app.0 }.host.session.rendering_suspended());
        std::fs::remove_file(file_path).unwrap();
        println!("PASS platform {platform}: 61 MP JPEG, G-Pen, opaque touched tiles with original samples, exact history/save/reopen and GPU loss/replacement");
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes, "Original JPEG must remain unchanged");
}

#[test]
fn photo_open_and_place_retain_source_depth_profile_samples_and_save_safety() {
    for platform in [0, 1] {
        for (space, depth) in [(RgbSpace::DisplayP3, SampleDepth::U8), (RgbSpace::ProPhoto, SampleDepth::U16)] {
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
            app.invoke("apply_transform"); app.draw_until_idle();
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
        assert_eq!(document.color.depth, SampleDepth::U16);
        let source = document.layers.iter().find_map(|l| l.source.as_ref()).unwrap();
        assert_eq!(source.interpretation.depth, SampleDepth::U8, "Promotion affects future edits, not the retained original");
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

#[test]
fn photo_batch_placement_is_provisional_atomic_and_keeps_original_samples() {
    let images: Vec<_> = [(RgbSpace::DisplayP3, SampleDepth::U8), (RgbSpace::ProPhoto, SampleDepth::U16)]
        .into_iter().map(|(space, depth)| {
            let mut encoded = std::io::Cursor::new(Vec::new());
            layer_color::photo::write_tiff(&mut encoded, &source(space, depth)).unwrap();
            let bytes = encoded.into_inner();
            let decoded = layer_color::photo::read_photo(std::io::Cursor::new(&bytes), Default::default()).unwrap();
            (bytes, decoded)
        }).collect();
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let new = ProjectJob::new(&app, true);
        assert_eq!(new.create([7, 5]), 0); adopt(&app, &new, false);
        let before = unsafe { &*app.0 }.host.session.engine().document().layers.clone();
        for apply in [false, true] {
            let job = place_job(&app);
            for (index, (bytes, _)) in images.iter().enumerate() {
                read_bytes(&job, &format!("Photo-{}.tiff", index + 1), bytes);
                assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before,
                    "Nothing enters the live drawing during batch preparation");
            }
            adopt(&app, &job, false);
            let session = &unsafe { &*app.0 }.host.session;
            assert!(!session.engine().can_undo(), "Provisional placement has no artwork history");
            assert!(session.capture_project_recovery().is_err(), "Pending placement cannot enter recovery");
            let placed: Vec<_> = session.engine().document().layers.iter().filter(|l| l.source.is_some()).collect();
            assert_eq!(placed.len(), 2);
            for (index, layer) in placed.iter().enumerate() {
                assert_eq!(layer.name.as_ref(), format!("Photo-{}", index + 1));
                assert_eq!(layer.source.as_deref(), Some(&images[index].1));
                assert!((layer.properties.placement.0[0] - 7. / 13.).abs() < 0.00001);
                let center = layer.properties.placement.map(layer_core::Point { x: 6.5, y: 4.5 });
                assert!((center.x - 3.5).abs() < 0.00001 && (center.y - 2.5).abs() < 0.00001);
            }
            app.invoke("placement_original_size"); app.draw_until_prepared(true);
            for layer in unsafe { &*app.0 }.host.session.engine().document().layers.iter().filter(|l| l.source.is_some()) {
                assert_eq!(&layer.properties.placement.0[..4], &layer_core::Affine::IDENTITY.0[..4]);
            }
            if !apply {
                app.invoke("cancel_transform"); app.draw_until_idle();
                assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before);
                assert!(!unsafe { &*app.0 }.host.session.engine().can_undo());
            } else {
                app.invoke("apply_transform"); app.draw_until_idle();
                let committed = unsafe { &*app.0 }.host.session.engine().document().layers.clone();
                let pixels = app.pixels();
                app.invoke("undo"); app.draw_until_idle();
                assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before);
                assert!(!unsafe { &*app.0 }.host.session.engine().can_undo(), "The entire batch is one Undo");
                app.invoke("redo"); app.draw_until_idle();
                assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, committed);
                assert_eq!(app.pixels(), pixels);
                let mut archive = Vec::new();
                unsafe { &*app.0 }.host.session.capture_project_recovery().unwrap().write(&mut archive).unwrap();
                let opened = ProjectJob::new(&app, true); read_bytes(&opened, "Batch.capy", &archive); adopt(&app, &opened, false);
                assert_eq!(app.pixels(), pixels);
                assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, committed);
            }
        }
        let before = unsafe { &*app.0 }.host.session.engine().document().layers.clone();
        let job = place_job(&app); read_bytes(&job, "First.tiff", &images[0].0);
        assert_eq!(unsafe { capy_project_read_bytes(job.0, b"broken".as_ptr(), 6, c"Second.png".as_ptr()) }, -1);
        assert_eq!(unsafe { capy_project_read_bytes(job.0, images[1].0.as_ptr(), images[1].0.len(), c"Retry.tiff".as_ptr()) }, -1,
            "A failed batch cannot resume as a partial insertion");
        assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"".as_ptr(), c"".as_ptr()) }, -1);
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document().layers, before);
    }
}
