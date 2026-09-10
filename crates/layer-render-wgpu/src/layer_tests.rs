//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use layer_core::{
    BrushDeform, BrushRendering, BrushWetMix, LayerMask, LayerOperation, LayerOperationKind, Point,
    Rect, Selection, StrokeId,
};
use layer_render::{DabStyle, ViewState};

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
fn dab(color: [f32; 4]) -> Dab {
    Dab {
        center: Point { x: 64., y: 64. },
        radii: [60., 60.],
        rotation: [1., 0.],
        motion: [0.; 2],
        color_rgba_linear: color,
        flow: 1.,
        hardness: 1.,
        texture_sign: [1.; 2],
        material: [0.; 4],
    }
}
fn batch(id: u64) -> DabBatch {
    DabBatch {
        stroke_id: StrokeId(1),
        layer_id: LayerId(id),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: DabStyle {
            alpha_locked: false,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution: BrushExecution::Dry,
            grain: None,
            dual: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
        },
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 128., y: 128. },
        },
    }
}
fn submit(
    r: &mut WgpuRasterizer,
    layers: &[Layer],
    dabs: &[Dab],
    batches: &[DabBatch],
    reset: bool,
) {
    r.submit(FramePacket {
        view: view(),
        document_extent: [128, 128],
        layers,
        dabs,
        dab_batches: batches,
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
}
fn pixel(r: &mut WgpuRasterizer, x: usize, y: usize) -> [u8; 4] {
    r.readback_srgb_rgba8().unwrap()[(y * 128 + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn left_mask(id: u64) -> LayerMask {
    let mut m = LayerMask::reveal_all(LayerId(id), Point::default());
    m.default_coverage = 0.;
    m.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 64., y: 0. },
            Point { x: 64., y: 128. },
            Point { x: 0., y: 128. },
        ])
        .unwrap(),
    );
    m
}

#[test]
fn point_sampling_reads_visible_or_raw_layer_color_without_recompositing() {
    use layer_render::{ColorSampleRequest, ColorSampleSource as Source};
    let mut r = WgpuRasterizer::new().unwrap();
    let mut layer = Layer::paint(LayerId(1), "paint");
    layer.opacity = 0.5;
    layer.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([0.25, 0.5, 1.0, 0.8])],
        &[batch(1)],
        true,
    );
    let revision = r.composite_revision;
    let sample = |r: &mut WgpuRasterizer, source, position| {
        let request = ColorSampleRequest {
            request_id: 77,
            source,
            position,
        };
        assert!(r.request_color_sample(request).unwrap());
        assert!(
            !r.request_color_sample(request).unwrap(),
            "bounded single-flight sample"
        );
        let start = std::time::Instant::now();
        loop {
            r.device.poll(wgpu::PollType::Poll).unwrap();
            if let Some(result) = r.take_color_sample() {
                let sample = result.unwrap();
                assert_eq!(sample.request_id, 77);
                break sample.rgba;
            }
            assert!(start.elapsed().as_secs() < 5);
            std::thread::yield_now();
        }
    };
    let near = |a: [f32; 4], b: [f32; 4]| {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.015, "{a:?} vs {b:?}");
        }
    };
    near(
        sample(&mut r, Source::Composite, [40, 64]),
        [0.25, 0.5, 1.0, 0.4],
    );
    near(sample(&mut r, Source::Composite, [80, 64]), [0.0; 4]);
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [80, 64]),
        [0.25, 0.5, 1.0, 0.8],
    );
    // Bounds and empty paint tiles yield no color; never clamp to an edge pixel.
    near(sample(&mut r, Source::Composite, [500, 64]), [0.0; 4]);
    near(sample(&mut r, Source::Layer(LayerId(1)), [0, 0]), [0.0; 4]);
    assert_eq!(
        r.composite_revision, revision,
        "sampling never invalidates composition"
    );
    // A subsequent edit must be sampled from the current texture, not a cached color.
    submit(
        &mut r,
        &[layer],
        &[dab([1.0, 0.0, 0.0, 1.0])],
        &[batch(1)],
        true,
    );
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [40, 64]),
        [1.0, 0.0, 0.0, 1.0],
    );
    // Raw layers use independently allocated pages, not document-sized textures.
    let mut dot = dab([0.0, 1.0, 0.0, 1.0]);
    dot.center = Point { x: 320.0, y: 320.0 };
    let mut stroke = batch(1);
    stroke.damage = Rect {
        min: Point { x: 256.0, y: 256.0 },
        max: Point { x: 384.0, y: 384.0 },
    };
    r.submit(FramePacket {
        view: view(),
        document_extent: [384, 384],
        layers: &[Layer::paint(LayerId(1), "paint")],
        dabs: &[dot],
        dab_batches: &[stroke],
        reset_layers: true,
        time_seconds: 0.0,
        composite_all: true,
    })
    .unwrap();
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [320, 320]),
        [0.0, 1.0, 0.0, 1.0],
    );
    near(
        sample(&mut r, Source::Composite, [320, 320]),
        [0.0, 1.0, 0.0, 1.0],
    );
    near(
        sample(&mut r, Source::Layer(LayerId(1)), [64, 64]),
        [0.0; 4],
    );
}

