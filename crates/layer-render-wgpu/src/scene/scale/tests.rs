use super::*;
use crate::test_support::{dab_batch, packet};
use layer_core::color::{SampleDepth, source::*};
use layer_core::{DefaultBrushPreset, Document};

#[path = "effect_tests.rs"]
mod effects;

#[path = "refinement_tests.rs"]
mod refinement;

fn assert_settled(r: &mut WgpuRasterizer, frame: FramePacket<'_>, reference: &[[f32; 4]]) {
    let revision = r.artwork_revision;
    let pages = page_coordinates(PixelRect::full(frame.document_extent)).count();
    let idle = FramePacket { composite_all: false, reset_layers: false, dabs: &[], dab_batches: &[], restore_rasters: &[], ..frame };
    for step in 0..=pages + 1 {
        if !r.has_pending_work() { break; }
        assert!(step < pages + 1, "refinement must finish within one visit per native page");
        let work = r.metrics.composited_pixels;
        r.submit(idle).unwrap();
        assert!(r.metrics.composited_pixels - work <= u64::from(PAGE_SIZE).pow(2));
        assert_eq!(r.artwork_revision, revision, "idle refinement does not edit artwork");
    }
    let cache = r.scale_display.as_ref().unwrap();
    let error = quality(&display_pixels(r), reference, cache.plan);
    assert!(error[2] < 2e-5, "settled display {error:?}");
    if let Some(overview) = &cache.overview {
        let error = quality(&pixels(r, overview.texture()), reference, overview.plan);
        assert!(error[2] < 2e-5, "settled overview {error:?}");
    }
    assert_presentation_mip(r);
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
    let work = r.metrics.composited_pixels;
    r.submit(idle).unwrap();
    assert_eq!(r.metrics.composited_pixels, work, "completed refinement has no further work");
}

fn document() -> Document {
    document_at([517, 259])
}
fn document_at(extent: [u32; 2]) -> Document {
    let mut doc = Document::new("display composition oracle", extent[0], extent[1]);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        extent[0] as usize * extent[1] as usize * 16,
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
fn idle_display_converges_to_exact_composition_after_edits() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document();
        let extent = [doc.width, doc.height];
        doc.layers[0].source = Some(layer_core::color::source::rgba8_source(extent, |x, y|
            [if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }, 128, 70, 255]));
        let mut top = Layer::paint(LayerId(90), "correlated coverage");
        top.source = Some(layer_core::color::source::rgba8_source(extent, |x, y|
            [180, 20, 100, if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }]));
        doc.layers.insert(0, top);
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        for (step, opacity) in [0.8, 0.3, 0.9].into_iter().enumerate() {
            doc.layers[0].opacity = opacity;
            let mut frame = packet(&doc.layers, extent);
            frame.blend_space = space;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let reference = pixels(&exact, exact.composite_texture.as_ref().unwrap());
            let before = quality(&display_pixels(&r), &reference, r.scale_display.as_ref().unwrap().plan);
            assert!(before[2] > 1e-4, "fixture must need refinement: {before:?}");
            if step == 0 {
                r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
                assert!(r.has_pending_work(), "the next edit interrupts partial refinement");
                continue;
            }
            assert_settled(&mut r, frame, &reference);
        }
    }
}

