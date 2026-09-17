use super::new_photo::{capture_ui, combo, finish, invoke, ready, response};
use super::place_source::{snapshot, source};
use super::*;
use layer_color::DocumentColorChange;
use layer_core::{Document, Project, color::*};

fn document(w: &Rc<Workspace>) -> Document {
    w.gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .clone()
}
fn dialog(w: &Rc<Workspace>, completed: bool) -> adw::AlertDialog {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        pump(if completed { 20 } else { 1 });
        if let Some(d) = w
            .window
            .visible_dialog()
            .filter(|d| d.widget_name() == "document-color-dialog")
        {
            let d = d.downcast::<adw::AlertDialog>().unwrap();
            if !completed || d.is_response_enabled("apply") {
                return d;
            }
            assert!(
                Instant::now() < deadline,
                "{}",
                find_named(d.upcast_ref(), "color-preview-status")
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap()
                    .label()
            );
        }
        assert!(
            Instant::now() < deadline,
            "color dialog: {}",
            w.status.text()
        );
    }
}
fn exact_document(mut actual: Document, expected: &Document) {
    actual.revision = expected.revision;
    assert_eq!(actual, *expected);
}
fn same_backing(actual: &Document, expected: &Document) {
    assert_eq!(actual.color, expected.color);
    for (a, b) in actual.layers.iter().zip(&expected.layers) {
        assert_eq!(a.properties, b.properties);
        assert_eq!(a.effect, b.effect);
        assert_eq!(a.source, b.source);
        for (a, b) in std::iter::once((&a.raster, &b.raster)).chain(
            a.mask
                .iter()
                .zip(&b.mask)
                .map(|(a, b)| (&a.raster, &b.raster)),
        ) {
            let a = a.wait_data().unwrap();
            let b = b.wait_data().unwrap();
            assert_eq!(a.watercolor, b.watercolor);
            assert!(a.tiles.keys().eq(b.tiles.keys()));
            for (key, a) in &a.tiles {
                let a = a.wait_backing().unwrap();
                let b = b.tiles[key].wait_backing().unwrap();
                assert_eq!(a.descriptor, b.descriptor);
                assert_eq!(a.digest, b.digest, "{key:?}");
            }
        }
    }
}
fn assert_mode(w: &Rc<Workspace>, color: DocumentColor) {
    use layer_render::CanvasRenderer;
    let gpu = w.gpu.borrow();
    let engine = gpu.as_ref().unwrap().session.engine();
    assert_eq!(engine.document().color, color);
    assert_eq!(engine.backend().document_color(), color);
    assert_eq!(state(w).colors.rgb_space(), color.space);
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
}

