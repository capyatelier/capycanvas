use super::new_photo::{capture_ui, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};

fn source() -> SourceImage {
    let mut builder = SourceBuilder::new(
        [513, 257],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Icc(
                layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3))
                    .unwrap()
                    .into(),
            ),
            profile_assumed: false,
        },
        8 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..257 {
        let row: Vec<u8> = (0..513)
            .flat_map(|x| {
                [
                    65535u16,
                    (x * 113 % 65536) as u16,
                    (y * 219 % 65536) as u16,
                    65535,
                ]
            })
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}
fn dialog(w: &Rc<Workspace>, completed: bool) -> adw::AlertDialog {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(if completed { 20 } else { 1 });
        if let Some(d) = w
            .window
            .visible_dialog()
            .filter(|d| d.widget_name() == "rasterize-source-dialog")
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
        assert!(Instant::now() < deadline, "rasterization dialog");
    }
}
fn current(w: &Rc<Workspace>, id: layer_core::LayerId) -> layer_core::Layer {
    w.gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .layer(id)
        .unwrap()
        .clone()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_rasterization_keeps_off_canvas_source_paint_mask_and_reopen() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.SourceRasterize");
    let mut project = new_drawing(256, 128).unwrap();
    let id = project.document.active_layer;
    let source = std::sync::Arc::new(source());
    let mask_id = project.document.allocate_layer_id();
    let layer = project
        .document
        .layers
        .iter_mut()
        .find(|l| l.id == id)
        .unwrap();
    layer.source = Some(source.clone());
    layer.mask = Some(layer_core::LayerMask::reveal_all(
        mask_id,
        layer_core::Point { x: 11., y: -5. },
    ));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    invoke(&w, CommandId::Pen);
    w.dispatch(UiAction::SetBrushSize { value: 17. });
    ready(&w);
    native_pen_path(&w, &[[30., 35.], [55., 35.], [95., 35.]]);
    ready(&w);
    assert!(!current(&w, id).raster.wait_data().unwrap().tiles.is_empty());
    invoke(&w, CommandId::Move);
    ready(&w);
    native_pen_path(&w, &[[100., 80.], [-160., 19.5], [-160., 19.5]]);
    ready(&w);
    let offset = current(&w, id).properties.offset;
    assert!(
        (offset.x + 260.).abs() < 0.01 && (offset.y + 60.5).abs() < 0.01,
        "{offset:?}"
    );
    invoke(&w, CommandId::Pen);
    // A separate paint overlay also survives the source-only conversion.
    w.dispatch(UiAction::Layer {
        action: LayerAction::New {
            group: false,
            clipped: false,
        },
    });
    ready(&w);
    native_pen_path(&w, &[[30., 40.], [55., 40.], [95., 40.]]);
    ready(&w);
    let overlay = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .active_layer;
    let paint = current(&w, overlay);
    assert!(!paint.raster.wait_data().unwrap().tiles.is_empty());
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: id.0,
            mask: false,
        },
    });
    ready(&w);
    let original = current(&w, id);
    // This pixel comes from the part of the image initially beyond the canvas.
    // A renderer that clips layer-local source coordinates to document extent
    // would show white paper here, even though Move brings the photo into view.
    let moved_pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9949))
        .unwrap();
    let moved_sample = &moved_pixels.bytes[(90 * 256 + 120) * 4..][..4];
    assert!(
        moved_sample[0] > 240 && moved_sample[1] < 240,
        "translated source content remains visible: {moved_sample:?}"
    );
    let before = snapshot(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::RasterizeSource { id: id.0 },
    });
    dialog(&w, false);
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), before);
    invoke(&w, CommandId::RasterizeSource);
    let d = dialog(&w, true);
    let output = std::path::Path::new("../../artifacts/color-m2/rasterize-ui")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    pump(200);
    capture_ui(&w, &output, "source-before-after.png");
    assert!(
        find_named(d.upcast_ref(), "rasterize-reduction")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .label()
            .contains("clipped")
    );
    response(&w, "apply");
    finish(&w);
    ready(&w);
    let rasterized = current(&w, id);
    let image = rasterized.source.as_ref().unwrap();
    assert_eq!(image.kind, SourceKind::Rasterized);
    assert_eq!(image.extent, source.extent);
    assert_eq!(image.interpretation.depth, SampleDepth::U8);
    assert_eq!(
        image.interpretation.profile,
        ColorProfile::Builtin(RgbSpace::Srgb)
    );
    assert_eq!(rasterized.properties, original.properties);
    assert_eq!(rasterized.raster, original.raster);
    assert_eq!(rasterized.mask, original.mask);
    assert_eq!(current(&w, overlay), paint);
    assert!(
        !state(&w)
            .commands
            .iter()
            .find(|c| c.id == CommandId::RepairSourceProfile)
            .unwrap()
            .enabled
    );
    assert!(
        !state(&w)
            .commands
            .iter()
            .find(|c| c.id == CommandId::RasterizeSource)
            .unwrap()
            .enabled
    );
    let expected =
        layer_color::rasterize_source(&source, Default::default(), 8 * 1024 * 1024, || false)
            .unwrap()
            .0;
    assert_eq!(image.as_ref(), &expected);
    let saved = snapshot(&w);
    assert_eq!(&saved[..12], b"CAPYRASTER\x04\0");
    let project =
        layer_core::Project::read(std::io::Cursor::new(saved.clone()), Default::default()).unwrap();
    let restored = Workspace::with_project(&app, Some((project, None)));
    restored.window.present();
    ready(&restored);
    assert_eq!(snapshot(&restored), saved);
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&restored, 9951))
            .unwrap()
            .bytes,
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&w, 9952))
            .unwrap()
            .bytes
    );
    restored.window.destroy();
    w.window.present();
    ready(&w);
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert_eq!(current(&w, id), original);
    assert_eq!(current(&w, overlay), paint);
    invoke(&w, CommandId::Redo);
    ready(&w);
    assert_eq!(current(&w, id), rasterized);
    assert_eq!(current(&w, overlay), paint);
    // Restore the source's placement and continue painting on its materialized
    // image. The source-layer paint bytes survived conversion/reopen unchanged.
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: id.0,
            mask: false,
        },
    });
    invoke(&w, CommandId::Move);
    ready(&w);
    native_pen_path(&w, &[[100., 80.], [360., 140.5], [360., 140.5]]);
    ready(&w);
    let restored_offset = current(&w, id).properties.offset;
    assert!(
        restored_offset.x.abs() < 0.01 && restored_offset.y.abs() < 0.01,
        "{restored_offset:?}"
    );
    invoke(&w, CommandId::Pen);
    ready(&w);
    native_pen_path(&w, &[[35., 70.], [65., 70.], [100., 70.]]);
    ready(&w);
    assert_ne!(current(&w, id).raster, original.raster);
    assert_eq!(current(&w, id).source.as_deref(), Some(&expected));
    w.window.destroy();
    pump(100);
}
