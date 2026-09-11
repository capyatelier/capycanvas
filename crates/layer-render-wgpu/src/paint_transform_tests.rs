use super::*;
use layer_core::{Affine, ImageTransform, Interpolation};

fn operation(id: u64, affine: Affine, selection: Option<Selection>) -> LayerOperation {
    let mut coverage = LayerMask::reveal_all(LayerId(id), Point::default());
    coverage.default_coverage = if selection.is_some() { 0. } else { 1. };
    coverage.initial = selection;
    LayerOperation {
        after_stroke: 0,
        coverage,
        kind: LayerOperationKind::Transform(ImageTransform {
            affine,
            interpolation: Interpolation::Nearest,
        }),
    }
}
fn op_batch(index: u32, operation: &LayerOperation) -> DabBatch {
    DabBatch {
        kind: DabBatchKind::LayerOperation(index),
        dab_count: 0,
        damage: operation.bounds([128; 2]),
        ..batch(1)
    }
}

#[test]
fn live_transform_uses_immutable_pixels_cancels_exactly_and_commits_without_jump() {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut reference = WgpuRasterizer::new_headless().unwrap();
    let extent = [640, 384];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    let frame =
        |r: &mut WgpuRasterizer, layers: &[Layer], dabs: &[Dab], batches: &[DabBatch], reset| {
            r.submit(FramePacket {
                view,
                document_extent: extent,
                layers,
                dabs,
                dab_batches: batches,
                time_seconds: 0.,
                reset_layers: reset,
                composite_all: false,
            })
            .unwrap();
        };
    let raw = |r: &WgpuRasterizer| {
        let l = &r.paint_layers[0];
        let mut channels = vec![
            l.pages
                .iter()
                .map(|p| (p.coordinate, page_bytes(r, &p.active().texture)))
                .collect::<Vec<_>>(),
        ];
        channels.push(
            l.material_pages
                .iter()
                .map(|p| (p.coordinate, page_bytes(r, &p.wetness.texture)))
                .collect(),
        );
        channels.push(
            l.watercolor_wetness_pages
                .iter()
                .map(|p| (p.coordinate, page_bytes(r, &p.active().texture)))
                .collect(),
        );
        channels
    };
    for preset in [GPen, WetRound, WatercolorWash] {
        let mut layer = Layer::paint(LayerId(1), "live transform");
        layer.properties.offset = Point { x: 5., y: 7. };
        let mut b = batch(1);
        b.style = preset_style(preset);
        b.damage = Rect {
            min: Point { x: 70., y: 70. },
            max: Point { x: 160., y: 160. },
        };
        let mut d = dab([0.8, 0.1, 0.6, 0.7]);
        d.center = Point { x: 115., y: 115. };
        d.radii = [40.; 2];
        d.material = [0.5, 0.8, 1., 0.8];
        let layers = std::slice::from_ref(&layer);
        frame(&mut r, layers, &[d], &[b.clone()], true);
        let original = r.readback_srgb_rgba8().unwrap();
        let original_raw = raw(&r);
        let selection = Selection::polygon(vec![
            Point { x: 115.25, y: 0. },
            Point { x: 300., y: 0. },
            Point { x: 300., y: 300. },
            Point { x: 115.25, y: 300. },
        ])
        .unwrap();
        let mut preview = layer_render::TransformPreview {
            transaction: 1,
            layer: layer.id,
            selection: Some(selection.clone()),
            transform: ImageTransform::default(),
        };
        let matrices = [
            Affine::translation(Point { x: 310., y: 150. }),
            Affine::around(d.center, [1.4, 0.7], 0.37, Point { x: 100., y: 50. }),
            Affine::translation(Point { x: -500., y: 0. }),
            Affine::IDENTITY,
            Affine::translation(Point {
                x: 270.5,
                y: 110.25,
            }),
        ];
        for affine in matrices {
            preview.transform.affine = affine;
            r.set_transform_preview(Some(&preview)).unwrap();
            frame(&mut r, layers, &[], &[], false);
            let live = r.readback_srgb_rgba8().unwrap();
            let mut expected = layer.clone();
            let mut op = operation(20, affine, Some(selection.clone()));
            op.kind = LayerOperationKind::Transform(preview.transform);
            expected.operations.push(op.clone());
            let operation = DabBatch {
                damage: op.bounds(extent),
                ..op_batch(0, &op)
            };
            frame(
                &mut reference,
                &[expected],
                &[d],
                &[b.clone(), operation],
                true,
            );
            assert_eq!(
                live,
                reference.readback_srgb_rgba8().unwrap(),
                "{preset:?} {affine:?}"
            );
            let captured = r.transforms.as_ref().unwrap().storage_bytes();
            let composed = r.metrics.composited_pixels;
            frame(&mut r, layers, &[], &[], false);
            assert!(
                r.transform_damage.is_empty(),
                "unchanged preview does no raster work"
            );
            assert_eq!(r.metrics.composited_pixels, composed);
            assert_eq!(r.transforms.as_ref().unwrap().storage_bytes(), captured);
            r.submit(FramePacket {
                view: ViewState {
                    document_to_surface: [1.5, 0., 0., 1.5, 30., -20.],
                    ..view
                },
                document_extent: extent,
                layers,
                dabs: &[],
                dab_batches: &[],
                time_seconds: 0.,
                reset_layers: false,
                composite_all: false,
            })
            .unwrap();
            assert!(r.transform_damage.is_empty());
            assert_eq!(
                r.metrics.composited_pixels, composed,
                "camera-only update keeps the transformed document"
            );
            // Other tools/mask passes reuse selection_clip. They must not
            // mutate the selection held by the transform's immutable source.
            let unrelated = selection.translated(Point { x: 200., y: 0. });
            let mut encoder = r.device.create_command_encoder(&Default::default());
            r.selection_clip
                .prepare(
                    &r.device,
                    &mut encoder,
                    extent,
                    &std::sync::Arc::new(unrelated),
                )
                .unwrap();
            r.queue.submit([encoder.finish()]);
        }
        r.set_transform_preview(None).unwrap();
        frame(&mut r, layers, &[], &[], false);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), original);
        assert_eq!(
            raw(&r),
            original_raw,
            "{preset:?} cancel restores sparse pigment and wetness"
        );

        // A new preview may be committed as an ordinary history operation.
        preview.transaction += 1;
        r.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut r, layers, &[], &[], false);
        let before_commit = r.readback_srgb_rgba8().unwrap();
        let captures = r.transforms.as_ref().unwrap().source_captures;
        let mut op = operation(21, preview.transform.affine, preview.selection.clone());
        op.kind = LayerOperationKind::Transform(preview.transform);
        let operation = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        layer.operations.push(op);
        r.set_transform_preview(None).unwrap();
        frame(&mut r, &[layer], &[], &[operation], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            before_commit,
            "{preset:?} apply must not jump"
        );
        assert_eq!(
            r.transforms.as_ref().unwrap().source_captures,
            captures,
            "apply reuses the matching preview result"
        );
    }
}