#[test]
fn blend_space_changes_refresh_branches_and_source_representations() {
    use layer_core::{Affine, BlendSpace};
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let photo = doc.layers[0].clone();
    doc.layers = (0..12).map(|i| {
        let mut layer = photo.clone();
        layer.id = LayerId(100 + i);
        layer.opacity = 0.15 + i as f32 * 0.04;
        layer
    }).collect();
    doc.layers[0].properties.blend = layer_core::LayerBlend::SoftLight;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut fresh = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let shifted = Affine([1., 0., 0., 1., 8., -4.]);
    for (step, (space, placement, level)) in [
        (BlendSpace::Linear, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 3),
        (BlendSpace::Linear, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, shifted, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 0),
        (BlendSpace::Linear, Affine::IDENTITY, 0),
    ].into_iter().enumerate() {
        doc.layers[0].properties.placement = placement;
        let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
        frame.composite_all = matches!(step, 3 | 4);
        frame.view.background_rgba_linear = [0.17, 0.39, 0.81, 0.7];
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(frame).unwrap();
        fresh.submit(FramePacket { reset_layers: true, ..frame }).unwrap();
        exact.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        let actual = display_pixels(&r);
        let expected = display_pixels(&fresh);
        let error = actual.iter().flatten().zip(expected.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max);
        assert!(error < 2e-5, "step={step} {space:?} {placement:?} level={level}: {error}");
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.graph.storage_bytes() > 0);
        let sources = &r.scene.as_ref().unwrap().scale_sources;
        let encoding = if placement == Affine::IDENTITY { space } else { BlendSpace::Linear };
        assert_eq!(sources.entries[&doc.layers[0].id].blend_space, encoding);
        if encoding == BlendSpace::Perceptual {
            assert!(sources.complete_texture(&doc.layers[0], extent, level).is_none());
        }
        if level == 0 {
            let reference = pixels(&exact, exact.composite_texture.as_ref().unwrap());
            let error = actual.iter().flatten().zip(reference.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max);
            assert!(error < 2e-5, "native {space:?}: {error}");
        }
        let mut actual = vec![0; extent[0] as usize * extent[1] as usize * 4];
        let mut expected = actual.clone();
        r.copy_rgba8_srgb(&mut actual, extent[0] as usize * 4).unwrap();
        exact.copy_rgba8_srgb(&mut expected, extent[0] as usize * 4).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn clipped_contacts_do_not_require_unmaterialized_source_levels() {
    let doc = Document::new("clipped contact", 517, 259);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let expected = display_pixels(&r);
    let dab = crate::tests::test_dab([10000., 10000.], [0.8, 0.2, 0.1, 1.], 1.);
    let batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    for kind in [DabBatchKind::Persistent, DabBatchKind::Preview] {
        let batch = DabBatch { kind, ..batch.clone() };
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert_eq!(display_pixels(&r), expected);
        assert!(r.paint_layers.iter().all(|layer| layer.pages.is_empty()));
        assert!(r.preview_pages.is_empty());
        assert!(r.live_display.is_none() && r.composite_texture.is_none());
    }
}

#[test]
fn native_graph_admits_a_full_4k_view_with_bounded_scratch() {
    let mut doc = document_at([65, 33]);
    doc.width = 4096;
    doc.height = 4096;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for count in [1, 32] {
        while doc.layers.len() <= count {
            let mut layer = doc.layers[0].clone();
            layer.id = LayerId(100 + doc.layers.len() as u64);
            layer.opacity = 0.35;
            doc.layers.insert(0, layer);
        }
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.view.width_px = doc.width;
        frame.view.height_px = doc.height;
        assert_eq!(level(&r, frame), Some(0));
        r.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        assert_eq!(cache.plan.level, 0);
        assert!(cache.graph.root.is_some());
        assert!(scene.scale_sources.entries.values().all(|s| !s.levels.contains_key(&0)));
        let bytes = cache.storage_bytes() + scene.scale_sources.storage_bytes()
            + scene.scale_commands.as_ref().unwrap().storage_bytes()
            + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>();
        assert!(bytes <= live_display::CACHE_BYTES, "layers={count}, bytes={bytes}");
        assert!(scene.used.iter().all(|used| !used));
    }
}

#[test]
fn reduced_photo_stacks_use_bounded_working_tiles() {
    let mut doc = document_at([65, 33]);
    doc.width = 9504; doc.height = 6336;
    for index in 0..4 {
        let mut layer = doc.layers[0].clone();
        layer.id = LayerId(40 + index);
        layer.opacity = 0.35;
        layer.properties.blend = layer_core::LayerBlend::SoftLight;
        doc.layers.insert(0, layer);
    }
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.width_px = 1500;
    frame.view.height_px = 1000;
    frame.view.document_to_surface = [0.1578, 0., 0., 0.1578, 0., 0.];
    assert_eq!(level(&r, frame), Some(2));
    r.submit(frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    assert_eq!(cache.output.len(), 1, "only the completed root needs a viewport image");
    assert!(scene.pool.iter().all(|p| p.texture.width() == PAGE_SIZE && p.texture.height() == PAGE_SIZE));
    let bytes = cache.storage_bytes() + scene.scale_sources.storage_bytes()
        + scene.scale_commands.as_ref().unwrap().storage_bytes()
        + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>();
    assert!(bytes <= live_display::CACHE_BYTES, "resident bytes={bytes}");
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
}

#[test]
fn partial_reduced_views_admit_the_visible_window() {
    let mut doc = document_at([65, 33]);
    doc.width = 9504;
    doc.height = 6336;
    let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for detail in [1, 2, 4] {
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.view.width_px = 960;
        frame.view.height_px = 640;
        let scale = 1. / (1 << detail) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, -1600. * scale, -900. * scale];
        assert_eq!(level(&r, frame), Some(detail));
        let plan = request(frame, detail).unwrap();
        assert!(plan.bounds.area() < PixelRect::full(plan.extent).area());
        assert!(allocation(&r, plan, frame, None).into_iter().sum::<u64>() <= live_display::CACHE_BYTES);
    }
}

#[test]
fn rotated_reduced_stacks_admit_bounded_source_working_storage() {
    let mut doc = document_at([65, 33]);
    doc.width = 9504; doc.height = 6336;
    for index in 0..4 {
        let mut layer = doc.layers[0].clone();
        layer.id = LayerId(40 + index);
        layer.opacity = 0.35;
        layer.properties.blend = layer_core::LayerBlend::SoftLight;
        doc.layers.insert(0, layer);
    }
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.width_px = 1600; frame.view.height_px = 1000;
    let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
    let scale = 0.307;
    frame.view.document_to_surface = [scale * c, scale * s, -scale * s, scale * c,
        800. - scale * (c * doc.width as f32 - s * doc.height as f32) * 0.5,
        500. - scale * (s * doc.width as f32 + c * doc.height as f32) * 0.5];
    assert_eq!(level(&r, frame), Some(1));
    r.submit(frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    let bytes = cache.storage_bytes() + scene.scale_sources.storage_bytes()
        + scene.scale_commands.as_ref().unwrap().storage_bytes()
        + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>();
    assert!(bytes <= live_display::CACHE_BYTES, "resident bytes={bytes}");
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
    assert!(scene.used.iter().all(|used| !used));
}

#[test]
fn streamed_sources_match_cached_pixels_through_masks_paint_and_admission_changes() {
    let mut doc = document_at([2053, 1541]);
    let extent = [doc.width, doc.height];
    for index in 0..63 {
        let mut layer = doc.layers[0].clone();
        layer.id = LayerId(40 + index);
        layer.opacity = 0.35;
        layer.properties.blend = layer_core::LayerBlend::SoftLight;
        doc.layers.insert(0, layer);
    }
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(200), Default::default());
    mask.initial = Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 280., y: 210. }, layer_core::Point { x: 1600., y: 260. },
        layer_core::Point { x: 1700., y: 1200. }, layer_core::Point { x: 330., y: 1100. },
    ]).unwrap());
    doc.layers[0].mask = Some(mask);
    let mut streamed = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut cached = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for state in 0..6 {
        for (i, layer) in doc.layers.iter_mut().enumerate() { layer.visible = state != 3 || i < 2; }
        doc.layers[0].mask.as_mut().unwrap().inverted = state == 2;
        let dab = crate::tests::test_dab([420. + state as f32 * 7., 310.], [0.8, 0.2, 0.1, 1.], 0.7);
        let mut batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state < 4 { batch.kind = DabBatchKind::Preview; }
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = (2..=4).contains(&state);
        frame.view.width_px = extent[0]; frame.view.height_px = extent[1];
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
        if state == 1 || state >= 4 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        streamed.submit(frame).unwrap();
        let full = streamed.scale_display.as_ref().unwrap();
        assert_eq!(full.streamed_sources, state != 3, "state {state}");
        let full_plan = full.plan;
        let complete = display_pixels(&streamed);
        frame.view.width_px = 192; frame.view.height_px = 128;
        let [x, y] = if state == 2 { [-910., -660.] } else { [-160., -110.] };
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, x, y];
        cached.submit(frame).unwrap();
        let window = cached.scale_display.as_ref().unwrap();
        assert!(!window.streamed_sources, "cached oracle state {state}");
        let actual = display_pixels(&cached);
        let origin = [window.plan.bounds.min_x() >> 1, window.plan.bounds.min_y() >> 1];
        for (i, pixel) in actual.iter().enumerate() {
            let x = origin[0] + i as u32 % window.plan.size[0];
            let y = origin[1] + i as u32 / window.plan.size[0];
            let expected = complete[(y * full_plan.size[0] + x) as usize];
            assert!(pixel.iter().zip(expected).all(|(a, b)| (a - b).abs() < 2e-5),
                "state {state}, [{x}, {y}]: {pixel:?} != {expected:?}");
        }
        let scene = streamed.scene.as_ref().unwrap();
        let bytes = full.storage_bytes() + scene.scale_sources.storage_bytes()
            + scene.scale_commands.as_ref().unwrap().storage_bytes()
            + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>();
        assert!(bytes <= live_display::CACHE_BYTES, "state {state}, bytes {bytes}");
        assert!(scene.used.iter().all(|used| !used));
        assert!(streamed.live_display.is_none() && streamed.composite_texture.is_none());
        assert_presentation_mip(&streamed);
    }
}

