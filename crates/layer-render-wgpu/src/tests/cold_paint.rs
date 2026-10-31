//! A zero-sized mutable cache forces native artwork through its lossless backing.
//! Compare complete consumers, not just allocation counts or the decoded cache.
use super::*;
use crate::test_support::complete;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, authored::{PortableId,SourceTarget,PaintHandle,PaintSource}};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource, ThumbnailTarget};

const EXTENT: [u32; 2] = [4352, 512]; // 33 backed tiles and one transparent hole.
const RESIDENT: u64 = 1 << 40;
fn document(color: DocumentColor) -> Document {
    let mut document = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let mut data = RasterData::default();
    for y in 0..2 {
        for x in 0..17 {
            if [x, y] == [3, 0] {
                continue;
            }
            let pixel = [((x * 2351 + y * 71) % 65536) as u16, 32123, 51007, 65535];
            let pixel: Vec<_> = match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                SampleDepth::U16 => pixel.into_iter().flat_map(u16::to_le_bytes).collect(),
                SampleDepth::U8 => pixel.map(|v| (v >> 8) as u8).to_vec(),
            };
            data.tiles.insert(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [x, y],
                },
                RasterTile::backed(
                    TileBlob::encode(color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap(),
                ),
            );
        }
    }
    paint_mut(&mut document).raster = RasterRevision::backed(data);
    document
}
fn paint_handle(document: &Document) -> PaintHandle { document.artwork.paint.iter().next().unwrap().0 }
fn target(document: &Document) -> SourceTarget { SourceTarget::Paint(paint_handle(document)) }
fn paint(document: &Document) -> &PaintSource { document.artwork.paint.get(paint_handle(document)).unwrap() }
fn paint_mut(document: &mut Document) -> &mut PaintSource { let target = paint_handle(document); document.artwork.paint.get_mut(target).unwrap() }

fn packet(document: &Document, all: bool) -> FramePacket<'_> {
    FramePacket {
        view: crate::test_support::view(EXTENT),
        composite_all: all,
        ..crate::test_support::packet(document.scene(), EXTENT)
    }
}
fn renderer(document: &Document, limit: u64) -> WgpuRasterizer { renderer_with(document, limit, packet(document, true)) }
fn renderer_with(document: &Document, limit: u64, frame: FramePacket<'_>) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    r.native_edit.as_mut().unwrap().color_cache_bytes = limit;
    crate::test_support::submit_settled(&mut r, frame);
    r
}
fn image(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}
fn close(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()).enumerate() {
        let a = f32::from_le_bytes(*a);
        let b = f32::from_le_bytes(*b);
        assert!((a - b).abs() <= 3e-6, "working sample {i}: {a} != {b}");
    }
}
fn thumbnail(r: &mut WgpuRasterizer, id: SourceTarget) -> Vec<u8> {
    r.request_thumbnail(7, ThumbnailTarget::Source(id)).unwrap();
    complete(r);
    r.take_thumbnail().unwrap().unwrap().bytes
}
fn sample(
    r: &mut WgpuRasterizer,
    id: SourceTarget,
    position: [u32; 2],
    area: ColorSampleArea,
) -> [f32; 4] {
    assert!(
        r.request_color_sample(ColorSampleRequest {
            request_id: 8,
            source: ColorSampleSource::Source(id),
            position,
            area
        })
        .unwrap()
    );
    complete(r);
    r.take_color_sample().unwrap().unwrap().rgba
}
fn backing(root: &RasterRevision) -> std::collections::BTreeMap<TileKey, Vec<u8>> {
    root.wait_data()
        .unwrap()
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
        .collect()
}