#[test]
fn deleting_a_transform_preview_target_discards_it_without_restoring_missing_pixels() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layer = Layer::paint(LayerId(1), "remove preview target");
    submit(
        &mut r,
        &[layer],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1,
        layer: LayerId(1),
        selection: None,
        transform: ImageTransform {
            affine: Affine::translation(Point { x: 20., y: 0. }),
            ..Default::default()
        },
    }))
    .unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "remove preview target")],
        &[],
        &[],
        false,
    );
    r.set_transform_preview(None).unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(2), "empty remaining")],
        &[],
        &[],
        false,
    );
    assert!(!r.transforms.as_ref().unwrap().has_preview());
    assert!(r.readback_srgb_rgba8().unwrap().iter().all(|v| *v == 0));
}

#[test]
fn ordered_transforms_preserve_wetness_and_match_combined_replay() {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    for preset in [GPen, WetRound, WatercolorWash] {
        let mut layer = Layer::paint(LayerId(1), "transform");
        let mut brush = batch(1);
        brush.style = preset_style(preset);
        let mut blue = dab([0., 0., 1., 0.7]);
        blue.center = Point { x: 36., y: 60. };
        blue.radii = [20., 20.];
        blue.material = [0.5, 0.8, 1., 0.8];
        submit(&mut r, &[layer.clone()], &[blue], &[brush.clone()], true);
        let initial = r.readback_srgb_rgba8().unwrap();
        let state = |r: &WgpuRasterizer| {
            let l = &r.paint_layers[0];
            let mut data = vec![page_bytes(r, &l.pages[0].active().texture)];
            data.extend(
                l.material_pages
                    .iter()
                    .map(|p| page_bytes(r, &p.wetness.texture)),
            );
            data.extend(
                l.watercolor_wetness_pages
                    .iter()
                    .map(|p| page_bytes(r, &p.active().texture)),
            );
            data
        };
        let before = state(&r);
        if preset != GPen {
            assert!(before.len() > 1 && before[1].iter().any(|v| *v != 0));
        }
        let translate = operation(10, Affine::translation(Point { x: 40., y: 0. }), None);
        layer.operations.push(translate.clone());
        let first = op_batch(0, &translate);
        submit(
            &mut r,
            std::slice::from_ref(&layer),
            &[],
            std::slice::from_ref(&first),
            false,
        );
        let after = state(&r);
        assert!(r.scene.is_none(), "transforms write paint pages directly");
        for (source, target) in before.iter().zip(&after) {
            let channels = source.len() / (256 * 256);
            for y in 0..128 {
                for x in 0..128 {
                    let i = (y * 256 + x) * channels;
                    let expected = if x >= 40 {
                        &source[(y * 256 + x - 40) * channels..][..channels]
                    } else {
                        &[0; 4][..channels]
                    };
                    assert_eq!(
                        &target[i..i + channels],
                        expected,
                        "{preset:?}, {channels} channels, {x},{y}"
                    );
                }
            }
        }
        assert_ne!(r.readback_srgb_rgba8().unwrap(), initial);
        // A second transform in the same submission must not overwrite the
        // first transform's uniforms or source before the first draw executes.
        let undo = operation(11, Affine::translation(Point { x: -40., y: 0. }), None);
        layer.operations.push(undo.clone());
        let second = op_batch(1, &undo);
        submit(
            &mut r,
            std::slice::from_ref(&layer),
            &[],
            std::slice::from_ref(&second),
            false,
        );
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            initial,
            "{preset:?} round trip"
        );
        assert_eq!(
            state(&r),
            before,
            "{preset:?} keeps unbaked pigment and wetness"
        );
        submit(
            &mut r,
            &[layer.clone()],
            &[blue],
            &[brush.clone(), first, second],
            true,
        );
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            initial,
            "{preset:?} same-frame replay"
        );
        assert_eq!(state(&r), before);
        // Later paint consumes the transformed persistent channels normally.
        let mut red = blue;
        red.center.x += 12.;
        red.color_rgba_linear = [1., 0., 0., 0.7];
        brush.stroke_id = StrokeId(2);
        submit(&mut r, &[layer.clone()], &[red], &[brush], false);
        assert_ne!(r.readback_srgb_rgba8().unwrap(), initial);
    }
}