#[test]
fn streamed_sources_reuse_valid_finer_pages_without_native_decoding() {
    let doc = document_at([1027, 773]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let expected = display_pixels(&r);
    let mut cache = r.scale_display.take().unwrap();
    let mut scene = r.scene.take().unwrap();
    let mut commands = scene.scale_commands.take().unwrap();
    let source = scene.scale_sources.entries.get_mut(&doc.layers[0].id).unwrap();
    assert!(source.levels.contains_key(&2));
    source.levels.remove(&3);
    cache.streamed_sources = true;
    for missing in [None, Some([4, 3])] {
        let source = scene.scale_sources.entries.get_mut(&doc.layers[0].id).unwrap();
        if let Some(tile) = missing { assert!(source.levels.get_mut(&2).unwrap().valid.remove(&tile)); }
        let updates = source.updates;
        cache.graph = Default::default();
        cache.valid.clear();
        scene.begin_frame();
        commands.begin();
        cache.prepare_graph(&r, frame, &scene.scale_sources, &commands).unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        cache.render(&mut scene, &mut r, frame, PixelRect::full(frame.document_extent),
            &mut Encoding { encoder: &mut encoder, commands: &mut commands }, None).unwrap();
        commands.flush(&mut r, &mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        assert_eq!(scene.scale_sources.entries[&doc.layers[0].id].updates, updates + u64::from(missing.is_some()),
            "only gaps in retained finer pages may require native reduction");
        let actual = pixels(&r, cache.texture());
        assert!(actual.iter().zip(&expected).all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-5)));
    }
    scene.scale_commands = Some(commands);
    r.scene = Some(scene);
    r.scale_display = Some(cache);
    assert_presentation_mip(&r);
}

#[test]
fn source_window_growth_remains_admitted_during_rotated_pans() {
    let mut doc = document_at([65, 33]);
    doc.width = 9504; doc.height = 6336;
    for index in 0..4 {
        let mut layer = doc.layers[0].clone();
        layer.id = LayerId(40 + index);
        layer.opacity = 0.35;
        layer.properties.blend = layer_core::LayerBlend::SoftLight;
        doc.layers.insert(0, layer);
    }
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for step in 0..24 {
        let angle = step as f32 / 23. * std::f32::consts::TAU;
        let (s, c) = angle.sin_cos();
        let [x, y] = [(0.5 + 0.4 * c) * doc.width as f32, (0.5 + 0.4 * s) * doc.height as f32];
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.view.width_px = 1600; frame.view.height_px = 1000;
        frame.view.document_to_surface = [0.5 * c, 0.5 * s, -0.5 * s, 0.5 * c,
            800. - 0.5 * (c * x - s * y), 500. - 0.5 * (s * x + c * y)];
        assert_eq!(level(&r, frame), Some(1), "step {step}");
        r.submit(frame).unwrap();
        assert!(r.scale_display.is_some() && r.live_display.is_none());
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        let bytes = cache.storage_bytes() + scene.scale_sources.storage_bytes()
            + scene.scale_commands.as_ref().unwrap().storage_bytes()
            + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>();
        assert!(bytes <= live_display::CACHE_BYTES, "step {step}: {bytes}");
    }
}

#[test]
fn retained_windows_stream_sources_when_source_requirements_grow() {
    let mut doc = document_at([65, 33]);
    doc.width = 3584; doc.height = 3584;
    for index in 0..15 {
        let mut layer = doc.layers[0].clone();
        layer.id = LayerId(20 + index);
        layer.visible = false;
        doc.layers.push(layer);
    }
    let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut full = packet(&doc.layers, [doc.width, doc.height]);
    full.view.width_px = doc.width;
    full.view.height_px = doc.height;
    full.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
    let previous = request(full, 1).unwrap();
    assert_eq!(previous.bounds, PixelRect::full(previous.extent));
    assert_eq!(level(&r, full), Some(1));
    let old = Cache::new(&r, previous, doc.layers.len());
    for layer in &mut doc.layers { layer.visible = true; }
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.width_px = 192;
    frame.view.height_px = 128;
    frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
    assert_eq!(level(&r, frame), Some(1));
    assert!(allocation(&r, previous, frame, None).into_iter().sum::<u64>() > live_display::CACHE_BYTES);
    let (selected, _) = Cache::select(Some(old), &r, frame, 1, false);
    assert_eq!(selected.plan, previous);
    assert!(selected.streamed_sources);
    assert!(allocation_for(&r, selected.plan, frame, None, true).into_iter().sum::<u64>() <= live_display::CACHE_BYTES);
}