#[test]
fn stale_thumbnail_batches_cancel_without_publishing_and_leave_live_artwork_available() {
    use layer_core::authored::*;
    let original = document(DocumentColor::default());
    let stale_source = target(&original);
    let stale_occurrence = original.scene().source_owner(stale_source).unwrap();
    let mut r = renderer(&original, 0);
    let stale = ThumbnailTarget::Occurrence(stale_occurrence);
    r.prepare_thumbnail_batch(stale).unwrap();
    assert!(!r.thumbnails_pending());

    let mut artwork = original.artwork.clone();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.retain(|handle| *handle != stale_occurrence);
    artwork.occurrences.remove(stale_occurrence).unwrap();
    let SourceTarget::Paint(handle) = stale_source else { unreachable!() };
    artwork.paint.remove(handle).unwrap();
    let (live_occurrence, live_source) = crate::test_support::add_paint(&mut artwork, "Live ink", EXTENT);
    let SourceTarget::Paint(live_handle) = live_source else { unreachable!() };
    let bytes = [0, 0, 255, 255].repeat(256 * 256);
    let root = RasterRevision::backed(RasterData {tiles: [(TileKey {plane: RasterPlane::Color, coordinate: [0, 0]},
        RasterTile::backed(TileBlob::encode(original.composition().color.paint_descriptor(), &bytes).unwrap()))].into(), ..Default::default()});
    artwork.paint.get_mut(live_handle).unwrap().raster = root.clone();
    let current = Document::from_artwork(artwork).unwrap();
    crate::test_support::submit_ready(&mut r, packet(&current, true)).unwrap();
    complete(&r);
    let before = backing(&root);
    for target in [stale, ThumbnailTarget::Source(stale_source)] {
        assert_eq!(r.prepare_thumbnail_batch(target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert_eq!(r.request_thumbnail(99, target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert!(r.take_thumbnail().is_none());
        assert!(!r.thumbnails_pending());
    }
    let live = ThumbnailTarget::Occurrence(live_occurrence);
    for _ in 0..100 {
        if r.prepare_thumbnail_batch(live).unwrap() { break; }
    }
    r.request_thumbnail(77, live).unwrap();
    complete(&r);
    let image = r.take_thumbnail().unwrap().unwrap();
    assert_eq!((image.request_id, image.width, image.height), (77, 32, 32));
    assert!(image.bytes.as_chunks::<4>().0.iter().all(|pixel| *pixel == [0, 0, 255, 255]));
    assert!(!r.thumbnails_pending());
    assert_eq!(backing(&root), before);
    r.submit(packet(&current, true)).unwrap();
    complete(&r);
    assert_eq!(backing(&root), before);
}

#[test]
fn native_thumbnail_preparation_reduces_pages_in_bounded_batches() {
    let p = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let id = target(&p);
    let mut cold = renderer(&p, 0);
    let (polls, reduced) = polls_until_ready(&mut cold, id);
    assert_eq!((polls, reduced), (33usize.div_ceil(crate::thumbnails::BATCH_PAGES), 33), "each backed page is reduced once, a batch per poll");
    let mut resident = renderer(&p, RESIDENT);
    assert_eq!(thumbnail(&mut cold, id), thumbnail(&mut resident, id));
}

#[test]
fn cold_native_color_composition_sampling_and_thumbnails_match_resident_tiles() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let p = document(DocumentColor { space, depth });
            let id = target(&p);
            let mut resident = renderer(&p, RESIDENT);
            let mut cold = renderer(&p, 0);
            assert_eq!(resident.paint_layers[0].pages.len(), 33);
            assert!(cold.paint_layers[0].pages.is_empty());
            close(&image(&cold), &image(&resident));
            let a = thumbnail(&mut cold, id);
            let b = thumbnail(&mut resident, id);
            assert!(
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1),
                "thumbnail {space:?} {depth:?}"
            );
            for point in [[257, 257], [4331, 510], [3 * 256 + 80, 80]] {
                for area in [ColorSampleArea::Point, ColorSampleArea::Average5] {
                    let a = sample(&mut cold, id, point, area);
                    let b = sample(&mut resident, id, point, area);
                    assert!(
                        a.into_iter().zip(b).all(|(a, b)| (a - b).abs() <= 3e-6),
                        "sample {space:?} {depth:?} {point:?}"
                    );
                }
            }
            let region = |r: &mut WgpuRasterizer| {
                assert!(
                    r.request_region(layer_render::RegionRequest {
                        enclosure: None, contiguous: true,
                        selection: None,
                        request_id: 9,
                        source: layer_render::RegionSource::Source(id),
                        position: [259, 258],
                        tolerance: 0.,
                        refinement: Default::default(),
                        limit: None,
                    })
                    .unwrap()
                );
                complete(r);
                r.take_region().unwrap().unwrap().pixels
            };
            assert_eq!(
                region(&mut cold),
                region(&mut resident),
                "raw region {space:?} {depth:?}"
            );
            assert!(
                cold.paint_layers[0].pages.is_empty(),
                "read-only consumers must not materialize mutable tiles"
            );
            let unique_samples: std::collections::BTreeSet<_> = paint(&p).raster
                .wait_data().unwrap().tiles.values()
                .map(|tile| tile.wait_backing().unwrap().content_digest().unwrap()).collect();
            assert_eq!(cold.source_cache_work()[1], unique_samples.len() as u64);
            assert_eq!(cold.source_tiles.borrow().evictions, 0, "resident capacity holds every unique sample");
        }
    }
}

