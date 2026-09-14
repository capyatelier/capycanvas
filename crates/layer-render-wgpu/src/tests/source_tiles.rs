use super::*;
use layer_core::color::{IntegerDepth, source::*};

#[test]
fn restored_source_tiles_update_the_displayed_blur_without_full_image_invalidation() {
    let extent = [1024, 768];
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0])
            .flat_map(|x| [x as u8, y as u8, (x ^ y) as u8, 255])
            .collect();
        builder.push_row(&row).unwrap();
    }
    let mut paint = Layer::paint(LayerId(1), "source");
    paint.source = Some(Arc::new(builder.finish().unwrap()));
    let original = paint.raster.clone();
    let mut blur = Layer::paint(LayerId(2), "blur");
    blur.kind = LayerKind::Effect;
    blur.effect = Some(Arc::new(fixture("gaussian_blur").preview().unwrap()));
    let mut layers = [blur, paint];
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let frame =
        |r: &mut WgpuRasterizer, layers: &[Layer], dabs: &[Dab], batches: &[DabBatch], all| {
            r.submit(FramePacket {
                view: ViewState {
                    width_px: extent[0],
                    height_px: extent[1],
                    background_rgba_linear: [0.; 4],
                    ..test_view()
                },
                document_extent: extent,
                layers,
                dabs,
                dab_batches: batches,
                restore_rasters: &[],
                reset_layers: false,
                time_seconds: 0.,
                composite_all: all,
            })
            .unwrap();
        };
    let display = |r: &WgpuRasterizer| {
        crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
    };
    frame(&mut r, &layers, &[], &[], true);
    let before = display(&r);
    layers[1].raster = layer_core::raster::RasterRevision::pending();
    let dab = test_dab([100., 100.], [1., 0., 0., 1.], 0.5);
    let batch = DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 80., y: 80. },
            max: Point { x: 120., y: 120. },
        },
    };
    frame(&mut r, &layers, &[dab], &[batch], false);
    layers[1].raster.wait_data().unwrap();
    assert!(display(&r) != before);
    let work = r.scene.as_ref().unwrap().image_pass_pixels();
    layers[1].raster = original;
    frame(&mut r, &layers, &[], &[], false);
    let restored = display(&r);
    assert_eq!(
        restored
            .iter()
            .zip(&before)
            .enumerate()
            .find(|(_, (a, b))| a != b),
        None
    );
    assert!(
        r.scene.as_ref().unwrap().image_pass_pixels() - work
            < u64::from(extent[0]) * u64::from(extent[1])
    );
    r.scene.as_mut().unwrap().force_image_rebuild();
    frame(&mut r, &layers, &[], &[], true);
    let rebuilt = display(&r);
    assert_eq!(
        restored
            .iter()
            .zip(&rebuilt)
            .enumerate()
            .find(|(_, (a, b))| a != b),
        None
    );
}

#[test]
fn tiled_sources_stream_through_fixed_slots_and_materialize_only_painted_pages() {
    let extent = [2049, 513]; // 27 tiles: exceeds the 16-slot/upload bound.
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgb,
        depth: IntegerDepth::U8,
        profile: Default::default(),
        profile_assumed: false,
    };
    let mut builder = SourceBuilder::new(extent, interpretation, 16 * 1024 * 1024).unwrap();
    let mut expected = Vec::new();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            let rgb = [(x * 617) as u8, (y * 251) as u8, (x ^ y) as u8];
            row.extend_from_slice(&rgb);
            expected.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let mut layer = Layer::paint(LayerId(1), "Source-backed paint");
    layer.source = Some(source.clone());
    let layers = [layer];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        background_rgba_linear: [0.; 4],
        ..test_view()
    };
    let mut r = WgpuRasterizer::new_headless().expect("physical GPU required");
    let frame = |r: &mut WgpuRasterizer,
                 dabs: &[Dab],
                 batches: &[DabBatch],
                 restore: &[(LayerId, layer_core::raster::RasterRevision)]| {
        r.submit(FramePacket {
            view,
            document_extent: extent,
            layers: &layers,
            dabs,
            dab_batches: batches,
            restore_rasters: restore,
            reset_layers: false,
            time_seconds: 0.,
            composite_all: false,
        })
        .unwrap();
    };
    let image = |r: &mut WgpuRasterizer| {
        crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
    };
    frame(&mut r, &[], &[], &[]);
    assert!(
        r.paint_layers[0].pages.is_empty(),
        "an unedited photograph owns no paint pages"
    );
    let actual = image(&mut r);
    assert_eq!(
        actual
            .iter()
            .zip(&expected)
            .enumerate()
            .find(|(_, (a, b))| a != b),
        None,
        "first differing byte; source submissions={}",
        r.metrics.source_upload_submissions
    );
    assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
    // The seventeenth tile drains capacity; the final eleven join the ordinary
    // frame submission without another source-only wait.
    assert_eq!(r.metrics.source_upload_submissions, 1);
    let before = layers[0].raster.clone();
    let dab = test_dab([100., 100.], [1., 0., 0., 1.], 0.5);
    let batch = DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 80., y: 80. },
            max: Point { x: 120., y: 120. },
        },
    };
    frame(&mut r, &[dab], &[batch], &[]);
    assert_eq!(r.paint_layers[0].pages.len(), 1);
    let painted = image(&mut r);
    let at = (100 * extent[0] as usize + 100) * 4;
    assert_ne!(&painted[at..at + 4], &expected[at..at + 4]);
    // Cold copy-on-write initialization must not erase source outside the dab.
    for [x, y] in [[1usize, 1usize], [255, 255], [256, 256], [2048, 512]] {
        let at = (y * extent[0] as usize + x) * 4;
        assert_eq!(&painted[at..at + 4], &expected[at..at + 4]);
    }
    frame(&mut r, &[], &[], &[(LayerId(1), before)]);
    assert!(r.paint_layers[0].pages.is_empty());
    let actual = image(&mut r);
    assert_eq!(
        actual
            .iter()
            .zip(&expected)
            .enumerate()
            .find(|(_, (a, b))| a != b),
        None,
        "undo restores source-backed composition without replay"
    );
    assert!(Arc::ptr_eq(layers[0].source.as_ref().unwrap(), &source));
}