#[test]
fn view_windows_reuse_overlap_and_preserve_global_sampling() {
    let doc = document_at([2053, 1541]);
    let extent = [doc.width, doc.height];
    for detail in [0, 1, 2, 4] {
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut whole = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        exact.native_edit.as_mut().unwrap().display_dense_bytes = 0;
        exact.set_complete_display_allowance(256 << 20);
        let mut frame = packet(&doc.layers, extent);
        exact.submit(frame).unwrap();
        let oracle = pixels(&exact, exact.live_display.as_ref().unwrap().level_texture(0).unwrap());
        frame.composite_all = false;
        frame.view.width_px = 192;
        frame.view.height_px = 128;
        let mut presenter = crate::present::ViewportPresenter::for_surface(
            &r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let mut reference_presenter = crate::present::ViewportPresenter::for_surface(
            &whole, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let surface = |renderer: &WgpuRasterizer, presenter: &mut crate::present::ViewportPresenter, view| {
            let (texture, target) = create_color_target(&renderer.device, [192, 128], "composition viewport oracle");
            presenter.present(renderer, &target, view, [0.2; 4]).unwrap();
            pixels(renderer, &texture)
        };
        let views = [
            [1., 0., 0., 1., -300., -200.],
            [1., 0., 0., 1., -310., -210.],
            [1., 0., 0., 1., -750., -410.],
            [0., 1., -1., 0., 900., -510.],
            [-1.5, 0., 0., 0.75, 1700., -300.],
            [1., 0., 0., 1., -1900., -1430.],
        ];
        let mut work = 0;
        let mut revision = None;
        for (i, transform) in views.into_iter().enumerate() {
            let transform = transform.map(|n| n / (1 << detail) as f32);
            frame.view.document_to_surface = transform;
            r.submit(frame).unwrap();
            let current = r.canvas_preview_revision();
            assert!(revision.is_none_or(|previous| previous == current), "camera motion preserves artwork revision");
            revision = Some(current);
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, display_mips::view_level(transform, 4).unwrap());
            assert!(cache.graph.root.is_some());
            if detail == 0 { assert!(cache.plan.bounds.area() < PixelRect::full(extent).area() / 2); }
            if let Some(overview) = &cache.overview { assert!(overview.plan.level > cache.plan.level); }
            assert!(r.live_display.is_none() && r.composite_texture.is_none());
            assert!(quality(&display_pixels(&r), &oracle, cache.plan)[2] < 2e-5, "view={i}");
            assert_presentation_mip(&r);
            if i == 1 { assert_eq!(r.metrics.composited_pixels, work); }
            if i == 2 { assert!(r.metrics.composited_pixels - work < cache.plan.size.map(u64::from).into_iter().product()); }
            work = r.metrics.composited_pixels;
            let actual = surface(&r, &mut presenter, frame.view);
            let scale = 1. / (1 << cache.plan.level) as f32;
            let mut full = packet(&doc.layers, extent);
            full.view.width_px = extent[0];
            full.view.height_px = extent[1];
            full.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            whole.submit(full).unwrap();
            let complete = whole.scale_display.as_ref().unwrap();
            assert_eq!(complete.plan.bounds, PixelRect::full(extent));
            assert_eq!(complete.plan.level, cache.plan.level);
            assert!(quality(&display_pixels(&whole), &oracle, complete.plan)[2] < 2e-5);
            let reference = surface(&whole, &mut reference_presenter, frame.view);
            let error = actual.iter().flatten().zip(reference.iter().flatten()).map(|(a,b)| (a-b).abs()).fold(0., f32::max);
            assert!(error < 0.0001, "window presentation detail={detail} view={i} plan={:?} error={error}", cache.plan);
        }
        let dab = crate::tests::test_dab([1980., 1480.], [0.9, 0.2, 0.1, 1.], 0.7);
        let batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        frame.dabs = std::slice::from_ref(&dab);
        frame.dab_batches = std::slice::from_ref(&batch);
        r.submit(frame).unwrap();
        assert!(r.canvas_preview_revision() > revision.unwrap(), "painting advances artwork revision");
        exact.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let oracle = pixels(&exact, exact.live_display.as_ref().unwrap().level_texture(0).unwrap());
        assert!(quality(&display_pixels(&r), &oracle, cache.plan)[2] < 2e-5);
        if let Some(overview) = &cache.overview {
            assert!(quality(&pixels(&r, overview.texture()), &oracle, overview.plan)[2] < 2e-5);
        }
        assert_presentation_mip(&r);
    }
}

#[test]
fn small_placed_source_remains_visible_beyond_its_local_extent() {
    let mut doc = document_at([256, 256]);
    let extent = [2048, 1536];
    doc.width = extent[0]; doc.height = extent[1];
    doc.layers[0].properties.placement = layer_core::Affine([1., 0., 0., 1., 896., 640.]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.385, 0., 0., 0.385, 0., 0.];
    r.submit(frame).unwrap();
    let display = display_pixels(&r);
    assert!(display.iter().any(|p| p[3] > 0.99 && p[2] > 0.05), "placed source contributes to presentation");
    let mut exact = vec![0; (extent[0] * extent[1] * 4) as usize];
    r.copy_rgba8_srgb(&mut exact, extent[0] as usize * 4).unwrap();
    let center = ((768 * extent[0] + 1024) * 4) as usize;
    assert_eq!(&exact[center..center + 4], &[42, 64, 80, 255]);
}

#[test]
fn placed_sources_and_masks_compose_in_document_scale_and_keep_exact_queries() {
    let source_extent = [517, 259];
    let extent = [389, 277];
    let mut doc = document_at(source_extent);
    doc.width = extent[0];
    doc.height = extent[1];
    doc.layers[0].opacity = 0.71;
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(40), layer_core::Point { x: 17., y: -11. });
    mask.default_coverage = 0.63;
    mask.initial = Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 30., y: 10. }, layer_core::Point { x: 390., y: 10. },
        layer_core::Point { x: 390., y: 200. }, layer_core::Point { x: 30., y: 200. },
    ]).unwrap());
    doc.layers[0].mask = Some(mask);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for (pose, placement) in [
        [0.5, 0., 0., 0.5, 16., 32.], [-0.5, 0., 0., 0.5, 320., 32.],
        [0.6, 0.2, -0.1, 0.5, 30., 4.], [0.35, 0.1, 0.2, 0.75, -17., 21.],
    ].into_iter().enumerate() {
        doc.layers[0].properties.placement = layer_core::Affine(placement);
        doc.layers[0].mask.as_mut().unwrap().inverted = pose % 2 != 0;
        for enabled in [true, false] {
            doc.layers[0].mask.as_mut().unwrap().enabled = enabled;
            for level in [0, 1, 2, 3] {
                let scale = 1. / (1 << level) as f32;
                let mut frame = packet(&doc.layers, extent);
                frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
                r.submit(frame).unwrap();
                exact.submit(frame).unwrap();
                let cache = r.scale_display.as_ref().unwrap();
                assert_eq!(cache.plan.level, level);
                assert!(r.live_display.is_none() && r.composite_texture.is_none());
                let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
                eprintln!("placed pose={pose} level={level} mask={enabled}: {error:?}");
                assert!(error[0] < 0.004 && error[1] < 0.04, "pose={pose} level={level}: {error:?}");
                assert_presentation_mip(&r);
                let mut actual = vec![0; (extent[0] * extent[1] * 4) as usize];
                let mut expected = actual.clone();
                r.copy_rgba8_srgb(&mut actual, extent[0] as usize * 4).unwrap();
                exact.copy_rgba8_srgb(&mut expected, extent[0] as usize * 4).unwrap();
                assert!(actual == expected, "exact placed export pose={pose} level={level} mask={enabled}");
            }
        }
    }
}

#[test]
fn source_windows_keep_overlap_and_sample_global_coordinates() {
    let source_extent = [4099, 2053];
    let mut doc = document_at(source_extent);
    let extent = [513, 257];
    doc.width = extent[0]; doc.height = extent[1];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut updates = 0;
    let mut previous = None;
    for (step, offset) in [[-2050., -1000.], [-2100., -1000.], [-2350., -1000.], [-2100., -1000.], [-3575., -1790.], [700., 400.]].into_iter().enumerate() {
        doc.layers[0].properties.placement = layer_core::Affine::translation(layer_core::Point { x: offset[0], y: offset[1] });
        let mut frame = packet(&doc.layers, extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        assert!(r.live_display.is_none() && r.composite_texture.is_none());
        let sources = &r.scene.as_ref().unwrap().scale_sources;
        assert!(sources.storage_bytes() < 8 << 20, "small output retains bounded source windows: {}", sources.storage_bytes());
        let source = &sources.entries[&doc.layers[0].id];
        eprintln!("source window step={step} bytes={} native_page_updates={}", sources.storage_bytes(), source.updates);
        let images: Vec<_> = source.levels.values().map(|l| l.image.texture.clone()).collect();
        if step == 1 || step == 3 {
            assert_eq!(source.updates, updates, "motion inside the retained window reuses source pixels");
            assert_eq!(previous.as_ref().unwrap(), &images, "covered motion retains the same textures");
        }
        previous = Some(images);
        updates = source.updates;
        let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
        assert!(error[0] < 0.004 && error[1] < 0.04, "step={step}: {error:?}");
        assert_presentation_mip(&r);
    }
    doc.layers[0].properties.placement = layer_core::Affine([0.5, 0., 0., 0.5, -100., -100.]);
    let frame = packet(&doc.layers, extent);
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), r.scale_display.as_ref().unwrap().plan);
    assert!(error[0] < 0.004 && error[1] < 0.04, "native placement after a partial source: {error:?}");
}

