//! Actual native SDR preparation, backing, history and GPU replacement.
use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
use std::{io::Seek, os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

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
