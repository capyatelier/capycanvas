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
fn current(w: &Rc<Workspace>, id: layer_core::OccurrenceHandle) -> layer_core::PaintSource {
    ui_session(w).engine().document().scene().paint_source(id).unwrap().clone()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_rasterization_keeps_off_canvas_source_paint_mask_and_reopen() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.SourceRasterize");
    let mut project = new_drawing(256, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let id = project.working.occurrence.unwrap();
    let source = std::sync::Arc::new(source());
    let layer_core::SourceTarget::Paint(handle) = project.working.target.unwrap() else { panic!("Paint source") };
    let paint = project.artwork.paint.get_mut(handle).unwrap();
    paint.domain = source.extent;
    paint.base = Some(layer_core::PaintBase::new((source.clone()).into()));
    let mask = project.allocate_coverage_handle();
    let coverage = layer_core::CoverageSnapshot::reveal_all(mask, source.extent, layer_core::Point { x: 11., y: -5. });
    project.artwork.coverage.install(mask, coverage.source).unwrap();
    let mut occurrence = project.scene().occurrence(id).unwrap().clone();
    occurrence.mask = Some(coverage.use_);
    project.apply(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&project.artwork.occurrences, id, Some(occurrence)).unwrap())).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    apply_fixture_theme(&w);
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
    let offset = ui_session(&w).engine().document().target_geometry(layer_core::SourceTarget::Paint(handle)).map(layer_core::Point::default()).unwrap();
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
    let overlay = ui_session(&w)
        .engine()
        .document().working.occurrence.unwrap();
    let paint = current(&w, overlay);
    assert!(!paint.raster.wait_data().unwrap().tiles.is_empty());
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: layer_ui::occurrence_token(id),
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
        action: LayerAction::RasterizeSource { id: layer_ui::occurrence_token(id) },
    });
    apply_dialog(&w, "rasterize-source-dialog", false);
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), before);
    invoke(&w, CommandId::RasterizeSource);
    let d = apply_dialog(&w, "rasterize-source-dialog", true);
    let output = std::path::Path::new("../../artifacts/color-m2/rasterize-ui")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&output).unwrap();
    pump(200);
    capture_ui(&w, &output, "source-before-after.png");
    assert!(
        named::<gtk::Label>(d.upcast_ref(), "rasterize-reduction")
            .label()
            .contains("clipped")
    );
    response(&w, "apply");
    finish(&w);
    ready(&w);
    let expected =
        layer_color::rasterize_source(&source, Default::default(), 8 * 1024 * 1024, || false)
            .unwrap()
            .0;
    let saved = snapshot(&w);
    let project =
        open_native_document(std::io::Cursor::new(saved.clone()));
    let restored = Workspace::with_project(&app, Some((project, None)));
    restored.window.present();
    ready(&restored);
    apply_fixture_theme(&restored);
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
    // Restore the source's placement and continue painting on its materialized
    // image. The source-layer paint bytes survived conversion/reopen unchanged.
    w.dispatch(UiAction::Layer {
        action: LayerAction::Select {
            id: layer_ui::occurrence_token(id),
            mask: false,
        },
    });
    invoke(&w, CommandId::Move);
    ready(&w);
    native_pen_path(&w, &[[100., 80.], [360., 140.5], [360., 140.5]]);
    ready(&w);
    let restored_offset = ui_session(&w).engine().document().target_geometry(layer_core::SourceTarget::Paint(handle)).map(layer_core::Point::default()).unwrap();
    assert!(
        restored_offset.x.abs() < 0.01 && restored_offset.y.abs() < 0.01,
        "{restored_offset:?}"
    );
    invoke(&w, CommandId::Pen);
    ready(&w);
    native_pen_path(&w, &[[35., 70.], [65., 70.], [100., 70.]]);
    ready(&w);
    assert_ne!(current(&w, id).raster, original.raster);
    assert_source_samples(current(&w, id).base.as_ref().unwrap().image.storage(), &expected);
    w.window.destroy();
    pump(100);
}