#[test]
fn cold_identity_sources_prepare_adjacent_detail_without_redecoding() {
    let doc = document_at([1025, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.width_px = 512; frame.view.height_px = 512;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&doc.layers[0].id];
    let updates = source.updates;
    assert_eq!(updates, 20);
    assert!(source.levels.contains_key(&2) && source.levels.contains_key(&3));
    let finer = source.levels[&2].image.texture.clone();
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    r.submit(frame).unwrap();
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&doc.layers[0].id];
    assert_eq!(source.updates, updates);
    assert_eq!(source.levels[&2].image.texture, finer);
    assert!(!source.levels.contains_key(&1), "scale changes must not repeatedly promote optional detail");
}

#[test]
fn optional_source_detail_yields_to_unallocated_required_images() {
    let mut doc = document_at([1024, 1024]);
    doc.width = 128; doc.height = 128;
    doc.layers[0].properties.placement = layer_core::Affine([0.125, 0., 0., 0.125, 0., 0.]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.submit(packet(&doc.layers, [doc.width, doc.height])).unwrap();
    let mut scene = r.scene.take().unwrap();
    let source = &scene.scale_sources.entries[&doc.layers[0].id];
    assert_eq!(source.updates, 16, "cold detail and its required level decode each native page once");
    assert!(source.levels.contains_key(&1) && source.levels.contains_key(&2));
    let mut second = doc.layers[0].clone();
    second.id = LayerId(20);
    second.opacity = 0.5;
    doc.layers.insert(0, second);
    let frame = packet(&doc.layers, [doc.width, doc.height]);
    scene.scale_sources.prepare(&r, frame);
    let requested = r.scale_display.as_ref().unwrap().source_levels(&r, frame, &scene.scale_sources);
    let budget = requested.values().flat_map(|levels| levels.values()).map(|p| p.level_bytes(p.level)).sum();
    assert_eq!(scene.scale_sources.retain_levels(&requested, budget), budget);
    assert!(scene.scale_sources.entries.values().all(|s| !s.levels.contains_key(&1)));
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for layer in &doc.layers {
        if let Some(levels) = requested.get(&layer.id) {
            for &plan in levels.values() {
                scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, layer,
                    SourceRequest { plan, required: plan.bounds, covered: PixelRect::EMPTY }).unwrap();
            }
        }
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    assert_eq!(scene.scale_sources.storage_bytes(), budget);
    assert!(requested.iter().all(|(id, levels)| levels.keys().all(|level|
        scene.scale_sources.entries[id].levels.contains_key(level))));
}

#[test]
fn source_retention_reserves_images_that_composition_allocates_later() {
    let mut doc = document_at([33, 17]);
    doc.width = 4096; doc.height = 4096;
    let mut front = doc.layers[0].clone();
    front.id = LayerId(20);
    front.opacity = 0.5;
    doc.layers.insert(0, front);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    let cache = Cache::new(&r, display_mips::Plan::at(frame.document_extent, 2), doc.layers.len());
    assert!(cache.output.is_empty() && cache.next.is_none());
    let source_budget = cache.source_budget(&r, frame, &Commands::new(&r), None);
    r.scale_display = Some(cache);
    r.submit(frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    assert!(!cache.output.is_empty() && cache.next.is_some());
    let commands = r.scene.as_ref().unwrap().scale_commands.as_ref().unwrap();
    assert!(source_budget + cache.storage_bytes() + commands.storage_bytes() <= live_display::CACHE_BYTES);
}

#[test]
fn source_windows_derive_across_origins_and_fill_only_missing_pages() {
    let doc = document_at([1027, 773]);
    let extent = [doc.width, doc.height];
    let id = doc.layers[0].id;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    r.submit(frame).unwrap();
    let reference = pixels(&r, r.scale_display.as_ref().unwrap().texture());
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let mut scene = r.scene.take().unwrap();
    let full = PixelRect::full(extent);
    let window = PixelRect::new(256, 256, extent[0], extent[1]);
    for (fine, coarse) in [(full, window), (window, full)] {
        let mut commands = Commands::new(&r);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, &doc.layers[0], SourceRequest {
            plan: display_mips::Plan::window(extent, 1, fine), required: fine, covered: PixelRect::EMPTY,
        }).unwrap();
        scene.scale_sources.entries.get_mut(&id).unwrap().levels.remove(&2);
        let plan = display_mips::Plan::window(extent, 2, coarse);
        let updates = scene.scale_sources.entries[&id].updates;
        scene.scale_sources.ensure_level(&mut commands, &mut r, &mut encoder, id, plan).unwrap();
        let complete = scene.scale_sources.image(id, 2).valid.len();
        assert_eq!(complete, page_coordinates(fine.intersect(coarse)).count());
        assert_eq!(scene.scale_sources.entries[&id].updates, updates);
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, &doc.layers[0], SourceRequest {
            plan, required: coarse, covered: PixelRect::EMPTY,
        }).unwrap();
        assert_eq!(scene.scale_sources.entries[&id].updates - updates, (page_coordinates(coarse).count() - complete) as u64);
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let actual = pixels(&r, &scene.scale_sources.image(id, 2).image.texture);
        assert!(quality(&actual, &reference, plan)[2] < 1e-6);
        assert_eq!(scene.scale_sources.complete_texture(&doc.layers[0], extent, 2).is_some(), coarse == full);
    }
}

