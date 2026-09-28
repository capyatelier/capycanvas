use super::*;
use crate::test_support::{dab_batch, packet};
use layer_core::color::{SampleDepth, source::*};
use layer_core::{DefaultBrushPreset, Document};

fn document() -> Document {
    let extent = [517, 259];
    let mut doc = Document::new("display composition oracle", extent[0], extent[1]);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        8 << 20,
    )
    .unwrap();
    for y in 0..extent[1] {
        builder
            .push_row(
                &(0..extent[0])
                    .flat_map(|x| [(x / 3) as u8, (y / 2) as u8, 80, 255])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    doc.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    doc
}

#[test]
fn groups_clipping_and_all_blends_share_exact_stack_semantics() {
    let extent = [33, 19];
    let mut doc = Document::new("nested composition", extent[0], extent[1]);
    let solid = |id, color: [u8; 4]| {
        let mut source = SourceBuilder::new(extent, SourceInterpretation {
            channels: SourceChannels::Rgba, depth: SampleDepth::U8,
            profile: Default::default(), profile_assumed: false,
        }, 1 << 20).unwrap();
        let row = color.repeat(extent[0] as usize);
        for _ in 0..extent[1] { source.push_row(&row).unwrap(); }
        let mut layer = Layer::paint(LayerId(id), "solid");
        layer.source = Some(Arc::new(source.finish().unwrap()));
        layer
    };
    let mut outer = Layer::paint(LayerId(10), "outer");
    outer.kind = LayerKind::Group;
    outer.opacity = 0.63;
    let mut group_mask = layer_core::LayerMask::reveal_all(LayerId(30), Default::default());
    group_mask.default_coverage = 0.61;
    outer.mask = Some(group_mask);
    let mut inner = Layer::paint(LayerId(20), "inner");
    inner.kind = LayerKind::Group;
    inner.properties.parent = Some(outer.id);
    inner.opacity = 0.71;
    inner.properties.blend = layer_core::LayerBlend::Multiply;
    let mut base = solid(4, [50, 170, 80, 117]);
    base.properties.parent = Some(inner.id);
    base.opacity = 0.81;
    let mut clipped = solid(3, [230, 30, 120, 193]);
    clipped.properties.parent = Some(inner.id);
    clipped.properties.clipped = true;
    clipped.opacity = 0.54;
    let mut clip_mask = layer_core::LayerMask::reveal_all(LayerId(31), Default::default());
    clip_mask.default_coverage = 0.42;
    clip_mask.inverted = true;
    clipped.mask = Some(clip_mask);
    let mut behind = solid(1, [170, 210, 70, 230]);
    behind.opacity = 0.79;
    doc.layers = vec![outer, inner, clipped, base, behind, doc.layers.pop().unwrap()];
    let mut reduced = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for mode in layer_core::LayerBlend::ALL {
        doc.layers[0].properties.blend = mode;
        doc.layers[2].properties.blend = mode;
        for state in 0..4 {
            doc.layers[1].visible = state != 1;
            doc.layers[2].visible = state != 2;
            doc.layers[3].visible = state != 3;
            doc.layers[0].mask.as_mut().unwrap().enabled = state != 2;
            doc.layers[2].mask.as_mut().unwrap().show_area = state == 1;
            let mut frame = packet(&doc.layers, extent);
            frame.composite_all = false;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            reduced.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let cache = reduced.scale_display.as_ref().unwrap();
            let error = quality(&display_pixels(&reduced), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
            assert!(error[2] < 2e-5, "{mode:?} state={state}: {error:?}");
            assert_presentation_mip(&reduced);
            assert!(reduced.live_display.is_none() && reduced.composite_texture.is_none());
            let work = reduced.metrics.composited_pixels;
            reduced.submit(frame).unwrap();
            assert_eq!(work, reduced.metrics.composited_pixels);
        }
    }
}

#[test]
fn masks_refresh_coverage_properties_and_paint_without_exact_display() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(40), Default::default());
    mask.default_coverage = 0.;
    mask.initial = Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 258., y: 0. },
        layer_core::Point { x: 258., y: 259. }, layer_core::Point { x: 0., y: 259. },
    ]).unwrap());
    doc.layers[0].mask = Some(mask);
    let mut reduced = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for state in 0..7 {
        let mask = doc.layers[0].mask.as_mut().unwrap();
        mask.enabled = state != 3;
        mask.inverted = state == 1;
        mask.show_area = state == 2;
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        let dab = crate::tests::test_dab([370., 129.], [1.; 4], 1.);
        let batch = dab_batch(LayerId(40), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state == 5 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        reduced.submit(frame).unwrap();
        exact.submit(frame).unwrap();
        let cache = reduced.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&reduced), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
        assert!(error[0] < 0.001 && error[2] < 0.05, "mask state={state}: {error:?}");
        assert_presentation_mip(&reduced);
        let mut a = vec![0; (extent[0] * extent[1] * 4) as usize];
        let mut b = a.clone();
        reduced.copy_rgba8_srgb(&mut a, extent[0] as usize * 4).unwrap();
        exact.copy_rgba8_srgb(&mut b, extent[0] as usize * 4).unwrap();
        assert_eq!(a, b, "mask state={state} exact query");
        assert!(reduced.live_display.is_none() && reduced.composite_texture.is_none());
    }
}
fn pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}
fn display_pixels(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
    let cache = r.scale_display.as_ref().unwrap();
    pixels(r, &cache.output[cache.selected].texture)
}

