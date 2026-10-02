use super::*;
use crate::test_support::{preimage, receive_request};
use layer_core::{Affine, ImageTransform, Interpolation, LayerPlacement};

fn operation(id: u64, affine: Affine, selection: Option<Selection>) -> LayerOperation {
    let mut coverage = LayerMask::reveal_all(LayerId(id), Point::default());
    coverage.default_coverage = if selection.is_some() { 0. } else { 1. };
    coverage.initial = selection;
    LayerOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage,
        kind: LayerOperationKind::Transform(ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(affine) },
            ..Default::default()
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
fn frame(r: &mut WgpuRasterizer, layers: &[Layer], extent: [u32; 2], dabs: &[Dab], batches: &[DabBatch], reset: bool) {
    r.submit(FramePacket {
        dabs,
        dab_batches: batches,
        reset_layers: reset,
        composite_all: false,
        ..packet(layers, extent)
    })
    .unwrap();
}
/// Words of `extent` pixels' coverage `value`, `count` pixels per word.
pub(super) fn packed(extent: [u32; 2], count: u32, value: impl Fn(u32, u32) -> u32) -> Vec<u32> {
    let mut words = Vec::new();
    for y in 0..extent[1] {
        for w in 0..extent[0].div_ceil(count) {
            let x = |i| w * count + i;
            words.push((0..count).filter(|i| x(*i) < extent[0]).fold(0, |word, i| word | value(x(i), y) << (i * 32 / count)));
        }
    }
    words
}
/// A layer whose mask, LayerId(9), reveals only the polygon `corners`.
pub(super) fn masked(corners: [[f32; 2]; 4]) -> Layer {
    let mut layer = Layer::paint(LayerId(1), "masked");
    let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(Selection::polygon(corners.map(|[x, y]| Point { x, y }).to_vec()).unwrap());
    layer.mask = Some(mask);
    layer
}
/// The coverage of each page of the mask LayerId(9).
pub(super) fn mask_values(r: &WgpuRasterizer) -> std::collections::BTreeMap<[u32; 2], Vec<f32>> {
    r.layer_masks
        .pages
        .iter()
        .filter(|((owner, _), _)| *owner == LayerId(9))
        .map(|((_, c), page)| {
            let values = page_bytes(r, &page.texture).chunks_exact(4).map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect();
            (*c, values)
        })
        .collect()
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
fn bicubic_and_lanczos_transforms_clamp_overshoot_at_every_sample_depth() {
    use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
    let extent = [256, 256];
    for (depth, interpolation) in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32]
        .into_iter()
        .flat_map(|depth| [Interpolation::Bicubic, Interpolation::Lanczos].map(|i| (depth, i)))
    {
        let peak = if matches!(depth, SampleDepth::F16 | SampleDepth::F32) { 8. } else { 1. };
        let source = crate::test_support::depth_source([64, 64], depth, RgbSpace::Srgb, 8 * 1024 * 1024, |x, y| {
            match (x / 3 + y / 5) % 3 {
                0 => [peak, peak, peak, 1.],
                1 => [0., 0., 0., 1.],
                _ => [0.; 4],
            }
        });
        let mut layer = Layer::paint(LayerId(1), "overshoot");
        layer.source = Some(source);
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        let frame = |r: &mut WgpuRasterizer| {
            r.submit(FramePacket {
                ..packet(std::slice::from_ref(&layer), extent)
            })
            .unwrap();
        };
        frame(&mut r);
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            layer: layer.id,
            selection: None,
            transform: ImageTransform { placement: layer_core::LayerPlacement { interpolation: interpolation, ..LayerPlacement::from_affine(Affine([3.7, 0.3, -0.2, 3.9, 1.5, 0.5])) },
                ..Default::default()
            },
        }))
        .unwrap();
        frame(&mut r);
        let mut brightest = 0f32;
        for page in &r.paint_layers[0].pages {
            let bytes = page_bytes(&r, &page.active().texture);
            for texel in bytes.chunks_exact(16) {
                let v: [f32; 4] = std::array::from_fn(|k| f32::from_le_bytes(texel[k * 4..k * 4 + 4].try_into().unwrap()));
                assert!(v.iter().all(|c| c.is_finite() && *c >= 0.), "{depth:?} {interpolation:?}: negative lobe {v:?}");
                assert!(v[3] <= 1., "{depth:?} {interpolation:?}: coverage overshoot {v:?}");
                assert!(v[..3].iter().all(|c| *c <= peak * v[3] + 1e-5), "{depth:?} {interpolation:?}: color overshoot {v:?}");
                brightest = brightest.max(v[0]);
            }
        }
        assert!(brightest > peak * 0.99, "{depth:?} {interpolation:?}: bright texels survive, {brightest}");
    }
}