#[test]
fn deferred_placement_samples_the_final_surface_without_a_canvas_image() {
    let mut doc = document_at([1025, 513]);
    let extent = [641, 385];
    doc.width = extent[0]; doc.height = extent[1];
    doc.layers[0].opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut exact_presenter = crate::ViewportPresenter::for_surface(
        &exact, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut presenter = crate::ViewportPresenter::for_surface(
        &r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let render = |r: &WgpuRasterizer, presenter: &mut crate::ViewportPresenter, view: layer_render::ViewState| {
        let (texture, target) = create_color_target(&r.device, [view.width_px, view.height_px], "deferred presentation oracle");
        presenter.present(r, &target, view, [1.; 4]).unwrap();
        pixels(r, &texture)
    };
    for placement in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]] {
        doc.layers[0].properties.placement = layer_core::Affine(placement);
        for camera in [[0.25, 0., 0., 0.25, 8.25, 7.5], [0.19, 0., 0., 0.19, 8.25, 7.5], [0.13, 0., 0., 0.13, 8.25, 7.5],
            [0.17, 0.075, -0.075, 0.17, 37.5, 6.25], [0.14, -0.06, 0.02, 0.24, 8.25, 42.5]] {
            let mut frame = packet(&doc.layers, extent);
            frame.view.background_rgba_linear = [1.; 4];
            frame.view.width_px = 192; frame.view.height_px = 128;
            frame.view.document_to_surface = camera;
            let inverse = layer_core::Affine(camera).inverse().unwrap();
            let inside = |x, y| {
                let p = inverse.map(layer_core::Point { x, y });
                p.x >= 0. && p.y >= 0. && p.x < extent[0] as f32 && p.y < extent[1] as f32
            };
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            assert!(r.scale_display.as_ref().unwrap().output.is_empty());
            assert!(r.scale_display.as_ref().unwrap().next.is_none());
            let actual = render(&r, &mut presenter, frame.view);
            let image = materialized_display(&r);
            let mut cache = r.scale_display.take().unwrap();
            let root = cache.placed.take().unwrap();
            cache.output = vec![image]; cache.used = vec![false]; cache.selected = 0;
            let mut commands = Commands::new(&r);
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            cache.reduce_output(&mut r, &mut encoder, PixelRect::full(cache.plan.size), &mut commands).unwrap();
            r.uploads.finish(&encoder); encoder.submit(&r.queue);
            r.scale_display = Some(cache);
            let materialized = render(&r, &mut presenter, frame.view);
            let mut high = frame.view;
            high.width_px *= 8; high.height_px *= 8;
            high.document_to_surface = high.document_to_surface.map(|n| n * 8.);
            let reference = render(&exact, &mut exact_presenter, high);
            let point = render(&exact, &mut exact_presenter, frame.view);
            let expected: Vec<[f32; 4]> = (0..frame.view.height_px).flat_map(|y| (0..frame.view.width_px).map(move |x| (x, y))).map(|(x, y)| {
                if !inside(x as f32 + 0.5, y as f32 + 0.5) {
                    return point[(y * frame.view.width_px + x) as usize];
                }
                let mut value = [0.; 4];
                let mut count = 0.;
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    if !inside((xx as f32 + 0.5) / 8., (yy as f32 + 0.5) / 8.) { continue; }
                    for c in 0..4 { value[c] += reference[(yy * high.width_px + xx) as usize][c]; }
                    count += 1.;
                }}
                value.map(|c| c / count)
            }).collect();
            let cache = r.scale_display.as_mut().unwrap();
            cache.placed = Some(root); cache.output.clear(); cache.used.clear(); cache.next = None;
            let mut errors: Vec<_> = actual.iter().zip(&expected).map(|(a,b)| a.iter().zip(b).map(|(a,b)| (a-b).abs()).fold(0., f32::max)).collect();
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            let mut prior: Vec<_> = materialized.iter().zip(&expected).map(|(a,b)| a.iter().zip(b).map(|(a,b)| (a-b).abs()).fold(0., f32::max)).collect();
            prior.sort_by(f32::total_cmp);
            eprintln!("deferred placement={placement:?} camera={camera:?}: mean={mean} p99={p99} max={}; materialized mean={} p99={}", errors.last().unwrap(), prior.iter().sum::<f32>() / prior.len() as f32, prior[prior.len() * 99 / 100]);
            assert!(mean < 0.004 && p99 < 0.04);
        }
    }
}

#[test]
fn deferred_placement_navigator_matches_supersampled_exact_artwork() {
    let mut doc = document_at([1025, 513]);
    doc.width = 641; doc.height = 385;
    doc.layers[0].opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let overview = |r: &WgpuRasterizer, size: [u32; 2]| {
        let mut presenter = crate::ViewportPresenter::for_overview_surface(
            r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        presenter.set_overviews(r, &[crate::OverviewPlacement {
            bounds: [0., 0., size[0] as f32, size[1] as f32], clip: None,
            work_area: [[-1000.; 2]; 4], outline_linear: [0.; 3], background_linear: [1.; 3], scale: 1., opacity: 1.,
        }]);
        let (texture, target) = create_color_target(&r.device, size, "placement navigator oracle");
        presenter.present_overviews(r, &target, size).unwrap();
        pixels(r, &texture)
    };
    for placement in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]] {
        doc.layers[0].properties.placement = layer_core::Affine(placement);
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.view.background_rgba_linear = [1.; 4];
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 8.25, 7.5];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().placed.is_some());
        for size in [[128, 77], [64, 39], [32, 19], [16, 10], [7, 4]] {
            let actual = overview(&r, size);
            let high = size.map(|n| n * 8);
            let reference = overview(&exact, high);
            let mut errors = Vec::new();
            for y in 0..size[1] { for x in 0..size[0] {
                let mut expected = [0.; 4];
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    for c in 0..4 { expected[c] += reference[(yy * high[0] + xx) as usize][c] / 64.; }
                }}
                errors.push(actual[(y * size[0] + x) as usize].iter().zip(expected).map(|(a,b)| (a-b).abs()).fold(0., f32::max));
            }}
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            assert!(mean < 0.004 && p99 < 0.04, "navigator placement={placement:?} size={size:?}: mean={mean} p99={p99}");
        }
    }
}

#[test]
fn deriving_partial_sources_preserves_completed_texels_between_refreshed_regions() {
    let doc = document_at([769, 769]);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let mut scene = r.scene.take().unwrap();
    let id = doc.layers[0].id;
    let expected = pixels(&r, &scene.scale_sources.image(id, 3).image.texture);
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for tile in [[0, 0], [3, 3]] {
        scene.scale_sources.entries.get_mut(&id).unwrap().levels.get_mut(&3).unwrap().valid.remove(&tile);
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, &doc.layers[0],
            SourceRequest { plan: display_mips::Plan::at(extent, 1), required: page_rect(tile), covered: PixelRect::EMPTY }).unwrap();
    }
    scene.scale_sources.ensure_level(&mut commands, &mut r, &mut encoder, id, display_mips::Plan::at(extent, 3)).unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    let actual = pixels(&r, &scene.scale_sources.image(id, 3).image.texture);
    assert!(actual.iter().flatten().zip(expected.iter().flatten()).all(|(a,b)| (a-b).abs() < 1e-6));
    assert_eq!(scene.scale_sources.image(id, 3).valid.len(), 16);
}

