use super::*;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use std::{
    io::{Read, Seek},
    os::fd::AsRawFd,
};
fn new_drawing(app: &App, depth: SampleDepth) {
    app.invoke("new_document");
    let job = ProjectJob::new(app, true);
    let options = layer_ui::NewDocumentOptions {
        extent: [96, 64],
        color: DocumentColor {
            space: RgbSpace::DisplayP3,
            depth,
        },
        ..Default::default()
    };
    let text = CString::new(serde_json::to_string(&options).unwrap()).unwrap();
    assert_eq!(
        unsafe { capy_project_new(job.0, text.as_ptr()) },
        0,
        "{:?}",
        job.error()
    );
    assert_eq!(
        unsafe { capy_apple_project_adopt(app.0, job.0, c"Untitled".as_ptr(), c"".as_ptr()) },
        0,
        "{:?}",
        unsafe { &*app.0 }.error
    );
    app.draw_until_idle();
}
fn tabs(app: &App) -> Value {
    app.request(2, json!({"type":"document_tabs","op":"view","width":800}))
        .unwrap()
}
fn switch(app: &App, id: u64, closing: bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while unsafe { capy_apple_document_prepare_switch(app.0, 2_000_000_000) } == 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "Retained history capture did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let task = unsafe { capy_apple_document_switch(app.0, id, closing) };
    assert!(!task.is_null(), "{:?}", unsafe { &*app.0 }.error);
    assert_eq!(
        unsafe { capy_document_prepare(task, c"/tmp/capy-apple-tabs-test-spill".as_ptr()) },
        0
    );
    assert_eq!(
        unsafe { capy_apple_document_resume(app.0, task) },
        0,
        "{:?}",
        unsafe { &*app.0 }.error
    );
    unsafe { capy_document_free(task) };
    app.draw_until_idle();
}
fn tempfile() -> std::fs::File {
    let path = std::env::temp_dir().join(format!(
        "capy-tabs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    std::fs::remove_file(path).unwrap();
    file
}
#[test]
fn apple_tabs_preserve_history_pixels_recovery_and_disk_parking() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        app.stroke();
        app.draw_until_idle();
        let first = unsafe { &*app.0 }.host.session.engine().document().clone();
        new_drawing(&app, SampleDepth::F32);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["parked_renderers"], 0);
        app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[1.8,1.2,0.4,1.]}}}));
        app.invoke("select_all");
        app.layer_action(json!({"op":"fill_selection"}));
        app.invoke("deselect");
        app.draw_until_idle();
        let second = unsafe { &*app.0 }.host.session.engine().document().clone();
        unsafe { &mut *app.0 }.window.documents.budget.inactive_ram = 0;
        switch(&app, 1, false);
        assert_eq!(tabs(&app)["resident_bytes"], 0);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &first);
        app.invoke("undo");
        app.draw_until_idle();
        assert_ne!(
            raster_samples(
                &unsafe { &*app.0 }
                    .host
                    .session
                    .engine()
                    .document()
                    .layer(first.active_layer)
                    .unwrap()
                    .raster
            ),
            raster_samples(&first.layer(first.active_layer).unwrap().raster)
        );
        app.invoke("redo");
        app.draw_until_idle();
        let mut redone = first.clone();
        redone.revision = unsafe { &*app.0 }.host.session.engine().document().revision;
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &redone);
        // Recovery of an inactive drawing uses its own document and history.
        let job = ProjectJob(unsafe { capy_apple_document_recovery(app.0, 2) });
        assert!(!job.0.is_null());
        let mut file = tempfile();
        assert_eq!(
            unsafe { capy_project_write(job.0, file.as_raw_fd()) },
            0,
            "{:?}",
            job.error()
        );
        file.rewind().unwrap();
        let restored = layer_core::Project::read(&mut file, Default::default()).unwrap();
        assert_project_document(&restored.document, &second);
        app.request(
            2,
            json!({"type":"document_tabs","op":"reorder","id":2,"before":1}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 2);
        app.request(
            2,
            json!({"type":"document_tabs","op":"history","redo":false}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 1);
        app.request(
            2,
            json!({"type":"document_tabs","op":"history","redo":true}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 2);
        assert!(unsafe { capy_apple_document_switch(app.0, 99, false) }.is_null());
        assert_eq!(tabs(&app)["selected"], 1);
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        let request = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "confirm_close")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_close(app.0, request, 3) }, 0);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert!(
            !app.state()["document_file"]["close_ready"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        let request = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "confirm_close")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_close(app.0, request, 2) }, 0);
        switch(&app, 0, true);
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(tabs(&app)["can_undo"], false);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &second);
    }
}
#[test]
fn apple_float32_exr_and_gainmaps_use_actual_delivery_and_decoded_previews() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        new_drawing(&app, SampleDepth::F32);
        app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[1.8,1.2,0.4,0.75]}}}));
        app.invoke("select_all");
        app.layer_action(json!({"op":"fill_selection"}));
        app.invoke("deselect");
        app.draw_until_idle();
        let original = unsafe { &*app.0 }.host.session.engine().document().clone();
        app.invoke("export_document");
        let id = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "export")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        for format in [
            layer_ui::ExportFormat::Exr,
            layer_ui::ExportFormat::JpegHdr,
            layer_ui::ExportFormat::AvifHdr,
        ] {
            let mut pointer = std::ptr::null_mut();
            assert_eq!(
                unsafe { capy_apple_export_task(app.0, id, 2_000_000_000, &mut pointer) },
                1
            );
            let job = ProjectJob(pointer);
            let recipe = layer_ui::ExportRecipe::web_share()
                .draft_for_color(original.color, layer_ui::ExportDraftAction::Format(format))
                .recipe;
            let text = CString::new(serde_json::to_string(&recipe).unwrap()).unwrap();
            assert_eq!(
                unsafe { capy_project_export_options(job.0, text.as_ptr()) },
                0,
                "{:?}",
                job.error()
            );
            assert_eq!(
                unsafe { capy_project_compare(job.0) },
                0,
                "{:?}",
                job.error()
            );
            let mut preview = CapyProjectPreview {
                width: 0,
                height: 0,
                pixels: std::ptr::null(),
                count: 0,
            };
            assert_eq!(
                unsafe { capy_project_preview_at(job.0, 2, &mut preview) },
                if format.gainmap().is_some() { 0 } else { -1 }
            );
            let mut output = tempfile();
            assert_eq!(
                unsafe { capy_project_write(job.0, output.as_raw_fd()) },
                0,
                "{:?}",
                job.error()
            );
            output.rewind().unwrap();
            let mut magic = [0; 4];
            output.read_exact(&mut magic).unwrap();
            output.rewind().unwrap();
            if format == layer_ui::ExportFormat::Exr {
                assert_eq!(magic, [0x76, 0x2f, 0x31, 0x01]);
            }
            let photo =
                layer_color::photo::read_photo(std::io::BufReader::new(output), Default::default())
                    .unwrap();
            assert!(photo.interpretation.depth.is_float());
            if format == layer_ui::ExportFormat::Exr {
                assert_eq!(photo.interpretation.depth, SampleDepth::F32);
            }
            assert_project_document(
                unsafe { &*app.0 }.host.session.engine().document(),
                &original,
            );
        }
    }
}