#[test]
fn moving_bicubic_previews_draw_bilinearly_and_only_still_previews_commit_in_place() {
    let extent = [512, 384];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let layer = Layer::paint(LayerId(1), "moving preview");
    let mut d = dab([0.9, 0.2, 0.1, 1.]);
    d.center = Point { x: 150., y: 140. };
    d.radii = [70.; 2];
    frame(&mut r, std::slice::from_ref(&layer), extent, &[d], &[batch(1)], false);
    let affine = Affine::around(d.center, [2.3, 1.7], 0.4, Point { x: 60., y: 30. });
    let preview = |interpolation, moving, transaction| layer_render::TransformPreview {
        transaction,
        moving,
        layer: layer.id,
        selection: None,
        transform: ImageTransform { placement: layer_core::LayerPlacement { interpolation: interpolation, ..LayerPlacement::from_affine(affine) },
            ..Default::default()
        },
    };
    let mut shown = Vec::new();
    for (interpolation, moving) in [
        (Interpolation::Linear, false),
        (Interpolation::Bicubic, true),
        (Interpolation::Bicubic, false),
    ] {
        r.set_transform_preview(Some(&preview(interpolation, moving, 1))).unwrap();
        frame(&mut r, std::slice::from_ref(&layer), extent, &[], &[], false);
        shown.push(r.readback_srgb_rgba8().unwrap());
    }
    assert_eq!(shown[0], shown[1], "a moving bicubic preview draws bilinearly");
    assert_ne!(shown[1], shown[2], "releasing the handle draws the requested filter");
    for moving in [true, false] {
        let preview = preview(Interpolation::Bicubic, moving, if moving { 2 } else { 3 });
        r.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut r, std::slice::from_ref(&layer), extent, &[], &[], false);
        let captures = r.test.source_captures.get();
        let mut op = operation(30, Affine::IDENTITY, None);
        op.kind = LayerOperationKind::Transform(preview.transform.clone());
        let apply = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        let mut committed = layer.clone();
        committed.pending_operations.push(op);
        r.set_transform_preview(None).unwrap();
        frame(&mut r, std::slice::from_ref(&committed), extent, &[], &[apply], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            shown[2],
            "apply after a moving={moving} preview shows the bicubic result"
        );
        assert_eq!(
            r.test.source_captures.get() == captures,
            !moving,
            "only a still preview is kept as the commit"
        );
        r.submit(FramePacket {
            dabs: &[d],
            dab_batches: &[batch(1)],
            reset_layers: true,
            ..packet(std::slice::from_ref(&layer), extent)
        })
        .unwrap();
    }
}

/// Previews of each map of a selection's `source` over paint and wetness,
/// cutting the source or keeping it, match replaying it as an operation,
/// cancelling restores the layer, and applying the last keeps its preview.
fn live_previews_match_replay(maps: fn(Rect) -> Vec<LayerPlacement>, keep_source: bool) {
    use layer_core::DefaultBrushPreset::*;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent = [640, 384];
    let source = Rect {
        min: Point { x: 60., y: 60. },
        max: Point { x: 300., y: 300. },
    };
    for preset in [GPen, WatercolorWash] {
        let mut layer = Layer::paint(LayerId(1), "live transform");
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
        frame(&mut r, layers, extent, &[d], &[b.clone()], true);
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
        for (n, map) in maps(source).into_iter().enumerate() {
            preview.transform = ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Linear, ..map },
                keep_source,
                source_from_owner: None,
            };
            r.set_transform_preview(Some(&preview)).unwrap();
            frame(&mut r, layers, extent, &[], &[], false);
            assert_ne!(r.readback_srgb_rgba8().unwrap(), original, "{preset:?} map {n} moves pixels");
            let mut expected = layer.clone();
            let mut op = operation(20, Affine::IDENTITY, Some(selection.clone()));
            op.kind = LayerOperationKind::Transform(preview.transform.clone());
            expected.pending_operations.push(op.clone());
            let operation = DabBatch {
                damage: op.bounds(extent),
                ..op_batch(0, &op)
            };
            frame(&mut reference, &[expected], extent, &[d], &[b.clone(), operation], true);
            assert_eq!(
                r.readback_srgb_rgba8().unwrap(),
                reference.readback_srgb_rgba8().unwrap(),
                "{preset:?} map {n}"
            );
        }
        r.set_transform_preview(None).unwrap();
        frame(&mut r, layers, extent, &[], &[], false);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), original, "{preset:?} cancel");
        preview.transaction += 1;
        r.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut r, layers, extent, &[], &[], false);
        let before_commit = r.readback_srgb_rgba8().unwrap();
        let captures = r.test.source_captures.get();
        let mut op = operation(21, Affine::IDENTITY, preview.selection.clone());
        op.kind = LayerOperationKind::Transform(preview.transform.clone());
        let operation = DabBatch {
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        layer.pending_operations.push(op);
        r.set_transform_preview(None).unwrap();
        frame(&mut r, &[layer], extent, &[], &[operation], false);
        assert_eq!(
            r.readback_srgb_rgba8().unwrap(),
            before_commit,
            "{preset:?} apply must not jump"
        );
        assert_eq!(
            r.test.source_captures.get(),
            captures,
            "apply reuses the matching preview result"
        );
    }
}