#[test]
fn cold_native_paint_rehydrates_only_damage_and_keeps_other_tiles_in_saved_revisions() {
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let original = paint(&p).raster.clone();
    let original_bytes = backing(&original);
    let id = target(&p);
    let mut r = renderer(&p, 0);
    let before = image(&r);
    let dabs = [test_dab([275., 275.], [0.13, 0.72, 0.41, 0.37], 0.5)];
    let batches = [crate::test_support::dab_batch(
        id,
        test_style(BrushExecution::Dry),
        Rect { min: Point { x: 257., y: 257. }, max: Point { x: 295., y: 295. } },
    )];
    paint_mut(&mut p).raster = RasterRevision::pending();
    crate::test_support::submit_ready(&mut r, FramePacket {
        dabs: &dabs,
        dab_batches: &batches,
        ..packet(&p, false)
    })
    .unwrap();
    let edited = paint(&p).raster.clone();
    let edited_bytes = backing(&edited);
    assert_eq!(
        edited_bytes.len(),
        original_bytes.len(),
        "cold tiles must not disappear at publication"
    );
    for (key, data) in &original_bytes {
        if key.coordinate != [1, 1] {
            assert_eq!(&edited_bytes[key], data);
        }
    }
    assert_ne!(
        edited_bytes[&TileKey {
            plane: RasterPlane::Color,
            coordinate: [1, 1]
        }],
        original_bytes[&TileKey {
            plane: RasterPlane::Color,
            coordinate: [1, 1]
        }]
    );
    assert_eq!(r.paint_layers[0].pages.len(), 1);
    let changed = image(&r);
    assert_ne!(changed, before);
    crate::test_support::submit_settled(&mut r, packet(&p, true));
    assert!(
        r.paint_layers[0].pages.is_empty(),
        "completed color may leave the mutable cache"
    );
    close(&image(&r), &changed);
    let reopened = super::native_effects::roundtrip(p.clone());
    assert_eq!(backing(&paint(&reopened).raster), edited_bytes);
    let replacement = renderer(&reopened, 0);
    close(&image(&replacement), &changed);
    paint_mut(&mut p).raster = original;
    crate::test_support::submit_settled(&mut r, packet(&p, false));
    assert_eq!(backing(&paint(&p).raster), original_bytes);
    close(&image(&r), &before);
    paint_mut(&mut p).raster = edited;
    crate::test_support::submit_settled(&mut r, packet(&p, false));
    close(&image(&r), &changed);
}

#[test]
fn cold_native_transform_snapshots_keep_original_tiles_through_preview_and_cancel() {
    let p = document(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    let id = target(&p);
    let mut resident = renderer(&p, RESIDENT);
    let mut cold = renderer(&p, 0);
    let original = image(&cold);
    for offset in [Point { x: 17., y: -11. }, Point { x: -31., y: 9. }] {
        let preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            target: id,
            selection: None,
            transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(offset)),
        };
        for r in [&mut resident, &mut cold] {
            r.set_transform_preview(Some(&preview)).unwrap();
            r.submit(packet(&p, false)).unwrap();
        }
        close(&image(&cold), &image(&resident));
    }
    for r in [&mut resident, &mut cold] {
        r.set_transform_preview(None).unwrap();
        r.submit(packet(&p, false)).unwrap();
    }
    close(&image(&cold), &original);
    close(&image(&cold), &image(&resident));
    cold.submit(packet(&p, true)).unwrap();
    assert!(cold.paint_layers[0].pages.is_empty());
    close(&image(&cold), &original);
}

#[test]
fn cold_native_overrides_keep_the_original_photo_and_its_thumbnail_contributions() {
    use layer_core::color::{ColorProfile, source::*};
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let mut source = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::AdobeRgb),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for _ in 0..EXTENT[1] {
        source
            .push_row(&[31, 201, 133].repeat(EXTENT[0] as usize))
            .unwrap();
    }
    paint_mut(&mut p).base = Some(layer_core::authored::PaintBase::new((Arc::new(source.finish().unwrap())).into()));
    let id = target(&p);
    let mut resident = renderer(&p, RESIDENT);
    let mut cold = renderer(&p, 0);
    close(&image(&cold), &image(&resident));
    let mut preparations = 1;
    while !cold.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap() {
        preparations += 1;
        assert!(preparations < 100);
        complete(&cold);
        pollster::block_on(cold.await_image_decode()).unwrap();
    }
    assert!(preparations >= (34usize + 33).div_ceil(4), "photo overrides must share the four-tile preparation budget: {preparations}");
    let a = crate::test_support::thumbnail(&mut cold, 7, ThumbnailTarget::Source(id));
    let b = crate::test_support::thumbnail(&mut resident, 7, ThumbnailTarget::Source(id));
    assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1));
    for point in [[257, 257], [3 * 256 + 80, 80]] {
        let a = sample(&mut cold, id, point, ColorSampleArea::Average5);
        let b = sample(&mut resident, id, point, ColorSampleArea::Average5);
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() <= 3e-6));
        assert_eq!(a[3], 1., "holes in paint must reveal the original photo");
    }
    assert!(cold.paint_layers[0].pages.is_empty());
}