#[test]
fn placed_compact_prediction_keeps_the_most_magnified_source_axis() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for placement in [[2., 0., 0., 1., -300., -20.], [-2., 0., 0., 0.25, 600., 80.]] {
        doc.layers[0].properties.placement = layer_core::Affine(placement);
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        let baseline = display_pixels(&r);
        let mut dab = crate::tests::test_dab([200., 110.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [32.; 2];
        let mut batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        batch.kind = DabBatchKind::Preview;
        batch.style.brush_to_layer = doc.layers[0].properties.placement.inverse().unwrap();
        r.submit(FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        assert_eq!(r.preview_level, 1);
        assert_ne!(display_pixels(&r), baseline);
        r.submit(frame).unwrap();
        let actual = display_pixels(&r);
        let changed: Vec<_> = actual.iter().zip(&baseline).enumerate().filter(|(_, (a,b))| a != b).collect();
        let error = actual.iter().flatten().zip(baseline.iter().flatten()).map(|(a,b)| (a-b).abs()).fold(0., f32::max);
        assert!(changed.is_empty(), "pose={placement:?} differing={} max={error} first={:?}", changed.len(), changed.first());
    }
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
            assert_settled(&mut reduced, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
        }
    }
}

#[test]
fn cached_branches_recompose_logarithmic_work_and_preserve_untouched_regions() {
    let mut doc = document();
    let photo = doc.layers[0].clone();
    doc.layers = (0..32).map(|i| {
        let mut layer = photo.clone();
        layer.id = LayerId(100 + i);
        layer.opacity = 0.17 + i as f32 * 0.02;
        layer
    }).chain(doc.layers.last().cloned()).collect();
    doc.layers.insert(15, Layer::paint(LayerId(200), "empty paint target"));
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    fn frame(layers: &[Layer], extent: [u32; 2]) -> FramePacket<'_> {
        let mut p = packet(layers, extent);
        p.composite_all = false;
        p.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        p
    }
    r.submit(frame(&doc.layers, extent)).unwrap();
    exact.submit(frame(&doc.layers, extent)).unwrap();
    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&LayerId(200)));
    let record_bound = page_coordinates(PixelRect::full(extent)).count() as u32
        + doc.layers.len().next_power_of_two().ilog2() + 1;
    for (step, index) in [15, 0, 32, 15, 0].into_iter().enumerate() {
        let mut dab = crate::tests::test_dab([30. + step as f32 * 100., 97.], [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [21.; 2];
        let batch = dab_batch(doc.layers[index].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch { kind, ..batch.clone() };
            let p = FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame(&doc.layers, extent) };
            r.submit(p).unwrap();
            exact.submit(p).unwrap();
            let scene = r.scene.as_ref().unwrap();
            let records = scene.scale_commands.as_ref().unwrap().cursor;
            assert!(records <= record_bound, "32 photos, first paint on an empty layer and end edits must reuse the other branches: step={step} {kind:?}, {records} > {record_bound}");
            let cache = r.scale_display.as_ref().unwrap();
            let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
            assert!(error[0] < 0.003 && error[1] < 0.025, "step={step} kind={kind:?}: {error:?}");
            assert_presentation_mip(&r);
            assert!(cache.storage_bytes() + scene.scale_sources.storage_bytes() + scene.scale_commands.as_ref().unwrap().storage_bytes() <= live_display::CACHE_BYTES);
        }
    }
    for step in 0..4 {
        match step {
            0 => doc.layers[15].opacity = 0.91,
            1 => doc.layers[0].visible = false,
            2 => doc.layers.swap(15, 31),
            _ => doc.layers[15].source = Some(Arc::new((**photo.source.as_ref().unwrap()).clone())),
        }
        r.submit(frame(&doc.layers, extent)).unwrap();
        exact.submit(frame(&doc.layers, extent)).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), cache.plan);
        assert!(error[0] < 0.003 && error[1] < 0.025, "metadata step={step}: {error:?}");
    }
}

#[test]
fn identity_edits_recompose_only_damaged_pages() {
    let doc = document_at([1024, 768]);
    let dabs: Vec<_> = [[125., 125.], [893., 125.], [893., 637.], [125., 637.]].into_iter().map(|p| {
        let mut dab = crate::tests::test_dab(p, [1., 0., 0., 1.], 1.);
        dab.radii = [16.; 2];
        dab
    }).collect();
    let damage = dabs.iter().fold(layer_core::Rect::default(), |r, d| r.union(d.bounds()));
    let batch = DabBatch { dab_count: dabs.len() as u32,
        ..dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), damage) };
    for level in [0, 2] {
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.composite_all = false;
        frame.view.width_px = doc.width;
        frame.view.height_px = doc.height;
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(frame).unwrap();
        frame.dabs = &dabs;
        frame.dab_batches = std::slice::from_ref(&batch);
        let work = r.metrics.composited_pixels;
        r.submit(frame).unwrap();
        assert_eq!(r.metrics.composited_pixels - work, 4 * u64::from(PAGE_SIZE >> level).pow(2));
        let incremental = display_pixels(&r);
        r.scale_display = None;
        r.submit(FramePacket { dabs: &[], dab_batches: &[], composite_all: true, ..frame }).unwrap();
        assert_eq!(display_pixels(&r), incremental);
    }
}