#[test]
fn live_perspective_matches_replay_cancels_exactly_and_commits_without_jump() {
    live_previews_match_replay(|source| {
        [
            [[90., 40.], [420., 90.], [460., 330.], [40., 250.]],
            [[260., 70.], [300., 70.], [600., 370.], [10., 370.]],
            [[300., 300.], [60., 300.], [60., 60.], [300., 60.]],
        ]
        .map(|quad| {
            LayerPlacement::from_projective(layer_core::Projective::rect_to_quad(source, quad.map(|[x, y]| Point { x, y })).unwrap())
        })
        .to_vec()
    }, false);
}

#[test]
fn moved_copies_that_keep_their_source_match_replay_and_commit_without_jump() {
    live_previews_match_replay(|_| {
        [[120., -40.], [-35., 16.], [250., 70.]]
            .map(|[x, y]| LayerPlacement::from_affine(Affine::translation(Point { x, y })))
            .to_vec()
    }, true);
}

#[test]
fn transform_selection_moves_to_new_tiles_preserves_unselected_and_layer_offset() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent = [768, 512];
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
    let submit = |r: &mut WgpuRasterizer, layer: &Layer, dabs: &[Dab], batches: &[DabBatch], reset| {
        frame(r, std::slice::from_ref(layer), extent, dabs, batches, reset)
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

#[test]
fn mapped_pixel_selections_resample_through_perspective_like_a_cpu_reference() {
    use layer_core::{Projective, SelectionPixels};
    use layer_render::{RegionRequest, RegionSource};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let layer = Layer::paint(LayerId(1), "mapped selection");
    submit(&mut r, std::slice::from_ref(&layer), &[], &[], true);
    let extent = [128u32, 128];
    let soft = |x: u32, y: u32| -> u32 {
        let inside = (20..90).contains(&x) && (30..100).contains(&y);
        if !inside {
            0
        } else if (x + y) % 17 == 0 {
            64
        } else {
            255 - (x * 3 + y) % 200
        }
    };
    let byte_pixels = SelectionPixels::bytes(extent, [20, 30, 90, 100], packed(extent, 4, soft)).unwrap();
    let nibbles = packed(extent, 8, |x, y| soft(x, y) * 4 / 255);
    let nibble_pixels = SelectionPixels::new(extent, [20, 30, 90, 100], nibbles).unwrap();
    let placement = Affine::around(Point::default(), [1.1, 0.9], 0.2, Point { x: 3., y: -4. });
    let map = Projective::rect_to_quad(
        Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 128., y: 128. },
        },
        [[10.3, 5.1], [118.2, 20.4], [100.1, 120.3], [2.2, 110.4]].map(|[x, y]| Point { x, y }),
    )
    .unwrap();
    let forward: [f64; 9] = {
        let [a, b, c, d, x, y] = placement.0.map(f64::from);
        let m = map.0.map(f64::from);
        let p = [a, c, x, b, d, y, 0., 0., 1.];
        std::array::from_fn(|i| (0..3).map(|k| m[i / 3 * 3 + k] * p[k * 3 + i % 3]).sum())
    };
    for (n, pixels) in [byte_pixels, nibble_pixels].into_iter().enumerate() {
        let scale = if pixels.coverage_format() == 2 { 255. } else { 4. };
        let coverage = |x: i32, y: i32| -> f64 {
            if x < 0 || y < 0 || x >= extent[0] as i32 || y >= extent[1] as i32 {
                return 0.;
            }
            let per = pixels.pixels_per_word();
            let word =
                pixels.words()[(y as u32 * extent[0].div_ceil(per) + x as u32 / per) as usize];
            let bits = 32 / per;
            f64::from((word >> ((x as u32 % per) * bits)) & ((1 << bits) - 1)) / scale
        };
        for inverted in [false, true] {
            let mut selection = Selection::pixels(std::sync::Arc::new(pixels.clone()))
                .transformed(placement)
                .unwrap();
            selection.inverted = inverted;
            let id = 70 + n as u64 * 2 + u64::from(inverted);
            let request = RegionRequest {
                contiguous: false,
                selection: None,
                request_id: id,
                source: RegionSource::TransformedSelection {
                    layer: layer.id,
                    selection: std::sync::Arc::new(selection),
                    map: LayerPlacement::from_projective(map),
                },
                position: [0, 0],
                tolerance: 0.,
                refinement: Default::default(),
                limit: None,
            };
            let result = receive_request(&mut r, request);
            assert_eq!(result.request_id, id);
            assert_eq!(result.pixels.extent(), extent);
            assert_eq!(result.pixels.coverage_format(), 2);
            let mut bounds = [u32::MAX, u32::MAX, 0, 0];
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let value = preimage(forward, [x as f64 + 0.5, y as f64 + 0.5]).map_or(0., |[u, v]| {
                        let [u, v] = [u - 0.5, v - 0.5];
                        let [ix, iy] = [u.floor() as i32, v.floor() as i32];
                        let [tx, ty] = [u - u.floor(), v - v.floor()];
                        (coverage(ix, iy) * (1. - tx) + coverage(ix + 1, iy) * tx) * (1. - ty)
                            + (coverage(ix, iy + 1) * (1. - tx) + coverage(ix + 1, iy + 1) * tx) * ty
                    });
                    let expected = (value * 255.).round() as u32;
                    let word = result.pixels.words()[(y * extent[0].div_ceil(4) + x / 4) as usize];
                    let actual = (word >> ((x % 4) * 8)) & 255;
                    assert!(
                        actual.abs_diff(expected) <= 1,
                        "format {n} inverted {inverted} at {x},{y}: {actual} != {expected}"
                    );
                    if actual > 0 {
                        bounds = [
                            bounds[0].min(x),
                            bounds[1].min(y),
                            bounds[2].max(x + 1),
                            bounds[3].max(y + 1),
                        ];
                    }
                }
            }
            assert_eq!(result.pixels.bounds(), bounds);
        }
    }
    let contours = RegionRequest {
        contiguous: false,
        selection: None,
        request_id: 80,
        source: RegionSource::TransformedSelection {
            layer: layer.id,
            selection: std::sync::Arc::new(
                Selection::polygon(vec![
                    Point::default(),
                    Point { x: 9., y: 0. },
                    Point { x: 0., y: 9. },
                ])
                .unwrap(),
            ),
            map: LayerPlacement::from_projective(map),
        },
        position: [0, 0],
        tolerance: 0.,
        refinement: Default::default(),
        limit: None,
    };
    assert!(r.request_region(contours).is_err(), "contours map exactly on the CPU");
}