#[test]
fn transform_selection_moves_to_new_tiles_preserves_unselected_and_layer_offset() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [768, 512];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    let mut layer = Layer::paint(LayerId(1), "selected transform");
    layer.properties.offset = Point { x: 5., y: 7. };
    let mut d = dab([1., 0., 0., 1.]);
    d.center = Point { x: 128., y: 128. };
    d.radii = [30.; 2];
    let mut b = batch(1);
    b.damage = Rect {
        min: Point { x: 80., y: 80. },
        max: Point { x: 176., y: 176. },
    };
    let submit =
        |r: &mut WgpuRasterizer, layer: &Layer, dabs: &[Dab], batches: &[DabBatch], reset| {
            r.submit(FramePacket {
                view,
                document_extent: extent,
                layers: std::slice::from_ref(layer),
                dabs,
                dab_batches: batches,
                reset_layers: reset,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        };
    submit(&mut r, &layer, &[d], &[b.clone()], true);
    let before = r.readback_srgb_rgba8().unwrap();
    let selection = Selection::polygon(vec![
        Point { x: 128., y: 0. },
        Point { x: 256., y: 0. },
        Point { x: 256., y: 256. },
        Point { x: 128., y: 256. },
    ])
    .unwrap();
    let op = operation(
        10,
        Affine::translation(Point { x: 300., y: 100. }),
        Some(selection),
    );
    let op_batch = DabBatch {
        damage: op.bounds(extent),
        ..op_batch(0, &op)
    };
    layer.operations.push(op);
    submit(&mut r, &layer, &[], std::slice::from_ref(&op_batch), false);
    let incremental = r.readback_srgb_rgba8().unwrap();
    assert_ne!(incremental, before);
    let at =
        |image: &[u8], x: usize, y: usize| image[(y * extent[0] as usize + x) * 4..][..4].to_vec();
    assert_eq!(
        at(&incremental, 120 + 5, 128 + 7),
        vec![255, 0, 0, 255],
        "unselected stays"
    );
    assert_eq!(
        at(&incremental, 140 + 5, 128 + 7),
        vec![0; 4],
        "selected is cut"
    );
    assert_eq!(
        at(&incremental, 440 + 5, 228 + 7),
        vec![255, 0, 0, 255],
        "placement crosses tiles"
    );
    submit(&mut r, &layer, &[d], &[b, op_batch], true);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        incremental,
        "replay across tile boundary"
    );
    assert!(
        r.paint_layers[0].pages.len() < 6,
        "only intersecting pages are allocated"
    );
}

