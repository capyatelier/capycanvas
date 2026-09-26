use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};

fn bounded_renderer(color: DocumentColor) -> Result<WgpuRasterizer, GpuRasterError> {
    let mut r = WgpuRasterizer::new_native_headless(color)?;
    r.set_complete_display_allowance(0);
    Ok(r)
}

#[test]
fn filter_images_borrow_unused_display_allowance_with_a_combined_bound() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    let extent = [5184, 3456];
    let layers = [crate::tests::image_windows::effect(1, false, false)];
    let image_bytes = scene::Scene::capture_image_bound(&layers, PixelRect::full(extent));
    for allowance in [0, 512 << 20, 1536 << 20] {
        r.set_complete_display_allowance(allowance);
        let native = r.native_edit.as_ref().unwrap();
        let display = Cache::allocation_bound(&r, extent, native.display_cache_bytes, native.display_allowance(&layers, extent)).unwrap();
        let images = native.image_pixel_budget(&r, &layers, extent).unwrap();
        let floor = native.display_cache_bytes + scene::windows::DEFAULT_IMAGE_PIXEL_BYTES;
        assert_eq!(display + images, native.composition_bytes.max(floor));
        let plan = scene::windows::Plan::new(&layers, extent, images).unwrap();
        assert_eq!(plan.is_none(), allowance == 1536 << 20);
        if plan.is_none() { assert!(image_bytes + display <= native.composition_bytes); }
    }
}

#[test]
fn completed_filter_publication_matches_tile_composition_through_windows_and_edits() {
    let mut doc = document([777, 533]);
    doc.layers.insert(0, crate::tests::image_windows::effect(20, false, false));
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    for complete in [false, true] {
        r.set_complete_display_allowance(if complete { 320 << 20 } else { 0 });
        for limit in [u64::MAX, 8 << 20] {
            r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(limit);
            for step in 0..5 {
                doc.layers[0].opacity = [1., 0.37, 0., 0.81, 1.][step];
                let mut mask = layer_core::LayerMask::reveal_all(LayerId(99), Default::default());
                mask.default_coverage = 0.63;
                mask.show_area = step == 3;
                doc.layers[0].mask = (step > 0).then_some(mask);
                if step == 4 {
                    let mut foreground = doc.layers[1].clone();
                    foreground.id = LayerId(100);
                    foreground.opacity = 0.3;
                    doc.layers.insert(0, foreground);
                }
                let v = centered_view([doc.width, doc.height], [320, 240], 0.501, 0.12);
                r.scene.as_mut().unwrap().set_tiled_composition(false);
                submit(&mut r, &doc, v, true);
                let actual = present(&r, &mut presenter, v);
                r.scene.as_mut().unwrap().set_tiled_composition(true);
                submit(&mut r, &doc, v, true);
                close(&actual, &present(&r, &mut presenter, v));
                if step == 4 { doc.layers.remove(0); }
            }
        }
    }
}

#[test]
fn filter_working_allowance_does_not_expand_unfiltered_caches() {
    let mut doc = document([777, 533]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().composition_bytes = 1536 << 20;
    let source_limits = r.scene.as_ref().unwrap().source_cache_limits();
    let v = centered_view([doc.width, doc.height], [320, 240], 0.501, 0.12);
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    submit(&mut r, &doc, v, true);
    let unpainted = present(&r, &mut presenter, v);
    let mut dab = crate::tests::test_dab([310., 230.], [0.8, 0.04, 0.2, 1.], 1.);
    dab.radii = [90.; 2];
    let batch = DabBatch {
        material_update: 0, stroke_id: StrokeId(1), layer_id: doc.layers[0].id,
        kind: DabBatchKind::Persistent, stroke_start: true, stroke_end: true,
        first_dab: 0, dab_count: 1, damage: dab.bounds(),
        style: crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),
    };
    r.submit(FramePacket {
        layers: &doc.layers, document_extent: [doc.width, doc.height], view: v,
        time_seconds: 0., dabs: &[dab], dab_batches: &[batch],
        restore_rasters: &[], reset_layers: false, composite_all: false,
    }).unwrap();
    let original = present(&r, &mut presenter, v);
    assert_ne!(original, unpainted);
    assert!(!r.live_display.as_ref().unwrap().is_complete());
    doc.layers.insert(0, crate::tests::image_windows::effect(20, false, false));
    submit(&mut r, &doc, v, false);
    assert!(r.live_display.as_ref().unwrap().borrowed_image);
    doc.layers.remove(0);
    submit(&mut r, &doc, v, false);
    assert!(!r.live_display.as_ref().unwrap().is_complete());
    close(&original, &present(&r, &mut presenter, v));
    assert_eq!(source_limits, r.scene.as_ref().unwrap().source_cache_limits());
    assert_eq!(r.native_edit.as_ref().unwrap().display_complete_bytes, 0);
}

#[test]
fn display_batches_preserve_direct_and_fallback_submission_bounds() {
    for complete in [false, true] {
        let cases = if complete {
            let batch = display_mips::CompleteUpdates::BATCH as u32;
            [[8, 1], [16, batch / 16], [17, batch / 16], [16, batch / 8], [9, 1]]
        } else {
            [[8, 1], [9, 1], [16, 1], [17, 1], [32, 1]]
        };
        for [columns, rows] in cases {
            let tiles = columns * rows;
            let doc = layer_core::Document::new("paper batches", columns * PAGE_SIZE, rows * PAGE_SIZE);
            let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
            r.set_complete_display_allowance(if complete { u64::MAX } else { 0 });
            // Constant paper can write directly on every device, including
            // Float32 devices without attachment blending.
            let batch = if complete {
                display_mips::CompleteUpdates::BATCH
            } else {
                SOURCE_SLOTS / 2
            } as u32;
            let v = ViewState {
                width_px: columns * 8,
                height_px: rows * 8,
                ..view([1. / 32., 0., 0., 1. / 32., 0., 0.])
            };
            submit(&mut r, &doc, v, true);
            assert_eq!(r.live_display.as_ref().unwrap().complete_updates.is_some(), complete);
            assert_eq!(r.metrics.display_composition_submissions, u64::from((tiles - 1) / batch));
            assert_eq!(r.metrics.composited_pixels, u64::from(doc.width) * u64::from(doc.height));
            let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
                SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
            for pixel in present(&r, &mut presenter, v) {
                assert!(pixel.into_iter().all(|channel| (channel - 1.).abs() < 5e-6));
            }
        }
    }
}

