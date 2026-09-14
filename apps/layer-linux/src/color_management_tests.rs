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
