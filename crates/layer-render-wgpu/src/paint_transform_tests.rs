use super::*;
use layer_core::{Affine, ImageTransform, Interpolation, TransformMap};

fn operation(id: u64, affine: Affine, selection: Option<Selection>) -> LayerOperation {
    let mut coverage = LayerMask::reveal_all(LayerId(id), Point::default());
    coverage.default_coverage = if selection.is_some() { 0. } else { 1. };
    coverage.initial = selection;
    LayerOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage,
        kind: LayerOperationKind::Transform(ImageTransform {
            map: TransformMap::Affine(affine),
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
fn deleting_a_transform_preview_target_discards_it_without_restoring_missing_pixels() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
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
        moving: false,
        layer: LayerId(1),
        selection: None,
        transform: ImageTransform::affine(Affine::translation(Point { x: 20., y: 0. })),
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
fn bicubic_transforms_clamp_overshoot_at_every_sample_depth() {
    use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth, source::*};
    let extent = [256, 256];
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
        let peak = if matches!(depth, SampleDepth::F16 | SampleDepth::F32) { 8. } else { 1. };
        let mut builder = SourceBuilder::new(
            [64, 64],
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth,
                profile: ColorProfile::Builtin(RgbSpace::Srgb),
                profile_assumed: false,
            },
            8 * 1024 * 1024,
        )
        .unwrap();
        for y in 0..64u32 {
            let row: Vec<u8> = (0..64u32)
                .flat_map(|x| {
                    let pixel: [f32; 4] = match (x / 3 + y / 5) % 3 {
                        0 => [peak, peak, peak, 1.],
                        1 => [0., 0., 0., 1.],
                        _ => [0.; 4],
                    };
                    match depth {
                        SampleDepth::U8 => pixel.map(|v| (v * 255.) as u8).to_vec(),
                        SampleDepth::U16 => {
                            pixel.into_iter().flat_map(|v| ((v * 65535.) as u16).to_le_bytes()).collect()
                        }
                        SampleDepth::F16 => layer_core::color::hdr::encode_pixel(pixel)
                            .unwrap()
                            .into_iter()
                            .flat_map(u16::to_le_bytes)
                            .collect(),
                        SampleDepth::F32 => pixel.into_iter().flat_map(f32::to_le_bytes).collect(),
                    }
                })
                .collect();
            builder.push_row(&row).unwrap();
        }
        let mut layer = Layer::paint(LayerId(1), "overshoot");
        layer.source = Some(std::sync::Arc::new(builder.finish().unwrap()));
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        let frame = |r: &mut WgpuRasterizer| {
            r.submit(FramePacket {
                view: ViewState {
                    width_px: extent[0],
                    height_px: extent[1],
                    ..view()
                },
                document_extent: extent,
                layers: std::slice::from_ref(&layer),
                dabs: &[],
                dab_batches: &[],
                restore_rasters: &[],
                reset_layers: false,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        };
        frame(&mut r);
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            layer: layer.id,
            selection: None,
            transform: ImageTransform {
                map: TransformMap::Affine(Affine([3.7, 0.3, -0.2, 3.9, 1.5, 0.5])),
                interpolation: Interpolation::Bicubic,
            },
        }))
        .unwrap();
        frame(&mut r);
        let mut brightest = 0f32;
        for page in &r.paint_layers[0].pages {
            let bytes = page_bytes(&r, &page.active().texture);
            for texel in bytes.chunks_exact(16) {
                let v: [f32; 4] = std::array::from_fn(|k| f32::from_le_bytes(texel[k * 4..k * 4 + 4].try_into().unwrap()));
                assert!(v.iter().all(|c| c.is_finite() && *c >= 0.), "{depth:?}: negative lobe {v:?}");
                assert!(v[3] <= 1., "{depth:?}: coverage overshoot {v:?}");
                assert!(v[..3].iter().all(|c| *c <= peak * v[3] + 1e-5), "{depth:?}: color overshoot {v:?}");
                brightest = brightest.max(v[0]);
            }
        }
        assert!(brightest > peak * 0.99, "{depth:?}: bright texels survive, {brightest}");
    }
}