#[test]
fn cold_native_neighborhood_brushes_keep_prediction_and_terminal_backing() {
    for execution in [
        BrushExecution::Dry,
        BrushExecution::Smudge,
        BrushExecution::Wet,
        BrushExecution::Liquify,
        BrushExecution::Watercolor,
    ] {
        let mut a = document(DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        });
        let mut b = a.clone();
        let id = target(&a);
        let mut resident = renderer(&a, RESIDENT);
        let mut cold = renderer(&b, 0);
        let mut style = test_style(execution);
        style.wet_mix.amount_of_paint = 0.3;
        style.wet_mix.dilution = 0.4;
        style.wet_mix.pull = 0.9;
        style.wet_mix.blur = 1.;
        style.wet_mix.wetness = 0.7;
        let mut dab = test_dab([257., 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        dab.motion = [13.25, -24.5];
        dab.material = [0., 0.8, 0.9, 0.7];
        let mut batch = DabBatch {
            stroke_end: false,
            ..crate::test_support::dab_batch(
                id,
                style,
                Rect { min: Point { x: 225., y: 225. }, max: Point { x: 288., y: 284. } },
            )
        };
        for phase in 0..4 {
            eprintln!("cold neighborhood {execution:?} phase={phase}");
            if phase == 1 {
                batch.kind = DabBatchKind::Preview;
                batch.stroke_start = false;
                dab.center.x += 9.;
            }
            if phase == 2 {
                batch.kind = DabBatchKind::Persistent;
                batch.material_update = 1;
            }
            if phase == 3 {
                batch.dab_count = 0;
                batch.stroke_end = true;
                batch.material_update = 2;
                paint_mut(&mut a).raster = RasterRevision::pending();
                paint_mut(&mut b).raster = RasterRevision::pending();
            }
            for (r, p) in [(&mut resident, &a), (&mut cold, &b)] {
                r.submit(FramePacket {
                    dabs: &[dab],
                    dab_batches: std::slice::from_ref(&batch),
                    ..packet(p, false)
                })
                .unwrap();
            }
            close(&image(&cold), &image(&resident));
        }
        assert_eq!(
            backing(&paint(&a).raster),
            backing(&paint(&b).raster),
            "{execution:?}"
        );
        cold.submit(packet(&b, true)).unwrap();
        resident.submit(packet(&a, true)).unwrap();
        eprintln!("cold neighborhood {execution:?} after eviction");
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &image(&resident));
    }
}

#[test]
fn native_cache_keeps_prepared_brush_pages() {
    for execution in [BrushExecution::Dry, BrushExecution::Wet] {
        let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
        let mut b = a.clone();
        let id = target(&a);
        let mut resident = renderer(&a, 256 * 1024 * 1024);
        let mut bounded = renderer(&b, 256 * 1024 * 1024);
        let data = paint(&b).raster.wait_data().unwrap();
        paint_mut(&mut a).raster = RasterRevision::pending();
        paint_mut(&mut b).raster = RasterRevision::pending();
        let style = if execution == BrushExecution::Dry {
            DabStyle::for_brush(&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), layer_core::StrokeTool::Brush)
        } else { test_style(execution) };
        let mut dab = test_dab([257., 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        dab.motion = [13.25, -24.5];
        let batch = crate::test_support::dab_batch(id, style, dab.bounds());
        let frame = FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(&b, false) };
        assert!(bounded.raster_dependencies_ready(frame, None));
        let required = bounded.restore_preflight.as_ref().unwrap().dependencies().to_vec();
        assert!(!required.is_empty());
        bounded.native_edit.as_mut().unwrap().color_cache_bytes = 0;
        let tiles = brush_tiles::plan_packet(frame, |target| b.scene().target_extent(target)).unwrap();
        bounded.trim_native_color_cache(frame.dab_batches, &tiles);
        for (target, coordinates) in &required {
            let layer = bounded.paint_layers.iter().find(|layer| layer.id == *target).unwrap();
            for coordinate in coordinates {
                if data.tiles.contains_key(&TileKey {plane: RasterPlane::Color, coordinate: *coordinate}) {
                    assert!(layer.pages.iter().any(|page| page.coordinate == *coordinate), "{execution:?}: required {coordinate:?}");
                }
            }
        }
        assert!(bounded.paint_layers[0].pages.len() < resident.paint_layers[0].pages.len());
        crate::test_support::submit_ready(&mut bounded, frame).unwrap();
        crate::test_support::submit_ready(&mut resident, FramePacket {scene: a.scene(), ..frame}).unwrap();
        complete(&bounded);
        complete(&resident);
        close(&image(&bounded), &image(&resident));
        bounded.retire_stroke_sources();
        bounded.trim_native_color_cache(&[], &[]);
        assert!(bounded.paint_layers[0].pages.is_empty());
    }
}

