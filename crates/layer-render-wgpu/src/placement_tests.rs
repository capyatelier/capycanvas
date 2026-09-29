use super::*;
use layer_core::{
    Affine,
    color::{SampleDepth, source::*},
};

// An interior backtrace must read original pixels even when placement maps the
// stroke across several source tiles. A two-color source also catches smudge's
// transparent-sample fallback silently keeping the destination color.
fn placed_material_backtrace(execution: BrushExecution, alpha_locked: bool) {
    let alpha = if alpha_locked { 128 } else { 255 };
    let source = |side: u32, pose: Affine| {
        rgba8_source([side; 2], |x, y| {
            let at = pose.map(Point { x: x as f32 + 0.5, y: y as f32 + 0.5 });
            if at.x < 0. || at.y < 0. || at.x >= 4096. || at.y >= 4096. {
                [0; 4]
            } else if at.x < 1792. {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, alpha]
            }
        })
    };
    let original = source(4096, Affine::IDENTITY);
    let digests: Vec<_> = original.tiles.values().map(|tile| tile.digest).collect();
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut placed = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut checked = 0;
    for (scales, angle) in [
        ([1. / 32.; 2], 0.),
        ([1. / 32.; 2], std::f32::consts::FRAC_PI_2),
        ([-1. / 32., 1. / 32.], 0.),
        ([1. / 32., 1. / 64.], 0.4),
    ] {
        let pose = Affine::around(
            Point { x: 2048., y: 2048. },
            scales,
            angle,
            Point {
                x: -1984.,
                y: -1984.,
            },
        );
        let reference_source = source(128, pose.inverse().unwrap());
        let blurs: &[f32] = if execution == BrushExecution::Smudge {
            &[0., 1.]
        } else {
            &[0.]
        };
        for &blur in blurs {
            let mut outputs = Vec::new();
            for (r, image, placement) in [
                (&mut reference, reference_source.clone(), Affine::IDENTITY),
                (&mut placed, original.clone(), pose),
            ] {
                let mut layer = Layer::paint(LayerId(1), "backtrace across placed source tiles");
                layer.source = Some(image);
                layer.properties.placement = placement;
                submit(r, &[layer.clone()], &[], &[], true);
                assert_eq!(pixel(r, 64, 64), [0, 255, 0, alpha]);
                let original_view = r.readback_srgb_rgba8().unwrap();
                let before = layer.raster.clone();
                let mut ink = dab([0., 0., 1., 1.]);
                ink.radii = [12.; 2];
                let from = pose.map(Point { x: 2048., y: 2048. });
                let to = pose.map(Point { x: 2688., y: 2048. });
                ink.motion = [to.x - from.x, to.y - from.y];
                ink.material = [0., 1., 1., 1.];
                let mut stroke = batch(1);
                stroke.style.execution = execution;
                stroke.style.alpha_locked = alpha_locked;
                stroke.style.wet_mix.blur = blur;
                stroke.style.brush_to_layer = placement.inverse().unwrap();
                stroke.damage = ink.bounds();
                // One predicted batch reads persistent source; multiple batches
                // use private preview pages. Neither may publish paint/history.
                let mut single = stroke.clone();
                single.kind = DabBatchKind::Preview;
                submit(r, &[layer.clone()], &[ink], &[single.clone()], false);
                assert_eq!(
                    pixel(r, 64, 64),
                    [255, 0, 0, alpha],
                    "single prediction {execution:?} {pose:?} blur={blur}"
                );
                let preview = r.readback_srgb_rgba8().unwrap();
                for y in [52usize, 75] {
                    for x in [52usize, 75] {
                        let index = (y * 128 + x) * 4;
                        assert_eq!(
                            &preview[index..index + 4],
                            &original_view[index..index + 4],
                            "single preview must preserve pixels outside the contact but inside its tile"
                        );
                    }
                }
                let mut second = single.clone();
                second.first_dab = 1;
                second.stroke_start = false;
                let mut follow = ink;
                follow.motion = ink.motion.map(|v| v * 0.2);
                submit(
                    r,
                    &[layer.clone()],
                    &[ink, follow],
                    &[single, second],
                    false,
                );
                assert_eq!(
                    pixel(r, 64, 64),
                    [255, 0, 0, alpha],
                    "private prediction {execution:?} {pose:?} blur={blur}"
                );
                assert!(layer.raster.is_empty());
                submit(r, &[layer.clone()], &[], &[], false);
                assert_eq!(pixel(r, 64, 64), [0, 255, 0, alpha], "prediction cancel");
                layer.raster = layer_core::raster::RasterRevision::pending();
                submit(r, &[layer.clone()], &[ink], &[stroke], false);
                layer.raster.wait_data().unwrap();
                let accepted = pixel(r, 64, 64);
                outputs.push(accepted);
                let after = layer.raster.clone();
                layer.raster = before;
                submit(r, &[layer.clone()], &[], &[], false);
                assert_eq!(pixel(r, 64, 64), [0, 255, 0, alpha], "undo");
                layer.raster = after;
                submit(r, &[layer.clone()], &[], &[], false);
                assert_eq!(pixel(r, 64, 64), accepted, "redo");
            }
            assert_eq!(
                outputs[0],
                [255, 0, 0, alpha],
                "native-size reference pulls red into green"
            );
            assert_eq!(
                outputs[1], outputs[0],
                "{execution:?} {pose:?}: scaling must not lose the backtrace source"
            );
            checked += 1;
        }
    }
    assert_eq!(
        original
            .tiles
            .values()
            .map(|tile| tile.digest)
            .collect::<Vec<_>>(),
        digests
    );
    assert!(placed.material_gather.as_ref().unwrap().storage_bytes() <= 2 * 1024 * 1024 + 320);
    println!(
        "{execution:?}, alpha_locked={alpha_locked}: {checked} placements/blur settings, prediction/cancel/commit/undo/redo and source retention passed"
    );
}

