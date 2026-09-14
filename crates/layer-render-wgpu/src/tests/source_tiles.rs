use super::*;
use layer_core::color::{IntegerDepth, source::*};

#[test]
fn raw_sampling_combines_integer_paint_and_unmaterialized_sixteen_bit_source() {
    use layer_core::{color::PixelDescriptor, raster::*};
    use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};
    let values = |x, y| -> [u16; 4] {
        match (x < 256, y < 256) {
            (true, true) => [65535, 0, 0, 65535],
            (false, true) => [0, 40000, 0, 30000],
            (true, false) => [65535, 0, 65535, 0],
            (false, false) => [0, 0, 50000, 65535],
        }
    };
    let mut builder = SourceBuilder::new(
        [513, 257],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: Default::default(),
            profile_assumed: false,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..257 {
        let row: Vec<_> = (0..513)
            .flat_map(|x| values(x, y).into_iter().flat_map(u16::to_le_bytes))
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let weak = Arc::downgrade(&source);
    let mut layer = Layer::paint(LayerId(1), "source");
    layer.source = Some(source.clone());
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([768, 512], std::slice::from_ref(&layer))
        .unwrap();
    let sample = |r: &mut WgpuRasterizer, position, area| {
        assert!(
            r.request_color_sample(ColorSampleRequest {
                request_id: 1,
                source: ColorSampleSource::Layer(LayerId(1)),
                position,
                area,
            })
            .unwrap()
        );
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .unwrap();
        r.take_color_sample().unwrap().unwrap().rgba
    };
    assert_eq!(
        sample(&mut r, [20, 20], ColorSampleArea::Point),
        [1., 0., 0., 1.]
    );
    assert!(
        r.paint_layers[0].pages.is_empty(),
        "sampling must not rasterize a source"
    );
    let mut raster = RasterData::default();
    raster.tiles.insert(
        TileKey {
            plane: RasterPlane::Color,
            coordinate: [0, 0],
        },
        RasterTile::backed(
            TileBlob::encode(
                PixelDescriptor::SRGB8_PAINT,
                &[188, 0, 0, 128].repeat(256 * 256),
            )
            .unwrap(),
        ),
    );
    r.restore_raster(LayerId(1), &RasterData::default(), &raster)
        .unwrap();
    let revision = r.composite_revision;
    for position in [[255, 255], [512, 256], [767, 511]] {
        let actual = sample(&mut r, position, ColorSampleArea::Average5);
        let [x, y] = position;
        let mut sum = [0.; 4];
        let mut count = 0;
        for y in y.saturating_sub(2)..(y + 3).min(512) {
            for x in x.saturating_sub(2)..(x + 3).min(768) {
                count += 1;
                if x < 256 && y < 256 {
                    sum[0] += ((188f64 / 255. + 0.055) / 1.055).powf(2.4);
                    sum[3] += 128. / 255.;
                } else if x < 513 && y < 257 {
                    let value = values(x, y).map(|v| f64::from(v) / 65535.);
                    if value[3] > 0. {
                        for c in 0..3 {
                            let linear = if value[c] <= 0.04045 {
                                value[c] / 12.92
                            } else {
                                ((value[c] + 0.055) / 1.055).powf(2.4)
                            };
                            sum[c] += linear * value[3];
                        }
                        sum[3] += value[3];
                    }
                }
            }
        }
        let expected = if sum[3] == 0. {
            [0.; 4]
        } else {
            [
                sum[0] / sum[3],
                sum[1] / sum[3],
                sum[2] / sum[3],
                sum[3] / count as f64,
            ]
        };
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (f64::from(actual) - expected).abs() < 0.000001,
                "{position:?}: {actual} vs {expected}"
            );
        }
    }
    assert_eq!(r.paint_layers[0].pages.len(), 1);
    assert_eq!(r.composite_revision, revision);
    r.ensure_document([768, 512], &[Layer::paint(LayerId(1), "empty")])
        .unwrap();
    drop(source);
    drop(layer);
    assert_eq!(
        weak.strong_count(),
        0,
        "deleted source must not be pinned by query caches"
    );
}

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
    frame(&mut r, &[dab], std::slice::from_ref(&batch), &[]);
    assert_eq!(r.paint_layers[0].pages.len(), 1);
    let painted = image(&mut r);
    let at = (100 * extent[0] as usize + 100) * 4;
    assert_ne!(&painted[at..at + 4], &expected[at..at + 4]);
    // Cold copy-on-write initialization must not erase source outside the dab.
    for [x, y] in [[1usize, 1usize], [255, 255], [256, 256], [2048, 512]] {
        let at = (y * extent[0] as usize + x) * 4;
        assert_eq!(&painted[at..at + 4], &expected[at..at + 4]);
    }
    // A second disconnected edit must not make undo reload the source between
    // them. Separate frames also exercise accumulated uncommitted GPU damage.
    let second = test_dab([1900., 350.], [0., 1., 0., 1.], 0.5);
    let mut second_batch = batch.clone();
    second_batch.stroke_id = StrokeId(2);
    second_batch.damage = Rect {
        min: Point { x: 1880., y: 330. },
        max: Point { x: 1920., y: 370. },
    };
    frame(&mut r, &[second], &[second_batch], &[]);
    assert_eq!(r.paint_layers[0].pages.len(), 2);
    let work = r.metrics.composited_pixels;
    frame(&mut r, &[], &[], &[(LayerId(1), before)]);
    assert_eq!(
        r.metrics.composited_pixels - work,
        2 * u64::from(PAGE_SIZE * PAGE_SIZE)
    );
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
