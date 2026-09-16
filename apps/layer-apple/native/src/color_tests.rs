//! Actual native SDR preparation, backing, history and GPU replacement.
use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
use std::{io::Seek, os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

#[test]
fn native_new_options_preserve_space_depth_background_and_captured_defaults() {
    use layer_ui::{DocumentBackground, NewDocumentOptions};
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let defaults = NewDocumentOptions {
            extent: [67, 43],
            color: DocumentColor { space: RgbSpace::AdobeRgb, depth: IntegerDepth::U16 },
            background: DocumentBackground::Transparent,
        };
        app.action(json!({"type":"new_document_settings","settings":{"defaults":defaults,"presets":[]}}));
        let adopt = |job: &ProjectJob, options: NewDocumentOptions| {
            assert_eq!(unsafe {
                capy_apple_project_adopt(app.0, job.0, c"New drawing".as_ptr(), c"".as_ptr())
            }, 0, "{:?}", job.error());
            app.draw_until_idle();
            let document = unsafe { &*app.0 }.host.session.engine().document();
            assert_eq!(document.color, options.color);
            assert_eq!([document.width, document.height], options.extent);
            assert_eq!(document.layers[1].visible, options.background == DocumentBackground::White);
            assert!(document.layers.iter().all(|layer| raster_samples(&layer.raster).0.is_empty()));
            assert!(!app.state()["document_file"]["modified"].as_bool().unwrap());
            assert!(!app.pixels().is_empty());
        };
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                for background in [DocumentBackground::White, DocumentBackground::Transparent] {
                    let options = NewDocumentOptions {
                        extent: [63, 47], color: DocumentColor { space, depth }, background,
                    };
                    let job = ProjectJob::new(&app, true);
                    let text = CString::new(serde_json::to_string(&options).unwrap()).unwrap();
                    assert_eq!(unsafe { capy_project_new(job.0, text.as_ptr()) }, 0, "{:?}", job.error());
                    adopt(&job, options);
                    assert_eq!(app.state()["settings"]["new_document"]["defaults"], json!(defaults));
                }
            }
        }
        // A worker consumes the settings captured by its owner, not later edits.
        let captured = ProjectJob::new(&app, true);
        let changed = NewDocumentOptions { extent: [73, 51], ..defaults };
        app.action(json!({"type":"new_document_settings","settings":{"defaults":changed,"presets":[]}}));
        assert_eq!(unsafe { capy_project_read(captured.0, -1) }, 0, "{:?}", captured.error());
        adopt(&captured, defaults);
        assert_eq!(app.state()["settings"]["new_document"]["defaults"], json!(changed));
        let fresh = ProjectJob::new(&app, true);
        assert_eq!(unsafe { capy_project_read(fresh.0, -1) }, 0, "{:?}", fresh.error());
        adopt(&fresh, changed);

        app.stroke(); app.draw_until_idle();
        let before = unsafe { &*app.0 }.host.session.engine().document().clone();
        let pixels = app.pixels();
        for value in [json!({}), json!({"extent":[0,47],"color":defaults.color,"background":"White"}),
            json!({"extent":[8193,47],"color":defaults.color,"background":"White"}),
            json!({"extent":[63,47],"color":{"space":"Unknown","depth":"U16"},"background":"White"})] {
            let job = ProjectJob::new(&app, true);
            let text = CString::new(value.to_string()).unwrap();
            assert_eq!(unsafe { capy_project_new(job.0, text.as_ptr()) }, -1);
            assert!(job.error().is_some());
            assert_eq!(unsafe {
                capy_apple_project_adopt(app.0, job.0, c"Invalid".as_ptr(), c"".as_ptr())
            }, -1);
            assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &before);
            assert!(app.pixels() == pixels, "Rejected New must preserve the live drawing");
        }
    }
}