#[test]
fn native_cache_keeps_dither_originals_after_publication_and_revisits() {
    let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let mut b = a.clone();
    let id = target(&a);
    let mut resident = renderer(&a, RESIDENT);
    let mut bounded = renderer(&b, 0);
    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
    brush.diameter = 128.;
    brush.blender.reach = 0.4;
    let style = DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
    for (phase, x) in [257., 1281., 257.].into_iter().enumerate() {
        let mut dab = test_dab([x, 255.], [0.; 4], 1.);
        dab.radii = [64.; 2];
        dab.motion = [48., 0.];
        let batch = DabBatch { stroke_id: layer_core::StrokeId(100), stroke_start: phase == 0,
            stroke_end: phase == 2, ..crate::test_support::dab_batch(id, style.clone(), dab.bounds()) };
        for (r, p) in [(&mut resident, &mut a), (&mut bounded, &mut b)] {
            paint_mut(p).raster = RasterRevision::pending();
            r.submit(FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(p, false) }).unwrap();
            complete(r);
            paint(p).raster.wait_data().unwrap();
            r.trim_native_color_cache(&[], &[]);
            assert!(r.paint_layers.iter().flat_map(|l| &l.pages).any(|p| p.stroke_start.is_some()),
                "published originals survive cache pressure during the same stroke");
        }
        close(&image(&bounded), &image(&resident));
    }
    let state_bytes = bounded.metrics.paint_state_storage_bytes;
    bounded.retire_stroke_sources();
    assert!(bounded.paint_layers.iter().flat_map(|l| &l.pages).all(|p| p.stroke_start.is_none()));
    assert!(bounded.paint_layers.iter().all(|l| l.coverage_pages.is_empty()), "completed Dither exposure no longer occupies stroke scratch");
    assert!(bounded.metrics.paint_state_storage_bytes < state_bytes, "retirement updates scratch accounting");
    bounded.trim_native_color_cache(&[], &[]);
    assert!(bounded.paint_layers.iter().all(|l| l.pages.is_empty()), "completed originals no longer pin the native cache");
}

#[test]
fn native_cache_retires_blend_scratch_before_artwork_and_recreates_stroke_edges() {
    for (depth, execution) in [SampleDepth::U8, SampleDepth::U16].into_iter()
        .flat_map(|depth| [BrushExecution::Dry, BrushExecution::Wet].map(|execution| (depth, execution))) {
        let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth });
        let mut b = a.clone();
        let id = target(&a);
        let original = paint(&a).raster.clone();
        let mut resident = renderer(&a, RESIDENT);
        let mut bounded = renderer(&b, RESIDENT);
        let page_count = bounded.paint_layers[0].pages.len();
        // Enough for the canonical pages and this frame's four blend targets,
        // but not for scratch retained at the distant previous mark.
        bounded.native_edit.as_mut().unwrap().color_cache_bytes =
            (page_count as u64 + 4) * 256 * 256 * 16;
        let mut style = test_style(execution);
        style.wet_mix.wetness = 0.8;
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.rendering.edge_after_stroke = true;
        style.rendering.wet_edge = 0.8;
        style.rendering.burnt_edge = 0.4;
        style.rendering.edge_width = 4.;
        let mut batch = DabBatch {
            stroke_end: false,
            ..crate::test_support::dab_batch(id, style, Rect::default())
        };
        // Consecutive and returning writes exercise both ping-pong roles. The
        // final edge pass must recreate destinations at the distant earlier mark.
        for (phase, x) in [257., 257., 1281., 257., 1281.].into_iter().enumerate() {
            let mut dab = test_dab([x, 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
            dab.radii = [14., 10.];
            dab.motion = [13.25, -24.5];
            dab.material = [0., 0.8, 0.9, 0.7];
            batch.material_update = phase as u32;
            batch.stroke_start = phase == 0;
            batch.damage = Rect {
                min: Point { x: x - 32., y: 223. },
                max: Point { x: x + 32., y: 287. },
            };
            for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
                crate::test_support::submit_ready(r, FramePacket {
                    dabs: &[dab],
                    dab_batches: std::slice::from_ref(&batch),
                    ..packet(p, false)
                }).unwrap();
            }
            close(&image(&bounded), &image(&resident));
            assert_eq!(bounded.paint_layers[0].pages.len(), page_count,
                "artwork should survive pressure from disposable blend scratch");
            assert!(bounded.paint_layers[0].pages.iter()
                .filter(|p| p.secondary.is_some()).count() <= 8);
        }
        // An idle redraw can retire every companion while the stroke is still
        // active. Pen-up must recreate both distant sets of edge destinations.
        bounded.native_edit.as_mut().unwrap().color_cache_bytes =
            page_count as u64 * 256 * 256 * 16;
        crate::test_support::submit_ready(&mut bounded, packet(&b, false)).unwrap();
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        close(&image(&bounded), &image(&resident));
        batch.stroke_start = false;
        batch.stroke_end = true;
        batch.dab_count = 0;
        batch.material_update += 1;
        paint_mut(&mut a).raster = RasterRevision::pending();
        paint_mut(&mut b).raster = RasterRevision::pending();
        for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
            crate::test_support::submit_ready(r, FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            }).unwrap();
        }
        close(&image(&bounded), &image(&resident));
        assert_eq!(backing(&paint(&a).raster), backing(&paint(&b).raster));
        crate::test_support::submit_ready(&mut bounded, packet(&b, false)).unwrap();
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        let edited = paint(&b).raster.clone();
        paint_mut(&mut b).raster = original;
        crate::test_support::submit_settled(&mut bounded, packet(&b, true));
        paint_mut(&mut b).raster = edited;
        crate::test_support::submit_settled(&mut bounded, packet(&b, true));
        close(&image(&bounded), &image(&resident));
    }
}