#[test]
fn apple_tab_slides_follow_the_contact_and_cancel_off_the_strip() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        new_drawing(&app, SampleDepth::U8);
        new_drawing(&app, SampleDepth::U8);
        let ids: Vec<u64> = tabs(&app)["tabs"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), 3);
        let hits: Vec<Value> = ids.iter().enumerate()
            .map(|(i, id)| json!({"id":id,"bounds":{"x":i as f32 * 146.,"y":1.,"width":140.,"height":34.}})).collect();
        let slide = |point: [f32; 2]| app.request(2, json!({"type":"document_tabs","op":"slide","id":ids[0],"hits":hits,
            "clip":{"x":0.,"y":1.,"width":432.,"height":34.},"press":[70.,18.],"point":point})).unwrap();
        let past_one = slide([220., 18.]);
        assert_eq!(past_one["attached"], true);
        assert_eq!(past_one["bounds"]["x"].as_f64().unwrap(), 150.);
        assert_eq!(past_one["before"], json!(ids[2]));
        assert_eq!(past_one["offsets"], json!([0., -146., 0.]), "Neighbors slide aside by the source pitch");
        let to_end = slide([300., 18.]);
        assert_eq!(to_end["before"], Value::Null);
        assert_eq!(to_end["offsets"], json!([0., -146., -146.]));
        let away = slide([300., 90.]);
        assert_eq!(away["attached"], false);
        assert!(away["offsets"].as_array().unwrap().iter().all(|o| o.as_f64() == Some(0.)));
        assert_eq!(slide([70., 18.])["attached"], true);
    }
}