#[test]
fn repeated_wide_composition_reuses_decoded_sources_with_exact_pixels_and_bounded_memory() {
    // More unique source tiles than the decoded cache can hold. Partial edges
    // and translucent pixels expose stale scratch or reordered layer blending.
    let mut doc = document([17 * PAGE_SIZE + 3, 4 * PAGE_SIZE + 7]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let v = centered_view([doc.width, doc.height], [320, 240], 0.06, 0.12);
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    submit(&mut r, &doc, v, true);
    let original = present(&r, &mut presenter, v);
    let bytes = r.scene.as_ref().unwrap().scratch_bytes();
    let tiles = u64::from(doc.width.div_ceil(PAGE_SIZE) * doc.height.div_ceil(PAGE_SIZE));
    for _ in 0..4 {
        let before = r.scene.as_ref().unwrap().source_cache_work();
        submit(&mut r, &doc, v, true);
        assert_eq!(original, present(&r, &mut presenter, v));
        let after = r.scene.as_ref().unwrap().source_cache_work();
        assert!(after[1] - before[1] < tiles / 2,
            "unchanged sources should survive repeated wider-than-cache compositions: {} misses / {tiles} tiles", after[1] - before[1]);
        assert!(r.scene.as_ref().unwrap().scratch_bytes() <= bytes);
    }
    // The same reuse must reflect an actual layer edit immediately.
    doc.layers[0].opacity = 0.;
    submit(&mut r, &doc, v, true);
    assert_ne!(original, present(&r, &mut presenter, v));
    doc.layers[0].opacity = 1.;
    submit(&mut r, &doc, v, true);
    assert_eq!(original, present(&r, &mut presenter, v));
}

#[test]
fn direct_mip_tile_composition_matches_scratch_through_layer_and_view_changes() {
    // Translucent U16 source layers, partial edge tiles and a nonzero backdrop
    // expose incomplete clears, wrong local coordinates and double blending.
    let mut doc = document([777, 533]);
    let mut upper = doc.layers[0].clone();
    upper.id = doc.allocate_layer_id();
    upper.opacity = 0.43;
    doc.layers.insert(0, upper);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    for step in 0..4 {
        if step == 1 { doc.layers[0].opacity = 0.; }
        if step == 2 {
            doc.layers[0].opacity = 0.7;
            let mut mask = layer_core::LayerMask::reveal_all(LayerId(99), Default::default());
            mask.default_coverage = 0.;
            mask.initial = Some(layer_core::Selection::polygon(vec![
                layer_core::Point { x: 0., y: 0. },
                layer_core::Point { x: 640., y: 0. },
                layer_core::Point { x: 440., y: 533. },
                layer_core::Point { x: 0., y: 533. },
            ]).unwrap());
            doc.layers[0].mask = Some(mask);
        }
        if step == 3 { doc.layers[0].properties.offset = layer_core::Point { x: 17., y: -9. }; }
        for scale in [0.25, 1., 2.] {
            let v = centered_view([doc.width, doc.height], [320, 240], scale, 0.12);
            if let Some(scene) = &mut r.scene { scene.set_tiled_composition(false); }
            submit(&mut r, &doc, v, true);
            let actual = present(&r, &mut presenter, v);
            let cache = r.live_display.as_ref().unwrap();
            assert!(!cache.is_complete());
            let (texture, _) = cache.composition_target();
            assert_eq!([texture.width(), texture.height()], [PAGE_SIZE; 2]);
            let bytes = r.metrics.composite_storage_bytes;
            r.scene.as_mut().unwrap().set_tiled_composition(true);
            submit(&mut r, &doc, v, true);
            close(&actual, &present(&r, &mut presenter, v));
            assert_eq!(r.metrics.composite_storage_bytes, bytes);
        }
    }
}

#[test]
fn sparse_contact_prediction_retirement_matches_full_recomposition() {
    let doc = document([1537, 1025]);
    let mut incremental = bounded_renderer(doc.color).unwrap();
    let mut reference = bounded_renderer(doc.color).unwrap();
    for r in [&mut incremental, &mut reference] {
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
        r.set_complete_display_allowance(64 * 1024 * 1024);
    }
    let mut a = ViewportPresenter::for_surface(&incremental, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&reference, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let v = centered_view([doc.width, doc.height], [320, 240], 0.2, 0.12);
    let style = crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    for (frame, (center, radius, preview)) in [
        ([310., 330.], 285., false),
        ([780., 520.], 355., true),
        ([1230., 710.], 235., true),
        ([650., 210.], 190., true),
        ([650., 210.], 190., false),
    ].into_iter().enumerate() {
        let mut dab = crate::tests::test_dab(center, [0.8, 0.04, 0.2, 0.7], 1.);
        dab.radii = [radius; 2];
        dab.previous = [radius * 0.7, radius * 0.7, 1., 0.];
        dab.motion = [320., -130.];
        dab.contact = [1., 0., 0., 0.];
        dab.previous_contact = [0.7, 0., 0., 0.];
        let batch = DabBatch {
            material_update: 0, stroke_id: StrokeId(1), layer_id: doc.layers[0].id,
            kind: if preview { DabBatchKind::Preview } else { DabBatchKind::Persistent },
            stroke_start: frame == 0, stroke_end: frame == 4, first_dab: 0, dab_count: 1,
            style: style.clone(), damage: dab.bounds(),
        };
        for (r, all) in [(&mut incremental, frame == 0), (&mut reference, true)] {
            r.submit(FramePacket {
                layers: &doc.layers, document_extent: [doc.width, doc.height], view: v,
                time_seconds: 0., dabs: &[dab], dab_batches: std::slice::from_ref(&batch),
                restore_rasters: &[], reset_layers: frame == 0, composite_all: all,
            }).unwrap();
        }
        assert_eq!(present(&incremental, &mut a, v), present(&reference, &mut b, v), "frame {frame}");
    }
    assert!(incremental.metrics.composited_pixels < reference.metrics.composited_pixels);
    for r in [&mut incremental, &mut reference] { submit(r, &doc, v, false); }
    assert_eq!(present(&incremental, &mut a, v), present(&reference, &mut b, v));
}

fn centered_view(extent: [u32; 2], viewport: [u32; 2], scale: f32, angle: f32) -> ViewState {
    let (sin, cos) = angle.sin_cos();
    let a = scale * cos;
    let b = scale * sin;
    let [x, y] = extent.map(|v| v as f32 * 0.5);
    ViewState {
        width_px: viewport[0], height_px: viewport[1],
        ..view([a, b, -b, a, viewport[0] as f32 * 0.5 - a*x + b*y,
            viewport[1] as f32 * 0.5 - b*x - a*y])
    }
}

#[test]
fn complete_admission_respects_texture_extent_even_with_free_memory() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    r.set_complete_display_allowance(u64::MAX);
    r.document_extent = [r.device.limits().max_texture_dimension_2d + 1, 16];
    let pipelines = display_mips::Pipelines::new(&r.device);
    let cache = Cache::new(&r, &pipelines, CACHE_BYTES, r.native_edit.as_ref().unwrap().display_complete_bytes).unwrap();
    assert!(cache.retained.iter().all(|level| level.level > 0));
    assert!(cache.retained.iter().all(|level|
        level.texture.width() <= r.device.limits().max_texture_dimension_2d));
    assert!(cache.storage_bytes() <= CACHE_BYTES);
}

#[test]
fn unchanged_hidpi_navigation_keeps_reserved_detail_slots() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    let pipelines = display_mips::Pipelines::new(&r.device);
    for extent in [[8192, 7324], [9504, 6336]] {
        r.document_extent = extent;
        let mut cache = Cache::new(&r, &pipelines, CACHE_BYTES, r.native_edit.as_ref().unwrap().display_complete_bytes).unwrap();
        let mut previous = 0;
        for (scale, angle) in [(1., 0.), (1., 0.2), (2., 0.2), (1., 0.), (0.5, 0.)] {
            let v = centered_view(extent, [2752, 2064], scale, angle);
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            cache.prepare(&mut r, v, &mut encoder).unwrap();
            let slots = cache.fine.as_ref().unwrap().keys.len();
            assert!(slots >= previous,
                "unchanged viewport shrank its detail cache: {extent:?}, {scale}, {previous} -> {slots}");
            assert!(cache.storage_bytes() <= CACHE_BYTES);
            previous = slots;
        }
    }
}