fn assert_presentation_mip(r: &WgpuRasterizer) {
    let cache = r.scale_display.as_ref().unwrap();
    let input = display_pixels(r);
    let actual = pixels(r, &cache.next.texture);
    let size = cache.plan.level_size(cache.plan.level + 1);
    let side = 1 << cache.plan.level;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut sum = [0.; 4];
            let mut area = 0.;
            for yy in y * 2..((y + 1) * 2).min(cache.plan.size[1]) {
                for xx in x * 2..((x + 1) * 2).min(cache.plan.size[0]) {
                    let weight = (side.min(cache.plan.extent[0] - xx * side)
                        * side.min(cache.plan.extent[1] - yy * side))
                        as f32;
                    for c in 0..4 {
                        sum[c] += input[(yy * cache.plan.size[0] + xx) as usize][c] * weight;
                    }
                    area += weight;
                }
            }
            for c in 0..4 {
                assert!(
                    (actual[(y * size[0] + x) as usize][c] - sum[c] / area).abs() < 1e-5,
                    "adjacent output level must include every changed region and weight partial cells"
                );
            }
        }
    }
}

fn quality(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan) -> [f32; 3] {
    let side = 1 << plan.level;
    let mut errors = Vec::new();
    for y in 0..plan.size[1] {
        for x in 0..plan.size[0] {
            let mut sum = [0.; 4];
            let mut count = 0.;
            for yy in y * side..((y + 1) * side).min(plan.extent[1]) {
                for xx in x * side..((x + 1) * side).min(plan.extent[0]) {
                    for c in 0..4 {
                        sum[c] += exact[(yy * plan.extent[0] + xx) as usize][c];
                    }
                    count += 1.;
                }
            }
            for c in 0..4 {
                errors.push((actual[(y * plan.size[0] + x) as usize][c] - sum[c] / count).abs());
            }
        }
    }
    errors.sort_by(f32::total_cmp);
    [
        errors.iter().sum::<f32>() / errors.len() as f32,
        errors[errors.len() * 99 / 100],
        *errors.last().unwrap(),
    ]
}

#[test]
fn scaled_composition_preserves_exact_paint_and_replaces_full_display() {
    let mut doc = document();
    let mut paint = doc.layers[0].clone();
    paint.id = LayerId(50);
    paint.source = None;
    doc.layers.insert(0, paint);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut p = packet(&doc.layers, extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(p).unwrap();
    exact.submit(p).unwrap();
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
    assert!(r.scale_display.as_ref().unwrap().storage_bytes() < 1 << 20);
    let mut presenter = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let (_, target) = create_color_target(
        &r.device,
        [p.view.width_px, p.view.height_px],
        "display oracle viewport",
    );
    presenter.present(&r, &target, p.view, [0.; 4]).unwrap();
    let original = display_pixels(&r);
    let reference = pixels(&exact, exact.composite_texture.as_ref().unwrap());
    let plan = r.scale_display.as_ref().unwrap().plan;
    for y in 0..plan.size[1] {
        for x in 0..plan.size[0] {
            let mut sum = [0.; 4];
            let mut n = 0.;
            for yy in y * 8..((y + 1) * 8).min(extent[1]) {
                for xx in x * 8..((x + 1) * 8).min(extent[0]) {
                    for c in 0..4 {
                        sum[c] += reference[(yy * extent[0] + xx) as usize][c];
                    }
                    n += 1.;
                }
            }
            for c in 0..4 {
                assert!((original[(y * plan.size[0] + x) as usize][c] - sum[c] / n).abs() < 1e-5);
            }
        }
    }
    let mut dab = crate::tests::test_dab([255., 129.], [0.9, 0.02, 0.1, 0.7], 1.);
    dab.radii = [45.; 2];
    let batch = dab_batch(
        doc.layers[0].id,
        crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
        dab.bounds(),
    );
    for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
        let batch = DabBatch {
            kind,
            ..batch.clone()
        };
        let stroke = FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        };
        r.submit(stroke).unwrap();
        exact.submit(stroke).unwrap();
        assert_presentation_mip(&r);
        if kind == DabBatchKind::Preview {
            assert_eq!(r.preview_level, 2);
            assert!(
                r.preview_pages
                    .iter()
                    .all(|p| p.primary.texture.width() == 64)
            );
            assert!(r.preview_coverage_pages.is_empty());
        }
        let coarse = r.scale_display.as_ref().unwrap();
        let error = quality(
            &pixels(&r, &coarse.output[coarse.selected].texture),
            &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
            coarse.plan,
        );
        eprintln!("{kind:?} display mean/p99/max channel error: {error:?}");
        assert!(
            error[0] < 0.003,
            "large brush preview must remain close to exact reduction"
        );
        let mut a = vec![0; (extent[0] * extent[1] * 4) as usize];
        let mut b = a.clone();
        r.copy_rgba8_srgb(&mut a, extent[0] as usize * 4).unwrap();
        exact
            .copy_rgba8_srgb(&mut b, extent[0] as usize * 4)
            .unwrap();
        assert_eq!(a, b, "display resolution must not change exact output");
    }
    r.submit(FramePacket {
        composite_all: false,
        ..p
    })
    .unwrap();
    let final_pixels = display_pixels(&r);
    assert_ne!(original, final_pixels);
    let work = r.metrics.composited_pixels;
    r.submit(FramePacket {
        composite_all: false,
        ..p
    })
    .unwrap();
    assert_eq!(
        r.metrics.composited_pixels, work,
        "an unchanged display does no composition"
    );
    p.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    r.submit(p).unwrap();
    assert!(r.scale_display.is_none());
    assert!(r.composite_texture.is_some());
}