#[test]
fn cold_native_color_feeds_bounded_live_filter_windows() {
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    for _ in 0..2 { super::image_windows::insert_effect(&mut p, super::image_windows::program(false,false)); }
    let resident = renderer(&p, RESIDENT);
    let mut cold = WgpuRasterizer::new_native_headless(p.composition().color).unwrap();
    cold.native_edit.as_mut().unwrap().color_cache_bytes = 0;
    cold.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 * 1024 * 1024);
    crate::test_support::submit_settled(&mut cold, packet(&p, true));
    close(&image(&cold), &image(&resident));
    assert!(cold.paint_layers[0].pages.is_empty());
    assert!(cold.metrics().image_window_submissions > 1);
    assert!(cold.metrics().image_window_peak_bytes <= 8 * 1024 * 1024);
}

#[test]
fn cold_native_operations_publish_complete_color_and_restore_exact_history() {
    use layer_core::{Affine, ImageTransform, CoverageSnapshot, RasterOperation, RasterOperationKind};
    for kind in [
        RasterOperationKind::Fill { texture: Default::default(),
            color: [0.12, 0.38, 0.73, 0.42],
            alpha_locked: true,
        },
        RasterOperationKind::ApplyMask,
        RasterOperationKind::Erase { alpha_locked: false },
        RasterOperationKind::Transform(ImageTransform::affine(Affine::translation(Point { x: 83.25, y: 127.5 }))),
    ] {
        let mut a = document(DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        });
        let mut b = a.clone();
        let id = target(&a);
        let original = paint(&a).raster.clone();
        let mut resident = renderer(&a, RESIDENT);
        let mut cold = renderer(&b, 0);
        let before = image(&cold);
        let mut coverage = CoverageSnapshot::reveal_all(a.artwork.coverage.next_handle(), EXTENT, [0, 0]);
        if matches!(kind, RasterOperationKind::ApplyMask | RasterOperationKind::Erase { .. }) {
            coverage.source.default_coverage = 0.5;
        }
        let operation = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind };
        let batch = DabBatch {
            stroke_id: StrokeId(8),
            kind: DabBatchKind::RasterOperation(0),
            dab_count: 0,
            ..crate::test_support::dab_batch(
                id,
                test_style(BrushExecution::Dry),
                operation.bounds(EXTENT),
            )
        };
        for (r, p) in [(&mut resident, &mut a), (&mut cold, &mut b)] {
            if let RasterOperationKind::Transform(transform) = &operation.kind {
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction: 8,
                    moving: false,
                    target: id,
                    selection: None,
                    transform: transform.clone(),
                }))
                .unwrap();
                crate::test_support::submit_ready(r, packet(p, false)).unwrap();
                r.set_transform_preview(None).unwrap();
            }
            Arc::make_mut(&mut paint_mut(p).operations).push(operation.clone());
            paint_mut(p).raster = RasterRevision::pending();
            crate::test_support::submit_settled(r, FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            });
            Arc::make_mut(&mut paint_mut(p).operations).clear();
        }
        assert_eq!(
            backing(&paint(&a).raster),
            backing(&paint(&b).raster),
            "{:?}",
            operation.kind
        );
        close(&image(&cold), &image(&resident));
        let after = paint(&b).raster.clone();
        let changed = image(&cold);
        assert_ne!(changed, before);
        crate::test_support::submit_settled(&mut cold, packet(&b, true));
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &changed);
        let replacement = renderer(&b, 0);
        close(&image(&replacement), &changed);
        paint_mut(&mut b).raster = original;
        crate::test_support::submit_settled(&mut cold, packet(&b, false));
        close(&image(&cold), &before);
        paint_mut(&mut b).raster = after;
        crate::test_support::submit_settled(&mut cold, packet(&b, false));
        close(&image(&cold), &changed);
    }
}

