//! SDR workflow checks through native controls and the production render worker.
use super::*;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_sampling_controls() {
    let app = native_test_app("art.capycanvas.SdrSampling");
    let w = Workspace::with_project(&app, Some((new_drawing(128, 128).unwrap(), None)));
    w.window.present();
    let ready = |w: &Rc<Workspace>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            pump(20);
            if w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().backend().startup.complete
                    && !g.session.state().filter_load.pending
                    && !g.session.engine().has_pending_document_edits()
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "canvas startup");
        }
    };
    ready(&w);
    let image = layer_core::ProjectAsset {
        extent: [128, 128],
        format: layer_core::ProjectAssetFormat::Rgba8Srgb,
        bytes: (0..128 * 128)
            .flat_map(|i| {
                if (i / 128 + i % 128) % 2 == 0 {
                    [0, 0, 255, 0]
                } else {
                    [255, 0, 0, 255]
                }
            })
            .collect(),
    };
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_asset("Sampling", image)
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    ready(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickLayer,
        },
    });
    w.dispatch(UiAction::SetBrushOpacity { value: 0.37 });
    let revision = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .revision;
    for width in [1, 3, 5] {
        let button = w
            .tool_set
            .buttons
            .borrow()
            .iter()
            .find(|(item, _, _)| item.action == UiAction::SetColorSampleSize { width })
            .unwrap()
            .1
            .clone();
        button.emit_clicked();
        pump(20);
        assert!(
            state(&w)
                .tool_set
                .subtools
                .iter()
                .any(|item| item.selected && item.action == UiAction::SetColorSampleSize { width })
        );
        w.dispatch(UiAction::SetColor {
            rgba: [0., 0., 1., 1.],
        });
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Up)] {
            w.gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .pen(PenEvent {
                    device_id: 91,
                    sequence,
                    timestamp_ns: glib::monotonic_time() as u64 * 1000,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * 64.5 + m[2] * 64.5 + m[4],
                        y: m[1] * 64.5 + m[3] * 64.5 + m[5],
                    },
                    pressure: 1.,
                    tilt_radians: [0.; 2],
                    twist_radians: 0.,
                    distance: 0.,
                    phase,
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .unwrap();
        }
        w.wake();
        pump(200);
        let expected = if width == 1 {
            [0., 0., 1., 1.]
        } else {
            [1., 0., 0., 1.]
        };
        for (actual, expected) in state(&w).colors.rgba().into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000001);
        }
        assert_eq!(state(&w).brush.opacity, 0.37);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .revision,
            revision
        );
    }
    w.window.destroy();
    pump(100);
}