#[test]
fn compact_preview_weights_partial_edge_texels_and_retires_corrections() {
    let doc = document();
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut p = packet(&doc.layers, extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(p).unwrap();
    let original = display_pixels(&r);
    for position in [[516., 258.], [100., 100.], [514., 256.]] {
        let mut dab = crate::tests::test_dab(position, [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [12.; 2];
        let batch = DabBatch {
            kind: DabBatchKind::Preview,
            ..dab_batch(
                doc.layers[0].id,
                crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
                dab.bounds(),
            )
        };
        r.submit(FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        })
        .unwrap();
        if let Some(page) = r.preview_page([2, 1]) {
            let compact = pixels(&r, &page.active().texture);
            let cache = r.scale_display.as_ref().unwrap();
            let layer = pixels(&r, &cache.layers[&doc.layers[0].id].image.texture);
            // The 517 × 259 document ends with a 5 × 3 block: these two
            // compact texels cover 4 × 3 and 1 × 3 original pixels.
            let last = layer.last().unwrap();
            for c in 0..4 {
                assert!((last[c] - (compact[0][c] * 4. + compact[1][c]) / 5.).abs() < 1e-6);
            }
        }
    }
    r.submit(FramePacket {
        composite_all: false,
        ..p
    })
    .unwrap();
    assert_eq!(
        display_pixels(&r),
        original,
        "discarding a corrected tail restores every old footprint"
    );
    assert!(r.preview_pages.is_empty());
}

#[test]
fn scaled_layer_cache_tracks_stack_changes_and_odd_edges_at_each_level() {
    let mut doc = document();
    let mut foreground = doc.layers[0].clone();
    foreground.id = LayerId(50);
    foreground.opacity = 0.37;
    doc.layers.insert(0, foreground);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for level in [1, 3, 2, 4] {
        let scale = 1. / (1u32 << level) as f32;
        for change in 0..5 {
            match change {
                1 => doc.layers[0].opacity = 0.12,
                2 => doc.layers.swap(0, 1),
                3 => doc.layers[0].source = None,
                4 => doc.layers[0].source = doc.layers[1].source.clone(),
                _ => (),
            }
            let mut p = packet(&doc.layers, extent);
            p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(p).unwrap();
            exact.submit(p).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            let error = quality(
                &pixels(&r, &cache.output[cache.selected].texture),
                &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
                cache.plan,
            );
            assert!(
                error[2] < 2e-5,
                "constant-alpha stack, level {level}, change {change}: {error:?}"
            );
        }
    }
}

#[test]
fn scale_retirement_and_exact_effect_fallback_recreate_their_own_pixels() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let effect = crate::tests::image_windows::effect(99, false, false);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for filtered in [true, false, true, false] {
        doc.layers.retain(|l| l.id != effect.id);
        if filtered {
            doc.layers.insert(0, effect.clone());
        }
        let mut p = packet(&doc.layers, extent);
        p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        assert_eq!(r.scale_display.is_none(), filtered);
        if !filtered {
            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
        }
        let mut a = vec![0; (extent[0] * extent[1] * 4) as usize];
        let mut b = a.clone();
        r.copy_rgba8_srgb(&mut a, extent[0] as usize * 4).unwrap();
        exact
            .copy_rgba8_srgb(&mut b, extent[0] as usize * 4)
            .unwrap();
        assert_eq!(a, b);
    }
}

/// Optional photographic oracle, without putting a licensed photo in the repo.
/// Supply a 2048 × 1536, row-major, RGBA8 sRGB crop in LAYER_DISPLAY_PHOTO_RGBA.
#[test]
#[ignore = "requires a local photographic RGBA fixture in LAYER_DISPLAY_PHOTO_RGBA"]
fn photographic_preview_and_committed_display_quality() {
    let extent = [2048, 1536];
    let bytes = std::fs::read(std::env::var("LAYER_DISPLAY_PHOTO_RGBA").unwrap()).unwrap();
    assert_eq!(bytes.len(), (extent[0] * extent[1] * 4) as usize);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 << 20,
    )
    .unwrap();
    for row in bytes.chunks_exact(extent[0] as usize * 4) {
        builder.push_row(row).unwrap();
    }
    let mut doc = Document::new("photographic display oracle", extent[0], extent[1]);
    let mut photo = doc.layers[0].clone();
    photo.id = LayerId(50);
    photo.source = Some(Arc::new(builder.finish().unwrap()));
    doc.layers.insert(1, photo);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut p = packet(&doc.layers, extent);
    p.view.document_to_surface = [0.071382575, 0., 0., 0.071382575, 0., 0.];
    for diameter in [50., 460., 1000., 2000.] {
        r.submit(FramePacket {
            reset_layers: true,
            ..p
        })
        .unwrap();
        exact
            .submit(FramePacket {
                reset_layers: true,
                ..p
            })
            .unwrap();
        let mut dab = crate::tests::test_dab([1055., 780.], [0.01, 0.02, 0.03, 0.7], 1.);
        dab.radii = [diameter / 2.; 2];
        let batch = dab_batch(
            doc.layers[0].id,
            crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
            dab.bounds(),
        );
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch {
                kind,
                ..batch.clone()
            };
            let stroke = FramePacket {
                dabs: &[dab],
                dab_batches: &[batch],
                composite_all: false,
                ..p
            };
            r.submit(stroke).unwrap();
            exact.submit(stroke).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            let errors = quality(
                &display_pixels(&r),
                &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
                cache.plan,
            );
            eprintln!("photo {diameter}px {kind:?} mean/p99/max linear channel error: {errors:?}");
            assert!(
                errors[0] < 0.003 && errors[1] < 0.03,
                "photographic display quality regressed"
            );
            let mut actual = vec![0; bytes.len()];
            let mut expected = actual.clone();
            r.copy_rgba8_srgb(&mut actual, extent[0] as usize * 4)
                .unwrap();
            exact
                .copy_rgba8_srgb(&mut expected, extent[0] as usize * 4)
                .unwrap();
            assert_eq!(
                actual, expected,
                "photographic exact output at {diameter}px {kind:?}"
            );
        }
    }
}