#[test]
fn placed_photo_smudge_reads_beyond_adjacent_source_tiles() {
    placed_material_backtrace(BrushExecution::Smudge, false);
}

#[test]
fn placed_photo_gradient_and_figure_use_document_geometry() {
    let size = [900, 700];
    let mut layer = Layer::paint(LayerId(1), "placed operation geometry");
    layer.source = Some(rgba8_source(size, |_, _| [255; 4]));
    let affine = Affine::around(
        Point { x: 450., y: 350. },
        [0.2, 0.3],
        0.4,
        Point { x: -386., y: -286. },
    );
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut actual = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for kind in [
        LayerOperationKind::Gradient {
            start: Point { x: 48., y: 48. },
            end: Point { x: 85., y: 70. },
            colors: [[1., 0., 0., 1.], [0., 0., 1., 1.]],
            radial: true,
            alpha_locked: false,
        },
        LayerOperationKind::Figure(layer_core::Figure {
            shape: layer_core::FigureShape::Ellipse,
            paint: layer_core::FigurePaint::Outline,
            start: Point { x: 40., y: 43. },
            end: Point { x: 85., y: 79. },
            width: 7.,
            colors: [[1., 0., 0., 1.]; 2],
            alpha_locked: false,
            erase: false,
        }),
    ] {
        let mut images = Vec::new();
        for (r, pose) in [(&mut reference, Affine::IDENTITY), (&mut actual, affine)] {
            layer.properties.placement = pose;
            let op = LayerOperation {
                placement: pose.inverse().unwrap(),
                coverage: LayerMask::reveal_all(LayerId(9), Point::default()),
                kind: kind.clone(),
            };
            let command = DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                damage: op.bounds(size),
                ..batch(1)
            };
            layer.pending_operations = vec![op];
            layer.raster = layer_core::raster::RasterRevision::pending();
            submit(r, &[layer.clone()], &[], &[command], true);
            layer.raster.wait_data().unwrap();
            images.push(r.readback_srgb_rgba8().unwrap());
        }
        let mut error = 0usize;
        for y in 30..100 {
            for x in 30..100 {
                for c in 0..4 {
                    let i = (y * 128 + x) * 4 + c;
                    error += images[0][i].abs_diff(images[1][i]) as usize;
                }
            }
        }
        let mean = error as f32 / (70 * 70 * 4) as f32;
        assert!(mean < 2.5, "{kind:?}: mean channel error {mean}");
    }
}

