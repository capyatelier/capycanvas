//! Complete Apple HDR owner/worker journey on real Metal, for both platform policies.
use super::*;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use std::{
    io::{Read, Seek},
    os::fd::AsRawFd,
    time::{Duration, Instant},
};
fn file() -> std::fs::File {
    let p = std::env::temp_dir().join(format!(
        "capy-apple-hdr-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&p)
        .unwrap();
    std::fs::remove_file(p).unwrap();
    f
}
fn proof(app: &App, action: Value) -> Value {
    app.request(2, json!({"type":"proof_panel","action":action}))
        .unwrap()
}
#[test]
fn apple_hdr_edit_proof_export_recovery_and_analysis_reuse() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let options = layer_ui::NewDocumentOptions {
            extent: [128, 96],
            color: DocumentColor {
                space: RgbSpace::DisplayP3,
                depth: SampleDepth::F16,
            },
            ..Default::default()
        };
        let job = ProjectJob::new(&app, true);
        let text = CString::new(serde_json::to_string(&options).unwrap()).unwrap();
        assert_eq!(
            unsafe { capy_project_new(job.0, text.as_ptr()) },
            0,
            "{:?}",
            job.error()
        );
        assert_eq!(
            unsafe { capy_apple_project_adopt(app.0, job.0, c"HDR".as_ptr(), c"".as_ptr()) },
            0
        );
        app.draw_until_idle();
        app.invoke("fit_canvas");
        app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[1.8,1.2,0.4,1.]}}}));
        app.invoke("select_all");
        app.layer_action(json!({"op":"fill_selection"}));
        app.invoke("deselect");
        app.draw_until_idle();
        let mut original = unsafe { &*app.0 }.host.session.engine().document().clone();
        let raster = raster_samples(&original.layer(original.active_layer).unwrap().raster);
        assert!(!raster.0.is_empty());
        assert!(
            raster
                .0
                .values()
                .all(|(d, _)| *d == options.color.paint_descriptor())
        );
        assert!(
            raster
                .0
                .values()
                .flat_map(|(_, b)| b.chunks_exact(8))
                .any(|b| {
                    let v = layer_core::color::hdr::decode_pixel(std::array::from_fn(|i| {
                        u16::from_le_bytes([b[i * 2], b[i * 2 + 1]])
                    }))
                    .unwrap();
                    v[..3].iter().any(|v| *v > 1.)
                }),
            "Above-white paint must survive publication"
        );
        app.request(2, json!({"type":"display_headroom","value":4.}));
        proof(&app, json!({"type":"mode","mode":"sdr"}));
        let initial = proof(&app, json!({"type":"reveal"}))["recipe"].clone();
        proof(
            &app,
            json!({"type":"edit","phase":"down","control":"exposure","value":0.}),
        );
        for i in 1..=20 {
            proof(
                &app,
                json!({"type":"edit","phase":"move","control":"exposure","value":i as f32/20.}),
            );
        }
        proof(
            &app,
            json!({"type":"edit","phase":"up","control":"exposure","value":1.}),
        );
        assert_eq!(
            raster_samples(
                &unsafe { &*app.0 }
                    .host
                    .session
                    .engine()
                    .document()
                    .layer(original.active_layer)
                    .unwrap()
                    .raster
            ),
            raster
        );
        app.invoke("undo");
        assert_eq!(
            app.request(2, json!({"type":"proof_panel"})).unwrap()["recipe"],
            initial
        );
        proof(
            &app,
            json!({"type":"edit","phase":"down","control":"exposure","value":0.}),
        );
        proof(
            &app,
            json!({"type":"edit","phase":"move","control":"exposure","value":-1.}),
        );
        proof(
            &app,
            json!({"type":"edit","phase":"cancel","control":"exposure","value":-1.}),
        );
        app.invoke("redo");
        assert_eq!(
            app.request(2, json!({"type":"proof_panel"})).unwrap()["recipe"]["exposure"],
            1.
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let a = unsafe { &mut *app.0 };
            a.metal.poll_color(&mut a.host).unwrap();
            if a.metal.local_tone.guide.is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "{:?}", a.metal.local_tone.error);
            std::thread::sleep(Duration::from_millis(10));
        }
        let count = unsafe { &*app.0 }.metal.local_tone.completed;
        let guide = unsafe { &*app.0 }.metal.local_tone.guide.clone().unwrap();
        let start = Instant::now();
        proof(
            &app,
            json!({"type":"edit","phase":"down","control":"exposure","value":1.}),
        );
        for i in 0..60 {
            proof(
                &app,
                json!({"type":"edit","phase":"move","control":"exposure","value":i as f32/60.}),
            );
            let a = unsafe { &mut *app.0 };
            a.metal.poll_color(&mut a.host).unwrap();
        }
        proof(
            &app,
            json!({"type":"edit","phase":"up","control":"exposure","value":1.}),
        );
        assert_eq!(unsafe { &*app.0 }.metal.local_tone.completed, count);
        assert!(std::sync::Arc::ptr_eq(
            &guide,
            unsafe { &*app.0 }.metal.local_tone.guide.as_ref().unwrap()
        ));
        eprintln!(
            "APPLE_HDR_CONTROL platform={platform} updates=60 elapsed_ms={:.3} guide_bytes={}",
            start.elapsed().as_secs_f64() * 1000.,
            guide.byte_len()
        );
        original = unsafe { &*app.0 }.host.session.engine().document().clone();
        // Exact archive/recovery bytes, including the saved rendition.
        let save = ProjectJob(unsafe { capy_apple_project_task(app.0, 2, std::ptr::null()) });
        assert!(!save.0.is_null());
        let mut archive = file();
        assert_eq!(
            unsafe { capy_project_write(save.0, archive.as_raw_fd()) },
            0,
            "{:?}",
            save.error()
        );
        archive.rewind().unwrap();
        let saved = layer_core::Project::read(&mut archive, Default::default()).unwrap();
        assert_project_document(&saved.document, &original);
        assert_eq!(saved.document.sdr_rendition, original.sdr_rendition);
        // Profiled SDR and portable HDR PNG use the actual immutable file worker.
        app.invoke("export_document");
        let id = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "export")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        for hdr in [false, true] {
            let mut pointer = std::ptr::null_mut();
            assert_eq!(
                unsafe { capy_apple_export_task(app.0, id, 2_000_000_000, &mut pointer) },
                1
            );
            let export = ProjectJob(pointer);
            let recipe = layer_ui::ExportRecipe::web_share()
                .draft(layer_ui::ExportDraftAction::Format(if hdr {
                    layer_ui::ExportFormat::PngHdr
                } else {
                    layer_ui::ExportFormat::Png
                }))
                .recipe;
            let text = CString::new(serde_json::to_string(&recipe).unwrap()).unwrap();
            assert_eq!(
                unsafe { capy_project_export_options(export.0, text.as_ptr()) },
                0,
                "{:?}",
                export.error()
            );
            assert_eq!(
                unsafe { capy_project_compare(export.0) },
                0,
                "{:?}",
                export.error()
            );
            let mut output = file();
            assert_eq!(
                unsafe { capy_project_write(export.0, output.as_raw_fd()) },
                0,
                "{:?}",
                export.error()
            );
            output.rewind().unwrap();
            let image =
                layer_color::photo::read_photo(std::io::BufReader::new(output), Default::default())
                    .unwrap();
            assert_eq!(
                image.interpretation.depth,
                if hdr {
                    SampleDepth::F16
                } else {
                    SampleDepth::U8
                }
            );
            assert_project_document(
                unsafe { &*app.0 }.host.session.engine().document(),
                &original,
            );
        }
        archive.rewind().unwrap();
        let mut bytes = Vec::new();
        archive.read_to_end(&mut bytes).unwrap();
        let restored = App::new(platform);
        unsafe { &mut *restored.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        restored.draw_until_idle();
        let open = ProjectJob::new(&restored, true);
        assert_eq!(
            unsafe {
                capy_project_read_bytes(open.0, bytes.as_ptr(), bytes.len(), c"HDR.capy".as_ptr())
            },
            0
        );
        assert_eq!(unsafe { capy_apple_project_recover(restored.0, open.0) }, 0);
        restored.draw_until_idle();
        assert_project_document(
            unsafe { &*restored.0 }.host.session.engine().document(),
            &original,
        );
        assert_eq!(unsafe { capy_apple_suspend_renderer(restored.0) }, 0);
        let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(options.color).unwrap();
        let a = unsafe { &mut *restored.0 };
        a.metal.install_renderer(&mut a.host, gpu).unwrap();
        restored.draw_until_idle();
        assert_project_document(
            unsafe { &*restored.0 }.host.session.engine().document(),
            &original,
        );
    }
}