#[test]
fn transform_damage_reaches_masks_groups_and_cached_clipped_filters() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [768, 512];
    let mut group = Layer::paint(LayerId(3), "group");
    group.kind = LayerKind::Group;
    group.opacity = 0.8;
    group.properties.offset = Point { x: 7., y: 5. };
    let mut filter = Layer::paint(LayerId(4), "blur");
    filter.kind = LayerKind::Effect;
    filter.properties.parent = Some(group.id);
    filter.effect = Some(std::sync::Arc::new(
        layer_core::bundled_effect_catalog()
            .get("gaussian_blur")
            .unwrap()
            .preview()
            .unwrap(),
    ));
    let mut paint = Layer::paint(LayerId(1), "transformed");
    paint.properties.parent = Some(group.id);
    paint.mask = Some(left_mask(9));
    paint.mask.as_mut().unwrap().inverted = true;
    let mut layers = vec![group, filter, paint, Layer::paint(LayerId(2), "backdrop")];
    let mut blue = dab([0., 0.2, 0.8, 0.8]);
    blue.center = Point { x: 130., y: 120. };
    blue.radii = [60.; 2];
    let mut red = dab([0.7, 0.1, 0., 1.]);
    red.center = Point { x: 250., y: 140. };
    red.radii = [200.; 2];
    let dabs = [red, blue];
    let mut brushes = [batch(2), batch(1)];
    brushes[0].damage = Rect {
        min: Point::default(),
        max: Point { x: 460., y: 350. },
    };
    brushes[1].first_dab = 1;
    brushes[1].damage = Rect {
        min: Point { x: 65., y: 55. },
        max: Point { x: 195., y: 185. },
    };
    let render = |r: &mut WgpuRasterizer,
                  layers: &[Layer],
                  dabs: &[Dab],
                  batches: &[DabBatch],
                  reset,
                  all| {
        r.submit(FramePacket {
            view: ViewState {
                width_px: extent[0],
                height_px: extent[1],
                ..view()
            },
            document_extent: extent,
            layers,
            dabs,
            dab_batches: batches,
            reset_layers: reset,
            time_seconds: 0.,
            composite_all: all,
        })
        .unwrap();
        r.readback_srgb_rgba8().unwrap()
    };
    for clipped in [false, true] {
        layers[1].properties.clipped = clipped;
        layers[2].operations.clear();
        let before = render(&mut r, &layers, &dabs, &brushes, true, false);
        let mut preview = layer_render::TransformPreview {
            transaction: 1,
            layer: LayerId(1),
            selection: None,
            transform: ImageTransform {
                affine: Affine::translation(Point { x: 150., y: 40. }),
                ..Default::default()
            },
        };
        for x in [150., 300., 70.] {
            preview.transform.affine.0[4] = x;
            r.set_transform_preview(Some(&preview)).unwrap();
            let live = render(&mut r, &layers, &[], &[], false, false);
            assert!(live != before);
            assert!(
                live == render(&mut r, &layers, &[], &[], false, true),
                "live clipped={clipped} cache invalidation"
            );
        }
        r.set_transform_preview(None).unwrap();
        assert!(
            render(&mut r, &layers, &[], &[], false, false) == before,
            "cancel clipped={clipped} cache invalidation"
        );
        let op = operation(10, Affine::translation(Point { x: 180., y: -40. }), None);
        let change = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        layers[2].operations.push(op);
        let incremental = render(
            &mut r,
            &layers,
            &[],
            std::slice::from_ref(&change),
            false,
            false,
        );
        assert!(
            incremental != before,
            "transform must visibly change the filtered layer"
        );
        assert!(
            incremental == render(&mut r, &layers, &[], &[], false, true),
            "cached filter damage, clipped={clipped}"
        );
        let mut replay = brushes.to_vec();
        replay.push(change);
        assert!(
            incremental == render(&mut r, &layers, &dabs, &replay, true, false),
            "masked/grouped replay, clipped={clipped}"
        );
    }
}