const TIER: [u32; 2] = [6000, 4000];
fn filled(pixel: [u8; 4]) -> Document {
    let mut document = Document::new(PortableId::random(), TIER[0], TIER[1], layer_core::DocumentNames { paint: "Fill".into(), paper: "Paper".into() });
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let tile = RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap());
    let data = RasterData { tiles: page_coordinates(PixelRect::full(TIER))
        .map(|coordinate| (TileKey { plane: RasterPlane::Color, coordinate }, tile.clone())).collect(), ..Default::default() };
    paint_mut(&mut document).raster = RasterRevision::backed(data);
    document
}
fn retiled(document: &Document, coordinate: [u32; 2], pixel: [u8; 4]) -> Document {
    let mut edited = document.clone();
    let mut data = (*paint(document).raster.wait_data().unwrap()).clone();
    data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate },
        RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap()));
    paint_mut(&mut edited).raster = RasterRevision::backed(data);
    edited
}
fn tier_frame(document: &Document, all: bool) -> FramePacket<'_> {
    FramePacket { view: crate::test_support::view(TIER), composite_all: all, ..crate::test_support::packet(document.scene(), TIER) }
}
fn polls_until_ready(r: &mut WgpuRasterizer, id: SourceTarget) -> (usize, u64) {
    let (mut polls, before) = (0, r.metrics.thumbnail_pages);
    loop {
        let pages = r.metrics.thumbnail_pages;
        polls += 1;
        let ready = r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap();
        assert!(r.metrics.thumbnail_pages - pages <= crate::thumbnails::BATCH_PAGES as u64, "one poll reduces at most a batch of pages");
        if ready { return (polls, r.metrics.thumbnail_pages - before); }
        assert!(polls < 1000);
    }
}

#[test]
fn edited_layer_thumbnails_reduce_only_their_changed_pages_in_bounded_batches() {
    let blue = [40, 90, 200, 255];
    let document = filled(blue);
    let id = target(&document);
    let pages = page_coordinates(PixelRect::full(TIER)).count();
    for residency in [0, 256 << 20] {
        let mut r = renderer_with(&document, residency, tier_frame(&document, true));
        let (polls, reduced) = polls_until_ready(&mut r, id);
        assert!(polls <= pages.div_ceil(crate::thumbnails::BATCH_PAGES), "a filled 24 MP layer is ready after {polls} polls");
        assert_eq!(reduced, pages as u64, "a filled layer reduces each page once");
        let filled_thumbnail = thumbnail(&mut r, id);
        assert!(filled_thumbnail[5 * 128..27 * 128].as_chunks::<4>().0.iter().all(|pixel| *pixel == blue), "the framed 3:2 layer shows the fill");
        let edited = retiled(&document, [0, 0], [250, 20, 20, 255]);
        crate::test_support::submit_settled(&mut r, tier_frame(&edited, false));
        assert_eq!(polls_until_ready(&mut r, id), (1, 1), "a one-page edit reduces one page");
        let edited_thumbnail = thumbnail(&mut r, id);
        let corner = &edited_thumbnail[5 * 128..5 * 128 + 4];
        assert!(corner[0] > 150 && corner[2] < 150, "the edited corner shows: {corner:?}");
        assert_eq!(&edited_thumbnail[7 * 128..], &filled_thumbnail[7 * 128..], "the rest stays");
    }
}