#[test]
fn moving_bicubic_previews_draw_bilinearly_and_only_still_previews_commit_in_place() {
    let extent = [512, 384];
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..view()
    };
    let frame = |r: &mut WgpuRasterizer, layers: &[Layer], dabs: &[Dab], batches: &[DabBatch]| {
        r.submit(FramePacket {
            view,
            document_extent: extent,
            layers,
            dabs,
            dab_batches: batches,
            time_seconds: 0.,
            restore_rasters: &[],
            reset_layers: false,
            composite_all: false,
        })
        .unwrap();
    };
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let layer = Layer::paint(LayerId(1), "moving preview");
    let mut d = dab([0.9, 0.2, 0.1, 1.]);
    d.center = Point { x: 150., y: 140. };
    d.radii = [70.; 2];
    frame(&mut r, std::slice::from_ref(&layer), &[d], &[batch(1)]);
    let affine = Affine::around(d.center, [2.3, 1.7], 0.4, Point { x: 60., y: 30. });
    let preview = |interpolation, moving, transaction| layer_render::TransformPreview {
        transaction,
        moving,
        layer: layer.id,
        selection: None,
        transform: ImageTransform {
            map: TransformMap::Affine(affine),
            interpolation,
        },
    };
    let mut shown = Vec::new();
    for (interpolation, moving) in [
        (Interpolation::Linear, false),
        (Interpolation::Bicubic, true),
        (Interpolation::Bicubic, false),
    ] {
        r.set_transform_preview(Some(&preview(interpolation, moving, 1))).unwrap();
        frame(&mut r, std::slice::from_ref(&layer), &[], &[]);
        shown.push(r.readback_srgb_rgba8().unwrap());
    }
    assert_eq!(shown[0], shown[1], "a moving bicubic preview draws bilinearly");
    assert_ne!(shown[1], shown[2], "releasing the handle draws the requested filter");
    for moving in [true, false] {
        let preview = preview(Interpolation::Bicubic, moving, if moving { 2 } else { 3 });
        r.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut r, std::slice::from_ref(&layer), &[], &[]);
        let captures = r.transforms.as_ref().unwrap().source_captures();
        let mut op = operation(30, Affine::IDENTITY, None);
        op.kind = LayerOperationKind::Transform(preview.transform.clone());
        let apply = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        let mut committed = layer.clone();
        committed.pending_operations.push(op);
        r.set_transform_preview(None).unwrap();
        frame(&mut r, std::slice::from_ref(&committed), &[], &[apply]);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            shown[2],
            "apply after a moving={moving} preview shows the bicubic result"
        );
        assert_eq!(
            r.transforms.as_ref().unwrap().source_captures() == captures,
            !moving,
            "only a still preview is kept as the commit"
        );
        r.submit(FramePacket {
            view,
            document_extent: extent,
            layers: std::slice::from_ref(&layer),
            dabs: &[d],
            dab_batches: &[batch(1)],
            time_seconds: 0.,
            restore_rasters: &[],
            reset_layers: true,
            composite_all: true,
        })
        .unwrap();
    }
}

#[test]
fn live_perspective_matches_replay_cancels_exactly_and_commits_without_jump() {
    use layer_core::DefaultBrushPreset::*;
    use layer_core::Projective;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
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
                restore_rasters: &[],
                reset_layers: reset,
                composite_all: false,
            })
            .unwrap();
        };
    let source = Rect {
        min: Point { x: 60., y: 60. },
        max: Point { x: 300., y: 300. },
    };
    let quads = [
        [[90., 40.], [420., 90.], [460., 330.], [40., 250.]],
        [[260., 70.], [300., 70.], [600., 370.], [10., 370.]],
        [[300., 300.], [60., 300.], [60., 60.], [300., 60.]],
    ];
    for preset in [GPen, WatercolorWash] {
        let mut layer = Layer::paint(LayerId(1), "live perspective");
        layer.properties.offset = Point { x: 5., y: 7. };
        let mut b = batch(1);
        b.style = preset_style(preset);
        b.damage = Rect {
            min: Point { x: 70., y: 70. },
            max: Point { x: 280., y: 280. },
        };
        let mut d = dab([0.8, 0.1, 0.6, 0.7]);
        d.center = Point { x: 175., y: 175. };
        d.radii = [100.; 2];
        d.material = [0.5, 0.8, 1., 0.8];
        let layers = std::slice::from_ref(&layer);
        frame(&mut r, layers, &[d], &[b.clone()], true);
        let original = r.readback_srgb_rgba8().unwrap();
        let selection = Selection::polygon(vec![
            Point { x: 60., y: 60. },
            Point { x: 300., y: 60. },
            Point { x: 300., y: 300. },
            Point { x: 60., y: 300. },
        ])
        .unwrap();
        let mut preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            layer: layer.id,
            selection: Some(selection.clone()),
            transform: ImageTransform::default(),
        };
        for quad in quads {
            let map = Projective::rect_to_quad(source, quad.map(|[x, y]| Point { x, y })).unwrap();
            preview.transform = ImageTransform {
                map: TransformMap::Projective(map),
                interpolation: Interpolation::Linear,
            };
            r.set_transform_preview(Some(&preview)).unwrap();
            frame(&mut r, layers, &[], &[], false);
            let mut expected = layer.clone();
            let mut op = operation(20, Affine::IDENTITY, Some(selection.clone()));
            op.kind = LayerOperationKind::Transform(preview.transform.clone());
            expected.pending_operations.push(op.clone());
            let operation = DabBatch {
                damage: op.bounds(extent),
                ..op_batch(0, &op)
            };
            frame(&mut reference, &[expected], &[d], &[b.clone(), operation], true);
            assert_eq!(
                r.readback_srgb_rgba8().unwrap(),
                reference.readback_srgb_rgba8().unwrap(),
                "{preset:?} {quad:?}"
            );
        }
        r.set_transform_preview(None).unwrap();
        frame(&mut r, layers, &[], &[], false);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), original, "{preset:?} cancel");
        preview.transaction += 1;
        r.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut r, layers, &[], &[], false);
        let before_commit = r.readback_srgb_rgba8().unwrap();
        let captures = r.transforms.as_ref().unwrap().source_captures();
        let mut op = operation(21, Affine::IDENTITY, preview.selection.clone());
        op.kind = LayerOperationKind::Transform(preview.transform.clone());
        let operation = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        layer.pending_operations.push(op);
        r.set_transform_preview(None).unwrap();
        frame(&mut r, &[layer], &[], &[operation], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            before_commit,
            "{preset:?} apply must not jump"
        );
        assert_eq!(
            r.transforms.as_ref().unwrap().source_captures(),
            captures,
            "apply reuses the matching preview result"
        );
    }
}

#[test]
fn transform_selection_moves_to_new_tiles_preserves_unselected_and_layer_offset() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
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
                restore_rasters: &[],
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
    layer.pending_operations.push(op);
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