#[test]
fn native_p3_u8_and_prophoto_u16_survive_save_open_recovery_and_gpu_replacement() {
    for platform in [0, 1] {
        for (space, depth) in [
            (RgbSpace::DisplayP3, IntegerDepth::U8),
            (RgbSpace::ProPhoto, IntegerDepth::U16),
        ] {
            let color = DocumentColor { space, depth };
            let project = layer_ui::NewDocumentOptions {
                extent: [128, 96],
                color,
                ..Default::default()
            }
            .project()
            .unwrap();
            let path = std::env::temp_dir().join(format!(
                "capy-native-color-{}-{platform}-{space:?}.capy",
                std::process::id()
            ));
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            // The exclusively owned descriptor remains usable without leaving
            // a disposable drawing behind when an assertion fails.
            std::fs::remove_file(path).unwrap();
            project.write(&mut file).unwrap();
            file.rewind().unwrap();
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
            app.draw_until_idle();
            let open = ProjectJob::new(&app, true);
            assert_eq!(
                unsafe { capy_project_read(open.0, file.as_raw_fd()) },
                0,
                "{:?}",
                open.error()
            );
            assert_eq!(
                unsafe {
                    capy_apple_project_adopt(app.0, open.0, c"Color check".as_ptr(), c"".as_ptr())
                },
                0
            );
            app.draw_until_idle();
            assert_eq!(
                unsafe { &*app.0 }.host.session.engine().document().color,
                color
            );
            let blank = app.pixels();
            app.action(json!({"type":"color","action":{"op":"definition","color":{
                "space":space,"rgba":[0.12345678,0.45678912,0.34567891,0.654321]
            }}}));
            app.action(json!({"type":"set_brush_size","value":5}));
            app.draw_until_idle();
            app.stroke();
            app.draw_until_idle();
            let painted = app.pixels();
            assert!(
                painted != blank,
                "{platform}/{space:?}: actual stroke required"
            );
            let mut document = unsafe { &*app.0 }.host.session.engine().document().clone();
            let samples: Vec<_> = document
                .layers
                .iter()
                .flat_map(|layer| raster_samples(&layer.raster).0.into_values())
                .collect();
            assert!(!samples.is_empty());
            assert!(
                samples
                    .iter()
                    .all(|(descriptor, _)| *descriptor == color.paint_descriptor())
            );
            if depth == IntegerDepth::U16 {
                assert!(
                    samples
                        .iter()
                        .flat_map(|(_, bytes)| bytes.chunks_exact(2))
                        .any(|sample| u16::from_le_bytes([sample[0], sample[1]]) % 257 != 0),
                    "16-bit backing must retain more than 8-bit precision"
                );
            }
            app.invoke("undo");
            app.draw_until_idle();
            assert!(app.pixels() == blank);
            app.invoke("redo");
            app.draw_until_idle();
            // History advances the document revision while restoring every
            // retained sample and all other document metadata exactly.
            document.revision = unsafe { &*app.0 }.host.session.engine().document().revision;
            assert_project_document(
                unsafe { &*app.0 }.host.session.engine().document(),
                &document,
            );
            assert!(app.pixels() == painted);

            for recovered in [false, true] {
                let save = if recovered {
                    ProjectJob(unsafe { capy_apple_project_task(app.0, 2) })
                } else {
                    ProjectJob::new(&app, false)
                };
                assert!(!save.0.is_null());
                file.set_len(0).unwrap();
                file.rewind().unwrap();
                assert_eq!(
                    unsafe { capy_project_write(save.0, file.as_raw_fd()) },
                    0,
                    "{:?}",
                    save.error()
                );
                file.rewind().unwrap();
                let saved = layer_core::Project::read(&mut file, Default::default()).unwrap();
                assert_project_document(&saved.document, &document);
                let restored = App::new(platform);
                unsafe { &mut *restored.0 }.host.session.renderer_mut().0 = Some(native_renderer());
                restored.draw_until_idle();
                let open = ProjectJob::new(&restored, true);
                file.rewind().unwrap();
                assert_eq!(
                    unsafe { capy_project_read(open.0, file.as_raw_fd()) },
                    0,
                    "{:?}",
                    open.error()
                );
                let result = if recovered {
                    unsafe { capy_apple_project_recover(restored.0, open.0) }
                } else {
                    unsafe {
                        capy_apple_project_adopt(
                            restored.0,
                            open.0,
                            c"Saved.capy".as_ptr(),
                            c"file:///fixture.capy".as_ptr(),
                        )
                    }
                };
                assert_eq!(result, 0);
                restored.draw_until_idle();
                assert_project_document(
                    unsafe { &*restored.0 }.host.session.engine().document(),
                    &document,
                );
                assert!(restored.pixels() == painted);
                assert_eq!(restored.state()["document_file"]["modified"], recovered);
                assert_eq!(unsafe { capy_apple_suspend_renderer(restored.0) }, 0);
                let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(color).unwrap();
                let owner = unsafe { &mut *restored.0 };
                owner.metal.install_renderer(&mut owner.host, gpu).unwrap();
                restored.draw_until_idle();
                assert_project_document(
                    unsafe { &*restored.0 }.host.session.engine().document(),
                    &document,
                );
                assert!(
                    restored.pixels() == painted,
                    "{platform}/{space:?}: GPU replacement must preserve artwork"
                );
            }
        }
    }
}