fn assert_visible_choice(w: &Rc<Workspace>, name: &str, expected: &str) {
    fn visible_label(widget: &gtk::Widget, expected: &str) -> bool {
        if !widget.is_mapped() {
            return false;
        }
        if widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.text() == expected)
        {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if visible_label(&widget, expected) {
                return true;
            }
            child = widget.next_sibling();
        }
        false
    }
    assert!(
        visible_label(combo(w, name).upcast_ref(), expected),
        "{name}: {expected} must be visible"
    );
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_document_color_assignment_conversion_depth_history_and_copy() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.DocumentColor");
    let mut project = new_drawing(256, 128).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let paint = project.document.active_layer;
    let original = std::sync::Arc::new(source());
    let mask = layer_core::LayerMask::reveal_all(
        project.document.allocate_layer_id(),
        Point { x: 0., y: 0. },
    );
    project.document.layers[0].source = Some(original.clone());
    project.document.layers[0].mask = Some(mask);
    let w = Workspace::with_project(&app, Some((project, None)));
    let created = Rc::new(RefCell::new(None));
    *w.open_document.borrow_mut() = Some({
        let created = created.clone();
        Rc::new(move |project, location, recovered| {
            assert!(location.is_none());
            assert!(recovered.is_none());
            created.replace(Some(project));
        })
    });
    w.window.present();
    ready(&w);
    invoke(&w, CommandId::Pen);
    w.dispatch(UiAction::SetBrushSize { value: 19. });
    w.dispatch(UiAction::SetColor {
        rgba: [0.2, 0.7, 0.1, 0.65],
    });
    ready(&w);
    native_pen_path(&w, &[[30., 80.], [90., 80.], [180., 80.]]);
    ready(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: paint.0,
            mask: true,
        },
    });
    ready(&w);
    native_pen_path(&w, &[[60., 70.], [120., 70.], [160., 70.]]);
    ready(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: paint.0,
            mask: false,
        },
    });
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "exposure".into(),
        },
    });
    ready(&w);
    let effect = document(&w).active_layer;
    w.dispatch(UiAction::Effect {
        action: EffectAction::Set {
            layer: effect.0,
            key: "exposure".into(),
            value: layer_core::EffectValue::Number(-0.25),
        },
    });
    ready(&w);
    let before = document(&w);
    let saved = snapshot(&w);
    let output = std::path::Path::new("../../artifacts/color-m2/document-color-ui")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    // Rapid choices then Cancel must retire the worker before releasing busy.
    invoke(&w, CommandId::AssignProfile);
    dialog(&w, false);
    for selected in [2, 0, 3, 2] {
        combo(&w, "document-color-space").set_selected(selected);
    }
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), saved);
    for (command, selected, change) in [
        (
            CommandId::AssignProfile,
            2,
            DocumentColorChange::Assign(RgbSpace::AdobeRgb),
        ),
        (
            CommandId::ConvertColorSpace,
            3,
            DocumentColorChange::Convert {
                space: RgbSpace::ProPhoto,
                options: Default::default(),
            },
        ),
        (
            CommandId::ChangeBitDepth,
            0,
            DocumentColorChange::Depth {
                depth: SampleDepth::U8,
                dither: OutputDither::None,
            },
        ),
    ] {
        let source = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .capture_project_recovery()
            .unwrap();
        let expected =
            layer_color::prepare_document_color(&source, change, 16 * 1024 * 1024, || false)
                .unwrap();
        let checkpoint = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .checkpoint();
        invoke(&w, command);
        dialog(&w, false);
        combo(
            &w,
            if command == CommandId::ChangeBitDepth {
                "document-color-depth"
            } else {
                "document-color-space"
            },
        )
        .set_selected(selected);
        dialog(&w, true);
        if command == CommandId::ConvertColorSpace {
            assert_visible_choice(&w, "document-color-result", "Convert editable layers");
            assert_visible_choice(&w, "document-color-intent", "Relative colorimetric");
        }
        capture_ui(&w, &output, &format!("{command:?}-comparison.png"));
        response(&w, "apply");
        finish(&w);
        ready(&w);
        assert_mode(&w, expected.project.document.color);
        let changed = document(&w);
        same_backing(&changed, &expected.project.document);
        assert!(std::sync::Arc::ptr_eq(
            changed.layer(paint).unwrap().source.as_ref().unwrap(),
            &original
        ));
        invoke(&w, CommandId::Undo);
        finish(&w);
        ready(&w);
        exact_document(document(&w), &source.document);
        assert_mode(&w, source.document.color);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .checkpoint(),
            checkpoint
        );
        invoke(&w, CommandId::Redo);
        finish(&w);
        ready(&w);
        exact_document(document(&w), &changed);
        assert_mode(&w, changed.color);
    }
    let changed = document(&w);
    assert_ne!(changed.color, before.color);
    let saved = snapshot(&w);
    let pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9960))
        .unwrap();
    let reopened = Workspace::with_project(
        &app,
        Some((
            Project::read(std::io::Cursor::new(saved.clone()), Default::default()).unwrap(),
            None,
        )),
    );
    reopened.window.present();
    ready(&reopened);
    assert_eq!(snapshot(&reopened), saved);
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&reopened, 9961))
            .unwrap()
            .bytes,
        pixels.bytes
    );
    reopened.window.destroy();
    w.window.present();
    ready(&w);
    // Cancel a prepared GPU candidate, including its in-flight command, and
    // wait for destruction before observing the unchanged live document.
    let project = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .capture_project_recovery()
        .unwrap();
    let candidate = layer_color::prepare_document_color(
        &project,
        DocumentColorChange::Assign(RgbSpace::Srgb),
        16 * 1024 * 1024,
        || false,
    )
    .unwrap();
    let acknowledgement = {
        let mut gpu = w.gpu.borrow_mut();
        let session = &mut gpu.as_mut().unwrap().session;
        let brush = session.engine().configured_brush().clone();
        let view = session.engine().view();
        session
            .renderer_mut()
            .prepare_color(candidate.project, brush, view, 0.)
            .unwrap();
        session.renderer_mut().discard_prepared_color().unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while matches!(
        acknowledgement.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ) {
        pump(5);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(snapshot(&w), saved);
    assert_mode(&w, changed.color);
    // A flattened conversion opens a separate editable document and leaves
    // the layered document, source, adjustments, mask and history unchanged.
    invoke(&w, CommandId::ConvertColorSpace);
    dialog(&w, false);
    combo(&w, "document-color-result").set_selected(1);
    combo(&w, "document-color-space").set_selected(0);
    dialog(&w, true);
    capture_ui(&w, &output, "flattened-copy-comparison.png");
    response(&w, "apply");
    finish(&w);
    assert_eq!(snapshot(&w), saved);
    let copy = created.borrow_mut().take().unwrap();
    assert_eq!(copy.document.layers.len(), 1);
    assert_eq!(copy.document.color.space, RgbSpace::Srgb);
    assert_eq!(copy.document.color.depth, changed.color.depth);
    let copy_window = Workspace::with_project(&app, Some((copy, None)));
    copy_window.window.present();
    ready(&copy_window);
    let copy_pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&copy_window, 9962))
        .unwrap();
    assert!(
        pixels
            .bytes
            .iter()
            .zip(&copy_pixels.bytes)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    assert!(state(&copy_window).document_file.modified);
    copy_window.window.destroy();
    w.window.present();
    ready(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: paint.0,
            mask: false,
        },
    });
    invoke(&w, CommandId::Pen);
    ready(&w);
    native_pen_path(&w, &[[30., 100.], [100., 100.], [200., 100.]]);
    ready(&w);
    assert_ne!(
        document(&w).layer(paint).unwrap().raster,
        changed.layer(paint).unwrap().raster
    );
    assert_mode(&w, changed.color);
    // A failed new stroke must leave the converted checkpoint recoverable,
    // including earlier color Undo/Redo after the native restart action.
    let surviving = document(&w);
    let surviving_pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9963))
        .unwrap();
    {
        let mut gpu = w.gpu.borrow_mut();
        let session = &mut gpu.as_mut().unwrap().session;
        session.renderer_mut().fail_next_frame();
        let camera = session.state().camera.clone();
        let m = camera.document_to_surface();
        let now = glib::monotonic_time() as u64 * 1000;
        for (i, (phase, x)) in [(PenPhase::Down, 20.), (PenPhase::Up, 220.)]
            .into_iter()
            .enumerate()
        {
            session
                .pen(PenEvent {
                    device_id: 94,
                    sequence: now + i as u64,
                    timestamp_ns: now + i as u64,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * x + m[2] * 110. + m[4],
                        y: m[1] * x + m[3] * 110. + m[5],
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
        session.frame(now + 2, now + 2).unwrap();
    }
    w.wake();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .rendering_suspended()
    {
        pump(20);
        assert!(Instant::now() < deadline, "color recovery suspension");
    }
    // Failed contacts consume their unique ID; recovery restores artwork and
    // history without reusing that identity for a later stroke.
    let mut recovered = surviving.clone();
    recovered.allocate_stroke_id();
    exact_document(document(&w), &recovered);
    assert!(w.restart_canvas.is_visible());
    click(&w.restart_canvas);
    ready(&w);
    assert_mode(&w, changed.color);
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&w, 9964))
            .unwrap()
            .bytes,
        surviving_pixels.bytes
    );
    invoke(&w, CommandId::Undo);
    finish(&w);
    ready(&w);
    invoke(&w, CommandId::Undo);
    finish(&w);
    ready(&w);
    assert_mode(
        &w,
        DocumentColor {
            depth: SampleDepth::U16,
            ..changed.color
        },
    );
    for _ in 0..2 {
        invoke(&w, CommandId::Redo);
        finish(&w);
        ready(&w);
    }
    exact_document(document(&w), &recovered);
    assert_mode(&w, changed.color);
    w.window.destroy();
    pump(100);
}