#[test]
fn placed_photo_incremental_composition_matches_rebuild_with_alpha_and_affine_edges() {
    let size = [1024, 768];
    let mut photo = Layer::paint(LayerId(1), "moving alpha photo");
    photo.source = Some(rgba8_source(size, |x, y| {
        [(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, ((x / 7 + y / 11) % 256) as u8]
    }));
    photo.opacity = 0.63;
    let mut behind = photo.clone();
    behind.id = LayerId(2);
    behind.opacity = 0.78;
    behind.properties.placement = Affine([0.3, 0., 0., 0.3, 140.25, 87.5]);
    let canvas = [1031, 777]; // Both partial edge tiles and full interior tiles.
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut rebuilt = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut source_misses = None;
    for (step, transform) in [
        Affine([0.4, 0., 0., 0.4, 10.25, 19.75]),
        Affine([0.4, 0., 0., 0.4, -71.25, -45.75]),
        Affine([0.35, 0.15, -0.15, 0.35, 100.25, -38.5]),
        Affine([-0.4, 0., 0., 0.4, 403.25, 33.75]),
    ].into_iter().enumerate() {
        photo.properties.placement = transform;
        let layers = [photo.clone(), behind.clone()];
        let mut images = Vec::new();
        for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
            if full { r.scale_display = None; }
            r.submit(FramePacket {
                view: ViewState { width_px: canvas[0], height_px: canvas[1], background_rgba_linear: [0.12, 0.25, 0.37, 0.5], ..view() },
                reset_layers: step == 0,
                ..packet(&layers, canvas)
            }).unwrap();
            images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
        }
        let misses = r.metrics().source_tile_misses;
        if let Some(before) = source_misses { assert_eq!(misses, before, "moving reuses source pixels"); }
        else { source_misses = Some(misses); }
        let maximum = images[0].chunks_exact(4).zip(images[1].chunks_exact(4))
            .map(|(a, b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
            .fold(0.0f32, f32::max);
        assert!(maximum < 0.0001, "pose {step}: live pixel error {maximum}");
    }
    for (opacity, mode, mask) in [(1., DabMode::Paint, false), (0.63, DabMode::Paint, false),
        (1., DabMode::Erase, false), (0.63, DabMode::Erase, true)] {
        photo.opacity = opacity;
        if mask {
            photo.properties.placement = Affine([0.35, 0.15, -0.15, 0.35, 120.25, -38.5]);
            photo.mask = Some(LayerMask::reveal_all(LayerId(9), Point { x: 10.25, y: 20.5 }));
        }
        let layers = [photo.clone(), behind.clone()];
        let mut ink = dab([0.1, 0.7, 0.2, 0.45]);
        ink.center = Point { x: 230., y: 180. };
        ink.radii = [32.; 2];
        ink.contact = [1., 0., 0., 0.];
        let mut stroke = batch(1);
        stroke.style = preset_style(layer_core::DefaultBrushPreset::GPen);
        stroke.kind = DabBatchKind::Preview;
        stroke.style.mode = mode;
        stroke.style.brush_to_layer = layer_core::target_transform(&layers, stroke.layer_id).inverse().unwrap();
        stroke.damage = ink.bounds();
        let mut baseline = None;
        for prediction in [false, true, false] {
            let mut images = Vec::new();
            for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
                if full { r.scale_display = None; }
                let before = r.metrics.composited_pixels;
                r.submit(FramePacket {
                    view: ViewState { width_px: canvas[0], height_px: canvas[1], background_rgba_linear: [0.12, 0.25, 0.37, 0.5], ..view() },
                    dabs: if prediction { std::slice::from_ref(&ink) } else { &[] },
                    dab_batches: if prediction { std::slice::from_ref(&stroke) } else { &[] },
                    composite_all: full || baseline.is_none(),
                    ..packet(&layers, canvas)
                }).unwrap();
                if prediction && !full {
                    assert!(r.metrics.composited_pixels - before < u64::from(canvas[0]) * u64::from(canvas[1]),
                        "a placed contact must not rebuild the entire canvas");
                }
                images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
            }
            let maximum = images[0].chunks_exact(4).zip(images[1].chunks_exact(4))
                .map(|(a, b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
                .fold(0.0f32, f32::max);
            assert!(maximum < 0.0001, "prediction={prediction}, {mode:?}, opacity={opacity}: {maximum}");
            if let Some(before) = &baseline {
                if prediction { assert!(&images[0] != before, "prediction changes live pixels"); }
                else { assert!(&images[0] == before, "cancel restores live pixels exactly"); }
            } else { baseline = Some(images.remove(0)); }
        }
        if mask {
            stroke.layer_id = LayerId(9);
            stroke.kind = DabBatchKind::Persistent;
            stroke.style.brush_to_layer = layer_core::target_transform(&layers, stroke.layer_id).inverse().unwrap();
            let mut images = Vec::new();
            for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
                if full { r.scale_display = None; }
                r.submit(FramePacket {
                    view: ViewState { width_px: canvas[0], height_px: canvas[1], background_rgba_linear: [0.12, 0.25, 0.37, 0.5], ..view() },
                    dabs: std::slice::from_ref(&ink),
                    dab_batches: std::slice::from_ref(&stroke),
                    composite_all: full,
                    ..packet(&layers, canvas)
                }).unwrap();
                images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
            }
            assert!(images[0] != baseline.unwrap(), "mask painting changes live pixels");
            let maximum = images[0].chunks_exact(4).zip(images[1].chunks_exact(4))
                .map(|(a, b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
                .fold(0.0f32, f32::max);
            assert!(maximum < 0.0001, "transformed mask damage matches rebuilt composition: {maximum}");
        }
    }
}

#[test]
fn placed_photo_display_cache_updates_paint_preview_undo_and_retains_lod() {
    let size = [2048; 2];
    let mut layer = Layer::paint(LayerId(1), "cached photo");
    layer.source = Some(rgba8_source(size, |_, _| [255; 4]));
    layer.properties.placement = Affine([0.0625, 0., 0., 0.0625, 0., 0.]);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[layer.clone()], &[], &[], true);
    let cache = |r: &WgpuRasterizer| {
        r.scene
            .as_ref()
            .unwrap()
            .placement_cache(LayerId(1))
            .unwrap()
    };
    let cached_pixel = |r: &WgpuRasterizer, x: usize, y: usize| {
        let (texture, _, level) = cache(r);
        let bytes = page_bytes(r, &texture);
        let index = ((y >> level) * texture.width() as usize + (x >> level)) * 16;
        std::array::from_fn::<f32, 4, _>(|c| {
            f32::from_le_bytes(bytes[index + c * 4..index + c * 4 + 4].try_into().unwrap())
        })
    };
    assert_eq!(cache(&r).1, 64);
    let initial_texture = cache(&r).0;
    assert!(
        cache(&r).2 < 4,
        "reserve scale-up detail during cold preparation"
    );
    let before = layer.raster.clone();
    layer.raster = layer_core::raster::RasterRevision::pending();
    let mut ink = dab([1., 0., 0., 1.]);
    ink.radii = [8.; 2];
    let mut stroke = batch(1);
    stroke.style.brush_to_layer = layer.properties.placement.inverse().unwrap();
    stroke.damage = ink.bounds();
    submit(&mut r, &[layer.clone()], &[ink], &[stroke.clone()], false);
    layer.raster.wait_data().unwrap();
    assert_eq!(cached_pixel(&r, 1024, 1024), [1., 0., 0., 1.]);
    assert!(
        cache(&r).1 <= 73,
        "painting updates only nearby source tiles"
    );
    let after = layer.raster.clone();
    stroke.kind = DabBatchKind::Preview;
    ink.color_rgba_linear = [0., 0., 1., 1.];
    submit(&mut r, &[layer.clone()], &[ink], &[stroke], false);
    assert_eq!(cached_pixel(&r, 1024, 1024), [0., 0., 1., 1.]);
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(
        cached_pixel(&r, 1024, 1024),
        [1., 0., 0., 1.],
        "cancel clears the cached prediction"
    );
    layer.raster = before;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(cached_pixel(&r, 1024, 1024), [1.; 4]);
    layer.raster = after;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(cached_pixel(&r, 1024, 1024), [1., 0., 0., 1.]);
    let before_scale_updates = cache(&r).1;
    layer.properties.placement = Affine([0.064, 0., 0., 0.064, 0., 0.]);
    submit(&mut r, &[layer.clone()], &[], &[], false);
    let (texture, updates, _) = cache(&r);
    assert_eq!(
        texture, initial_texture,
        "the first scale-up must reuse prepared detail"
    );
    assert_eq!(
        updates, before_scale_updates,
        "the first scale-up must not reload the source"
    );
    for scale in [0.062, 0.064, 1., 0.062] {
        layer.properties.placement = Affine([scale, 0., 0., scale, 0., 0.]);
        submit(&mut r, &[layer.clone()], &[], &[], false);
        let (current, work, _) = cache(&r);
        assert_eq!(
            current, texture,
            "reuse the finer preview across a LOD boundary or 100% inspection"
        );
        assert_eq!(work, updates, "pose changes do not reread source tiles");
    }
    let mut second = Layer::paint(LayerId(2), "second photo needs a finer preview");
    second.source = Some(rgba8_source(size, |_, _| [0, 255, 0, 255]));
    second.properties.placement = Affine([0.3, 0., 0., 0.3, 0., 0.]);
    submit(&mut r, &[layer, second], &[], &[], false);
    let scene = r.scene.as_ref().unwrap();
    assert!(
        scene.placement_cache(LayerId(1)).is_some(),
        "retain the first photo's required preview"
    );
    assert!(scene.placement_cache(LayerId(2)).is_none(), "native source samples borrow tiles");
    let bytes = page_bytes(&r, crate::test_support::document_texture(&r));
    let edge = (127 * 128 + 127) * 16;
    let actual = std::array::from_fn::<f32, 4, _>(|c|
        f32::from_le_bytes(bytes[edge + c * 4..edge + c * 4 + 4].try_into().unwrap()));
    assert_eq!(actual, [0., 1., 0., 1.], "second photo contributes native pixels");
}

#[test]
fn oversized_photo_preview_uses_admitted_memory_across_scale_boundary() {
    let size = [8192, 4352];
    let mut layer = Layer::paint(LayerId(1), "oversized cached photo");
    layer.source = Some(rgba8_source(size, |_, _| [255; 4]));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    layer.properties.placement = Affine([0.24, 0., 0., 0.24, -900., -450.]);
    submit(&mut r, &[layer.clone()], &[], &[], true);
    let (texture, updates, level) = r.scene.as_ref().unwrap().placement_cache(layer.id).unwrap();
    assert_eq!(level, 1);
    assert!(u64::from(texture.width()) * u64::from(texture.height()) * 16 < 8 * 1024 * 1024,
        "the source window is bounded by visible output");
    for scale in [0.26, 0.42, 0.24] {
        layer.properties.placement = Affine([scale, 0., 0., scale, -900., -450.]);
        submit(&mut r, &[layer.clone()], &[], &[], false);
        let cache = r.scene.as_ref().unwrap().placement_cache(layer.id).unwrap();
        assert_eq!((cache.0, cache.1, cache.2), (texture.clone(), updates, level));
        let bytes = page_bytes(&r, crate::test_support::document_texture(&r));
        assert!(bytes.chunks_exact(4).all(|v| (f32::from_le_bytes(v.try_into().unwrap()) - 1.).abs() < 1e-5));
        let misses = r.metrics().source_tile_misses;
        submit(&mut r, &[layer.clone()], &[], &[], false);
        assert_eq!(r.metrics().source_tile_misses, misses, "covered motion must reuse source tiles");
    }
}

#[test]
fn placed_photo_mask_linking_preserves_pose_and_apply_preserves_pixels() {
    let size = [600, 400];
    let mut layer = Layer::paint(LayerId(1), "placed masked photo");
    layer.source = Some(rgba8_source(size, |_, _| [255; 4]));
    layer.properties.placement = Affine([0.2, 0., 0., 0.2, 0., 0.]);
    let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 150., y: 0. },
            Point { x: 400., y: 0. },
            Point { x: 400., y: 400. },
            Point { x: 150., y: 400. },
        ])
        .unwrap(),
    );
    layer.mask = Some(mask);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[layer.clone()], &[], &[], true);
    let original = r.readback_srgb_rgba8().unwrap();
    assert_eq!(pixel(&mut r, 40, 30), [255; 4]);
    assert_eq!(pixel(&mut r, 20, 30), [0; 4]);
    layer
        .mask
        .as_mut()
        .unwrap()
        .set_linked(false, &layer.properties)
        .unwrap();
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        original,
        "unlink does not move coverage"
    );
    layer.properties.placement.0[4] += 10.;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(pixel(&mut r, 35, 30), [255; 4]);
    assert_eq!(pixel(&mut r, 85, 30), [0; 4]);
    let unlinked = r.readback_srgb_rgba8().unwrap();
    layer
        .mask
        .as_mut()
        .unwrap()
        .set_linked(true, &layer.properties)
        .unwrap();
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        unlinked,
        "relink does not move coverage"
    );
    layer.properties.placement.0[4] += 10.;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(pixel(&mut r, 35, 30), [0; 4]);
    assert_eq!(pixel(&mut r, 85, 30), [255; 4]);
    let before_apply = r.readback_srgb_rgba8().unwrap();
    let mut mask = layer.mask.take().unwrap();
    mask.placement = mask
        .transform_in_parent(&layer.properties)
        .then(layer.properties.placement.inverse().unwrap());
    mask.offset = Point::default();
    mask.linked = false;
    let op = LayerOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage: mask,
        kind: LayerOperationKind::ApplyMask,
    };
    let command = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        damage: op.bounds(size),
        ..batch(1)
    };
    layer.pending_operations.push(op);
    layer.raster = layer_core::raster::RasterRevision::pending();
    submit(&mut r, &[layer.clone()], &[], &[command], false);
    layer.raster.wait_data().unwrap();
    layer.pending_operations.clear();
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        before_apply,
        "applying a placed mask does not change the visible result"
    );
    assert_eq!(layer.source.as_ref().unwrap().extent, size);
}