/// Warps seeded from perspectives, with a node and a tangent moved, and a net
/// mapped through a perspective as the Warp handles leave it.
fn warps(source: Rect) -> Vec<layer_core::MeshMap> {
    use layer_core::{MeshMap, Projective};
    let quad = |q: [[f32; 2]; 4]| Projective::rect_to_quad(source, q.map(|[x, y]| Point { x, y })).unwrap();
    let keystone = quad([[90., 40.], [420., 90.], [460., 330.], [40., 250.]]);
    let edited = MeshMap::fit(source, [3, 3], |p| keystone.map(p))
        .unwrap()
        .move_node(5, Point { x: 21.5, y: -14.25 })
        .unwrap()
        .move_tangent(10, 1, Point { x: 190.3, y: 150.7 })
        .unwrap();
    let flat = MeshMap::identity(source, [4, 4]).unwrap().move_node(7, Point { x: -12., y: 18. }).unwrap();
    let through = quad([[260., 70.], [300., 70.], [600., 370.], [10., 370.]]);
    let mapped = MeshMap {
        net: flat.net.iter().map(|p| through.map(*p).unwrap_or(*p)).collect(),
        ..flat.clone()
    };
    vec![edited, flat, mapped]
}

#[test]
fn live_warps_match_replay_cancel_exactly_and_commit_without_jump() {
    live_previews_match_replay(|source| {
        warps(source).into_iter().map(|mesh| layer_core::LayerPlacement { mesh: Some(std::sync::Arc::new(mesh)), ..Default::default() }).collect()
    }, false);
}