#[test]
#[ignore = "hardware capture + transform + composition benchmark; release, serial"]
fn ordered_transform_latency() {
    measure_transform_latency(false);
}

#[test]
#[ignore = "hardware live transform + composition benchmark; release, serial"]
fn live_transform_latency() {
    measure_transform_latency(true);
}

fn measure_transform_latency(live: bool) {
    use layer_core::DefaultBrushPreset::*;
    use std::time::Instant;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [2048, 1536];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    let asset = AssetId::from("test:transform latency");
    let pixels: Vec<_> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            let (x, y) = (i % extent[0], i / extent[0]);
            if x < 128 || y < 128 || x >= extent[0] - 128 || y >= extent[1] - 128 {
                [0; 4]
            } else {
                [(x % 200) as u8, (y % 220) as u8, 80, 220]
            }
        })
        .collect();
    r.prepare_asset(
        &asset,
        HostImage {
            width: extent[0],
            height: extent[1],
            stride: extent[0] * 4,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels,
        },
    )
    .unwrap();
    let percentile = |mut values: Vec<f32>| {
        values.sort_by(f32::total_cmp);
        [0.5, 0.95, 0.99].map(|p| values[(values.len() as f32 * p).ceil() as usize - 1])
    };
    for (preset, selected) in [
        (GPen, false),
        (GPen, true),
        (WetRound, true),
        (WatercolorWash, true),
    ] {
        r.set_transform_preview(None).unwrap();
        let mut layer = Layer::paint(LayerId(1), "capture benchmark");
        layer.asset = Some(asset.clone());
        let mut b = batch(1);
        b.style = preset_style(preset);
        b.damage = Rect {
            min: Point { x: 700., y: 400. },
            max: Point { x: 1348., y: 1100. },
        };
        let mut d = dab([0.2, 0.3, 0.8, 0.7]);
        d.center = Point { x: 1024., y: 768. };
        d.radii = [300.; 2];
        d.material = [0.5, 0.8, 1., 0.8];
        r.submit(FramePacket {
            view,
            document_extent: extent,
            layers: std::slice::from_ref(&layer),
            dabs: &[d],
            dab_batches: &[b],
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        r.wait_idle().unwrap();
        let selected = selected.then(|| {
            Selection::polygon(vec![
                Point { x: 256., y: 256. },
                Point { x: 1792., y: 256. },
                Point { x: 1792., y: 1280. },
                Point { x: 256., y: 1280. },
            ])
            .unwrap()
        });
        let delta = Point { x: 1., y: 1. };
        let operations = vec![
            operation(10, Affine::translation(delta), selected.clone()),
            operation(
                11,
                Affine::translation(Point { x: -1., y: -1. }),
                selected.as_ref().map(|s| s.translated(delta)),
            ),
        ];
        let batches = [0, 1].map(|i| DabBatch {
            damage: operations[i].bounds(extent),
            ..op_batch(i as u32, &operations[i])
        });
        if !live {
            layer.operations = operations;
        }
        r.telemetry = telemetry::Telemetry::new(r.device(), r.queue());
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        let mut scratch = 0;
        let mut captures = 0;
        for i in 0..160 {
            r.set_telemetry_enabled(i >= 40);
            let start = Instant::now();
            if live {
                let t = i as f32 * 0.04;
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction: 1,
                    layer: layer.id,
                    selection: selected.clone(),
                    transform: ImageTransform {
                        affine: Affine::around(
                            Point { x: 1024., y: 768. },
                            [1. + t.sin() * 0.02; 2],
                            t.cos() * 0.01,
                            Point {
                                x: t.sin() * 5.,
                                y: t.cos() * 3.,
                            },
                        ),
                        ..Default::default()
                    },
                }))
                .unwrap();
            }
            r.submit(FramePacket {
                view,
                document_extent: extent,
                layers: std::slice::from_ref(&layer),
                dabs: &[],
                dab_batches: if live {
                    &[]
                } else {
                    &batches[i % 2..i % 2 + 1]
                },
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
            let submitted = start.elapsed().as_secs_f32() * 1000.;
            r.wait_idle().unwrap();
            let elapsed = start.elapsed().as_secs_f32() * 1000.;
            if i == 0 {
                captures = r.transforms.as_ref().unwrap().source_captures;
                eprintln!(
                    "{preset:?} live={live} selected={}: first complete {elapsed:.3}ms",
                    selected.is_some()
                );
            }
            if live {
                assert_eq!(
                    r.transforms.as_ref().unwrap().source_captures,
                    captures,
                    "one source capture per live transaction"
                );
            }
            if i >= 40 {
                cpu.push(submitted);
                completed.push(elapsed);
                assert_eq!(
                    r.transforms.as_ref().unwrap().storage_bytes(),
                    scratch,
                    "warm storage remains stable"
                );
            } else {
                scratch = r.transforms.as_ref().unwrap().storage_bytes();
            }
        }
        let telemetry = r.telemetry();
        assert_eq!(telemetry.gpu.count, 120);
        let (cpu, gpu, completed) = (
            percentile(cpu),
            percentile(telemetry.gpu.ordered()),
            percentile(completed),
        );
        eprintln!(
            "{preset:?} live={live} selected={}: CPU median/p95/p99 {cpu:.3?}ms, GPU {gpu:.3?}ms, complete {completed:.3?}ms; capture+uniform storage {scratch}B",
            selected.is_some()
        );
        assert!(completed[2] < 8.333);
    }
}