#[test]
fn placed_page_edge_edits_match_rebuilding_the_entire_display() {
    let mut doc = document_at([513, 513]);
    let mut moving = doc.layers[0].clone();
    moving.id = LayerId(40);
    moving.opacity = 0.71;
    moving.properties.placement = layer_core::Affine([0.7, 0.7, -0.7, 0.7, 76.8, 77.8]);
    let mut front = doc.layers[0].clone();
    front.id = LayerId(41);
    front.opacity = 0.3;
    doc.layers.insert(0, moving);
    doc.layers.insert(0, front);
    let extent = [doc.width, doc.height];
    let mut incremental = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut rebuilt = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    incremental.submit(frame).unwrap();
    rebuilt.submit(frame).unwrap();
    let original = display_pixels(&incremental);
    let mut dab = crate::tests::test_dab([254.5, 0.5], [1., 0., 0., 1.], 1.);
    dab.radii = [0.45; 2];
    let batch = dab_batch(LayerId(40), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    rebuilt.scale_display = None;
    incremental.submit(frame).unwrap();
    rebuilt.submit(frame).unwrap();
    let expected = display_pixels(&rebuilt);
    let actual = display_pixels(&incremental);
    assert_ne!(actual, original);
    let error = actual.iter().flatten().zip(expected.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max);
    assert!(error < 1e-6, "a filtered source page reaches pixels beyond its mapped bounds: {error}");
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
    pixels(r, &materialized_display(r).texture)
}
fn materialized_display(r: &WgpuRasterizer) -> Image {
    let cache = r.scale_display.as_ref().unwrap();
    if let Some(root) = &cache.placed {
        let [width, height] = cache.plan.size;
        let texels = [0, 0, width, height];
        let uniforms = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("materialized display query"), contents: &root.value.record(cache.plan, texels).unwrap(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let (texture, view) = create_color_target(&r.device, cache.plan.size, "materialized display query");
        let pass = &r.scene_pipelines.resample;
        let binding = pass.binding(&r.device, &uniforms, 0, [&root.value.view, &view, &root.value.view]);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        pass.encode(&mut encoder, &binding, texels, scene::resample::Sampling::AffineArea);
        encoder.submit(&r.queue);
        Image { texture, view, plan: cache.plan }
    } else { Image { texture: cache.texture().clone(), view: cache.view().clone(), plan: cache.plan } }
}

fn assert_presentation_mip(r: &WgpuRasterizer) {
    let cache = r.scale_display.as_ref().unwrap();
    let (input, actual, plan) = if let Some(root) = &cache.placed {
        let source = &r.scene.as_ref().unwrap().scale_sources;
        (pixels(r, &source.image(root.value.id, root.value.plan.level).image.texture),
            pixels(r, &source.image(root.value.id, root.value.plan.level + 1).image.texture), root.value.plan)
    } else { (display_pixels(r), pixels(r, &cache.next.as_ref().unwrap().texture), cache.plan) };
    let size = plan.level_size(plan.level + 1);
    let side = 1 << plan.level;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut sum = [0.; 4];
            let mut area = 0.;
            for yy in y * 2..((y + 1) * 2).min(plan.size[1]) {
                for xx in x * 2..((x + 1) * 2).min(plan.size[0]) {
                    let weight = (side.min(plan.bounds.width() - xx * side)
                        * side.min(plan.bounds.height() - yy * side))
                        as f32;
                    for c in 0..4 {
                        sum[c] += input[(yy * plan.size[0] + xx) as usize][c] * weight;
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
    quality_linear(actual, exact, plan, |color| color)
}

fn linear_color(color: [f32; 4], space: layer_core::BlendSpace, rgb: layer_core::color::RgbSpace) -> [f32; 4] {
    let a = color[3];
    if space == layer_core::BlendSpace::Linear || a <= 0. { return color; }
    [rgb.decode(f64::from(color[0] / a)) as f32 * a,
     rgb.decode(f64::from(color[1] / a)) as f32 * a,
     rgb.decode(f64::from(color[2] / a)) as f32 * a, a]
}

fn quality_linear(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan, linear: impl Fn([f32; 4]) -> [f32; 4]) -> [f32; 3] {
    let side = 1 << plan.level;
    let mut errors = Vec::new();
    for y in 0..plan.size[1] {
        for x in 0..plan.size[0] {
            let mut sum = [0.; 4];
            let mut count = 0.;
            for yy in plan.bounds.min_y() + y * side..(plan.bounds.min_y() + (y + 1) * side).min(plan.bounds.max_y()) {
                for xx in plan.bounds.min_x() + x * side..(plan.bounds.min_x() + (x + 1) * side).min(plan.bounds.max_x()) {
                    for c in 0..4 {
                        sum[c] += exact[(yy * plan.extent[0] + xx) as usize][c];
                    }
                    count += 1.;
                }
            }
            let a = linear(actual[(y * plan.size[0] + x) as usize]);
            let b = linear(sum.map(|v| v / count));
            for c in 0..4 { errors.push((a[c] - b[c]).abs()); }
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
    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&doc.layers[0].id));
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
    assert_settled(&mut r, p, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
    p.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    r.submit(p).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);
    assert!(r.composite_texture.is_none());
    assert!(quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), r.scale_display.as_ref().unwrap().plan)[2] < 1e-6);
}

#[test]
fn native_predictions_match_exact_composition_before_layer_opacity_and_blending() {
    let mut doc = document_at([257, 259]);
    let mut front = doc.layers[0].clone();
    front.id = LayerId(50);
    front.opacity = 0.37;
    doc.layers[0].opacity = 0.63;
    doc.layers.insert(0, front);
    let extent = [doc.width, doc.height];
    for target in [0, 1] {
        for mode in [layer_render::DabMode::Paint, layer_render::DabMode::Erase] {
            let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(&doc.layers, extent);
            frame.view.width_px = extent[0];
            frame.view.height_px = extent[1];
            frame.view.background_rgba_linear = [0.; 4];
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let original = display_pixels(&r);
            let mut dab = crate::tests::test_dab([253., 254.], [0.9, 0.02, 0.1, 0.7], 1.);
            dab.radii = [35.; 2];
            let mut style = crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
            style.mode = mode;
            let batch = DabBatch { kind: DabBatchKind::Preview,
                ..dab_batch(doc.layers[target].id, style, dab.bounds()) };
            for prediction in [true, false] {
                let stroke = FramePacket {
                    dabs: if prediction { std::slice::from_ref(&dab) } else { &[] },
                    dab_batches: if prediction { std::slice::from_ref(&batch) } else { &[] },
                    composite_all: false, ..frame
                };
                r.submit(stroke).unwrap();
                exact.submit(stroke).unwrap();
                let actual = display_pixels(&r);
                let expected = pixels(&exact, crate::test_support::document_texture(&exact));
                let error = quality(&actual, &expected, r.scale_display.as_ref().unwrap().plan);
                assert!(error[2] < 1e-6, "target={target}, mode={mode:?}, preview={prediction}: {error:?}");
                if prediction { assert_ne!(actual, original); }
                else { assert_eq!(actual, original); }
            }
        }
    }
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
            let layer = pixels(&r, &r.scene.as_ref().unwrap().scale_sources.image(doc.layers[0].id, cache.plan.level).image.texture);
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
    let mut effect = crate::tests::image_windows::effect(99, false, false);
    Arc::make_mut(&mut Arc::make_mut(effect.effect.as_mut().unwrap()).program).resolution = layer_core::EffectResolution::Native;
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
    for space in layer_core::BlendSpace::ALL {
      p.blend_space = space;
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
            let errors = quality_linear(
                &display_pixels(&r),
                &pixels(&exact, exact.composite_texture.as_ref().unwrap()),
                cache.plan,
                |color| linear_color(color, space, doc.color.space),
            );
            eprintln!("photo {space:?} {diameter}px {kind:?} mean/p99/max linear channel error: {errors:?}");
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
                "photographic exact output at {space:?} {diameter}px {kind:?}"
            );
        }
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

#[path = "transform_tests.rs"]
mod transforms;