#[test]
fn placed_photo_edits_and_restores_tiles_outside_canvas_bounds() {
    use layer_core::raster::RasterRevision;
    let size = [900, 700];
    let source = rgba8_source(size, |_, _| [255; 4]);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "placed editable photo");
    layer.source = Some(source.clone());
    layer.properties.placement = Affine::translation(Point { x: -560., y: -440. });
    submit(&mut r, &[layer.clone()], &[], &[], true);
    let before = layer.raster.clone();
    layer.raster = RasterRevision::pending();
    let mut ink = dab([1., 0., 0., 1.]);
    ink.center = Point { x: 624., y: 504. };
    ink.radii = [24.; 2];
    let mut stroke = batch(1);
    stroke.damage = ink.bounds();
    stroke.style.selection = Some(Arc::new(
        Selection::polygon(vec![
            Point { x: 610., y: 490. },
            Point { x: 650., y: 490. },
            Point { x: 650., y: 540. },
            Point { x: 610., y: 540. },
        ])
        .unwrap(),
    ));
    submit(&mut r, &[layer.clone()], &[ink], &[stroke], false);
    let data = layer.raster.wait_data().unwrap();
    assert!(data.tiles.keys().any(|key| key.coordinate == [2, 1]));
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&mut r, 45, 64),
        [255; 4],
        "selection clips in source coordinates"
    );
    let after = layer.raster.clone();
    layer.raster = before;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255; 4]);
    layer.raster = after;
    submit(&mut r, &[layer.clone()], &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
    // Recreate the renderer to exercise cold restoration, not just resident history.
    let mut reopened = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut reopened, &[layer], &[], &[], true);
    assert_eq!(pixel(&mut reopened, 64, 64), [255, 0, 0, 255]);
}