#[test]
fn gradients_share_fill_compositing_and_respect_coverage_and_alpha_lock() {
    let mut r = WgpuRasterizer::new().unwrap();
    for radial in [false, true] {
        for transparent in [false, true] {
            for alpha_locked in [false, true] {
                for selected in [false, true] {
                    let mut layer = Layer::paint(LayerId(1), "gradient");
                    let start = [1.0, 0.1, 0.0, 0.7];
                    let end = [0.0, 0.2, 1.0, if transparent { 0.0 } else { 0.7 }];
                    let coverage = if selected {
                        left_mask(9)
                    } else {
                        LayerMask::reveal_all(LayerId(9), Point::default())
                    };
                    layer.operations.push(LayerOperation {
                        after_stroke: 1,
                        coverage,
                        kind: LayerOperationKind::Gradient {
                            start: Point { x: 16.0, y: 64.0 },
                            end: Point { x: 112.0, y: 64.0 },
                            colors: [start, end],
                            radial,
                            alpha_locked,
                        },
                    });
                    let mut operation = batch(1);
                    operation.kind = DabBatchKind::LayerOperation(0);
                    operation.dab_count = 0;
                    submit(
                        &mut r,
                        &[layer],
                        &[dab([0.1, 0.2, 0.3, 0.6])],
                        &[batch(1), operation],
                        true,
                    );
                    for (x, y) in [(40, 64), (80, 64), (64, 32), (0, 0)] {
                        let p = [x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 64.0];
                        let t = (if radial {
                            p[0].hypot(p[1]) / 96.0
                        } else {
                            p[0] / 96.0
                        })
                        .clamp(0.0, 1.0);
                        let mask = f32::from(!selected || x < 64);
                        let a = ((1.0 - t) * start[3] + t * end[3]) * mask;
                        let old_alpha = if x == 0 { 0.0 } else { 0.6 };
                        let alpha = if alpha_locked {
                            old_alpha
                        } else {
                            a + old_alpha * (1.0 - a)
                        };
                        let encode = |v: f32| {
                            if v <= 0.0031308 {
                                v * 12.92
                            } else {
                                1.055 * v.powf(1.0 / 2.4) - 0.055
                            }
                        };
                        let mut expected = [0u8; 4];
                        for i in 0..3 {
                            let pigment =
                                ((1.0 - t) * start[i] * start[3] + t * end[i] * end[3]) * mask;
                            let rgb = pigment * if alpha_locked { old_alpha } else { 1.0 }
                                + [0.1, 0.2, 0.3][i] * old_alpha * (1.0 - a);
                            expected[i] = (encode(rgb / alpha.max(0.000001)) * 255.0).round() as u8;
                        }
                        expected[3] = (alpha * 255.0).round() as u8;
                        let actual = pixel(&mut r, x, y);
                        for i in 0..4 {
                            assert!(
                                actual[i].abs_diff(expected[i]) <= 4,
                                "radial {radial} clear {transparent} lock {alpha_locked} selection {selected} at {x},{y}: {actual:?} vs {expected:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn gradient_respects_layer_mask_and_clipping_base_alpha() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut gradient = Layer::paint(LayerId(1), "gradient");
    gradient.properties.clipped = true;
    gradient.mask = Some(left_mask(8));
    gradient.operations.push(LayerOperation {
        after_stroke: 0,
        coverage: LayerMask::reveal_all(LayerId(9), Point::default()),
        kind: LayerOperationKind::Gradient {
            start: Point::default(),
            end: Point { x: 128., y: 0. },
            colors: [[1., 0., 0., 0.8], [0., 0., 1., 0.8]],
            radial: false,
            alpha_locked: false,
        },
    });
    let op = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        ..batch(1)
    };
    submit(
        &mut r,
        &[gradient, Layer::paint(LayerId(2), "base")],
        &[dab([0., 0., 1., 0.5])],
        &[batch(2), op],
        true,
    );
    let tinted = pixel(&mut r, 32, 64);
    for (actual, expected) in tinted.into_iter().zip([203_u8, 0, 170, 128]) {
        assert!(
            actual.abs_diff(expected) <= 3,
            "masked clipped gradient: {tinted:?}"
        );
    }
    let masked_out = pixel(&mut r, 96, 64);
    assert_eq!(&masked_out[..3], &[0, 0, 255]);
    assert!(masked_out[3].abs_diff(128) <= 1);
    assert_eq!(pixel(&mut r, 0, 0)[3], 0);
}

#[test]
fn queued_gradients_match_replay_across_tiles_and_inverted_offset_masks() {
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [512, 384];
    let v = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    for inverted in [false, true] {
        let mut layer = Layer::paint(LayerId(1), "gradient");
        let mut coverage = LayerMask::reveal_all(LayerId(9), Point { x: -20., y: 12. });
        coverage.default_coverage = 0.;
        coverage.inverted = inverted;
        coverage.initial = Some(
            Selection::polygon(vec![
                Point { x: 250., y: 50. },
                Point { x: 410., y: 50. },
                Point { x: 410., y: 280. },
                Point { x: 250., y: 280. },
            ])
            .unwrap(),
        );
        layer.operations.push(LayerOperation {
            after_stroke: 1,
            coverage,
            kind: LayerOperationKind::Gradient {
                start: Point { x: 160., y: 64. },
                end: Point { x: 400., y: 64. },
                colors: [[1., 0., 0., 0.8], [0., 0., 1., 0.8]],
                radial: false,
                alpha_locked: false,
            },
        });
        let mut second = layer.operations[0].clone();
        second.coverage.id = LayerId(10);
        second.coverage.offset.x -= 35.;
        second.kind = LayerOperationKind::Fill {
            color: [0., 1., 0., 0.3],
            alpha_locked: false,
        };
        layer.operations.push(second);
        let dabs = [dab([0.1, 0.1, 0.1, 0.5])];
        let operations: Vec<_> = (0..2)
            .map(|i| DabBatch {
                kind: DabBatchKind::LayerOperation(i),
                dab_count: 0,
                damage: layer.operations[i as usize].bounds(extent),
                ..batch(1)
            })
            .collect();
        let render = |r: &mut WgpuRasterizer, layers: &[Layer], batches: &[DabBatch], reset| {
            r.submit(FramePacket {
                view: v,
                document_extent: extent,
                layers,
                dabs: &dabs,
                dab_batches: batches,
                reset_layers: reset,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        };
        render(
            &mut r,
            &[Layer::paint(LayerId(1), "gradient")],
            &[batch(1)],
            true,
        );
        render(&mut r, &[layer.clone()], &operations, false);
        let queued = r.readback_srgb_rgba8().unwrap();
        // A translated selection crosses the tile boundary, while its inverse
        // also has coverage in tiles with no original mask or paint page.
        for (x, y, inside) in [
            (240, 200, true),
            (256, 200, true),
            (340, 200, true),
            (480, 320, false),
        ] {
            let a = queued[(y * extent[0] as usize + x) * 4 + 3];
            assert_eq!(
                a > 0,
                inside != inverted,
                "coverage {inverted} at {x},{y}: {a}"
            );
        }
        let mut first_only = layer.clone();
        first_only.operations.truncate(1);
        render(
            &mut r,
            &[Layer::paint(LayerId(1), "gradient")],
            &[batch(1)],
            true,
        );
        render(&mut r, &[first_only], &operations[..1], false);
        render(&mut r, &[layer.clone()], &operations[1..], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            queued,
            "split frames must match queued operations"
        );
        let replay = [vec![batch(1)], operations].concat();
        render(&mut r, &[layer], &replay, true);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            queued,
            "full replay must match incremental rendering"
        );
    }
}

#[test]
fn navigator_preview_reuses_composition_and_tracks_paint_mask_and_camera() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut layer = Layer::paint(LayerId(1), "paint");
    layer.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([1.0, 0.0, 0.0, 0.5])],
        &[batch(1)],
        true,
    );
    let await_preview = |r: &mut WgpuRasterizer| {
        let start = std::time::Instant::now();
        loop {
            r.device.poll(wgpu::PollType::Poll).unwrap();
            if let Some(result) = r.take_canvas_preview() {
                break result.unwrap();
            }
            assert!(start.elapsed().as_secs() < 10, "preview map timed out");
            std::thread::yield_now();
        }
    };
    assert!(r.request_canvas_preview(None).unwrap());
    assert!(
        !r.request_canvas_preview(None).unwrap(),
        "one in-flight map"
    );
    let first = await_preview(&mut r);
    let image = first.image.unwrap();
    assert_eq!([image.width, image.height], [256, 256]);
    let at = |image: &ReadbackImage, x: usize, y: usize| -> [u8; 4] {
        image.bytes[y * image.stride as usize + x * 4..][..4]
            .try_into()
            .unwrap()
    };
    let original = at(&image, 60, 128);
    assert_eq!(&original[..3], &[255, 0, 0]);
    assert!(original[3].abs_diff(128) <= 1);
    assert_eq!(at(&image, 190, 128), [0; 4]);
    let pixels = r.metrics.composited_pixels;
    let mut camera = view();
    camera.document_to_surface = [-1.0, 0.0, 0.0, 1.0, 100.0, 50.0];
    for _ in 0..8 {
        r.submit(FramePacket {
            view: camera,
            layers: &[layer.clone()],
            document_extent: [128, 128],
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            composite_all: false,
            time_seconds: 0.0,
        })
        .unwrap();
        assert!(r.request_canvas_preview(Some(first.revision)).unwrap());
        assert!(await_preview(&mut r).image.is_none());
    }
    assert_eq!(
        r.metrics.composited_pixels, pixels,
        "camera and overview never rebuild composition"
    );
    // Live provisional ink changes the overview before pen-up, using the same target.
    let mut b = batch(1);
    b.kind = DabBatchKind::Preview;
    submit(
        &mut r,
        &[layer.clone()],
        &[dab([0.0, 0.0, 1.0, 1.0])],
        &[b],
        false,
    );
    assert!(r.request_canvas_preview(Some(first.revision)).unwrap());
    let painted = await_preview(&mut r);
    assert_ne!(painted.revision, first.revision);
    assert_eq!(
        at(painted.image.as_ref().unwrap(), 60, 128),
        [0, 0, 255, 255]
    );
    assert_eq!(at(painted.image.as_ref().unwrap(), 190, 128), [0; 4]);
    // Removing the provisional stroke restores the persistent masked image.
    submit(&mut r, &[layer], &[], &[], false);
    assert!(r.request_canvas_preview(Some(painted.revision)).unwrap());
    assert_eq!(
        at(await_preview(&mut r).image.as_ref().unwrap(), 60, 128),
        original
    );
}

#[test]
fn clipping_stack_keeps_soft_base_alpha_and_group_opacity_once() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut base = Layer::paint(LayerId(1), "base");
    let mut a = Layer::paint(LayerId(2), "clip a");
    a.properties.clipped = true;
    let mut b = Layer::paint(LayerId(3), "clip b");
    b.properties.clipped = true;
    let mut batches = vec![batch(1), batch(2), batch(3)];
    for (i, b) in batches.iter_mut().enumerate() {
        b.first_dab = i as u32;
    }
    let dabs = [
        dab([1., 0., 0., 0.4]),
        dab([0., 1., 0., 1.]),
        dab([0., 0., 1., 1.]),
    ];
    let mut layers = vec![b.clone(), a.clone(), base.clone()];
    submit(&mut r, &layers, &dabs, &batches, true);
    assert_eq!(pixel(&mut r, 64, 64), [0, 0, 255, 102]); // export is straight-alpha sRGB
    let mut group = Layer::paint(LayerId(4), "group");
    group.kind = LayerKind::Group;
    group.opacity = 0.5;
    for l in [&mut base, &mut a, &mut b] {
        l.properties.parent = Some(group.id);
    }
    layers = vec![group, b, a, base];
    submit(&mut r, &layers, &[], &[], false);
    let p = pixel(&mut r, 64, 64);
    assert!((p[3] as i32 - 51).abs() <= 1, "{p:?}");
    layers[0].visible = false;
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
}

#[test]
fn apply_mask_preserves_pixels_and_does_not_remain_a_live_mask() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    let mut mask = l.mask.take().unwrap();
    mask.show_area = false;
    l.operations.push(LayerOperation {
        after_stroke: 1,
        coverage: mask,
        kind: LayerOperationKind::ApplyMask,
    });
    let mut op = batch(1);
    op.dab_count = 0;
    op.kind = DabBatchKind::LayerOperation(0);
    submit(&mut r, &[l.clone()], &[], &[op], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    submit(&mut r, &[l], &[dab([0., 1., 0., 1.])], &[batch(1)], false);
    assert_eq!(
        pixel(&mut r, 90, 64),
        [0, 255, 0, 255],
        "new paint can extend the baked silhouette"
    );
}

#[test]
fn inspection_is_not_exported_and_translated_mask_keeps_source() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    l.mask.as_mut().unwrap().show_area = true;
    submit(&mut r, &[l.clone()], &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    l.mask.as_mut().unwrap().offset.x = 40.;
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 20, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 80, 64), [255, 0, 0, 255]);
}

#[test]
fn imported_texture_is_linearized_premultiplied_and_masked_on_gpu() {
    let mut r = WgpuRasterizer::new().unwrap();
    let id = AssetId::from("test:image");
    let bytes = [128, 0, 255, 128].repeat(128 * 128);
    r.prepare_asset(
        &id,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &bytes,
        },
    )
    .unwrap();
    let mut l = Layer::paint(LayerId(1), "texture");
    l.asset = Some(id);
    l.mask = Some(left_mask(9));
    submit(&mut r, &[l.clone()], &[], &[], true);
    let p = pixel(&mut r, 30, 64);
    assert!(
        (p[0] as i32 - 128).abs() <= 2 && p[2] == 255 && p[3] == 128,
        "{p:?}"
    );
    assert_eq!(pixel(&mut r, 90, 64), [0; 4]);
    l.mask = None;
    submit(&mut r, &[l], &[], &[], true);
    assert_eq!(pixel(&mut r, 90, 64), p);
}