#[test]
fn partial_admission_preserves_reduced_levels_during_hidpi_zoom() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    r.document_extent = [9504, 6336];
    let allowance = 768 * 1024 * 1024;
    r.set_complete_display_allowance(allowance);
    let pipelines = display_mips::Pipelines::new(&r.device);
    let mut cache = Cache::new(&r, &pipelines, CACHE_BYTES, r.native_edit.as_ref().unwrap().display_complete_bytes).unwrap();
    assert!(cache.retained.iter().all(|level| level.level > 0));
    let original_mips = cache.retained_bytes();
    for scale in [2., 1., 0.50001, 0.25, 0.125, 0.50001] {
        for angle in [0., 0.12, 0.7, 1.2] {
            let v = centered_view(r.document_extent, [2752, 2064], scale, angle);
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            cache.prepare(&mut r, v, &mut encoder).unwrap();
            assert!(cache.storage_bytes() <= allowance);
            assert_eq!(cache.retained_bytes(), original_mips,
                "unused admitted memory must preserve completed zoomed-out pixels");
        }
    }
}

#[test]
fn large_rotated_hidpi_views_fit_the_original_display_budget() {
    let mut r = bounded_renderer(DocumentColor {
        space: RgbSpace::ProPhoto, depth: SampleDepth::U16,
    }).unwrap();
    r.document_extent = [9504, 6336];
    let pipelines = display_mips::Pipelines::new(&r.device);
    let mut cache = Cache::new(&r, &pipelines, CACHE_BYTES, r.native_edit.as_ref().unwrap().display_complete_bytes).unwrap();
    let original_mips = cache.retained_bytes();
    for viewport in [[2400, 1800], [3840, 2160]] {
        for scale in [0.50001, 0.6, 0.75, 1., 0.25, 2.] {
            for step in 0..72 {
                let v = centered_view(r.document_extent, viewport, scale,
                    step as f32 * std::f32::consts::TAU / 72.);
                let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
                cache.prepare(&mut r, v, &mut encoder).unwrap();
                assert!(cache.storage_bytes() <= CACHE_BYTES);
                if let Some(fine) = &cache.fine {
                    assert!(cache.visible.len() <= fine.keys.len());
                    let unique = cache.visible.iter().map(|&coordinate|
                        fine.slots[&(cache.window.unwrap().level, coordinate)]).collect::<BTreeSet<_>>();
                    assert_eq!(unique.len(), cache.visible.len());
                }
            }
        }
    }
    assert!(cache.retained_bytes() < original_mips, "optional mips yield to visible detail");
}