fn sdr_ready(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(20);
        if w.gpu.borrow().as_ref().is_some_and(|g| {
            g.session.engine().backend().startup.complete
                && !g.session.state().filter_load.pending
                && !g.session.engine().has_pending_document_edits()
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "SDR canvas startup: {}",
            w.status.text()
        );
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_document_modes() {
    use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
    use layer_render::CanvasRenderer;
    use std::sync::Arc;
    let app = native_test_app("art.capycanvas.SdrDocuments");
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let color = DocumentColor { space, depth };
            let mut project = new_drawing(513, 257).unwrap();
            project.document.color = color;
            let mut builder = SourceBuilder::new(
                [513, 257],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth,
                    profile: ColorProfile::Icc(
                        layer_color::profile_bytes(&ColorProfile::Builtin(space))
                            .unwrap()
                            .into(),
                    ),
                    profile_assumed: false,
                },
                16 * 1024 * 1024,
            )
            .unwrap();
            for y in 0..257 {
                let mut row = Vec::new();
                for x in 0..513 {
                    for value in [((x * 71 + y * 37) % 65536) as u16, 31001, 52999, 50000] {
                        match depth {
                            IntegerDepth::U8 => row.push((value / 257) as u8),
                            IntegerDepth::U16 => row.extend_from_slice(&value.to_le_bytes()),
                        }
                    }
                }
                builder.push_row(&row).unwrap();
            }
            let source = Arc::new(builder.finish().unwrap());
            project.document.layers[0].source = Some(source.clone());
            let w = Workspace::with_project(&app, Some((project, None)));
            w.window.present();
            sdr_ready(&w);
            assert_eq!(
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .backend()
                    .document_color(),
                color
            );
            let pixels = |w: &Rc<Workspace>, id| {
                glib::MainContext::default()
                    .block_on(read_canvas_pixels(w, id))
                    .unwrap()
                    .bytes
            };
            let original = pixels(&w, 9100);
            w.dispatch(UiAction::SetColor {
                rgba: [0., 0., 0., 1.],
            });
            w.dispatch(UiAction::SetBrushSize { value: 21. });
            sdr_ready(&w);
            native_pen_path(
                &w,
                &[[230., 120.], [250., 120.], [270., 120.], [285., 120.]],
            );
            sdr_ready(&w);
            let painted = pixels(&w, 9101);
            assert_ne!(
                painted, original,
                "{color:?} brush draws over retained photo"
            );
            let root = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .layers[0]
                .raster
                .clone();
            let backing = root.wait_data().unwrap();
            assert!(backing.tiles.len() >= 2, "stroke crosses page boundary");
            for (key, tile) in &backing.tiles {
                assert_eq!(
                    tile.wait_backing().unwrap().descriptor,
                    key.plane.descriptor(color)
                );
            }
            w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            sdr_ready(&w);
            assert_eq!(pixels(&w, 9102), original, "{color:?} undo");
            w.dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            });
            sdr_ready(&w);
            assert_eq!(pixels(&w, 9103), painted, "{color:?} redo");
            let project = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .capture_project_recovery()
                .unwrap();
            assert!(Arc::ptr_eq(
                project.document.layers[0].source.as_ref().unwrap(),
                &source
            ));
            let mut bytes = Vec::new();
            project.write(&mut bytes).unwrap();
            let reopened =
                layer_core::Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
            assert_eq!(reopened.document.color, color);
            assert_eq!(
                reopened.document.layers[0].source.as_ref().unwrap(),
                &source
            );
            let restored = reopened.document.layers[0].raster.wait_data().unwrap();
            for (key, tile) in &backing.tiles {
                assert_eq!(
                    tile.wait_backing().unwrap().decode().unwrap(),
                    restored.tiles[key]
                        .wait_backing()
                        .unwrap()
                        .decode()
                        .unwrap()
                );
            }
            w.window.destroy();
            pump(50);
            let w = Workspace::with_project(&app, Some((reopened, None)));
            w.window.present();
            sdr_ready(&w);
            assert_eq!(
                pixels(&w, 9104),
                painted,
                "{color:?} reopened native window"
            );
            w.window.destroy();
            pump(50);
        }
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_bounded_canvas_startup_and_paint() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
    use layer_render::{CanvasRenderer, ColorSampleArea, ColorSampleRequest, ColorSampleSource};
    let app = native_test_app("art.capycanvas.SdrBoundedCanvas");
    // Exceeds the dense Float32 display ceiling and starts zoomed out in a real
    // GTK window, including the host's initial paper presentation and warmup.
    let mut project = new_drawing(4097, 1025).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    sdr_ready(&w);
    w.dispatch(UiAction::SetColor {
        rgba: [0., 0., 0., 1.],
    });
    w.dispatch(UiAction::SetBrushSize { value: 80. });
    sdr_ready(&w);
    native_pen_path(
        &w,
        &[[1990., 512.], [2030., 512.], [2070., 512.], [2100., 512.]],
    );
    sdr_ready(&w);
    assert!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .renderer_mut()
            .request_color_sample(ColorSampleRequest {
                request_id: 9700,
                source: ColorSampleSource::Composite,
                position: [2048, 512],
                area: ColorSampleArea::Point,
            })
            .unwrap()
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let sample = loop {
        pump(10);
        let sample = w
            .gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .renderer_mut()
            .take_color_sample();
        if let Some(sample) = sample {
            break sample.unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "exact sample after bounded display"
        );
    };
    assert_eq!(sample.request_id, 9700);
    assert!(
        sample.rgba[..3].iter().all(|v| v.abs() < 0.0001),
        "{:?}",
        sample.rgba
    );
    assert_eq!(sample.rgba[3], 1.);
    w.window.destroy();
    pump(50);
}