#[test]
fn unchanged_navigation_derives_and_reuses_a_bounded_neighbor_level() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut misses = None;
    let mut composed = 0;
    for (index, level) in [2, 3, 2, 3, 2].into_iter().enumerate() {
        let scale = 1. / (1u32 << level) as f32;
        let mut p = packet(&doc.layers, extent);
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        let work = r.scene.as_ref().unwrap().source_cache_work()[1];
        assert_eq!(
            *misses.get_or_insert(work),
            work,
            "zooming must reuse already-decoded content"
        );
        if index >= 2 {
            assert_eq!(
                r.metrics.composited_pixels, composed,
                "returning to a cached level needs no composition"
            );
        }
        composed = r.metrics.composited_pixels;
        assert_presentation_mip(&r);
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.storage_bytes() <= live_display::CACHE_BYTES);
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "derived level must include correct partial-edge weights: {error:?}"
        );
    }
    assert!(r.scale_display.as_ref().unwrap().spare.is_some());
    // Changing artwork retires the spare and any native backing references it
    // holds. Returning to that level must not resurrect its stale composition.
    doc.layers[0].opacity = 0.3;
    for scale in [0.25, 0.125] {
        let mut p = packet(&doc.layers, extent);
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        if scale == 0.25 {
            assert!(r.scale_display.as_ref().unwrap().spare.is_none());
        }
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "changed artwork must replace the retired neighbor: {error:?}"
        );
    }
    doc.layers[0].visible = false;
    for (index, scale) in [0.25, 0.125, 0.25, 0.125, 0.125].into_iter().enumerate() {
        if index == 4 {
            doc.layers[0].visible = true;
        }
        let mut p = packet(&doc.layers, extent);
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "an empty retained level must remain reusable after layer visibility changes"
        );
    }
}