#[test]
fn alpha_lock_preserves_partial_alpha_and_eraser_is_noop() {
    let mut r = WgpuRasterizer::new().unwrap();
    let l = Layer::paint(LayerId(1), "paint");
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([1., 0., 0., 0.4])],
        &[batch(1)],
        true,
    );
    let mut locked = batch(1);
    locked.style.alpha_locked = true;
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([0., 0., 1., 0.5])],
        &[locked.clone()],
        false,
    );
    let p = pixel(&mut r, 64, 64);
    assert!(
        (p[0] as i32 - p[2] as i32).abs() <= 1,
        "equal red/blue contributions: {p:?}"
    );
    assert_eq!(p[3], 102);
    locked.style.mode = DabMode::Erase;
    submit(&mut r, &[l], &[dab([1.; 4])], &[locked], false);
    assert_eq!(pixel(&mut r, 64, 64), p);
}

#[test]
fn mask_scene_preview_keeps_pixels_outside_preview_damage() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut preview = batch(1);
    preview.kind = DabBatchKind::Preview;
    preview.style.mode = DabMode::Erase;
    preview.damage = Rect {
        min: Point { x: 50., y: 50. },
        max: Point { x: 78., y: 78. },
    };
    let mut d = dab([1.; 4]);
    d.radii = [12.; 2];
    submit(&mut r, &[l.clone()], &[d], &[preview], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 30, 64), [255, 0, 0, 255]);
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
}