#[test]
fn atlas_omits_rotated_corners_without_changing_visible_pixels() {
    let doc = document([1537, 1025]);
    let mut dense = bounded_renderer(doc.color).unwrap();
    // A complete reference atlas uses the same Float32 bilinear arithmetic.
    // Hardware bilinear weights on an ordinary dense texture are quantized,
    // so they are not a bit-precision oracle at arbitrary rotation angles.
    dense.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    dense.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let mut cached = bounded_renderer(doc.color).unwrap();
    cached.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    cached.native_edit.as_mut().unwrap().display_cache_bytes = 16 * 1024 * 1024;
    let mut a = ViewportPresenter::for_surface(&dense, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&cached, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    for (index, angle) in [0.7, 1., -0.8, 2., 0.7].into_iter().enumerate() {
        let v = centered_view([doc.width, doc.height], [1024, 128], 1., angle);
        submit(&mut dense, &doc, ViewState {
            width_px: doc.width, height_px: doc.height,
            ..view([1., 0., 0., 1., 0., 0.])
        }, index == 0);
        submit(&mut cached, &doc, v, index == 0);
        let cache = cached.live_display.as_ref().unwrap();
        let bounds = cache.window.unwrap().tiles;
        assert!(cache.visible.len() < (bounds.width() * bounds.height()) as usize);
        assert!(cache.storage_bytes() <= 16 * 1024 * 1024);
        close(&present(&dense, &mut a, v), &present(&cached, &mut b, v));
    }
}

#[test]
fn growing_the_display_atlas_preserves_completed_native_pixels() {
    let doc = document([1537, 1025]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let v = view([4., 0., 0., 4., 0., 0.]);
    submit(&mut r, &doc, v, true);
    let bytes = r.live_display.as_ref().unwrap().storage_bytes();
    let work = r.metrics.composited_pixels;
    submit(&mut r, &doc, ViewState { width_px: 960, height_px: 720, ..v }, false);
    assert!(r.live_display.as_ref().unwrap().storage_bytes() > bytes);
    assert_eq!(r.metrics.composited_pixels, work,
        "growing slots must not regenerate already completed visible pixels");
}

#[test]
fn complete_display_matches_bounded_pixels_and_never_recomposes_for_navigation() {
    let mut doc = document([1537, 1025]);
    // Include a full-resolution physical filter. Reusing its completed output
    // must not change the filter's input resolution or evaluate it on navigation.
    doc.layers.insert(0, crate::tests::image_windows::effect(20, false, false));
    let mut full = bounded_renderer(doc.color).unwrap();
    full.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    full.set_complete_display_allowance(64 * 1024 * 1024);
    let mut bounded = bounded_renderer(doc.color).unwrap();
    bounded.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut a = ViewportPresenter::for_surface(&full, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&bounded, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let first = centered_view([doc.width, doc.height], [320, 240], 1., 0.);
    for r in [&mut full, &mut bounded] { submit(r, &doc, first, true); }
    let cache = full.live_display.as_ref().unwrap();
    assert_eq!(cache.retained[0].level, 0);
    assert!(cache.fine.is_none());
    assert!(cache.storage_bytes() < 64 * 1024 * 1024,
        "admission allowance is not an allocation target");
    for edit in [false, true] {
        if edit {
            doc.layers[0].opacity = 0.4;
            for r in [&mut full, &mut bounded] { submit(r, &doc, first, true); }
        }
        let work = full.metrics.composited_pixels;
        let misses = full.metrics.source_tile_misses;
        for scale in [0.125, 0.5, 0.501, 1., 2., 0.25] {
            for angle in [0., 0.7, 2., -1.] {
                let v = centered_view([doc.width, doc.height], [320, 240], scale, angle);
                for r in [&mut full, &mut bounded] { submit(r, &doc, v, false); }
                let direct = present(&full, &mut a, v);
                let atlas = present(&bounded, &mut b, v);
                // Display filtering uses quantized hardware interpolation weights
                // for contiguous textures. Bound the visible SDR difference to one
                // output code; native backing and composite values remain exact.
                let srgb = |v: f32| if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
                let mut largest = 0_f32;
                for (a, b) in direct.iter().zip(&atlas) {
                    for c in 0..3 { largest = largest.max((srgb(a[c]) - srgb(b[c])).abs()); }
                    assert_eq!(a[3], b[3]);
                }
                assert!(largest <= 1. / 255., "display interpolation error {largest} at scale {scale}, angle {angle}");
                assert_eq!(full.metrics.composited_pixels, work);
                assert_eq!(full.metrics.source_tile_misses, misses);
            }
        }
    }
}

#[test]
fn complete_display_batches_match_scratch_reduction_exactly_through_edits() {
    // Odd edges, transparency and 136 tiles exercise weighted reduction,
    // multiple complete/partial batches and every retained mip level.
    let mut doc = document([4097, 1793]);
    let mut direct = bounded_renderer(doc.color).unwrap();
    let mut reference = bounded_renderer(doc.color).unwrap();
    for r in [&mut direct, &mut reference] {
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
        r.set_complete_display_allowance(256 * 1024 * 1024);
        r.ensure_document([doc.width, doc.height], &doc.layers).unwrap();
        assert!(r.live_display.as_ref().unwrap().complete_updates.is_some());
    }
    // Keep the existing scratch implementation as an independent pixel oracle.
    reference.live_display.as_mut().unwrap().complete_updates = None;
    let v = centered_view([doc.width, doc.height], [320, 240], 0.125, 0.);
    let levels = |r: &WgpuRasterizer| {
        let cache = r.live_display.as_ref().unwrap();
        cache.retained.iter().map(|level| &level.texture)
            .chain(std::iter::once(&cache.coarse.texture))
            .map(|texture| pixels(r, texture)).collect::<Vec<_>>()
    };
    for opacity in [1., 0.35, 0., 0.7, 1.] {
        doc.layers[0].opacity = opacity;
        for r in [&mut direct, &mut reference] { submit(r, &doc, v, true); }
        assert_eq!(levels(&direct), levels(&reference));
        let cache = direct.live_display.as_ref().unwrap();
        assert!(cache.storage_bytes() <= cache.limit);
    }
}

#[test]
fn camera_rotation_preserves_power_of_two_detail_levels() {
    let plan = display_mips::Plan::new([8192, 7324]).unwrap();
    for level in 0..plan.level {
        let scale = 1. / (1u32 << level) as f32;
        for step in 0..1440 {
            let angle = step as f32 * std::f32::consts::TAU / 1440.;
            let (sin, cos) = angle.sin_cos();
            for reflect in [1., -1.] {
                let window = Window::new(view([
                    reflect * scale * cos, reflect * scale * sin,
                    -scale * sin, scale * cos, 160., 120.,
                ]), plan).unwrap().unwrap();
                assert_eq!(window.level, level, "level={level} angle={angle} reflect={reflect}");
            }
        }
        if level > 0 {
            // A real zoom crossing is not held at the coarser level. Include
            // nonuniform transforms: the largest stretch controls resolution.
            let larger = scale * 1.00001;
            let window = Window::new(view([larger, 0., 0., scale, 0., 0.]), plan)
                .unwrap().unwrap();
            assert_eq!(window.level, level - 1);
        }
    }
}

#[test]
fn retained_mips_match_windowed_pixels_and_zoomed_out_edits_invalidate_native_detail() {
    let mut doc = document([1537, 769]);
    let mut retained = bounded_renderer(doc.color).unwrap();
    let mut windowed = bounded_renderer(doc.color).unwrap();
    for r in [&mut retained, &mut windowed] {
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    }
    windowed.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let mut a = ViewportPresenter::for_surface(&retained, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&windowed, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let native = view([1., 0., 0., 1., -700., -300.]);
    for r in [&mut retained, &mut windowed] { submit(r, &doc, native, true); }
    assert!(!retained.live_display.as_ref().unwrap().retained.is_empty());
    assert!(windowed.live_display.as_ref().unwrap().retained.is_empty());
    let work = retained.metrics.composited_pixels;
    for matrix in [
        [0.5, 0., 0., 0.5, 0., 0.],
        [0.5, 0., 0., 0.5, -550., -150.],
        [0., 0.5, -0.5, 0., 350., -200.],
        [0.25, 0., 0., 0.25, 0., 0.],
    ] {
        let v = view(matrix);
        for r in [&mut retained, &mut windowed] { submit(r, &doc, v, false); }
        close(&present(&retained, &mut a, v), &present(&windowed, &mut b, v));
        assert_eq!(retained.metrics.composited_pixels, work,
            "retained display levels must not re-evaluate the document for navigation");
    }
    // The old native window is not visible during this edit. Returning to it
    // must reject stale detail, including when the new view uses retained mips.
    doc.layers[0].opacity = 0.35;
    let zoomed_out = view([0.25, 0., 0., 0.25, 0., 0.]);
    for r in [&mut retained, &mut windowed] {
        submit(r, &doc, zoomed_out, true);
        submit(r, &doc, native, false);
    }
    let mut dense = bounded_renderer(doc.color).unwrap();
    let mut c = ViewportPresenter::for_surface(&dense, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    submit(&mut dense, &doc, native, true);
    let expected = present(&dense, &mut c, native);
    close(&expected, &present(&retained, &mut a, native));
    close(&expected, &present(&windowed, &mut b, native));
    // Force native cache misses without an artwork edit, including the partial
    // bottom/right source page. Native detail must match full composition while
    // already-completed coarse and retained levels remain bit-identical.
    let levels = |r: &WgpuRasterizer| {
        let cache = r.live_display.as_ref().unwrap();
        std::iter::once(&cache.coarse.texture)
            .chain(cache.retained.iter().map(|level| &level.texture))
            .map(|texture| pixels(r, texture))
            .collect::<Vec<_>>()
    };
    let before = levels(&retained);
    for v in [native, view([1., 0., 0., 1., -1350., -650.])] {
        retained.live_display.as_mut().unwrap().fine.as_mut().unwrap().keys.fill(None);
        let work = retained.metrics.composited_pixels;
        submit(&mut retained, &doc, v, false);
        submit(&mut dense, &doc, v, false);
        assert!(retained.metrics.composited_pixels > work);
        close(&present(&dense, &mut c, v), &present(&retained, &mut a, v));
        assert_eq!(before, levels(&retained));
    }
}

fn document(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new("bounded live display", extent[0], extent[1]);
    doc.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::with_capacity(extent[0] as usize * 8);
        for x in 0..extent[0] {
            for v in [
                ((x * 53 + y * 17) % 60000) as u16,
                ((x * 7 + y * 31) % 50000) as u16,
                (5000 + (x * 3 + y * 19) % 55000) as u16,
                40000,
            ] {
                row.extend_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row).unwrap();
    }
    doc.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    doc
}
fn view(matrix: [f32; 6]) -> ViewState {
    ViewState {
        width_px: 320,
        height_px: 240,
        document_to_surface: matrix,
        background_rgba_linear: [1.; 4],
    }
}
fn submit(r: &mut WgpuRasterizer, doc: &layer_core::Document, view: ViewState, all: bool) {
    r.submit(FramePacket {
        layers: &doc.layers,
        document_extent: [doc.width, doc.height],
        view,
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: all,
    })
    .unwrap();
}
fn pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}
fn present(
    r: &WgpuRasterizer,
    presenter: &mut ViewportPresenter,
    view: ViewState,
) -> Vec<[f32; 4]> {
    let (texture, target) = create_target(
        &r.device,
        [view.width_px, view.height_px],
        wgpu::TextureFormat::Rgba32Float,
        "bounded display oracle target",
    );
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    pixels(r, &texture)
}
fn close(a: &[[f32; 4]], b: &[[f32; 4]]) {
    assert_eq!(a.len(), b.len());
    for (pixel, (a, b)) in a.iter().zip(b).enumerate() {
        for i in 0..4 {
            assert!(
                (a[i] - b[i]).abs() < 5e-6,
                "pixel {pixel}, channel {i}: {} != {}",
                a[i],
                b[i]
            );
        }
    }
}

#[test]
fn visible_detail_matches_dense_composition_through_pan_wrap_rotation_and_resize() {
    let doc = document([1537, 769]);
    let mut dense = bounded_renderer(doc.color).unwrap();
    let mut cached = bounded_renderer(doc.color).unwrap();
    cached.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &cached,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    for (index, matrix) in [
        [1., 0., 0., 1., 0., 0.],
        [1., 0., 0., 1., -100., -100.],
        [1., 0., 0., 1., -700., -450.],
        [1., 0., 0., 1., -1200., -529.],
        [1., 0., 0., 1., -256., -256.],
        [0., 1., -1., 0., 600., -700.],
        [-1., 0., 0., 1., 1200., -300.],
        [2., 0., 0., 2., -1111., -555.],
        [0.5, 0., 0., 0.5, -233., -100.],
        [1., 0., 0., 1., 0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        let view = view(matrix);
        submit(&mut dense, &doc, view, index == 0);
        submit(&mut cached, &doc, view, index == 0);
        assert!(cached.composite_texture.is_none());
        assert!(cached.live_display.as_ref().unwrap().storage_bytes() <= CACHE_BYTES);
        close(
            &present(&dense, &mut a, view),
            &present(&cached, &mut b, view),
        );
        if index > 0 {
            assert_eq!(
                dense.composite_revision, cached.composite_revision,
                "camera motion is not an artwork change"
            );
        }
    }
    let revision = cached.composite_revision;
    let work = cached.metrics.composited_pixels;
    submit(&mut cached, &doc, view([1., 0., 0., 1., -1., 0.]), false);
    assert_eq!(
        cached.metrics.composited_pixels, work,
        "resident pan must reuse detail"
    );
    assert_eq!(cached.composite_revision, revision);
    assert!(
        cached
            .request_color_sample(ColorSampleRequest {
                request_id: 11,
                source: ColorSampleSource::Composite,
                position: [255, 256],
                area: ColorSampleArea::Average5,
            })
            .unwrap()
    );
    cached
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let sample = cached.take_color_sample().unwrap().unwrap();
    assert!(sample.rgba.iter().all(|v| v.is_finite()));
    assert!(cached.request_canvas_preview(None).unwrap());
    cached
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    assert!(
        cached
            .take_canvas_preview()
            .unwrap()
            .unwrap()
            .image
            .is_some()
    );
}

#[test]
fn large_live_composite_uses_bounded_pixels_and_zoom_out_matches_full_area_reference() {
    let doc = document([4097, 1025]);
    let mut cached = bounded_renderer(doc.color).unwrap();
    cached.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let v = view([0.25, 0., 0., 0.25, -256., 0.]);
    submit(&mut cached, &doc, v, true);
    assert!(
        cached.composite_texture.is_none(),
        "the default threshold must choose bounded storage"
    );
    // The constrained cache keeps a complete coarse image and visible detail;
    // it does not allocate the optional retained levels or seed native detail.
    assert!(cached.metrics.composite_storage_bytes < 16 * 1024 * 1024);
    let mut dense = bounded_renderer(doc.color).unwrap();
    dense.native_edit.as_mut().unwrap().display_dense_bytes = u64::MAX;
    submit(&mut dense, &doc, v, true);
    let working = pixels(&dense, dense.composite_texture.as_ref().unwrap());
    let mut expected = Vec::new();
    for y in 0..v.height_px {
        for x in 0..v.width_px {
            let mut rgb = [0f64; 3];
            for yy in y * 4..y * 4 + 4 {
                for xx in 1024 + x * 4..1024 + x * 4 + 4 {
                    for i in 0..3 {
                        rgb[i] += working[(yy * doc.width + xx) as usize][i] as f64 / 16.;
                    }
                }
            }
            let rgb = layer_core::color::rgb::apply(
                RgbSpace::DisplayP3.linear_transform(RgbSpace::Srgb),
                rgb,
            );
            expected.push([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.]);
        }
    }
    let mut presenter = ViewportPresenter::for_surface(
        &cached,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&expected, &present(&cached, &mut presenter, v));
}

#[test]
fn rejected_views_and_abandoned_cache_writes_preserve_artwork_and_rebuild_missing_detail() {
    let doc = document([1537, 769]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().display_cache_bytes = 8 * 1024 * 1024;
    let v = view([1., 0., 0., 1., 0., 0.]);
    submit(&mut r, &doc, v, true);
    let mut presenter = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let before = present(&r, &mut presenter, v);
    let bytes = r.live_display.as_ref().unwrap().storage_bytes();
    let rejected = r
        .submit(FramePacket {
            layers: &doc.layers,
            document_extent: [doc.width, doc.height],
            view: ViewState {
                width_px: 2048,
                height_px: 1536,
                ..v
            },
            time_seconds: 0.,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: false,
        })
        .unwrap_err();
    assert!(rejected.to_string().contains("cache limit"));
    assert_eq!(r.live_display.as_ref().unwrap().storage_bytes(), bytes);
    close(&before, &present(&r, &mut presenter, v));

    let mut cache = r.live_display.take().unwrap();
    let source = create_color_target(&r.device, [PAGE_SIZE; 2], "abandoned display overwrite");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.encode_clear_value(
        &mut encoder,
        &source.1,
        "unsubmitted white display tile",
        1.,
    );
    cache
        .write_tile(
            &r,
            r.display_pipelines.as_ref().unwrap(),
            &mut encoder,
            &source.0,
            [0; 2],
            [0; 2],
        )
        .unwrap();
    drop(encoder);
    r.live_display = Some(cache);
    let work = r.metrics.composited_pixels;
    submit(&mut r, &doc, v, false);
    assert_eq!(
        r.metrics.composited_pixels - work,
        u64::from(PAGE_SIZE).pow(2)
    );
    close(&before, &present(&r, &mut presenter, v));
    let mut dense = bounded_renderer(doc.color).unwrap();
    submit(&mut dense, &doc, v, true);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        dense.readback_srgb_rgba8().unwrap(),
        "explicit readback captures artwork independently of either display cache"
    );
    close(&before, &present(&r, &mut presenter, v));
}

#[test]
fn cache_changes_shape_within_budget_and_rebinds_retired_presenter_views() {
    let doc = document([1537, 769]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().display_cache_bytes = 8 * 1024 * 1024;
    let mut dense = bounded_renderer(doc.color).unwrap();
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    // Revisit retired views across many completed command-pool lifetimes,
    // including Android's occasional reclamation of retained driver storage.
    for (index, [width_px, height_px]) in [[1024, 240], [240, 768], [1024, 240]]
        .into_iter()
        .cycle()
        .take(384)
        .enumerate()
    {
        let v = ViewState {
            width_px,
            height_px,
            ..view([1., 0., 0., 1., 0., 0.])
        };
        submit(&mut dense, &doc, v, index == 0);
        submit(&mut r, &doc, v, index == 0);
        assert!(r.live_display.as_ref().unwrap().storage_bytes() <= 8 * 1024 * 1024);
        close(&present(&dense, &mut a, v), &present(&r, &mut b, v));
    }
}

#[test]
fn filtered_masked_source_edits_and_restoration_refresh_detail_and_coarse_display() {
    use layer_core::{LayerMask, Point, Selection};
    let mut doc = document([777, 533]);
    let source = doc.layers[0].source.clone().unwrap();
    let mut mask = LayerMask::reveal_all(LayerId(99), Point { x: 7., y: -9. });
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 760., y: 99. },
            Point { x: 440., y: 533. },
        ])
        .unwrap(),
    );
    doc.layers[0].mask = Some(mask);
    doc.layers
        .insert(0, crate::tests::image_windows::effect(20, false, false));
    doc.layers
        .insert(0, crate::tests::image_windows::effect(21, false, false));
    let original = doc.clone();
    let mut dense = bounded_renderer(doc.color).unwrap();
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let v = view([1., 0., 0., 1., -233., -111.]);
    let mut first = Vec::new();
    for step in 0..5 {
        match step {
            1 => doc.layers[2].properties.offset = Point { x: 17., y: -9. },
            2 => {
                doc.layers[2].mask.as_mut().unwrap().inverted = true;
                doc.layers[0].opacity = 0.6;
            }
            3 => doc.layers[2].mask.as_mut().unwrap().show_area = true,
            4 => doc = original.clone(),
            _ => {}
        }
        // Layer property actions request recomposition through FramePacket;
        // only camera motion uses an otherwise clean packet.
        submit(&mut dense, &doc, v, true);
        submit(&mut r, &doc, v, true);
        let displayed = present(&r, &mut b, v);
        close(&present(&dense, &mut a, v), &displayed);
        if step == 0 {
            first = displayed;
        } else if step == 4 {
            close(&first, &displayed);
        } else {
            assert!(
                first != displayed,
                "step {step} must change visible artwork"
            );
        }
        // Both preview routes must reflect the same revised artwork, including
        // masked physical filter halos and the whole-image inspection display.
        for renderer in [&mut dense, &mut r] {
            assert!(renderer.request_canvas_preview(None).unwrap());
            renderer
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
        }
        let expected = dense.take_canvas_preview().unwrap().unwrap().image.unwrap();
        let actual = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
        assert!(
            expected.bytes == actual.bytes,
            "coarse preview at step {step}"
        );
        assert!(Arc::ptr_eq(doc.layers[2].source.as_ref().unwrap(), &source));
    }
    // Recreating the GPU cache reconstructs the same pixels from retained source.
    let mut recovered = bounded_renderer(doc.color).unwrap();
    recovered.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    submit(&mut recovered, &doc, v, true);
    let mut p = ViewportPresenter::for_surface(
        &recovered,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&first, &present(&recovered, &mut p, v));
}

#[test]
fn in_surface_navigator_matches_bounded_preview_and_keeps_clipped_geometry() {
    let doc = document([1537, 769]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    submit(&mut r, &doc, view([1., 0., 0., 1., -900., -400.]), true);
    assert!(r.request_canvas_preview(None).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let preview = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
    let size = [preview.width, preview.height];
    let (texture, target) = create_target(
        &r.device,
        size,
        wgpu::TextureFormat::Rgba8Unorm,
        "coarse native Navigator",
    );
    let mut presenter = ViewportPresenter::for_overviews(&r, wgpu::TextureFormat::Rgba8Unorm);
    for clipped in [false, true] {
        presenter.set_overviews(
            &r,
            &[OverviewPlacement {
                bounds: [0., 0., size[0] as f32, size[1] as f32],
                clip: clipped.then_some([13., 17., (size[0] - 31) as f32, (size[1] - 37) as f32]),
                work_area: [[-1000.; 2]; 4],
                outline_linear: [0.; 3],
                background_linear: [1.; 3],
                scale: 1.,
                opacity: 1.,
            }],
        );
        presenter.present_overviews(&r, &target, size).unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &texture);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let i = ((y * size[0] + x) * 4) as usize;
                if clipped && (!(13..size[0] - 18).contains(&x) || !(17..size[1] - 20).contains(&y))
                {
                    assert_eq!(&actual[i..i + 4], [0; 4]);
                } else {
                    for c in 0..4 {
                        assert!(
                            actual[i + c].abs_diff(preview.bytes[i + c]) <= 1,
                            "Navigator {x},{y} channel {c}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn large_document_waits_for_mip_compilation_before_reporting_canvas_ready() {
    use std::time::{Duration, Instant};
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let mut doc = layer_core::Document::new("large staged document", 4097, 1025);
    doc.color = color;
    let mut r = bounded_renderer(color).unwrap();
    r.startup = Some(startup::Startup::new(&r.device).unwrap());
    let (release, wait) = mpsc::channel();
    let (entered, blocked) = mpsc::channel();
    let compiler = &r.startup.as_ref().unwrap().compiler;
    compiler.enqueue(0, move || {
        entered.send(()).map_err(|e| e.to_string())?;
        wait.recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())
    });
    compiler.start();
    blocked.recv_timeout(Duration::from_secs(20)).unwrap();
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    r.prepare_startup(&doc, &brush, false).unwrap();
    assert!(!r.poll_startup().unwrap().canvas_ready);
    assert!(!r.display_pipelines.as_ref().unwrap().reduce.ready());
    assert!(!r.display_pipelines.as_ref().unwrap().fused_reduce.ready());
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !r.poll_startup().unwrap().canvas_ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(r.display_pipelines.as_ref().unwrap().reduce.ready());
    assert!(r.display_pipelines.as_ref().unwrap().fused_reduce.ready());
    submit(&mut r, &doc, view([1., 0., 0., 1., 0., 0.]), true);
    assert!(r.live_display.is_some() && r.composite_texture.is_none());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}

#[test]
fn coalesced_contact_composition_matches_a_full_rebuild_without_repainting_its_empty_center() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ToolKind, ViewTransform, input_queue};
    let doc = document([4096, 3072]);
    let mut r = bounded_renderer(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let v = view([1. / 16., 0., 0., 1. / 16., 0., 0.]);
    let (mut input, consumer) = input_queue(64);
    let mut engine = CanvasEngine::new(r, doc, consumer, v,
        ViewTransform { revision: 0, surface_to_document: [16., 0., 0., 16., 0., 0.] }).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    brush.diameter = 48.;
    engine.set_brush(brush).unwrap();
    engine.render_frame().unwrap();
    engine.backend_mut().wait_idle().unwrap();
    let before = engine.backend().metrics().composited_pixels;
    for i in 0..33 {
        let angle = i as f32 / 32. * std::f32::consts::TAU;
        input.push(PenEvent {
            device_id: 1, sequence: i + 1, timestamp_ns: (i + 1) * 4_166_667,
            view_revision: 0,
            surface_position: layer_core::Point { x: 128. + 112. * angle.cos(), y: 96. + 80. * angle.sin() },
            pressure: 0.65, tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
            phase: if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move },
            tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
        }).unwrap();
    }
    loop {
        engine.render_frame().unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() { break; }
    }
    let changed = engine.backend().metrics().composited_pixels - before;
    assert!(changed > 0 && changed < 4096 * 3072 / 2,
        "a narrow circle must not recompose its untouched center: {changed}");
    let mut presenter = ViewportPresenter::for_surface(engine.backend(),
        wgpu::TextureFormat::Rgba32Float, SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let actual = present(engine.backend(), &mut presenter, v);
    let doc = engine.document().clone();
    submit(engine.backend_mut(), &doc, v, true);
    close(&actual, &present(engine.backend(), &mut presenter, v));
}

#[test]
fn committed_contact_strokes_present_their_canonical_native_pixels() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ToolKind, ViewTransform, input_queue};
    for preset in [layer_core::DefaultBrushPreset::AntiquePen, layer_core::DefaultBrushPreset::BrushedInk] {
        let doc = layer_core::Document::new("canonical contact", 1024, 512);
        let v = view([0.5, 0., 0., 0.5, 0., 0.]);
        let (mut input, consumer) = input_queue(64);
        let mut engine = CanvasEngine::new(bounded_renderer(doc.color).unwrap(), doc, consumer, v,
            ViewTransform { revision: 0, surface_to_document: [2., 0., 0., 2., 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
        let mut brush = layer_core::default_brush(preset);
        brush.diameter = 70.;
        brush.color_rgba_linear = [0.08, 0.015, 0.25, 1.];
        engine.set_brush(brush).unwrap();
        for i in 0..=32 {
            let t = i as f32 / 32.;
            input.push(PenEvent {
                device_id: 1, sequence: i + 1, timestamp_ns: (i + 1) * 8_333_333,
                view_revision: 0,
                surface_position: layer_core::Point { x: 40. + 240. * t, y: 120. + 50. * (t * std::f32::consts::TAU).sin() },
                pressure: if i < 32 { 0.2 + 0.7 * (t * std::f32::consts::PI).sin() } else { 0. },
                tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
                phase: if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move },
                tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
            }).unwrap();
            loop {
                engine.render_frame().unwrap();
                engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() { break; }
            }
        }
        assert_eq!(engine.metrics().committed_strokes, 1);
        let mut presenter = ViewportPresenter::for_surface(engine.backend(),
            wgpu::TextureFormat::Rgba32Float, SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let live = present(engine.backend(), &mut presenter, v);
        let doc = engine.document().clone();
        submit(engine.backend_mut(), &doc, v, true);
        assert!(live == present(engine.backend(), &mut presenter, v),
            "{preset:?}: the committed stroke must present the pixels Undo/Redo and reopening restore");
    }
}

#[test]
fn native_stroke_undo_redo_and_replaced_device_rebuild_visible_tiles_from_exact_backing() {
    use layer_engine::{
        CanvasEngine, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform, input_queue,
    };
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut doc = layer_core::Document::new("bounded native drawing", 1025, 513);
    doc.color = color;
    let v = view([1., 0., 0., 1., 0., 0.]);
    let renderer = || {
        let mut r = bounded_renderer(color).unwrap();
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
        r
    };
    let (mut input, consumer) = input_queue(64);
    let mut engine =
        CanvasEngine::new(renderer(), doc, consumer, v, ViewTransform::IDENTITY).unwrap();
    let flush = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    };
    flush(&mut engine);
    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let blank = present(engine.backend(), &mut p, v);
    for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up]
        .into_iter()
        .enumerate()
    {
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i as u64 + 1,
                timestamp_ns: (i as u64 + 1) * 10_000_000,
                view_revision: 0,
                surface_position: layer_core::Point {
                    x: 245. + i as f32 * 18.,
                    y: 80.,
                },
                pressure: 0.37,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        flush(&mut engine);
    }
    let painted = present(engine.backend(), &mut p, v);
    assert!(painted != blank);
    let root = engine.document().layers[0].raster.clone();
    let backed = root.wait_data().unwrap();
    assert!(
        backed.tiles.len() >= 2,
        "stroke crosses native tile boundaries"
    );
    let exact: Vec<_> = backed
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
        .collect();
    assert!(engine.undo().unwrap());
    flush(&mut engine);
    close(&blank, &present(engine.backend(), &mut p, v));
    assert!(engine.redo().unwrap());
    flush(&mut engine);
    close(&painted, &present(engine.backend(), &mut p, v));
    let retired = engine.replace_backend(renderer()).unwrap();
    retired.device.destroy();
    drop(retired);
    drop(p);
    flush(&mut engine);
    assert!(engine.backend().composite_texture.is_none());
    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&painted, &present(engine.backend(), &mut p, v));
    let restored = engine.document().layers[0].raster.wait_data().unwrap();
    for (key, bytes) in exact {
        assert_eq!(
            bytes,
            restored.tiles[&key]
                .wait_backing()
                .unwrap()
                .decode()
                .unwrap()
        );
    }
}