#[test]
fn placed_photo_brush_footprint_matches_document_brush_under_affine() {
    let size = [900, 700];
    let source = rgba8_source(size, |_, _| [255; 4]);
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut placed = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "photo brush geometry");
    layer.source = Some(source);
    let selection = Selection::polygon(vec![
        Point { x: 50., y: 30. },
        Point { x: 100., y: 30. },
        Point { x: 100., y: 100. },
        Point { x: 50., y: 100. },
    ])
    .unwrap();
    for preset in [
        layer_core::DefaultBrushPreset::GPen,
        layer_core::DefaultBrushPreset::Marker,
        layer_core::DefaultBrushPreset::WetRound,
    ] {
        let mut ink = dab([0.05, 0.3, 0.8, 1.]);
        ink.radii = [23., 16.];
        ink.rotation = [0.3_f32.cos(), 0.3_f32.sin()];
        ink.material = [0.5, 0.4, 0.7, 0.2];
        let mut stroke = batch(1);
        stroke.style = preset_style(preset);
        stroke.style.selection = Some(Arc::new(selection.clone()));
        stroke.damage = ink.bounds();
        layer.properties.placement = Affine::IDENTITY;
        submit(
            &mut reference,
            &[layer.clone()],
            &[ink],
            &[stroke.clone()],
            true,
        );
        let expected = reference.readback_srgb_rgba8().unwrap();
        for affine in [
            Affine::around(
                Point { x: 450., y: 350. },
                [0.2, 0.3],
                0.4,
                Point { x: -386., y: -286. },
            ),
            Affine([-0.2, 0.02, 0.04, 0.3, 140., -50.]),
        ] {
            layer.properties.placement = affine;
            let mut stroke = stroke.clone();
            stroke.style.brush_to_layer = affine.inverse().unwrap();
            stroke.style.selection = Some(Arc::new(
                selection.transformed(affine.inverse().unwrap()).unwrap(),
            ));
            submit(&mut placed, &[layer.clone()], &[ink], &[stroke], true);
            let actual = placed.readback_srgb_rgba8().unwrap();
            let mut total = 0usize;
            for y in 30..100 {
                for x in 30..100 {
                    for c in 0..4 {
                        let i = (y * 128 + x) * 4 + c;
                        total += actual[i].abs_diff(expected[i]) as usize;
                    }
                }
            }
            let mean = total as f32 / (70 * 70 * 4) as f32;
            assert!(
                mean < 2.5,
                "{preset:?} {affine:?}: mean channel error {mean}"
            );
            assert_eq!(
                &actual[(64 * 128 + 42) * 4..(64 * 128 + 42) * 4 + 4],
                &[255; 4],
                "selection clips the visible brush"
            );
        }
    }
}