#[test]
fn mask_warps_commit_and_replay_as_previewed() {
    let extent = [384, 256];
    let frame = |r: &mut WgpuRasterizer, layer: &Layer, batches: &[DabBatch], reset| {
        frame(r, std::slice::from_ref(layer), extent, &[], batches, reset)
    };
    let layer = masked([[20., 20.], [260., 30.], [230., 200.], [30., 180.]]);
    let source = Rect {
        min: Point { x: 10., y: 10. },
        max: Point { x: 280., y: 220. },
    };
    for (n, mesh) in warps(source).into_iter().enumerate() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        frame(&mut r, &layer, &[], true);
        let original = mask_values(&r);
        let transform = ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Bicubic, ..layer_core::LayerPlacement { mesh: Some(std::sync::Arc::new(mesh)), ..Default::default() } },
            ..Default::default()
        };
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            layer: LayerId(9),
            selection: None,
            transform: transform.clone(),
        }))
        .unwrap();
        frame(&mut r, &layer, &[], false);
        let previewed = mask_values(&r);
        assert_ne!(previewed, original, "warp {n} moves the mask");
        let mut op = operation(30, Affine::IDENTITY, None);
        op.kind = LayerOperationKind::Transform(transform);
        let mut committed = layer.clone();
        committed.mask.as_mut().unwrap().pending_operations = std::sync::Arc::new(vec![op.clone()]);
        let operation = DabBatch {
            layer_id: LayerId(9),
            damage: op.bounds(extent),
            ..op_batch(0, &op)
        };
        r.set_transform_preview(None).unwrap();
        frame(&mut r, &committed, std::slice::from_ref(&operation), false);
        assert_eq!(mask_values(&r), previewed, "warp {n}: apply keeps the preview");
        let mut replay = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        frame(&mut replay, &committed, &[operation], true);
        assert_eq!(mask_values(&replay), previewed, "warp {n}: replay matches the preview");
    }
}

#[test]
fn pixel_selections_resample_through_warps_like_their_affine() {
    use layer_core::{MeshMap, SelectionPixels};
    use layer_render::{RegionRequest, RegionSource};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent = [1200u32, 700];
    let layer = Layer::paint(LayerId(1), "warped selection");
    r.submit(packet(std::slice::from_ref(&layer), extent)).unwrap();
    let soft = |x: u32, y: u32| -> u32 {
        if !((100..900).contains(&x) && (80..560).contains(&y)) {
            0
        } else {
            255 - (x * 3 + y * 5) % 200
        }
    };
    let pixels = std::sync::Arc::new(SelectionPixels::bytes(extent, [100, 80, 900, 560], packed(extent, 4, soft)).unwrap());
    let selection = std::sync::Arc::new(
        Selection::pixels(pixels)
            .transformed(Affine::translation(Point { x: 4., y: -3. }))
            .unwrap(),
    );
    let affine = Affine::around(Point { x: 600., y: 350. }, [0.9, 1.15], 0.3, Point { x: 40., y: 25. });
    let bounds = Rect {
        min: Point::default(),
        max: Point { x: extent[0] as f32, y: extent[1] as f32 },
    };
    let resample = |r: &mut WgpuRasterizer, id: u64, map: LayerPlacement| {
        let request = RegionRequest {
            contiguous: false,
            selection: None,
            request_id: id,
            source: RegionSource::TransformedSelection {
                layer: layer.id,
                selection: selection.clone(),
                map,
            },
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
        };
        let result = receive_request(r, request);
        assert_eq!(result.request_id, id);
        result.pixels
    };
    let expected = resample(&mut r, 90, LayerPlacement::from_affine(affine));
    let warped = resample(
        &mut r,
        91,
        layer_core::LayerPlacement { mesh: Some(std::sync::Arc::new(MeshMap::from_affine(bounds, [4, 3], affine).unwrap())), ..Default::default() },
    );
    assert_eq!(warped.extent(), extent);
    let byte = |words: &[u32], x: u32, y: u32| (words[(y * extent[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) & 255;
    let mut covered = 0;
    for y in 0..extent[1] {
        for x in 0..extent[0] {
            let [e, w] = [byte(expected.words(), x, y), byte(warped.words(), x, y)];
            assert!(e.abs_diff(w) <= 1, "at {x},{y}: warp {w} != affine {e}");
            covered += usize::from(e > 0);
        }
    }
    assert!(covered > 100_000, "the selection lands inside the layer");
    assert_eq!(warped.bounds(), expected.bounds());
}