#[test]
fn mask_scene_destination_preview_preserves_untouched_color() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut b = batch(1);
    b.kind = DabBatchKind::Preview;
    b.style.alpha_locked = true;
    b.damage = Rect {
        min: Point { x: 20., y: 50. },
        max: Point { x: 44., y: 78. },
    };
    let mut d = dab([0., 0., 1., 1.]);
    d.center.x = 32.;
    d.radii = [10.; 2];
    submit(&mut r, &[l], &[d], &[b], false);
    assert_eq!(pixel(&mut r, 32, 64), [0, 0, 255, 255]);
    assert_eq!(pixel(&mut r, 50, 64), [255, 0, 0, 255]);
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn paint_operation_latency() {
    let mut r = WgpuRasterizer::new().unwrap();
    r.set_telemetry_enabled(true);
    let extent = [2048, 1536];
    let v = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    for selected in [false, true] {
        for (name, kind) in [
            (
                "fill",
                LayerOperationKind::Fill {
                    color: [0.3, 0.4, 0.8, 0.5],
                    alpha_locked: false,
                },
            ),
            (
                "linear",
                LayerOperationKind::Gradient {
                    start: Point { x: 500., y: 500. },
                    end: Point { x: 1200., y: 900. },
                    colors: [[1., 0., 0., 0.5], [0., 0., 1., 0.5]],
                    radial: false,
                    alpha_locked: false,
                },
            ),
            (
                "radial",
                LayerOperationKind::Gradient {
                    start: Point { x: 500., y: 500. },
                    end: Point { x: 1200., y: 900. },
                    colors: [[1., 0., 0., 0.5], [0., 0., 1., 0.5]],
                    radial: true,
                    alpha_locked: false,
                },
            ),
        ] {
            let mut layer = Layer::paint(LayerId(1), "paint operation");
            let coverage = if selected {
                left_mask(9)
            } else {
                LayerMask::reveal_all(LayerId(9), Point::default())
            };
            layer.operations.push(LayerOperation {
                after_stroke: 0,
                coverage,
                kind,
            });
            let op = DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                damage: layer.operations[0].bounds(extent),
                ..batch(1)
            };
            let mut completed = Vec::new();
            let mut cold = 0.;
            for i in 0..160 {
                let start = std::time::Instant::now();
                r.submit(FramePacket {
                    view: v,
                    document_extent: extent,
                    layers: std::slice::from_ref(&layer),
                    dabs: &[],
                    dab_batches: std::slice::from_ref(&op),
                    reset_layers: i == 0,
                    time_seconds: 0.,
                    composite_all: false,
                })
                .unwrap();
                r.wait_idle().unwrap(); // Test only; production never waits.
                let ms = start.elapsed().as_secs_f64() * 1000.;
                if i == 0 {
                    cold = ms;
                }
                if i >= 40 {
                    completed.push(ms);
                }
            }
            let stats = r.telemetry();
            assert!(stats.gpu_timestamps);
            let summary = |mut values: Vec<f32>| {
                values.sort_by(f32::total_cmp);
                [values[60], values[114], values[118]]
            };
            let cpu = summary(stats.cpu.ordered());
            let gpu = summary(stats.gpu.ordered());
            completed.sort_by(f64::total_cmp);
            eprintln!(
                "{name} selection={selected}: cold completed={cold:.3}ms; CPU {cpu:.3?}; GPU {gpu:.3?}; completed p50/p95/p99={:.3}/{:.3}/{:.3}ms",
                completed[60], completed[114], completed[118]
            );
        }
    }
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn layer_composition_latency() {
    let mut r = WgpuRasterizer::new().unwrap();
    for (name, count, masked) in [
        ("plain", 1, false),
        ("masked", 1, true),
        ("24 masks", 24, true),
        ("100 masks", 100, true),
    ] {
        let layers: Vec<_> = (1..=count)
            .map(|id| {
                let mut l = Layer::paint(LayerId(id), "layer");
                if masked {
                    l.mask = Some(left_mask(id + 1000));
                }
                l
            })
            .collect();
        let dabs: Vec<_> = (1..=count).map(|_| dab([0.2, 0.3, 0.4, 0.2])).collect();
        let batches: Vec<_> = (1..=count)
            .map(|id| {
                let mut b = batch(id);
                b.first_dab = (id - 1) as u32;
                b
            })
            .collect();
        submit(&mut r, &layers, &dabs, &batches, true);
        r.wait_idle().unwrap();
        let mut times = Vec::new();
        for i in 0..140 {
            let start = std::time::Instant::now();
            submit(&mut r, &layers, &dabs[..1], &[batch(1)], false);
            r.wait_idle().unwrap();
            if i >= 20 {
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: completed GPU frame ms p50={:.3} p95={:.3} p99={:.3}",
            times[60], times[114], times[118]
        );
    }
}