#[test]
fn retained_placement_samples_full_source_across_tiles_without_creating_raster() {
    let size = [1537, 1025];
    let canvas = [384, 256];
    let alpha = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
            return 0.;
        }
        f32::from((x / 71 + y / 53) % 3 != 0)
    };
    let mut builder = SourceBuilder::new(
        size,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..size[1] {
        let row: Vec<_> = (0..size[0])
            .flat_map(|x| {
                [
                    65535u16,
                    65535,
                    65535,
                    (alpha(x as i32, y as i32) * 65535.) as u16,
                ]
                .into_iter()
                .flat_map(u16::to_le_bytes)
            })
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let digests: Vec<_> = source.tiles.values().map(|t| t.digest).collect();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "retained placement");
    layer.source = Some(source.clone());
    for (n, affine) in [
        Affine([0.125, 0., 0., 0.125, 30., 10.]),
        Affine::around(
            Point::default(),
            [0.23, 0.19],
            0.3,
            Point { x: 60., y: -20. },
        ),
        Affine([-0.2, 0., 0., 0.2, 350., 15.]),
        Affine([1., 0., 0., 1., -1050., -700.]),
        Affine::IDENTITY,
    ]
    .into_iter()
    .enumerate()
    {
        layer.properties.placement = affine;
        r.submit(FramePacket {
            reset_layers: n == 0,
            ..packet(std::slice::from_ref(&layer), canvas)
        })
        .unwrap();
        let pixels = r.readback_srgb_rgba8().unwrap();
        let inverse = affine.inverse().unwrap();
        let [a, b, c, d, _, _] = inverse.0;
        // The readback is an exact capture: a minified pixel averages a grid
        // of bilinear taps over its whole area.
        let count = [a.hypot(b), c.hypot(d)]
            .map(|reach| ((reach + 0.5).floor() as u32).clamp(1, crate::pixel_transform::EXACT_TAPS));
        let bilinear = |p: Point| {
            let (sx, sy) = (p.x - 0.5, p.y - 0.5);
            let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
            let (tx, ty) = (sx - sx.floor(), sy - sy.floor());
            (alpha(ix, iy) * (1. - tx) + alpha(ix + 1, iy) * tx) * (1. - ty)
                + (alpha(ix, iy + 1) * (1. - tx) + alpha(ix + 1, iy + 1) * tx) * ty
        };
        for y in 0..canvas[1] {
            for x in 0..canvas[0] {
                let mut expected = 0.;
                for j in 0..count[1] {
                    for i in 0..count[0] {
                        expected += bilinear(inverse.map(Point {
                            x: x as f32 + (i as f32 + 0.5) / count[0] as f32,
                            y: y as f32 + (j as f32 + 0.5) / count[1] as f32,
                        }));
                    }
                }
                let expected = expected / (count[0] * count[1]) as f32 * 255.;
                let actual = pixels[((y * canvas[0] + x) * 4 + 3) as usize] as f32;
                assert!(
                    (actual - expected).abs() <= 1.1,
                    "pose={n} at={x},{y} alpha={actual} expected={expected}"
                );
            }
        }
        assert!(layer.raster.is_empty());
        assert!(
            r.paint_layers.iter().all(|p| p.pages.is_empty()),
            "placement must not create paint backing"
        );
        assert_eq!(
            source.tiles.values().map(|t| t.digest).collect::<Vec<_>>(),
            digests
        );
    }
}