#[test]
fn reduced_thumbnails_frame_and_fill_content_that_leaves_most_of_its_pages_transparent() {
    let extent = [2048, 2048];
    let (content, ink) = (PixelRect::new(517, 303, 853, 805), [0u8, 0, 0, 255]);
    let mut document = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Kept".into(), paper: "Paper".into() });
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let descriptor = document.composition().color.paint_descriptor();
    let tiles = page_coordinates(PixelRect::full(extent)).map(|c| {
        let page = page_rect(c);
        let pixels: Vec<u8> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|i| {
            let [x, y] = [page.min_x() + i % PAGE_SIZE, page.min_y() + i / PAGE_SIZE];
            if (content.min_x()..content.max_x()).contains(&x) && (content.min_y()..content.max_y()).contains(&y) { ink } else { [0; 4] }
        }).collect();
        (TileKey { plane: RasterPlane::Color, coordinate: c }, RasterTile::backed(TileBlob::encode(descriptor, &pixels).unwrap()))
    }).collect();
    paint_mut(&mut document).raster = RasterRevision::backed(RasterData { tiles, ..Default::default() });
    let id = target(&document);
    let frame = FramePacket { view: crate::test_support::view(extent), composite_all: true, ..crate::test_support::packet(document.scene(), extent) };
    let mut r = renderer_with(&document, RESIDENT, frame);
    let image = thumbnail(&mut r, id);
    let size = [content.width(), content.height()].map(|v| v as f32 * 32. / content.width().max(content.height()) as f32);
    let low = size.map(|v| (32. - v) / 2.);
    let rows: Vec<String> = image.as_chunks::<{ 32 * 4 }>().0.iter().map(|row| row.as_chunks::<4>().0.iter().map(|p| if p[0] < 100 { '#' } else { 'o' }).collect()).collect();
    for (i, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let p = [i % 32, i / 32].map(|v| v as f32);
        let inside = (0..2).all(|a| p[a] >= low[a] && p[a] + 1. <= low[a] + size[a]);
        let outside = (0..2).any(|a| p[a] + 1. <= low[a] || p[a] >= low[a] + size[a]);
        assert!(!inside || pixel[0] < 32, "thumbnail pixel {p:?} inside the content shows the ink, got {pixel:?}:\n{}", rows.join("\n"));
        assert!(!outside || pixel[0] > 200, "thumbnail pixel {p:?} outside the content shows the checkerboard, got {pixel:?}:\n{}", rows.join("\n"));
    }
}

#[test]
#[ignore = "workstation thumbnail latency and frame cost benchmark"]
fn thumbnail_latency_and_frames_with_thumbnails_pending() {
    let document = filled([40, 90, 200, 255]);
    let id = target(&document);
    let quantile = |values: &mut Vec<f64>, q: f64| { values.sort_by(f64::total_cmp); values[((values.len() - 1) as f64 * q).round() as usize] };
    for residency in [0, 256 << 20] {
        let mut r = renderer_with(&document, residency, tier_frame(&document, true));
        complete(&r);
        let (mut cpu, mut gpu, mut polls) = (Vec::new(), Vec::new(), 0);
        loop {
            polls += 1;
            let started = std::time::Instant::now();
            let ready = r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap();
            cpu.push(started.elapsed().as_secs_f64() * 1e3);
            let submitted = std::time::Instant::now();
            complete(&r);
            gpu.push(submitted.elapsed().as_secs_f64() * 1e3);
            if ready { break; }
        }
        eprintln!("residency {residency}: {polls} polls ({:.1} s at 120 ms); poll CPU p50 {:.2} max {:.2} ms; GPU wait p50 {:.2} max {:.2} ms",
            polls as f64 * 0.12, quantile(&mut cpu, 0.5), quantile(&mut cpu, 1.), quantile(&mut gpu, 0.5), quantile(&mut gpu, 1.));
        for pending in [false, true] {
            let mut edited = retiled(&document, [7, 5], [250, 20, 20, 255]);
            r.submit(tier_frame(&edited, false)).unwrap();
            complete(&r);
            let mut frames = Vec::new();
            for frame in 0..60u32 {
                let mut view = crate::test_support::view(TIER);
                view.width_px = 2560; view.height_px = 1600;
                view.document_to_surface = [0.4, 0., 0., 0.4, -(frame as f32 * 7.), 0.];
                if frame % 3 == 0 { edited = retiled(&edited, [frame / 3 % 23, 3], [250, (frame * 4) as u8, 20, 255]); }
                let started = std::time::Instant::now();
                r.submit(FramePacket { view, composite_all: false, ..tier_frame(&edited, false) }).unwrap();
                if pending { r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap(); }
                complete(&r);
                if frame >= 5 { frames.push(started.elapsed().as_secs_f64() * 1e3); }
            }
            eprintln!("  pan and edit frames with thumbnails {}: wall p50 {:.2} p95 {:.2} max {:.2} ms", if pending { "pending" } else { "idle" },
                quantile(&mut frames, 0.5), quantile(&mut frames, 0.95), quantile(&mut frames, 1.));
        }
    }
}
